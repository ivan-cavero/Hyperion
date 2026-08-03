# ADR 0006 — Java plugin compatibility: strategy (TeaVM → JVM)

- Status: **Accepted**
- Date: 2026-08-02
- Relation: reevaluates and extends ADR 0003.

## Context

Future retrocompatibility with Bukkit/Spigot/Paper plugins and mods is desired.
ADR 0003 discarded the JVM for the **core**. This ADR decides the strategy
for an **opt-in** compatibility layer, without compromising the product.

## Decision

Two paths, in priority order (detail: docs/COMPATIBILITY.md):

1. **Path C (goal)**: compile Java plugins to WebAssembly with TeaVM and
   run them in Hyperion's WASM runtime (`wasmtime`). The Bukkit API is
   reimplemented as WASM host imports. Research with PoC and go/no-go
   criterion.
2. **Path B (plan B)**: opt-in module `hyperion_compat` with embedded JVM
   (`jni-rs`) + Bukkit API subset in Rust. Only "API-only" plugins.

**Discarded by team decision**: the proxy coexistence path
(Java servers behind a proxy). It adds no product value and is not
contemplated.

Mods: client-side free; server-side only manual ports to the native API.

## Consequences

- `jni-rs` becomes **optional** on the allowlist: only inside
  `hyperion_compat` (Path B), never in the core (ADR 0003 remains for the core).
- TeaVM is an **external tool** (Java compiler), not a runtime crate;
  audited separately (see docs/DEPENDENCIES.md).
- Compatibility is **not a priority and opt-in**; native performance is not sacrificed.
- Java plugins will run worse than on Paper (no native JVM, synchronous model
  broken by multi-threading — evidence: Folia "expect compatibility at 0").
- License: Paper's API is MIT; implementing it clean-room from Rust does not
  copy the GPL implementation. Documented if Cardboard code is incorporated.
