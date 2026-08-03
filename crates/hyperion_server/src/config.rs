//! Server startup configuration.
//!
//! Everything the network layer needs to know about how the server was
//! launched, so the whole behavior is visible in one place (and loggable).

/// Default compression threshold (same as vanilla: 256 bytes).
pub const DEFAULT_COMPRESSION_THRESHOLD: usize = 256;

/// Mojang session server base URL used for online-mode verification.
pub const DEFAULT_SESSION_SERVER_URL: &str = "https://sessionserver.mojang.com";

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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:25565".to_owned(),
            online_mode: false,
            compression_threshold: DEFAULT_COMPRESSION_THRESHOLD,
            session_server_url: DEFAULT_SESSION_SERVER_URL.to_owned(),
        }
    }
}
