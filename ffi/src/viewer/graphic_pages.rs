//! SONG and TRK for the app's graphic views (out of the
//! text-only look). The viewer hands over what each page shows; the app draws
//! it with icons, large type and bars.

use super::pages::{Page, instrument_summary, synth_params};
use super::{Viewer, write_c_string};
use m8fmt::Instrument;
use std::ffi::c_char;

const TEXT: usize = 16;
const FILE: usize = 48;

/// SONG (mirrored in zt_viewer.h as ZtSongPage).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SongPage {
    pub name: [c_char; TEXT],
    pub file: [c_char; FILE],
    pub tempo: f32,
    pub transpose: u8,
    /// 0 NAME, 1 TEMPO, 2 TRANSPOSE, 3 SAVE, 4 NEW, 5 LOAD, 6 RENDER.
    pub field: u8,
    pub editing: u8,
    pub dirty: u8,
    /// Song rows up to the last one in use; chains, phrases, instruments,
    /// tables in use.
    pub rows: u16,
    pub chains: u16,
    pub phrases: u16,
    pub instruments: u16,
}

/// One TRK track or POOL slot.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TrackRow {
    /// Instrument slot, or -1.
    pub instrument: i16,
    /// 0 none, 1 WAVSYNTH, 2 MACROSYNTH, 3 SAMPLER, 4 FMSYNTH, 5 HYPERSYNTH,
    /// 6 MIDI OUT, 7 EXTERNAL.
    pub kind: u8,
    /// Bit 0 muted, bit 1 soloed, bit 2 playing.
    pub flags: u8,
    pub name: [c_char; TEXT],
    pub detail: [c_char; TEXT],
    /// DRY, MFX, DEL, REV (the pool), EQ (0x80 none).
    pub sends: [u8; 4],
    pub eq: u8,
    /// The note the track plays now (C-4, OFF), empty when stopped.
    pub note: [c_char; 4],
}

pub const POOL_ROWS: usize = 6;

/// TRK (mirrored as ZtTracksPage): the eight tracks, then a window of the
/// POOL around the cursor.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TracksPage {
    pub tracks: [TrackRow; 8],
    pub pool: [TrackRow; POOL_ROWS],
    /// First pool slot shown.
    pub pool_first: u8,
    /// Cursor: 0-7 a track, 8+ the pool slot (cursor - 8).
    pub cursor: u16,
    /// Pool column: 0 NAME, 1 DRY, 2 MFX, 3 DEL, 4 REV, 5 EQ.
    pub column: u8,
    pub editing: u8,
}

fn text<const N: usize>(s: &str) -> [c_char; N] {
    let mut out = [0 as c_char; N];
    // SAFETY: `out` has N writable bytes.
    unsafe { write_c_string(s, out.as_mut_ptr(), N) };
    out
}

fn kind_of(instrument: &Instrument) -> u8 {
    match instrument {
        Instrument::None => 0,
        Instrument::WavSynth(_) => 1,
        Instrument::MacroSynth(_) => 2,
        Instrument::Sampler(_) => 3,
        Instrument::FMSynth(_) => 4,
        Instrument::HyperSynth(_) => 5,
        Instrument::MIDIOut(_) => 6,
        Instrument::External(_) => 7,
    }
}

impl Viewer {
    fn track_row(&self, slot: Option<u8>, flags: u8) -> TrackRow {
        let mut row = TrackRow {
            instrument: -1,
            kind: 0,
            flags,
            name: [0; TEXT],
            detail: [0; TEXT],
            sends: [0; 4],
            eq: 0x80,
            note: [0; 4],
        };
        let Some(slot) = slot else { return row };
        let Some(inst) = self.song().instruments.get(usize::from(slot)) else {
            return row;
        };
        let (_, name, detail) = instrument_summary(inst);
        row.instrument = i16::from(slot);
        row.kind = kind_of(inst);
        row.name = text(&name);
        row.detail = text(&detail);
        if let Some(p) = synth_params(inst) {
            row.sends = [p.mixer_dry, p.mixer_mfx, p.mixer_delay, p.mixer_reverb];
            row.eq = p.associated_eq;
        }
        row
    }

    /// The last note set in the phrase a track plays, up to its current step.
    fn track_note(&self, t: usize) -> Option<String> {
        let s = self
            .engine()
            .and_then(|e| e.track_status(t as u8))
            .filter(|s| s.active != 0)?;
        let steps = &self.song().phrases.get(s.phrase as usize)?.steps;
        let step = (s.phrase_step as usize).min(steps.len() - 1);
        let st = steps[..=step].iter().rev().find(|st| st.note.0 != 0xFF)?;
        Some(st.note.to_string())
    }

    pub fn song_page(&self) -> SongPage {
        let song = self.song();
        let used = |n: usize, f: &dyn Fn(usize) -> bool| (0..n).filter(|&i| f(i)).count() as u16;
        let rows = song
            .song
            .steps
            .chunks(8)
            .rposition(|row| row.iter().any(|&c| c != 0xFF))
            .map_or(0, |r| r as u16 + 1);
        let chains = used(song.chains.len(), &|i| {
            song.chains[i].steps.iter().any(|s| s.phrase != 0xFF)
        });
        let phrases = used(song.phrases.len(), &|i| {
            song.phrases[i]
                .steps
                .iter()
                .any(|s| s.note.0 != 0xFF || s.instrument != 0xFF)
        });
        let instruments = used(song.instruments.len(), &|i| {
            !matches!(song.instruments[i], Instrument::None)
        });
        let file = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        SongPage {
            name: text(song.name.trim_end_matches(['\0', ' '])),
            file: text(&file),
            tempo: song.tempo,
            transpose: song.transpose,
            field: self.song_field as u8,
            editing: u8::from(self.edit_mode.is_some()),
            dirty: u8::from(self.editor.is_dirty()),
            rows,
            chains,
            phrases,
            instruments,
        }
    }

    pub fn tracks_page(&self) -> TracksPage {
        let (mute, solo) = (self.mute, self.solo);
        let playing = self.playing_rows();
        let tracks = std::array::from_fn(|t| {
            let flags = u8::from(mute & (1 << t) != 0)
                | (u8::from(solo & (1 << t) != 0) << 1)
                | (u8::from(playing[t].is_some()) << 2);
            let mut row = self.track_row(self.track_instrument(t), flags);
            if let Some(note) = self.track_note(t) {
                row.note = text(&note);
            }
            row
        });
        let slots = self.song().instruments.len();
        let cursor_slot = self.tracks_row.saturating_sub(8);
        let first = cursor_slot
            .saturating_sub(POOL_ROWS / 2)
            .min(slots.saturating_sub(POOL_ROWS));
        let pool = std::array::from_fn(|i| {
            let slot = first + i;
            if slot < slots {
                self.track_row(Some(slot as u8), 0)
            } else {
                self.track_row(None, 0)
            }
        });
        TracksPage {
            tracks,
            pool,
            pool_first: first as u8,
            cursor: self.tracks_row as u16,
            column: self.pool_col as u8,
            editing: u8::from(self.edit_mode.is_some()),
        }
    }
}

/// SONG for the graphic view; 1 when SONG is on screen.
///
/// # Safety
///
/// `viewer` must be live; `out` must point to a writable `ZtSongPage`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_song_page(viewer: *const Viewer, out: *mut SongPage) -> i32 {
    // SAFETY: the caller passes a live viewer and a writable struct.
    let (Some(v), Some(out)) = (unsafe { viewer.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    if v.picker.is_some() || v.page != Page::Song {
        return 0;
    }
    *out = v.song_page();
    1
}

/// TRK for the graphic view; 1 when TRK is on screen.
///
/// # Safety
///
/// `viewer` must be live; `out` must point to a writable `ZtTracksPage`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_tracks_page(viewer: *const Viewer, out: *mut TracksPage) -> i32 {
    // SAFETY: the caller passes a live viewer and a writable struct.
    let (Some(v), Some(out)) = (unsafe { viewer.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    if v.picker.is_some() || v.page != Page::Tracks {
        return 0;
    }
    *out = v.tracks_page();
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_and_tracks_pages_describe_the_song() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Song;
        let s = v.song_page();
        assert!(s.tempo > 0.0 && s.rows > 0 && s.chains > 0 && s.instruments > 0);
        v.page = Page::Tracks;
        v.tracks_row = 8 + 0x40;
        let t = v.tracks_page();
        assert_eq!(t.cursor, 8 + 0x40);
        let shown = usize::from(t.pool_first)..usize::from(t.pool_first) + POOL_ROWS;
        assert!(shown.contains(&0x40));
    }
}
