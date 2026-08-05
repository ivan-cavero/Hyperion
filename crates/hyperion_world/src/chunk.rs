//! Anvil chunk column model, storage NBT, and network encoding helpers.
//!
//! Supports:
//! - Single-valued sections (flat world hot path)
//! - Multi-palette sections (`set_block` / arbitrary terrain)
//! - Storage NBT round-trip (1.18+ Anvil layout)
//! - Network `level_chunk_with_light` via [`NetworkChunkSection`]

use std::path::Path;

use hyperion_protocol::{
    BLOCK_SECTION_SIZE, HEIGHTMAP_MOTION_BLOCKING, HEIGHTMAP_WORLD_SURFACE, NbtTag,
    NetworkChunkSection, NetworkHeightmap, NetworkPalettedContainer, PaletteKind, ProtocolError,
    bits_needed, decode_named_tag, encode_chunk_payload, encode_named_tag, pack_heightmap_values,
    pack_simple_bit_storage, simple_bit_storage_long_count,
};

use crate::anvil::{RegionFile, region_path};
use crate::error::WorldError;
use crate::generated::block_states;
use crate::level_dat::DEFAULT_DATA_VERSION;

/// Lowest section Y in the overworld (block Y -64).
pub const MIN_SECTION_Y: i8 = -4;
/// Number of 16-block sections in a default overworld column (Y -64..=319).
pub const SECTION_COUNT: usize = 24;
/// Highest section Y (`MIN_SECTION_Y + SECTION_COUNT - 1`).
pub const MAX_SECTION_Y: i8 = 19;

/// Biome resource location used for flat spawn columns.
pub const PLAINS_BIOME: &str = "minecraft:plains";
/// Network biome id for `minecraft:plains` in our join registry (first entry).
pub const PLAINS_BIOME_NETWORK_ID: i32 = 0;

// ---------------------------------------------------------------------------
// Block-state ids (from generated 26.2 report)
// ---------------------------------------------------------------------------

/// Global palette id for `minecraft:air`.
pub const BLOCK_STATE_AIR: i32 = block_states::AIR;
/// Global palette id for `minecraft:stone` (default state).
pub const BLOCK_STATE_STONE: i32 = block_states::STONE;
/// Global palette id for `minecraft:bedrock` (default state).
pub const BLOCK_STATE_BEDROCK: i32 = block_states::BEDROCK;
/// Global palette id for `minecraft:dirt` (default state).
pub const BLOCK_STATE_DIRT: i32 = block_states::DIRT;
/// Global palette id for `minecraft:grass_block` (default state).
pub const BLOCK_STATE_GRASS_BLOCK: i32 = block_states::GRASS_BLOCK;

/// A block state identified by resource location (Anvil palette entry).
///
/// Phase 2: default state only (no property map). Property-aware states land
/// with placement/worldgen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockState {
    /// e.g. `minecraft:stone`
    pub name: String,
}

impl BlockState {
    /// Builds a namespaced block state (`minecraft:` prefix added when missing).
    pub fn new(name: impl Into<String>) -> Self {
        let mut name = name.into();
        if !name.contains(':') {
            name = format!("minecraft:{name}");
        }
        Self { name }
    }

    /// Air.
    pub fn air() -> Self {
        Self::new("minecraft:air")
    }

    /// Stone.
    pub fn stone() -> Self {
        Self::new("minecraft:stone")
    }

    /// Bedrock.
    pub fn bedrock() -> Self {
        Self::new("minecraft:bedrock")
    }

    /// Dirt.
    pub fn dirt() -> Self {
        Self::new("minecraft:dirt")
    }

    /// Grass block (default `snowy=false`).
    pub fn grass_block() -> Self {
        Self::new("minecraft:grass_block")
    }

    /// Whether this is air (including cave/void air names).
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "air"
        )
    }

    /// Whether this is a fluid block for `fluidCount` (default states).
    pub fn is_fluid(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:water"
                | "minecraft:lava"
                | "minecraft:flowing_water"
                | "minecraft:flowing_lava"
        )
    }

    /// Global palette id for the network encoder (default state from 26.2 report).
    ///
    /// Unknown names fall back to stone so a wrong id cannot become glass/water.
    pub fn network_id(&self) -> i32 {
        match self.name.as_str() {
            "minecraft:air" | "air" => BLOCK_STATE_AIR,
            "minecraft:cave_air" => block_states::CAVE_AIR,
            "minecraft:void_air" => block_states::VOID_AIR,
            other => block_states::default_state_id(other).unwrap_or(BLOCK_STATE_STONE),
        }
    }
}

/// Block storage for one 16×16×16 section.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SectionBlocks {
    /// Entire section is one block (flat / uniform fill).
    Single(BlockState),
    /// Multi-palette: unique states + 4096 local indices (YZX order).
    Multi {
        palette: Vec<BlockState>,
        indices: Box<[u16; BLOCK_SECTION_SIZE]>,
    },
}

/// One 16×16×16 section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSection {
    /// Section Y index (e.g. -4 for the bottom overworld section).
    pub y: i8,
    blocks: SectionBlocks,
    /// Biome resource location filling the entire section (single-value for now).
    pub biome: String,
}

impl ChunkSection {
    /// All-air section.
    pub fn air(y: i8, biome: impl Into<String>) -> Self {
        Self {
            y,
            blocks: SectionBlocks::Single(BlockState::air()),
            biome: normalize_biome(biome),
        }
    }

    /// Solid single-block section.
    pub fn solid(y: i8, block: BlockState, biome: impl Into<String>) -> Self {
        Self {
            y,
            blocks: SectionBlocks::Single(block),
            biome: normalize_biome(biome),
        }
    }

    /// Whether every cell is air.
    pub fn is_all_air(&self) -> bool {
        match &self.blocks {
            SectionBlocks::Single(b) => b.is_air(),
            SectionBlocks::Multi { palette, indices } => {
                indices.iter().all(|&i| palette[i as usize].is_air())
            }
        }
    }

    /// Block at local coordinates (0..16 each).
    pub fn get_block(&self, x: u8, y: u8, z: u8) -> BlockState {
        debug_assert!(x < 16 && y < 16 && z < 16);
        match &self.blocks {
            SectionBlocks::Single(b) => b.clone(),
            SectionBlocks::Multi { palette, indices } => {
                let idx = section_index(x, y, z);
                palette[indices[idx] as usize].clone()
            }
        }
    }

    /// Sets a block at local coordinates, promoting single → multi as needed.
    pub fn set_block(&mut self, x: u8, y: u8, z: u8, state: BlockState) {
        debug_assert!(x < 16 && y < 16 && z < 16);
        let idx = section_index(x, y, z);
        match &mut self.blocks {
            SectionBlocks::Single(current) => {
                if *current == state {
                    return;
                }
                let old = current.clone();
                let mut palette = vec![old];
                let mut indices = Box::new([0u16; BLOCK_SECTION_SIZE]);
                let new_i = palette_index(&mut palette, state);
                indices[idx] = new_i;
                self.blocks = SectionBlocks::Multi { palette, indices };
            }
            SectionBlocks::Multi { palette, indices } => {
                let pi = palette_index(palette, state);
                indices[idx] = pi;
            }
        }
    }

    /// Non-air block count (0..=4096).
    pub fn non_air_count(&self) -> i16 {
        match &self.blocks {
            SectionBlocks::Single(b) => {
                if b.is_air() {
                    0
                } else {
                    4096
                }
            }
            SectionBlocks::Multi { palette, indices } => indices
                .iter()
                .filter(|&&i| !palette[i as usize].is_air())
                .count() as i16,
        }
    }

    /// Fluid cell count for the network section header.
    pub fn fluid_count(&self) -> i16 {
        match &self.blocks {
            SectionBlocks::Single(b) => {
                if b.is_fluid() {
                    4096
                } else {
                    0
                }
            }
            SectionBlocks::Multi { palette, indices } => indices
                .iter()
                .filter(|&&i| palette[i as usize].is_fluid())
                .count() as i16,
        }
    }

    /// Builds a section from 4096 block states in YZX order.
    ///
    /// Collapses to a single-valued section when every cell is identical.
    pub fn from_blocks(y: i8, blocks: &[BlockState], biome: impl Into<String>) -> Self {
        assert_eq!(
            blocks.len(),
            BLOCK_SECTION_SIZE,
            "section requires {BLOCK_SECTION_SIZE} blocks"
        );
        let first = &blocks[0];
        if blocks.iter().all(|b| b == first) {
            return if first.is_air() {
                Self::air(y, biome)
            } else {
                Self::solid(y, first.clone(), biome)
            };
        }
        let mut palette: Vec<BlockState> = Vec::new();
        let mut indices = Box::new([0u16; BLOCK_SECTION_SIZE]);
        for (i, state) in blocks.iter().enumerate() {
            indices[i] = palette_index(&mut palette, state.clone());
        }
        Self {
            y,
            blocks: SectionBlocks::Multi { palette, indices },
            biome: normalize_biome(biome),
        }
    }

    /// Network encoding of this section.
    pub fn to_network(&self) -> NetworkChunkSection {
        let biome_id = PLAINS_BIOME_NETWORK_ID;
        let biomes = NetworkPalettedContainer::single(biome_id);
        let blocks = match &self.blocks {
            SectionBlocks::Single(b) => NetworkPalettedContainer::single(b.network_id()),
            SectionBlocks::Multi { palette, indices } => {
                let global: Vec<i32> = palette.iter().map(BlockState::network_id).collect();
                NetworkPalettedContainer::from_palette_indices(
                    PaletteKind::Blocks,
                    &global,
                    indices.as_ref(),
                )
                .unwrap_or_else(|_| NetworkPalettedContainer::single(BLOCK_STATE_STONE))
            }
        };
        NetworkChunkSection::with_containers(
            self.non_air_count(),
            self.fluid_count(),
            blocks,
            biomes,
        )
    }

    /// True when storage is single-valued (used by flat network cache path).
    pub fn is_single_valued(&self) -> bool {
        matches!(self.blocks, SectionBlocks::Single(_))
    }

    /// Single fill block when single-valued; `None` if multi.
    pub fn single_block(&self) -> Option<&BlockState> {
        match &self.blocks {
            SectionBlocks::Single(b) => Some(b),
            SectionBlocks::Multi { .. } => None,
        }
    }

    fn to_block_states_nbt(&self) -> NbtTag {
        match &self.blocks {
            SectionBlocks::Single(block) => {
                let palette = NbtTag::List(vec![block_state_compound(block)]);
                NbtTag::Compound(vec![("palette".to_owned(), palette)])
            }
            SectionBlocks::Multi { palette, indices } => {
                let palette_tag =
                    NbtTag::List(palette.iter().map(block_state_compound).collect::<Vec<_>>());
                let bits = bits_for_block_storage(palette.len());
                let longs = pack_simple_bit_storage(bits, indices.iter().map(|&i| u32::from(i)));
                NbtTag::Compound(vec![
                    ("palette".to_owned(), palette_tag),
                    ("data".to_owned(), NbtTag::LongArray(longs)),
                ])
            }
        }
    }

    fn from_block_states_nbt(states: &[(String, NbtTag)]) -> Result<SectionBlocks, WorldError> {
        let palette_tags = find_list(states, "palette")?;
        if palette_tags.is_empty() {
            return Ok(SectionBlocks::Single(BlockState::air()));
        }
        let mut palette = Vec::with_capacity(palette_tags.len());
        for entry in palette_tags {
            palette.push(parse_block_palette_entry(entry)?);
        }
        if palette.len() == 1 {
            // Single-valued: data array optional/absent.
            return Ok(SectionBlocks::Single(palette.pop().expect("len 1")));
        }

        let data = match find_tag(states, "data") {
            Ok(NbtTag::LongArray(longs)) => longs.as_slice(),
            Ok(_) => {
                return Err(WorldError::InvalidChunk(
                    "block_states.data must be a long array".to_owned(),
                ));
            }
            Err(_) => {
                return Err(WorldError::InvalidChunk(
                    "multi-entry block palette missing data".to_owned(),
                ));
            }
        };

        let bits = bits_for_block_storage(palette.len());
        let expected_longs = simple_bit_storage_long_count(bits, BLOCK_SECTION_SIZE);
        if data.len() < expected_longs {
            return Err(WorldError::InvalidChunk(format!(
                "block_states.data too short: {} < {expected_longs}",
                data.len()
            )));
        }
        let indices = unpack_simple_bit_storage(bits, BLOCK_SECTION_SIZE, data);
        let max = (palette.len() - 1) as u16;
        for &i in indices.iter() {
            if i > max {
                return Err(WorldError::InvalidChunk(format!(
                    "block palette index {i} out of range (palette {})",
                    palette.len()
                )));
            }
        }
        Ok(SectionBlocks::Multi {
            palette,
            indices: Box::new(indices),
        })
    }
}

/// A full chunk column (24 overworld sections by default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkColumn {
    /// Chunk X (global).
    pub x: i32,
    /// Chunk Z (global).
    pub z: i32,
    /// Lowest section Y stored in NBT `yPos` (overworld: -4).
    pub min_section_y: i8,
    /// Ordered sections from bottom to top (`min_section_y` ..).
    pub sections: Vec<ChunkSection>,
    /// Absolute world Y of the highest solid block + 1 (max feet-ish height
    /// across the column; used for spawn heuristics).
    pub surface_y: i32,
    /// Per-column heightmap values (index = `z * 16 + x`), absolute Y of the
    /// first empty block above the surface (vanilla WORLD_SURFACE packing).
    pub heightmap: [u16; 256],
    /// `DataVersion` written into storage NBT.
    pub data_version: i32,
}

impl ChunkColumn {
    /// Builds an empty (all-air) overworld column.
    pub fn empty(x: i32, z: i32) -> Self {
        let sections = (0..SECTION_COUNT)
            .map(|i| ChunkSection::air(MIN_SECTION_Y + i as i8, PLAINS_BIOME))
            .collect();
        let surface = (i32::from(MIN_SECTION_Y) * 16).clamp(0, 511) as u16;
        Self {
            x,
            z,
            min_section_y: MIN_SECTION_Y,
            sections,
            surface_y: i32::from(MIN_SECTION_Y) * 16, // no surface
            heightmap: [surface; 256],
            data_version: DEFAULT_DATA_VERSION,
        }
    }

    /// Flat stone platform: stone from the bottom of the world up through
    /// `ground_y` (inclusive, snapped down to a section top), air above.
    ///
    /// Uses only `minecraft:stone` for solid fill. `ground_y` is the highest
    /// solid block Y; player feet should be `ground_y + 1`.
    pub fn flat(x: i32, z: i32, ground_y: i32) -> Self {
        let ground_y = snap_ground_y(ground_y);
        let solid_max_section = (ground_y.div_euclid(16)) as i8;
        let mut sections = Vec::with_capacity(SECTION_COUNT);
        for i in 0..SECTION_COUNT {
            let y = MIN_SECTION_Y + i as i8;
            let section = if y <= solid_max_section {
                ChunkSection::solid(y, BlockState::stone(), PLAINS_BIOME)
            } else {
                ChunkSection::air(y, PLAINS_BIOME)
            };
            sections.push(section);
        }
        let surface = (ground_y + 1).clamp(0, 511) as u16;
        Self {
            x,
            z,
            min_section_y: MIN_SECTION_Y,
            sections,
            surface_y: ground_y + 1,
            heightmap: [surface; 256],
            data_version: DEFAULT_DATA_VERSION,
        }
    }

    /// Sets a block in world coordinates, creating multi-palette sections as needed.
    ///
    /// Returns `false` if Y is outside the column's section range.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: BlockState) -> bool {
        let section_y = y.div_euclid(16) as i8;
        let Some(section) = self.sections.iter_mut().find(|s| s.y == section_y) else {
            return false;
        };
        let lx = (x.rem_euclid(16)) as u8;
        let ly = (y.rem_euclid(16)) as u8;
        let lz = (z.rem_euclid(16)) as u8;
        section.set_block(lx, ly, lz, state);
        true
    }

    /// Gets a block in world coordinates; air if outside the column.
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockState {
        let section_y = y.div_euclid(16) as i8;
        let Some(section) = self.sections.iter().find(|s| s.y == section_y) else {
            return BlockState::air();
        };
        let lx = (x.rem_euclid(16)) as u8;
        let ly = (y.rem_euclid(16)) as u8;
        let lz = (z.rem_euclid(16)) as u8;
        section.get_block(lx, ly, lz)
    }

    /// Encodes this column as storage NBT (named root `""`).
    pub fn encode_storage_nbt(&self) -> Result<Vec<u8>, WorldError> {
        let tag = self.to_nbt_tag();
        encode_named_tag("", &tag).map_err(|error| WorldError::Nbt(error.to_string()))
    }

    /// Decodes a storage-NBT column (Anvil uncompressed payload).
    pub fn decode_storage_nbt(bytes: &[u8]) -> Result<Self, WorldError> {
        let (name, tag) =
            decode_named_tag(bytes).map_err(|error| WorldError::Nbt(error.to_string()))?;
        if !name.is_empty() {
            return Err(WorldError::InvalidChunk(format!(
                "expected empty root name, got {name:?}"
            )));
        }
        Self::from_nbt_tag(&tag)
    }

    /// Encodes the clientbound `level_chunk_with_light` payload for this column.
    pub fn encode_network_payload(&self, has_sky_light: bool) -> Result<Vec<u8>, ProtocolError> {
        let sections: Vec<NetworkChunkSection> =
            self.sections.iter().map(ChunkSection::to_network).collect();
        let packed = pack_heightmap_values(&self.heightmap);
        let heightmaps = [
            NetworkHeightmap {
                type_id: HEIGHTMAP_WORLD_SURFACE,
                data: packed.clone(),
            },
            NetworkHeightmap {
                type_id: HEIGHTMAP_MOTION_BLOCKING,
                data: packed,
            },
        ];
        encode_chunk_payload(self.x, self.z, &heightmaps, &sections, has_sky_light)
    }

    fn to_nbt_tag(&self) -> NbtTag {
        let mut section_tags = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            let block_states = section.to_block_states_nbt();
            let biome_palette = NbtTag::List(vec![NbtTag::String(section.biome.clone())]);
            let biomes = NbtTag::Compound(vec![("palette".to_owned(), biome_palette)]);
            section_tags.push(NbtTag::Compound(vec![
                ("Y".to_owned(), NbtTag::Byte(section.y)),
                ("block_states".to_owned(), block_states),
                ("biomes".to_owned(), biomes),
            ]));
        }

        let packed = pack_heightmap_values(&self.heightmap);
        let heightmaps = NbtTag::Compound(vec![
            (
                "WORLD_SURFACE".to_owned(),
                NbtTag::LongArray(packed.clone()),
            ),
            ("MOTION_BLOCKING".to_owned(), NbtTag::LongArray(packed)),
        ]);

        NbtTag::Compound(vec![
            ("DataVersion".to_owned(), NbtTag::Int(self.data_version)),
            ("xPos".to_owned(), NbtTag::Int(self.x)),
            ("zPos".to_owned(), NbtTag::Int(self.z)),
            (
                "yPos".to_owned(),
                NbtTag::Int(i32::from(self.min_section_y)),
            ),
            (
                "Status".to_owned(),
                NbtTag::String("minecraft:full".to_owned()),
            ),
            ("LastUpdate".to_owned(), NbtTag::Long(0)),
            ("sections".to_owned(), NbtTag::List(section_tags)),
            ("Heightmaps".to_owned(), heightmaps),
            ("isLightOn".to_owned(), NbtTag::Byte(1)),
            ("block_entities".to_owned(), NbtTag::List(vec![])),
        ])
    }

    fn from_nbt_tag(tag: &NbtTag) -> Result<Self, WorldError> {
        let NbtTag::Compound(root) = tag else {
            return Err(WorldError::InvalidChunk(
                "chunk root is not a compound".to_owned(),
            ));
        };
        let data_version = find_int(root, "DataVersion").unwrap_or(DEFAULT_DATA_VERSION);
        let x = find_int(root, "xPos")?;
        let z = find_int(root, "zPos")?;
        let y_pos = find_int(root, "yPos").unwrap_or(i32::from(MIN_SECTION_Y));
        let min_section_y = y_pos as i8;

        let sections_tag = find_list(root, "sections")?;
        let mut sections = Vec::with_capacity(sections_tag.len());
        for entry in sections_tag {
            let NbtTag::Compound(sec) = entry else {
                return Err(WorldError::InvalidChunk(
                    "section entry is not a compound".to_owned(),
                ));
            };
            let y = find_byte(sec, "Y")?;
            let blocks = match find_optional_compound(sec, "block_states") {
                Some(states) => ChunkSection::from_block_states_nbt(states)?,
                None => SectionBlocks::Single(BlockState::air()),
            };
            let biome = parse_section_biome(sec)?;
            sections.push(ChunkSection { y, blocks, biome });
        }
        sections.sort_by_key(|s| s.y);

        if sections.is_empty() {
            for i in 0..SECTION_COUNT {
                sections.push(ChunkSection::air(min_section_y + i as i8, PLAINS_BIOME));
            }
        }

        let heightmap = parse_heightmap(root).unwrap_or_else(|| {
            let surface_y = sections
                .iter()
                .rev()
                .find(|s| !s.is_all_air())
                .map(|s| (i32::from(s.y) + 1) * 16)
                .unwrap_or(i32::from(min_section_y) * 16)
                .clamp(0, 511) as u16;
            [surface_y; 256]
        });
        let surface_y = heightmap.iter().copied().map(i32::from).max().unwrap_or(0);

        Ok(Self {
            x,
            z,
            min_section_y,
            sections,
            surface_y,
            heightmap,
            data_version,
        })
    }
}

/// Cached network encoding of a **flat** column shared by every (x, z).
///
/// Flat worlds only differ by chunk coordinates in the packet header. Rebuilding
/// ~50 KiB of full-bright light per column is what made flying feel slower than
/// vanilla; a template clone+patch is the correct hot path until multi-palette
/// worldgen needs unique section data.
#[derive(Debug, Clone)]
pub struct FlatNetworkCache {
    /// Fully encoded payload for chunk (0, 0).
    template: Vec<u8>,
    /// Snapped ground Y used to build the template.
    pub ground_y: i32,
    /// Feet Y for spawn on this platform.
    pub surface_y: i32,
}

impl FlatNetworkCache {
    /// Builds the template once for the given preferred ground Y.
    pub fn new(ground_y: i32) -> Result<Self, ProtocolError> {
        let ground_y = snap_ground_y(ground_y);
        let column = ChunkColumn::flat(0, 0, ground_y);
        let template = column.encode_network_payload(true)?;
        Ok(Self {
            template,
            ground_y,
            surface_y: column.surface_y,
        })
    }

    /// Clones the template and patches `chunk_x` / `chunk_z` (first 8 bytes).
    pub fn payload(&self, chunk_x: i32, chunk_z: i32) -> Vec<u8> {
        let mut bytes = self.template.clone();
        bytes[0..4].copy_from_slice(&chunk_x.to_be_bytes());
        bytes[4..8].copy_from_slice(&chunk_z.to_be_bytes());
        bytes
    }
}

// ---------------------------------------------------------------------------
// Anvil I/O helpers
// ---------------------------------------------------------------------------

/// Loads a chunk column from the world directory, or `None` if missing.
pub fn load_chunk(
    world_dir: impl AsRef<Path>,
    chunk_x: i32,
    chunk_z: i32,
) -> Result<Option<ChunkColumn>, WorldError> {
    let world_dir = world_dir.as_ref();
    let (rx, rz) = crate::anvil::chunk_to_region(chunk_x, chunk_z);
    let path = region_path(world_dir, rx, rz);
    if !path.exists() {
        return Ok(None);
    }
    let region = RegionFile::open(&path)?;
    let Some(bytes) = region.read_chunk(chunk_x, chunk_z)? else {
        return Ok(None);
    };
    Ok(Some(ChunkColumn::decode_storage_nbt(&bytes)?))
}

/// Writes a chunk column into the world directory's Anvil region files.
pub fn save_chunk(world_dir: impl AsRef<Path>, column: &ChunkColumn) -> Result<(), WorldError> {
    let world_dir = world_dir.as_ref();
    let (rx, rz) = crate::anvil::chunk_to_region(column.x, column.z);
    let path = region_path(world_dir, rx, rz);
    let region = RegionFile::open_or_create(&path)?;
    let nbt = column.encode_storage_nbt()?;
    region.write_chunk(column.x, column.z, &nbt)
}

/// Ensures chunk `(0, 0)` exists as a flat stone platform.
///
/// Idempotent: does not overwrite an existing chunk. Returns the column
/// (loaded or newly written) and whether it was created.
pub fn ensure_spawn_chunk(
    world_dir: impl AsRef<Path>,
    ground_y: i32,
) -> Result<(ChunkColumn, bool), WorldError> {
    load_or_create_flat(world_dir, 0, 0, ground_y)
}

/// Loads chunk `(chunk_x, chunk_z)` or builds a flat column (without saving).
pub fn load_or_flat(
    world_dir: impl AsRef<Path>,
    chunk_x: i32,
    chunk_z: i32,
    ground_y: i32,
) -> Result<ChunkColumn, WorldError> {
    if let Some(existing) = load_chunk(world_dir, chunk_x, chunk_z)? {
        return Ok(existing);
    }
    Ok(ChunkColumn::flat(chunk_x, chunk_z, ground_y))
}

/// Loads a chunk or generates a flat one and **persists** it to Anvil.
///
/// Returns `(column, created)` where `created` is true when the file was
/// written for the first time. Used by Play streaming so explored terrain
/// survives restarts (Phase 2.2).
pub fn load_or_create_flat(
    world_dir: impl AsRef<Path>,
    chunk_x: i32,
    chunk_z: i32,
    ground_y: i32,
) -> Result<(ChunkColumn, bool), WorldError> {
    let world_dir = world_dir.as_ref();
    if let Some(existing) = load_chunk(world_dir, chunk_x, chunk_z)? {
        return Ok((existing, false));
    }
    let column = ChunkColumn::flat(chunk_x, chunk_z, ground_y);
    save_chunk(world_dir, &column)?;
    Ok((column, true))
}

/// Ensures a flat column exists on disk **without** reading/decoding NBT when
/// the slot is already occupied (header check only).
///
/// Prefer this on the streaming hot path: network uses [`FlatNetworkCache`];
/// disk is append-only when missing.
pub fn ensure_flat_on_disk(
    world_dir: impl AsRef<Path>,
    chunk_x: i32,
    chunk_z: i32,
    ground_y: i32,
) -> Result<bool, WorldError> {
    let world_dir = world_dir.as_ref();
    let (rx, rz) = crate::anvil::chunk_to_region(chunk_x, chunk_z);
    let path = region_path(world_dir, rx, rz);
    let region = RegionFile::open_or_create(&path)?;
    if region.has_chunk(chunk_x, chunk_z)? {
        return Ok(false);
    }
    let column = ChunkColumn::flat(chunk_x, chunk_z, ground_y);
    let nbt = column.encode_storage_nbt()?;
    region.write_chunk(chunk_x, chunk_z, &nbt)?;
    Ok(true)
}

/// Converts a block coordinate to a chunk coordinate (vanilla `>> 4` / floor div).
#[inline]
pub fn block_to_chunk(block: i32) -> i32 {
    block.div_euclid(16)
}

/// Converts a player/entity world position to chunk coordinates.
#[inline]
pub fn position_to_chunk(x: f64, z: f64) -> (i32, i32) {
    (
        block_to_chunk(x.floor() as i32),
        block_to_chunk(z.floor() as i32),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Vanilla section index: `((y & 15) << 8) | ((z & 15) << 4) | (x & 15)`.
#[inline]
pub fn section_index(x: u8, y: u8, z: u8) -> usize {
    ((u16::from(y & 15) << 8) | (u16::from(z & 15) << 4) | u16::from(x & 15)) as usize
}

/// Storage / network bits for a block palette of the given length (26.2 strategy).
fn bits_for_block_storage(palette_len: usize) -> u8 {
    if palette_len <= 1 {
        0
    } else {
        // Same as network indirect: min 4 bits for 2..=16 entries.
        bits_needed(palette_len).max(4)
    }
}

fn palette_index(palette: &mut Vec<BlockState>, state: BlockState) -> u16 {
    if let Some(pos) = palette.iter().position(|b| b == &state) {
        return pos as u16;
    }
    let idx = palette.len() as u16;
    palette.push(state);
    idx
}

fn block_state_compound(block: &BlockState) -> NbtTag {
    NbtTag::Compound(vec![(
        "Name".to_owned(),
        NbtTag::String(block.name.clone()),
    )])
}

fn parse_block_palette_entry(tag: &NbtTag) -> Result<BlockState, WorldError> {
    let NbtTag::Compound(entries) = tag else {
        return Err(WorldError::InvalidChunk(
            "block palette entry is not a compound".to_owned(),
        ));
    };
    let name = find_string(entries, "Name")?;
    Ok(BlockState::new(name))
}

/// Unpacks SimpleBitStorage into a fixed array of `entry_count` values.
fn unpack_simple_bit_storage(
    bits: u8,
    entry_count: usize,
    data: &[i64],
) -> [u16; BLOCK_SECTION_SIZE] {
    assert!(entry_count <= BLOCK_SECTION_SIZE);
    let mut out = [0u16; BLOCK_SECTION_SIZE];
    if bits == 0 || entry_count == 0 {
        return out;
    }
    let bits_u = bits as u32;
    let values_per_long = 64 / bits_u;
    let mask = if bits_u == 32 {
        u64::MAX
    } else {
        (1u64 << bits_u) - 1
    };
    for (i, slot) in out.iter_mut().enumerate().take(entry_count) {
        let cell = i / values_per_long as usize;
        let offset = ((i as u32) % values_per_long) * bits_u;
        let word = data.get(cell).copied().unwrap_or(0) as u64;
        *slot = ((word >> offset) & mask) as u16;
    }
    out
}

/// Snaps `ground_y` down to the top of its section so single-value sections
/// stay valid (whole section solid or air).
///
/// Example: 99 → 95 (top of section Y=5), 63 → 63, 64 → 63.
pub fn snap_ground_y(ground_y: i32) -> i32 {
    let min_block = i32::from(MIN_SECTION_Y) * 16;
    let max_block = (i32::from(MAX_SECTION_Y) + 1) * 16 - 1;
    let y = ground_y.clamp(min_block, max_block);
    let section = y.div_euclid(16);
    let top = (section + 1) * 16 - 1;
    if top == y {
        y
    } else {
        // Previous section top (or min if already at bottom section start).
        let prev_top = section * 16 - 1;
        prev_top.max(min_block)
    }
}

fn normalize_biome(biome: impl Into<String>) -> String {
    let mut biome = biome.into();
    if !biome.contains(':') {
        biome = format!("minecraft:{biome}");
    }
    biome
}

fn parse_section_biome(sec: &[(String, NbtTag)]) -> Result<String, WorldError> {
    let Some(biomes) = find_optional_compound(sec, "biomes") else {
        return Ok(PLAINS_BIOME.to_owned());
    };
    let palette = find_list(biomes, "palette")?;
    match palette.first() {
        Some(NbtTag::String(name)) => Ok(normalize_biome(name.clone())),
        Some(NbtTag::Compound(entries)) => find_string(entries, "Name").map(normalize_biome),
        _ => Ok(PLAINS_BIOME.to_owned()),
    }
}

/// Unpacks WORLD_SURFACE into 256 height values (compact 9-bit packing).
fn parse_heightmap(root: &[(String, NbtTag)]) -> Option<[u16; 256]> {
    let heightmaps = find_optional_compound(root, "Heightmaps")?;
    let longs = match find_tag(heightmaps, "WORLD_SURFACE").ok()? {
        NbtTag::LongArray(data) => data,
        _ => return None,
    };
    if longs.is_empty() {
        return None;
    }
    let bits = 9usize;
    let mask = (1u64 << bits) - 1;
    let mut out = [0u16; 256];
    for (index, slot) in out.iter_mut().enumerate() {
        let bit_index = index * bits;
        let long_index = bit_index / 64;
        let offset = bit_index % 64;
        let mut value = (longs.get(long_index).copied().unwrap_or(0) as u64 >> offset) & mask;
        let bits_in_first = 64 - offset;
        if bits_in_first < bits {
            let next = longs.get(long_index + 1).copied().unwrap_or(0) as u64;
            value |= (next << bits_in_first) & mask;
        }
        *slot = value as u16;
    }
    Some(out)
}

fn find_tag<'a>(entries: &'a [(String, NbtTag)], name: &str) -> Result<&'a NbtTag, WorldError> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, tag)| tag)
        .ok_or_else(|| WorldError::InvalidChunk(format!("missing field {name:?}")))
}

fn find_optional_compound<'a>(
    entries: &'a [(String, NbtTag)],
    name: &str,
) -> Option<&'a [(String, NbtTag)]> {
    match find_tag(entries, name).ok()? {
        NbtTag::Compound(inner) => Some(inner.as_slice()),
        _ => None,
    }
}

fn find_int(entries: &[(String, NbtTag)], name: &str) -> Result<i32, WorldError> {
    match find_tag(entries, name)? {
        NbtTag::Int(v) => Ok(*v),
        other => Err(WorldError::InvalidChunk(format!(
            "field {name:?} expected Int, got {other:?}"
        ))),
    }
}

fn find_byte(entries: &[(String, NbtTag)], name: &str) -> Result<i8, WorldError> {
    match find_tag(entries, name)? {
        NbtTag::Byte(v) => Ok(*v),
        NbtTag::Int(v) => i8::try_from(*v).map_err(|_| {
            WorldError::InvalidChunk(format!("field {name:?} Int {v} out of byte range"))
        }),
        other => Err(WorldError::InvalidChunk(format!(
            "field {name:?} expected Byte, got {other:?}"
        ))),
    }
}

fn find_string(entries: &[(String, NbtTag)], name: &str) -> Result<String, WorldError> {
    match find_tag(entries, name)? {
        NbtTag::String(v) => Ok(v.clone()),
        other => Err(WorldError::InvalidChunk(format!(
            "field {name:?} expected String, got {other:?}"
        ))),
    }
}

fn find_list<'a>(entries: &'a [(String, NbtTag)], name: &str) -> Result<&'a [NbtTag], WorldError> {
    match find_tag(entries, name)? {
        NbtTag::List(v) => Ok(v.as_slice()),
        other => Err(WorldError::InvalidChunk(format!(
            "field {name:?} expected List, got {other:?}"
        ))),
    }
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
            "hyperion-chunk-{label}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("region")).expect("mkdir");
        dir
    }

    #[test]
    fn snap_ground_aligns_to_section_top() {
        assert_eq!(snap_ground_y(63), 63);
        assert_eq!(snap_ground_y(64), 63);
        assert_eq!(snap_ground_y(99), 95);
        assert_eq!(snap_ground_y(95), 95);
        assert_eq!(snap_ground_y(0), -1);
        assert_eq!(snap_ground_y(-1), -1);
    }

    #[test]
    fn section_index_matches_vanilla_yzx() {
        assert_eq!(section_index(0, 0, 0), 0);
        assert_eq!(section_index(1, 0, 0), 1);
        assert_eq!(section_index(0, 0, 1), 16);
        assert_eq!(section_index(0, 1, 0), 256);
        assert_eq!(section_index(15, 15, 15), 4095);
    }

    #[test]
    fn flat_column_has_stone_and_air() {
        let col = ChunkColumn::flat(0, 0, 63);
        assert_eq!(col.sections.len(), SECTION_COUNT);
        assert_eq!(col.sections[0].single_block(), Some(&BlockState::stone()));
        let stone_section = col.sections.iter().find(|s| s.y == 0).expect("y0");
        assert_eq!(stone_section.single_block(), Some(&BlockState::stone()));
        let air_section = col.sections.iter().find(|s| s.y == 4).expect("y4");
        assert!(air_section.is_all_air());
        assert_eq!(col.surface_y, 64);
    }

    #[test]
    fn set_block_promotes_to_multi_palette() {
        let mut col = ChunkColumn::flat(0, 0, 63);
        assert!(col.set_block(0, 63, 0, BlockState::dirt()));
        assert_eq!(col.get_block(0, 63, 0), BlockState::dirt());
        assert_eq!(col.get_block(1, 63, 0), BlockState::stone());
        let section = col.sections.iter().find(|s| s.y == 3).expect("section 3");
        assert!(!section.is_single_valued());
        assert_eq!(section.non_air_count(), 4096);
    }

    #[test]
    fn multi_palette_storage_nbt_round_trip() {
        let mut original = ChunkColumn::flat(2, -3, 63);
        original.set_block(2, 63, -3, BlockState::dirt());
        original.set_block(3, 63, -3, BlockState::grass_block());
        original.set_block(2, 64, -3, BlockState::stone()); // still air section above? 64 is air section
        // y=64 is section 4 (air) — placing stone there.
        assert_eq!(original.get_block(2, 63, -3), BlockState::dirt());
        assert_eq!(original.get_block(3, 63, -3), BlockState::grass_block());

        let bytes = original.encode_storage_nbt().expect("encode");
        let decoded = ChunkColumn::decode_storage_nbt(&bytes).expect("decode");
        assert_eq!(decoded.get_block(2, 63, -3), BlockState::dirt());
        assert_eq!(decoded.get_block(3, 63, -3), BlockState::grass_block());
        assert_eq!(decoded.get_block(4, 63, -3), BlockState::stone());
        assert_eq!(decoded.get_block(2, 64, -3), BlockState::stone());
    }

    #[test]
    fn storage_nbt_round_trip_flat() {
        let original = ChunkColumn::flat(2, -3, 63);
        let bytes = original.encode_storage_nbt().expect("encode");
        let decoded = ChunkColumn::decode_storage_nbt(&bytes).expect("decode");
        assert_eq!(decoded.x, 2);
        assert_eq!(decoded.z, -3);
        assert_eq!(decoded.sections.len(), original.sections.len());
        for (a, b) in original.sections.iter().zip(decoded.sections.iter()) {
            assert_eq!(a.y, b.y);
            assert_eq!(a.single_block(), b.single_block());
            assert_eq!(a.biome, b.biome);
        }
        assert_eq!(decoded.surface_y, original.surface_y);
    }

    #[test]
    fn multi_palette_network_payload_encodes() {
        let mut col = ChunkColumn::flat(0, 0, 63);
        col.set_block(0, 63, 0, BlockState::dirt());
        let payload = col.encode_network_payload(true).expect("network");
        // Larger than pure single-value flat (multi section has 2KiB data).
        assert!(payload.len() > 50_000);
        assert_eq!(&payload[..8], &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multi_palette_anvil_round_trip_many_blocks() {
        let world = temp_world("multi-anvil");
        let mut col = ChunkColumn::flat(1, 2, 63);
        // Sprinkle several default states on the surface section.
        let samples = [
            ((0, 63, 0), BlockState::dirt()),
            ((1, 63, 0), BlockState::grass_block()),
            ((2, 63, 0), BlockState::bedrock()),
            ((3, 63, 0), BlockState::new("minecraft:sand")),
            ((4, 63, 0), BlockState::new("minecraft:gravel")),
            ((5, 63, 0), BlockState::new("minecraft:oak_log")),
            ((0, 64, 0), BlockState::stone()), // air section → multi with air+stone
        ];
        for &((x, y, z), ref state) in &samples {
            assert!(col.set_block(x, y, z, state.clone()));
        }
        save_chunk(&world, &col).expect("save");
        let loaded = load_chunk(&world, 1, 2).expect("load").expect("present");
        for &((x, y, z), ref state) in &samples {
            assert_eq!(
                loaded.get_block(x, y, z),
                *state,
                "mismatch at ({x},{y},{z})"
            );
        }
        // Untouched neighbour stays stone.
        assert_eq!(loaded.get_block(6, 63, 0), BlockState::stone());
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn fluid_count_tracks_water_cells() {
        let mut section = ChunkSection::air(0, PLAINS_BIOME);
        assert_eq!(section.fluid_count(), 0);
        section.set_block(0, 0, 0, BlockState::new("minecraft:water"));
        section.set_block(1, 0, 0, BlockState::new("minecraft:water"));
        section.set_block(2, 0, 0, BlockState::stone());
        assert_eq!(section.fluid_count(), 2);
        assert_eq!(section.non_air_count(), 3);
        let net = section.to_network();
        assert_eq!(net.fluid_count, 2);
        assert_eq!(net.non_air_count, 3);
    }

    #[test]
    fn multi_section_network_larger_than_single() {
        let single = ChunkColumn::flat(0, 0, 63)
            .encode_network_payload(false)
            .expect("single");
        let mut multi = ChunkColumn::flat(0, 0, 63);
        multi.set_block(0, 63, 0, BlockState::dirt());
        let multi_bytes = multi.encode_network_payload(false).expect("multi");
        // Multi-valued block palette adds ~2 KiB of bit storage for one section.
        assert!(
            multi_bytes.len() > single.len() + 1500,
            "multi {} vs single {}",
            multi_bytes.len(),
            single.len()
        );
    }

    #[test]
    fn block_state_ids_from_report() {
        assert_eq!(BlockState::air().network_id(), 0);
        assert_eq!(BlockState::stone().network_id(), 1);
        assert_eq!(BlockState::dirt().network_id(), BLOCK_STATE_DIRT);
        assert_eq!(
            BlockState::grass_block().network_id(),
            BLOCK_STATE_GRASS_BLOCK
        );
        assert_eq!(BlockState::bedrock().network_id(), BLOCK_STATE_BEDROCK);
        assert_eq!(BlockState::new("minecraft:oak_log").network_id(), 137);
    }

    #[test]
    fn anvil_save_load_round_trip() {
        let world = temp_world("anvil");
        let col = ChunkColumn::flat(0, 0, 63);
        save_chunk(&world, &col).expect("save");
        let loaded = load_chunk(&world, 0, 0).expect("load").expect("present");
        assert_eq!(
            loaded.sections[0].single_block(),
            Some(&BlockState::stone())
        );
        assert!(!loaded.sections[1].is_all_air());
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn ensure_spawn_chunk_is_idempotent() {
        let world = temp_world("spawn");
        let (first, created) = ensure_spawn_chunk(&world, 63).expect("create");
        assert!(created);
        assert_eq!(first.surface_y, 64);
        let (second, created_again) = ensure_spawn_chunk(&world, 63).expect("again");
        assert!(!created_again);
        assert_eq!(second.sections.len(), first.sections.len());
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn load_or_create_flat_persists_new_columns() {
        let world = temp_world("persist");
        let (col, created) = load_or_create_flat(&world, 3, -2, 63).expect("create");
        assert!(created);
        assert_eq!(col.x, 3);
        assert_eq!(col.z, -2);
        let again = load_chunk(&world, 3, -2).expect("load").expect("present");
        assert_eq!(again.sections.len(), SECTION_COUNT);
        let (_, created_again) = load_or_create_flat(&world, 3, -2, 63).expect("second");
        assert!(!created_again);
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn position_to_chunk_matches_vanilla_floor_div() {
        assert_eq!(position_to_chunk(0.0, 0.0), (0, 0));
        assert_eq!(position_to_chunk(15.9, 16.0), (0, 1));
        assert_eq!(position_to_chunk(-0.1, -16.0), (-1, -1));
        assert_eq!(block_to_chunk(-1), -1);
        assert_eq!(block_to_chunk(16), 1);
    }

    #[test]
    fn network_payload_encodes() {
        let col = ChunkColumn::flat(0, 0, 63);
        let payload = col.encode_network_payload(true).expect("network");
        assert!(payload.len() > 50_000 && payload.len() < 80_000);
        assert_eq!(&payload[..8], &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn flat_cache_builds_hundreds_of_payloads_quickly() {
        use std::time::Instant;
        let cache = FlatNetworkCache::new(63).expect("cache");
        let start = Instant::now();
        for z in -8..=8 {
            for x in -8..=8 {
                let p = cache.payload(x, z);
                assert_eq!(&p[..4], &x.to_be_bytes());
            }
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 100,
            "289 template clones took {elapsed:?} (expected <100ms)"
        );
    }
}
