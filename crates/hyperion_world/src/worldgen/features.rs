//! Vegetation features: trees + ground plants (JE FEATURES step, simplified).
//!
//! Not a full datapack feature interpreter — places biome-appropriate trees and
//! plants deterministically so worlds look alive. Expand toward placed_feature
//! JSON parity with golden dumps.

use crate::chunk::{BlockState, ChunkColumn, MIN_SECTION_Y};
use crate::worldgen::random::{PositionalRandomFactory, RandomSource, XoroshiroRandom};

/// Apply trees and plants after ores (FEATURES pipeline slot).
pub fn apply_vegetation(column: &mut ChunkColumn, seed: i64, biome: &str, sea_level: i32) {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    let mut rng = factory.from_hash_of(&format!(
        "minecraft:vegetation/{}/{}",
        column.x, column.z
    ));

    place_plants(column, &mut rng, biome, sea_level);
    place_trees(column, &mut rng, biome, sea_level);
}

fn place_plants(
    column: &mut ChunkColumn,
    rng: &mut XoroshiroRandom,
    biome: &str,
    sea_level: i32,
) {
    let base_x = column.x * 16;
    let base_z = column.z * 16;
    let desert = biome.contains("desert") || biome.contains("badlands");
    let ocean = biome.contains("ocean") || biome.contains("river") || biome.contains("beach");
    let snowy = biome.contains("snowy") || biome.contains("frozen") || biome.contains("ice");
    let swamp = biome.contains("swamp");
    let mushroom = biome.contains("mushroom");

    let grass_attempts = if desert || ocean {
        2
    } else if swamp {
        48
    } else {
        24
    };
    let flower_attempts = if desert || ocean || snowy { 1 } else { 8 };

    // Grass / fern / short plants
    for _ in 0..grass_attempts {
        let x = base_x + rng.next_int(16);
        let z = base_z + rng.next_int(16);
        let Some(sy) = surface_solid_y(column, x, z) else {
            continue;
        };
        if sy < sea_level - 1 && !swamp {
            continue;
        }
        let ground = column.get_block(x, sy, z);
        if !is_plantable_ground(&ground) {
            continue;
        }
        let above = column.get_block(x, sy + 1, z);
        if !above.is_air() {
            continue;
        }
        if desert {
            if rng.next_int(5) == 0 {
                column.set_block(x, sy + 1, z, BlockState::new("minecraft:dead_bush"));
            } else if rng.next_int(8) == 0 {
                place_cactus(column, x, sy + 1, z, rng);
            }
        } else if mushroom {
            let m = if rng.next_int(2) == 0 {
                "minecraft:brown_mushroom"
            } else {
                "minecraft:red_mushroom"
            };
            column.set_block(x, sy + 1, z, BlockState::new(m));
        } else if snowy {
            if rng.next_int(3) == 0 {
                column.set_block(x, sy + 1, z, BlockState::new("minecraft:short_grass"));
            }
        } else {
            let plant = if rng.next_int(8) == 0 {
                "minecraft:fern"
            } else if rng.next_int(12) == 0 && !swamp {
                "minecraft:tall_grass" // may need 2-high; place short if fails
            } else {
                "minecraft:short_grass"
            };
            if plant == "minecraft:tall_grass" {
                if column.get_block(x, sy + 2, z).is_air() {
                    column.set_block(x, sy + 1, z, BlockState::new("minecraft:tall_grass"));
                    // upper half often separate state; use second tall grass as approx
                    column.set_block(x, sy + 2, z, BlockState::new("minecraft:tall_grass"));
                } else {
                    column.set_block(x, sy + 1, z, BlockState::new("minecraft:short_grass"));
                }
            } else {
                column.set_block(x, sy + 1, z, BlockState::new(plant));
            }
        }
    }

    // Flowers
    for _ in 0..flower_attempts {
        let x = base_x + rng.next_int(16);
        let z = base_z + rng.next_int(16);
        let Some(sy) = surface_solid_y(column, x, z) else {
            continue;
        };
        if sy < sea_level {
            continue;
        }
        if !is_plantable_ground(&column.get_block(x, sy, z)) {
            continue;
        }
        if !column.get_block(x, sy + 1, z).is_air() {
            continue;
        }
        let flower = pick_flower(biome, rng);
        column.set_block(x, sy + 1, z, BlockState::new(flower));
    }

    // Sugar cane near water
    if !desert && !snowy {
        for _ in 0..6 {
            let x = base_x + rng.next_int(16);
            let z = base_z + rng.next_int(16);
            let Some(sy) = surface_solid_y(column, x, z) else {
                continue;
            };
            if !adjacent_water(column, x, sy, z) {
                continue;
            }
            if !column.get_block(x, sy + 1, z).is_air() {
                continue;
            }
            let h = 1 + rng.next_int(3);
            for dy in 1..=h {
                if column.get_block(x, sy + dy, z).is_air() {
                    column.set_block(x, sy + dy, z, BlockState::new("minecraft:sugar_cane"));
                } else {
                    break;
                }
            }
        }
    }
}

fn place_cactus(column: &mut ChunkColumn, x: i32, y: i32, z: i32, rng: &mut XoroshiroRandom) {
    let h = 1 + rng.next_int(3);
    for dy in 0..h {
        let yy = y + dy;
        if !column.get_block(x, yy, z).is_air() {
            break;
        }
        // Cactus needs air on sides — skip strict check for now.
        column.set_block(x, yy, z, BlockState::new("minecraft:cactus"));
    }
}

fn place_trees(
    column: &mut ChunkColumn,
    rng: &mut XoroshiroRandom,
    biome: &str,
    sea_level: i32,
) {
    let density = tree_attempts(biome);
    if density == 0 {
        return;
    }
    let base_x = column.x * 16;
    let base_z = column.z * 16;
    let kind = tree_kind(biome);

    for _ in 0..density {
        // plains: rare trees (~1/20 chance of one attempt succeeding)
        if biome.contains("plains") && rng.next_int(20) != 0 {
            continue;
        }
        let x = base_x + 2 + rng.next_int(12);
        let z = base_z + 2 + rng.next_int(12);
        let Some(sy) = surface_solid_y(column, x, z) else {
            continue;
        };
        if sy < sea_level {
            continue;
        }
        let ground = column.get_block(x, sy, z);
        if !is_plantable_ground(&ground) && ground.name != "minecraft:podzol" {
            continue;
        }
        if !column.get_block(x, sy + 1, z).is_air() {
            continue;
        }
        grow_tree(column, x, sy + 1, z, kind, rng);
    }
}

#[derive(Clone, Copy)]
enum TreeKind {
    Oak,
    Birch,
    Spruce,
    Jungle,
    Acacia,
    DarkOak,
}

fn tree_kind(biome: &str) -> TreeKind {
    if biome.contains("birch") {
        TreeKind::Birch
    } else if biome.contains("taiga") || biome.contains("grove") || biome.contains("snowy") {
        TreeKind::Spruce
    } else if biome.contains("jungle") {
        TreeKind::Jungle
    } else if biome.contains("savanna") {
        TreeKind::Acacia
    } else if biome.contains("dark_forest") {
        TreeKind::DarkOak
    } else {
        TreeKind::Oak
    }
}

fn tree_attempts(biome: &str) -> i32 {
    if biome.contains("ocean")
        || biome.contains("river")
        || biome.contains("desert")
        || biome.contains("beach")
        || biome.contains("badlands")
    {
        0
    } else if biome.contains("dark_forest") {
        8
    } else if biome.contains("forest") || biome.contains("taiga") || biome.contains("jungle") {
        6
    } else if biome.contains("swamp") {
        3
    } else if biome.contains("plains") || biome.contains("meadow") || biome.contains("savanna") {
        2
    } else {
        2
    }
}

fn grow_tree(
    column: &mut ChunkColumn,
    x: i32,
    y: i32,
    z: i32,
    kind: TreeKind,
    rng: &mut XoroshiroRandom,
) {
    let (log, leaves, trunk_h) = match kind {
        TreeKind::Oak => (
            "minecraft:oak_log",
            "minecraft:oak_leaves",
            4 + rng.next_int(3),
        ),
        TreeKind::Birch => (
            "minecraft:birch_log",
            "minecraft:birch_leaves",
            5 + rng.next_int(2),
        ),
        TreeKind::Spruce => (
            "minecraft:spruce_log",
            "minecraft:spruce_leaves",
            6 + rng.next_int(4),
        ),
        TreeKind::Jungle => (
            "minecraft:jungle_log",
            "minecraft:jungle_leaves",
            6 + rng.next_int(5),
        ),
        TreeKind::Acacia => (
            "minecraft:acacia_log",
            "minecraft:acacia_leaves",
            5 + rng.next_int(2),
        ),
        TreeKind::DarkOak => (
            "minecraft:dark_oak_log",
            "minecraft:dark_oak_leaves",
            5 + rng.next_int(2),
        ),
    };

    // Clear check: trunk space
    for dy in 0..trunk_h {
        if !column.get_block(x, y + dy, z).is_air()
            && !column.get_block(x, y + dy, z).name.contains("leaves")
            && !column.get_block(x, y + dy, z).name.contains("grass")
        {
            return;
        }
    }

    // Dirt under tree
    let below = column.get_block(x, y - 1, z);
    if below.name.contains("grass") {
        column.set_block(x, y - 1, z, BlockState::dirt());
    }

    for dy in 0..trunk_h {
        column.set_block(x, y + dy, z, BlockState::new(log));
    }

    // Leaf canopy
    let top = y + trunk_h;
    let r = match kind {
        TreeKind::Spruce => 2,
        TreeKind::Jungle => 3,
        TreeKind::DarkOak => 3,
        _ => 2,
    };
    for dy in -2..=1 {
        let layer_r = if dy >= 0 { r - 1 } else { r };
        for dz in -layer_r..=layer_r {
            for dx in -layer_r..=layer_r {
                if dx * dx + dz * dz > layer_r * layer_r + 1 {
                    continue;
                }
                if dx == 0 && dz == 0 && dy < 0 {
                    continue; // trunk
                }
                let lx = x + dx;
                let ly = top + dy;
                let lz = z + dz;
                let cur = column.get_block(lx, ly, lz);
                if cur.is_air() || cur.name.contains("leaves") || cur.name.contains("grass") {
                    // Skip corner thinning
                    if dx.abs() == layer_r && dz.abs() == layer_r && rng.next_int(2) == 0 {
                        continue;
                    }
                    column.set_block(lx, ly, lz, BlockState::new(leaves));
                }
            }
        }
    }
    // Top leaf
    if column.get_block(x, top + 1, z).is_air() {
        column.set_block(x, top + 1, z, BlockState::new(leaves));
    }
}

fn pick_flower(biome: &str, rng: &mut XoroshiroRandom) -> &'static str {
    if biome.contains("flower") || biome.contains("meadow") {
        const M: &[&str] = &[
            "minecraft:dandelion",
            "minecraft:poppy",
            "minecraft:allium",
            "minecraft:azure_bluet",
            "minecraft:oxeye_daisy",
            "minecraft:cornflower",
        ];
        return M[rng.next_int(M.len() as i32) as usize];
    }
    if biome.contains("swamp") {
        return "minecraft:blue_orchid";
    }
    if rng.next_int(2) == 0 {
        "minecraft:dandelion"
    } else {
        "minecraft:poppy"
    }
}

fn is_plantable_ground(b: &BlockState) -> bool {
    matches!(
        b.name.as_str(),
        "minecraft:grass_block"
            | "minecraft:dirt"
            | "minecraft:coarse_dirt"
            | "minecraft:podzol"
            | "minecraft:mycelium"
            | "minecraft:mud"
            | "minecraft:rooted_dirt"
            | "minecraft:moss_block"
            | "minecraft:sand"
            | "minecraft:red_sand"
            | "minecraft:snow_block"
    )
}

fn adjacent_water(column: &ChunkColumn, x: i32, y: i32, z: i32) -> bool {
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let b = column.get_block(x + dx, y, z + dz);
        if b.name.contains("water") {
            return true;
        }
    }
    false
}

/// Highest non-air, non-fluid solid block Y in column, or None.
pub fn surface_solid_y(column: &ChunkColumn, x: i32, z: i32) -> Option<i32> {
    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(crate::chunk::MAX_SECTION_Y) + 1) * 16 - 1;
    for y in (min_block..=max_block).rev() {
        let b = column.get_block(x, y, z);
        if !b.is_air() && !b.is_fluid() {
            return Some(y);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::chunk_fill::generate_column_from_density;
    use crate::worldgen::density::DensityLibrary;
    use crate::worldgen::noise_settings::NoiseSettings;

    fn flat_col(seed: i64) -> ChunkColumn {
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
        let mut lib = DensityLibrary::new(seed);
        generate_column_from_density(seed, 0, 0, &settings, &mut lib).unwrap()
    }

    #[test]
    fn forest_gets_logs_or_leaves() {
        let mut col = flat_col(99);
        apply_vegetation(&mut col, 99, "minecraft:forest", 63);
        let mut found = false;
        for z in 0..16 {
            for x in 0..16 {
                for y in 120..140 {
                    let n = col.get_block(x, y, z).name;
                    if n.contains("log") || n.contains("leaves") {
                        found = true;
                        break;
                    }
                }
            }
        }
        assert!(found, "expected tree blocks in forest vegetation pass");
    }

    #[test]
    fn plains_gets_some_grass_or_flower() {
        let mut col = flat_col(3);
        apply_vegetation(&mut col, 3, "minecraft:plains", 63);
        let mut found = false;
        for z in 0..16 {
            for x in 0..16 {
                for y in 125..132 {
                    let n = col.get_block(x, y, z).name;
                    if n.contains("grass") || n.contains("poppy") || n.contains("dandelion") {
                        found = true;
                        break;
                    }
                }
            }
        }
        assert!(found, "expected plants on plains");
    }
}
