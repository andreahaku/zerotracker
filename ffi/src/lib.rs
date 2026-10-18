//! C ABI for the C++ app.
//!
//! The app links one static library. Two Rust static libraries in the same binary
//! would duplicate the Rust standard library, so `m8fmt` and `engine` are bundled
//! here. `engine` exports its own `extern "C"` functions, declared in
//! `engine/include/zt_engine.h`; this crate only re-links them.

pub use engine;

pub mod library;
pub mod preview;
pub mod sample_edit;
pub mod samples;
pub mod viewer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_header_and_rejects_foreign_bytes() {
        let mut header = b"M8VERSION\0".to_vec();
        header.extend_from_slice(&[0x62, 0x06, 0x00, 0x10]); // 6.6.2
        let v = read_version(&header).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (6, 6, 2));
        assert!(read_version(b"M8VERSION").is_none());
        assert!(read_version(&[0u8; 32]).is_none());
    }
}

/// Firmware version read from an M8 file header, as returned to C.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ZtFileVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

/// Reads the firmware version from the first `len` bytes at `data`.
///
/// Returns 1 and fills `out` on success, 0 when the header is not an M8 header.
///
/// # Safety
///
/// `data` must point to `len` readable bytes and `out` to a writable `ZtFileVersion`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_fmt_read_version(
    data: *const u8,
    len: usize,
    out: *mut ZtFileVersion,
) -> i32 {
    if data.is_null() || out.is_null() {
        return 0;
    }
    // SAFETY: the caller guarantees `len` readable bytes at `data`.
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match read_version(bytes) {
        Some(v) => {
            // SAFETY: the caller guarantees `out` is writable.
            unsafe {
                *out = ZtFileVersion {
                    major: v.major,
                    minor: v.minor,
                    patch: v.patch,
                }
            };
            1
        }
        None => 0,
    }
}

/// Reads the version header, or `None` when the bytes are not an M8 file.
fn read_version(bytes: &[u8]) -> Option<m8fmt::Version> {
    if bytes.len() < m8fmt::Version::SIZE || !bytes.starts_with(b"M8VERSION") {
        return None;
    }
    let mut reader = m8fmt::reader::Reader::new(bytes[..m8fmt::Version::SIZE].to_vec());
    m8fmt::Version::from_reader(&mut reader).ok()
}

/// Version string of the core library, NUL-terminated, static lifetime.
#[unsafe(no_mangle)]
pub extern "C" fn zt_core_version() -> *const std::ffi::c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}
