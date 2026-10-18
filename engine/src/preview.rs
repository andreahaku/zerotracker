//! One-shot sample preview with UI-side reclamation of replaced buffers.

use crate::Sample;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

const IDLE: u8 = 0;
const WRITING: u8 = 1;
const PENDING: u8 = 2;
const RETIRED: u8 = 3;

pub(crate) struct PreviewSample {
    pub(crate) sample: Sample,
}

impl PreviewSample {
    pub(crate) fn new(sample: Sample) -> Self {
        Self { sample }
    }
}

/// UI publishes a pending replacement or stop request. The audio thread
/// transfers the old active generation to `retired`; only the UI frees it.
pub(crate) struct PreviewMailbox {
    state: AtomicU8,
    pending: AtomicPtr<PreviewSample>,
    retired: AtomicPtr<PreviewSample>,
}

impl PreviewMailbox {
    pub(crate) fn new() -> Self {
        Self {
            state: AtomicU8::new(IDLE),
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub(crate) fn publish(&self, sample: Option<PreviewSample>) -> Result<(), Option<Sample>> {
        if self
            .state
            .compare_exchange(IDLE, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(sample.map(|sample| sample.sample));
        }
        self.pending.store(
            sample.map_or(ptr::null_mut(), |sample| Box::into_raw(Box::new(sample))),
            Ordering::Relaxed,
        );
        self.state.store(PENDING, Ordering::Release);
        Ok(())
    }

    pub(crate) fn adopt(&self, active: &mut *mut PreviewSample, position: &mut f64) -> bool {
        if self.state.load(Ordering::Acquire) != PENDING {
            return false;
        }
        let replacement = self.pending.swap(ptr::null_mut(), Ordering::Relaxed);
        let old = std::mem::replace(active, replacement);
        *position = 0.0;
        self.retired.store(old, Ordering::Relaxed);
        self.state.store(RETIRED, Ordering::Release);
        true
    }

    /// Audio thread only. Returns true when the active generation was retired.
    pub(crate) fn retire_active(&self, active: &mut *mut PreviewSample) -> bool {
        if self
            .state
            .compare_exchange(IDLE, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.retired.store(*active, Ordering::Relaxed);
        *active = ptr::null_mut();
        self.state.store(RETIRED, Ordering::Release);
        true
    }

    pub(crate) fn collect_retired(&self) -> Option<Box<PreviewSample>> {
        if self.state.load(Ordering::Acquire) != RETIRED {
            return None;
        }
        let pointer = self.retired.swap(ptr::null_mut(), Ordering::Relaxed);
        self.state.store(IDLE, Ordering::Release);
        if pointer.is_null() {
            None
        } else {
            // SAFETY: RETIRED acknowledges that audio no longer accesses it.
            Some(unsafe { Box::from_raw(pointer) })
        }
    }

    pub(crate) fn release_remaining(&mut self) {
        for pointer in [*self.pending.get_mut(), *self.retired.get_mut()] {
            if !pointer.is_null() {
                // SAFETY: destruction requires that the audio thread stopped.
                unsafe { drop(Box::from_raw(pointer)) };
            }
        }
        *self.pending.get_mut() = ptr::null_mut();
        *self.retired.get_mut() = ptr::null_mut();
    }
}
