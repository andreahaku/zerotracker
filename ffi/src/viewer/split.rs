//! CHAIN and PHRASE side by side: the chain on the
//! left, the phrase of its selected row on the right, for the graphic view
//! of the app. TABLE uses the same view on its own, full width. The cell
//! texts come from m8view, so they read as the M8's.

use super::{Kind, Viewer};
use m8view::{Field, View};
use std::ffi::c_char;

/// Roles of a cell, for its colour in the app.
pub const CELL_EMPTY: u8 = 0;
pub const CELL_PHRASE: u8 = 1;
pub const CELL_TRANSPOSE: u8 = 2;
pub const CELL_NOTE: u8 = 3;
pub const CELL_VELOCITY: u8 = 4;
pub const CELL_INSTRUMENT: u8 = 5;
pub const CELL_FX_COMMAND: u8 = 6;
pub const CELL_FX_VALUE: u8 = 7;

pub const CHAIN_FIELDS: usize = 2;
pub const PHRASE_FIELDS: usize = 9;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SplitCell {
    /// NUL-terminated text, at most 7 characters.
    pub text: [c_char; 8],
    pub role: u8,
}

impl Default for SplitCell {
    fn default() -> Self {
        Self {
            text: [0; 8],
            role: CELL_EMPTY,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ChainPhraseView {
    pub chain: u8,
    pub phrase: u8,
    /// 0 when the selected chain row has no phrase.
    pub phrase_valid: u8,
    /// 0: the cursor is in the chain; 1: in the phrase.
    pub focus: u8,
    pub chain_row: u8,
    pub chain_field: u8,
    pub phrase_row: u8,
    pub phrase_field: u8,
    /// Row playing on the cursor's track, or -1.
    pub chain_play: i8,
    pub phrase_play: i8,
    pub chain_cells: [[SplitCell; CHAIN_FIELDS]; 16],
    /// In a TABLE: transpose, volume, 3 x (fx, value), then an empty cell.
    pub phrase_cells: [[SplitCell; PHRASE_FIELDS]; 16],
    /// 1: a TABLE (its number in `phrase`), drawn alone without the chain.
    pub table: u8,
}

fn role(field: Field) -> u8 {
    match field {
        Field::ChainPhrase { .. } => CELL_PHRASE,
        Field::ChainTranspose { .. } => CELL_TRANSPOSE,
        Field::PhraseNote { .. } => CELL_NOTE,
        Field::PhraseVelocity { .. } => CELL_VELOCITY,
        Field::PhraseInstrument { .. } => CELL_INSTRUMENT,
        Field::PhraseFxCommand { .. } => CELL_FX_COMMAND,
        Field::PhraseFxValue { .. } => CELL_FX_VALUE,
        Field::TableTranspose { .. } => CELL_TRANSPOSE,
        Field::TableVelocity { .. } => CELL_VELOCITY,
        Field::TableFxCommand { .. } => CELL_FX_COMMAND,
        Field::TableFxValue { .. } => CELL_FX_VALUE,
        _ => CELL_EMPTY,
    }
}

/// The cells of one view, row by row, from its rendered lines and spans.
fn cells<const N: usize>(viewer: &Viewer, view: View) -> [[SplitCell; N]; 16] {
    let mut out = [[SplitCell::default(); N]; 16];
    let Ok(frame) = m8view::render(viewer.song(), view) else {
        return out;
    };
    let lines = frame.plain_lines();
    let mut spans = m8view::field_spans(viewer.song(), view).unwrap_or_default();
    spans.sort_by_key(|s| (s.line, s.column));
    for (row, row_cells) in out.iter_mut().enumerate() {
        let line: Vec<char> = lines
            .get(2 + row)
            .map(|l| l.chars().collect())
            .unwrap_or_default();
        for (i, span) in spans
            .iter()
            .filter(|s| s.line == 2 + row)
            .take(N)
            .enumerate()
        {
            let text: String = line
                .iter()
                .skip(span.column)
                .take(span.width.min(7))
                .collect();
            let cell = &mut row_cells[i];
            for (dst, b) in cell.text.iter_mut().zip(text.bytes()) {
                *dst = b as c_char;
            }
            // Dashes only: an empty cell, drawn dim.
            cell.role = if text.trim().chars().all(|c| c == '-') {
                CELL_EMPTY
            } else {
                role(span.field)
            };
        }
        // The value of an empty FX command is dim too.
        for i in 1..N {
            if row_cells[i].role == CELL_FX_VALUE && row_cells[i - 1].role == CELL_EMPTY {
                row_cells[i].role = CELL_EMPTY;
            }
        }
    }
    out
}

impl Viewer {
    /// The CHAIN | PHRASE screen, when the cursor is in a chain or a phrase.
    pub fn chain_phrase(&self) -> Option<ChainPhraseView> {
        if self.picker.is_some() || !matches!(self.kind, Kind::Chain | Kind::Phrase | Kind::Table) {
            return None;
        }
        if self.kind == Kind::Table {
            return Some(ChainPhraseView {
                chain: 0,
                phrase: self.table,
                phrase_valid: 1,
                focus: 1,
                chain_row: 0,
                chain_field: 0,
                phrase_row: self.table_row as u8,
                phrase_field: self
                    .current_span()
                    .and_then(|cur| {
                        let mut spans = self.row_spans(self.row());
                        spans.sort_by_key(|s| s.column);
                        spans.iter().position(|s| s.field == cur.field)
                    })
                    .unwrap_or(0) as u8,
                chain_play: -1,
                phrase_play: -1,
                chain_cells: [[SplitCell::default(); CHAIN_FIELDS]; 16],
                phrase_cells: cells(self, View::Table { index: self.table }),
                table: 1,
            });
        }
        let focus = u8::from(self.kind == Kind::Phrase);
        // In the chain, the phrase of the selected row; in the phrase, itself.
        let (phrase, phrase_valid) = if focus == 1 {
            (self.phrase, true)
        } else {
            let p = self.song().chains[self.chain as usize].steps[self.chain_row].phrase;
            (p, p != 0xFF)
        };
        let field_index = |kind: Kind| {
            self.current_span()
                .filter(|_| self.kind == kind)
                .and_then(|cur| {
                    let mut spans = self.row_spans(self.row());
                    spans.sort_by_key(|s| s.column);
                    spans.iter().position(|s| s.field == cur.field)
                })
                .unwrap_or(0) as u8
        };
        let (mut chain_play, mut phrase_play) = (-1, -1);
        if let Some(s) = self
            .engine()
            .and_then(|e| e.track_status(self.track as u8))
            .filter(|s| s.active != 0)
        {
            if s.chain == self.chain {
                chain_play = s.chain_row as i8;
            }
            if phrase_valid && s.phrase == phrase {
                phrase_play = s.phrase_step as i8;
            }
        }
        Some(ChainPhraseView {
            chain: self.chain,
            phrase,
            phrase_valid: u8::from(phrase_valid),
            focus,
            chain_row: self.chain_row as u8,
            chain_field: field_index(Kind::Chain),
            phrase_row: if focus == 1 { self.phrase_row as u8 } else { 0 },
            phrase_field: field_index(Kind::Phrase),
            chain_play,
            phrase_play,
            chain_cells: cells(self, View::Chain { index: self.chain }),
            phrase_cells: if phrase_valid {
                cells(self, View::Phrase { index: phrase })
            } else {
                [[SplitCell::default(); PHRASE_FIELDS]; 16]
            },
            table: 0,
        })
    }
}

/// Fills `out` with the CHAIN | PHRASE screen; 1 when the cursor is in a
/// chain or a phrase (and no picker is open), else 0.
///
/// # Safety
///
/// `viewer` must be live; `out` must point to a writable `ZtChainPhrase`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_chain_phrase(
    viewer: *const Viewer,
    out: *mut ChainPhraseView,
) -> i32 {
    // SAFETY: the caller passes a live viewer.
    let Some(view) = (unsafe { viewer.as_ref() }).and_then(Viewer::chain_phrase) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a writable struct.
    unsafe { out.write(view) };
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    // TABLE shows on the graphic view, alone, with its eight fields.
    #[test]
    fn table_is_drawn_like_a_phrase() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        if !std::path::Path::new(p).exists() {
            return;
        }
        let mut v = Viewer::open(p).unwrap();
        v.kind = Kind::Table;
        v.table = 1;
        v.table_row = 3;
        let view = v.chain_phrase().unwrap();
        assert_eq!((view.table, view.phrase, view.phrase_row), (1, 1, 3));
        let row = &view.phrase_cells[0];
        assert_eq!(row[0].role, CELL_TRANSPOSE);
        assert!(row[..8].iter().all(|c| c.text[0] != 0));
        assert_eq!(row[8].text[0], 0);
    }
}
