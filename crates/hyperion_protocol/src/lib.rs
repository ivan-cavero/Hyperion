//! Minecraft protocol primitives used by Hyperion.

mod compression;
mod crypto;
mod error;
mod frame;
mod handshake;
mod login;
mod status;

pub use compression::{compress_body, decompress_body, MAX_UNCOMPRESSED_LENGTH};
pub use crypto::{
    decrypt_pkcs1v15, generate_rsa_keypair, server_id_hash, Cfb8Stream, SHARED_SECRET_LENGTH,
    VERIFY_TOKEN_LENGTH,
};
pub use error::ProtocolError;
pub use frame::{
    decode_frame, decode_packet_data, decode_var_i32, encode_frame, encode_var_i32, split_frame,
    PacketFrame, MAX_PACKET_LENGTH,
};
pub use handshake::{
    decode_handshake, HandshakeIntent, HandshakePacket, MAX_HANDSHAKE_SERVER_ADDRESS_UTF16_UNITS,
};
pub use login::{
    decode_encryption_response, decode_login_acknowledged, decode_login_start,
    encode_encryption_request, encode_encryption_request_payload, encode_login_disconnect,
    encode_login_disconnect_payload, encode_login_success, encode_login_success_payload,
    encode_set_compression, offline_mode_uuid, EncryptionRequest, EncryptionResponse, GameProfile,
    GameProfileProperty, LoginStart, LoginSuccess, SetCompression, ENCRYPTION_REQUEST_PACKET_ID,
    LOGIN_ACKNOWLEDGED_PACKET_ID, LOGIN_DISCONNECT_PACKET_ID, LOGIN_SUCCESS_PACKET_ID,
    MAX_PROPERTIES, MAX_PROPERTY_NAME_UTF16_UNITS, MAX_PROPERTY_SIGNATURE_UTF16_UNITS,
    MAX_PROPERTY_VALUE_UTF16_UNITS, MAX_PUBLIC_KEY_LENGTH, MAX_SHARED_SECRET_LENGTH,
    MAX_USERNAME_UTF16_UNITS, MAX_VERIFY_TOKEN_LENGTH, SET_COMPRESSION_PACKET_ID,
};
pub use status::{
    decode_ping_request, decode_status_request, encode_pong_response, encode_status_response,
    encode_status_response_payload, PingRequest, StatusDescription, StatusPlayer, StatusPlayers,
    StatusResponse, StatusVersion, MAX_STATUS_RESPONSE_UTF16_UNITS,
};

/// Protocol version used by Minecraft Java Edition 26.2.
pub const SUPPORTED_PROTOCOL_VERSION: i32 = 776;

/// Client connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Handshake,
    Status,
    Login,
    Configuration,
    Play,
}

#[cfg(test)]
mod tests {
    use super::ConnectionState;

    #[test]
    fn states_are_distinct() {
        assert_ne!(ConnectionState::Handshake, ConnectionState::Play);
    }
}
