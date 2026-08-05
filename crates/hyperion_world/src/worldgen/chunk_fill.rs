//! Fill a chunk column from a density function (`final_density` rule).
//!
//! Official rule (wiki / noise settings): if `final_density(x,y,z) > 0` place
//! `default_block`, else air/fluid. Then surface rules dress the top.
//!
//! Sampling uses the noise cell grid (`size_horizontal` / `size_vertical`)
//! with trilinear interpolation — same structure as Java Edition's
//! NoiseChunk cell sampling. Full per-node `interpolated` / cache semantics
//! still refine toward golden dumps.

use hyperion_protocol::BLOCK_SECTION_SIZE;

use crate::chunk::{
    BlockState, ChunkColumn, ChunkSection, MIN_SECTION_Y, PLAINS_BIOME, SECTION_COUNT,
    section_index,
};
use crate::level_dat::DEFAULT_DATA_VERSION;
use crate::worldgen::climate::ClimateSampler;
use crate::worldgen::density::{DensityContext, DensityFunction, DensityLibrary};
use crate::worldgen::noise_settings::NoiseSettings;
use crate::worldgen::surface_rules::apply_basic_surface;

/// Generates one column by sampling `final_density` on the noise cell grid,
/// multi-noise biome (when router has climate axes), then surface pass.
///
/// Not full official parity until golden dumps match (noise tables + full
/// surface_rule tree + aquifers).
pub fn generate_column_from_density(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    settings: &NoiseSettings,
    lib: &mut DensityLibrary,
) -> Result<ChunkColumn, String> {
    let mut column = generate_column_density_only(seed, chunk_x, chunk_z, settings, lib)?;

    // Biome at chunk center surface band (quart-aligned block coords).
    let biome = if settings.has_climate_router() {
        match ClimateSampler::from_settings(settings, lib) {
            Ok(sampler) => {
                let bx = chunk_x * 16 + 8;
                let bz = chunk_z * 16 + 8;
                let by = settings.sea_level;
                sampler
                    .biome_at(bx, by, bz, lib)
                    .unwrap_or_else(|_| PLAINS_BIOME.to_owned())
            }
            Err(_) => PLAINS_BIOME.to_owned(),
        }
    } else {
        PLAINS_BIOME.to_owned()
    };
    for section in &mut column.sections {
        section.biome = biome.clone();
    }

    apply_basic_surface(
        &mut column,
        settings.sea_level,
        settings.noise.min_y,
        seed,
        settings.surface_rule.as_ref(),
        &biome,
    );
    Ok(column)
}

/// Density fill only (no surface rules) — useful for testing the density graph.
pub fn generate_column_density_only(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    settings: &NoiseSettings,
    lib: &mut DensityLibrary,
) -> Result<ChunkColumn, String> {
    let _ = seed;
    let final_density = settings.final_density(lib)?;
    let min_y = settings.noise.min_y;
    let max_y = settings.noise.max_y();
    let cell_w = settings.noise.cell_width();
    let cell_h = settings.noise.cell_height();
    let solid = BlockState::new(settings.default_block.clone());
    let fluid = BlockState::new(settings.default_fluid.clone());
    let lava = BlockState::new("minecraft:lava");
    let sea = settings.sea_level;
    // Rough aquifer floor: deep open space becomes lava (full AquiferSampler later).
    let lava_y = -54;

    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    let grid = DensityGrid::build(
        &final_density,
        lib,
        base_x,
        base_z,
        min_y,
        max_y,
        cell_w,
        cell_h,
    )?;

    let mut heightmap = [0u16; 256];
    let mut max_surface = min_y;

    let mut sections = Vec::with_capacity(SECTION_COUNT);
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
                        let d = grid.sample(world_x, world_y, world_z);
                        if d > 0.0 {
                            solid.clone()
                        } else if world_y < sea {
                            if world_y < lava_y {
                                lava.clone()
                            } else {
                                fluid.clone()
                            }
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

/// Pre-sampled density on a regular cell grid covering one chunk column.
struct DensityGrid {
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    cell_w: i32,
    cell_h: i32,
    nx: usize,
    ny: usize,
    nz: usize,
    values: Vec<f64>,
}

impl DensityGrid {
    #[allow(clippy::too_many_arguments)]
    fn build(
        density: &DensityFunction,
        lib: &mut DensityLibrary,
        base_x: i32,
        base_z: i32,
        min_y: i32,
        max_y: i32,
        cell_w: i32,
        cell_h: i32,
    ) -> Result<Self, String> {
        let cell_w = cell_w.max(1);
        let cell_h = cell_h.max(1);
        // Align to world cell lattice (JE NoiseChunk style).
        let origin_x = floor_div(base_x, cell_w) * cell_w;
        let origin_z = floor_div(base_z, cell_w) * cell_w;
        let origin_y = floor_div(min_y, cell_h) * cell_h;
        let end_x = base_x + 16;
        let end_z = base_z + 16;
        let end_y = max_y;

        let nx = ((end_x - origin_x + cell_w - 1) / cell_w + 1) as usize;
        let nz = ((end_z - origin_z + cell_w - 1) / cell_w + 1) as usize;
        let ny = ((end_y - origin_y + cell_h - 1) / cell_h + 1) as usize;

        let mut values = Vec::with_capacity(nx * ny * nz);
        for iz in 0..nz {
            for iy in 0..ny {
                for ix in 0..nx {
                    let wx = origin_x + (ix as i32) * cell_w;
                    let wy = origin_y + (iy as i32) * cell_h;
                    let wz = origin_z + (iz as i32) * cell_w;
                    let d = density.compute(DensityContext::new(wx, wy, wz), &mut lib.noises)?;
                    values.push(d);
                }
            }
        }
        Ok(Self {
            origin_x,
            origin_y,
            origin_z,
            cell_w,
            cell_h,
            nx,
            ny,
            nz,
            values,
        })
    }

    fn at(&self, ix: usize, iy: usize, iz: usize) -> f64 {
        self.values[iz * self.ny * self.nx + iy * self.nx + ix]
    }

    /// Trilinear sample at integer block coordinates.
    fn sample(&self, x: i32, y: i32, z: i32) -> f64 {
        let fx = (x - self.origin_x) as f64 / self.cell_w as f64;
        let fy = (y - self.origin_y) as f64 / self.cell_h as f64;
        let fz = (z - self.origin_z) as f64 / self.cell_w as f64;

        let x0 = fx.floor() as i32;
        let y0 = fy.floor() as i32;
        let z0 = fz.floor() as i32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;
        let z1 = z0 + 1;

        let tx = fx - f64::from(x0);
        let ty = fy - f64::from(y0);
        let tz = fz - f64::from(z0);

        let ix0 = (x0.clamp(0, self.nx as i32 - 1)) as usize;
        let ix1 = (x1.clamp(0, self.nx as i32 - 1)) as usize;
        let iy0 = (y0.clamp(0, self.ny as i32 - 1)) as usize;
        let iy1 = (y1.clamp(0, self.ny as i32 - 1)) as usize;
        let iz0 = (z0.clamp(0, self.nz as i32 - 1)) as usize;
        let iz1 = (z1.clamp(0, self.nz as i32 - 1)) as usize;

        let c000 = self.at(ix0, iy0, iz0);
        let c100 = self.at(ix1, iy0, iz0);
        let c010 = self.at(ix0, iy1, iz0);
        let c110 = self.at(ix1, iy1, iz0);
        let c001 = self.at(ix0, iy0, iz1);
        let c101 = self.at(ix1, iy0, iz1);
        let c011 = self.at(ix0, iy1, iz1);
        let c111 = self.at(ix1, iy1, iz1);

        let c00 = lerp(c000, c100, tx);
        let c10 = lerp(c010, c110, tx);
        let c01 = lerp(c001, c101, tx);
        let c11 = lerp(c011, c111, tx);
        let c0 = lerp(c00, c10, ty);
        let c1 = lerp(c01, c11, ty);
        lerp(c0, c1, tz)
    }
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
fn floor_div(a: i32, b: i32) -> i32 {
    // Floor division toward -∞ (matches Java for positive b).
    let d = a / b;
    let r = a % b;
    if (r != 0) && ((r < 0) != (b < 0)) {
        d - 1
    } else {
        d
    }
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
        // Density-only: no surface pass.
        let col = generate_column_density_only(0, 0, 0, &settings, &mut lib).unwrap();
        assert_eq!(col.get_block(0, -64, 0), BlockState::stone());
        assert!(col.get_block(0, 200, 0).is_air());
        let mid = col.get_block(0, 128, 0);
        assert!(mid.is_air() || mid == BlockState::new("minecraft:water"));
    }

    #[test]
    fn flat_density_with_surface_has_bedrock_floor() {
        let settings = flat_settings();
        let mut lib = DensityLibrary::new(0);
        let col = generate_column_from_density(0, 0, 0, &settings, &mut lib).unwrap();
        assert_eq!(col.get_block(0, -64, 0), BlockState::bedrock());
        // Zero-crossing of y_clamped_gradient(-64→320, 1→-1): density>0 for y<128.
        // Top solid ≈127 ≥ sea → grass.
        assert_eq!(col.get_block(0, 127, 0), BlockState::grass_block());
    }

    #[test]
    fn floor_div_matches_java_style() {
        assert_eq!(floor_div(5, 2), 2);
        assert_eq!(floor_div(-5, 2), -3);
        assert_eq!(floor_div(-4, 2), -2);
    }
}
