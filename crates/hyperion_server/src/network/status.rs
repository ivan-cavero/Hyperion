//! The Status exchange: server list and ping/pong.

use std::time::Duration;

use hyperion_protocol::{
    SUPPORTED_PROTOCOL_VERSION, StatusDescription, StatusPlayers, StatusResponse, StatusVersion,
    decode_ping_request, decode_status_request, encode_status_response_payload,
};
use tracing::trace;

use super::connection::{Connection, ConnectionError};
use crate::config::ServerConfig;

/// Clientbound Status packet IDs.
pub const STATUS_RESPONSE_PACKET_ID: i32 = 0;
pub const PONG_RESPONSE_PACKET_ID: i32 = 1;

/// An idle status connection is dropped after this long; it only exists to
/// answer a server-list probe.
const STATUS_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Version name advertised in the server list.
const PROTOCOL_VERSION_NAME: &str = "26.2";

/// Handles the Status exchange: server list and ping/pong.
///
/// Player cap and MOTD come from [`ServerConfig`] so the multiplayer list
/// matches `server.properties` (`max-players`, `motd`).
pub(super) async fn serve_status(
    connection: &mut Connection,
    config: &ServerConfig,
) -> Result<(), ConnectionError> {
    loop {
        let frame = match connection.read_frame_timeout(STATUS_READ_TIMEOUT).await {
            Ok(frame) => frame,
            Err(ConnectionError::Disconnected) | Err(ConnectionError::TimedOut) => return Ok(()),
            Err(error) => return Err(error),
        };
        match frame.packet_id {
            // Status Request (0): respond with the server list.
            0 => {
                decode_status_request(&frame)?;
                trace!("status request received");
                let payload = encode_status_response_payload(&status_response(config))?;
                connection
                    .write_frame(STATUS_RESPONSE_PACKET_ID, &payload)
                    .await?;
            }
            // Ping Request (1): Pong with the same payload.
            1 => {
                let ping = decode_ping_request(&frame)?;
                trace!(ping_payload = ping.payload, "ping request received");
                connection
                    .write_frame(PONG_RESPONSE_PACKET_ID, &ping.payload.to_be_bytes())
                    .await?;
            }
            _ => return Ok(()),
        }
    }
}

/// Builds the server-list payload from the live configuration.
fn status_response(config: &ServerConfig) -> StatusResponse {
    StatusResponse {
        version: StatusVersion {
            name: PROTOCOL_VERSION_NAME.to_owned(),
            protocol: SUPPORTED_PROTOCOL_VERSION,
        },
        players: StatusPlayers {
            max: config.max_players,
            // Live online count is tracked once multi-player sessions exist.
            online: 0,
            sample: Vec::new(),
        },
        description: StatusDescription {
            text: config.motd.clone(),
        },
        favicon: None,
        enforces_secure_chat: false,
    }
}
