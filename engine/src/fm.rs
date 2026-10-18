//! Operator routing described by the format's twelve FM algorithm labels.
//!
//! Operator routing uses measured controls. Unavailable wavetable IDs fall back
//! to generic base waves; no wavetable samples are included.

use crate::wave_fallback::wavetable_fallback;
use m8fmt::FMWave;
use std::f64::consts::TAU;

fn fallback_wavetable_wave(wave: FMWave) -> FMWave {
    let wave_id = u8::from(wave);
    let Some(wav_shape_id) = wave_id
        .checked_sub(0x10)
        .and_then(|index| index.checked_add(0x09))
        .and_then(wavetable_fallback)
    else {
        return wave;
    };
    match wav_shape_id {
        0 => FMWave::PUL,
        4 => FMWave::SAW,
        5 => FMWave::TRI,
        6 => FMWave::SIN,
        7 => FMWave::NOI,
        _ => wave,
    }
}

#[derive(Clone, Copy)]
struct Graph {
    inputs: [u8; 4],
    carriers: u8,
}

const A: u8 = 1;
const B: u8 = 2;
const C: u8 = 4;
const D: u8 = 8;

const GRAPHS: [Graph; 12] = [
    Graph {
        inputs: [0, A, B, C],
        carriers: D,
    }, // A>B>C>D
    Graph {
        inputs: [0, 0, A | B, C],
        carriers: D,
    }, // [A+B]>C>D
    Graph {
        inputs: [0, A, 0, B | C],
        carriers: D,
    }, // [A>B+C]>D
    Graph {
        inputs: [0, A, A, B | C],
        carriers: D,
    }, // [A>B+A>C]>D
    Graph {
        inputs: [0, 0, 0, A | B | C],
        carriers: D,
    }, // [A+B+C]>D
    Graph {
        inputs: [0, A, B, 0],
        carriers: C | D,
    }, // [A>B>C]+D
    Graph {
        inputs: [0, A, B, B],
        carriers: C | D,
    }, // [A>B>C]+[A>B>D]
    Graph {
        inputs: [0, A, 0, C],
        carriers: B | D,
    }, // [A>B]+[C>D]
    Graph {
        inputs: [0, A, A, A],
        carriers: B | C | D,
    }, // [A>B]+[A>C]+[A>D]
    Graph {
        inputs: [0, A, A, 0],
        carriers: B | C | D,
    }, // [A>B]+[A>C]+D
    Graph {
        inputs: [0, A, 0, 0],
        carriers: B | C | D,
    }, // [A>B]+C+D
    Graph {
        inputs: [0, 0, 0, 0],
        carriers: A | B | C | D,
    }, // A+B+C+D
];

pub(crate) fn operator_drive(level: u8) -> f32 {
    // Isolated additive controls establish a linear curve through 80, then
    // approximately doubled drive at A0, C0, and E0. CALFM1 constrains the
    // saturated FF endpoint. Dividing the additive carrier mix by four makes
    // this same drive match the isolated serial C>D control.
    if level <= 0x80 {
        0.986 * f32::from(level) / 128.0
    } else {
        0.986 * (15.2_f32 / 0.986).powf(f32::from(level - 0x80) / 127.0)
    }
}

fn phase_modulator_gain(drive: f32) -> f32 {
    // CALFM2, CALFO2C0, and CALFO2FF isolate a sine operator feeding a
    // level-80 sine carrier. Fitting their output to sin(t + depth*sin(t))
    // gives phase depths 1.011, 4.0426, and 15.7891 radians, versus the
    // carrier-calibrated drives 0.986, 3.913247, and 15.2. Interpolate the
    // ratio so carrier loudness remains calibrated separately.
    if drive <= 0.986 {
        1.02535
    } else if drive <= 3.913_247 {
        1.02535 + 0.007705 * (drive - 0.986) / (3.913_247 - 0.986)
    } else {
        1.033055 + 0.005702 * (drive - 3.913_247) / (15.2 - 3.913_247)
    }
}

pub(crate) fn modulated_drives(
    algo: u8,
    base: &[f32; 4],
    static_values: &[f32; 4],
    live_values: &[f32; 4],
    assignments: &[[u8; 2]; 4],
) -> [f32; 4] {
    let Some(graph) = GRAPHS.get(usize::from(algo)) else {
        return *base;
    };
    let mut drives = *base;
    for (index, operator) in assignments.iter().enumerate() {
        let is_carrier = graph.carriers & (1 << index) != 0;
        for (slot, assignment) in operator.iter().enumerate() {
            if (1..=4).contains(assignment) {
                let bus = usize::from(*assignment - 1);
                let value = (static_values[bus] + live_values[bus]).clamp(0.0, 255.0);
                if is_carrier {
                    // The bus scales a carrier linearly in amplitude. Duplicate
                    // slots are applied once, matching the shared bus control.
                    if slot > 0 && operator[..slot].contains(assignment) {
                        continue;
                    }
                    drives[index] *= value / 255.0;
                } else {
                    // Controlled measurements show a normalized knob-plus-modulator bus
                    // on serial operators. Each routing slot contributes a
                    // factor, including repeated assignments.
                    drives[index] *= value / 255.0;
                }
            }
        }
    }
    drives
}

pub(crate) fn modulated_increments(
    base: &[f64; 4],
    values: &[f32; 4],
    assignments: &[[u8; 2]; 4],
    pitch_headroom: f32,
) -> [f64; 4] {
    let mut increments = *base;
    for (index, operator) in assignments.iter().enumerate() {
        for (slot, assignment) in operator.iter().enumerate() {
            if (5..=8).contains(assignment) {
                // Operator routing codes 5-8 map MOD1-4 to RAT. The isolated
                // CALRAT sweep measures a 0..16 additive ratio offset.
                if slot > 0 && operator[..slot].contains(assignment) {
                    continue;
                }
                let bus = usize::from(*assignment - 5);
                increments[index] *= f64::from(ratio_mod_multiplier(values[bus]));
            } else if (9..=12).contains(assignment) {
                // Operator routing codes 9-12 map MOD1-4 to PIT. The
                // ZTPITMAP reference sweep measures the bus as a semitone
                // offset: 0x08, 0x20, and 0x40 raise pitch by 8, 32, and
                // 64 semitones respectively.
                if slot > 0 && operator[..slot].contains(assignment) {
                    continue;
                }
                let bus = usize::from(*assignment - 9);
                increments[index] *= f64::from(pitch_mod_multiplier(values[bus], pitch_headroom));
            }
        }
    }
    increments
}

fn ratio_mod_multiplier(value: f32) -> f32 {
    1.0 + 16.0 * value.clamp(0.0, 255.0) / 255.0
}

fn pitch_mod_multiplier(value: f32, pitch_headroom: f32) -> f32 {
    2.0_f32.powf(value.clamp(0.0, pitch_headroom.max(0.0)) / 12.0)
}

fn wave_sample(shape: FMWave, phase: f64, phase_increment: f64) -> f32 {
    match shape {
        FMWave::SIN => (TAU * phase).sin() as f32,
        // Measurements show these are mathematical sine variants, not sampled
        // tables: rectified sine waves and gated sine bursts.
        FMWave::SW2 => (TAU * phase).sin().max(0.0) as f32,
        FMWave::SW3 => (TAU * phase).sin().abs() as f32,
        FMWave::SW4 => {
            let subphase = (2.0 * phase).fract();
            if subphase < 0.5 {
                (std::f64::consts::PI * subphase).sin() as f32
            } else {
                0.0
            }
        }
        FMWave::SW5 => {
            if phase < 0.5 {
                (2.0 * TAU * phase).sin() as f32
            } else {
                0.0
            }
        }
        FMWave::SW6 => {
            if phase < 0.5 {
                (2.0 * TAU * phase).sin().abs() as f32
            } else {
                0.0
            }
        }
        FMWave::SQR => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        FMWave::SAW => (2.0 * phase - 1.0) as f32,
        FMWave::PUL => {
            if phase < 0.25 {
                1.0
            } else {
                -1.0
            }
        }
        // IMP is a negative DC level with one positive sample
        // per cycle. Using the phase increment keeps the pulse one sample wide
        // across pitches and sample rates.
        FMWave::IMP => {
            if phase < phase_increment {
                1.0
            } else {
                -1.0
            }
        }
        FMWave::TRI => {
            if phase < 0.25 {
                (4.0 * phase) as f32
            } else if phase < 0.75 {
                (2.0 - 4.0 * phase) as f32
            } else {
                (4.0 * phase - 4.0) as f32
            }
        }
        FMWave::NOI => {
            let mut value = (phase * 64.0).floor() as u32 ^ 0x9e37_79b9;
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            (value as i32 as f32) / i32::MAX as f32
        }
        FMWave::W12 => {
            // CALFW80W isolates a level-80 W12 carrier; CALFW12 checks the
            // clipped level-FF endpoint.
            let angle = TAU * phase;
            (0.831 * angle.sin() + 0.418 * (3.0 * angle - 0.448).sin()) as f32
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
pub(crate) fn frame(
    algo: u8,
    phases: &[f64; 4],
    drives: &[f32; 4],
    shapes: &[FMWave; 4],
    feedback: &[f32; 4],
    previous: &mut [f32; 4],
) -> f32 {
    frame_with_increments(
        algo,
        phases,
        &[1.0 / 65_536.0; 4],
        drives,
        shapes,
        feedback,
        previous,
    )
}

pub(crate) fn frame_with_increments(
    algo: u8,
    phases: &[f64; 4],
    phase_increments: &[f64; 4],
    drives: &[f32; 4],
    shapes: &[FMWave; 4],
    feedback: &[f32; 4],
    previous: &mut [f32; 4],
) -> f32 {
    let Some(graph) = GRAPHS.get(usize::from(algo)) else {
        return 0.0;
    };
    let mut operators = [0.0_f32; 4];
    for index in 0..4 {
        let shape = fallback_wavetable_wave(shapes[index]);
        if !matches!(
            shape,
            FMWave::SIN
                | FMWave::SW2
                | FMWave::SW3
                | FMWave::SW4
                | FMWave::SW5
                | FMWave::SW6
                | FMWave::SQR
                | FMWave::SAW
                | FMWave::TRI
                | FMWave::PUL
                | FMWave::IMP
                | FMWave::NOI
                | FMWave::W12
        ) {
            continue;
        }
        let mut modulation = 0.0_f32;
        for (source, value) in operators.iter().enumerate().take(index) {
            if graph.inputs[index] & (1 << source) != 0 {
                modulation += value * phase_modulator_gain(drives[source]);
            }
        }
        // CALFB20/80/FF fit self-feedback from the prior sample of the same
        // operator. Each voice owns `previous`, so note starts reset it.
        let phase = (phases[index]
            + f64::from(modulation + feedback[index] * previous[index]) / TAU)
            .rem_euclid(1.0);
        let wave = wave_sample(shape, phase, phase_increments[index]);
        operators[index] = drives[index] * wave;
        previous[index] = operators[index];
    }
    let mut output = 0.0;
    for (index, value) in operators.into_iter().enumerate() {
        if graph.carriers & (1 << index) != 0 {
            output += value;
        }
    }
    (output / graph.carriers.count_ones() as f32).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_wave(shape: FMWave, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| wave_sample(shape, index as f64 / samples as f64, 1.0 / samples as f64))
            .collect()
    }

    fn harmonic_amplitude(samples: &[f32], harmonic: usize) -> f32 {
        let len = samples.len() as f64;
        let (sin_sum, cos_sum) =
            samples
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(sin_sum, cos_sum), (index, sample)| {
                    let angle = TAU * harmonic as f64 * index as f64 / len;
                    (
                        sin_sum + f64::from(*sample) * angle.sin(),
                        cos_sum + f64::from(*sample) * angle.cos(),
                    )
                });
        (2.0 * sin_sum.hypot(cos_sum) / len) as f32
    }

    #[test]
    fn measured_basic_fm_shapes_have_expected_rms() {
        let samples = 16_384;
        let sine_rms = 0.5_f32.sqrt();
        for shape in [FMWave::SW2, FMWave::SW4, FMWave::SW5, FMWave::SW6] {
            let rms = (render_wave(shape, samples)
                .iter()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / samples as f32)
                .sqrt();
            assert!(
                (rms / sine_rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.002,
                "{shape:?}: {rms}"
            );
        }
        let sw3_rms = (render_wave(FMWave::SW3, samples)
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>()
            / samples as f32)
            .sqrt();
        assert!((sw3_rms / sine_rms - 1.0).abs() < 0.002);
        for shape in [FMWave::TRI, FMWave::SAW] {
            let rms = (render_wave(shape, samples)
                .iter()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / samples as f32)
                .sqrt();
            assert!(
                (rms / sine_rms - 0.816_496_6).abs() < 0.01,
                "{shape:?}: {rms}"
            );
        }
    }

    #[test]
    fn sw4_and_sw6_harmonics_match_suite_73() {
        let samples = 16_384;
        let sw4 = render_wave(FMWave::SW4, samples);
        let sw4_h2 = harmonic_amplitude(&sw4, 2);
        for (harmonic, expected_db) in [(4, -8.7), (6, -12.6), (8, -15.3)] {
            let actual_db = 20.0 * (harmonic_amplitude(&sw4, harmonic) / sw4_h2).log10();
            assert!(
                (actual_db - expected_db).abs() < 0.2,
                "SW4 H{harmonic}: {actual_db}"
            );
        }

        let sw6 = render_wave(FMWave::SW6, samples);
        let sw6_h1 = harmonic_amplitude(&sw6, 1);
        for (harmonic, expected_db) in [(3, -4.4), (4, -6.0), (5, -16.9), (7, -23.5)] {
            let actual_db = 20.0 * (harmonic_amplitude(&sw6, harmonic) / sw6_h1).log10();
            assert!(
                (actual_db - expected_db).abs() < 0.2,
                "SW6 H{harmonic}: {actual_db}"
            );
        }
        assert!(harmonic_amplitude(&sw6, 2) < sw6_h1 * 1e-3);
    }

    #[test]
    fn pulse_is_a_quarter_duty_bipolar_wave() {
        let wave = render_wave(FMWave::PUL, 16_384);
        assert!(
            (wave.iter().map(|sample| sample * sample).sum::<f32>() / wave.len() as f32 - 1.0)
                .abs()
                < 1e-6
        );
        assert_eq!(
            wave.iter().filter(|sample| **sample > 0.0).count(),
            wave.len() / 4
        );
        assert_eq!(wave[0], 1.0);
        assert_eq!(wave[4096], -1.0);
    }

    #[test]
    fn square_and_impulse_carriers_match_suite_71_and_are_audible() {
        let square = render_wave(FMWave::SQR, 16_384);
        assert!(
            square
                .iter()
                .all(|sample| *sample == 1.0 || *sample == -1.0)
        );
        assert_eq!(square.iter().filter(|sample| **sample > 0.0).count(), 8_192);

        let impulse = render_wave(FMWave::IMP, 16_384);
        assert_eq!(impulse.iter().filter(|sample| **sample > 0.0).count(), 1);
        assert_eq!(
            impulse.iter().filter(|sample| **sample < 0.0).count(),
            16_383
        );

        let increment = 1.0 / 4_096.0;
        let mut phases = [0.0; 4];
        let mut previous = [0.0; 4];
        let mut positive_samples = Vec::new();
        for sample_index in 0..8_192 {
            let value = frame_with_increments(
                11,
                &phases,
                &[increment; 4],
                &[1.0; 4],
                &[FMWave::IMP; 4],
                &[0.0; 4],
                &mut previous,
            );
            if value > 0.0 {
                positive_samples.push(sample_index);
            }
            for phase in &mut phases {
                *phase = (*phase + increment).fract();
            }
        }
        assert_eq!(positive_samples, [0, 4_096]);

        let phases = [0.0; 4];
        let drives = [1.0; 4];
        let feedback = [0.0; 4];
        let mut previous = [0.0; 4];
        assert_ne!(
            frame(
                11,
                &phases,
                &drives,
                &[FMWave::SQR; 4],
                &feedback,
                &mut previous
            ),
            0.0
        );
        previous = [0.0; 4];
        assert_ne!(
            frame(
                11,
                &phases,
                &drives,
                &[FMWave::IMP; 4],
                &feedback,
                &mut previous
            ),
            0.0
        );
    }

    #[test]
    fn additive_operators_are_averaged_before_clipping() {
        let phases = [0.25; 4];
        let shapes = [FMWave::SIN; 4];
        assert!(
            (frame(11, &phases, &[0.2; 4], &shapes, &[0.0; 4], &mut [0.0; 4]) - 0.2).abs() < 1e-6
        );
        assert_eq!(
            frame(11, &phases, &[2.0; 4], &shapes, &[0.0; 4], &mut [0.0; 4]),
            1.0
        );
    }

    #[test]
    fn fm_wavetable_w12_uses_its_generic_base_shape_fallback() {
        let phases = [0.25, 0.0, 0.0, 0.0];
        let shapes = [FMWave::W12, FMWave::SIN, FMWave::SIN, FMWave::SIN];
        let output = frame(
            11,
            &phases,
            &[0.986, 0.0, 0.0, 0.0],
            &shapes,
            &[0.0; 4],
            &mut [0.0; 4],
        );
        assert!((output - 0.2465).abs() < 0.002);
    }

    #[test]
    fn every_fm_wavetable_fallback_is_audible() {
        for wave_id in 0x10..=0x4c {
            let wave = FMWave::try_from(wave_id).expect("FM wavetable enum range");
            let shapes = [wave, FMWave::SIN, FMWave::SIN, FMWave::SIN];
            let output = frame(
                11,
                &[0.25; 4],
                &[1.0, 0.0, 0.0, 0.0],
                &shapes,
                &[0.0; 4],
                &mut [0.0; 4],
            );
            assert!(output.abs() > 0.01, "FM wavetable {wave_id:02x} was silent");
        }
    }

    #[test]
    fn serial_graph_uses_upstream_operator_as_phase_offset() {
        let phases = [0.25, 0.0, 0.0, 0.0];
        let shapes = [FMWave::SIN; 4];
        let actual = frame(0, &phases, &[1.0; 4], &shapes, &[0.0; 4], &mut [0.0; 4]);
        assert!((actual - 0.709009).abs() < 1e-5);
    }

    #[test]
    fn invalid_algorithm_is_silent() {
        assert_eq!(
            frame(
                12,
                &[0.25; 4],
                &[1.0; 4],
                &[FMWave::SIN; 4],
                &[0.0; 4],
                &mut [0.0; 4]
            ),
            0.0
        );
    }

    #[test]
    fn saw_modulators_reach_the_sine_carrier() {
        let phases = [0.25; 4];
        let drives = [0.5; 4];
        let sine = [FMWave::SIN; 4];
        let saw = [FMWave::SIN, FMWave::SAW, FMWave::SAW, FMWave::SIN];
        let baseline = frame(0, &phases, &drives, &sine, &[0.0; 4], &mut [0.0; 4]);
        let modulated = frame(0, &phases, &drives, &saw, &[0.0; 4], &mut [0.0; 4]);
        assert_ne!(modulated, baseline);
    }

    #[test]
    fn fm_mod_byte_controls_only_assigned_operator_level() {
        let base = [1.0, 2.0, 3.0, 4.0];
        let assignments = [[0, 0], [0, 0], [0, 0], [1, 0]];
        assert_eq!(
            modulated_drives(11, &base, &[0.0; 4], &[0.0; 4], &assignments),
            [1.0, 2.0, 3.0, 0.0]
        );
        let full = modulated_drives(
            11,
            &base,
            &[255.0, 0.0, 0.0, 0.0],
            &[255.0, 0.0, 0.0, 0.0],
            &assignments,
        );
        assert_eq!(&full[..3], &base[..3]);
        assert!((full[3] - 4.0).abs() < 1e-6);
    }

    #[test]
    fn repeated_operator_bus_assignment_is_applied_once() {
        let base = [1.0; 4];
        let static_values = [0.0; 4];
        let live_values = [0.0, 0.0, 0.0, 160.0];
        let once = modulated_drives(
            11,
            &base,
            &static_values,
            &live_values,
            &[[0, 0], [0, 0], [0, 0], [4, 0]],
        );
        let twice = modulated_drives(
            11,
            &base,
            &static_values,
            &live_values,
            &[[0, 0], [0, 0], [0, 0], [4, 4]],
        );

        assert_eq!(twice, once);
        assert!((twice[3] - 160.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn serial_operator_and_carrier_use_normalized_linear_bus() {
        let base = [1.0; 4];
        let assignments = [[0, 0], [0, 0], [2, 2], [0, 0]];
        let static_values = [0.0; 4];
        let live_values = [0.0, 255.0, 0.0, 0.0];
        let serial = modulated_drives(0, &base, &static_values, &live_values, &assignments);
        assert_eq!(serial[2], 1.0);

        let carrier = modulated_drives(11, &base, &static_values, &live_values, &assignments);
        assert_eq!(carrier[2], 1.0);
    }

    #[test]
    fn serial_operator_adds_knob_and_live_bus_then_multiplies_slots() {
        let base = [1.0; 4];
        let assignments = [[0, 0], [0, 0], [2, 2], [0, 0]];
        let static_values = [0.0, 128.0, 0.0, 0.0];
        let live_values = [0.0, 127.0, 0.0, 0.0];
        let serial = modulated_drives(0, &base, &static_values, &live_values, &assignments);
        assert_eq!(serial[2], 1.0);

        let static_values = [0.0, 64.0, 0.0, 0.0];
        let live_values = [0.0; 4];
        let serial = modulated_drives(0, &base, &static_values, &live_values, &assignments);
        assert!((serial[2] - (64.0_f32 / 255.0).powi(2)).abs() < 1e-6);
    }

    #[test]
    fn modulator_uses_linear_bus_amplitude_scale() {
        let base = [1.0; 4];
        let assignments = [[0, 0], [0, 0], [1, 0], [0, 0]];
        for value in [0.0, 64.0, 128.0, 255.0] {
            let live_values = [value, 0.0, 0.0, 0.0];
            let drives = modulated_drives(0, &base, &[0.0; 4], &live_values, &assignments);
            assert!((drives[2] - value / 255.0).abs() < 1e-6);
        }
    }

    #[test]
    fn carrier_uses_linear_bus_amplitude_scale() {
        let base = [1.0; 4];
        let assignments = [[0, 0], [0, 0], [0, 0], [2, 0]];
        let static_values = [0.0; 4];
        for value in [0.0, 64.0, 128.0, 255.0] {
            let live_values = [0.0, value, 0.0, 0.0];
            let carrier = modulated_drives(0, &base, &static_values, &live_values, &assignments);
            assert!((carrier[3] - value / 255.0).abs() < 1e-6);
        }
    }

    #[test]
    fn measured_ratio_bus_adds_up_to_sixteen_to_operator_ratio() {
        let base = [1.0; 4];
        let assignments = [[8, 0], [0; 2], [0; 2], [0; 2]];
        for (value, expected) in [
            (8.0, 1.502),
            (32.0, 3.008),
            (64.0, 5.016),
            (128.0, 9.031),
            (192.0, 13.047),
            (255.0, 17.0),
        ] {
            let increments =
                modulated_increments(&base, &[0.0, 0.0, 0.0, value], &assignments, 91.0);
            assert!((increments[0] - expected).abs() < 0.002);
            assert_eq!(&increments[1..], &[1.0; 3]);
        }
    }

    #[test]
    fn duplicate_ratio_bus_assignment_is_applied_once() {
        let increments = modulated_increments(
            &[1.0; 4],
            &[0.0, 0.0, 0.0, 255.0],
            &[[8, 8], [0; 2], [0; 2], [0; 2]],
            91.0,
        );
        assert_eq!(increments[0], 17.0);
    }

    #[test]
    fn measured_pitch_bus_offsets_operator_in_semitones() {
        let base = [1.0; 4];
        let assignments = [[12, 0], [0; 2], [0; 2], [0; 2]];
        for (value, semitones) in [(0.0, 0.0), (8.0, 8.0), (32.0, 32.0), (64.0, 64.0)] {
            let increments =
                modulated_increments(&base, &[0.0, 0.0, 0.0, value], &assignments, 91.0);
            let expected = 2.0_f64.powf(semitones / 12.0);
            assert!((increments[0] - expected).abs() / expected < 1e-6);
            assert_eq!(&increments[1..], &[1.0; 3]);
        }
    }

    #[test]
    fn duplicate_pitch_bus_assignment_is_applied_once() {
        let once = modulated_increments(
            &[1.0; 4],
            &[0.0, 0.0, 0.0, 32.0],
            &[[12, 0], [0; 2], [0; 2], [0; 2]],
            91.0,
        );
        let twice = modulated_increments(
            &[1.0; 4],
            &[0.0, 0.0, 0.0, 32.0],
            &[[12, 12], [0; 2], [0; 2], [0; 2]],
            91.0,
        );
        assert_eq!(twice, once);
    }

    #[test]
    fn pitch_bus_clamps_at_midi_note_127() {
        let increments = modulated_increments(
            &[65.406_391_3 / 44_100.0; 4],
            &[0.0, 0.0, 0.0, 255.0],
            &[[12, 0], [0; 2], [0; 2], [0; 2]],
            91.0,
        );
        let expected = 12_543.853_766 / 44_100.0;
        assert!((increments[0] - expected).abs() / expected < 1e-6);
    }

    #[test]
    fn measured_operator_level_knots_are_monotonic() {
        assert_eq!(operator_drive(0), 0.0);
        assert!((operator_drive(0x40) - 0.493).abs() < 1e-6);
        assert!((operator_drive(0x80) - 0.986).abs() < 1e-6);
        assert!((operator_drive(0xff) - 15.2).abs() < 1e-4);
    }

    #[test]
    fn feedback_preserves_state_across_render_blocks() {
        fn render(blocks: &[usize], feedback: f32) -> Vec<f32> {
            let mut phases = [0.0; 4];
            let mut previous = [0.0; 4];
            let mut output = Vec::new();
            for &length in blocks {
                for _ in 0..length {
                    output.push(frame(
                        0,
                        &phases,
                        &[0.0, 0.0, 0.986, 0.986],
                        &[FMWave::SIN; 4],
                        &[0.0, 0.0, feedback, 0.0],
                        &mut previous,
                    ));
                    for phase in &mut phases {
                        *phase = (*phase + 65.406_391_325_15 / 44_100.0).fract();
                    }
                }
            }
            output
        }
        let whole = render(&[1024], 1.0);
        assert_eq!(whole, render(&[256, 768], 1.0));
        assert_ne!(whole, render(&[1024], 0.0));
    }
}
