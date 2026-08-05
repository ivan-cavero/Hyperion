//! World generation (ADR 0001: own core, target **vanilla 1:1**).
//!
//! | Module | Role | Parity |
//! |--------|------|--------|
//! | [`scaffold`] | Temporary hills so Play is non-flat | Hyperion-only (same seed ⇒ same *our* world) |
//! | [`vanilla`] | RNG, noise, density functions → future router | Building toward 1:1 |
//!
//! **North star**: `seed S` + protocol 26.2 datapack ⇒ same blocks as the
//! official server for every column we claim. Layers light up with golden
//! diffs in CI; we never silently claim parity for scaffold terrain.
//!
//! Full policy: `docs/WORLDGEN.md`.

pub mod scaffold;
pub mod vanilla;

// Play path still uses the scaffold generator until vanilla columns ship.
pub use scaffold::{
    SEA_LEVEL, ensure_generated_on_disk, generate_column, load_or_generate, spawn_feet_y,
    surface_height, value_noise_2d,
};
