use crate::{EditError, FieldValue, Selection, read_field, write_field};
use m8fmt::Song;
use m8view::{Field, View, field_spans};
use std::mem::{Discriminant, discriminant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridKind {
    Song,
    Chain,
    Phrase,
    Table,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GridView {
    Song,
    Chain(u8),
    Phrase(u8),
    Table(u8),
}

impl GridView {
    fn kind(self) -> GridKind {
        match self {
            Self::Song => GridKind::Song,
            Self::Chain(_) => GridKind::Chain,
            Self::Phrase(_) => GridKind::Phrase,
            Self::Table(_) => GridKind::Table,
        }
    }
    fn rows(self) -> u8 {
        if matches!(self, Self::Song) { 255 } else { 15 }
    }
    fn columns(self) -> u8 {
        match self {
            Self::Song => 8,
            Self::Chain(_) => 2,
            Self::Phrase(_) => 9,
            Self::Table(_) => 8,
        }
    }
}

fn position(field: Field) -> Result<(GridView, u8, u8), EditError> {
    Ok(match field {
        Field::SongChain { row, track } if track < 8 => (GridView::Song, row, track),
        Field::ChainPhrase { chain, row } if row < 16 => (GridView::Chain(chain), row, 0),
        Field::ChainTranspose { chain, row } if row < 16 => (GridView::Chain(chain), row, 1),
        Field::PhraseNote { phrase, row } if row < 16 => (GridView::Phrase(phrase), row, 0),
        Field::PhraseVelocity { phrase, row } if row < 16 => (GridView::Phrase(phrase), row, 1),
        Field::PhraseInstrument { phrase, row } if row < 16 => (GridView::Phrase(phrase), row, 2),
        Field::PhraseFxCommand { phrase, row, lane } if row < 16 && lane < 3 => {
            (GridView::Phrase(phrase), row, 3 + lane * 2)
        }
        Field::PhraseFxValue { phrase, row, lane } if row < 16 && lane < 3 => {
            (GridView::Phrase(phrase), row, 4 + lane * 2)
        }
        Field::TableTranspose { table, row } if row < 16 => (GridView::Table(table), row, 0),
        Field::TableVelocity { table, row } if row < 16 => (GridView::Table(table), row, 1),
        Field::TableFxCommand { table, row, lane } if row < 16 && lane < 3 => {
            (GridView::Table(table), row, 2 + lane * 2)
        }
        Field::TableFxValue { table, row, lane } if row < 16 && lane < 3 => {
            (GridView::Table(table), row, 3 + lane * 2)
        }
        _ => return Err(EditError::IncompatibleSelection),
    })
}

fn field_at(view: GridView, row: u8, column: u8) -> Result<Field, EditError> {
    if row > view.rows() || column >= view.columns() {
        return Err(EditError::OutOfRange);
    }
    Ok(match view {
        GridView::Song => Field::SongChain { row, track: column },
        GridView::Chain(chain) => match column {
            0 => Field::ChainPhrase { chain, row },
            _ => Field::ChainTranspose { chain, row },
        },
        GridView::Phrase(phrase) => match column {
            0 => Field::PhraseNote { phrase, row },
            1 => Field::PhraseVelocity { phrase, row },
            2 => Field::PhraseInstrument { phrase, row },
            column if column % 2 == 1 => Field::PhraseFxCommand {
                phrase,
                row,
                lane: (column - 3) / 2,
            },
            column => Field::PhraseFxValue {
                phrase,
                row,
                lane: (column - 4) / 2,
            },
        },
        GridView::Table(table) => match column {
            0 => Field::TableTranspose { table, row },
            1 => Field::TableVelocity { table, row },
            column if column % 2 == 0 => Field::TableFxCommand {
                table,
                row,
                lane: (column - 2) / 2,
            },
            column => Field::TableFxValue {
                table,
                row,
                lane: (column - 3) / 2,
            },
        },
    })
}

pub(crate) enum Clipboard {
    Grid {
        kind: GridKind,
        cells: Vec<(u8, u8, FieldValue)>,
    },
    Instrument {
        kind: Discriminant<m8fmt::Instrument>,
        values: Vec<FieldValue>,
    },
}

impl Clipboard {
    pub(crate) fn copy(song: &Song, selection: Selection) -> Result<Self, EditError> {
        if let Field::Instrument { index, .. } = selection.anchor {
            return Self::copy_instrument(song, index, selection);
        }
        let (view, a_row, a_col) = position(selection.anchor)?;
        let (focus_view, b_row, b_col) = position(selection.focus)?;
        if view != focus_view {
            return Err(EditError::IncompatibleSelection);
        }
        let top = a_row.min(b_row);
        let left = a_col.min(b_col);
        let mut cells = Vec::new();
        for row in top..=a_row.max(b_row) {
            for column in left..=a_col.max(b_col) {
                let field = field_at(view, row, column)?;
                cells.push((row - top, column - left, read_field(song, field)?));
            }
        }
        Ok(Self::Grid {
            kind: view.kind(),
            cells,
        })
    }

    pub(crate) fn paste(&self, song: &mut Song, anchor: Field) -> Result<(), EditError> {
        if let Self::Instrument { kind, values } = self {
            return Self::paste_instrument(song, anchor, *kind, values);
        }
        let Self::Grid { kind, cells } = self else {
            unreachable!()
        };
        let (view, top, left) = position(anchor)?;
        if view.kind() != *kind {
            return Err(EditError::IncompatibleSelection);
        }
        for (row_offset, column_offset, value) in cells {
            let row = top.checked_add(*row_offset).ok_or(EditError::OutOfRange)?;
            let column = left
                .checked_add(*column_offset)
                .ok_or(EditError::OutOfRange)?;
            let field = field_at(view, row, column)?;
            write_field(song, field, value.clone())?;
        }
        Ok(())
    }

    fn copy_instrument(song: &Song, index: u8, selection: Selection) -> Result<Self, EditError> {
        let Field::Instrument {
            index: focus_index, ..
        } = selection.focus
        else {
            return Err(EditError::IncompatibleSelection);
        };
        if focus_index != index {
            return Err(EditError::IncompatibleSelection);
        }
        let instrument = song
            .instruments
            .get(usize::from(index))
            .ok_or(EditError::OutOfRange)?;
        let spans =
            field_spans(song, View::Instrument { index }).map_err(|_| EditError::OutOfRange)?;
        let start = spans
            .iter()
            .position(|span| span.field == selection.anchor)
            .ok_or(EditError::IncompatibleSelection)?;
        let end = spans
            .iter()
            .position(|span| span.field == selection.focus)
            .ok_or(EditError::IncompatibleSelection)?;
        let values = spans[start.min(end)..=start.max(end)]
            .iter()
            .map(|span| read_field(song, span.field))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::Instrument {
            kind: discriminant(instrument),
            values,
        })
    }

    fn paste_instrument(
        song: &mut Song,
        anchor: Field,
        kind: Discriminant<m8fmt::Instrument>,
        values: &[FieldValue],
    ) -> Result<(), EditError> {
        let Field::Instrument { index, .. } = anchor else {
            return Err(EditError::IncompatibleSelection);
        };
        let instrument = song
            .instruments
            .get(usize::from(index))
            .ok_or(EditError::OutOfRange)?;
        if discriminant(instrument) != kind {
            return Err(EditError::IncompatibleSelection);
        }
        let spans =
            field_spans(song, View::Instrument { index }).map_err(|_| EditError::OutOfRange)?;
        let start = spans
            .iter()
            .position(|span| span.field == anchor)
            .ok_or(EditError::IncompatibleSelection)?;
        let end = start
            .checked_add(values.len())
            .ok_or(EditError::OutOfRange)?;
        if end > spans.len() {
            return Err(EditError::OutOfRange);
        }
        let fields = spans[start..end]
            .iter()
            .map(|span| span.field)
            .collect::<Vec<_>>();
        for (field, value) in fields.into_iter().zip(values) {
            write_field(song, field, value.clone())?;
        }
        Ok(())
    }
}
