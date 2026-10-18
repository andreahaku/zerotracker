//! Immutable playback data and its one-slot real-time mailbox.

use m8fmt::Song;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

const IDLE: u8 = 0;
const WRITING: u8 = 1;
const PENDING: u8 = 2;
const RETIRED: u8 = 3;

/// A predecoded mono or stereo sample, interleaved and normalized to f32.
#[derive(Debug, Clone)]
pub struct Sample {
    pub rate: u32,
    pub channels: u8,
    pub pcm: Arc<[f32]>,
}

impl Sample {
    pub fn frames(&self) -> usize {
        self.pcm.len() / usize::from(self.channels.max(1))
    }

    pub fn is_valid(&self) -> bool {
        self.rate > 0
            && (1..=2).contains(&self.channels)
            && !self.pcm.is_empty()
            && self.pcm.len().is_multiple_of(usize::from(self.channels))
    }
}

/// Each sample slot corresponds to the same instrument index in `song`.
/// Build this on the UI thread and treat it as immutable after publication.
pub struct PlaybackSnapshot {
    pub song: Song,
    pub samples: Vec<Option<Sample>>,
    /// WAV cue positions by instrument slot, in source sample frames.
    /// Empty for samples without file-defined slices.
    pub sample_cues: Vec<Arc<[u32]>>,
}

impl PlaybackSnapshot {
    pub fn new(song: Song, samples: Vec<Option<Sample>>) -> Option<Self> {
        if samples.len() != song.instruments.len()
            || samples.iter().flatten().any(|sample| !sample.is_valid())
        {
            return None;
        }
        let sample_cues = vec![Arc::from([]); samples.len()];
        Some(Self {
            song,
            samples,
            sample_cues,
        })
    }

    /// Attach cue points decoded on the UI thread before publication.
    pub fn with_sample_cues(mut self, sample_cues: Vec<Arc<[u32]>>) -> Option<Self> {
        if sample_cues.len() != self.samples.len() {
            return None;
        }
        for (cues, sample) in sample_cues.iter().zip(&self.samples) {
            if !cues.is_empty() {
                let frames = sample.as_ref()?.frames();
                if cues.windows(2).any(|pair| pair[0] >= pair[1])
                    || cues.iter().any(|&cue| cue as usize > frames)
                {
                    return None;
                }
            }
        }
        self.sample_cues = sample_cues;
        Some(self)
    }
}

/// Ownership protocol: UI writes pending, audio adopts it and acknowledges the
/// old snapshot as retired, UI collects it. The audio thread never frees data.
pub(crate) struct Mailbox {
    state: AtomicU8,
    pending: AtomicPtr<PlaybackSnapshot>,
    retired: AtomicPtr<PlaybackSnapshot>,
}

impl Mailbox {
    pub(crate) fn new() -> Self {
        Self {
            state: AtomicU8::new(IDLE),
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Returns the snapshot on failure, so the UI may coalesce and retry.
    pub(crate) fn publish(
        &self,
        snapshot: Box<PlaybackSnapshot>,
    ) -> Result<(), Box<PlaybackSnapshot>> {
        if self
            .state
            .compare_exchange(IDLE, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(snapshot);
        }
        self.pending
            .store(Box::into_raw(snapshot), Ordering::Relaxed);
        self.state.store(PENDING, Ordering::Release);
        Ok(())
    }

    /// The audio thread calls this only at the beginning of a render call.
    /// `active` is an exclusively held pointer owned by the engine.
    pub(crate) fn adopt(&self, active: &mut *mut PlaybackSnapshot) -> bool {
        if self.state.load(Ordering::Acquire) != PENDING {
            return false;
        }
        let replacement = self.pending.swap(ptr::null_mut(), Ordering::Relaxed);
        debug_assert!(!replacement.is_null());
        let old = std::mem::replace(active, replacement);
        self.retired.store(old, Ordering::Relaxed);
        self.state.store(RETIRED, Ordering::Release);
        true
    }

    /// UI thread only. It may drop the returned value after the audio thread
    /// has completed the exchange. The first exchange retires a null pointer.
    pub(crate) fn collect_retired(&self) -> Option<Box<PlaybackSnapshot>> {
        if self.state.load(Ordering::Acquire) != RETIRED {
            return None;
        }
        let pointer = self.retired.swap(ptr::null_mut(), Ordering::Relaxed);
        self.state.store(IDLE, Ordering::Release);
        if pointer.is_null() {
            None
        } else {
            // SAFETY: only the UI collects an acknowledged retired pointer.
            Some(unsafe { Box::from_raw(pointer) })
        }
    }

    /// Called only when the audio thread has stopped and the engine is
    /// destroyed; pending and retired pointers are distinct generations.
    pub(crate) fn release_remaining(&mut self) {
        for pointer in [*self.pending.get_mut(), *self.retired.get_mut()] {
            if !pointer.is_null() {
                // SAFETY: engine destruction owns all unpublished generations.
                unsafe { drop(Box::from_raw(pointer)) };
            }
        }
        *self.pending.get_mut() = ptr::null_mut();
        *self.retired.get_mut() = ptr::null_mut();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_validation() {
        let valid = Sample {
            rate: 44_100,
            channels: 2,
            pcm: Arc::from([0.0, 0.5]),
        };
        assert!(valid.is_valid());
        assert_eq!(valid.frames(), 1);
        let invalid = Sample {
            channels: 3,
            ..valid
        };
        assert!(!invalid.is_valid());
    }
}
