//! Hyperion world.
//!
//! Phase 2: vanilla-like server data directory bootstrap, minimal `level.dat`,
//! Anvil region I/O, and **chunk columns** (single- and multi-palette sections).
//! Terminology matches Java Edition / `server.properties`:
//! - `level-name` → world folder + NBT `LevelName`
//! - `level-seed` → NBT `RandomSeed` / `WorldGenSettings.seed` (0 = random once)
//!
//! Own-core worldgen (ADR 0001): default behaviour aims at **1:1** with the
//! official Java server; scaffold is temporary for Play (see `docs/WORLDGEN.md`).

mod anvil;
mod bootstrap;
mod chunk;
mod error;
mod generated;
mod gzip_util;
mod level_dat;
mod worldgen;

pub use anvil::{
    COMPRESSION_GZIP, COMPRESSION_NONE, COMPRESSION_ZLIB, MAX_CHUNK_UNCOMPRESSED, RegionFile,
    SECTOR_SIZE, chunk_index, chunk_to_region, region_file_name, region_path,
};
pub use bootstrap::{BootstrapConfig, DataPaths, prepare_data_directory};
pub use chunk::{
    BLOCK_STATE_AIR, BLOCK_STATE_BEDROCK, BLOCK_STATE_DIRT, BLOCK_STATE_GRASS_BLOCK,
    BLOCK_STATE_STONE, BlockState, ChunkColumn, ChunkSection, FlatNetworkCache, MAX_SECTION_Y,
    MIN_SECTION_Y, PLAINS_BIOME, PLAINS_BIOME_NETWORK_ID, SECTION_COUNT, block_to_chunk,
    ensure_flat_on_disk, ensure_spawn_chunk, load_chunk, load_or_create_flat, load_or_flat,
    position_to_chunk, save_chunk, section_index, snap_ground_y,
};
pub use error::WorldError;
pub use generated::block_states;
pub use gzip_util::{gzip_compress, gzip_decompress};
pub use level_dat::{DEFAULT_DATA_VERSION, LevelMeta, read_level_dat, write_level_dat};
pub use worldgen::{
    ColumnGenerator, DensityContext, DensityFunction, DensityLibrary, GenDetail, LegacyRandom,
    NoiseParameters, NoiseRegistry, NoiseSettings, NoiseSize, NormalNoise, PerlinNoise,
    PositionalRandomFactory, RandomSource, SEA_LEVEL, Seed128, WorldgenMode, XoroshiroRandom,
    apply_basic_surface, default_server_inner_jar, ensure_generated_on_disk, find_spawn_feet_y,
    find_workspace_root, generate_column, generate_column_density_only, generate_column_from_density,
    generate_column_from_density_with_detail, load_or_generate, load_overworld_from_jar,
    recompute_heightmap, resolve_server_jar, seed_from_hash_of, spawn_feet_y, surface_height,
    upgrade_seed_to_128bit, value_noise_2d, world_seed_to_xoroshiro, y_clamped_gradient,
};

/// World seed type used by future worldgen (u64, same range as Java `long` bits).
pub type Seed = u64;
