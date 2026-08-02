//! Hyperion server — punto de entrada.
//!
//! Fase 1: arranca la red async (tokio) y atiende el flujo Handshake → Status.
//! El argumento opcional es la dirección de bind (por defecto 0.0.0.0:25565).

mod network;

use std::process::ExitCode;

use hyperion_core::VERSION;

/// Dirección de bind por defecto.
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:25565";

#[tokio::main]
async fn main() -> ExitCode {
    println!("Hyperion {VERSION} — servidor de Minecraft nativo en Rust");

    let bind_address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_BIND_ADDRESS.to_owned());

    match network::serve(&bind_address).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("El servidor no pudo arrancar: {error}");
            ExitCode::FAILURE
        }
    }
}
