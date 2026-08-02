//! API de plugins de Hyperion.
//!
//! Contrato estable WASM/WIT, ciclo de vida de plugins, eventos, comandos y
//! scheduler. Plugins sandboxed por capacidades (wasmtime) + scripting Lua
//! (MLua) para simplicidad. Fase 4 del ROADMAP.

/// Evento base que reciben los plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    PlayerJoin,
    PlayerQuit,
    Chat,
    BlockBreak,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_distinct() {
        assert_ne!(Event::PlayerJoin, Event::PlayerQuit);
    }
}
