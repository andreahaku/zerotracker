//! Lock-free output taps for master and per-track UI visualizations.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub(crate) const OUTPUT_WINDOW_FRAMES: usize = 2048;
const RING_FRAMES: usize = OUTPUT_WINDOW_FRAMES * 2;
pub const TRACK_OUTPUT_WINDOW_FRAMES: usize = 512;
const TRACK_RING_FRAMES: usize = TRACK_OUTPUT_WINDOW_FRAMES * 2;
const TRACK_COUNT: usize = 8;

/// Per-track post-fader mono waveform taps plus stereo peak meters.
pub(crate) struct TrackOutputTaps {
    tracks: [TrackOutputTap; TRACK_COUNT],
    peaks: [AtomicU32; TRACK_COUNT + 1],
    activity: [AtomicU32; TRACK_COUNT],
}

struct TrackOutputTap {
    slots: [AtomicU64; TRACK_RING_FRAMES],
    published: AtomicU64,
}

impl TrackOutputTaps {
    pub(crate) fn new() -> Self {
        Self {
            tracks: std::array::from_fn(|_| TrackOutputTap {
                slots: std::array::from_fn(|_| AtomicU64::new(0)),
                published: AtomicU64::new(0),
            }),
            peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            activity: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    pub(crate) fn begin_block(&self) -> [u64; TRACK_COUNT] {
        std::array::from_fn(|track| self.tracks[track].published.load(Ordering::Relaxed))
    }

    /// Called by the render thread for each frame. Samples are post-fader mono.
    pub(crate) fn write_frame(&self, next: &mut [u64; TRACK_COUNT], samples: [f32; TRACK_COUNT]) {
        for track in 0..TRACK_COUNT {
            let index = next[track];
            let packed = ((index as u32 as u64) << 32) | u64::from(samples[track].to_bits());
            self.tracks[track].slots[index as usize % TRACK_RING_FRAMES]
                .store(packed, Ordering::Release);
            next[track] = index.wrapping_add(1);
        }
    }

    /// Called once by the render thread when a block has been fully written.
    /// The peak pairs contain each track's post-fader L/R and master's L/R.
    pub(crate) fn end_block(
        &self,
        next: [u64; TRACK_COUNT],
        peaks: &[[u16; 2]; TRACK_COUNT + 1],
        activity: &[[u16; 2]; TRACK_COUNT],
    ) {
        for (tap, end) in self.tracks.iter().zip(next) {
            tap.published.store(end, Ordering::Release);
        }
        for (slot, pair) in self.peaks.iter().zip(peaks) {
            slot.store(
                u32::from(pair[0]) | (u32::from(pair[1]) << 16),
                Ordering::Release,
            );
        }
        for (slot, pair) in self.activity.iter().zip(activity) {
            slot.store(
                u32::from(pair[0]) | (u32::from(pair[1]) << 16),
                Ordering::Release,
            );
        }
    }

    pub(crate) fn read_track(&self, track: u8, output: &mut [f32]) -> bool {
        let Some(tap) = self.tracks.get(usize::from(track)) else {
            return false;
        };
        if output.is_empty() || output.len() > TRACK_OUTPUT_WINDOW_FRAMES {
            return false;
        }
        let end = tap.published.load(Ordering::Acquire);
        let requested = output.len() as u64;
        if end < requested {
            return false;
        }
        let start = end - requested;
        for (offset, sample) in output.iter_mut().enumerate() {
            let index = start + offset as u64;
            let packed = tap.slots[index as usize % TRACK_RING_FRAMES].load(Ordering::Acquire);
            if packed >> 32 != u64::from(index as u32) {
                return false;
            }
            *sample = f32::from_bits(packed as u32);
        }
        tap.published.load(Ordering::Acquire).wrapping_sub(start) <= TRACK_RING_FRAMES as u64
    }

    pub(crate) fn read_peaks(&self, output: &mut [u16; (TRACK_COUNT + 1) * 2]) {
        for (index, slot) in self.peaks.iter().enumerate() {
            let packed = slot.load(Ordering::Acquire);
            output[index * 2] = packed as u16;
            output[index * 2 + 1] = (packed >> 16) as u16;
        }
    }

    pub(crate) fn read_activity(&self, output: &mut [u16; TRACK_COUNT * 2]) {
        for (index, slot) in self.activity.iter().enumerate() {
            let packed = slot.load(Ordering::Acquire);
            output[index * 2] = packed as u16;
            output[index * 2 + 1] = (packed >> 16) as u16;
        }
    }
}

pub(crate) struct OutputTap {
    slots: [AtomicU64; RING_FRAMES],
    published: AtomicU64,
}

impl OutputTap {
    pub(crate) fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
            published: AtomicU64::new(0),
        }
    }

    /// Called only by the render thread, after the final PCM is written.
    pub(crate) fn publish_pcm(&self, output: &[i16], channels: usize) {
        let mut next = self.published.load(Ordering::Relaxed);
        for frame in output.chunks_exact(channels) {
            let mono = if channels == 2 {
                (f32::from(frame[0]) + f32::from(frame[1])) / (2.0 * 32768.0)
            } else {
                f32::from(frame[0]) / 32768.0
            };
            let packed = ((next as u32 as u64) << 32) | u64::from(mono.to_bits());
            self.slots[next as usize % RING_FRAMES].store(packed, Ordering::Release);
            next = next.wrapping_add(1);
            self.published.store(next, Ordering::Release);
        }
    }

    /// Returns false when history is missing or the writer overtakes this copy.
    pub(crate) fn read(&self, output: &mut [f32]) -> bool {
        if output.is_empty() || output.len() > OUTPUT_WINDOW_FRAMES {
            return false;
        }
        let end = self.published.load(Ordering::Acquire);
        let requested = output.len() as u64;
        if end < requested {
            return false;
        }
        let start = end - requested;
        for (offset, sample) in output.iter_mut().enumerate() {
            let index = start + offset as u64;
            let packed = self.slots[index as usize % RING_FRAMES].load(Ordering::Acquire);
            if packed >> 32 != u64::from(index as u32) {
                return false;
            }
            *sample = f32::from_bits(packed as u32);
        }
        self.published.load(Ordering::Acquire).wrapping_sub(start) <= RING_FRAMES as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_waveform_and_stereo_peaks_are_readable_after_a_block() {
        let taps = TrackOutputTaps::new();
        let mut next = taps.begin_block();
        taps.write_frame(&mut next, [0.25; TRACK_COUNT]);
        taps.write_frame(&mut next, [-0.5; TRACK_COUNT]);
        let mut peaks = [[0; 2]; TRACK_COUNT + 1];
        peaks[0] = [10, 20];
        peaks[8] = [30, 40];
        let mut activity = [[0; 2]; TRACK_COUNT];
        activity[0] = [50, 60];
        taps.end_block(next, &peaks, &activity);

        let mut waveform = [0.0; 2];
        assert!(taps.read_track(0, &mut waveform));
        assert_eq!(waveform, [0.25, -0.5]);
        let mut read_peaks = [0_u16; (TRACK_COUNT + 1) * 2];
        taps.read_peaks(&mut read_peaks);
        assert_eq!(&read_peaks[..2], &[10, 20]);
        assert_eq!(&read_peaks[16..18], &[30, 40]);
        let mut read_activity = [0_u16; TRACK_COUNT * 2];
        taps.read_activity(&mut read_activity);
        assert_eq!(&read_activity[..2], &[50, 60]);
        assert_eq!(&read_activity[2..], &[0; TRACK_COUNT * 2 - 2]);
        assert!(!taps.read_track(8, &mut waveform));
    }
}
