//! World generation (ADR 0001: own core → **1:1 with the official Java server**).
//!
//! | Module | Role |
//! |--------|------|
//! | [`random`], noise, [`density`] | **Default generator math** — same algorithms as Java Edition |
//! | [`scaffold`] | **Temporary** hills for Play only until density fill is ready |
//!
//! The end state is one default path: same seed → same blocks as the official
//! server. Scaffold is not “a second product”; it is scaffolding to remove.
//!
//! Policy: `docs/WORLDGEN.md`.

pub mod density;
pub mod improved_noise;
pub mod normal_noise;
pub mod perlin_noise;
pub mod random;
pub mod scaffold;

// Play still uses scaffold until the density-based column fill is ready.
pub use scaffold::{
    SEA_LEVEL, ensure_generated_on_disk, generate_column, load_or_generate, spawn_feet_y,
    surface_height, value_noise_2d,
};

// Default / 1:1 math surface.
pub use density::{DensityContext, DensityFunction, NoiseRegistry, y_clamped_gradient};
pub use normal_noise::{NoiseParameters, NormalNoise};
pub use perlin_noise::PerlinNoise;
pub use random::{LegacyRandom, RandomSource, XoroshiroRandom, world_seed_to_xoroshiro};
