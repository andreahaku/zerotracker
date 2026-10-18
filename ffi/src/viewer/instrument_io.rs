//! INSTRUMENT actions: EDIT on LOAD asks the app for
//! the browser of .m8i files, EDIT on SAVE writes `Instruments/NAME.m8i`
//! under the songs root, EDIT on a SAMPLER's sample asks for the browser of
//! .wav files. Loading and the new sample go through
//! `Command::ReplaceInstrument`, so DEL undoes them.

use super::{Request, Viewer};
use m8edit::Command;
use m8fmt::{Instrument, Table, Version};
use m8view::{Field, InstrumentParameter};
use std::path::{Path, PathBuf};

/// What the app browses for, in `Request::Browse`.
pub const BROWSE_INSTRUMENT: i32 = 1;
pub const BROWSE_SAMPLE: i32 = 2;

impl Viewer {
    /// EDIT alone on LOAD, SAVE or a sample path. True when handled.
    pub(crate) fn instrument_action(&mut self, field: Field) -> bool {
        let Field::Instrument { parameter, .. } = field else {
            return false;
        };
        match parameter {
            InstrumentParameter::Load => self.request = Some(Request::Browse(BROWSE_INSTRUMENT)),
            InstrumentParameter::SamplePath => self.request = Some(Request::Browse(BROWSE_SAMPLE)),
            // ENTER on EQ opens the EQ editor (EDIT + arrows still pick the
            // EQ's number). An instrument without one gets its own.
            InstrumentParameter::Eq => match self.eq_open() {
                Some(_) => self.request = Some(Request::EditEq),
                None => self.message = "NO EQ".into(),
            },
            InstrumentParameter::SampleEdit => match self.sample_file() {
                Some(path) => self.request = Some(Request::EditSample(path)),
                None => self.request = Some(Request::Record),
            },
            InstrumentParameter::Save => {
                self.message = match self.save_instrument() {
                    Ok(msg) => msg,
                    Err(e) => e,
                };
            }
            _ => return false,
        }
        true
    }

    /// The current SAMPLER's file on disk (its path is from the songs root).
    fn sample_file(&self) -> Option<String> {
        let Instrument::Sampler(s) = &self.song().instruments[self.instrument as usize] else {
            return None;
        };
        let rel = s.sample_path.trim_start_matches('/');
        (!rel.is_empty()).then(|| {
            self.root()
                .map(|r| r.join(rel).to_string_lossy().into_owned())
        })?
    }

    fn root(&self) -> Option<PathBuf> {
        self.template
            .as_ref()
            .and_then(|t| t.parent())
            .map(Path::to_path_buf)
    }

    /// Writes the instrument and its table to `Instruments/NAME.m8i`. An
    /// existing file is overwritten only on a second EDIT.
    fn save_instrument(&mut self) -> Result<String, String> {
        let root = self.root().ok_or("NO SONGS ROOT")?;
        let slot = self.instrument as usize;
        let instrument = self.song().instruments[slot].clone();
        if matches!(instrument, Instrument::None) {
            return Err("EMPTY INSTRUMENT".into());
        }
        let name = instrument.name().unwrap_or("").trim().to_string();
        let name = if name.is_empty() {
            format!("INST{slot:02X}")
        } else {
            name
        };
        let dir = root.join("Instruments");
        std::fs::create_dir_all(&dir).map_err(|_| "CANNOT CREATE INSTRUMENTS")?;
        let path = dir.join(format!("{name}.m8i"));
        if path.exists() && self.save_armed.as_deref() != Some(path.as_path()) {
            self.save_armed = Some(path);
            return Ok(String::new()); // the app asks (UX_CONFIRM_OVERWRITE)
        }
        self.save_armed = None;
        let bytes = instrument_file(
            &instrument,
            &self.song().tables[slot],
            self.song().version,
            &path,
        )?;
        std::fs::write(&path, bytes).map_err(|_| "SAVE FAILED")?;
        Ok(format!("SAVED {name}"))
    }

    /// Replaces the current instrument (and its table) with a .m8i file.
    pub fn load_instrument(&mut self, path: &str) -> Result<(), String> {
        let data = std::fs::read(path).map_err(|e| format!("{e}").to_uppercase())?;
        let mut file =
            Instrument::read(&mut data.as_slice()).map_err(|_| "NOT AN INSTRUMENT".to_string())?;
        if let (Instrument::Sampler(s), Some(root)) = (&mut file.instrument, self.root())
            && let Some(rooted) = bundled_sample(Path::new(path), &s.sample_path, &root)
        {
            s.sample_path = rooted;
        }
        let slot = self.instrument;
        if !self.run(Command::ReplaceInstrument {
            slot,
            instrument: Box::new(file.instrument),
            table: Some(Box::new(file.table)),
        }) {
            return Err(std::mem::take(&mut self.message));
        }
        self.reload_samples();
        Ok(())
    }

    /// Points the current SAMPLER at `path`, a file under the songs root.
    pub fn set_sample(&mut self, path: &str) -> Result<(), String> {
        let root = self.root().ok_or("NO SONGS ROOT")?;
        let rel = Path::new(path)
            .strip_prefix(&root)
            .map_err(|_| "OUTSIDE SONGS ROOT")?;
        let slot = self.instrument;
        let mut instrument = self.song().instruments[slot as usize].clone();
        let Instrument::Sampler(s) = &mut instrument else {
            return Err("NOT A SAMPLER".into());
        };
        // The M8 stores the path from the card's root: /Samples/....
        s.sample_path = format!("/{}", rel.to_string_lossy());
        if !self.run(Command::ReplaceInstrument {
            slot,
            instrument: Box::new(instrument),
            table: None,
        }) {
            return Err(std::mem::take(&mut self.message));
        }
        self.reload_samples();
        Ok(())
    }

    /// Reloads the sample bank for the song as it is now and republishes.
    pub(crate) fn reload_samples(&mut self) {
        if self.engine.is_none() {
            return;
        }
        let problems = self.bank.load(self.editor.song());
        if let Some((slot, path, problem)) = problems.first() {
            eprintln!("[viewer] sample problem: {slot:02X} {path}: {problem:?}");
            self.message = format!("{} SAMPLES MISSING", problems.len());
        }
        self.publish();
    }
}

/// A preset pack stores `Samples/x.wav` relative to the .m8i; once the
/// instrument is in a song the path must be from the songs root.
fn bundled_sample(m8i: &Path, sample: &str, root: &Path) -> Option<String> {
    if sample.is_empty() || sample.starts_with('/') {
        return None;
    }
    let file = m8i.parent()?.join(sample);
    let rel = file.strip_prefix(root).ok()?;
    file.is_file()
        .then(|| format!("/{}", rel.to_string_lossy()))
}

/// Whether two instruments serialise to the same bytes (the slot number
/// they carry in memory is not part of them).
fn same_bytes(a: &Instrument, b: &Instrument, version: Version) -> bool {
    let bytes = |i: &Instrument| {
        let mut w = m8fmt::writer::Writer::new(vec![0; Instrument::INSTRUMENT_MEMORY_SIZE]);
        i.write(version, &mut w);
        w.finish()
    };
    bytes(a) == bytes(b)
}

/// The bytes of a .m8i: version, instrument, table. Over an existing file the
/// bytes it already has (the EQ after the table) are kept.
fn instrument_file(
    instrument: &Instrument,
    table: &Table,
    version: Version,
    path: &Path,
) -> Result<Vec<u8>, String> {
    let size = Version::SIZE + Instrument::INSTRUMENT_MEMORY_SIZE + Table::V4_SIZE;
    let base = std::fs::read(path)
        .ok()
        .filter(|b| b.len() >= size)
        // Bytes the writer leaves alone read as FF, as in an empty M8 slot.
        .unwrap_or_else(|| vec![0xFF; size]);
    let file = m8fmt::InstrumentWithEq {
        instrument: instrument.clone(),
        table: table.clone(),
        eq: None,
        version,
    };
    let mut w = m8fmt::writer::Writer::new(base);
    file.write(&mut w);
    let out = w.finish();
    // Check that it reads back as the same instrument.
    let back = Instrument::read(&mut out.as_slice()).map_err(|_| "SAVE CHECK FAILED")?;
    if !same_bytes(&back.instrument, instrument, version) {
        return Err("SAVE CHECK FAILED".into());
    }
    Ok(out)
}

/// Reloads the song's samples (after the sample editor saved a file).
///
/// # Safety
///
/// `viewer` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_reload_samples(viewer: *mut Viewer) {
    // SAFETY: the caller passes a live viewer or NULL.
    if let Some(v) = unsafe { viewer.as_mut() } {
        v.reload_samples();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::Kind;

    #[test]
    fn bundled_sample_paths_become_rooted() {
        let root = std::env::temp_dir().join(format!("zt-pack-{}", std::process::id()));
        let pack = root.join("Instruments").join("Pack");
        std::fs::create_dir_all(pack.join("Samples")).unwrap();
        std::fs::write(pack.join("Samples").join("a.wav"), b"x").unwrap();
        let m8i = pack.join("A.m8i");
        assert_eq!(
            bundled_sample(&m8i, "Samples/a.wav", &root).as_deref(),
            Some("/Instruments/Pack/Samples/a.wav")
        );
        assert_eq!(bundled_sample(&m8i, "/Samples/a.wav", &root), None);
        assert_eq!(bundled_sample(&m8i, "Samples/b.wav", &root), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn save_load_and_sample_round_trip() {
        let song = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !Path::new(song).exists() {
            return;
        }
        let root = std::env::temp_dir().join(format!("zt-inst-io-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Samples")).unwrap();
        let engine = engine::zt_engine_create(44100, 0);
        let mut v = Viewer::open(song).unwrap();
        v.attach_engine(engine, root.to_str().unwrap());
        v.kind = Kind::Instrument;
        let Some(slot) =
            (0..0x80u8).find(|&i| !matches!(v.song().instruments[i as usize], Instrument::None))
        else {
            return;
        };
        v.instrument = slot;
        let save = Field::Instrument {
            index: slot,
            parameter: InstrumentParameter::Save,
        };
        assert!(v.instrument_action(save));
        assert!(v.message.starts_with("SAVED"), "{}", v.message);
        let file = std::fs::read_dir(root.join("Instruments"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        // A second SAVE asks first, a third overwrites.
        assert!(v.instrument_action(save));
        assert!(v.save_armed.is_some(), "the app asks before overwriting");
        assert!(v.instrument_action(save));
        assert!(v.message.starts_with("SAVED"));

        // LOAD it into an empty slot; DEL undoes it.
        let empty = (0..0x80u8)
            .find(|&i| matches!(v.song().instruments[i as usize], Instrument::None))
            .unwrap();
        v.instrument = empty;
        v.load_instrument(file.to_str().unwrap()).unwrap();
        let version = v.song().version;
        assert!(same_bytes(
            &v.song().instruments[empty as usize],
            &v.song().instruments[slot as usize],
            version
        ));
        assert!(v.undo(false));
        assert!(matches!(
            v.song().instruments[empty as usize],
            Instrument::None
        ));

        // A sampler takes a new sample path from under the root.
        if let Some(s) = (0..0x80u8)
            .find(|&i| matches!(v.song().instruments[i as usize], Instrument::Sampler(_)))
        {
            v.instrument = s;
            let wav = root.join("Samples").join("kick.wav");
            v.set_sample(wav.to_str().unwrap()).unwrap();
            let Instrument::Sampler(sm) = &v.song().instruments[s as usize] else {
                unreachable!()
            };
            assert_eq!(sm.sample_path, "/Samples/kick.wav");
        }
        std::fs::remove_dir_all(&root).unwrap();
        // SAFETY: created above, the viewer no longer renders.
        unsafe { engine::zt_engine_destroy(engine) };
    }
}
