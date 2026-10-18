//! Initial HYPERSYNTH voice calibrated on four dry chord controls. Other
//! shapes, chord offsets, and independent parameter maps remain provisional.

use m8fmt::{HyperSynth, HyperSynthShape};

#[derive(Clone, Copy)]
pub(crate) struct HyperVoice {
    phases: [[f64; 2]; 6],
    base_increments: [[f64; 2]; 6],
    increments: [[f64; 2]; 6],
    active: [bool; 6],
    sine: bool,
    voice_gain: f32,
    width: f32,
    sub_phase: f64,
    sub_base_increment: f64,
    sub_increment: f64,
    sub_gain: f32,
}

impl HyperVoice {
    pub(crate) fn new(instrument: &HyperSynth, note: u8, sample_rate: u32) -> Option<Self> {
        let sine = match instrument.shape {
            Some(HyperSynthShape::Saw) => false,
            Some(HyperSynthShape::Sine) => true,
            _ => return None,
        };
        let chord_index = usize::from(instrument.default_chord[0] & 0x0f);
        let chord = &instrument.chords[chord_index];
        let active = std::array::from_fn(|index| chord.is_osc_on(index));
        let sub_gain = subosc_gain(instrument.subosc) * if sine { 0.5 } else { 1.0 };
        if !active.iter().any(|enabled| *enabled) && sub_gain == 0.0 {
            return None;
        }
        let fundamental = 65.406_391_325_15 * 2.0_f64.powf((f64::from(note) - 36.0) / 12.0);
        let detune = f64::from(instrument.swarm) / 255.0 * 0.5;
        let base_increments = std::array::from_fn(|index| {
            let offset = f64::from(chord.offsets[index] as i8);
            std::array::from_fn(|pair| {
                let pair_offset = if pair == 0 { -detune } else { detune };
                fundamental * 2.0_f64.powf((offset + pair_offset) / 12.0) / f64::from(sample_rate)
            })
        });
        let active_count = active.iter().filter(|enabled| **enabled).count();
        // The dry one- and six-oscillator controls scale close to sqrt(N).
        // The original SHIFT/SWARM/WIDTH bundle raises level further, but
        // these controls do not isolate which parameter does it. WIDTH is
        // the current interpolation proxy for the combined bundle.
        let spread = (f32::from(instrument.width) / 160.0).min(1.0);
        let shape_gain = if sine { 1.0 } else { 1.44 };
        let voice_gain = 0.234 * shape_gain * (active_count as f32).sqrt() * (1.0 + 0.19 * spread);
        let phases = oscillator_phases(instrument.shift, instrument.width);
        let sub_divisor = if instrument.subosc > 128 { 2.0 } else { 4.0 };
        let sub_base_increment = fundamental / sub_divisor / f64::from(sample_rate);
        Some(Self {
            phases,
            base_increments,
            increments: base_increments,
            active,
            sine,
            voice_gain,
            width: f32::from(instrument.width) / 255.0,
            sub_phase: 0.0,
            sub_base_increment,
            sub_increment: sub_base_increment,
            sub_gain,
        })
    }

    pub(crate) fn transpose(&mut self, semitones: f64) {
        let ratio = 2.0_f64.powf(semitones / 12.0);
        for (increments, bases) in self.increments.iter_mut().zip(self.base_increments) {
            for (increment, base) in increments.iter_mut().zip(bases) {
                *increment = base * ratio;
            }
        }
        self.sub_increment = self.sub_base_increment * ratio;
    }

    pub(crate) fn frame(&mut self) -> (f32, f32) {
        let mut pair = [0.0_f32; 2];
        let mut count = 0_u8;
        for index in 0..6 {
            if !self.active[index] {
                continue;
            }
            count += 1;
            for (side, sample) in pair.iter_mut().enumerate() {
                let phase = &mut self.phases[index][side];
                *sample += if self.sine {
                    (std::f64::consts::TAU * *phase).sin() as f32
                } else {
                    bandlimited_saw(*phase, self.increments[index][side])
                };
                *phase = (*phase + self.increments[index][side]).fract();
            }
        }
        if count > 0 {
            pair = pair.map(|sample| sample / f32::from(count));
        }
        let sub = if self.sub_phase < 0.5 { 1.0 } else { -1.0 };
        self.sub_phase = (self.sub_phase + self.sub_increment).fract();
        let centre = (pair[0] + pair[1]) * 0.5;
        let left = centre * (1.0 - self.width) + pair[0] * self.width;
        let right = centre * (1.0 - self.width) + pair[1] * self.width;
        (
            left * self.voice_gain + sub * self.sub_gain,
            right * self.voice_gain + sub * self.sub_gain,
        )
    }
}

fn bandlimited_saw(phase: f64, increment: f64) -> f32 {
    (2.0 * phase - 1.0) as f32 - poly_blep(phase, increment)
}

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

// Measurements indicate a nonzero six-voice phase spread at SHIFT 00, with two
// groups whose phase motion differs by about half a cycle across the SHIFT
// range. These fitted starts/slopes reproduce the observed SAW partial
// exchange and SINE level curve.
fn oscillator_phases(shift: u8, width: u8) -> [[f64; 2]; 6] {
    const START_PHASES: [f64; 6] = [0.0, 0.7289, 0.1587, 0.4281, 0.8417, 0.9294];
    const SHIFT_SLOPES: [f64; 6] = [0.0, -0.3152, -0.2351, -0.0236, -0.3657, 0.2959];
    // WIDTH adds a stereo phase offset.
    let pair_phase = 0.25 * f64::from(width) / 255.0;
    let shift_phase = f64::from(shift) / 256.0;
    std::array::from_fn(|index| {
        let phase = (START_PHASES[index] + SHIFT_SLOPES[index] * shift_phase).rem_euclid(1.0);
        [phase, (phase + pair_phase).rem_euclid(1.0)]
    })
}

// SUBOSC is linear around 80, with level 1 at either end.
// The calibrated full-scale gain is applied after the common voice scaling.
fn subosc_gain(value: u8) -> f32 {
    0.5 * (f32::from(value.abs_diff(128)) / 128.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use m8fmt::reader::Reader;
    use m8fmt::{Instrument, Song};

    #[test]
    fn subosc_parameter_has_full_at_zero_and_off_at_half_scale() {
        assert!((subosc_gain(0) - 0.5).abs() < 1e-6);
        assert!((subosc_gain(64) - 0.25).abs() < 1e-6);
        assert_eq!(subosc_gain(128), 0.0);
        assert!((subosc_gain(255) - 0.496_093_75).abs() < 1e-6);
    }

    #[test]
    fn shift_and_width_keep_every_voice_phase_in_unit_cycle() {
        for shift in 0..=u8::MAX {
            for width in 0..=u8::MAX {
                for [left, right] in oscillator_phases(shift, width) {
                    assert!(
                        (0.0..1.0).contains(&left),
                        "left phase {left} at shift={shift}, width={width}"
                    );
                    assert!(
                        (0.0..1.0).contains(&right),
                        "right phase {right} at shift={shift}, width={width}"
                    );
                }
            }
        }
    }

    #[test]
    fn saw_chord_is_bounded_and_block_independent() {
        let Ok(path) = std::env::var("ZT_HYPER_FIXTURE") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let song = Song::read_from_reader(&mut Reader::new(bytes)).unwrap();
        let Instrument::HyperSynth(instrument) = &song.instruments[0] else {
            panic!("not HYPERSYNTH")
        };
        let mut whole = HyperVoice::new(instrument, 36, 44_100).unwrap();
        let mut split = whole;
        let a: Vec<_> = (0..1024).map(|_| whole.frame()).collect();
        let mut b: Vec<_> = (0..256).map(|_| split.frame()).collect();
        b.extend((0..768).map(|_| split.frame()));
        assert_eq!(a, b);
        assert!(a.iter().all(|(l, r)| l.abs() <= 1.0 && r.abs() <= 1.0));
    }

    #[test]
    fn fractional_transpose_scales_all_voice_increments() {
        let instrument = HyperSynth {
            number: 0,
            name: String::new(),
            transpose: false,
            table_tick: 0,
            synth_params: m8fmt::SynthParams {
                volume: 0,
                pitch: 0,
                fine_tune: 0,
                filter_type: 0,
                filter_cutoff: 0,
                filter_res: 0,
                amp: 0,
                limit: m8fmt::LimitType(0),
                shape: 0,
                mixer_pan: 128,
                mixer_dry: 0,
                mixer_mfx: 0,
                mixer_delay: 0,
                mixer_reverb: 0,
                associated_eq: 0xff,
                mods: std::array::from_fn(|_| m8fmt::Mod::AHDEnv(m8fmt::AHDEnv::default())),
            },
            scale: 0,
            default_chord: [0; 7],
            shift: 0,
            swarm: 0,
            width: 0,
            subosc: 128,
            chords: std::array::from_fn(|_| m8fmt::Chord {
                mask: 1,
                offsets: [0; 6],
            }),
            shape: Some(HyperSynthShape::Sine),
        };
        let mut voice = HyperVoice::new(&instrument, 60, 44_100).unwrap();
        let before = voice.increments[0][0];
        voice.transpose(0.5);
        let expected = before * 2.0_f64.powf(0.5 / 12.0);
        assert!((voice.increments[0][0] - expected).abs() < 1e-12);
    }
}
