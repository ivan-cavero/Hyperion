//! Own-core worldgen (ADR 0001) — Phase 2.4 surface scaffold.
//!
//! Deterministic height noise + layered surface (bedrock / stone / dirt / grass).
//! Not vanilla parity yet: no density functions, biomes climate, caves, or
//! features. Same seed always yields the same Hyperion column.

mod noise;
mod surface;

pub use noise::{surface_height, value_noise_2d};
pub use surface::{
    SEA_LEVEL, ensure_generated_on_disk, generate_column, load_or_generate, spawn_feet_y,
};
