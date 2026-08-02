//! Hyperion server — punto de entrada.
//!
//! Fase 0: esqueleto. En fases siguientes, aquí viven la red async (tokio),
//! el wiring de los crates y la máquina de estados del protocolo.

use hyperion_core::VERSION;

fn main() {
    println!("Hyperion {VERSION} — servidor de Minecraft nativo en Rust");
    println!("Estado: planificación (ver ROADMAP.md)");
}
