//! Anvil chunk column model, storage NBT, and network encoding helpers.
//!
//! Phase 2.1 scope:
//! - Single-valued sections (whole section = one block + one biome)
//! - Storage NBT round-trip compatible with modern Anvil (1.18+ layout)
//! - Flat spawn column written on first boot
//! - Convert to [`NetworkChunkSection`] for Play

use std::path::Path;

use hyperion_protocol::{
    HEIGHTMAP_MOTION_BLOCKING, HEIGHTMAP_WORLD_SURFACE, NbtTag, NetworkChunkSection,
    NetworkHeightmap, ProtocolError, decode_named_tag, encode_chunk_payload, encode_named_tag,
    pack_heightmap_values,
};

use crate::anvil::{RegionFile, region_path};
use crate::error::WorldError;
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
// Provisional global block-state ids (protocol 776 / 26.2)
// ---------------------------------------------------------------------------
// Ordered like recent 1.21.x registries (air=0, stone=1, …). Replace with
// codegen from Mojang reports when the Phase 2 data pipeline lands.

/// Global palette id for `minecraft:air`.
pub const BLOCK_STATE_AIR: i32 = 0;
/// Global palette id for `minecraft:stone` (default state).
pub const BLOCK_STATE_STONE: i32 = 1;
/// Global palette id for `minecraft:bedrock` (26.1 registry = 85).
/// Prefer stone for network solid fill until the full 26.2 block map is codegen'd.
pub const BLOCK_STATE_BEDROCK: i32 = 85;

/// A block state identified by resource location (Anvil palette entry).
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

    /// Whether this is air (including cave/void air names).
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "air"
        )
    }

    /// Provisional global palette id for the network encoder.
    ///
    /// Until block-state codegen lands we only trust air (0) and stone (1).
    /// Other names (including bedrock) map to stone so a wrong provisional id
    /// cannot turn the platform into glass/water/etc.
    pub fn network_id(&self) -> i32 {
        match self.name.as_str() {
            "minecraft:air" | "air" | "minecraft:cave_air" | "minecraft:void_air" => {
                BLOCK_STATE_AIR
            }
            // stone, bedrock, dirt, … → solid stone id
            _ => BLOCK_STATE_STONE,
        }
    }
}

/// One 16×16×16 section with a single block filling the volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSection {
    /// Section Y index (e.g. -4 for the bottom overworld section).
    pub y: i8,
    /// Block filling the entire section.
    pub block: BlockState,
    /// Biome resource location filling the entire section.
    pub biome: String,
}

impl ChunkSection {
    /// All-air section.
    pub fn air(y: i8, biome: impl Into<String>) -> Self {
        Self {
            y,
            block: BlockState::air(),
            biome: normalize_biome(biome),
        }
    }

    /// Solid single-block section.
    pub fn solid(y: i8, block: BlockState, biome: impl Into<String>) -> Self {
        Self {
            y,
            block,
            biome: normalize_biome(biome),
        }
    }

    /// Network encoding of this section.
    ///
    /// Biome network ids are not resolved yet (always plains = 0) until the
    /// registry codegen lands; storage still keeps the resource location.
    pub fn to_network(&self) -> NetworkChunkSection {
        let biome_id = PLAINS_BIOME_NETWORK_ID;
        if self.block.is_air() {
            NetworkChunkSection::air(biome_id)
        } else {
            NetworkChunkSection::solid(self.block.network_id(), biome_id)
        }
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
    /// Absolute world Y of the highest solid block + 1 (feet spawn height
    /// for a flat column), packed into heightmaps.
    pub surface_y: i32,
    /// `DataVersion` written into storage NBT.
    pub data_version: i32,
}

impl ChunkColumn {
    /// Builds an empty (all-air) overworld column.
    pub fn empty(x: i32, z: i32) -> Self {
        let sections = (0..SECTION_COUNT)
            .map(|i| ChunkSection::air(MIN_SECTION_Y + i as i8, PLAINS_BIOME))
            .collect();
        Self {
            x,
            z,
            min_section_y: MIN_SECTION_Y,
            sections,
            surface_y: MIN_SECTION_Y as i32 * 16, // no surface
            data_version: DEFAULT_DATA_VERSION,
        }
    }

    /// Flat stone platform: stone from the bottom of the world up through
    /// `ground_y` (inclusive, snapped down to a section top), air above.
    ///
    /// Uses only `minecraft:stone` (global id 1) for solid fill so the network
    /// palette stays on the most stable block-state id until full registry
    /// codegen lands. `ground_y` is the highest solid block Y; player feet
    /// should be `ground_y + 1`.
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
        Self {
            x,
            z,
            min_section_y: MIN_SECTION_Y,
            sections,
            surface_y: ground_y + 1,
            data_version: DEFAULT_DATA_VERSION,
        }
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
        let height_value = self.surface_y.clamp(0, 511) as u16;
        let heights = [height_value; 256];
        let packed = pack_heightmap_values(&heights);
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
            let block_palette = NbtTag::List(vec![NbtTag::Compound(vec![(
                "Name".to_owned(),
                NbtTag::String(section.block.name.clone()),
            )])]);
            let block_states = NbtTag::Compound(vec![("palette".to_owned(), block_palette)]);
            // Biome palette entries are bare strings in modern Anvil.
            let biome_palette = NbtTag::List(vec![NbtTag::String(section.biome.clone())]);
            let biomes = NbtTag::Compound(vec![("palette".to_owned(), biome_palette)]);
            section_tags.push(NbtTag::Compound(vec![
                ("Y".to_owned(), NbtTag::Byte(section.y)),
                ("block_states".to_owned(), block_states),
                ("biomes".to_owned(), biomes),
            ]));
        }

        // Heightmaps as long arrays (same packing as the network format).
        let height_value = self.surface_y.clamp(0, 511) as u16;
        let heights = [height_value; 256];
        let packed = pack_heightmap_values(&heights);
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
            let block = parse_section_block(sec)?;
            let biome = parse_section_biome(sec)?;
            sections.push(ChunkSection { y, block, biome });
        }
        // Ensure bottom-to-top order.
        sections.sort_by_key(|s| s.y);

        // If sections were omitted, pad to a full column of air.
        if sections.is_empty() {
            for i in 0..SECTION_COUNT {
                sections.push(ChunkSection::air(min_section_y + i as i8, PLAINS_BIOME));
            }
        }

        let surface_y = parse_surface_y(root).unwrap_or_else(|| {
            // Infer from highest non-air section.
            sections
                .iter()
                .rev()
                .find(|s| !s.block.is_air())
                .map(|s| (i32::from(s.y) + 1) * 16)
                .unwrap_or(i32::from(min_section_y) * 16)
        });

        Ok(Self {
            x,
            z,
            min_section_y,
            sections,
            surface_y,
            data_version,
        })
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
    let world_dir = world_dir.as_ref();
    if let Some(existing) = load_chunk(world_dir, 0, 0)? {
        return Ok((existing, false));
    }
    let column = ChunkColumn::flat(0, 0, ground_y);
    save_chunk(world_dir, &column)?;
    Ok((column, true))
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn parse_section_block(sec: &[(String, NbtTag)]) -> Result<BlockState, WorldError> {
    let Some(states) = find_optional_compound(sec, "block_states") else {
        return Ok(BlockState::air());
    };
    let palette = find_list(states, "palette")?;
    let Some(first) = palette.first() else {
        return Ok(BlockState::air());
    };
    let NbtTag::Compound(entries) = first else {
        return Err(WorldError::InvalidChunk(
            "block palette entry is not a compound".to_owned(),
        ));
    };
    let name = find_string(entries, "Name")?;
    Ok(BlockState::new(name))
}

fn parse_section_biome(sec: &[(String, NbtTag)]) -> Result<String, WorldError> {
    let Some(biomes) = find_optional_compound(sec, "biomes") else {
        return Ok(PLAINS_BIOME.to_owned());
    };
    let palette = find_list(biomes, "palette")?;
    match palette.first() {
        Some(NbtTag::String(name)) => Ok(normalize_biome(name.clone())),
        Some(NbtTag::Compound(entries)) => {
            // Some tools nest Name inside a compound.
            find_string(entries, "Name").map(normalize_biome)
        }
        _ => Ok(PLAINS_BIOME.to_owned()),
    }
}

fn parse_surface_y(root: &[(String, NbtTag)]) -> Option<i32> {
    let heightmaps = find_optional_compound(root, "Heightmaps")?;
    let longs = match find_tag(heightmaps, "WORLD_SURFACE").ok()? {
        NbtTag::LongArray(data) => data,
        _ => return None,
    };
    if longs.is_empty() {
        return None;
    }
    // Unpack first heightmap entry (9 bits).
    let bits = 9u32;
    let mask = (1u64 << bits) - 1;
    let value = (longs[0] as u64) & mask;
    Some(value as i32)
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
        assert_eq!(snap_ground_y(0), -1); // section Y=-1 top? 0 is section 0 bottom
        // Y=0 is bottom of section 0; snap down to section -1 top = -1
        assert_eq!(snap_ground_y(-1), -1);
    }

    #[test]
    fn flat_column_has_stone_and_air() {
        let col = ChunkColumn::flat(0, 0, 63);
        assert_eq!(col.sections.len(), SECTION_COUNT);
        assert_eq!(col.sections[0].block, BlockState::stone());
        // Section Y=0 is index 4 (MIN=-4)
        let stone_section = col.sections.iter().find(|s| s.y == 0).expect("y0");
        assert_eq!(stone_section.block, BlockState::stone());
        let air_section = col.sections.iter().find(|s| s.y == 4).expect("y4");
        assert!(air_section.block.is_air());
        assert_eq!(col.surface_y, 64);
    }

    #[test]
    fn storage_nbt_round_trip() {
        let original = ChunkColumn::flat(2, -3, 63);
        let bytes = original.encode_storage_nbt().expect("encode");
        let decoded = ChunkColumn::decode_storage_nbt(&bytes).expect("decode");
        assert_eq!(decoded.x, 2);
        assert_eq!(decoded.z, -3);
        assert_eq!(decoded.sections.len(), original.sections.len());
        for (a, b) in original.sections.iter().zip(decoded.sections.iter()) {
            assert_eq!(a.y, b.y);
            assert_eq!(a.block, b.block);
            assert_eq!(a.biome, b.biome);
        }
        assert_eq!(decoded.surface_y, original.surface_y);
    }

    #[test]
    fn anvil_save_load_round_trip() {
        let world = temp_world("anvil");
        let col = ChunkColumn::flat(0, 0, 63);
        save_chunk(&world, &col).expect("save");
        let loaded = load_chunk(&world, 0, 0).expect("load").expect("present");
        assert_eq!(loaded.sections[0].block, BlockState::stone());
        assert!(!loaded.sections[1].block.is_air());
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
    fn network_payload_encodes() {
        let col = ChunkColumn::flat(0, 0, 63);
        let payload = col.encode_network_payload(true).expect("network");
        // Same ballpark as the empty-chunk smoke test (light dominates size).
        assert!(payload.len() > 50_000 && payload.len() < 80_000);
        assert_eq!(&payload[..8], &[0, 0, 0, 0, 0, 0, 0, 0]);
    }
}
