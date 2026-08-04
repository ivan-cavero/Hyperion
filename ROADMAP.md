# 🗺️ Hyperion — ROADMAP

> Full development plan. Dates are indicative for a team of 1–3 people part-time.
> Each phase ends with verifiable exit criteria. The plan is reviewed at the end of each phase.

**Status legend**: ⬜ pending · 🔄 in progress · ✅ completed · ⏸️ deferred

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
- [ ] Worldgen decision: own core vs. cubiomes reference (MIT) vs. inherit from Pumpkin (GPL) — **ADR 0001 open, Phase 2 deadline**
- [x] Data extraction setup (see Phase 2) — chosen and implemented: Mojang data generators + jar extractor (`tools/mc-ref`, ready; generates 26.2 reports + embedded join data)
- [x] README + ROADMAP + CONTRIBUTING + docs (ARCHITECTURE, DEPENDENCIES, ADR 0001–0005) published
- [x] First foundation commit created

**Exit criterion**: `cargo test` green in CI, a `hyperion-server` binary that prints version and starts. ✅

---

## PHASE 1 — Network and protocol (month 1–4) · 🔄

The heart of the project: speak the Minecraft protocol securely.

- [x] **Handshake + status (ping)**: respond to the client server list
- [x] **Live TCP server + status**: tokio listener on 25565, Handshake→Status state machine; a vanilla 26.x client sees the server list and ping responds
- [x] **Full login**: RSA-1024 handshake, AES/CFB8, zlib compression — offline AND online-mode (Mojang auth) working with tests
- [x] **Configuration**: correct vanilla flow (Feature Flags → Known Packs → Registry Data without NBT via `minecraft:core` → Update Tags → Code of Conduct → Finish) with listings generated from the 26.2 jar
- [x] **Play**: basic packets (login (play), keep-alive with timeout kick, chat with echo, ping answer, position, spawn with empty chunk with light) — join to empty world on protocol 776 / 26.2, E2E-tested offline/online through spawn and verified with a real vanilla client
- [ ] **Generated packet codec** by codegen from extracted JSON (registries + protocol)
- [x] **Own NBT** (network-NBT reader/writer + fuzz; **storage NBT** named-root encode/decode for `level.dat` / Anvil — done; full Anvil chunk payload still Phase 2)
- [x] **Fuzzing**: `cargo-fuzz` frame/handshake/status/login/configuration/play/nbt smoke in CI
- [x] **Unit tests** for frame/handshake/status/login/config/play codecs + E2E offline/online through spawn
- [ ] `tools/packet_inspector` tool to debug real traffic against a vanilla client

**Exit criterion**: a vanilla 26.x client joins the server, sees the empty world, chats and moves, with fuzzing green in CI. ✅ (join/chat/moves verified E2E and with a real client; fuzz smoke green for the existing targets)

---

## PHASE 2 — World and 1:1 worldgen (month 4–8) · 🔄

The "same seed, same world" promise is delivered here.

### Foundation in progress (bootstrap / storage NBT — partial only)

- [x] **Server data directory bootstrap**: first-start vanilla-shaped layout via `hyperion_world::prepare_data_directory` — `level-name` / `level-seed` from `server.properties` → `<level-name>/level.dat`, `session.lock`, and empty `ops.json` / `whitelist.json` / `banned-players.json` / `banned-ips.json`; idempotent (does not overwrite existing `level.dat`); wired from `hyperion-server` before `serve`
- [x] **Minimal `level.dat` (gzip storage NBT)**: write/read named-root compound with `Data.LevelName`, `SpawnX/Y/Z`, `DataVersion`, legacy `RandomSeed`, and `WorldGenSettings.seed` — *minimal subset only; not full vanilla level.dat*
- [x] **Storage NBT API**: `encode_named_tag` / `decode_named_tag` in `hyperion_protocol` (distinct from network NBT) + gzip helpers in `hyperion_world`

### Remaining Phase 2 work

- [ ] **Data extraction pipeline**: Fabric mod or Mojang data generators → versioned JSON (registries, biomes, items, protocol) — *partial: `tools/mc-ref` already generates 26.2 reports + join data; the generic versioned pipeline is pending*
- [ ] **Codegen**: `build.rs` generates Rust from the JSON (structs, coders, registries)
- [🔄] **Chunks**: Anvil **region** I/O (`.mca` read/write, zlib/gzip, size caps) done; full chunk NBT schema + serving in Play still pending
- [ ] **Worldgen**: noise (simplex/octaves), biomes, surface, caves, ores, trees — goal block-by-block parity
- [ ] **Structures**: stronghold, villages, bastions… (WIP phase — declare honest parity level)
- [ ] **Own format** "Hyperion chunk format" (HCF) for ultra-fast multi-threaded load/save
- [ ] **Light**: multi-threaded light calculation (sky/block)
- [ ] **Differential testing**: compare generated chunks against vanilla (same seed) in CI
- [ ] Decision: inherited vs own worldgen (close the open Phase 0 item)

**Exit criterion**: same seed → same chunk in Hyperion and vanilla (diff test suite), existing Anvil worlds loadable.

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
| Full 1:1 worldgen (jigsaw structures) | 🟠 Medium | cubiomes reference (MIT) + diff testing; structures in phases |
| Wide multi-version (1.8+) | 🟠 Medium | v1 short range (1.21+); ViaVersion in front as an option |
| Pumpkin GPLv3: if code is inherited, the project becomes GPL | 🟠 Medium | License decision in Phase 0; own code whenever possible |
| WASM performance in plugins | 🟡 Low | wasmtime JIT; Lua scripting for hot paths; benchmark in Phase 6 |
| Abandoned dependencies | 🟡 Low | Allowlist with verified maintenance (docs/DEPENDENCIES.md); re-audit each phase |

---

## Open decisions (pending ADRs)

1. **Worldgen**: own core vs. cubiomes (MIT) vs. inherit from Pumpkin (GPL). *Deadline: Phase 2.*
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
