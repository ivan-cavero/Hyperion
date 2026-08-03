# ADR 0001 — Worldgen source (1:1 vanilla parity)

- Status: **Proposed** (open decision — deadline: Phase 2 of the ROADMAP)
- Date: 2026-08-02

## Context

Hyperion promises "same seed, same world" (1:1 with Mojang's vanilla server).
Implementing full block-level generation (noise, biomes, surface, caves, ores,
trees, structures/jigsaw) is the highest-risk component of the project: the best
native server (Pumpkin) has spent 2 years and still has not completed structures.

## Options

| Option | Description | Pros | Cons |
|--------|-------------|------|---------|
| **A. Own core** | Implement the full generator from scratch in Rust | Full control, zero third-party debt, maximum learning | Years of work; high risk |
| **B. cubiomes reference** | Use `cubiomes` (C, **MIT**) via FFI for biomes/structures + own terrain core | Proven biome parity, MIT-compatible | FFI `unsafe`; cubiomes does not cover block-by-block; terrain layer remains own |
| **C. Inherit from Pumpkin** | Port/inherit Pumpkin's generator (Rust, claimed 1:1) | Less work; partial parity already | **GPLv3** — forces GPL license on the whole project; depends on their roadmap |

## Decision (pending)

A vs B will be evaluated as the first options. C only if the project moves to GPL.
Closing criterion: decide at the start of Phase 2 with a PoC of each option
(reference chunk vs. vanilla, same seed).

## Consequences

- ADR 0004 (region ticking) assumes parallel chunk generation → the chosen
  option must be thread-safe and parallelizable.
