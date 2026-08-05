//! `NormalNoise` — two Perlin fields mixed as in vanilla JE.
//!
//! `getValue(x,y,z) = (first(x,y,z) + second(x·IF, y·IF, z·IF)) * valueFactor`
//! with `INPUT_FACTOR ≈ 1.0181268882175227` and
//! `valueFactor = (1/6) / expectedDeviation(maxNonZero - minNonZero)`.

use crate::worldgen::perlin_noise::PerlinNoise;
use crate::worldgen::random::RandomSource;

/// JE `NormalNoise.INPUT_FACTOR`.
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;

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
        let value_factor = value_factor_for_amplitudes(&params.amplitudes);
        let max_value = (first.max_value() + second.max_value()) * value_factor;
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
        let b = self.second.get_value(
            x * INPUT_FACTOR,
            y * INPUT_FACTOR,
            z * INPUT_FACTOR,
        );
        (a + b) * self.value_factor
    }
}

/// `NormalNoise.expectedDeviation(octaveSpan)`.
fn expected_deviation(octave_span: i32) -> f64 {
    0.1 * (1.0 + 1.0 / f64::from(octave_span + 1))
}

/// `valueFactor = (1/6) / expectedDeviation(maxIdx - minIdx)` over non-zero amps.
fn value_factor_for_amplitudes(amplitudes: &[f64]) -> f64 {
    let mut min_i = i32::MAX;
    let mut max_i = i32::MIN;
    for (i, &amp) in amplitudes.iter().enumerate() {
        if amp != 0.0 {
            let i = i as i32;
            min_i = min_i.min(i);
            max_i = max_i.max(i);
        }
    }
    if min_i == i32::MAX {
        return 1.0;
    }
    (1.0 / 6.0) / expected_deviation(max_i - min_i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::random::XoroshiroRandom;

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

    #[test]
    fn value_factor_matches_je_formula() {
        // amps with non-zero at 0 and 2 → span 2
        // expectedDeviation(2) = 0.1 * (1 + 1/3) = 0.1 * 4/3 ≈ 0.1333...
        // valueFactor = (1/6) / that
        let vf = value_factor_for_amplitudes(&[1.0, 0.0, 1.0]);
        let expected = (1.0 / 6.0) / expected_deviation(2);
        assert!((vf - expected).abs() < 1e-12);
    }
}
