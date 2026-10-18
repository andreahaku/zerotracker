//! Per-instrument three-band EQ. The file stores shelf frequencies in hertz
//! and signed gains in hundredths of a decibel.

use m8fmt::{EqBand, EqMode, EqType, Equ};
use std::f64::consts::PI;

#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    state: [[f32; 2]; 2],
}

impl Biquad {
    const fn bypass() -> Self {
        Self {
            b: [1.0, 0.0, 0.0],
            a: [0.0, 0.0],
            state: [[0.0; 2]; 2],
        }
    }

    fn shelf(band: &EqBand, sample_rate: u32) -> Self {
        let frequency = band.frequency() as f64;
        let gain = band.gain();
        let q = eq_q(band.q);
        match band.mode.eq_type() {
            EqType::LowShelf => {
                Self::shelf_at(frequency, gain, false, eq_shelf_slope(band.q), sample_rate)
            }
            EqType::HiShelf => {
                Self::shelf_at(frequency, gain, true, eq_shelf_slope(band.q), sample_rate)
            }
            EqType::Bell => Self::peaking(frequency, gain, q, sample_rate),
            // LOWCUT removes low frequencies (a high-pass response); HI.CUT
            // removes high frequencies (a low-pass response).
            EqType::LowCut => Self::cut(frequency, eq_cut_q(band.q), sample_rate, true),
            EqType::HiCut => Self::cut(frequency, eq_cut_q(band.q), sample_rate, false),
            EqType::BandPass => {
                let bp_q = if band.q <= 0x0a {
                    0.01
                } else if band.q <= 0x32 {
                    q * 0.8
                } else {
                    q
                };
                Self::band_pass(frequency, gain, bp_q, sample_rate)
            }
            EqType::AllPass => Self::all_pass(frequency, q, sample_rate),
        }
    }

    fn valid_frequency(frequency: f64, sample_rate: u32) -> bool {
        frequency > 0.0 && frequency < f64::from(sample_rate) * 0.5
    }

    fn shelf_at(frequency: f64, gain: f64, high: bool, q: f64, sample_rate: u32) -> Self {
        if gain == 0.0 {
            return Self::bypass();
        }
        if !Self::valid_frequency(frequency, sample_rate) {
            return Self::bypass();
        }
        // RBJ audio EQ cookbook. The M8 Q control maps to shelf slope empirically.
        let amplitude = 10.0_f64.powf(gain / 40.0);
        let angle = 2.0 * PI * frequency / f64::from(sample_rate);
        let cosine = angle.cos();
        let alpha =
            angle.sin() / 2.0 * ((amplitude + amplitude.recip()) * (q.recip() - 1.0) + 2.0).sqrt();
        let root = 2.0 * amplitude.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = if high {
            (
                amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cosine + root),
                -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * cosine),
                amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cosine - root),
                (amplitude + 1.0) - (amplitude - 1.0) * cosine + root,
                2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * cosine),
                (amplitude + 1.0) - (amplitude - 1.0) * cosine - root,
            )
        } else {
            (
                amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cosine + root),
                2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cosine),
                amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cosine - root),
                (amplitude + 1.0) + (amplitude - 1.0) * cosine + root,
                -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cosine),
                (amplitude + 1.0) + (amplitude - 1.0) * cosine - root,
            )
        };
        Self {
            b: [(b0 / a0) as f32, (b1 / a0) as f32, (b2 / a0) as f32],
            a: [(a1 / a0) as f32, (a2 / a0) as f32],
            state: [[0.0; 2]; 2],
        }
    }

    fn peaking(frequency: f64, gain: f64, q: f64, sample_rate: u32) -> Self {
        if gain == 0.0 || !Self::valid_frequency(frequency, sample_rate) {
            return Self::bypass();
        }
        let a = 10.0_f64.powf(gain / 40.0);
        let w = 2.0 * PI * frequency / f64::from(sample_rate);
        let alpha = w.sin() / (2.0 * q);
        let c = w.cos();
        Self::normalized(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    fn cut(frequency: f64, q: f64, sample_rate: u32, high: bool) -> Self {
        if !Self::valid_frequency(frequency, sample_rate) {
            return Self::bypass();
        }
        let w = 2.0 * PI * frequency / f64::from(sample_rate);
        let alpha = w.sin() / (2.0 * q);
        let c = w.cos();
        if high {
            Self::normalized(
                (1.0 + c) / 2.0,
                -(1.0 + c),
                (1.0 + c) / 2.0,
                1.0 + alpha,
                -2.0 * c,
                1.0 - alpha,
            )
        } else {
            Self::normalized(
                (1.0 - c) / 2.0,
                1.0 - c,
                (1.0 - c) / 2.0,
                1.0 + alpha,
                -2.0 * c,
                1.0 - alpha,
            )
        }
    }

    fn band_pass(frequency: f64, gain: f64, q: f64, sample_rate: u32) -> Self {
        if !Self::valid_frequency(frequency, sample_rate) {
            return Self::bypass();
        }
        let w = 2.0 * PI * frequency / f64::from(sample_rate);
        let alpha = w.sin() / (2.0 * q);
        let c = w.cos();
        let makeup = 10.0_f64.powf(gain / 20.0);
        let mut filter = Self::normalized(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
        for coefficient in &mut filter.b {
            *coefficient *= makeup as f32;
        }
        filter
    }

    fn all_pass(frequency: f64, q: f64, sample_rate: u32) -> Self {
        if !Self::valid_frequency(frequency, sample_rate) {
            return Self::bypass();
        }
        let w = 2.0 * PI * frequency / f64::from(sample_rate);
        let alpha = w.sin() / (2.0 * q);
        let c = w.cos();
        Self::normalized(
            1.0 - alpha,
            -2.0 * c,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b: [(b0 / a0) as f32, (b1 / a0) as f32, (b2 / a0) as f32],
            a: [(a1 / a0) as f32, (a2 / a0) as f32],
            state: [[0.0; 2]; 2],
        }
    }

    fn frame(&mut self, channel: usize, input: f32) -> f32 {
        let [z1, z2] = self.state[channel];
        let output = self.b[0].mul_add(input, z1);
        self.state[channel] = [
            self.b[1].mul_add(input, z2) - self.a[0] * output,
            self.b[2] * input - self.a[1] * output,
        ];
        output
    }

    fn response_db(&self, frequency: f64, sample_rate: u32) -> f64 {
        if !frequency.is_finite() || frequency < 0.0 || frequency >= f64::from(sample_rate) * 0.5 {
            return 0.0;
        }
        let omega = 2.0 * PI * frequency / f64::from(sample_rate);
        let (sin_1, cos_1) = omega.sin_cos();
        let (sin_2, cos_2) = (2.0 * omega).sin_cos();
        let numerator = complex_response(self.b, cos_1, sin_1, cos_2, sin_2);
        let denominator = complex_response([1.0, self.a[0], self.a[1]], cos_1, sin_1, cos_2, sin_2);
        let magnitude = numerator.0.hypot(numerator.1) / denominator.0.hypot(denominator.1);
        if magnitude.is_finite() && magnitude > 0.0 {
            20.0 * magnitude.log10()
        } else {
            0.0
        }
    }
}

fn complex_response(
    coefficients: [f32; 3],
    cos_1: f64,
    sin_1: f64,
    cos_2: f64,
    sin_2: f64,
) -> (f64, f64) {
    (
        f64::from(coefficients[0])
            + f64::from(coefficients[1]) * cos_1
            + f64::from(coefficients[2]) * cos_2,
        -f64::from(coefficients[1]) * sin_1 - f64::from(coefficients[2]) * sin_2,
    )
}

/// Return the magnitude response of one M8 EQ band in dB at 44.1 kHz.
///
/// This uses the same coefficient design and Q mappings as the instrument
/// renderer. Invalid or out-of-range frequencies resolve to the bypass level.
pub fn band_response_db(band: &EqBand, freq_hz: f64) -> f64 {
    Biquad::shelf(band, 44_100).response_db(freq_hz, 44_100)
}

// Q is logarithmic on the M8 control. Anchors are fit to the measured broad,
// default, and resonant responses.
fn eq_q(value: u8) -> f64 {
    let (x0, y0, x1, y1): (f64, f64, f64, f64) = if value <= 0x32 {
        (10.0, 0.02, 50.0, 0.35)
    } else {
        (50.0, 0.35, 99.0, 10.0)
    };
    let t = ((f64::from(value) - x0) / (x1 - x0)).clamp(0.0, 1.0);
    y0 * (y1 / y0).powf(t)
}

fn eq_cut_q(value: u8) -> f64 {
    let (x0, y0, x1, y1): (f64, f64, f64, f64) = if value <= 0x32 {
        (10.0, 0.05, 50.0, 0.707)
    } else {
        (50.0, 0.707, 99.0, 20.0)
    };
    let t = ((f64::from(value) - x0) / (x1 - x0)).clamp(0.0, 1.0);
    y0 * (y1 / y0).powf(t)
}

fn eq_shelf_slope(value: u8) -> f64 {
    let (x0, y0, x1, y1): (f64, f64, f64, f64) = if value <= 0x32 {
        (10.0, 0.10, 50.0, 0.625)
    } else {
        (50.0, 0.625, 99.0, 1.5)
    };
    let t = ((f64::from(value) - x0) / (x1 - x0)).clamp(0.0, 1.0);
    y0 * (y1 / y0).powf(t)
}

/// Broad corrective shelves and a fixed mid-band gain for measured effect returns.
pub(crate) struct EffectTransferEq {
    low: Biquad,
    high: Biquad,
    mid_gain: f32,
    output_gain: f32,
}

impl EffectTransferEq {
    pub(crate) fn new(
        low_gain_db: f64,
        mid_gain_db: f64,
        high_gain_db: f64,
        output_trim_db: f64,
        sample_rate: u32,
    ) -> Self {
        Self {
            low: Biquad::shelf_at(160.0, low_gain_db - mid_gain_db, false, 0.625, sample_rate),
            high: Biquad::shelf_at(
                10_000.0,
                high_gain_db - mid_gain_db,
                true,
                0.625,
                sample_rate,
            ),
            mid_gain: 10.0_f32.powf((mid_gain_db / 20.0) as f32),
            output_gain: 10.0_f32.powf((output_trim_db / 20.0) as f32),
        }
    }

    pub(crate) fn frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mut output = [left, right];
        for filter in [&mut self.low, &mut self.high] {
            for (channel, value) in output.iter_mut().enumerate() {
                *value = filter.frame(channel, *value);
            }
        }
        (
            output[0] * self.mid_gain * self.output_gain,
            output[1] * self.mid_gain * self.output_gain,
        )
    }
}

pub(crate) struct StereoEq {
    bands: [Biquad; 3],
    modes: [EqMode; 3],
    active: bool,
}

impl StereoEq {
    pub(crate) const fn off() -> Self {
        Self {
            bands: [Biquad::bypass(); 3],
            modes: [EqMode::Stereo; 3],
            active: false,
        }
    }

    pub(crate) fn from_params(eq: &Equ, sample_rate: u32) -> Self {
        let bands = [
            Biquad::shelf(&eq.low, sample_rate),
            Biquad::shelf(&eq.mid, sample_rate),
            Biquad::shelf(&eq.high, sample_rate),
        ];
        let modes = [
            eq.low.mode.eq_mode(),
            eq.mid.mode.eq_mode(),
            eq.high.mode.eq_mode(),
        ];
        let active = bands.iter().any(|band| band.b != [1.0, 0.0, 0.0]);
        Self {
            bands,
            modes,
            active,
        }
    }

    pub(crate) fn frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.active {
            return (left, right);
        }
        let mut output = [left, right];
        for (band, mode) in self.bands.iter_mut().zip(self.modes) {
            match mode {
                EqMode::Stereo | EqMode::Mid => {
                    for (channel, value) in output.iter_mut().enumerate() {
                        *value = band.frame(channel, *value);
                    }
                }
                EqMode::Side => {
                    let mid = (output[0] + output[1]) * 0.5;
                    let side = band.frame(0, (output[0] - output[1]) * 0.5);
                    output = [mid + side, mid - side];
                }
                EqMode::Left => output[0] = band.frame(0, output[0]),
                EqMode::Right => output[1] = band.frame(1, output[1]),
            }
        }
        (output[0], output[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_reach_saved_dc_and_high_frequency_gains() {
        let mut eq = Equ {
            low: EqBand::default_low(),
            mid: EqBand::default_mid(),
            high: EqBand::default_high(),
        };
        eq.low.level = 0xfc;
        eq.low.level_fin = 0x18; // -10.00 dB
        eq.low.freq = 0;
        eq.low.freq_fin = 72;
        eq.high.level = 0xfe;
        eq.high.level_fin = 0x70; // -4.00 dB
        let mut filter = StereoEq::from_params(&eq, 44_100);
        let mut value = 0.0;
        for _ in 0..44_100 {
            value = filter.frame(1.0, 1.0).0;
        }
        assert!((value - 10.0_f32.powf(-10.0 / 20.0)).abs() < 0.01);
    }

    #[test]
    fn public_band_response_matches_the_rendered_sine() {
        let mut band = EqBand::default_mid();
        band.level = 0x04;
        band.level_fin = 0xb0;
        band.q = 0x32;
        assert!((band_response_db(&band, 1_000.0) - 12.0).abs() < 0.001);

        let expected = band_response_db(&band, 1_000.0);
        let mut filter = Biquad::shelf(&band, 44_100);
        let mut input_energy = 0.0;
        let mut output_energy = 0.0;
        let mut phase = 0.0_f64;
        let increment = 2.0 * PI * 1_000.0 / 44_100.0;
        for frame in 0..88_200 {
            let input = phase.sin() as f32;
            let output = filter.frame(0, input);
            if frame >= 44_100 {
                input_energy += f64::from(input).powi(2);
                output_energy += f64::from(output).powi(2);
            }
            phase = (phase + increment).rem_euclid(2.0 * PI);
        }
        let measured = 10.0 * (output_energy / input_energy).log10();
        assert!(
            (measured - expected).abs() < 0.01,
            "{measured} dB vs {expected} dB"
        );
    }

    #[test]
    fn each_band_type_builds_its_expected_response() {
        let mut eq = Equ {
            low: EqBand::default_low(),
            mid: EqBand::default_mid(),
            high: EqBand::default_high(),
        };
        eq.mid.mode = m8fmt::EqModeType::new(EqType::LowCut, EqMode::Stereo);
        let mut low_cut = StereoEq::from_params(&eq, 44_100);
        let mut dc = 1.0;
        for _ in 0..44_100 {
            dc = low_cut.frame(dc, dc).0;
        }
        assert!(dc.abs() < 0.01, "LOWCUT should remove DC, got {dc}");

        eq.mid.mode = m8fmt::EqModeType::new(EqType::HiCut, EqMode::Stereo);
        let mut high_cut = StereoEq::from_params(&eq, 44_100);
        let mut dc = 0.0;
        for _ in 0..44_100 {
            dc = high_cut.frame(1.0, 1.0).0;
        }
        assert!(
            (dc - 1.0).abs() < 0.01,
            "HI.CUT should preserve DC, got {dc}"
        );

        for ty in [EqType::Bell, EqType::BandPass, EqType::AllPass] {
            eq.mid.mode = m8fmt::EqModeType::new(ty, EqMode::Stereo);
            eq.mid.level = 0x04;
            eq.mid.level_fin = 0xb0;
            let filter = StereoEq::from_params(&eq, 44_100);
            assert!(filter.active, "{ty:?} should be active");
        }
    }

    #[test]
    fn eq_mid_and_side_modes_handle_stereo_pairs() {
        let mut eq = Equ {
            low: EqBand::default_low(),
            mid: EqBand::default_mid(),
            high: EqBand::default_high(),
        };
        eq.mid.mode = m8fmt::EqModeType::new(EqType::Bell, EqMode::Side);
        eq.mid.level = 0x04;
        eq.mid.level_fin = 0xb0;
        let mut side = StereoEq::from_params(&eq, 44_100);
        let (l, r) = side.frame(0.25, 0.25);
        assert!((l - 0.25).abs() < 1e-6 && (r - 0.25).abs() < 1e-6);
        let (l, r) = side.frame(0.25, -0.25);
        assert!((l + r).abs() < 1e-6);
    }
}
