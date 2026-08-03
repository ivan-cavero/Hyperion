# 🔌 Compatibility with Java plugins and mods — Strategy

> Strategy document. Compatibility is **NOT a priority** (does not block the
> core) and **opt-in** (enabled per server). Native performance is never
> sacrificed for compatibility.

**Status**: research (Path C). Architecture decision: ADR 0006.

---

## Vision

That a Hyperion server can, **optionally**, load existing Bukkit/Spigot/Paper
plugins — accepting that those plugins will run **worse than on Paper**
(the price of having neither a JVM nor Mojang's code). The native core
(WASM/Lua) remains the product.

---

## The two paths

| Path | Description | Priority | Status |
|-----|-------------|-----------|--------|
| **C. TeaVM → WASM** | Compile the Java plugin to WebAssembly and run it in our WASM runtime (`wasmtime`). The Bukkit API is reimplemented as WASM *host imports* | 🥇 Goal | Research (Phase 4.5) |
| **B. Embedded JVM** | Embedded JVM (`jni-rs`) + reimplementation of a Bukkit API subset in Rust, with Java shims | 🥈 Plan B | Only if Path C fails the go/no-go |

### Path C — TeaVM → WebAssembly (goal)

**Target architecture**:

```
plugin.jar ──TeaVM──▶ plugin.wasm ──▶ wasmtime (Hyperion sandbox)
                        ▲
   Bukkit API (subset) = host imports implemented in Rust
```

- TeaVM (3k★, active) compiles Java bytecode → WASM/JS/C.
- The plugin "sees" the Bukkit API (JavaPlugin, onEnable, commands…) but
  execution is **sandboxed** WASM — fits Hyperion's security architecture
  (ADR 0002) and introduces neither a JVM nor external GC.
- The Bukkit API is reimplemented **only** as host import functions.
  Precedent that the API can be reimplemented: Cardboard (1.1k★, active)
  on Fabric — though Cardboard runs on Mojang's code; we do not.

**Known risks** (documented, not ignored):
- TeaVM does not support well: full reflection, threads, dynamic class-loading
  → a fraction of plugins will not compile.
- "Runtime inside a runtime": the WASM would contain TeaVM's GC → unpredictable
  performance (open debate in Pumpkin #2299, Jun-2026).
- Nobody has done the Bukkit→WASM PoC; it is new ground.

**PoC milestones (Phase 4.5)**:
1. TeaVM compiles a minimal plugin (JavaPlugin + onEnable + 1 command) → WASM.
2. Host imports: expose `Player.send_message`, `command` dispatch, permissions.
3. The plugin runs in wasmtime on Hyperion with an API subset.
4. **Go/no-go**: if >X% of target plugins depend on features TeaVM does not
   support → degrade to Path B (embedded JVM).

### Path B — Embedded JVM (plan B)

- Opt-in module `hyperion_compat`: embedded JVM via `jni-rs` + Bukkit API
  subset implemented in Rust; the Java plugin runs in the JVM and delegates
  to the native core via JNI.
- Cost precedent: Cardboard has taken years and still has incompatibilities.
- Expectation: only "API-only" plugins (no NMS/reflection) — and even then with
  performance inferior to Paper (JNI bridge + synchronous model broken by
  multi-threading, as Folia documents: "expect compatibility at 0").
- License: implementing the *API* (MIT) clean-room is different from copying the
  implementation (GPL). Documented in ADR 0006.

---

## Mods (Fabric / Forge / NeoForge)

| Type | Works on the native core? | Path |
|------|-------------------------------|-----|
| **Client-side** | ✅ Yes (the client loads them; the server needs nothing) | — |
| **Server-side** | ❌ No (they are mixins over Mojang's Java code) | Manual ports to the native API |

Honest expectation: heavy mods (Create, Tinkers…) **will not run** on the
native core except via manual ports. The future goal is that Hyperion's native
API is good enough to *attract* ports of key mods.

---

## Priority and product protection

1. **The native core (WASM/Lua) always comes first** — compat is opt-in.
2. Path C is research; if the PoC does not convince → Path B; if B is unviable
   by cost → documented decision to abandon (no shortcuts that degrade the
   core).
3. All compat code lives isolated in `crates/hyperion_compat` (or an external
   tool) — never touches the server's hot paths.
