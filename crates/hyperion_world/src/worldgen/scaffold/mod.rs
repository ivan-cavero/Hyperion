//! **Hyperion scaffold terrain** — provisional, **not** vanilla 1:1.
//!
//! Same seed → same *Hyperion* hills. Different algorithms than Mojang’s
//! density/noise router. Used so Play has non-flat columns while the
//! vanilla-aligned generator (`worldgen::vanilla`) is built layer by layer.
//!
//! See `docs/WORLDGEN.md`.

mod noise;
mod surface;

pub use noise::{surface_height, value_noise_2d};
pub use surface::{
    SEA_LEVEL, ensure_generated_on_disk, generate_column, load_or_generate, spawn_feet_y,
};
