//! INSTRUMENT for the app's graphic view: the M8 text
//! layout (visible lines, scrolled like the text view) and where each value
//! sits on them, so the app draws labels in bold and a bar beside each
//! numeric value.

use super::{Kind, Viewer, fit_rows};
use m8view::{Field, InstrumentParameter};
use std::ffi::c_char;

pub const MAX_LINES: usize = 24;
pub const LINE_LEN: usize = 48;
pub const MAX_SPANS: usize = 64;

/// A value shown as text (a type, a name, a mode).
pub const SPAN_TEXT: u8 = 0;
/// A value 00-FF: the app draws a bar from the left.
pub const SPAN_NUMBER: u8 = 1;
/// A value 00-FF centred on 80 (DETUNE, PAN, FINE): a bar from the middle.
pub const SPAN_BIPOLAR: u8 = 2;

const BIPOLAR_LABELS: [&str; 3] = ["DETUNE", "PAN", "FINE"];
/// Labels whose 00-FF value picks a kind (filter type, limiter, play mode),
/// not an amount: no bar.
const CHOICE_LABELS: [&str; 11] = [
    "FILTER", "LIM", "ALGO", "PLAY", "SLICE", "SHAPE", "TYPE", "SCALE", "OSC", "TRIG", "SRC",
];

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PageSpan {
    pub line: u8,
    pub column: u8,
    pub width: u8,
    pub kind: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InstrumentPage {
    pub line_count: u8,
    pub span_count: u8,
    /// Cursor (line, column, width), or line -1.
    pub cursor_line: i8,
    pub cursor_column: u8,
    pub cursor_width: u8,
    pub lines: [[c_char; LINE_LEN]; MAX_LINES],
    pub spans: [PageSpan; MAX_SPANS],
}

/// The label just left of `column` on `line` ("DETUNE" in "DETUNE   80").
fn label_before(line: &str, column: usize) -> String {
    let head: String = line.chars().take(column).collect();
    head.split_whitespace().last().unwrap_or("").to_string()
}

impl Viewer {
    /// The INSTRUMENT page fitted to `rows` lines, when it is on screen.
    pub fn instrument_page(&self, rows: usize) -> Option<InstrumentPage> {
        if self.picker.is_some() || self.kind != Kind::Instrument {
            return None;
        }
        let (mut lines, cursor) = self.frame();
        // The NAME shows as one run of 12 characters: m8view splits it 8 + 4
        // like the M8 screen, which reads as a space inside a typed name.
        if let Some(span) = self.spans().into_iter().find(|s| {
            matches!(
                s.field,
                Field::Instrument {
                    parameter: InstrumentParameter::Name,
                    ..
                }
            )
        }) && let Some(line) = lines.get_mut(span.line)
        {
            let name = self.song().instruments[self.instrument as usize]
                .name()
                .unwrap_or("")
                .to_string();
            let mut field: String = name.chars().take(12).collect();
            while field.chars().count() < span.width {
                field.push(if field.chars().count() < 12 { '-' } else { ' ' });
            }
            let mut chars: Vec<char> = line.chars().collect();
            chars.resize(chars.len().max(span.column + span.width), ' ');
            for (i, c) in field.chars().enumerate() {
                chars[span.column + i] = c;
            }
            *line = chars.into_iter().collect();
        }
        let (lines, cursor, map) = fit_rows(lines, cursor, rows, self.has_column_header());
        let mut page = InstrumentPage {
            line_count: lines.len().min(MAX_LINES) as u8,
            span_count: 0,
            cursor_line: -1,
            cursor_column: 0,
            cursor_width: 0,
            lines: [[0; LINE_LEN]; MAX_LINES],
            spans: [PageSpan::default(); MAX_SPANS],
        };
        for (dst, line) in page.lines.iter_mut().zip(&lines) {
            for (d, b) in dst.iter_mut().take(LINE_LEN - 1).zip(line.bytes()) {
                *d = b as c_char;
            }
        }
        if let Some((l, c, w)) = cursor.filter(|&(l, _, _)| l < MAX_LINES) {
            page.cursor_line = l as i8;
            page.cursor_column = c as u8;
            page.cursor_width = w as u8;
        }
        let mut n = 0;
        for span in self.spans() {
            let Some(line) = map.map(span.line).filter(|&l| l < lines.len()) else {
                continue;
            };
            if n == MAX_SPANS {
                break;
            }
            let text: String = lines[line]
                .chars()
                .skip(span.column)
                .take(span.width)
                .collect();
            let numeric = span.width == 2 && text.chars().all(|c| c.is_ascii_hexdigit());
            let label = label_before(&lines[line], span.column);
            // CHORD (a bank and six intervals) and a modulator's type and
            // DEST pick, they do not measure: no bars.
            let chord = matches!(
                span.field,
                Field::Instrument {
                    parameter: InstrumentParameter::HyperChord
                        | InstrumentParameter::HyperInterval(_)
                        | InstrumentParameter::ModType(_)
                        | InstrumentParameter::ModDest(_),
                    ..
                }
            );
            let kind = if !numeric || chord || CHOICE_LABELS.contains(&label.as_str()) {
                SPAN_TEXT
            } else if BIPOLAR_LABELS.contains(&label.as_str()) {
                SPAN_BIPOLAR
            } else {
                SPAN_NUMBER
            };
            page.spans[n] = PageSpan {
                line: line as u8,
                column: span.column as u8,
                width: span.width as u8,
                kind,
            };
            n += 1;
        }
        page.span_count = n as u8;
        Some(page)
    }
}

/// Fills `out` with the INSTRUMENT page fitted to `rows`; 1 when the cursor
/// is on an instrument (and no picker is open), else 0.
///
/// # Safety
///
/// `viewer` must be live; `out` must point to a writable `ZtInstrumentPage`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_instrument_page(
    viewer: *const Viewer,
    rows: usize,
    out: *mut InstrumentPage,
) -> i32 {
    // SAFETY: the caller passes a live viewer.
    let Some(page) = (unsafe { viewer.as_ref() }).and_then(|v| v.instrument_page(rows)) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a writable struct.
    unsafe { out.write(page) };
    1
}
