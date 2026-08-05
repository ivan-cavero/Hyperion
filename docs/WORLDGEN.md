# Worldgen — path to vanilla 1:1

Hyperion’s product promise: **same seed, same world, block by block** as
Java Edition (26.2 first). That is a multi-layer project, not a single PR.

## Decision (ADR 0001)

- **Own core in pure Rust** (MIT).
- No cubiomes FFI, no Pumpkin/GPL inheritance.
- Verification = **diff against vanilla** (same seed → same chunk), not “looks nice”.

## Two generators in the tree

| Path | What it is | When used |
|------|------------|-----------|
| `worldgen::scaffold` | Hyperion hills (value noise + stone/dirt/grass) | **Play today** — so clients explore non-flat land |
| `worldgen::vanilla` | Mojang-shaped RNG / Perlin / density AST | **Parity work** — not yet filling chunks |

Scaffold **must not** be marketed as 1:1. Same seed is only guaranteed to
match **our** scaffold, not Mojang.

## Vanilla pipeline (target architecture)

```
level.dat seed
    → WorldgenRandom (Xoroshiro; overworld legacy_random_source=false)
    → NoiseRouter (density function graph from noise_settings + density_function JSON)
    → final_density / aquifers / ore veins
    → block state fill (default_block / default_fluid + surface_rule)
    → biomes (multi-noise climate from temperature/vegetation/… router outputs)
    → carvers → features → structures
    → Anvil column + heightmaps
```

Datapack sources (from `server-inner-26.2.jar`):

- `worldgen/noise_settings/overworld.json` — router + surface_rule + sea_level
- `worldgen/density_function/**` — named density graph nodes
- `worldgen/noise/**` — NormalNoise parameters (`firstOctave`, `amplitudes`)
- `worldgen/biome/**` + biome parameter lists — multi-noise biomes
- Surface rules, carvers, features, structures — later layers

## Layer checklist (declare parity only when green)

| Layer | Status | Claim |
|-------|--------|--------|
| Multi-palette chunks + block ids | ✅ | Storage/network |
| Scaffold surface (Hyperion) | ✅ | Not vanilla |
| Xoroshiro / Legacy RNG | 🔄 foundation | Deterministic streams |
| ImprovedNoise / Perlin / NormalNoise | 🔄 foundation | Math only |
| Density AST (add/mul/y_gradient/noise/…) | 🔄 partial | No full router |
| NoiseRouter + chunk fill from `final_density` | ⬜ | — |
| Surface rules | ⬜ | — |
| Multi-noise biomes | ⬜ | — |
| Aquifers / carvers / ores | ⬜ | — |
| Features / structures | ⬜ | — |
| CI golden chunks vs vanilla dump | ⬜ | Exit criterion Phase 2 |

## How we will prove 1:1

1. Dump reference columns from a vanilla 26.2 server (same seed, known chunk coords).
2. Store as fixtures under `tests/fixtures/worldgen/` (or generate in CI with a jar job).
3. `assert_eq!(hyperion_column.blocks, vanilla_fixture.blocks)` per layer.
4. Expand fixtures as each layer claims parity.

Until the golden suite is green for a layer, docs and MOTD must say
**pre-alpha / scaffold**, not “vanilla worldgen”.

## Play integration rule

- Default Play path: **scaffold** (current).
- Switch to vanilla columns only when router fill produces joinable terrain
  and at least a smoke golden test exists for seed + chunk (0,0).

## Regenerating datapack inputs

```powershell
# After placing server-26.2.jar / server-inner-26.2.jar (see tools/mc-ref/README.md)
cargo run -p hyperion_tools --bin gen-mc-ref   # blocks + join data
# Worldgen JSON is read from the jar / reports as the vanilla path matures.
```

JSON as an intermediate format is intentional (Mojang’s own data model).
