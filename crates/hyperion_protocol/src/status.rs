use std::io::{self, Write};

use serde::Serialize;

use crate::ProtocolError;
use crate::frame::{PacketFrame, encode_frame, encode_string};

/// Maximum UTF-16 code units in a Status Response JSON string.
pub const MAX_STATUS_RESPONSE_UTF16_UNITS: usize = 32_767;

const STATUS_REQUEST_PACKET_ID: i32 = 0;
const STATUS_RESPONSE_PACKET_ID: i32 = 0;
const PING_REQUEST_PACKET_ID: i32 = 1;
const PONG_RESPONSE_PACKET_ID: i32 = 1;

/// The serverbound Status Ping packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingRequest {
    /// Opaque timestamp echoed by the server.
    pub payload: i64,
}

/// The version section of a server-list Status Response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusVersion {
    /// Human-readable Minecraft version.
    pub name: String,
    /// Minecraft protocol version.
    pub protocol: i32,
}

/// A player shown in the server-list hover sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusPlayer {
    /// Player name.
    pub name: String,
    /// Player UUID in its canonical textual representation.
    pub id: String,
}

/// The players section of a server-list Status Response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusPlayers {
    /// Maximum configured player count.
    pub max: i32,
    /// Current player count.
    pub online: i32,
    /// Optional hover sample.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sample: Vec<StatusPlayer>,
}

/// The description section of a server-list Status Response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusDescription {
    /// Plain text MOTD.
    pub text: String,
}

/// The JSON object returned by the server-list Status exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusResponse {
    /// Server protocol information.
    pub version: StatusVersion,
    /// Current and maximum player counts.
    pub players: StatusPlayers,
    /// Server MOTD as a text component.
    pub description: StatusDescription,
    /// Optional server icon as a `data:image/png;base64,...` data URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// Whether the server enforces secure chat. Clients treat a missing
    /// `enforcesSecureChat` field as `false`, but vanilla always sends it.
    #[serde(rename = "enforcesSecureChat")]
    pub enforces_secure_chat: bool,
}

/// Validate an empty serverbound Status Request frame.
pub fn decode_status_request(frame: &PacketFrame) -> Result<(), ProtocolError> {
    if frame.packet_id != STATUS_REQUEST_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }
    if !frame.payload.is_empty() {
        return Err(ProtocolError::InvalidPacketPayload);
    }

    Ok(())
}

/// Decode a serverbound Status Ping frame.
pub fn decode_ping_request(frame: &PacketFrame) -> Result<PingRequest, ProtocolError> {
    if frame.packet_id != PING_REQUEST_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }
    if frame.payload.len() != 8 {
        return Err(ProtocolError::InvalidPacketPayload);
    }

    let timestamp_bytes: [u8; 8] = frame
        .payload
        .as_slice()
        .try_into()
        .map_err(|_| ProtocolError::InvalidPacketPayload)?;

    Ok(PingRequest {
        payload: i64::from_be_bytes(timestamp_bytes),
    })
}

/// Encode a clientbound Status Pong frame.
pub fn encode_pong_response(payload: i64) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(PONG_RESPONSE_PACKET_ID, &payload.to_be_bytes())
}

/// Encodes the payload of a Status Response: the bounded JSON string.
pub fn encode_status_response_payload(response: &StatusResponse) -> Result<Vec<u8>, ProtocolError> {
    let maximum_json_bytes = MAX_STATUS_RESPONSE_UTF16_UNITS
        .checked_mul(3)
        .ok_or(ProtocolError::StringTooLong)?;
    let mut writer = LimitedWriter::new(maximum_json_bytes);
    let mut serializer = serde_json::Serializer::new(&mut writer);
    response
        .serialize(&mut serializer)
        .map_err(|error| ProtocolError::JsonSerialization(error.to_string()))?;
    let response_json = String::from_utf8(writer.into_inner())
        .map_err(|error| ProtocolError::JsonSerialization(error.to_string()))?;

    encode_string(&response_json, MAX_STATUS_RESPONSE_UTF16_UNITS)
}

/// Encode a clientbound Status Response frame containing bounded JSON.
pub fn encode_status_response(response: &StatusResponse) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(
        STATUS_RESPONSE_PACKET_ID,
        &encode_status_response_payload(response)?,
    )
}

struct LimitedWriter {
    bytes: Vec<u8>,
    maximum_bytes: usize,
}

impl LimitedWriter {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(maximum_bytes),
            maximum_bytes,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining_bytes = self.maximum_bytes.saturating_sub(self.bytes.len());
        if bytes.len() > remaining_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "JSON response exceeds the protocol byte limit",
            ));
        }

        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
