//! `old_blended_noise` — legacy blended 3D noise used by base terrain.
//!
//! Structure follows Java Edition `BlendedNoise`:
//! - one shared RNG stream creates min-limit, max-limit, then main Perlin stacks
//! - sample min/max at scaled coords; map main into [0,1] and lerp between them
//!
//! Octave tables still refined against official golden dumps.

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
        // Official path: one RandomSource, construct min → max → main in order.
        let mut random = XoroshiroRandom::from_seed(seed);
        // Octaves at indices [-15..=0] for limits (16), [-7..=0] for main (8).
        let amps_limit = vec![1.0; 16];
        let amps_main = vec![1.0; 8];
        let min_limit = PerlinNoise::create(&mut random as &mut dyn RandomSource, -15, &amps_limit);
        let max_limit = PerlinNoise::create(&mut random as &mut dyn RandomSource, -15, &amps_limit);
        let main = PerlinNoise::create(&mut random as &mut dyn RandomSource, -7, &amps_main);
        Self {
            params,
            min_limit,
            max_limit,
            main,
        }
    }

    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let p = self.params;
        let scaled_x = x * p.xz_scale;
        let scaled_y = y * p.y_scale;
        let scaled_z = z * p.xz_scale;

        // Limit noise uses factor-scaled coordinates.
        let limit_x = scaled_x / p.xz_factor;
        let limit_y = scaled_y / p.y_factor;
        let limit_z = scaled_z / p.xz_factor;

        // Smear stretches Y for main noise (caves / overhangs).
        let smear_y = scaled_y * p.smear_scale_multiplier;

        let min = self.min_limit.get_value(limit_x, limit_y, limit_z) / 512.0;
        let max = self.max_limit.get_value(limit_x, limit_y, limit_z) / 512.0;

        // Main noise drives the blend factor (historical /512 or /20 paths exist;
        // /20 + 0.5 then clamp is the common JE mapping for the lerp weight).
        let main = self.main.get_value(scaled_x, smear_y, scaled_z) / 20.0 + 0.5;
        let t = main.clamp(0.0, 1.0);

        clamped_lerp(min, max, t)
    }
}

#[inline]
fn clamped_lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
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

    #[test]
    fn sample_is_finite() {
        let p = BlendedNoiseParams {
            xz_scale: 0.25,
            y_scale: 0.125,
            xz_factor: 80.0,
            y_factor: 160.0,
            smear_scale_multiplier: 8.0,
        };
        let n = BlendedNoise::create(1, p);
        for z in -2..2 {
            for y in -2..2 {
                for x in -2..2 {
                    let v = n.sample(f64::from(x), f64::from(y), f64::from(z));
                    assert!(v.is_finite(), "non-finite at {x},{y},{z}: {v}");
                }
            }
        }
    }
}
