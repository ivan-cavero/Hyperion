//! Hyperion server — entry point.
//!
//! Phase 1: starts the async network (tokio) and handles the Handshake →
//! Status flow. Supports both online-mode (Mojang authentication) and
//! offline-mode (unauthenticated).
//!
//! Usage:
//!   hyperion-server [BIND_ADDRESS] [--online-mode]

mod network;

use std::process::ExitCode;

use hyperion_core::VERSION;

/// Default bind address.
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:25565";

#[tokio::main]
async fn main() -> ExitCode {
    println!("Hyperion {VERSION} — native Minecraft server in Rust");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut bind_address = DEFAULT_BIND_ADDRESS.to_owned();
    let mut online_mode = false;

    for arg in &args {
        match arg.as_str() {
            "--online-mode" => online_mode = true,
            "--offline-mode" => online_mode = false,
            _ if !arg.starts_with('-') => bind_address = arg.clone(),
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                return ExitCode::FAILURE;
            }
        }
    }

    match network::serve(&bind_address, online_mode).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Server failed to start: {error}");
            ExitCode::FAILURE
        }
    }
}
