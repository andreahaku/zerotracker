//! RENDER on the SONG page: the song to WAV files, the
//! mix and one stem per track in use, laid out as the M8 renders them:
//! `Renders/<SONG>/<SONG>.wav` and `Renders/<SONG>/STEMS/<SONG>_<n>.wav`,
//! 44.1 kHz, 16-bit stereo. A thread of its own renders with an engine of
//! its own, so playback and the UI go on.

use std::ffi::{CStr, c_char};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::thread::JoinHandle;

const SAMPLE_RATE: u32 = 44_100;
const BLOCK_FRAMES: usize = 256;
/// Effects tails after the last note.
const TAIL_FRAMES: usize = SAMPLE_RATE as usize * 2;
/// A song that loops forever stops here.
const MAX_FRAMES: usize = SAMPLE_RATE as usize * 60 * 10;

/// What RENDER writes: the mix, the stems, or both.
pub const WRITE_MIX: u8 = 1;
pub const WRITE_STEMS: u8 = 2;

pub const STATE_RUNNING: u8 = 1;
pub const STATE_DONE: u8 = 2;
pub const STATE_FAILED: u8 = 3;
pub const STATE_CANCELLED: u8 = 4;

/// What a pass plays: the song and its samples; each pass builds its own
/// snapshot (the engine takes ownership of it).
struct Source {
    song: m8fmt::Song,
    samples: Vec<Option<engine::Sample>>,
    cues: Vec<Arc<[u32]>>,
}

impl Source {
    fn snapshot(&self) -> Option<engine::PlaybackSnapshot> {
        engine::PlaybackSnapshot::new(self.song.clone(), self.samples.clone())
            .and_then(|s| s.with_sample_cues(self.cues.clone()))
    }
}

struct Shared {
    state: AtomicU8,
    /// 0 while the mix renders, then 1 + the stems finished; and how many
    /// passes in all (the mix and the stems).
    pass: AtomicU8,
    passes: AtomicU8,
    /// Progress of the mix, 0..1000 (against the length guessed).
    permille: AtomicU32,
    /// The mix's length, and the frames the stems have rendered so far.
    frames: AtomicUsize,
    stem_frames: AtomicUsize,
    cancel: AtomicBool,
}

/// Stems render this many at a time: the Cardputer has four cores, one is
/// left to playback and the UI.
const STEM_THREADS: usize = 3;

impl Shared {
    /// Progress over every pass, 0..1000: the mix against the length
    /// guessed, then the stems' frames against the mix's length.
    fn progress(&self) -> u32 {
        let passes = usize::from(self.passes.load(Ordering::Acquire)).max(1);
        let frames = self.frames.load(Ordering::Acquire);
        if frames == 0 {
            return self.permille.load(Ordering::Relaxed) / passes as u32;
        }
        let done = frames + self.stem_frames.load(Ordering::Relaxed);
        ((done * 1000) / (frames * passes)).min(1000) as u32
    }
}

pub struct Render {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Render {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The tracks a song uses: those with a chain on any row.
fn used_tracks(song: &m8fmt::Song) -> Vec<u8> {
    (0..8u8)
        .filter(|&t| {
            song.song
                .steps
                .chunks(8)
                .any(|row| row[usize::from(t)] != 0xFF)
        })
        .collect()
}

/// A guess of the song's length in frames, for the mix pass's progress: per
/// song row the longest chain on it, 16 steps a phrase, a step a sixteenth
/// at the song's tempo (grooves and FX that change the timing are ignored).
fn estimated_frames(song: &m8fmt::Song) -> usize {
    let step_seconds = 60.0 / (f64::from(song.tempo).max(1.0) * 4.0);
    let mut steps = 0usize;
    for row in song.song.steps.chunks(8) {
        if row.iter().all(|&c| c == 0xFF) {
            break;
        }
        let longest = row
            .iter()
            .filter(|&&c| c != 0xFF)
            .map(|&c| {
                song.chains[usize::from(c)]
                    .steps
                    .iter()
                    .filter(|s| s.phrase != 0xFF)
                    .count()
            })
            .max()
            .unwrap_or(0);
        steps += longest * 16;
    }
    ((steps as f64 * step_seconds * f64::from(SAMPLE_RATE)) as usize)
        .clamp(SAMPLE_RATE as usize, MAX_FRAMES)
}

/// Renders the snapshot with `solo` (0: every track) into `path`. The first
/// pass decides the length (`frames` None): until every track has stopped or
/// the song loops, then the effects' tail; the stems take the same length.
fn render_pass(
    source: &Source,
    solo: u8,
    frames: Option<usize>,
    path: Option<&Path>,
    shared: &Shared,
) -> Result<usize, String> {
    let ctx = engine::zt_engine_create(SAMPLE_RATE, 0);
    if ctx.is_null() {
        return Err("engine".into());
    }
    // SAFETY: this thread owns the engine and destroys it below.
    let engine = unsafe { &*ctx };
    let result = (|| {
        engine.set_master_limiter_bypass(solo != 0);
        let snapshot = source.snapshot().ok_or("snapshot")?;
        engine
            .publish(Box::new(snapshot))
            .map_err(|_| "snapshot".to_string())?;
        engine.set_track_flags(0, solo);
        engine.request_transport(engine::TransportMode::Song, 0, 0, 0);
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        // No path: the mix is played only to find the song's length (STEMS
        // alone).
        let mut wav = match path {
            Some(path) => Some(hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?),
            None => None,
        };
        let mut pcm = [0_i16; BLOCK_FRAMES * 2];
        let mut done = 0usize;
        let mut stopped_at: Option<usize> = None;
        // Song row, chain row and phrase step of each track, as one number.
        let mut last_position = [0u32; 8];
        let target = frames.unwrap_or(MAX_FRAMES);
        let estimate = estimated_frames(&source.song);
        while done < target {
            if shared.cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            let n = BLOCK_FRAMES.min(target - done);
            // SAFETY: `ctx` is live and only this thread renders it.
            if unsafe { engine::zt_engine_render(ctx, pcm.as_mut_ptr(), n, 2) } != 1 {
                return Err("render".into());
            }
            if let Some(wav) = wav.as_mut() {
                for &s in &pcm[..n * 2] {
                    wav.write_sample(s).map_err(|e| e.to_string())?;
                }
            }
            done += n;
            if frames.is_none() {
                // The end: every track stopped, or a track's position going
                // back (the song loops); then the tail.
                if stopped_at.is_none() {
                    let mut active = false;
                    let mut looped = false;
                    for t in 0..8u8 {
                        if let Some(s) = engine.track_status(t).filter(|s| s.active != 0) {
                            active = true;
                            let position = u32::from(s.song_row) << 16
                                | u32::from(s.chain_row) << 8
                                | u32::from(s.phrase_step);
                            looped |= position < last_position[usize::from(t)];
                            last_position[usize::from(t)] = position;
                        }
                    }
                    if !active || looped {
                        engine.request_transport(engine::TransportMode::Stop, 0, 0, 0);
                        stopped_at = Some(done);
                    }
                }
                if let Some(at) = stopped_at
                    && done >= at + TAIL_FRAMES
                {
                    break;
                }
                shared
                    .permille
                    .store((done * 1000 / estimate).min(999) as u32, Ordering::Relaxed);
            } else {
                shared.stem_frames.fetch_add(n, Ordering::Relaxed);
            }
        }
        if let Some(wav) = wav {
            wav.finalize().map_err(|e| e.to_string())?;
        }
        Ok(done)
    })();
    // SAFETY: created above, no other user.
    unsafe { engine::zt_engine_destroy(ctx) };
    result
}

fn run(source: Source, tracks: Vec<u8>, dir: PathBuf, name: String, write: u8, shared: &Shared) {
    // Only the mix: no stems.
    let tracks = if write & WRITE_STEMS != 0 {
        tracks
    } else {
        Vec::new()
    };
    let stems = dir.join("STEMS");
    let folder = if tracks.is_empty() { &dir } else { &stems };
    if let Err(e) = std::fs::create_dir_all(folder) {
        eprintln!("[render] {}: {e}", folder.display());
        shared.state.store(STATE_FAILED, Ordering::Release);
        return;
    }
    shared
        .passes
        .store(1 + tracks.len() as u8, Ordering::Release);
    let mix = dir.join(format!("{name}.wav"));
    let mix_path = (write & WRITE_MIX != 0).then_some(mix.as_path());
    let frames = match render_pass(&source, 0, None, mix_path, shared) {
        Ok(frames) => frames,
        Err(e) => {
            eprintln!("[render] {}: {e}", mix.display());
            let cancelled = shared.cancel.load(Ordering::Acquire);
            shared.state.store(
                if cancelled {
                    STATE_CANCELLED
                } else {
                    STATE_FAILED
                },
                Ordering::Release,
            );
            return;
        }
    };
    shared.frames.store(frames, Ordering::Release);
    shared.pass.store(1, Ordering::Release);
    // The stems, STEM_THREADS at a time, each taking the next track.
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..STEM_THREADS.min(tracks.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::AcqRel);
                    let Some(&t) = tracks.get(i) else { break };
                    if failed.load(Ordering::Acquire) {
                        break;
                    }
                    let path = stems.join(format!("{name}_{}.wav", t + 1));
                    match render_pass(&source, 1 << t, Some(frames), Some(&path), shared) {
                        Ok(_) => {
                            shared.pass.fetch_add(1, Ordering::AcqRel);
                        }
                        Err(e) => {
                            eprintln!("[render] {}: {e}", path.display());
                            failed.store(true, Ordering::Release);
                        }
                    }
                }
            });
        }
    });
    let state = if !failed.load(Ordering::Acquire) {
        STATE_DONE
    } else if shared.cancel.load(Ordering::Acquire) {
        STATE_CANCELLED
    } else {
        STATE_FAILED
    };
    shared.state.store(state, Ordering::Release);
}

impl super::Viewer {
    /// Starts rendering the song as it is now into `<root>/Renders/<NAME>/`.
    pub fn start_render(&self, root: &Path, write: u8) -> Option<Render> {
        if write & (WRITE_MIX | WRITE_STEMS) == 0 {
            return None;
        }
        // One slot per instrument, loaded or not (no engine attached: none).
        let slots = self.song().instruments.len();
        let mut samples = self.render_samples();
        samples.resize(slots, None);
        let mut cues = self.bank.cues();
        cues.resize(slots, Arc::from([]));
        let source = Source {
            song: self.song().clone(),
            samples,
            cues,
        };
        source.snapshot()?;
        let name = self
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_uppercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "SONG".into());
        let dir = root.join("Renders").join(&name);
        let tracks = if write & WRITE_STEMS != 0 {
            used_tracks(self.song())
        } else {
            Vec::new()
        };
        let shared = Arc::new(Shared {
            state: AtomicU8::new(STATE_RUNNING),
            pass: AtomicU8::new(0),
            passes: AtomicU8::new(1 + tracks.len() as u8),
            permille: AtomicU32::new(0),
            frames: AtomicUsize::new(0),
            stem_frames: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("zt-render".into())
            .spawn(move || run(source, tracks, dir, name, write, &worker))
            .ok()?;
        Some(Render {
            shared,
            thread: Some(thread),
        })
    }
}

/// Starts RENDER of the viewer's song under `root` (the songs folder):
/// `write` is WRITE_MIX, WRITE_STEMS or both. NULL when there is nothing to
/// render.
///
/// # Safety
///
/// `viewer` must be live or NULL; `root` a NUL-terminated path or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_render_start(
    viewer: *const super::Viewer,
    root: *const c_char,
    write: u8,
) -> *mut Render {
    // SAFETY: the caller passes a live viewer and a C string, or NULL.
    let (Some(v), false) = (unsafe { viewer.as_ref() }, root.is_null()) else {
        return std::ptr::null_mut();
    };
    // SAFETY: checked non-null above.
    let root = unsafe { CStr::from_ptr(root) }
        .to_string_lossy()
        .into_owned();
    v.start_render(Path::new(&root), write)
        .map_or(std::ptr::null_mut(), |r| Box::into_raw(Box::new(r)))
}

/// The render's state (ZT_RENDER_*), passes finished (0 while the mix
/// renders), number of passes and the progress over all of them in
/// thousandths.
///
/// # Safety
///
/// `render` must be live or NULL; the out pointers writable or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_render_status(
    render: *const Render,
    pass: *mut u8,
    passes: *mut u8,
    permille: *mut u32,
) -> u8 {
    // SAFETY: the caller passes a live render or NULL.
    let Some(r) = (unsafe { render.as_ref() }) else {
        return 0;
    };
    // SAFETY: each pointer is writable or NULL.
    unsafe {
        if let Some(p) = pass.as_mut() {
            *p = r.shared.pass.load(Ordering::Acquire);
        }
        if let Some(p) = passes.as_mut() {
            *p = r.shared.passes.load(Ordering::Acquire);
        }
        if let Some(p) = permille.as_mut() {
            *p = r.shared.progress();
        }
    }
    r.shared.state.load(Ordering::Acquire)
}

/// Cancels a running render, waits for its thread and frees it.
///
/// # Safety
///
/// `render` must come from `zt_render_start` (or be NULL) and not be used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_render_free(render: *mut Render) {
    if !render.is_null() {
        // SAFETY: from Box::into_raw in zt_render_start.
        drop(unsafe { Box::from_raw(render) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A short song renders the mix and its stems, the same length, and stops
    // at its end.
    #[test]
    fn renders_mix_and_stems() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let v = super::super::Viewer::open(p).unwrap();
        let root = std::env::temp_dir().join(format!("zt-render-{}", std::process::id()));
        let r = v
            .start_render(&root, WRITE_MIX | WRITE_STEMS)
            .expect("render starts");
        let mut state = STATE_RUNNING;
        for _ in 0..6000 {
            // SAFETY: `r` is live.
            state = unsafe {
                zt_render_status(
                    &r,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if state != STATE_RUNNING {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(state, STATE_DONE);
        let dir = root.join("Renders/TEST-FILE");
        let mix = hound::WavReader::open(dir.join("TEST-FILE.wav")).unwrap();
        assert_eq!(
            (mix.spec().channels, mix.spec().sample_rate),
            (2, SAMPLE_RATE)
        );
        let stems: Vec<_> = std::fs::read_dir(dir.join("STEMS")).unwrap().collect();
        assert_eq!(stems.len(), used_tracks(v.song()).len());
        for stem in stems {
            let w = hound::WavReader::open(stem.unwrap().path()).unwrap();
            assert_eq!(w.duration(), mix.duration());
        }
        assert!(mix.duration() < MAX_FRAMES as u32, "the song ends");
        let _ = std::fs::remove_dir_all(&root);
    }
}
