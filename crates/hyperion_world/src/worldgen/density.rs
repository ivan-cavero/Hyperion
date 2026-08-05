//! Density function AST + evaluator (Java Edition density graph).
//!
//! Architecture note (learned from open native servers such as Pumpkin’s
//! `generation/noise/router` layout — **GPL, not copied**): keep
//! density math separate from chunk I/O; resolve datapack IDs into a graph;
//! sample `final_density` per block when filling.
//!
//! **Parity**: arithmetic / gradients / range_choice / noise / shifts / spline /
//! old_blended_noise are landing. Full overworld still needs aquifers, surface
//! rules, and golden diffs — do not claim 1:1 yet.

use std::collections::HashMap;

use serde_json::Value;

use crate::worldgen::blended_noise::{BlendedNoise, BlendedNoiseParams};
use crate::worldgen::normal_noise::{NoiseParameters, NormalNoise};
use crate::worldgen::random::{RandomSource, XoroshiroRandom};
use crate::worldgen::spline::CubicSpline;

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

/// Registry of named NormalNoise fields used by noise-typed density nodes.
#[derive(Debug, Default)]
pub struct NoiseRegistry {
    params: HashMap<String, NoiseParameters>,
    instances: HashMap<String, NormalNoise>,
    blended: HashMap<String, BlendedNoise>,
    seed: i64,
}

impl NoiseRegistry {
    pub fn new(seed: i64) -> Self {
        Self {
            params: HashMap::new(),
            instances: HashMap::new(),
            blended: HashMap::new(),
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
                .cloned()
                .or_else(|| {
                    // Allow unprefixed lookup.
                    id.strip_prefix("minecraft:")
                        .and_then(|s| self.params.get(s).cloned())
                })
                .ok_or_else(|| format!("unknown noise id {id}"))?;
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

    fn blended(&mut self, params: BlendedNoiseParams) -> &BlendedNoise {
        let key = format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            params.xz_scale,
            params.y_scale,
            params.xz_factor,
            params.y_factor,
            params.smear_scale_multiplier
        );
        if !self.blended.contains_key(&key) {
            let noise = BlendedNoise::create(self.seed, params);
            self.blended.insert(key.clone(), noise);
        }
        self.blended.get(&key).expect("just inserted")
    }
}

/// Named density functions + noise params from the datapack.
#[derive(Debug)]
pub struct DensityLibrary {
    /// Raw JSON for `worldgen/density_function/<path>` (key = full id).
    raw: HashMap<String, Value>,
    /// Memoized parsed graphs.
    parsed: HashMap<String, DensityFunction>,
    pub noises: NoiseRegistry,
    resolving: Vec<String>,
}

impl DensityLibrary {
    pub fn new(seed: i64) -> Self {
        Self {
            raw: HashMap::new(),
            parsed: HashMap::new(),
            noises: NoiseRegistry::new(seed),
            resolving: Vec::new(),
        }
    }

    pub fn insert_density_json(&mut self, id: impl Into<String>, json: Value) {
        self.raw.insert(normalize_id(&id.into()), json);
    }

    pub fn insert_noise_json(&mut self, id: &str, json: &Value) -> Result<(), String> {
        self.noises.insert_params_json(&normalize_id(id), json)
    }

    /// Resolve a density JSON value (number, object, or string id).
    pub fn resolve(&mut self, value: &Value) -> Result<DensityFunction, String> {
        match value {
            Value::Number(n) => Ok(DensityFunction::Constant(
                n.as_f64().ok_or_else(|| format!("bad number {n}"))?,
            )),
            Value::String(s) => self.resolve_id(s),
            Value::Object(map) => {
                if let Some(type_v) = map.get("type") {
                    let type_name = type_v
                        .as_str()
                        .ok_or_else(|| "density type must be string".to_owned())?;
                    let type_name = type_name.strip_prefix("minecraft:").unwrap_or(type_name);
                    parse_typed(type_name, map, self)
                } else {
                    Err("density object missing type".to_owned())
                }
            }
            other => Err(format!("unsupported density JSON {other}")),
        }
    }

    pub fn resolve_id(&mut self, id: &str) -> Result<DensityFunction, String> {
        let id = normalize_id(id);
        if let Some(parsed) = self.parsed.get(&id) {
            return Ok(parsed.clone());
        }
        if self.resolving.iter().any(|r| r == &id) {
            return Err(format!("cyclic density reference involving {id}"));
        }
        let raw = self
            .raw
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("unknown density function id {id}"))?;
        self.resolving.push(id.clone());
        let parsed = self.resolve(&raw);
        self.resolving.pop();
        let parsed = parsed?;
        self.parsed.insert(id, parsed.clone());
        Ok(parsed)
    }

    /// Number of density function definitions loaded.
    pub fn density_count(&self) -> usize {
        self.raw.len()
    }

    /// Number of noise parameter definitions loaded.
    pub fn noise_param_count(&self) -> usize {
        self.noises.params.len()
    }
}

fn normalize_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// Density function node.
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
    Invert(Box<DensityFunction>),
    HalfNegative(Box<DensityFunction>),
    QuarterNegative(Box<DensityFunction>),
    Squeeze(Box<DensityFunction>),
    Clamp {
        input: Box<DensityFunction>,
        min: f64,
        max: f64,
    },
    RangeChoice {
        input: Box<DensityFunction>,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Box<DensityFunction>,
        when_out_of_range: Box<DensityFunction>,
    },
    Interpolated(Box<DensityFunction>),
    FlatCache(Box<DensityFunction>),
    Cache2d(Box<DensityFunction>),
    CacheOnce(Box<DensityFunction>),
    CacheAllInCell(Box<DensityFunction>),
    BlendDensity(Box<DensityFunction>),
    /// Official blend helpers for old-chunk transitions — stubs.
    BlendAlpha,
    BlendOffset,
    Noise {
        noise_id: String,
        xz_scale: f64,
        y_scale: f64,
    },
    ShiftedNoise {
        noise_id: String,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Box<DensityFunction>,
        shift_y: Box<DensityFunction>,
        shift_z: Box<DensityFunction>,
    },
    /// Sample noise at (x/4, 0, z/4) * 4.
    ShiftA {
        noise_id: String,
    },
    /// Sample noise at (z/4, x/4, 0) * 4.
    ShiftB {
        noise_id: String,
    },
    /// `minecraft:spline` — cubic multipoint spline.
    Spline(CubicSpline),
    /// `minecraft:old_blended_noise` — legacy blended 3D noise.
    OldBlendedNoise(BlendedNoiseParams),
    /// `minecraft:find_top_surface` — scan column for density > 0.
    FindTopSurface {
        density: Box<DensityFunction>,
        upper_bound: Box<DensityFunction>,
        lower_bound: i32,
        cell_height: i32,
    },
    /// `minecraft:interval_select` — pick a function by thresholds on input.
    IntervalSelect {
        input: Box<DensityFunction>,
        thresholds: Vec<f64>,
        functions: Vec<DensityFunction>,
    },
    Unsupported {
        type_name: String,
    },
}

impl DensityFunction {
    /// Parse without library (no string id resolution). Prefer [`DensityLibrary::resolve`].
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let mut lib = DensityLibrary::new(0);
        lib.resolve(value)
    }

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
            Self::Invert(a) => {
                let v = a.compute(ctx, noises)?;
                if v == 0.0 { 0.0 } else { 1.0 / v }
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
                let v = a.compute(ctx, noises)?.clamp(-1.0, 1.0);
                v / 2.0 - v * v * v / 24.0
            }
            Self::Clamp { input, min, max } => input.compute(ctx, noises)?.clamp(*min, *max),
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.compute(ctx, noises)?;
                if v >= *min_inclusive && v < *max_exclusive {
                    when_in_range.compute(ctx, noises)?
                } else {
                    when_out_of_range.compute(ctx, noises)?
                }
            }
            Self::Interpolated(a)
            | Self::FlatCache(a)
            | Self::Cache2d(a)
            | Self::CacheOnce(a)
            | Self::CacheAllInCell(a)
            | Self::BlendDensity(a) => a.compute(ctx, noises)?,
            Self::BlendAlpha => 1.0,
            Self::BlendOffset => 0.0,
            Self::Noise {
                noise_id,
                xz_scale,
                y_scale,
            } => {
                let x = f64::from(ctx.x) * xz_scale;
                let y = f64::from(ctx.y) * y_scale;
                let z = f64::from(ctx.z) * xz_scale;
                let id = noise_id.clone();
                noises.noise(&id)?.get_value(x, y, z)
            }
            Self::ShiftedNoise {
                noise_id,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let sx = shift_x.compute(ctx, noises)?;
                let sy = shift_y.compute(ctx, noises)?;
                let sz = shift_z.compute(ctx, noises)?;
                let x = f64::from(ctx.x) * xz_scale + sx;
                let y = f64::from(ctx.y) * y_scale + sy;
                let z = f64::from(ctx.z) * xz_scale + sz;
                let id = noise_id.clone();
                noises.noise(&id)?.get_value(x, y, z)
            }
            Self::ShiftA { noise_id } => {
                let id = noise_id.clone();
                let n = noises.noise(&id)?.get_value(
                    f64::from(ctx.x) / 4.0,
                    0.0,
                    f64::from(ctx.z) / 4.0,
                );
                n * 4.0
            }
            Self::ShiftB { noise_id } => {
                let id = noise_id.clone();
                let n = noises.noise(&id)?.get_value(
                    f64::from(ctx.z) / 4.0,
                    f64::from(ctx.x) / 4.0,
                    0.0,
                );
                n * 4.0
            }
            Self::Spline(spline) => spline.compute(ctx, noises)?,
            Self::OldBlendedNoise(params) => {
                let p = *params;
                noises
                    .blended(p)
                    .sample(f64::from(ctx.x), f64::from(ctx.y), f64::from(ctx.z))
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let mut y = upper_bound.compute(ctx, noises)? as i32;
                let step = (*cell_height).max(1);
                let lower = *lower_bound;
                while y >= lower {
                    let d = density.compute(DensityContext::new(ctx.x, y, ctx.z), noises)?;
                    if d > 0.0 {
                        return Ok(f64::from(y));
                    }
                    y -= step;
                }
                f64::from(lower)
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let v = input.compute(ctx, noises)?;
                // thresholds.len() == functions.len() - 1
                let mut idx = functions.len() - 1;
                for (i, &t) in thresholds.iter().enumerate() {
                    if v < t {
                        idx = i;
                        break;
                    }
                }
                functions
                    .get(idx)
                    .ok_or_else(|| "interval_select empty functions".to_owned())?
                    .compute(ctx, noises)?
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
    lib: &mut DensityLibrary,
) -> Result<DensityFunction, String> {
    use DensityFunction::*;
    match type_name {
        "constant" => {
            let v = map
                .get("argument")
                .or_else(|| map.get("value"))
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
            Box::new(arg(map, "argument1", lib)?),
            Box::new(arg(map, "argument2", lib)?),
        )),
        "mul" => Ok(Mul(
            Box::new(arg(map, "argument1", lib)?),
            Box::new(arg(map, "argument2", lib)?),
        )),
        "min" => Ok(Min(
            Box::new(arg(map, "argument1", lib)?),
            Box::new(arg(map, "argument2", lib)?),
        )),
        "max" => Ok(Max(
            Box::new(arg(map, "argument1", lib)?),
            Box::new(arg(map, "argument2", lib)?),
        )),
        "abs" => Ok(Abs(Box::new(arg(map, "argument", lib)?))),
        "square" => Ok(Square(Box::new(arg(map, "argument", lib)?))),
        "cube" => Ok(Cube(Box::new(arg(map, "argument", lib)?))),
        "invert" => Ok(Invert(Box::new(arg(map, "argument", lib)?))),
        "half_negative" => Ok(HalfNegative(Box::new(arg(map, "argument", lib)?))),
        "quarter_negative" => Ok(QuarterNegative(Box::new(arg(map, "argument", lib)?))),
        "squeeze" => Ok(Squeeze(Box::new(arg(map, "argument", lib)?))),
        "clamp" => Ok(Clamp {
            input: Box::new(arg(map, "input", lib)?),
            min: map_f64(map, "min")?,
            max: map_f64(map, "max")?,
        }),
        "range_choice" => Ok(RangeChoice {
            input: Box::new(arg(map, "input", lib)?),
            min_inclusive: map_f64(map, "min_inclusive")?,
            max_exclusive: map_f64(map, "max_exclusive")?,
            when_in_range: Box::new(arg(map, "when_in_range", lib)?),
            when_out_of_range: Box::new(arg(map, "when_out_of_range", lib)?),
        }),
        "interpolated" => Ok(Interpolated(Box::new(arg(map, "argument", lib)?))),
        "flat_cache" => Ok(FlatCache(Box::new(arg(map, "argument", lib)?))),
        "cache_2d" => Ok(Cache2d(Box::new(arg(map, "argument", lib)?))),
        "cache_once" => Ok(CacheOnce(Box::new(arg(map, "argument", lib)?))),
        "cache_all_in_cell" => Ok(CacheAllInCell(Box::new(arg(map, "argument", lib)?))),
        "blend_density" => Ok(BlendDensity(Box::new(arg(map, "argument", lib)?))),
        "blend_alpha" => Ok(BlendAlpha),
        "blend_offset" => Ok(BlendOffset),
        "noise" => {
            let noise_id = map
                .get("noise")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "noise missing noise id".to_owned())?
                .to_owned();
            Ok(Noise {
                noise_id: normalize_id(&noise_id),
                xz_scale: map_f64(map, "xz_scale").unwrap_or(1.0),
                y_scale: map_f64(map, "y_scale").unwrap_or(1.0),
            })
        }
        "shifted_noise" => {
            let noise_id = map
                .get("noise")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "shifted_noise missing noise".to_owned())?
                .to_owned();
            Ok(ShiftedNoise {
                noise_id: normalize_id(&noise_id),
                xz_scale: map_f64(map, "xz_scale").unwrap_or(1.0),
                y_scale: map_f64(map, "y_scale").unwrap_or(1.0),
                shift_x: Box::new(arg(map, "shift_x", lib)?),
                shift_y: Box::new(arg(map, "shift_y", lib)?),
                shift_z: Box::new(arg(map, "shift_z", lib)?),
            })
        }
        "shift_a" | "shift" => {
            let noise_id = map
                .get("argument")
                .or_else(|| map.get("noise"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "shift_a missing noise".to_owned())?;
            Ok(ShiftA {
                noise_id: normalize_id(noise_id),
            })
        }
        "shift_b" => {
            let noise_id = map
                .get("argument")
                .or_else(|| map.get("noise"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "shift_b missing noise".to_owned())?;
            Ok(ShiftB {
                noise_id: normalize_id(noise_id),
            })
        }
        "spline" => {
            let spline_json = map
                .get("spline")
                .ok_or_else(|| "spline missing spline field".to_owned())?;
            Ok(Spline(CubicSpline::from_json(spline_json, lib)?))
        }
        "old_blended_noise" => Ok(OldBlendedNoise(BlendedNoiseParams {
            xz_scale: map_f64(map, "xz_scale")?,
            y_scale: map_f64(map, "y_scale")?,
            xz_factor: map_f64(map, "xz_factor")?,
            y_factor: map_f64(map, "y_factor")?,
            smear_scale_multiplier: map_f64(map, "smear_scale_multiplier")?,
        })),
        "find_top_surface" => Ok(FindTopSurface {
            density: Box::new(arg(map, "density", lib)?),
            upper_bound: Box::new(arg(map, "upper_bound", lib)?),
            lower_bound: map_i32(map, "lower_bound")?,
            cell_height: map_i32(map, "cell_height").unwrap_or(1),
        }),
        "interval_select" => {
            let thresholds = map
                .get("thresholds")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "interval_select missing thresholds".to_owned())?
                .iter()
                .map(|v| v.as_f64().ok_or_else(|| format!("bad threshold {v}")))
                .collect::<Result<Vec<_>, _>>()?;
            let functions = map
                .get("functions")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "interval_select missing functions".to_owned())?
                .iter()
                .map(|v| lib.resolve(v))
                .collect::<Result<Vec<_>, _>>()?;
            if functions.len() != thresholds.len() + 1 {
                return Err(format!(
                    "interval_select expects functions = thresholds+1 (got {} / {})",
                    functions.len(),
                    thresholds.len()
                ));
            }
            Ok(IntervalSelect {
                input: Box::new(arg(map, "input", lib)?),
                thresholds,
                functions,
            })
        }
        other => Ok(Unsupported {
            type_name: other.to_owned(),
        }),
    }
}

fn arg(
    map: &serde_json::Map<String, Value>,
    key: &str,
    lib: &mut DensityLibrary,
) -> Result<DensityFunction, String> {
    let v = map
        .get(key)
        .ok_or_else(|| format!("missing density field {key}"))?;
    lib.resolve(v)
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
        let mut lib = DensityLibrary::new(0);
        let zero = lib.resolve(&serde_json::json!(0.0)).unwrap();
        assert_eq!(
            zero.compute(DensityContext::new(0, 0, 0), &mut lib.noises)
                .unwrap(),
            0.0
        );

        let y = lib
            .resolve(&serde_json::json!({
                "type": "minecraft:y_clamped_gradient",
                "from_y": 0,
                "to_y": 100,
                "from_value": 0.0,
                "to_value": 1.0
            }))
            .unwrap();
        assert!(
            (y.compute(DensityContext::new(0, 50, 0), &mut lib.noises)
                .unwrap()
                - 0.5)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn range_choice() {
        let mut lib = DensityLibrary::new(0);
        let f = lib
            .resolve(&serde_json::json!({
                "type": "minecraft:range_choice",
                "input": 0.5,
                "min_inclusive": 0.0,
                "max_exclusive": 1.0,
                "when_in_range": 10.0,
                "when_out_of_range": -10.0
            }))
            .unwrap();
        assert_eq!(
            f.compute(DensityContext::new(0, 0, 0), &mut lib.noises)
                .unwrap(),
            10.0
        );
    }

    #[test]
    fn string_ref_resolves() {
        let mut lib = DensityLibrary::new(0);
        lib.insert_density_json("minecraft:test/const_one", serde_json::json!(1.0));
        let f = lib
            .resolve(&serde_json::json!("minecraft:test/const_one"))
            .unwrap();
        assert_eq!(
            f.compute(DensityContext::new(0, 0, 0), &mut lib.noises)
                .unwrap(),
            1.0
        );
    }

    #[test]
    fn add_mul() {
        let mut lib = DensityLibrary::new(0);
        let f = lib
            .resolve(&serde_json::json!({
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
            f.compute(DensityContext::new(0, 0, 0), &mut lib.noises)
                .unwrap(),
            7.0
        );
    }
}
