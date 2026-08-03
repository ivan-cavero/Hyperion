//! Login-state packets (protocol 776 / 26.2).
//!
//! Covers Login Start (serverbound), Encryption Request/Response, Set
//! Compression, Login Success (including the Session ID added in 26.2), Login
//! Acknowledged, and Disconnect. Encrypted wire flows live in `crypto`.

use uuid::Uuid;

use crate::ProtocolError;
use crate::frame::{
    PacketCursor, PacketFrame, encode_boolean, encode_bytes, encode_frame, encode_string,
    encode_uuid, encode_var_i32,
};

pub const LOGIN_START_PACKET_ID: i32 = 0;
pub const ENCRYPTION_REQUEST_PACKET_ID: i32 = 1;
pub const ENCRYPTION_RESPONSE_PACKET_ID: i32 = 1;
pub const LOGIN_SUCCESS_PACKET_ID: i32 = 2;
pub const SET_COMPRESSION_PACKET_ID: i32 = 3;
pub const LOGIN_ACKNOWLEDGED_PACKET_ID: i32 = 3;
pub const LOGIN_DISCONNECT_PACKET_ID: i32 = 0;

/// Game Profile and username limits.
pub const MAX_USERNAME_UTF16_UNITS: usize = 16;
pub const MAX_PROPERTIES: usize = 16;
pub const MAX_PROPERTY_NAME_UTF16_UNITS: usize = 64;
pub const MAX_PROPERTY_VALUE_UTF16_UNITS: usize = 32_767;
pub const MAX_PROPERTY_SIGNATURE_UTF16_UNITS: usize = 1024;
pub const MAX_SERVER_ID_UTF16_UNITS: usize = 20;
pub const MAX_REASON_UTF16_UNITS: usize = 32_767;

/// Limits for encrypted byte arrays (RSA-1024 yields 128 bytes; headroom is
/// kept without exceeding the protocol cap).
pub const MAX_PUBLIC_KEY_LENGTH: usize = 512;
pub const MAX_VERIFY_TOKEN_LENGTH: usize = 128;
pub const MAX_SHARED_SECRET_LENGTH: usize = 512;

/// Serverbound Login Start packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    /// Username declared by the client.
    pub username: String,
    /// UUID declared by the client (ignored by the vanilla server).
    pub uuid: Uuid,
}

/// Clientbound Encryption Request packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionRequest {
    /// Server ID (empty string on modern servers).
    pub server_id: String,
    /// Server public key as DER SubjectPublicKeyInfo.
    pub public_key: Vec<u8>,
    /// Random token the client must return encrypted.
    pub verify_token: Vec<u8>,
    /// Whether the client must authenticate with Mojang's session server.
    pub should_authenticate: bool,
}

/// Serverbound Encryption Response packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionResponse {
    /// 16-byte shared secret encrypted with RSA.
    pub shared_secret: Vec<u8>,
    /// Verify token encrypted with RSA.
    pub verify_token: Vec<u8>,
}

/// Clientbound Set Compression packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCompression {
    /// Threshold above which packets are compressed.
    pub threshold: i32,
}

/// A game-profile property (e.g. the skin texture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfileProperty {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

/// Player profile sent in Login Success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    pub uuid: Uuid,
    pub username: String,
    pub properties: Vec<GameProfileProperty>,
}

/// Clientbound Login Success packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    pub profile: GameProfile,
    /// Session ID added in protocol 776 (26.2).
    pub session_id: Uuid,
}

/// Decodes a Login Start packet.
pub fn decode_login_start(frame: &PacketFrame) -> Result<LoginStart, ProtocolError> {
    if frame.packet_id != LOGIN_START_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }

    let mut cursor = PacketCursor::new(&frame.payload);
    let username = cursor.read_string(MAX_USERNAME_UTF16_UNITS)?;
    if !is_valid_username(&username) {
        return Err(ProtocolError::InvalidUsername);
    }
    let uuid = cursor.read_uuid()?;
    cursor.finish()?;

    Ok(LoginStart { username, uuid })
}

/// Checks a username against the vanilla rules: only `[a-zA-Z0-9_]`, 1 to 16
/// characters (length is enforced by `MAX_USERNAME_UTF16_UNITS` above).
fn is_valid_username(username: &str) -> bool {
    !username.is_empty()
        && username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Decodes an Encryption Response packet.
pub fn decode_encryption_response(
    frame: &PacketFrame,
) -> Result<EncryptionResponse, ProtocolError> {
    if frame.packet_id != ENCRYPTION_RESPONSE_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }

    let mut cursor = PacketCursor::new(&frame.payload);
    let shared_secret = cursor.read_byte_array(MAX_SHARED_SECRET_LENGTH)?;
    let verify_token = cursor.read_byte_array(MAX_VERIFY_TOKEN_LENGTH)?;
    cursor.finish()?;

    Ok(EncryptionResponse {
        shared_secret,
        verify_token,
    })
}

/// Validates a Login Acknowledged packet (must be empty).
pub fn decode_login_acknowledged(frame: &PacketFrame) -> Result<(), ProtocolError> {
    if frame.packet_id != LOGIN_ACKNOWLEDGED_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }
    if !frame.payload.is_empty() {
        return Err(ProtocolError::InvalidPacketPayload);
    }

    Ok(())
}

/// Encodes an Encryption Request payload (without the frame prefix).
pub fn encode_encryption_request_payload(
    request: &EncryptionRequest,
) -> Result<Vec<u8>, ProtocolError> {
    let payload = [
        encode_string(&request.server_id, MAX_SERVER_ID_UTF16_UNITS)?,
        encode_bytes(&request.public_key, MAX_PUBLIC_KEY_LENGTH)?,
        encode_bytes(&request.verify_token, MAX_VERIFY_TOKEN_LENGTH)?,
        encode_boolean(request.should_authenticate),
    ]
    .concat();

    Ok(payload)
}

/// Encodes a complete Encryption Request frame.
pub fn encode_encryption_request(request: &EncryptionRequest) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(
        ENCRYPTION_REQUEST_PACKET_ID,
        &encode_encryption_request_payload(request)?,
    )
}

/// Encodes a complete Set Compression frame.
pub fn encode_set_compression(threshold: i32) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(SET_COMPRESSION_PACKET_ID, &encode_var_i32(threshold))
}

/// Encodes a Login Success payload (profile + session ID).
pub fn encode_login_success_payload(success: &LoginSuccess) -> Result<Vec<u8>, ProtocolError> {
    if success.profile.properties.len() > MAX_PROPERTIES {
        return Err(ProtocolError::InvalidPacketPayload);
    }

    let mut payload = encode_uuid(success.profile.uuid);
    payload.extend_from_slice(&encode_string(
        &success.profile.username,
        MAX_USERNAME_UTF16_UNITS,
    )?);

    let property_count = i32::try_from(success.profile.properties.len())
        .map_err(|_| ProtocolError::InvalidPacketPayload)?;
    payload.extend_from_slice(&encode_var_i32(property_count));

    for property in &success.profile.properties {
        payload.extend_from_slice(&encode_string(
            &property.name,
            MAX_PROPERTY_NAME_UTF16_UNITS,
        )?);
        payload.extend_from_slice(&encode_string(
            &property.value,
            MAX_PROPERTY_VALUE_UTF16_UNITS,
        )?);
        match &property.signature {
            Some(signature) => {
                payload.extend_from_slice(&encode_boolean(true));
                payload.extend_from_slice(&encode_string(
                    signature,
                    MAX_PROPERTY_SIGNATURE_UTF16_UNITS,
                )?);
            }
            None => payload.extend_from_slice(&encode_boolean(false)),
        }
    }

    payload.extend_from_slice(&encode_uuid(success.session_id));

    Ok(payload)
}

/// Encodes a complete Login Success frame.
pub fn encode_login_success(success: &LoginSuccess) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(
        LOGIN_SUCCESS_PACKET_ID,
        &encode_login_success_payload(success)?,
    )
}

/// Encodes a login Disconnect payload (reason as a JSON text component).
pub fn encode_login_disconnect_payload(reason: &str) -> Result<Vec<u8>, ProtocolError> {
    let reason_json = serde_json::json!({ "text": reason }).to_string();
    encode_string(&reason_json, MAX_REASON_UTF16_UNITS)
}

/// Encodes a complete login Disconnect frame with a text-component reason.
pub fn encode_login_disconnect(reason: &str) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(
        LOGIN_DISCONNECT_PACKET_ID,
        &encode_login_disconnect_payload(reason)?,
    )
}

/// The UUID vanilla assigns to offline-mode players: the MD5 digest of the
/// UTF-8 bytes of `"OfflinePlayer:" + name` with the version-3 and IETF
/// variant bits set, i.e. Java's `UUID.nameUUIDFromBytes`.
pub fn offline_mode_uuid(username: &str) -> Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{username}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(digest.as_ref());
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // IETF variant
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn decode_frame_payload(frame: &[u8]) -> (PacketFrame, usize) {
        crate::frame::decode_frame(frame).expect("frame should decode")
    }

    /// Builds a Login Start frame as a client would send it, without
    /// validating the name (the decoder is responsible for that).
    fn build_login_start_frame(username: &str, uuid: Uuid) -> Vec<u8> {
        let payload = [
            encode_var_i32(username.len() as i32),
            username.as_bytes().to_vec(),
            encode_uuid(uuid),
        ]
        .concat();
        encode_frame(LOGIN_START_PACKET_ID, &payload).expect("login start frame should encode")
    }

    #[test]
    fn login_start_round_trips() {
        let uuid = Uuid::from_u128(0x0123456789abcdef_0123456789abcdef);
        let frame_bytes = build_login_start_frame("Hyperion", uuid);
        let (frame, _) = decode_frame_payload(&frame_bytes);
        let login_start = decode_login_start(&frame).expect("login start should decode");

        assert_eq!(login_start.username, "Hyperion");
        assert_eq!(login_start.uuid, uuid);
    }

    #[test]
    fn login_start_rejects_long_username() {
        let frame_bytes =
            build_login_start_frame("a very long username that exceeds the limit", Uuid::nil());
        let (frame, _) = decode_frame_payload(&frame_bytes);

        assert_eq!(
            decode_login_start(&frame),
            Err(ProtocolError::StringTooLong)
        );
    }

    #[test]
    fn login_start_rejects_invalid_username() {
        let frame_bytes = build_login_start_frame("Bad-Name!", Uuid::nil());
        let (frame, _) = decode_frame_payload(&frame_bytes);

        assert_eq!(
            decode_login_start(&frame),
            Err(ProtocolError::InvalidUsername)
        );
    }

    #[test]
    fn login_start_rejects_wrong_packet_id() {
        let frame = PacketFrame {
            packet_id: LOGIN_ACKNOWLEDGED_PACKET_ID,
            payload: Bytes::new(),
        };

        assert_eq!(
            decode_login_start(&frame),
            Err(ProtocolError::InvalidPacketId)
        );
    }

    #[test]
    fn encryption_response_decodes_shared_secret_and_token() {
        let payload = [
            encode_bytes(&[0u8; 128], MAX_SHARED_SECRET_LENGTH).expect("secret should encode"),
            encode_bytes(&[7u8; 4], MAX_VERIFY_TOKEN_LENGTH).expect("token should encode"),
        ]
        .concat();
        let frame = PacketFrame {
            packet_id: ENCRYPTION_RESPONSE_PACKET_ID,
            payload: Bytes::from(payload),
        };

        let response = decode_encryption_response(&frame).expect("response should decode");
        assert_eq!(response.shared_secret, vec![0u8; 128]);
        assert_eq!(response.verify_token, vec![7u8; 4]);
    }

    #[test]
    fn encryption_request_encodes_fields_in_order() {
        let request = EncryptionRequest {
            server_id: String::new(),
            public_key: vec![0x30, 0x82, 0x01, 0x22],
            verify_token: vec![1, 2, 3, 4],
            should_authenticate: true,
        };
        let frame_bytes = encode_encryption_request(&request).expect("request should encode");
        let (frame, _) = decode_frame_payload(&frame_bytes);

        assert_eq!(frame.packet_id, ENCRYPTION_REQUEST_PACKET_ID);
        let expected_payload = [
            encode_string("", MAX_SERVER_ID_UTF16_UNITS).expect("server id should encode"),
            encode_bytes(&[0x30, 0x82, 0x01, 0x22], MAX_PUBLIC_KEY_LENGTH)
                .expect("public key should encode"),
            encode_bytes(&[1, 2, 3, 4], MAX_VERIFY_TOKEN_LENGTH)
                .expect("verify token should encode"),
            encode_boolean(true),
        ]
        .concat();
        assert_eq!(frame.payload, expected_payload);
    }

    #[test]
    fn set_compression_encodes_threshold() {
        let frame_bytes = encode_set_compression(256).expect("should encode");
        let (frame, _) = decode_frame_payload(&frame_bytes);

        assert_eq!(frame.packet_id, SET_COMPRESSION_PACKET_ID);
        assert_eq!(frame.payload, encode_var_i32(256));
    }

    #[test]
    fn login_success_encodes_profile_and_session_id() {
        let success = LoginSuccess {
            profile: GameProfile {
                uuid: Uuid::from_u128(0xabcdef0123456789_abcdef0123456789),
                username: "Hyperion".to_owned(),
                properties: vec![
                    GameProfileProperty {
                        name: "textures".to_owned(),
                        value: "base64".to_owned(),
                        signature: Some("sig".to_owned()),
                    },
                    GameProfileProperty {
                        name: "empty".to_owned(),
                        value: "no signature".to_owned(),
                        signature: None,
                    },
                ],
            },
            session_id: Uuid::nil(),
        };
        let frame_bytes = encode_login_success(&success).expect("success should encode");
        let (frame, _) = decode_frame_payload(&frame_bytes);

        assert_eq!(frame.packet_id, LOGIN_SUCCESS_PACKET_ID);
        let expected_payload = [
            encode_uuid(success.profile.uuid),
            encode_string("Hyperion", MAX_USERNAME_UTF16_UNITS).expect("username should encode"),
            encode_var_i32(2),
            encode_string("textures", MAX_PROPERTY_NAME_UTF16_UNITS).expect("name should encode"),
            encode_string("base64", MAX_PROPERTY_VALUE_UTF16_UNITS).expect("value should encode"),
            encode_boolean(true),
            encode_string("sig", MAX_PROPERTY_SIGNATURE_UTF16_UNITS)
                .expect("signature should encode"),
            encode_string("empty", MAX_PROPERTY_NAME_UTF16_UNITS).expect("name should encode"),
            encode_string("no signature", MAX_PROPERTY_VALUE_UTF16_UNITS)
                .expect("value should encode"),
            encode_boolean(false),
            encode_uuid(Uuid::nil()),
        ]
        .concat();
        assert_eq!(frame.payload, expected_payload);
    }

    #[test]
    fn login_success_rejects_too_many_properties() {
        let success = LoginSuccess {
            profile: GameProfile {
                uuid: Uuid::nil(),
                username: "Hyperion".to_owned(),
                properties: (0..=MAX_PROPERTIES)
                    .map(|index| GameProfileProperty {
                        name: format!("prop{index}"),
                        value: "value".to_owned(),
                        signature: None,
                    })
                    .collect(),
            },
            session_id: Uuid::nil(),
        };

        assert_eq!(
            encode_login_success(&success),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn login_acknowledged_requires_empty_payload() {
        let valid = PacketFrame {
            packet_id: LOGIN_ACKNOWLEDGED_PACKET_ID,
            payload: Bytes::new(),
        };
        let invalid = PacketFrame {
            packet_id: LOGIN_ACKNOWLEDGED_PACKET_ID,
            payload: Bytes::from(vec![0]),
        };

        assert_eq!(decode_login_acknowledged(&valid), Ok(()));
        assert_eq!(
            decode_login_acknowledged(&invalid),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn login_disconnect_encodes_json_reason() {
        let frame_bytes = encode_login_disconnect("Too many players").expect("should encode");
        let (frame, _) = decode_frame_payload(&frame_bytes);

        assert_eq!(frame.packet_id, LOGIN_DISCONNECT_PACKET_ID);
        let mut cursor = PacketCursor::new(&frame.payload);
        let reason = cursor
            .read_string(MAX_REASON_UTF16_UNITS)
            .expect("reason should decode");
        assert_eq!(reason, r#"{"text":"Too many players"}"#);
    }
}
