//! Cubic spline used by density function `minecraft:spline`.
//!
//! Hermite multipoint form matching Java Edition `CubicSpline` (coordinate +
//! points with location / value / derivative; values may nest).

use serde_json::Value;

use crate::worldgen::density::{DensityContext, DensityFunction, DensityLibrary, NoiseRegistry};

/// A cubic spline (constant or multipoint).
#[derive(Debug, Clone)]
pub enum CubicSpline {
    Constant(f64),
    Multipoint {
        coordinate: Box<DensityFunction>,
        points: Vec<SplinePoint>,
    },
}

/// One control point on a multipoint spline.
#[derive(Debug, Clone)]
pub struct SplinePoint {
    pub location: f64,
    pub derivative: f64,
    pub value: CubicSpline,
}

impl CubicSpline {
    /// Parse a spline JSON value (number or `{ coordinate, points }`).
    pub fn from_json(value: &Value, lib: &mut DensityLibrary) -> Result<Self, String> {
        match value {
            Value::Number(n) => Ok(Self::Constant(
                n.as_f64()
                    .ok_or_else(|| format!("bad spline constant {n}"))?,
            )),
            Value::Object(map) => {
                // Nested multipoint without type wrapper, or wrapped under "spline".
                if map.contains_key("points") {
                    parse_multipoint(map, lib)
                } else {
                    Err("spline object missing points".to_owned())
                }
            }
            other => Err(format!("unsupported spline JSON {other}")),
        }
    }

    pub fn compute(&self, ctx: DensityContext, noises: &mut NoiseRegistry) -> Result<f64, String> {
        match self {
            Self::Constant(v) => Ok(*v),
            Self::Multipoint { coordinate, points } => {
                let loc = coordinate.compute(ctx, noises)?;
                apply_multipoint(loc, points, ctx, noises)
            }
        }
    }
}

fn parse_multipoint(
    map: &serde_json::Map<String, Value>,
    lib: &mut DensityLibrary,
) -> Result<CubicSpline, String> {
    let coordinate = lib.resolve(
        map.get("coordinate")
            .ok_or_else(|| "spline missing coordinate".to_owned())?,
    )?;
    let points_json = map
        .get("points")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "spline missing points".to_owned())?;
    if points_json.is_empty() {
        return Err("spline points empty".to_owned());
    }
    let mut points = Vec::with_capacity(points_json.len());
    for p in points_json {
        let obj = p
            .as_object()
            .ok_or_else(|| "spline point must be object".to_owned())?;
        let location = obj
            .get("location")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| "point missing location".to_owned())?;
        let derivative = obj
            .get("derivative")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let value = CubicSpline::from_json(
            obj.get("value")
                .ok_or_else(|| "point missing value".to_owned())?,
            lib,
        )?;
        points.push(SplinePoint {
            location,
            derivative,
            value,
        });
    }
    // Official data is sorted by location; sort defensively.
    points.sort_by(|a, b| {
        a.location
            .partial_cmp(&b.location)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(CubicSpline::Multipoint {
        coordinate: Box::new(coordinate),
        points,
    })
}

fn apply_multipoint(
    location: f64,
    points: &[SplinePoint],
    ctx: DensityContext,
    noises: &mut NoiseRegistry,
) -> Result<f64, String> {
    let first = &points[0];
    let last = points.last().expect("non-empty");
    if location < first.location {
        let v0 = first.value.compute(ctx, noises)?;
        return Ok(v0 + first.derivative * (location - first.location));
    }
    if location > last.location {
        let vn = last.value.compute(ctx, noises)?;
        return Ok(vn + last.derivative * (location - last.location));
    }
    // Find segment i such that points[i].location <= location <= points[i+1].location
    let mut i = 0usize;
    while i + 1 < points.len() && points[i + 1].location < location {
        i += 1;
    }
    if i + 1 >= points.len() {
        return last.value.compute(ctx, noises);
    }
    let p0 = &points[i];
    let p1 = &points[i + 1];
    let range = p1.location - p0.location;
    if range.abs() < 1e-12 {
        return p0.value.compute(ctx, noises);
    }
    let t = ((location - p0.location) / range).clamp(0.0, 1.0);
    let v0 = p0.value.compute(ctx, noises)?;
    let v1 = p1.value.compute(ctx, noises)?;
    let m0 = p0.derivative * range;
    let m1 = p1.derivative * range;
    Ok(hermite(t, v0, v1, m0, m1))
}

/// Java `Mth.hermite` / cubic Hermite between p0 and p1 with slopes m0, m1.
fn hermite(t: f64, p0: f64, p1: f64, m0: f64, m1: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    // h00 = 2t³ - 3t² + 1
    // h10 = t³ - 2t² + t
    // h01 = -2t³ + 3t²
    // h11 = t³ - t²
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    h00 * p0 + h10 * m0 + h01 * p1 + h11 * m1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::density::DensityLibrary;

    #[test]
    fn linear_two_point_spline() {
        let mut lib = DensityLibrary::new(0);
        // coordinate = y mapped via constant? use y_clamped as coord input via constant location
        // Use constant coordinate value by using a constant density as coordinate — wait,
        // we pass location directly via a custom multipoint with coordinate = constant 0.5
        let spline = CubicSpline::from_json(
            &serde_json::json!({
                "coordinate": 0.5,
                "points": [
                    { "location": 0.0, "derivative": 0.0, "value": 0.0 },
                    { "location": 1.0, "derivative": 0.0, "value": 10.0 }
                ]
            }),
            &mut lib,
        )
        .unwrap();
        let v = spline
            .compute(DensityContext::new(0, 0, 0), &mut lib.noises)
            .unwrap();
        // t=0.5, zero derivatives → mid value 5
        assert!((v - 5.0).abs() < 1e-6, "got {v}");
    }
}
