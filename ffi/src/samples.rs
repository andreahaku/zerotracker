//! Sample loading for playback: resolves the SAMPLER paths of a song under
//! the songs root, decodes the WAV files to interleaved `f32`, and keeps them
//! in a bank shared across edited snapshots (agreed in
//! docs/proposals/phase4-engine-api.md).
//!
//! Runs on the UI side, never on the audio thread.

use m8fmt::{Instrument, Song};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Decoded PCM of one sample file.
#[derive(Debug, Clone)]
pub struct DecodedSample {
    pub rate: u32,
    pub channels: u8,
    /// Interleaved frames, -1.0..1.0.
    pub pcm: Arc<[f32]>,
    /// WAV `cue ` markers (sample offsets in frames), sorted, unique; the
    /// SAMPLER slices on them.
    pub cues: Arc<[u32]>,
}

impl From<DecodedSample> for engine::Sample {
    fn from(s: DecodedSample) -> Self {
        engine::Sample {
            rate: s.rate,
            channels: s.channels,
            pcm: s.pcm,
        }
    }
}

impl SampleBank {
    /// Cue markers per instrument slot, for `PlaybackSnapshot::with_sample_cues`.
    pub fn cues(&self) -> Vec<Arc<[u32]>> {
        self.slots
            .iter()
            .map(|slot| match slot {
                Some(Ok(s)) => s.cues.clone(),
                _ => Arc::from([]),
            })
            .collect()
    }
}

impl DecodedSample {
    pub fn bytes(&self) -> usize {
        self.pcm.len() * std::mem::size_of::<f32>()
    }
}

/// Why a sample is not available for playback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleProblem {
    OutsideRoot,
    Missing,
    Unsupported(String),
    OverBudget,
}

/// Samples of a song, by instrument slot, decoded within a byte budget.
#[derive(Debug, Default)]
pub struct SampleBank {
    root: PathBuf,
    /// Folder of the open song: bundled songs store `Samples/x.wav` relative to it.
    song_dir: Option<PathBuf>,
    budget: usize,
    /// Decoded files by resolved path, shared by the instruments that use them.
    files: HashMap<PathBuf, DecodedSample>,
    /// Per instrument slot: the sample, or why there is none.
    pub slots: Vec<Option<Result<DecodedSample, SampleProblem>>>,
}

/// Default decoded-PCM budget on the device (48 MiB), see the API proposal.
pub const DEFAULT_BUDGET: usize = 48 << 20;

impl SampleBank {
    pub fn new(root: impl Into<PathBuf>, budget: usize) -> Self {
        Self {
            root: root.into(),
            budget,
            ..Self::default()
        }
    }

    /// Sets the folder of the open song, for paths without a leading `/`.
    pub fn set_song_dir(&mut self, dir: Option<PathBuf>) {
        self.song_dir = dir;
    }

    /// Bytes of decoded PCM held.
    pub fn used(&self) -> usize {
        self.files.values().map(DecodedSample::bytes).sum()
    }

    /// Loads the samples `song` uses. Files already decoded are reused, files
    /// no longer used are dropped. Returns the problems by instrument slot.
    pub fn load(&mut self, song: &Song) -> Vec<(u8, String, SampleProblem)> {
        let mut problems = Vec::new();
        let mut keep: HashMap<PathBuf, DecodedSample> = HashMap::new();
        let mut slots = Vec::with_capacity(song.instruments.len());
        for (index, instrument) in song.instruments.iter().enumerate() {
            let Instrument::Sampler(sampler) = instrument else {
                slots.push(None);
                continue;
            };
            let path = sampler.sample_path.clone();
            if path.is_empty() {
                slots.push(None);
                continue;
            }
            let result = self.resolve(&path).and_then(|file| {
                if let Some(s) = keep.get(&file).or_else(|| self.files.get(&file)) {
                    return Ok((file, s.clone()));
                }
                // Checked from the header first: decoding a file that does not
                // fit can take more memory than the device has.
                let used: usize = keep.values().map(DecodedSample::bytes).sum();
                if used + decoded_size(&file)? > self.budget {
                    return Err(SampleProblem::OverBudget);
                }
                Ok((file.clone(), decode_wav(&file)?))
            });
            match result {
                Ok((file, sample)) => {
                    keep.insert(file, sample.clone());
                    slots.push(Some(Ok(sample)));
                }
                Err(problem) => {
                    problems.push((index as u8, path, problem.clone()));
                    slots.push(Some(Err(problem)));
                }
            }
        }
        self.files = keep;
        self.slots = slots;
        problems
    }

    /// `/Samples/x.wav` -> `<root>/Samples/x.wav`; a bundled song's
    /// `Samples/x.wav` -> `<song folder>/Samples/x.wav`, then `<root>/...`.
    /// Rejects `..` and anything that would leave the root.
    fn resolve(&self, m8_path: &str) -> Result<PathBuf, SampleProblem> {
        let rel = Path::new(m8_path.trim_start_matches('/'));
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(SampleProblem::OutsideRoot);
        }
        let bundled = self
            .song_dir
            .as_ref()
            .filter(|_| !m8_path.starts_with('/'))
            .map(|dir| dir.join(rel))
            .filter(|file| file.is_file());
        let file = bundled.unwrap_or_else(|| self.root.join(rel));
        if !file.is_file() {
            return Err(SampleProblem::Missing);
        }
        // A symlink inside the root may still point outside it.
        let root = self
            .root
            .canonicalize()
            .map_err(|_| SampleProblem::Missing)?;
        let real = file.canonicalize().map_err(|_| SampleProblem::Missing)?;
        if real.starts_with(&root) {
            Ok(real)
        } else {
            Err(SampleProblem::OutsideRoot)
        }
    }
}

/// Bytes of interleaved f32 PCM that `decode_wav` would hold for `path`,
/// read from the WAV header only.
pub fn decoded_size(path: &Path) -> Result<usize, SampleProblem> {
    let reader =
        hound::WavReader::open(path).map_err(|e| SampleProblem::Unsupported(e.to_string()))?;
    Ok(reader.len() as usize * std::mem::size_of::<f32>())
}

/// Decodes a WAV file (PCM 8-32 bit or float) to interleaved f32.
pub fn decode_wav(path: &Path) -> Result<DecodedSample, SampleProblem> {
    let unsupported = |e: hound::Error| SampleProblem::Unsupported(e.to_string());
    let mut reader = hound::WavReader::open(path).map_err(unsupported)?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.channels > 2 {
        return Err(SampleProblem::Unsupported(format!(
            "{} channels",
            spec.channels
        )));
    }
    let pcm: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(unsupported)?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(unsupported)?
        }
    };
    let frames = pcm.len() / spec.channels as usize;
    let mut cues = read_cues(path).unwrap_or_default();
    cues.retain(|&c| c as usize <= frames);
    Ok(DecodedSample {
        rate: spec.sample_rate,
        channels: spec.channels as u8,
        pcm: pcm.into(),
        cues: cues.into(),
    })
}

/// Reads the sample offsets of a WAV `cue ` chunk (bytes 20..24 of each
/// 24-byte cue point), sorted and deduplicated. Empty when there is none.
fn read_cues(path: &Path) -> std::io::Result<Vec<u32>> {
    let data = std::fs::read(path)?;
    let mut cues = Vec::new();
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Ok(cues);
    }
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"cue " && size >= 4 && body + size <= data.len() {
            let count = u32::from_le_bytes(data[body..body + 4].try_into().unwrap()) as usize;
            for i in 0..count {
                let point = body + 4 + i * 24;
                if point + 24 > body + size {
                    break;
                }
                cues.push(u32::from_le_bytes(
                    data[point + 20..point + 24].try_into().unwrap(),
                ));
            }
            cues.sort_unstable();
            cues.dedup();
            break;
        }
        // Chunks are padded to an even size.
        pos = body + size + (size & 1);
    }
    Ok(cues)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, channels: u16, bits: u16, frames: &[i32]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 44100,
            bits_per_sample: bits,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &f in frames {
            w.write_sample(f).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn decodes_16_and_24_bit() {
        let dir = std::env::temp_dir().join(format!("zt-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.wav");
        write_wav(&a, 1, 16, &[0, 16384, -32768]);
        let s = decode_wav(&a).unwrap();
        assert_eq!((s.rate, s.channels), (44100, 1));
        assert_eq!(&*s.pcm, &[0.0, 0.5, -1.0]);
        let b = dir.join("b.wav");
        write_wav(&b, 2, 24, &[4194304, -4194304]);
        let s = decode_wav(&b).unwrap();
        assert_eq!(s.channels, 2);
        assert_eq!(&*s.pcm, &[0.5, -0.5]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// With ZT_M8_ROOT (a copy of the SD tree) and ZT_TEST_SONG, loads the
    /// samples of a real song and reports what it decoded.
    #[test]
    fn loads_a_real_song_when_configured() {
        let (Some(root), Some(song)) = (
            std::env::var_os("ZT_M8_ROOT"),
            std::env::var_os("ZT_TEST_SONG"),
        ) else {
            return;
        };
        let bytes = std::fs::read(song).unwrap();
        let song = Song::read_from_reader(&mut m8fmt::reader::Reader::new(bytes)).unwrap();
        let mut bank = SampleBank::new(root, DEFAULT_BUDGET);
        let problems = bank.load(&song);
        let loaded = bank
            .slots
            .iter()
            .filter(|s| matches!(s, Some(Ok(_))))
            .count();
        println!(
            "loaded {loaded} slots, {} KiB, problems {problems:?}",
            bank.used() / 1024
        );
        assert!(problems.is_empty());
        let with_cues = bank.cues().iter().filter(|c| !c.is_empty()).count();
        println!("slots with cue markers: {with_cues}");
    }

    #[test]
    fn resolves_bundled_paths_from_the_song_folder() {
        let root = std::env::temp_dir().join(format!("zt-bundle-{}", std::process::id()));
        let song_dir = root.join("Songs").join("Pack");
        std::fs::create_dir_all(song_dir.join("Samples")).unwrap();
        std::fs::create_dir_all(root.join("Samples")).unwrap();
        std::fs::write(song_dir.join("Samples").join("a.wav"), b"x").unwrap();
        std::fs::write(root.join("Samples").join("b.wav"), b"x").unwrap();
        let mut bank = SampleBank::new(&root, DEFAULT_BUDGET);
        bank.set_song_dir(Some(song_dir.clone()));
        let real = |p: PathBuf| p.canonicalize().unwrap();
        assert_eq!(
            bank.resolve("Samples/a.wav"),
            Ok(real(song_dir.join("Samples/a.wav")))
        );
        assert_eq!(
            bank.resolve("Samples/b.wav"),
            Ok(real(root.join("Samples/b.wav")))
        );
        assert_eq!(bank.resolve("/Samples/a.wav"), Err(SampleProblem::Missing));
        bank.set_song_dir(Some(std::env::temp_dir()));
        assert_eq!(bank.resolve("Samples/a.wav"), Err(SampleProblem::Missing));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn over_budget_is_found_from_the_header() {
        let dir = std::env::temp_dir().join(format!("zt-budget-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..2_000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        assert_eq!(decoded_size(&path), Ok(2_000 * 4));
        assert_eq!(decode_wav(&path).unwrap().bytes(), 2_000 * 4);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_paths_outside_the_root() {
        let bank = SampleBank::new("/tmp", DEFAULT_BUDGET);
        assert_eq!(
            bank.resolve("/Samples/../../etc/passwd"),
            Err(SampleProblem::OutsideRoot)
        );
    }
}
