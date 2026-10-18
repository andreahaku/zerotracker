//! Eight-track song/chain/phrase stepping and initial playback voices.
//!
//! This deliberately models the file's structure rather than the firmware's
//! implementation. Timing and sound parameters are calibrated against reference
//! recordings.

use crate::chorus::StereoChorus;
use crate::delay::StereoDelay;
use crate::eq::{EffectTransferEq, StereoEq};
use crate::filter::VoiceFilter;
use crate::fm;
use crate::hyper_osc::HyperVoice;
use crate::macro_osc::{BraidsStorage, MacroVoice};
use crate::modulation::{
    CutoffMod, EnvelopeParameter, FmLevelMod, MacroParameterMod, PanMod, ParameterMod, PitchMod,
    VolumeMod,
};
use crate::output_tap::TrackOutputTaps;
use crate::reverb::RoomReverb;
use crate::snapshot::{PlaybackSnapshot, Sample};
use crate::transport::{Telemetry, TransportMode, TransportRequest, ZtTrackStatus};
use crate::wave_fallback::wavetable_fallback;
use m8fmt::{FMWave, FX, Instrument, Mod, SamplePlayMode, Song, SynthParams, WavShape};
use std::f64::consts::TAU;

// Calibrated from the first 120 ms of isolated SAMPLER notes at velocities
// 7F and 40. Other mixer/instrument stages still require reference checks.
const MASTER_GAIN: f32 = 0.496;
const VELOCITY_EXPONENT: f32 = 2.285;
const MIXER_UNITY: f32 = 224.0;
const TRACK_VOLUME_DB_PER_STEP: f32 = 0.2397;
const MASTER_VOLUME_DB_PER_STEP: f32 = 0.2745;

// Mixer-limiter controls measured with low and driven sine carriers. Each knot
// stores the small-signal makeup gain and the high-level gain reduction.
const MASTER_LIMITER_KNOTS: [(u8, f32, f32); 6] = [
    (0x00, 0.0, 0.0),
    (0x20, 2.37, 0.01),
    (0x40, 4.72, 0.45),
    (0x80, 9.45, 10.10),
    (0xC0, 12.93, 18.62),
    (0xFF, 7.76, 18.70),
];
const MASTER_LIMITER_THRESHOLD_DB: f32 = -25.0;
const MASTER_LIMITER_ATTACK_SECONDS: f32 = 0.005;
const MASTER_LIMITER_RELEASE_SECONDS: f32 = 0.080;

fn master_limiter_parameters(level: u8) -> (f32, f32) {
    if level == 0 {
        return (1.0, 0.0);
    }
    let upper = MASTER_LIMITER_KNOTS
        .iter()
        .position(|(knot, _, _)| level <= *knot)
        .unwrap_or(MASTER_LIMITER_KNOTS.len() - 1);
    let (low_level, low_makeup_db, low_reduction_db) =
        MASTER_LIMITER_KNOTS[upper.saturating_sub(1)];
    let (high_level, high_makeup_db, high_reduction_db) = MASTER_LIMITER_KNOTS[upper];
    let fraction = f32::from(level.saturating_sub(low_level))
        / f32::from(high_level.saturating_sub(low_level)).max(1.0);
    let makeup_db = low_makeup_db + (high_makeup_db - low_makeup_db) * fraction;
    let reduction_db = low_reduction_db + (high_reduction_db - low_reduction_db) * fraction;
    let input_range_db = -9.64 - MASTER_LIMITER_THRESHOLD_DB;
    let compression_slope = reduction_db / input_range_db;
    (10.0_f32.powf(makeup_db / 20.0), compression_slope)
}

fn master_limiter_gain(envelope_db: f32, makeup: f32, compression_slope: f32) -> f32 {
    let reduction_db = (envelope_db - MASTER_LIMITER_THRESHOLD_DB).max(0.0) * compression_slope;
    makeup * 10.0_f32.powf(-reduction_db / 20.0)
}
// Isolated measurements report larger wet-only deficits, but the full-song gate supports
// only these conservative +1/+2 dB corrections across the current corpus.
const REVERB_RETURN_CORRECTION: f32 = 1.122_018;
const CHORUS_RETURN_CORRECTION: f32 = 1.258_925;
// DEL=80 and 40 isolated controls differ by 17.4 dB, close to a cubic
// send curve. The 80 point is matched to the first echo's 20 ms RMS.
const DELAY_SEND_80_GAIN: f32 = 0.026;
const REVERB_SEND_80_GAIN: f32 = 0.33;
const DELAY_SEND_CORRECTIONS_DB: [(u8, f32); 7] = [
    (0x20, 11.95),
    (0x40, 1.55),
    (0x60, -0.55),
    (0x80, -0.35),
    (0xa0, 0.55),
    (0xc0, 4.0),
    (0xff, 5.2),
];
const REVERB_SEND_CORRECTIONS_DB: [(u8, f32); 7] = [
    (0x20, 4.45),
    (0x40, -6.35),
    (0x60, -9.15),
    (0x80, -8.85),
    (0xa0, -6.8),
    (0xc0, -3.25),
    (0xff, -2.15),
];

fn to_peak(sample: f32) -> u16 {
    (sample.abs().min(1.0) * f32::from(u16::MAX)) as u16
}

fn track_volume_gain(value: u8) -> f32 {
    if value == 0 {
        0.0
    } else {
        10.0_f32.powf((f32::from(value) - MIXER_UNITY) * TRACK_VOLUME_DB_PER_STEP / 20.0)
    }
}

fn master_volume_gain(value: u8) -> f32 {
    if value == 0 {
        0.0
    } else {
        10.0_f32.powf((f32::from(value) - MIXER_UNITY) * MASTER_VOLUME_DB_PER_STEP / 20.0)
    }
}
// CALD2F0/FP/RP isolate the FM POST stage after the voice filter.
const FM_POST_THRESHOLD: f32 = 0.1 / 0.496;
const FM_POST_MAKEUP: f32 = 3.5;
// Suites 47-49 show a 0.349 output ceiling after the engine's calibrated
// 0.496 master/track gain. The voice-stage limit therefore uses the
// equivalent pre-volume threshold.
const MACRO_LIMIT_THRESHOLD: f32 = 0.349_365 / MASTER_GAIN;
// Isolated wet-only stems provide a per-level send correction.
const CHORUS_SEND_80_GAIN: f32 = 0.078;
const CHORUS_SEND_CORRECTIONS_DB: [(u8, f32); 7] = [
    (0x20, 28.25),
    (0x40, 8.5),
    (0x60, 1.4),
    (0x80, -1.45),
    (0xa0, -3.2),
    (0xc0, -2.05),
    (0xff, -9.9),
];

fn interpolated_send_correction(level: u8, anchors: &[(u8, f32)]) -> f32 {
    let Some(&(first_level, first_db)) = anchors.first() else {
        return 0.0;
    };
    if level <= first_level {
        return first_db;
    }
    for pair in anchors.windows(2) {
        let [(low_level, low_db), (high_level, high_db)] = pair else {
            unreachable!();
        };
        if level <= *high_level {
            let position = f32::from(level - low_level) / f32::from(high_level - low_level);
            return low_db + position * (high_db - low_db);
        }
    }
    anchors.last().map_or(0.0, |&(_, db)| db)
}

fn calibrated_send_gain(base_gain: f32, level: u8, anchors: &[(u8, f32)]) -> f32 {
    if level == 0 || base_gain == 0.0 {
        return 0.0;
    }
    base_gain * 10.0_f32.powf(interpolated_send_correction(level, anchors) / 20.0)
}

fn delay_send_gain(level: u8) -> f32 {
    let base_gain = (f32::from(level) / 128.0).powi(3) * DELAY_SEND_80_GAIN;
    calibrated_send_gain(base_gain, level, &DELAY_SEND_CORRECTIONS_DB)
}

fn reverb_send_gain(level: u8) -> f32 {
    let base_gain = (f32::from(level) / 128.0).powi(3) * REVERB_SEND_80_GAIN;
    calibrated_send_gain(base_gain, level, &REVERB_SEND_CORRECTIONS_DB)
}

fn chorus_send_gain(level: u8) -> f32 {
    // Keep the fitted base curve and correct it at the measured suite levels.
    let level_byte = level;
    let level = f32::from(level_byte);
    let base_gain = if level <= 192.0 {
        CHORUS_SEND_80_GAIN * (level / 128.0).powf(4.36)
    } else {
        CHORUS_SEND_80_GAIN * 1.5_f32.powf(4.36) * (level / 192.0).powf(6.8)
    };
    calibrated_send_gain(base_gain, level_byte, &CHORUS_SEND_CORRECTIONS_DB)
}

fn dry_gain(level: u8) -> f32 {
    // reference levels from the same WAVSYNTH voice: DRY 40/80/90/A0/C0
    // measure -30.7/-15.4/-11.5/-7.7/0 dB relative to C0. CALD2B0
    // independently places B0 at -3.83 dB, while CALD1SE places E0 at
    // +7.5 dB. Interpolate in dB between measured points; keep the fitted
    // high-level power curve above C0.
    const LOW_ANCHORS: [(u8, f32); 6] = [
        (0x40, -30.7),
        (0x80, -15.4),
        (0x90, -11.5),
        (0xa0, -7.7),
        (0xb0, -3.83),
        (0xc0, 0.0),
    ];

    if level == 0 {
        return 0.0;
    }
    if level < LOW_ANCHORS[0].0 {
        let anchor_gain = 10.0_f32.powf(LOW_ANCHORS[0].1 / 20.0);
        return anchor_gain * (f32::from(level) / f32::from(LOW_ANCHORS[0].0)).powi(4);
    }
    if level == LOW_ANCHORS[0].0 {
        return 10.0_f32.powf(LOW_ANCHORS[0].1 / 20.0);
    }
    if level <= 0xc0 {
        let upper = LOW_ANCHORS
            .iter()
            .position(|(anchor, _)| level <= *anchor)
            .unwrap_or(LOW_ANCHORS.len() - 1);
        let (lo_level, lo_db) = LOW_ANCHORS[upper - 1];
        let (hi_level, hi_db) = LOW_ANCHORS[upper];
        let fraction = f32::from(level - lo_level) / f32::from(hi_level - lo_level);
        let db = lo_db + (hi_db - lo_db) * fraction;
        return 10.0_f32.powf(db / 20.0);
    }

    // CALD1SC/SE put E0 7.5 dB above C0 on the same sampler.
    (f32::from(level) / 192.0).powf(5.6)
}

#[derive(Clone, Copy)]
enum Voice {
    Silent,
    Sample {
        slot: u8,
        position: f64,
        /// Lower bound for one-shot playback or for the loop region.
        start: f64,
        loop_start: f64,
        end: f64,
        base_increment: f64,
        increment: f64,
        degrade: u8,
        degrade_hold_remaining: u16,
        held_left: f32,
        held_right: f32,
        looped: bool,
        ping_pong: bool,
        play_mode: SamplePlayMode,
        direction: f64,
        gain_left: f32,
        gain_right: f32,
    },
    Wave {
        shape: WavShape,
        phase: f64,
        base_increment: f64,
        increment: f64,
        noise: u32,
        noise_hold: f32,
        gain_left: f32,
        gain_right: f32,
    },
    Fm {
        algo: u8,
        phases: [f64; 4],
        base_increments: [f64; 4],
        increments: [f64; 4],
        pitch_headroom: f32,
        drives: [f32; 4],
        mod_values: [u8; 4],
        operator_mods: [[u8; 2]; 4],
        level_mods: [FmLevelMod; 4],
        shapes: [FMWave; 4],
        feedback: [f32; 4],
        feedback_previous: [f32; 4],
        dc_previous_input: f32,
        dc_previous_output: f32,
        dc_pole: f32,
        gain_left: f32,
        gain_right: f32,
    },
    Macro {
        degrade: u8,
        degrade_hold_remaining: u16,
        held_value: f32,
        redux: u8,
        redux_peak: f32,
        gain_left: f32,
        gain_right: f32,
    },
    Hyper {
        oscillator: HyperVoice,
        gain_left: f32,
        gain_right: f32,
    },
}

impl Voice {
    fn transpose_fractional(
        &mut self,
        semitones: f64,
        macro_state: &mut BraidsStorage,
        sample_rate: u32,
    ) {
        let ratio = 2.0_f64.powf(semitones / 12.0);
        match self {
            Self::Sample {
                base_increment,
                increment,
                ..
            }
            | Self::Wave {
                base_increment,
                increment,
                ..
            } => *increment = *base_increment * ratio,
            Self::Fm {
                base_increments,
                increments,
                ..
            } => {
                for (increment, base) in increments.iter_mut().zip(base_increments) {
                    *increment = *base * ratio;
                }
            }
            Self::Hyper { oscillator, .. } => oscillator.transpose(semitones),
            Self::Macro { .. } => MacroVoice::transpose(macro_state, semitones as f32),
            Self::Silent => {}
        }
        let _ = sample_rate;
    }

    fn set_pan(&mut self, amp: u8, pan: u8, mode: u8) {
        let (center_left, center_right) = instrument_limit_gains(amp, 0x80, mode);
        let (left_balance, right_balance) = pan_balance(pan);
        let (gain_left, gain_right) = (center_left * left_balance, center_right * right_balance);
        match self {
            Self::Sample {
                gain_left: left,
                gain_right: right,
                ..
            }
            | Self::Wave {
                gain_left: left,
                gain_right: right,
                ..
            }
            | Self::Fm {
                gain_left: left,
                gain_right: right,
                ..
            }
            | Self::Macro {
                gain_left: left,
                gain_right: right,
                ..
            }
            | Self::Hyper {
                gain_left: left,
                gain_right: right,
                ..
            } => {
                *left = gain_left;
                *right = gain_right;
            }
            Self::Silent => {}
        }
    }

    fn set_modulator_parameter(
        &mut self,
        modulator: usize,
        parameter: EnvelopeParameter,
        value: u8,
    ) {
        if let Self::Fm { level_mods, .. } = self
            && let Some(level_mod) = level_mods.get_mut(modulator)
        {
            level_mod.set_parameter(parameter, value);
        }
    }

    fn set_fm_mod_value(&mut self, modulator: usize, value: u8) {
        if let Self::Fm { mod_values, .. } = self
            && let Some(mod_value) = mod_values.get_mut(modulator)
        {
            *mod_value = value;
        }
    }

    fn transpose(&mut self, semitones: i8, macro_state: &mut BraidsStorage, sample_rate: u32) {
        let ratio = 2.0_f64.powf(f64::from(semitones) / 12.0);
        match self {
            Self::Sample {
                base_increment,
                increment,
                ..
            } => *increment = *base_increment * ratio,
            Self::Wave {
                base_increment,
                increment,
                ..
            } => {
                let minimum_increment = 8.0 / f64::from(sample_rate);
                *increment = (*base_increment * ratio).max(minimum_increment);
            }
            Self::Fm {
                base_increments,
                increments,
                ..
            } => {
                for (increment, base) in increments.iter_mut().zip(base_increments) {
                    *increment = *base * ratio;
                }
            }
            Self::Macro { .. } => MacroVoice::transpose(macro_state, f32::from(semitones)),
            Self::Hyper { oscillator, .. } => oscillator.transpose(f64::from(semitones)),
            Self::Silent => {}
        }
    }

    fn frame(
        &mut self,
        snapshot: &PlaybackSnapshot,
        macro_state: &mut BraidsStorage,
        sampler_degrade_offset: u8,
    ) -> (f32, f32) {
        match self {
            Self::Silent => (0.0, 0.0),
            Self::Sample {
                slot,
                position,
                start,
                loop_start,
                end,
                base_increment: _,
                increment,
                degrade,
                degrade_hold_remaining,
                held_left,
                held_right,
                looped,
                ping_pong,
                play_mode: _,
                direction,
                gain_left,
                gain_right,
            } => {
                let effective_degrade = degrade.saturating_add(sampler_degrade_offset);
                let Some(sample) = snapshot
                    .samples
                    .get(usize::from(*slot))
                    .and_then(Option::as_ref)
                else {
                    *self = Self::Silent;
                    return (0.0, 0.0);
                };
                // A newly published snapshot may replace this slot with a
                // shorter sample while a voice is already playing.
                let sample_end = (*end).min(sample.frames() as f64);
                if *start >= sample_end {
                    *self = Self::Silent;
                    return (0.0, 0.0);
                }
                if !loop_position(
                    position,
                    direction,
                    *start,
                    *loop_start,
                    sample_end,
                    *looped,
                    *ping_pong,
                ) {
                    *self = Self::Silent;
                    return (0.0, 0.0);
                }
                let (left, right) = if effective_degrade == 0 {
                    let index = *position as usize;
                    let next = if ((index + 1) as f64) < sample_end {
                        index + 1
                    } else if *looped {
                        *loop_start as usize
                    } else {
                        index
                    };
                    let fraction = (*position - index as f64) as f32;
                    let channels = usize::from(sample.channels);
                    let left = interpolate(
                        sample.pcm[index * channels],
                        sample.pcm[next * channels],
                        fraction,
                    );
                    let right = if channels == 2 {
                        interpolate(
                            sample.pcm[index * channels + 1],
                            sample.pcm[next * channels + 1],
                            fraction,
                        )
                    } else {
                        left
                    };
                    (left, right)
                } else if sampler_degrade_refresh(effective_degrade, degrade_hold_remaining) {
                    let channels = usize::from(sample.channels);
                    let index =
                        ((*position + 0.5) as usize).min((sample_end as usize).saturating_sub(1));
                    *held_left = sample.pcm[index * channels];
                    *held_right = if channels == 2 {
                        sample.pcm[index * channels + 1]
                    } else {
                        *held_left
                    };
                    (*held_left, *held_right)
                } else {
                    (*held_left, *held_right)
                };
                *position += *increment * *direction;
                (left * *gain_left, right * *gain_right)
            }
            Self::Wave {
                shape,
                phase,
                base_increment: _,
                increment,
                noise,
                noise_hold,
                gain_left,
                gain_right,
            } => {
                let value = match shape {
                    WavShape::SINE => (TAU * *phase).sin() as f32,
                    WavShape::SAW => saw_sample(*phase, *increment),
                    WavShape::TRIANGLE => (1.0 - 4.0 * (*phase - 0.5).abs()) as f32,
                    WavShape::PULSE12 => pulse_sample(*phase, *increment, 0.125),
                    WavShape::PULSE25 => pulse_sample(*phase, *increment, 0.25),
                    WavShape::PULSE50 => pulse_sample(*phase, *increment, 0.5),
                    WavShape::PULSE75 => pulse_sample(*phase, *increment, 0.75),
                    WavShape::NOISE | WavShape::NOISE_PITCHED => {
                        *phase += *increment;
                        if *phase >= 1.0 {
                            *phase = phase.fract();
                            *noise_hold = next_noise_sample(noise);
                        }
                        *noise_hold
                    }
                    // Complex algorithms need a separately licensed and
                    // measured implementation; silence is explicit for now.
                    _ => 0.0,
                };
                if !matches!(shape, WavShape::NOISE | WavShape::NOISE_PITCHED) {
                    *phase = (*phase + *increment).fract();
                }
                (value * *gain_left, value * *gain_right)
            }
            Self::Fm {
                algo,
                phases,
                base_increments: _,
                increments,
                pitch_headroom,
                drives,
                mod_values,
                operator_mods,
                level_mods,
                shapes,
                feedback,
                feedback_previous,
                dc_previous_input,
                dc_previous_output,
                dc_pole,
                gain_left,
                gain_right,
            } => {
                let static_values = mod_values.map(f32::from);
                let mut live_values = [0.0; 4];
                for modulator in level_mods {
                    modulator.frame(&mut live_values);
                }
                let values = std::array::from_fn(|index| {
                    (static_values[index] + live_values[index]).clamp(0.0, 255.0)
                });
                let effective_drives = fm::modulated_drives(
                    *algo,
                    drives,
                    &static_values,
                    &live_values,
                    operator_mods,
                );
                let effective_increments =
                    fm::modulated_increments(increments, &values, operator_mods, *pitch_headroom);
                let raw_value = fm::frame_with_increments(
                    *algo,
                    phases,
                    &effective_increments,
                    &effective_drives,
                    shapes,
                    feedback,
                    feedback_previous,
                );
                // High feedback can create an unintended DC offset.
                // Keep the measured DC bias of basic shapes such as SW2/SW3/IMP.
                let value = if feedback.iter().any(|amount| *amount != 0.0) {
                    let filtered = raw_value - *dc_previous_input + *dc_pole * *dc_previous_output;
                    *dc_previous_input = raw_value;
                    *dc_previous_output = filtered;
                    filtered
                } else {
                    raw_value
                };
                for index in 0..4 {
                    phases[index] = (phases[index] + effective_increments[index]).fract();
                }
                (value * *gain_left, value * *gain_right)
            }
            Self::Macro {
                degrade,
                degrade_hold_remaining,
                held_value,
                redux,
                redux_peak,
                gain_left,
                gain_right,
            } => {
                let source = MacroVoice::frame(macro_state);
                let mut value =
                    if *degrade == 0 || sampler_degrade_refresh(*degrade, degrade_hold_remaining) {
                        *held_value = source;
                        source
                    } else {
                        *held_value
                    };
                if *redux != 0 {
                    *redux_peak = redux_peak.max(value.abs());
                    value = macro_redux_sample(value, *redux, *redux_peak);
                }
                (value * *gain_left, value * *gain_right)
            }
            Self::Hyper {
                oscillator,
                gain_left,
                gain_right,
            } => {
                let (left, right) = oscillator.frame();
                (left * *gain_left, right * *gain_right)
            }
        }
    }
}

fn interpolate(a: f32, b: f32, fraction: f32) -> f32 {
    a + (b - a) * fraction
}

/// Correct the local discontinuity of a naive band-limited oscillator.
/// `phase` and `increment` are expressed in cycles, with phase in [0, 1).
fn poly_blep(phase: f64, increment: f64) -> f32 {
    if increment <= 0.0 || increment >= 0.5 {
        return 0.0;
    }
    if phase < increment {
        let t = phase / increment;
        (2.0 * t - t * t - 1.0) as f32
    } else if phase > 1.0 - increment {
        let t = (phase - 1.0) / increment;
        (t * t + 2.0 * t + 1.0) as f32
    } else {
        0.0
    }
}

fn pulse_sample(phase: f64, increment: f64, duty: f64) -> f32 {
    let raw = if phase < duty { 1.0 } else { -1.0 };
    raw + poly_blep(phase, increment) - poly_blep((phase - duty).rem_euclid(1.0), increment)
}

fn saw_sample(phase: f64, increment: f64) -> f32 {
    (2.0 * phase - 1.0) as f32 - poly_blep(phase, increment)
}

fn loop_position(
    position: &mut f64,
    direction: &mut f64,
    start: f64,
    loop_start: f64,
    end: f64,
    looped: bool,
    ping_pong: bool,
) -> bool {
    // Forward loop and ping-pong modes may play the intro before LOOP ST.
    // After a ping-pong has turned around, however, it must reflect at the
    // loop start instead of continuing back through that intro.
    if *position >= start
        && *position < end
        && (!ping_pong || *direction >= 0.0 || *position >= loop_start)
    {
        return true;
    }
    if !looped {
        return false;
    }
    if ping_pong && end - loop_start > 1.0 {
        let last = end - 1.0;
        let span = last - loop_start;
        let period = 2.0 * span;
        let phase = (*position - loop_start).rem_euclid(period);
        *position = if phase <= span {
            loop_start + phase
        } else {
            last - (phase - span)
        };
        if phase > span {
            *direction = -*direction;
        }
    } else {
        *position = loop_start + (*position - loop_start).rem_euclid(end - loop_start);
    }
    true
}

fn velocity_gain(velocity: u8) -> f32 {
    (f32::from(velocity.min(127)) / 127.0).powf(VELOCITY_EXPONENT)
}

fn pan_gains(amp: u8, pan: u8) -> (f32, f32) {
    // AMP=00 is the neutral instrument setting in existing songs. Its
    // precise boost curve still needs calibration against the reference.
    let amplitude = 1.0 + f32::from(amp) / 255.0;
    // The reference keeps the near channel at its centre level and scales only
    // the far channel linearly with pan.
    let center = std::f64::consts::FRAC_1_SQRT_2 as f32 * amplitude;
    if pan <= 128 {
        (center, center * f32::from(pan) / 128.0)
    } else {
        (center * f32::from(u8::MAX - pan) / 127.0, center)
    }
}

fn pan_balance(pan: u8) -> (f32, f32) {
    if pan <= 0x80 {
        (1.0, f32::from(pan) / 128.0)
    } else {
        (f32::from(u8::MAX - pan) / 127.0, 1.0)
    }
}

fn macro_pan_gains(amp: u8, pan: u8) -> (f32, f32) {
    let (left, right) = pan_gains(amp, pan);
    let old_gain = 1.0 + f32::from(amp) / 255.0;
    let m8_gain = 1.0 + 16.0 * f32::from(amp) / 255.0;
    let correction = m8_gain / old_gain;
    (left * correction, right * correction)
}

fn instrument_limit_gains(amp: u8, pan: u8, mode: u8) -> (f32, f32) {
    if matches!(mode, 5 | 7 | 8) {
        // POSTAD, W2, and W3 incorporate AMP in their transfer curve.
        pan_gains(0, pan)
    } else {
        macro_pan_gains(amp, pan)
    }
}

fn macro_post_ad(sample: f32, amp: u8) -> f32 {
    // The measured small-signal slope grows by about 1.6 per 0x10 of AMP, while
    // the saturation ceiling rises from roughly 0.72 to 1.0 of the CLIP level.
    let amp_drive = (f32::from(amp) * (1.6_f32.ln() / 16.0)).exp();
    let curve_scale = MACRO_LIMIT_THRESHOLD * (1.0 - 0.28 * (-f32::from(amp) / 48.0).exp());
    curve_scale * (0.96 * amp_drive * sample / curve_scale).tanh()
}

fn macro_post_w2(sample: f32, amp: u8) -> f32 {
    let threshold = MACRO_LIMIT_THRESHOLD;
    let amp_gain = 1.0 + 16.0 * f32::from(amp) / 255.0;
    let phase = std::f32::consts::TAU / 2.8 * amp_gain * sample / threshold;
    let sine_level = 0.48 + 0.06 * (f32::from(amp) / 32.0).clamp(0.0, 1.0);
    let wave = sine_level * threshold * phase.sin();
    let blend = ((f32::from(amp) - 32.0) / 32.0).clamp(0.0, 1.0);
    let blend = blend.powf(0.4);
    let post_ad = macro_post_ad(sample, amp);
    wave * (1.0 - blend) + post_ad * blend
}

const MACRO_W3_AMP: [u8; 6] = [0x00, 0x10, 0x20, 0x40, 0x80, 0xFF];
// Fine W3 transfer measurements, in input/output units of the
// clip threshold. Input knots are bin centers from -0.29 through -0.01 and
// +0.01 through +0.29. Keep the two branches separate: W3 is asymmetric and
// its measured bias is part of the active-voice transfer curve.
const MACRO_W3_FINE_NEGATIVE: [[f32; 15]; 6] = [
    [
        -0.266, -0.243, -0.229, -0.207, -0.191, -0.172, -0.149, -0.132, -0.116, -0.098, -0.073,
        -0.061, -0.039, -0.022, -0.004,
    ],
    [
        -0.370, -0.343, -0.328, -0.299, -0.282, -0.255, -0.227, -0.200, -0.182, -0.154, -0.116,
        -0.097, -0.057, -0.035, 0.002,
    ],
    [
        -0.524, -0.501, -0.487, -0.463, -0.443, -0.419, -0.386, -0.359, -0.331, -0.293, -0.235,
        -0.202, -0.135, -0.079, -0.013,
    ],
    [
        -0.810, -0.799, -0.791, -0.779, -0.768, -0.753, -0.732, -0.713, -0.691, -0.658, -0.596,
        -0.552, -0.434, -0.292, -0.055,
    ],
    [
        -0.974, -0.971, -0.969, -0.966, -0.963, -0.959, -0.953, -0.945, -0.939, -0.927, -0.902,
        -0.882, -0.801, -0.694, 0.103,
    ],
    [
        -0.993, -0.992, -0.991, -0.990, -0.990, -0.989, -0.987, -0.985, -0.983, -0.980, -0.973,
        -0.967, -0.944, -0.907, 0.105,
    ],
];
const MACRO_W3_FINE_POSITIVE: [[f32; 15]; 6] = [
    [
        0.027, 0.048, 0.074, 0.093, 0.122, 0.143, 0.170, 0.190, 0.218, 0.234, 0.255, 0.280, 0.303,
        0.321, 0.339,
    ],
    [
        0.087, 0.145, 0.210, 0.236, 0.279, 0.306, 0.336, 0.357, 0.384, 0.399, 0.418, 0.439, 0.458,
        0.473, 0.488,
    ],
    [
        0.225, 0.321, 0.390, 0.425, 0.463, 0.485, 0.508, 0.524, 0.544, 0.555, 0.569, 0.584, 0.598,
        0.610, 0.620,
    ],
    [
        0.610, 0.708, 0.758, 0.779, 0.800, 0.811, 0.822, 0.829, 0.838, 0.842, 0.848, 0.854, 0.859,
        0.864, 0.868,
    ],
    [
        0.911, 0.950, 0.970, 0.975, 0.982, 0.984, 0.987, 0.988, 0.990, 0.991, 0.991, 0.992, 0.993,
        0.993, 0.994,
    ],
    [
        0.973, 0.985, 0.992, 0.993, 0.995, 0.996, 0.996, 0.997, 0.997, 0.998, 0.998, 0.998, 0.998,
        0.998, 0.998,
    ],
];
const MACRO_W3_CURVES: [[f32; 26]; 6] = [
    [
        -0.72, -0.72, -0.72, -0.72, -0.72, -0.72, -0.72, -0.72, -0.72, -0.72, -0.47, -0.28, -0.09,
        0.14, 0.36, 0.55, 0.78, 0.78, 0.78, 0.78, 0.78, 0.78, 0.78, 0.78, 0.78, 0.78,
    ],
    [
        -0.79, -0.79, -0.79, -0.79, -0.79, -0.79, -0.69, -0.61, -0.52, -0.43, -0.32, -0.21, -0.06,
        0.20, 0.36, 0.46, 0.54, 0.61, 0.68, 0.76, 0.84, 0.84, 0.84, 0.84, 0.84, 0.84,
    ],
    [
        -0.85, -0.85, -0.80, -0.76, -0.72, -0.68, -0.63, -0.58, -0.53, -0.47, -0.38, -0.27, -0.08,
        0.34, 0.48, 0.54, 0.59, 0.63, 0.67, 0.70, 0.74, 0.77, 0.81, 0.84, 0.88, 0.88,
    ],
    [
        -0.88, -0.87, -0.85, -0.84, -0.83, -0.81, -0.79, -0.77, -0.74, -0.70, -0.63, -0.49, -0.14,
        0.66, 0.77, 0.81, 0.83, 0.84, 0.85, 0.86, 0.87, 0.88, 0.89, 0.89, 0.90, 0.91,
    ],
    [
        -0.97, -0.97, -0.97, -0.96, -0.96, -0.95, -0.94, -0.93, -0.91, -0.89, -0.84, -0.68, -0.05,
        0.89, 0.95, 0.97, 0.98, 0.98, 0.99, 0.99, 0.99, 0.99, 0.99, 0.99, 0.99, 0.99,
    ],
    [
        -0.98, -0.98, -0.98, -0.98, -0.97, -0.97, -0.97, -0.96, -0.94, -0.93, -0.92, -0.55, 0.19,
        0.93, 0.98, 0.99, 0.99, 0.99, 0.99, 0.99, 0.99, 1.00, 1.00, 0.99, 1.00, 1.00,
    ],
];

fn macro_post_w3(sample: f32, amp: u8) -> f32 {
    let threshold = MACRO_LIMIT_THRESHOLD;
    let amp_gain = 1.0 + 16.0 * f32::from(amp) / 255.0;
    let input = (sample / threshold).clamp(-2.5, 2.5);
    let coarse_input = (amp_gain * input).clamp(-2.5, 2.5);
    let coarse_position = ((coarse_input + 2.5) / 0.2).clamp(0.0, 25.0);
    let coarse_index = (coarse_position.floor() as usize).min(24);
    let coarse_fraction = coarse_position - coarse_index as f32;
    let coarse_at = |curve: &[f32; 26]| {
        curve[coarse_index] + (curve[coarse_index + 1] - curve[coarse_index]) * coarse_fraction
    };
    let amp_index = MACRO_W3_AMP.partition_point(|anchor| *anchor < amp);
    let curve_at_amp = |curves: &[[f32; 15]; 6], index: usize| {
        let curve = &curves[index];
        let position = ((input.abs() - 0.01) / 0.02).clamp(0.0, 14.0);
        let point = (position.floor() as usize).min(13);
        curve[point] + (curve[point + 1] - curve[point]) * (position - point as f32)
    };
    let output_at_amp = |index: usize| {
        let fine = if input < -0.01 {
            curve_at_amp(&MACRO_W3_FINE_NEGATIVE, index)
        } else if input > 0.01 {
            curve_at_amp(&MACRO_W3_FINE_POSITIVE, index)
        } else {
            let negative = MACRO_W3_FINE_NEGATIVE[index][14];
            let positive = MACRO_W3_FINE_POSITIVE[index][0];
            negative + (positive - negative) * ((input + 0.01) * 50.0)
        };
        if input.abs() <= 0.29 {
            fine
        } else {
            let edge = if input.is_sign_negative() {
                MACRO_W3_FINE_NEGATIVE[index][0]
            } else {
                MACRO_W3_FINE_POSITIVE[index][14]
            };
            let coarse = coarse_at(&MACRO_W3_CURVES[index]);
            let blend = ((input.abs() - 0.29) / 0.02).clamp(0.0, 1.0);
            edge + (coarse - edge) * blend
        }
    };
    let output = if amp_index == 0 {
        output_at_amp(0)
    } else if amp_index == MACRO_W3_AMP.len() {
        output_at_amp(MACRO_W3_AMP.len() - 1)
    } else {
        let low_amp = MACRO_W3_AMP[amp_index - 1];
        let high_amp = MACRO_W3_AMP[amp_index];
        let fraction = f32::from(amp - low_amp) / f32::from(high_amp - low_amp);
        let low = output_at_amp(amp_index - 1);
        let high = output_at_amp(amp_index);
        low + (high - low) * fraction
    };
    output * threshold
}

fn macro_limit_sample(sample: f32, mode: u8, amp: u8) -> f32 {
    let threshold = MACRO_LIMIT_THRESHOLD;
    match mode {
        0 => sample.clamp(-threshold, threshold),
        1 => threshold * (std::f32::consts::FRAC_PI_2 * sample / threshold).sin(),
        2 => threshold - ((sample + threshold).rem_euclid(4.0 * threshold) - 2.0 * threshold).abs(),
        3 => (sample + threshold).rem_euclid(2.0 * threshold) - threshold,
        4 => sample.clamp(-threshold, threshold),
        5 => macro_post_ad(sample, amp),
        7 => macro_post_w2(sample, amp),
        8 => macro_post_w3(sample, amp),
        6 => {
            // reference measurements fit 1.25r - 0.25r^3 on [0, 1], mirrored
            // around r=1 on [1, 2], then zero. This is an odd transfer curve.
            let magnitude = (sample / threshold).abs();
            let folded = if magnitude <= 1.0 {
                magnitude * (1.25 - 0.25 * magnitude * magnitude)
            } else if magnitude < 2.0 {
                let reflected = 2.0 - magnitude;
                reflected * (1.25 - 0.25 * reflected * reflected)
            } else {
                0.0
            };
            sample.signum() * threshold * folded
        }
        _ => sample,
    }
}

fn macro_post_limit_frame(
    left: f32,
    right: f32,
    mode: u8,
    amp: u8,
    voice_silent: bool,
) -> (f32, f32) {
    if voice_silent {
        (left, right)
    } else {
        (
            macro_limit_sample(left, mode, amp),
            macro_limit_sample(right, mode, amp),
        )
    }
}

fn next_noise_sample(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    if *state & 0x8000_0000 == 0 { -1.0 } else { 1.0 }
}

fn noise_update_rate(note: u8) -> f64 {
    // reference captures are bipolar sample-and-hold noise. Twice the measured
    // polarity-transition rate estimates the underlying update clock; exact
    // PCM changes also include small output quantization steps. Log
    // interpolation follows the measured nonlinear pitch response.
    const KNOTS: [(u8, f64); 5] = [
        (0x0c, 1_088.0),
        (0x24, 1_702.0),
        (0x3c, 3_266.0),
        (0x58, 8_405.0),
        (0x70, 19_827.0),
    ];
    let upper = KNOTS
        .iter()
        .position(|(key, _)| note <= *key)
        .unwrap_or(KNOTS.len() - 1);
    if upper == 0 {
        return KNOTS[0].1;
    }
    let (lo_note, lo_rate) = KNOTS[upper - 1];
    let (hi_note, hi_rate) = KNOTS[upper];
    let fraction =
        f64::from(note.saturating_sub(lo_note)) / f64::from(hi_note.saturating_sub(lo_note));
    (lo_rate.ln() + (hi_rate.ln() - lo_rate.ln()) * fraction).exp()
}

fn note_ratio(note: u8) -> f64 {
    2.0_f64.powf((f64::from(note) - 36.0) / 12.0)
}

fn sample_region(sample: &Sample, cues: &[u32], slice: u8, note: u8) -> Option<(usize, usize)> {
    let frames = sample.frames();
    let (start, end) = match slice {
        0 => (0, frames),
        // 01 FILE maps C-1 to the first WAV cue, C#-1 to the second, etc.
        1 => {
            let index = usize::from(note);
            let start = *cues.get(index)? as usize;
            let end = cues.get(index + 1).map_or(frames, |&cue| cue as usize);
            (start, end)
        }
        count => {
            let index = usize::from(note);
            let count = usize::from(count);
            if index >= count {
                return None;
            }
            (frames * index / count, frames * (index + 1) / count)
        }
    };
    (start < end && end <= frames).then_some((start, end))
}

#[derive(Clone, Copy)]
struct SamplePlaybackRegion {
    position: f64,
    start: f64,
    loop_start: f64,
    end: f64,
    direction: f64,
    looped: bool,
    ping_pong: bool,
    oscillator: bool,
}

fn sampler_mode_carries_osc_phase(mode: SamplePlayMode) -> bool {
    !matches!(
        mode,
        SamplePlayMode::FWD | SamplePlayMode::REV | SamplePlayMode::REPITCH
    )
}

fn sample_playback_region(
    region_start: usize,
    region_end: usize,
    start: u8,
    loop_start: u8,
    length: u8,
    mode: SamplePlayMode,
) -> Option<SamplePlaybackRegion> {
    use SamplePlayMode::*;

    let frames = region_end.checked_sub(region_start)?;
    if frames < 2 {
        return None;
    }
    let frame_at = |offset: u16| -> usize {
        region_start + (frames as u64 * u64::from(offset.min(256)) / 256) as usize
    };
    let sample_start = frame_at(u16::from(start));
    let loop_begin = frame_at(u16::from(loop_start));
    // The M8 length byte counts the starting unit too: FF therefore spans
    // the full 256-unit sample range rather than stopping one unit early.
    let one_shot_end = frame_at(u16::from(start) + u16::from(length) + 1);
    let loop_end = frame_at(u16::from(loop_start) + u16::from(length) + 1);
    let end_of_sample = region_end;

    let (play_start, play_end, loop_begin, reverse, looped, ping_pong, oscillator) = match mode {
        FWD => (
            sample_start,
            one_shot_end,
            loop_begin,
            false,
            false,
            false,
            false,
        ),
        REV => (
            sample_start,
            one_shot_end,
            loop_begin,
            true,
            false,
            false,
            false,
        ),
        FWDLOOP => (
            sample_start,
            loop_end,
            loop_begin,
            false,
            true,
            false,
            false,
        ),
        REVLOOP => (loop_begin, loop_end, loop_begin, true, true, false, false),
        FWD_PP => (sample_start, loop_end, loop_begin, false, true, true, false),
        REV_PP => (loop_begin, loop_end, loop_begin, true, true, true, false),
        OSC => (loop_begin, loop_end, loop_begin, false, true, false, true),
        OSC_REV => (loop_begin, loop_end, loop_begin, true, true, false, true),
        // OSC PP starts at LOOP ST going forward, then reflects at the loop
        // boundaries. Retriggers also restart its direction forward.
        OSC_PP => (loop_begin, loop_end, loop_begin, false, true, true, true),
        // REPITCH uses the STEPS-derived rate, and the reference shows its first
        // pass continues beyond START+LENGTH. The loop end bounds that pass.
        REPITCH => (
            sample_start,
            loop_end,
            loop_begin,
            false,
            false,
            false,
            false,
        ),
        REP_REV => (loop_begin, loop_end, loop_begin, true, true, false, false),
        REP_PP => (sample_start, loop_end, loop_begin, false, true, true, false),
        REP_BPM => (
            sample_start,
            loop_end,
            loop_begin,
            false,
            true,
            false,
            false,
        ),
        BPM_REV => (loop_begin, loop_end, loop_begin, true, true, false, false),
        BPM_PP => (sample_start, loop_end, loop_begin, false, true, true, false),
    };

    let play_end = play_end.min(end_of_sample);
    if play_start >= play_end || (looped && loop_begin >= play_end) {
        return None;
    }
    let position = if reverse {
        (play_end - 1) as f64
    } else {
        play_start as f64
    };
    Some(SamplePlaybackRegion {
        position,
        start: play_start as f64,
        loop_start: loop_begin as f64,
        end: play_end as f64,
        direction: if reverse { -1.0 } else { 1.0 },
        looped,
        ping_pong,
        oscillator,
    })
}

#[derive(Clone, Copy)]
struct SampleTiming {
    song_tempo: f64,
    source_rate: u32,
    output_rate: u32,
    sample_frames: usize,
}

fn sample_increment(
    mode: SamplePlayMode,
    detune: u8,
    timing: SampleTiming,
    note: u8,
    sliced: bool,
) -> f64 {
    let native = f64::from(timing.source_rate) / f64::from(timing.output_rate);
    // In ordinary playback modes FINE is a semitone offset around 0x80,
    // with 1/16 semitone per byte. REPITCH and BPM modes use the same field
    // as their timing parameter instead.
    let fine_ratio = if detune == 0 {
        1.0
    } else {
        2.0_f64.powf((f64::from(detune) - 128.0) / 192.0)
    };
    match mode {
        SamplePlayMode::REP_BPM | SamplePlayMode::BPM_REV | SamplePlayMode::BPM_PP
            if detune != 0 =>
        {
            native * timing.song_tempo / f64::from(detune)
        }
        SamplePlayMode::REPITCH | SamplePlayMode::REP_REV | SamplePlayMode::REP_PP
            if detune != 0 =>
        {
            timing.sample_frames as f64 * timing.song_tempo
                / (f64::from(detune) * 15.0 * f64::from(timing.output_rate))
        }
        _ if sliced => native * fine_ratio,
        _ => native * note_ratio(note) * fine_ratio,
    }
}

/// Refresh the held sample once every DEGRADE+1 output frames.
fn sampler_degrade_refresh(degrade: u8, hold_remaining: &mut u16) -> bool {
    if degrade == 0 {
        return true;
    }
    if *hold_remaining == 0 {
        *hold_remaining = u16::from(degrade);
        true
    } else {
        *hold_remaining -= 1;
        false
    }
}

fn macro_redux_step(redux: u8, signal_peak: f32) -> f32 {
    if redux == 0 || signal_peak <= f32::EPSILON {
        return 0.0;
    }
    let step = 0.0429 * 2.0_f32.powf((f32::from(redux) - 1.0) / 51.2) * signal_peak;
    step.max(f32::MIN_POSITIVE)
}

fn macro_redux_sample(sample: f32, redux: u8, signal_peak: f32) -> f32 {
    let step = macro_redux_step(redux, signal_peak);
    if step == 0.0 {
        sample
    } else {
        (sample / step).trunc() * step
    }
}

fn wav_frequency(note: u8) -> f64 {
    // reference sweeps show that SIZE changes the wavetable shape without changing pitch.
    65.406_391_3 * note_ratio(note)
}

struct Track {
    mode: TransportMode,
    playing: bool,
    song_row: u8,
    chain: u8,
    chain_row: u8,
    phrase: u8,
    phrase_step: u8,
    groove: u8,
    groove_row: u8,
    frames_left: f64,
    transport_frames: u64,
    last_instrument: u8,
    instrument_pan_override: Option<u8>,
    pan_mod_base: u8,
    pan_mod_amp: u8,
    pan_mod_update_counter: u8,
    last_velocity: u8,
    voice_velocity_gain: f32,
    voice_amp_gain: f32,
    instrument_amp_offset: i16,
    instrument_pitch_offset: i16,
    table_pitch_offset: i8,
    voice: Voice,
    pitch_mod_update_counter: u8,
    /// OSC sample position is retained per track across note-off and can be
    /// resumed by the next OSC instrument on that track.
    sample_osc_phase: Option<(u8, SamplePlayMode, f64, f64)>,
    macro_state: BraidsStorage,
    macro_timbre: u8,
    macro_color: u8,
    macro_parameter_update_counter: u8,
    macro_post_limit: bool,
    macro_pre_limit: bool,
    macro_limit_mode: u8,
    macro_amp: u8,
    table_index: u8,
    table_row: u8,
    table_tick: u8,
    table_frames_left: f64,
    table_level: f32,
    table_active: bool,
    table_hop_target: Option<u8>,
    table_override: Option<u8>,
    table_tick_override: Option<u8>,
    kill_frames_left: Option<f64>,
    off_frames_left: Option<f64>,
    kill_level: f32,
    volume_mods: [VolumeMod; 4],
    pitch_mods: [PitchMod; 4],
    amp_mods: [ParameterMod; 4],
    res_mods: [ParameterMod; 4],
    degrade_mods: [ParameterMod; 4],
    macro_parameter_mods: [MacroParameterMod; 4],
    pan_mods: [PanMod; 4],
    cutoff_mods: [CutoffMod; 4],
    cutoff_modulated: bool,
    filter_type: u8,
    filter_base_cutoff: u8,
    filter_current_cutoff: u8,
    filter_res: u8,
    filter_current_res: u8,
    filter_update_counter: u8,
    filter: VoiceFilter,
    fm_post_limit: bool,
    eq: StereoEq,
    mixer_dry: u8,
    mixer_dry_gain: f32,
    mixer_chorus_gain: f32,
    mixer_delay_gain: f32,
    mixer_reverb_gain: f32,
    send_overrides: [Option<u8>; 3],
    mod_offsets: [[i16; 5]; 4],
    fm_mod_overrides: [Option<u8>; 4],
    repeat_fx: [RepeatFx; 3],
    pending_effect_fx: [Option<PendingEffectFx>; PENDING_EFFECT_FX_CAPACITY],
    shown: ZtTrackStatus,
}

#[derive(Clone, Copy)]
enum PendingEffectFx {
    ReverbDecay(u8),
    ReverbReturn(u8),
}

#[derive(Clone, Copy, Default)]
struct RepeatFx {
    last: Option<FX>,
    increment: u8,
    active: bool,
}

fn resolve_repeat_fx(
    states: &mut [RepeatFx; 3],
    effects: [FX; 3],
    repeat_id: u8,
) -> [Option<FX>; 3] {
    let mut resolved = [None; 3];
    for (lane, fx) in effects.into_iter().enumerate() {
        let state = &mut states[lane];
        if fx.command == repeat_id {
            if fx.value == 0 {
                state.active = false;
            } else if let Some(mut last) = state.last {
                state.increment = fx.value;
                state.active = true;
                last.value = last.value.saturating_add(state.increment);
                state.last = Some(last);
                resolved[lane] = Some(last);
            }
        } else if fx.command != 0xff {
            state.last = Some(fx);
            state.active = false;
            resolved[lane] = Some(fx);
        } else if state.active
            && let Some(mut last) = state.last
        {
            last.value = last.value.saturating_add(state.increment);
            state.last = Some(last);
            resolved[lane] = Some(last);
        }
    }
    resolved
}

fn offset_parameter(value: &mut u8, offset: i16) {
    *value = (i16::from(*value) + offset).clamp(0, i16::from(u8::MAX)) as u8;
}

fn relative_fx_offset(value: u8) -> i16 {
    if value < 0x80 {
        i16::from(value)
    } else {
        i16::from(value) - 0x100
    }
}

fn apply_mod_offsets(mods: &[Mod; 4], offsets: [[i16; 5]; 4]) -> [Mod; 4] {
    std::array::from_fn(|index| {
        let mut modifier = mods[index].clone();
        let values = offsets[index];
        match &mut modifier {
            Mod::AHDEnv(env) => {
                offset_parameter(&mut env.amount, values[0]);
                offset_parameter(&mut env.attack, values[1]);
                offset_parameter(&mut env.hold, values[2]);
                offset_parameter(&mut env.decay, values[3]);
            }
            Mod::ADSREnv(env) => {
                offset_parameter(&mut env.amount, values[0]);
                offset_parameter(&mut env.attack, values[1]);
                offset_parameter(&mut env.decay, values[3]);
                offset_parameter(&mut env.sustain, values[4]);
            }
            Mod::TrigEnv(env) => {
                offset_parameter(&mut env.amount, values[0]);
                offset_parameter(&mut env.attack, values[1]);
                offset_parameter(&mut env.hold, values[2]);
                offset_parameter(&mut env.decay, values[3]);
            }
            _ => {}
        }
        modifier
    })
}

fn envelope_offset_command(command: &str) -> Option<(usize, usize)> {
    let bytes = command.as_bytes();
    if bytes.len() != 3 || !(b'1'..=b'4').contains(&bytes[2]) {
        return None;
    }
    let parameter = match &bytes[..2] {
        b"EA" | b"EV" => 0,
        b"AT" => 1,
        b"HO" => 2,
        b"DE" => 3,
        b"SU" => 4,
        _ => return None,
    };
    Some((usize::from(bytes[2] - b'1'), parameter))
}

fn envelope_parameter(index: usize) -> Option<EnvelopeParameter> {
    match index {
        0 => Some(EnvelopeParameter::Amount),
        1 => Some(EnvelopeParameter::Attack),
        2 => Some(EnvelopeParameter::Hold),
        3 => Some(EnvelopeParameter::Decay),
        4 => Some(EnvelopeParameter::Sustain),
        _ => None,
    }
}

fn envelope_parameter_value(modifier: &Mod, parameter: usize) -> Option<u8> {
    match modifier {
        Mod::AHDEnv(env) => match parameter {
            0 => Some(env.amount),
            1 => Some(env.attack),
            2 => Some(env.hold),
            3 => Some(env.decay),
            _ => None,
        },
        Mod::ADSREnv(env) => match parameter {
            0 => Some(env.amount),
            1 => Some(env.attack),
            3 => Some(env.decay),
            4 => Some(env.sustain),
            _ => None,
        },
        Mod::TrigEnv(env) => match parameter {
            0 => Some(env.amount),
            1 => Some(env.attack),
            2 => Some(env.hold),
            3 => Some(env.decay),
            _ => None,
        },
        _ => None,
    }
}

const PENDING_EFFECT_FX_CAPACITY: usize = 6;

impl Track {
    fn new() -> Self {
        Self {
            mode: TransportMode::Stop,
            playing: false,
            song_row: 0,
            chain: 0xff,
            chain_row: 0,
            phrase: 0xff,
            phrase_step: 0,
            groove: 0,
            groove_row: 0,
            frames_left: 0.0,
            transport_frames: 0,
            last_instrument: 0,
            instrument_pan_override: None,
            pan_mod_base: 0x80,
            pan_mod_amp: 0,
            pan_mod_update_counter: 0,
            last_velocity: 0x64,
            voice_velocity_gain: velocity_gain(0x64),
            voice_amp_gain: 1.0,
            instrument_amp_offset: 0,
            instrument_pitch_offset: 0,
            table_pitch_offset: 0,
            voice: Voice::Silent,
            pitch_mod_update_counter: 0,
            sample_osc_phase: None,
            macro_state: MacroVoice::empty(),
            macro_timbre: 0,
            macro_color: 0,
            macro_parameter_update_counter: 0,
            macro_post_limit: false,
            macro_pre_limit: false,
            macro_limit_mode: 0,
            macro_amp: 0,
            table_index: 0,
            table_row: 0,
            table_tick: 0,
            table_frames_left: 0.0,
            table_level: 1.0,
            table_active: false,
            table_hop_target: None,
            table_override: None,
            table_tick_override: None,
            kill_frames_left: None,
            off_frames_left: None,
            kill_level: 1.0,
            volume_mods: [VolumeMod::Off; 4],
            pitch_mods: [PitchMod::Off; 4],
            amp_mods: [ParameterMod::Off; 4],
            res_mods: [ParameterMod::Off; 4],
            degrade_mods: [ParameterMod::Off; 4],
            macro_parameter_mods: [MacroParameterMod::Off; 4],
            pan_mods: [PanMod::Off; 4],
            cutoff_mods: [CutoffMod::Off; 4],
            cutoff_modulated: false,
            filter_type: 0,
            filter_base_cutoff: 0,
            filter_current_cutoff: 0,
            filter_res: 0,
            filter_current_res: 0,
            filter_update_counter: 0,
            filter: VoiceFilter::Off,
            fm_post_limit: false,
            eq: StereoEq::off(),
            mixer_dry: 0xc0,
            mixer_dry_gain: 1.0,
            mixer_chorus_gain: 0.0,
            mixer_delay_gain: 0.0,
            mixer_reverb_gain: 0.0,
            send_overrides: [None; 3],
            mod_offsets: [[0; 5]; 4],
            fm_mod_overrides: [None; 4],
            repeat_fx: [RepeatFx::default(); 3],
            pending_effect_fx: [None; PENDING_EFFECT_FX_CAPACITY],
            shown: ZtTrackStatus::default(),
        }
    }

    fn stop(&mut self) {
        self.playing = false;
        self.mode = TransportMode::Stop;
        self.transport_frames = 0;
        self.voice = Voice::Silent;
        self.pitch_mod_update_counter = 0;
        self.sample_osc_phase = None;
        self.table_active = false;
        self.table_hop_target = None;
        self.table_level = 1.0;
        self.table_override = None;
        self.table_tick_override = None;
        self.kill_frames_left = None;
        self.off_frames_left = None;
        self.kill_level = 1.0;
        self.volume_mods = [VolumeMod::Off; 4];
        self.pitch_mods = [PitchMod::Off; 4];
        self.amp_mods = [ParameterMod::Off; 4];
        self.res_mods = [ParameterMod::Off; 4];
        self.degrade_mods = [ParameterMod::Off; 4];
        self.macro_parameter_mods = [MacroParameterMod::Off; 4];
        self.macro_parameter_update_counter = 0;
        self.pan_mods = [PanMod::Off; 4];
        self.pan_mod_base = 0x80;
        self.pan_mod_amp = 0;
        self.pan_mod_update_counter = 0;
        self.cutoff_mods = [CutoffMod::Off; 4];
        self.amp_mods = [ParameterMod::Off; 4];
        self.res_mods = [ParameterMod::Off; 4];
        self.degrade_mods = [ParameterMod::Off; 4];
        self.cutoff_modulated = false;
        self.filter = VoiceFilter::Off;
        self.fm_post_limit = false;
        self.voice_velocity_gain = velocity_gain(self.last_velocity);
        self.voice_amp_gain = 1.0;
        self.instrument_amp_offset = 0;
        self.instrument_pan_override = None;
        self.instrument_pitch_offset = 0;
        self.table_pitch_offset = 0;
        self.eq = StereoEq::off();
        self.mixer_dry = 0xc0;
        self.mixer_dry_gain = 1.0;
        self.mixer_chorus_gain = 0.0;
        self.mixer_delay_gain = 0.0;
        self.mixer_reverb_gain = 0.0;
        self.send_overrides = [None; 3];
        self.mod_offsets = [[0; 5]; 4];
        self.fm_mod_overrides = [None; 4];
        self.repeat_fx = [RepeatFx::default(); 3];
        self.pending_effect_fx = [None; PENDING_EFFECT_FX_CAPACITY];
        self.shown.active = 0;
        self.shown.peak = 0;
    }

    fn seek_chain(&mut self, song: &Song, from: u8) -> bool {
        let Some(chain) = song.chains.get(usize::from(self.chain)) else {
            return false;
        };
        let Some(step) = chain.steps.get(usize::from(from)) else {
            return false;
        };
        let phrase = step.phrase;
        if usize::from(phrase) >= song.phrases.len() {
            return false;
        }
        self.chain_row = from;
        self.phrase = phrase;
        self.phrase_step = 0;
        true
    }

    fn seek_song(&mut self, song: &Song, from: u8, track: usize) -> bool {
        let chain = song.song.steps[usize::from(from) * 8 + track];
        if usize::from(chain) >= song.chains.len() {
            return false;
        }
        self.song_row = from;
        self.chain = chain;
        self.seek_chain(song, 0)
    }

    fn advance_position(&mut self, song: &Song, track: usize) {
        if self.phrase_step < 15 {
            self.phrase_step += 1;
            return;
        }
        self.phrase_step = 0;
        match self.mode {
            TransportMode::Phrase => return,
            TransportMode::Chain | TransportMode::Song => {
                if self.chain_row < 15 && self.seek_chain(song, self.chain_row + 1) {
                    return;
                }
                if self.mode == TransportMode::Chain {
                    self.playing = self.seek_chain(song, 0);
                    return;
                }
                if self.song_row < 255 && self.seek_song(song, self.song_row + 1, track) {
                    return;
                }
                // Each song column loops when it reaches its first empty
                // entry; columns may therefore have different lengths.
                self.playing = self.seek_song(song, 0, track);
                return;
            }
            TransportMode::Stop => {}
        }
        self.playing = false;
    }

    fn step_duration(&mut self, song: &Song, sample_rate: u32) -> f64 {
        let Some(groove) = song.grooves.get(usize::from(self.groove)) else {
            return sample_rate as f64 * 60.0 / 120.0 / 4.0;
        };
        let mut row = usize::from(self.groove_row);
        if groove.steps[row] == 0xff {
            row = 0;
        }
        let ticks = groove.steps[row];
        self.groove_row = if row == 15 || groove.steps[row + 1] == 0xff {
            0
        } else {
            (row + 1) as u8
        };
        let tempo = f64::from(song.tempo).clamp(1.0, 999.0);
        f64::from(sample_rate) * 60.0 * f64::from(ticks) / (tempo * groove.active_ppqn() as f64)
    }

    fn table_row_duration(&self, song: &Song, sample_rate: u32) -> f64 {
        let tempo = f64::from(song.tempo).clamp(1.0, 999.0);
        f64::from(sample_rate) * 60.0 * f64::from(self.table_tick) / (tempo * 24.0)
    }

    fn schedule_kill(&mut self, ticks: u8, song: &Song, sample_rate: u32) {
        let tempo = f64::from(song.tempo).clamp(1.0, 999.0);
        self.kill_frames_left =
            Some(f64::from(sample_rate) * 60.0 * f64::from(ticks) / (tempo * 24.0));
    }

    fn release_voice(&mut self) {
        if let Voice::Sample {
            slot,
            play_mode,
            position,
            direction,
            ..
        } = self.voice
            && sampler_mode_carries_osc_phase(play_mode)
        {
            self.sample_osc_phase = Some((slot, play_mode, position, direction));
        }
        let mut releasing = false;
        for modulator in &mut self.volume_mods {
            releasing |= modulator.release();
        }
        for modulator in &mut self.pitch_mods {
            releasing |= modulator.release();
        }
        if !releasing {
            self.voice = Voice::Silent;
            self.table_active = false;
        }
    }

    fn set_velocity(&mut self, velocity: u8) {
        self.last_velocity = velocity;
        self.voice_velocity_gain = velocity_gain(velocity);
    }

    fn apply_instrument_send(&mut self, command: &str, value: u8) {
        let (index, gain) = match command {
            "SMX" | "SCH" => (0, chorus_send_gain(value)),
            "SDL" => (1, delay_send_gain(value)),
            "SRV" => (2, reverb_send_gain(value)),
            _ => return,
        };
        self.send_overrides[index] = Some(value);
        match index {
            0 => self.mixer_chorus_gain = gain,
            1 => self.mixer_delay_gain = gain,
            _ => self.mixer_reverb_gain = gain,
        }
    }

    fn apply_instrument_pan(&mut self, command: &str, value: u8, instrument: &Instrument) {
        if command != "PAN" {
            return;
        }
        if let Some(params) = instrument_synth_params(instrument) {
            self.instrument_pan_override = Some(value);
            self.pan_mod_base = value;
            self.pan_mod_amp = params.amp;
            self.voice.set_pan(params.amp, value, self.macro_limit_mode);
        }
    }

    fn apply_instrument_amp(&mut self, command: &str, value: u8, instrument: &Instrument) {
        if command != "AMP" {
            return;
        }
        self.instrument_amp_offset = self
            .instrument_amp_offset
            .saturating_add(relative_fx_offset(value));
        self.voice_amp_gain = instrument_amp_gain(instrument, self.instrument_amp_offset);
    }

    fn apply_instrument_pitch(&mut self, command: &str, value: u8, sample_rate: u32) {
        if command != "PIT" {
            return;
        }
        self.instrument_pitch_offset = (self.instrument_pitch_offset + relative_fx_offset(value))
            .clamp(i16::from(i8::MIN), i16::from(i8::MAX));
        self.update_voice_pitch(sample_rate);
    }

    fn update_voice_pitch(&mut self, sample_rate: u32) {
        let semitones = (i16::from(self.table_pitch_offset) + self.instrument_pitch_offset)
            .clamp(i16::from(i8::MIN), i16::from(i8::MAX)) as i8;
        self.voice
            .transpose(semitones, &mut self.macro_state, sample_rate);
    }

    fn apply_instrument_modulator_command(
        &mut self,
        command: &str,
        value: u8,
        instrument: &Instrument,
    ) {
        let Some((index, parameter)) = envelope_offset_command(command) else {
            return;
        };
        self.mod_offsets[index][parameter] =
            self.mod_offsets[index][parameter].saturating_add(relative_fx_offset(value));
        let Some(params) = instrument_synth_params(instrument) else {
            return;
        };
        let effective = apply_mod_offsets(&params.mods, self.mod_offsets);
        let effective_value = envelope_parameter_value(&effective[index], parameter);
        if let (Some(parameter_kind), Some(value)) =
            (envelope_parameter(parameter), effective_value)
        {
            self.volume_mods[index].set_parameter(parameter_kind, value);
            self.pitch_mods[index].set_parameter(parameter_kind, value);
            self.cutoff_mods[index].set_parameter(parameter_kind, value);
            self.voice
                .set_modulator_parameter(index, parameter_kind, value);
        }
    }

    fn resolve_fx(&mut self, effects: [FX; 3], song: &Song) -> [Option<FX>; 3] {
        let commands = m8fmt::FX::fx_command_names(song.version);
        let repeat_id = commands
            .commands
            .iter()
            .position(|name| *name == "REP")
            .unwrap_or(usize::from(u8::MAX)) as u8;
        resolve_repeat_fx(&mut self.repeat_fx, effects, repeat_id)
    }

    fn clear_send_overrides(&mut self) {
        self.send_overrides = [None; 3];
    }

    fn clear_mod_offsets(&mut self) {
        self.mod_offsets = [[0; 5]; 4];
    }

    fn apply_fm_mod_command(&mut self, command: &str, value: u8) {
        let index = match command {
            "FM1" => 0,
            "FM2" => 1,
            "FM3" => 2,
            "FM4" => 3,
            _ => return,
        };
        self.fm_mod_overrides[index] = Some(value);
        self.voice.set_fm_mod_value(index, value);
    }

    fn queue_effect_fx(&mut self, effect_fx: PendingEffectFx) {
        if let Some(slot) = self
            .pending_effect_fx
            .iter_mut()
            .find(|slot| slot.is_none())
        {
            *slot = Some(effect_fx);
        }
    }

    fn schedule_off(&mut self, ticks: u8, song: &Song, sample_rate: u32) {
        if ticks == 0 {
            self.release_voice();
        } else {
            let tempo = f64::from(song.tempo).clamp(1.0, 999.0);
            self.off_frames_left =
                Some(f64::from(sample_rate) * 60.0 * f64::from(ticks) / (tempo * 24.0));
        }
    }

    fn apply_table_row(&mut self, song: &Song, sample_rate: u32) {
        self.table_hop_target = None;
        let Some(step) = song
            .tables
            .get(usize::from(self.table_index))
            .and_then(|table| table.steps.get(usize::from(self.table_row)))
        else {
            self.table_active = false;
            return;
        };
        self.table_pitch_offset = step.transpose as i8;
        self.update_voice_pitch(sample_rate);
        if step.velocity != 0xff {
            self.table_level = (f32::from(step.velocity.min(127)) / 127.0).powf(VELOCITY_EXPONENT);
        }
        let commands = m8fmt::FX::fx_command_names(song.version);
        let instrument = song.instruments.get(usize::from(self.last_instrument));
        let instrument_commands =
            instrument.map(|instrument| instrument.instr_command_text(song.version));
        for fx in self.resolve_fx(step.all_fx(), song).into_iter().flatten() {
            match commands.try_render(fx.command) {
                Some("KIL") => self.schedule_kill(fx.value, song, sample_rate),
                Some("OFF") => self.schedule_off(fx.value, song, sample_rate),
                Some("HOP") => self.table_hop_target = Some(fx.value),
                Some("XRD") => self.queue_effect_fx(PendingEffectFx::ReverbDecay(fx.value)),
                Some("VRE") => self.queue_effect_fx(PendingEffectFx::ReverbReturn(fx.value)),
                _ => {}
            }
            if fx.value != 0
                && instrument_commands.and_then(|pack| pack.try_render(fx.command)) == Some("ET1")
                && let Some(params) = instrument.and_then(instrument_synth_params)
            {
                let frames_per_tick = f64::from(sample_rate) * 60.0
                    / (f64::from(song.tempo).clamp(1.0, 999.0) * 24.0);
                let destinations = instrument
                    .and_then(|instrument| instrument_destination_names(instrument, song.version));
                self.volume_mods[0] = VolumeMod::from_mod(
                    &params.mods[0],
                    destination_byte(destinations, m8fmt::dests::VOLUME),
                    frames_per_tick,
                    sample_rate,
                );
                self.cutoff_mods[0] = CutoffMod::from_mod(
                    &params.mods[0],
                    destination_byte(destinations, m8fmt::dests::CUTOFF),
                    frames_per_tick,
                );
            }
            if let Some(command) = instrument_commands.and_then(|pack| pack.try_render(fx.command))
            {
                self.apply_fm_mod_command(command, fx.value);
                self.apply_instrument_send(command, fx.value);
                if let Some(instrument) = instrument {
                    self.apply_instrument_pan(command, fx.value, instrument);
                    self.apply_instrument_amp(command, fx.value, instrument);
                    self.apply_instrument_pitch(command, fx.value, sample_rate);
                    self.apply_instrument_modulator_command(command, fx.value, instrument);
                }
            }
        }
    }

    fn resolve_table_hops(&mut self, song: &Song, sample_rate: u32) {
        // HOP takes effect when its row is entered. Bound the chain so a
        // malformed table cannot spin forever in the audio callback.
        for _ in 0..16 {
            let Some(target) = self.table_hop_target.take() else {
                return;
            };
            if target == 0xff {
                self.table_active = false;
                return;
            }
            self.table_row = target & 0x0f;
            self.apply_table_row(song, sample_rate);
        }
        self.table_active = false;
    }

    fn start_table(&mut self, song: &Song, sample_rate: u32) {
        let index = self.table_override.unwrap_or(self.last_instrument);
        let Some(table) = song.tables.get(usize::from(index)) else {
            self.table_active = false;
            return;
        };
        let tick = self.table_tick_override.unwrap_or_else(|| {
            song.instruments
                .get(usize::from(self.last_instrument))
                .map_or(0, |instrument| match instrument {
                    Instrument::Sampler(sampler) => sampler.table_tick,
                    Instrument::WavSynth(wav) => wav.table_tick,
                    Instrument::FMSynth(fm) => fm.table_tick,
                    Instrument::MacroSynth(macro_synth) => macro_synth.table_tick,
                    Instrument::HyperSynth(hyper_synth) => hyper_synth.table_tick,
                    _ => 0,
                })
        });
        let triggered_row = self.table_active && self.table_index == index && tick == 0;
        self.table_index = index;
        self.table_row = if triggered_row {
            (self.table_row + 1) & 0x0f
        } else {
            0
        };
        self.table_tick = tick;
        self.table_pitch_offset = 0;
        self.table_hop_target = None;
        self.table_frames_left = self.table_row_duration(song, sample_rate);
        self.table_level = 1.0;
        self.table_active = table.steps.iter().any(|step| {
            step.transpose != 0
                || step.velocity != 0xff
                || step.all_fx().iter().any(|fx| !fx.is_empty())
        });
        if self.table_active {
            self.apply_table_row(song, sample_rate);
            self.resolve_table_hops(song, sample_rate);
        }
    }

    fn advance_table(&mut self, song: &Song, sample_rate: u32) {
        if !self.table_active || self.table_tick == 0 {
            return;
        }
        self.table_frames_left -= 1.0;
        if self.table_frames_left <= 0.0 {
            self.table_row = (self.table_row + 1) & 0x0f;
            self.table_frames_left += self.table_row_duration(song, sample_rate);
            self.apply_table_row(song, sample_rate);
            self.resolve_table_hops(song, sample_rate);
        }
    }

    fn trigger_step(&mut self, snapshot: &PlaybackSnapshot, sample_rate: u32, track: usize) {
        let song = &snapshot.song;
        let Some(step) = song
            .phrases
            .get(usize::from(self.phrase))
            .and_then(|phrase| phrase.steps.get(usize::from(self.phrase_step)))
        else {
            self.playing = false;
            return;
        };
        let chain_step = song
            .chains
            .get(usize::from(self.chain))
            .map(|chain| chain.steps[usize::from(self.chain_row)]);
        self.shown = ZtTrackStatus {
            song_row: self.song_row,
            chain: self.chain,
            chain_row: self.chain_row,
            phrase: self.phrase,
            phrase_step: self.phrase_step,
            peak: 0,
            active: 1,
        };
        if step.instrument != 0xff {
            self.instrument_amp_offset = 0;
            self.voice_amp_gain = 1.0;
            self.instrument_pitch_offset = 0;
            self.clear_mod_offsets();
            if step.instrument != self.last_instrument {
                self.table_override = None;
                self.table_tick_override = None;
                self.clear_send_overrides();
                self.instrument_pan_override = None;
                self.fm_mod_overrides = [None; 4];
            }
            self.last_instrument = step.instrument;
        }
        if step.velocity != 0xff {
            self.set_velocity(step.velocity);
        }
        // The command table is versioned. Use its names rather than assuming
        // numeric IDs across firmware generations.
        let commands = m8fmt::FX::fx_command_names(song.version);
        let mut kill_ticks = None;
        let mut off_ticks = None;
        let mut hop = None;
        for fx in self.resolve_fx(step.all_fx(), song).into_iter().flatten() {
            match commands.try_render(fx.command) {
                Some("GRV") if usize::from(fx.value) < song.grooves.len() => {
                    self.groove = fx.value;
                    self.groove_row = 0;
                }
                Some("INS") if usize::from(fx.value) < song.instruments.len() => {
                    if fx.value != self.last_instrument {
                        self.table_override = None;
                        self.table_tick_override = None;
                        self.clear_send_overrides();
                        self.clear_mod_offsets();
                        self.instrument_pan_override = None;
                        self.fm_mod_overrides = [None; 4];
                    }
                    self.last_instrument = fx.value;
                }
                Some("TBL") if usize::from(fx.value) < song.tables.len() => {
                    self.table_override = Some(fx.value);
                }
                Some("TIC") if fx.value <= 0xfb => {
                    self.table_tick_override = Some(fx.value);
                }
                Some("KIL") => kill_ticks = Some(fx.value),
                Some("OFF") => off_ticks = Some(fx.value),
                Some("HOP") => hop = Some(fx.value),
                Some("XRD") => self.queue_effect_fx(PendingEffectFx::ReverbDecay(fx.value)),
                Some("VRE") => self.queue_effect_fx(PendingEffectFx::ReverbReturn(fx.value)),
                _ => {}
            }
            let instrument = song.instruments.get(usize::from(self.last_instrument));
            if let Some(command) = instrument.and_then(|instrument| {
                instrument
                    .instr_command_text(song.version)
                    .try_render(fx.command)
            }) {
                self.apply_fm_mod_command(command, fx.value);
                self.apply_instrument_send(command, fx.value);
                if let Some(instrument) = instrument {
                    self.apply_instrument_pan(command, fx.value, instrument);
                    self.apply_instrument_amp(command, fx.value, instrument);
                    self.apply_instrument_pitch(command, fx.value, sample_rate);
                    self.apply_instrument_modulator_command(command, fx.value, instrument);
                }
            }
        }
        match step.note.0 {
            0x80 => {
                self.release_voice();
            }
            note @ 0..=127 => {
                let transpose_enabled = song
                    .instruments
                    .get(usize::from(self.last_instrument))
                    .is_some_and(instrument_transpose_enabled);
                let transpose = if transpose_enabled {
                    chain_step.map_or(0, |cs| cs.transpose as i8 as i16)
                        + i16::from(song.transpose as i8)
                } else {
                    0
                };
                let note = (i16::from(note) + transpose).clamp(0, 127) as u8;
                self.trigger_note(snapshot, sample_rate, note);
            }
            _ => {}
        }
        if let Some(ticks) = kill_ticks {
            self.schedule_kill(ticks, song, sample_rate);
        }
        if let Some(ticks) = off_ticks {
            self.schedule_off(ticks, song, sample_rate);
        }
        let step_duration = self.step_duration(song, sample_rate);
        if let Some(target) = hop {
            if target == 0xff {
                self.playing = false;
            } else {
                // In a phrase, HOP jumps to row Y of the following phrase.
                // The row with HOP is processed immediately and consumes no
                // step duration before playback resumes at the target row.
                self.phrase_step = 15;
                self.advance_position(song, track);
                if self.playing {
                    self.phrase_step = target & 0x0f;
                }
            }
        } else {
            self.frames_left += step_duration;
            self.advance_position(song, track);
        }
    }

    fn trigger_note(&mut self, snapshot: &PlaybackSnapshot, sample_rate: u32, note: u8) {
        let slot = self.last_instrument;
        if let Voice::Sample {
            slot: previous_slot,
            play_mode,
            position,
            direction,
            ..
        } = self.voice
            && sampler_mode_carries_osc_phase(play_mode)
        {
            self.sample_osc_phase = Some((previous_slot, play_mode, position, direction));
        }
        let previous_oscillator = self
            .sample_osc_phase
            .take()
            .filter(|(_, mode, _, _)| sampler_mode_carries_osc_phase(*mode));
        self.voice = Voice::Silent;
        self.pitch_mod_update_counter = 0;
        self.kill_frames_left = None;
        self.off_frames_left = None;
        self.kill_level = 1.0;
        let Some(instrument) = snapshot.song.instruments.get(usize::from(slot)) else {
            self.sample_osc_phase = None;
            self.table_active = false;
            return;
        };
        let instrument_is_oscillator = matches!(
            instrument,
            Instrument::Sampler(sampler)
                if matches!(sampler.play_mode, SamplePlayMode::OSC | SamplePlayMode::OSC_REV | SamplePlayMode::OSC_PP)
        );
        if !instrument_is_oscillator {
            self.sample_osc_phase = None;
        }
        self.voice_amp_gain = instrument_amp_gain(instrument, self.instrument_amp_offset);
        let effective_mods = instrument_synth_params(instrument)
            .map(|params| apply_mod_offsets(&params.mods, self.mod_offsets));
        if let Some(params) = instrument_synth_params(instrument) {
            self.mixer_dry = params.mixer_dry;
            self.mixer_dry_gain = dry_gain(self.mixer_dry);
            self.mixer_chorus_gain = self.send_overrides[0]
                .map_or_else(|| chorus_send_gain(params.mixer_mfx), chorus_send_gain);
            self.mixer_delay_gain = self.send_overrides[1]
                .map_or_else(|| delay_send_gain(params.mixer_delay), delay_send_gain);
            self.mixer_reverb_gain = self.send_overrides[2]
                .map_or_else(|| reverb_send_gain(params.mixer_reverb), reverb_send_gain);
        } else {
            self.mixer_dry = 0xc0;
            self.mixer_dry_gain = 1.0;
            self.mixer_chorus_gain = 0.0;
            self.mixer_delay_gain = 0.0;
            self.mixer_reverb_gain = 0.0;
        }
        self.volume_mods =
            instrument_synth_params(instrument).map_or([VolumeMod::Off; 4], |params| {
                let mods = effective_mods.as_ref().unwrap_or(&params.mods);
                let frames_per_tick = f64::from(sample_rate) * 60.0
                    / (f64::from(snapshot.song.tempo).clamp(1.0, 999.0) * 24.0);
                std::array::from_fn(|index| {
                    VolumeMod::from_mod(
                        &mods[index],
                        destination_byte(
                            instrument_destination_names(instrument, snapshot.song.version),
                            m8fmt::dests::VOLUME,
                        ),
                        frames_per_tick,
                        sample_rate,
                    )
                })
            });
        self.pitch_mods =
            instrument_synth_params(instrument).map_or([PitchMod::Off; 4], |params| {
                let mods = effective_mods.as_ref().unwrap_or(&params.mods);
                let frames_per_tick = f64::from(sample_rate) * 60.0
                    / (f64::from(snapshot.song.tempo).clamp(1.0, 999.0) * 24.0);
                std::array::from_fn(|index| {
                    PitchMod::from_mod(
                        &mods[index],
                        destination_byte(
                            instrument_destination_names(instrument, snapshot.song.version),
                            m8fmt::dests::PITCH,
                        ),
                        48.0,
                        matches!(instrument, Instrument::Sampler(_)),
                        frames_per_tick,
                        sample_rate,
                    )
                })
            });
        for modulator in &mut self.pitch_mods {
            modulator.sync_free_phase(self.transport_frames);
        }
        self.pan_mods = instrument_synth_params(instrument).map_or([PanMod::Off; 4], |params| {
            let mods = effective_mods.as_ref().unwrap_or(&params.mods);
            let pan_destination = destination_byte(
                instrument_destination_names(instrument, snapshot.song.version),
                m8fmt::dests::PAN,
            );
            let frames_per_tick = f64::from(sample_rate) * 60.0
                / (f64::from(snapshot.song.tempo).clamp(1.0, 999.0) * 24.0);
            std::array::from_fn(|index| {
                PanMod::from_mod(&mods[index], pan_destination, frames_per_tick)
            })
        });
        for modulator in &mut self.pan_mods {
            modulator.sync_free_phase(self.transport_frames);
        }
        self.cutoff_mods =
            instrument_synth_params(instrument).map_or([CutoffMod::Off; 4], |params| {
                let mods = effective_mods.as_ref().unwrap_or(&params.mods);
                let cutoff_destination = destination_byte(
                    instrument_destination_names(instrument, snapshot.song.version),
                    m8fmt::dests::CUTOFF,
                );
                let frames_per_tick = f64::from(sample_rate) * 60.0
                    / (f64::from(snapshot.song.tempo).clamp(1.0, 999.0) * 24.0);
                std::array::from_fn(|index| {
                    CutoffMod::from_mod(&mods[index], cutoff_destination, frames_per_tick)
                })
            });
        let frames_per_tick = f64::from(sample_rate) * 60.0
            / (f64::from(snapshot.song.tempo).clamp(1.0, 999.0) * 24.0);
        let destinations = instrument_destination_names(instrument, snapshot.song.version);
        let amp_destination = destination_byte(destinations, m8fmt::dests::AMP);
        let res_destination = destination_byte(destinations, m8fmt::dests::RES);
        let degrade_destination = destination_byte(destinations, m8fmt::dests::DEGRADE);
        let mods = effective_mods
            .as_ref()
            .or_else(|| instrument_synth_params(instrument).map(|params| &params.mods));
        let timbre_destination = destination_byte(destinations, "TIMBRE");
        let color_destination = destination_byte(destinations, "COLOR");
        self.macro_parameter_mods = if matches!(instrument, Instrument::MacroSynth(_)) {
            mods.map_or([MacroParameterMod::Off; 4], |mods| {
                std::array::from_fn(|index| {
                    MacroParameterMod::from_mod(
                        &mods[index],
                        timbre_destination,
                        color_destination,
                        frames_per_tick,
                    )
                })
            })
        } else {
            [MacroParameterMod::Off; 4]
        };
        for modulator in &mut self.macro_parameter_mods {
            modulator.sync_free_phase(self.transport_frames);
        }
        self.amp_mods = mods.map_or([ParameterMod::Off; 4], |mods| {
            std::array::from_fn(|index| {
                ParameterMod::from_mod(&mods[index], amp_destination, frames_per_tick)
            })
        });
        self.res_mods = mods.map_or([ParameterMod::Off; 4], |mods| {
            std::array::from_fn(|index| {
                ParameterMod::from_mod(&mods[index], res_destination, frames_per_tick)
            })
        });
        self.degrade_mods = if matches!(instrument, Instrument::Sampler(_)) {
            mods.map_or([ParameterMod::Off; 4], |mods| {
                std::array::from_fn(|index| {
                    ParameterMod::from_mod(&mods[index], degrade_destination, frames_per_tick)
                })
            })
        } else {
            [ParameterMod::Off; 4]
        };
        for modulator in self
            .amp_mods
            .iter_mut()
            .chain(self.res_mods.iter_mut())
            .chain(self.degrade_mods.iter_mut())
        {
            modulator.sync_free_phase(self.transport_frames);
        }
        self.cutoff_modulated = (self.cutoff_mods.iter().any(CutoffMod::is_active)
            || self.res_mods.iter().any(ParameterMod::is_active))
            && instrument_synth_params(instrument)
                .is_some_and(|params| matches!(params.filter_type, 1..=7));
        if let Some(params) = instrument_synth_params(instrument) {
            self.filter_type = params.filter_type;
            self.filter_base_cutoff = params.filter_cutoff;
            self.filter_current_cutoff = params.filter_cutoff;
            self.filter_res = params.filter_res;
            self.filter_current_res = params.filter_res;
        }
        self.filter_update_counter = 0;
        self.filter = instrument_synth_params(instrument).map_or(VoiceFilter::Off, |params| {
            VoiceFilter::from_params(params, sample_rate)
        });
        let limit_mode = instrument_synth_params(instrument).map_or(0, |params| params.limit.id());
        // Suites 53-56 confirm the shared limiter routing and input ceiling
        // across the FM, sampler, WAV, and hyper voices.
        let supports_shared_limit = matches!(
            instrument,
            Instrument::Sampler(_)
                | Instrument::WavSynth(_)
                | Instrument::FMSynth(_)
                | Instrument::HyperSynth(_)
        );
        self.fm_post_limit = matches!(instrument, Instrument::FMSynth(fm)
            if limit_mode == 4 && fm.synth_params.amp != 0);
        self.eq = instrument_synth_params(instrument)
            .and_then(|params| snapshot.song.eqs.get(usize::from(params.associated_eq)))
            .map_or_else(StereoEq::off, |eq| StereoEq::from_params(eq, sample_rate));
        self.macro_post_limit = false;
        self.macro_pre_limit = false;
        self.macro_limit_mode = 0;
        self.macro_amp = 0;
        self.macro_timbre = 0;
        self.macro_color = 0;
        self.macro_parameter_update_counter = 0;
        if supports_shared_limit {
            self.macro_limit_mode = limit_mode;
            self.macro_amp = instrument_synth_params(instrument).map_or(0, |params| params.amp);
            self.macro_pre_limit = limit_mode <= 3;
            self.macro_post_limit = matches!(limit_mode, 4..=8);
        }
        self.voice = match instrument {
            Instrument::Sampler(sampler) => {
                let Some(sample) = snapshot
                    .samples
                    .get(usize::from(slot))
                    .and_then(Option::as_ref)
                else {
                    self.table_active = false;
                    return;
                };
                let cues = snapshot
                    .sample_cues
                    .get(usize::from(slot))
                    .map_or(&[][..], |c| c);
                let Some((region_start, region_end)) =
                    sample_region(sample, cues, sampler.slice, note)
                else {
                    self.table_active = false;
                    return;
                };
                let Some(playback) = sample_playback_region(
                    region_start,
                    region_end,
                    sampler.start,
                    sampler.loop_start,
                    sampler.length,
                    sampler.play_mode,
                ) else {
                    self.table_active = false;
                    return;
                };
                let (gain_left, gain_right) = instrument_limit_gains(
                    sampler.synth_params.amp,
                    sampler.synth_params.mixer_pan,
                    sampler.synth_params.limit.id(),
                );
                let increment = sample_increment(
                    sampler.play_mode,
                    sampler.synth_params.fine_tune,
                    SampleTiming {
                        song_tempo: f64::from(snapshot.song.tempo),
                        source_rate: sample.rate,
                        output_rate: sample_rate,
                        sample_frames: sample.frames(),
                    },
                    note,
                    sampler.slice != 0,
                );
                Voice::Sample {
                    slot,
                    position: if playback.oscillator {
                        previous_oscillator.map_or(
                            playback.position,
                            |(_, previous_mode, position, direction)| {
                                // OSC REV wraps at LOOP ST. If the next slot is
                                // OSC PP and the handoff lands exactly at that
                                // lower edge while moving backward, continue from
                                // the wrapped end of the oscillator window.
                                if sampler.play_mode == SamplePlayMode::OSC_PP
                                    && previous_mode == SamplePlayMode::OSC_REV
                                    && direction < 0.0
                                    && position <= playback.loop_start
                                {
                                    playback.end - 1.0
                                } else {
                                    position
                                }
                            },
                        )
                    } else {
                        playback.position
                    },
                    start: playback.start,
                    loop_start: playback.loop_start,
                    end: playback.end,
                    base_increment: increment,
                    increment,
                    degrade: sampler.degrade,
                    degrade_hold_remaining: 0,
                    held_left: 0.0,
                    held_right: 0.0,
                    looped: playback.looped,
                    ping_pong: playback.ping_pong,
                    play_mode: sampler.play_mode,
                    direction: if playback.oscillator {
                        previous_oscillator.map_or(
                            playback.direction,
                            |(previous_slot, _, _, previous_direction)| match sampler.play_mode {
                                SamplePlayMode::OSC_PP => playback.direction,
                                _ if previous_slot == slot => previous_direction,
                                _ => playback.direction,
                            },
                        )
                    } else {
                        playback.direction
                    },
                    gain_left,
                    gain_right,
                }
            }
            Instrument::WavSynth(wav) => {
                let (gain_left, gain_right) = instrument_limit_gains(
                    wav.synth_params.amp,
                    wav.synth_params.mixer_pan,
                    wav.synth_params.limit.id(),
                );
                let shape = wavetable_fallback(u8::from(wav.shape))
                    .and_then(|shape_id| WavShape::try_from(shape_id).ok())
                    .unwrap_or(wav.shape);
                let frequency = wav_frequency(note);
                let mut noise = 0x1234_5678_u32.wrapping_add(u32::from(slot));
                let noise_hold = next_noise_sample(&mut noise);
                let increment = if matches!(shape, WavShape::NOISE | WavShape::NOISE_PITCHED) {
                    noise_update_rate(note) / f64::from(sample_rate)
                } else {
                    frequency / f64::from(sample_rate)
                };
                Voice::Wave {
                    shape,
                    phase: 0.0,
                    base_increment: increment,
                    increment,
                    noise,
                    noise_hold,
                    gain_left,
                    gain_right,
                }
            }
            Instrument::FMSynth(fm) => {
                let (gain_left, gain_right) = instrument_limit_gains(
                    fm.synth_params.amp,
                    fm.synth_params.mixer_pan,
                    fm.synth_params.limit.id(),
                );
                // FM ratio 01.00 on C-4 is 65.4 Hz in the isolated reference
                // control. Operator level FF saturates a sine at roughly
                // 3.8 times the output limiter's input range.
                let fundamental = 261.625_565_3 * note_ratio(note) / 4.0;
                let base_increments = std::array::from_fn(|index| {
                    let op = &fm.operators[index];
                    let ratio = f64::from(op.ratio) + f64::from(op.ratio_fine) / 100.0;
                    fundamental * ratio / f64::from(sample_rate)
                });
                let frames_per_tick = f64::from(sample_rate) * 60.0
                    / (f64::from(snapshot.song.tempo).clamp(1.0, 999.0) * 24.0);
                let mods = effective_mods.as_ref().unwrap_or(&fm.synth_params.mods);
                let mut level_mods = std::array::from_fn(|index| {
                    let destinations =
                        instrument_destination_names(instrument, snapshot.song.version);
                    FmLevelMod::from_mod(
                        &mods[index],
                        ["MOD1", "MOD2", "MOD3", "MOD4"]
                            .map(|name| destination_byte(destinations, name)),
                        frames_per_tick,
                    )
                });
                for modulator in &mut level_mods {
                    modulator.sync_free_phase(self.transport_frames);
                }
                Voice::Fm {
                    algo: fm.algo.0,
                    phases: [0.0; 4],
                    base_increments,
                    increments: base_increments,
                    pitch_headroom: (127.0 - f32::from(note)).max(0.0),
                    drives: std::array::from_fn(|index| {
                        fm::operator_drive(fm.operators[index].level)
                    }),
                    mod_values: std::array::from_fn(|index| {
                        self.fm_mod_overrides[index]
                            .unwrap_or([fm.mod1, fm.mod2, fm.mod3, fm.mod4][index])
                    }),
                    operator_mods: std::array::from_fn(|index| {
                        [fm.operators[index].mod_a, fm.operators[index].mod_b]
                    }),
                    level_mods,
                    shapes: std::array::from_fn(|index| fm.operators[index].shape),
                    feedback: std::array::from_fn(|index| {
                        // CALFB20/80/FF fit a two-radian maximum with a
                        // slightly concave byte map. Other feedback levels
                        // remain subject to additional independent checks.
                        let amount = f32::from(fm.operators[index].feedback) / 255.0;
                        2.0 * amount.powf(0.9)
                    }),
                    feedback_previous: [0.0; 4],
                    dc_previous_input: 0.0,
                    dc_previous_output: 0.0,
                    dc_pole: (-TAU * 8.0 / f64::from(sample_rate)).exp() as f32,
                    gain_left,
                    gain_right,
                }
            }
            Instrument::MacroSynth(macro_synth) => {
                // These measurements separate the two hard limiters: CLIP is before
                // the voice filter, while POST is after it. Both use the same
                // threshold on the instrument signal, before velocity and
                // mixer gains.
                self.macro_limit_mode = macro_synth.synth_params.limit.id();
                self.macro_amp = macro_synth.synth_params.amp;
                self.macro_timbre = macro_synth.timbre;
                self.macro_color = macro_synth.color;
                self.macro_pre_limit = self.macro_limit_mode <= 3;
                self.macro_post_limit = matches!(self.macro_limit_mode, 4..=8);
                let (gain_left, gain_right) = if matches!(self.macro_limit_mode, 5 | 7 | 8) {
                    pan_gains(0, macro_synth.synth_params.mixer_pan)
                } else if self.macro_limit_mode <= 4 || self.macro_limit_mode == 6 {
                    macro_pan_gains(
                        macro_synth.synth_params.amp,
                        macro_synth.synth_params.mixer_pan,
                    )
                } else {
                    pan_gains(
                        macro_synth.synth_params.amp,
                        macro_synth.synth_params.mixer_pan,
                    )
                };
                MacroVoice::start(
                    &mut self.macro_state,
                    macro_synth.shape,
                    note,
                    macro_synth.timbre,
                    macro_synth.color,
                    sample_rate,
                );
                Voice::Macro {
                    degrade: macro_synth.degrade,
                    degrade_hold_remaining: 0,
                    held_value: 0.0,
                    redux: macro_synth.redux,
                    redux_peak: 0.0,
                    gain_left,
                    gain_right,
                }
            }
            Instrument::HyperSynth(hyper_synth) => {
                let Some(oscillator) = HyperVoice::new(hyper_synth, note, sample_rate) else {
                    self.table_active = false;
                    return;
                };
                let (gain_left, gain_right) = instrument_limit_gains(
                    hyper_synth.synth_params.amp,
                    hyper_synth.synth_params.mixer_pan,
                    hyper_synth.synth_params.limit.id(),
                );
                Voice::Hyper {
                    oscillator,
                    gain_left,
                    gain_right,
                }
            }
            _ => Voice::Silent,
        };
        if let Some(params) = instrument_synth_params(instrument) {
            self.pan_mod_base = self.instrument_pan_override.unwrap_or(params.mixer_pan);
            self.pan_mod_amp = params.amp;
            self.pan_mod_update_counter = 0;
        }
        if let (Some(pan), Some(params)) = (
            self.instrument_pan_override,
            instrument_synth_params(instrument),
        ) {
            self.voice.set_pan(params.amp, pan, self.macro_limit_mode);
        }
        self.table_pitch_offset = 0;
        self.update_voice_pitch(sample_rate);
        if matches!(self.voice, Voice::Silent) {
            self.table_active = false;
        } else {
            self.start_table(&snapshot.song, sample_rate);
        }
    }

    fn frame(&mut self, snapshot: &PlaybackSnapshot, sample_rate: u32, track: usize) -> (f32, f32) {
        // Zero-tick grooves can skip several steps at one sample instant.
        for _ in 0..16 {
            if !self.playing || self.frames_left > 0.0 {
                break;
            }
            self.trigger_step(snapshot, sample_rate, track);
        }
        if self.playing {
            self.frames_left -= 1.0;
        }
        self.advance_table(&snapshot.song, sample_rate);
        if let Some(frames_left) = &mut self.off_frames_left {
            if *frames_left > 0.0 {
                *frames_left -= 1.0;
            } else {
                self.off_frames_left = None;
                self.release_voice();
            }
        }
        if let Some(frames_left) = &mut self.kill_frames_left {
            if *frames_left > 0.0 {
                *frames_left -= 1.0;
            } else {
                // A short release avoids a hard discontinuity when KIL stops
                // a voice. The isolated KIL01 tail falls by roughly
                // 0.5 every millisecond and reaches silence after about 13 ms.
                self.kill_level *= (-1.0_f32 / (0.00135 * sample_rate as f32)).exp();
                if self.kill_level < 0.00005 {
                    self.voice = Voice::Silent;
                    self.kill_frames_left = None;
                }
            }
        }
        let voice_was_silent = matches!(self.voice, Voice::Silent);
        let pitch_mod = self.pitch_mods.iter_mut().map(PitchMod::frame).sum::<f32>();
        let amp_mod = self
            .amp_mods
            .iter_mut()
            .map(ParameterMod::frame)
            .sum::<f32>();
        let res_mod = self
            .res_mods
            .iter_mut()
            .map(ParameterMod::frame)
            .sum::<f32>();
        let degrade_mod = self
            .degrade_mods
            .iter_mut()
            .map(ParameterMod::frame)
            .sum::<f32>()
            .round()
            .clamp(0.0, 255.0) as u8;
        let (timbre_offset, color_offset) = self
            .macro_parameter_mods
            .iter_mut()
            .map(MacroParameterMod::frame)
            .fold((0.0, 0.0), |(timbre_sum, color_sum), (timbre, color)| {
                (timbre_sum + timbre, color_sum + color)
            });
        if matches!(self.voice, Voice::Macro { .. }) {
            if self.macro_parameter_update_counter == 0 {
                let timbre = (f32::from(self.macro_timbre) + timbre_offset)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let color = (f32::from(self.macro_color) + color_offset)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                MacroVoice::set_parameters(&mut self.macro_state, timbre, color);
            }
            self.macro_parameter_update_counter = (self.macro_parameter_update_counter + 1) % 24;
        } else {
            self.macro_parameter_update_counter = 0;
        }
        if self.pitch_mods.iter().any(PitchMod::is_active) && self.pitch_mod_update_counter == 0 {
            let static_pitch = (i16::from(self.table_pitch_offset) + self.instrument_pitch_offset)
                .clamp(i16::from(i8::MIN), i16::from(i8::MAX));
            self.voice.transpose_fractional(
                f64::from(static_pitch) + f64::from(pitch_mod),
                &mut self.macro_state,
                sample_rate,
            );
        }
        self.pitch_mod_update_counter = self.pitch_mod_update_counter.wrapping_add(1) & 0x3f;
        let effective_amp = (f32::from(self.macro_amp) + amp_mod)
            .round()
            .clamp(0.0, 255.0) as u8;
        let amp_drive = if self.amp_mods.iter().any(ParameterMod::is_active) {
            (1.0 + 16.0 * f32::from(effective_amp) / 255.0)
                / (1.0 + 16.0 * f32::from(self.macro_amp) / 255.0)
        } else {
            1.0
        };
        let (left, right) = self
            .voice
            .frame(snapshot, &mut self.macro_state, degrade_mod);
        let (left, right) = if self.macro_pre_limit {
            (
                macro_limit_sample(left * amp_drive, self.macro_limit_mode, effective_amp),
                macro_limit_sample(right * amp_drive, self.macro_limit_mode, effective_amp),
            )
        } else {
            (left, right)
        };
        let macro_velocity_after_limit =
            self.macro_pre_limit || (self.macro_post_limit && !self.fm_post_limit);
        let (left, right) = if self.fm_post_limit || macro_velocity_after_limit {
            (left, right)
        } else {
            (
                left * self.voice_velocity_gain,
                right * self.voice_velocity_gain,
            )
        };
        if self.cutoff_modulated {
            let offset = self
                .cutoff_mods
                .iter_mut()
                .map(CutoffMod::frame)
                .sum::<f32>();
            if self.filter_update_counter == 0 {
                let cutoff = (f32::from(self.filter_base_cutoff) + offset)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let resonance = (f32::from(self.filter_res) + res_mod)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                if cutoff != self.filter_current_cutoff || resonance != self.filter_current_res {
                    self.filter
                        .retune(self.filter_type, cutoff, resonance, sample_rate);
                    self.filter_current_cutoff = cutoff;
                    self.filter_current_res = resonance;
                }
            }
            self.filter_update_counter = (self.filter_update_counter + 1) & 63;
        }
        let (left, right) = self.filter.frame(left, right);
        let (left, right) = if self.fm_post_limit {
            // CALD2F0/FP/RP isolate FM LIMIT=POST after the filter.
            (
                FM_POST_MAKEUP
                    * left.clamp(-FM_POST_THRESHOLD, FM_POST_THRESHOLD)
                    * self.voice_velocity_gain,
                FM_POST_MAKEUP
                    * right.clamp(-FM_POST_THRESHOLD, FM_POST_THRESHOLD)
                    * self.voice_velocity_gain,
            )
        } else if self.macro_post_limit {
            macro_post_limit_frame(
                left,
                right,
                self.macro_limit_mode,
                effective_amp,
                voice_was_silent,
            )
        } else {
            (left, right)
        };
        let (left, right) = if macro_velocity_after_limit {
            (
                left * self.voice_velocity_gain,
                right * self.voice_velocity_gain,
            )
        } else {
            (left, right)
        };
        let (left, right) = self.eq.frame(left, right);
        let pan_modulated = self.pan_mods.iter().any(PanMod::is_active);
        if pan_modulated {
            let displacement = self.pan_mods.iter_mut().map(PanMod::frame).sum::<f32>();
            if self.pan_mod_update_counter == 0 {
                let pan = (f32::from(self.pan_mod_base) + displacement)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                self.voice
                    .set_pan(self.pan_mod_amp, pan, self.macro_limit_mode);
            }
            self.pan_mod_update_counter = self.pan_mod_update_counter.wrapping_add(1) & 0x3f;
        }
        let volume = self
            .volume_mods
            .iter_mut()
            .fold(1.0, |gain, modulator| gain * modulator.frame());
        if self.volume_mods.iter().any(VolumeMod::is_done) {
            self.voice = Voice::Silent;
            self.table_active = false;
        }
        if self.playing {
            self.transport_frames = self.transport_frames.saturating_add(1);
        }
        (
            left * self.table_level * self.kill_level * volume * self.voice_amp_gain,
            right * self.table_level * self.kill_level * volume * self.voice_amp_gain,
        )
    }
}

fn instrument_synth_params(instrument: &Instrument) -> Option<&SynthParams> {
    match instrument {
        Instrument::Sampler(x) => Some(&x.synth_params),
        Instrument::WavSynth(x) => Some(&x.synth_params),
        Instrument::FMSynth(x) => Some(&x.synth_params),
        Instrument::MacroSynth(x) => Some(&x.synth_params),
        Instrument::HyperSynth(x) => Some(&x.synth_params),
        _ => None,
    }
}

fn instrument_destination_names(
    instrument: &Instrument,
    version: m8fmt::Version,
) -> Option<&'static [&'static str]> {
    Some(match instrument {
        Instrument::Sampler(value) => value.destination_names(version),
        Instrument::WavSynth(value) => value.destination_names(version),
        Instrument::FMSynth(value) => value.destination_names(version),
        Instrument::MacroSynth(value) => value.destination_names(version),
        Instrument::HyperSynth(value) => value.destination_names(version),
        Instrument::MIDIOut(value) => value.destination_names(version),
        Instrument::External(value) => value.destination_names(version),
        Instrument::None => return None,
    })
}

fn destination_byte(destinations: Option<&[&str]>, name: &str) -> Option<u8> {
    destinations?
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as u8)
}

fn instrument_transpose_enabled(instrument: &Instrument) -> bool {
    match instrument {
        Instrument::WavSynth(value) => value.transpose,
        Instrument::MacroSynth(value) => value.transpose,
        Instrument::Sampler(value) => value.transpose,
        Instrument::MIDIOut(value) => value.transpose,
        Instrument::FMSynth(value) => value.transpose,
        Instrument::HyperSynth(value) => value.transpose,
        Instrument::External(value) => value.transpose,
        Instrument::None => false,
    }
}

fn instrument_amp_gain(instrument: &Instrument, amp_offset: i16) -> f32 {
    // reference sweeps show that WAVSYNTH AMP commands do not change its output.
    if matches!(instrument, Instrument::WavSynth(_)) {
        return 1.0;
    }
    instrument_synth_params(instrument).map_or(1.0, |params| {
        let effective_amp = (i16::from(params.amp) + amp_offset).clamp(0, i16::from(u8::MAX)) as u8;
        (1.0 + f32::from(effective_amp) / 255.0) / (1.0 + f32::from(params.amp) / 255.0)
    })
}

pub(crate) struct Sequencer {
    sample_rate: u32,
    master_limiter_power: f32,
    master_limiter_bypass: bool,
    tracks: Box<[Track]>,
    chorus: StereoChorus,
    chorus_eq: EffectTransferEq,
    delay: StereoDelay,
    delay_eq: EffectTransferEq,
    reverb: RoomReverb,
    reverb_eq: EffectTransferEq,
    reverb_decay_override: Option<u8>,
    reverb_return_override: Option<u8>,
}

impl Sequencer {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let tracks = (0..8)
            .map(|_| Track::new())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            sample_rate,
            master_limiter_power: 0.0,
            master_limiter_bypass: false,
            tracks,
            chorus: StereoChorus::new(sample_rate),
            chorus_eq: EffectTransferEq::new(5.5, 5.5, 5.5, -4.9, sample_rate),
            delay: StereoDelay::new(sample_rate),
            delay_eq: EffectTransferEq::new(1.7, 9.0, 3.8, -3.1, sample_rate),
            reverb: RoomReverb::new(sample_rate),
            reverb_eq: EffectTransferEq::new(0.0, 5.5, -6.3, -0.8, sample_rate),
            reverb_decay_override: None,
            reverb_return_override: None,
        }
    }

    pub(crate) fn set_master_limiter_bypass(&mut self, bypass: bool) {
        self.master_limiter_bypass = bypass;
    }

    pub(crate) fn transport(
        &mut self,
        snapshot: Option<&PlaybackSnapshot>,
        request: TransportRequest,
    ) {
        self.master_limiter_power = 0.0;
        for track in &mut self.tracks {
            track.stop();
        }
        self.reverb_decay_override = None;
        self.reverb_return_override = None;
        let Some(snapshot) = snapshot else { return };
        let song = &snapshot.song;
        match request.mode {
            TransportMode::Stop => {}
            TransportMode::Song => {
                for (index, track) in self.tracks.iter_mut().enumerate() {
                    track.mode = TransportMode::Song;
                    track.playing = track.seek_song(song, request.row, index);
                    track.frames_left = 0.0;
                }
            }
            TransportMode::Chain => {
                let track = &mut self.tracks[usize::from(request.track)];
                track.mode = TransportMode::Chain;
                track.chain = request.index;
                track.playing = track.seek_chain(song, request.row.min(15));
                track.frames_left = 0.0;
            }
            TransportMode::Phrase => {
                let track = &mut self.tracks[usize::from(request.track)];
                if usize::from(request.index) < song.phrases.len() {
                    track.mode = TransportMode::Phrase;
                    track.phrase = request.index;
                    track.phrase_step = request.row.min(15);
                    track.playing = true;
                    track.frames_left = 0.0;
                }
            }
        }
    }

    pub(crate) fn render(
        &mut self,
        snapshot: Option<&PlaybackSnapshot>,
        output: &mut [i16],
        channels: usize,
        telemetry: &Telemetry,
        output_taps: &TrackOutputTaps,
        track_flags: (u8, u8),
    ) {
        let (mute_mask, solo_mask) = track_flags;
        let mut track_peaks = [[0_u16; 2]; 8];
        let mut track_activity = [[0_u16; 2]; 8];
        let mut master_peak = [0_u16; 2];
        let mut tap_next = output_taps.begin_block();
        let chorus_params =
            snapshot.and_then(|snapshot| self.chorus.params(&snapshot.song.effects_settings));
        let delay_params = snapshot.map(|snapshot| {
            self.delay.params(
                &snapshot.song.effects_settings,
                f64::from(snapshot.song.tempo),
            )
        });
        let reverb_params = snapshot.map(|snapshot| {
            self.reverb
                .params(&snapshot.song.effects_settings, self.reverb_decay_override)
        });
        let (master_gain, track_volume_gains, track_gains) = if let Some(snapshot) = snapshot {
            let volume_gains = std::array::from_fn(|index| {
                MASTER_GAIN * track_volume_gain(snapshot.song.mixer_settings.track_volume[index])
            });
            let gains = std::array::from_fn(|index| {
                let bit = 1_u8 << index;
                let audible = mute_mask & bit == 0 && (solo_mask == 0 || solo_mask & bit != 0);
                if audible { volume_gains[index] } else { 0.0 }
            });
            (
                master_volume_gain(snapshot.song.mixer_settings.master_volume),
                volume_gains,
                gains,
            )
        } else {
            (0.0, [0.0; 8], [0.0; 8])
        };
        let (limiter_makeup, limiter_slope) = snapshot.map_or((1.0, 0.0), |snapshot| {
            master_limiter_parameters(snapshot.song.mixer_settings.limiter.level)
        });
        let limiter_attack =
            (-1.0 / (self.sample_rate as f32 * MASTER_LIMITER_ATTACK_SECONDS)).exp();
        let limiter_release =
            (-1.0 / (self.sample_rate as f32 * MASTER_LIMITER_RELEASE_SECONDS)).exp();
        for frame in output.chunks_exact_mut(channels) {
            let mut track_mono = [0.0_f32; 8];
            let mut left = 0.0_f32;
            let mut right = 0.0_f32;
            let mut chorus_input = (0.0_f32, 0.0_f32);
            let mut delay_input = 0.0_f32;
            let mut reverb_input = 0.0_f32;
            if let Some(snapshot) = snapshot {
                for (index, track) in self.tracks.iter_mut().enumerate() {
                    let (l, r) = track.frame(snapshot, self.sample_rate, index);
                    let activity_l = l * track_volume_gains[index];
                    let activity_r = r * track_volume_gains[index];
                    track_activity[index][0] = track_activity[index][0].max(to_peak(activity_l));
                    track_activity[index][1] = track_activity[index][1].max(to_peak(activity_r));
                    for effect_fx in track.pending_effect_fx.iter_mut().filter_map(Option::take) {
                        match effect_fx {
                            PendingEffectFx::ReverbDecay(value) => {
                                self.reverb_decay_override = Some(value);
                            }
                            PendingEffectFx::ReverbReturn(value) => {
                                self.reverb_return_override = Some(value);
                            }
                        }
                    }
                    let l = l * track_gains[index];
                    let r = r * track_gains[index];
                    track_mono[index] = (l + r) * 0.5;
                    let dry_left = l * track.mixer_dry_gain * master_gain;
                    let dry_right = r * track.mixer_dry_gain * master_gain;
                    chorus_input.0 += l * track.mixer_chorus_gain;
                    chorus_input.1 += r * track.mixer_chorus_gain;
                    delay_input += (l + r) * 0.5 * track.mixer_delay_gain;
                    reverb_input += (l + r) * 0.5 * track.mixer_reverb_gain;
                    left += dry_left;
                    right += dry_right;
                    track_peaks[index][0] = track_peaks[index][0].max(to_peak(l));
                    track_peaks[index][1] = track_peaks[index][1].max(to_peak(r));
                }
                if let Some(params) = chorus_params {
                    let (chorus_left, chorus_right) = self.chorus.frame(chorus_input, params);
                    let (chorus_left, chorus_right) =
                        self.chorus_eq.frame(chorus_left, chorus_right);
                    let chorus_return =
                        track_volume_gain(snapshot.song.mixer_settings.chorus_volume);
                    left += chorus_left * chorus_return * CHORUS_RETURN_CORRECTION * master_gain;
                    right += chorus_right * chorus_return * CHORUS_RETURN_CORRECTION * master_gain;
                }
                if let Some(params) = delay_params {
                    let (delay_left, delay_right) = self.delay.frame(delay_input, params);
                    let (delay_left, delay_right) = self.delay_eq.frame(delay_left, delay_right);
                    let delay_return = track_volume_gain(snapshot.song.mixer_settings.delay_volume);
                    left += delay_left * delay_return * master_gain;
                    right += delay_right * delay_return * master_gain;
                }
                if let Some(params) = reverb_params {
                    let (reverb_left, reverb_right) = self.reverb.frame(reverb_input, params);
                    let (reverb_left, reverb_right) =
                        self.reverb_eq.frame(reverb_left, reverb_right);
                    let reverb_return = track_volume_gain(
                        self.reverb_return_override
                            .unwrap_or(snapshot.song.mixer_settings.reverb_volume),
                    ) * REVERB_RETURN_CORRECTION;
                    left += reverb_left * reverb_return * master_gain;
                    right += reverb_right * reverb_return * master_gain;
                }
            }
            let input_power = if channels == 2 {
                (left * left + right * right) * 0.5
            } else {
                left * left
            };
            let envelope_coeff = if input_power > self.master_limiter_power {
                limiter_attack
            } else {
                limiter_release
            };
            self.master_limiter_power =
                envelope_coeff * self.master_limiter_power + (1.0 - envelope_coeff) * input_power;
            let envelope_db = 10.0 * self.master_limiter_power.max(1e-12).log10();
            let limiter_gain = if self.master_limiter_bypass {
                1.0
            } else {
                master_limiter_gain(envelope_db, limiter_makeup, limiter_slope)
            };
            let master_left = (left * limiter_gain).clamp(-1.0, 1.0);
            frame[0] = (master_left * f32::from(i16::MAX)) as i16;
            let master_right = if channels == 2 {
                (right * limiter_gain).clamp(-1.0, 1.0)
            } else {
                master_left
            };
            if channels == 2 {
                frame[1] = (master_right * f32::from(i16::MAX)) as i16;
            }
            master_peak[0] = master_peak[0].max(to_peak(master_left));
            master_peak[1] = master_peak[1].max(to_peak(master_right));
            output_taps.write_frame(&mut tap_next, track_mono);
        }
        let mut all_peaks = [[0_u16; 2]; 9];
        all_peaks[..8].copy_from_slice(&track_peaks);
        all_peaks[8] = master_peak;
        output_taps.end_block(tap_next, &all_peaks, &track_activity);
        for (index, track) in self.tracks.iter_mut().enumerate() {
            track.shown.peak = track_peaks[index][0].max(track_peaks[index][1]);
            track.shown.active = u8::from(track.playing);
            telemetry.publish(index, track.shown);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poly_blep_pulse_preserves_level_and_smooths_both_edges() {
        let increment = 494.0 / 44_100.0;
        let samples = (0..44_100)
            .map(|frame| pulse_sample((frame as f64 * increment).fract(), increment, 0.5))
            .collect::<Vec<_>>();
        let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>()
            / samples.len() as f32)
            .sqrt();

        assert!((rms - 1.0).abs() < 0.012, "RMS changed: {rms}");
        assert!(samples[0].abs() < 1.0);
        assert!(samples[(0.5 / increment) as usize].abs() < 1.0);
    }

    #[test]
    fn master_limiter_hits_measured_low_and_high_level_gains() {
        assert_eq!(master_limiter_parameters(0x00), (1.0, 0.0));
        for (level, makeup_db, high_reduction_db) in MASTER_LIMITER_KNOTS {
            let (makeup, slope) = master_limiter_parameters(level);
            let low_level_gain_db = 20.0 * master_limiter_gain(-30.36, makeup, slope).log10();
            let high_level_gain_db = 20.0 * master_limiter_gain(-9.64, makeup, slope).log10();
            assert!((low_level_gain_db - makeup_db).abs() < 0.001);
            assert!((makeup_db - high_level_gain_db - high_reduction_db).abs() < 0.001);
        }
    }

    #[test]
    fn engine_state_fits_the_audio_thread_stack_budget() {
        assert!(std::mem::size_of::<Track>() < 24 * 1024);
        assert!(std::mem::size_of::<Sequencer>() < 16 * 1024);
        assert!(std::mem::size_of::<PlaybackSnapshot>() < 8 * 1024);
    }
    use std::sync::Arc;

    fn fx(command: u8, value: u8) -> FX {
        FX { command, value }
    }

    fn test_synth_params() -> SynthParams {
        SynthParams {
            volume: 0,
            pitch: 0,
            fine_tune: 0,
            filter_type: 0,
            filter_cutoff: 0,
            filter_res: 0,
            amp: 0,
            limit: m8fmt::LimitType(0),
            shape: 0,
            mixer_pan: 0,
            mixer_dry: 0,
            mixer_mfx: 0,
            mixer_delay: 0,
            mixer_reverb: 0,
            associated_eq: 0,
            mods: std::array::from_fn(|_| m8fmt::AHDEnv::default().to_mod()),
        }
    }

    #[test]
    fn every_supported_instrument_destination_byte_resolves_by_name() {
        let sampler = Instrument::Sampler(m8fmt::Sampler {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: test_synth_params(),
            sample_path: String::new(),
            play_mode: SamplePlayMode::FWD,
            slice: 0,
            start: 0,
            loop_start: 0,
            length: 0,
            degrade: 0,
        });
        let wav = Instrument::WavSynth(m8fmt::WavSynth {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: test_synth_params(),
            shape: WavShape::default(),
            size: 0,
            mult: 0,
            warp: 0,
            scan: 0,
        });
        let macro_synth = Instrument::MacroSynth(m8fmt::MacroSynth {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: test_synth_params(),
            shape: m8fmt::MacroSynthOsc::default(),
            timbre: 0,
            color: 0,
            degrade: 0,
            redux: 0,
        });
        let fm = Instrument::FMSynth(m8fmt::FMSynth {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: test_synth_params(),
            algo: m8fmt::FmAlgo(0),
            operators: std::array::from_fn(|_| m8fmt::Operator::default()),
            mod1: 0,
            mod2: 0,
            mod3: 0,
            mod4: 0,
        });
        let hyper = Instrument::HyperSynth(m8fmt::HyperSynth {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: test_synth_params(),
            scale: 0,
            default_chord: [0; 7],
            shift: 0,
            swarm: 0,
            width: 0,
            subosc: 0,
            chords: std::array::from_fn(|_| m8fmt::Chord::default()),
            shape: None,
        });

        let version = m8fmt::FIRMWARE_6_0_SONG_VERSION;
        let expected: [(&Instrument, &[&str]); 5] = [
            (
                &sampler,
                &[
                    "OFF", "VOLUME", "PITCH", "LOOP ST", "LENGTH", "DEGRADE", "CUTOFF", "RES",
                    "AMP", "PAN", "MOD AMT", "MOD RATE", "MOD BOTH", "MOD BINV",
                ],
            ),
            (
                &wav,
                &[
                    "OFF", "VOLUME", "PITCH", "SIZE", "MULT", "WARP", "SCAN", "CUTOFF", "RES",
                    "AMP", "PAN", "MOD AMT", "MOD RATE", "MOD BOTH", "MOD BINV",
                ],
            ),
            (
                &macro_synth,
                &[
                    "OFF", "VOLUME", "PITCH", "TIMBRE", "COLOR", "DEGRADE", "REDUX", "CUTOFF",
                    "RES", "AMP", "PAN", "MOD AMT", "MOD RATE", "MOD BOTH", "MOD BINV",
                ],
            ),
            (
                &fm,
                &[
                    "OFF", "VOLUME", "PITCH", "MOD1", "MOD2", "MOD3", "MOD4", "CUTOFF", "RES",
                    "AMP", "PAN", "MOD AMT", "MOD RATE", "MOD BOTH", "MOD BINV",
                ],
            ),
            (
                &hyper,
                &[
                    "OFF", "VOLUME", "PITCH", "SHIFT", "SWARM", "WIDTH", "SUBOSC", "CUTOFF", "RES",
                    "AMP", "PAN", "MOD AMT", "MOD RATE", "MOD BOTH", "MOD BINV",
                ],
            ),
        ];
        for (instrument, expected_names) in expected {
            let names = instrument_destination_names(instrument, version).unwrap();
            assert_eq!(names, expected_names);
            for (byte, name) in names.iter().enumerate() {
                assert_eq!(
                    destination_byte(Some(names), name),
                    Some(byte as u8),
                    "{name}"
                );
            }
            for routed_name in [
                "VOLUME", "PITCH", "CUTOFF", "RES", "AMP", "PAN", "MOD1", "MOD2", "MOD3", "MOD4",
            ] {
                let expected_byte = names
                    .iter()
                    .position(|name| *name == routed_name)
                    .map(|i| i as u8);
                assert_eq!(
                    destination_byte(Some(names), routed_name),
                    expected_byte,
                    "{routed_name}"
                );
            }
            let route_probe = |name| {
                let destination = destination_byte(Some(names), name);
                m8fmt::Mod::AHDEnv(m8fmt::AHDEnv {
                    dest: destination.unwrap_or(0xff),
                    amount: 0xff,
                    attack: 0,
                    hold: 0,
                    decay: 0x20,
                })
            };
            let frames_per_tick = 100.0;
            assert_eq!(
                !matches!(
                    VolumeMod::from_mod(
                        &route_probe("VOLUME"),
                        destination_byte(Some(names), "VOLUME"),
                        frames_per_tick,
                        44_100,
                    ),
                    VolumeMod::Off
                ),
                names.contains(&"VOLUME")
            );
            assert_eq!(
                PitchMod::from_mod(
                    &route_probe("PITCH"),
                    destination_byte(Some(names), "PITCH"),
                    48.0,
                    false,
                    frames_per_tick,
                    44_100,
                )
                .is_active(),
                names.contains(&"PITCH")
            );
            assert_eq!(
                CutoffMod::from_mod(
                    &route_probe("CUTOFF"),
                    destination_byte(Some(names), "CUTOFF"),
                    frames_per_tick,
                )
                .is_active(),
                names.contains(&"CUTOFF")
            );
            assert_eq!(
                PanMod::from_mod(
                    &route_probe("PAN"),
                    destination_byte(Some(names), "PAN"),
                    frames_per_tick,
                )
                .is_active(),
                names.contains(&"PAN")
            );
        }
        assert_eq!(
            destination_byte(instrument_destination_names(&sampler, version), "PAN"),
            Some(9)
        );
        assert_eq!(
            destination_byte(instrument_destination_names(&wav, version), "SIZE"),
            Some(3)
        );
        assert_eq!(
            destination_byte(
                instrument_destination_names(&macro_synth, version),
                "TIMBRE"
            ),
            Some(3)
        );
        assert_eq!(
            destination_byte(instrument_destination_names(&fm, version), "MOD1"),
            Some(3)
        );
        let fm_destinations = ["MOD1", "MOD2", "MOD3", "MOD4"]
            .map(|name| destination_byte(instrument_destination_names(&fm, version), name));
        for (bus, dest) in fm_destinations.iter().enumerate() {
            let mut modulator = FmLevelMod::from_mod(
                &m8fmt::Mod::AHDEnv(m8fmt::AHDEnv {
                    dest: dest.unwrap(),
                    amount: 0xff,
                    attack: 0,
                    hold: 0,
                    decay: 0x20,
                }),
                fm_destinations,
                100.0,
            );
            let mut values = [0.0; 4];
            modulator.frame(&mut values);
            assert!(values[bus] > 0.0, "FM bus {bus}");
            assert!(
                values
                    .iter()
                    .enumerate()
                    .all(|(i, value)| i == bus || *value == 0.0)
            );
        }
    }

    #[test]
    fn wavsynth_amp_fx_does_not_change_voice_gain() {
        let params = SynthParams {
            volume: 0xff,
            pitch: 0,
            fine_tune: 0,
            filter_type: 0,
            filter_cutoff: 0xff,
            filter_res: 0,
            amp: 0,
            limit: m8fmt::LimitType(0),
            shape: 0,
            mixer_pan: 0x80,
            mixer_dry: 0xc0,
            mixer_mfx: 0,
            mixer_delay: 0,
            mixer_reverb: 0,
            associated_eq: 0x80,
            mods: std::array::from_fn(|_| m8fmt::AHDEnv::default().to_mod()),
        };
        let instrument = Instrument::WavSynth(m8fmt::WavSynth {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: params,
            shape: m8fmt::WavShape::PULSE50,
            size: 0xff,
            mult: 0,
            warp: 0,
            scan: 0,
        });

        for offset in [-128, -16, 0, 16, 127] {
            assert_eq!(instrument_amp_gain(&instrument, offset), 1.0);
        }
    }

    #[test]
    fn wavsynth_pitch_fx_accumulates_signed_semitones_and_clamps_at_eight_hz() {
        let mut track = Track::new();
        track.voice = Voice::Wave {
            shape: WavShape::PULSE50,
            phase: 0.0,
            base_increment: 262.0 / 44_100.0,
            increment: 262.0 / 44_100.0,
            noise: 1,
            noise_hold: 1.0,
            gain_left: 1.0,
            gain_right: 1.0,
        };

        track.apply_instrument_pitch("PIT", 0xf4, 44_100);
        if let Voice::Wave { increment, .. } = track.voice {
            assert!((increment * 44_100.0 - 131.0).abs() < 0.01);
        } else {
            panic!("expected wavsynth voice");
        }

        track.apply_instrument_pitch("PIT", 0xf4, 44_100);
        if let Voice::Wave { increment, .. } = track.voice {
            assert!((increment * 44_100.0 - 65.5).abs() < 0.01);
        } else {
            panic!("expected wavsynth voice");
        }

        track.instrument_pitch_offset = 0;
        track.apply_instrument_pitch("PIT", 0x80, 44_100);
        if let Voice::Wave { increment, .. } = track.voice {
            assert!((increment * 44_100.0 - 8.0).abs() < 0.01);
        } else {
            panic!("expected wavsynth voice");
        }
    }

    #[test]
    fn wavsynth_table_transpose_combines_with_instrument_pitch_fx() {
        let mut track = Track::new();
        track.voice = Voice::Wave {
            shape: WavShape::PULSE50,
            phase: 0.0,
            base_increment: 262.0 / 44_100.0,
            increment: 262.0 / 44_100.0,
            noise: 1,
            noise_hold: 1.0,
            gain_left: 1.0,
            gain_right: 1.0,
        };

        track.table_pitch_offset = 0xfc_u8 as i8;
        track.apply_instrument_pitch("PIT", 0xf4, 44_100);

        if let Voice::Wave { increment, .. } = track.voice {
            let expected = 262.0 * 2.0_f64.powf(-16.0 / 12.0);
            assert!((increment * 44_100.0 - expected).abs() < 0.01);
        } else {
            panic!("expected wavsynth voice");
        }
    }

    #[test]
    fn rep_increments_the_last_fx_per_lane_until_stopped() {
        let mut states = [RepeatFx::default(); 3];
        let none = fx(0xff, 0xff);
        let applied = resolve_repeat_fx(&mut states, [fx(0x8b, 0x01), none, none], 0x09);
        assert_eq!(applied[0].map(|x| (x.command, x.value)), Some((0x8b, 0x01)));

        let applied = resolve_repeat_fx(&mut states, [fx(0x09, 0x04), none, none], 0x09);
        assert_eq!(applied[0].map(|x| (x.command, x.value)), Some((0x8b, 0x05)));
        let applied = resolve_repeat_fx(&mut states, [none; 3], 0x09);
        assert_eq!(applied[0].map(|x| (x.command, x.value)), Some((0x8b, 0x09)));

        let applied = resolve_repeat_fx(&mut states, [fx(0x09, 0x00), none, none], 0x09);
        assert_eq!(applied[0], None);
        let applied = resolve_repeat_fx(&mut states, [none; 3], 0x09);
        assert_eq!(applied[0], None);
    }

    #[test]
    fn new_fx_command_stops_rep_only_on_its_own_lane() {
        let mut states = [RepeatFx::default(); 3];
        let none = fx(0xff, 0xff);
        resolve_repeat_fx(&mut states, [fx(0x8b, 0x01), fx(0x91, 0x20), none], 0x09);
        resolve_repeat_fx(&mut states, [fx(0x09, 0x04), fx(0x09, 0x02), none], 0x09);
        let applied = resolve_repeat_fx(&mut states, [fx(0x95, 0x30), none, none], 0x09);
        assert_eq!(applied[0].map(|x| (x.command, x.value)), Some((0x95, 0x30)));
        let applied = resolve_repeat_fx(&mut states, [none; 3], 0x09);
        assert_eq!(applied[0], None);
        assert_eq!(applied[1].map(|x| (x.command, x.value)), Some((0x91, 0x26)));
    }

    #[test]
    fn instrument_offsets_use_signed_relative_values_and_accumulate() {
        assert_eq!(relative_fx_offset(0x70), 112);
        assert_eq!(relative_fx_offset(0xff), -1);
        assert_eq!(relative_fx_offset(0x80), -128);
        let mut value = 0x16;
        offset_parameter(&mut value, relative_fx_offset(0x70));
        assert_eq!(value, 0x86);
        offset_parameter(&mut value, relative_fx_offset(0xff));
        assert_eq!(value, 0x85);
        offset_parameter(&mut value, -0x100);
        assert_eq!(value, 0);
    }

    #[test]
    fn voice_math_is_bounded() {
        assert!((note_ratio(48) - 2.0).abs() < 1e-9);
        assert!((wav_frequency(60) - 261.625_565_2).abs() < 1e-6);
        for pan in [0, 64, 128, 192, 255] {
            let (l, r) = pan_gains(0, pan);
            assert!((0.0..=1.0).contains(&l));
            assert!((0.0..=1.0).contains(&r));
        }
        let (center_left, center_right) = pan_gains(0, 128);
        assert_eq!(center_left, center_right);
        assert!(pan_gains(0, 128).0 > 0.0);
        let left_center = pan_gains(0, 0x80).0;
        let (left_hard, _) = pan_gains(0, 0x00);
        let (left_mid, _) = pan_gains(0, 0x40);
        assert_eq!(left_hard, left_center);
        assert_eq!(left_mid, left_center);
        let (_, right_mid) = pan_gains(0, 0x40);
        assert_eq!(right_mid, center_right * 0.5);
    }

    #[test]
    fn wavsynth_noise_uses_full_scale_bipolar_samples() {
        let mut state = 0x1234_5678;
        let mut sum = 0.0;
        let mut squares = 0.0;
        for _ in 0..100_000 {
            let sample = next_noise_sample(&mut state);
            assert!(sample == -1.0 || sample == 1.0);
            sum += f64::from(sample);
            squares += f64::from(sample * sample);
        }
        assert!(sum.abs() < 1_000.0);
        assert!((squares / 100_000.0 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn wavsynth_noise_update_rate_matches_reference_note_knots() {
        let measured = [
            (0x0c, 1_088.0),
            (0x24, 1_702.0),
            (0x3c, 3_266.0),
            (0x58, 8_405.0),
            (0x70, 19_827.0),
        ];
        for (note, rate) in measured {
            assert!((noise_update_rate(note) - rate).abs() < 0.01);
        }
        assert!(noise_update_rate(0x48) > noise_update_rate(0x3c));
        assert!(noise_update_rate(0x48) < noise_update_rate(0x58));
        assert!(noise_update_rate(0x70) < 44_100.0);
    }

    #[test]
    fn pan_fx_40_places_more_level_on_the_left_side() {
        let mut voice = Voice::Wave {
            shape: WavShape::SINE,
            phase: 0.0,
            base_increment: 0.0,
            increment: 0.0,
            noise: 1,
            noise_hold: 1.0,
            gain_left: std::f32::consts::FRAC_1_SQRT_2,
            gain_right: std::f32::consts::FRAC_1_SQRT_2,
        };
        voice.set_pan(0, 0x40, 0);
        if let Voice::Wave {
            gain_left,
            gain_right,
            ..
        } = voice
        {
            assert!(gain_left > gain_right);
            assert!((gain_left - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
            assert!((gain_right - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        } else {
            panic!("expected wavsynth voice");
        }
    }

    #[test]
    fn velocity_changes_scale_a_voice_after_its_note_on() {
        assert_eq!(velocity_gain(0), 0.0);
        assert_eq!(velocity_gain(127), 1.0);
        assert!((velocity_gain(64) - 0.209).abs() < 0.001);

        let mut track = Track::new();
        track.set_velocity(127);
        assert_eq!(track.voice_velocity_gain, 1.0);
        track.set_velocity(64);
        assert!((track.voice_velocity_gain - 0.209).abs() < 0.001);
        track.set_velocity(0);
        assert_eq!(track.voice_velocity_gain, 0.0);
    }

    #[test]
    fn dry_levels_follow_the_isolated_reference_ratios() {
        assert_eq!(dry_gain(0), 0.0);
        assert_eq!(dry_gain(0xc0), 1.0);
        for (level, expected_db) in [
            (0x40, -30.7),
            (0x80, -15.4),
            (0x90, -11.5),
            (0xa0, -7.7),
            (0xb0, -3.83),
        ] {
            let measured_db = 20.0 * dry_gain(level).log10();
            assert!(
                (measured_db - expected_db).abs() < 0.01,
                "DRY {level:02X}: {measured_db} dB"
            );
        }
        let difference_db = 20.0 * (dry_gain(0xe0) / dry_gain(0xc0)).log10();
        assert!((difference_db - 7.5).abs() < 0.2);
        for level in 1..=u8::MAX {
            assert!(
                dry_gain(level) >= dry_gain(level - 1),
                "DRY curve decreases at {level:02X}"
            );
        }
    }

    #[test]
    fn track_volume_is_linear_in_decibels_around_e0() {
        assert_eq!(track_volume_gain(0), 0.0);
        assert!((track_volume_gain(0xe0) - 1.0).abs() < 1e-6);
        for (value, expected_db, tolerance_db) in [
            (0xff, 7.42, 0.08),
            (0xf0, 3.83, 0.08),
            (0xd0, -3.83, 0.08),
            (0xc0, -7.67, 0.08),
            (0x80, -23.05, 0.08),
            // Low-level PCM quantization makes the last point less precise.
            (0x20, -46.95, 1.0),
            (0x10, -51.05, 1.3),
        ] {
            let measured_db = 20.0 * track_volume_gain(value).log10();
            assert!(
                (measured_db - expected_db).abs() < tolerance_db,
                "track volume {value:02X}: {measured_db} dB"
            );
        }
        for value in 1..=u8::MAX {
            assert!(
                track_volume_gain(value) >= track_volume_gain(value - 1),
                "track volume decreases at {value:02X}"
            );
        }
    }

    #[test]
    fn master_volume_uses_the_measured_decibel_scale() {
        let unity = master_volume_gain(0xe0);
        for (value, expected_db) in [(0xdc, -1.10), (0xee, 3.84)] {
            let measured_db = 20.0 * (master_volume_gain(value) / unity).log10();
            assert!((measured_db - expected_db).abs() < 0.02);
        }
    }

    #[test]
    fn effect_send_corrections_match_suite_control_points() {
        assert_eq!(chorus_send_gain(0), 0.0);
        for &(level, correction) in &DELAY_SEND_CORRECTIONS_DB {
            assert!(
                (interpolated_send_correction(level, &DELAY_SEND_CORRECTIONS_DB) - correction)
                    .abs()
                    < 0.001
            );
        }
        for &(level, correction) in &REVERB_SEND_CORRECTIONS_DB {
            assert!(
                (interpolated_send_correction(level, &REVERB_SEND_CORRECTIONS_DB) - correction)
                    .abs()
                    < 0.001
            );
        }
        for &(level, correction) in &CHORUS_SEND_CORRECTIONS_DB {
            assert!(
                (interpolated_send_correction(level, &CHORUS_SEND_CORRECTIONS_DB) - correction)
                    .abs()
                    < 0.001
            );
        }
    }

    #[test]
    fn instrument_send_commands_override_and_clear_each_wet_bus() {
        let mut track = Track::new();
        track.mixer_chorus_gain = chorus_send_gain(0x20);
        track.mixer_delay_gain = delay_send_gain(0x30);
        track.mixer_reverb_gain = reverb_send_gain(0xa0);

        track.apply_instrument_send("SMX", 0x40);
        track.apply_instrument_send("SDL", 0x50);
        track.apply_instrument_send("SRV", 0x10);
        assert_eq!(track.mixer_chorus_gain, chorus_send_gain(0x40));
        assert_eq!(track.mixer_delay_gain, delay_send_gain(0x50));
        assert_eq!(track.mixer_reverb_gain, reverb_send_gain(0x10));
        assert_eq!(track.send_overrides, [Some(0x40), Some(0x50), Some(0x10)]);

        track.clear_send_overrides();
        assert_eq!(track.send_overrides, [None; 3]);
    }

    #[test]
    fn file_cues_select_distinct_sample_regions() {
        let sample = Sample {
            rate: 48_000,
            channels: 1,
            pcm: Arc::from(vec![0.0; 100]),
        };
        assert_eq!(sample_region(&sample, &[0, 20, 75], 1, 0), Some((0, 20)));
        assert_eq!(sample_region(&sample, &[0, 20, 75], 1, 1), Some((20, 75)));
        assert_eq!(sample_region(&sample, &[0, 20, 75], 1, 2), Some((75, 100)));
        assert_eq!(sample_region(&sample, &[0, 20, 75], 1, 3), None);
        assert_eq!(sample_region(&sample, &[], 4, 2), Some((50, 75)));
    }

    #[test]
    fn sampler_modes_apply_start_length_and_loop_region() {
        let fwd = sample_playback_region(0, 256, 0x40, 0x80, 0x60, SamplePlayMode::FWD).unwrap();
        assert_eq!((fwd.position, fwd.start, fwd.end), (64.0, 64.0, 161.0));
        assert!(!fwd.looped);

        let fwd_loop =
            sample_playback_region(0, 256, 0x40, 0x80, 0x60, SamplePlayMode::FWDLOOP).unwrap();
        assert_eq!(
            (
                fwd_loop.position,
                fwd_loop.start,
                fwd_loop.loop_start,
                fwd_loop.end
            ),
            (64.0, 64.0, 128.0, 225.0)
        );
        assert!(fwd_loop.looped);

        let rev = sample_playback_region(0, 256, 0x40, 0x80, 0x60, SamplePlayMode::REV).unwrap();
        assert_eq!(
            (rev.position, rev.start, rev.end, rev.direction),
            (160.0, 64.0, 161.0, -1.0)
        );

        let osc_rev =
            sample_playback_region(0, 256, 0x40, 0x80, 0x60, SamplePlayMode::OSC_REV).unwrap();
        assert_eq!(
            (
                osc_rev.position,
                osc_rev.start,
                osc_rev.end,
                osc_rev.direction
            ),
            (224.0, 128.0, 225.0, -1.0)
        );
        assert!(osc_rev.oscillator && osc_rev.looped);

        let osc_pp =
            sample_playback_region(0, 256, 0x40, 0x80, 0x60, SamplePlayMode::OSC_PP).unwrap();
        assert_eq!(
            (osc_pp.position, osc_pp.start, osc_pp.end, osc_pp.direction),
            (128.0, 128.0, 225.0, 1.0)
        );
        assert!(osc_pp.oscillator && osc_pp.looped && osc_pp.ping_pong);

        let full_length = sample_playback_region(0, 256, 0, 0, 0xff, SamplePlayMode::FWD).unwrap();
        assert_eq!(full_length.end, 256.0);
    }

    #[test]
    fn sampler_degrade_holds_each_sample_for_degrade_plus_one_frames() {
        let mut hold_remaining = 0;
        let refreshes: Vec<_> = (0..8)
            .map(|_| sampler_degrade_refresh(1, &mut hold_remaining))
            .collect();
        assert_eq!(
            refreshes,
            [true, false, true, false, true, false, true, false]
        );

        let mut hold_remaining = 0;
        let refreshes: Vec<_> = (0..7)
            .map(|_| sampler_degrade_refresh(2, &mut hold_remaining))
            .collect();
        assert_eq!(refreshes, [true, false, false, true, false, false, true]);

        let mut hold_remaining = 0;
        assert!(sampler_degrade_refresh(0, &mut hold_remaining));
        assert!(sampler_degrade_refresh(0, &mut hold_remaining));
    }

    #[test]
    fn macro_redux_uses_level_relative_truncation_toward_zero() {
        let step = macro_redux_step(1, 1.0);
        assert!((step - 0.0429).abs() < 1e-6);
        assert!((macro_redux_sample(0.1, 1, 1.0) - 2.0 * step).abs() < 1e-6);
        assert!((macro_redux_sample(-0.1, 1, 1.0) + 2.0 * step).abs() < 1e-6);
        assert_eq!(macro_redux_sample(0.5, 0xff, 1.0), 0.0);
        assert_eq!(macro_redux_sample(-0.5, 0xff, 1.0), 0.0);
        assert_eq!(macro_redux_sample(0.1, 0, 1.0), 0.1);
    }

    #[test]
    fn macro_sin_fold_and_wrap_match_their_transfer_curves() {
        let threshold = MACRO_LIMIT_THRESHOLD;
        assert_eq!(macro_limit_sample(0.0, 1, 0), 0.0);
        assert!(
            (macro_limit_sample(threshold * 0.5, 1, 0)
                - threshold * std::f32::consts::FRAC_1_SQRT_2)
                .abs()
                < 1e-6
        );
        assert!((macro_limit_sample(threshold, 1, 0) - threshold).abs() < 1e-6);

        for sample in [-threshold, 0.0, threshold * 0.9] {
            assert!((macro_limit_sample(sample, 2, 0) - sample).abs() < 1e-6);
            assert!((macro_limit_sample(sample, 3, 0) - sample).abs() < 1e-6);
        }
        assert!((macro_limit_sample(2.0 * threshold, 2, 0)).abs() < 1e-6);
        assert!((macro_limit_sample(1.5 * threshold, 2, 0) - 0.5 * threshold).abs() < 1e-6);
        assert!((macro_limit_sample(2.0 * threshold, 3, 0)).abs() < 1e-6);
        assert!((macro_limit_sample(3.0 * threshold, 3, 0) + threshold).abs() < 1e-6);
    }

    #[test]
    fn macro_w1_follows_the_measured_transfer_curve() {
        let threshold = MACRO_LIMIT_THRESHOLD;
        assert!((macro_limit_sample(0.45 * threshold, 6, 0) / threshold - 0.53971875).abs() < 1e-6);
        assert_eq!(macro_limit_sample(threshold, 6, 0), threshold);
        assert!((macro_limit_sample(1.55 * threshold, 6, 0) / threshold - 0.53971875).abs() < 1e-6);
        assert_eq!(macro_limit_sample(2.0 * threshold, 6, 0), 0.0);
        assert_eq!(macro_limit_sample(-2.0 * threshold, 6, 0), 0.0);
    }

    #[test]
    fn shared_limiter_amp_is_applied_once() {
        let neutral = instrument_limit_gains(0, 0x80, 0);
        let amp_40 = instrument_limit_gains(0x40, 0x80, 0);
        let post_ad = instrument_limit_gains(0x40, 0x80, 5);
        let (expected_amp_40, _) = macro_pan_gains(0x40, 0x80);
        let (expected_neutral, _) = pan_gains(0, 0x80);

        assert!((neutral.0 - expected_neutral).abs() < 1e-6);
        assert!((amp_40.0 - expected_amp_40).abs() < 1e-6);
        assert!((post_ad.0 - expected_neutral).abs() < 1e-6);
    }

    #[test]
    fn macro_postad_uses_exponential_amp_drive_and_fixed_curve() {
        let threshold = MACRO_LIMIT_THRESHOLD;
        let input_peak = 0.79 * threshold;
        let amp_00 = macro_limit_sample(input_peak, 5, 0) / threshold;
        let amp_10 = macro_limit_sample(input_peak, 5, 0x10) / threshold;
        assert!((amp_00 - 0.55).abs() < 0.02);
        assert!((amp_10 - 0.72).abs() < 0.02);

        let small = 0.001 * threshold;
        let slope_00 = macro_limit_sample(small, 5, 0) / small;
        let slope_10 = macro_limit_sample(small, 5, 0x10) / small;
        assert!((slope_10 / slope_00 - 1.6).abs() < 0.01);
    }

    #[test]
    fn macro_post_w2_matches_the_low_drive_sine_curve() {
        let threshold = MACRO_LIMIT_THRESHOLD;
        let amp = 0x10;
        let input_gain = 1.0 + 16.0 * f32::from(amp) / 255.0;
        let input = 0.7 * threshold / input_gain;
        let expected = 0.51 * threshold * (std::f32::consts::TAU / 2.8 * 0.7).sin();
        assert!((macro_limit_sample(input, 7, amp) - expected).abs() < 1e-6);
        assert_eq!(
            macro_limit_sample(input, 7, 0x40),
            macro_post_ad(input, 0x40)
        );
    }

    #[test]
    fn macro_post_w3_preserves_the_measured_asymmetry_and_amp_curve() {
        let threshold = MACRO_LIMIT_THRESHOLD;
        let negative = macro_limit_sample(-0.01 * threshold, 8, 0x40) / threshold;
        let positive = macro_limit_sample(0.01 * threshold, 8, 0x40) / threshold;
        assert!((negative + 0.055).abs() < 0.002);
        assert!((positive - 0.610).abs() < 0.002);

        let small = 0.001 * threshold;
        let slope_00 = macro_limit_sample(small, 8, 0x00) / small;
        let slope_40 = macro_limit_sample(small, 8, 0x40) / small;
        assert!(slope_40 > slope_00 * 8.0);
        assert!(macro_limit_sample(2.0 * threshold, 8, 0xFF) > 0.95 * threshold);
    }

    #[test]
    fn macro_post_w3_bias_does_not_leak_from_a_silent_voice() {
        let threshold = MACRO_LIMIT_THRESHOLD;
        assert_ne!(macro_limit_sample(0.0, 8, 0x40), 0.0);
        assert_eq!(macro_post_limit_frame(0.0, 0.0, 8, 0x40, true), (0.0, 0.0));
        let (left, right) = macro_post_limit_frame(0.0, 0.0, 8, 0x40, false);
        assert!(left > 0.2 * threshold);
        assert_eq!(left, right);
    }

    #[test]
    fn repitch_modes_use_song_tempo() {
        let timing = SampleTiming {
            song_tempo: 90.0,
            source_rate: 48_000,
            output_rate: 48_000,
            sample_frames: 24_000,
        };
        // BPM lives in fine_tune for these modes; pitch can remain zero.
        let bpm = sample_increment(SamplePlayMode::REP_BPM, 120, timing, 0, true);
        assert!((bpm - 0.75).abs() < 1e-9);
        let timing = SampleTiming {
            song_tempo: 120.0,
            sample_frames: 24_000,
            ..timing
        };
        // DETUNE is a hex byte: 10h = 16 sixteenth steps, 80h = 128.
        let steps_10 = sample_increment(SamplePlayMode::REPITCH, 0x10, timing, 0, true);
        let steps_80 = sample_increment(SamplePlayMode::REPITCH, 0x80, timing, 0, true);
        assert!((steps_10 - 0.25).abs() < 1e-9);
        assert!((steps_80 - 0.03125).abs() < 1e-9);
    }

    #[test]
    fn ordinary_sampler_modes_apply_fine_tune_around_80() {
        let timing = SampleTiming {
            song_tempo: 120.0,
            source_rate: 48_000,
            output_rate: 48_000,
            sample_frames: 24_000,
        };
        let neutral = sample_increment(SamplePlayMode::FWD, 0x80, timing, 36, false);
        let down_seven = sample_increment(SamplePlayMode::FWD, 0x10, timing, 36, false);
        assert!((neutral - 1.0).abs() < 1e-9);
        assert!((down_seven / neutral - 2.0_f64.powf(-7.0 / 12.0)).abs() < 1e-9);

        let sliced = sample_increment(SamplePlayMode::OSC, 0x10, timing, 36, true);
        assert!((sliced / neutral - 2.0_f64.powf(-7.0 / 12.0)).abs() < 1e-9);
        let unset = sample_increment(SamplePlayMode::FWD, 0, timing, 36, false);
        assert!((unset - neutral).abs() < 1e-9);
    }

    #[test]
    fn ping_pong_reflects_at_both_sample_edges() {
        let mut position = 4.25;
        let mut direction = 1.0;
        assert!(loop_position(
            &mut position,
            &mut direction,
            0.0,
            0.0,
            4.0,
            true,
            true
        ));
        assert!((position - 1.75).abs() < 1e-9);
        assert_eq!(direction, -1.0);
        position = -0.5;
        assert!(loop_position(
            &mut position,
            &mut direction,
            0.0,
            0.0,
            4.0,
            true,
            true
        ));
        assert!((position - 0.5).abs() < 1e-9);
        assert_eq!(direction, 1.0);
    }

    #[test]
    fn forward_ping_pong_keeps_intro_only_on_its_first_pass() {
        let mut position = 224.25;
        let mut direction = 1.0;
        assert!(loop_position(
            &mut position,
            &mut direction,
            64.0,
            128.0,
            224.0,
            true,
            true,
        ));
        assert!((128.0..224.0).contains(&position));
        assert_eq!(direction, -1.0);

        position = 127.75;
        assert!(loop_position(
            &mut position,
            &mut direction,
            64.0,
            128.0,
            224.0,
            true,
            true,
        ));
        assert!((128.0..224.0).contains(&position));
        assert_eq!(direction, 1.0);
    }
}
