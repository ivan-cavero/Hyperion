//! Hyperion server — entry point.
//!
//! Phase 1: starts the async network (tokio) and handles the Handshake →
//! Status flow. Supports both online-mode (Mojang authentication) and
//! offline-mode (unauthenticated).
//!
//! Usage:
//!   hyperion-server [BIND_ADDRESS] [--online-mode | --offline-mode]
//!
//! Log level is controlled via `RUST_LOG` (default: `hyperion=info`).

use std::process::ExitCode;

use hyperion_core::VERSION;
use hyperion_server::config::ServerConfig;
use hyperion_server::network::serve;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
Usage: hyperion-server [BIND_ADDRESS] [--online-mode | --offline-mode]

Options:
  BIND_ADDRESS     address to listen on (default: 0.0.0.0:25565)
  --online-mode    authenticate players against Mojang (encrypted sessions)
  --offline-mode   allow unauthenticated players (default)
  --help           show this help";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("hyperion=info")),
        )
        .init();

    info!("Hyperion {VERSION} — native Minecraft server in Rust");

    let mut config = ServerConfig::default();
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--online-mode" => config.online_mode = true,
            "--offline-mode" => config.online_mode = false,
            "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            value if !value.starts_with('-') => config.bind_address = value.to_owned(),
            unknown => {
                error!("unknown argument: {unknown}");
                println!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    info!(
        bind_address = %config.bind_address,
        online_mode = config.online_mode,
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
