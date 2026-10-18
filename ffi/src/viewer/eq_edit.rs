//! EQ editor: the instrument's three-band EQ as a
//! response curve with a handle per band, edited with the arrows instead of
//! the M8's table. The values and their steps follow the M8 file: gain
//! in hundredths of a dB (0.25 dB, or 1 dB coarse), frequency in Hz, Q,
//! TYPE and MODE as the file stores them.

use super::Viewer;
use m8edit::{Command, FieldValue};
use m8fmt::{EqBand, EqModeType, Equ};
use m8view::{Field, InstrumentParameter};

/// No EQ on the instrument (its EQ field shows "--").
const NO_EQ: u8 = 0x80;

/// Band parameters for `zt_viewer_eq_adjust`.
pub const EQ_GAIN: i32 = 0;
pub const EQ_FREQ: i32 = 1;
pub const EQ_Q: i32 = 2;
pub const EQ_TYPE: i32 = 3;
pub const EQ_MODE: i32 = 4;

/// Limits. The M8 takes Q 01-99 and gains past -34 dB; the editor stops at
/// +-48 dB (a value beyond it in a file stays until it is edited, without a
/// jump).
const GAIN_LIMIT: i16 = 4800;
const FREQ_MIN: f64 = 20.0;
const FREQ_MAX: f64 = 20_000.0;
const Q_MIN: u8 = 1;
const Q_MAX: u8 = 99;
const TYPES: u8 = 7;
const MODES: u8 = 5;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct EqBandState {
    /// LOWCUT, LOWSHELF, BELL, BANDPASS, HI.SHELF, HI.CUT, ALLPASS.
    pub kind: u8,
    /// STEREO, MID, SIDE, LEFT, RIGHT.
    pub mode: u8,
    pub q: u8,
    pub freq: u16,
    /// Hundredths of a dB.
    pub gain: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct EqState {
    /// The EQ number (00-7F), or -1.
    pub index: i32,
    pub bands: [EqBandState; 3],
}

fn band_state(band: &EqBand) -> EqBandState {
    EqBandState {
        kind: band.mode.eq_type_hex(),
        mode: band.mode.eq_mode_hex(),
        q: band.q,
        freq: band.frequency().min(usize::from(u16::MAX)) as u16,
        gain: ((i16::from(band.level as i8)) << 8) | i16::from(band.level_fin),
    }
}

fn set_band(band: &mut EqBand, s: EqBandState) {
    band.mode = EqModeType(s.kind | (s.mode << 5));
    band.q = s.q;
    band.freq = (s.freq >> 8) as u8;
    band.freq_fin = (s.freq & 0xFF) as u8;
    let gain = s.gain as u16;
    band.level = (gain >> 8) as u8;
    band.level_fin = (gain & 0xFF) as u8;
}

fn bands(eq: &Equ) -> [&EqBand; 3] {
    [&eq.low, &eq.mid, &eq.high]
}

impl Viewer {
    fn eq_field(&self) -> Field {
        Field::Instrument {
            index: self.instrument,
            parameter: InstrumentParameter::Eq,
        }
    }

    /// The current instrument's EQ number, if it has one.
    pub fn eq_index(&self) -> Option<u8> {
        match m8edit::read_field(self.song(), self.eq_field()) {
            Ok(FieldValue::Byte(i)) if i < NO_EQ && usize::from(i) < self.song().eqs.len() => {
                Some(i)
            }
            _ => None,
        }
    }

    /// Opens the instrument's EQ, giving it the EQ with its own number when
    /// it has none (as the M8 does on EDIT).
    pub fn eq_open(&mut self) -> Option<u8> {
        if let Some(i) = self.eq_index() {
            return Some(i);
        }
        let own = self.instrument;
        if usize::from(own) >= self.song().eqs.len() {
            return None;
        }
        let field = self.eq_field();
        self.run(Command::Set {
            field,
            value: FieldValue::Byte(own),
        });
        self.eq_index()
    }

    pub fn eq_state(&self) -> EqState {
        let Some(i) = self.eq_index() else {
            return EqState {
                index: -1,
                ..Default::default()
            };
        };
        let eq = &self.song().eqs[usize::from(i)];
        let b = bands(eq);
        EqState {
            index: i32::from(i),
            bands: [band_state(b[0]), band_state(b[1]), band_state(b[2])],
        }
    }

    fn eq_write(&mut self, band: usize, change: impl FnOnce(&mut EqBandState)) -> bool {
        let Some(i) = self.eq_index() else {
            return false;
        };
        let mut eq = self.song().eqs[usize::from(i)].clone();
        let target = match band {
            0 => &mut eq.low,
            1 => &mut eq.mid,
            2 => &mut eq.high,
            _ => return false,
        };
        let mut s = band_state(target);
        change(&mut s);
        set_band(target, s);
        self.run(Command::ReplaceEq {
            index: i,
            eq: Box::new(eq),
        })
    }

    /// One step of a band's value: `dir` -1 or 1, `coarse` for big steps.
    pub fn eq_adjust(&mut self, band: usize, param: i32, dir: i32, coarse: bool) -> bool {
        let dir = dir.signum();
        self.eq_write(band, |s| match param {
            EQ_GAIN => {
                let step = if coarse { 100 } else { 25 };
                let limit = i32::from(GAIN_LIMIT).max(i32::from(s.gain).abs());
                s.gain = (i32::from(s.gain) + dir * step).clamp(-limit, limit) as i16;
            }
            EQ_FREQ => {
                // A step is a ratio, so it feels the same across the range;
                // at least 1 Hz.
                let ratio: f64 = if coarse { 1.12 } else { 1.02 };
                let f = f64::from(s.freq);
                let next = if dir > 0 { f * ratio } else { f / ratio };
                let next = next.round().clamp(FREQ_MIN, FREQ_MAX);
                let next = if next == f { f + f64::from(dir) } else { next };
                s.freq = next.clamp(FREQ_MIN, FREQ_MAX) as u16;
            }
            EQ_Q => {
                let step = if coarse { 10 } else { 1 };
                s.q = (i32::from(s.q) + dir * step).clamp(i32::from(Q_MIN), i32::from(Q_MAX)) as u8;
            }
            EQ_TYPE => s.kind = (i32::from(s.kind) + dir).rem_euclid(i32::from(TYPES)) as u8,
            EQ_MODE => s.mode = (i32::from(s.mode) + dir).rem_euclid(i32::from(MODES)) as u8,
            _ => {}
        })
    }

    /// The band back to the M8's default (LOW shelf 100 Hz, MID bell 1 kHz,
    /// HIGH shelf 5 kHz, 0 dB, Q 50, STEREO).
    pub fn eq_reset(&mut self, band: usize) -> bool {
        let default = match band {
            0 => EqBand::default_low(),
            1 => EqBand::default_mid(),
            2 => EqBand::default_high(),
            _ => return false,
        };
        self.eq_write(band, |s| *s = band_state(&default))
    }
}

/// The response of one band in dB at `freq` Hz, for drawing: the engine's own
/// filter (`engine::band_response_db`), so the curve is what plays.
pub fn band_db(s: &EqBandState, freq: f64) -> f64 {
    let gain = s.gain as u16;
    let band = m8fmt::EqBand {
        mode: m8fmt::EqModeType((s.kind & 0x07) | ((s.mode & 0x07) << 5)),
        freq: (s.freq >> 8) as u8,
        freq_fin: (s.freq & 0xFF) as u8,
        level: (gain >> 8) as u8,
        level_fin: (gain & 0xFF) as u8,
        q: s.q,
    };
    engine::band_response_db(&band, freq)
}

/// `n` frequencies from 20 Hz to 20 kHz on a log scale.
pub fn curve_freq(i: usize, n: usize) -> f64 {
    let t = if n > 1 {
        i as f64 / (n - 1) as f64
    } else {
        0.0
    };
    FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(t)
}

/// Opens the current instrument's EQ (giving it one if it has none);
/// its number, or -1 when the instrument has no EQ field.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_eq_open(viewer: *mut Viewer) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_mut() }
        .and_then(Viewer::eq_open)
        .map_or(-1, i32::from)
}

/// Fills `out` with the current instrument's EQ; its index is -1 without one.
///
/// # Safety
///
/// `viewer` live or NULL; `out` writable or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_eq_state(viewer: *const Viewer, out: *mut EqState) {
    // SAFETY: the caller passes a live viewer or NULL, and `out` writable.
    if let (Some(v), Some(out)) = (unsafe { viewer.as_ref() }, unsafe { out.as_mut() }) {
        *out = v.eq_state();
    }
}

impl Viewer {
    /// Tracks playing the instrument whose EQ is open, bit 0 track 1: the
    /// EQ editor's spectrum is theirs.
    pub fn eq_tracks(&self) -> u8 {
        (0..8)
            .filter(|&t| self.track_instrument(t) == Some(self.instrument))
            .fold(0, |mask, t| mask | 1 << t)
    }
}

/// The tracks playing the instrument whose EQ is open, as a mask (bit 0 is
/// track 1); 0 when none plays it.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_eq_tracks(viewer: *const Viewer) -> u8 {
    // SAFETY: the caller passes a live viewer or NULL.
    unsafe { viewer.as_ref() }.map_or(0, Viewer::eq_tracks)
}

/// One step of `param` (ZT_EQ_GAIN ...) on `band` 0-2; 1 when it changed.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_eq_adjust(
    viewer: *mut Viewer,
    band: i32,
    param: i32,
    dir: i32,
    coarse: i32,
) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    i32::from(v.eq_adjust(band.max(0) as usize, param, dir, coarse != 0))
}

/// `band` 0-2 back to the M8's default; 1 when it changed.
///
/// # Safety
///
/// `viewer` must be live or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_viewer_eq_reset(viewer: *mut Viewer, band: i32) -> i32 {
    // SAFETY: the caller passes a live viewer or NULL.
    let Some(v) = (unsafe { viewer.as_mut() }) else {
        return 0;
    };
    i32::from(v.eq_reset(band.max(0) as usize))
}

/// The response in dB at `n` log-spaced points from 20 Hz to 20 kHz: of
/// `band` 0-2 alone, or of the three together with `band` -1.
///
/// # Safety
///
/// `state` readable; `out` must point to `n` writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_eq_curve(state: *const EqState, band: i32, out: *mut f32, n: usize) {
    // SAFETY: the caller passes a readable state.
    let Some(s) = (unsafe { state.as_ref() }) else {
        return;
    };
    if out.is_null() {
        return;
    }
    // SAFETY: the caller guarantees `n` writable floats.
    let out = unsafe { std::slice::from_raw_parts_mut(out, n) };
    for (i, o) in out.iter_mut().enumerate() {
        let f = curve_freq(i, n);
        let db = match band {
            0..=2 => band_db(&s.bands[band as usize], f),
            _ => s.bands.iter().map(|b| band_db(b, f)).sum(),
        };
        *o = db as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bell(gain: i16) -> EqBandState {
        EqBandState {
            kind: 2,
            mode: 0,
            q: 50,
            freq: 1000,
            gain,
        }
    }

    #[test]
    fn a_bell_peaks_at_its_gain_and_is_flat_far_away() {
        let b = bell(600);
        assert!((band_db(&b, 1000.0) - 6.0).abs() < 0.1);
        assert!(band_db(&b, 20.0).abs() < 0.3);
        assert!((band_db(&bell(-600), 1000.0) + 6.0).abs() < 0.1);
    }

    #[test]
    fn cuts_and_shelves_go_the_right_way() {
        let low_cut = EqBandState { kind: 0, ..bell(0) };
        assert!(band_db(&low_cut, 50.0) < -20.0);
        assert!(band_db(&low_cut, 10_000.0).abs() < 0.5);
        let shelf = EqBandState {
            kind: 4,
            freq: 5000,
            ..bell(900)
        };
        assert!((band_db(&shelf, 18_000.0) - 9.0).abs() < 1.0);
        assert!(band_db(&shelf, 100.0).abs() < 0.5);
    }

    #[test]
    fn gain_round_trips_through_the_band_bytes() {
        for gain in [-4800i16, -25, 0, 25, 650, 4800] {
            let mut band = EqBand::default_mid();
            set_band(&mut band, bell(gain));
            assert_eq!(band_state(&band).gain, gain);
            assert!((band.gain() - f64::from(gain) / 100.0).abs() < 1e-9);
        }
    }
}
