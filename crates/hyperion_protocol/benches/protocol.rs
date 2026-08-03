//! Hot-path benchmarks for the protocol primitives.
//!
//! Run with:
//!   cargo bench -p hyperion_protocol --bench protocol
//!
//! These measure the primitives behind `Connection::read_frame` /
//! `Connection::write_frame` in `hyperion_server`, so the numbers here are
//! the baseline the server hot path is built on.

use divan::black_box;
use hyperion_protocol::{
    Cfb8Stream, SHARED_SECRET_LENGTH, compress_body, decode_packet_data, decode_var_i32,
    decompress_body, encode_frame, encode_var_i32, generate_rsa_keypair, server_id_hash,
    split_frame,
};

fn main() {
    divan::main();
}

/// A typical small packet body: Login Start sized (username + client UUID).
const TINY_BODY: &[u8] = &[0x00, 0x06, b'B', b'e', b'n', b'c', b'h'];
/// A status-response-sized body (a server-list JSON).
const MEDIUM_BODY: &[u8] = &[0x00, 0x01, 0x7b, 0x22, 0x76, 0x65, 0x72];

/// Encodes a VarInt, the most common wire operation.
#[divan::bench]
fn encode_varint_common(bencher: divan::Bencher) {
    bencher.bench(|| black_box(encode_var_i32(black_box(2))));
}

/// Encodes a 5-byte VarInt (the worst case, e.g. -1).
#[divan::bench]
fn encode_varint_five_bytes(bencher: divan::Bencher) {
    bencher.bench(|| black_box(encode_var_i32(black_box(-1))));
}

/// Decodes a typical VarInt (packet ID 2, three bytes).
#[divan::bench]
fn decode_varint_common(bencher: divan::Bencher) {
    let encoded = encode_var_i32(2);
    bencher.bench(|| {
        black_box(decode_var_i32(black_box(&encoded), 0, 5).expect("VarInt should decode"))
    });
}

/// Encodes a complete frame with a tiny payload.
#[divan::bench]
fn encode_frame_tiny(bencher: divan::Bencher) {
    bencher.bench(|| black_box(encode_frame(black_box(0), black_box(TINY_BODY)).expect("frame")));
}

/// Splits a complete tiny frame from a buffer (the read side of the
/// uncompressed hot path).
#[divan::bench]
fn split_frame_tiny(bencher: divan::Bencher) {
    let frame = encode_frame(0, TINY_BODY).expect("frame");
    bencher.bench(|| {
        black_box(
            split_frame(black_box(&frame))
                .expect("frame should split")
                .expect("frame complete"),
        )
    });
}

/// The full uncompressed read path: split + decode (no socket I/O).
#[divan::bench]
fn read_direct(bencher: divan::Bencher) {
    let frame = encode_frame(0, MEDIUM_BODY).expect("frame");
    bencher.bench(|| {
        let split = split_frame(black_box(&frame))
            .expect("frame should split")
            .expect("frame complete");
        let body = &frame[split.body_offset..split.total_consumed];
        black_box(
            decode_packet_data(bytes::Bytes::copy_from_slice(body)).expect("packet should decode"),
        )
    });
}

/// The full compressed read path, as the server sees it after login:
/// split a compressed frame, inflate it, decode the packet.
#[divan::bench]
fn read_compressed(bencher: divan::Bencher) {
    let packet_body = encode_frame(0, MEDIUM_BODY).expect("frame");
    let threshold = 16usize; // force zlib so the compressed path is covered
    let frame_body = compress_body(&packet_body, threshold).expect("should compress");
    let mut frame = encode_var_i32(frame_body.len() as i32);
    frame.extend_from_slice(&frame_body);

    bencher.bench(|| {
        let split = split_frame(black_box(&frame))
            .expect("frame should split")
            .expect("frame complete");
        let bytes = &frame[split.body_offset..split.total_consumed];
        let packet = decompress_body(bytes).expect("should inflate");
        black_box(decode_packet_data(bytes::Bytes::from(packet)).expect("packet should decode"))
    });
}

/// Compressing a body above the threshold (zlib).
#[divan::bench]
fn compress_body_large(bencher: divan::Bencher) {
    let body = vec![0x42u8; 4096];
    bencher.bench(|| black_box(compress_body(black_box(&body), black_box(256)).expect("zlib")));
}

/// Decompressing the zlib body from `compress_body_large`.
#[divan::bench]
fn decompress_body_large(bencher: divan::Bencher) {
    let body = vec![0x42u8; 4096];
    let compressed = compress_body(&body, 256).expect("zlib");
    bencher.bench(|| black_box(decompress_body(black_box(&compressed)).expect("inflate")));
}

/// AES-128/CFB8 encryption of a 4 KiB session chunk (outbound traffic).
/// AES-128/CFB8 encryption of a 4 KiB session chunk (outbound traffic).
#[divan::bench]
fn cfb8_encrypt_4kib(bencher: divan::Bencher) {
    let key = [0u8; SHARED_SECRET_LENGTH];
    let chunk = std::sync::Mutex::new(vec![0xabu8; 4096]);
    bencher.bench(|| {
        let mut data = chunk.lock().expect("poisoned bench data");
        let mut cipher = Cfb8Stream::new(black_box(&key));
        cipher.encrypt(black_box(&mut data));
        black_box(data.len())
    });
}

/// RSA-1024 key generation — the expensive per-login operation that must
/// not run inside a tokio worker (see `hyperion_server::network::login`).
#[divan::bench]
fn generate_rsa_keypair_1024(bencher: divan::Bencher) {
    bencher.bench(|| black_box(generate_rsa_keypair().expect("keypair")));
}

/// The Mojang server-hash computation (SHA-1 + signed hex) per online login.
#[divan::bench]
fn server_id_hash_common(bencher: divan::Bencher) {
    let secret = [0u8; SHARED_SECRET_LENGTH];
    let public_key_der = generate_rsa_keypair()
        .map(|(der, _private)| der)
        .expect("keypair");
    bencher.bench(|| {
        black_box(server_id_hash(
            black_box(""),
            black_box(&secret),
            black_box(&public_key_der),
        ))
    });
}
