//! Beat-synchronised alternating stereo delay. The timing and feedback
//! topology follow isolated reference captures; input EQ and send gain are
//! provisional.

use m8fmt::EffectsSettings;

const MAX_DELAY_SECONDS: usize = 8;

pub(crate) struct StereoDelay {
    ring: Vec<[f32; 2]>,
    position: usize,
    sample_rate: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct DelayParams {
    left_frames: usize,
    right_frames: usize,
    feedback: f32,
}

impl StereoDelay {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let frames = sample_rate as usize * MAX_DELAY_SECONDS + 1;
        Self {
            ring: vec![[0.0; 2]; frames],
            position: 0,
            sample_rate,
        }
    }

    pub(crate) fn params(&self, settings: &EffectsSettings, tempo: f64) -> DelayParams {
        DelayParams {
            left_frames: time_frames(
                self.sample_rate,
                tempo,
                settings.delay_time_l,
                self.ring.len(),
            ),
            right_frames: time_frames(
                self.sample_rate,
                tempo,
                settings.delay_time_r,
                self.ring.len(),
            ),
            feedback: f32::from(settings.delay_feedback) / 256.0,
        }
    }

    pub(crate) fn frame(&mut self, input: f32, params: DelayParams) -> (f32, f32) {
        let len = self.ring.len();
        let left = self.ring[(self.position + len - params.left_frames) % len][0];
        let right = self.ring[(self.position + len - params.right_frames) % len][1];
        // The first left and right echoes have the same level. The next
        // left/right pair is reduced by FEEDBACK, as seen at FEEDBACK=80.
        self.ring[self.position] = [input + right * params.feedback, left];
        self.position += 1;
        if self.position == len {
            self.position = 0;
        }
        (left, right)
    }
}

fn time_frames(sample_rate: u32, tempo: f64, time: u8, ring_len: usize) -> usize {
    let beat_frames = f64::from(sample_rate) * 60.0 / tempo.clamp(1.0, 999.0);
    ((beat_frames * f64::from(time) / 64.0).round() as usize).clamp(1, ring_len - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_byte_follows_reference_beat_controls() {
        let ring_len = 8_000 * 8 + 1;
        assert_eq!(time_frames(8_000, 120.0, 0x10, ring_len), 1_000);
        assert_eq!(time_frames(8_000, 120.0, 0x30, ring_len), 3_000);
        assert_eq!(time_frames(8_000, 120.0, 0x60, ring_len), 6_000);
        assert_eq!(time_frames(8_000, 90.0, 0x30, ring_len), 4_000);
        assert_eq!(time_frames(8_000, 150.0, 0x30, ring_len), 2_400);
    }

    #[test]
    fn alternating_echoes_follow_beat_and_feedback() {
        let mut delay = StereoDelay::new(8_000);
        let params = DelayParams {
            left_frames: 3_000,
            right_frames: 3_000,
            feedback: 0.5,
        };
        let mut echoes = Vec::new();
        for frame in 0..12_001 {
            let (left, right) = delay.frame(f32::from(frame == 0), params);
            if left != 0.0 || right != 0.0 {
                echoes.push((frame, left, right));
            }
        }
        assert_eq!(
            echoes,
            [
                (3_000, 1.0, 0.0),
                (6_000, 0.0, 1.0),
                (9_000, 0.5, 0.0),
                (12_000, 0.0, 0.5)
            ]
        );
    }
}
