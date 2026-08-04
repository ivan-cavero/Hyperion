//! Hyperion server — entry point.
//!
//! Phase 1: starts the async network (tokio) and handles Handshake →
//! Status / Login → Configuration → Play. Supports both online-mode
//! (Mojang authentication) and offline-mode (unauthenticated).
//!
//! Configuration is loaded from `server.properties` (Minecraft-style).
//! CLI flags override file values after the file is loaded.
//!
//! Usage:
//!   hyperion-server [OPTIONS] [BIND_ADDRESS]
//!
//! Log level is controlled via `RUST_LOG` (default: `info`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hyperion_core::VERSION;
use hyperion_server::config::{DEFAULT_CONFIG_PATH, ServerConfig};
use hyperion_server::network::serve;
use hyperion_world::{BootstrapConfig, prepare_data_directory};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
Usage: hyperion-server [OPTIONS] [BIND_ADDRESS]

Configuration is read from server.properties (created with defaults on first run).
CLI flags override values from the file.

Options:
  BIND_ADDRESS         host:port to listen on (overrides server-ip / server-port)
  -c, --config PATH    path to server.properties (default: server.properties)
  --online-mode        authenticate players against Mojang (encrypted sessions)
  --offline-mode       allow unauthenticated players
  --help               show this help";

#[tokio::main]
async fn main() -> ExitCode {
    // Default to INFO for the whole process so join/login always show up.
    // Override with e.g. RUST_LOG=hyperion_server=debug,hyperion_protocol=warn
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .init();

    info!("Hyperion {VERSION} — native Minecraft server in Rust");

    let mut config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
    let mut online_mode_override: Option<bool> = None;
    let mut bind_override: Option<String> = None;

    let mut args = std::env::args().skip(1).peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--online-mode" => online_mode_override = Some(true),
            "--offline-mode" => online_mode_override = Some(false),
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-c" | "--config" => {
                let Some(path) = args.next() else {
                    error!("--config requires a path argument");
                    println!("{USAGE}");
                    return ExitCode::FAILURE;
                };
                config_path = PathBuf::from(path);
            }
            value if value.starts_with("--config=") => {
                config_path = PathBuf::from(value.trim_start_matches("--config="));
            }
            value if !value.starts_with('-') => bind_override = Some(value.to_owned()),
            unknown => {
                error!("unknown argument: {unknown}");
                println!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut config = match ServerConfig::load(&config_path) {
        Ok(config) => {
            info!(path = %config_path.display(), "loaded server.properties");
            config
        }
        Err(error) => {
            error!("failed to load configuration: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(online_mode) = online_mode_override {
        config.online_mode = online_mode;
    }
    if let Some(bind) = bind_override
        && let Err(error) = apply_bind_override(&mut config, &bind)
    {
        error!("{error}");
        println!("{USAGE}");
        return ExitCode::FAILURE;
    }

    // Data root = parent of server.properties, or cwd when the config path has
    // no parent component (e.g. plain "server.properties").
    let data_root = config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let data_paths = match prepare_data_directory(&BootstrapConfig {
        root: data_root,
        level_name: config.level_name.clone(),
        level_seed: config.level_seed,
        spawn_y: config.spawn_y,
    }) {
        Ok(paths) => paths,
        Err(error) => {
            error!("failed to prepare data directory: {error}");
            return ExitCode::FAILURE;
        }
    };

    info!(
        root = %data_paths.root.display(),
        world_dir = %data_paths.world_dir.display(),
        level_dat = %data_paths.level_dat.display(),
        session_lock = %data_paths.session_lock.display(),
        "data directory ready"
    );

    // Play loads Anvil chunks from this directory (spawn chunk created above).
    config.world_dir = data_paths.world_dir.clone();

    info!(
        config_path = %config_path.display(),
        bind_address = %config.bind_address(),
        online_mode = config.online_mode,
        max_players = config.max_players,
        motd = %config.motd,
        level_name = %config.level_name,
        level_seed = config.level_seed,
        world_dir = %config.world_dir.display(),
        view_distance = config.view_distance,
        simulation_distance = config.simulation_distance,
        compression_threshold = config.compression_threshold,
        session_server = %config.session_server_url,
        "starting server"
    );

    match serve(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("server failed to start: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Parses a `host:port` override into `server_ip` / `server_port`.
///
/// Accepts `25565`, `:25565`, `127.0.0.1:25565`, and `[::1]:25565`.
fn apply_bind_override(config: &mut ServerConfig, bind: &str) -> Result<(), String> {
    if let Ok(port) = bind.parse::<u16>() {
        config.server_ip.clear();
        config.server_port = port;
        return Ok(());
    }

    // Bracketed IPv6: [::1]:25565
    if let Some(rest) = bind.strip_prefix('[') {
        let Some((host, port_part)) = rest.split_once("]:") else {
            return Err(format!(
                "invalid BIND_ADDRESS {bind:?}: expected [ipv6]:port"
            ));
        };
        let port: u16 = port_part
            .parse()
            .map_err(|_| format!("invalid port in BIND_ADDRESS {bind:?}"))?;
        config.server_ip = host.to_owned();
        config.server_port = port;
        return Ok(());
    }

    // host:port — split on the last colon so bare IPv6 without brackets is rejected.
    if let Some((host, port_part)) = bind.rsplit_once(':') {
        if host.contains(':') {
            return Err(format!(
                "invalid BIND_ADDRESS {bind:?}: use [ipv6]:port for IPv6"
            ));
        }
        let port: u16 = port_part
            .parse()
            .map_err(|_| format!("invalid port in BIND_ADDRESS {bind:?}"))?;
        config.server_ip = if host.is_empty() || host == "0.0.0.0" {
            String::new()
        } else {
            host.to_owned()
        };
        config.server_port = port;
        return Ok(());
    }

    Err(format!(
        "invalid BIND_ADDRESS {bind:?}: expected host:port or port"
    ))
}
