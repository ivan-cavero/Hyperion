//! Network connections and protocol state machine (Phase 1).
//!
//! Handles the Handshake → Status (server list + ping) flow and the
//! Handshake → Login flow in both offline and online modes. Online mode
//! follows the vanilla authentication protocol: Encryption Request,
//! shared-secret exchange via RSA, AES/CFB8 activation, and Mojang
//! session-server verification.

pub(crate) mod connection;
mod login;
mod status;

use std::io;
use std::net::SocketAddr;

use hyperion_protocol::{HandshakeIntent, decode_handshake};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info, warn};

pub use self::connection::ConnectionError;
pub use self::status::{PONG_RESPONSE_PACKET_ID, STATUS_RESPONSE_PACKET_ID};

use self::connection::Connection;
use self::login::serve_login;
use self::status::serve_status;
use crate::config::ServerConfig;

/// Accepts connections on `config.bind_address` and dispatches each to its
/// own task.
pub async fn serve(config: ServerConfig) -> io::Result<()> {
    let listener = TcpListener::bind(&config.bind_address).await?;
    info!(
        bind_address = %config.bind_address,
        online_mode = config.online_mode,
        compression_threshold = config.compression_threshold,
        session_server = %config.session_server_url,
        "server listening"
    );

    loop {
        let (stream, peer_address) = listener.accept().await?;
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, peer_address, config).await {
                match error {
                    ConnectionError::Disconnected => {}
                    ConnectionError::Io(io_error) => {
                        warn!(%peer_address, %io_error, "connection I/O error");
                    }
                    ConnectionError::Protocol(protocol_error) => {
                        warn!(%peer_address, %protocol_error, "protocol violation");
                    }
                    ConnectionError::Auth(reason) => {
                        warn!(%peer_address, %reason, "authentication failed");
                    }
                }
            }
        });
    }
}

/// Handles a single connection: Handshake then the chosen intent flow.
pub async fn handle_connection(
    stream: TcpStream,
    peer_address: SocketAddr,
    config: ServerConfig,
) -> Result<(), ConnectionError> {
    let mut connection = Connection::new(stream);
    debug!(%peer_address, "connection accepted");

    let handshake_frame = connection.read_frame().await?;
    let handshake = decode_handshake(&handshake_frame)?;
    debug!(
        %peer_address,
        intent = ?handshake.intent,
        protocol_version = handshake.protocol_version,
        server_address = %handshake.server_address,
        "handshake received"
    );

    match handshake.intent {
        HandshakeIntent::Status => serve_status(&mut connection).await,
        HandshakeIntent::Login => serve_login(&mut connection, &config, peer_address).await,
        // Transfer arrives in a later milestone.
        HandshakeIntent::Transfer => {
            warn!(%peer_address, "transfer intent is not implemented yet");
            Ok(())
        }
    }
}
