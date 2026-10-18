//! Read-only song viewer: navigation state and the C ABI the app draws from.
//!
//! The app reports M8 button state (held and newly pressed); the viewer moves
//! the cursor and the views the way the M8 does for the parts phase 2 covers:
//! arrows move the cursor, SHIFT+RIGHT opens the item under the cursor
//! (SONG -> CHAIN -> PHRASE -> INSTRUMENT -> TABLE), SHIFT+LEFT goes back.
//! The frame is a fixed grid of text rows built from `m8view`.

use m8edit::{Command, Editor, FieldValue, Granularity, Selection};
use m8fmt::Song;
use m8view::{Field, FieldSpan, View};
use std::ffi::{CStr, c_char};

mod eq_edit;
mod fx_picker;
mod graphic_pages;
pub(crate) mod instrument_defaults;
mod instrument_io;
mod instrument_page;
mod pages;
pub mod render;
mod split;
mod ux;
pub(crate) use pages::EMPTY_SONG;
pub use pages::empty_song_bytes;
pub use pages::{MixerView, Page, Style, TimelineView};

pub const BTN_UP: u32 = 1;
pub const BTN_DOWN: u32 = 1 << 1;
pub const BTN_LEFT: u32 = 1 << 2;
pub const BTN_RIGHT: u32 = 1 << 3;
pub const BTN_SHIFT: u32 = 1 << 4;
pub const BTN_PLAY: u32 = 1 << 5;
pub const BTN_OPTION: u32 = 1 << 6;
pub const BTN_EDIT: u32 = 1 << 7;

/// Rows of the M8 data panel shown per view: title, column header, 16 rows.
const SONG_VISIBLE_ROWS: usize = 16;
const SONG_ROWS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Song,
    Chain,
    Phrase,
    Instrument,
    Table,
}

/// What the app should do next, asked from the SONG page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Close the song and go back to the browser (LOAD).
    Close,
    /// Close the song and open this file (NEW).
    Open(String),
    /// Open the browser for an instrument or a sample (instrument_io).
    Browse(i32),
    /// Open the sample editor on this file (INSTRUMENT: EDIT by the sample).
    EditSample(String),
    /// Open the EQ editor on the instrument's EQ (INSTRUMENT: ENTER on EQ).
    EditEq,
    /// Preview this instrument (INSTRUMENT: EDIT+PLAY; POOL: PLAY while stopped).
    PreviewInstrument(u8),
    /// Record a sample for this SAMPLER (REC. on an empty SAMPLE).
    Record,
    /// Render the song to WAV files (SONG: RENDER).
    Render,
}

/// Cursor and selection of every view, kept when moving between views.
pub struct Viewer {
    editor: Editor,
    /// Playback: engine borrowed from the app (outlives the viewer), the
    /// sample bank, and a snapshot waiting for the mailbox to free up.
    engine: Option<std::ptr::NonNull<engine::ZtEngine>>,
    bank: crate::samples::SampleBank,
    pending: Option<Box<engine::PlaybackSnapshot>>,
    playing: bool,
    /// FX command picker: open on EDIT+UP/DOWN over a command cell.
    picker: Option<Picker>,
    /// Selection anchor while in selection mode (SHIFT+OPTION).
    select_anchor: Option<Field>,
    /// Last EDIT tap (time, field), to detect the M8 double tap.
    last_edit_tap: Option<(std::time::Instant, Field)>,
    /// File bytes as last loaded or saved: the writer patches over them so
    /// unmodelled bytes survive.
    bytes: Vec<u8>,
    path: std::path::PathBuf,
    backed_up: bool,
    name: String,
    kind: Kind,
    song_row: usize,
    song_top: usize,
    track: usize,
    chain: u8,
    chain_row: usize,
    phrase: u8,
    phrase_row: usize,
    instrument: u8,
    /// Field under the cursor in the INSTRUMENT view (None: first field).
    instrument_field: Option<Field>,
    table: u8,
    table_row: usize,
    /// Field index within the row, per tabular view.
    chain_col: usize,
    phrase_col: usize,
    table_col: usize,
    message: String,
    /// Phase 6 pages (pages.rs); TIMELINE shows the SONG view turned.
    page: Page,
    /// First song row shown on the TIMELINE (follows the cursor at draw time).
    song_left: std::cell::Cell<usize>,
    /// Screen line of track 1 on the TIMELINE (0 when the row numbers do not fit).
    timeline_first: std::cell::Cell<usize>,
    /// Live mute and solo masks (bit 0 = track 1), not saved.
    mute: u8,
    solo: u8,
    mixer_strip: usize,
    /// MIXER: LIMIT pressed, the arrows move the limiter level.
    limit_mode: bool,
    /// TRK: rows 0-7 the tracks, then the instrument pool (row 8 is
    /// instrument 00).
    tracks_row: usize,
    /// TRK pool column: NAME, DRY, MFX, DEL, REV, EQ.
    pool_col: usize,
    song_field: usize,
    request: Option<Request>,
    /// Song copied by NEW: TEMPLATE.m8s in the songs root.
    template: Option<std::path::PathBuf>,
    /// Transport to start once the snapshot is published (previews).
    autoplay: Option<(engine::TransportMode, u8, u8)>,
    /// SAVE on an instrument whose file exists: the path a second EDIT
    /// overwrites.
    save_armed: Option<std::path::PathBuf>,
    /// Edit mode (ux): the arrows change the value; the count of changes
    /// made, which ESC undoes.
    edit_mode: Option<usize>,
}

#[derive(Debug, Clone)]
struct Picker {
    field: Field,
    items: Vec<(u8, &'static str)>,
    selected: usize,
}

impl Viewer {
    pub(crate) fn song(&self) -> &Song {
        self.editor.song()
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        Self::from_bytes(bytes, path)
    }

    /// A viewer on song bytes already in memory; `path` names the file it
    /// saves to (the browser's previews never save).
    pub(crate) fn from_bytes(bytes: Vec<u8>, path: &str) -> Result<Self, String> {
        let song = Song::read_from_reader(&mut m8fmt::reader::Reader::new(bytes.clone()))
            .map_err(|e| format!("{path}: {e:?}"))?;
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self {
            editor: Editor::new(song),
            engine: None,
            bank: crate::samples::SampleBank::default(),
            pending: None,
            playing: false,
            picker: None,
            select_anchor: None,
            last_edit_tap: None,
            bytes,
            path: std::path::PathBuf::from(path),
            backed_up: false,
            name,
            kind: Kind::Song,
            song_row: 0,
            song_top: 0,
            track: 0,
            chain: 0,
            chain_row: 0,
            phrase: 0,
            phrase_row: 0,
            instrument: 0,
            instrument_field: None,
            table: 0,
            table_row: 0,
            chain_col: 0,
            phrase_col: 0,
            table_col: 0,
            message: String::new(),
            page: Page::Timeline,
            song_left: std::cell::Cell::new(0),
            timeline_first: std::cell::Cell::new(1),
            mute: 0,
            solo: 0,
            mixer_strip: 0,
            limit_mode: false,
            tracks_row: 0,
            pool_col: 0,
            song_field: 1,
            request: None,
            template: None,
            autoplay: None,
            save_armed: None,
            edit_mode: None,
        })
    }

    /// Plays the song from its first row as soon as the engine holds this
    /// viewer's snapshot (the browser's preview).
    pub(crate) fn play_when_ready(&mut self) {
        self.autoplay = Some((engine::TransportMode::Song, 0, 0));
    }

    /// Applies one input poll. `pressed` holds the buttons that went down
    /// since the last poll, `held` every button currently down.
    pub fn input(&mut self, held: u32, pressed: u32) {
        let shift = held & BTN_SHIFT != 0;
        // SHIFT+PLAY saves the song in place.
        if shift && pressed & BTN_PLAY != 0 {
            self.message = match self.save() {
                Ok(()) => "SAVED".into(),
                Err(e) => {
                    eprintln!("[viewer] save failed: {e}");
                    "SAVE FAILED".into()
                }
            };
            return;
        }
        if self.picker.is_some() {
            self.picker_input(pressed);
            return;
        }
        if pressed & BTN_PLAY != 0 && !shift {
            // EDIT+PLAY on an instrument, PLAY on a POOL row or on an
            // instrument while stopped: preview the
            // instrument.
            let pool = (self.page == Page::Tracks && self.tracks_row >= 8 && !self.is_playing())
                .then(|| (self.tracks_row - 8) as u8);
            let page = (self.page == Page::Timeline
                && self.kind == Kind::Instrument
                && (held & BTN_EDIT != 0 || !self.is_playing()))
            .then_some(self.instrument);
            if let Some(slot) = pool.or(page) {
                self.request = Some(Request::PreviewInstrument(slot));
                return;
            }
            self.toggle_play();
            return;
        }
        if self.page_input(held, pressed) {
            return;
        }
        // In edit mode (ux) the arrows change the value, as with EDIT held.
        let edit = held & BTN_EDIT != 0 || self.edit_mode.is_some();
        let option = held & BTN_OPTION != 0;
        let arrows = pressed & (BTN_UP | BTN_DOWN | BTN_LEFT | BTN_RIGHT);
        // Selection (M8): SHIFT+OPTION starts it at the cursor, arrows move the
        // other corner, OPTION copies and leaves; SHIFT+EDIT pastes at the cursor.
        if shift && pressed & BTN_OPTION != 0 && self.select_anchor.is_none() {
            self.select_anchor = self.current_field();
            self.message = "SELECT".into();
            return;
        }
        if let Some(anchor) = self.select_anchor
            && pressed & BTN_OPTION != 0
        {
            if let Some(focus) = self.current_field() {
                let copied = self.editor.apply(Command::Copy {
                    selection: Selection { anchor, focus },
                });
                self.message = match copied {
                    Ok(_) => "COPIED".into(),
                    Err(e) => format!("{e:?}").to_uppercase(),
                };
            }
            self.select_anchor = None;
            return;
        }
        // SHIFT+OPTION then EDIT (no arrow moved): clone the chain (SONG) or
        // phrase (CHAIN) under the cursor into a new slot, as on the M8.
        if let Some(anchor) = self.select_anchor
            && pressed & BTN_EDIT != 0
            && self.current_field() == Some(anchor)
        {
            self.select_anchor = None;
            self.clone_under_cursor(anchor);
            return;
        }
        if shift && pressed & BTN_EDIT != 0 {
            if let Some(anchor) = self.current_field() {
                self.message.clear();
                self.run(Command::Paste { anchor });
            }
            return;
        }
        // EDIT+OPTION clears; EDIT alone inserts the last value on an empty cell.
        if edit && pressed & BTN_OPTION != 0 || option && pressed & BTN_EDIT != 0 {
            if let Some(field) = self.current_field() {
                self.message.clear();
                self.clear_field(field);
            }
            return;
        }
        if pressed & BTN_EDIT != 0 && arrows == 0 && !shift && !option {
            // INSTRUMENT: LOAD, SAVE and the sample path are actions.
            if let Some(field) = self.current_field()
                && self.instrument_action(field)
            {
                return;
            }
            if let Some(field) = self.current_field() {
                self.message.clear();
                let now = std::time::Instant::now();
                let double = self.last_edit_tap.is_some_and(|(t, f)| {
                    f == field && now.duration_since(t) < std::time::Duration::from_millis(300)
                });
                self.last_edit_tap = Some((now, field));
                if double {
                    self.last_edit_tap = None;
                    self.create_new(field);
                } else {
                    self.run(Command::InsertLast { field });
                }
            }
            return;
        }
        for (bit, dir) in [
            (BTN_UP, Dir::Up),
            (BTN_DOWN, Dir::Down),
            (BTN_LEFT, Dir::Left),
            (BTN_RIGHT, Dir::Right),
        ] {
            if pressed & bit == 0 {
                continue;
            }
            self.message.clear();
            self.last_edit_tap = None;
            if edit {
                self.adjust(dir);
            } else if shift {
                self.navigate(dir);
            } else {
                self.move_cursor(dir);
            }
        }
    }

    /// Clones the referenced chain or phrase into the first free slot and
    /// points the cell at the copy.
    fn clone_under_cursor(&mut self, field: Field) {
        match field {
            Field::SongChain { row, track } => {
                let source = self.song().song.steps[row as usize * 8 + track as usize];
                if source == 0xFF {
                    self.message = "EMPTY".into();
                    return;
                }
                self.run(Command::CreateChain {
                    row,
                    track,
                    clone_from: Some(source),
                    clone_phrases: false,
                });
            }
            Field::ChainPhrase { chain, row } => {
                let source = self.song().chains[chain as usize].steps[row as usize].phrase;
                if source == 0xFF {
                    self.message = "EMPTY".into();
                    return;
                }
                self.run(Command::CreatePhrase {
                    chain,
                    row,
                    clone_from: Some(source),
                });
            }
            _ => self.message = "CLONE: SONG OR CHAIN".into(),
        }
    }

    /// Selection rectangle in panel coordinates (first line, last line,
    /// first column, last column + 1), while selecting.
    pub fn selection(&self) -> Option<(usize, usize, usize, usize)> {
        let anchor = self.select_anchor?;
        let spans = self.spans();
        let a = spans.iter().find(|s| s.field == anchor)?;
        let b = self.current_span()?;
        Some((
            a.line.min(b.line),
            a.line.max(b.line),
            a.column.min(b.column),
            (a.column + a.width).max(b.column + b.width),
        ))
    }

    /// Double tap EDIT: a new chain on a SONG cell, a new phrase on a CHAIN
    /// phrase cell (the first empty, unreferenced slot).
    fn create_new(&mut self, field: Field) {
        match field {
            Field::SongChain { row, track } => {
                self.run(Command::CreateChain {
                    row,
                    track,
                    clone_from: None,
                    clone_phrases: false,
                });
            }
            Field::ChainPhrase { chain, row } => {
                self.run(Command::CreatePhrase {
                    chain,
                    row,
                    clone_from: None,
                });
            }
            _ => {}
        }
    }

    /// EDIT+arrow: LEFT/RIGHT fine, UP/DOWN coarse. On an FX command cell
    /// UP/DOWN opens the command picker instead, as on the M8.
    fn adjust(&mut self, dir: Dir) {
        let Some(field) = self.current_field() else {
            return;
        };
        // TYPE: the arrows step through the instrument kinds (instrument_defaults).
        if matches!(
            field,
            Field::Instrument {
                parameter: m8view::InstrumentParameter::Kind,
                ..
            }
        ) {
            self.change_type(dir);
            return;
        }
        let is_command = matches!(
            field,
            Field::PhraseFxCommand { .. } | Field::TableFxCommand { .. }
        );
        if is_command && matches!(dir, Dir::Up | Dir::Down) {
            let items = m8view::fx_commands(self.song(), field);
            if items.is_empty() {
                return;
            }
            let current = match m8edit::read_field(self.song(), field) {
                Ok(FieldValue::Byte(b)) => items.iter().position(|(id, _)| *id == b),
                _ => None,
            };
            self.picker = Some(Picker {
                field,
                items,
                selected: current.unwrap_or(0),
            });
            return;
        }
        let (direction, granularity) = match dir {
            Dir::Left => (-1, Granularity::Fine),
            Dir::Right => (1, Granularity::Fine),
            Dir::Down => (-1, Granularity::Coarse),
            Dir::Up => (1, Granularity::Coarse),
        };
        self.run(Command::Adjust {
            field,
            direction,
            granularity,
        });
    }

    /// Picker keys: the commands are a grid of fx_picker::COLUMNS (the
    /// app's view), LEFT/RIGHT move by one, UP/DOWN by a row; EDIT confirms,
    /// OPTION cancels.
    fn picker_input(&mut self, pressed: u32) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let n = picker.items.len();
        let cols = fx_picker::COLUMNS;
        if pressed & BTN_LEFT != 0 {
            picker.selected = picker.selected.saturating_sub(1);
        }
        if pressed & BTN_RIGHT != 0 {
            picker.selected = (picker.selected + 1).min(n - 1);
        }
        if pressed & BTN_UP != 0 && picker.selected >= cols {
            picker.selected -= cols;
        }
        if pressed & BTN_DOWN != 0 {
            picker.selected = (picker.selected + cols).min(n - 1);
        }
        if pressed & BTN_OPTION != 0 {
            self.picker = None;
        } else if pressed & BTN_EDIT != 0 {
            self.picker_choose();
        }
    }

    /// Sets the command chosen in the FX list and closes it.
    fn picker_choose(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let id = picker.items[picker.selected].0;
        self.run(Command::Set {
            field: picker.field,
            value: FieldValue::Byte(id),
        });
    }

    fn move_cursor(&mut self, dir: Dir) {
        match self.kind {
            // TIMELINE: song rows run left to right, tracks top to bottom.
            Kind::Song => match dir {
                Dir::Left => self.song_row = self.song_row.saturating_sub(1),
                Dir::Right => self.song_row = (self.song_row + 1).min(SONG_ROWS - 1),
                Dir::Up => self.track = self.track.saturating_sub(1),
                Dir::Down => self.track = (self.track + 1).min(7),
            },
            Kind::Chain | Kind::Phrase | Kind::Table => {
                let row = self.row();
                let fields = self.row_spans(row).len();
                let (row, col) = match self.kind {
                    Kind::Chain => (&mut self.chain_row, &mut self.chain_col),
                    Kind::Phrase => (&mut self.phrase_row, &mut self.phrase_col),
                    _ => (&mut self.table_row, &mut self.table_col),
                };
                // CHAIN and PHRASE sit side by side: RIGHT past the chain's
                // last field enters the phrase, LEFT from the note goes back.
                let at_end = *col + 1 >= fields;
                let at_start = *col == 0;
                match dir {
                    Dir::Up | Dir::Down => step_row(row, dir),
                    Dir::Left if self.kind == Kind::Phrase && at_start => {
                        self.navigate(Dir::Left);
                    }
                    Dir::Right if self.kind == Kind::Chain && at_end => {
                        self.navigate(Dir::Right);
                        self.phrase_col = 0;
                    }
                    Dir::Left => *col = col.saturating_sub(1),
                    Dir::Right => *col = (*col + 1).min(fields.saturating_sub(1)),
                }
            }
            Kind::Instrument => self.move_instrument_cursor(dir),
        }
        if self.song_row < self.song_top {
            self.song_top = self.song_row;
        } else if self.song_row >= self.song_top + SONG_VISIBLE_ROWS {
            self.song_top = self.song_row + 1 - SONG_VISIBLE_ROWS;
        }
    }

    /// Current data row of the tabular views.
    fn row(&self) -> usize {
        match self.kind {
            Kind::Chain => self.chain_row,
            Kind::Phrase => self.phrase_row,
            Kind::Table => self.table_row,
            Kind::Song => self.song_row,
            Kind::Instrument => 0,
        }
    }

    fn spans(&self) -> Vec<FieldSpan> {
        m8view::field_spans(self.song(), self.view()).unwrap_or_default()
    }

    /// Spans of data row `row` (frame line 2 + row), left to right.
    fn row_spans(&self, row: usize) -> Vec<FieldSpan> {
        let mut spans: Vec<FieldSpan> = self
            .spans()
            .into_iter()
            .filter(|s| s.line == 2 + row)
            .collect();
        spans.sort_by_key(|s| s.column);
        spans
    }

    /// The field under the cursor, if the view has editable fields.
    pub fn current_field(&self) -> Option<Field> {
        self.current_span().map(|s| s.field)
    }

    fn current_span(&self) -> Option<FieldSpan> {
        match self.kind {
            Kind::Song => self.spans().into_iter().find(|s| {
                s.field
                    == Field::SongChain {
                        row: self.song_row as u8,
                        track: self.track as u8,
                    }
            }),
            Kind::Chain | Kind::Phrase | Kind::Table => {
                let col = match self.kind {
                    Kind::Chain => self.chain_col,
                    Kind::Phrase => self.phrase_col,
                    _ => self.table_col,
                };
                let spans = self.row_spans(self.row());
                spans.get(col.min(spans.len().saturating_sub(1))).copied()
            }
            Kind::Instrument => {
                let spans = self.spans();
                spans
                    .iter()
                    .find(|s| Some(s.field) == self.instrument_field)
                    .or_else(|| spans.first())
                    .copied()
            }
        }
    }

    /// INSTRUMENT: LEFT/RIGHT along the line, UP/DOWN to the nearest field
    /// (by column) on the closest line above or below.
    fn move_instrument_cursor(&mut self, dir: Dir) {
        let spans = self.spans();
        let Some(cur) = self.current_span() else {
            return;
        };
        let next = match dir {
            Dir::Left => spans
                .iter()
                .filter(|s| s.line == cur.line && s.column < cur.column)
                .max_by_key(|s| s.column),
            Dir::Right => spans
                .iter()
                .filter(|s| s.line == cur.line && s.column > cur.column)
                .min_by_key(|s| s.column),
            Dir::Up | Dir::Down => {
                let line = spans
                    .iter()
                    .map(|s| s.line)
                    .filter(|&l| {
                        if matches!(dir, Dir::Up) {
                            l < cur.line
                        } else {
                            l > cur.line
                        }
                    })
                    .min_by_key(|&l| l.abs_diff(cur.line));
                line.and_then(|l| {
                    spans
                        .iter()
                        .filter(|s| s.line == l)
                        .min_by_key(|s| s.column.abs_diff(cur.column))
                })
            }
        };
        if let Some(n) = next {
            self.instrument_field = Some(n.field);
        }
    }

    /// SHIFT+arrow. RIGHT opens what the cursor points at, LEFT goes back.
    fn navigate(&mut self, dir: Dir) {
        match (self.kind, dir) {
            (Kind::Song, Dir::Right) => {
                let cell = self.song().song.steps[self.song_row * 8 + self.track];
                if cell == 0xFF {
                    self.message = "EMPTY".into();
                } else {
                    self.chain = cell;
                    self.chain_row = 0;
                    self.kind = Kind::Chain;
                }
            }
            (Kind::Chain, Dir::Right) => {
                let step = &self.song().chains[self.chain as usize].steps[self.chain_row];
                if step.phrase == 0xFF {
                    self.message = "EMPTY".into();
                } else {
                    self.phrase = step.phrase;
                    self.phrase_row = 0;
                    self.kind = Kind::Phrase;
                }
            }
            (Kind::Phrase, Dir::Right) => {
                let step = &self.song().phrases[self.phrase as usize].steps[self.phrase_row];
                if step.instrument != 0xFF {
                    self.instrument = step.instrument;
                }
                self.kind = Kind::Instrument;
            }
            (Kind::Instrument, Dir::Right) => {
                self.table = self.instrument;
                self.kind = Kind::Table;
            }
            (Kind::Chain, Dir::Left) => self.kind = Kind::Song,
            (Kind::Phrase, Dir::Left) => self.kind = Kind::Chain,
            (Kind::Instrument, Dir::Left) => self.kind = Kind::Phrase,
            (Kind::Table, Dir::Left) => self.kind = Kind::Instrument,
            _ => {}
        }
    }

    fn view(&self) -> View {
        match self.kind {
            Kind::Song => View::Song {
                top_row: self.song_top as u8,
            },
            Kind::Chain => View::Chain { index: self.chain },
            Kind::Phrase => View::Phrase { index: self.phrase },
            Kind::Instrument => View::Instrument {
                index: self.instrument,
            },
            Kind::Table => View::Table { index: self.table },
        }
    }

    /// Cursor in panel coordinates: (line, column, width), or None.
    fn cursor(&self) -> Option<(usize, usize, usize)> {
        self.current_span().map(|s| (s.line, s.column, s.width))
    }

    /// Whether line 1 of the panel is a column header (tabular views).
    fn has_column_header(&self) -> bool {
        !matches!(self.kind, Kind::Instrument)
    }

    /// Song name and tempo for the top bar, e.g. "MYSONG  T 067";
    /// a '*' after the name marks unsaved changes.
    pub fn header(&self) -> String {
        let name = if self.song().name.is_empty() {
            &self.name
        } else {
            &self.song().name
        };
        let mark = if self.editor.is_dirty() { "*" } else { "" };
        format!("{name}{mark}  BPM {}", self.song().tempo.round() as i32)
    }

    /// Writes the song over the bytes it was read from, then replaces the
    /// file atomically: NAME.tmp is written and renamed over NAME. The first
    /// save of a session copies the original to NAME.bak.
    pub fn save(&mut self) -> Result<(), String> {
        let mut w = m8fmt::writer::Writer::new(self.bytes.clone());
        self.song().write(&mut w)?;
        let out = w.finish();
        let file = self.path.to_string_lossy().into_owned();
        if !self.backed_up {
            std::fs::copy(&self.path, format!("{file}.bak")).map_err(|e| format!("backup: {e}"))?;
            self.backed_up = true;
        }
        let tmp = format!("{file}.tmp");
        std::fs::write(&tmp, &out).map_err(|e| format!("write {tmp}: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("rename: {e}"))?;
        self.bytes = out;
        self.editor.mark_saved();
        Ok(())
    }

    /// Attaches the engine and the songs root (for `/Samples/...` paths),
    /// loads the samples and publishes the first snapshot.
    pub fn attach_engine(&mut self, engine: *mut engine::ZtEngine, root: &str) {
        self.engine = std::ptr::NonNull::new(engine);
        self.bank = crate::samples::SampleBank::new(root, crate::samples::DEFAULT_BUDGET);
        self.bank
            .set_song_dir(self.path.parent().map(std::path::Path::to_path_buf));
        self.template = Some(std::path::Path::new(root).join("TEMPLATE.m8s"));
        let problems = self.bank.load(self.editor.song());
        if let Some((slot, path, problem)) = problems.first() {
            eprintln!(
                "[viewer] {} sample problem(s), first: {slot:02X} {path}: {problem:?}",
                problems.len()
            );
            self.message = format!("{} SAMPLES MISSING", problems.len());
        }
        self.publish();
        self.apply_track_flags();
    }

    fn engine(&self) -> Option<&engine::ZtEngine> {
        // SAFETY: the app keeps the engine alive while any viewer exists.
        self.engine.map(|e| unsafe { e.as_ref() })
    }

    /// Builds a snapshot of the current song and hands it to the engine; if
    /// the mailbox is busy it waits in `pending` and `tick` retries.
    fn publish(&mut self) {
        if self.engine.is_none() {
            return;
        }
        // Sample paths may have changed with the edit: reload (decoded files are reused).
        self.bank.load(self.editor.song());
        if let Some(snapshot) = self.render_snapshot() {
            self.pending = Some(Box::new(snapshot));
            self.tick();
        }
    }

    /// The song as it is now with its loaded samples, for the engine (and
    /// for RENDER, which plays it on an engine of its own).
    pub(crate) fn render_snapshot(&self) -> Option<engine::PlaybackSnapshot> {
        engine::PlaybackSnapshot::new(self.song().clone(), self.render_samples())
            .and_then(|s| s.with_sample_cues(self.bank.cues()))
    }

    /// The loaded samples by instrument slot, for a snapshot.
    pub(crate) fn render_samples(&self) -> Vec<Option<engine::Sample>> {
        self.bank
            .slots
            .iter()
            .map(|slot| match slot {
                Some(Ok(sample)) => Some(engine::Sample::from(sample.clone())),
                _ => None,
            })
            .collect()
    }

    /// Called every UI frame: frees retired snapshots, retries a pending one,
    /// and refreshes the playing flag.
    pub fn tick(&mut self) {
        let Some(engine) = self.engine else { return };
        // SAFETY: see `engine()`.
        let engine = unsafe { engine.as_ref() };
        while engine.collect_retired().is_some() {}
        if let Some(snapshot) = self.pending.take()
            && let Err(back) = engine.publish(snapshot)
        {
            self.pending = Some(back);
        }
        self.playing = (0..8).any(|t| engine.track_status(t).is_some_and(|s| s.active != 0));
        if self.pending.is_none()
            && let Some((mode, index, row)) = self.autoplay.take()
        {
            engine.request_transport(mode, index, row, 0);
            self.playing = true;
        }
    }

    /// PLAY: stop when playing, else play from the cursor like the M8:
    /// the song from the cursor row, the chain or the phrase on the track.
    fn toggle_play(&mut self) {
        let Some(engine) = self.engine() else { return };
        let (mode, index, row) = if self.playing {
            (engine::TransportMode::Stop, 0, 0)
        } else {
            match self.kind {
                Kind::Song => (engine::TransportMode::Song, 0, self.song_row as u8),
                Kind::Chain => (
                    engine::TransportMode::Chain,
                    self.chain,
                    self.chain_row as u8,
                ),
                Kind::Phrase => (
                    engine::TransportMode::Phrase,
                    self.phrase,
                    self.phrase_row as u8,
                ),
                Kind::Instrument | Kind::Table => (
                    engine::TransportMode::Phrase,
                    self.phrase,
                    self.phrase_row as u8,
                ),
            }
        };
        engine.request_transport(mode, index, row, self.track as u8);
        self.playing = !self.playing;
    }

    /// Whether the transport runs (a track is active).
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Stops playback (song closed).
    pub fn stop(&mut self) {
        if let Some(engine) = self.engine() {
            engine.request_transport(engine::TransportMode::Stop, 0, 0, 0);
        }
        self.playing = false;
    }

    /// Peak level per track, 0..65535, for the top-band meters.
    pub fn levels(&self) -> [u16; 8] {
        let mut out = [0u16; 8];
        if let Some(engine) = self.engine() {
            for (t, level) in out.iter_mut().enumerate() {
                if let Some(s) = engine.track_status(t as u8) {
                    *level = if s.active != 0 { s.peak } else { 0 };
                }
            }
        }
        out
    }

    /// Undo or redo the last edit; false when there is nothing to do.
    pub fn undo(&mut self, redo: bool) -> bool {
        let command = if redo { Command::Redo } else { Command::Undo };
        self.run(command)
    }

    /// Applies an edit command; errors become the title-line message.
    fn run(&mut self, command: Command) -> bool {
        match self.editor.apply(command) {
            Ok(outcome) => {
                if outcome.changed {
                    self.publish();
                    // Counted for ESC to undo an edit made in edit mode.
                    if let Some(n) = self.edit_mode.as_mut() {
                        *n += 1;
                    }
                }
                outcome.changed
            }
            Err(e) => {
                self.message = format!("{e:?}").to_uppercase();
                false
            }
        }
    }

    /// Writes unsaved edits to NAME.autosave next to the file, leaving the
    /// original untouched (used when the app quits with unsaved edits).
    pub fn autosave(&self) -> Result<(), String> {
        let mut w = m8fmt::writer::Writer::new(self.bytes.clone());
        self.song().write(&mut w)?;
        let file = format!("{}.autosave", self.path.to_string_lossy());
        std::fs::write(&file, w.finish()).map_err(|e| format!("{file}: {e}"))
    }

    /// Whether the song has edits that are not saved.
    pub fn is_dirty(&self) -> bool {
        self.editor.is_dirty()
    }

    /// Text rows of the current view. A message (e.g. EMPTY) goes at the
    /// right end of the title line: the screen has no status line.
    /// Puts '>' in column 0 of the rows being played in this view.
    fn mark_playhead(&self, lines: &mut [String]) {
        let Some(engine) = self.engine() else { return };
        for t in 0..8u8 {
            let Some(s) = engine.track_status(t).filter(|s| s.active != 0) else {
                continue;
            };
            let line = match self.kind {
                Kind::Song => (s.song_row as usize)
                    .checked_sub(self.song_top)
                    .filter(|&r| r < SONG_VISIBLE_ROWS)
                    .map(|r| 2 + r),
                Kind::Chain if s.chain == self.chain => Some(2 + s.chain_row as usize),
                Kind::Phrase if s.phrase == self.phrase => Some(2 + s.phrase_step as usize),
                _ => None,
            };
            if let Some(l) = line.filter(|&l| l < lines.len()) {
                let rest: String = lines[l].chars().skip(1).collect();
                lines[l] = format!(">{rest}");
            }
        }
    }

    /// Whether the current screen is a phase 6 page frame (TIMELINE on the
    /// SONG level, or another page) rather than an M8 view.
    fn on_page(&self) -> bool {
        self.picker.is_none() && (self.page != Page::Timeline || self.kind == Kind::Song)
    }

    /// The screen for a grid of `rows` x `cols`: a page frame, or an M8 view
    /// fitted with `fit_rows`.
    pub fn sized_frame(&self, rows: usize, cols: usize) -> pages::PageFrame {
        if self.on_page() {
            return self.page_frame(rows, cols);
        }
        let (lines, cursor) = self.frame();
        let (lines, cursor, _) = fit_rows(lines, cursor, rows, self.has_column_header());
        pages::PageFrame {
            lines,
            cursor,
            highlights: Vec::new(),
        }
    }

    pub fn frame(&self) -> (Vec<String>, Option<(usize, usize, usize)>) {
        if let Some(p) = &self.picker {
            let mut lines = vec![
                " FX COMMAND  ENTER ok  ESC cancel".to_string(),
                String::new(),
            ];
            lines.extend(p.items.iter().map(|(id, name)| format!(" {id:02X} {name}")));
            return (lines, Some((2 + p.selected, 1, 6)));
        }
        let mut lines = match m8view::render(self.song(), self.view()) {
            Ok(frame) => frame.plain_lines(),
            Err(e) => vec![format!(" {e:?}")],
        };
        self.mark_playhead(&mut lines);
        if !self.message.is_empty()
            && let Some(title) = lines.first_mut()
        {
            let pad = 30usize.saturating_sub(title.chars().count());
            title.push_str(&" ".repeat(pad));
            title.push_str(&self.message);
        }
        (lines, self.cursor())
    }
}

#[derive(Debug, Clone, Copy)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

fn step_row(row: &mut usize, dir: Dir) {
    match dir {
        Dir::Up => *row = row.saturating_sub(1),
        Dir::Down => *row = (*row + 1).min(15),
        Dir::Left | Dir::Right => {}
    }
}

// ---------------------------------------------------------------- C ABI

/// Opens a song for viewing. Returns NULL on error and writes a NUL-terminated
/// message into `err` (when not NULL).
///
/// # Safety
///
/// `path` must be a valid NUL-terminated string; `err` must point to
/// `err_len` writable bytes or be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_open(
    path: *const c_char,
    err: *mut c_char,
    err_len: usize,
) -> *mut Viewer {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller passes a valid C string.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    match Viewer::open(&path) {
        Ok(v) => Box::into_raw(Box::new(v)),
        Err(e) => {
            // SAFETY: the caller guarantees `err_len` writable bytes.
            unsafe { write_c_string(&e, err, err_len) };
            std::ptr::null_mut()
        }
    }
}

/// # Safety
///
/// `viewer` must come from `zt_viewer_open` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_close(viewer: *mut Viewer) {
    if !viewer.is_null() {
        // SAFETY: allocated by zt_viewer_open.
        drop(unsafe { Box::from_raw(viewer) });
    }
}

/// # Safety
///
/// `viewer` must be a live pointer from `zt_viewer_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_input(viewer: *mut Viewer, held: u32, pressed: u32) {
    // SAFETY: the caller passes a live viewer.
    if let Some(v) = unsafe { viewer.as_mut() } {
        v.input(held, pressed);
    }
}

/// Writes up to `rows` lines of `cols` characters into `out`, each line
/// padded with spaces and NUL-terminated (`rows * (cols + 1)` bytes). Non-ASCII
/// glyphs become '?'. Returns the number of lines written. The cursor (line,
/// column, width) goes to the out parameters, -1 when there is none.
///
/// # Safety
///
/// `viewer` must be live; `out` must hold `rows * (cols + 1)` bytes; the
/// cursor pointers must be writable or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_frame(
    viewer: *const Viewer,
    out: *mut c_char,
    rows: usize,
    cols: usize,
    cursor_line: *mut i32,
    cursor_col: *mut i32,
    cursor_width: *mut i32,
) -> usize {
    // SAFETY: the caller passes a live viewer.
    let Some(v) = (unsafe { viewer.as_ref() }) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    let pages::PageFrame { lines, cursor, .. } = v.sized_frame(rows, cols);
    // SAFETY: the caller guarantees rows * (cols + 1) writable bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(out.cast::<u8>(), rows * (cols + 1)) };
    let n = lines.len().min(rows);
    for (r, chunk) in buf.chunks_mut(cols + 1).enumerate() {
        chunk.fill(b' ');
        chunk[cols] = 0;
        if let Some(line) = lines.get(r).filter(|_| r < n) {
            for (c, ch) in line.chars().take(cols).enumerate() {
                chunk[c] = if ch.is_ascii() { ch as u8 } else { b'?' };
            }
        }
    }
    let (l, c, w) = cursor
        .filter(|&(_, _, w)| w > 0)
        .map_or((-1, -1, -1), |(l, c, w)| (l as i32, c as i32, w as i32));
    // SAFETY: optional writable out parameters.
    unsafe {
        if let Some(p) = cursor_line.as_mut() {
            *p = l;
        }
        if let Some(p) = cursor_col.as_mut() {
            *p = c;
        }
        if let Some(p) = cursor_width.as_mut() {
            *p = w;
        }
    }
    n
}

/// Fits a frame into `rows` lines for large fonts: first the column header
/// (line 1) goes, then the data rows scroll: the title stays and a window of
/// rows follows the cursor.
fn fit_rows(
    mut lines: Vec<String>,
    cursor: Option<(usize, usize, usize)>,
    rows: usize,
    column_header: bool,
) -> (Vec<String>, Option<(usize, usize, usize)>, LineMap) {
    let mut map = LineMap::default();
    if column_header && lines.len() > rows && lines.len() > 2 {
        lines.remove(1);
        map.header_removed = true;
    }
    let cursor_line = cursor.and_then(|(l, _, _)| map.without_header(l));
    if lines.len() <= rows || rows < 2 {
        let cursor = cursor.and_then(|(l, c, w)| map.map(l).map(|l| (l, c, w)));
        return (lines, cursor, map);
    }
    let title = lines.remove(0);
    let window = rows - 1;
    let focus = cursor_line.map_or(0, |l| l.saturating_sub(1));
    let start = focus
        .saturating_sub(window / 2)
        .min(lines.len().saturating_sub(window));
    map.scroll = Some((start, window));
    let mut out = vec![title];
    out.extend(lines.into_iter().skip(start).take(window));
    let cursor = cursor.and_then(|(l, c, w)| map.map(l).map(|l| (l, c, w)));
    (out, cursor, map)
}

/// How `fit_rows` moved panel lines on screen: column header removed and/or
/// the data rows scrolled to (start, window).
#[derive(Debug, Default, Clone, Copy)]
struct LineMap {
    header_removed: bool,
    scroll: Option<(usize, usize)>,
}

impl LineMap {
    fn without_header(&self, l: usize) -> Option<usize> {
        match (self.header_removed, l) {
            (true, 1) => None,
            (true, l) if l > 1 => Some(l - 1),
            (_, l) => Some(l),
        }
    }

    /// Screen line of panel line `l`, or None when it is not shown.
    fn map(&self, l: usize) -> Option<usize> {
        let l = self.without_header(l)?;
        match self.scroll {
            None => Some(l),
            Some(_) if l == 0 => Some(0),
            Some((start, window)) => {
                let r = l.checked_sub(1 + start)?;
                (r < window).then_some(1 + r)
            }
        }
    }
}

/// Saves the song in place (see `Viewer::save`). Returns 1 on success.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_save(viewer: *mut Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    match v.save() {
        Ok(()) => 1,
        Err(e) => {
            eprintln!("[viewer] save failed: {e}");
            0
        }
    }
}

/// Undo (`redo` = 0) or redo (`redo` = 1) the last edit. Returns 1 when
/// something changed (m8edit's history).
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_undo(viewer: *mut Viewer, redo: i32) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    v.undo(redo != 0) as i32
}

/// NEW without an open song (the home screen): writes NEW00.m8s (or the
/// first free NEWnn.m8s) in `dir`, from TEMPLATE.m8s in `root` when there
/// is one, else from the generated empty-song model. Writes the new path,
/// or a short error, into `out`; 1 on success.
///
/// # Safety
///
/// `dir` and `root` must be valid NUL-terminated strings; `out` must point
/// to `out_len` writable bytes or be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_new_song(
    dir: *const c_char,
    root: *const c_char,
    out: *mut c_char,
    out_len: usize,
) -> i32 {
    if dir.is_null() || root.is_null() {
        return 0;
    }
    // SAFETY: the caller passes valid C strings.
    let (dir, root) = unsafe {
        (
            CStr::from_ptr(dir).to_string_lossy(),
            CStr::from_ptr(root).to_string_lossy(),
        )
    };
    let template = std::path::Path::new(root.as_ref()).join("TEMPLATE.m8s");
    let (text, ok) =
        match pages::create_new_song(std::path::Path::new(dir.as_ref()), Some(&template)) {
            Ok(path) => (path.to_string_lossy().into_owned(), 1),
            Err(e) => (e, 0),
        };
    // SAFETY: the caller guarantees `out_len` writable bytes.
    unsafe { write_c_string(&text, out, out_len) };
    ok
}

/// Writes unsaved edits to NAME.autosave (the original is untouched).
/// Returns 1 on success.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_autosave(viewer: *const Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_ref() }.is_some_and(|v| v.autosave().is_ok()) as i32
}

/// Attaches the app's engine and the songs root; loads samples and
/// publishes the song. The engine must outlive the viewer.
///
/// # Safety
///
/// `viewer` live; `engine` NULL or live; `root` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_attach_engine(
    viewer: *mut Viewer,
    engine: *mut engine::ZtEngine,
    root: *const c_char,
) {
    // SAFETY: the caller passes live pointers.
    let (Some(v), false) = (unsafe { viewer.as_mut() }, root.is_null()) else {
        return;
    };
    let root = unsafe { CStr::from_ptr(root) }.to_string_lossy();
    v.attach_engine(engine, &root);
}

/// Per UI frame: frees retired snapshots and retries pending publication.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_tick(viewer: *mut Viewer) {
    // SAFETY: the caller passes a live viewer or NULL.
    if let Some(v) = unsafe { viewer.as_mut() } {
        v.tick();
    }
}

/// Stops playback.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_stop(viewer: *mut Viewer) {
    // SAFETY: the caller passes a live viewer or NULL.
    if let Some(v) = unsafe { viewer.as_mut() } {
        v.stop();
    }
}

/// Writes the 8 track peak levels (0..65535) into `out`.
///
/// # Safety
///
/// `viewer` live or NULL; `out` must hold 8 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_levels(viewer: *const Viewer, out: *mut u16) {
    // SAFETY: the caller passes a live viewer and an 8-value buffer.
    if let (Some(v), false) = (unsafe { viewer.as_ref() }, out.is_null()) {
        let levels = v.levels();
        unsafe { std::ptr::copy_nonoverlapping(levels.as_ptr(), out, 8) };
    }
}

/// Selection rectangle while selecting, in screen lines for a grid of
/// `rows` lines (same mapping as the cursor in `zt_viewer_frame`): writes
/// first line, last line, first column, end column and returns 1, else 0.
///
/// # Safety
///
/// `viewer` live or NULL; `out` must hold 4 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_selection(
    viewer: *const Viewer,
    rows: usize,
    cols: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: the caller passes a live viewer and a 4-value buffer.
    let (Some(v), false) = (unsafe { viewer.as_ref() }, out.is_null()) else {
        return 0;
    };
    if v.on_page() {
        let Some((l0, l1, c0, c1)) = v.timeline_selection(cols) else {
            return 0;
        };
        let vals = [l0 as i32, l1 as i32, c0 as i32, c1 as i32];
        // SAFETY: 4 writable values.
        unsafe { std::ptr::copy_nonoverlapping(vals.as_ptr(), out, 4) };
        return 1;
    }
    let Some((l0, l1, c0, c1)) = v.selection() else {
        return 0;
    };
    let (lines, cursor) = v.frame();
    let (_, _, map) = fit_rows(lines, cursor, rows, v.has_column_header());
    // Clamp to the visible lines: first and last panel lines that are shown.
    let shown: Vec<usize> = (l0..=l1).filter_map(|l| map.map(l)).collect();
    let (Some(&top), Some(&bottom)) = (shown.first(), shown.last()) else {
        return 0;
    };
    let vals = [top as i32, bottom as i32, c0 as i32, c1 as i32];
    // SAFETY: 4 writable values.
    unsafe { std::ptr::copy_nonoverlapping(vals.as_ptr(), out, 4) };
    1
}

/// 1 when the song has unsaved edits, else 0.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_dirty(viewer: *const Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_ref() }.is_some_and(Viewer::is_dirty) as i32
}

/// Writes the top-bar text (song name and tempo) into `out`, NUL-terminated.
///
/// # Safety
///
/// `viewer` must be live; `out` must point to `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_header(viewer: *const Viewer, out: *mut c_char, len: usize) {
    // SAFETY: the caller passes a live viewer and a writable buffer.
    if let Some(v) = unsafe { viewer.as_ref() } {
        unsafe { write_c_string(&v.header(), out, len) };
    }
}

/// Current page (0 TIMELINE, 1 MIXER, 2 TRACKS, 3 SONG), -1 for NULL.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_page(viewer: *const Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_ref() }.map_or(-1, |v| v.page.index())
}

/// Switches the page; other values are ignored.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_set_page(viewer: *mut Viewer, page: i32) {
    // SAFETY: the caller passes a live viewer or NULL.
    if let (Some(v), Some(p)) = (unsafe { viewer.as_mut() }, Page::from_index(page)) {
        v.page = p;
        v.select_anchor = None;
        v.message.clear();
    }
}

/// Highlight spans of the screen for a `rows` x `cols` grid: writes up to
/// `max` spans of 4 values (line, column, width, style) and returns how many.
///
/// # Safety
///
/// `viewer` live or NULL; `out` must hold `max * 4` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_highlights(
    viewer: *const Viewer,
    rows: usize,
    cols: usize,
    out: *mut i32,
    max: usize,
) -> usize {
    // SAFETY: the caller passes a live viewer or NULL.
    let (Some(v), false) = (unsafe { viewer.as_ref() }, out.is_null()) else {
        return 0;
    };
    let frame = v.sized_frame(rows, cols);
    let spans: Vec<_> = frame
        .highlights
        .iter()
        .filter(|(l, c, w, _)| *l < rows && *c < cols && *w > 0)
        .take(max)
        .collect();
    // SAFETY: the caller guarantees max * 4 writable values.
    let buf = unsafe { std::slice::from_raw_parts_mut(out, max * 4) };
    for (i, (l, c, w, style)) in spans.iter().enumerate() {
        let w = (*w).min(cols - *c);
        buf[i * 4..i * 4 + 4].copy_from_slice(&[*l as i32, *c as i32, w as i32, *style as i32]);
    }
    spans.len()
}

/// MIXER meter area: first line, last line, first column, column step of the
/// 9 strips (8 tracks and master). Returns 1 on the MIXER page, else 0.
///
/// # Safety
///
/// `viewer` live or NULL; `out` must hold 4 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_meter_area(viewer: *const Viewer, out: *mut i32) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let (Some(v), false) = (unsafe { viewer.as_ref() }, out.is_null()) else {
        return 0;
    };
    let Some((l0, l1, c0, step)) = v.mixer_meter_area() else {
        return 0;
    };
    let vals = [l0 as i32, l1 as i32, c0 as i32, step as i32];
    // SAFETY: 4 writable values.
    unsafe { std::ptr::copy_nonoverlapping(vals.as_ptr(), out, 4) };
    1
}

/// Keys 5-8 as slots 1-4 of the bottom bar on the MIXER, TRACKS and SONG
/// pages. Returns 1 when the page did something with it.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_soft_key(viewer: *mut Viewer, slot: i32) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    (slot >= 1 && v.soft_key(slot as usize)) as i32
}

/// The text field under the cursor (an instrument NAME): writes its value
/// into `out` and returns its maximum length, or 0 when there is none.
///
/// # Safety
///
/// `viewer` live or NULL; `out` must point to `len` writable bytes or be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_text_target(
    viewer: *const Viewer,
    out: *mut c_char,
    len: usize,
) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some((_, text, max)) = unsafe { viewer.as_ref() }.and_then(Viewer::text_target) else {
        return 0;
    };
    // SAFETY: the caller guarantees `len` writable bytes.
    unsafe { write_c_string(&text, out, len) };
    max as i32
}

/// Sets the text field under the cursor. Returns 1 when the song changed.
///
/// # Safety
///
/// `viewer` live or NULL; `text` a NUL-terminated string or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_set_text(viewer: *mut Viewer, text: *const c_char) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let (Some(v), false) = (unsafe { viewer.as_mut() }, text.is_null()) else {
        return 0;
    };
    // SAFETY: the caller passes a valid C string.
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    v.set_text(&text) as i32
}

/// 1 while the song plays, else 0./// 1 while the song plays, else 0.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_playing(viewer: *const Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_ref() }.is_some_and(|v| v.playing) as i32
}

/// Takes the pending request of the SONG page/// Takes the pending request of the SONG page: 0 none, 1 close the song
/// (LOAD), 2 open the file written into `out` (NEW).
///
/// # Safety
///
/// `viewer` live or NULL; `out` must point to `len` writable bytes or be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_take_request(
    viewer: *mut Viewer,
    out: *mut c_char,
    len: usize,
) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    match v.request.take() {
        None => 0,
        Some(Request::Close) => 1,
        Some(Request::Open(path)) => {
            // SAFETY: the caller guarantees `len` writable bytes.
            unsafe { write_c_string(&path, out, len) };
            2
        }
        Some(Request::Browse(kind)) => 2 + kind,
        Some(Request::EditSample(path)) => {
            // SAFETY: the caller guarantees `len` writable bytes.
            unsafe { write_c_string(&path, out, len) };
            5
        }
        Some(Request::EditEq) => 6,
        Some(Request::PreviewInstrument(slot)) => 0x100 | i32::from(slot),
        Some(Request::Record) => 7,
        Some(Request::Render) => 8,
    }
}

/// ENTER on the TRK page: open the row's instrument, or edit (and keep) a
/// pool value.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_tracks_enter(viewer: *mut Viewer) {
    // SAFETY: the caller passes a live viewer or NULL.
    if let Some(v) = unsafe { viewer.as_mut() } {
        v.tracks_enter();
    }
}

/// Replaces the instrument under the cursor with a .m8i file (undoable).
/// 1 on success; else the viewer's message says why.
///
/// # Safety
///
/// `viewer` must be live; `path` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_load_instrument(
    viewer: *mut Viewer,
    path: *const c_char,
) -> i32 {
    // SAFETY: the caller passes a live viewer.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    if path.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a valid C string.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    match v.load_instrument(&path) {
        Ok(()) => 1,
        Err(e) => {
            v.message = e;
            0
        }
    }
}

/// Points the SAMPLER under the cursor at `path` (under the songs root;
/// undoable). 1 on success; else the viewer's message says why.
///
/// # Safety
///
/// `viewer` must be live; `path` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_set_sample(viewer: *mut Viewer, path: *const c_char) -> i32 {
    // SAFETY: the caller passes a live viewer.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    if path.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a valid C string.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    match v.set_sample(&path) {
        Ok(()) => 1,
        Err(e) => {
            v.message = e;
            0
        }
    }
}

/// # Safety
///
/// `dst` must point to `len` writable bytes or be NULL.
pub(crate) unsafe fn write_c_string(s: &str, dst: *mut c_char, len: usize) {
    if dst.is_null() || len == 0 {
        return;
    }
    let bytes = s.as_bytes();
    let n = bytes.len().min(len - 1);
    // SAFETY: n + 1 <= len bytes are writable.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.cast::<u8>(), n);
        *dst.add(n) = 0;
    }
}

/// TIMELINE for the app's graphic view: `visible` song rows (at most
/// ZT_TIMELINE_MAX) from the scroll position that keeps the cursor on screen.
/// Returns 1 on the TIMELINE page, else 0.
///
/// # Safety
///
/// `viewer` live or NULL; `out` writable or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_timeline(
    viewer: *const Viewer,
    visible: usize,
    out: *mut TimelineView,
) -> i32 {
    // SAFETY: the caller passes a live viewer and a writable snapshot.
    let (Some(v), Some(out)) = (unsafe { viewer.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    if !v.on_page() || v.page != Page::Timeline {
        return 0;
    }
    *out = v.timeline_view(visible);
    1
}

/// MIXER for the app's graphic view. Returns 1 on the MIXER page, else 0.
///
/// # Safety
///
/// `viewer` live or NULL; `out` writable or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_mixer(viewer: *const Viewer, out: *mut MixerView) -> i32 {
    // SAFETY: the caller passes a live viewer and a writable snapshot.
    let (Some(v), Some(out)) = (unsafe { viewer.as_ref() }, unsafe { out.as_mut() }) else {
        return 0;
    };
    if !v.on_page() || v.page != Page::Mixer {
        return 0;
    }
    *out = v.mixer_view();
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Option<String> {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        std::path::Path::new(p).exists().then(|| p.to_string())
    }

    #[test]
    fn shift_right_walks_song_to_table_and_back() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        assert_eq!(v.kind, Kind::Song);
        // Find a non-empty song cell on row 0.
        let Some(t) = (0..8).find(|&t| v.song().song.steps[t] != 0xFF) else {
            return;
        };
        v.track = t;
        v.input(BTN_SHIFT, BTN_RIGHT);
        assert_eq!(v.kind, Kind::Chain);
        assert!(v.frame().0[0].contains("CHAIN"));
        v.input(BTN_SHIFT, BTN_LEFT);
        assert_eq!(v.kind, Kind::Song);
    }

    #[test]
    fn chain_and_phrase_sit_side_by_side() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        assert!(v.chain_phrase().is_none(), "not on the song level");
        let Some(t) = (0..8).find(|&t| v.song().song.steps[t] != 0xFF) else {
            return;
        };
        v.track = t;
        v.input(BTN_SHIFT, BTN_RIGHT);
        let view = v.chain_phrase().unwrap();
        assert_eq!((view.focus, view.chain), (0, v.chain));
        let phrase = v.song().chains[v.chain as usize].steps[0].phrase;
        assert_eq!(view.phrase_valid == 1, phrase != 0xFF);
        if phrase == 0xFF {
            return;
        }
        assert_eq!(view.phrase, phrase);
        assert_eq!(view.chain_cells[0][0].role, split::CELL_PHRASE);
        // RIGHT past the transpose enters the phrase on its note.
        v.input(0, BTN_RIGHT);
        v.input(0, BTN_RIGHT);
        let view = v.chain_phrase().unwrap();
        assert_eq!((view.focus, view.phrase, view.phrase_field), (1, phrase, 0));
        // LEFT from the note goes back to the chain.
        v.input(0, BTN_LEFT);
        assert_eq!(v.chain_phrase().unwrap().focus, 0);
    }

    #[test]
    fn instrument_page_marks_numeric_values() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        assert!(v.instrument_page(12).is_none());
        // The first instrument m8view lays out (some types are not supported).
        v.kind = Kind::Instrument;
        let Some(i) = (0..=0x7Fu8).find(|&i| {
            v.instrument = i;
            !v.song().instruments[i as usize].is_empty() && !v.spans().is_empty()
        }) else {
            return;
        };
        v.instrument = i;
        let page = v.instrument_page(12).unwrap();
        assert!(page.line_count as usize <= 12);
        assert!(page.span_count > 0);
        assert!(page.cursor_line >= 0);
        let numbers = page.spans[..page.span_count as usize]
            .iter()
            .filter(|s| s.kind != instrument_page::SPAN_TEXT)
            .count();
        assert!(numbers > 0, "some values get a bar");
    }

    #[test]
    fn save_keeps_bytes_and_makes_a_backup() {
        let src = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/V6_6EMPTY.m8s"
        );
        if !std::path::Path::new(src).exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("zt-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SONG.m8s");
        std::fs::copy(src, &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut v = Viewer::open(path.to_str().unwrap()).unwrap();
        v.input(BTN_SHIFT | BTN_PLAY, BTN_PLAY);
        assert_eq!(v.message, "SAVED");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(dir.join("SONG.m8s.bak")).unwrap(), original);
        assert!(!dir.join("SONG.m8s.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn edit_undo_redo_and_save_reach_the_file() {
        let src = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/V6_6EMPTY.m8s"
        );
        if !std::path::Path::new(src).exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("zt-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("EDIT.m8s");
        std::fs::copy(src, &path).unwrap();
        let mut v = Viewer::open(path.to_str().unwrap()).unwrap();
        assert_eq!(v.song().song.steps[0], 0xFF);

        // EDIT on the empty SONG cell inserts a chain, EDIT+RIGHT raises it.
        v.input(BTN_EDIT, BTN_EDIT);
        let inserted = v.song().song.steps[0];
        assert_ne!(inserted, 0xFF);
        v.input(BTN_EDIT, BTN_RIGHT);
        assert_eq!(v.song().song.steps[0], inserted + 1);
        assert!(v.is_dirty());
        assert!(v.header().contains('*'));

        assert!(v.undo(false));
        assert_eq!(v.song().song.steps[0], inserted);
        assert!(v.undo(true));
        assert_eq!(v.song().song.steps[0], inserted + 1);

        v.input(BTN_SHIFT | BTN_PLAY, BTN_PLAY);
        assert_eq!(v.message, "SAVED");
        assert!(!v.is_dirty());
        let saved = Viewer::open(path.to_str().unwrap()).unwrap();
        assert_eq!(saved.song().song.steps[0], inserted + 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn select_copy_paste_on_a_phrase() {
        let src = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/V6_6EMPTY.m8s"
        );
        if !std::path::Path::new(src).exists() {
            return;
        }
        let mut v = Viewer::open(src).unwrap();
        v.kind = Kind::Phrase;
        v.input(BTN_EDIT, BTN_EDIT); // note on row 0
        let note = v.song().phrases[0].steps[0].note;
        v.input(BTN_SHIFT | BTN_OPTION, BTN_OPTION);
        assert_eq!(v.message, "SELECT");
        v.input(BTN_OPTION, BTN_OPTION);
        assert_eq!(v.message, "COPIED");
        v.input(0, BTN_DOWN);
        v.input(0, BTN_DOWN);
        v.input(BTN_SHIFT | BTN_EDIT, BTN_EDIT);
        assert_eq!(v.song().phrases[0].steps[2].note, note);
        assert!(v.undo(false));
        assert_ne!(v.song().phrases[0].steps[2].note, note);
    }

    /// With ZT_M8_ROOT and ZT_TEST_SONG: plays the song through the engine
    /// without UI and prints the track status (peaks and positions).
    #[test]
    fn plays_a_real_song_when_configured() {
        let (Some(root), Some(song)) = (
            std::env::var("ZT_M8_ROOT").ok(),
            std::env::var("ZT_TEST_SONG").ok(),
        ) else {
            return;
        };
        let engine = engine::zt_engine_create(44100, 0);
        let mut v = Viewer::open(&song).unwrap();
        v.attach_engine(engine, &root);
        let ok = v
            .bank
            .slots
            .iter()
            .filter(|s| matches!(s, Some(Ok(_))))
            .count();
        println!(
            "message {:?}, sample slots ok {ok}, pending {}",
            v.message,
            v.pending.is_some()
        );
        let sm = &v.song().phrases[0].steps[0];
        println!(
            "phrase 00 step 0: note {:?} vel {} instr {}",
            sm.note.0, sm.velocity, sm.instrument
        );
        v.input(0, BTN_PLAY);
        let mut buf = vec![0i16; 512];
        let mut max = 0i32;
        for block in 0..400 {
            unsafe { engine::zt_engine_render(engine, buf.as_mut_ptr(), 256, 2) };
            max = max.max(buf.iter().map(|&x| (x as i32).abs()).max().unwrap());
            v.tick();
            if block % 100 == 99 {
                let st: Vec<_> = (0..8)
                    .map(|t| {
                        let s = v.engine().unwrap().track_status(t).unwrap();
                        (s.active, s.song_row, s.phrase_step, s.peak)
                    })
                    .collect();
                println!("block {block}: {st:?}");
            }
        }
        println!("max sample {max}, levels {:?}", v.levels());
        drop(v);
        unsafe { engine::zt_engine_destroy(engine) };
    }

    #[test]
    fn shift_option_then_edit_clones_a_chain() {
        let src = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/V6_6EMPTY.m8s"
        );
        if !std::path::Path::new(src).exists() {
            return;
        }
        let mut v = Viewer::open(src).unwrap();
        v.input(BTN_EDIT, BTN_EDIT); // chain on song row 00
        let original = v.song().song.steps[0];
        v.input(BTN_SHIFT | BTN_OPTION, BTN_OPTION);
        assert!(v.selection().is_some());
        v.input(BTN_EDIT, BTN_EDIT);
        let clone = v.song().song.steps[0];
        assert_ne!(clone, original);
        assert_ne!(clone, 0xFF);
        assert!(v.selection().is_none());
    }

    #[test]
    fn fit_rows_keeps_title_and_cursor_row() {
        let lines: Vec<String> = (0..18).map(|i| format!("L{i}")).collect();
        // Cursor on data row 0x0E (line 2 + 14).
        let (out, cur, _) = fit_rows(lines, Some((16, 3, 3)), 12, true);
        assert_eq!(out.len(), 12);
        assert_eq!(out[0], "L0");
        let (l, _, _) = cur.unwrap();
        assert_eq!(out[l], "L16");
    }

    #[test]
    fn timeline_cursor_scrolls_the_song_rows() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        for _ in 0..20 {
            v.input(0, BTN_RIGHT);
        }
        v.input(0, BTN_DOWN);
        assert_eq!((v.song_row, v.track), (20, 1));
        // 40 columns: a 3-column label and 12 song rows of 3 columns.
        let f = v.sized_frame(11, 40);
        assert_eq!(v.song_left.get(), 9);
        assert_eq!(f.cursor, Some((2, 3 + 11 * 3, 2)));
        assert!(f.lines[0].starts_with("   09 0A"));
        assert!(f.lines[2].starts_with(" 2 "));
        assert_eq!(f.lines.len(), 11);
    }

    #[test]
    fn mixer_page_edits_volume_and_toggles_mute() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        v.page = Page::Mixer;
        v.input(0, BTN_RIGHT);
        let field = Field::MixerTrackVolume { track: 1 };
        let before = m8edit::read_field(v.song(), field).unwrap();
        v.input(BTN_EDIT, BTN_RIGHT);
        assert_ne!(m8edit::read_field(v.song(), field).unwrap(), before);
        assert!(v.is_dirty());
        v.input(0, BTN_OPTION);
        assert_eq!(v.mute, 0b10);
        // SOLO on the muted track 2 unmutes it: MUTE and SOLO exclude each other.
        v.input(BTN_SHIFT, BTN_OPTION);
        assert_eq!((v.mute, v.solo), (0, 0b10));
        let f = v.sized_frame(11, 40);
        assert_eq!(&f.lines[pages::MIXER_METER_LINES + 1][5..7], "-S");
        // SOLO is exclusive: track 3 takes it from track 2, again clears it.
        v.input(0, BTN_RIGHT);
        v.input(BTN_SHIFT, BTN_OPTION);
        assert_eq!(v.solo, 0b100);
        // MUTE on the soloed track 3 takes its SOLO off.
        v.input(0, BTN_OPTION);
        assert_eq!((v.mute, v.solo), (0b100, 0));
        v.input(BTN_SHIFT, BTN_OPTION);
        assert_eq!((v.mute, v.solo), (0, 0b100));
        v.input(BTN_SHIFT, BTN_OPTION);
        assert_eq!(v.solo, 0);
    }

    #[test]
    fn song_page_new_writes_an_empty_song_and_asks_to_open_it() {
        let Some(path) = fixture() else { return };
        let dir = std::env::temp_dir().join(format!("zt-new-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("A.m8s");
        std::fs::copy(&path, &song).unwrap();
        let mut v = Viewer::open(song.to_str().unwrap()).unwrap();
        v.page = Page::Song;
        v.song_field = 4;
        v.input(0, BTN_EDIT);
        let new = dir.join("NEW00.m8s");
        assert_eq!(
            v.request,
            Some(Request::Open(new.to_string_lossy().into_owned()))
        );
        let opened = Viewer::open(new.to_str().unwrap()).unwrap();
        assert!(opened.song().song.steps.iter().all(|&s| s == 0xFF));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn song_page_renames_the_song() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        v.page = Page::Song;
        v.song_field = 0;
        assert!(v.text_target().is_some());
        assert!(v.set_text("RENAMED"));
        assert_eq!(v.song().name.trim_end_matches(['\0', ' ']), "RENAMED");
        assert!(v.undo(false));
        assert_ne!(v.song().name.trim_end_matches(['\0', ' ']), "RENAMED");
    }

    #[test]
    fn song_page_edits_tempo_and_asks_to_close() {
        let Some(path) = fixture() else { return };
        let mut v = Viewer::open(&path).unwrap();
        v.page = Page::Song;
        let tempo = v.song().tempo;
        v.input(BTN_EDIT, BTN_UP);
        assert!((v.song().tempo - tempo - 1.0).abs() < 0.01);
        // To LOAD in the right-hand column: up to NAME, across, down twice.
        v.input(0, BTN_UP);
        v.input(0, BTN_RIGHT);
        v.input(0, BTN_DOWN);
        v.input(0, BTN_DOWN);
        v.input(0, BTN_EDIT);
        assert_eq!(v.request, Some(Request::Close));
    }
}
