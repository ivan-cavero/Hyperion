//! Gzip helpers for storage NBT files (`level.dat`).

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::error::WorldError;

/// Hard cap on uncompressed size when reading gzip payloads (anti zip-bomb).
///
/// A normal `level.dat` is a few KiB; 16 MiB is far above any legitimate
/// Hyperion/vanilla metadata while still cheap to reject.
pub const MAX_GZIP_UNCOMPRESSED: u64 = 16 * 1024 * 1024;

/// Gzip-compresses `bytes` (RFC 1952). Used for `level.dat` on disk.
pub fn gzip_compress(bytes: &[u8]) -> Result<Vec<u8>, WorldError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .map_err(|error| WorldError::Gzip(error.to_string()))?;
    encoder
        .finish()
        .map_err(|error| WorldError::Gzip(error.to_string()))
}

/// Gzip-decompresses `bytes` (RFC 1952). Inverse of [`gzip_compress`].
///
/// Refuses to expand more than [`MAX_GZIP_UNCOMPRESSED`] bytes so a hostile
/// archive cannot allocate unbounded memory.
pub fn gzip_decompress(bytes: &[u8]) -> Result<Vec<u8>, WorldError> {
    let mut decoder = GzDecoder::new(bytes);
    let mut out = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let n = decoder
            .read(&mut buffer)
            .map_err(|error| WorldError::Gzip(error.to_string()))?;
        if n == 0 {
            break;
        }
        let new_len = out.len() as u64 + n as u64;
        if new_len > MAX_GZIP_UNCOMPRESSED {
            return Err(WorldError::Gzip(format!(
                "uncompressed payload exceeds {MAX_GZIP_UNCOMPRESSED} bytes"
            )));
        }
        out.extend_from_slice(&buffer[..n]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_round_trip() {
        let original = b"hello level.dat payload";
        let compressed = gzip_compress(original).expect("compress");
        assert_ne!(compressed.as_slice(), original.as_slice());
        let decompressed = gzip_decompress(&compressed).expect("decompress");
        assert_eq!(decompressed, original);
    }

    #[test]
    fn gzip_decompress_rejects_oversize_payload() {
        // Build a small gzip that expands past the cap by streaming zeros.
        // We compress a buffer larger than MAX so decompress must fail.
        let huge = vec![0_u8; (MAX_GZIP_UNCOMPRESSED as usize) + 1];
        let compressed = gzip_compress(&huge).expect("compress");
        let err = gzip_decompress(&compressed).expect_err("must reject oversize");
        assert!(
            matches!(err, WorldError::Gzip(_)),
            "expected Gzip error, got {err:?}"
        );
    }
}
