use hyperion_protocol::{
    HandshakeIntent, ProtocolError, SUPPORTED_PROTOCOL_VERSION, decode_frame, decode_handshake,
    encode_frame,
};

fn encode_var_i32(value: i32) -> Vec<u8> {
    let mut encoded_bytes = Vec::new();
    let mut remaining_value = value as u32;

    loop {
        let mut current_byte = (remaining_value as u8) & 0x7f;
        remaining_value >>= 7;
        if remaining_value != 0 {
            current_byte |= 0x80;
        }
        encoded_bytes = [encoded_bytes, vec![current_byte]].concat();
        if remaining_value == 0 {
            return encoded_bytes;
        }
    }
}

fn encode_string(value: &str) -> Vec<u8> {
    [
        encode_var_i32(value.len() as i32),
        value.as_bytes().to_vec(),
    ]
    .concat()
}

fn decode_handshake_bytes(payload: Vec<u8>) -> hyperion_protocol::HandshakePacket {
    let frame = encode_frame(0, &payload).expect("Handshake frame should encode");
    let (frame, _) = decode_frame(&frame).expect("Handshake frame should decode");

    decode_handshake(&frame).expect("Handshake should decode")
}

#[test]
fn decodes_status_intent() {
    let handshake = decode_handshake_bytes(
        [
            encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
            encode_string("localhost"),
            255u16.to_be_bytes().to_vec(),
            encode_var_i32(1),
        ]
        .concat(),
    );

    assert_eq!(handshake.protocol_version, SUPPORTED_PROTOCOL_VERSION);
    assert_eq!(handshake.server_address, "localhost");
    assert_eq!(handshake.server_port, 255);
    assert_eq!(handshake.intent, HandshakeIntent::Status);
}

#[test]
fn decodes_transfer_intent() {
    let handshake = decode_handshake_bytes(
        [
            encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
            encode_string("example.org"),
            25565u16.to_be_bytes().to_vec(),
            encode_var_i32(3),
        ]
        .concat(),
    );

    assert_eq!(handshake.intent, HandshakeIntent::Transfer);
}

#[test]
fn rejects_address_over_limit() {
    let oversized_address = "a".repeat(256);
    let payload = [
        encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
        encode_string(&oversized_address),
    ]
    .concat();
    let frame = encode_frame(0, &payload).expect("frame should encode");
    let (frame, _) = decode_frame(&frame).expect("frame should decode");

    assert_eq!(decode_handshake(&frame), Err(ProtocolError::StringTooLong));
}

#[test]
fn rejects_trailing_fields() {
    let payload = [
        encode_var_i32(SUPPORTED_PROTOCOL_VERSION),
        encode_string("localhost"),
        25565u16.to_be_bytes().to_vec(),
        encode_var_i32(1),
        vec![0],
    ]
    .concat();
    let frame = encode_frame(0, &payload).expect("frame should encode");
    let (frame, _) = decode_frame(&frame).expect("frame should decode");

    assert_eq!(
        decode_handshake(&frame),
        Err(ProtocolError::InvalidPacketPayload)
    );
}
