# Architecture Decision Records (ADR)

Record of architecture decisions. Each ADR documents a decision,
its context, and its consequences — including *open* decisions.

Convention: `NNNN-name.md`. Status: **Accepted**, **Proposed** (open),
**Superseded**.

| ADR | Title | Status |
|-----|--------|--------|
| 0001 | Worldgen source (1:1 vanilla) | **Accepted** — own core (pure Rust), no cubiomes/Pumpkin |
| 0002 | Plugin runtime: WASM/WIT (wasmtime) | Accepted |
| 0003 | No JVM bridge (do not embed Java) | Accepted (core) — extended by 0006 |
| 0004 | Multi-threaded region ticking | Accepted |
| 0005 | Data → codegen pipeline for the protocol | Accepted |
| 0006 | Java plugin compatibility: TeaVM → JVM | Accepted (strategic) |
| 0007 | Version strategy: 26.2 first, multi-version later | Accepted |
