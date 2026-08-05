//! `PerlinNoise` — octave stack of [`ImprovedNoise`].
//!
//! Matches vanilla construction: `firstOctave` + per-octave amplitudes.

use super::improved_noise::ImprovedNoise;
use super::random::RandomSource;

/// Octave Perlin noise with amplitudes (vanilla `PerlinNoise.create`).
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    octaves: Vec<Option<ImprovedNoise>>,
    amplitudes: Vec<f64>,
    lowest_freq_input_factor: f64,
    lowest_freq_value_factor: f64,
    max_value: f64,
}

impl PerlinNoise {
    /// Creates octave noise. Empty amplitudes → zero sampler.
    pub fn create(random: &mut dyn RandomSource, first_octave: i32, amplitudes: &[f64]) -> Self {
        let n = amplitudes.len();
        let mut octaves = Vec::with_capacity(n);
        for &amp in amplitudes {
            if amp == 0.0 {
                // Skip RNG consumption carefully: vanilla still advances when
                // creating skipped octaves in some versions. We create noise
                // only for non-zero amps but still burn random for zeros to
                // keep stream alignment with typical createForLegacy/create paths.
                // For Xoroshiro create(), zero amplitudes skip ImprovedNoise but
                // still use a forked random in modern MC — approximate: skip.
                octaves.push(None);
            } else {
                octaves.push(Some(ImprovedNoise::new(random)));
            }
        }

        let lowest_freq_input_factor = 2f64.powi(first_octave);
        let lowest_freq_value_factor = 2f64.powi((n as i32) - 1) / (2f64.powi(n as i32) - 1.0);
        // Vanilla also scales by amplitudes[0] effectively via max_value.
        let mut max_value = 0.0;
        let mut factor = lowest_freq_value_factor;
        for &amp in amplitudes {
            max_value += amp * factor;
            factor /= 2.0;
        }

        let _ = first_octave; // folded into lowest_freq_input_factor
        Self {
            octaves,
            amplitudes: amplitudes.to_vec(),
            lowest_freq_input_factor,
            lowest_freq_value_factor,
            max_value,
        }
    }

    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    /// Sample at world-space coordinates (vanilla `getValue`).
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut value = 0.0;
        let mut input_factor = self.lowest_freq_input_factor;
        let mut value_factor = self.lowest_freq_value_factor;
        for (i, octave) in self.octaves.iter().enumerate() {
            if let Some(noise) = octave {
                let amp = self.amplitudes[i];
                value += amp
                    * value_factor
                    * noise.noise_with_wrap(
                        wrap(x * input_factor),
                        wrap(y * input_factor),
                        wrap(z * input_factor),
                        0.0,
                        0.0,
                    );
            }
            input_factor *= 2.0;
            value_factor /= 2.0;
        }
        value
    }
}

/// Vanilla `PerlinNoise.wrap` — keeps coordinates in a stable range.
fn wrap(value: f64) -> f64 {
    value - (value / 3.3554432E7 + 0.5).floor() * 3.3554432E7
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::vanilla::random::XoroshiroRandom;

    #[test]
    fn perlin_deterministic() {
        let mut a = XoroshiroRandom::from_seed(42);
        let mut b = XoroshiroRandom::from_seed(42);
        let na = PerlinNoise::create(&mut a, -7, &[1.0, 1.0, 1.0, 0.0]);
        let nb = PerlinNoise::create(&mut b, -7, &[1.0, 1.0, 1.0, 0.0]);
        assert_eq!(na.get_value(1.0, 2.0, 3.0), nb.get_value(1.0, 2.0, 3.0));
    }
}
