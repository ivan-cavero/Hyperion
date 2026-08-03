//! Compresión zlib de paquetes (RFC 1950).
//!
//! Con compresión activa, el cuerpo de cada trama es:
//!
//! ```text
//! Data Length (VarInt) + datos
//! ```
//!
//! donde `Data Length` es `0` si el paquete no se comprimió, o la longitud del
//! paquete sin comprimir si sí; en ese caso los bytes zlib llenan el resto de
//! la trama. No hay un campo separado con la longitud comprimida.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::ProtocolError;
use crate::frame::{decode_var_i32_at, encode_var_i32};

/// Longitud máxima de un paquete sin comprimir en el wire (2^23).
pub const MAX_UNCOMPRESSED_LENGTH: usize = 8_388_608;

const MAX_VARINT_BYTES: usize = 5;

/// Compresses the packet body (ID + payload) according to the threshold.
///
/// If the body is smaller than `threshold` it is emitted uncompressed with
/// `Data Length = 0`; otherwise `Data Length` is emitted with the
/// uncompressed length followed by the zlib bytes.
pub fn compress_body(packet_body: &[u8], threshold: usize) -> Result<Vec<u8>, ProtocolError> {
    if packet_body.len() >= threshold {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(packet_body)
            .map_err(|error| ProtocolError::Compression(error.to_string()))?;
        let compressed = encoder
            .finish()
            .map_err(|error| ProtocolError::Compression(error.to_string()))?;

        let mut body = encode_var_i32(packet_body.len() as i32);
        body.extend_from_slice(&compressed);
        Ok(body)
    } else {
        let mut body = encode_var_i32(0);
        body.extend_from_slice(packet_body);
        Ok(body)
    }
}

/// Descomprime el cuerpo de una trama, devolviendo el paquete (ID + payload).
pub fn decompress_body(input: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let (data_length, data_offset) = decode_var_i32_at(input, 0, 0, 0, 0, MAX_VARINT_BYTES)?;
    if data_length < 0 {
        return Err(ProtocolError::Compression(
            "negative data length".to_owned(),
        ));
    }

    let data = &input[data_offset..];
    if data_length == 0 {
        return Ok(data.to_vec());
    }

    let uncompressed_length = data_length as usize;
    if uncompressed_length > MAX_UNCOMPRESSED_LENGTH {
        return Err(ProtocolError::Compression(
            "uncompressed length exceeds the protocol maximum".to_owned(),
        ));
    }

    let decoder = ZlibDecoder::new(data);
    // Bound the read: a malicious peer may declare a small length but send a
    // stream that inflates far beyond it (zip bomb). Reading at most
    // `uncompressed_length + 1` bytes caps the allocation, and the
    // size-mismatch check below rejects the packet.
    let mut output = Vec::with_capacity(uncompressed_length);
    decoder
        .take(uncompressed_length as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|error| ProtocolError::Compression(error.to_string()))?;

    if output.len() != uncompressed_length {
        return Err(ProtocolError::Compression(
            "decompressed size does not match the declared length".to_owned(),
        ));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_var_i32(input: &[u8]) -> (i32, usize) {
        decode_var_i32_at(input, 0, 0, 0, 0, MAX_VARINT_BYTES).expect("varint should decode")
    }

    #[test]
    fn small_bodies_are_not_compressed() {
        let body = vec![0u8, 1, 2, 3];
        let encoded = compress_body(&body, 256).expect("should encode");
        let (data_length, data_offset) = decode_var_i32(&encoded);

        assert_eq!(data_length, 0);
        assert_eq!(&encoded[data_offset..], body.as_slice());
    }

    #[test]
    fn large_bodies_round_trip_through_zlib() {
        let body = vec![7u8; 10_000];
        let encoded = compress_body(&body, 256).expect("should encode");
        let (data_length, _) = decode_var_i32(&encoded);

        assert_eq!(data_length, body.len() as i32);
        assert_ne!(encoded.len(), body.len() + 1);

        let decoded = decompress_body(&encoded).expect("should decode");
        assert_eq!(decoded, body);
    }

    #[test]
    fn boundary_size_is_compressed() {
        let body = vec![3u8; 256];
        let encoded = compress_body(&body, 256).expect("should encode");
        let (data_length, _) = decode_var_i32(&encoded);

        assert_eq!(data_length, body.len() as i32);
        assert_eq!(decompress_body(&encoded).expect("should decode"), body);
    }

    #[test]
    fn negative_declared_length_is_rejected() {
        let encoded = encode_var_i32(-1);
        assert!(decompress_body(&encoded).is_err());
    }

    #[test]
    fn oversized_declared_length_is_rejected() {
        let encoded = encode_var_i32(MAX_UNCOMPRESSED_LENGTH as i32 + 1);
        assert!(decompress_body(&encoded).is_err());
    }

    #[test]
    fn zip_bomb_is_rejected() {
        // A stream that inflates far beyond its declared length (10 KB of
        // zeros compressed to a few hundred bytes, declared as 100) must
        // fail. The bounded read caps the allocation at declared + 1 bytes.
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&[0u8; 10_000])
            .expect("encoder should write");
        let zlib = encoder.finish().expect("encoder should finish");

        let mut bomb = encode_var_i32(100); // declared: 100 bytes
        bomb.extend_from_slice(&zlib);
        assert!(decompress_body(&bomb).is_err());
    }

    #[test]
    fn declared_length_larger_than_actual_is_rejected() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&[1u8; 1_000])
            .expect("encoder should write");
        let zlib = encoder.finish().expect("encoder should finish");

        let mut body = encode_var_i32(20_000); // declared: 20_000 bytes
        body.extend_from_slice(&zlib);
        assert!(decompress_body(&body).is_err());
    }
}
