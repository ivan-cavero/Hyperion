use crate::frame::{PacketCursor, PacketFrame};
use crate::ProtocolError;

/// Maximum UTF-16 code units in the Handshake server address.
pub const MAX_HANDSHAKE_SERVER_ADDRESS_UTF16_UNITS: usize = 255;

const HANDSHAKE_PACKET_ID: i32 = 0;
const STATUS_INTENT: i32 = 1;
const LOGIN_INTENT: i32 = 2;
const TRANSFER_INTENT: i32 = 3;

/// The connection intent encoded by a Handshake packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeIntent {
    /// Switch to the Status state.
    Status,
    /// Switch to the Login state.
    Login,
    /// Switch to the Login state after a server transfer.
    Transfer,
}

/// The serverbound Handshake packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakePacket {
    /// Protocol version announced by the client.
    pub protocol_version: i32,
    /// Hostname or address supplied by the client.
    pub server_address: String,
    /// Port supplied by the client.
    pub server_port: u16,
    /// State transition requested by the client.
    pub intent: HandshakeIntent,
}

/// Decode a serverbound Handshake frame.
pub fn decode_handshake(frame: &PacketFrame) -> Result<HandshakePacket, ProtocolError> {
    if frame.packet_id != HANDSHAKE_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }

    let mut cursor = PacketCursor::new(&frame.payload);
    let protocol_version = cursor.read_var_i32()?;
    let server_address = cursor.read_string(MAX_HANDSHAKE_SERVER_ADDRESS_UTF16_UNITS)?;
    let server_port = cursor.read_u16()?;
    let intent = match cursor.read_var_i32()? {
        STATUS_INTENT => HandshakeIntent::Status,
        LOGIN_INTENT => HandshakeIntent::Login,
        TRANSFER_INTENT => HandshakeIntent::Transfer,
        _ => return Err(ProtocolError::InvalidHandshakeIntent),
    };

    cursor.finish()?;

    Ok(HandshakePacket {
        protocol_version,
        server_address,
        server_port,
        intent,
    })
}
