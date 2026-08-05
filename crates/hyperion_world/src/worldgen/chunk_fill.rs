//! Fill a chunk column from a density function (`final_density` rule).
//!
//! Official rule (wiki / noise settings): if `final_density(x,y,z) > 0` place
//! `default_block`, else air (aquifers later place fluid). Surface rules run
//! afterward — not yet implemented here.

use hyperion_protocol::BLOCK_SECTION_SIZE;

use crate::chunk::{
    BlockState, ChunkColumn, ChunkSection, MIN_SECTION_Y, PLAINS_BIOME, SECTION_COUNT,
    section_index,
};
use crate::level_dat::DEFAULT_DATA_VERSION;
use crate::worldgen::density::{DensityContext, DensityLibrary};
use crate::worldgen::noise_settings::NoiseSettings;

/// Generates one column by sampling `final_density` at every block.
///
/// Does **not** claim full overworld parity until the density graph (including
/// spline / old_blended_noise / aquifers / surface rules) is complete. Use with
/// simple routers (e.g. y_clamped_gradient flat world) for correctness tests.
pub fn generate_column_from_density(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    settings: &NoiseSettings,
    lib: &mut DensityLibrary,
) -> Result<ChunkColumn, String> {
    let _ = seed; // seed is already in lib.noises
    let final_density = settings.final_density(lib)?;
    let min_y = settings.noise.min_y;
    let max_y = settings.noise.max_y();
    let solid = BlockState::new(settings.default_block.clone());
    let fluid = BlockState::new(settings.default_fluid.clone());
    let sea = settings.sea_level;

    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    // heightmap: first empty above solid surface
    let mut heightmap = [0u16; 256];
    let mut max_surface = min_y;

    let mut sections = Vec::with_capacity(SECTION_COUNT);
    // We still use Hyperion overworld section count (-4..19). Clamp density sampling
    // to noise height range.
    for i in 0..SECTION_COUNT {
        let section_y = MIN_SECTION_Y + i as i8;
        let y0 = i32::from(section_y) * 16;
        let mut cells = vec![BlockState::air(); BLOCK_SECTION_SIZE];
        for lz in 0..16i32 {
            for lx in 0..16i32 {
                for ly in 0..16i32 {
                    let world_y = y0 + ly;
                    let world_x = base_x + lx;
                    let world_z = base_z + lz;
                    let block = if world_y < min_y || world_y >= max_y {
                        BlockState::air()
                    } else {
                        let d = final_density.compute(
                            DensityContext::new(world_x, world_y, world_z),
                            &mut lib.noises,
                        )?;
                        if d > 0.0 {
                            solid.clone()
                        } else if world_y < sea {
                            fluid.clone()
                        } else {
                            BlockState::air()
                        }
                    };
                    let idx = section_index(lx as u8, ly as u8, lz as u8);
                    cells[idx] = block;
                }
            }
        }
        sections.push(ChunkSection::from_blocks(section_y, &cells, PLAINS_BIOME));
    }

    // Heightmaps from filled column
    for lz in 0..16 {
        for lx in 0..16 {
            let mut top = min_y;
            for y in (min_y..max_y).rev() {
                let b = sections
                    .iter()
                    .find(|s| {
                        let y0 = i32::from(s.y) * 16;
                        y >= y0 && y < y0 + 16
                    })
                    .map(|s| s.get_block(lx as u8, (y.rem_euclid(16)) as u8, lz as u8))
                    .unwrap_or_else(BlockState::air);
                if !b.is_air() {
                    top = y;
                    break;
                }
            }
            let surface = (top + 1).clamp(0, 511) as u16;
            heightmap[(lz * 16 + lx) as usize] = surface;
            max_surface = max_surface.max(top + 1);
        }
    }

    Ok(ChunkColumn {
        x: chunk_x,
        z: chunk_z,
        min_section_y: MIN_SECTION_Y,
        sections,
        surface_y: max_surface,
        heightmap,
        data_version: DEFAULT_DATA_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::density::DensityLibrary;
    use crate::worldgen::noise_settings::NoiseSettings;

    fn flat_settings() -> NoiseSettings {
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
        NoiseSettings::from_json(&v).unwrap()
    }

    #[test]
    fn flat_density_fill_has_stone_bottom_and_air_top() {
        let settings = flat_settings();
        let mut lib = DensityLibrary::new(0);
        let col = generate_column_from_density(0, 0, 0, &settings, &mut lib).unwrap();
        // Bottom of world solid
        assert_eq!(col.get_block(0, -64, 0), BlockState::stone());
        // High air
        assert!(col.get_block(0, 200, 0).is_air());
        // Cross-over around mid gradient
        let mid = col.get_block(0, 128, 0);
        // density at y=128: halfway from 1 to -1 ≈ 0 → not solid (≤0)
        assert!(mid.is_air() || mid == BlockState::new("minecraft:water"));
    }
}
