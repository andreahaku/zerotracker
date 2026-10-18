//! Initial stereo room return. The independent comb lengths produce a
//! diffuse tail without allocating or locking in the audio callback.

use crate::filter::BiquadStereo;
use m8fmt::EffectsSettings;

const LEFT_MS: [f64; 4] = [70.0, 83.0, 95.0, 109.0];
const RIGHT_MS: [f64; 4] = [73.0, 88.0, 99.0, 115.0];

struct Comb {
    ring: Vec<f32>,
    position: usize,
    filtered: f32,
}

impl Comb {
    fn new(frames: usize) -> Self {
        Self {
            ring: vec![0.0; frames + 1],
            position: 0,
            filtered: 0.0,
        }
    }

    fn frame(&mut self, input: f32, delay: usize, feedback: f32) -> f32 {
        let len = self.ring.len();
        let output = self.ring[(self.position + len - delay) % len];
        // A low-pass in the feedback path prevents a bright metallic tail.
        self.filtered += 0.9 * (output - self.filtered);
        self.ring[self.position] = input + self.filtered * feedback;
        self.position += 1;
        if self.position == len {
            self.position = 0;
        }
        output
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ReverbParams {
    delays: [[usize; 4]; 2],
    feedback: [[f32; 4]; 2],
}

pub(crate) struct RoomReverb {
    combs: [[Comb; 4]; 2],
    sample_rate: u32,
    input_filter: Option<BiquadStereo>,
    filter_byte: Option<u8>,
}

impl RoomReverb {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let lengths = [LEFT_MS, RIGHT_MS];
        Self {
            combs: std::array::from_fn(|side| {
                std::array::from_fn(|index| {
                    Comb::new(
                        (f64::from(sample_rate) * lengths[side][index] / 1_000.0).ceil() as usize,
                    )
                })
            }),
            sample_rate,
            input_filter: None,
            filter_byte: None,
        }
    }

    pub(crate) fn params(
        &mut self,
        settings: &EffectsSettings,
        decay_override: Option<u8>,
    ) -> ReverbParams {
        let high_pass = settings.reverb_filter.as_ref().map(|eq| eq.high_pass);
        if self.filter_byte != high_pass {
            self.filter_byte = high_pass;
            self.input_filter = high_pass.filter(|&value| value != 0).map(|value| {
                // Calibrated only at byte 41 (about 160 Hz). Other bytes
                // follow an extrapolated exponential mapping.
                let cutoff = (20.0 * 2.0_f64.powf(f64::from(value) / 21.6))
                    .min(f64::from(self.sample_rate) * 0.45);
                BiquadStereo::high_pass(cutoff, 0.707, self.sample_rate)
            });
        }
        let size = 0.25 + 0.75 * f64::from(settings.reverb_size) / 255.0;
        let decay = decay_override.unwrap_or(settings.reverb_damping);
        let t60 = reverb_t60(decay);
        let lengths = [LEFT_MS, RIGHT_MS];
        let delays = std::array::from_fn(|side| {
            std::array::from_fn(|index| {
                ((f64::from(self.sample_rate) * lengths[side][index] * size / 1_000.0).round()
                    as usize)
                    .clamp(1, self.combs[side][index].ring.len() - 1)
            })
        });
        let feedback = std::array::from_fn(|side| {
            std::array::from_fn(|index| {
                (10.0_f64
                    .powf(-3.0 * delays[side][index] as f64 / (f64::from(self.sample_rate) * t60)))
                    as f32
            })
        });
        ReverbParams { delays, feedback }
    }

    pub(crate) fn frame(&mut self, input: f32, params: ReverbParams) -> (f32, f32) {
        let input = self
            .input_filter
            .as_mut()
            .map_or(input, |filter| filter.frame(input, 0.0).0);
        let mut output = [0.0; 2];
        for (side, output_side) in output.iter_mut().enumerate() {
            for index in 0..4 {
                *output_side += self.combs[side][index].frame(
                    input,
                    params.delays[side][index],
                    params.feedback[side][index],
                );
            }
            *output_side *= 0.25;
        }
        let early = input * 0.09;
        (output[0] + early, output[1] + early)
    }
}

fn reverb_t60(decay: u8) -> f64 {
    // The measured tail slopes map to these feedback parameters in the current
    // comb network. Interpolate in log space between measured decay controls.
    // XRD 00 is the separately verified shortest decay; C0 retains the earlier
    // CALER80 measurement.
    const ANCHORS: [(u8, f64); 5] = [
        (0x00, 0.55),
        (0xc0, 3.5),
        (0xd0, 4.17),
        (0xe0, 6.41),
        (0xf0, 13.8),
    ];

    let upper = ANCHORS
        .iter()
        .position(|(value, _)| decay <= *value)
        .unwrap_or(ANCHORS.len() - 1);
    let (lo_byte, lo_t60, hi_byte, hi_t60) = if upper == 0 {
        return ANCHORS[0].1;
    } else if decay > ANCHORS[ANCHORS.len() - 1].0 {
        let (lo_byte, lo_t60) = ANCHORS[ANCHORS.len() - 2];
        let (hi_byte, hi_t60) = ANCHORS[ANCHORS.len() - 1];
        (lo_byte, lo_t60, hi_byte, hi_t60)
    } else {
        let (lo_byte, lo_t60) = ANCHORS[upper - 1];
        let (hi_byte, hi_t60) = ANCHORS[upper];
        (lo_byte, lo_t60, hi_byte, hi_t60)
    };
    let fraction = (f64::from(decay) - f64::from(lo_byte)) / f64::from(hi_byte - lo_byte);
    (lo_t60.ln() + (hi_t60.ln() - lo_t60.ln()) * fraction).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_curve_matches_reference_anchors_and_grows_past_e0() {
        assert!((reverb_t60(0x00) - 0.55).abs() < 1e-9);
        assert!((reverb_t60(0xc0) - 3.5).abs() < 1e-9);
        assert!((reverb_t60(0xd0) - 4.17).abs() < 1e-9);
        assert!((reverb_t60(0xe0) - 6.41).abs() < 1e-9);
        assert!((reverb_t60(0xf0) - 13.8).abs() < 1e-9);
        assert!(reverb_t60(0xf0) > reverb_t60(0xe0));
        assert!(reverb_t60(0xff) > reverb_t60(0xf0));
    }

    #[test]
    fn room_tail_is_stereo_and_bounded() {
        let mut room = RoomReverb::new(8_000);
        let delays = [[560, 664, 760, 872], [584, 704, 792, 920]];
        let params = ReverbParams {
            delays,
            feedback: [[0.9; 4]; 2],
        };
        let mut left_first = None;
        let mut right_first = None;
        let mut stereo_tail = false;
        for frame in 0..8_000 {
            let (l, r) = room.frame(f32::from(frame == 0), params);
            if l != 0.0 && left_first.is_none() {
                left_first = Some(frame);
            }
            if r != 0.0 && right_first.is_none() {
                right_first = Some(frame);
            }
            stereo_tail |= frame > 1_000 && (l - r).abs() > 1e-6;
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
        }
        assert_eq!((left_first, right_first), (Some(0), Some(0)));
        assert!(stereo_tail);
    }

    #[test]
    fn changing_to_short_decay_preserves_the_existing_reverb_tail() {
        let mut room = RoomReverb::new(8_000);
        let delays = [[560, 664, 760, 872], [584, 704, 792, 920]];
        let params = ReverbParams {
            delays,
            feedback: [[0.9; 4]; 2],
        };
        for frame in 0..2_000 {
            room.frame(f32::from(frame == 0), params);
        }
        let short_decay = ReverbParams {
            delays,
            feedback: [[0.1; 4]; 2],
        };
        let mut tail_remains = false;
        for _ in 0..2_000 {
            let (left, right) = room.frame(0.0, short_decay);
            tail_remains |= left != 0.0 || right != 0.0;
        }
        assert!(
            tail_remains,
            "shortening decay must not erase the ring buffers"
        );
    }
}
