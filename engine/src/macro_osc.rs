//! Braids macro-oscillator adapter for M8 MACROSYNTH voices.
//!
//! The upstream MIT-licensed DSP sources and their notices live in
//! `vendor/braids/`. A fixed-size, inline state block keeps note starts and
//! rendering allocation-free. Braids emits 24-sample blocks; the adapter
//! consumes those blocks one sample at a time from the audio callback.

use m8fmt::MacroSynthOsc;

const BRAIDS_STORAGE_WORDS: usize = 2_152;

#[repr(C, align(8))]
pub(crate) struct BraidsStorage([u64; BRAIDS_STORAGE_WORDS]);

/// Stateless Rust handle for the Braids voice stored inline in a Track.
pub(crate) struct MacroVoice;

unsafe extern "C" {
    fn zt_braids_voice_size() -> usize;
    fn zt_braids_voice_init(
        storage: *mut core::ffi::c_void,
        shape: u8,
        timbre: u8,
        color: u8,
        pitch: i16,
    );
    fn zt_braids_voice_transpose_pitch(storage: *mut core::ffi::c_void, pitch_units: i16);
    fn zt_braids_voice_set_parameters(storage: *mut core::ffi::c_void, timbre: u8, color: u8);
    fn zt_braids_voice_frame(storage: *mut core::ffi::c_void) -> f32;
}

impl MacroVoice {
    pub(crate) fn empty() -> BraidsStorage {
        BraidsStorage([0; BRAIDS_STORAGE_WORDS])
    }

    pub(crate) fn start(
        state: &mut BraidsStorage,
        shape: MacroSynthOsc,
        note: u8,
        timbre: u8,
        color: u8,
        sample_rate: u32,
    ) {
        let shape = u8::from(shape);
        let pitch = braids_pitch(note, 0, sample_rate);
        // SAFETY: state is an 8-byte-aligned fixed block. Its capacity is
        // checked against the C++ wrapper size below.
        unsafe {
            debug_assert!(zt_braids_voice_size() <= core::mem::size_of_val(state));
            zt_braids_voice_init(state.0.as_mut_ptr().cast(), shape, timbre, color, pitch);
        }
    }

    pub(crate) fn transpose(state: &mut BraidsStorage, semitones: f32) {
        let pitch_units = (semitones * 128.0).round() as i16;
        // SAFETY: the oscillator was initialized in `start` and storage remains
        // pinned inside its Track for the lifetime of the engine.
        unsafe { zt_braids_voice_transpose_pitch(state.0.as_mut_ptr().cast(), pitch_units) }
    }

    pub(crate) fn set_parameters(state: &mut BraidsStorage, timbre: u8, color: u8) {
        // SAFETY: the oscillator was initialized in `start` and storage remains
        // pinned inside its Track for the lifetime of the engine.
        unsafe { zt_braids_voice_set_parameters(state.0.as_mut_ptr().cast(), timbre, color) }
    }

    pub(crate) fn frame(state: &mut BraidsStorage) -> f32 {
        // SAFETY: the C++ wrapper owns and advances the initialized oscillator
        // and its fixed 24-sample cache in this same storage block.
        unsafe { zt_braids_voice_frame(state.0.as_mut_ptr().cast()) }
    }
}

fn braids_pitch(note: u8, transpose: i8, sample_rate: u32) -> i16 {
    // Braids' pitch table is referenced to MIDI 128; M8 note 36 is the 65.4 Hz
    // reference used by the isolated CALMFM control, one octave above that
    // table reference. The oscillator tables and fixed-time DSP are calibrated
    // for 48 kHz. Adjust pitch so playback at the engine's output rate preserves
    // the intended frequency.
    let midi_pitch = f64::from(i16::from(note) + 12 + i16::from(transpose))
        + 12.0 * (48_000.0 / f64::from(sample_rate.max(1))).log2();
    (midi_pitch * 128.0).round().clamp(0.0, 140.0 * 128.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braids_storage_fits_native_voice() {
        // The C++ wrapper has a compile-time size assertion against 17,216
        // bytes; Rust's inline state is exactly that size.
        assert_eq!(core::mem::size_of::<BraidsStorage>(), 17_216);
        assert!(unsafe { zt_braids_voice_size() } <= core::mem::size_of::<BraidsStorage>());
    }

    #[test]
    fn every_m8_macro_shape_renders_finite_samples() {
        for raw_shape in 0_u8..=47 {
            let shape = MacroSynthOsc::try_from(raw_shape).unwrap();
            let mut state = MacroVoice::empty();
            MacroVoice::start(&mut state, shape, 36, 0x3f, 0x80, 48_000);
            let samples: Vec<f32> = (0..8_192).map(|_| MacroVoice::frame(&mut state)).collect();
            assert!(
                samples.iter().all(|sample| sample.is_finite()),
                "shape {raw_shape}"
            );
            assert!(
                samples.iter().any(|sample| sample.abs() > 1e-5),
                "shape {raw_shape}"
            );
        }
    }

    #[test]
    fn parameter_updates_change_the_next_braids_block_without_restarting_voice() {
        let shape = MacroSynthOsc::try_from(3).unwrap();
        let mut baseline_state = MacroVoice::empty();
        let mut updated_state = MacroVoice::empty();
        MacroVoice::start(&mut baseline_state, shape, 60, 0, 0, 48_000);
        MacroVoice::start(&mut updated_state, shape, 60, 0, 0, 48_000);
        for _ in 0..24 {
            MacroVoice::frame(&mut baseline_state);
            MacroVoice::frame(&mut updated_state);
        }
        MacroVoice::set_parameters(&mut updated_state, 0xff, 0x80);

        let baseline: Vec<f32> = (0..240)
            .map(|_| MacroVoice::frame(&mut baseline_state))
            .collect();
        let updated: Vec<f32> = (0..240)
            .map(|_| MacroVoice::frame(&mut updated_state))
            .collect();
        assert!(
            baseline
                .iter()
                .zip(&updated)
                .any(|(before, after)| (before - after).abs() > 1e-4),
            "changing TIMBRE/COLOR should alter the oscillator output"
        );
    }

    #[test]
    fn braids_pitch_tracks_m8_semitones_and_transpose() {
        assert_eq!(braids_pitch(36, 0, 48_000), 48 * 128);
        assert_eq!(braids_pitch(36, 12, 48_000), 60 * 128);
        assert_eq!(braids_pitch(0, -12, 48_000), 0);
    }

    #[test]
    fn braids_pitch_compensates_its_48khz_tables_for_output_rate() {
        let pitch_48k = braids_pitch(33, 0, 48_000);
        let pitch_44k1 = braids_pitch(33, 0, 44_100);
        assert_eq!(pitch_44k1 - pitch_48k, 188);
    }
}
