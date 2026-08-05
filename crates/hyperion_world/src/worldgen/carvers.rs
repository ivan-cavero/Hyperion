//! Post-surface carvers (JE `CaveWorldCarver` / `CanyonWorldCarver` role).
//!
//! Full random-walk + ellipsoid math is ported stepwise. This module carves
//! deterministic worm tunnels and occasional canyon slits so open caves appear
//! after density + surface — same pipeline slot as vanilla CARVERS status.
//!
//! Config probabilities / Y ranges mirror `configured_carver/*.json` defaults.

use crate::chunk::{BlockState, ChunkColumn, MIN_SECTION_Y};
use crate::worldgen::random::{PositionalRandomFactory, RandomSource, XoroshiroRandom};

/// Y level at/below which carved cells fill with lava (config `above_bottom: 8` ≈ min_y+8).
const DEFAULT_LAVA_ABOVE_BOTTOM: i32 = 8;

/// Apply overworld carvers to a column (in place).
pub fn apply_overworld_carvers(column: &mut ChunkColumn, seed: i64, sea_level: i32, min_y: i32) {
    let factory = PositionalRandomFactory::from_world_seed(seed);
    // JE salts carvers with chunk coords; we fork from hash of chunk id.
    let mut cave_rng = factory.from_hash_of(&format!(
        "minecraft:cave/{}/{}",
        column.x, column.z
    ));
    let mut extra_rng = factory.from_hash_of(&format!(
        "minecraft:cave_extra/{}/{}",
        column.x, column.z
    ));
    let mut canyon_rng = factory.from_hash_of(&format!(
        "minecraft:canyon/{}/{}",
        column.x, column.z
    ));

    let lava_y = min_y + DEFAULT_LAVA_ABOVE_BOTTOM;

    // cave.json probability 0.15 — a few room starts per chunk.
    let cave_rooms = 1 + cave_rng.next_int(3) as usize;
    carve_caves(
        column,
        &mut cave_rng,
        sea_level,
        min_y,
        lava_y,
        CavePass {
            probability: 0.15,
            y_min: min_y + 8,
            y_max: 180,
            rooms: cave_rooms,
            max_steps: 24,
            base_radius: 2.2,
        },
    );

    // cave_extra_underground.json probability 0.07, lower Y.
    let extra_rooms = 1 + extra_rng.next_int(2) as usize;
    carve_caves(
        column,
        &mut extra_rng,
        sea_level,
        min_y,
        lava_y,
        CavePass {
            probability: 0.07,
            y_min: min_y + 8,
            y_max: 47,
            rooms: extra_rooms,
            max_steps: 18,
            base_radius: 1.8,
        },
    );

    // canyon.json probability 0.01 — rare wider slit.
    if canyon_rng.next_double() < 0.01 {
        carve_canyon(column, &mut canyon_rng, sea_level, min_y, lava_y);
    }
}

struct CavePass {
    probability: f64,
    y_min: i32,
    y_max: i32,
    rooms: usize,
    max_steps: usize,
    base_radius: f64,
}

fn carve_caves(
    column: &mut ChunkColumn,
    rng: &mut XoroshiroRandom,
    sea_level: i32,
    min_y: i32,
    lava_y: i32,
    pass: CavePass,
) {
    let _ = min_y;
    if rng.next_double() >= pass.probability {
        // Still allow a tiny chance of one short tunnel for density of caves.
        if rng.next_double() > 0.35 {
            return;
        }
    }
    let base_x = column.x * 16;
    let base_z = column.z * 16;
    let y_span = (pass.y_max - pass.y_min).max(1);

    for _ in 0..pass.rooms {
        let mut x = base_x as f64 + rng.next_double() * 16.0;
        let mut y = f64::from(pass.y_min) + rng.next_double() * f64::from(y_span);
        let mut z = base_z as f64 + rng.next_double() * 16.0;
        let mut yaw = rng.next_double() * std::f64::consts::TAU;
        let mut pitch = (rng.next_double() - 0.5) * 0.5;
        let steps = pass.max_steps / 2 + rng.next_int(pass.max_steps as i32 / 2 + 1) as usize;
        let radius0 = pass.base_radius * (0.7 + rng.next_double() * 0.7);

        for step in 0..steps {
            let t = step as f64 / steps as f64;
            let radius = radius0 * (1.0 - 0.35 * t);
            carve_ellipsoid(
                column,
                x,
                y,
                z,
                radius,
                radius * 0.85,
                sea_level,
                lava_y,
            );
            // Random walk
            yaw += (rng.next_double() - 0.5) * 0.9;
            pitch += (rng.next_double() - 0.5) * 0.35;
            pitch = pitch.clamp(-1.2, 1.2);
            let dist = 1.4 + rng.next_double() * 0.8;
            x += yaw.cos() * pitch.cos() * dist;
            y += pitch.sin() * dist * 0.55;
            z += yaw.sin() * pitch.cos() * dist;
            // Keep roughly in chunk band (+margin for edges).
            if x < f64::from(base_x - 4) || x > f64::from(base_x + 20) {
                yaw += std::f64::consts::PI;
            }
            if z < f64::from(base_z - 4) || z > f64::from(base_z + 20) {
                yaw += std::f64::consts::PI;
            }
            y = y.clamp(f64::from(pass.y_min), f64::from(pass.y_max));
        }
    }
}

fn carve_canyon(
    column: &mut ChunkColumn,
    rng: &mut XoroshiroRandom,
    sea_level: i32,
    min_y: i32,
    lava_y: i32,
) {
    let base_x = column.x * 16;
    let base_z = column.z * 16;
    let mut x = base_x as f64 + rng.next_double() * 16.0;
    let y0 = 10.0 + rng.next_double() * 50.0; // config absolute 10..67
    let mut z = base_z as f64 + rng.next_double() * 16.0;
    let yaw = rng.next_double() * std::f64::consts::TAU;
    let steps = 20 + rng.next_int(16) as usize;
    let thickness = 1.5 + rng.next_double() * 3.5; // trapezoid-ish

    for step in 0..steps {
        let t = step as f64 / steps as f64;
        let width = thickness * (0.6 + 0.8 * (1.0 - (2.0 * t - 1.0).abs()));
        let y = y0 + (rng.next_double() - 0.5) * 2.0;
        carve_ellipsoid(
            column,
            x,
            y,
            z,
            width,
            width * 1.6, // taller canyon
            sea_level,
            lava_y,
        );
        // Also carve upward toward surface a bit (ravine walls).
        carve_ellipsoid(
            column,
            x,
            y + width,
            z,
            width * 0.7,
            width * 1.2,
            sea_level,
            lava_y,
        );
        x += yaw.cos() * 1.6;
        z += yaw.sin() * 1.6;
        let _ = min_y;
    }
}

#[allow(clippy::too_many_arguments)]
fn carve_ellipsoid(
    column: &mut ChunkColumn,
    cx: f64,
    cy: f64,
    cz: f64,
    rx: f64,
    ry: f64,
    sea_level: i32,
    lava_y: i32,
) {
    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(crate::chunk::MAX_SECTION_Y) + 1) * 16 - 1;
    let x0 = (cx - rx).floor() as i32 - 1;
    let x1 = (cx + rx).ceil() as i32 + 1;
    let y0 = (cy - ry).floor() as i32 - 1;
    let y1 = (cy + ry).ceil() as i32 + 1;
    let z0 = (cz - rx).floor() as i32 - 1;
    let z1 = (cz + rx).ceil() as i32 + 1;

    let base_x = column.x * 16;
    let base_z = column.z * 16;

    for z in z0..=z1 {
        if z < base_z || z >= base_z + 16 {
            continue;
        }
        for x in x0..=x1 {
            if x < base_x || x >= base_x + 16 {
                continue;
            }
            for y in y0.max(min_block)..=y1.min(max_block) {
                let dx = (f64::from(x) + 0.5 - cx) / rx;
                let dy = (f64::from(y) + 0.5 - cy) / ry;
                let dz = (f64::from(z) + 0.5 - cz) / rx;
                if dx * dx + dy * dy + dz * dz > 1.0 {
                    continue;
                }
                if !is_replaceable(&column.get_block(x, y, z)) {
                    continue;
                }
                let fill = if y <= lava_y {
                    BlockState::new("minecraft:lava")
                } else if y < sea_level {
                    // Open to aquifer water below sea; carvers in JE schedule fluid updates.
                    BlockState::new("minecraft:water")
                } else {
                    BlockState::air()
                };
                column.set_block(x, y, z, fill);
            }
        }
    }
}

fn is_replaceable(b: &BlockState) -> bool {
    matches!(
        b.name.as_str(),
        "minecraft:stone"
            | "minecraft:deepslate"
            | "minecraft:dirt"
            | "minecraft:grass_block"
            | "minecraft:coarse_dirt"
            | "minecraft:podzol"
            | "minecraft:sand"
            | "minecraft:red_sand"
            | "minecraft:gravel"
            | "minecraft:sandstone"
            | "minecraft:granite"
            | "minecraft:diorite"
            | "minecraft:andesite"
            | "minecraft:tuff"
            | "minecraft:calcite"
            | "minecraft:smooth_basalt"
            | "minecraft:packed_mud"
            | "minecraft:mud"
            | "minecraft:snow_block"
            | "minecraft:powder_snow"
            | "minecraft:mycelium"
            | "minecraft:terracotta"
            | "minecraft:netherrack"
    ) || b.name.contains("terracotta")
        || b.name.contains("ore")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::chunk_fill::generate_column_from_density;
    use crate::worldgen::density::DensityLibrary;
    use crate::worldgen::noise_settings::NoiseSettings;

    #[test]
    fn carver_can_open_solid_flat_column() {
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
        let col = generate_column_from_density(42, 0, 0, &settings, &mut lib).unwrap();
        // Try several seeds until carvers open a cell in the solid band.
        let mut carved = false;
        for seed in 0..40 {
            let mut c = col.clone();
            apply_overworld_carvers(&mut c, seed, 63, -64);
            for y in 20..80 {
                let b = c.get_block(8, y, 8);
                if b.is_air() || b.is_fluid() {
                    carved = true;
                    break;
                }
            }
            if carved {
                break;
            }
        }
        assert!(carved, "expected at least one seed to carve open cells");
    }
}
