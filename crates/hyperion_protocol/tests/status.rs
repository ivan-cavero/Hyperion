use hyperion_protocol::{
    ProtocolError, SUPPORTED_PROTOCOL_VERSION, StatusDescription, StatusPlayers, StatusResponse,
    StatusVersion, decode_frame, decode_ping_request, decode_status_request, encode_pong_response,
    encode_status_response,
};

#[test]
fn status_request_requires_empty_payload() {
    let valid_frame = decode_frame(&[1, 0])
        .expect("Status Request should decode")
        .0;
    let invalid_frame = decode_frame(&[2, 0, 1])
        .expect("invalid Status frame should decode")
        .0;

    assert_eq!(decode_status_request(&valid_frame), Ok(()));
    assert_eq!(
        decode_status_request(&invalid_frame),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn ping_and_pong_use_big_endian_i64_payloads() {
    let ping_frame = decode_frame(&[9, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x85])
        .expect("Ping frame should decode")
        .0;
    let ping = decode_ping_request(&ping_frame).expect("Ping should decode");
    let pong_frame = decode_frame(&encode_pong_response(ping.payload).expect("Pong should encode"))
        .expect("Pong frame should decode")
        .0;

    assert_eq!(ping.payload, -123);
    assert_eq!(pong_frame.packet_id, 1);
    assert_eq!(pong_frame.payload, (-123_i64).to_be_bytes());
}

#[test]
fn status_response_escapes_json_text() {
    let response = StatusResponse {
        version: StatusVersion {
            name: "26.2".to_owned(),
            protocol: SUPPORTED_PROTOCOL_VERSION,
        },
        players: StatusPlayers {
            max: 20,
            online: 0,
            sample: Vec::new(),
        },
        description: StatusDescription {
            text: "Hello \"Hyperion\"".to_owned(),
        },
        favicon: None,
        enforces_secure_chat: false,
    };
    let encoded_response = encode_status_response(&response).expect("Status should encode");
    let (status_frame, _) = decode_frame(&encoded_response).expect("Status frame should decode");
    let string_length_byte_count = status_frame
        .payload
        .iter()
        .position(|byte| byte & 0x80 == 0)
        .expect("JSON string length should terminate");
    let response_json: serde_json::Value =
        serde_json::from_slice(&status_frame.payload[string_length_byte_count + 1..])
            .expect("response JSON should be valid");

    assert_eq!(response_json["description"]["text"], "Hello \"Hyperion\"");
    assert_eq!(response_json["enforcesSecureChat"], false);
    assert!(response_json.get("favicon").is_none());
}

fn decode_status_response_json(response: &StatusResponse) -> serde_json::Value {
    let encoded_response = encode_status_response(response).expect("Status should encode");
    let (status_frame, _) = decode_frame(&encoded_response).expect("Status frame should decode");
    let string_length_byte_count = status_frame
        .payload
        .iter()
        .position(|byte| byte & 0x80 == 0)
        .expect("JSON string length should terminate");

    serde_json::from_slice(&status_frame.payload[string_length_byte_count + 1..])
        .expect("response JSON should be valid")
}

#[test]
fn status_response_serializes_optional_fields() {
    let response = StatusResponse {
        version: StatusVersion {
            name: "26.2".to_owned(),
            protocol: SUPPORTED_PROTOCOL_VERSION,
        },
        players: StatusPlayers {
            max: 100,
            online: 3,
            sample: Vec::new(),
        },
        description: StatusDescription {
            text: "Hyperion".to_owned(),
        },
        favicon: Some("data:image/png;base64,AAECAwQ=".to_owned()),
        enforces_secure_chat: true,
    };
    let response_json = decode_status_response_json(&response);

    assert_eq!(response_json["favicon"], "data:image/png;base64,AAECAwQ=");
    assert_eq!(response_json["enforcesSecureChat"], true);
}
