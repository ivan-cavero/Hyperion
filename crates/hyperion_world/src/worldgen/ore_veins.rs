//! Large copper/iron ore veins (`OreVeinifier` from JE noise generation).
//!
//! Uses router densities `vein_toggle`, `vein_ridged`, `vein_gap` when present.
//! Applied to solid stone/deepslate after density fill, before surface rules.

use crate::chunk::{BlockState, ChunkColumn};
use crate::worldgen::density::{DensityContext, DensityFunction, DensityLibrary};
use crate::worldgen::noise_settings::NoiseSettings;
use crate::worldgen::random::{PositionalRandomFactory, RandomSource};

const VEININESS_THRESHOLD: f64 = 0.4;
const EDGE_ROUNDOFF_BEGIN: f64 = 20.0;
const MAX_EDGE_ROUNDOFF: f64 = 0.2;
const VEIN_SOLIDNESS: f32 = 0.7;
const MIN_RICHNESS: f64 = 0.1;
const MAX_RICHNESS: f64 = 0.3;
const MAX_RICHNESS_THRESHOLD: f64 = 0.6;
const CHANCE_OF_RAW_ORE: f32 = 0.02;
const SKIP_ORE_IF_GAP_BELOW: f64 = -0.3;

struct VeinType {
    ore: BlockState,
    raw: BlockState,
    filler: BlockState,
    min_y: i32,
    max_y: i32,
}

fn copper() -> VeinType {
    VeinType {
        ore: BlockState::new("minecraft:copper_ore"),
        raw: BlockState::new("minecraft:raw_copper_block"),
        filler: BlockState::new("minecraft:granite"),
        min_y: 0,
        max_y: 50,
    }
}

fn iron() -> VeinType {
    VeinType {
        ore: BlockState::new("minecraft:deepslate_iron_ore"),
        raw: BlockState::new("minecraft:raw_iron_block"),
        filler: BlockState::new("minecraft:tuff"),
        min_y: -60,
        max_y: -8,
    }
}

/// Apply large ore veins when `ore_veins_enabled` and router fields exist.
pub fn apply_ore_veins(
    column: &mut ChunkColumn,
    seed: i64,
    settings: &NoiseSettings,
    lib: &mut DensityLibrary,
) -> Result<(), String> {
    if !settings.ore_veins_enabled {
        return Ok(());
    }
    let r = &settings.noise_router;
    let Some(toggle_v) = r.get("vein_toggle") else {
        return Ok(());
    };
    let Some(ridged_v) = r.get("vein_ridged") else {
        return Ok(());
    };
    let Some(gap_v) = r.get("vein_gap") else {
        return Ok(());
    };
    let toggle = lib.resolve(toggle_v)?;
    let ridged = lib.resolve(ridged_v)?;
    let gap = lib.resolve(gap_v)?;
    let factory = PositionalRandomFactory::from_world_seed(seed);

    // Only copper (0..=50) and iron (-60..=-8) bands — skip empty sky/deep void.
    let bands = [(-60i32, -8i32), (0i32, 50i32)];
    let base_x = column.x * 16;
    let base_z = column.z * 16;

    for z in 0..16 {
        for x in 0..16 {
            let wx = base_x + x;
            let wz = base_z + z;
            for &(y0, y1) in &bands {
                for y in y0..=y1 {
                    let b = column.get_block(wx, y, wz);
                    if !is_vein_host(&b) {
                        continue;
                    }
                    if let Some(placed) =
                        compute_vein_block(wx, y, wz, &toggle, &ridged, &gap, &factory, lib)?
                    {
                        column.set_block(wx, y, wz, placed);
                    }
                }
            }
        }
    }
    Ok(())
}

fn is_vein_host(b: &BlockState) -> bool {
    matches!(
        b.name.as_str(),
        "minecraft:stone"
            | "minecraft:deepslate"
            | "minecraft:tuff"
            | "minecraft:granite"
            | "minecraft:diorite"
            | "minecraft:andesite"
    )
}

#[allow(clippy::too_many_arguments)]
fn compute_vein_block(
    x: i32,
    y: i32,
    z: i32,
    toggle: &DensityFunction,
    ridged: &DensityFunction,
    gap: &DensityFunction,
    factory: &PositionalRandomFactory,
    lib: &mut DensityLibrary,
) -> Result<Option<BlockState>, String> {
    let ctx = DensityContext::new(x, y, z);
    let veininess = toggle.compute(ctx, &mut lib.noises)?;
    // Positive toggle → copper band; negative → iron band (JE).
    let vein = if veininess > 0.0 { copper() } else { iron() };
    if y < vein.min_y || y > vein.max_y {
        return Ok(None);
    }
    let abs_v = veininess.abs();
    let dist_from_edge = (vein.max_y - y).min(y - vein.min_y);
    let edge_roundoff = clamped_map(
        f64::from(dist_from_edge),
        0.0,
        EDGE_ROUNDOFF_BEGIN,
        -MAX_EDGE_ROUNDOFF,
        0.0,
    );
    if abs_v + edge_roundoff < VEININESS_THRESHOLD {
        return Ok(None);
    }

    let mut random = factory.at(x, y, z);
    // VEIN_SOLIDNESS: skip if nextFloat > 0.7
    if random.next_float() > VEIN_SOLIDNESS {
        return Ok(None);
    }

    let ridged_v = ridged.compute(ctx, &mut lib.noises)?;
    if ridged_v >= 0.0 {
        return Ok(None);
    }

    let richness = clamped_map(
        abs_v,
        VEININESS_THRESHOLD,
        MAX_RICHNESS_THRESHOLD,
        MIN_RICHNESS,
        MAX_RICHNESS,
    );
    let roll = f64::from(random.next_float());
    if roll < richness {
        let gap_v = gap.compute(ctx, &mut lib.noises)?;
        if gap_v > SKIP_ORE_IF_GAP_BELOW {
            if random.next_float() < CHANCE_OF_RAW_ORE {
                return Ok(Some(vein.raw));
            }
            return Ok(Some(vein.ore));
        }
    }
    Ok(Some(vein.filler))
}

#[inline]
fn clamped_map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    if (from_max - from_min).abs() < f64::EPSILON {
        return to_min;
    }
    let t = ((value - from_min) / (from_max - from_min)).clamp(0.0, 1.0);
    to_min + t * (to_max - to_min)
}

/// Scatter common feature ores (post-carver approximation of placed_feature ores).
pub fn apply_scatter_ores(column: &mut ChunkColumn, seed: i64, min_y: i32) {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    let mut rng = factory.from_hash_of(&format!(
        "minecraft:ore_scatter/{}/{}",
        column.x, column.z
    ));
    let base_x = column.x * 16;
    let base_z = column.z * 16;

    // (block, attempts, y_min, y_max) — rough JE distribution counts.
    let specs: &[(&str, i32, i32, i32)] = &[
        ("minecraft:coal_ore", 20, min_y, 192),
        ("minecraft:iron_ore", 20, min_y, 72),
        ("minecraft:copper_ore", 16, min_y, 112),
        ("minecraft:gold_ore", 4, min_y, 32),
        ("minecraft:redstone_ore", 8, min_y, 16),
        ("minecraft:lapis_ore", 4, min_y, 64),
        ("minecraft:diamond_ore", 3, min_y, 16),
        ("minecraft:emerald_ore", 2, -16, 256),
    ];

    for &(name, attempts, y0, y1) in specs {
        let span = (y1 - y0).max(1);
        for _ in 0..attempts {
            let x = base_x + rng.next_int(16);
            let z = base_z + rng.next_int(16);
            let y = y0 + rng.next_int(span);
            let host = column.get_block(x, y, z);
            if !is_vein_host(&host) {
                continue;
            }
            // Prefer deepslate variants below y=0.
            let block = if y < 0 && name.ends_with("_ore") && !name.contains("deepslate") {
                let deep = name.replacen("minecraft:", "minecraft:deepslate_", 1);
                BlockState::new(deep)
            } else {
                BlockState::new(name)
            };
            // Small blob 1-4 cells.
            let size = 1 + rng.next_int(4);
            for _ in 0..size {
                let dx = rng.next_int(3) - 1;
                let dy = rng.next_int(3) - 1;
                let dz = rng.next_int(3) - 1;
                let px = x + dx;
                let py = y + dy;
                let pz = z + dz;
                if px < base_x || px >= base_x + 16 || pz < base_z || pz >= base_z + 16 {
                    continue;
                }
                if is_vein_host(&column.get_block(px, py, pz)) {
                    column.set_block(px, py, pz, block.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::chunk_fill::generate_column_from_density;
    use crate::worldgen::datapack::{
        default_server_inner_jar, find_workspace_root, load_overworld_from_jar,
    };

    #[test]
    fn clamped_map_endpoints() {
        assert!((clamped_map(0.0, 0.0, 20.0, -0.2, 0.0) + 0.2).abs() < 1e-9);
        assert!(clamped_map(20.0, 0.0, 20.0, -0.2, 0.0).abs() < 1e-9);
        assert!(clamped_map(10.0, 0.0, 20.0, -0.2, 0.0).abs() < 0.11);
    }

    #[test]
    fn scatter_places_some_ore_on_flat_column() {
        let v = serde_json::json!({
            "sea_level": 63,
            "aquifers_enabled": false,
            "ore_veins_enabled": false,
            "legacy_random_source": false,
            "default_block": { "Name": "minecraft:stone" },
            "default_fluid": { "Name": "minecraft:water" },
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
        let mut col = generate_column_from_density(7, 0, 0, &settings, &mut lib).unwrap();
        apply_scatter_ores(&mut col, 7, -64);
        let mut found = false;
        for y in -64..100 {
            let n = col.get_block(4, y, 4).name;
            if n.contains("ore") {
                found = true;
                break;
            }
        }
        // Search whole column
        if !found {
            for z in 0..16 {
                for x in 0..16 {
                    for y in -64..80 {
                        if col.get_block(x, y, z).name.contains("ore") {
                            found = true;
                            break;
                        }
                    }
                }
            }
        }
        assert!(found, "expected scatter ores in stone column");
    }

    #[test]
    fn ore_veins_resolve_if_jar_present() {
        let Some(root) = find_workspace_root() else {
            return;
        };
        let jar = default_server_inner_jar(&root);
        if !jar.is_file() {
            return;
        }
        let (settings, mut lib) = load_overworld_from_jar(&jar, 1).unwrap();
        assert!(settings.ore_veins_enabled);
        let col = generate_column_from_density(1, 0, 0, &settings, &mut lib).unwrap();
        // Veins may or may not hit chunk 0,0; just ensure generation completed.
        assert_eq!(col.sections.len(), 24);
    }
}
