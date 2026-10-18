//! Per-voice filter topologies calibrated from isolated black-box captures.

use m8fmt::SynthParams;
use std::f64::consts::PI;

#[derive(Clone, Copy)]
pub(crate) enum VoiceFilter {
    Off,
    StateVariable(TptStereo),
    LowHigh(TptStereo, OnePoleHighPassStereo),
    Tpt(TptStereo),
    OnePoleHighPass(OnePoleHighPassStereo),
}

impl VoiceFilter {
    pub(crate) fn from_params(params: &SynthParams, sample_rate: u32) -> Self {
        Self::from_values(
            params.filter_type,
            params.filter_cutoff,
            params.filter_res,
            sample_rate,
        )
    }

    fn from_values(filter_type: u8, filter_cutoff: u8, filter_res: u8, sample_rate: u32) -> Self {
        match filter_type {
            1..=4 => Self::StateVariable(TptStereo::new(
                if filter_type == 4 && filter_res >= 0xf0 {
                    5
                } else {
                    filter_type
                },
                if filter_type == 1 {
                    legacy_lowpass_cutoff(filter_cutoff, filter_res, sample_rate)
                } else if filter_type == 2 {
                    highpass_cutoff(filter_cutoff, filter_res, sample_rate)
                } else {
                    measured_cutoff(filter_cutoff, sample_rate)
                },
                if filter_type == 1 {
                    legacy_lowpass_q(filter_cutoff, filter_res)
                } else if filter_type == 2 {
                    highpass_q(filter_cutoff, filter_res)
                } else {
                    measured_q(filter_res)
                },
                sample_rate,
            )),
            5 if filter_res == 0 => Self::StateVariable(TptStereo::new(
                1,
                measured_cutoff(filter_cutoff, sample_rate),
                measured_q(0),
                sample_rate,
            )),
            5 => Self::LowHigh(
                TptStereo::new(
                    1,
                    measured_cutoff(filter_cutoff, sample_rate),
                    measured_q(0),
                    sample_rate,
                ),
                OnePoleHighPassStereo::new(highpass_corner(filter_res, sample_rate), sample_rate),
            ),
            7 if filter_res == 0 => Self::OnePoleHighPass(OnePoleHighPassStereo::new(
                zdf_highpass_cutoff(filter_cutoff, sample_rate),
                sample_rate,
            )),
            6 | 7 => Self::Tpt(TptStereo::new(
                filter_type,
                if filter_type == 7 {
                    zdf_highpass_cutoff(filter_cutoff, sample_rate)
                } else {
                    zdf_cutoff(filter_cutoff, sample_rate)
                },
                zdf_q(filter_res),
                sample_rate,
            )),
            _ => Self::Off,
        }
    }

    pub(crate) fn retune(&mut self, filter_type: u8, cutoff: u8, res: u8, sample_rate: u32) {
        let next = Self::from_values(filter_type, cutoff, res, sample_rate);
        match (self, next) {
            (Self::StateVariable(current), Self::StateVariable(next)) => current.retune(next),
            (Self::LowHigh(current_lp, current_hp), Self::LowHigh(next_lp, next_hp)) => {
                current_lp.retune(next_lp);
                current_hp.retune(next_hp);
            }
            (Self::Tpt(current), Self::Tpt(next)) => current.retune(next),
            (Self::OnePoleHighPass(current), Self::OnePoleHighPass(next)) => current.retune(next),
            (current, next) => *current = next,
        }
    }

    pub(crate) fn frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        let output = match self {
            Self::Off => (left, right),
            Self::StateVariable(filter) => filter.frame(left, right),
            Self::LowHigh(low, high) => {
                let (left, right) = low.frame(left, right);
                high.frame(left, right)
            }
            Self::Tpt(filter) => filter.frame(left, right),
            Self::OnePoleHighPass(filter) => filter.frame(left, right),
        };
        if output.0.is_finite()
            && output.1.is_finite()
            && output.0.abs() <= 16.0
            && output.1.abs() <= 16.0
        {
            output
        } else {
            self.reset();
            (0.0, 0.0)
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Off => {}
            Self::StateVariable(filter) | Self::Tpt(filter) => filter.reset(),
            Self::LowHigh(low, high) => {
                low.reset();
                high.reset();
            }
            Self::OnePoleHighPass(filter) => filter.reset(),
        }
    }
}

fn legacy_lowpass_cutoff(value: u8, resonance: u8, sample_rate: u32) -> f64 {
    let legacy = 14.25 * (0.027_688 * f64::from(value)).exp();
    if resonance <= 0x80 {
        return legacy.min(f64::from(sample_rate) * 0.45);
    }

    // At high resonance the measured low-pass peak follows a different cutoff
    // curve. Blend that curve by RES so the legacy RES 00 response is unchanged.
    const HIGH_RESONANCE_CUTOFF: [(u8, f64); 7] = [
        (0x20, 32.3),
        (0x40, 78.1),
        (0x60, 188.4),
        (0x80, 446.8),
        (0xa0, 1_068.6),
        (0xc0, 2_546.3),
        (0xe0, 6_112.7),
    ];
    let measured = interpolated_log_anchor(value, &HIGH_RESONANCE_CUTOFF);
    let amount = f64::from(resonance - 0x80) / f64::from(u8::MAX - 0x80);
    (legacy * (measured / legacy).powf(amount)).min(f64::from(sample_rate) * 0.45)
}

fn highpass_cutoff(value: u8, resonance: u8, sample_rate: u32) -> f64 {
    let legacy = measured_cutoff(value, sample_rate);
    if resonance <= 0x80 {
        return legacy;
    }

    // At high resonance the HP peak follows the measured peak frequencies.
    // Keep the legacy cutoff through RES 80 and preserve its slope above C0.
    const HIGH_RESONANCE_CUTOFF: [(u8, f64); 8] = [
        (0x10, 22.0),
        (0x20, 32.0),
        (0x2a, 46.0),
        (0x40, 78.0),
        (0x60, 188.0),
        (0x80, 450.0),
        (0xa0, 1_066.0),
        (0xc0, 2_552.0),
    ];
    let measured = if value <= 0xc0 {
        interpolated_log_anchor(value, &HIGH_RESONANCE_CUTOFF)
    } else {
        2_552.0 * legacy / measured_cutoff(0xc0, sample_rate)
    };
    let amount = f64::from(resonance - 0x80) / f64::from(u8::MAX - 0x80);
    (legacy * (measured / legacy).powf(amount)).min(f64::from(sample_rate) * 0.45)
}

fn legacy_lowpass_q(cutoff: u8, resonance: u8) -> f64 {
    let base_q = match cutoff {
        0..=0x20 => 0.56,
        0x21..=0x40 => interpolate_q(cutoff, 0x20, 0.56, 0x40, 0.60),
        0x41..=0x60 => interpolate_q(cutoff, 0x40, 0.60, 0x60, 0.79),
        0x61..=0x80 => interpolate_q(cutoff, 0x60, 0.79, 0x80, 0.82),
        0x81..=0xa0 => interpolate_q(cutoff, 0x80, 0.82, 0xa0, 0.85),
        0xa1..=0xc0 => interpolate_q(cutoff, 0xa0, 0.85, 0xc0, 0.87),
        _ => 0.87,
    };
    let legacy_q = base_q + 0.2 * (2.0_f64.powf(f64::from(resonance) / 64.0) - 1.0);
    if resonance <= 0x80 {
        return legacy_q;
    }

    // Preserve the legacy response through RES 80. The intermediate
    // resonance rise is delayed until near RES FF to match the measured peaks.
    const HIGH_RESONANCE_Q: [(u8, f64); 7] = [
        (0x20, 6.04),
        (0x40, 11.62),
        (0x60, 21.40),
        (0x80, 32.57),
        (0xa0, 46.50),
        (0xc0, 67.31),
        (0xe0, 96.07),
    ];
    let high_q = interpolated_log_anchor(cutoff, &HIGH_RESONANCE_Q);
    let resonance_exponent = match cutoff {
        0..=0x30 => 1.0,
        0x31..=0x60 => interpolate_q(cutoff, 0x30, 1.0, 0x60, 1.83),
        0x61..=0xa0 => interpolate_q(cutoff, 0x60, 1.83, 0xa0, 2.76),
        _ => 2.76,
    };
    let amount = (f64::from(resonance - 0x80) / f64::from(0xff - 0x80)).powf(resonance_exponent);
    legacy_q * (high_q / legacy_q).powf(amount)
}

fn highpass_q(cutoff: u8, resonance: u8) -> f64 {
    let legacy_q = measured_q(resonance);
    if resonance <= 0x80 {
        return legacy_q;
    }

    // High-pass resonance rises with cutoff above RES 80. The nonlinear blend
    // preserves the measured endpoints and delays most of the rise until the
    // upper resonance range.
    const HIGH_RESONANCE_Q: [(u8, f64); 8] = [
        (0x10, 8.8),
        (0x20, 12.5),
        (0x2a, 14.1),
        (0x40, 18.2),
        (0x60, 36.2),
        (0x80, 48.5),
        (0xa0, 50.2),
        (0xc0, 71.5),
    ];
    let high_q = interpolated_log_anchor(cutoff, &HIGH_RESONANCE_Q);
    let amount = (f64::from(resonance - 0x80) / f64::from(u8::MAX - 0x80)).powf(5.36);
    legacy_q * (high_q / legacy_q).powf(amount)
}

fn interpolate_q(value: u8, left: u8, left_q: f64, right: u8, right_q: f64) -> f64 {
    let amount = f64::from(value - left) / f64::from(right - left);
    left_q + (right_q - left_q) * amount
}

fn measured_cutoff(value: u8, sample_rate: u32) -> f64 {
    const ANCHORS: [(u8, f64); 8] = [
        (0x00, 20.0),
        (0x20, 35.0),
        (0x40, 82.0),
        (0x60, 212.0),
        (0x80, 484.0),
        (0xa0, 1_155.0),
        (0xc0, 2_882.0),
        (0xe0, 7_236.0),
    ];
    interpolated_log_anchor(value, &ANCHORS).min(f64::from(sample_rate) * 0.2)
}

fn zdf_cutoff(value: u8, sample_rate: u32) -> f64 {
    const ANCHORS: [(u8, f64); 4] = [
        (0x00, 20.0),
        (0x40, 220.0),
        (0x80, 1_100.0),
        (0xc0, 7_000.0),
    ];
    interpolated_log_anchor(value, &ANCHORS).min(f64::from(sample_rate) * 0.2)
}

fn zdf_highpass_cutoff(value: u8, sample_rate: u32) -> f64 {
    const ANCHORS: [(u8, f64); 4] = [(0x00, 20.0), (0x40, 70.0), (0x80, 500.0), (0xc0, 4_000.0)];
    interpolated_log_anchor(value, &ANCHORS).min(f64::from(sample_rate) * 0.2)
}

fn highpass_corner(value: u8, sample_rate: u32) -> f64 {
    if value == 0 {
        return 20.0;
    }
    (500.0 * 2.0_f64.powf((f64::from(value) - 160.0) / 48.0))
        .clamp(20.0, f64::from(sample_rate) * 0.2)
}

fn interpolated_log_anchor(value: u8, anchors: &[(u8, f64)]) -> f64 {
    let upper = anchors.partition_point(|(byte, _)| *byte < value);
    if upper == 0 {
        return anchors[0].1;
    }
    if upper == anchors.len() {
        let (lo_byte, lo_hz) = anchors[anchors.len() - 1];
        let (prev_byte, prev_hz) = anchors[anchors.len() - 2];
        let slope = (lo_hz / prev_hz).ln() / f64::from(lo_byte - prev_byte);
        return lo_hz * (slope * f64::from(value - lo_byte)).exp();
    }
    let (lo_byte, lo_hz) = anchors[upper - 1];
    let (hi_byte, hi_hz) = anchors[upper];
    let fraction = f64::from(value - lo_byte) / f64::from(hi_byte - lo_byte);
    (lo_hz.ln() + (hi_hz.ln() - lo_hz.ln()) * fraction).exp()
}

fn measured_q(value: u8) -> f64 {
    const ANCHORS: [(u8, f64); 5] = [
        (0x00, 0.707),
        (0x80, 2.2),
        (0xc0, 3.8),
        (0xe0, 6.3),
        (0xff, 17.0),
    ];
    interpolated_log_anchor(value, &ANCHORS)
}

fn zdf_q(resonance: u8) -> f64 {
    // The observed ZDF LP/HP resonance is substantially gentler than the
    // low-pass family, including at the top of the measured resonance range.
    let q = measured_q(resonance);
    0.707 + (q - 0.707) * 0.2
}

#[derive(Clone, Copy)]
pub(crate) struct OnePoleHighPassStereo {
    alpha: f32,
    left_input: f32,
    right_input: f32,
    left_output: f32,
    right_output: f32,
}

impl OnePoleHighPassStereo {
    fn new(cutoff: f64, sample_rate: u32) -> Self {
        let alpha = (1.0 - (-2.0 * PI * cutoff / f64::from(sample_rate)).exp()) as f32;
        Self {
            alpha,
            left_input: 0.0,
            right_input: 0.0,
            left_output: 0.0,
            right_output: 0.0,
        }
    }
    fn retune(&mut self, next: Self) {
        self.alpha = next.alpha;
    }
    fn reset(&mut self) {
        self.left_input = 0.0;
        self.right_input = 0.0;
        self.left_output = 0.0;
        self.right_output = 0.0;
    }
    fn frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        (self.channel(left, true), self.channel(right, false))
    }
    fn channel(&mut self, input: f32, is_left: bool) -> f32 {
        let (previous_input, previous_output) = if is_left {
            (&mut self.left_input, &mut self.left_output)
        } else {
            (&mut self.right_input, &mut self.right_output)
        };
        let output = (1.0 - self.alpha) * (*previous_output + input - *previous_input);
        *previous_input = input;
        *previous_output = output;
        output
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TptStereo {
    mode: u8,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    left: [f32; 2],
    right: [f32; 2],
}

impl TptStereo {
    fn new(mode: u8, cutoff: f64, q: f64, sample_rate: u32) -> Self {
        let g = (PI * cutoff / f64::from(sample_rate)).tan() as f32;
        let k = (1.0 / q) as f32;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self {
            mode,
            g,
            k,
            a1,
            a2,
            a3,
            left: [0.0; 2],
            right: [0.0; 2],
        }
    }
    fn retune(&mut self, next: Self) {
        self.mode = next.mode;
        self.g = next.g;
        self.k = next.k;
        self.a1 = next.a1;
        self.a2 = next.a2;
        self.a3 = next.a3;
    }
    fn reset(&mut self) {
        self.left = [0.0; 2];
        self.right = [0.0; 2];
    }
    fn frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        (self.channel(left, true), self.channel(right, false))
    }
    fn channel(&mut self, input: f32, is_left: bool) -> f32 {
        let state = if is_left {
            &mut self.left
        } else {
            &mut self.right
        };
        let v3 = input - state[1];
        let v1 = self.a1 * state[0] + self.a2 * v3;
        let v2 = state[1] + self.a2 * state[0] + self.a3 * v3;
        state[0] = 2.0 * v1 - state[0];
        state[1] = 2.0 * v2 - state[1];
        match self.mode {
            1 | 6 => v2,
            2 | 7 => input - self.k * v1 - v2,
            3 => v1,
            4 => input - self.k * v1,
            5 => input + v1,
            _ => input,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BiquadStereo {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    left: [f32; 2],
    right: [f32; 2],
}

impl BiquadStereo {
    #[cfg(test)]
    fn low_pass(cutoff: f64, q: f64, sample_rate: u32) -> Self {
        let (cosine, alpha, a0) = Self::design_terms(cutoff, q, sample_rate);
        // The reference's low-pass output has approximately 0.94 passband gain
        // at RES=00, measured against the same dry oscillator.
        let gain = 0.94;
        Self::from_coefficients(
            gain * (1.0 - cosine) / 2.0,
            gain * (1.0 - cosine),
            gain * (1.0 - cosine) / 2.0,
            -2.0 * cosine,
            1.0 - alpha,
            a0,
        )
    }

    #[cfg(test)]
    fn band_pass(cutoff: f64, q: f64, sample_rate: u32) -> Self {
        let (cosine, alpha, a0) = Self::design_terms(cutoff, q, sample_rate);
        // The isolated FM noise control needs a calibrated band-pass return
        // level in addition to the resonant response.
        let gain = 3.4;
        Self::from_coefficients(
            gain * alpha,
            0.0,
            -gain * alpha,
            -2.0 * cosine,
            1.0 - alpha,
            a0,
        )
    }

    pub(crate) fn high_pass(cutoff: f64, q: f64, sample_rate: u32) -> Self {
        let (cosine, alpha, a0) = Self::design_terms(cutoff, q, sample_rate);
        Self::from_coefficients(
            (1.0 + cosine) / 2.0,
            -(1.0 + cosine),
            (1.0 + cosine) / 2.0,
            -2.0 * cosine,
            1.0 - alpha,
            a0,
        )
    }

    #[cfg(test)]
    fn band_stop(cutoff: f64, q: f64, sample_rate: u32) -> Self {
        let (cosine, alpha, a0) = Self::design_terms(cutoff, q, sample_rate);
        Self::from_coefficients(1.0, -2.0 * cosine, 1.0, -2.0 * cosine, 1.0 - alpha, a0)
    }

    fn design_terms(cutoff: f64, q: f64, sample_rate: u32) -> (f64, f64, f64) {
        let omega = 2.0 * PI * cutoff / f64::from(sample_rate);
        let cosine = omega.cos();
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        (cosine, alpha, a0)
    }

    fn from_coefficients(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64, a0: f64) -> Self {
        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
            left: [0.0; 2],
            right: [0.0; 2],
        }
    }

    pub(crate) fn frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        let output_left = self.channel(left, true);
        let output_right = self.channel(right, false);
        (output_left, output_right)
    }

    fn channel(&mut self, input: f32, is_left: bool) -> f32 {
        let state = if is_left {
            &mut self.left
        } else {
            &mut self.right
        };
        let output = self.b0 * input + state[0];
        state[0] = self.b1 * input - self.a1 * output + state[1];
        state[1] = self.b2 * input - self.a2 * output;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BiquadStereo, VoiceFilter, highpass_cutoff, highpass_q, legacy_lowpass_cutoff,
        legacy_lowpass_q, measured_cutoff,
    };
    use std::f32::consts::TAU;

    #[test]
    fn lowpass_resonance_preserves_legacy_through_80_and_rises_above_it() {
        let low = legacy_lowpass_q(0xa0, 0x00);
        let medium = legacy_lowpass_q(0xa0, 0x80);
        assert!((low - 0.85).abs() < 1e-9);
        assert!((medium - 1.45).abs() < 1e-9);
        assert!((legacy_lowpass_q(0xa0, 0xff) - 46.50).abs() < 0.01);
        assert!(legacy_lowpass_q(0xa0, 0xbf) < 5.0);
        assert!(legacy_lowpass_q(0x30, 0xbf) > legacy_lowpass_q(0x30, 0x80));
    }

    #[test]
    fn highpass_resonance_is_cutoff_dependent_only_above_res_80() {
        assert_eq!(highpass_q(0xa0, 0x80), highpass_q(0x20, 0x80));
        assert!((highpass_q(0xa0, 0xff) - 50.2).abs() < 0.01);
        assert!(highpass_q(0xa0, 0xff) > highpass_q(0x20, 0xff));
        assert!(highpass_q(0xa0, 0xbf) < 5.0);
    }

    #[test]
    fn highpass_cutoff_preserves_low_resonance_and_fits_high_resonance_peaks() {
        for cutoff in [0x10, 0x20, 0x2a, 0x40, 0x60, 0x80, 0xa0, 0xc0] {
            assert_eq!(
                highpass_cutoff(cutoff, 0x80, 44_100),
                measured_cutoff(cutoff, 44_100)
            );
        }
        assert!((highpass_cutoff(0xa0, 0xff, 44_100) - 1_066.0).abs() < 0.01);
        assert!((highpass_cutoff(0xc0, 0xff, 44_100) - 2_552.0).abs() < 0.01);
    }

    #[test]
    fn lowpass_cutoff_preserves_low_resonance_and_blends_above_80() {
        for cutoff in [0x20, 0x40, 0x60, 0x80, 0xa0, 0xc0, 0xe0] {
            let legacy = (14.25 * (0.027_688 * f64::from(cutoff)).exp()).min(19_845.0);
            assert_eq!(legacy_lowpass_cutoff(cutoff, 0, 44_100), legacy);
            assert_eq!(legacy_lowpass_cutoff(cutoff, 0x80, 44_100), legacy);
        }
        assert!((legacy_lowpass_cutoff(0xa0, 0xff, 44_100) - 1_068.6).abs() < 0.01);
        let at_res_80 = legacy_lowpass_cutoff(0xa0, 0x80, 44_100);
        let legacy_a0 = legacy_lowpass_cutoff(0xa0, 0, 44_100);
        assert_eq!(at_res_80, legacy_a0);
        let above_res_80 = legacy_lowpass_cutoff(0xa0, 0xc0, 44_100);
        assert!(above_res_80 > 1_068.6 && above_res_80 < legacy_a0);
    }

    #[test]
    fn retuning_preserves_filter_history() {
        let mut filter = VoiceFilter::from_values(1, 0, 0, 44_100);
        for _ in 0..1000 {
            filter.frame(0.5, 0.25);
        }
        let VoiceFilter::StateVariable(before) = filter else {
            panic!("low-pass should be active");
        };
        filter.retune(1, 0xbf, 0, 44_100);
        let VoiceFilter::StateVariable(after) = filter else {
            panic!("low-pass should remain active");
        };
        assert_eq!(after.left, before.left);
        assert_eq!(after.right, before.right);
        assert_ne!(after.g, before.g);
    }

    #[test]
    fn all_measured_filter_types_are_enabled() {
        for filter_type in 1..=7 {
            assert!(!matches!(
                VoiceFilter::from_values(filter_type, 0x80, 0x40, 44_100),
                VoiceFilter::Off
            ));
        }
    }

    #[test]
    fn rapid_cutoff_sweeps_remain_finite_and_bounded() {
        for filter_type in 1..=7 {
            let mut filter = VoiceFilter::from_values(filter_type, 0, 0xff, 44_100);
            for frame in 0..20_000 {
                let cutoff = if (frame / 17) % 2 == 0 { 0 } else { 0xff };
                filter.retune(filter_type, cutoff, 0xff, 44_100);
                let phase = frame as f32 * 0.071;
                let (left, right) = filter.frame(phase.sin() * 0.8, phase.cos() * 0.7);
                assert!(left.is_finite() && right.is_finite(), "type {filter_type}");
                assert!(
                    left.abs() <= 16.0 && right.abs() <= 16.0,
                    "type {filter_type}"
                );
            }
        }
    }

    #[test]
    fn filter_types_attenuate_the_expected_bands() {
        fn rms_at(mut filter: BiquadStereo, frequency: f32) -> f32 {
            let mut power = 0.0;
            for frame in 0..44_100 {
                let value = (TAU * frequency * frame as f32 / 44_100.0).sin();
                let (filtered, _) = filter.frame(value, 0.0);
                if frame > 22_050 {
                    power += filtered * filtered;
                }
            }
            (power / 22_049.0).sqrt()
        }
        let low = || BiquadStereo::low_pass(460.0, 0.95, 44_100);
        let high = || BiquadStereo::high_pass(460.0, 0.95, 44_100);
        let stop = || BiquadStereo::band_stop(460.0, 0.95, 44_100);
        assert!(rms_at(low(), 65.0) > 0.6);
        assert!(rms_at(low(), 2_000.0) < 0.05);
        assert!(rms_at(high(), 65.0) < 0.05);
        assert!(rms_at(high(), 2_000.0) > 0.6);
        assert!(rms_at(stop(), 460.0) < 0.05);
        assert!(rms_at(stop(), 2_000.0) > 0.5);
        let band = || BiquadStereo::band_pass(460.0, 1.0, 44_100);
        assert!(rms_at(band(), 460.0) > rms_at(band(), 65.0));
        assert!(rms_at(band(), 460.0) > rms_at(band(), 2_000.0));
    }
}
