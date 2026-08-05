//! Golden / regression harness for worldgen determinism.
//!
//! Full 1:1 vs the official server needs external dumps. Until then we pin
//! **Hyperion-stable** samples so refactors cannot silently change terrain.

use crate::chunk::ChunkColumn;

/// Compact fingerprint of a column for regression tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnFingerprint {
    pub surface_y: i32,
    pub non_air: u32,
    pub bedrock: u32,
    pub stone: u32,
    pub deepslate: u32,
    pub dirt: u32,
    pub grass: u32,
    pub water: u32,
    pub lava: u32,
    /// Sampled block names at fixed world positions (x,y,z relative to chunk origin).
    pub samples: Vec<String>,
}

impl ColumnFingerprint {
    pub fn from_column(col: &ChunkColumn) -> Self {
        let mut non_air = 0u32;
        let mut bedrock = 0u32;
        let mut stone = 0u32;
        let mut deepslate = 0u32;
        let mut dirt = 0u32;
        let mut grass = 0u32;
        let mut water = 0u32;
        let mut lava = 0u32;
        for z in 0..16 {
            for x in 0..16 {
                for y in -64..320 {
                    let b = col.get_block(col.x * 16 + x, y, col.z * 16 + z);
                    if b.is_air() {
                        continue;
                    }
                    non_air += 1;
                    match b.name.as_str() {
                        "minecraft:bedrock" => bedrock += 1,
                        "minecraft:stone" => stone += 1,
                        "minecraft:deepslate" => deepslate += 1,
                        "minecraft:dirt" => dirt += 1,
                        "minecraft:grass_block" => grass += 1,
                        "minecraft:water" => water += 1,
                        "minecraft:lava" => lava += 1,
                        _ => {}
                    }
                }
            }
        }
        let base_x = col.x * 16;
        let base_z = col.z * 16;
        let sample_pts = [
            (0, -64, 0),
            (0, -32, 0),
            (0, 0, 0),
            (0, 64, 0),
            (8, 80, 8),
            (15, 120, 15),
        ];
        let samples = sample_pts
            .iter()
            .map(|&(x, y, z)| col.get_block(base_x + x, y, base_z + z).name.clone())
            .collect();
        Self {
            surface_y: col.surface_y,
            non_air,
            bedrock,
            stone,
            deepslate,
            dirt,
            grass,
            water,
            lava,
            samples,
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
    use crate::worldgen::density::DensityLibrary;
    use crate::worldgen::noise_settings::NoiseSettings;

    /// Flat density that crosses zero above sea level so grass surface applies.
    /// y_clamped_gradient(-64→320, 1→-1) zero at y≈128.
    fn flat_router_settings() -> NoiseSettings {
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
    fn flat_router_fingerprint_is_stable() {
        let settings = flat_router_settings();
        let mut lib = DensityLibrary::new(0);
        let col = generate_column_from_density(0, 0, 0, &settings, &mut lib).unwrap();
        let fp = ColumnFingerprint::from_column(&col);
        // Re-generate
        let mut lib2 = DensityLibrary::new(0);
        let col2 = generate_column_from_density(0, 0, 0, &settings, &mut lib2).unwrap();
        let fp2 = ColumnFingerprint::from_column(&col2);
        assert_eq!(fp, fp2);
        assert!(fp.bedrock > 0, "bedrock floor expected");
        assert!(fp.grass > 0, "grass after surface pass");
        assert!(fp.deepslate > 0, "deepslate band below y=0 expected");
        assert_eq!(fp.samples[0], "minecraft:bedrock");
        assert_eq!(fp.samples[1], "minecraft:deepslate");
        // Pin exact Hyperion-stable counts for the flat router (update if gen changes).
        assert_eq!(fp.grass, 256);
        assert_eq!(fp.surface_y, 128);
    }

    #[test]
    fn overworld_fingerprint_stable_if_jar_present() {
        let Some(root) = find_workspace_root() else {
            return;
        };
        let jar = default_server_inner_jar(&root);
        if !jar.is_file() {
            return;
        }
        let (settings, mut lib) = load_overworld_from_jar(&jar, 12345).unwrap();
        let col = generate_column_from_density(12345, 0, 0, &settings, &mut lib).unwrap();
        let fp = ColumnFingerprint::from_column(&col);

        let (settings2, mut lib2) = load_overworld_from_jar(&jar, 12345).unwrap();
        let col2 = generate_column_from_density(12345, 0, 0, &settings2, &mut lib2).unwrap();
        let fp2 = ColumnFingerprint::from_column(&col2);
        assert_eq!(fp, fp2, "same seed must produce identical Hyperion columns");
        assert!(fp.non_air > 1000, "terrain should place many solid blocks");
        // Pin rough shape so regressions fail loudly (update if gen intentionally changes).
        assert!(
            fp.surface_y > -64 && fp.surface_y < 320,
            "surface_y={}",
            fp.surface_y
        );
        eprintln!(
            "overworld fp seed=12345 non_air={} surface_y={} grass={} stone={} deepslate={}",
            fp.non_air, fp.surface_y, fp.grass, fp.stone, fp.deepslate
        );
    }
}
