//! Surface terrain generator: heightmap + stone/dirt/grass layers.

use std::path::Path;

use crate::anvil::{RegionFile, region_path};
use hyperion_protocol::BLOCK_SECTION_SIZE;

use super::noise::surface_height;
use crate::chunk::{
    BlockState, ChunkColumn, ChunkSection, MIN_SECTION_Y, PLAINS_BIOME, SECTION_COUNT,
    section_index,
};
use crate::error::WorldError;
use crate::level_dat::DEFAULT_DATA_VERSION;

/// Vanilla-ish sea level (not used for water yet; documents the height band).
pub const SEA_LEVEL: i32 = 63;
/// Dirt layers below the grass block (inclusive count of dirt-only cells).
const DIRT_DEPTH: i32 = 3;

/// Compact block kinds used while filling a section (avoids String thrash).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GenBlock {
    Air,
    Bedrock,
    Stone,
    Dirt,
    Grass,
}

impl GenBlock {
    fn to_state(self) -> BlockState {
        match self {
            Self::Air => BlockState::air(),
            Self::Bedrock => BlockState::bedrock(),
            Self::Stone => BlockState::stone(),
            Self::Dirt => BlockState::dirt(),
            Self::Grass => BlockState::grass_block(),
        }
    }
}

/// Feet Y for spawn at block (0, 0) for this seed (`surface + 1`).
pub fn spawn_feet_y(seed: u64) -> i32 {
    surface_height(0, 0, seed) + 1
}

/// Generates one overworld column for `(chunk_x, chunk_z)`.
pub fn generate_column(seed: u64, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    // Local heightmap: highest solid Y per column (index = z * 16 + x).
    let mut heights = [0i32; 256];
    let mut heightmap_net = [0u16; 256];
    let mut max_surface = i32::MIN;
    for lz in 0..16i32 {
        for lx in 0..16i32 {
            let h = surface_height(base_x + lx, base_z + lz, seed);
            let idx = (lz * 16 + lx) as usize;
            heights[idx] = h;
            // Vanilla heightmap: first empty block above the surface (feet-ish).
            let surface = (h + 1).clamp(0, 511) as u16;
            heightmap_net[idx] = surface;
            max_surface = max_surface.max(h + 1);
        }
    }

    let min_block_y = i32::from(MIN_SECTION_Y) * 16;
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for i in 0..SECTION_COUNT {
        let section_y = MIN_SECTION_Y + i as i8;
        let y0 = i32::from(section_y) * 16;
        sections.push(build_section(section_y, y0, min_block_y, &heights));
    }

    ChunkColumn {
        x: chunk_x,
        z: chunk_z,
        min_section_y: MIN_SECTION_Y,
        sections,
        surface_y: max_surface.max(min_block_y),
        heightmap: heightmap_net,
        data_version: DEFAULT_DATA_VERSION,
    }
}

fn build_section(section_y: i8, y0: i32, min_block_y: i32, heights: &[i32; 256]) -> ChunkSection {
    let mut cells = [GenBlock::Air; BLOCK_SECTION_SIZE];
    let mut first = None;
    let mut uniform = true;

    for lz in 0..16u8 {
        for lx in 0..16u8 {
            let h = heights[(u16::from(lz) * 16 + u16::from(lx)) as usize];
            for ly in 0..16u8 {
                let world_y = y0 + i32::from(ly);
                let block = block_at(world_y, h, min_block_y);
                let idx = section_index(lx, ly, lz);
                cells[idx] = block;
                match first {
                    None => first = Some(block),
                    Some(f) if f != block => uniform = false,
                    _ => {}
                }
            }
        }
    }

    if uniform {
        let block = first.unwrap_or(GenBlock::Air).to_state();
        return if block.is_air() {
            ChunkSection::air(section_y, PLAINS_BIOME)
        } else {
            ChunkSection::solid(section_y, block, PLAINS_BIOME)
        };
    }

    let states: Vec<BlockState> = cells.iter().map(|b| b.to_state()).collect();
    ChunkSection::from_blocks(section_y, &states, PLAINS_BIOME)
}

fn block_at(world_y: i32, surface: i32, min_block_y: i32) -> GenBlock {
    if world_y > surface {
        GenBlock::Air
    } else if world_y == min_block_y {
        GenBlock::Bedrock
    } else if world_y == surface {
        GenBlock::Grass
    } else if world_y >= surface - DIRT_DEPTH {
        GenBlock::Dirt
    } else {
        GenBlock::Stone
    }
}

/// Loads a chunk from Anvil or generates + saves a new terrain column.
pub fn load_or_generate(
    world_dir: impl AsRef<Path>,
    seed: u64,
    chunk_x: i32,
    chunk_z: i32,
) -> Result<(ChunkColumn, bool), WorldError> {
    let world_dir = world_dir.as_ref();
    if let Some(existing) = crate::chunk::load_chunk(world_dir, chunk_x, chunk_z)? {
        return Ok((existing, false));
    }
    let column = generate_column(seed, chunk_x, chunk_z);
    crate::chunk::save_chunk(world_dir, &column)?;
    Ok((column, true))
}

/// Ensures a terrain column exists on disk (header check; generate only if missing).
pub fn ensure_generated_on_disk(
    world_dir: impl AsRef<Path>,
    seed: u64,
    chunk_x: i32,
    chunk_z: i32,
) -> Result<bool, WorldError> {
    let world_dir = world_dir.as_ref();
    let (rx, rz) = crate::anvil::chunk_to_region(chunk_x, chunk_z);
    let path = region_path(world_dir, rx, rz);
    let region = RegionFile::open_or_create(&path)?;
    if region.has_chunk(chunk_x, chunk_z)? {
        return Ok(false);
    }
    let column = generate_column(seed, chunk_x, chunk_z);
    let nbt = column.encode_storage_nbt()?;
    region.write_chunk(chunk_x, chunk_z, &nbt)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_world(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "hyperion-gen-{label}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("region")).expect("mkdir");
        dir
    }

    #[test]
    fn column_has_grass_surface_and_air_above() {
        let col = generate_column(42, 0, 0);
        let h = surface_height(0, 0, 42);
        assert_eq!(col.get_block(0, h, 0), BlockState::grass_block());
        assert_eq!(col.get_block(0, h + 1, 0), BlockState::air());
        if h > i32::from(MIN_SECTION_Y) * 16 {
            assert_eq!(col.get_block(0, h - 1, 0), BlockState::dirt());
        }
        // Deep stone.
        assert_eq!(col.get_block(0, 0, 0), BlockState::stone());
        // Bedrock floor.
        assert_eq!(
            col.get_block(0, i32::from(MIN_SECTION_Y) * 16, 0),
            BlockState::bedrock()
        );
    }

    #[test]
    fn same_seed_same_column() {
        let a = generate_column(7, 3, -2);
        let b = generate_column(7, 3, -2);
        assert_eq!(a.surface_y, b.surface_y);
        assert_eq!(
            a.get_block(5, a.surface_y - 1, 5),
            b.get_block(5, b.surface_y - 1, 5)
        );
        assert_eq!(a.heightmap, b.heightmap);
    }

    #[test]
    fn different_chunks_differ() {
        let a = generate_column(1, 0, 0);
        let b = generate_column(1, 5, 0);
        // Heights almost certainly differ somewhere; check heightmaps.
        assert_ne!(a.heightmap, b.heightmap);
    }

    #[test]
    fn network_payload_encodes() {
        let col = generate_column(99, 0, 0);
        let payload = col.encode_network_payload(true).expect("encode");
        assert!(payload.len() > 50_000);
        assert_eq!(&payload[..4], &0i32.to_be_bytes());
    }

    #[test]
    fn load_or_generate_persists() {
        let world = temp_world("persist");
        let (first, created) = load_or_generate(&world, 55, 1, 2).expect("gen");
        assert!(created);
        let (second, created_again) = load_or_generate(&world, 55, 1, 2).expect("load");
        assert!(!created_again);
        assert_eq!(first.surface_y, second.surface_y);
        assert_eq!(
            first.get_block(0, first.surface_y - 1, 0),
            second.get_block(0, second.surface_y - 1, 0)
        );
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn spawn_feet_matches_height_at_origin() {
        let seed = 123u64;
        assert_eq!(spawn_feet_y(seed), surface_height(0, 0, seed) + 1);
    }
}
