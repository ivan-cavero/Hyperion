//! Hyperion world.
//!
//! Phase 2 foundation (partial): vanilla-like server data directory bootstrap
//! and minimal `level.dat` (gzipped **storage NBT**). Terminology matches
//! Java Edition / `server.properties`:
//! - `level-name` → world folder + NBT `LevelName`
//! - `level-seed` → NBT `RandomSeed` / `WorldGenSettings.seed` (0 = random once)
//!
//! Chunks, worldgen, Anvil, and HCF land later.

mod bootstrap;
mod error;
mod gzip_util;
mod level_dat;

pub use bootstrap::{BootstrapConfig, DataPaths, prepare_data_directory};
pub use error::WorldError;
pub use gzip_util::{gzip_compress, gzip_decompress};
pub use level_dat::{DEFAULT_DATA_VERSION, LevelMeta, read_level_dat, write_level_dat};

/// World seed type used by future worldgen (u64, same range as Java `long` bits).
pub type Seed = u64;
