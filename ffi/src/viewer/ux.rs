//! ZeroTracker's own keys, in place of the M8's
//! OPTION / EDIT / SHIFT combinations:
//!
//! - ENTER does what suits the field: opens a chain, phrase or instrument
//!   (making a new one on an empty cell), types a name, runs LOAD / SAVE /
//!   the sample, picks an FX command, or edits a value;
//! - EDIT toggles edit mode on any value: the arrows change it (UP/DOWN big
//!   steps, LEFT/RIGHT by one) until ENTER keeps it or ESC undoes it;
//! - ESC cancels an edit, then a selection, then goes back one level;
//! - OPEN, SELECT, COPY, PASTE and DEL (clear) for the action keys.

use super::{Dir, Kind, Viewer};
use m8edit::{Command, Selection};
use m8view::{Field, InstrumentParameter};

/// `zt_viewer_ux_state` bits, for the bottom bar.
pub const UX_EDITING: u32 = 1;
pub const UX_SELECTING: u32 = 1 << 1;
pub const UX_CAN_OPEN: u32 = 1 << 2;
pub const UX_INSTRUMENT: u32 = 1 << 3;
pub const UX_PICKER: u32 = 1 << 4;
/// SAVE found the instrument's file: the app asks before overwriting.
pub const UX_CONFIRM_OVERWRITE: u32 = 1 << 5;

/// ENTER results for the app.
pub const ENTER_NONE: i32 = 0;
pub const ENTER_DONE: i32 = 1;
pub const ENTER_TEXT: i32 = 2;

impl Viewer {
    /// A field that opens a level below: the chain of a song cell, the
    /// phrase of a chain step, the instrument of a phrase step.
    fn is_reference(field: Field) -> bool {
        matches!(
            field,
            Field::SongChain { .. } | Field::ChainPhrase { .. } | Field::PhraseInstrument { .. }
        )
    }

    pub fn ux_state(&self) -> u32 {
        let mut s = 0;
        if self.edit_mode.is_some() {
            s |= UX_EDITING;
        }
        if self.select_anchor.is_some() {
            s |= UX_SELECTING;
        }
        if self.picker.is_some() {
            s |= UX_PICKER;
        }
        if self.kind == Kind::Instrument {
            s |= UX_INSTRUMENT;
        }
        if self.save_armed.is_some() {
            s |= UX_CONFIRM_OVERWRITE;
        }
        if matches!(
            self.kind,
            Kind::Song | Kind::Chain | Kind::Phrase | Kind::Instrument
        ) {
            s |= UX_CAN_OPEN;
        }
        s
    }

    /// ENTER on the field under the cursor.
    pub fn ux_enter(&mut self) -> i32 {
        self.message.clear();
        if self.picker.is_some() {
            self.picker_choose();
            return ENTER_DONE;
        }
        if self.edit_mode.is_some() {
            self.edit_mode = None; // keep the value
            return ENTER_DONE;
        }
        let Some(field) = self.current_field() else {
            return ENTER_NONE;
        };
        if self.text_target().is_some() {
            return ENTER_TEXT;
        }
        if self.instrument_action(field) {
            return ENTER_DONE;
        }
        if Self::is_reference(field) {
            self.ux_open();
            return ENTER_DONE;
        }
        if matches!(
            field,
            Field::PhraseFxCommand { .. } | Field::TableFxCommand { .. }
        ) {
            self.adjust(Dir::Down); // opens the command list
            return ENTER_DONE;
        }
        self.ux_edit();
        ENTER_DONE
    }

    /// EDIT: edit mode on the value under the cursor, or out of it.
    pub fn ux_edit(&mut self) {
        if self.edit_mode.take().is_some() {
            return;
        }
        let Some(field) = self.current_field() else {
            return;
        };
        if matches!(
            field,
            Field::Instrument {
                parameter: InstrumentParameter::Load | InstrumentParameter::Save,
                ..
            }
        ) {
            return;
        }
        // An empty cell starts from the last value, as on the M8; counted
        // as an edit, so ESC takes it back too. TYPE has no last value.
        self.edit_mode = Some(0);
        if !matches!(
            field,
            Field::Instrument {
                parameter: InstrumentParameter::Kind,
                ..
            }
        ) {
            self.run(Command::InsertLast { field });
        }
    }

    /// OPEN: the chain, phrase or instrument under the cursor; an empty
    /// song or chain cell gets a new chain or phrase first.
    pub fn ux_open(&mut self) {
        self.edit_mode = None;
        if let Some(field) = self.current_field() {
            let empty = match field {
                Field::SongChain { row, track } => {
                    self.song().song.steps[row as usize * 8 + track as usize] == 0xFF
                }
                Field::ChainPhrase { chain, row } => {
                    self.song().chains[chain as usize].steps[row as usize].phrase == 0xFF
                }
                _ => false,
            };
            if empty {
                self.create_new(field);
            }
        }
        self.navigate(Dir::Right);
    }

    /// ESC: cancels an edit (undoing it), then a selection or the command
    /// list, then goes back one level. False on the song level: the app
    /// handles it (back to HOME).
    pub fn ux_back(&mut self) -> bool {
        if self.save_armed.take().is_some() {
            self.message.clear();
            return true;
        }
        if let Some(changes) = self.edit_mode.take() {
            for _ in 0..changes {
                self.undo(false);
            }
            self.message.clear();
            return true;
        }
        if self.picker.take().is_some() || self.select_anchor.take().is_some() {
            self.message.clear();
            return true;
        }
        if self.kind == Kind::Song {
            return false;
        }
        self.navigate(Dir::Left);
        true
    }

    /// SELECT: starts a selection at the cursor (the arrows grow it).
    pub fn ux_select(&mut self) {
        self.edit_mode = None;
        self.select_anchor = self.current_field();
        self.message = "SELECT".into();
    }

    /// COPY: the selection to the clipboard, and the selection ends.
    pub fn ux_copy(&mut self) {
        let (Some(anchor), Some(focus)) = (self.select_anchor.take(), self.current_field()) else {
            return;
        };
        self.message = match self.editor.apply(Command::Copy {
            selection: Selection { anchor, focus },
        }) {
            Ok(_) => "COPIED".into(),
            Err(e) => format!("{e:?}").to_uppercase(),
        };
    }

    /// PASTE: the clipboard at the cursor (a selection in progress ends).
    pub fn ux_paste(&mut self) {
        self.select_anchor = None;
        if let Some(anchor) = self.current_field() {
            self.message.clear();
            self.run(Command::Paste { anchor });
        }
    }

    /// LOAD and SAVE as action keys on INSTRUMENT, wherever the cursor is.
    pub fn ux_load(&mut self) {
        let index = self.instrument;
        self.instrument_action(Field::Instrument {
            index,
            parameter: InstrumentParameter::Load,
        });
    }

    /// The file SAVE would overwrite, for the app's question.
    pub fn overwrite_name(&self) -> Option<String> {
        self.save_armed
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    pub fn ux_save(&mut self) {
        let index = self.instrument;
        self.instrument_action(Field::Instrument {
            index,
            parameter: InstrumentParameter::Save,
        });
    }

    /// DEL: clears the field under the cursor.
    pub fn ux_clear(&mut self) {
        self.edit_mode = None;
        if let Some(field) = self.current_field() {
            self.message.clear();
            self.clear_field(field);
        }
    }
}

macro_rules! ux_fn {
    ($(#[$doc:meta])* $name:ident, $method:ident) => {
        $(#[$doc])*
        ///
        /// # Safety
        ///
        /// `viewer` must be live (or NULL).
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(viewer: *mut Viewer) {
            // SAFETY: the caller passes a live viewer or NULL.
            if let Some(v) = unsafe { viewer.as_mut() } {
                v.$method();
            }
        }
    };
}

ux_fn!(/// EDIT: edit mode on the value under the cursor, or out of it.
    zt_viewer_ux_edit, ux_edit);
ux_fn!(/// OPEN the chain, phrase or instrument under the cursor.
    zt_viewer_ux_open, ux_open);
ux_fn!(/// SELECT: start a selection at the cursor.
    zt_viewer_ux_select, ux_select);
ux_fn!(/// COPY the selection.
    zt_viewer_ux_copy, ux_copy);
ux_fn!(/// PASTE at the cursor.
    zt_viewer_ux_paste, ux_paste);
ux_fn!(/// DEL: clear the field under the cursor.
    zt_viewer_ux_clear, ux_clear);
ux_fn!(/// INSTRUMENT: LOAD (asks the app for the browser, request 3).
    zt_viewer_ux_load, ux_load);
ux_fn!(/// INSTRUMENT: SAVE the instrument to Instruments/NAME.m8i.
    zt_viewer_ux_save, ux_save);

/// ENTER: 0 nothing to do, 1 done, 2 the app opens text entry.
///
/// # Safety
///
/// `viewer` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_ux_enter(viewer: *mut Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_mut() }.map_or(ENTER_NONE, Viewer::ux_enter)
}

/// ESC: 1 when the viewer handled it (an edit, a selection, a level up).
///
/// # Safety
///
/// `viewer` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_ux_back(viewer: *mut Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_mut() }.is_some_and(Viewer::ux_back) as i32
}

/// The file name SAVE would overwrite (for the question), or "".
///
/// # Safety
///
/// `viewer` must be live (or NULL); `out` NULL or `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_overwrite_name(
    viewer: *const Viewer,
    out: *mut std::ffi::c_char,
    len: usize,
) {
    // SAFETY: the caller passes a live viewer or NULL.
    let name = unsafe { viewer.as_ref() }
        .and_then(Viewer::overwrite_name)
        .unwrap_or_default();
    // SAFETY: the caller guarantees `len` writable bytes.
    unsafe { crate::viewer::write_c_string(&name, out, len) };
}

/// UX_* bits: editing, selecting, can open, instrument, command list.
///
/// # Safety
///
/// `viewer` must be live (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_ux_state(viewer: *const Viewer) -> u32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_ref() }.map_or(0, Viewer::ux_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::{BTN_DOWN, BTN_RIGHT, BTN_UP};

    fn song() -> Option<Viewer> {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../m8fmt/examples/songs/TEST-FILE.m8s"
        );
        std::path::Path::new(p)
            .exists()
            .then(|| Viewer::open(p).unwrap())
    }

    #[test]
    fn enter_opens_and_esc_goes_back() {
        let Some(mut v) = song() else { return };
        let Some(t) = (0..8).find(|&t| v.song().song.steps[t] != 0xFF) else {
            return;
        };
        v.track = t;
        assert_eq!(v.ux_enter(), ENTER_DONE);
        assert_eq!(v.kind, Kind::Chain);
        assert!(v.ux_back());
        assert_eq!(v.kind, Kind::Song);
        assert!(!v.ux_back(), "the song level is the app's");
    }

    #[test]
    fn edit_mode_changes_a_value_and_esc_undoes_it() {
        let Some(mut v) = song() else { return };
        let Some(t) = (0..8).find(|&t| v.song().song.steps[t] != 0xFF) else {
            return;
        };
        v.track = t;
        v.ux_open();
        // The chain's transpose: a plain value.
        v.input(0, BTN_RIGHT);
        let field = v.current_field().unwrap();
        let before = m8edit::read_field(v.song(), field).unwrap();
        assert_eq!(v.ux_enter(), ENTER_DONE);
        assert_ne!(v.ux_state() & UX_EDITING, 0);
        v.input(0, BTN_UP);
        v.input(0, BTN_RIGHT);
        assert_ne!(m8edit::read_field(v.song(), field).unwrap(), before);
        assert_eq!(v.current_field(), Some(field), "the arrows edit, not move");
        assert!(v.ux_back());
        assert_eq!(
            m8edit::read_field(v.song(), field).unwrap(),
            before,
            "ESC undoes"
        );
        // ENTER keeps the change.
        v.ux_edit();
        v.input(0, BTN_DOWN);
        let changed = m8edit::read_field(v.song(), field).unwrap();
        assert_eq!(v.ux_enter(), ENTER_DONE);
        assert_eq!(v.ux_state() & UX_EDITING, 0);
        assert_eq!(m8edit::read_field(v.song(), field).unwrap(), changed);
    }

    #[test]
    fn select_copy_paste_and_clear() {
        let Some(mut v) = song() else { return };
        let Some(t) = (0..8).find(|&t| v.song().song.steps[t] != 0xFF) else {
            return;
        };
        v.track = t;
        v.ux_open();
        let first = v.current_field().unwrap();
        let value = m8edit::read_field(v.song(), first).unwrap();
        v.ux_select();
        assert_ne!(v.ux_state() & UX_SELECTING, 0);
        v.ux_copy();
        assert_eq!(v.message, "COPIED");
        // Down a few rows, paste: the cell takes the copied value.
        for _ in 0..5 {
            v.input(0, BTN_DOWN);
        }
        let target = v.current_field().unwrap();
        v.ux_paste();
        assert_eq!(m8edit::read_field(v.song(), target).unwrap(), value);
        v.ux_clear();
        assert_ne!(m8edit::read_field(v.song(), target).unwrap(), value);
    }
}
