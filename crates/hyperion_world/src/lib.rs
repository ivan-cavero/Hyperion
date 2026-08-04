//! Hyperion world.
//!
//! Phase 2: vanilla-like server data directory bootstrap, minimal `level.dat`,
//! Anvil region I/O, and **chunk columns** (storage NBT + network encoding).
//! Terminology matches Java Edition / `server.properties`:
//! - `level-name` → world folder + NBT `LevelName`
//! - `level-seed` → NBT `RandomSeed` / `WorldGenSettings.seed` (0 = random once)
//!
//! Worldgen / multi-palette sections / HCF follow later in Phase 2.

mod anvil;
mod bootstrap;
mod chunk;
mod error;
mod gzip_util;
mod level_dat;

pub use anvil::{
    COMPRESSION_GZIP, COMPRESSION_NONE, COMPRESSION_ZLIB, MAX_CHUNK_UNCOMPRESSED, RegionFile,
    SECTOR_SIZE, chunk_index, chunk_to_region, region_file_name, region_path,
};
pub use bootstrap::{BootstrapConfig, DataPaths, prepare_data_directory};
pub use chunk::{
    BLOCK_STATE_AIR, BLOCK_STATE_BEDROCK, BLOCK_STATE_STONE, BlockState, ChunkColumn, ChunkSection,
    FlatNetworkCache, MAX_SECTION_Y, MIN_SECTION_Y, PLAINS_BIOME, PLAINS_BIOME_NETWORK_ID,
    SECTION_COUNT, block_to_chunk, ensure_flat_on_disk, ensure_spawn_chunk, load_chunk,
    load_or_create_flat, load_or_flat, position_to_chunk, save_chunk, snap_ground_y,
};
pub use error::WorldError;
pub use gzip_util::{gzip_compress, gzip_decompress};
pub use level_dat::{DEFAULT_DATA_VERSION, LevelMeta, read_level_dat, write_level_dat};

/// World seed type used by future worldgen (u64, same range as Java `long` bits).
pub type Seed = u64;
