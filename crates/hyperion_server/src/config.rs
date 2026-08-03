//! Server startup configuration.
//!
//! Everything the network layer needs to know about how the server was
//! launched, so the whole behavior is visible in one place (and loggable).

/// Default compression threshold (same as vanilla: 256 bytes).
pub const DEFAULT_COMPRESSION_THRESHOLD: usize = 256;

/// Mojang session server base URL used for online-mode verification.
pub const DEFAULT_SESSION_SERVER_URL: &str = "https://sessionserver.mojang.com";

/// Vanilla keep-alive interval: 15 seconds.
pub const DEFAULT_KEEP_ALIVE_INTERVAL_SECONDS: u64 = 10;
/// Vanilla keep-alive timeout: a client that does not answer within 30
/// seconds (two intervals) is kicked with "Timed out".
pub const DEFAULT_KEEP_ALIVE_TIMEOUT_SECONDS: u64 = 30;

/// Startup configuration of the Hyperion server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address the TCP listener binds to.
    pub bind_address: String,
    /// Online mode: authenticate players against Mojang and encrypt sessions.
    pub online_mode: bool,
    /// Packets whose uncompressed body reaches this size are zlib-compressed.
    pub compression_threshold: usize,
    /// Base URL of the Mojang-style session server. Overridable so tests can
    /// point at a local mock instead of sessionserver.mojang.com.
    pub session_server_url: String,
    /// Client render (chunk cache) distance sent to players.
    pub view_distance: i32,
    /// Maximum number of players advertised in the status response.
    pub max_players: i32,
    /// Y level of the default spawn point.
    pub spawn_y: i32,
    /// How often the server sends a keep-alive to each player.
    pub keep_alive_interval_seconds: u64,
    /// How long a player may stay silent after a keep-alive before being
    /// kicked, mirroring vanilla's two-interval grace period.
    pub keep_alive_timeout_seconds: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:25565".to_owned(),
            online_mode: true,
            compression_threshold: DEFAULT_COMPRESSION_THRESHOLD,
            session_server_url: DEFAULT_SESSION_SERVER_URL.to_owned(),
            view_distance: 8,
            max_players: 20,
            spawn_y: 100,
            keep_alive_interval_seconds: DEFAULT_KEEP_ALIVE_INTERVAL_SECONDS,
            keep_alive_timeout_seconds: DEFAULT_KEEP_ALIVE_TIMEOUT_SECONDS,
        }
    }
}
