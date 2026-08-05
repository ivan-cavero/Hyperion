# Worldgen — default behaviour = 1:1 with Java Edition

Hyperion is a **Minecraft Java server**. The default world for a given seed
must match the **official server** block by block (26.2 first). That is the
product, not an optional mode.

Research sources (we reverse-engineer and reimplement; we do not ship GPL code):

- Mojang datapack JSON in `server-inner-*.jar` (`noise_settings`, density, noise, biomes, …)
- Official data generators / reports under `tools/mc-ref/datagen/`
- Public protocol and observed client/server behaviour
- Independent analysis online when useful (always verify against the jar)

## Decision (ADR 0001)

- **Own core in pure Rust** (MIT).
- No cubiomes FFI, no Pumpkin/GPL **code**.
- **Learning from other Rust servers is encouraged**: study architecture and
  algorithms (e.g. Pumpkin’s `pumpkin-world/src/generation/noise/router/` layout:
  density functions → noise router → aquifers/ores → surface → features). Reimplement
  under MIT; never vendor GPL sources.
- Verification = **diff against the official server** (same seed → same chunk).

## Layout in code

| Path | Role |
|------|------|
| `worldgen::{random, improved_noise, perlin_noise, normal_noise, density}` | **Default generator math** — reimplementation of Java Edition algorithms |
| `worldgen::scaffold` | **Temporary** Play hills until density fill lands — **not** default long-term |

Scaffold exists only so multiplayer works while we build the real pipeline.
It will be **removed** (or demoted to a debug flat preset) once the density
router produces joinable, testable terrain.

## Default pipeline (target)

```
level.dat seed
    → WorldgenRandom (Xoroshiro for overworld; legacy_random_source=false)
    → NoiseRouter (density graph from noise_settings + density_function JSON)
    → final_density / aquifers / ore veins
    → block fill (default_block / default_fluid + surface_rule)
    → biomes (multi-noise climate)
    → carvers → features → structures
    → Anvil column + heightmaps
```

Datapack inputs (from the official jar):

- `worldgen/noise_settings/overworld.json`
- `worldgen/density_function/**`
- `worldgen/noise/**`
- biomes, surface rules, carvers, features, structures — later layers

## Layer checklist

Declare “matches official server” **only** when golden diffs are green.

| Layer | Status |
|-------|--------|
| Multi-palette chunks + block ids | ✅ |
| Scaffold hills (Play only) | ✅ temporary |
| Xoroshiro / Legacy RNG | 🔄 foundation |
| ImprovedNoise / Perlin / NormalNoise | 🔄 foundation |
| Density AST + library + string refs | 🔄 expanding |
| `NoiseSettings` + fill from `final_density` | 🔄 cell-grid sample + trilinear; overworld resolves + fills |
| Overworld graph types (spline, old_blended, interval_select, …) | 🔄 implemented enough to evaluate/fill; noise tables not golden-matched yet |
| Golden parity vs official server dump | ⬜ Hyperion-stable fingerprints only (need official dumps) |
| Surface rules | 🔄 basic bedrock + grass/dirt (not full JSON tree) |
| Multi-noise biomes | ⬜ |
| Aquifers / carvers / ores | ⬜ |
| Features / structures | ⬜ |
| CI golden chunks vs official dump | ⬜ Phase 2 exit |

## How we prove 1:1

1. Dump reference columns from an **official** 26.2 server (fixed seed + coords).
2. Fixtures under tests (or CI job with the jar).
3. `assert_eq!(hyperion, official)` per layer as it lands.
4. Research online / decompile **only** to learn algorithms; parity is proven by diffs.

Until a layer is green: say **pre-alpha / scaffold**, never “full vanilla worlds”.

## Play integration

- Today: scaffold columns.
- Switch default Play to density fill when: router produces terrain **and** a
  smoke golden test for seed + chunk (0,0) exists.

## Regenerating data extracts

```powershell
cargo run -p hyperion_tools --bin gen-mc-ref
```

JSON from Mojang is the intermediate format; that is fine and intentional.
