//! ZeroTracker sequencer and real-time audio engine.
//!
//! The host owns the audio device. One audio thread calls `zt_engine_render`;
//! the UI may publish prepared song snapshots and request transport changes.

mod chorus;
mod delay;
mod eq;
mod filter;
mod fm;
mod hyper_osc;
mod macro_osc;
mod modulation;
mod output_tap;
mod preview;
mod reverb;
mod sequencer;
mod snapshot;
mod transport;
mod wave_fallback;

pub use eq::band_response_db;
pub use snapshot::{PlaybackSnapshot, Sample};
pub use transport::{TransportMode, ZtTrackStatus};

use output_tap::{OUTPUT_WINDOW_FRAMES, OutputTap, TRACK_OUTPUT_WINDOW_FRAMES, TrackOutputTaps};
use preview::{PreviewMailbox, PreviewSample};
use sequencer::Sequencer;
use snapshot::Mailbox;
use std::cell::UnsafeCell;
use std::f64::consts::TAU;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use transport::{Telemetry, Transport, TransportRequest};

const TEST_FREQUENCY_HZ: f64 = 440.0;
const TEST_GAIN: f64 = 0.05;

struct RenderState {
    active: *mut PlaybackSnapshot,
    active_preview: *mut PreviewSample,
    preview_position: f64,
    phase: f64,
    phase_increment: f64,
    test_tone: bool,
    last_transport_sequence: u32,
    sequencer: Sequencer,
}

impl RenderState {
    fn render_tone(&mut self, output: &mut [i16], channels: usize) {
        for frame in output.chunks_exact_mut(channels) {
            let sample = (self.phase.sin() * TEST_GAIN * f64::from(i16::MAX)) as i16;
            frame.fill(sample);
            self.phase += self.phase_increment;
            if self.phase >= TAU {
                self.phase -= TAU;
            }
        }
    }
}

/// Opaque to C callers. Render state is exclusive to one audio thread. The UI
/// uses only atomic control and the one-slot snapshot mailbox.
pub struct ZtEngine {
    render_state: UnsafeCell<RenderState>,
    mailbox: Mailbox,
    transport: Transport,
    telemetry: Telemetry,
    track_flags: AtomicU16,
    master_limiter_bypass: AtomicBool,
    output_tap: OutputTap,
    track_output_taps: TrackOutputTaps,
    preview_mailbox: PreviewMailbox,
    preview_playing: std::sync::atomic::AtomicBool,
    sample_rate: u32,
}

// SAFETY: only the audio thread accesses render_state. Mailbox, transport,
// and telemetry use atomics. Destroy requires the audio thread to have stopped.
unsafe impl Send for ZtEngine {}
// SAFETY: shared UI/audio access is restricted to the atomic fields above.
unsafe impl Sync for ZtEngine {}

impl ZtEngine {
    fn new(sample_rate: u32, test_tone: bool) -> Option<Self> {
        if !(8_000..=192_000).contains(&sample_rate) {
            return None;
        }
        Some(Self {
            render_state: UnsafeCell::new(RenderState {
                active: ptr::null_mut(),
                active_preview: ptr::null_mut(),
                preview_position: 0.0,
                phase: 0.0,
                phase_increment: TAU * TEST_FREQUENCY_HZ / f64::from(sample_rate),
                test_tone,
                last_transport_sequence: 0,
                sequencer: Sequencer::new(sample_rate),
            }),
            mailbox: Mailbox::new(),
            transport: Transport::new(),
            telemetry: Telemetry::new(),
            track_flags: AtomicU16::new(0),
            master_limiter_bypass: AtomicBool::new(false),
            output_tap: OutputTap::new(),
            track_output_taps: TrackOutputTaps::new(),
            preview_mailbox: PreviewMailbox::new(),
            preview_playing: std::sync::atomic::AtomicBool::new(false),
            sample_rate,
        })
    }

    /// UI thread: submit a new immutable generation or get it back if the
    /// mailbox is busy. No old data is freed on the audio thread.
    pub fn publish(&self, snapshot: Box<PlaybackSnapshot>) -> Result<(), Box<PlaybackSnapshot>> {
        self.mailbox.publish(snapshot)
    }

    /// UI thread: collect and drop the previous generation after the audio
    /// thread has acknowledged a swap.
    pub fn collect_retired(&self) -> Option<Box<PlaybackSnapshot>> {
        drop(self.preview_mailbox.collect_retired());
        self.mailbox.collect_retired()
    }

    /// Submit or stop a one-shot WAV preview from the UI thread.
    pub fn set_preview(&self, sample: Option<Sample>) -> Result<(), Option<Sample>> {
        if sample.as_ref().is_some_and(|sample| !sample.is_valid()) {
            return Err(sample);
        }
        self.preview_mailbox.publish(sample.map(PreviewSample::new))
    }

    pub fn preview_playing(&self) -> bool {
        self.preview_playing.load(Ordering::Acquire)
    }

    pub fn request_transport(&self, mode: TransportMode, index: u8, row: u8, track: u8) -> bool {
        if track >= 8 {
            return false;
        }
        self.transport.request(TransportRequest {
            mode,
            index,
            row,
            track,
        });
        true
    }

    pub fn track_status(&self, track: u8) -> Option<ZtTrackStatus> {
        self.telemetry.read(track)
    }

    pub fn set_track_flags(&self, mute_mask: u8, solo_mask: u8) {
        self.track_flags.store(
            u16::from(mute_mask) | (u16::from(solo_mask) << 8),
            Ordering::Release,
        );
    }

    pub fn set_master_limiter_bypass(&self, bypass: bool) {
        self.master_limiter_bypass.store(bypass, Ordering::Release);
    }

    pub fn track_flags(&self) -> (u8, u8) {
        let flags = self.track_flags.load(Ordering::Acquire);
        (flags as u8, (flags >> 8) as u8)
    }

    fn render(&self, output: &mut [i16], channels: usize) {
        // SAFETY: the C API allows only one render caller at a time, and the
        // UI never accesses render_state directly.
        let state = unsafe { &mut *self.render_state.get() };
        self.mailbox.adopt(&mut state.active);
        self.preview_mailbox
            .adopt(&mut state.active_preview, &mut state.preview_position);
        // SAFETY: the audio thread exclusively owns active until it retires
        // it, and published snapshots are immutable.
        let snapshot = unsafe { state.active.as_ref() };
        if let Some(request) = self
            .transport
            .take_latest(&mut state.last_transport_sequence)
        {
            state.sequencer.transport(snapshot, request);
        }
        if state.test_tone {
            state.render_tone(output, channels);
        } else {
            state
                .sequencer
                .set_master_limiter_bypass(self.master_limiter_bypass.load(Ordering::Acquire));
            let (mute_mask, solo_mask) = self.track_flags();
            state.sequencer.render(
                snapshot,
                output,
                channels,
                &self.telemetry,
                &self.track_output_taps,
                (mute_mask, solo_mask),
            );
        }
        self.render_preview(state, output, channels);
        self.output_tap.publish_pcm(output, channels);
    }

    fn render_preview(&self, state: &mut RenderState, output: &mut [i16], channels: usize) {
        let Some(preview) = (unsafe { state.active_preview.as_ref() }) else {
            self.preview_playing.store(false, Ordering::Release);
            return;
        };
        let source_channels = usize::from(preview.sample.channels);
        let source_frames = preview.sample.frames();
        let step = f64::from(preview.sample.rate) / f64::from(self.sample_rate);
        let mut ended = false;
        for frame in output.chunks_exact_mut(channels) {
            let index = state.preview_position as usize;
            if index >= source_frames {
                ended = true;
                break;
            }
            let next = (index + 1).min(source_frames - 1);
            let fraction = (state.preview_position - index as f64) as f32;
            let left0 = preview.sample.pcm[index * source_channels];
            let right0 = if source_channels == 2 {
                preview.sample.pcm[index * 2 + 1]
            } else {
                left0
            };
            let left1 = preview.sample.pcm[next * source_channels];
            let right1 = if source_channels == 2 {
                preview.sample.pcm[next * 2 + 1]
            } else {
                left1
            };
            let left = left0 + (left1 - left0) * fraction;
            let right = right0 + (right1 - right0) * fraction;
            let scale = f32::from(i16::MAX);
            if channels == 1 {
                let mono = (left + right) * 0.5 * scale;
                frame[0] = (f32::from(frame[0]) + mono).clamp(-32768.0, 32767.0) as i16;
            } else {
                frame[0] = (f32::from(frame[0]) + left * scale).clamp(-32768.0, 32767.0) as i16;
                frame[1] = (f32::from(frame[1]) + right * scale).clamp(-32768.0, 32767.0) as i16;
            }
            state.preview_position += step;
        }
        if ended || state.preview_position >= source_frames as f64 {
            if self
                .preview_mailbox
                .retire_active(&mut state.active_preview)
            {
                state.preview_position = 0.0;
            } else {
                // Keep the cursor at EOF while the UI collects the previous
                // generation; otherwise a short sample would restart.
                state.preview_position = source_frames as f64;
            }
            self.preview_playing.store(false, Ordering::Release);
        } else {
            self.preview_playing.store(true, Ordering::Release);
        }
    }
}

impl Drop for ZtEngine {
    fn drop(&mut self) {
        // SAFETY: destroy happens after the audio callback stops, with
        // exclusive ownership. The active snapshot is one Box generation.
        let state = unsafe { &mut *self.render_state.get() };
        if !state.active.is_null() {
            unsafe { drop(Box::from_raw(state.active)) };
            state.active = ptr::null_mut();
        }
        if !state.active_preview.is_null() {
            unsafe { drop(Box::from_raw(state.active_preview)) };
            state.active_preview = ptr::null_mut();
        }
        self.mailbox.release_remaining();
        self.preview_mailbox.release_remaining();
    }
}

/// Create an engine. `test_tone=1` emits a quiet 440 Hz tone; zero is silent
/// until the song transport is implemented and started.
#[unsafe(no_mangle)]
pub extern "C" fn zt_engine_create(sample_rate: u32, test_tone: i32) -> *mut ZtEngine {
    ZtEngine::new(sample_rate, test_tone != 0)
        .map(|engine| Box::into_raw(Box::new(engine)))
        .unwrap_or(ptr::null_mut())
}

/// Destroy an engine after stopping and joining the audio callback.
///
/// # Safety
/// `ctx` must be null or a live pointer returned by `zt_engine_create`, and
/// no other thread may access it during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_destroy(ctx: *mut ZtEngine) {
    if !ctx.is_null() {
        // SAFETY: the caller owns the live pointer exclusively.
        drop(unsafe { Box::from_raw(ctx) });
    }
}

/// Request STOP, SONG, CHAIN, or PHRASE transport; the latest request wins.
/// Returns zero for invalid arguments.
///
/// # Safety
/// `ctx` must remain live during this call. It may render concurrently, but
/// it must not be destroyed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_transport(
    ctx: *mut ZtEngine,
    mode: i32,
    index: u8,
    row: u8,
    track: u8,
) -> i32 {
    let Some(engine) = (unsafe { ctx.as_ref() }) else {
        return 0;
    };
    let Some(mode) = TransportMode::from_i32(mode) else {
        return 0;
    };
    i32::from(engine.request_transport(mode, index, row, track))
}

/// Read one track's packed status without locking.
///
/// # Safety
/// `ctx` and `out` must be valid, nonoverlapping pointers. The engine must
/// not be destroyed during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_track_status(
    ctx: *const ZtEngine,
    track: u8,
    out: *mut ZtTrackStatus,
) -> i32 {
    let (Some(engine), Some(out)) = (unsafe { ctx.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    let Some(status) = engine.track_status(track) else {
        return 0;
    };
    *out = status;
    1
}

/// Atomically replace the live mute and solo masks. Bit 0 is track 1.
///
/// # Safety
/// `ctx` must remain live during this call and must not be destroyed
/// concurrently. Rendering on a different thread is allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_set_track_flags(
    ctx: *mut ZtEngine,
    mute_mask: u8,
    solo_mask: u8,
) -> i32 {
    let Some(engine) = (unsafe { ctx.as_ref() }) else {
        return 0;
    };
    engine.set_track_flags(mute_mask, solo_mask);
    1
}

/// Bypass the master limiter for stem renders that are exported pre-limiter.
///
/// # Safety
/// `ctx` must remain live during this call and must not be destroyed
/// concurrently. Rendering on a different thread is allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_set_master_limiter_bypass(
    ctx: *mut ZtEngine,
    bypass: i32,
) -> i32 {
    let Some(engine) = (unsafe { ctx.as_ref() }) else {
        return 0;
    };
    engine.set_master_limiter_bypass(bypass != 0);
    1
}

/// Read both live masks from one atomic value.
///
/// # Safety
/// `ctx`, `mute_mask`, and `solo_mask` must be valid; output pointers must
/// not overlap. The engine must not be destroyed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_get_track_flags(
    ctx: *const ZtEngine,
    mute_mask: *mut u8,
    solo_mask: *mut u8,
) -> i32 {
    if mute_mask.is_null() || solo_mask.is_null() || mute_mask == solo_mask {
        return 0;
    }
    let Some(engine) = (unsafe { ctx.as_ref() }) else {
        return 0;
    };
    let (mute, solo) = engine.track_flags();
    unsafe {
        *mute_mask = mute;
        *solo_mask = solo;
    }
    1
}

/// Fill interleaved signed 16-bit PCM without allocation or locking.
///
/// # Safety
/// `ctx` must point to a live engine and be accessed by only one render caller
/// at a time. `output` must point to `frames * channels` writable aligned
/// samples, with no overlap with `ctx`. Neither may be destroyed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_render(
    ctx: *mut ZtEngine,
    output: *mut i16,
    frames: usize,
    channels: u32,
) -> i32 {
    if ctx.is_null() || !(1..=2).contains(&channels) {
        return 0;
    }
    let Some(samples) = frames.checked_mul(channels as usize) else {
        return 0;
    };
    if samples > isize::MAX as usize / size_of::<i16>() {
        return 0;
    }
    if samples == 0 {
        return 1;
    }
    if output.is_null() {
        return 0;
    }
    // SAFETY: the caller guarantees a writable buffer and exclusive render.
    let buffer = unsafe { std::slice::from_raw_parts_mut(output, samples) };
    let engine = unsafe { &*ctx };
    engine.render(buffer, channels as usize);
    1
}

/// Copy the latest final master-output frames as normalized mono samples.
///
/// # Safety
/// `ctx` must point to a live engine and must not be destroyed concurrently.
/// `out` must point to `frames` writable aligned floats, with no overlap with
/// `ctx`. A failed copy may have partially modified `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_read_output(
    ctx: *const ZtEngine,
    out: *mut f32,
    frames: usize,
) -> usize {
    if ctx.is_null()
        || out.is_null()
        || frames == 0
        || frames > OUTPUT_WINDOW_FRAMES
        || frames > isize::MAX as usize / size_of::<f32>()
    {
        return 0;
    }
    let engine = unsafe { &*ctx };
    let output = unsafe { std::slice::from_raw_parts_mut(out, frames) };
    if engine.output_tap.read(output) {
        frames
    } else {
        0
    }
}

/// Copy post-track-fader mono waveform samples for one track, oldest first.
/// The output is normalized to approximately -1..1. `frames` must be
/// 1..=ZT_TRACK_OUTPUT_WINDOW_FRAMES; returns the frame count or zero if the
/// arguments are invalid, history is insufficient, or the writer overtakes
/// the copy.
///
/// # Safety
/// `ctx` must point to a live engine and must not be destroyed concurrently.
/// `out` must point to `frames` writable aligned floats and not overlap `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_read_track_output(
    ctx: *const ZtEngine,
    track: u8,
    out: *mut f32,
    frames: usize,
) -> usize {
    if ctx.is_null()
        || out.is_null()
        || frames == 0
        || frames > TRACK_OUTPUT_WINDOW_FRAMES
        || frames > isize::MAX as usize / size_of::<f32>()
    {
        return 0;
    }
    let engine = unsafe { &*ctx };
    let output = unsafe { std::slice::from_raw_parts_mut(out, frames) };
    if engine.track_output_taps.read_track(track, output) {
        frames
    } else {
        0
    }
}

/// Read 18 peak values as interleaved L/R pairs: tracks 0..7, then master.
/// Each value is linear 0..65535. Track peaks are post-track-fader; master
/// peaks are post-master-fader and post-effects. Returns 1 on success.
///
/// # Safety
/// `ctx` and `out` must be valid and not overlap. The engine must not be
/// destroyed during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_track_peaks(ctx: *const ZtEngine, out: *mut u16) -> i32 {
    let (Some(engine), Some(out)) = (unsafe { ctx.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    // SAFETY: the C contract requires 18 writable u16 values.
    let output = unsafe { std::slice::from_raw_parts_mut(out, 18) };
    let mut peaks = [0_u16; 18];
    engine.track_output_taps.read_peaks(&mut peaks);
    output.copy_from_slice(&peaks);
    1
}

/// Read eight stereo peak pairs before MUTE/SOLO, after each track's volume.
/// Values are linear 0..65535, interleaved L/R for tracks 0..7. Returns 1 on
/// success, 0 for invalid arguments.
///
/// # Safety
/// `ctx` and `out` must be valid and not overlap. `out` must point to 16
/// writable `u16` values. The engine must not be destroyed during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_engine_track_activity(ctx: *const ZtEngine, out: *mut u16) -> i32 {
    let (Some(engine), Some(out)) = (unsafe { ctx.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    // SAFETY: the C contract requires 16 writable u16 values.
    let output = unsafe { std::slice::from_raw_parts_mut(out, 16) };
    let mut activity = [0_u16; 16];
    engine.track_output_taps.read_activity(&mut activity);
    output.copy_from_slice(&activity);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn silent_mode_and_invalid_arguments() {
        let ctx = zt_engine_create(44_100, 0);
        assert!(!ctx.is_null());
        let mut output = [123_i16; 32];
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 16, 2) },
            1
        );
        assert!(output.iter().all(|&sample| sample == 0));
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 16, 3) },
            0
        );
        assert_eq!(unsafe { zt_engine_render(ctx, ptr::null_mut(), 16, 2) }, 0);
        assert_eq!(unsafe { zt_engine_transport(ctx, 4, 0, 0, 0) }, 0);
        unsafe { zt_engine_destroy(ctx) };
        assert!(zt_engine_create(0, 1).is_null());
    }

    #[test]
    fn preview_resamples_and_plays_without_a_song_then_retires_on_ui_thread() {
        let ctx = zt_engine_create(8_000, 0);
        let engine = unsafe { &*ctx };
        let sample = Sample {
            rate: 4_000,
            channels: 1,
            pcm: Arc::from([0.0_f32, 0.5, 1.0]),
        };
        assert!(engine.set_preview(Some(sample)).is_ok());
        let mut output = [0_i16; 8];
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 4, 2) },
            1
        );
        assert!(engine.preview_playing());
        assert_eq!(output, [0, 0, 8191, 8191, 16383, 16383, 24575, 24575]);
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 1, 2) },
            1
        );
        assert!(engine.preview_playing());
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 1, 2) },
            1
        );
        assert!(!engine.preview_playing());
        // The retirement slot is still occupied by the initial adoption.
        // Rendering again at EOF must stay silent rather than restart.
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 2, 2) },
            1
        );
        assert!(output[..4].iter().all(|&value| value == 0));
        // The render callback retires the sample; UI collection frees it.
        assert!(engine.collect_retired().is_none());
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 2, 2) },
            1
        );
        assert!(output[..4].iter().all(|&value| value == 0));
        unsafe { zt_engine_destroy(ctx) };
    }

    #[test]
    fn preview_stop_is_published_and_reclaimed_off_audio_thread() {
        let ctx = zt_engine_create(8_000, 0);
        let engine = unsafe { &*ctx };
        assert!(
            engine
                .set_preview(Some(Sample {
                    rate: 8_000,
                    channels: 2,
                    pcm: Arc::from([0.25_f32, -0.25_f32].repeat(32)),
                }))
                .is_ok()
        );
        let mut output = [0_i16; 4];
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 1, 2) },
            1
        );
        assert!(engine.preview_playing());
        assert!(engine.collect_retired().is_none());
        assert!(engine.set_preview(None).is_ok());
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 1, 2) },
            1
        );
        assert!(!engine.preview_playing());
        assert!(engine.collect_retired().is_none());
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 1, 2) },
            1
        );
        assert!(output.iter().all(|&value| value == 0));
        unsafe { zt_engine_destroy(ctx) };
    }

    #[test]
    fn test_tone_is_bounded_and_continuous_across_callbacks() {
        let ctx = zt_engine_create(44_100, 1);
        assert!(!ctx.is_null());
        let mut output = vec![0_i16; 44_100 * 2];
        let split = 1_000 * 2;
        assert_eq!(
            unsafe { zt_engine_render(ctx, output.as_mut_ptr(), 1_000, 2) },
            1
        );
        assert_eq!(
            unsafe { zt_engine_render(ctx, output[split..].as_mut_ptr(), 43_100, 2) },
            1
        );
        unsafe { zt_engine_destroy(ctx) };
        assert!(output.iter().any(|&sample| sample != 0));
        assert!(output.iter().all(|&sample| sample.unsigned_abs() <= 1_639));
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[0] == frame[1])
        );
        let rising_zero_crossings = output
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|pair| pair[0] <= 0 && pair[1] > 0)
            .count();
        assert_eq!(rising_zero_crossings, 440);
    }

    #[test]
    fn output_tap_matches_the_final_pcm_across_uneven_blocks() {
        let ctx = zt_engine_create(44_100, 1);
        let mut pcm = vec![0_i16; 3001 * 2];
        let mut window = [0.0_f32; OUTPUT_WINDOW_FRAMES];
        assert_eq!(
            unsafe { zt_engine_read_output(ctx, window.as_mut_ptr(), 2048) },
            0
        );
        assert_eq!(
            unsafe { zt_engine_render(ctx, pcm.as_mut_ptr(), 997, 2) },
            1
        );
        assert_eq!(
            unsafe { zt_engine_render(ctx, pcm[997 * 2..].as_mut_ptr(), 2004, 2) },
            1
        );
        assert_eq!(
            unsafe { zt_engine_read_output(ctx, window.as_mut_ptr(), 2048) },
            2048
        );
        for (got, frame) in window.iter().zip(pcm.as_chunks::<2>().0[953..].iter()) {
            assert_eq!(*got, f32::from(frame[0]) / 32768.0);
        }
        assert_eq!(
            unsafe { zt_engine_read_output(ctx, window.as_mut_ptr(), 0) },
            0
        );
        assert_eq!(
            unsafe { zt_engine_read_output(ctx, window.as_mut_ptr(), 2049) },
            0
        );
        assert_eq!(unsafe { zt_engine_read_output(ctx, ptr::null_mut(), 1) }, 0);
        assert_eq!(
            unsafe { zt_engine_read_output(ptr::null(), window.as_mut_ptr(), 1) },
            0
        );
        unsafe { zt_engine_destroy(ctx) };
    }

    #[test]
    fn track_taps_reject_invalid_arguments_and_missing_history() {
        let ctx = zt_engine_create(44_100, 0);
        let mut waveform = [0.0_f32; 32];
        let mut peaks = [0_u16; 18];
        let mut activity = [0_u16; 16];
        assert_eq!(
            unsafe { zt_engine_read_track_output(ctx, 0, waveform.as_mut_ptr(), 32) },
            0
        );
        assert_eq!(
            unsafe { zt_engine_read_track_output(ctx, 8, waveform.as_mut_ptr(), 32) },
            0
        );
        assert_eq!(
            unsafe { zt_engine_read_track_output(ctx, 0, waveform.as_mut_ptr(), 513) },
            0
        );
        assert_eq!(
            unsafe { zt_engine_read_track_output(ctx, 0, ptr::null_mut(), 32) },
            0
        );
        assert_eq!(unsafe { zt_engine_track_peaks(ctx, peaks.as_mut_ptr()) }, 1);
        assert_eq!(peaks, [0; 18]);
        assert_eq!(unsafe { zt_engine_track_peaks(ctx, ptr::null_mut()) }, 0);
        assert_eq!(
            unsafe { zt_engine_track_activity(ctx, activity.as_mut_ptr()) },
            1
        );
        assert_eq!(activity, [0; 16]);
        assert_eq!(unsafe { zt_engine_track_activity(ctx, ptr::null_mut()) }, 0);
        unsafe { zt_engine_destroy(ctx) };
    }

    #[test]
    fn output_tap_concurrent_copies_are_contiguous() {
        let tap = Arc::new(OutputTap::new());
        let writer = Arc::clone(&tap);
        let thread = std::thread::spawn(move || {
            let mut pcm = [0_i16; 257];
            let mut next = 0;
            for block in 0..400 {
                let len = if block % 2 == 0 { 257 } else { 113 };
                for sample in &mut pcm[..len] {
                    *sample = (next % 30_000) as i16;
                    next += 1;
                }
                writer.publish_pcm(&pcm[..len], 1);
            }
        });
        let mut window = [0.0_f32; OUTPUT_WINDOW_FRAMES];
        for _ in 0..600 {
            if tap.read(&mut window) {
                for pair in window.windows(2) {
                    let left = (pair[0] * 32768.0).round() as i32;
                    let right = (pair[1] * 32768.0).round() as i32;
                    assert_eq!(right, (left + 1) % 30_000);
                }
            }
        }
        thread.join().unwrap();
        assert!(tap.read(&mut window));
        for pair in window.windows(2) {
            let left = (pair[0] * 32768.0).round() as i32;
            let right = (pair[1] * 32768.0).round() as i32;
            assert_eq!(right, (left + 1) % 30_000);
        }
    }
}
