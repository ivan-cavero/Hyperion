//! `PerlinNoise` — octave stack of [`ImprovedNoise`].
//!
//! Modern JE path (`PerlinNoise.create` with non-legacy flag):
//! `forkPositional()` then each non-zero amplitude octave uses
//! `fromHashOf("octave_{firstOctave+i}")`.

use crate::worldgen::improved_noise::ImprovedNoise;
use crate::worldgen::random::{PositionalRandomFactory, RandomSource};

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
    /// Modern create path used by overworld `NormalNoise`
    /// (`forkPositional` + `fromHashOf("octave_N")`).
    pub fn create(random: &mut dyn RandomSource, first_octave: i32, amplitudes: &[f64]) -> Self {
        let n = amplitudes.len();
        // forkPositional() ≡ two nextLong values as factory seeds.
        let factory = PositionalRandomFactory::new(
            random.next_long() as u64,
            random.next_long() as u64,
        );

        let mut octaves = Vec::with_capacity(n);
        for (i, &amp) in amplitudes.iter().enumerate() {
            if amp == 0.0 {
                octaves.push(None);
            } else {
                let octave_index = first_octave + i as i32;
                // JE string concat: "octave_" + octaveIndex
                let name = format!("octave_{octave_index}");
                let mut r = factory.from_hash_of(&name);
                octaves.push(Some(ImprovedNoise::new(&mut r as &mut dyn RandomSource)));
            }
        }

        Self::finish(first_octave, amplitudes, octaves)
    }

    /// Legacy sequential create (`createLegacyForBlendedNoise` style):
    /// each non-zero octave consumes the same RandomSource in order;
    /// zero amps call skipOctave (consume without storing).
    pub fn create_legacy(random: &mut dyn RandomSource, first_octave: i32, amplitudes: &[f64]) -> Self {
        let n = amplitudes.len();
        let mut octaves = Vec::with_capacity(n);
        for &amp in amplitudes {
            if amp == 0.0 {
                skip_octave(random);
                octaves.push(None);
            } else {
                octaves.push(Some(ImprovedNoise::new(random)));
            }
        }
        Self::finish(first_octave, amplitudes, octaves)
    }

    fn finish(
        first_octave: i32,
        amplitudes: &[f64],
        octaves: Vec<Option<ImprovedNoise>>,
    ) -> Self {
        let n = amplitudes.len();
        let lowest_freq_input_factor = 2f64.powi(first_octave);
        let lowest_freq_value_factor = if n == 0 {
            0.0
        } else {
            2f64.powi((n as i32) - 1) / (2f64.powi(n as i32) - 1.0)
        };
        let mut max_value = 0.0;
        let mut factor = lowest_freq_value_factor;
        for &amp in amplitudes {
            max_value += amp * factor;
            factor /= 2.0;
        }

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

/// `PerlinNoise.skipOctave` — JE burns 262 `nextInt()` calls via `consumeCount(262)`.
/// On Xoroshiro, unbounded `nextInt()` is the low 32 bits of `nextLong`.
fn skip_octave(random: &mut dyn RandomSource) {
    for _ in 0..262 {
        let _ = random.next_long();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::random::XoroshiroRandom;

    #[test]
    fn perlin_deterministic() {
        let mut a = XoroshiroRandom::from_seed(42);
        let mut b = XoroshiroRandom::from_seed(42);
        let na = PerlinNoise::create(&mut a, -7, &[1.0, 1.0, 1.0, 0.0]);
        let nb = PerlinNoise::create(&mut b, -7, &[1.0, 1.0, 1.0, 0.0]);
        assert_eq!(na.get_value(1.0, 2.0, 3.0), nb.get_value(1.0, 2.0, 3.0));
    }

    #[test]
    fn zero_amplitude_skips_octave() {
        let mut r = XoroshiroRandom::from_seed(1);
        let n = PerlinNoise::create(&mut r, -3, &[1.0, 0.0, 1.0]);
        assert!(n.get_value(0.0, 0.0, 0.0).is_finite());
    }
}
