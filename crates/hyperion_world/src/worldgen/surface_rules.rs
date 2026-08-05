//! Minimal surface-rule pass after density fill.
//!
//! Full surface_rule trees (biome, stone_depth, noise, …) land layer by layer.
//! This module implements the pieces needed for a recognizable overworld top:
//! - bedrock floor gradient near `min_y`
//! - deepslate band below Y=0 (stone → deepslate)
//! - grass + dirt + stone cap on the highest solid blocks of each column
//!
//! That matches the *role* of surface rules after `final_density` (wiki), not
//! yet every JSON node in `noise_settings.surface_rule`.

use crate::chunk::{BlockState, ChunkColumn, MIN_SECTION_Y};
use crate::worldgen::random::{PositionalRandomFactory, RandomSource};

/// Y at and below which solid stone becomes deepslate (overworld convention).
const DEEPSLATE_Y: i32 = 0;

/// Apply post-density surface dressing in place.
///
/// `seed` drives the bedrock vertical_gradient (JE `minecraft:bedrock_floor`).
/// Optional datapack `surface_rule` is evaluated for known nodes (fail-closed).
pub fn apply_basic_surface(
    column: &mut ChunkColumn,
    sea_level: i32,
    min_y: i32,
    seed: i64,
    surface_rule: Option<&serde_json::Value>,
) {
    apply_bedrock_floor(column, min_y, seed);
    apply_deepslate_band(column, min_y);
    apply_surface_layers(column, sea_level);
    if let Some(rule) = surface_rule {
        apply_json_surface_pass(column, sea_level, min_y, seed, rule);
    }
    recompute_heightmap(column);
}

/// Second pass: try datapack surface_rule on solid cells (bedrock / simple leaves).
fn apply_json_surface_pass(
    column: &mut ChunkColumn,
    sea_level: i32,
    min_y: i32,
    seed: i64,
    rule: &serde_json::Value,
) {
    use crate::worldgen::surface_rule_json::{SurfaceCtx, eval_rule};

    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(crate::chunk::MAX_SECTION_Y) + 1) * 16 - 1;
    for z in 0..16 {
        for x in 0..16 {
            let wx = column.x * 16 + x;
            let wz = column.z * 16 + z;
            let mut surface_y = min_block;
            for y in (min_block..=max_block).rev() {
                let b = column.get_block(wx, y, wz);
                if !b.is_air() && !b.is_fluid() {
                    surface_y = y;
                    break;
                }
            }
            for y in min_block..=surface_y {
                let b = column.get_block(wx, y, wz);
                if b.is_air() || b.is_fluid() {
                    continue;
                }
                let ctx = SurfaceCtx {
                    x: wx,
                    y,
                    z: wz,
                    min_y,
                    sea_level,
                    surface_y,
                    seed,
                };
                if let Some(placed) = eval_rule(rule, &ctx) {
                    // Only override with bedrock from JSON for now (avoid wiping grass
                    // until biome conditions work). Full rule tree later.
                    if placed == BlockState::bedrock() {
                        column.set_block(wx, y, wz, placed);
                    }
                }
            }
        }
    }
}

/// JE-ish `vertical_gradient` for bedrock:
/// always true at `min_y`, always false at `min_y+5`+, linear probability in between.
fn apply_bedrock_floor(column: &mut ChunkColumn, min_y: i32, seed: i64) {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    // Surface rule uses random_name "minecraft:bedrock_floor".
    let mut floor_rng = factory.from_hash_of("minecraft:bedrock_floor");
    // Derive a stable salt stream for (x,z,y) without full WorldgenRandom.at yet:
    // mix factory long with position for each cell.
    let salt = floor_rng.next_long();

    for z in 0..16 {
        for x in 0..16 {
            let wx = column.x * 16 + x;
            let wz = column.z * 16 + z;
            for dy in 0..5 {
                let y = min_y + dy;
                if dy == 0 {
                    column.set_block(wx, y, wz, BlockState::bedrock());
                    continue;
                }
                // Probability of bedrock decreases with height (true_at 0 → false_at 5).
                // p = 1 - dy/5
                let p = 1.0 - f64::from(dy) / 5.0;
                let h = mix64(salt, wx, y, wz);
                let unit = (h as f64) / (u64::MAX as f64);
                if unit < p {
                    column.set_block(wx, y, wz, BlockState::bedrock());
                }
            }
        }
    }
}

/// Replace stone with deepslate for Y < 0 (and Y==0 uses a soft edge like vanilla).
fn apply_deepslate_band(column: &mut ChunkColumn, min_y: i32) {
    for z in 0..16 {
        for x in 0..16 {
            let wx = column.x * 16 + x;
            let wz = column.z * 16 + z;
            for y in min_y..DEEPSLATE_Y {
                let b = column.get_block(wx, y, wz);
                if b == BlockState::stone() {
                    column.set_block(wx, y, wz, BlockState::deepslate());
                }
            }
            // Soft edge at Y=0: half the stone becomes deepslate (hash), similar to
            // vanilla's y-transition noise without full surface_rule graph yet.
            let b = column.get_block(wx, DEEPSLATE_Y, wz);
            if b == BlockState::stone() && mix(wx, DEEPSLATE_Y, wz).is_multiple_of(2) {
                column.set_block(wx, DEEPSLATE_Y, wz, BlockState::deepslate());
            }
        }
    }
}

fn apply_surface_layers(column: &mut ChunkColumn, sea_level: i32) {
    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(crate::chunk::MAX_SECTION_Y) + 1) * 16 - 1;
    for z in 0..16 {
        for x in 0..16 {
            let wx = column.x * 16 + x;
            let wz = column.z * 16 + z;
            // Find top non-air (ignore fluid for surface — treat water as empty for grass).
            let mut top = None;
            for y in (min_block..=max_block).rev() {
                let b = column.get_block(wx, y, wz);
                if !b.is_air() && !b.is_fluid() {
                    top = Some(y);
                    break;
                }
            }
            let Some(surface_y) = top else {
                continue;
            };
            // Underwater: dirt top; above sea: grass.
            if surface_y >= sea_level - 1 {
                column.set_block(wx, surface_y, wz, BlockState::grass_block());
                for d in 1..=3 {
                    let y = surface_y - d;
                    if y <= min_block {
                        break;
                    }
                    let b = column.get_block(wx, y, wz);
                    if b.is_air() || b.is_fluid() {
                        break;
                    }
                    if b == BlockState::stone()
                        || b == BlockState::dirt()
                        || b == BlockState::deepslate()
                    {
                        column.set_block(wx, y, wz, BlockState::dirt());
                    }
                }
            } else {
                column.set_block(wx, surface_y, wz, BlockState::dirt());
                for d in 1..=2 {
                    let y = surface_y - d;
                    if y <= min_block {
                        break;
                    }
                    let b = column.get_block(wx, y, wz);
                    if !b.is_air() && !b.is_fluid() {
                        column.set_block(wx, y, wz, BlockState::dirt());
                    }
                }
            }
        }
    }
}

fn recompute_heightmap(column: &mut ChunkColumn) {
    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(crate::chunk::MAX_SECTION_Y) + 1) * 16 - 1;
    let mut max_surface = min_block;
    for z in 0..16 {
        for x in 0..16 {
            let wx = column.x * 16 + x;
            let wz = column.z * 16 + z;
            let mut top = min_block;
            for y in (min_block..=max_block).rev() {
                if !column.get_block(wx, y, wz).is_air() {
                    top = y;
                    break;
                }
            }
            let surface = (top + 1).clamp(0, 511) as u16;
            column.heightmap[(z * 16 + x) as usize] = surface;
            max_surface = max_surface.max(top + 1);
        }
    }
    column.surface_y = max_surface;
}

fn mix(x: i32, y: i32, z: i32) -> u32 {
    mix64(0, x, y, z) as u32
}

fn mix64(salt: i64, x: i32, y: i32, z: i32) -> u64 {
    let mut n = salt as u64;
    n = n
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(x as u64);
    n ^= (y as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    n = n
        .wrapping_mul(0x94D0_49BB_1331_11EB)
        .wrapping_add(z as u64);
    n ^ (n >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::chunk_fill::generate_column_from_density;
    use crate::worldgen::density::DensityLibrary;
    use crate::worldgen::noise_settings::NoiseSettings;

    #[test]
    fn flat_density_plus_surface_has_grass() {
        let v = serde_json::json!({
            "sea_level": 63,
            "aquifers_enabled": false,
            "ore_veins_enabled": false,
            "legacy_random_source": false,
            "default_block": { "Name": "minecraft:stone" },
            "default_fluid": { "Name": "minecraft:water", "Properties": { "level": "0" } },
            "noise": { "min_y": -64, "height": 384, "size_horizontal": 1, "size_vertical": 2 },
            "noise_router": {
                "final_density": {
                    "type": "minecraft:y_clamped_gradient",
                    "from_y": -64,
                    "to_y": 320,
                    "from_value": 1.0,
                    "to_value": -1.0
                }
            }
        });
        let settings = NoiseSettings::from_json(&v).unwrap();
        let mut lib = DensityLibrary::new(0);
        let col = generate_column_from_density(0, 0, 0, &settings, &mut lib).unwrap();
        assert_eq!(col.get_block(0, -64, 0), BlockState::bedrock());
        let mut found_grass = false;
        for y in -64..200 {
            if col.get_block(0, y, 0) == BlockState::grass_block() {
                found_grass = true;
                break;
            }
        }
        assert!(found_grass, "expected grass after surface pass");
        // Deep below sea: stone became deepslate
        assert_eq!(col.get_block(0, -32, 0), BlockState::deepslate());
    }
}
