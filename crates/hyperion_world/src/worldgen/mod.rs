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

pub mod aquifers;
pub mod blended_noise;
pub mod carvers;
pub mod chunk_fill;
pub mod climate;
pub mod datapack;
pub mod density;
pub mod engine;
pub mod features;
#[cfg(test)]
pub mod golden;
pub mod improved_noise;
pub mod noise_settings;
pub mod normal_noise;
pub mod ore_veins;
pub mod perlin_noise;
pub mod random;
pub mod scaffold;
pub mod spline;
pub mod structures;
pub mod surface_rule_json;
pub mod surface_rules;

// Play still uses scaffold until the density-based column fill is ready.
pub use scaffold::{
    SEA_LEVEL, ensure_generated_on_disk, generate_column, load_or_generate, spawn_feet_y,
    surface_height, value_noise_2d,
};

// Default / 1:1 math surface.
pub use chunk_fill::{
    GenDetail, generate_column_density_only, generate_column_from_density,
    generate_column_from_density_with_detail,
};
pub use datapack::{default_server_inner_jar, find_workspace_root, load_overworld_from_jar};
pub use density::{
    DensityContext, DensityFunction, DensityLibrary, NoiseRegistry, y_clamped_gradient,
};
pub use engine::{ColumnGenerator, WorldgenMode, resolve_server_jar};
pub use noise_settings::{NoiseSettings, NoiseSize};
pub use normal_noise::{NoiseParameters, NormalNoise};
pub use perlin_noise::PerlinNoise;
pub use random::{
    LegacyRandom, PositionalRandomFactory, RandomSource, Seed128, XoroshiroRandom,
    seed_from_hash_of, upgrade_seed_to_128bit, world_seed_to_xoroshiro,
};
pub use surface_rules::{apply_basic_surface, find_spawn_feet_y, recompute_heightmap};
