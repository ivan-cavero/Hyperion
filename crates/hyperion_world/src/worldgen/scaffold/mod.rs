//! **Temporary** Play terrain only — **not** the 1:1 default generator.
//!
//! Same seed → same *scaffold* hills. Different algorithms than the official
//! density/noise router. Exists only so clients can explore non-flat land while
//! the real generator (`worldgen::{density, noise, …}`) is finished.
//!
//! See `docs/WORLDGEN.md`.

mod noise;
mod surface;

pub use noise::{surface_height, value_noise_2d};
pub use surface::{
    SEA_LEVEL, ensure_generated_on_disk, generate_column, load_or_generate, spawn_feet_y,
};
