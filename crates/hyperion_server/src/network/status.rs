//! The Status exchange: server list and ping/pong.

use hyperion_protocol::{
    SUPPORTED_PROTOCOL_VERSION, StatusDescription, StatusPlayers, StatusResponse, StatusVersion,
    decode_ping_request, decode_status_request, encode_status_response_payload,
};
use tracing::trace;

use super::connection::{Connection, ConnectionError};

/// Clientbound Status packet IDs.
pub const STATUS_RESPONSE_PACKET_ID: i32 = 0;
pub const PONG_RESPONSE_PACKET_ID: i32 = 1;

/// Version name advertised in the server list.
const PROTOCOL_VERSION_NAME: &str = "26.2";
/// Maximum player count advertised.
const MAX_PLAYERS: i32 = 20;
/// Server list MOTD.
const SERVER_MOTD: &str = "A Hyperion server";

/// Handles the Status exchange: server list and ping/pong.
pub(super) async fn serve_status(connection: &mut Connection) -> Result<(), ConnectionError> {
    loop {
        let frame = match connection.read_frame().await {
            Ok(frame) => frame,
            Err(ConnectionError::Disconnected) => return Ok(()),
            Err(error) => return Err(error),
        };
        match frame.packet_id {
            // Status Request (0): respond with the server list.
            0 => {
                decode_status_request(&frame)?;
                trace!("status request received");
                let payload = encode_status_response_payload(&default_status_response())?;
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

/// The server list advertised by Hyperion.
fn default_status_response() -> StatusResponse {
    StatusResponse {
        version: StatusVersion {
            name: PROTOCOL_VERSION_NAME.to_owned(),
            protocol: SUPPORTED_PROTOCOL_VERSION,
        },
        players: StatusPlayers {
            max: MAX_PLAYERS,
            online: 0,
            sample: Vec::new(),
        },
        description: StatusDescription {
            text: SERVER_MOTD.to_owned(),
        },
        favicon: None,
        enforces_secure_chat: false,
    }
}
