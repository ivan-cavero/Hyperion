use hyperion_protocol::{decode_frame, encode_frame, ProtocolError};

#[test]
fn frame_round_trip_preserves_packet_and_payload() {
    let encoded_frame = encode_frame(0x42, &[1, 2, 3]).expect("frame should encode");
    let (decoded_frame, consumed_bytes) =
        decode_frame(&encoded_frame).expect("frame should decode");

    assert_eq!(consumed_bytes, encoded_frame.len());
    assert_eq!(decoded_frame.packet_id, 0x42);
    assert_eq!(decoded_frame.payload, vec![1, 2, 3]);
}

#[test]
fn frame_decoder_returns_consumed_bytes_for_concatenated_frames() {
    let first_frame = encode_frame(0, &[]).expect("frame should encode");
    let second_frame = encode_frame(1, &[9]).expect("frame should encode");
    let input = [first_frame.clone(), second_frame.clone()].concat();

    let (decoded_frame, consumed_bytes) = decode_frame(&input).expect("first frame should decode");

    assert_eq!(decoded_frame.packet_id, 0);
    assert_eq!(consumed_bytes, first_frame.len());
    assert_eq!(&input[consumed_bytes..], second_frame.as_slice());
}

#[test]
fn frame_decoder_rejects_truncated_payload() {
    let error = decode_frame(&[3, 0, 1]).expect_err("truncated frame should fail");

    assert_eq!(error, ProtocolError::UnexpectedEndOfInput);
}

#[test]
fn frame_decoder_rejects_four_byte_length_prefix() {
    let error = decode_frame(&[0x80, 0x80, 0x80, 0x01]).expect_err("length should fail");

    assert_eq!(error, ProtocolError::VarIntTooLong);
}
