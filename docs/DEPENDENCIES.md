# 📦 Hyperion — Dependency policy

> **Principle**: less is more. Every external crate is a maintenance debt.
> Only dependencies with proven maintenance, permissive license, and no
> unnecessary `unsafe` are accepted. Minecraft-specific code is implemented here.

## Rules

1. **No new dependency without an audit PR** (justify: purpose, license, maintenance, alternative).
2. **Audit each phase** of the ROADMAP: if a dependency has 6+ months without a release and without commits, it is replaced or removed.
3. **`cargo audit` in CI** (vulnerability alerts).
4. **Minimize transitive dependencies**: prefer crates from the same maintained ecosystem.
5. **Prefer zero-dependency** for everything Minecraft-specific.

## Allowlist (verified 2026-08-03)

| Crate | Purpose | License | Maintained by | Status | Added in |
|---|---|---|---|---|---|
| `tokio` | Async runtime (network) | MIT | Tokio team (AWS) | ✅ active | Phase 1 |
| `bytes` | Network buffers | MIT | Tokio team | ✅ active | Phase 1 |
| `flate2` / `miniz_oxide` | zlib compression | MIT/Apache-2.0 | Alex Crichton | ✅ active | Phase 1 |
| `sha1`, `sha2`, `aes`, `rsa` | Handshake crypto | Apache-2.0/MIT | RustCrypto | ✅ active | Phase 1 |
| `md5` | Offline UUID v3 (vanilla `OfflinePlayer:<name>` parity) | MIT/Apache-2.0 | RustCrypto | ✅ active | Phase 1 |
| `rand` | RNG for RSA key generation | MIT/Apache-2.0 | Rust Random project | ✅ active | Phase 1 |
| `serde` + `serde_json` | Extracted data | MIT/Apache-2.0 | Serde team | ✅ active | Phase 1 |
| `uuid` | Player/entity IDs | MIT/Apache-2.0 | uuid-rs | ✅ active | Phase 1 |
| `thiserror` | Ergonomic errors | MIT/Apache-2.0 | dtolnay | ✅ active | Phase 1 |
| `tracing` | Structured logging | MIT | Tokio team | ✅ active | Phase 1 |
| `tracing-subscriber` | Logging runtime + `RUST_LOG` (env-filter) | MIT | Tokio team | ✅ active | Phase 1 |
| `reqwest` (with `rustls-tls`, no default-tls) | HTTP client for Mojang session server | MIT/Apache-2.0 | Hyper (Sean McArthur et al.) | ✅ active | Phase 1 |
| `rustls` (transitive) | Pure-Rust TLS — avoids openssl/native-tls | MIT/Apache-2.0 | rustls team | ✅ active | Phase 1 |
| `webpki-roots` (transitive via rustls) | Mozilla CA root store for TLS | **CDLA-Permissive-2.0** | rustls / webpki-roots | ✅ active | Phase 1 |
| `libfuzzer-sys` | libFuzzer engine, only in `crates/hyperion_protocol/fuzz/` | Apache-2.0/MIT | Rust Fuzz project | ✅ active | Phase 1 |
| `rayon` | Data parallelism | MIT/Apache-2.0 | Rayon team | ✅ active | Phase 3 |
| `crossbeam` | Concurrency utilities | MIT/Apache-2.0 | Crossbeam team | ✅ active | Phase 3 |
| `parking_lot` | Faster locks | MIT/Apache-2.0 | Amanieu | ✅ active | Phase 3 |
| `dashmap` | Concurrent maps | MIT | xacrimon | ✅ active | Phase 3 |
| `wasmtime` | Plugin WASM runtime | Apache-2.0 | Bytecode Alliance (18.4k★) | ✅ active | Phase 4 |
| `mlua` | Lua scripting | MIT | mlua-rs (2.8k★) | ✅ active | Phase 4 |
| `cubiomes` (C, FFI) | Worldgen biomes/structures reference | **MIT** | Cubitect | ✅ active | Phase 2 (optional) |
| `bevy_ecs` | ECS (open decision) | MIT/Apache-2.0 | Bevy org | ✅ active | Phase 3 (decision) |

## What we implement ourselves (no external dependency)

| Module | Why |
|---|---|
| **Protocol / packets** | Minecraft-specific; codegen from JSON |
| **NBT** | Simple public format; full performance control |
| **Worldgen core** | 1:1 parity requires fine control; cubiomes only as reference |
| **HCF world format** | Ultra-fast multi-threaded I/O |
| **Simulation / region ticking** | Product core; no delegation |
| **Plugin ABI** | Own stable contract (WIT) |

## Explicitly excluded

| Crate | Why NOT |
|---|---|
| `jni-rs` / JVM bridges | Excluded from the CORE (ADR 0003). Optional ONLY inside `hyperion_compat` (Path B, ADR 0006) |
| `libloading` / `dlopen` | Native plugins = arbitrary code without sandbox; also cannot be unloaded on Windows. WASM solves the problem |
| Third-party "minecraft server" crates (valence, etc.) | Hyperion is a from-scratch project: learn and control everything. Studied only as reference |

## Fallbacks (if a dependency dies)

- `tokio`/`bytes` → std + threads (cost: more code, less abstraction)
- `flate2` → `miniz_oxide` (already transitive) or own zlib-sys
- RustCrypto → own handshake implementation (AES/CFB8 + RSA-1024 are small algorithms)
- `wasmtime` → `wasmer` (same license, similar ecosystem) — decision with expiry date
- `mlua` → own Lua runtime (Lua 5.4 is small) or Rhai scripting

## Recorded decision: why WASM and not cdylib for plugins?

- **Security**: WASM = capability sandbox; cdylib = arbitrary code with full access.
- **Portability**: WASM works the same on Windows/Linux/macOS; cdylib cannot be unloaded on Windows.
- **Hot-reload**: WASM unloads cleanly; `.so`/`.dll` do not.
- **It is the direction the native ecosystem is taking** (Pumpkin migrates from libloading to WIT/WASM).

*Last audit: 2026-08-02 · Next: at the start of each ROADMAP phase.*
## External compatibility tools (NOT runtime crates)

Audited separately, used only in the `hyperion_compat` pipeline (ADR 0006):

| Tool | Use | License | Status |
|---|---|---|---|
| **TeaVM** | Compile Java plugins (bytecode) → WebAssembly (Path C) | Apache-2.0 | 3k★, active — Phase 4.5 research |
| **JDK** (toolchain) | Compile test plugins / Bukkit API jar | GPLv2+CE / OpenJDK | Development toolchain, not runtime |

These tools do NOT enter the runtime crate allowlist and cannot be used in the
Hyperion core.
