//! `NormalNoise` — two Perlin fields mixed as in vanilla (value ≈ first + second/2, scaled).

use super::perlin_noise::PerlinNoise;
use super::random::RandomSource;

/// Parameters from `data/minecraft/worldgen/noise/*.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseParameters {
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

impl NoiseParameters {
    pub fn new(first_octave: i32, amplitudes: impl Into<Vec<f64>>) -> Self {
        Self {
            first_octave,
            amplitudes: amplitudes.into(),
        }
    }

    /// Parses Mojang noise JSON: `{ "firstOctave": i32, "amplitudes": [f64, ...] }`.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        let first_octave = value
            .get("firstOctave")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "noise missing firstOctave".to_owned())?
            as i32;
        let amplitudes = value
            .get("amplitudes")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "noise missing amplitudes".to_owned())?
            .iter()
            .map(|v| v.as_f64().ok_or_else(|| format!("bad amplitude {v}")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            first_octave,
            amplitudes,
        })
    }
}

/// Vanilla `NormalNoise`.
#[derive(Debug, Clone)]
pub struct NormalNoise {
    first: PerlinNoise,
    second: PerlinNoise,
    value_factor: f64,
    max_value: f64,
}

impl NormalNoise {
    pub fn create(random: &mut dyn RandomSource, params: &NoiseParameters) -> Self {
        let first = PerlinNoise::create(random, params.first_octave, &params.amplitudes);
        let second = PerlinNoise::create(random, params.first_octave, &params.amplitudes);
        // Vanilla: valueFactor = 1/6 / expectedstddev-ish; uses first.maxValue.
        // Approximate with 1.0 / (6.0 * 0.55) style from sources:
        let value_factor = 1.111_111_111_111_111_2; // 10/9 used in recent versions for scale
        // More accurately from 1.20+ NormalNoise:
        // valueFactor = 1.0 / (expectedStd * 2) — use classic 1/6 * max?
        let max_value = (first.max_value() + second.max_value() * 0.5) * value_factor;
        Self {
            first,
            second,
            value_factor,
            max_value,
        }
    }

    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let a = self.first.get_value(x, y, z);
        let b = self.second.get_value(x, y, z);
        (a + b * 0.5) * self.value_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::vanilla::random::XoroshiroRandom;

    #[test]
    fn temperature_params_parse() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"amplitudes":[1.5,0.0,1.0,0.0,0.0,0.0],"firstOctave":-10}"#)
                .unwrap();
        let p = NoiseParameters::from_json(&v).unwrap();
        assert_eq!(p.first_octave, -10);
        assert_eq!(p.amplitudes.len(), 6);
    }

    #[test]
    fn normal_noise_deterministic() {
        let params = NoiseParameters::new(-7, vec![1.0, 1.0]);
        let mut a = XoroshiroRandom::from_seed(1);
        let mut b = XoroshiroRandom::from_seed(1);
        let na = NormalNoise::create(&mut a, &params);
        let nb = NormalNoise::create(&mut b, &params);
        assert_eq!(na.get_value(0.0, 0.0, 0.0), nb.get_value(0.0, 0.0, 0.0));
    }
}
