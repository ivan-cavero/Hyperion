//! Aquifer fluid pick for open cells (density ≤ 0).
//!
//! Full JE `NoiseBasedAquifer` grids barrier / floodedness / spread per cell.
//! This module implements a **faithful-role** sampler:
//! - `aquifers_enabled=false` → flood almost everything below sea (lava deep)
//! - `aquifers_enabled=true` → use router `fluid_level_floodedness` + `lava`
//!   noises to leave dry caves and place deep lava
//!
//! Expand toward full NoiseBasedAquifer + barrier when golden dumps require it.

use crate::chunk::BlockState;
use crate::worldgen::density::{DensityContext, DensityFunction, DensityLibrary};
use crate::worldgen::noise_settings::NoiseSettings;

/// Result of aquifer evaluation for one open cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AquiferFluid {
    Air,
    Water,
    Lava,
}

impl AquiferFluid {
    pub fn to_block(&self, default_fluid: &BlockState) -> BlockState {
        match self {
            Self::Air => BlockState::air(),
            Self::Water => default_fluid.clone(),
            Self::Lava => BlockState::new("minecraft:lava"),
        }
    }
}

/// Minimal aquifer sampler wired from noise_router aquifer fields when present.
pub struct SimpleAquifer {
    sea_level: i32,
    /// Approximate lava threshold (JE ~ -54).
    lava_y: i32,
    aquifers_enabled: bool,
    floodedness: Option<DensityFunction>,
    lava_noise: Option<DensityFunction>,
}

impl SimpleAquifer {
    pub fn from_settings(
        settings: &NoiseSettings,
        lib: &mut DensityLibrary,
    ) -> Result<Self, String> {
        let r = &settings.noise_router;
        let floodedness = r
            .get("fluid_level_floodedness")
            .map(|v| lib.resolve(v))
            .transpose()?;
        let lava_noise = r.get("lava").map(|v| lib.resolve(v)).transpose()?;
        Ok(Self {
            sea_level: settings.sea_level,
            lava_y: -54,
            aquifers_enabled: settings.aquifers_enabled,
            floodedness,
            lava_noise,
        })
    }

    /// Fluid (or air) for a cell that is **not solid** (`density ≤ 0`).
    pub fn compute(
        &self,
        x: i32,
        y: i32,
        z: i32,
        lib: &mut DensityLibrary,
    ) -> Result<AquiferFluid, String> {
        if y >= self.sea_level {
            return Ok(AquiferFluid::Air);
        }

        // Deep lava band.
        if y < self.lava_y {
            if let Some(ln) = &self.lava_noise {
                let v = ln.compute(DensityContext::new(x, y, z), &mut lib.noises)?;
                // Positive lava noise → lava; else water/air from rules below.
                if v > 0.0 {
                    return Ok(AquiferFluid::Lava);
                }
            } else {
                // No lava noise: solid lava flood deep (legacy-ish).
                return Ok(AquiferFluid::Lava);
            }
        }

        if !self.aquifers_enabled {
            // Wiki: almost all caves below sea filled with water when disabled.
            return Ok(AquiferFluid::Water);
        }

        // Enabled aquifers: floodedness decides water vs dry cave.
        if let Some(fd) = &self.floodedness {
            let f = fd.compute(DensityContext::new(x, y, z), &mut lib.noises)?;
            // floodedness is roughly [-1, 1]; higher → more flooded.
            // Threshold ~ -0.4 leaves many caves dry while keeping oceans wet.
            if f > -0.4 {
                return Ok(AquiferFluid::Water);
            }
            return Ok(AquiferFluid::Air);
        }

        // No floodedness function: water below sea.
        Ok(AquiferFluid::Water)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::density::DensityLibrary;

    fn settings_with(aquifers: bool) -> NoiseSettings {
        let v = serde_json::json!({
            "sea_level": 63,
            "aquifers_enabled": aquifers,
            "ore_veins_enabled": false,
            "legacy_random_source": false,
            "default_block": { "Name": "minecraft:stone" },
            "default_fluid": { "Name": "minecraft:water" },
            "noise": { "min_y": -64, "height": 384, "size_horizontal": 1, "size_vertical": 2 },
            "noise_router": {
                "final_density": 1.0,
                "fluid_level_floodedness": {
                    "type": "minecraft:constant",
                    "argument": 1.0
                },
                "lava": {
                    "type": "minecraft:constant",
                    "argument": 1.0
                }
            }
        });
        NoiseSettings::from_json(&v).unwrap()
    }

    #[test]
    fn disabled_floods_below_sea_with_water() {
        let settings = settings_with(false);
        let mut lib = DensityLibrary::new(0);
        let aq = SimpleAquifer::from_settings(&settings, &mut lib).unwrap();
        assert_eq!(
            aq.compute(0, 40, 0, &mut lib).unwrap(),
            AquiferFluid::Water
        );
        assert_eq!(aq.compute(0, 80, 0, &mut lib).unwrap(), AquiferFluid::Air);
    }

    #[test]
    fn deep_lava_when_lava_noise_positive() {
        let settings = settings_with(true);
        let mut lib = DensityLibrary::new(0);
        let aq = SimpleAquifer::from_settings(&settings, &mut lib).unwrap();
        assert_eq!(
            aq.compute(0, -60, 0, &mut lib).unwrap(),
            AquiferFluid::Lava
        );
    }
}
