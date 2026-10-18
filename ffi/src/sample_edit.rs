//! The sample editor: one .wav in memory, a selection,
//! a loop region and slice markers, the M8's processes (crop, delete,
//! duplicate, normalize, silence, reverse, invert, fades, crossfade loop,
//! mono, downsample, bit depth, slicing), undo, preview and save.
//!
//! Slice markers live in the WAV `cue ` chunk (as the M8 reads them for
//! SLICE 01 FILE), the loop region in the `smpl` chunk. Runs on the UI
//! thread; the PCM is f32 interleaved, -1.0..1.0.

use std::ffi::{CStr, c_char};
use std::path::{Path, PathBuf};

/// Processes, as the app numbers them (`zt_sample_process`).
pub const OP_CROP: i32 = 0;
pub const OP_DELETE: i32 = 1;
pub const OP_DUPLICATE: i32 = 2;
pub const OP_NORMALIZE: i32 = 3;
pub const OP_SILENCE: i32 = 4;
pub const OP_REVERSE: i32 = 5;
pub const OP_INVERT: i32 = 6;
pub const OP_FADE_IN: i32 = 7;
pub const OP_FADE_OUT: i32 = 8;
pub const OP_XFADE_LOOP: i32 = 9;
pub const OP_MONO_MIX: i32 = 10;
pub const OP_MONO_LEFT: i32 = 11;
pub const OP_MONO_RIGHT: i32 = 12;
pub const OP_DOWNSAMPLE: i32 = 13;
pub const OP_BITS_16: i32 = 14;
pub const OP_BITS_8: i32 = 15;
/// `param` = number of equal slices (2-128).
pub const OP_SLICE_EVEN: i32 = 16;
pub const OP_SLICE_AUTO: i32 = 17;
pub const OP_SLICE_SILENCE: i32 = 18;
pub const OP_CLEAR_SLICES: i32 = 19;

const MAX_SLICES: usize = 128;
/// Gain of the preview voice (-6 dB).
pub(crate) const PREVIEW_GAIN: f32 = 0.5;
/// Undo history stops growing past this many bytes of PCM.
const UNDO_BUDGET: usize = 48 << 20;

#[derive(Clone)]
struct State {
    pcm: Vec<f32>,
    rate: u32,
    channels: u16,
    bits: u16,
    slices: Vec<u32>,
    loop_region: Option<(u32, u32)>,
    selection: (u32, u32),
}

pub struct SampleEdit {
    path: PathBuf,
    s: State,
    undo: Vec<State>,
    redo: Vec<State>,
    dirty: bool,
}

/// Plain numbers for the app (`zt_sample_info`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SampleInfo {
    pub frames: u32,
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub sel_start: u32,
    pub sel_end: u32,
    /// Loop region, or start = end = u32::MAX when there is none.
    pub loop_start: u32,
    pub loop_end: u32,
    pub slices: u32,
    pub dirty: u8,
    pub can_undo: u8,
    pub can_redo: u8,
}

impl State {
    fn frames(&self) -> usize {
        self.pcm.len() / usize::from(self.channels.max(1))
    }

    fn bytes(&self) -> usize {
        self.pcm.len() * 4
    }
}

impl SampleEdit {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.channels > 2 {
            return Err(format!("{} channels", spec.channels));
        }
        let pcm: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<_, _>>()
                    .map_err(|e| e.to_string())?
            }
        };
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let frames = (pcm.len() / usize::from(spec.channels)) as u32;
        let mut slices = chunk(&data, b"cue ")
            .map(read_cue_points)
            .unwrap_or_default();
        slices.retain(|&c| c < frames);
        let loop_region = chunk(&data, b"smpl")
            .and_then(read_smpl_loop)
            .filter(|&(a, b)| a < b && b <= frames);
        let bits = match spec.sample_format {
            hound::SampleFormat::Int => spec.bits_per_sample.clamp(8, 24),
            hound::SampleFormat::Float => 16,
        };
        Ok(Self {
            path: path.to_path_buf(),
            s: State {
                pcm,
                rate: spec.sample_rate,
                channels: spec.channels,
                bits,
                slices,
                loop_region,
                selection: (0, frames),
            },
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        })
    }

    pub fn info(&self) -> SampleInfo {
        let (ls, le) = self.s.loop_region.unwrap_or((u32::MAX, u32::MAX));
        SampleInfo {
            frames: self.s.frames() as u32,
            rate: self.s.rate,
            channels: self.s.channels,
            bits: self.s.bits,
            sel_start: self.s.selection.0,
            sel_end: self.s.selection.1,
            loop_start: ls,
            loop_end: le,
            slices: self.s.slices.len() as u32,
            dirty: u8::from(self.dirty),
            can_undo: u8::from(!self.undo.is_empty()),
            can_redo: u8::from(!self.redo.is_empty()),
        }
    }

    pub fn slices(&self) -> &[u32] {
        &self.s.slices
    }

    fn frames(&self) -> u32 {
        self.s.frames() as u32
    }

    /// The range a process works on: the selection, or the whole sample.
    fn range(&self) -> (usize, usize) {
        let n = self.s.frames();
        let (a, b) = (self.s.selection.0 as usize, self.s.selection.1 as usize);
        let (a, b) = (a.min(n), b.min(n));
        if a < b { (a, b) } else { (0, n) }
    }

    pub fn set_selection(&mut self, a: u32, b: u32) {
        let n = self.frames();
        let (a, b) = (a.min(n), b.min(n));
        self.s.selection = (a.min(b), a.max(b));
    }

    /// Marker edits are undoable, as the processes are.
    fn checkpoint(&mut self) {
        self.undo.push(self.s.clone());
        self.redo.clear();
        let mut total: usize = self.undo.iter().map(State::bytes).sum();
        while total > UNDO_BUDGET && self.undo.len() > 1 {
            total -= self.undo.remove(0).bytes();
        }
        self.dirty = true;
    }

    pub fn set_loop(&mut self, region: Option<(u32, u32)>) {
        let n = self.frames();
        let region = region
            .map(|(a, b)| (a.min(b).min(n), a.max(b).min(n)))
            .filter(|(a, b)| a < b);
        // Moving a loop marker is not an undo step: each step would copy
        // the whole PCM. The edit still marks the file as changed.
        if region != self.s.loop_region {
            self.s.loop_region = region;
            self.dirty = true;
        }
    }

    pub fn add_slice(&mut self, at: u32) {
        if self.s.slices.len() >= MAX_SLICES || at >= self.frames() || self.s.slices.contains(&at) {
            return;
        }
        self.checkpoint();
        self.s.slices.push(at);
        self.s.slices.sort_unstable();
    }

    /// Moves slice `index` to `at` (kept between its neighbours). Not an
    /// undo step by itself: `begin_edit` makes one before a run of moves.
    pub fn move_slice(&mut self, index: usize, at: u32) {
        let Some(&cur) = self.s.slices.get(index) else {
            return;
        };
        let lo = if index > 0 {
            self.s.slices[index - 1] + 1
        } else {
            0
        };
        let hi = self
            .s
            .slices
            .get(index + 1)
            .map_or(self.frames(), |&n| n.saturating_sub(1));
        let at = at.clamp(lo, hi.max(lo));
        if at != cur {
            self.s.slices[index] = at;
            self.dirty = true;
        }
    }

    /// One undo step for the marker moves that follow (a grabbed slice).
    pub fn begin_edit(&mut self) {
        self.checkpoint();
    }

    pub fn remove_slice(&mut self, index: usize) {
        if index < self.s.slices.len() {
            self.checkpoint();
            self.s.slices.remove(index);
        }
    }

    pub fn undo(&mut self, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(state) = from.pop() else {
            return false;
        };
        to.push(std::mem::replace(&mut self.s, state));
        self.dirty = true;
        true
    }

    /// Min and max of the mono mix per column, frames [from, to).
    pub fn peaks(&self, from: u32, to: u32, columns: usize) -> Vec<(f32, f32)> {
        let n = self.s.frames();
        let ch = usize::from(self.s.channels.max(1));
        let (from, to) = (from as usize, (to as usize).min(n));
        let mut out = vec![(0.0, 0.0); columns];
        if from >= to || columns == 0 {
            return out;
        }
        let span = (to - from) as f64 / columns as f64;
        for (c, slot) in out.iter_mut().enumerate() {
            let a = from + (c as f64 * span) as usize;
            let b = (from + ((c + 1) as f64 * span) as usize).clamp(a + 1, to);
            // Long spans: every frame would cost too much on the device; a
            // stride of up to 64 points per column keeps the shape.
            let step = ((b - a) / 64).max(1);
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            let mut f = a;
            while f < b {
                let v = self.s.pcm[f * ch..f * ch + ch].iter().sum::<f32>() / ch as f32;
                lo = lo.min(v);
                hi = hi.max(v);
                f += step;
            }
            *slot = (lo, hi);
        }
        out
    }

    /// The frames [from, to) as an engine sample, for the preview voice.
    pub fn preview_sample(&self, from: u32, to: u32) -> Option<engine::Sample> {
        let n = self.s.frames();
        let ch = usize::from(self.s.channels.max(1));
        let (a, b) = ((from as usize).min(n), (to as usize).min(n));
        let (a, b) = if a < b { (a, b) } else { (0, n) };
        if a >= b {
            return None;
        }
        // The preview has no mixer: -6 dB, about a track at its usual
        // volume, so a normalized sample does not clip the speaker.
        let pcm: Vec<f32> = self.s.pcm[a * ch..b * ch]
            .iter()
            .map(|v| v * PREVIEW_GAIN)
            .collect();
        Some(engine::Sample {
            rate: self.s.rate,
            channels: self.s.channels as u8,
            pcm: pcm.into(),
        })
    }

    pub fn process(&mut self, op: i32, param: i32) -> Result<(), String> {
        let ch = usize::from(self.s.channels.max(1));
        let (a, b) = self.range();
        if a >= b && !matches!(op, OP_CLEAR_SLICES) {
            return Err("EMPTY".into());
        }
        self.checkpoint();
        let s = &mut self.s;
        let len = b - a;
        match op {
            OP_CROP => {
                s.pcm = s.pcm[a * ch..b * ch].to_vec();
                shift_markers(s, |f| (f >= a && f < b).then(|| f - a));
                s.selection = (0, len as u32);
            }
            OP_DELETE => {
                s.pcm.drain(a * ch..b * ch);
                shift_markers(s, |f| {
                    if f < a {
                        Some(f)
                    } else if f >= b {
                        Some(f - len)
                    } else {
                        None
                    }
                });
                s.selection = (a as u32, a as u32);
            }
            OP_DUPLICATE => {
                let copy = s.pcm[a * ch..b * ch].to_vec();
                s.pcm.splice(b * ch..b * ch, copy);
                shift_markers(s, |f| Some(if f >= b { f + len } else { f }));
                s.selection = (b as u32, (b + len) as u32);
            }
            OP_NORMALIZE => {
                let peak = s.pcm[a * ch..b * ch]
                    .iter()
                    .fold(0.0f32, |m, v| m.max(v.abs()));
                if peak > 0.0 {
                    let gain = 0.999 / peak;
                    s.pcm[a * ch..b * ch].iter_mut().for_each(|v| *v *= gain);
                }
            }
            OP_SILENCE => s.pcm[a * ch..b * ch].iter_mut().for_each(|v| *v = 0.0),
            OP_REVERSE => {
                let frames: Vec<Vec<f32>> = s.pcm[a * ch..b * ch]
                    .chunks(ch)
                    .rev()
                    .map(<[f32]>::to_vec)
                    .collect();
                s.pcm.splice(a * ch..b * ch, frames.into_iter().flatten());
            }
            OP_INVERT => s.pcm[a * ch..b * ch].iter_mut().for_each(|v| *v = -*v),
            OP_FADE_IN | OP_FADE_OUT => {
                for f in 0..len {
                    let t = f as f32 / len as f32;
                    let g = if op == OP_FADE_IN { t } else { 1.0 - t };
                    s.pcm[(a + f) * ch..(a + f + 1) * ch]
                        .iter_mut()
                        .for_each(|v| *v *= g);
                }
            }
            OP_XFADE_LOOP => {
                // The selection becomes the loop; its end fades into what came
                // before its start, so the jump back is seamless.
                let n = (len / 4).min(a);
                if n == 0 {
                    return Err("NEEDS AUDIO BEFORE".into());
                }
                for i in 0..n {
                    let t = i as f32 / n as f32;
                    for c in 0..ch {
                        let end = (b - n + i) * ch + c;
                        let before = (a - n + i) * ch + c;
                        s.pcm[end] = s.pcm[end] * (1.0 - t) + s.pcm[before] * t;
                    }
                }
                s.loop_region = Some((a as u32, b as u32));
            }
            OP_MONO_MIX | OP_MONO_LEFT | OP_MONO_RIGHT => {
                if ch == 2 {
                    s.pcm = s
                        .pcm
                        .chunks(2)
                        .map(|f| match op {
                            OP_MONO_LEFT => f[0],
                            OP_MONO_RIGHT => f[1],
                            _ => (f[0] + f[1]) * 0.5,
                        })
                        .collect();
                    s.channels = 1;
                }
            }
            OP_DOWNSAMPLE => {
                if s.rate < 4000 {
                    return Err("RATE TOO LOW".into());
                }
                // Two-frame average, then every other frame.
                s.pcm = s
                    .pcm
                    .chunks(2 * ch)
                    .flat_map(|pair| {
                        (0..ch).map(move |c| {
                            if pair.len() == 2 * ch {
                                (pair[c] + pair[ch + c]) * 0.5
                            } else {
                                pair[c]
                            }
                        })
                    })
                    .collect();
                s.rate /= 2;
                shift_markers(s, |f| Some(f / 2));
                s.selection = (s.selection.0 / 2, s.selection.1 / 2);
            }
            OP_BITS_16 | OP_BITS_8 => {
                let bits = if op == OP_BITS_8 { 8 } else { 16 };
                let q = (1u32 << (bits - 1)) as f32;
                s.pcm
                    .iter_mut()
                    .for_each(|v| *v = (*v * q).round().clamp(-q, q - 1.0) / q);
                s.bits = bits;
            }
            OP_SLICE_EVEN => {
                let parts = param.clamp(2, MAX_SLICES as i32) as usize;
                s.slices = (0..parts).map(|i| (a + i * len / parts) as u32).collect();
            }
            OP_SLICE_AUTO => s.slices = detect(s, a, b, false),
            OP_SLICE_SILENCE => s.slices = detect(s, a, b, true),
            OP_CLEAR_SLICES => s.slices.clear(),
            _ => return Err("UNKNOWN PROCESS".into()),
        }
        Ok(())
    }

    /// Writes the WAV with its markers: over the original when `path` is
    /// None, else to `path` (which becomes the editor's file).
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf, String> {
        let target = path.map_or_else(|| self.path.clone(), Path::to_path_buf);
        let bytes = self.wav_bytes()?;
        let tmp = target.with_extension("wav.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        self.path = target.clone();
        self.dirty = false;
        Ok(target)
    }

    fn wav_bytes(&self) -> Result<Vec<u8>, String> {
        let s = &self.s;
        let spec = hound::WavSpec {
            channels: s.channels,
            sample_rate: s.rate,
            bits_per_sample: s.bits,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
            let q = (1i64 << (s.bits - 1)) as f32;
            for &v in &s.pcm {
                let x = (v * q).round().clamp(-q, q - 1.0) as i32;
                if s.bits == 8 {
                    w.write_sample(x as i8).map_err(|e| e.to_string())?;
                } else {
                    w.write_sample(x).map_err(|e| e.to_string())?;
                }
            }
            w.finalize().map_err(|e| e.to_string())?;
        }
        let mut out = cursor.into_inner();
        if !s.slices.is_empty() {
            let mut body = (s.slices.len() as u32).to_le_bytes().to_vec();
            for (i, &f) in s.slices.iter().enumerate() {
                for v in [i as u32, f] {
                    body.extend(v.to_le_bytes());
                }
                body.extend(b"data");
                for v in [0u32, 0, f] {
                    body.extend(v.to_le_bytes());
                }
            }
            push_chunk(&mut out, b"cue ", &body);
        }
        if let Some((a, b)) = s.loop_region {
            let mut body = Vec::new();
            let period = 1_000_000_000 / s.rate.max(1);
            // manufacturer, product, period, unity note, fraction, SMPTE format
            // and offset, loop count, sampler data; then the loop.
            for v in [
                0u32,
                0,
                period,
                60,
                0,
                0,
                0,
                1,
                0,
                0,
                0,
                a,
                b.saturating_sub(1),
                0,
                0,
            ] {
                body.extend(v.to_le_bytes());
            }
            push_chunk(&mut out, b"smpl", &body);
        }
        let riff = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&riff.to_le_bytes());
        Ok(out)
    }
}

/// Moves (or drops) slices, the loop and keeps them sorted after an edit.
fn shift_markers(s: &mut State, map: impl Fn(usize) -> Option<usize>) {
    s.slices = s
        .slices
        .iter()
        .filter_map(|&f| map(f as usize))
        .map(|f| f as u32)
        .collect();
    s.slices.sort_unstable();
    s.slices.dedup();
    s.loop_region = s
        .loop_region
        .and_then(|(a, b)| {
            Some((
                map(a as usize)? as u32,
                map(b.saturating_sub(1) as usize)? as u32 + 1,
            ))
        })
        .filter(|(a, b)| a < b);
}

/// Slices from the audio in [a, b): note onsets (energy jumps) or the ends of
/// silences.
fn detect(s: &State, a: usize, b: usize, silence: bool) -> Vec<u32> {
    let ch = usize::from(s.channels.max(1));
    let hop = (s.rate as usize / 100).max(1); // 10 ms
    let energy: Vec<f32> = (a..b)
        .step_by(hop)
        .map(|f| {
            let end = (f + hop).min(b);
            let sum: f32 = s.pcm[f * ch..end * ch].iter().map(|v| v * v).sum();
            (sum / ((end - f) * ch).max(1) as f32).sqrt()
        })
        .collect();
    let peak = energy.iter().cloned().fold(0.0f32, f32::max);
    let mut out = vec![a as u32];
    let mut last = 0usize;
    for i in 1..energy.len() {
        let hit = if silence {
            energy[i - 1] < peak * 0.02 && energy[i] >= peak * 0.02
        } else {
            energy[i] > energy[i - 1] * 2.5 && energy[i] > peak * 0.1
        };
        // At least 50 ms between slices.
        if hit && i - last >= 5 {
            out.push((a + i * hop) as u32);
            last = i;
        }
        if out.len() >= MAX_SLICES {
            break;
        }
    }
    out
}

fn chunk<'a>(data: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if &data[pos..pos + 4] == id {
            return data.get(body..body + size);
        }
        pos = body + size + (size & 1);
    }
    None
}

fn read_cue_points(body: &[u8]) -> Vec<u32> {
    let count = body
        .get(..4)
        .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let mut out: Vec<u32> = (0..count)
        .filter_map(|i| body.get(4 + i * 24 + 20..4 + i * 24 + 24))
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn read_smpl_loop(body: &[u8]) -> Option<(u32, u32)> {
    let word = |o: usize| {
        body.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    (word(28)? > 0).then_some(())?;
    // First loop: start and end (inclusive) at 36 + 8 and 36 + 12.
    Some((word(44)?, word(48)? + 1))
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend(id);
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    if body.len() & 1 == 1 {
        out.push(0);
    }
}

// ---- C ABI -------------------------------------------------------------

unsafe fn str_arg<'a>(p: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    // SAFETY: the caller passes a valid C string or NULL.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy())
}

/// Opens a .wav for editing; NULL (and `err`) when it cannot be read.
///
/// # Safety
///
/// `path` a valid C string; `err` NULL or `err_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_open(
    path: *const c_char,
    err: *mut c_char,
    err_len: usize,
) -> *mut SampleEdit {
    // SAFETY: see the function's contract.
    let Some(path) = (unsafe { str_arg(path) }) else {
        return std::ptr::null_mut();
    };
    match SampleEdit::open(Path::new(path.as_ref())) {
        Ok(e) => Box::into_raw(Box::new(e)),
        Err(e) => {
            // SAFETY: the caller guarantees `err_len` writable bytes.
            unsafe { crate::viewer::write_c_string(&e.to_uppercase(), err, err_len) };
            std::ptr::null_mut()
        }
    }
}

/// # Safety
///
/// `edit` from `zt_sample_open` (or NULL), not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_close(edit: *mut SampleEdit) {
    if !edit.is_null() {
        // SAFETY: allocated by zt_sample_open.
        drop(unsafe { Box::from_raw(edit) });
    }
}

macro_rules! with_edit {
    ($edit:expr, $default:expr, |$e:ident| $body:expr) => {{
        // SAFETY: the caller passes a live editor or NULL.
        match unsafe { $edit.as_mut() } {
            Some($e) => $body,
            None => $default,
        }
    }};
}

/// # Safety
///
/// `edit` live or NULL; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_info(edit: *mut SampleEdit, out: *mut SampleInfo) {
    with_edit!(edit, (), |e| if !out.is_null() {
        // SAFETY: the caller passes a writable struct.
        unsafe { out.write(e.info()) }
    })
}

/// Copies up to `max` slice positions; returns how many there are.
///
/// # Safety
///
/// `edit` live or NULL; `out` NULL or `max` writable u32.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_slices(
    edit: *mut SampleEdit,
    out: *mut u32,
    max: usize,
) -> usize {
    with_edit!(edit, 0, |e| {
        let s = e.slices();
        if !out.is_null() {
            for (i, &v) in s.iter().take(max).enumerate() {
                // SAFETY: i < max writable slots.
                unsafe { out.add(i).write(v) };
            }
        }
        s.len()
    })
}

/// Min/max of the mono mix in `columns` columns over frames [from, to).
///
/// # Safety
///
/// `edit` live or NULL; `lo` and `hi` `columns` writable floats each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_peaks(
    edit: *mut SampleEdit,
    from: u32,
    to: u32,
    columns: usize,
    lo: *mut f32,
    hi: *mut f32,
) {
    with_edit!(edit, (), |e| {
        if lo.is_null() || hi.is_null() {
            return;
        }
        for (i, (a, b)) in e.peaks(from, to, columns).into_iter().enumerate() {
            // SAFETY: i < columns writable slots in each.
            unsafe {
                lo.add(i).write(a);
                hi.add(i).write(b);
            }
        }
    })
}

/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_set_selection(edit: *mut SampleEdit, a: u32, b: u32) {
    with_edit!(edit, (), |e| e.set_selection(a, b))
}

/// Sets the loop region [a, b); a >= b clears it.
///
/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_set_loop(edit: *mut SampleEdit, a: u32, b: u32) {
    with_edit!(edit, (), |e| e.set_loop((a < b).then_some((a, b))))
}

/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_add_slice(edit: *mut SampleEdit, at: u32) {
    with_edit!(edit, (), |e| e.add_slice(at))
}

/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_move_slice(edit: *mut SampleEdit, index: usize, at: u32) {
    with_edit!(edit, (), |e| e.move_slice(index, at))
}

/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_begin_edit(edit: *mut SampleEdit) {
    with_edit!(edit, (), |e| e.begin_edit())
}

/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_remove_slice(edit: *mut SampleEdit, index: usize) {
    with_edit!(edit, (), |e| e.remove_slice(index))
}

/// Runs a process (OP_*); 1 on success, else a short error in `err`.
///
/// # Safety
///
/// `edit` live or NULL; `err` NULL or `err_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_process(
    edit: *mut SampleEdit,
    op: i32,
    param: i32,
    err: *mut c_char,
    err_len: usize,
) -> i32 {
    with_edit!(edit, 0, |e| match e.process(op, param) {
        Ok(()) => 1,
        Err(msg) => {
            // A failed process leaves no undo step.
            e.undo.pop();
            // SAFETY: the caller guarantees `err_len` writable bytes.
            unsafe { crate::viewer::write_c_string(&msg, err, err_len) };
            0
        }
    })
}

/// # Safety
///
/// `edit` live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_undo(edit: *mut SampleEdit, redo: i32) -> i32 {
    with_edit!(edit, 0, |e| e.undo(redo != 0) as i32)
}

/// Plays frames [from, to) once through the engine's preview voice (all of
/// it when from >= to). 1 when it started.
///
/// # Safety
///
/// `edit` live or NULL; `engine` the app's engine or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_preview(
    edit: *mut SampleEdit,
    engine: *mut engine::ZtEngine,
    from: u32,
    to: u32,
) -> i32 {
    // SAFETY: the caller passes the live engine or NULL.
    let Some(engine) = (unsafe { engine.as_ref() }) else {
        return 0;
    };
    with_edit!(edit, 0, |e| {
        while engine.collect_retired().is_some() {}
        e.preview_sample(from, to)
            .is_some_and(|s| engine.set_preview(Some(s)).is_ok()) as i32
    })
}

/// Saves over the file (`path` NULL) or to `path`; writes the saved path to
/// `out`. 1 on success, else the error in `out`.
///
/// # Safety
///
/// `edit` live or NULL; `path` NULL or a valid C string; `out` NULL or
/// `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_save(
    edit: *mut SampleEdit,
    path: *const c_char,
    out: *mut c_char,
    len: usize,
) -> i32 {
    // SAFETY: see the function's contract.
    let path = unsafe { str_arg(path) };
    with_edit!(edit, 0, |e| {
        let (text, ok) = match e.save(path.as_deref().map(Path::new)) {
            Ok(p) => (p.to_string_lossy().into_owned(), 1),
            Err(msg) => (msg.to_uppercase(), 0),
        };
        // SAFETY: the caller guarantees `len` writable bytes.
        unsafe { crate::viewer::write_c_string(&text, out, len) };
        ok
    })
}

/// Stops the preview voice.
///
/// # Safety
///
/// `engine` the app's engine or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_sample_preview_stop(engine: *mut engine::ZtEngine) {
    // SAFETY: the caller passes the live engine or NULL.
    if let Some(engine) = unsafe { engine.as_ref() } {
        while engine.collect_retired().is_some() {}
        let _ = engine.set_preview(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(path: &Path, frames: usize, channels: u16) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            // A burst every 4410 frames over a quiet bed, for the slicers.
            let burst = (i % 4410) < 400;
            let v = (f32::sin(i as f32 * 0.05) * if burst { 12000.0 } else { 60.0 }) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zt-sample-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn markers_survive_a_save() {
        let p = temp("markers.wav");
        tone(&p, 44100, 2);
        let mut e = SampleEdit::open(&p).unwrap();
        e.add_slice(1000);
        e.add_slice(22050);
        e.set_loop(Some((4410, 8820)));
        e.save(None).unwrap();
        let back = SampleEdit::open(&p).unwrap();
        assert_eq!(back.slices(), &[1000, 22050]);
        assert_eq!(back.s.loop_region, Some((4410, 8820)));
        assert_eq!(back.info().channels, 2);
        // The player's own reader sees the same slices.
        let decoded = crate::samples::decode_wav(&p).unwrap();
        assert_eq!(&*decoded.cues, &[1000, 22050]);
    }

    #[test]
    fn processes_edit_the_range_and_undo() {
        let p = temp("proc.wav");
        tone(&p, 44100, 1);
        let mut e = SampleEdit::open(&p).unwrap();
        e.add_slice(30000);
        e.set_selection(10000, 20000);
        e.process(OP_CROP, 0).unwrap();
        assert_eq!(e.info().frames, 10000);
        assert!(e.slices().is_empty(), "a slice outside the crop goes");
        assert!(e.undo(false));
        assert_eq!(e.info().frames, 44100);
        e.set_selection(0, 4410);
        e.process(OP_DELETE, 0).unwrap();
        assert_eq!(e.info().frames, 44100 - 4410);
        assert_eq!(e.slices(), &[30000 - 4410]);
        e.set_selection(0, 0);
        e.process(OP_NORMALIZE, 0).unwrap();
        let peak = e.s.pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.999).abs() < 1e-3);
        e.process(OP_SLICE_EVEN, 4).unwrap();
        assert_eq!(e.slices().len(), 4);
        e.process(OP_SLICE_AUTO, 0).unwrap();
        assert!(e.slices().len() >= 8, "one per burst: {:?}", e.slices());
        e.process(OP_DOWNSAMPLE, 0).unwrap();
        assert_eq!(e.info().rate, 22050);
        e.process(OP_BITS_8, 0).unwrap();
        e.save(None).unwrap();
        let back = hound::WavReader::open(&p).unwrap();
        assert_eq!(back.spec().bits_per_sample, 8);
        assert_eq!(back.spec().sample_rate, 22050);
    }

    #[test]
    fn a_grabbed_slice_moves_between_its_neighbours_and_undoes() {
        let p = temp("move.wav");
        tone(&p, 44100, 1);
        let mut e = SampleEdit::open(&p).unwrap();
        e.process(OP_SLICE_EVEN, 4).unwrap(); // 0, 11025, 22050, 33075
        e.begin_edit();
        e.move_slice(1, 15000);
        assert_eq!(e.slices()[1], 15000);
        e.move_slice(1, 40000); // stops before the next one
        assert_eq!(e.slices()[1], 22049);
        assert!(e.undo(false));
        assert_eq!(e.slices()[1], 11025);
    }

    #[test]
    fn stereo_to_mono_and_preview() {
        let p = temp("stereo.wav");
        tone(&p, 4410, 2);
        let mut e = SampleEdit::open(&p).unwrap();
        e.process(OP_MONO_MIX, 0).unwrap();
        assert_eq!(e.info().channels, 1);
        let s = e.preview_sample(100, 200).unwrap();
        assert_eq!(s.pcm.len(), 100);
        assert!(s.is_valid());
    }
}
