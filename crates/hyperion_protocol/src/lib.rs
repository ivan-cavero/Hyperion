//! Minecraft protocol primitives used by Hyperion.

mod error;
mod frame;
mod handshake;
mod status;

pub use error::ProtocolError;
pub use frame::{decode_frame, encode_frame, PacketFrame, MAX_PACKET_LENGTH};
pub use handshake::{
    decode_handshake, HandshakeIntent, HandshakePacket, MAX_HANDSHAKE_SERVER_ADDRESS_UTF16_UNITS,
};
pub use status::{
    decode_ping_request, decode_status_request, encode_pong_response, encode_status_response,
    PingRequest, StatusDescription, StatusPlayer, StatusPlayers, StatusResponse, StatusVersion,
    MAX_STATUS_RESPONSE_UTF16_UNITS,
};

/// Protocol version used by Minecraft Java Edition 26.2.
pub const SUPPORTED_PROTOCOL_VERSION: i32 = 776;

/// Estado de conexión de un cliente.
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
