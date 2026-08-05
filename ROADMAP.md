# 🗺️ Hyperion — ROADMAP

> Full development plan. Dates are indicative for a team of 1–3 people part-time.
> Each phase ends with verifiable exit criteria. The plan is reviewed at the end of each phase.

**Status legend**: ⬜ pending · 🔄 in progress · ✅ completed · ⏸️ deferred

---

## Where we are (snapshot · 2026-08-04)

**Current phase**: **Phase 2** (world + performance foundation) · Phase 0–1 **done**.

A vanilla **26.2** client can join offline/online, land on a **flat stone platform**,
fly with **chunk streaming** (load/unload by view distance), see brand **Hyperion**
in F3, and use **real creative** mode. CI green (test, clippy, fmt, audit, deny, fuzz).

| Layer | Status | Notes |
|-------|--------|--------|
| Network / protocol 776 | ✅ | Handshake→Status/Login→Config→Play; fuzz; E2E |
| Auth | ✅ | Offline + online (Mojang); AES/CFB8; compression |
| World disk | ✅ | `server.properties`, `level.dat`, Anvil `.mca` (append path) |
| Chunks in Play | ✅ | Flat + multi-palette; view-distance stream |
| Performance path | ✅ baseline | No per-packet flush; flat network template; lazy disk |
| Worldgen 1:1 | ⬜ | ADR 0001 **Accepted**: own core (pure Rust) |
| Simulation / plugins | ⬜ | Stub crates only |

**Product bar (unchanged)**: vanilla **1:1 behaviour** where we claim parity, with a
server path **much faster than Paper** (RAM, disk, login, serve). Flat world is not
full parity yet — it is the scaffold to prove the engine before real worldgen.

### What we shipped recently (2.0 → 2.4)

1. **2.0** — Data dir bootstrap, storage NBT, minimal `level.dat`
2. **Anvil region I/O** — `.mca` read/write with size caps (later: append, not full rewrite)
3. **2.1** — Chunk column NBT, flat platform, Play serves real chunks, creative + brand
4. **2.2** — `ChunkView` streaming on move, unload, cache center, dirty Anvil persist
5. **Perf pass** — TCP batch flush, lazy disk budget, zlib fast
6. **2.3** — Multi-palette sections + generated default block-state ids (26.2); ADR 0001 own core
7. **2.4** — Own-core height noise + surface layers (grass/dirt/stone/bedrock); seed from `level.dat`

### Honest gaps (next work)

- Own-core worldgen layers: noise/surface → biomes → caves/ores → features  
- Async workers (gen/load/save) like Paper  
- Packet codegen (codecs still hand-written; **block-state default ids** are generated)  
- Property-aware block states (only default states today)  
- Light engine (not only full-bright sky mask)

---

## Product vision (north star)

> A native Rust Minecraft server that the ecosystem **chooses for performance and security**:
> faster than Paper, safer than any current server, with sandboxed plugins
> and version updates in days. First hubs/minigames, then high-performance survival.

**Anti-north** (things we are NOT): a Bukkit/Forge replacement, a magic "thousands of players" process, a Paper clone.

---

## PHASE 0 — Foundation (month 0–1) · ✅ (completed 2026-08-02)

Prepare the ground: decisions, toolchain, CI, workspace skeleton.

- [x] Name (Hyperion), MIT license, and basic branding — **public repo: created and push done: https://github.com/ivan-cavero/Hyperion**
- [x] Cargo workspace with the 6 crates (`hyperion_core`, `hyperion_protocol`, `hyperion_world`, `hyperion_simulation`, `hyperion_plugin_api`, `hyperion_server`)
- [x] Pinned stable Rust toolchain (`rust-toolchain.toml`) + `rustfmt.toml` + `.editorconfig` + `.gitattributes` (LF)
- [x] Full CI: `cargo build` · `cargo test` · `cargo clippy -D warnings` · `cargo fmt --check` + **cargo-audit** (security) + **cargo-deny** (licenses) + dependabot — fuzz is added in Phase 1
- [x] Worldgen decision: **own core** (pure Rust) — ADR 0001 Accepted 2026-08-05; no cubiomes FFI, no Pumpkin gen
- [x] Data extraction setup (see Phase 2) — chosen and implemented: Mojang data generators + jar extractor (`tools/mc-ref`, ready; generates 26.2 reports + embedded join data)
- [x] README + ROADMAP + CONTRIBUTING + docs (ARCHITECTURE, DEPENDENCIES, ADR 0001–0005) published
- [x] First foundation commit created

**Exit criterion**: `cargo test` green in CI, a `hyperion-server` binary that prints version and starts. ✅

---

## PHASE 1 — Network and protocol (month 1–4) · ✅ (completed 2026-08)

The heart of the project: speak the Minecraft protocol securely.

- [x] **Handshake + status (ping)**: respond to the client server list
- [x] **Live TCP server + status**: tokio listener on 25565, Handshake→Status state machine; a vanilla 26.x client sees the server list and ping responds
- [x] **Full login**: RSA-1024 handshake, AES/CFB8, zlib compression — offline AND online-mode (Mojang auth) working with tests
- [x] **Configuration**: correct vanilla flow (Feature Flags → Known Packs → Registry Data without NBT via `minecraft:core` → Update Tags → Code of Conduct → Finish) with listings generated from the 26.2 jar
- [x] **Play**: login (play), keep-alive with timeout kick, chat echo, ping answer, position, spawn — verified with a real vanilla 26.2 client (now lands on flat platform, not only void)
- [ ] **Generated packet codec** by codegen from extracted JSON (registries + protocol) — still open; codecs are hand-written for 776
- [x] **Own NBT** (network + storage named-root; fuzz; used for `level.dat` and Anvil)
- [x] **Fuzzing**: `cargo-fuzz` frame/handshake/status/login/configuration/play/nbt smoke in CI
- [x] **Unit + E2E tests** offline/online through spawn
- [ ] `tools/packet_inspector` tool to debug real traffic against a vanilla client

**Exit criterion**: a vanilla 26.x client joins, sees a world, chats and moves, fuzzing green in CI. ✅

---

## PHASE 2 — World and 1:1 worldgen (month 4–8) · 🔄

The "same seed, same world" promise is delivered here. **Scaffold done; 1:1 gen not yet.**

### Phase 2.0 — data directory + storage NBT · ✅

- [x] **Server data directory bootstrap**: vanilla-shaped layout (`level.dat`, `session.lock`, list JSONs, `region/`)
- [x] **Minimal `level.dat`** (gzip storage NBT): `LevelName`, spawn, seed, `DataVersion`
- [x] **Storage NBT API**: `encode_named_tag` / `decode_named_tag` + gzip helpers

### Phase 2.1 — chunk schema + serve from Anvil · ✅

- [x] **Chunk column model**: single-valued sections (block + biome), storage NBT encode/decode
- [x] **Flat spawn**: stone platform (section-aligned ground), air above
- [x] **Play serves real columns** via `level_chunk_with_light` (void fallback when `world_dir` empty for tests)
- [x] **Network heightmaps**: packed 9-bit WORLD_SURFACE + MOTION_BLOCKING
- [x] **Join polish**: full creative abilities (`0x0F`), `minecraft:brand` = Hyperion, view-distance batch at spawn
- [x] **E2E**: `play_world_spawn` asserts brand + N chunks for view distance

### Phase 2.2 — chunk streaming + Anvil · ✅

- [x] **ChunkView**: per-connection loaded set + center; stream on chunk-border move
- [x] **Unload** far columns (`unload_chunk`, Z-then-X wire order for 26.2)
- [x] **Set cache center** when the player crosses borders
- [x] **Anvil append path**: in-place or append sectors (no full-file rewrite / no hot-path `fsync`)
- [x] **Dirty set + lazy persist**: disk work budgeted after spawn / on keep-alive / after stream strips
- [x] **FlatNetworkCache**: one template per connection; send = clone + patch x/z only

### Phase 2.2b — performance baseline (login / serve / RAM / disk) · ✅

Aligned with how vanilla/Paper actually win (batch I/O, never block the join path on full region rewrite):

- [x] **TCP**: `write_frame` does **not** flush every packet; explicit `flush()` after logical steps / chunk batches
- [x] **RAM**: spawn does **not** allocate a Vec of all chunk payloads (stream encode→write)
- [x] **CPU**: zlib `Compression::fast` for network + Anvil; static full-bright light array
- [x] **Disk**: append Anvil; dirty budget; regression test “many writes stay linear”
- [x] **Tests**: view geometry, template patch, persist, join E2E still green

**Next perf (not done yet)**: async worker pool (gen/load/save), process-wide column cache, pre-encoded Configuration payloads, real light engine.

### Remaining Phase 2 work

- [ ] **Data extraction pipeline**: generic versioned JSON pipeline (*partial: `tools/mc-ref` for 26.2 join data + block states*)
- [ ] **Codegen**: `build.rs` → packet structs/coders (*block-state default ids generated*)
- [x] **Chunks (region I/O + stream)**: Anvil + Play streaming (flat)
- [x] **ADR 0001**: own core (pure Rust) — Accepted 2026-08-05
- [x] **Multi-palette sections**: single + indirect + global network encode; Anvil NBT `data`; `set_block`
- [x] **Block-state default ids**: `cargo run -p hyperion_tools --bin gen-block-states` → generated table (26.2)
- [x] **mc-ref codegen in Rust**: `hyperion_tools` (join data, registry NBT, block states; no Python)
- [x] **Worldgen surface scaffold (2.4)**: Hyperion hills (not 1:1); stone/dirt/grass/bedrock; Anvil persist
- [x] **Worldgen default math (2.5)**: Xoroshiro/Legacy RNG, Improved/Perlin/NormalNoise, density AST; `docs/WORLDGEN.md`
- [x] **Worldgen density fill (2.6)**: `NoiseSettings` + `DensityLibrary` + column fill from `final_density` (simple routers); jar load for overworld datapack; learn architecture from open Rust servers without copying GPL
- [ ] **Worldgen (default 1:1)**: complete overworld graph (spline, old_blended_noise, …), surface rules, biomes, carvers, features, golden diffs vs official server
- [ ] **Structures**: stronghold, villages, bastions… (phased parity; jigsaw last)
- [ ] **HCF** (Hyperion Chunk Format) for ultra-fast multi-threaded load/save
- [ ] **Light**: multi-threaded sky/block (today: full-bright sky mask only)
- [ ] **Differential testing**: same seed vs vanilla in CI

**Exit criterion**: same seed → same chunk in Hyperion and vanilla (diff suite), existing Anvil worlds loadable. 🔄 (Anvil load/save path exists; parity gen does not)

---

## PHASE 3 — Multi-core simulation (month 8–12) · ⬜

Where Hyperion separates from the rest: the single thread disappears.

- [ ] **Region ticking**: independent chunk groups, each with its own thread (Folia/MCHPRS pattern)
- [ ] **ECS** (`bevy_ecs` or own system): entities, components, systems — cache-friendly
- [ ] **Physics and movement**: gravity, world collisions
- [ ] **Fluids**: basic flow (water/lava) with per-region updates
- [ ] **Redstone**: basic circuit (wire, torches, repeaters, comparators) — no global locks
- [ ] **Basic mobs**: spawn/despawn, simple AI (zombies, skeletons), damage and death
- [ ] **Inventories and interactive blocks**: chests, furnaces, basic crafting
- [ ] **Cross-region interaction** ONLY via messages (ports, teleports) — no shared data
- [ ] Profiling (perf/tracy) and internal benchmarks

**Exit criterion**: 100+ players in a world with active simulation at stable 20 TPS, no global locks.

---

## PHASE 4 — Plugin API (month 10–14) · ⬜

The differentiator: safe and simple plugins.

- [ ] **WASM/WIT ABI**: stable plugin contract (`on_load`/`on_enable`/`on_disable` lifecycle)
- [ ] **Runtime**: embedded `wasmtime`, capability sandbox (no system access unless granted)
- [ ] **Events**: event system (player join, block break, chat…) with per-region dispatch
- [ ] **Commands**: registration with Brigadier-style tree
- [ ] **Scheduler**: async/sync tasks, scheduled per region
- [ ] **Lua scripting** (MLua) as a simplicity layer: plugin in 20 lines
- [ ] **Plugin SDK**: templates (cargo-generate), docs, examples
- [ ] Safe plugin hot-reload (no `dlopen` — WASM unloads cleanly)

**Exit criterion**: an example WASM plugin (command + event + scheduler) works end-to-end, documented on the landing page.

---

## PHASE 4.5 — Java compatibility (research, NOT priority) · ⬜

Evaluate retrocompatibility with Bukkit/Spigot/Paper plugins without compromising the
core. Full strategy: docs/COMPATIBILITY.md · ADR 0006.

- [ ] **Path C PoC (TeaVM → WASM)**: compile a minimal plugin (JavaPlugin + onEnable + 1 command) to WASM with TeaVM
- [ ] **Host imports PoC**: expose a Bukkit API subset (Player.send_message, command dispatch, permissions) as WASM host imports
- [ ] **Execution PoC**: plugin running in wasmtime on Hyperion (API subset)
- [ ] **Go/no-go criterion**: if features TeaVM does not support (reflection, threads, class-loading) block >X% of target plugins → degrade to Path B
- [ ] **Plan B (Path B)**: embedded JVM PoC (`jni-rs`) + Bukkit API subset in Rust (cost precedent: Cardboard)

**Exit criterion**: go/no-go decision documented with evidence; functional PoC (Path C or B) or decision to abandon. Does not block the main roadmap.

---

## PHASE 5 — Multi-version and assisted updates (month 12–16) · ⬜

The idea of "adapting to each release easily" becomes a system.

- [ ] **Block-state remapping** between versions (Pumpkin model: 1.21→26.x range)
- [ ] **Automatic diff**: when a new version ships, compare JSONs → generate mappings
- [ ] **Versioned codegen**: one release = one data commit + regenerate code
- [ ] **LLM as diff assistant** (with mandatory verification: fuzzing + diff testing against vanilla)
- [ ] Wide range (1.8+): evaluate ViaVersion proxy in front vs. own native translator (decision in Phase 6)
- [ ] Document the "release playbook": exact steps to update to a new version

**Exit criterion**: update from 26.x to 26.(x+1) taking ≤3 person-days with the playbook, 1.21+ clients connecting.

---

## PHASE 6 — Scaling and benchmarks (month 14–18) · ⬜

Prove the performance promise with data.

- [ ] Public benchmarks vs Paper and Folia (same hardware/world/population)
- [ ] Large-scale world pregeneration
- [ ] 500+ players in a world with feature subset at 20 TPS
- [ ] Bandwidth optimization (dynamic view distance, packet coalescing)
- [ ] Architecture decision for "thousands": horizontal sharding with proxy (Velocity/Bungee) + MultiPaper-style, or own multi-version native translator
- [ ] Instrumentation: exportable metrics (tick time per region, chunk-gen, network)

**Exit criterion**: reproducible benchmark published on the landing page with documented advantage over Paper/Folia in realistic scenarios.

---

## PHASE 7 — Bedrock (optional, month 16–24) · ⬜

RakNet protocol + own registries. Only if Java is solid and there is demand.

- [ ] Base RakNet + ECDH/AES encryption
- [ ] Java↔Bedrock entity/block mapping
- [ ] Bedrock skin system

**Exit criterion**: Bedrock and Java clients in the same world (optional product goal).

---

## PHASE 8 — Launch (month 18–24) · ⬜

Turn the project into a product with community.

- [ ] Landing page (Astro + Tailwind): hero, benchmarks, public roadmap, "first plugin in 5 min"
- [ ] Full docs (Starlight/Docusaurus + embedded rustdoc)
- [ ] Public beta: demo server (hub/minigames)
- [ ] Discord + active contribution guidelines
- [ ] v1.0.0: current protocol, declared 1:1 worldgen, stable plugin API

**Exit criterion**: v1.0.0 published, 100+ example plugins created by the community, benchmarks on the front page.

---

## Risks and mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| **Scope** (the largest): full vanilla parity is a multi-year team project (Pumpkin has spent 2 years without 1.0) | 🔴 High | MVP = hub/minigames; declare parity by layers; feature subset for v1 |
| Rust MC project mortality (many die from scope) | 🔴 High | Roadmap with verifiable milestones, community from day 1 |
| Full 1:1 worldgen (jigsaw structures) | 🟠 Medium | Own core + layered parity + diff testing; structures last |
| Wide multi-version (1.8+) | 🟠 Medium | v1 short range (1.21+); ViaVersion in front as an option |
| Pumpkin GPLv3: if code is inherited, the project becomes GPL | 🟠 Medium | License decision in Phase 0; own code whenever possible |
| WASM performance in plugins | 🟡 Low | wasmtime JIT; Lua scripting for hot paths; benchmark in Phase 6 |
| Abandoned dependencies | 🟡 Low | Allowlist with verified maintenance (docs/DEPENDENCIES.md); re-audit each phase |

---

## Open decisions (pending ADRs)

1. ~~**Worldgen**~~ → **ADR 0001 Accepted**: own core (pure Rust).
2. **ECS**: use `bevy_ecs` vs. minimal own ECS system. *Phase 3.*
3. **Final license** if third-party code is incorporated. *Phase 0.*
4. **Multi-version range** for v1: only 26.x vs. 1.21+ like Pumpkin. *Phase 5.*
5. **Bedrock**: include in v1.0 or defer. *Phase 7.*

---

## Success metrics (definition of "we made it")

- Sustained 20 TPS with 500+ players in a world with feature subset (Phase 6)
- Minor version update in ≤3 person-days (Phase 5)
- Working example WASM plugin in <5 minutes for a new developer (Phase 4)
- 100+ contributors and 10+ community plugins before v1.0 (Phase 8)

---

*Hyperion · MIT · Independent project, not affiliated with Mojang/Microsoft.*
