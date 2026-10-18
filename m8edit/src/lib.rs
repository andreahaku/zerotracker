//! Key-independent editing of parsed songs.

use m8fmt::{Equ, FX, Instrument, Note, Song, Table};
use m8view::{Field, InstrumentParameter, fx_commands};
use std::collections::HashMap;
use std::mem::{Discriminant, discriminant};

const HISTORY_LIMIT: usize = 32;
mod clipboard;
mod instrument;
use clipboard::Clipboard;
pub use instrument::read_parameter as read_instrument_parameter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Fine,
    Coarse,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Byte(u8),
    Bool(bool),
    Text(String),
    Tempo(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Field,
    pub focus: Field,
}

#[derive(Clone, PartialEq)]
pub enum Command {
    Adjust {
        field: Field,
        direction: i8,
        granularity: Granularity,
    },
    InsertLast {
        field: Field,
    },
    Clear {
        field: Field,
    },
    Set {
        field: Field,
        value: FieldValue,
    },
    CreateChain {
        row: u8,
        track: u8,
        clone_from: Option<u8>,
        clone_phrases: bool,
    },
    CreatePhrase {
        chain: u8,
        row: u8,
        clone_from: Option<u8>,
    },
    ReplaceInstrument {
        slot: u8,
        instrument: Box<Instrument>,
        table: Option<Box<Table>>,
    },
    /// Swaps two instrument slots with their tables; phrase steps, and the
    /// INS/NXT commands in phrases and tables, follow (the POOL's
    /// EDIT+UP/DOWN on the name column).
    SwapInstruments {
        a: u8,
        b: u8,
    },
    /// Replaces one of the song's EQs (the EQ editor writes a whole band
    /// set per change, one undo step each).
    ReplaceEq {
        index: u8,
        eq: Box<Equ>,
    },
    Copy {
        selection: Selection,
    },
    Paste {
        anchor: Field,
    },
    Undo,
    Redo,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adjust {
                field,
                direction,
                granularity,
            } => f
                .debug_struct("Adjust")
                .field("field", field)
                .field("direction", direction)
                .field("granularity", granularity)
                .finish(),
            Self::InsertLast { field } => f.debug_tuple("InsertLast").field(field).finish(),
            Self::Clear { field } => f.debug_tuple("Clear").field(field).finish(),
            Self::Set { field, value } => f
                .debug_struct("Set")
                .field("field", field)
                .field("value", value)
                .finish(),
            Self::CreateChain {
                row,
                track,
                clone_from,
                clone_phrases,
            } => f
                .debug_struct("CreateChain")
                .field("row", row)
                .field("track", track)
                .field("clone_from", clone_from)
                .field("clone_phrases", clone_phrases)
                .finish(),
            Self::CreatePhrase {
                chain,
                row,
                clone_from,
            } => f
                .debug_struct("CreatePhrase")
                .field("chain", chain)
                .field("row", row)
                .field("clone_from", clone_from)
                .finish(),
            Self::SwapInstruments { a, b } => f
                .debug_struct("SwapInstruments")
                .field("a", a)
                .field("b", b)
                .finish(),
            Self::ReplaceEq { index, eq } => f
                .debug_struct("ReplaceEq")
                .field("index", index)
                .field("eq", eq)
                .finish(),
            Self::ReplaceInstrument {
                slot,
                instrument,
                table,
            } => f
                .debug_struct("ReplaceInstrument")
                .field("slot", slot)
                .field("instrument", instrument)
                .field("has_table", &table.is_some())
                .finish(),
            Self::Copy { selection } => f.debug_tuple("Copy").field(selection).finish(),
            Self::Paste { anchor } => f.debug_tuple("Paste").field(anchor).finish(),
            Self::Undo => f.write_str("Undo"),
            Self::Redo => f.write_str("Redo"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    pub changed: bool,
    pub assigned_index: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    OutOfRange,
    InvalidValue,
    UnsupportedOperation,
    NoFreeSlot,
    EmptyClipboard,
    IncompatibleSelection,
}

type FieldKind = (
    Discriminant<Field>,
    Option<Discriminant<InstrumentParameter>>,
);

fn field_kind(field: Field) -> FieldKind {
    let parameter = match field {
        Field::Instrument { parameter, .. } => Some(discriminant(&parameter)),
        _ => None,
    };
    (discriminant(&field), parameter)
}

pub struct Editor {
    song: Song,
    saved: Song,
    undo: Vec<Song>,
    redo: Vec<Song>,
    last: HashMap<FieldKind, FieldValue>,
    clipboard: Option<Clipboard>,
}

impl Editor {
    pub fn new(song: Song) -> Self {
        Self {
            saved: song.clone(),
            song,
            undo: Vec::new(),
            redo: Vec::new(),
            last: HashMap::new(),
            clipboard: None,
        }
    }

    pub fn song(&self) -> &Song {
        &self.song
    }
    pub fn is_dirty(&self) -> bool {
        self.song != self.saved
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.song.clone();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn apply(&mut self, command: Command) -> Result<Outcome, EditError> {
        match command {
            Command::Undo => return Ok(self.undo()),
            Command::Redo => return Ok(self.redo()),
            Command::Copy { selection } => {
                self.clipboard = Some(Clipboard::copy(&self.song, selection)?);
                return Ok(Outcome::default());
            }
            _ => {}
        }
        let before = self.song.clone();
        let result = match command {
            Command::Adjust {
                field,
                direction,
                granularity,
            } => self.adjust(field, direction, granularity).map(|_| None),
            Command::InsertLast { field } => self.insert_last(field).map(|_| None),
            Command::Clear { field } => self.clear(field).map(|_| None),
            Command::Set { field, value } => self.set(field, value).map(|_| None),
            Command::CreateChain {
                row,
                track,
                clone_from,
                clone_phrases,
            } => self
                .create_chain(row, track, clone_from, clone_phrases)
                .map(Some),
            Command::CreatePhrase {
                chain,
                row,
                clone_from,
            } => self.create_phrase(chain, row, clone_from).map(Some),
            Command::ReplaceInstrument {
                slot,
                instrument,
                table,
            } => self
                .replace_instrument(slot, *instrument, table.map(|table| *table))
                .map(|_| None),
            Command::SwapInstruments { a, b } => self.swap_instruments(a, b).map(|_| None),
            Command::ReplaceEq { index, eq } => match self.song.eqs.get_mut(usize::from(index)) {
                Some(slot) => {
                    *slot = *eq;
                    Ok(None)
                }
                None => Err(EditError::OutOfRange),
            },
            Command::Paste { anchor } => self
                .clipboard
                .as_ref()
                .ok_or(EditError::EmptyClipboard)?
                .paste(&mut self.song, anchor)
                .map(|_| None),
            _ => unreachable!(),
        };
        let assigned_index = match result {
            Ok(index) => index,
            Err(error) => {
                self.song = before;
                return Err(error);
            }
        };
        let changed = self.song != before;
        if changed {
            if self.undo.len() == HISTORY_LIMIT {
                self.undo.remove(0);
            }
            self.undo.push(before);
            self.redo.clear();
        }
        Ok(Outcome {
            changed,
            assigned_index,
        })
    }

    fn undo(&mut self) -> Outcome {
        let Some(previous) = self.undo.pop() else {
            return Outcome::default();
        };
        self.redo.push(std::mem::replace(&mut self.song, previous));
        Outcome {
            changed: true,
            assigned_index: None,
        }
    }

    fn redo(&mut self) -> Outcome {
        let Some(next) = self.redo.pop() else {
            return Outcome::default();
        };
        self.undo.push(std::mem::replace(&mut self.song, next));
        Outcome {
            changed: true,
            assigned_index: None,
        }
    }

    fn swap_instruments(&mut self, a: u8, b: u8) -> Result<(), EditError> {
        let (a, b) = (usize::from(a), usize::from(b));
        let n = self.song.instruments.len().min(self.song.tables.len());
        if a == b || a >= n || b >= n {
            return Err(EditError::OutOfRange);
        }
        self.song.instruments.swap(a, b);
        self.song.tables.swap(a, b);
        let mut map = m8fmt::remapper::Remapper::default_ver(self.song.version);
        map.instrument_mapping.mapping.swap(a, b);
        for phrase in self.song.phrases.iter_mut() {
            *phrase = phrase.map_instruments(
                &map.instrument_mapping,
                &map.table_mapping,
                &map.eq_mapping,
            );
        }
        for table in self.song.tables.iter_mut() {
            *table = table.map_instr(&map.instrument_mapping, &map.table_mapping, &map.eq_mapping);
        }
        Ok(())
    }

    fn replace_instrument(
        &mut self,
        slot: u8,
        instrument: Instrument,
        table: Option<Table>,
    ) -> Result<(), EditError> {
        let index = usize::from(slot);
        if index >= self.song.instruments.len() {
            return Err(EditError::OutOfRange);
        }
        if table.is_some() && index >= self.song.tables.len() {
            return Err(EditError::OutOfRange);
        }

        self.song.instruments[index] = instrument;
        if let Some(table) = table {
            self.song.tables[index] = table;
        }
        Ok(())
    }

    fn set(&mut self, field: Field, value: FieldValue) -> Result<(), EditError> {
        write_field(&mut self.song, field, value.clone())?;
        if is_concrete(field, &value) {
            self.last.insert(field_kind(field), value);
        }
        Ok(())
    }

    fn clear(&mut self, field: Field) -> Result<(), EditError> {
        let old = read_field(&self.song, field)?;
        // A HYPERSYNTH interval clears to 00 first, then off, as on the M8.
        let cleared = match (field, &old) {
            (
                Field::Instrument {
                    parameter: InstrumentParameter::HyperInterval(_),
                    ..
                },
                FieldValue::Byte(0),
            ) => FieldValue::Byte(instrument::INTERVAL_OFF),
            _ => match instrument::mod_param_default(&self.song, field) {
                Some(value) => FieldValue::Byte(value),
                None => clear_value(field)?,
            },
        };
        write_field(&mut self.song, field, cleared)?;
        match field {
            Field::PhraseFxCommand { phrase, row, lane } => write_field(
                &mut self.song,
                Field::PhraseFxValue { phrase, row, lane },
                FieldValue::Byte(0),
            )?,
            Field::TableFxCommand { table, row, lane } => write_field(
                &mut self.song,
                Field::TableFxValue { table, row, lane },
                FieldValue::Byte(0),
            )?,
            _ => {}
        }
        if is_concrete(field, &old) {
            self.last.insert(field_kind(field), old);
        }
        Ok(())
    }

    fn insert_last(&mut self, field: Field) -> Result<(), EditError> {
        if is_concrete(field, &read_field(&self.song, field)?) {
            return Ok(());
        }
        let value = self
            .last
            .get(&field_kind(field))
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| default_insert_value(field))?;
        self.set(field, value)?;
        if let Field::PhraseNote { phrase, row } = field {
            for companion in [
                Field::PhraseVelocity { phrase, row },
                Field::PhraseInstrument { phrase, row },
            ] {
                self.insert_last(companion)?;
            }
        }
        Ok(())
    }

    fn adjust(
        &mut self,
        field: Field,
        direction: i8,
        granularity: Granularity,
    ) -> Result<(), EditError> {
        if direction != -1 && direction != 1 {
            return Err(EditError::InvalidValue);
        }
        if matches!(
            field,
            Field::PhraseFxCommand { .. } | Field::TableFxCommand { .. }
        ) {
            return Err(EditError::UnsupportedOperation);
        }
        let old = read_field(&self.song, field)?;
        let current = if is_concrete(field, &old) {
            old
        } else {
            self.last
                .get(&field_kind(field))
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| default_insert_value(field))?
        };
        let next = match current {
            FieldValue::Tempo(value) if field == Field::SongTempo => {
                let step = match granularity {
                    Granularity::Fine => 0.1,
                    Granularity::Coarse => 1.0,
                };
                let next = (value + f32::from(direction) * step).clamp(1.0, 999.0);
                FieldValue::Tempo((next * 100.0).round() / 100.0)
            }
            FieldValue::Byte(value) => {
                if field == Field::SongTranspose {
                    let step = match granularity {
                        Granularity::Fine => 1,
                        Granularity::Coarse => 12,
                    };
                    let signed = i16::from(value as i8);
                    let next = (signed + i16::from(direction) * step).clamp(-128, 127);
                    return self.set(field, FieldValue::Byte((next as i8) as u8));
                }
                let (min, max, fine, coarse) = byte_range(&self.song, field)?;
                let step = match granularity {
                    Granularity::Fine => fine,
                    Granularity::Coarse => coarse,
                };
                FieldValue::Byte(
                    (i16::from(value) + i16::from(direction) * i16::from(step))
                        .clamp(i16::from(min), i16::from(max)) as u8,
                )
            }
            FieldValue::Bool(value) => FieldValue::Bool(!value),
            FieldValue::Text(_) | FieldValue::Tempo(_) => {
                return Err(EditError::UnsupportedOperation);
            }
        };
        self.set(field, next)
    }

    fn create_chain(
        &mut self,
        row: u8,
        track: u8,
        clone_from: Option<u8>,
        clone_phrases: bool,
    ) -> Result<u8, EditError> {
        if track >= 8 || clone_from.is_some_and(|id| usize::from(id) >= self.song.chains.len()) {
            return Err(EditError::OutOfRange);
        }
        let target = first_free_chain(&self.song).ok_or(EditError::NoFreeSlot)?;
        let mut chain = clone_from
            .map(|id| self.song.chains[usize::from(id)].clone())
            .unwrap_or_default();
        if clone_phrases {
            let mut clones = HashMap::new();
            let mut reserved = Vec::new();
            for step in &mut chain.steps {
                if step.phrase == 0xFF {
                    continue;
                }
                let source = step.phrase;
                let new = if let Some(&existing) = clones.get(&source) {
                    existing
                } else {
                    let new =
                        first_free_phrase(&self.song, &reserved).ok_or(EditError::NoFreeSlot)?;
                    self.song.phrases[usize::from(new)] =
                        self.song.phrases[usize::from(source)].clone();
                    reserved.push(new);
                    clones.insert(source, new);
                    new
                };
                step.phrase = new;
            }
        }
        self.song.chains[usize::from(target)] = chain;
        self.song.song.steps[usize::from(row) * 8 + usize::from(track)] = target;
        Ok(target)
    }

    fn create_phrase(
        &mut self,
        chain: u8,
        row: u8,
        clone_from: Option<u8>,
    ) -> Result<u8, EditError> {
        if row >= 16
            || usize::from(chain) >= self.song.chains.len()
            || clone_from.is_some_and(|id| usize::from(id) >= self.song.phrases.len())
        {
            return Err(EditError::OutOfRange);
        }
        let target = first_free_phrase(&self.song, &[]).ok_or(EditError::NoFreeSlot)?;
        self.song.phrases[usize::from(target)] = clone_from
            .map(|id| self.song.phrases[usize::from(id)].clone())
            .unwrap_or_else(|| m8fmt::Phrase::default_ver(self.song.version));
        self.song.chains[usize::from(chain)].steps[usize::from(row)].phrase = target;
        Ok(target)
    }
}

fn first_free_chain(song: &Song) -> Option<u8> {
    let mut referenced = vec![false; song.chains.len()];
    for &id in &song.song.steps {
        if let Some(used) = referenced.get_mut(usize::from(id)) {
            *used = true;
        }
    }
    song.chains
        .iter()
        .enumerate()
        .find(|(id, chain)| !referenced[*id] && chain.is_empty())
        .map(|(id, _)| id as u8)
}

fn first_free_phrase(song: &Song, reserved: &[u8]) -> Option<u8> {
    let mut referenced = vec![false; song.phrases.len()];
    for chain in &song.chains {
        for step in &chain.steps {
            if let Some(used) = referenced.get_mut(usize::from(step.phrase)) {
                *used = true;
            }
        }
    }
    song.phrases
        .iter()
        .enumerate()
        .find(|(id, phrase)| {
            !referenced[*id] && !reserved.contains(&(*id as u8)) && phrase.is_empty()
        })
        .map(|(id, _)| id as u8)
}

fn is_concrete(field: Field, value: &FieldValue) -> bool {
    match value {
        // 0xFF is a value for these fields, but an interval switched off.
        FieldValue::Byte(0xFF) => {
            !matches!(
                field,
                Field::Instrument {
                    parameter: InstrumentParameter::HyperInterval(_),
                    ..
                }
            ) && matches!(
                field,
                Field::SongTranspose
                    | Field::MixerTrackVolume { .. }
                    | Field::MixerMasterVolume
                    | Field::MixerLimiter
                    | Field::ChainTranspose { .. }
                    | Field::TableTranspose { .. }
                    | Field::PhraseFxValue { .. }
                    | Field::TableFxValue { .. }
                    | Field::Instrument { .. }
            )
        }
        FieldValue::Text(text) => !text.is_empty(),
        _ => true,
    }
}

fn clear_value(field: Field) -> Result<FieldValue, EditError> {
    if field == Field::SongName {
        return Ok(FieldValue::Text(String::new()));
    }
    if field == Field::SongTempo {
        return Ok(FieldValue::Tempo(120.0));
    }
    let value = match field {
        Field::SongTranspose => 0,
        Field::MixerTrackVolume { .. } | Field::MixerMasterVolume => 0xE0,
        Field::MixerLimiter => 0,
        Field::SongChain { .. }
        | Field::ChainPhrase { .. }
        | Field::PhraseNote { .. }
        | Field::PhraseVelocity { .. }
        | Field::PhraseInstrument { .. }
        | Field::TableVelocity { .. }
        | Field::PhraseFxCommand { .. }
        | Field::TableFxCommand { .. } => 0xFF,
        Field::ChainTranspose { .. }
        | Field::TableTranspose { .. }
        | Field::PhraseFxValue { .. }
        | Field::TableFxValue { .. } => 0,
        Field::Instrument { .. } => return instrument::clear_value(field),
        Field::SongName | Field::SongTempo => unreachable!(),
    };
    Ok(FieldValue::Byte(value))
}

fn default_insert_value(field: Field) -> Result<FieldValue, EditError> {
    if field == Field::SongName {
        return Ok(FieldValue::Text(String::new()));
    }
    if field == Field::SongTempo {
        return Ok(FieldValue::Tempo(120.0));
    }
    let value = match field {
        Field::MixerTrackVolume { .. } | Field::MixerMasterVolume => 0xE0,
        Field::MixerLimiter => 0,
        Field::PhraseNote { .. } => 36, // C-4 in the current file encoding.
        Field::PhraseVelocity { .. } | Field::TableVelocity { .. } => 0x64,
        Field::Instrument { .. } => return instrument::default_insert_value(field),
        Field::SongName | Field::SongTempo => unreachable!(),
        _ => 0,
    };
    Ok(FieldValue::Byte(value))
}

fn byte_range(song: &Song, field: Field) -> Result<(u8, u8, u8, u8), EditError> {
    Ok(match field {
        Field::SongTranspose => (0, 255, 1, 12),
        Field::MixerTrackVolume { .. } | Field::MixerMasterVolume => (0, 255, 1, 16),
        Field::MixerLimiter => (0, 255, 1, 16),
        Field::SongChain { .. } | Field::ChainPhrase { .. } => (0, 254, 1, 16),
        Field::PhraseNote { .. } => (0, 127, 1, 12),
        Field::PhraseVelocity { .. } | Field::TableVelocity { .. } => (0, 127, 1, 16),
        Field::PhraseInstrument { .. } => (0, (song.instruments.len() - 1) as u8, 1, 16),
        Field::ChainTranspose { .. }
        | Field::TableTranspose { .. }
        | Field::PhraseFxValue { .. }
        | Field::TableFxValue { .. } => (0, 255, 1, 16),
        Field::Instrument { index, parameter } => instrument::byte_range(
            song.instruments
                .get(usize::from(index))
                .ok_or(EditError::OutOfRange)?,
            parameter,
        )?,
        _ => return Err(EditError::UnsupportedOperation),
    })
}

fn fx_ref(step: &m8fmt::Step, lane: u8) -> Result<&FX, EditError> {
    match lane {
        0 => Ok(&step.fx1),
        1 => Ok(&step.fx2),
        2 => Ok(&step.fx3),
        _ => Err(EditError::OutOfRange),
    }
}
fn fx_mut(step: &mut m8fmt::Step, lane: u8) -> Result<&mut FX, EditError> {
    match lane {
        0 => Ok(&mut step.fx1),
        1 => Ok(&mut step.fx2),
        2 => Ok(&mut step.fx3),
        _ => Err(EditError::OutOfRange),
    }
}
fn table_fx_ref(step: &m8fmt::TableStep, lane: u8) -> Result<&FX, EditError> {
    match lane {
        0 => Ok(&step.fx1),
        1 => Ok(&step.fx2),
        2 => Ok(&step.fx3),
        _ => Err(EditError::OutOfRange),
    }
}
fn table_fx_mut(step: &mut m8fmt::TableStep, lane: u8) -> Result<&mut FX, EditError> {
    match lane {
        0 => Ok(&mut step.fx1),
        1 => Ok(&mut step.fx2),
        2 => Ok(&mut step.fx3),
        _ => Err(EditError::OutOfRange),
    }
}

pub fn read_field(song: &Song, field: Field) -> Result<FieldValue, EditError> {
    if field == Field::SongName {
        return Ok(FieldValue::Text(song.name.clone()));
    }
    if field == Field::SongTempo {
        return Ok(FieldValue::Tempo(song.tempo));
    }
    if matches!(field, Field::Instrument { .. }) {
        return instrument::read_field(song, field);
    }
    let byte = match field {
        Field::SongTranspose => song.transpose,
        Field::MixerTrackVolume { track } => *song
            .mixer_settings
            .track_volume
            .get(usize::from(track))
            .ok_or(EditError::OutOfRange)?,
        Field::MixerMasterVolume => song.mixer_settings.master_volume,
        Field::MixerLimiter => song.mixer_settings.limiter.level,
        Field::SongChain { row, track } if track < 8 => *song
            .song
            .steps
            .get(usize::from(row) * 8 + usize::from(track))
            .ok_or(EditError::OutOfRange)?,
        Field::ChainPhrase { chain, row } => {
            song.chains
                .get(usize::from(chain))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .phrase
        }
        Field::ChainTranspose { chain, row } => {
            song.chains
                .get(usize::from(chain))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .transpose
        }
        Field::PhraseNote { phrase, row } => {
            song.phrases
                .get(usize::from(phrase))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .note
                .0
        }
        Field::PhraseVelocity { phrase, row } => {
            song.phrases
                .get(usize::from(phrase))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .velocity
        }
        Field::PhraseInstrument { phrase, row } => {
            song.phrases
                .get(usize::from(phrase))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .instrument
        }
        Field::PhraseFxCommand { phrase, row, lane } => {
            fx_ref(
                song.phrases
                    .get(usize::from(phrase))
                    .and_then(|v| v.steps.get(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .command
        }
        Field::PhraseFxValue { phrase, row, lane } => {
            fx_ref(
                song.phrases
                    .get(usize::from(phrase))
                    .and_then(|v| v.steps.get(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .value
        }
        Field::TableTranspose { table, row } => {
            song.tables
                .get(usize::from(table))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .transpose
        }
        Field::TableVelocity { table, row } => {
            song.tables
                .get(usize::from(table))
                .and_then(|v| v.steps.get(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .velocity
        }
        Field::TableFxCommand { table, row, lane } => {
            table_fx_ref(
                song.tables
                    .get(usize::from(table))
                    .and_then(|v| v.steps.get(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .command
        }
        Field::TableFxValue { table, row, lane } => {
            table_fx_ref(
                song.tables
                    .get(usize::from(table))
                    .and_then(|v| v.steps.get(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .value
        }
        _ => return Err(EditError::UnsupportedOperation),
    };
    Ok(FieldValue::Byte(byte))
}

fn require_byte(value: FieldValue) -> Result<u8, EditError> {
    if let FieldValue::Byte(value) = value {
        Ok(value)
    } else {
        Err(EditError::InvalidValue)
    }
}

fn write_field(song: &mut Song, field: Field, value: FieldValue) -> Result<(), EditError> {
    if field == Field::SongName {
        let FieldValue::Text(name) = value else {
            return Err(EditError::InvalidValue);
        };
        if !instrument::valid_text(&name, 12) {
            return Err(EditError::InvalidValue);
        }
        song.name = name;
        return Ok(());
    }
    if field == Field::SongTempo {
        let FieldValue::Tempo(tempo) = value else {
            return Err(EditError::InvalidValue);
        };
        if !tempo.is_finite() || !(1.0..=999.0).contains(&tempo) {
            return Err(EditError::InvalidValue);
        }
        song.tempo = tempo;
        return Ok(());
    }
    if matches!(field, Field::Instrument { .. }) {
        return instrument::write_field(song, field, value);
    }
    let byte = require_byte(value)?;
    match field {
        Field::SongTranspose => song.transpose = byte,
        Field::MixerTrackVolume { track } => {
            *song
                .mixer_settings
                .track_volume
                .get_mut(usize::from(track))
                .ok_or(EditError::OutOfRange)? = byte
        }
        Field::MixerMasterVolume => song.mixer_settings.master_volume = byte,
        Field::MixerLimiter => song.mixer_settings.limiter.level = byte,
        Field::SongChain { row, track }
            if track < 8 && (byte == 0xFF || usize::from(byte) < song.chains.len()) =>
        {
            *song
                .song
                .steps
                .get_mut(usize::from(row) * 8 + usize::from(track))
                .ok_or(EditError::OutOfRange)? = byte
        }
        Field::ChainPhrase { chain, row }
            if byte == 0xFF || usize::from(byte) < song.phrases.len() =>
        {
            song.chains
                .get_mut(usize::from(chain))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .phrase = byte
        }
        Field::ChainTranspose { chain, row } => {
            song.chains
                .get_mut(usize::from(chain))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .transpose = byte
        }
        Field::PhraseNote { phrase, row } if byte <= 0x80 || byte == 0xFF => {
            song.phrases
                .get_mut(usize::from(phrase))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .note = Note(byte)
        }
        Field::PhraseVelocity { phrase, row } if byte <= 0x7F || byte == 0xFF => {
            song.phrases
                .get_mut(usize::from(phrase))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .velocity = byte
        }
        Field::PhraseInstrument { phrase, row }
            if byte == 0xFF || usize::from(byte) < song.instruments.len() =>
        {
            song.phrases
                .get_mut(usize::from(phrase))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .instrument = byte
        }
        Field::PhraseFxCommand { .. } | Field::TableFxCommand { .. }
            if byte != 0xFF && !fx_commands(song, field).iter().any(|(id, _)| *id == byte) =>
        {
            return Err(EditError::InvalidValue);
        }
        Field::PhraseFxCommand { phrase, row, lane } => {
            fx_mut(
                song.phrases
                    .get_mut(usize::from(phrase))
                    .and_then(|v| v.steps.get_mut(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .command = byte
        }
        Field::PhraseFxValue { phrase, row, lane } => {
            fx_mut(
                song.phrases
                    .get_mut(usize::from(phrase))
                    .and_then(|v| v.steps.get_mut(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .value = byte
        }
        Field::TableTranspose { table, row } => {
            song.tables
                .get_mut(usize::from(table))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .transpose = byte
        }
        Field::TableVelocity { table, row } if byte <= 0x7F || byte == 0xFF => {
            song.tables
                .get_mut(usize::from(table))
                .and_then(|v| v.steps.get_mut(usize::from(row)))
                .ok_or(EditError::OutOfRange)?
                .velocity = byte
        }
        Field::TableFxCommand { table, row, lane } => {
            table_fx_mut(
                song.tables
                    .get_mut(usize::from(table))
                    .and_then(|v| v.steps.get_mut(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .command = byte
        }
        Field::TableFxValue { table, row, lane } => {
            table_fx_mut(
                song.tables
                    .get_mut(usize::from(table))
                    .and_then(|v| v.steps.get_mut(usize::from(row)))
                    .ok_or(EditError::OutOfRange)?,
                lane,
            )?
            .value = byte
        }
        Field::Instrument { .. } => return Err(EditError::UnsupportedOperation),
        _ => return Err(EditError::InvalidValue),
    }
    Ok(())
}
