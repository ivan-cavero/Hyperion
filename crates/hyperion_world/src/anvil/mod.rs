//! Anvil region format (`.mca`) — Minecraft Java Edition chunk storage.
//!
//! A region file holds a 32×32 grid of chunks. Layout (vanilla):
//! - 4 KiB location table (1024 × 4 bytes)
//! - 4 KiB timestamp table (1024 × 4 bytes)
//! - payload sectors of 4 KiB each
//!
//! Each occupied chunk entry points at a sector offset + sector count. The
//! chunk payload is: `u32` big-endian length (including the compression byte)
//! + `u8` compression type + compressed bytes.
//!
//! Compression types: `1` = gzip, `2` = zlib (default), `3` = uncompressed.

mod region;

pub use region::{
    COMPRESSION_GZIP, COMPRESSION_NONE, COMPRESSION_ZLIB, MAX_CHUNK_UNCOMPRESSED, RegionFile,
    SECTOR_SIZE, chunk_index, chunk_to_region, region_file_name, region_path,
};
