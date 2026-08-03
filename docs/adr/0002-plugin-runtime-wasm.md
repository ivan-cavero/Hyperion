# ADR 0002 — Plugin runtime: WASM/WIT (wasmtime)

- Status: **Accepted**
- Date: 2026-08-02

## Context

A plugin API is needed that is **secure** (the project's main claim) and that
does not drag in the insecure `dlopen`/`libloading` model (arbitrary code with
full access; also, DLLs cannot be unloaded on Windows).

## Decision

Hyperion plugins run as **WebAssembly** modules with a **WIT** interface
(WebAssembly Interface Types), using `wasmtime` (Bytecode Alliance, 18.4k★,
Apache-2.0, active) as the runtime.

## Consequences

- Capability sandbox: the plugin only accesses what the API grants.
- Clean hot-reload: WASM unloads without `dlopen`.
- Identical portability on Windows/Linux/macOS.
- Cost: `wasmtime` is added as an allowlist dependency in Phase 4
  (audit: docs/DEPENDENCIES.md). Fallback: `wasmer`.
- Lua scripting (MLua) is built on the same API for simplicity.
