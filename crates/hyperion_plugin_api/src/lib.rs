//! Hyperion plugin API.
//!
//! Stable WASM/WIT contract, plugin lifecycle, events, commands, and
//! scheduler. Plugins sandboxed by capabilities (wasmtime) + Lua scripting
//! (MLua) for simplicity. Roadmap Phase 4.

/// Base event received by plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    PlayerJoin,
    PlayerQuit,
    Chat,
    BlockBreak,
}
