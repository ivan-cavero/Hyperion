# 🏗️ Hyperion — Architecture

> Living document. This is the target architecture; it is refined with each phase.

## Principles

1. **One thread per region, never shared data between regions.** A region's tick only touches its world. Cross-region interactions (portals, teleports) are asynchronous messages.
2. **Zero GC, zero global locks.** Everything that can be parallelized is parallelized; what cannot is serialized explicitly.
3. **Generated data is not written by hand.** Protocol, registries, and NBT come from codegen from JSONs extracted from Mojang.
4. **No `unsafe` except audited, one-off FFI.** The network and plugin surface is 100% safe.

## Layers

### 1. Network (hyperion_server + hyperion_protocol)
- `tokio` async: hundreds of thousands of connections with a small thread pool.
- AES/CFB8 encryption, zlib compression, RSA-1024 handshake (Mojang format).
- Rate-limiting, per-state timeouts, flood mitigation.
- **Fuzzing** of every packet parser (`cargo-fuzz`), corpus in the repo.

### 2. Protocol (hyperion_protocol)
- Packets defined in extracted JSON → `build.rs` generates structs + coders.
- **Own NBT**: streaming reader/writer, no external dependencies.
- Multi-version: block-state remapping layer between supported versions (short range in v1: 1.21 → 26.x, Pumpkin model).

### 3. World (hyperion_world)
- **Bootstrap (Phase 2)**: first-start data dir — vanilla `level-name` / `level-seed` → `<level-name>/level.dat` (gzip storage NBT), `session.lock`, list JSON stubs, spawn chunk in Anvil.
- **Chunks (Phase 2.1–2.3)**: Anvil region I/O + column model with **single- and multi-palette** sections (storage NBT ↔ network `level_chunk_with_light`). Flat stone platform at spawn until worldgen; `set_block` promotes to multi-palette. Default block-state ids from generated 26.2 report. **HCF** later.
- **1:1 worldgen (ADR 0001)**: **own core** in pure Rust — default behaviour matches the official Java server (density/noise router, biomes, surface rules, features, structures). Play currently uses a **temporary scaffold** heightmap until router fill lands. Verification by golden diffs (same seed). See `docs/WORLDGEN.md`.
- **Light**: full-bright sky mask for now; multi-threaded sky/block light later.

### 4. Simulation (hyperion_simulation)
- **Region ticking**: world partitioned into regions of ~N×N chunks; each region has its own tick loop at 20 TPS.
- **ECS**: entities as components; systems per region; cache-friendly access.
- Subsystems: physics, fluids, redstone, mob AI, inventories, damage.
- **Cross-region interaction**: message queue; a player crossing regions "migrates" (state transfer, not shared data).

### 5. Plugins (hyperion_plugin_api)
- **WASM/WIT ABI**: stable plugin contract (WIT = WebAssembly Interface Types).
- **wasmtime runtime**: capability sandbox; the plugin does not touch the system except what the API grants.
- Lifecycle: `on_load` / `on_enable` / `on_disable`; events; commands (Brigadier tree); per-region scheduler.
- **Lua scripting** (MLua) over the same API for plugins in 20 lines.
- Clean hot-reload (WASM unloads without `dlopen`).

## Data flow (player joining)

```
Client ──handshake/login──▶ Network (tokio) ──▶ Protocol (parse + fuzz)
    │                                               │
    │                                    Auth/encryption/compression
    │                                               ▼
    │                                   Simulation (assigns region)
    │                                               │
    ◀────────── chunk data + entities ──────────────┘
```

## Codegen (the update machine)

```
Mojang jar / data generators ──▶ tools/extract ──▶ versioned JSON
                                                      │
                        build.rs (codegen) ◀───────────┘
                                                      │
                          structs + coders + registries
```

Each Mojang release = a new data commit + regenerate. The LLM part (optional) assists with diffs, always verified by fuzzing and diff testing.

## Connection state (state machine)

`Handshake → Status | Login → (encryption → compression) → Configuration → Play`

Each state has its own packet set, timeouts, and fuzzing.
