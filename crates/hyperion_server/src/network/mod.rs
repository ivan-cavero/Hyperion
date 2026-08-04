//! Network connections and protocol state machine (Phase 1).
//!
//! Handles the Handshake → Status (server list + ping) flow and the
//! Handshake → Login flow in both offline and online modes. Online mode
//! follows the vanilla authentication protocol: Encryption Request,
//! shared-secret exchange via RSA, AES/CFB8 activation, and Mojang
//! session-server verification.

pub(crate) mod chunk_view;
pub(crate) mod configuration;
pub(crate) mod connection;
mod join_data;
mod login;
mod play;
mod status;

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use hyperion_protocol::{HandshakeIntent, decode_handshake};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

pub use self::connection::ConnectionError;
pub use self::status::{PONG_RESPONSE_PACKET_ID, STATUS_RESPONSE_PACKET_ID};

use self::configuration::serve_configuration;
use self::connection::Connection;
use self::login::serve_login;
use self::play::serve_play;
use self::status::serve_status;
use crate::config::ServerConfig;
use crate::key_pool::KeyPool;

/// Number of background RSA key generators for online-mode logins.
///
/// A pool of 4 means up to 4 key generations can run concurrently in the
/// background, which easily handles burst logins: median keygen takes ≈37 ms,
/// so 4 generators produce ~108 keys/second — far more than any realistic
/// server needs.
const KEY_POOL_SIZE: usize = 4;

/// Accepts connections on the address from [`ServerConfig`] and dispatches
/// each to its own task.
pub async fn serve(config: ServerConfig) -> io::Result<()> {
    let bind_address = config.bind_address();
    let listener = TcpListener::bind(&bind_address).await?;
    let key_pool = KeyPool::new(KEY_POOL_SIZE);
    info!(
        bind_address = %bind_address,
        online_mode = config.online_mode,
        max_players = config.max_players,
        motd = %config.motd,
        compression_threshold = config.compression_threshold,
        session_server = %config.session_server_url,
        key_pool_size = KEY_POOL_SIZE,
        "server listening"
    );

    serve_with_listener(listener, config, key_pool).await
}

/// Accept loop with a connection cap, dispatches each connection to its
/// own task. Separate from [`serve`] so tests can hand over their own
/// bound listener.
///
/// An anti-DoS backpressure: at most `config.max_connections` tasks run
/// concurrently. The permit is acquired *before* `accept`, so excess
/// clients wait in the OS backlog instead of piling up tasks, sockets and
/// memory; each permit is held until its connection task ends.
pub async fn serve_with_listener(
    listener: TcpListener,
    config: ServerConfig,
    key_pool: KeyPool,
) -> io::Result<()> {
    let connection_slots = Arc::new(Semaphore::new(config.max_connections));
    loop {
        let permit = connection_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| io::Error::other("connection semaphore closed"))?;
        let (stream, peer_address) = listener.accept().await?;
        let config = config.clone();
        let key_pool = key_pool.clone();
        tokio::spawn(async move {
            // Held until the connection task ends.
            let _permit = permit;
            // Every failure is logged with stage context by
            // `handle_connection`; this line only marks task completion.
            match handle_connection(stream, peer_address, config, &key_pool).await {
                Ok(()) => debug!(%peer_address, "connection closed"),
                Err(error) => {
                    debug!(%peer_address, ?error, "connection task ended with an error (logged above)");
                }
            }
        });
    }
}

/// Handles a single connection: Handshake then the chosen intent flow.
///
/// Every failure is logged here — with the stage in which it happened and the
/// player name when known — so a broken login shows up in the server console
/// with enough context to debug it. `ConnectionError::Disconnected` is not
/// silent: during Configuration it is almost always the client rejecting a
/// packet we sent (registry data, tags, ...) and closing the connection.
pub async fn handle_connection(
    stream: TcpStream,
    peer_address: SocketAddr,
    config: ServerConfig,
    key_pool: &KeyPool,
) -> Result<(), ConnectionError> {
    let mut connection = Connection::new(stream);
    debug!(%peer_address, "connection accepted");

    let handshake_frame = connection
        .read_frame()
        .await
        .map_err(|error| fail("handshake", &peer_address, None, error))?;
    let handshake = decode_handshake(&handshake_frame).map_err(|error| {
        fail(
            "handshake",
            &peer_address,
            None,
            ConnectionError::Protocol(error),
        )
    })?;
    debug!(
        %peer_address,
        intent = ?handshake.intent,
        protocol_version = handshake.protocol_version,
        server_address = %handshake.server_address,
        "handshake received"
    );

    match handshake.intent {
        HandshakeIntent::Status => match serve_status(&mut connection, &config).await {
            Ok(()) => Ok(()),
            Err(error) => Err(fail("status", &peer_address, None, error)),
        },
        HandshakeIntent::Login => {
            let profile = serve_login(&mut connection, &config, peer_address, key_pool)
                .await
                .map_err(|error| fail("login", &peer_address, None, error))?;
            let username = profile.username.clone();
            serve_configuration(&mut connection, &config)
                .await
                .map_err(|error| fail("configuration", &peer_address, Some(&username), error))?;
            serve_play(&mut connection, profile, &config)
                .await
                .map_err(|error| fail("play", &peer_address, Some(&username), error))
        }
        // Transfer arrives in a later milestone.
        HandshakeIntent::Transfer => {
            warn!(%peer_address, "transfer intent is not implemented yet");
            Ok(())
        }
    }
}

/// Logs a connection failure with the stage in which it happened and returns
/// the error unchanged. This is the single place connection errors are
/// surfaced to the console.
fn fail(
    stage: &'static str,
    peer_address: &SocketAddr,
    username: Option<&str>,
    error: ConnectionError,
) -> ConnectionError {
    let who = username.unwrap_or("<anonymous>");
    match &error {
        ConnectionError::Disconnected => match stage {
            // The client closed TCP without an error packet. During the join
            // sequence this is usually the client rejecting something we sent
            // (registry data, feature flags, chunks...) and disconnecting from
            // its side — the reason appears in the client's own error screen.
            "configuration" => warn!(
                %peer_address,
                username = who,
                "client disconnected during configuration — the client likely rejected a packet we sent; check the client's error message"
            ),
            // A normal quit: the player closed the game.
            "play" => info!(
                %peer_address,
                username = who,
                "player left the game"
            ),
            _ => debug!(
                %peer_address,
                username = who,
                stage,
                "client disconnected"
            ),
        },
        ConnectionError::TimedOut => warn!(
            %peer_address,
            username = who,
            stage,
            "connection timed out: the client stopped responding"
        ),
        ConnectionError::Io(io_error) => error!(
            %peer_address,
            username = who,
            stage,
            %io_error,
            "connection I/O error"
        ),
        ConnectionError::Protocol(protocol_error) => error!(
            %peer_address,
            username = who,
            stage,
            %protocol_error,
            "connection failed: protocol error"
        ),
        ConnectionError::Auth(reason) => warn!(
            %peer_address,
            username = who,
            stage,
            %reason,
            "connection failed: authentication error"
        ),
    }
    error
}
