//! Multi-noise climate sampling + biome pick (JE `Climate` / multi-noise source).
//!
//! Samples the noise_router fields (temperature, vegetation→humidity, continents,
//! erosion, depth, ridges→weirdness), quantizes like JE (`* 10000`), and picks
//! the nearest parameter point by squared distance + offset².
//!
//! The overworld parameter list is a **compact Hyperion subset** of the full
//! OverworldBiomeBuilder table — enough for ocean/land variety and surface_rule
//! `biome` conditions. Expand toward full preset parity with golden diffs.

use crate::worldgen::density::{DensityContext, DensityFunction, DensityLibrary};
use crate::worldgen::noise_settings::NoiseSettings;

/// JE `Climate.quantizeCoord`.
#[inline]
pub fn quantize_coord(v: f64) -> i64 {
    (v as f32 * 10_000.0) as i64
}

/// One climate axis span (`Climate.Parameter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClimateParameter {
    pub min: i64,
    pub max: i64,
}

impl ClimateParameter {
    pub fn span(min: f64, max: f64) -> Self {
        Self {
            min: quantize_coord(min),
            max: quantize_coord(max),
        }
    }

    /// Distance of a quantized target to this span (0 if inside).
    pub fn distance(self, value: i64) -> i64 {
        let above = value - self.max;
        if above > 0 {
            return above;
        }
        (self.min - value).max(0)
    }
}

/// JE `Climate.TargetPoint` — quantized climate at one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClimateTarget {
    pub temperature: i64,
    pub humidity: i64,
    pub continentalness: i64,
    pub erosion: i64,
    pub depth: i64,
    pub weirdness: i64,
}

impl ClimateTarget {
    pub fn from_floats(
        temperature: f64,
        humidity: f64,
        continentalness: f64,
        erosion: f64,
        depth: f64,
        weirdness: f64,
    ) -> Self {
        Self {
            temperature: quantize_coord(temperature),
            humidity: quantize_coord(humidity),
            continentalness: quantize_coord(continentalness),
            erosion: quantize_coord(erosion),
            depth: quantize_coord(depth),
            weirdness: quantize_coord(weirdness),
        }
    }
}

/// One multi-noise entry: climate box → biome id.
#[derive(Debug, Clone)]
pub struct BiomeParameterPoint {
    pub temperature: ClimateParameter,
    pub humidity: ClimateParameter,
    pub continentalness: ClimateParameter,
    pub erosion: ClimateParameter,
    pub depth: ClimateParameter,
    pub weirdness: ClimateParameter,
    /// Quantized offset (usually 0).
    pub offset: i64,
    pub biome: String,
}

impl BiomeParameterPoint {
    /// Squared fitness (lower is better). JE `ParameterPoint.fitness`.
    pub fn fitness(&self, target: &ClimateTarget) -> i64 {
        let d = |p: ClimateParameter, v: i64| {
            let x = p.distance(v);
            x.saturating_mul(x)
        };
        d(self.temperature, target.temperature)
            .saturating_add(d(self.humidity, target.humidity))
            .saturating_add(d(self.continentalness, target.continentalness))
            .saturating_add(d(self.erosion, target.erosion))
            .saturating_add(d(self.depth, target.depth))
            .saturating_add(d(self.weirdness, target.weirdness))
            .saturating_add(self.offset.saturating_mul(self.offset))
    }
}

/// Resolved density functions for the six climate axes.
pub struct ClimateSampler {
    temperature: DensityFunction,
    humidity: DensityFunction,
    continentalness: DensityFunction,
    erosion: DensityFunction,
    depth: DensityFunction,
    weirdness: DensityFunction,
    points: Vec<BiomeParameterPoint>,
}

impl ClimateSampler {
    /// Build from noise_settings router + library (overworld field names).
    pub fn from_settings(
        settings: &NoiseSettings,
        lib: &mut DensityLibrary,
    ) -> Result<Self, String> {
        let r = &settings.noise_router;
        let mut resolve = |key: &str, alt: &str| -> Result<DensityFunction, String> {
            let v = r
                .get(key)
                .or_else(|| r.get(alt))
                .ok_or_else(|| format!("noise_router missing {key}/{alt}"))?;
            lib.resolve(v)
        };
        // JE maps: temperature, vegetation→humidity, continents, erosion, depth, ridges→weirdness.
        Ok(Self {
            temperature: resolve("temperature", "temperature")?,
            humidity: resolve("vegetation", "humidity")?,
            continentalness: resolve("continents", "continentalness")?,
            erosion: resolve("erosion", "erosion")?,
            depth: resolve("depth", "depth")?,
            weirdness: resolve("ridges", "weirdness")?,
            points: overworld_parameter_subset(),
        })
    }

    /// Sample climate at block coordinates (quart sampling is caller's job).
    pub fn sample_target(
        &self,
        x: i32,
        y: i32,
        z: i32,
        lib: &mut DensityLibrary,
    ) -> Result<ClimateTarget, String> {
        let ctx = DensityContext::new(x, y, z);
        let t = self.temperature.compute(ctx, &mut lib.noises)?;
        let h = self.humidity.compute(ctx, &mut lib.noises)?;
        let c = self.continentalness.compute(ctx, &mut lib.noises)?;
        let e = self.erosion.compute(ctx, &mut lib.noises)?;
        let d = self.depth.compute(ctx, &mut lib.noises)?;
        let w = self.weirdness.compute(ctx, &mut lib.noises)?;
        Ok(ClimateTarget::from_floats(t, h, c, e, d, w))
    }

    /// Nearest biome id for this block position (JE samples at quart→block).
    pub fn biome_at(
        &self,
        x: i32,
        y: i32,
        z: i32,
        lib: &mut DensityLibrary,
    ) -> Result<String, String> {
        let bx = quart_to_block(block_to_quart(x));
        let by = quart_to_block(block_to_quart(y));
        let bz = quart_to_block(block_to_quart(z));
        let target = self.sample_target(bx, by, bz, lib)?;
        Ok(find_biome(&self.points, &target).to_owned())
    }
}

/// JE QuartPos.toBlock: quart * 4.
#[inline]
pub fn quart_to_block(quart: i32) -> i32 {
    quart * 4
}

/// Block → quart coordinate (floor div 4).
#[inline]
pub fn block_to_quart(block: i32) -> i32 {
    block.div_euclid(4)
}

pub fn find_biome<'a>(points: &'a [BiomeParameterPoint], target: &ClimateTarget) -> &'a str {
    let mut best = points
        .first()
        .map(|p| p.biome.as_str())
        .unwrap_or("minecraft:plains");
    let mut best_fit = i64::MAX;
    for p in points {
        let f = p.fitness(target);
        if f < best_fit {
            best_fit = f;
            best = p.biome.as_str();
        }
    }
    best
}

/// Compact multi-noise table (subset). Ranges follow wiki / OverworldBiomeBuilder spirit.
fn overworld_parameter_subset() -> Vec<BiomeParameterPoint> {
    #[allow(clippy::too_many_arguments)]
    fn pt(
        t0: f64,
        t1: f64,
        h0: f64,
        h1: f64,
        c0: f64,
        c1: f64,
        e0: f64,
        e1: f64,
        d0: f64,
        d1: f64,
        w0: f64,
        w1: f64,
        biome: &str,
    ) -> BiomeParameterPoint {
        BiomeParameterPoint {
            temperature: ClimateParameter::span(t0, t1),
            humidity: ClimateParameter::span(h0, h1),
            continentalness: ClimateParameter::span(c0, c1),
            erosion: ClimateParameter::span(e0, e1),
            depth: ClimateParameter::span(d0, d1),
            weirdness: ClimateParameter::span(w0, w1),
            offset: 0,
            biome: biome.to_owned(),
        }
    }

    // depth 0 = surface slice; full Y stack later.
    let d0 = 0.0;
    let d1 = 0.0;
    let full_t = (-1.0, 1.0);
    let full_h = (-1.0, 1.0);
    let full_e = (-1.0, 1.0);
    let full_w = (-1.0, 1.0);

    vec![
        // Oceans by continentalness
        pt(
            full_t.0, full_t.1, full_h.0, full_h.1, -1.2, -1.05, full_e.0, full_e.1, d0, d1,
            full_w.0, full_w.1, "minecraft:mushroom_fields",
        ),
        pt(
            -1.0, -0.45, full_h.0, full_h.1, -1.05, -0.455, full_e.0, full_e.1, d0, d1, full_w.0,
            full_w.1, "minecraft:frozen_ocean",
        ),
        pt(
            -0.45, 0.2, full_h.0, full_h.1, -1.05, -0.455, full_e.0, full_e.1, d0, d1, full_w.0,
            full_w.1, "minecraft:ocean",
        ),
        pt(
            0.2, 1.0, full_h.0, full_h.1, -1.05, -0.455, full_e.0, full_e.1, d0, d1, full_w.0,
            full_w.1, "minecraft:warm_ocean",
        ),
        pt(
            full_t.0, full_t.1, full_h.0, full_h.1, -0.455, -0.19, full_e.0, full_e.1, d0, d1,
            full_w.0, full_w.1, "minecraft:ocean",
        ),
        // Coasts
        pt(
            full_t.0, full_t.1, full_h.0, full_h.1, -0.19, -0.11, full_e.0, full_e.1, d0, d1,
            full_w.0, full_w.1, "minecraft:beach",
        ),
        // Rivers: high erosion corridors (approx)
        pt(
            full_t.0, full_t.1, full_h.0, full_h.1, -0.11, 0.55, 0.55, 1.0, d0, d1, full_w.0,
            full_w.1, "minecraft:river",
        ),
        // Land — cold
        pt(
            -1.0, -0.45, full_h.0, full_h.1, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:snowy_plains",
        ),
        pt(
            -0.45, -0.15, -1.0, 0.1, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:taiga",
        ),
        // temperate
        pt(
            -0.15, 0.2, -0.35, 0.4, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:plains",
        ),
        pt(
            -0.15, 0.2, 0.1, 1.0, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:forest",
        ),
        // hot
        pt(
            0.2, 0.55, -1.0, -0.1, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:savanna",
        ),
        pt(
            0.55, 1.0, -1.0, 0.3, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:desert",
        ),
        pt(
            0.2, 1.0, 0.3, 1.0, -0.11, 1.0, full_e.0, 0.55, d0, d1, full_w.0, full_w.1,
            "minecraft:jungle",
        ),
        // mountains / windswept (high weirdness or mid erosion)
        pt(
            full_t.0, full_t.1, full_h.0, full_h.1, 0.03, 1.0, -1.0, -0.3, d0, d1, 0.05, 1.0,
            "minecraft:windswept_hills",
        ),
        // default catch-all
        pt(
            full_t.0, full_t.1, full_h.0, full_h.1, -0.11, 1.0, full_e.0, full_e.1, d0, d1,
            full_w.0, full_w.1, "minecraft:plains",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_round_trip_rough() {
        assert_eq!(quantize_coord(0.5), 5000);
        assert_eq!(quantize_coord(-1.0), -10000);
        assert_eq!(quart_to_block(2), 8);
        assert_eq!(block_to_quart(9), 2);
    }

    #[test]
    fn parameter_distance_inside_is_zero() {
        let p = ClimateParameter::span(-0.5, 0.5);
        assert_eq!(p.distance(quantize_coord(0.0)), 0);
        assert!(p.distance(quantize_coord(0.9)) > 0);
    }

    #[test]
    fn ocean_beats_plains_when_deep_continentalness() {
        let points = overworld_parameter_subset();
        let target = ClimateTarget::from_floats(0.0, 0.0, -0.8, 0.0, 0.0, 0.0);
        let biome = find_biome(&points, &target);
        assert!(
            biome.contains("ocean") || biome.contains("mushroom"),
            "got {biome}"
        );
    }

    #[test]
    fn desert_prefers_hot_dry_inland() {
        let points = overworld_parameter_subset();
        let target = ClimateTarget::from_floats(0.8, -0.5, 0.4, 0.0, 0.0, 0.0);
        let biome = find_biome(&points, &target);
        assert_eq!(biome, "minecraft:desert");
    }
}
