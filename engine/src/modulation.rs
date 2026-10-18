//! Per-voice modulation evaluated without allocations in the audio callback.
//! ADSR and LFO curves are provisional until calibrated against isolated
//! reference recordings.

use m8fmt::{LfoShape, LfoTriggerMode, Mod};
use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU32, Ordering};

static LFO_SEED_COUNTER: AtomicU32 = AtomicU32::new(0x6d2b_79f5);

#[derive(Clone, Copy)]
pub(crate) enum VolumeMod {
    Off,
    Ahd { envelope: AhdVolume, amount: u8 },
    Adsr(AdsrVolume),
    Lfo(LfoVolume),
}

#[derive(Clone, Copy)]
pub(crate) enum EnvelopeParameter {
    Amount,
    Attack,
    Hold,
    Decay,
    Sustain,
}

#[derive(Clone, Copy)]
pub(crate) enum CutoffMod {
    Off,
    Ahd { envelope: AhdVolume, amount: u8 },
    Lfo { oscillator: LfoVolume, amount: u8 },
}

#[derive(Clone, Copy)]
pub(crate) enum ParameterMod {
    Off,
    Ahd { envelope: AhdVolume, amount: u8 },
    Lfo { oscillator: LfoVolume, amount: u8 },
}

#[derive(Clone, Copy)]
pub(crate) enum MacroParameterMod {
    Off,
    Timbre(ParameterMod),
    Color(ParameterMod),
}

impl MacroParameterMod {
    pub(crate) fn from_mod(
        source: &Mod,
        timbre_destination: Option<u8>,
        color_destination: Option<u8>,
        frames_per_tick: f64,
    ) -> Self {
        let source_destination = mod_destination(source);
        if let Some(destination) = timbre_destination
            && source_destination == Some(destination)
        {
            return Self::Timbre(ParameterMod::from_mod(
                source,
                timbre_destination,
                frames_per_tick,
            ));
        }
        if let Some(destination) = color_destination
            && source_destination == Some(destination)
        {
            return Self::Color(ParameterMod::from_mod(
                source,
                color_destination,
                frames_per_tick,
            ));
        }
        Self::Off
    }

    pub(crate) fn frame(&mut self) -> (f32, f32) {
        match self {
            Self::Off => (0.0, 0.0),
            Self::Timbre(modulator) => (modulator.frame(), 0.0),
            Self::Color(modulator) => (0.0, modulator.frame()),
        }
    }

    pub(crate) fn sync_free_phase(&mut self, elapsed_frames: u64) {
        match self {
            Self::Off => {}
            Self::Timbre(modulator) | Self::Color(modulator) => {
                modulator.sync_free_phase(elapsed_frames)
            }
        }
    }
}

fn mod_destination(source: &Mod) -> Option<u8> {
    match source {
        Mod::AHDEnv(env) => Some(env.dest),
        Mod::ADSREnv(env) => Some(env.dest),
        Mod::LFO(lfo) => Some(lfo.dest),
        _ => None,
    }
}

impl ParameterMod {
    pub(crate) fn from_mod(source: &Mod, destination: Option<u8>, frames_per_tick: f64) -> Self {
        match source {
            Mod::AHDEnv(env) if Some(env.dest) == destination && env.amount != 0 => Self::Ahd {
                envelope: AhdVolume::new(255, env.attack, env.hold, env.decay, frames_per_tick),
                amount: env.amount,
            },
            Mod::LFO(lfo) if Some(lfo.dest) == destination && lfo.amount != 0 => Self::Lfo {
                oscillator: LfoVolume::new(
                    lfo.shape,
                    lfo.trigger_mode,
                    255,
                    lfo.freq,
                    lfo.retrigger,
                    frames_per_tick,
                ),
                amount: lfo.amount,
            },
            _ => Self::Off,
        }
    }

    /// Positive parameter displacement in the instrument's 0..255 byte domain.
    pub(crate) fn frame(&mut self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Ahd { envelope, amount } => envelope.frame_envelope() * f32::from(*amount),
            Self::Lfo { oscillator, amount } => {
                (oscillator.frame_bipolar() + 1.0) * (f32::from(*amount) * 0.5)
            }
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub(crate) fn sync_free_phase(&mut self, elapsed_frames: u64) {
        if let Self::Lfo { oscillator, .. } = self {
            oscillator.sync_free_phase(elapsed_frames);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PitchMod {
    Off,
    Ahd {
        envelope: AhdVolume,
        amount: u8,
        depth: f32,
        sampler_ratio: bool,
    },
    Adsr {
        envelope: AdsrVolume,
        amount: u8,
        depth: f32,
        sampler_ratio: bool,
    },
    Lfo {
        oscillator: LfoVolume,
        amount: u8,
        depth: f32,
        sampler_ratio: bool,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum PanMod {
    Off,
    Ahd { envelope: AhdVolume, amount: u8 },
    Lfo { oscillator: LfoVolume, amount: u8 },
}

impl PanMod {
    pub(crate) fn from_mod(source: &Mod, destination: Option<u8>, frames_per_tick: f64) -> Self {
        match source {
            Mod::AHDEnv(env) if Some(env.dest) == destination && env.amount != 0 => Self::Ahd {
                envelope: AhdVolume::new(255, env.attack, env.hold, env.decay, frames_per_tick),
                amount: env.amount,
            },
            Mod::LFO(lfo) if Some(lfo.dest) == destination && lfo.amount != 0 => Self::Lfo {
                oscillator: LfoVolume::new(
                    lfo.shape,
                    lfo.trigger_mode,
                    255,
                    lfo.freq,
                    lfo.retrigger,
                    frames_per_tick,
                ),
                amount: lfo.amount,
            },
            _ => Self::Off,
        }
    }

    /// Signed pan displacement in the 0..255 M8 pan domain.
    pub(crate) fn frame(&mut self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Ahd { envelope, amount } => {
                (2.0 * envelope.frame_envelope() - 1.0) * f32::from(*amount)
            }
            Self::Lfo { oscillator, amount } => oscillator.frame_bipolar() * f32::from(*amount),
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub(crate) fn sync_free_phase(&mut self, elapsed_frames: u64) {
        if let Self::Lfo { oscillator, .. } = self {
            oscillator.sync_free_phase(elapsed_frames);
        }
    }
}

impl PitchMod {
    pub(crate) fn from_mod(
        source: &Mod,
        destination: Option<u8>,
        depth_semitones: f32,
        sampler_ratio: bool,
        frames_per_tick: f64,
        sample_rate: u32,
    ) -> Self {
        match source {
            Mod::AHDEnv(env) if Some(env.dest) == destination && env.amount != 0 => Self::Ahd {
                envelope: AhdVolume::new(255, env.attack, env.hold, env.decay, frames_per_tick),
                amount: env.amount,
                depth: depth_semitones,
                sampler_ratio,
            },
            Mod::ADSREnv(env) if Some(env.dest) == destination && env.amount != 0 => Self::Adsr {
                envelope: AdsrVolume::new(
                    255,
                    env.attack,
                    env.decay,
                    env.sustain,
                    env.release,
                    sample_rate,
                ),
                amount: env.amount,
                depth: depth_semitones,
                sampler_ratio,
            },
            Mod::LFO(lfo) if Some(lfo.dest) == destination && lfo.amount != 0 => Self::Lfo {
                oscillator: LfoVolume::new(
                    lfo.shape,
                    lfo.trigger_mode,
                    255,
                    lfo.freq,
                    lfo.retrigger,
                    frames_per_tick,
                ),
                amount: lfo.amount,
                depth: depth_semitones,
                sampler_ratio,
            },
            _ => Self::Off,
        }
    }

    pub(crate) fn frame(&mut self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Ahd {
                envelope,
                amount,
                depth,
                sampler_ratio,
            } => {
                let level = envelope.frame_envelope();
                if *sampler_ratio {
                    12.0 * (1.0 + level * f32::from(*amount) / 255.0 * 0.946).log2()
                } else {
                    level * (f32::from(*amount) * (*depth / 255.0))
                }
            }
            Self::Adsr {
                envelope,
                amount,
                depth,
                sampler_ratio,
            } => {
                let level = envelope.frame_level();
                if *sampler_ratio {
                    12.0 * (1.0 + level * f32::from(*amount) / 255.0 * 0.946).log2()
                } else {
                    level * (f32::from(*amount) * (*depth / 255.0))
                }
            }
            Self::Lfo {
                oscillator,
                amount,
                depth,
                sampler_ratio,
            } => {
                let shape = oscillator.frame_bipolar();
                if *sampler_ratio {
                    let ratio = 1.0 + shape * f32::from(*amount) / 255.0 * 0.98;
                    12.0 * ratio.max(0.000_1).log2()
                } else {
                    shape * (f32::from(*amount) * (*depth / 255.0))
                }
            }
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub(crate) fn release(&mut self) -> bool {
        if let Self::Adsr { envelope, .. } = self {
            envelope.release();
            true
        } else {
            false
        }
    }

    pub(crate) fn sync_free_phase(&mut self, elapsed_frames: u64) {
        if let Self::Lfo { oscillator, .. } = self {
            oscillator.sync_free_phase(elapsed_frames);
        }
    }

    pub(crate) fn set_parameter(&mut self, parameter: EnvelopeParameter, value: u8) {
        match self {
            Self::Off => {}
            Self::Ahd {
                envelope, amount, ..
            } => match parameter {
                EnvelopeParameter::Amount => *amount = value,
                _ => envelope.set_parameter(parameter, value),
            },
            Self::Adsr {
                envelope, amount, ..
            } => match parameter {
                EnvelopeParameter::Amount => *amount = value,
                _ => envelope.set_parameter(parameter, value),
            },
            Self::Lfo { amount, .. } if matches!(parameter, EnvelopeParameter::Amount) => {
                *amount = value;
            }
            Self::Lfo { .. } => {}
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum FmLevelMod {
    Off,
    Ahd {
        destination: usize,
        envelope: AhdVolume,
        amount: u8,
    },
    Lfo {
        destination: usize,
        oscillator: LfoVolume,
        amount: u8,
    },
}

impl FmLevelMod {
    pub(crate) fn from_mod(
        source: &Mod,
        destinations: [Option<u8>; 4],
        frames_per_tick: f64,
    ) -> Self {
        match source {
            Mod::AHDEnv(env) if env.amount != 0 && destinations.contains(&Some(env.dest)) => {
                Self::Ahd {
                    destination: destinations
                        .iter()
                        .position(|dest| *dest == Some(env.dest))
                        .unwrap(),
                    envelope: AhdVolume::new(255, env.attack, env.hold, env.decay, frames_per_tick),
                    amount: env.amount,
                }
            }
            Mod::LFO(lfo) if lfo.amount != 0 && destinations.contains(&Some(lfo.dest)) => {
                Self::Lfo {
                    destination: destinations
                        .iter()
                        .position(|dest| *dest == Some(lfo.dest))
                        .unwrap(),
                    oscillator: LfoVolume::new(
                        lfo.shape,
                        lfo.trigger_mode,
                        255,
                        lfo.freq,
                        lfo.retrigger,
                        frames_per_tick,
                    ),
                    amount: lfo.amount,
                }
            }
            _ => Self::Off,
        }
    }

    pub(crate) fn frame(&mut self, values: &mut [f32; 4]) {
        match self {
            Self::Off => {}
            Self::Ahd {
                destination,
                envelope,
                amount,
            } => {
                values[*destination] =
                    (values[*destination] + envelope.frame() * f32::from(*amount)).min(255.0);
            }
            Self::Lfo {
                destination,
                oscillator,
                amount,
            } => {
                values[*destination] =
                    (values[*destination] + oscillator.frame() * f32::from(*amount)).min(255.0);
            }
        }
    }

    pub(crate) fn sync_free_phase(&mut self, elapsed_frames: u64) {
        if let Self::Lfo { oscillator, .. } = self {
            oscillator.sync_free_phase(elapsed_frames);
        }
    }
}

impl CutoffMod {
    pub(crate) fn from_mod(
        source: &Mod,
        cutoff_destination: Option<u8>,
        frames_per_tick: f64,
    ) -> Self {
        match source {
            Mod::AHDEnv(env) if Some(env.dest) == cutoff_destination && env.amount != 0 => {
                Self::Ahd {
                    envelope: AhdVolume::new(255, env.attack, env.hold, env.decay, frames_per_tick),
                    amount: env.amount,
                }
            }
            Mod::LFO(lfo) if Some(lfo.dest) == cutoff_destination && lfo.amount != 0 => Self::Lfo {
                oscillator: LfoVolume::new(
                    lfo.shape,
                    lfo.trigger_mode,
                    255,
                    lfo.freq,
                    lfo.retrigger,
                    frames_per_tick,
                ),
                amount: lfo.amount,
            },
            _ => Self::Off,
        }
    }

    pub(crate) fn frame(&mut self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Ahd { envelope, amount } => envelope.frame() * f32::from(*amount),
            Self::Lfo { oscillator, amount } => {
                (oscillator.frame_bipolar() + 1.0) * (f32::from(*amount) * 0.5)
            }
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

impl VolumeMod {
    pub(crate) fn from_mod(
        source: &Mod,
        destination: Option<u8>,
        frames_per_tick: f64,
        sample_rate: u32,
    ) -> Self {
        match source {
            Mod::AHDEnv(env) if Some(env.dest) == destination && env.amount != 0 => Self::Ahd {
                envelope: AhdVolume::new(255, env.attack, env.hold, env.decay, frames_per_tick),
                amount: env.amount,
            },
            Mod::ADSREnv(env) if Some(env.dest) == destination && env.amount != 0 => {
                Self::Adsr(AdsrVolume::new(
                    env.amount,
                    env.attack,
                    env.decay,
                    env.sustain,
                    env.release,
                    sample_rate,
                ))
            }
            Mod::LFO(lfo)
                if Some(lfo.dest) == destination
                    && lfo.amount != 0
                    && ((lfo.trigger_mode == LfoTriggerMode::RETRIG
                        && matches!(lfo.shape, LfoShape::SIN | LfoShape::TRI))
                        || (lfo.trigger_mode == LfoTriggerMode::HOLD
                            && lfo.shape == LfoShape::EXP_DN)) =>
            {
                Self::Lfo(LfoVolume::new(
                    lfo.shape,
                    lfo.trigger_mode,
                    lfo.amount,
                    lfo.freq,
                    lfo.retrigger,
                    frames_per_tick,
                ))
            }
            _ => Self::Off,
        }
    }

    pub(crate) fn frame(&mut self) -> f32 {
        match self {
            Self::Off => 1.0,
            Self::Ahd { envelope, amount } => {
                envelope.frame_envelope() * ahd_volume_amount_gain(*amount)
            }
            Self::Adsr(envelope) => envelope.frame(),
            Self::Lfo(oscillator) => oscillator.frame(),
        }
    }

    pub(crate) fn release(&mut self) -> bool {
        match self {
            Self::Adsr(envelope) => {
                envelope.release();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        match self {
            Self::Ahd { envelope, .. } => envelope.is_done(),
            Self::Adsr(envelope) => envelope.is_done(),
            Self::Lfo(lfo) => lfo.is_done(),
            _ => false,
        }
    }

    pub(crate) fn set_parameter(&mut self, parameter: EnvelopeParameter, value: u8) {
        match self {
            Self::Ahd { envelope, amount } => match parameter {
                EnvelopeParameter::Amount => *amount = value,
                _ => envelope.set_parameter(parameter, value),
            },
            Self::Adsr(envelope) => envelope.set_parameter(parameter, value),
            _ => {}
        }
    }
}

fn ahd_volume_amount_gain(amount: u8) -> f32 {
    // reference captures show that AHD's amount is an initial amplitude level,
    // with the envelope decay applied after it. Interpolate the measured
    // amplitude curve in dB; AMT=00 is treated as disabled by from_mod.
    const POINTS: [(u8, f32); 4] = [(0x40, -12.7), (0x80, -4.5), (0xc0, -3.2), (0xff, 0.0)];
    let amount = amount.max(1);
    let (left, right) = POINTS
        .windows(2)
        .find(|pair| amount <= pair[1].0)
        .map_or((POINTS[2], POINTS[3]), |pair| (pair[0], pair[1]));
    let db = if amount < POINTS[0].0 {
        POINTS[0].1 * f32::from(amount) / f32::from(POINTS[0].0)
    } else {
        let t = f32::from(amount - left.0) / f32::from(right.0 - left.0);
        left.1 + t * (right.1 - left.1)
    };
    10.0_f32.powf(db / 20.0)
}

impl CutoffMod {
    pub(crate) fn set_parameter(&mut self, parameter: EnvelopeParameter, value: u8) {
        if let Self::Ahd { envelope, .. } = self {
            envelope.set_parameter(parameter, value);
        }
    }
}

impl FmLevelMod {
    pub(crate) fn set_parameter(&mut self, parameter: EnvelopeParameter, value: u8) {
        if let Self::Ahd { envelope, .. } = self {
            envelope.set_parameter(parameter, value);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct LfoVolume {
    shape: LfoShape,
    trigger_mode: LfoTriggerMode,
    phase: f64,
    increment: f64,
    depth: f32,
    ticks_per_cycle: f64,
    last_phase: f64,
    rng_state: u32,
    held_random: f32,
    segment_start: f32,
    segment_target: f32,
}

impl LfoVolume {
    fn new(
        shape: LfoShape,
        trigger_mode: LfoTriggerMode,
        amount: u8,
        freq: u8,
        retrigger: u8,
        frames_per_tick: f64,
    ) -> Self {
        // The manual describes FREQ in six-tick sequencer steps for ordinary
        // LFO shapes. The exact phase and amplitude maps await CALLFO controls.
        let tick_shape = matches!(
            shape,
            LfoShape::TRI_T
                | LfoShape::SIN_T
                | LfoShape::RAMPD_T
                | LfoShape::RAMPU_T
                | LfoShape::EXPD_T
                | LfoShape::EXPU_T
                | LfoShape::SQ_D_T
                | LfoShape::SQ_U_T
                | LfoShape::DRUNK
                | LfoShape::RAND_T
                | LfoShape::DRNK_T
        );
        // Tick variants and random glides use the byte as their segment length
        // in sequencer ticks. Continuous periodic variants use six ticks per
        // frequency unit.
        let ticks_per_cycle = f64::from(freq.max(1)) * if tick_shape { 1.0 } else { 6.0 };
        let period_frames = ticks_per_cycle * frames_per_tick;
        let phase = match trigger_mode {
            LfoTriggerMode::HOLD | LfoTriggerMode::FREE => 0.0,
            _ => f64::from(retrigger) / 256.0,
        };
        let mut result = Self {
            shape,
            trigger_mode,
            phase,
            increment: 1.0 / period_frames.max(1.0),
            depth: f32::from(amount) / 255.0,
            ticks_per_cycle,
            last_phase: phase,
            rng_state: next_lfo_seed(),
            held_random: 0.0,
            segment_start: 0.0,
            segment_target: 0.0,
        };
        if matches!(
            shape,
            LfoShape::RANDOM | LfoShape::RAND_T | LfoShape::DRUNK | LfoShape::DRNK_T
        ) {
            result.held_random = result.next_random();
            result.segment_start = result.held_random;
            if matches!(shape, LfoShape::RAND_T | LfoShape::DRUNK | LfoShape::DRNK_T) {
                result.segment_target = result.next_random();
            }
        }
        result
    }

    fn sync_free_phase(&mut self, elapsed_frames: u64) {
        if self.trigger_mode == LfoTriggerMode::FREE {
            // FREE oscillators are already running when a note is triggered.
            // The reference's first measured phase is half a cycle behind the
            // transport-relative phase used by the per-note model.
            self.phase = (elapsed_frames as f64 * self.increment - 0.5).rem_euclid(1.0);
        }
    }

    fn frame(&mut self) -> f32 {
        if self.shape == LfoShape::EXP_DN {
            return 1.0 - self.depth * (1.0 - ((self.frame_bipolar() + 1.0) * 0.5));
        }
        let bipolar = self.frame_bipolar();
        1.0 - self.depth * (1.0 - bipolar) * 0.5
    }

    fn frame_bipolar(&mut self) -> f32 {
        if self.shape == LfoShape::EXP_DN {
            // In HOLD mode the one-shot starts from phase zero and retains
            // its endpoint. Controlled values show that the
            // saved retrigger byte does not set this one-shot's start phase.
            let unipolar = if self.phase >= 1.0 {
                0.0
            } else {
                (-1000.0_f64.ln() * self.phase).exp() as f32
            };
            self.phase = (self.phase + self.increment).min(1.0);
            return 2.0 * unipolar - 1.0;
        }
        let tick_shape = matches!(
            self.shape,
            LfoShape::TRI_T
                | LfoShape::SIN_T
                | LfoShape::RAMPD_T
                | LfoShape::RAMPU_T
                | LfoShape::EXPD_T
                | LfoShape::EXPU_T
                | LfoShape::SQ_D_T
                | LfoShape::SQ_U_T
                | LfoShape::RAND_T
                | LfoShape::DRNK_T
        );
        if self.phase < self.last_phase
            && matches!(
                self.shape,
                LfoShape::RAND_T | LfoShape::DRUNK | LfoShape::DRNK_T
            )
        {
            self.segment_start = self.segment_target;
            self.segment_target = self.next_random();
        }
        self.last_phase = self.phase;
        let phase = if tick_shape {
            ((self.phase * self.ticks_per_cycle).floor() / self.ticks_per_cycle).rem_euclid(1.0)
        } else {
            self.phase
        };
        let bipolar = match self.shape {
            // CALLFO20 begins at minimum gain for RETRIG=00 and reaches its
            // maximum about half a cycle later. Sine needs a quarter-cycle
            // offset; TRI already begins at its minimum at phase zero.
            LfoShape::SIN | LfoShape::SIN_T => -(TAU * phase).cos() as f32,
            LfoShape::TRI | LfoShape::TRI_T => (1.0 - 4.0 * (phase - 0.5).abs()) as f32,
            LfoShape::RAMP_DOWN | LfoShape::RAMPD_T => 1.0 - 2.0 * phase as f32,
            LfoShape::RAMP_UP | LfoShape::RAMPU_T => 2.0 * phase as f32 - 1.0,
            LfoShape::SQR_DN | LfoShape::SQ_D_T => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SQR_UP | LfoShape::SQ_U_T => {
                if phase < 0.5 {
                    -1.0
                } else {
                    1.0
                }
            }
            LfoShape::EXP_DN | LfoShape::EXPD_T => {
                2.0 * (-1000.0_f64.ln() * phase).exp() as f32 - 1.0
            }
            LfoShape::EXP_UP => 2.0 * (phase * phase) as f32 - 1.0,
            LfoShape::EXPU_T => 2.0 * phase.powi(5) as f32 - 1.0,
            LfoShape::RANDOM => self.held_random,
            LfoShape::RAND_T => {
                let glide = (self.phase * self.ticks_per_cycle).min(1.0) as f32;
                self.segment_start + (self.segment_target - self.segment_start) * glide
            }
            LfoShape::DRUNK | LfoShape::DRNK_T => {
                self.segment_start + (self.segment_target - self.segment_start) * self.phase as f32
            }
        };
        self.phase = if self.trigger_mode == LfoTriggerMode::HOLD {
            (self.phase + self.increment).min(1.0)
        } else {
            (self.phase + self.increment).fract()
        };
        bipolar
    }

    fn next_random(&mut self) -> f32 {
        // A small deterministic generator keeps offline renders repeatable.
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 17;
        self.rng_state ^= self.rng_state << 5;
        (self.rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn is_done(&self) -> bool {
        self.shape == LfoShape::EXP_DN
            && self.trigger_mode == LfoTriggerMode::HOLD
            && self.depth >= 1.0
            && self.phase >= 1.0
    }
}

fn next_lfo_seed() -> u32 {
    let seed = LFO_SEED_COUNTER.fetch_add(0x9e37_79b9, Ordering::Relaxed);
    if seed == 0 { 1 } else { seed }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AdsrStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Clone, Copy)]
pub(crate) struct AdsrVolume {
    stage: AdsrStage,
    elapsed: u64,
    attack_frames: u64,
    decay_frames: u64,
    release_db_per_second: f32,
    sustain: f32,
    depth: f32,
    level: f32,
    release_start: f32,
    sample_rate: u32,
}

impl AdsrVolume {
    fn new(amount: u8, attack: u8, decay: u8, sustain: u8, release: u8, sample_rate: u32) -> Self {
        // CALADSR (ATK=08, DEC=10, REL=10) reaches its attack peak within
        // about 10 ms and falls silent about 20 ms after OFF. These ADSR
        // times are much shorter than AHD's tempo ticks; a millisecond map
        // is the first isolated-control approximation.
        let time_to_frames =
            |value: u8| (f64::from(value) * f64::from(sample_rate) / 1000.0).round() as u64;
        // Release tails are approximately linear in dB. Fitting
        // R40/R80/RC0 gives this exponential release-rate map.
        let release_db_per_second = (4_400.0 * (-0.0275 * f64::from(release)).exp()) as f32;
        Self {
            stage: AdsrStage::Attack,
            elapsed: 0,
            attack_frames: time_to_frames(attack),
            decay_frames: time_to_frames(decay),
            release_db_per_second,
            sustain: f32::from(sustain) / 255.0,
            depth: f32::from(amount) / 255.0,
            level: 0.0,
            release_start: 0.0,
            sample_rate,
        }
    }

    fn release(&mut self) {
        if !matches!(self.stage, AdsrStage::Release | AdsrStage::Done) {
            self.release_start = self.level;
            self.elapsed = 0;
            self.stage = AdsrStage::Release;
        }
    }

    fn is_done(&self) -> bool {
        self.stage == AdsrStage::Done
    }

    fn set_parameter(&mut self, parameter: EnvelopeParameter, value: u8) {
        let ticks_to_frames =
            |value: u8| (f64::from(value) * f64::from(self.sample_rate) / 1000.0).round() as u64;
        match parameter {
            EnvelopeParameter::Amount => self.depth = f32::from(value) / 255.0,
            EnvelopeParameter::Attack => self.attack_frames = ticks_to_frames(value),
            EnvelopeParameter::Decay => self.decay_frames = ticks_to_frames(value),
            EnvelopeParameter::Sustain => self.sustain = f32::from(value) / 255.0,
            EnvelopeParameter::Hold => {}
        }
    }

    fn frame(&mut self) -> f32 {
        let level = self.frame_level();
        1.0 - self.depth * (1.0 - level)
    }

    fn frame_level(&mut self) -> f32 {
        loop {
            match self.stage {
                AdsrStage::Attack if self.attack_frames == 0 => {
                    self.level = 1.0;
                    self.stage = AdsrStage::Decay;
                }
                AdsrStage::Attack => {
                    self.elapsed += 1;
                    self.level = self.elapsed as f32 / self.attack_frames as f32;
                    if self.elapsed == self.attack_frames {
                        self.elapsed = 0;
                        self.stage = AdsrStage::Decay;
                    }
                    break;
                }
                AdsrStage::Decay if self.decay_frames == 0 => {
                    self.level = self.sustain;
                    self.stage = AdsrStage::Sustain;
                }
                AdsrStage::Decay => {
                    self.elapsed += 1;
                    let fraction = self.elapsed as f32 / self.decay_frames as f32;
                    self.level = 1.0 - (1.0 - self.sustain) * fraction;
                    if self.elapsed == self.decay_frames {
                        self.elapsed = 0;
                        self.stage = AdsrStage::Sustain;
                    }
                    break;
                }
                AdsrStage::Sustain => {
                    self.level = self.sustain;
                    break;
                }
                AdsrStage::Release => {
                    self.elapsed += 1;
                    let elapsed_seconds = self.elapsed as f64 / f64::from(self.sample_rate);
                    self.level = self.release_start
                        * 10.0_f32
                            .powf(-self.release_db_per_second * elapsed_seconds as f32 / 20.0);
                    if self.level <= 0.000_01 {
                        self.level = 0.0;
                        self.stage = AdsrStage::Done;
                    }
                    break;
                }
                AdsrStage::Done => {
                    self.level = 0.0;
                    break;
                }
            }
        }
        self.level
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AhdVolume {
    elapsed: u64,
    attack_frames: u64,
    hold_frames: u64,
    decay_coefficient: f32,
    decay_level: f32,
    depth: f32,
    frames_per_tick: f64,
}

impl AhdVolume {
    fn new(amount: u8, attack: u8, hold: u8, decay: u8, frames_per_tick: f64) -> Self {
        let decay_frames = f64::from(decay) * frames_per_tick;
        let decay_coefficient = if decay_frames > 0.0 {
            (-1000.0_f64.ln() / decay_frames).exp() as f32
        } else {
            0.0
        };
        Self {
            elapsed: 0,
            attack_frames: (f64::from(attack) * frames_per_tick).round() as u64,
            hold_frames: (f64::from(hold) * frames_per_tick).round() as u64,
            decay_coefficient,
            decay_level: 1.0,
            depth: f32::from(amount) / 255.0,
            frames_per_tick,
        }
    }

    fn set_hold_ticks(&mut self, ticks: u8) {
        self.hold_frames = (f64::from(ticks) * self.frames_per_tick).round() as u64;
    }

    fn set_parameter(&mut self, parameter: EnvelopeParameter, value: u8) {
        match parameter {
            EnvelopeParameter::Amount => self.depth = f32::from(value) / 255.0,
            EnvelopeParameter::Attack => {
                self.attack_frames = (f64::from(value) * self.frames_per_tick).round() as u64;
            }
            EnvelopeParameter::Hold => self.set_hold_ticks(value),
            EnvelopeParameter::Decay => {
                let frames = f64::from(value) * self.frames_per_tick;
                self.decay_coefficient = if frames > 0.0 {
                    (-1000.0_f64.ln() / frames).exp() as f32
                } else {
                    0.0
                };
            }
            EnvelopeParameter::Sustain => {}
        }
    }

    fn frame(&mut self) -> f32 {
        let value = self.frame_envelope();
        1.0 - self.depth * (1.0 - value)
    }

    fn frame_envelope(&mut self) -> f32 {
        let value = if self.elapsed < self.attack_frames {
            (self.elapsed as f32 / self.attack_frames as f32).min(1.0)
        } else if self.elapsed < self.attack_frames + self.hold_frames {
            1.0
        } else {
            let value = self.decay_level;
            self.decay_level *= self.decay_coefficient;
            value
        };
        self.elapsed = self.elapsed.saturating_add(1);
        value
    }

    fn is_done(&self) -> bool {
        self.elapsed >= self.attack_frames + self.hold_frames && self.decay_level <= 0.0011
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdsrVolume, AhdVolume, CutoffMod, FmLevelMod, LfoVolume, MacroParameterMod, PitchMod,
        ahd_volume_amount_gain,
    };
    use m8fmt::{ADSREnv, AHDEnv, LFO, LfoShape, LfoTriggerMode, Mod};

    #[test]
    fn macro_parameter_mods_route_timbre_and_color_destinations() {
        let mut timbre = MacroParameterMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 3,
                amount: 0xff,
                attack: 0,
                hold: 0,
                decay: 0x80,
            }),
            Some(3),
            Some(4),
            2205.0,
        );
        assert_eq!(timbre.frame(), (255.0, 0.0));

        let mut color = MacroParameterMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 4,
                amount: 0xff,
                attack: 0,
                hold: 0,
                decay: 0x80,
            }),
            Some(3),
            Some(4),
            2205.0,
        );
        assert_eq!(color.frame(), (0.0, 255.0));

        let mut unrelated = MacroParameterMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 5,
                amount: 0xff,
                attack: 0,
                hold: 0,
                decay: 0x80,
            }),
            Some(3),
            Some(4),
            2205.0,
        );
        assert_eq!(unrelated.frame(), (0.0, 0.0));
    }

    #[test]
    fn ahd_volume_amount_matches_measured_initial_levels() {
        for (amount, expected_db) in [(0x40, -12.7), (0x80, -4.5), (0xc0, -3.2), (0xff, 0.0)] {
            let actual_db = 20.0 * ahd_volume_amount_gain(amount).log10();
            assert!(
                (actual_db - expected_db).abs() < 0.01,
                "amount {amount:#04x}"
            );
        }
    }

    #[test]
    fn lfo_pitch_uses_bipolar_shape_and_amount_as_semitones() {
        let mut modulator = PitchMod::from_mod(
            &Mod::LFO(LFO {
                shape: LfoShape::TRI,
                dest: 2,
                trigger_mode: LfoTriggerMode::RETRIG,
                freq: 0x80,
                amount: 0x40,
                retrigger: 0,
            }),
            Some(2),
            48.0,
            false,
            2205.0,
            44_100,
        );
        assert!((modulator.frame() + 12.047).abs() < 0.02);
    }

    #[test]
    fn tick_lfo_shapes_and_random_glides_stay_bounded() {
        for shape in [
            LfoShape::TRI_T,
            LfoShape::SIN_T,
            LfoShape::RAMPD_T,
            LfoShape::RAMPU_T,
            LfoShape::EXPD_T,
            LfoShape::EXPU_T,
            LfoShape::SQ_D_T,
            LfoShape::SQ_U_T,
            LfoShape::RAND_T,
            LfoShape::DRNK_T,
        ] {
            let mut lfo = LfoVolume::new(shape, LfoTriggerMode::RETRIG, 255, 0x80, 0, 100.0);
            for _ in 0..100_000 {
                let value = lfo.frame_bipolar();
                assert!(value.is_finite() && (-1.0..=1.0).contains(&value));
            }
        }

        for shape in [LfoShape::DRUNK, LfoShape::DRNK_T] {
            let mut lfo = LfoVolume::new(shape, LfoTriggerMode::RETRIG, 255, 0x20, 0, 100.0);
            let samples: Vec<_> = (0..100).map(|_| lfo.frame_bipolar()).collect();
            let first_slope = samples[1] - samples[0];
            for pair in samples.windows(2).skip(1) {
                assert!(((pair[1] - pair[0]) - first_slope).abs() < 1.0e-5);
            }
        }
    }

    #[test]
    fn random_lfo_shapes_use_generated_values_without_shape_specific_offsets() {
        let draws: Vec<f64> = (0..2048)
            .map(|_| {
                let mut random =
                    LfoVolume::new(LfoShape::RANDOM, LfoTriggerMode::RETRIG, 255, 0x20, 0, 1.0);
                f64::from(random.frame_bipolar())
            })
            .collect();
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let variance = draws.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / draws.len() as f64;
        assert!(draws.iter().all(|x| (-1.0..=1.0).contains(x)));
        assert!(mean.abs() < 0.06, "random mean {mean}");
        assert!((variance.sqrt() - 0.577).abs() < 0.04);

        let mut random =
            LfoVolume::new(LfoShape::RANDOM, LfoTriggerMode::RETRIG, 255, 0x20, 0, 1.0);
        let held = random.frame_bipolar();
        for _ in 0..100_000 {
            assert_eq!(random.frame_bipolar(), held);
        }
    }

    #[test]
    fn random_tick_lfo_glides_briefly_then_holds_until_the_next_segment() {
        let mut lfo = LfoVolume::new(LfoShape::RAND_T, LfoTriggerMode::RETRIG, 255, 0x20, 0, 20.0);
        let mut samples: Vec<_> = (0..60).map(|_| lfo.frame_bipolar()).collect();
        assert!(samples[..20].windows(2).any(|p| p[0] != p[1]));
        let held = samples[30];
        for _ in 0..600 {
            samples.push(lfo.frame_bipolar());
        }
        assert!(samples[30..641].iter().all(|x| (*x - held).abs() < 1.0e-6));
        assert!((samples[641] - held).abs() > 1.0e-3);
    }

    #[test]
    fn fm_level_lfo_modulates_each_mod_bus() {
        for destination in 3..=6 {
            let mut modulator = FmLevelMod::from_mod(
                &Mod::LFO(LFO {
                    shape: LfoShape::TRI,
                    dest: destination,
                    trigger_mode: LfoTriggerMode::RETRIG,
                    freq: 0x40,
                    amount: 0x80,
                    retrigger: 0,
                }),
                [Some(3), Some(4), Some(5), Some(6)],
                100.0,
            );
            let mut values = [0.0; 4];
            for _ in 0..20_000 {
                modulator.frame(&mut values);
            }
            let selected = usize::from(destination - 3);
            assert!(values[selected] > 0.0, "destination {destination}");
            for (index, value) in values.iter().enumerate() {
                if index != selected {
                    assert_eq!(*value, 0.0, "unexpected bus {index}");
                }
            }
        }
    }

    #[test]
    fn ahd_volume_finishes_after_its_decay_reaches_silence() {
        let mut envelope = AhdVolume::new(255, 0, 0, 128, 2205.0);
        for _ in 0..128 * 2205 {
            envelope.frame_envelope();
        }
        assert!(envelope.is_done());
    }

    #[test]
    fn ahd_cutoff_destination_opens_then_closes_the_filter() {
        let mut modulator = CutoffMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 7,
                amount: 0xbf,
                attack: 0,
                hold: 0,
                decay: 0x4f,
            }),
            Some(7),
            100.0,
        );
        assert!(modulator.is_active());
        assert!((modulator.frame() - 191.0).abs() < 0.01);
        for _ in 0..7900 {
            modulator.frame();
        }
        assert!(modulator.frame() < 1.0);
    }

    #[test]
    fn lfo_cutoff_maps_bipolar_wave_to_unipolar_cutoff_amount() {
        let mut modulator = CutoffMod::from_mod(
            &Mod::LFO(LFO {
                shape: LfoShape::TRI,
                dest: 7,
                trigger_mode: LfoTriggerMode::RETRIG,
                freq: 0x80,
                amount: 0xff,
                retrigger: 0,
            }),
            Some(7),
            2205.0,
        );
        assert!(modulator.is_active());
        assert!(modulator.frame().abs() < 0.01);

        let wrong_destination = CutoffMod::from_mod(
            &Mod::LFO(LFO {
                shape: LfoShape::TRI,
                dest: 8,
                trigger_mode: LfoTriggerMode::RETRIG,
                freq: 0x80,
                amount: 0xff,
                retrigger: 0,
            }),
            Some(7),
            2205.0,
        );
        assert!(!wrong_destination.is_active());
    }

    #[test]
    fn sampler_cutoff_uses_destination_six_not_resonance_seven() {
        let cutoff = Mod::AHDEnv(AHDEnv {
            dest: 6,
            amount: 0xbf,
            attack: 0,
            hold: 0,
            decay: 0x4f,
        });
        let resonance = Mod::AHDEnv(AHDEnv {
            dest: 7,
            amount: 0xbf,
            attack: 0,
            hold: 0,
            decay: 0x4f,
        });

        assert!(CutoffMod::from_mod(&cutoff, Some(6), 100.0).is_active());
        assert!(!CutoffMod::from_mod(&resonance, Some(6), 100.0).is_active());
    }

    #[test]
    fn ahd_to_fm_mod_one_decays_without_touching_other_mod_bytes() {
        let mut source = FmLevelMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 3,
                amount: 255,
                attack: 0,
                hold: 1,
                decay: 14,
            }),
            [Some(3), Some(4), Some(5), Some(6)],
            100.0,
        );
        let mut values = [0.0, 64.0, 128.0, 255.0];
        source.frame(&mut values);
        assert_eq!(values, [255.0, 64.0, 128.0, 255.0]);
        for _ in 0..1_500 {
            values[0] = 0.0;
            source.frame(&mut values);
        }
        assert!(values[0] < 1.0);
        assert_eq!(&values[1..], &[64.0, 128.0, 255.0]);
    }

    #[test]
    fn ramp_down_lfo_feeds_the_assigned_fm_mod_bus() {
        let mut source = FmLevelMod::from_mod(
            &Mod::LFO(LFO {
                shape: LfoShape::RAMP_DOWN,
                dest: 3,
                trigger_mode: LfoTriggerMode::RETRIG,
                freq: 1,
                amount: 255,
                retrigger: 255,
            }),
            [Some(3), Some(4), Some(5), Some(6)],
            100.0,
        );
        let mut bus = [0.0; 4];
        source.frame(&mut bus);
        assert!(bus[0] < 2.0);
        assert_eq!(&bus[1..], &[0.0; 3]);
        for _ in 0..300 {
            bus = [0.0; 4];
            source.frame(&mut bus);
        }
        assert!((bus[0] - 127.0).abs() < 3.0);
    }

    #[test]
    fn ahd_uses_tempo_ticks_for_attack_hold_and_decay() {
        let mut envelope = AhdVolume::new(255, 8, 8, 32, 100.0);
        assert_eq!(envelope.frame(), 0.0);
        for _ in 0..399 {
            envelope.frame();
        }
        assert!((envelope.frame() - 0.5).abs() < 0.01);
        for _ in 0..800 {
            envelope.frame();
        }
        assert!((envelope.frame() - 1.0).abs() < 0.01);
        for _ in 0..400 {
            envelope.frame();
        }
        assert!((envelope.frame() - 1.0).abs() < 0.01);
        for _ in 0..3200 {
            envelope.frame();
        }
        assert!(envelope.frame() < 0.002);
    }

    #[test]
    fn ahd_pitch_starts_at_its_amount_and_decays_to_unison() {
        let mut pitch = PitchMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 2,
                amount: 0xff,
                attack: 0,
                hold: 0,
                decay: 0x80,
            }),
            Some(2),
            48.0,
            false,
            918.75,
            44_100,
        );
        assert!((pitch.frame() - 48.0).abs() < 0.01);
        for _ in 0..114_000 {
            pitch.frame();
        }
        assert!(pitch.frame() < 1.0);
    }

    #[test]
    fn sampler_pitch_mod_uses_measured_ratio_range() {
        let mut ahd = PitchMod::from_mod(
            &Mod::AHDEnv(AHDEnv {
                dest: 2,
                amount: 0xff,
                attack: 0,
                hold: 0,
                decay: 0x80,
            }),
            Some(2),
            48.0,
            true,
            918.75,
            44_100,
        );
        let ahd_ratio = 2.0_f32.powf(ahd.frame() / 12.0);
        assert!((ahd_ratio - 1.946).abs() < 0.001);

        let mut lfo = PitchMod::from_mod(
            &Mod::LFO(LFO {
                shape: LfoShape::TRI,
                dest: 2,
                trigger_mode: LfoTriggerMode::RETRIG,
                freq: 0x20,
                amount: 0x80,
                retrigger: 0,
            }),
            Some(2),
            48.0,
            true,
            918.75,
            44_100,
        );
        let initial_ratio = 2.0_f32.powf(lfo.frame() / 12.0);
        assert!((initial_ratio - 0.508).abs() < 0.01);
    }

    #[test]
    fn adsr_releases_from_current_level_after_note_off() {
        let mut envelope = AdsrVolume::new(255, 2, 2, 128, 4, 10_000);
        let mut attack_end = 0.0;
        for _ in 0..20 {
            attack_end = envelope.frame();
        }
        assert!((attack_end - 1.0).abs() < 0.01);
        for _ in 0..20 {
            envelope.frame();
        }
        let sustain = envelope.frame();
        assert!((sustain - 128.0 / 255.0).abs() < 0.01);
        envelope.release();
        let first_release = envelope.frame();
        assert!(first_release < sustain && first_release > 0.0);
        for _ in 0..500 {
            envelope.frame();
        }
        assert!(envelope.is_done());
        assert_eq!(envelope.frame(), 0.0);
    }

    #[test]
    fn adsr_pitch_routes_envelope_level_to_semitones() {
        let mut pitch = PitchMod::from_mod(
            &Mod::ADSREnv(ADSREnv {
                dest: 2,
                amount: 0xff,
                attack: 0,
                decay: 0,
                sustain: 0x80,
                release: 0x40,
            }),
            Some(2),
            48.0,
            false,
            1_000.0,
            44_100,
        );
        let expected = 48.0 * 128.0 / 255.0;
        assert!((pitch.frame() - expected).abs() < 0.01);
        assert!((pitch.frame() - expected).abs() < 0.01);
    }

    #[test]
    fn retriggered_lfo_uses_step_duration_and_stays_bounded() {
        let mut lfo = LfoVolume::new(LfoShape::SIN, LfoTriggerMode::RETRIG, 128, 0x20, 0, 100.0);
        let first = lfo.frame();
        for _ in 0..(0x20 * 6 * 100 - 1) {
            assert!((0.0..=1.0).contains(&lfo.frame()));
        }
        assert!((lfo.frame() - first).abs() < 1e-5);
    }

    #[test]
    fn free_lfo_phase_tracks_transport_time_instead_of_retrigger() {
        let mut lfo = LfoVolume::new(LfoShape::TRI, LfoTriggerMode::FREE, 255, 0x20, 0x73, 100.0);
        lfo.sync_free_phase(19_200);
        assert!((lfo.frame_bipolar() - 1.0).abs() < 1e-6);
        assert!((lfo.phase - (0.5 + lfo.increment)).abs() < 1e-6);
    }

    #[test]
    fn exp_down_hold_ignores_retrigger_and_keeps_the_endpoint() {
        let mut lfo = LfoVolume::new(
            LfoShape::EXP_DN,
            LfoTriggerMode::HOLD,
            255,
            0x20,
            0x6d,
            100.0,
        );
        let mut zero_retrigger =
            LfoVolume::new(LfoShape::EXP_DN, LfoTriggerMode::HOLD, 255, 0x20, 0, 100.0);
        assert_eq!(lfo.frame(), 1.0);
        assert_eq!(zero_retrigger.frame(), 1.0);
        for _ in 0..20_000 {
            assert_eq!(lfo.frame(), zero_retrigger.frame());
        }
        assert_eq!(lfo.frame(), 0.0);
        assert_eq!(lfo.frame(), 0.0);
        assert!(lfo.is_done());
    }
}
