//! Experimental Java plugin compatibility layer.
//!
//! Strategy: ADR 0006 / docs/COMPATIBILITY.md. Non-priority, opt-in.
//!
//! Via C (goal): compile Java plugin jars to WebAssembly with TeaVM and run
//! them on our existing `wasmtime` sandbox. The Bukkit API subset is provided
//! as WASM host imports implemented in Rust.
//!
//! Via B (fallback): embed a JVM (`jni-rs`) behind this crate and reimplement
//! a Bukkit API subset in Rust.
//!
//! This crate NEVER touches the server hot paths.

/// A compatibility plugin as seen by Hyperion.
///
/// Via C: produced by compiling a Bukkit plugin jar with TeaVM.
/// Via B: produced by loading the jar in an embedded JVM.
pub enum CompatPlugin {
    /// WebAssembly module (TeaVM output) — sandboxed by default.
    Wasm(Vec<u8>),
    /// Java archive executed in an embedded JVM (plan B, opt-in).
    #[allow(dead_code)] // plan B, not implemented yet
    Jar(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_plugins_are_bytes() {
        let p = CompatPlugin::Wasm(vec![0x00, 0x61, 0x73, 0x6d]); // "\0asm" magic
        match p {
            CompatPlugin::Wasm(bytes) => assert_eq!(bytes, vec![0x00, 0x61, 0x73, 0x6d]),
            _ => unreachable!(),
        }
    }
}
