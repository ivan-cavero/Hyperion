# ADR 0003 — No JVM bridge (do not embed Java)

- Status: **Accepted**
- Date: 2026-08-02

## Context

Compatibility with Bukkit/Spigot/Paper plugins (Java bytecode) was considered
by embedding a JVM in Rust via `jni-rs` and reimplementing the Bukkit API.

## Decision

**No.** Hyperion does not embed Java. `jni-rs` and any JVM bridge are
excluded from the allowlist. Java plugin compatibility is discarded by
design (see README → "Non-goals").

## Justification (evidence)

- Bukkit plugins are Java bytecode: they require a JVM + the full API (1000+ classes).
- Advanced plugins use reflection/NMS (Mojang internals), which do not
  exist on a non-Java server.
- The JNI bridge adds latency and contradicts the performance goal.
- Folia (Java) already documents that multi-threading breaks almost all plugins
  ("expect compatibility at 0"); a multi-threaded native server would break them more.
- GraalVM Native Image does not solve the problem (JNI requires static metadata,
  incompatible with arbitrary jars at runtime).

## Consequences

- Hyperion's plugin ecosystem is native: WASM/WIT + scripting (ADR 0002).
- Legacy Java plugin compatibility is reevaluated in ADR 0006 (Path C: TeaVM → WASM; plan B: embedded JVM).
- Development budget is freed for the real product (WASM API).
---

## Update (2026-08-02)

Reevaluated by **ADR 0006**. The core remains **without a JVM** (this ADR stays
for the core). Java plugin compatibility becomes an opt-in, lower-performance
layer: Path C (TeaVM → WASM on wasmtime) as the goal, Path B (embedded JVM in
`hyperion_compat`) as plan B, no proxy path (discarded).
`jni-rs` remains excluded from the core but **optional** inside
`hyperion_compat`.
