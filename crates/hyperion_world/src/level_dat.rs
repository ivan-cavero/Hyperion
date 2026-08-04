//! Minimal vanilla-compatible `level.dat` (gzipped storage NBT).
//!
//! On disk, Java Edition stores `level.dat` as gzip (RFC 1952) over a
//! **storage NBT** named root (empty name `""` → compound with a `"Data"`
//! child). That is distinct from **network NBT** (unnamed root) used on the
//! wire. Field names below match vanilla NBT keys.

use std::fs;
use std::path::Path;

use hyperion_protocol::{NbtTag, decode_named_tag, encode_named_tag};

use crate::error::WorldError;
use crate::gzip_util::{gzip_compress, gzip_decompress};

/// Java Edition `DataVersion` written into newly created worlds.
///
/// `4189` is in the 1.21.x range; Hyperion is not version-locked to it beyond
/// writing a recognisable modern value for tools that inspect `level.dat`.
pub const DEFAULT_DATA_VERSION: i32 = 4189;

/// World metadata stored in (and recovered from) `level.dat`.
///
/// Maps to vanilla NBT under `Data` and, for the seed, also to the
/// `level-name` / `level-seed` keys in `server.properties` at first bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelMeta {
    /// Vanilla NBT `LevelName` (also the default folder name from `level-name`).
    pub level_name: String,
    /// World generation seed: written as both `WorldGenSettings.seed` and
    /// legacy `RandomSeed`; sourced from `level-seed` when creating a new world.
    pub seed: i64,
    /// Vanilla NBT `SpawnX`.
    pub spawn_x: i32,
    /// Vanilla NBT `SpawnY`.
    pub spawn_y: i32,
    /// Vanilla NBT `SpawnZ`.
    pub spawn_z: i32,
    /// Vanilla NBT `DataVersion` (Java Edition data version integer).
    pub data_version: i32,
}

impl LevelMeta {
    /// Builds metadata with [`DEFAULT_DATA_VERSION`] and spawn at `(0, spawn_y, 0)`.
    pub fn new(level_name: impl Into<String>, seed: i64, spawn_y: i32) -> Self {
        Self {
            level_name: level_name.into(),
            seed,
            spawn_x: 0,
            spawn_y,
            spawn_z: 0,
            data_version: DEFAULT_DATA_VERSION,
        }
    }
}

/// Writes a minimal gzipped storage-NBT `level.dat` at `path`.
///
/// Root is a **named** compound with an empty name (`""`) containing a single
/// `"Data"` compound with at least:
/// - `LevelName` (String)
/// - `SpawnX` / `SpawnY` / `SpawnZ` (Int)
/// - `DataVersion` (Int)
/// - `RandomSeed` (Long, legacy)
/// - `WorldGenSettings` compound with `seed` (Long)
pub fn write_level_dat(path: impl AsRef<Path>, meta: &LevelMeta) -> Result<(), WorldError> {
    let path = path.as_ref();
    let nbt = encode_named_tag("", &build_root_tag(meta))
        .map_err(|error| WorldError::Nbt(error.to_string()))?;
    let compressed = gzip_compress(&nbt)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| WorldError::io(path, source))?;
    }
    fs::write(path, compressed).map_err(|source| WorldError::io(path, source))
}

/// Reads `level.dat` and recovers [`LevelMeta`].
///
/// Seed is taken from `Data.WorldGenSettings.seed` when present, otherwise
/// from legacy `Data.RandomSeed`.
pub fn read_level_dat(path: impl AsRef<Path>) -> Result<LevelMeta, WorldError> {
    let path = path.as_ref();
    let compressed = fs::read(path).map_err(|source| WorldError::io(path, source))?;
    let nbt_bytes = gzip_decompress(&compressed)?;
    let (root_name, root_tag) =
        decode_named_tag(&nbt_bytes).map_err(|error| WorldError::Nbt(error.to_string()))?;
    if !root_name.is_empty() {
        return Err(WorldError::InvalidLevelDat(format!(
            "expected empty root name, got {root_name:?}"
        )));
    }
    let NbtTag::Compound(root_entries) = root_tag else {
        return Err(WorldError::InvalidLevelDat(
            "root tag is not a compound".to_owned(),
        ));
    };
    let data = find_compound(&root_entries, "Data")?;
    let level_name = find_string(data, "LevelName")?;
    let spawn_x = find_int(data, "SpawnX")?;
    let spawn_y = find_int(data, "SpawnY")?;
    let spawn_z = find_int(data, "SpawnZ")?;
    let data_version = find_int(data, "DataVersion")?;
    let seed = recover_seed(data)?;
    Ok(LevelMeta {
        level_name,
        seed,
        spawn_x,
        spawn_y,
        spawn_z,
        data_version,
    })
}

fn build_root_tag(meta: &LevelMeta) -> NbtTag {
    let world_gen_settings = NbtTag::Compound(vec![("seed".to_owned(), NbtTag::Long(meta.seed))]);
    let data = NbtTag::Compound(vec![
        (
            "LevelName".to_owned(),
            NbtTag::String(meta.level_name.clone()),
        ),
        ("SpawnX".to_owned(), NbtTag::Int(meta.spawn_x)),
        ("SpawnY".to_owned(), NbtTag::Int(meta.spawn_y)),
        ("SpawnZ".to_owned(), NbtTag::Int(meta.spawn_z)),
        ("DataVersion".to_owned(), NbtTag::Int(meta.data_version)),
        ("RandomSeed".to_owned(), NbtTag::Long(meta.seed)),
        ("WorldGenSettings".to_owned(), world_gen_settings),
    ]);
    NbtTag::Compound(vec![("Data".to_owned(), data)])
}

fn recover_seed(data: &[(String, NbtTag)]) -> Result<i64, WorldError> {
    if let Some(settings) = find_optional_compound(data, "WorldGenSettings")
        && let Ok(seed) = find_long(settings, "seed")
    {
        return Ok(seed);
    }
    find_long(data, "RandomSeed")
}

fn find_tag<'a>(entries: &'a [(String, NbtTag)], name: &str) -> Result<&'a NbtTag, WorldError> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, tag)| tag)
        .ok_or_else(|| WorldError::InvalidLevelDat(format!("missing field {name:?}")))
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

fn find_compound<'a>(
    entries: &'a [(String, NbtTag)],
    name: &str,
) -> Result<&'a [(String, NbtTag)], WorldError> {
    match find_tag(entries, name)? {
        NbtTag::Compound(inner) => Ok(inner.as_slice()),
        _ => Err(WorldError::InvalidLevelDat(format!(
            "field {name:?} is not a compound"
        ))),
    }
}

fn find_string(entries: &[(String, NbtTag)], name: &str) -> Result<String, WorldError> {
    match find_tag(entries, name)? {
        NbtTag::String(value) => Ok(value.clone()),
        _ => Err(WorldError::InvalidLevelDat(format!(
            "field {name:?} is not a string"
        ))),
    }
}

fn find_int(entries: &[(String, NbtTag)], name: &str) -> Result<i32, WorldError> {
    match find_tag(entries, name)? {
        NbtTag::Int(value) => Ok(*value),
        _ => Err(WorldError::InvalidLevelDat(format!(
            "field {name:?} is not an int"
        ))),
    }
}

fn find_long(entries: &[(String, NbtTag)], name: &str) -> Result<i64, WorldError> {
    match find_tag(entries, name)? {
        NbtTag::Long(value) => Ok(*value),
        _ => Err(WorldError::InvalidLevelDat(format!(
            "field {name:?} is not a long"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "hyperion-level-dat-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn write_read_round_trip_fields_match() {
        let dir = temp_path("roundtrip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("level.dat");

        let meta = LevelMeta {
            level_name: "HyperionTest".to_owned(),
            seed: 0x1A2B_3C4D_5E6F_7081,
            spawn_x: 12,
            spawn_y: 64,
            spawn_z: -8,
            data_version: DEFAULT_DATA_VERSION,
        };
        write_level_dat(&path, &meta).expect("write");
        let loaded = read_level_dat(&path).expect("read");
        assert_eq!(loaded, meta);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn level_dat_is_gzip_of_storage_nbt() {
        let dir = temp_path("gzip-shape");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("level.dat");

        let meta = LevelMeta::new("world", 42, 100);
        write_level_dat(&path, &meta).expect("write");
        let on_disk = fs::read(&path).expect("read bytes");
        // gzip magic 1f 8b
        assert_eq!(&on_disk[..2], &[0x1f, 0x8b]);
        let nbt = gzip_decompress(&on_disk).expect("decompress");
        let (name, tag) = decode_named_tag(&nbt).expect("named nbt");
        assert_eq!(name, "");
        assert!(matches!(tag, NbtTag::Compound(_)));

        let _ = fs::remove_dir_all(&dir);
    }
}
