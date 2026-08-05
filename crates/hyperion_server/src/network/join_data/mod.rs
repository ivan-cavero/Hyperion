//! Vanilla 26.2 join data used during Configuration.
//!
//! | Asset | Source | Purpose |
//! |---|---|---|
//! | `generated.rs` | datapack entry lists + tags | Registry Data IDs + Update Tags |
//! | `registry_nbt.bin` | datapack JSON → network NBT | Entry payloads (`data: Some`) |
//!
//! Regenerate:
//! ```text
//! cargo run -p hyperion_tools --bin gen-join-data
//! cargo run -p hyperion_tools --bin gen-registry-nbt
//! # or: cargo run -p hyperion_tools --bin gen-mc-ref
//! ```

mod generated;

use std::collections::HashMap;
use std::sync::LazyLock;

pub use generated::{CORE_KNOWN_PACK, VANILLA_REGISTRIES, VANILLA_TAGS};

/// Packed network-NBT for every synchronized registry entry.
///
/// Format (big-endian):
/// ```text
/// u32 count
/// repeated count times:
///   u16 reg_len  + reg_id UTF-8
///   u16 path_len + entry path UTF-8
///   u32 nbt_len  + nbt bytes (root compound, nameless)
/// ```
static REGISTRY_NBT_BLOB: &[u8] = include_bytes!("registry_nbt.bin");

/// `registry_id → (entry_path → network NBT bytes)`.
static REGISTRY_NBT: LazyLock<HashMap<String, HashMap<String, Vec<u8>>>> =
    LazyLock::new(parse_nbt_blob);

fn parse_nbt_blob() -> HashMap<String, HashMap<String, Vec<u8>>> {
    let mut map: HashMap<String, HashMap<String, Vec<u8>>> = HashMap::new();
    let data = REGISTRY_NBT_BLOB;
    if data.len() < 4 {
        return map;
    }
    let mut i = 0usize;
    let count = u32::from_be_bytes(data[i..i + 4].try_into().expect("count")) as usize;
    i += 4;
    for _ in 0..count {
        let reg_len = u16::from_be_bytes(data[i..i + 2].try_into().expect("reg_len")) as usize;
        i += 2;
        let reg_id = String::from_utf8(data[i..i + reg_len].to_vec()).expect("reg utf8");
        i += reg_len;
        let path_len = u16::from_be_bytes(data[i..i + 2].try_into().expect("path_len")) as usize;
        i += 2;
        let path = String::from_utf8(data[i..i + path_len].to_vec()).expect("path utf8");
        i += path_len;
        let nbt_len = u32::from_be_bytes(data[i..i + 4].try_into().expect("nbt_len")) as usize;
        i += 4;
        let nbt = data[i..i + nbt_len].to_vec();
        i += nbt_len;
        map.entry(reg_id).or_default().insert(path, nbt);
    }
    map
}

/// Looks up the network NBT for a synchronized registry entry path
/// (`plains`, `overworld`, …).
pub fn registry_entry_nbt(registry_id: &str, entry_path: &str) -> Option<&'static [u8]> {
    REGISTRY_NBT
        .get(registry_id)
        .and_then(|entries| entries.get(entry_path))
        .map(|v| v.as_slice())
}

/// Number of packed NBT entries (for diagnostics / tests).
#[cfg(test)]
pub fn registry_nbt_count() -> usize {
    REGISTRY_NBT.values().map(HashMap::len).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_nbt_covers_all_listed_entries() {
        let mut listed = 0usize;
        for (registry_id, paths) in VANILLA_REGISTRIES {
            for path in *paths {
                listed += 1;
                assert!(
                    registry_entry_nbt(registry_id, path).is_some(),
                    "missing NBT for {registry_id} / {path}"
                );
            }
        }
        assert_eq!(listed, registry_nbt_count());
        // Root compound tag type.
        let overworld = registry_entry_nbt("minecraft:dimension_type", "overworld").unwrap();
        assert_eq!(overworld[0], 0x0a);
    }
}
