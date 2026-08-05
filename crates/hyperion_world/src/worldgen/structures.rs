//! Structure starts (JE STRUCTURE_STARTS / STRUCTURE_REFERENCES, simplified).
//!
//! Full jigsaw/NBT templates land later. This places small deterministic
//! footprints so structure chunks exist in the world:
//! - ruined portal (obsidian frame + netherrack)
//! - desert pyramid stub (sandstone box)
//! - plains “house” village stub (planks + cobble)
//! - shipwreck stub (oak planks hull on beach/ocean surface)
//!
//! Placement uses structure-set style spacing/salt hashes (not full RarityFilter).

use crate::chunk::{BlockState, ChunkColumn, MIN_SECTION_Y};
use crate::worldgen::features::surface_solid_y;
use crate::worldgen::random::{PositionalRandomFactory, RandomSource, XoroshiroRandom, block_seed};

/// Apply structure stubs for this chunk if it is a start chunk for any set.
pub fn apply_structures(column: &mut ChunkColumn, seed: i64, biome: &str, sea_level: i32) {
    let cx = column.x;
    let cz = column.z;

    // Ruined portals — common-ish (spacing 40).
    if is_structure_chunk(seed, cx, cz, 40, 15, 3_429_021) {
        place_ruined_portal(column, seed, sea_level);
    }

    // Desert pyramid — desert biomes only, spacing 32.
    if biome.contains("desert") && is_structure_chunk(seed, cx, cz, 32, 8, 1_431_211) {
        place_desert_pyramid_stub(column, seed, sea_level);
    }

    // Village house — plains/savanna/taiga, spacing 34.
    if (biome.contains("plains") || biome.contains("savanna") || biome.contains("taiga"))
        && !biome.contains("snowy")
        && is_structure_chunk(seed, cx, cz, 34, 8, 1_031_731)
    {
        place_village_house(column, seed, sea_level);
    }

    // Shipwreck — ocean/beach, spacing 24.
    if (biome.contains("ocean") || biome.contains("beach"))
        && is_structure_chunk(seed, cx, cz, 24, 4, 1_658_457)
    {
        place_shipwreck_stub(column, seed, sea_level);
    }

    // Igloo — snowy, spacing 32.
    if biome.contains("snowy") && is_structure_chunk(seed, cx, cz, 32, 8, 1_431_376) {
        place_igloo_stub(column, seed, sea_level);
    }
}

/// JE-like structure set: one random start per spacing×spacing region.
fn is_structure_chunk(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    spacing: i32,
    separation: i32,
    salt: i32,
) -> bool {
    let spacing = spacing.max(1);
    let region_x = div_floor(chunk_x, spacing);
    let region_z = div_floor(chunk_z, spacing);
    let mut rng = structure_rng(seed, region_x, region_z, salt);
    let avail = (spacing - separation).max(1);
    let ox = rng.next_int(avail);
    let oz = rng.next_int(avail);
    let start_x = region_x * spacing + ox;
    let start_z = region_z * spacing + oz;
    chunk_x == start_x && chunk_z == start_z
}

fn structure_rng(seed: i64, region_x: i32, region_z: i32, salt: i32) -> XoroshiroRandom {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    // Mix region + salt like LegacyRandom structure placement spirit.
    let s = block_seed(region_x, salt, region_z) ^ seed;
    factory.from_seed_long(s)
}

fn div_floor(a: i32, b: i32) -> i32 {
    let d = a / b;
    let r = a % b;
    if r != 0 && ((r < 0) != (b < 0)) {
        d - 1
    } else {
        d
    }
}

fn place_ruined_portal(column: &mut ChunkColumn, seed: i64, sea_level: i32) {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    let mut rng = factory.from_hash_of(&format!("ruined_portal/{}/{}", column.x, column.z));
    let base_x = column.x * 16 + 4 + rng.next_int(6);
    let base_z = column.z * 16 + 4 + rng.next_int(6);
    let Some(sy) = surface_solid_y(column, base_x, base_z) else {
        return;
    };
    let y = sy.max(sea_level - 2);
    // Obsidian frame 4×5 incomplete (ruined).
    let ob = BlockState::new("minecraft:obsidian");
    let nr = BlockState::new("minecraft:netherrack");
    let cry = BlockState::new("minecraft:crying_obsidian");
    for dx in 0..4 {
        set_if_in_chunk(column, base_x + dx, y, base_z, ob.clone());
        set_if_in_chunk(column, base_x + dx, y, base_z + 4, ob.clone());
    }
    for dz in 1..4 {
        set_if_in_chunk(column, base_x, y, base_z + dz, ob.clone());
        set_if_in_chunk(column, base_x + 3, y, base_z + dz, ob.clone());
    }
    // Vertical pillars (broken)
    for dy in 1..=3 {
        if rng.next_int(3) != 0 {
            set_if_in_chunk(column, base_x, y + dy, base_z, ob.clone());
        }
        if rng.next_int(3) != 0 {
            set_if_in_chunk(column, base_x + 3, y + dy, base_z, ob.clone());
        }
    }
    set_if_in_chunk(column, base_x + 1, y + 1, base_z + 1, nr);
    if rng.next_int(2) == 0 {
        set_if_in_chunk(column, base_x + 2, y + 1, base_z + 2, cry);
    }
}

fn place_desert_pyramid_stub(column: &mut ChunkColumn, seed: i64, sea_level: i32) {
    let _ = seed;
    let base_x = column.x * 16 + 2;
    let base_z = column.z * 16 + 2;
    let Some(sy) = surface_solid_y(column, base_x + 4, base_z + 4) else {
        return;
    };
    let y = sy.max(sea_level);
    let sand = BlockState::new("minecraft:sandstone");
    let cut = BlockState::new("minecraft:cut_sandstone");
    let orange = BlockState::new("minecraft:orange_terracotta");
    // 9×9 platform + low walls
    for dz in 0..9 {
        for dx in 0..9 {
            set_if_in_chunk(column, base_x + dx, y, base_z + dz, sand.clone());
            if dx == 0 || dz == 0 || dx == 8 || dz == 8 {
                set_if_in_chunk(column, base_x + dx, y + 1, base_z + dz, cut.clone());
                set_if_in_chunk(column, base_x + dx, y + 2, base_z + dz, cut.clone());
            }
        }
    }
    // Center terracotta cross
    for i in 2..7 {
        set_if_in_chunk(column, base_x + i, y + 1, base_z + 4, orange.clone());
        set_if_in_chunk(column, base_x + 4, y + 1, base_z + i, orange.clone());
    }
}

fn place_village_house(column: &mut ChunkColumn, seed: i64, sea_level: i32) {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    let mut rng = factory.from_hash_of(&format!("village/{}/{}", column.x, column.z));
    let base_x = column.x * 16 + 3 + rng.next_int(4);
    let base_z = column.z * 16 + 3 + rng.next_int(4);
    let Some(sy) = surface_solid_y(column, base_x, base_z) else {
        return;
    };
    let y = sy.max(sea_level);
    let floor = BlockState::new("minecraft:oak_planks");
    let wall = BlockState::new("minecraft:cobblestone");
    let log = BlockState::new("minecraft:oak_log");
    let glass = BlockState::new("minecraft:glass_pane");
    let door = BlockState::new("minecraft:oak_door");
    let roof = BlockState::new("minecraft:oak_stairs");

    // 5×5 floor
    for dz in 0..5 {
        for dx in 0..5 {
            set_if_in_chunk(column, base_x + dx, y, base_z + dz, floor.clone());
        }
    }
    // Walls height 3
    for dy in 1..=3 {
        for dz in 0..5 {
            for dx in 0..5 {
                if dx == 0 || dz == 0 || dx == 4 || dz == 4 {
                    let corner = (dx == 0 || dx == 4) && (dz == 0 || dz == 4);
                    let b = if corner {
                        log.clone()
                    } else if dy == 2 && (dx == 2 || dz == 2) && !(dx == 2 && dz == 0) {
                        glass.clone()
                    } else {
                        wall.clone()
                    };
                    set_if_in_chunk(column, base_x + dx, y + dy, base_z + dz, b);
                }
            }
        }
    }
    // Door on +Z face
    set_if_in_chunk(column, base_x + 2, y + 1, base_z, door.clone());
    set_if_in_chunk(column, base_x + 2, y + 2, base_z, door);
    // Simple roof
    for dz in 0..5 {
        for dx in 0..5 {
            set_if_in_chunk(column, base_x + dx, y + 4, base_z + dz, roof.clone());
        }
    }
    // Torch
    set_if_in_chunk(
        column,
        base_x + 2,
        y + 3,
        base_z + 2,
        BlockState::new("minecraft:wall_torch"),
    );
}

fn place_shipwreck_stub(column: &mut ChunkColumn, seed: i64, sea_level: i32) {
    let _ = seed;
    let base_x = column.x * 16 + 3;
    let base_z = column.z * 16 + 5;
    let Some(sy) = surface_solid_y(column, base_x, base_z) else {
        // Ocean floor or beach
        return;
    };
    let y = sy.max(sea_level - 8).min(sea_level + 1);
    let planks = BlockState::new("minecraft:oak_planks");
    let log = BlockState::new("minecraft:oak_log");
    // Hull 7 long × 3 wide
    for dz in 0..3 {
        for dx in 0..7 {
            set_if_in_chunk(column, base_x + dx, y, base_z + dz, planks.clone());
            if dx == 0 || dx == 6 || dz == 0 || dz == 2 {
                set_if_in_chunk(column, base_x + dx, y + 1, base_z + dz, planks.clone());
            }
        }
    }
    // Mast
    for dy in 2..=5 {
        set_if_in_chunk(column, base_x + 3, y + dy, base_z + 1, log.clone());
    }
}

fn place_igloo_stub(column: &mut ChunkColumn, seed: i64, sea_level: i32) {
    let _ = seed;
    let base_x = column.x * 16 + 5;
    let base_z = column.z * 16 + 5;
    let Some(sy) = surface_solid_y(column, base_x, base_z) else {
        return;
    };
    let y = sy.max(sea_level);
    let snow = BlockState::new("minecraft:snow_block");
    // Dome-ish 5×5
    for dz in 0..5 {
        for dx in 0..5 {
            set_if_in_chunk(column, base_x + dx, y, base_z + dz, snow.clone());
            if dx == 0 || dz == 0 || dx == 4 || dz == 4 {
                set_if_in_chunk(column, base_x + dx, y + 1, base_z + dz, snow.clone());
                set_if_in_chunk(column, base_x + dx, y + 2, base_z + dz, snow.clone());
            }
        }
    }
    for dz in 1..4 {
        for dx in 1..4 {
            set_if_in_chunk(column, base_x + dx, y + 3, base_z + dz, snow.clone());
        }
    }
}

fn set_if_in_chunk(column: &mut ChunkColumn, x: i32, y: i32, z: i32, state: BlockState) {
    let base_x = column.x * 16;
    let base_z = column.z * 16;
    if x < base_x || x >= base_x + 16 || z < base_z || z >= base_z + 16 {
        return;
    }
    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(crate::chunk::MAX_SECTION_Y) + 1) * 16 - 1;
    if y < min_block || y > max_block {
        return;
    }
    column.set_block(x, y, z, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_chunk_is_deterministic() {
        let a = is_structure_chunk(12345, 0, 0, 40, 15, 3429021);
        let b = is_structure_chunk(12345, 0, 0, 40, 15, 3429021);
        assert_eq!(a, b);
        // Find at least one start in a region band
        let mut any = false;
        for z in 0..40 {
            for x in 0..40 {
                if is_structure_chunk(12345, x, z, 40, 15, 3429021) {
                    any = true;
                    break;
                }
            }
        }
        assert!(any, "expected a structure start in 40×40 chunks");
    }

    #[test]
    fn ruined_portal_places_obsidian() {
        use crate::worldgen::chunk_fill::generate_column_from_density;
        use crate::worldgen::density::DensityLibrary;
        use crate::worldgen::noise_settings::NoiseSettings;

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
        // Find a chunk that is a ruined portal start
        let mut found = false;
        for cz in 0..40 {
            for cx in 0..40 {
                if !is_structure_chunk(1, cx, cz, 40, 15, 3_429_021) {
                    continue;
                }
                let mut col =
                    generate_column_from_density(1, cx, cz, &settings, &mut lib).unwrap();
                // Clear and force portal only (generation already may have structures)
                place_ruined_portal(&mut col, 1, 63);
                for z in 0..16 {
                    for x in 0..16 {
                        for y in 120..140 {
                            if col.get_block(col.x * 16 + x, y, col.z * 16 + z).name
                                == "minecraft:obsidian"
                            {
                                found = true;
                                break;
                            }
                        }
                    }
                }
                break;
            }
            if found {
                break;
            }
        }
        assert!(found, "expected obsidian from ruined portal stub");
    }
}
