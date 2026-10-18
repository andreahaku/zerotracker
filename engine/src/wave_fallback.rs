//! Base-wave fallbacks for M8 wavetable IDs.
//!
//! This map keeps each table selection audible using only the closest
//! generic base waveform.

const WAVETABLE_FALLBACKS: [u8; 61] = [
    4, 6, 7, 4, 0, 4, 4, 4, 6, 6, 7, 4, 0, 4, 4, 4, 6, 6, 4, 6, 6, 6, 6, 6, 6, 4, 6, 4, 4, 6, 4, 4,
    6, 6, 6, 0, 4, 4, 4, 4, 4, 4, 4, 4, 0, 5, 6, 4, 6, 4, 4, 4, 4, 0, 4, 4, 4, 6, 7, 6, 4,
];

/// Return the generic WAVSYNTH base-shape ID for a WAVSYNTH table ID 09..45.
pub(crate) fn wavetable_fallback(shape_id: u8) -> Option<u8> {
    shape_id
        .checked_sub(0x09)
        .and_then(|index| WAVETABLE_FALLBACKS.get(usize::from(index)).copied())
}

#[cfg(test)]
mod tests {
    use super::{WAVETABLE_FALLBACKS, wavetable_fallback};

    #[test]
    fn every_wavetable_has_a_generic_base_shape_mapping() {
        assert_eq!(WAVETABLE_FALLBACKS.len(), 0x45 - 0x09 + 1);
        assert!(
            WAVETABLE_FALLBACKS
                .iter()
                .all(|shape| matches!(shape, 0 | 4 | 5 | 6 | 7))
        );
        for shape_id in 0x09..=0x45 {
            assert!(
                wavetable_fallback(shape_id).is_some(),
                "shape {shape_id:02x}"
            );
        }
        assert_eq!(wavetable_fallback(0x08), None);
        assert_eq!(wavetable_fallback(0x46), None);
    }
}
