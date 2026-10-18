//! The FX command picker for the app's graphic view: the commands as a grid
//! of names.

use super::Viewer;
use std::ffi::c_char;

/// Commands per row of the grid; the arrows move on it (picker_input).
pub const COLUMNS: usize = 6;
const MAX_ITEMS: usize = 96;

/// Mirrored in zt_viewer.h as ZtFxPicker.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FxPicker {
    pub count: u8,
    pub selected: u8,
    pub columns: u8,
    pub ids: [u8; MAX_ITEMS],
    /// NUL-terminated names, three letters.
    pub names: [[c_char; 4]; MAX_ITEMS],
}

impl Viewer {
    /// The open FX command picker, if any.
    pub fn fx_picker(&self) -> Option<FxPicker> {
        let p = self.picker.as_ref()?;
        let mut out = FxPicker {
            count: p.items.len().min(MAX_ITEMS) as u8,
            selected: p.selected.min(MAX_ITEMS - 1) as u8,
            columns: COLUMNS as u8,
            ids: [0; MAX_ITEMS],
            names: [[0; 4]; MAX_ITEMS],
        };
        for (i, (id, name)) in p.items.iter().take(MAX_ITEMS).enumerate() {
            out.ids[i] = *id;
            for (dst, b) in out.names[i].iter_mut().take(3).zip(name.bytes()) {
                *dst = b as c_char;
            }
        }
        Some(out)
    }
}

/// Fills `out` with the open FX command picker; 1 when one is open, else 0.
///
/// # Safety
///
/// `viewer` must be live; `out` must point to a writable `ZtFxPicker`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_fx_picker(viewer: *const Viewer, out: *mut FxPicker) -> i32 {
    // SAFETY: the caller passes a live viewer.
    let Some(picker) = (unsafe { viewer.as_ref() }).and_then(Viewer::fx_picker) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    // SAFETY: the caller passes a writable struct.
    unsafe { out.write(picker) };
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::{BTN_DOWN, BTN_EDIT, BTN_LEFT, BTN_RIGHT, BTN_UP, Kind};

    // EDIT+UP on a phrase FX command opens the grid; the arrows move on it.
    #[test]
    fn the_fx_picker_is_a_grid() {
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
        v.table_row = 0;
        v.table_col = 2;
        v.input(BTN_EDIT | BTN_UP, BTN_UP);
        let g = v.fx_picker().expect("picker open");
        assert!(usize::from(g.count) > COLUMNS);
        assert_eq!(g.names[0][0] as u8 as char, 'A');
        let start = usize::from(g.selected);
        v.input(0, BTN_RIGHT);
        v.input(0, BTN_DOWN);
        assert_eq!(
            usize::from(v.fx_picker().unwrap().selected),
            start + 1 + COLUMNS
        );
        v.input(0, BTN_UP);
        v.input(0, BTN_LEFT);
        assert_eq!(usize::from(v.fx_picker().unwrap().selected), start);
    }
}
