//! Density function AST + evaluator (vanilla `DensityFunction` graph).
//!
//! Parses Mojang datapack JSON shapes used under
//! `data/minecraft/worldgen/density_function/` and inline in `noise_settings`.
//!
//! **Parity status**: arithmetic / `y_clamped_gradient` / constants are solid.
//! `noise`, `shifted_noise`, `old_blended_noise`, `spline`, `find_top_surface`,
//! blend wrappers, etc. are stubs or partial — do not claim chunk 1:1 yet.

use std::collections::HashMap;

use serde_json::Value;

use super::normal_noise::{NoiseParameters, NormalNoise};
use super::random::{RandomSource, XoroshiroRandom};

/// Evaluation context for a density sample.
#[derive(Debug, Clone, Copy)]
pub struct DensityContext {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl DensityContext {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Registry of named normal-noise fields used by `minecraft:noise` nodes.
#[derive(Debug, Default)]
pub struct NoiseRegistry {
    params: HashMap<String, NoiseParameters>,
    /// Lazy instances keyed by noise id (built with a fixed seed fork).
    instances: HashMap<String, NormalNoise>,
    seed: i64,
}

impl NoiseRegistry {
    pub fn new(seed: i64) -> Self {
        Self {
            params: HashMap::new(),
            instances: HashMap::new(),
            seed,
        }
    }

    pub fn insert_params(&mut self, id: impl Into<String>, params: NoiseParameters) {
        self.params.insert(id.into(), params);
    }

    pub fn insert_params_json(&mut self, id: &str, json: &Value) -> Result<(), String> {
        self.insert_params(id, NoiseParameters::from_json(json)?);
        Ok(())
    }

    fn noise(&mut self, id: &str) -> Result<&NormalNoise, String> {
        if !self.instances.contains_key(id) {
            let params = self
                .params
                .get(id)
                .ok_or_else(|| format!("unknown noise id {id}"))?
                .clone();
            // Positional fork: hash seed with noise name (approximation of
            // vanilla ResourceKey salt; refined when full NoiseRouter lands).
            let mut salt = self.seed;
            for b in id.as_bytes() {
                salt = salt.wrapping_mul(31).wrapping_add(i64::from(*b));
            }
            let mut random = XoroshiroRandom::from_seed(salt);
            let noise = NormalNoise::create(&mut random as &mut dyn RandomSource, &params);
            self.instances.insert(id.to_owned(), noise);
        }
        Ok(self.instances.get(id).expect("just inserted"))
    }
}

/// Density function node (subset of vanilla types).
#[derive(Debug, Clone)]
pub enum DensityFunction {
    Constant(f64),
    YClampedGradient {
        from_y: i32,
        to_y: i32,
        from_value: f64,
        to_value: f64,
    },
    Add(Box<DensityFunction>, Box<DensityFunction>),
    Mul(Box<DensityFunction>, Box<DensityFunction>),
    Min(Box<DensityFunction>, Box<DensityFunction>),
    Max(Box<DensityFunction>, Box<DensityFunction>),
    Abs(Box<DensityFunction>),
    Square(Box<DensityFunction>),
    Cube(Box<DensityFunction>),
    HalfNegative(Box<DensityFunction>),
    QuarterNegative(Box<DensityFunction>),
    Squeeze(Box<DensityFunction>),
    Clamp {
        input: Box<DensityFunction>,
        min: f64,
        max: f64,
    },
    /// Transparent wrappers used by vanilla for caching / interpolation.
    Interpolated(Box<DensityFunction>),
    FlatCache(Box<DensityFunction>),
    Cache2d(Box<DensityFunction>),
    CacheAllInCell(Box<DensityFunction>),
    BlendDensity(Box<DensityFunction>),
    /// `minecraft:noise` — samples a named NormalNoise field.
    Noise {
        noise_id: String,
        xz_scale: f64,
        y_scale: f64,
    },
    /// Unimplemented type kept so we can load full graphs without panicking at parse.
    Unsupported {
        type_name: String,
    },
}

impl DensityFunction {
    /// Parses a density function JSON value (number or object with `type`).
    pub fn from_json(value: &Value) -> Result<Self, String> {
        match value {
            Value::Number(n) => Ok(Self::Constant(
                n.as_f64().ok_or_else(|| format!("bad number {n}"))?,
            )),
            Value::Object(map) => {
                // Reference form: { "type": "...", ... } or bare constant object?
                let Some(type_v) = map.get("type") else {
                    // Some files are bare numbers only; objects without type unsupported.
                    return Err("density object missing type".to_owned());
                };
                let type_name = type_v
                    .as_str()
                    .ok_or_else(|| "density type must be string".to_owned())?;
                let type_name = type_name.strip_prefix("minecraft:").unwrap_or(type_name);
                parse_typed(type_name, map)
            }
            Value::String(s) => {
                // Holder reference like "minecraft:overworld/base_3d_noise" —
                // resolved by a library loader; keep as unsupported leaf for now.
                Ok(Self::Unsupported {
                    type_name: format!("ref:{s}"),
                })
            }
            other => Err(format!("unsupported density JSON {other}")),
        }
    }

    /// Evaluates this function at `ctx` using `noises` for noise-typed nodes.
    pub fn compute(&self, ctx: DensityContext, noises: &mut NoiseRegistry) -> Result<f64, String> {
        Ok(match self {
            Self::Constant(v) => *v,
            Self::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => y_clamped_gradient(ctx.y, *from_y, *to_y, *from_value, *to_value),
            Self::Add(a, b) => a.compute(ctx, noises)? + b.compute(ctx, noises)?,
            Self::Mul(a, b) => a.compute(ctx, noises)? * b.compute(ctx, noises)?,
            Self::Min(a, b) => a.compute(ctx, noises)?.min(b.compute(ctx, noises)?),
            Self::Max(a, b) => a.compute(ctx, noises)?.max(b.compute(ctx, noises)?),
            Self::Abs(a) => a.compute(ctx, noises)?.abs(),
            Self::Square(a) => {
                let v = a.compute(ctx, noises)?;
                v * v
            }
            Self::Cube(a) => {
                let v = a.compute(ctx, noises)?;
                v * v * v
            }
            Self::HalfNegative(a) => {
                let v = a.compute(ctx, noises)?;
                if v > 0.0 { v } else { v * 0.5 }
            }
            Self::QuarterNegative(a) => {
                let v = a.compute(ctx, noises)?;
                if v > 0.0 { v } else { v * 0.25 }
            }
            Self::Squeeze(a) => {
                // Vanilla squeeze: clamp then cubic squash.
                let v = a.compute(ctx, noises)?.clamp(-1.0, 1.0);
                v / 2.0 - v * v * v / 24.0
            }
            Self::Clamp { input, min, max } => input.compute(ctx, noises)?.clamp(*min, *max),
            Self::Interpolated(a)
            | Self::FlatCache(a)
            | Self::Cache2d(a)
            | Self::CacheAllInCell(a)
            | Self::BlendDensity(a) => a.compute(ctx, noises)?,
            Self::Noise {
                noise_id,
                xz_scale,
                y_scale,
            } => {
                let x = f64::from(ctx.x) * xz_scale;
                let y = f64::from(ctx.y) * y_scale;
                let z = f64::from(ctx.z) * xz_scale;
                // Need re-borrow: get noise then sample.
                let id = noise_id.clone();
                let noise = noises.noise(&id)?;
                noise.get_value(x, y, z)
            }
            Self::Unsupported { type_name } => {
                return Err(format!("density type not implemented: {type_name}"));
            }
        })
    }
}

fn parse_typed(
    type_name: &str,
    map: &serde_json::Map<String, Value>,
) -> Result<DensityFunction, String> {
    use DensityFunction::*;
    match type_name {
        "constant" => {
            let v = map
                .get("argument")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| "constant missing argument".to_owned())?;
            Ok(Constant(v))
        }
        "y_clamped_gradient" => Ok(YClampedGradient {
            from_y: map_i32(map, "from_y")?,
            to_y: map_i32(map, "to_y")?,
            from_value: map_f64(map, "from_value")?,
            to_value: map_f64(map, "to_value")?,
        }),
        "add" => Ok(Add(
            Box::new(arg(map, "argument1")?),
            Box::new(arg(map, "argument2")?),
        )),
        "mul" => Ok(Mul(
            Box::new(arg(map, "argument1")?),
            Box::new(arg(map, "argument2")?),
        )),
        "min" => Ok(Min(
            Box::new(arg(map, "argument1")?),
            Box::new(arg(map, "argument2")?),
        )),
        "max" => Ok(Max(
            Box::new(arg(map, "argument1")?),
            Box::new(arg(map, "argument2")?),
        )),
        "abs" => Ok(Abs(Box::new(arg(map, "argument")?))),
        "square" => Ok(Square(Box::new(arg(map, "argument")?))),
        "cube" => Ok(Cube(Box::new(arg(map, "argument")?))),
        "half_negative" => Ok(HalfNegative(Box::new(arg(map, "argument")?))),
        "quarter_negative" => Ok(QuarterNegative(Box::new(arg(map, "argument")?))),
        "squeeze" => Ok(Squeeze(Box::new(arg(map, "argument")?))),
        "clamp" => Ok(Clamp {
            input: Box::new(arg(map, "input")?),
            min: map_f64(map, "min")?,
            max: map_f64(map, "max")?,
        }),
        "interpolated" => Ok(Interpolated(Box::new(arg(map, "argument")?))),
        "flat_cache" => Ok(FlatCache(Box::new(arg(map, "argument")?))),
        "cache_2d" => Ok(Cache2d(Box::new(arg(map, "argument")?))),
        "cache_all_in_cell" => Ok(CacheAllInCell(Box::new(arg(map, "argument")?))),
        "blend_density" => Ok(BlendDensity(Box::new(arg(map, "argument")?))),
        "noise" => {
            let noise_id = map
                .get("noise")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "noise missing noise id".to_owned())?
                .to_owned();
            Ok(Noise {
                noise_id,
                xz_scale: map_f64(map, "xz_scale").unwrap_or(1.0),
                y_scale: map_f64(map, "y_scale").unwrap_or(1.0),
            })
        }
        other => Ok(Unsupported {
            type_name: other.to_owned(),
        }),
    }
}

fn arg(map: &serde_json::Map<String, Value>, key: &str) -> Result<DensityFunction, String> {
    let v = map
        .get(key)
        .ok_or_else(|| format!("missing density field {key}"))?;
    DensityFunction::from_json(v)
}

fn map_f64(map: &serde_json::Map<String, Value>, key: &str) -> Result<f64, String> {
    map.get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| format!("missing f64 {key}"))
}

fn map_i32(map: &serde_json::Map<String, Value>, key: &str) -> Result<i32, String> {
    map.get(key)
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .ok_or_else(|| format!("missing i32 {key}"))
}

/// Linear clamp-lerp used by `y_clamped_gradient`.
pub fn y_clamped_gradient(y: i32, from_y: i32, to_y: i32, from_value: f64, to_value: f64) -> f64 {
    if from_y == to_y {
        return from_value;
    }
    let t = (f64::from(y - from_y) / f64::from(to_y - from_y)).clamp(0.0, 1.0);
    from_value + t * (to_value - from_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_and_y_gradient() {
        let mut noises = NoiseRegistry::new(0);
        let zero = DensityFunction::from_json(&serde_json::json!(0.0)).unwrap();
        assert_eq!(
            zero.compute(DensityContext::new(0, 0, 0), &mut noises)
                .unwrap(),
            0.0
        );

        let y = DensityFunction::from_json(&serde_json::json!({
            "type": "minecraft:y_clamped_gradient",
            "from_y": 0,
            "to_y": 100,
            "from_value": 0.0,
            "to_value": 1.0
        }))
        .unwrap();
        assert!(
            (y.compute(DensityContext::new(0, 0, 0), &mut noises)
                .unwrap()
                - 0.0)
                .abs()
                < 1e-9
        );
        assert!(
            (y.compute(DensityContext::new(0, 50, 0), &mut noises)
                .unwrap()
                - 0.5)
                .abs()
                < 1e-9
        );
        assert!(
            (y.compute(DensityContext::new(0, 100, 0), &mut noises)
                .unwrap()
                - 1.0)
                .abs()
                < 1e-9
        );
        assert!(
            (y.compute(DensityContext::new(0, 200, 0), &mut noises)
                .unwrap()
                - 1.0)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn add_mul_min() {
        let mut noises = NoiseRegistry::new(0);
        let f = DensityFunction::from_json(&serde_json::json!({
            "type": "minecraft:add",
            "argument1": 1.0,
            "argument2": {
                "type": "minecraft:mul",
                "argument1": 2.0,
                "argument2": 3.0
            }
        }))
        .unwrap();
        assert_eq!(
            f.compute(DensityContext::new(0, 0, 0), &mut noises)
                .unwrap(),
            7.0
        );
    }

    #[test]
    fn noise_node_samples_registry() {
        let mut noises = NoiseRegistry::new(42);
        noises
            .insert_params_json(
                "minecraft:temperature",
                &serde_json::json!({"firstOctave":-10,"amplitudes":[1.5,0.0,1.0,0.0,0.0,0.0]}),
            )
            .unwrap();
        let f = DensityFunction::from_json(&serde_json::json!({
            "type": "minecraft:noise",
            "noise": "minecraft:temperature",
            "xz_scale": 0.25,
            "y_scale": 0.0
        }))
        .unwrap();
        let a = f
            .compute(DensityContext::new(100, 64, -20), &mut noises)
            .unwrap();
        let b = f
            .compute(DensityContext::new(100, 64, -20), &mut noises)
            .unwrap();
        assert_eq!(a, b);
    }
}
