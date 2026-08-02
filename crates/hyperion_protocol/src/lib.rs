//! Protocolo de Minecraft para Hyperion.
//!
//! Codec de paquetes (generado por codegen desde JSON extraído), formato NBT
//! propio y la capa de multi-versión (remapping de block-states).
//! Fase 1 del ROADMAP. Nada de esto depende de crates de terceros.

/// Estado de conexión de un cliente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Handshake,
    Status,
    Login,
    Configuration,
    Play,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_distinct() {
        assert_ne!(ConnectionState::Handshake, ConnectionState::Play);
    }
}
