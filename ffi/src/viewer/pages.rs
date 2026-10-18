//! The app pages: TIMELINE (the SONG view turned 90
//! degrees), MIXER, TRACKS and SONG. Each page renders a grid of exactly the
//! size the app asks for, with highlight spans for what plain text cannot
//! show (playing chains in bold, muted tracks, accents).

use super::{BTN_DOWN, BTN_EDIT, BTN_LEFT, BTN_OPTION, BTN_RIGHT, BTN_SHIFT, BTN_UP, Dir, Kind};
use super::{Request, Viewer};
use m8edit::{Command, Granularity};
use m8fmt::{
    AnalogInputSettings, Chain, EqBand, Equ, FxKind, InputMixerSettings, Instrument,
    LimiterParameter, OttConfiguration, Phrase, Scale, SynthParams, Table, Version,
};
use m8view::{Field, InstrumentParameter};
use std::sync::LazyLock;

/// Pages switched by key 4. APP lives in the app shell (it needs no song).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Timeline,
    Mixer,
    Tracks,
    Song,
}

impl Page {
    pub fn from_index(i: i32) -> Option<Self> {
        match i {
            0 => Some(Self::Timeline),
            1 => Some(Self::Mixer),
            2 => Some(Self::Tracks),
            3 => Some(Self::Song),
            _ => None,
        }
    }

    pub fn index(self) -> i32 {
        self as i32
    }
}

/// How a highlight span is drawn (mirrored in zt_viewer.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Style {
    /// Bold, bright: the chain each track is playing.
    Playing = 1,
    /// Accent colour: track labels, row numbers.
    Accent = 2,
    /// Muted track.
    Muted = 3,
    /// Soloed track.
    Solo = 4,
    /// Dimmed: empty cells and hints.
    Dim = 5,
}

/// One highlight: screen line, column, width, style.
pub type Highlight = (usize, usize, usize, Style);

/// A page frame sized to the grid: text lines, cursor, highlights.
#[derive(Debug, Default)]
pub struct PageFrame {
    pub lines: Vec<String>,
    pub cursor: Option<(usize, usize, usize)>,
    pub highlights: Vec<Highlight>,
}

/// TRK pool columns (column, width): NAME, DRY, MFX, DEL, REV, EQ.
pub(super) const POOL_COLUMNS: [(usize, usize); 6] =
    [(8, 12), (21, 2), (24, 2), (27, 2), (30, 2), (33, 2)];

/// Song rows the graphic TIMELINE can ask for at once.
pub const TIMELINE_MAX: usize = 64;

/// TIMELINE snapshot for the app's graphic view (mirrored in zt_viewer.h as
/// ZtTimeline). Rows are song rows; -1 means none.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TimelineView {
    pub first_row: i32,
    pub count: i32,
    pub cursor_row: i32,
    pub cursor_track: i32,
    /// Selection: first row, last row, first track, last track.
    pub sel: [i32; 4],
    /// Song row each track is playing.
    pub playing: [i32; 8],
    pub mute: u8,
    pub solo: u8,
    /// Chain per track and visible row: `cells[track * TIMELINE_MAX + i]`, FF empty.
    pub cells: [u8; 8 * TIMELINE_MAX],
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            first_row: 0,
            count: 0,
            cursor_row: -1,
            cursor_track: -1,
            sel: [-1; 4],
            playing: [-1; 8],
            mute: 0,
            solo: 0,
            cells: [0xFF; 8 * TIMELINE_MAX],
        }
    }
}

/// MIXER snapshot for the app's graphic view (mirrored in zt_viewer.h as
/// ZtMixer). Strip 8 is the master.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct MixerView {
    pub selected: i32,
    /// 1 while LIMIT is on: the arrows move the limiter level.
    pub limit_mode: i32,
    /// Track volumes 0-7, master volume 8.
    pub volumes: [u8; 9],
    pub mute: u8,
    pub solo: u8,
    pub limiter: u8,
}

/// TIMELINE: track label width and song-row cell width, in columns.
const LABEL: usize = 3;
const CELL: usize = 3;
/// MIXER: columns per strip; strip 8 is the master.
const STRIP: usize = 4;
/// MIXER: lines of the meter area at the top of the page.
pub const MIXER_METER_LINES: usize = 6;

/// Builds a clean, empty 6.6 song from the format model. This keeps NEW
/// independent of private or third-party song files in the public build.
pub fn empty_song_bytes() -> Result<Vec<u8>, String> {
    let version = Version {
        major: 6,
        minor: 6,
        patch: 2,
    };
    let end = m8fmt::V4_1_OFFSETS
        .row_bookmark_offset
        .ok_or("Missing 6.6 row bookmark offset")?
        + m8fmt::SongSteps::ROW_COUNT;
    let mut writer = m8fmt::writer::Writer::new(vec![0; end]);
    version.write(&mut writer);

    // Header fields whose format defaults are not all zero.
    writer.seek(148);
    writer.write_string("NEW SONG", 12);
    writer.seek(143);
    writer.write_bytes(&120.0_f32.to_le_bytes());
    writer.seek(206);
    writer.write(0xE0);
    writer.skip(1);
    writer.write_bytes(&[0xE0; 8]);
    writer.seek(164);
    writer.write_bytes(&[9, 1, 0, 17, 11]);
    writer.write_bytes(&[1, 2, 3, 4, 5, 6, 7, 8]);
    writer.skip(8);
    writer.write_bytes(&[1, 1]);

    writer.seek(m8fmt::V4_1_OFFSETS.groove);
    for _ in 0..m8fmt::Song::N_GROOVES {
        writer.write_bytes(&[
            6, 6, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF,
        ]);
    }
    writer.seek(m8fmt::V4_1_OFFSETS.song);
    writer.write_bytes(&[0xFF; m8fmt::SongSteps::TRACK_COUNT * m8fmt::SongSteps::ROW_COUNT]);

    writer.seek(m8fmt::V4_1_OFFSETS.phrases);
    let mut phrase = Phrase::default_ver(version);
    phrase.clear();
    for _ in 0..m8fmt::Song::N_PHRASES {
        phrase.write(&mut writer);
    }
    writer.seek(m8fmt::V4_1_OFFSETS.chains);
    let chain = Chain::default();
    for _ in 0..m8fmt::Song::N_CHAINS {
        chain.write(&mut writer);
    }
    writer.seek(m8fmt::V4_1_OFFSETS.table);
    let table = Table::default_ver(version);
    for _ in 0..m8fmt::Song::N_TABLES {
        table.write(&mut writer);
    }
    writer.seek(m8fmt::V4_1_OFFSETS.instruments);
    for _ in 0..m8fmt::Song::N_INSTRUMENTS {
        let slot = writer.pos();
        Instrument::None.write(version, &mut writer);
        writer.seek(slot + Instrument::INSTRUMENT_MEMORY_SIZE);
    }
    let bytes = writer.finish();
    let mut song = m8fmt::Song::read_from_reader(&mut m8fmt::reader::Reader::new(bytes.clone()))
        .map_err(|error| format!("Generate empty song: {error:?}"))?;
    song.name = "NEW SONG".into();
    song.directory = "/Songs/".into();
    song.tempo = 120.0;
    song.mixer_settings.chorus_volume = 0xE0;
    song.mixer_settings.delay_volume = 0xE0;
    song.mixer_settings.reverb_volume = 0xE0;
    song.mixer_settings.analog_input = AnalogInputSettings::Stereo(InputMixerSettings {
        volume: 0,
        mfx: 0,
        delay: 0,
        reverb: 0,
    });
    song.mixer_settings.usb_input = InputMixerSettings {
        volume: 0,
        mfx: 0,
        delay: 0,
        reverb: 0,
    };
    song.mixer_settings.dj_filter = 0x80;
    song.mixer_settings.dj_peak = 0;
    song.mixer_settings.dj_filter_type = 0;
    song.mixer_settings.limiter = LimiterParameter {
        level: 0,
        attack_release: Some((0, 16, false)),
    };
    song.mixer_settings.ott_level = Some(0);
    song.midi_settings.record_note_channel = 9;
    song.midi_settings.record_note_velocity = true;
    song.midi_settings.control_map_channel = 17;
    song.midi_settings.song_row_cue_channel = 11;
    song.midi_settings.track_input_channel = [1, 2, 3, 4, 5, 6, 7, 8];
    song.midi_settings.track_input_program_change = true;
    song.midi_settings.track_input_mode = 1;
    song.effects_settings.mfx_kind = Some(FxKind::Chorus);
    song.effects_settings.chorus_mod_depth = 64;
    song.effects_settings.chorus_mod_freq = 128;
    song.effects_settings.chorus_width = 0xFF;
    song.effects_settings.chorus_reverb_send = 0;
    song.effects_settings.delay_time_l = 48;
    song.effects_settings.delay_time_r = 48;
    song.effects_settings.delay_feedback = 128;
    song.effects_settings.delay_width = 0xFF;
    song.effects_settings.delay_reverb_send = 0;
    song.effects_settings.reverb_size = 0xFF;
    song.effects_settings.reverb_damping = 192;
    song.effects_settings.reverb_mod_depth = 16;
    song.effects_settings.reverb_mod_freq = 0xFF;
    song.effects_settings.reverb_width = 0xFF;
    song.effects_settings.reverb_shimmer = Some(0);
    song.effects_settings.ott_configuration = Some(OttConfiguration {
        time: 128,
        color: 128,
    });
    for groove in &mut song.grooves {
        groove.steps[0] = 6;
        groove.steps[1] = 6;
        groove.ppqn = Some(0);
    }
    for eq in &mut song.eqs {
        *eq = Equ {
            low: EqBand::default_low(),
            mid: EqBand::default_mid(),
            high: EqBand::default_high(),
        };
    }
    set_empty_song_scales(&mut song.scales);
    let mut output = m8fmt::writer::Writer::new(bytes);
    song.write(&mut output)
        .map_err(|error| format!("Serialize empty song: {error}"))?;
    Ok(output.finish())
}

fn set_empty_song_scales(scales: &mut [Scale]) {
    // These are the stock 6.6 scale slots, expressed as model fields rather
    // than copied serialized bytes. Entries list (note index, semitone, cents).
    type ScaleSpec = (&'static str, u8, u16, &'static [(u8, u8, u8)]);
    const SPECS: [ScaleSpec; 16] = [
        ("CHROMATIC", 0, 0x0FFF, &[]),
        ("", 0, 0, &[(1, 181, 10)]),
        ("", 0x0F, 0x0FFF, &[(0, 0xFF, 0xFF), (3, 0xAD, 5)]),
        (
            "",
            0x0F,
            0x0FFF,
            &[
                (0, 0xFF, 0xFF),
                (1, 0xFF, 0xFF),
                (2, 0xFF, 0xFF),
                (5, 0xAD, 6),
            ],
        ),
        (
            "",
            4,
            0x0E41,
            &[
                (0, 0xFF, 0xFF),
                (1, 0xFF, 0xFF),
                (2, 0xFF, 0xFF),
                (3, 0xFF, 0xFF),
                (4, 0xFF, 0xFF),
                (7, 213, 10),
            ],
        ),
        (
            "",
            5,
            0x094C,
            &[
                (0, 68, 73),
                (1, 65, 78),
                (2, 0xFF, 0xFF),
                (3, 0xFF, 0xFF),
                (4, 0xFF, 0xFF),
                (5, 0xFF, 0xFF),
                (6, 0xFF, 0xFF),
                (9, 181, 6),
            ],
        ),
        (
            "",
            0,
            0,
            &[
                (1, 77, 73),
                (2, 88, 79),
                (3, 76, 89),
                (4, 68, 73),
                (5, 65, 78),
                (6, 0xFF, 0xFF),
                (7, 0xFF, 0xFF),
                (8, 0xFF, 0xFF),
                (11, 107, 5),
            ],
        ),
        (
            "",
            0,
            0,
            &[
                (3, 76, 79),
                (4, 67, 82),
                (5, 73, 65),
                (6, 78, 0xFF),
                (7, 0xFF, 0xFF),
                (8, 0xFF, 0xFF),
                (9, 0xFF, 0xFF),
                (10, 0xFF, 0xFF),
            ],
        ),
        (
            "",
            0,
            0,
            &[
                (5, 80, 69),
                (6, 78, 84),
                (7, 65, 84),
                (8, 79, 78),
                (9, 73, 67),
                (10, 0xFF, 0xFF),
                (11, 0xFF, 0xFF),
            ],
        ),
        (
            "ATONIC",
            0,
            0,
            &[
                (7, 77, 73),
                (8, 78, 79),
                (9, 82, 32),
                (10, 80, 69),
                (11, 78, 84),
            ],
        ),
        ("BLUES", 0, 0, &[(9, 77, 65), (10, 74, 79), (11, 82, 32)]),
        ("NOR BLUES", 0, 0, &[(11, 77, 73)]),
        ("", 0, 0, &[(0, 205, 6)]),
        ("", 0x0F, 0x0FFF, &[(2, 0x8D, 1)]),
        (
            "",
            0x0F,
            0x0FFF,
            &[(0, 0xFF, 0xFF), (1, 0xFF, 0xFF), (4, 0xA3, 1)],
        ),
        (
            "",
            5,
            0x034F,
            &[
                (0, 72, 73),
                (1, 0xFF, 0xFF),
                (2, 0xFF, 0xFF),
                (3, 0xFF, 0xFF),
                (6, 0xA3, 4),
            ],
        ),
    ];
    for (index, scale) in scales.iter_mut().enumerate() {
        let (name, high_bits, enabled_mask, offsets) = SPECS[index];
        scale.number = index as u8;
        scale.name = name.into();
        scale.map_high_bits = high_bits;
        for (note_index, note) in scale.notes.iter_mut().enumerate() {
            note.enabled = enabled_mask & (1 << note_index) != 0;
            note.semitone_byte = 0;
            note.cents_byte = 0;
        }
        for &(note_index, semitone, cents) in offsets {
            let note = &mut scale.notes[usize::from(note_index)];
            note.semitone_byte = semitone;
            note.cents_byte = cents;
        }
    }
}

pub(crate) static EMPTY_SONG: LazyLock<Vec<u8>> =
    LazyLock::new(|| empty_song_bytes().expect("generated empty song must parse and serialize"));

/// SONG page rows.
const SONG_FIELDS: [&str; 7] = [
    "NAME",
    "TEMPO",
    "TRANSPOSE",
    "SAVE",
    "NEW",
    "LOAD",
    "RENDER",
];

fn put(line: &mut String, col: usize, text: &str) {
    let mut chars: Vec<char> = line.chars().collect();
    if chars.len() < col + text.len() {
        chars.resize(col + text.len(), ' ');
    }
    for (i, ch) in text.chars().enumerate() {
        chars[col + i] = ch;
    }
    *line = chars.into_iter().collect();
}

fn hex(b: u8) -> String {
    if b == 0xFF {
        "--".into()
    } else {
        format!("{b:02X}")
    }
}

/// Joins spans of the same style on a line that are at most one column
/// apart (e.g. a row of empty `--` cells), so the app needs fewer overlays.
fn merge_highlights(mut spans: Vec<Highlight>) -> Vec<Highlight> {
    spans.sort_by_key(|&(l, c, _, _)| (l, c));
    let mut out: Vec<Highlight> = Vec::with_capacity(spans.len());
    for s in spans {
        if let Some(last) = out.last_mut()
            && last.0 == s.0
            && last.3 == s.3
            && s.1 <= last.1 + last.2 + 1
        {
            last.2 = (s.1 + s.2).max(last.1 + last.2) - last.1;
            continue;
        }
        out.push(s);
    }
    out
}

pub(super) fn synth_params(instrument: &Instrument) -> Option<&SynthParams> {
    match instrument {
        Instrument::WavSynth(x) => Some(&x.synth_params),
        Instrument::MacroSynth(x) => Some(&x.synth_params),
        Instrument::Sampler(x) => Some(&x.synth_params),
        Instrument::FMSynth(x) => Some(&x.synth_params),
        Instrument::HyperSynth(x) => Some(&x.synth_params),
        _ => None,
    }
}

/// Type, name and the engine detail of an instrument, for the TRACKS page.
pub(super) fn instrument_summary(instrument: &Instrument) -> (String, String, String) {
    let (kind, name, detail) = match instrument {
        Instrument::WavSynth(x) => ("WAVSYNTH", x.name.clone(), format!("{:?}", x.shape)),
        Instrument::MacroSynth(x) => ("MACROSYN", x.name.clone(), format!("{:?}", x.shape)),
        Instrument::Sampler(x) => {
            let file = x.sample_path.rsplit('/').next().unwrap_or("").to_string();
            ("SAMPLER", x.name.clone(), file)
        }
        Instrument::FMSynth(x) => ("FMSYNTH", x.name.clone(), format!("ALG {:02X}", x.algo.0)),
        Instrument::HyperSynth(x) => (
            "HYPERSYN",
            x.name.clone(),
            x.shape.map(|s| format!("{s:?}")).unwrap_or_default(),
        ),
        Instrument::MIDIOut(x) => ("MIDIOUT", x.name.clone(), String::new()),
        Instrument::External(x) => ("EXTERNAL", x.name.clone(), String::new()),
        Instrument::None => ("--", String::new(), String::new()),
    };
    (
        kind.to_string(),
        name.trim_end_matches(['\0', ' ']).to_string(),
        detail.to_uppercase(),
    )
}

impl Viewer {
    /// Song rows visible in the TIMELINE for a grid `cols` wide.
    fn timeline_visible(cols: usize) -> usize {
        (cols.saturating_sub(LABEL) / CELL).max(1)
    }

    /// Keeps the TIMELINE cursor column on screen, `visible` song rows wide.
    fn timeline_scroll(&self, visible: usize) -> usize {
        let visible = visible.max(1);
        let mut left = self.song_left.get();
        if self.song_row < left {
            left = self.song_row;
        } else if self.song_row >= left + visible {
            left = self.song_row + 1 - visible;
        }
        self.song_left.set(left);
        left
    }

    fn track_flags(&self) -> (u8, u8) {
        (self.mute, self.solo)
    }

    fn track_style(&self, t: usize) -> Style {
        let (mute, solo) = self.track_flags();
        if mute & (1 << t) != 0 {
            Style::Muted
        } else if solo & (1 << t) != 0 {
            Style::Solo
        } else {
            Style::Accent
        }
    }

    /// Song row each track is playing, if it is.
    pub(super) fn playing_rows(&self) -> [Option<usize>; 8] {
        let mut out = [None; 8];
        if let Some(engine) = self.engine() {
            for (t, slot) in out.iter_mut().enumerate() {
                *slot = engine
                    .track_status(t as u8)
                    .filter(|s| s.active != 0)
                    .map(|s| s.song_row as usize);
            }
        }
        out
    }

    pub(super) fn page_frame(&self, rows: usize, cols: usize) -> PageFrame {
        let mut f = match self.page {
            Page::Timeline => self.timeline_frame(rows, cols),
            Page::Mixer => self.mixer_frame(rows, cols),
            Page::Tracks => self.tracks_frame(rows, cols),
            Page::Song => self.song_page_frame(rows, cols),
        };
        f.lines.resize(rows, String::new());
        for l in f.lines.iter_mut() {
            let n = l.chars().count();
            if n > cols {
                *l = l.chars().take(cols).collect();
            }
        }
        f.highlights = merge_highlights(std::mem::take(&mut f.highlights));
        // The message (SAVED, EMPTY...) goes at the right end of the last line.
        if !self.message.is_empty()
            && let Some(last) = f.lines.last_mut()
        {
            let col = cols.saturating_sub(self.message.len() + 2);
            put(last, col, &format!(" {} ", self.message));
            let last_line = rows.saturating_sub(1);
            f.highlights
                .retain(|h| h.0 != last_line || h.1 + h.2 <= col);
            *last = last.chars().take(cols).collect();
        }
        f
    }

    /// TIMELINE: line 0 the song row numbers, lines 1-8 the tracks, then a
    /// preview of the chain under the cursor. With fewer than 10 lines the
    /// preview goes, with fewer than 9 the row numbers too.
    fn timeline_frame(&self, rows: usize, cols: usize) -> PageFrame {
        let mut f = PageFrame::default();
        let visible = Self::timeline_visible(cols);
        let left = self.timeline_scroll(visible);
        let playing = self.playing_rows();
        let first = usize::from(rows >= 9);
        self.timeline_first.set(first);
        let mut header = " ".repeat(LABEL);
        for i in 0..visible {
            let r = left + i;
            if r >= super::SONG_ROWS {
                break;
            }
            put(&mut header, LABEL + i * CELL, &format!("{r:02X}"));
            if first == 0 {
                continue;
            }
            if playing.contains(&Some(r)) {
                f.highlights.push((0, LABEL + i * CELL, 2, Style::Playing));
            } else {
                f.highlights.push((0, LABEL + i * CELL, 2, Style::Dim));
            }
        }
        if first == 1 {
            f.lines.push(header);
        }
        let steps = &self.song().song.steps;
        for t in 0..8 {
            let line = first + t;
            let mut text = format!(" {}", t + 1);
            f.highlights.push((line, 0, 2, self.track_style(t)));
            for i in 0..visible {
                let r = left + i;
                if r >= super::SONG_ROWS {
                    break;
                }
                let cell = steps[r * 8 + t];
                let col = LABEL + i * CELL;
                put(&mut text, col, &hex(cell));
                if playing[t] == Some(r) {
                    f.highlights.push((line, col, 2, Style::Playing));
                } else if cell == 0xFF {
                    f.highlights.push((line, col, 2, Style::Dim));
                }
            }
            f.lines.push(text);
        }
        if self.song_row >= left && self.song_row < left + visible {
            f.cursor = Some((first + self.track, LABEL + (self.song_row - left) * CELL, 2));
        }
        if rows < 10 {
            return f;
        }
        // Chain preview: its phrases, as many as fit.
        if rows > 10 {
            f.lines.push(String::new());
        }
        let chain = steps[self.song_row * 8 + self.track];
        let mut preview = format!(" CHAIN {}", hex(chain));
        if chain != 0xFF {
            preview.push(' ');
            for step in &self.song().chains[chain as usize].steps {
                if preview.len() + 3 > cols {
                    break;
                }
                preview.push(' ');
                preview.push_str(&hex(step.phrase));
            }
        }
        let line = f.lines.len();
        f.highlights.push((line, 0, 9, Style::Dim));
        f.lines.push(preview);
        f
    }

    /// TIMELINE for the graphic view: `visible` song rows from the scroll
    /// position, the chain of every track in them, what plays and where the
    /// cursor and the selection are.
    pub(super) fn timeline_view(&self, visible: usize) -> TimelineView {
        let visible = visible.clamp(1, TIMELINE_MAX);
        let left = self.timeline_scroll(visible);
        let steps = &self.song().song.steps;
        let mut v = TimelineView {
            first_row: left as i32,
            cursor_row: self.song_row as i32,
            cursor_track: self.track as i32,
            sel: [-1; 4],
            playing: [-1; 8],
            mute: self.mute,
            solo: self.solo,
            ..TimelineView::default()
        };
        for (t, p) in self.playing_rows().iter().enumerate() {
            v.playing[t] = p.map_or(-1, |r| r as i32);
        }
        let count = visible.min(super::SONG_ROWS.saturating_sub(left));
        v.count = count as i32;
        for t in 0..8 {
            for i in 0..count {
                v.cells[t * TIMELINE_MAX + i] = steps[(left + i) * 8 + t];
            }
        }
        if let Some(Field::SongChain { row, track }) = self.select_anchor {
            let (row, track) = (row as usize, track as usize);
            v.sel = [
                row.min(self.song_row) as i32,
                row.max(self.song_row) as i32,
                track.min(self.track) as i32,
                track.max(self.track) as i32,
            ];
        }
        v
    }

    pub(super) fn mixer_view(&self) -> MixerView {
        let m = &self.song().mixer_settings;
        let mut volumes = [0u8; 9];
        volumes[..8].copy_from_slice(&m.track_volume);
        volumes[8] = m.master_volume;
        MixerView {
            selected: self.mixer_strip as i32,
            limit_mode: i32::from(self.limit_mode),
            volumes,
            mute: self.mute,
            solo: self.solo,
            limiter: m.limiter.level,
        }
    }

    /// TIMELINE selection rectangle in screen coordinates.
    pub(super) fn timeline_selection(&self, cols: usize) -> Option<(usize, usize, usize, usize)> {
        let Some(Field::SongChain { row, track }) = self.select_anchor else {
            return None;
        };
        let left = self.song_left.get();
        let visible = Self::timeline_visible(cols);
        let (r0, r1) = (
            (row as usize).min(self.song_row),
            (row as usize).max(self.song_row),
        );
        let (t0, t1) = (
            (track as usize).min(self.track),
            (track as usize).max(self.track),
        );
        let first = r0.max(left);
        let last = r1.min(left + visible - 1);
        (first <= last).then(|| {
            (
                self.timeline_first.get() + t0,
                self.timeline_first.get() + t1,
                LABEL + (first - left) * CELL,
                LABEL + (last - left) * CELL + 2,
            )
        })
    }

    /// MIXER: the meter area (drawn by the app), then volumes, mute/solo
    /// flags and track numbers per strip; strip 8 is the master.
    fn mixer_frame(&self, _rows: usize, _cols: usize) -> PageFrame {
        let mut f = PageFrame::default();
        f.lines.resize(MIXER_METER_LINES, String::new());
        let mixer = &self.song().mixer_settings;
        let (mute, solo) = self.track_flags();
        let (mut vol, mut flags, mut names) = (String::new(), String::new(), String::new());
        let l = MIXER_METER_LINES;
        for s in 0..9 {
            let col = 1 + s * STRIP;
            let v = if s < 8 {
                mixer.track_volume[s]
            } else {
                mixer.master_volume
            };
            put(&mut vol, col, &format!("{v:02X}"));
            if s < 8 {
                let m = mute & (1 << s) != 0;
                let so = solo & (1 << s) != 0;
                put(&mut flags, col, if m { "M" } else { "-" });
                put(&mut flags, col + 1, if so { "S" } else { "-" });
                if m {
                    f.highlights.push((l + 1, col, 1, Style::Muted));
                }
                if so {
                    f.highlights.push((l + 1, col + 1, 1, Style::Solo));
                }
                put(&mut names, col, &format!(" {}", s + 1));
                f.highlights.push((l + 2, col, 2, self.track_style(s)));
            } else {
                put(&mut names, col, "MST");
                f.highlights.push((l + 2, col, 3, Style::Accent));
            }
        }
        f.lines.push(vol);
        f.lines.push(flags);
        f.lines.push(names);
        f.cursor = Some((l, 1 + self.mixer_strip * STRIP, 2));
        f
    }

    /// Strip columns and the meter area of the MIXER, for the app's bars:
    /// (first line, last line, first column, column step).
    pub(super) fn mixer_meter_area(&self) -> Option<(usize, usize, usize, usize)> {
        (self.page == Page::Mixer).then_some((0, MIXER_METER_LINES - 1, 1, STRIP))
    }

    /// The instrument a track plays now: the last one set in the playing
    /// phrase up to the current step, else the first one of the chain the
    /// track plays (or, when stopped, of the chain on the cursor row).
    pub(super) fn track_instrument(&self, t: usize) -> Option<u8> {
        let song = self.song();
        let status = self
            .engine()
            .and_then(|e| e.track_status(t as u8))
            .filter(|s| s.active != 0);
        if let Some(s) = status
            && (s.phrase as usize) < song.phrases.len()
        {
            let steps = &song.phrases[s.phrase as usize].steps;
            let step = (s.phrase_step as usize).min(steps.len() - 1);
            let before = steps[..=step].iter().rev().find(|st| st.instrument != 0xFF);
            if let Some(st) = before {
                return Some(st.instrument);
            }
        }
        let row = status.map_or(self.song_row, |s| s.song_row as usize);
        let chain = song.song.steps[row * 8 + t];
        if chain == 0xFF {
            return None;
        }
        song.chains[chain as usize]
            .steps
            .iter()
            .filter(|st| st.phrase != 0xFF)
            .flat_map(|st| song.phrases[st.phrase as usize].steps.iter())
            .find(|st| st.instrument != 0xFF)
            .map(|st| st.instrument)
    }

    fn tracks_frame(&self, rows: usize, cols: usize) -> PageFrame {
        let mut f = PageFrame::default();
        let playing = self.playing_rows();
        for (t, playing) in playing.iter().enumerate() {
            let mut line = format!(" {} ", t + 1);
            f.highlights.push((t, 0, 2, self.track_style(t)));
            match self.track_instrument(t) {
                Some(i) => {
                    let inst = &self.song().instruments[i as usize];
                    let (kind, name, detail) = instrument_summary(inst);
                    line.push_str(&format!("{i:02X} {kind:<8} {name:<12} "));
                    if let Some(p) = synth_params(inst).filter(|p| p.filter_type != 0) {
                        line.push_str(&format!("F{:X}{:02X} ", p.filter_type, p.filter_cutoff));
                    }
                    line.push_str(&detail);
                    if playing.is_some() {
                        f.highlights.push((t, 3, 2, Style::Playing));
                    }
                }
                None => {
                    line.push_str("--");
                    f.highlights.push((t, 3, 2, Style::Dim));
                }
            }
            f.lines.push(line);
        }
        f.cursor = Some((self.tracks_row, 3, cols.saturating_sub(4).min(24)));
        self.pool_lines(&mut f);
        // The page scrolls with the cursor in the pool.
        let cursor_line = f.cursor.map_or(0, |c| c.0);
        let top = (cursor_line + 2).saturating_sub(rows.max(3));
        if top > 0 {
            f.lines.drain(..top.min(f.lines.len()));
            f.highlights.retain(|h| h.0 >= top);
            for h in f.highlights.iter_mut() {
                h.0 -= top;
            }
            f.cursor = f.cursor.map(|(l, c, w)| (l - top, c, w));
        }
        f
    }

    /// TRK below the tracks: every instrument slot with its kind, name and
    /// mixer sends (the M8's INSTRUMENT POOL), one row each.
    fn pool_lines(&self, f: &mut PageFrame) {
        let header = f.lines.len();
        f.lines
            .push(format!(" {:<3}{:<4}{:<13}DR MX DE RV EQ", "", "", "POOL"));
        f.highlights.push((header, 1, 30, Style::Accent));
        for (i, inst) in self.song().instruments.iter().enumerate() {
            let line = f.lines.len();
            let kind = match inst {
                Instrument::WavSynth(_) => "WAV",
                Instrument::MacroSynth(_) => "MAC",
                Instrument::Sampler(_) => "SMP",
                Instrument::FMSynth(_) => "FM",
                Instrument::HyperSynth(_) => "HYP",
                Instrument::MIDIOut(_) => "MID",
                Instrument::External(_) => "EXT",
                Instrument::None => "---",
            };
            let name: String = inst
                .name()
                .unwrap_or("")
                .trim_end_matches(['\0', ' '])
                .chars()
                .take(12)
                .collect();
            let mut text = format!(" {i:02X} {kind:<4}{name:<13}");
            match synth_params(inst) {
                Some(p) => {
                    for v in [p.mixer_dry, p.mixer_mfx, p.mixer_delay, p.mixer_reverb] {
                        text.push_str(&format!("{v:02X} "));
                    }
                    if p.associated_eq >= 0x80 {
                        text.push_str("--");
                    } else {
                        text.push_str(&format!("{:02X}", p.associated_eq));
                    }
                }
                None => text.push_str("-- -- -- -- --"),
            }
            f.highlights.push((line, 1, 2, Style::Accent));
            if synth_params(inst).is_none() {
                f.highlights.push((line, 4, 3, Style::Dim));
            }
            f.lines.push(text);
        }
        if self.tracks_row >= 8 {
            let line = header + 1 + (self.tracks_row - 8);
            let (col, width) = POOL_COLUMNS[self.pool_col.min(POOL_COLUMNS.len() - 1)];
            f.cursor = Some((line, col, width));
        }
    }

    /// The instrument field of the pool cell under the cursor (DRY ... EQ).
    pub(super) fn pool_field(&self) -> Option<Field> {
        if self.page != Page::Tracks || self.tracks_row < 8 {
            return None;
        }
        let parameter = match self.pool_col {
            1 => InstrumentParameter::Dry,
            2 => InstrumentParameter::ModFx,
            3 => InstrumentParameter::Delay,
            4 => InstrumentParameter::Reverb,
            5 => InstrumentParameter::Eq,
            _ => return None,
        };
        Some(Field::Instrument {
            index: (self.tracks_row - 8) as u8,
            parameter,
        })
    }

    /// The instrument of the TRK row under the cursor: the one a track plays,
    /// or the pool's.
    pub(super) fn tracks_row_instrument(&self) -> Option<u8> {
        if self.tracks_row < 8 {
            self.track_instrument(self.tracks_row)
        } else {
            Some((self.tracks_row - 8) as u8)
        }
    }

    /// ENTER on TRK: on a track or a pool NAME, open the instrument; on a
    /// pool value, edit it (ENTER again keeps it, ESC undoes it).
    pub fn tracks_enter(&mut self) {
        if self.edit_mode.take().is_some() {
            return;
        }
        if let Some(field) = self.pool_field() {
            // Only the sound types have these values.
            if m8edit::read_field(self.song(), field).is_ok() {
                self.edit_mode = Some(0);
            } else {
                self.message = "NO MIXER".into();
            }
            return;
        }
        self.open_tracks_instrument();
    }

    fn open_tracks_instrument(&mut self) {
        self.message.clear();
        if let Some(i) = self.tracks_row_instrument() {
            self.instrument = i;
            self.instrument_field = None;
            self.kind = Kind::Instrument;
            self.page = Page::Timeline;
        }
    }

    fn song_page_frame(&self, _rows: usize, _cols: usize) -> PageFrame {
        let mut f = PageFrame::default();
        let song = self.song();
        let values = [
            song.name.trim_end_matches(['\0', ' ']).to_string(),
            format!("{:.2}", song.tempo),
            format!("{:02X}", song.transpose),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ];
        for (i, (label, value)) in SONG_FIELDS.iter().zip(values.iter()).enumerate() {
            let line = if i < 3 {
                format!(" {label:<10}{value}")
            } else {
                format!(" [{label}]")
            };
            if i == 3 {
                f.lines.push(String::new());
            }
            f.highlights.push((
                f.lines.len(),
                1,
                label.len() + 2 * (i >= 3) as usize,
                Style::Accent,
            ));
            f.lines.push(line);
        }
        let line = if self.song_field < 3 {
            self.song_field
        } else {
            self.song_field + 1
        };
        f.cursor = Some(if self.song_field < 3 {
            let width = values[self.song_field].len().max(2);
            (line, 11, width)
        } else {
            (line, 1, SONG_FIELDS[self.song_field].len() + 2)
        });
        let file = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        f.lines.push(String::new());
        let info = format!(" FILE {}", file.unwrap_or_default());
        f.highlights
            .push((f.lines.len(), 0, info.len(), Style::Dim));
        f.lines.push(info);
        f
    }

    /// Input on a page other than TIMELINE (and TIMELINE arrows), after the
    /// global keys (PLAY, SHIFT+PLAY). Returns true when it handled the keys.
    pub(super) fn page_input(&mut self, held: u32, pressed: u32) -> bool {
        let edit = held & BTN_EDIT != 0;
        let shift = held & BTN_SHIFT != 0;
        let option = held & BTN_OPTION != 0;
        let dir = [
            (BTN_UP, Dir::Up),
            (BTN_DOWN, Dir::Down),
            (BTN_LEFT, Dir::Left),
            (BTN_RIGHT, Dir::Right),
        ]
        .into_iter()
        .find(|(bit, _)| pressed & bit != 0)
        .map(|(_, d)| d);
        match self.page {
            Page::Timeline => false,
            Page::Mixer => {
                if pressed & BTN_OPTION != 0 && !edit {
                    let bit = 1u8 << self.mixer_strip.min(7);
                    if self.mixer_strip < 8 {
                        if shift {
                            self.toggle_solo(bit);
                        } else {
                            self.toggle_mute(bit);
                        }
                        self.apply_track_flags();
                    }
                    return true;
                }
                let Some(d) = dir else { return true };
                let field = if self.mixer_strip < 8 {
                    Field::MixerTrackVolume {
                        track: self.mixer_strip as u8,
                    }
                } else {
                    Field::MixerMasterVolume
                };
                // LEFT/RIGHT pick the strip, UP/DOWN its volume; with LIMIT
                // on, UP/DOWN the limiter.
                match d {
                    Dir::Left if !edit => self.mixer_strip = self.mixer_strip.saturating_sub(1),
                    Dir::Right if !edit => self.mixer_strip = (self.mixer_strip + 1).min(8),
                    Dir::Up | Dir::Down if self.limit_mode => {
                        self.adjust_field(Field::MixerLimiter, d);
                    }
                    _ => self.adjust_field(field, d),
                }
                true
            }
            Page::Tracks => {
                // A pool value in edit mode (ENTER), or EDIT held: the arrows
                // change it, UP/DOWN by 16, LEFT/RIGHT by 1.
                if let (Some(d), Some(field)) = (dir, self.pool_field())
                    && (edit || self.edit_mode.is_some())
                {
                    self.adjust_field(field, d);
                    return true;
                }
                // EDIT+UP/DOWN on a pool NAME moves the instrument, as on the M8.
                if edit
                    && self.tracks_row >= 8
                    && self.pool_col == 0
                    && let Some(d @ (Dir::Up | Dir::Down)) = dir
                {
                    let from = self.tracks_row - 8;
                    let to = if matches!(d, Dir::Up) {
                        from.checked_sub(1)
                    } else {
                        Some(from + 1).filter(|&i| i < self.song().instruments.len())
                    };
                    if let Some(to) = to
                        && self.run(Command::SwapInstruments {
                            a: from as u8,
                            b: to as u8,
                        })
                    {
                        self.tracks_row = 8 + to;
                    }
                    return true;
                }
                let last = 8 + self.song().instruments.len() - 1;
                let row = self.tracks_row;
                match dir {
                    // SHIFT+RIGHT opens that instrument on the TIMELINE page.
                    Some(Dir::Right) if shift => self.open_tracks_instrument(),
                    // The track cards are four by two: the arrows move on the
                    // grid, DOWN from the lower row goes to the POOL and UP
                    // from its first slot comes back.
                    Some(Dir::Left) if row < 8 => {
                        if !row.is_multiple_of(4) {
                            self.tracks_row = row - 1;
                        }
                    }
                    Some(Dir::Right) if row < 8 => {
                        if row % 4 < 3 {
                            self.tracks_row = row + 1;
                        }
                    }
                    Some(Dir::Up) if row < 8 => {
                        if row >= 4 {
                            self.tracks_row = row - 4;
                        }
                    }
                    Some(Dir::Down) if row < 4 => self.tracks_row = row + 4,
                    Some(Dir::Down) if row < 8 => self.tracks_row = 8,
                    Some(Dir::Up) if row == 8 => self.tracks_row = 4,
                    Some(Dir::Up) => self.tracks_row = row - 1,
                    Some(Dir::Down) => self.tracks_row = (row + 1).min(last),
                    // In the pool LEFT/RIGHT pick the column.
                    Some(Dir::Right) if self.tracks_row >= 8 => {
                        self.pool_col = (self.pool_col + 1).min(POOL_COLUMNS.len() - 1)
                    }
                    Some(Dir::Left) if self.tracks_row >= 8 => {
                        self.pool_col = self.pool_col.saturating_sub(1)
                    }
                    _ => {}
                }
                true
            }
            Page::Song => {
                let field = match self.song_field {
                    1 => Some(Field::SongTempo),
                    2 => Some(Field::SongTranspose),
                    _ => None,
                };
                if edit && let (Some(d), Some(field)) = (dir, field) {
                    self.adjust_field(field, d);
                    return true;
                }
                if option && pressed & BTN_EDIT != 0 || edit && pressed & BTN_OPTION != 0 {
                    if let Some(field) = field {
                        self.run(Command::Clear { field });
                    }
                    return true;
                }
                // Two columns, as drawn: NAME, BPM,
                // TRANSPOSE on the left, SAVE, NEW, LOAD, RENDER on the
                // right. UP/DOWN stay in the column; LEFT/RIGHT cross to the
                // row beside, except on BPM and TRANSPOSE, where they change
                // the value.
                let right = self.song_field >= 3;
                match dir {
                    Some(Dir::Up) => {
                        self.song_field =
                            self.song_field
                                .saturating_sub(1)
                                .max(if right { 3 } else { 0 })
                    }
                    Some(Dir::Down) => {
                        self.song_field =
                            (self.song_field + 1).min(if right { SONG_FIELDS.len() - 1 } else { 2 })
                    }
                    Some(Dir::Right) if self.song_field == 0 => self.song_field = 3,
                    Some(Dir::Left) if right => self.song_field = (self.song_field - 3).min(2),
                    // LEFT/RIGHT change BPM (by 1) and TRANSPOSE (by a
                    // semitone) directly, as on APP;
                    // EDIT+LEFT/RIGHT moves BPM by 0.1.
                    Some(d @ (Dir::Left | Dir::Right)) => {
                        if let Some(field) = field {
                            self.message.clear();
                            self.run(Command::Adjust {
                                field,
                                direction: if matches!(d, Dir::Left) { -1 } else { 1 },
                                granularity: if field == Field::SongTempo {
                                    Granularity::Coarse
                                } else {
                                    Granularity::Fine
                                },
                            });
                        }
                    }
                    _ => {}
                }
                if pressed & BTN_EDIT != 0 && dir.is_none() {
                    match self.song_field {
                        3 => {
                            self.message = match self.save() {
                                Ok(()) => "SAVED".into(),
                                Err(e) => {
                                    eprintln!("[viewer] save failed: {e}");
                                    "SAVE FAILED".into()
                                }
                            }
                        }
                        4 => self.new_song(),
                        5 => self.request = Some(Request::Close),
                        // RENDER: the song to WAV files (the app runs it).
                        6 => self.request = Some(Request::Render),
                        _ => {}
                    }
                }
                true
            }
        }
    }

    /// Keys 5-8 (slots 1-4 of the bottom bar) on MIXER, INST (Page::Tracks)
    /// and SONG.
    /// TIMELINE's slots are the M8 buttons, handled by the app. Returns
    /// false when the page has no action on that slot.
    pub(super) fn soft_key(&mut self, slot: usize) -> bool {
        self.message.clear();
        match (self.page, slot) {
            (Page::Mixer, 1 | 2) => {
                let track = self.mixer_strip;
                if track >= 8 {
                    return false;
                }
                let solo = slot == 2;
                let bit = 1u8 << track;
                if solo {
                    self.toggle_solo(bit);
                } else {
                    self.toggle_mute(bit);
                }
                self.apply_track_flags();
            }
            // LIMIT: the arrows move the limiter until LIMIT again. Slot 4
            // (OSCIL) is the app's oscilloscope view.
            (Page::Mixer, 3) => self.limit_mode = !self.limit_mode,
            (Page::Tracks, 1) => {
                self.page_input(BTN_SHIFT, BTN_RIGHT);
            }
            // INST: MUTE and SOLO the track under the cursor, as on MIXER
            //. The POOL has no track.
            (Page::Tracks, 2 | 3) if self.tracks_row < 8 => {
                let bit = 1u8 << self.tracks_row;
                if slot == 3 {
                    self.toggle_solo(bit);
                } else {
                    self.toggle_mute(bit);
                }
                self.apply_track_flags();
            }
            (Page::Song, 1) => {
                self.message = match self.save() {
                    Ok(()) => "SAVED".into(),
                    Err(e) => {
                        eprintln!("[viewer] save failed: {e}");
                        "SAVE FAILED".into()
                    }
                }
            }
            // RENDER on the bar (hard to find in the list); NEW stays
            // in the right-hand column and on HOME.
            (Page::Song, 2) => self.request = Some(Request::Render),
            (Page::Song, 3) => self.request = Some(Request::Close),
            (Page::Tracks | Page::Song, 4) => self.toggle_play(),
            _ => return false,
        }
        true
    }

    /// The text field under the cursor, its value and maximum length: the
    /// song NAME on the SONG page, the instrument NAME in the INSTRUMENT view.
    pub(super) fn text_target(&self) -> Option<(Field, String, usize)> {
        let field = if self.page == Page::Song && self.song_field == 0 {
            Field::SongName
        } else if self.on_page() {
            return None;
        } else {
            self.current_field()?
        };
        if !matches!(
            field,
            Field::SongName
                | Field::Instrument {
                    parameter: m8view::InstrumentParameter::Name,
                    ..
                }
        ) {
            return None;
        }
        match m8edit::read_field(self.song(), field) {
            Ok(m8edit::FieldValue::Text(text)) => Some((field, text, 12)),
            _ => None,
        }
    }

    /// Writes the text field under the cursor (see `text_target`).
    pub(super) fn set_text(&mut self, text: &str) -> bool {
        let Some((field, _, max)) = self.text_target() else {
            return false;
        };
        let text: String = text.chars().take(max).collect();
        self.message.clear();
        self.run(Command::Set {
            field,
            value: m8edit::FieldValue::Text(text),
        })
    }

    /// EDIT+arrow on a song-wide field: LEFT/RIGHT fine, UP/DOWN coarse.
    fn adjust_field(&mut self, field: Field, dir: Dir) {
        let (direction, granularity) = match dir {
            Dir::Left => (-1, Granularity::Fine),
            Dir::Right => (1, Granularity::Fine),
            Dir::Down => (-1, Granularity::Coarse),
            Dir::Up => (1, Granularity::Coarse),
        };
        self.message.clear();
        self.run(Command::Adjust {
            field,
            direction,
            granularity,
        });
    }

    pub(super) fn apply_track_flags(&self) {
        if let Some(engine) = self.engine() {
            engine.set_track_flags(self.mute, self.solo);
        }
    }

    /// NEW: a copy of the template song next to the current file, named
    /// NEW00.m8s, NEW01.m8s ... The app then opens it. The template is
    /// TEMPLATE.m8s in the songs root, else the empty 6.6 song of the
    /// m8-file-parser test files (MIT), built in.
    /// SOLO on one track at a time, and never with MUTE on the same track
    ///: soloing a track takes SOLO off the track that
    /// had it and unmutes it; SOLO again on the same track clears it.
    fn toggle_solo(&mut self, bit: u8) {
        self.solo = if self.solo & bit != 0 { 0 } else { bit };
        self.mute &= !self.solo;
    }

    /// MUTE toggles the track; muting a soloed track takes its SOLO off.
    fn toggle_mute(&mut self, bit: u8) {
        self.mute ^= bit;
        self.solo &= !self.mute;
    }

    fn new_song(&mut self) {
        let dir = self
            .path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        match create_new_song(&dir, self.template.as_deref()) {
            Ok(target) => self.request = Some(Request::Open(target.to_string_lossy().into_owned())),
            Err(e) => self.message = e,
        }
    }
}

/// Writes NEW00.m8s (or the first free NEWnn.m8s) in `dir`: a copy of
/// `template` when it is a file, else of the built-in empty 6.6 song.
/// The error is a short message for the screen.
pub(crate) fn create_new_song(
    dir: &std::path::Path,
    template: Option<&std::path::Path>,
) -> Result<std::path::PathBuf, String> {
    let target = (0..100)
        .map(|n| dir.join(format!("NEW{n:02}.m8s")))
        .find(|p| !p.exists())
        .ok_or_else(|| "NO FREE NAME".to_string())?;
    let bytes = match template.filter(|t| t.is_file()) {
        Some(t) => std::fs::read(t).map_err(|error| error.to_string()),
        None => empty_song_bytes(),
    };
    bytes
        .and_then(|b| std::fs::write(&target, b).map_err(|error| error.to_string()))
        .map(|_| target)
        .map_err(|e| {
            eprintln!("[viewer] new song: {e}");
            "NEW FAILED".to_string()
        })
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    #[test]
    fn generated_new_song_is_empty_and_parseable() {
        let bytes = empty_song_bytes().unwrap();
        let song =
            m8fmt::Song::read_from_reader(&mut m8fmt::reader::Reader::new(bytes.clone())).unwrap();
        assert_eq!((song.version.major, song.version.minor), (6, 6));
        assert_eq!(song.name, "NEW SONG");
        assert_eq!(song.tempo, 120.0);
        assert!(song.song.steps.iter().all(|step| *step == 0xFF));
        assert!(song.instruments.iter().all(Instrument::is_empty));
        assert!(song.phrases.iter().all(Phrase::is_empty));
        assert_eq!(song.mixer_settings.master_volume, 0xE0);
        assert!(
            song.mixer_settings
                .track_volume
                .iter()
                .all(|volume| *volume == 0xE0)
        );
    }

    #[test]
    fn generated_song_matches_open_format_fixture_field_by_field() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../m8fmt/examples/songs/V6_6EMPTY.m8s");
        if !fixture.is_file() {
            eprintln!("fixture missing; skipping comparison");
            return;
        }
        let bytes = std::fs::read(fixture).unwrap();
        let expected = m8fmt::Song::read_from_reader(&mut m8fmt::reader::Reader::new(bytes))
            .expect("format fixture should parse");
        let actual_bytes = empty_song_bytes().unwrap();
        let actual = m8fmt::Song::read_from_reader(&mut m8fmt::reader::Reader::new(actual_bytes))
            .expect("generated song should parse");

        assert_eq!(actual.version, expected.version);
        assert_eq!(actual.directory, expected.directory);
        assert_eq!(actual.transpose, expected.transpose);
        assert_eq!(actual.tempo, expected.tempo);
        assert_eq!(actual.quantize, expected.quantize);
        assert_eq!(actual.key, expected.key);
        assert_eq!(actual.midi_settings, expected.midi_settings);
        assert_eq!(actual.midi_mappings, expected.midi_mappings);
        assert_eq!(actual.mixer_settings, expected.mixer_settings);
        assert_eq!(actual.effects_settings, expected.effects_settings);
        assert_eq!(actual.eqs, expected.eqs);
        assert_eq!(actual.grooves, expected.grooves);
        assert_eq!(actual.scales.len(), expected.scales.len());
        for (index, (actual, expected)) in actual.scales.iter().zip(&expected.scales).enumerate() {
            assert_eq!(actual.number, expected.number, "scale {index} number");
            assert_eq!(actual.name, expected.name, "scale {index} name");
            assert_eq!(
                actual.map_high_bits, expected.map_high_bits,
                "scale {index} map"
            );
            for (note_index, (actual, expected)) in
                actual.notes.iter().zip(&expected.notes).enumerate()
            {
                assert_eq!(
                    actual.enabled, expected.enabled,
                    "scale {index} note {note_index} enabled"
                );
                assert_eq!(
                    actual.semitone_byte, expected.semitone_byte,
                    "scale {index} note {note_index} semitone"
                );
                assert_eq!(
                    actual.cents_byte, expected.cents_byte,
                    "scale {index} note {note_index} cents"
                );
            }
        }
        assert_eq!(actual.song, expected.song);
        assert!(actual.phrases == expected.phrases, "phrases differ");
        assert_eq!(actual.chains, expected.chains);
        assert!(actual.tables == expected.tables, "tables differ");
        assert_eq!(actual.instruments, expected.instruments);
    }

    // TRK below the tracks: ENTER edits a pool value (the arrows change it,
    // ESC undoes it), ENTER on NAME opens the instrument.
    #[test]
    fn pool_rows_edit_and_open_instruments() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Tracks;
        let slot = (0..0x80usize)
            .find(|&i| synth_params(&v.song().instruments[i]).is_some())
            .unwrap();
        let dry = |v: &Viewer| synth_params(&v.song().instruments[slot]).unwrap().mixer_dry;
        let before = dry(&v);
        v.tracks_row = 8 + slot;
        v.pool_col = 1;
        let f = v.page_frame(12, 40);
        let (line, col, width) = f.cursor.unwrap();
        assert_eq!(&f.lines[line][col..col + width], format!("{before:02X}"));
        v.tracks_enter();
        assert!(v.edit_mode.is_some());
        v.page_input(0, BTN_UP);
        assert_eq!(dry(&v), before.saturating_add(16));
        assert!(v.ux_back());
        assert_eq!(dry(&v), before);
        v.pool_col = 0;
        v.tracks_enter();
        assert_eq!(
            (v.page, v.kind, v.instrument as usize),
            (Page::Timeline, Kind::Instrument, slot)
        );
    }

    // EDIT+DOWN on a pool NAME moves the instrument one slot down with its
    // table; the phrase steps that played it follow, and undo puts it back.
    #[test]
    fn pool_edit_down_moves_an_instrument() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Tracks;
        let uses = |v: &Viewer, slot: u8| {
            v.song()
                .phrases
                .iter()
                .flat_map(|p| p.steps.iter())
                .filter(|s| s.instrument == slot)
                .count()
        };
        let slot = (0..0x7Fu8)
            .find(|&i| uses(&v, i) > 0 && uses(&v, i + 1) == 0)
            .unwrap();
        let moved = v.song().instruments[usize::from(slot)].clone();
        let table = v.song().tables[usize::from(slot)].clone();
        let n = uses(&v, slot);
        v.tracks_row = 8 + usize::from(slot);
        v.pool_col = 0;
        v.page_input(BTN_EDIT, BTN_DOWN);
        assert_eq!(v.tracks_row, 9 + usize::from(slot));
        assert!(v.song().instruments[usize::from(slot) + 1] == moved);
        assert!(v.song().tables[usize::from(slot) + 1] == table);
        assert_eq!((uses(&v, slot), uses(&v, slot + 1)), (0, n));
        assert!(v.undo(false));
        assert!(v.song().instruments[usize::from(slot)] == moved);
        assert_eq!(uses(&v, slot), n);
    }

    // INST's track cards are four by two: the arrows move on the grid, DOWN
    // from the lower row enters the POOL and UP from its first slot leaves it.
    #[test]
    fn inst_arrows_move_on_the_card_grid() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Tracks;
        v.tracks_row = 0;
        let go = |v: &mut Viewer, b: u32| {
            v.page_input(0, b);
            v.tracks_row
        };
        assert_eq!(go(&mut v, BTN_LEFT), 0);
        assert_eq!(go(&mut v, BTN_RIGHT), 1);
        assert_eq!(go(&mut v, BTN_DOWN), 5);
        assert_eq!(go(&mut v, BTN_UP), 1);
        assert_eq!(go(&mut v, BTN_UP), 1);
        v.tracks_row = 3;
        assert_eq!(go(&mut v, BTN_RIGHT), 3);
        assert_eq!(go(&mut v, BTN_DOWN), 7);
        assert_eq!(go(&mut v, BTN_DOWN), 8);
        assert_eq!(go(&mut v, BTN_DOWN), 9);
        assert_eq!(go(&mut v, BTN_UP), 8);
        assert_eq!(go(&mut v, BTN_UP), 4);
    }

    // SONG: LEFT/RIGHT change BPM and TRANSPOSE without EDIT.
    #[test]
    fn song_arrows_change_tempo_and_transpose() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Song;
        v.song_field = 1;
        let tempo = v.song().tempo;
        v.page_input(0, BTN_RIGHT);
        let up = v.song().tempo;
        assert!(up > tempo, "{tempo} -> {up}");
        v.page_input(0, BTN_LEFT);
        assert_eq!(v.song().tempo, tempo);
        v.song_field = 2;
        let transpose = v.song().transpose;
        v.page_input(0, BTN_RIGHT);
        assert_eq!(v.song().transpose, transpose.wrapping_add(1));
        assert!((up - tempo - 1.0).abs() < 1e-3, "BPM by 1: {tempo} -> {up}");
    }

    // SONG's arrows follow its two columns.
    #[test]
    fn song_arrows_follow_the_columns() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Song;
        v.song_field = 0;
        let go = |v: &mut Viewer, b: u32| {
            v.page_input(0, b);
            v.song_field
        };
        assert_eq!(go(&mut v, BTN_DOWN), 1);
        assert_eq!(go(&mut v, BTN_DOWN), 2);
        assert_eq!(go(&mut v, BTN_DOWN), 2, "the left column ends at TRANSPOSE");
        v.song_field = 0;
        assert_eq!(go(&mut v, BTN_RIGHT), 3);
        assert_eq!(go(&mut v, BTN_DOWN), 4);
        assert_eq!(go(&mut v, BTN_DOWN), 5);
        assert_eq!(go(&mut v, BTN_DOWN), 6, "RENDER under LOAD");
        assert_eq!(go(&mut v, BTN_DOWN), 6);
        assert_eq!(go(&mut v, BTN_LEFT), 2);
        v.song_field = 3;
        assert_eq!(go(&mut v, BTN_UP), 3, "the right column starts at SAVE");
        assert_eq!(go(&mut v, BTN_LEFT), 0);
        assert!(v.soft_key(2));
        assert_eq!(v.request.take(), Some(Request::Render));
    }

    // INST's keys 6 and 7 are MUTE and SOLO on the track under the cursor,
    // as on MIXER; the POOL has no track.
    #[test]
    fn inst_mute_and_solo_are_the_mixer_ones() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Tracks;
        v.tracks_row = 2;
        v.mute = 0b100;
        assert!(v.soft_key(3));
        assert_eq!((v.solo, v.mute), (0b100, 0));
        v.tracks_row = 5;
        assert!(v.soft_key(3));
        assert_eq!(v.solo, 0b10_0000);
        assert!(v.soft_key(3));
        assert_eq!(v.solo, 0);
        v.tracks_row = 5;
        assert!(v.soft_key(2));
        assert_eq!(v.mute, 0b10_0000);
        assert!(v.soft_key(3));
        assert_eq!((v.solo, v.mute), (0b10_0000, 0));
        v.tracks_row = 8;
        assert!(!v.soft_key(2));
        assert!(!v.soft_key(3));
    }

    // PLAY on a POOL row while stopped, and EDIT+PLAY on an instrument, ask
    // the screen to preview that instrument.
    #[test]
    fn play_previews_instruments_like_the_m8() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.page = Page::Tracks;
        v.tracks_row = 8 + 3;
        v.input(0, crate::viewer::BTN_PLAY);
        assert!(matches!(
            v.request.take(),
            Some(Request::PreviewInstrument(3))
        ));
        v.page = Page::Timeline;
        v.kind = Kind::Instrument;
        v.instrument = 5;
        let play = crate::viewer::BTN_PLAY;
        v.input(BTN_EDIT | play, play);
        assert!(matches!(
            v.request.take(),
            Some(Request::PreviewInstrument(5))
        ));
        // PLAY alone too, while stopped.
        v.input(play, play);
        assert!(matches!(
            v.request.take(),
            Some(Request::PreviewInstrument(5))
        ));
    }
}
