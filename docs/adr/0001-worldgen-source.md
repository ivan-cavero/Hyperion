# ADR 0001 — Worldgen source (1:1 vanilla parity)

- Status: **Accepted**
- Date: 2026-08-02
- Decided: 2026-08-05

## Context

Hyperion promises "same seed, same world" (1:1 with Mojang's vanilla server).
Implementing full block-level generation (noise, biomes, surface, caves, ores,
trees, structures/jigsaw) is the highest-risk component of the project: the best
native server (Pumpkin) has spent 2 years and still has not completed structures.

## Options

| Option | Description | Pros | Cons |
|--------|-------------|------|---------|
| **A. Own core** | Implement the full generator from scratch in Rust | Full control, zero third-party debt, maximum learning, MIT stays clean | Years of work; high risk |
| **B. cubiomes reference** | Use `cubiomes` (C, **MIT**) via FFI for biomes/structures + own terrain core | Proven biome parity, MIT-compatible | FFI `unsafe`; cubiomes does not cover block-by-block; terrain layer remains own |
| **C. Inherit from Pumpkin** | Port/inherit Pumpkin's generator (Rust, claimed 1:1) | Less work; partial parity already | **GPLv3** — forces GPL license on the whole project; depends on their roadmap |

## Decision

**Option A — own core in pure Rust.**

No cubiomes FFI and no Pumpkin inheritance for worldgen. Hyperion implements
noise, density functions, biomes, surface rules, features, and structures as
first-party code under MIT. Verification remains **diff against vanilla**
(same seed → same chunk) in CI as parity layers land.

### Why A

1. **License and control** — MIT stays non-negotiable; C/GPL paths create long-term debt.
2. **No `unsafe` worldgen path** — policy is zero FFI except audited one-offs; a C library in the hot path fights that.
3. **Learning and product identity** — "we own the generator" matches the rest of the stack (own NBT, own protocol codecs, own Anvil path).
4. **cubiomes is incomplete for block parity anyway** — even option B still requires a full own terrain/features stack; the hard work does not disappear.

### Explicit non-goals of this ADR

- Re-using Pumpkin worldgen code (rejected for GPLv3).
- Shipping cubiomes as a runtime dependency (rejected for FFI/`unsafe` and incomplete coverage).
- Academic study of cubiomes **algorithms** as external reading is fine; **vendoring or linking** it is not part of the design.

## Implementation strategy (phased parity)

Own core does not mean "everything day one". Layers ship with **declared** parity
only when golden diffs are green. Live doc: `docs/WORLDGEN.md`.

1. **Scaffold (done, temporary)** — flat then provisional hills for Play (explicitly **not** 1:1).
2. **Multi-palette sections + block-state registry (done)** — required before non-flat terrain.
3. **Default generator math (in progress)** — Xoroshiro/Legacy RNG, Perlin/NormalNoise, density AST (`worldgen::*`, no separate “vanilla” tree).
4. **NoiseRouter + `final_density` chunk fill** — first real 1:1 terrain claim candidate.
5. **Surface rules + multi-noise biomes** — climate from router outputs.
6. **Aquifers, carvers, ores** — underground structure of the column.
7. **Features** (trees, lakes, …) then **structures** (jigsaw last).
8. **Diff suite** — same seed vs vanilla dumps in CI (Phase 2 exit).

Structures and full jigsaw may lag other layers; the product bar is **declare parity by layer**, not silent half-parity.

## Consequences

- ADR 0004 (region ticking) assumes parallel chunk generation → the own core
  **must** be thread-safe and free of global mutable state on the gen path
  (seeded RNGs / pure functions per chunk or shared read-only density graphs).
- Phase 2 exit criterion ("same seed → same chunk") is owned entirely by Hyperion
  code + vanilla diff tests; no external gen oracle.
- Schedule risk stays **high** (see ROADMAP risks). Mitigation: hub/minigames and
  flat worlds remain valid product modes while gen layers mature.
- `docs/DEPENDENCIES.md`: cubiomes moves from "Phase 2 optional" to **not planned**.
- README/ARCHITECTURE stop advertising cubiomes as the reference path.
