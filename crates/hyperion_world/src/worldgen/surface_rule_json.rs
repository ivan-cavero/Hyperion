//! First-pass interpreter for datapack `surface_rule` JSON.
//!
//! Full JE surface system needs biomes, noise thresholds, steepness, bandlands,
//! and preliminary surface. This module walks the tree for rules we understand
//! and returns a block when a branch matches; unknown conditions fail closed
//! (condition false) so we never invent wrong surface for unhandled biomes.
//!
//! Used as a **supplement** after the basic surface pass for bedrock-style
//! rules and simple `block` leaves. Expands rule-by-rule toward 1:1.

use serde_json::Value;

use crate::chunk::BlockState;
use crate::worldgen::random::{PositionalRandomFactory, RandomSource};

/// Context for one cell while evaluating a surface rule.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceCtx {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub min_y: i32,
    pub sea_level: i32,
    /// Highest solid Y in this column (for stone_depth / above_preliminary).
    pub surface_y: i32,
    pub seed: i64,
}

/// Evaluate a surface rule node; `Some` means this rule places a block.
pub fn eval_rule(rule: &Value, ctx: &SurfaceCtx) -> Option<BlockState> {
    let map = rule.as_object()?;
    let type_name = map
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .strip_prefix("minecraft:")
        .unwrap_or_else(|| map.get("type").and_then(|v| v.as_str()).unwrap_or(""));

    match type_name {
        "block" => {
            let name = map
                .get("result_state")
                .and_then(|s| s.get("Name"))
                .and_then(|n| n.as_str())?;
            Some(BlockState::new(name))
        }
        "sequence" => {
            let seq = map.get("sequence")?.as_array()?;
            for step in seq {
                if let Some(b) = eval_rule(step, ctx) {
                    return Some(b);
                }
            }
            None
        }
        "condition" => {
            let if_true = map.get("if_true")?;
            let then_run = map.get("then_run")?;
            if eval_condition(if_true, ctx) {
                eval_rule(then_run, ctx)
            } else {
                None
            }
        }
        // Unimplemented material rules — leave to basic surface.
        "bandlands" => None,
        _ => None,
    }
}

fn eval_condition(cond: &Value, ctx: &SurfaceCtx) -> bool {
    let Some(map) = cond.as_object() else {
        return false;
    };
    let type_name = map
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .strip_prefix("minecraft:")
        .unwrap_or("");

    match type_name {
        "vertical_gradient" => eval_vertical_gradient(map, ctx),
        "above_preliminary_surface" => ctx.y >= ctx.surface_y - 8 && ctx.y <= ctx.surface_y,
        "stone_depth" => {
            // Approximate: depth from top solid (offset 0 floor → only surface cell).
            let offset = map.get("offset").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let depth = ctx.surface_y - ctx.y;
            depth == offset
        }
        "water" => {
            let offset = map
                .get("offset")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let surface_depth_mul = map
                .get("surface_depth_multiplier")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let _ = surface_depth_mul;
            ctx.y < ctx.sea_level + offset
        }
        "y_above" => {
            let anchor = map.get("anchor").unwrap_or(map.get("y").unwrap_or(&Value::Null));
            let y = resolve_y_anchor(anchor, ctx.min_y).unwrap_or(i32::MIN);
            let add_surface = map
                .get("add_stone_depth")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let thr = if add_surface {
                y + (ctx.surface_y - ctx.y).max(0)
            } else {
                y
            };
            ctx.y >= thr
        }
        "not" => {
            let inner = map.get("invert").or_else(|| map.get("condition"));
            inner.map(|c| !eval_condition(c, ctx)).unwrap_or(false)
        }
        // Biome / noise / steep / hole / temperature: fail closed until implemented.
        "biome" | "noise_threshold" | "steep" | "hole" | "temperature" => false,
        _ => false,
    }
}

fn eval_vertical_gradient(
    map: &serde_json::Map<String, Value>,
    ctx: &SurfaceCtx,
) -> bool {
    let true_at = resolve_y_anchor(map.get("true_at_and_below").unwrap_or(&Value::Null), ctx.min_y)
        .unwrap_or(ctx.min_y);
    let false_at =
        resolve_y_anchor(map.get("false_at_and_above").unwrap_or(&Value::Null), ctx.min_y)
            .unwrap_or(ctx.min_y + 5);
    if ctx.y <= true_at {
        return true;
    }
    if ctx.y >= false_at {
        return false;
    }
    // Linear probability between true_at and false_at.
    let span = (false_at - true_at).max(1);
    let t = f64::from(ctx.y - true_at) / f64::from(span);
    let p = 1.0 - t; // high near true_at
    let name = map
        .get("random_name")
        .and_then(|v| v.as_str())
        .unwrap_or("minecraft:bedrock_floor");
    let factory = PositionalRandomFactory::from_world_seed(ctx.seed);
    let mut r = factory.from_hash_of(name);
    let salt = r.next_long() as u64;
    let h = salt
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(ctx.x as u64)
        ^ (ctx.y as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (ctx.z as u64);
    let unit = (h as f64) / (u64::MAX as f64);
    unit < p
}

fn resolve_y_anchor(v: &Value, min_y: i32) -> Option<i32> {
    if let Some(n) = v.as_i64() {
        return Some(n as i32);
    }
    let obj = v.as_object()?;
    if let Some(ab) = obj.get("above_bottom").and_then(|x| x.as_i64()) {
        return Some(min_y + ab as i32);
    }
    if let Some(b) = obj.get("absolute").and_then(|x| x.as_i64()) {
        return Some(b as i32);
    }
    if let Some(bt) = obj.get("below_top").and_then(|x| x.as_i64()) {
        // Approximate top as min_y + 384 for overworld-sized worlds.
        return Some(min_y + 384 - 1 - bt as i32);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vertical_gradient_bedrock_at_floor() {
        let rule = json!({
            "type": "minecraft:condition",
            "if_true": {
                "type": "minecraft:vertical_gradient",
                "false_at_and_above": { "above_bottom": 5 },
                "random_name": "minecraft:bedrock_floor",
                "true_at_and_below": { "above_bottom": 0 }
            },
            "then_run": {
                "type": "minecraft:block",
                "result_state": { "Name": "minecraft:bedrock" }
            }
        });
        let ctx = SurfaceCtx {
            x: 0,
            y: -64,
            z: 0,
            min_y: -64,
            sea_level: 63,
            surface_y: 64,
            seed: 1,
        };
        assert_eq!(eval_rule(&rule, &ctx), Some(BlockState::bedrock()));
        let above = SurfaceCtx { y: -59, ..ctx };
        // At false_at (min_y+5) must not be bedrock from this rule alone.
        assert_eq!(eval_rule(&rule, &above), None);
    }
}
