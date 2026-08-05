//! `noise_settings` JSON (overworld, nether, end, …) + noise router fields.
//!
//! Layout mirrors the official datapack and the same layering used by other
//! native servers (load settings → evaluate density graph → fill blocks).
//! We reimplement algorithms ourselves; we do not copy GPL code.

use serde_json::Value;

use crate::worldgen::density::{DensityFunction, DensityLibrary};

/// Horizontal/vertical cell size from the `noise` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseSize {
    pub min_y: i32,
    pub height: i32,
    pub size_horizontal: i32,
    pub size_vertical: i32,
}

impl NoiseSize {
    pub fn max_y(self) -> i32 {
        self.min_y + self.height
    }
}

/// Subset of noise_settings needed for terrain fill.
#[derive(Debug, Clone)]
pub struct NoiseSettings {
    pub sea_level: i32,
    pub aquifers_enabled: bool,
    pub ore_veins_enabled: bool,
    pub legacy_random_source: bool,
    pub default_block: String,
    pub default_fluid: String,
    pub noise: NoiseSize,
    /// Raw `noise_router` object (each field is a density JSON value or id).
    pub noise_router: Value,
}

impl NoiseSettings {
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let noise = value
            .get("noise")
            .ok_or_else(|| "noise_settings missing noise".to_owned())?;
        Ok(Self {
            sea_level: value
                .get("sea_level")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "missing sea_level".to_owned())? as i32,
            aquifers_enabled: value
                .get("aquifers_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            ore_veins_enabled: value
                .get("ore_veins_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            legacy_random_source: value
                .get("legacy_random_source")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            default_block: block_name(value.get("default_block"))
                .unwrap_or_else(|| "minecraft:stone".to_owned()),
            default_fluid: block_name(value.get("default_fluid"))
                .unwrap_or_else(|| "minecraft:water".to_owned()),
            noise: NoiseSize {
                min_y: noise
                    .get("min_y")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| "missing min_y".to_owned())? as i32,
                height: noise
                    .get("height")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| "missing height".to_owned())? as i32,
                size_horizontal: noise
                    .get("size_horizontal")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1) as i32,
                size_vertical: noise
                    .get("size_vertical")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(2) as i32,
            },
            noise_router: value
                .get("noise_router")
                .cloned()
                .ok_or_else(|| "missing noise_router".to_owned())?,
        })
    }

    /// Parses the `final_density` field into a density function using `lib`.
    pub fn final_density(&self, lib: &mut DensityLibrary) -> Result<DensityFunction, String> {
        let fd = self
            .noise_router
            .get("final_density")
            .ok_or_else(|| "noise_router missing final_density".to_owned())?;
        lib.resolve(fd)
    }
}

fn block_name(v: Option<&Value>) -> Option<String> {
    let obj = v?.as_object()?;
    obj.get("Name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_flat_settings() {
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
        let s = NoiseSettings::from_json(&v).unwrap();
        assert_eq!(s.sea_level, 63);
        assert_eq!(s.noise.min_y, -64);
        let mut lib = DensityLibrary::new(0);
        let fd = s.final_density(&mut lib).unwrap();
        // Y=-64 solid, Y=320 air-ish
        assert!(
            fd.compute(
                crate::worldgen::density::DensityContext::new(0, -64, 0),
                &mut lib.noises
            )
            .unwrap()
                > 0.0
        );
        assert!(
            fd.compute(
                crate::worldgen::density::DensityContext::new(0, 320, 0),
                &mut lib.noises
            )
            .unwrap()
                < 0.0
        );
    }
}
