//! Vanilla-aligned worldgen math (ADR 0001 own core → **1:1 goal**).
//!
//! This module reimplements Mojang’s **algorithms** in pure Rust from public
//! behaviour / datapack schemas. It does **not** copy GPL servers.
//!
//! Status: RNG + Perlin/NormalNoise + density AST foundation. Full
//! `NoiseRouter` → chunk fill is still WIP. Play currently uses
//! [`crate::worldgen::scaffold`] until density-based columns land.

mod density;
mod improved_noise;
mod normal_noise;
mod perlin_noise;
mod random;

pub use density::{DensityContext, DensityFunction, NoiseRegistry, y_clamped_gradient};
pub use normal_noise::{NoiseParameters, NormalNoise};
pub use perlin_noise::PerlinNoise;
pub use random::{LegacyRandom, RandomSource, XoroshiroRandom, world_seed_to_xoroshiro};
