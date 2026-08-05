//! `old_blended_noise` — legacy blended 3D noise used by base terrain.
//!
//! Deterministic reimplementation of the sampling structure used by Java
//! Edition `BlendedNoise` (main + limit perlin stacks). Exact octave amplitudes
//! are refined against official dumps as golden tests land.

use crate::worldgen::perlin_noise::PerlinNoise;
use crate::worldgen::random::{RandomSource, XoroshiroRandom};

/// Parameters from density JSON `old_blended_noise`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendedNoiseParams {
    pub xz_scale: f64,
    pub y_scale: f64,
    pub xz_factor: f64,
    pub y_factor: f64,
    pub smear_scale_multiplier: f64,
}

/// Cached blended noise for one world seed + params.
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    params: BlendedNoiseParams,
    min_limit: PerlinNoise,
    max_limit: PerlinNoise,
    main: PerlinNoise,
}

impl BlendedNoise {
    pub fn create(seed: i64, params: BlendedNoiseParams) -> Self {
        // Three independent streams (mirrors forking three noise generators).
        let mut r0 = XoroshiroRandom::from_seed(seed);
        let mut r1 = XoroshiroRandom::from_seed(seed ^ 0x4F1B_B2C3_D5E6_F708);
        let mut r2 = XoroshiroRandom::from_seed(seed ^ 0x1A2B_3C4D_5E6F_7081);
        // Legacy blended noise used 16 octaves for main and 8 for limits in older
        // code; modern paths still sample multi-octave fields. Amplitudes of 1
        // across octaves approximate the shape; golden tests will pin exact tables.
        let amps_main: Vec<f64> = (0..16).map(|i| 1.0 / 2f64.powi(i)).collect();
        let amps_limit: Vec<f64> = (0..8).map(|i| 1.0 / 2f64.powi(i)).collect();
        Self {
            params,
            min_limit: PerlinNoise::create(&mut r0 as &mut dyn RandomSource, -15, &amps_limit),
            max_limit: PerlinNoise::create(&mut r1 as &mut dyn RandomSource, -15, &amps_limit),
            main: PerlinNoise::create(&mut r2 as &mut dyn RandomSource, -15, &amps_main),
        }
    }

    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let p = self.params;
        let scaled_x = x * p.xz_scale;
        let scaled_y = y * p.y_scale;
        let scaled_z = z * p.xz_scale;

        let limit_x = scaled_x / p.xz_factor;
        let limit_y = scaled_y / p.y_factor;
        let limit_z = scaled_z / p.xz_factor;

        let min = self.min_limit.get_value(limit_x, limit_y, limit_z) / 512.0;
        let max = self.max_limit.get_value(limit_x, limit_y, limit_z) / 512.0;

        let smear = p.smear_scale_multiplier;
        let main =
            self.main
                .get_value(scaled_x / 128.0, scaled_y / 128.0 * smear, scaled_z / 128.0)
                / 64.0;

        // Blend main into [min, max] band (structure of BlendedNoise).
        let lo = min.min(max);
        let hi = min.max(max);
        let t = ((main - lo) / (hi - lo + 1e-9)).clamp(0.0, 1.0);
        lo + t * (hi - lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blended_is_deterministic() {
        let p = BlendedNoiseParams {
            xz_scale: 0.25,
            y_scale: 0.125,
            xz_factor: 80.0,
            y_factor: 160.0,
            smear_scale_multiplier: 8.0,
        };
        let a = BlendedNoise::create(42, p);
        let b = BlendedNoise::create(42, p);
        assert_eq!(a.sample(1.0, 2.0, 3.0), b.sample(1.0, 2.0, 3.0));
        assert_ne!(a.sample(1.0, 2.0, 3.0), a.sample(4.0, 5.0, 6.0));
    }
}
