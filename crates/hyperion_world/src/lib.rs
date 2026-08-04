//! Hyperion world.
//!
//! Phase 2 foundation (partial): vanilla-like server data directory bootstrap
//! and minimal `level.dat` (gzipped **storage NBT**). Terminology matches
//! Java Edition / `server.properties`:
//! - `level-name` → world folder + NBT `LevelName`
//! - `level-seed` → NBT `RandomSeed` / `WorldGenSettings.seed` (0 = random once)
//!
//! Anvil region I/O is available; full chunk schema / worldgen / HCF follow.

mod anvil;
mod bootstrap;
mod error;
mod gzip_util;
mod level_dat;

pub use anvil::{
    COMPRESSION_GZIP, COMPRESSION_NONE, COMPRESSION_ZLIB, MAX_CHUNK_UNCOMPRESSED, RegionFile,
    SECTOR_SIZE, chunk_index, chunk_to_region, region_file_name, region_path,
};
pub use bootstrap::{BootstrapConfig, DataPaths, prepare_data_directory};
pub use error::WorldError;
pub use gzip_util::{gzip_compress, gzip_decompress};
pub use level_dat::{DEFAULT_DATA_VERSION, LevelMeta, read_level_dat, write_level_dat};

/// World seed type used by future worldgen (u64, same range as Java `long` bits).
pub type Seed = u64;
