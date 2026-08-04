//! Anvil `.mca` region file read/write with hard size bounds.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::ZlibEncoder;

use crate::error::WorldError;

/// Size of one Anvil sector (header and payload units).
pub const SECTOR_SIZE: usize = 4096;
/// Chunks per region edge (32×32).
pub const CHUNKS_PER_REGION_EDGE: i32 = 32;
/// Total chunk slots in a region file.
pub const CHUNKS_PER_REGION: usize = 1024;
/// Location table + timestamp table.
pub const HEADER_BYTES: usize = SECTOR_SIZE * 2;
/// Max uncompressed chunk NBT accepted when reading (anti zip-bomb).
pub const MAX_CHUNK_UNCOMPRESSED: usize = 2 * 1024 * 1024;
/// Max compressed payload length (vanilla-ish upper bound).
pub const MAX_CHUNK_COMPRESSED: usize = 1024 * 1024;

/// Compression type byte: gzip.
pub const COMPRESSION_GZIP: u8 = 1;
/// Compression type byte: zlib (vanilla default for Anvil).
pub const COMPRESSION_ZLIB: u8 = 2;
/// Compression type byte: uncompressed.
pub const COMPRESSION_NONE: u8 = 3;

/// Maps global chunk coordinates to region coordinates.
#[inline]
pub fn chunk_to_region(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (
        chunk_x.div_euclid(CHUNKS_PER_REGION_EDGE),
        chunk_z.div_euclid(CHUNKS_PER_REGION_EDGE),
    )
}

/// Local index 0..1023 inside a region for a global chunk coordinate.
#[inline]
pub fn chunk_index(chunk_x: i32, chunk_z: i32) -> usize {
    let local_x = chunk_x.rem_euclid(CHUNKS_PER_REGION_EDGE) as usize;
    let local_z = chunk_z.rem_euclid(CHUNKS_PER_REGION_EDGE) as usize;
    local_x + local_z * CHUNKS_PER_REGION_EDGE as usize
}

/// Vanilla file name for a region: `r.{rx}.{rz}.mca`.
pub fn region_file_name(region_x: i32, region_z: i32) -> String {
    format!("r.{region_x}.{region_z}.mca")
}

/// Full path: `world_dir/region/r.x.z.mca`.
pub fn region_path(world_dir: impl AsRef<Path>, region_x: i32, region_z: i32) -> PathBuf {
    world_dir
        .as_ref()
        .join("region")
        .join(region_file_name(region_x, region_z))
}

/// An open (or in-memory-backed) Anvil region file.
///
/// Reads and writes **uncompressed storage-NBT bytes** for a chunk (the
/// gzip/zlib wrapper is handled internally). Does not parse chunk NBT.
#[derive(Debug)]
pub struct RegionFile {
    path: PathBuf,
}

impl RegionFile {
    /// Opens an existing region file. Returns an error if the path is missing
    /// or the header is shorter than 8 KiB.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, WorldError> {
        let path = path.into();
        let meta = fs::metadata(&path).map_err(|source| WorldError::io(&path, source))?;
        if meta.len() < HEADER_BYTES as u64 {
            return Err(WorldError::InvalidRegion(format!(
                "{}: file shorter than Anvil header ({HEADER_BYTES} bytes)",
                path.display()
            )));
        }
        Ok(Self { path })
    }

    /// Creates a new empty region file (header only, no chunks).
    pub fn create_empty(path: impl Into<PathBuf>) -> Result<Self, WorldError> {
        let path = path.into();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| WorldError::io(parent, source))?;
        }
        let mut file = File::create(&path).map_err(|source| WorldError::io(&path, source))?;
        let header = vec![0_u8; HEADER_BYTES];
        file.write_all(&header)
            .map_err(|source| WorldError::io(&path, source))?;
        Ok(Self { path })
    }

    /// Opens the region if it exists, otherwise creates an empty one.
    pub fn open_or_create(path: impl Into<PathBuf>) -> Result<Self, WorldError> {
        let path = path.into();
        if path.exists() {
            Self::open(path)
        } else {
            Self::create_empty(path)
        }
    }

    /// Path of this region file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns whether the slot for `(chunk_x, chunk_z)` is occupied.
    pub fn has_chunk(&self, chunk_x: i32, chunk_z: i32) -> Result<bool, WorldError> {
        let (rx, rz) = chunk_to_region(chunk_x, chunk_z);
        self.ensure_region_coords(rx, rz)?;
        let index = chunk_index(chunk_x, chunk_z);
        let (offset_sectors, sector_count) = self.read_location(index)?;
        Ok(offset_sectors != 0 && sector_count != 0)
    }

    /// Reads and decompresses the chunk at global coordinates.
    ///
    /// Returns `Ok(None)` if the slot is empty. The `Vec<u8>` is **uncompressed
    /// storage NBT** (ready for [`crate::decode` path / `decode_named_tag`]).
    pub fn read_chunk(&self, chunk_x: i32, chunk_z: i32) -> Result<Option<Vec<u8>>, WorldError> {
        let (rx, rz) = chunk_to_region(chunk_x, chunk_z);
        self.ensure_region_coords(rx, rz)?;
        let index = chunk_index(chunk_x, chunk_z);
        let (offset_sectors, sector_count) = self.read_location(index)?;
        if offset_sectors == 0 || sector_count == 0 {
            return Ok(None);
        }

        let mut file =
            File::open(&self.path).map_err(|source| WorldError::io(&self.path, source))?;
        let byte_offset = offset_sectors as u64 * SECTOR_SIZE as u64;
        file.seek(SeekFrom::Start(byte_offset))
            .map_err(|source| WorldError::io(&self.path, source))?;

        let mut len_buf = [0_u8; 4];
        file.read_exact(&mut len_buf)
            .map_err(|source| WorldError::io(&self.path, source))?;
        let length = u32::from_be_bytes(len_buf) as usize;
        if length == 0 {
            return Ok(None);
        }
        if length > MAX_CHUNK_COMPRESSED + 1 {
            return Err(WorldError::InvalidRegion(format!(
                "chunk length {length} exceeds max {}",
                MAX_CHUNK_COMPRESSED + 1
            )));
        }
        // length includes the compression type byte.
        if length < 1 {
            return Err(WorldError::InvalidRegion(
                "chunk length smaller than 1".to_owned(),
            ));
        }
        let max_bytes = sector_count as usize * SECTOR_SIZE;
        if length + 4 > max_bytes {
            return Err(WorldError::InvalidRegion(format!(
                "chunk length {length} does not fit in {sector_count} sectors"
            )));
        }

        let mut compression = [0_u8; 1];
        file.read_exact(&mut compression)
            .map_err(|source| WorldError::io(&self.path, source))?;
        let mut compressed = vec![0_u8; length - 1];
        file.read_exact(&mut compressed)
            .map_err(|source| WorldError::io(&self.path, source))?;

        let uncompressed = decompress_chunk(compression[0], &compressed)?;
        Ok(Some(uncompressed))
    }

    /// Compresses (zlib) and writes the chunk at global coordinates.
    ///
    /// **Vanilla-style append path** (not a full-file rewrite):
    /// - If the existing sector allocation is large enough, overwrite in place.
    /// - Otherwise append at the end of the file and update the location table.
    /// - No `fsync` on the hot path (Paper/vanilla also batch fsyncs).
    ///
    /// The previous implementation rewrote the whole region for every chunk
    /// (`O(n²)` when filling a region) and called `sync_all`, which made flat
    /// streaming slower than vanilla. Do not reintroduce that without a
    /// compaction job.
    ///
    /// `nbt_bytes` must be uncompressed storage NBT.
    pub fn write_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        nbt_bytes: &[u8],
    ) -> Result<(), WorldError> {
        if nbt_bytes.len() > MAX_CHUNK_UNCOMPRESSED {
            return Err(WorldError::InvalidRegion(format!(
                "uncompressed chunk {} exceeds max {MAX_CHUNK_UNCOMPRESSED}",
                nbt_bytes.len()
            )));
        }
        let (rx, rz) = chunk_to_region(chunk_x, chunk_z);
        self.ensure_region_coords(rx, rz)?;
        let target_index = chunk_index(chunk_x, chunk_z);

        let compressed = compress_chunk_zlib(nbt_bytes)?;
        let length = (compressed.len() + 1) as u32; // includes compression byte
        let entry_body_len = 4 + 1 + compressed.len();
        let sectors_needed = entry_body_len.div_ceil(SECTOR_SIZE);
        if sectors_needed > 255 {
            return Err(WorldError::InvalidRegion(format!(
                "chunk needs {sectors_needed} sectors (max 255)"
            )));
        }

        let mut entry = Vec::with_capacity(sectors_needed * SECTOR_SIZE);
        entry.extend_from_slice(&length.to_be_bytes());
        entry.push(COMPRESSION_ZLIB);
        entry.extend_from_slice(&compressed);
        entry.resize(sectors_needed * SECTOR_SIZE, 0);

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|source| WorldError::io(&self.path, source))?;

        let (old_offset, old_sectors) = self.read_location(target_index)?;
        let write_sector = if old_offset != 0 && old_sectors as usize >= sectors_needed {
            old_offset
        } else {
            // Append past the last used sector (file length rounded up).
            let file_len = file
                .metadata()
                .map_err(|source| WorldError::io(&self.path, source))?
                .len();
            let end = file_len.max(HEADER_BYTES as u64);
            end.div_ceil(SECTOR_SIZE as u64) as u32
        };

        file.seek(SeekFrom::Start(
            u64::from(write_sector) * SECTOR_SIZE as u64,
        ))
        .map_err(|source| WorldError::io(&self.path, source))?;
        file.write_all(&entry)
            .map_err(|source| WorldError::io(&self.path, source))?;

        // Update location + timestamp tables in the header (4 bytes each).
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        let mut loc = [0_u8; 4];
        loc[0] = (write_sector >> 16) as u8;
        loc[1] = (write_sector >> 8) as u8;
        loc[2] = write_sector as u8;
        loc[3] = sectors_needed as u8;
        file.seek(SeekFrom::Start((target_index * 4) as u64))
            .map_err(|source| WorldError::io(&self.path, source))?;
        file.write_all(&loc)
            .map_err(|source| WorldError::io(&self.path, source))?;
        file.seek(SeekFrom::Start((SECTOR_SIZE + target_index * 4) as u64))
            .map_err(|source| WorldError::io(&self.path, source))?;
        file.write_all(&now.to_be_bytes())
            .map_err(|source| WorldError::io(&self.path, source))?;
        Ok(())
    }

    /// Removes a chunk slot (idempotent) by clearing its location entry.
    ///
    /// Does **not** reclaim file space (same as vanilla until a compaction);
    /// that keeps delete on the fast path.
    pub fn delete_chunk(&self, chunk_x: i32, chunk_z: i32) -> Result<(), WorldError> {
        let (rx, rz) = chunk_to_region(chunk_x, chunk_z);
        self.ensure_region_coords(rx, rz)?;
        let target_index = chunk_index(chunk_x, chunk_z);
        if !self.has_chunk(chunk_x, chunk_z)? {
            return Ok(());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|source| WorldError::io(&self.path, source))?;
        file.seek(SeekFrom::Start((target_index * 4) as u64))
            .map_err(|source| WorldError::io(&self.path, source))?;
        file.write_all(&[0, 0, 0, 0])
            .map_err(|source| WorldError::io(&self.path, source))?;
        Ok(())
    }

    fn ensure_region_coords(&self, _rx: i32, _rz: i32) -> Result<(), WorldError> {
        // Filename is not re-parsed; callers must use coords consistent with path.
        // Soft check: file must exist.
        if !self.path.exists() {
            return Err(WorldError::io(
                &self.path,
                std::io::Error::new(std::io::ErrorKind::NotFound, "region file missing"),
            ));
        }
        Ok(())
    }

    fn read_location(&self, index: usize) -> Result<(u32, u8), WorldError> {
        debug_assert!(index < CHUNKS_PER_REGION);
        let mut file =
            File::open(&self.path).map_err(|source| WorldError::io(&self.path, source))?;
        file.seek(SeekFrom::Start((index * 4) as u64))
            .map_err(|source| WorldError::io(&self.path, source))?;
        let mut entry = [0_u8; 4];
        file.read_exact(&mut entry)
            .map_err(|source| WorldError::io(&self.path, source))?;
        let offset = (u32::from(entry[0]) << 16) | (u32::from(entry[1]) << 8) | u32::from(entry[2]);
        let sectors = entry[3];
        Ok((offset, sectors))
    }
}

fn compress_chunk_zlib(uncompressed: &[u8]) -> Result<Vec<u8>, WorldError> {
    // Fast compression on the hot path (Paper-style): disk size slightly
    // larger, encode cost much lower than default zlib level 6.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(uncompressed)
        .map_err(|e| WorldError::Gzip(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| WorldError::Gzip(e.to_string()))
}

fn decompress_chunk(compression: u8, data: &[u8]) -> Result<Vec<u8>, WorldError> {
    match compression {
        COMPRESSION_GZIP => decompress_limited(GzDecoder::new(data)),
        COMPRESSION_ZLIB => decompress_limited(ZlibDecoder::new(data)),
        COMPRESSION_NONE => {
            if data.len() > MAX_CHUNK_UNCOMPRESSED {
                return Err(WorldError::InvalidRegion(format!(
                    "uncompressed chunk {} exceeds max {MAX_CHUNK_UNCOMPRESSED}",
                    data.len()
                )));
            }
            Ok(data.to_vec())
        }
        other => Err(WorldError::InvalidRegion(format!(
            "unknown chunk compression type {other}"
        ))),
    }
}

fn decompress_limited<R: Read>(mut decoder: R) -> Result<Vec<u8>, WorldError> {
    let mut out = Vec::new();
    let mut buf = [0_u8; 8 * 1024];
    loop {
        let n = decoder
            .read(&mut buf)
            .map_err(|e| WorldError::Gzip(e.to_string()))?;
        if n == 0 {
            break;
        }
        if out.len() + n > MAX_CHUNK_UNCOMPRESSED {
            return Err(WorldError::InvalidRegion(format!(
                "uncompressed chunk exceeds max {MAX_CHUNK_UNCOMPRESSED}"
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_protocol::{NbtTag, decode_named_tag, encode_named_tag};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn temp_region(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "hyperion-region-{label}-{}-{nanos}.mca",
            std::process::id()
        ))
    }

    fn sample_nbt(label: &str) -> Vec<u8> {
        encode_named_tag(
            "",
            &NbtTag::Compound(vec![
                (
                    "Level".to_owned(),
                    NbtTag::Compound(vec![
                        ("Status".to_owned(), NbtTag::String("full".to_owned())),
                        ("xPos".to_owned(), NbtTag::Int(0)),
                        ("zPos".to_owned(), NbtTag::Int(0)),
                        ("Label".to_owned(), NbtTag::String(label.to_owned())),
                    ]),
                ),
                ("DataVersion".to_owned(), NbtTag::Int(4189)),
            ]),
        )
        .expect("encode")
    }

    #[test]
    fn chunk_to_region_and_index_match_vanilla() {
        assert_eq!(chunk_to_region(0, 0), (0, 0));
        assert_eq!(chunk_to_region(31, 31), (0, 0));
        assert_eq!(chunk_to_region(32, 0), (1, 0));
        assert_eq!(chunk_to_region(-1, 0), (-1, 0));
        assert_eq!(chunk_index(0, 0), 0);
        assert_eq!(chunk_index(1, 0), 1);
        assert_eq!(chunk_index(0, 1), 32);
        assert_eq!(chunk_index(31, 31), 1023);
        // Negative chunk in region -1: local 31 for x=-1
        assert_eq!(chunk_index(-1, 0), 31);
    }

    #[test]
    fn region_file_name_format() {
        assert_eq!(region_file_name(0, 0), "r.0.0.mca");
        assert_eq!(region_file_name(-1, 2), "r.-1.2.mca");
    }

    #[test]
    fn write_read_round_trip_single_chunk() {
        let path = temp_region("single");
        let _ = fs::remove_file(&path);
        let region = RegionFile::create_empty(&path).expect("create");
        let nbt = sample_nbt("alpha");
        region.write_chunk(0, 0, &nbt).expect("write");
        assert!(region.has_chunk(0, 0).expect("has"));
        assert!(!region.has_chunk(1, 0).expect("empty"));
        let loaded = region.read_chunk(0, 0).expect("read").expect("some");
        assert_eq!(loaded, nbt);
        let (name, tag) = decode_named_tag(&loaded).expect("nbt");
        assert_eq!(name, "");
        assert!(matches!(tag, NbtTag::Compound(_)));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn write_two_chunks_and_delete_one() {
        let path = temp_region("two");
        let _ = fs::remove_file(&path);
        let region = RegionFile::create_empty(&path).expect("create");
        region.write_chunk(0, 0, &sample_nbt("a")).expect("write a");
        region.write_chunk(1, 0, &sample_nbt("b")).expect("write b");
        assert!(region.has_chunk(0, 0).unwrap());
        assert!(region.has_chunk(1, 0).unwrap());
        region.delete_chunk(0, 0).expect("delete");
        assert!(!region.has_chunk(0, 0).unwrap());
        let b = region.read_chunk(1, 0).unwrap().expect("b remains");
        let (_, tag) = decode_named_tag(&b).unwrap();
        // Ensure label still "b"
        let NbtTag::Compound(root) = tag else {
            panic!("not compound");
        };
        let level = root
            .iter()
            .find(|(k, _)| k == "Level")
            .map(|(_, t)| t)
            .expect("Level");
        let NbtTag::Compound(level_entries) = level else {
            panic!("Level not compound");
        };
        let label = level_entries
            .iter()
            .find(|(k, _)| k == "Label")
            .map(|(_, t)| t)
            .expect("Label");
        assert_eq!(label, &NbtTag::String("b".to_owned()));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn empty_slot_returns_none() {
        let path = temp_region("empty");
        let _ = fs::remove_file(&path);
        let region = RegionFile::create_empty(&path).expect("create");
        assert_eq!(region.read_chunk(5, 5).expect("read"), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn rejects_oversized_uncompressed_write() {
        let path = temp_region("oversize");
        let _ = fs::remove_file(&path);
        let region = RegionFile::create_empty(&path).expect("create");
        let huge = vec![0_u8; MAX_CHUNK_UNCOMPRESSED + 1];
        let err = region.write_chunk(0, 0, &huge).expect_err("reject");
        assert!(matches!(err, WorldError::InvalidRegion(_)));
        let _ = fs::remove_file(&path);
    }

    /// Regression: filling many slots must stay linear (append), not O(n²)
    /// full-file rewrites. 200 chunks in well under a second on a normal disk.
    #[test]
    fn writing_many_chunks_stays_fast() {
        let path = temp_region("perf");
        let _ = fs::remove_file(&path);
        let region = RegionFile::create_empty(&path).expect("create");
        let nbt = sample_nbt("perf");
        let start = Instant::now();
        for i in 0..200 {
            let x = i % 32;
            let z = i / 32;
            region.write_chunk(x, z, &nbt).expect("write");
        }
        let elapsed = start.elapsed();
        // CI/debug can be slow; full rewrite of a region 200× was multi-minute.
        // Append path must stay under a few seconds even on cold disks.
        assert!(
            elapsed.as_secs() < 10,
            "200 append writes took {elapsed:?} (expected <10s); region rewrite regress?"
        );
        assert!(region.has_chunk(0, 0).unwrap());
        assert!(region.has_chunk(31, 5).unwrap());
        let _ = fs::remove_file(&path);
    }
}
