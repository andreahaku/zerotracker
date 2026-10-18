//! Stereo chorus return with preallocated, modulated delay lines.

use m8fmt::{EffectsSettings, FxKind};
use std::f64::consts::TAU;

const MAX_DELAY_MS: usize = 50;

pub(crate) struct StereoChorus {
    rings: [Vec<f32>; 2],
    position: usize,
    phase: f64,
    sample_rate: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct ChorusParams {
    base_frames: f64,
    depth_frames: f64,
    phase_increment: f64,
    width: f64,
}

impl StereoChorus {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            rings: std::array::from_fn(|_| {
                vec![0.0; sample_rate as usize * MAX_DELAY_MS / 1000 + 2]
            }),
            position: 0,
            phase: 0.0,
            sample_rate,
        }
    }

    pub(crate) fn params(&self, settings: &EffectsSettings) -> Option<ChorusParams> {
        if !matches!(settings.mfx_kind, None | Some(FxKind::Chorus)) {
            return None;
        }
        // These initial maps are replaced by the isolated CALMX controls.
        let depth = f64::from(settings.chorus_mod_depth) / 255.0;
        let freq = f64::from(settings.chorus_mod_freq) / 255.0;
        Some(ChorusParams {
            base_frames: f64::from(self.sample_rate) * 0.015,
            depth_frames: f64::from(self.sample_rate) * (0.001 + 0.004 * depth),
            phase_increment: (0.1 + 2.0 * freq) / f64::from(self.sample_rate),
            width: f64::from(settings.chorus_width) / 255.0,
        })
    }

    pub(crate) fn frame(&mut self, input: (f32, f32), params: ChorusParams) -> (f32, f32) {
        self.rings[0][self.position] = input.0;
        self.rings[1][self.position] = input.1;
        let left_delay = params.base_frames + params.depth_frames * (TAU * self.phase).sin();
        let right_phase = (self.phase + params.width * 0.5).fract();
        let right_delay = params.base_frames + params.depth_frames * (TAU * right_phase).sin();
        let left = self.read(0, left_delay);
        let right = self.read(1, right_delay);
        self.position = (self.position + 1) % self.rings[0].len();
        self.phase = (self.phase + params.phase_increment).fract();
        // Isolated mixed-send measurements show the wet return must add with
        // the dry path; the current tap polarity partially cancels it.
        (-left, -right)
    }

    fn read(&self, channel: usize, delay: f64) -> f32 {
        let ring = &self.rings[channel];
        let len = ring.len() as f64;
        let position = (self.position as f64 - delay).rem_euclid(len);
        let first = position.floor() as usize;
        let second = (first + 1) % ring.len();
        let fraction = (position - first as f64) as f32;
        ring[first] * (1.0 - fraction) + ring[second] * fraction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_stereo_returns_are_bounded_and_keep_history() {
        let mut chorus = StereoChorus::new(8_000);
        let params = ChorusParams {
            base_frames: 80.0,
            depth_frames: 0.0,
            phase_increment: 0.0,
            width: 1.0,
        };
        let mut first = None;
        for frame in 0..160 {
            let (left, right) =
                chorus.frame((f32::from(frame == 0), f32::from(frame == 0)), params);
            if left != 0.0 || right != 0.0 {
                first = Some((frame, left, right));
            }
        }
        assert_eq!(first, Some((80, -1.0, -1.0)));
    }

    #[test]
    fn input_channels_remain_separate_in_the_delayed_returns() {
        let mut chorus = StereoChorus::new(8_000);
        let params = ChorusParams {
            base_frames: 80.0,
            depth_frames: 0.0,
            phase_increment: 0.0,
            width: 1.0,
        };
        for frame in 0..80 {
            chorus.frame((f32::from(frame == 0), 0.0), params);
        }
        assert_eq!(chorus.frame((0.0, 0.0), params), (-1.0, 0.0));
    }
}
