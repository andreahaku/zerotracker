//! PLAY in the LOAD browser: hear a file before loading it. Each kind has
//! its own path to the one engine:
//!
//! - a song plays from its first row, through a viewer of its own;
//! - an instrument (.m8i) goes into slot 00 of the built-in empty song and
//!   plays a C major arpeggio on track 1, looping;
//! - a sample (.wav) plays once through the engine's preview voice.
//!
//! Runs on the UI thread. Only one preview sounds at a time.

use crate::viewer::{EMPTY_SONG, Viewer};
use m8fmt::{Instrument, Song};
use std::ffi::{CStr, c_char};
use std::ptr::NonNull;

/// File kinds, as the browser numbers them.
pub const KIND_SONG: i32 = 0;
pub const KIND_INSTRUMENT: i32 = 1;
pub const KIND_SAMPLE: i32 = 2;

/// Arpeggio of the instrument preview: C-4 E-4 G-4 C-5 on steps 0, 4, 8, 12.
const ARPEGGIO: [(usize, u8); 4] = [(0, 0x24), (4, 0x28), (8, 0x2B), (12, 0x30)];
const VELOCITY: u8 = 0x60;

pub struct Preview {
    engine: NonNull<engine::ZtEngine>,
    root: String,
    /// The song or instrument preview, with the path it plays.
    viewer: Option<(Viewer, String)>,
    /// A sample waiting for the engine's preview mailbox.
    pending_sample: Option<engine::Sample>,
    sample_started: bool,
}

impl Preview {
    pub fn new(engine: NonNull<engine::ZtEngine>, root: &str) -> Self {
        Self {
            engine,
            root: root.to_string(),
            viewer: None,
            pending_sample: None,
            sample_started: false,
        }
    }

    fn engine(&self) -> &engine::ZtEngine {
        // SAFETY: the app keeps the engine alive while the preview exists.
        unsafe { self.engine.as_ref() }
    }

    /// Stops what plays and starts `path`. The error is a short message.
    pub fn play(&mut self, path: &str, kind: i32) -> Result<(), String> {
        self.stop();
        match kind {
            KIND_SONG => {
                let viewer = Viewer::open(path)?;
                self.start_viewer(viewer, path);
                Ok(())
            }
            KIND_INSTRUMENT => {
                let viewer = Viewer::from_bytes(instrument_song(path)?, path)?;
                self.start_viewer(viewer, path);
                Ok(())
            }
            KIND_SAMPLE => {
                let sample = crate::samples::decode_wav(std::path::Path::new(path))
                    .map_err(|e| format!("{e:?}").to_uppercase())?;
                // As the sample editor's preview: -6 dB, there is no mixer.
                let mut sample: engine::Sample = sample.into();
                let gain = crate::sample_edit::PREVIEW_GAIN;
                sample.pcm = sample
                    .pcm
                    .iter()
                    .map(|v| v * gain)
                    .collect::<Vec<f32>>()
                    .into();
                self.pending_sample = Some(sample);
                self.sample_started = false;
                self.tick();
                Ok(())
            }
            _ => Err("UNKNOWN KIND".into()),
        }
    }

    /// Plays instrument `slot` of an open song, with its table and EQ (the
    /// INSTRUMENT page's EDIT+PLAY and the POOL's PLAY).
    pub fn play_instrument_of(&mut self, song: &Song, slot: u8) -> Result<(), String> {
        self.stop();
        let index = usize::from(slot);
        let instrument = song.instruments.get(index).ok_or("NO INSTRUMENT")?.clone();
        if matches!(instrument, Instrument::None) {
            return Err("EMPTY SLOT".into());
        }
        let eq = instrument
            .equ()
            .and_then(|e| Some((e, song.eqs.get(usize::from(e))?.clone())));
        let table = song.tables.get(index).ok_or("NO TABLE")?.clone();
        let viewer = Viewer::from_bytes(arpeggio_song(instrument, table, eq)?, "")?;
        self.start_viewer(viewer, "");
        Ok(())
    }

    fn start_viewer(&mut self, mut viewer: Viewer, path: &str) {
        viewer.attach_engine(self.engine.as_ptr(), &self.root);
        viewer.play_when_ready();
        viewer.tick();
        self.viewer = Some((viewer, path.to_string()));
    }

    /// Retries what waits for the engine; call it every UI tick.
    pub fn tick(&mut self) {
        if let Some((viewer, _)) = &mut self.viewer {
            viewer.tick();
        } else {
            // Nothing else collects the engine's retired generations while no
            // song is open; the preview mailbox needs it to take a new sample.
            while self.engine().collect_retired().is_some() {}
        }
        if let Some(sample) = self.pending_sample.take() {
            match self.engine().set_preview(Some(sample)) {
                Ok(()) => self.sample_started = true,
                Err(back) => self.pending_sample = back,
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some((mut viewer, _)) = self.viewer.take() {
            viewer.stop();
        }
        self.pending_sample = None;
        if self.sample_started {
            // A busy mailbox only delays the stop by a tick; the voice ends anyway.
            let _ = self.engine().set_preview(None);
            self.sample_started = false;
        }
    }

    pub fn playing(&self) -> bool {
        self.viewer.as_ref().is_some_and(|(v, _)| v.is_playing())
            || self.pending_sample.is_some()
            || (self.sample_started && self.engine().preview_playing())
    }

    /// The song preview of `path`, stopped, to open without loading again.
    pub fn take_song(&mut self, path: &str) -> Option<Viewer> {
        if self.viewer.as_ref().is_some_and(|(_, p)| p == path) {
            let (mut viewer, _) = self.viewer.take()?;
            viewer.stop();
            return Some(viewer);
        }
        None
    }
}

/// The empty song with the .m8i in slot 00 and the arpeggio on track 1.
fn instrument_song(path: &str) -> Result<Vec<u8>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{e}").to_uppercase())?;
    let file = Instrument::read(&mut data.as_slice()).map_err(|e| format!("{e:?}"))?;
    // Its EQ (kept in the .m8i) is left out.
    arpeggio_song(file.instrument, file.table, None)
}

/// The empty song playing `instrument` (with its table, and its EQ when
/// given at its own index) in slot 00, the arpeggio on track 1.
fn arpeggio_song(
    mut instrument: Instrument,
    table: m8fmt::Table,
    eq: Option<(u8, m8fmt::Equ)>,
) -> Result<Vec<u8>, String> {
    let mut song = Song::read_from_reader(&mut m8fmt::reader::Reader::new(EMPTY_SONG.to_vec()))
        .map_err(|e| format!("{e:?}"))?;
    match eq {
        Some((index, eq)) if usize::from(index) < song.eqs.len() => {
            song.eqs[usize::from(index)] = eq
        }
        _ => {
            if instrument.equ().is_some() {
                instrument.set_eq(0x80);
            }
        }
    }
    // Table NN belongs to instrument NN.
    song.instruments[0] = instrument;
    song.tables[0] = table;
    for (step, note) in ARPEGGIO {
        let s = &mut song.phrases[0].steps[step];
        s.note = m8fmt::Note(note);
        s.velocity = VELOCITY;
        s.instrument = 0;
    }
    song.chains[0].steps[0].phrase = 0;
    song.song.steps[0] = 0;
    let mut w = m8fmt::writer::Writer::new(EMPTY_SONG.to_vec());
    song.write(&mut w)?;
    Ok(w.finish())
}

// ---- C ABI -------------------------------------------------------------

/// # Safety
///
/// `engine` must outlive the preview; `root` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_create(
    engine: *mut engine::ZtEngine,
    root: *const c_char,
) -> *mut Preview {
    let Some(engine) = NonNull::new(engine) else {
        return std::ptr::null_mut();
    };
    if root.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller passes a valid C string.
    let root = unsafe { CStr::from_ptr(root) }.to_string_lossy();
    Box::into_raw(Box::new(Preview::new(engine, &root)))
}

/// # Safety
///
/// `preview` must come from `zt_preview_create` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_destroy(preview: *mut Preview) {
    if !preview.is_null() {
        // SAFETY: allocated by zt_preview_create.
        let mut preview = unsafe { Box::from_raw(preview) };
        preview.stop();
    }
}

/// Plays `path` as `kind` (0 song, 1 instrument, 2 sample). 1 on success;
/// else a short error goes into `err` (when not NULL).
///
/// # Safety
///
/// `preview` must be live; `path` a valid C string; `err` NULL or
/// `err_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_play(
    preview: *mut Preview,
    path: *const c_char,
    kind: i32,
    err: *mut c_char,
    err_len: usize,
) -> i32 {
    // SAFETY: the caller passes a live preview.
    let Some(preview) = (unsafe { preview.as_mut() }) else {
        return 0;
    };
    if path.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a valid C string.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    match preview.play(&path, kind) {
        Ok(()) => 1,
        Err(e) => {
            // SAFETY: the caller guarantees `err_len` writable bytes.
            unsafe { crate::viewer::write_c_string(&e, err, err_len) };
            0
        }
    }
}

/// Plays instrument `slot` of the viewer's song (arpeggio, looping, until
/// zt_preview_stop). 1 on success; else a short error goes into `err`.
///
/// # Safety
///
/// `preview` and `viewer` must be live; `err` NULL or `err_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_play_instrument(
    preview: *mut Preview,
    viewer: *const Viewer,
    slot: u8,
    err: *mut c_char,
    err_len: usize,
) -> i32 {
    // SAFETY: the caller passes a live preview and viewer.
    let (Some(preview), Some(viewer)) = (unsafe { preview.as_mut() }, unsafe { viewer.as_ref() })
    else {
        return 0;
    };
    match preview.play_instrument_of(viewer.song(), slot) {
        Ok(()) => 1,
        Err(e) => {
            // SAFETY: the caller guarantees `err_len` writable bytes.
            unsafe { crate::viewer::write_c_string(&e, err, err_len) };
            0
        }
    }
}

/// # Safety
///
/// `preview` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_stop(preview: *mut Preview) {
    // SAFETY: the caller passes a live preview.
    if let Some(preview) = unsafe { preview.as_mut() } {
        preview.stop();
    }
}

/// # Safety
///
/// `preview` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_tick(preview: *mut Preview) {
    // SAFETY: the caller passes a live preview.
    if let Some(preview) = unsafe { preview.as_mut() } {
        preview.tick();
    }
}

/// # Safety
///
/// `preview` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_playing(preview: *const Preview) -> i32 {
    // SAFETY: the caller passes a live preview.
    unsafe { preview.as_ref() }.is_some_and(Preview::playing) as i32
}

/// The song preview of `path`, stopped, as a viewer to open; NULL when the
/// preview plays something else.
///
/// # Safety
///
/// `preview` must be live; `path` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_preview_take_song(
    preview: *mut Preview,
    path: *const c_char,
) -> *mut Viewer {
    // SAFETY: the caller passes a live preview.
    let Some(preview) = (unsafe { preview.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller passes a valid C string.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    preview
        .take_song(&path)
        .map_or(std::ptr::null_mut(), |v| Box::into_raw(Box::new(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders `blocks` x 256 frames and returns the peak, ticking the preview.
    fn peak(engine: *mut engine::ZtEngine, preview: &mut Preview, blocks: usize) -> i32 {
        let mut buf = vec![0i16; 512];
        let mut max = 0;
        for _ in 0..blocks {
            preview.tick();
            // SAFETY: a live engine and a buffer of 256 stereo frames.
            unsafe { engine::zt_engine_render(engine, buf.as_mut_ptr(), 256, 2) };
            max = buf
                .iter()
                .map(|&x| i32::from(x).abs())
                .max()
                .unwrap()
                .max(max);
        }
        max
    }

    /// A file of the m8-file-parser examples (scripts/fetch-fixtures.sh);
    /// None, and the test is skipped, when they are not fetched.
    fn fixture(name: &str) -> Option<String> {
        let path = format!("{}/../m8fmt/examples/{name}", env!("CARGO_MANIFEST_DIR"));
        std::path::Path::new(&path).is_file().then_some(path)
    }

    #[test]
    fn instrument_song_has_the_arpeggio() {
        let Some(path) = fixture("instruments/FMDUBSTAB_4_1.m8i") else {
            return;
        };
        let bytes = instrument_song(&path).unwrap();
        let song = Song::read_from_reader(&mut m8fmt::reader::Reader::new(bytes)).unwrap();
        assert_eq!(song.song.steps[0], 0);
        assert_eq!(song.chains[0].steps[0].phrase, 0);
        assert_eq!(song.phrases[0].steps[4].note.0, 0x28);
        assert!(matches!(song.instruments[0], Instrument::FMSynth(_)));
    }

    #[test]
    fn song_instrument_preview_sounds_with_its_eq_and_stops() {
        // A WAVSYNTH from the
        // built-in defaults in slot 05 of the empty song, with an EQ.
        let mut song =
            Song::read_from_reader(&mut m8fmt::reader::Reader::new(EMPTY_SONG.to_vec())).unwrap();
        let bytes = crate::viewer::instrument_defaults::TYPES[0].1.to_vec();
        let mut inst =
            Instrument::from_reader(&mut m8fmt::reader::Reader::new(bytes), 5, song.version)
                .unwrap();
        inst.set_eq(0x07);
        song.eqs[7].mid.level = 0x04; // +10.24 dB bell
        song.instruments[5] = inst.clone();
        let engine = engine::zt_engine_create(44100, 0);
        let mut preview = Preview::new(NonNull::new(engine).unwrap(), "/nonexistent");
        assert!(preview.play_instrument_of(&song, 4).is_err(), "empty slot");
        preview.play_instrument_of(&song, 5).unwrap();
        assert!(peak(engine, &mut preview, 200) > 500, "the WAVSYNTH sounds");
        assert!(preview.playing());
        preview.stop();
        peak(engine, &mut preview, 400);
        assert_eq!(peak(engine, &mut preview, 50), 0, "stopped");
        // SAFETY: created above, no longer used.
        unsafe { engine::zt_engine_destroy(engine) };
    }

    #[test]
    fn instrument_preview_plays_the_arpeggio_and_stops() {
        let Some(path) = fixture("instruments/FMDUBSTAB_4_1.m8i") else {
            return;
        };
        let engine = engine::zt_engine_create(44100, 0);
        let mut preview = Preview::new(NonNull::new(engine).unwrap(), "/nonexistent");
        preview.play(&path, KIND_INSTRUMENT).unwrap();
        // FMDUBSTAB's operators are SQR, which the engine does not render yet:
        // check the sequencer instead of the level.
        peak(engine, &mut preview, 200);
        // SAFETY: a live engine.
        let status = unsafe { engine.as_ref() }.unwrap().track_status(0).unwrap();
        assert_eq!(status.active, 1, "track 1 plays the arpeggio");
        assert!(status.phrase_step > 0);
        assert!(preview.playing());
        preview.stop();
        peak(engine, &mut preview, 400);
        // SAFETY: a live engine.
        let status = unsafe { engine.as_ref() }.unwrap().track_status(0).unwrap();
        assert_eq!(status.active, 0, "stopped");
        // SAFETY: created above, no longer used.
        unsafe { engine::zt_engine_destroy(engine) };
    }

    #[test]
    fn song_preview_hands_its_viewer_to_open() {
        let Some(song) = fixture("songs/DEFAULT.m8s") else {
            return;
        };
        let engine = engine::zt_engine_create(44100, 0);
        let mut preview = Preview::new(NonNull::new(engine).unwrap(), "/nonexistent");
        preview.play(&song, KIND_SONG).unwrap();
        peak(engine, &mut preview, 10);
        assert!(preview.take_song("/other.m8s").is_none());
        assert!(preview.take_song(&song).is_some());
        assert!(!preview.playing());
        // SAFETY: created above, no longer used.
        unsafe { engine::zt_engine_destroy(engine) };
    }

    #[test]
    fn sample_preview_plays_a_wav_once() {
        let dir = std::env::temp_dir().join(format!("zt-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("tone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&wav, spec).unwrap();
        for i in 0..2205 {
            let v = (f32::sin(i as f32 * 0.2) * 16000.0) as i16;
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
        let engine = engine::zt_engine_create(44100, 0);
        let mut preview = Preview::new(NonNull::new(engine).unwrap(), "/nonexistent");
        preview.play(wav.to_str().unwrap(), KIND_SAMPLE).unwrap();
        assert!(peak(engine, &mut preview, 20) > 5000, "the tone is audible");
        // 0.1 s at 22050 Hz is over well within 1 s.
        peak(engine, &mut preview, 200);
        assert!(!preview.playing(), "a sample plays once");
        std::fs::remove_dir_all(&dir).unwrap();
        // SAFETY: created above, no longer used.
        unsafe { engine::zt_engine_destroy(engine) };
    }
}
