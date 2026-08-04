//! Server configuration loaded from `server.properties`.
//!
//! Hyperion uses a single Minecraft-style Java properties file as the source
//! of truth for runtime settings — the same approach as vanilla
//! (`server.properties`). Every subsystem (status, login, play, …) reads from
//! [`ServerConfig`]; nothing that operators can tune is hard-coded elsewhere.
//!
//! On first start the server creates a default `server.properties` next to the
//! working directory (or at the path given by `--config`). CLI flags override
//! values from the file after it has been loaded.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Default path for the properties file (vanilla name).
pub const DEFAULT_CONFIG_PATH: &str = "server.properties";

/// Default compression threshold (same as vanilla: 256 bytes).
pub const DEFAULT_COMPRESSION_THRESHOLD: usize = 256;

/// Mojang session server base URL used for online-mode verification.
pub const DEFAULT_SESSION_SERVER_URL: &str = "https://sessionserver.mojang.com";

/// How often the server probes a client with a keep-alive.
pub const DEFAULT_KEEP_ALIVE_INTERVAL_SECONDS: u64 = 10;

/// How long a client may stay silent after a keep-alive before being kicked
/// ("Timed out"), mirroring vanilla's two-interval grace period.
pub const DEFAULT_KEEP_ALIVE_TIMEOUT_SECONDS: u64 = 30;

/// Default server list / tab-list message of the day.
pub const DEFAULT_MOTD: &str = "A Hyperion server";

/// Default TCP port (vanilla).
pub const DEFAULT_SERVER_PORT: u16 = 25565;

/// Default view / simulation distance in chunks.
pub const DEFAULT_VIEW_DISTANCE: i32 = 8;

/// Default advertised player cap.
pub const DEFAULT_MAX_PLAYERS: i32 = 20;

/// Default concurrent TCP connection cap (anti-DoS backpressure).
pub const DEFAULT_MAX_CONNECTIONS: usize = 1024;

/// Default spawn feet Y on the Phase 2.1 flat platform (ground top = 63).
pub const DEFAULT_SPAWN_Y: i32 = 64;

/// Default vanilla `level-name` (world folder under the server root, and
/// default NBT `LevelName` written into a new `level.dat`).
pub const DEFAULT_LEVEL_NAME: &str = "world";

/// Default vanilla `level-seed`.
///
/// `0` means “pick a random seed once” when bootstrapping a missing
/// `level.dat` (see `hyperion_world::prepare_data_directory`). The chosen seed
/// is persisted in NBT (`RandomSeed` + `WorldGenSettings.seed`); existing
/// worlds are never overwritten.
pub const DEFAULT_LEVEL_SEED: i64 = 0;

/// Startup configuration of the Hyperion server.
///
/// Values come from `server.properties` (see [`ServerConfig::load`]) and may
/// be overridden from the CLI. Tests construct this struct directly.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Interface to bind. Empty string means all interfaces (`0.0.0.0`),
    /// matching vanilla `server-ip=`.
    pub server_ip: String,
    /// TCP port to listen on (`server-port`).
    pub server_port: u16,
    /// Online mode: authenticate players against Mojang and encrypt sessions.
    pub online_mode: bool,
    /// Packets whose uncompressed body reaches this size are zlib-compressed
    /// (`network-compression-threshold`).
    pub compression_threshold: usize,
    /// Base URL of the Mojang-style session server. Overridable so tests can
    /// point at a local mock instead of sessionserver.mojang.com.
    pub session_server_url: String,
    /// Client render (chunk cache) distance sent to players (`view-distance`).
    pub view_distance: i32,
    /// Server-side simulation distance (`simulation-distance`).
    pub simulation_distance: i32,
    /// Maximum number of players advertised in the status response
    /// (`max-players`).
    pub max_players: i32,
    /// Maximum concurrent TCP connections accepted at once
    /// (`max-connections`). Excess clients wait in the OS backlog.
    pub max_connections: usize,
    /// Message of the day shown in the server list and the tab list (`motd`).
    pub motd: String,
    /// Vanilla `level-name`: world folder under the server root and default
    /// NBT `LevelName` for a new `level.dat`.
    pub level_name: String,
    /// Vanilla `level-seed`. `0` = random seed on first `level.dat` creation
    /// only (persisted as `RandomSeed` / `WorldGenSettings.seed`).
    pub level_seed: i64,
    /// Default spawn Y written into a new `level.dat` as `SpawnY` (player
    /// feet height on the flat platform until worldgen provides a surface).
    pub spawn_y: i32,
    /// World directory (`root/<level-name>/`) used to load Anvil chunks in
    /// Play. Empty means “no world” — Play falls back to a synthetic void
    /// chunk (used by unit tests that never bootstrap a data directory).
    pub world_dir: PathBuf,
    /// How often the server sends a keep-alive to each player.
    pub keep_alive_interval_seconds: u64,
    /// How long a player may stay silent after a keep-alive before being kicked.
    pub keep_alive_timeout_seconds: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server_ip: String::new(),
            server_port: DEFAULT_SERVER_PORT,
            online_mode: true,
            compression_threshold: DEFAULT_COMPRESSION_THRESHOLD,
            session_server_url: DEFAULT_SESSION_SERVER_URL.to_owned(),
            view_distance: DEFAULT_VIEW_DISTANCE,
            simulation_distance: DEFAULT_VIEW_DISTANCE,
            max_players: DEFAULT_MAX_PLAYERS,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            motd: DEFAULT_MOTD.to_owned(),
            level_name: DEFAULT_LEVEL_NAME.to_owned(),
            level_seed: DEFAULT_LEVEL_SEED,
            spawn_y: DEFAULT_SPAWN_Y,
            world_dir: PathBuf::new(),
            keep_alive_interval_seconds: DEFAULT_KEEP_ALIVE_INTERVAL_SECONDS,
            keep_alive_timeout_seconds: DEFAULT_KEEP_ALIVE_TIMEOUT_SECONDS,
        }
    }
}

impl ServerConfig {
    /// Address suitable for [`TcpListener::bind`](tokio::net::TcpListener::bind).
    ///
    /// Empty `server_ip` binds all interfaces (`0.0.0.0`), like vanilla.
    pub fn bind_address(&self) -> String {
        let host = if self.server_ip.is_empty() {
            "0.0.0.0"
        } else {
            self.server_ip.as_str()
        };
        format!("{host}:{}", self.server_port)
    }

    /// Loads configuration from `path`.
    ///
    /// If the file does not exist it is created with the default contents
    /// (vanilla behaviour). Existing keys are applied on top of
    /// [`ServerConfig::default`]; unknown keys are ignored so forward-
    /// compatible files stay readable.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            let defaults = Self::default();
            defaults.write(path)?;
            return Ok(defaults);
        }

        let contents = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_properties(&contents).map_err(|reason| ConfigError::Parse {
            path: path.to_path_buf(),
            reason,
        })
    }

    /// Parses a properties document into a config (defaults for missing keys).
    pub fn from_properties(contents: &str) -> Result<Self, String> {
        let map = parse_properties(contents)?;
        let mut config = Self::default();

        if let Some(value) = map.get("server-ip") {
            config.server_ip = value.clone();
        }
        if let Some(value) = map.get("server-port") {
            config.server_port = parse_value(value, "server-port")?;
        }
        if let Some(value) = map.get("online-mode") {
            config.online_mode = parse_bool(value, "online-mode")?;
        }
        if let Some(value) = map.get("network-compression-threshold") {
            let threshold: i32 = parse_value(value, "network-compression-threshold")?;
            // Vanilla uses -1 to disable compression; we treat it as "never compress".
            config.compression_threshold = if threshold < 0 {
                usize::MAX
            } else {
                threshold as usize
            };
        }
        if let Some(value) = map.get("session-server-url") {
            config.session_server_url = value.clone();
        }
        if let Some(value) = map.get("view-distance") {
            config.view_distance = parse_value(value, "view-distance")?;
        }
        if let Some(value) = map.get("simulation-distance") {
            config.simulation_distance = parse_value(value, "simulation-distance")?;
        }
        if let Some(value) = map.get("max-players") {
            config.max_players = parse_value(value, "max-players")?;
        }
        if let Some(value) = map.get("max-connections") {
            config.max_connections = parse_value(value, "max-connections")?;
            if config.max_connections == 0 {
                return Err("max-connections: must be at least 1".to_owned());
            }
        }
        if let Some(value) = map.get("motd") {
            config.motd = value.clone();
        }
        if let Some(value) = map.get("level-name") {
            if value.is_empty() {
                return Err("level-name: must not be empty".to_owned());
            }
            config.level_name = value.clone();
        }
        if let Some(value) = map.get("level-seed") {
            config.level_seed = parse_value(value, "level-seed")?;
        }
        if let Some(value) = map.get("spawn-y") {
            config.spawn_y = parse_value(value, "spawn-y")?;
        }
        if let Some(value) = map.get("keep-alive-interval") {
            config.keep_alive_interval_seconds = parse_value(value, "keep-alive-interval")?;
        }
        if let Some(value) = map.get("keep-alive-timeout") {
            config.keep_alive_timeout_seconds = parse_value(value, "keep-alive-timeout")?;
        }

        Ok(config)
    }

    /// Writes this configuration to `path` in vanilla properties format.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        fs::write(path, self.to_properties()).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Serialises the config as a commented `server.properties` document.
    pub fn to_properties(&self) -> String {
        let compression = if self.compression_threshold == usize::MAX {
            -1
        } else {
            self.compression_threshold as i32
        };

        format!(
            "\
#Hyperion server properties
#Generated by Hyperion — edit and restart to apply changes.
#Keys mirror vanilla Minecraft where possible.

# Interface to bind (empty = all interfaces, same as vanilla server-ip=).
server-ip={}
# TCP port (vanilla default 25565).
server-port={}
# Authenticate players with Mojang and encrypt the session.
online-mode={}
# Max players advertised in the multiplayer server list.
max-players={}
# Max concurrent TCP connections (backpressure; Hyperion extension).
max-connections={}
# Message of the day (server list + tab list).
motd={}
# World folder name (vanilla level-name → <level-name>/ and NBT LevelName).
level-name={}
# World seed (vanilla level-seed → level.dat RandomSeed / WorldGenSettings.seed).
# 0 = random seed once when creating a missing level.dat (never overwrites existing).
level-seed={}
# Client chunk render distance.
view-distance={}
# Server-side simulation distance.
simulation-distance={}
# Zlib threshold in bytes (-1 disables compression).
network-compression-threshold={}
# Base URL of the session server (override for proxies / tests).
session-server-url={}
# Default SpawnY in a new level.dat until worldgen is available (Hyperion extension).
spawn-y={}
# Seconds between keep-alive probes (Hyperion extension).
keep-alive-interval={}
# Seconds a client may stay silent after a keep-alive before Timed out (Hyperion extension).
keep-alive-timeout={}
",
            self.server_ip,
            self.server_port,
            self.online_mode,
            self.max_players,
            self.max_connections,
            escape_properties_value(&self.motd),
            escape_properties_value(&self.level_name),
            self.level_seed,
            self.view_distance,
            self.simulation_distance,
            compression,
            self.session_server_url,
            self.spawn_y,
            self.keep_alive_interval_seconds,
            self.keep_alive_timeout_seconds,
        )
    }
}

/// Errors produced while loading or writing `server.properties`.
#[derive(Debug)]
pub enum ConfigError {
    /// Filesystem failure while reading or writing the properties file.
    Io { path: PathBuf, source: io::Error },
    /// A key could not be parsed.
    Parse { path: PathBuf, reason: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to access {}: {source}", path.display())
            }
            Self::Parse { path, reason } => {
                write!(f, "invalid {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { .. } => None,
        }
    }
}

/// Parses a Java-style properties document into an ordered map.
///
/// Supports `#` / `!` comments, blank lines, `key=value` and `key:value`,
/// and leading/trailing whitespace around keys. Values keep interior spaces
/// (needed for multi-word MOTDs).
fn parse_properties(contents: &str) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    for (line_number, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }

        let (key, value) = split_property_line(line).ok_or_else(|| {
            format!(
                "line {}: expected key=value, got {raw_line:?}",
                line_number + 1
            )
        })?;
        map.insert(key, value);
    }
    Ok(map)
}

fn split_property_line(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'=' | b':' => {
                let key = line[..index].trim().to_owned();
                if key.is_empty() {
                    return None;
                }
                let value = line[index + 1..].trim().to_owned();
                return Some((key, unescape_properties_value(&value)));
            }
            b'\\' if index + 1 < bytes.len() => index += 2, // skip escaped separator
            _ => index += 1,
        }
    }
    None
}

fn parse_bool(value: &str, key: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{key}: expected true/false, got {value:?}")),
    }
}

fn parse_value<T: std::str::FromStr>(value: &str, key: &str) -> Result<T, String>
where
    T::Err: fmt::Display,
{
    value
        .parse()
        .map_err(|error| format!("{key}: {error} (got {value:?})"))
}

/// Escapes characters that would break a single-line properties value.
fn escape_properties_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn unescape_properties_value(value: &str) -> String {
    let mut unescaped = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n') => unescaped.push('\n'),
                Some('r') => unescaped.push('\r'),
                Some('t') => unescaped.push('\t'),
                Some(other) => unescaped.push(other),
                None => unescaped.push('\\'),
            }
        } else {
            unescaped.push(character);
        }
    }
    unescaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_properties() {
        let original = ServerConfig::default();
        let text = original.to_properties();
        let loaded = ServerConfig::from_properties(&text).expect("defaults should parse");
        assert_eq!(loaded.server_ip, original.server_ip);
        assert_eq!(loaded.server_port, original.server_port);
        assert_eq!(loaded.online_mode, original.online_mode);
        assert_eq!(loaded.compression_threshold, original.compression_threshold);
        assert_eq!(loaded.session_server_url, original.session_server_url);
        assert_eq!(loaded.view_distance, original.view_distance);
        assert_eq!(loaded.simulation_distance, original.simulation_distance);
        assert_eq!(loaded.max_players, original.max_players);
        assert_eq!(loaded.max_connections, original.max_connections);
        assert_eq!(loaded.motd, original.motd);
        assert_eq!(loaded.level_name, original.level_name);
        assert_eq!(loaded.level_seed, original.level_seed);
        assert_eq!(loaded.spawn_y, original.spawn_y);
        assert_eq!(
            loaded.keep_alive_interval_seconds,
            original.keep_alive_interval_seconds
        );
        assert_eq!(
            loaded.keep_alive_timeout_seconds,
            original.keep_alive_timeout_seconds
        );
    }

    #[test]
    fn level_name_and_seed_round_trip() {
        let config = ServerConfig {
            level_name: "myworld".to_owned(),
            level_seed: 12_345_678_901,
            ..ServerConfig::default()
        };
        let loaded = ServerConfig::from_properties(&config.to_properties()).expect("parse");
        assert_eq!(loaded.level_name, "myworld");
        assert_eq!(loaded.level_seed, 12_345_678_901);
        assert!(config.to_properties().contains("level-name=myworld"));
        assert!(config.to_properties().contains("level-seed=12345678901"));
    }

    #[test]
    fn empty_server_ip_binds_all_interfaces() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_address(), "0.0.0.0:25565");
    }

    #[test]
    fn custom_ip_is_used_in_bind_address() {
        let config = ServerConfig {
            server_ip: "127.0.0.1".to_owned(),
            server_port: 25566,
            ..ServerConfig::default()
        };
        assert_eq!(config.bind_address(), "127.0.0.1:25566");
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let loaded = ServerConfig::from_properties(
            "# comment\nonline-mode=false\nmotd=Hello World\nmax-players=50\n",
        )
        .expect("partial file should parse");
        assert!(!loaded.online_mode);
        assert_eq!(loaded.motd, "Hello World");
        assert_eq!(loaded.max_players, 50);
        assert_eq!(loaded.server_port, DEFAULT_SERVER_PORT);
        assert_eq!(loaded.view_distance, DEFAULT_VIEW_DISTANCE);
    }

    #[test]
    fn negative_compression_threshold_disables_compression() {
        let loaded =
            ServerConfig::from_properties("network-compression-threshold=-1\n").expect("parse");
        assert_eq!(loaded.compression_threshold, usize::MAX);
        let text = loaded.to_properties();
        assert!(text.contains("network-compression-threshold=-1"));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let loaded = ServerConfig::from_properties("pvp=true\nenable-command-block=true\n")
            .expect("unknown keys should not fail");
        assert_eq!(loaded.max_players, DEFAULT_MAX_PLAYERS);
        assert_eq!(loaded.level_name, DEFAULT_LEVEL_NAME);
    }

    #[test]
    fn invalid_bool_is_rejected() {
        let error = ServerConfig::from_properties("online-mode=maybe\n")
            .expect_err("invalid bool must fail");
        assert!(error.contains("online-mode"));
    }

    #[test]
    fn load_creates_missing_file() {
        let dir = std::env::temp_dir().join(format!("hyperion-config-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("server.properties");

        let config = ServerConfig::load(&path).expect("load should create defaults");
        assert!(path.exists());
        assert_eq!(config.motd, DEFAULT_MOTD);

        let on_disk = fs::read_to_string(&path).expect("read created file");
        assert!(on_disk.contains("online-mode=true"));
        assert!(on_disk.contains("max-players=20"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn motd_with_spaces_and_escapes_round_trips() {
        let config = ServerConfig {
            motd: "Hello\\World\tline".to_owned(),
            ..ServerConfig::default()
        };
        let loaded = ServerConfig::from_properties(&config.to_properties()).expect("parse");
        assert_eq!(loaded.motd, config.motd);
    }
}
