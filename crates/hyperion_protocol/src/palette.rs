//! Network paletted containers (blocks + biomes) for protocol 776 / 26.2.
//!
//! Encoding rules verified against the 26.2 jar (`PalettedContainer`,
//! `Strategy`, `SimpleBitStorage`, `LinearPalette`, `HashMapPalette`) and
//! cross-checked with real chunk captures (fluidCount + no long-array length).
//!
//! # Block strategy (`entry_count = 4096`)
//! - 0 unique → should not happen; treat as single air
//! - 1 unique → bits = 0 (single value, no data array)
//! - 2..=16 → bits = **4** (linear palette forced to 4 bits)
//! - 17..=32 → 5, …, 129..=256 → 8 (hash-map palette)
//! - more → global palette (`GLOBAL_PALETTE_BITS_BLOCKS`, no palette list)
//!
//! # Biome strategy (`entry_count = 64`)
//! - 1 → bits 0; 2 → 1; 3–4 → 2; 5–8 → 3; else global.

use crate::frame::encode_var_i32;

/// Entries in a block-state section (16³).
pub const BLOCK_SECTION_SIZE: usize = 4096;
/// Entries in a biome section (4³).
pub const BIOME_SECTION_SIZE: usize = 64;

/// Max bits for an *indirect* block palette before switching to global.
pub const MAX_INDIRECT_BLOCK_BITS: u8 = 8;
/// Max bits for an *indirect* biome palette before switching to global.
pub const MAX_INDIRECT_BIOME_BITS: u8 = 3;

/// Bits for the global block-state palette (26.2 has 32366 states → 15 bits).
pub const GLOBAL_PALETTE_BITS_BLOCKS: u8 = 15;
/// Bits for the global biome palette (plenty for vanilla biome count).
pub const GLOBAL_PALETTE_BITS_BIOMES: u8 = 6;

/// Which registry strategy to apply when choosing BPE / palette type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    /// 16×16×16 block states.
    Blocks,
    /// 4×4×4 biomes.
    Biomes,
}

impl PaletteKind {
    pub const fn entry_count(self) -> usize {
        match self {
            Self::Blocks => BLOCK_SECTION_SIZE,
            Self::Biomes => BIOME_SECTION_SIZE,
        }
    }

    pub const fn max_indirect_bits(self) -> u8 {
        match self {
            Self::Blocks => MAX_INDIRECT_BLOCK_BITS,
            Self::Biomes => MAX_INDIRECT_BIOME_BITS,
        }
    }

    pub const fn global_bits(self) -> u8 {
        match self {
            Self::Blocks => GLOBAL_PALETTE_BITS_BLOCKS,
            Self::Biomes => GLOBAL_PALETTE_BITS_BIOMES,
        }
    }

    /// Storage bits written on the wire for an indirect palette of `palette_len`.
    ///
    /// Returns `None` when the palette must use the global (direct) path.
    pub fn indirect_bits(self, palette_len: usize) -> Option<u8> {
        if palette_len <= 1 {
            return Some(0);
        }
        let needed = bits_needed(palette_len);
        match self {
            Self::Blocks => {
                // Strategy.createForBlockStates: 1..=4 → store as 4 (linear).
                let bits = needed.max(4);
                if bits <= MAX_INDIRECT_BLOCK_BITS {
                    Some(bits)
                } else {
                    None
                }
            }
            Self::Biomes => {
                if needed <= MAX_INDIRECT_BIOME_BITS {
                    Some(needed)
                } else {
                    None
                }
            }
        }
    }
}

/// A paletted container ready for the network section buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPalettedContainer {
    /// `bits = 0` + single global id (ZeroBitStorage — no data longs).
    Single(i32),
    /// Indirect linear/hash palette: BPE, global-id palette, local indices.
    Indirect {
        bits: u8,
        palette: Vec<i32>,
        /// Length = entry count; values are palette indices.
        indices: Vec<u16>,
    },
    /// Direct global ids packed with `bits` (no palette list on the wire).
    Global {
        bits: u8,
        /// Length = entry count; values are global registry ids.
        values: Vec<u32>,
    },
}

impl NetworkPalettedContainer {
    /// Builds a container from a palette of global ids and per-cell palette indices.
    ///
    /// `indices.len()` must equal `kind.entry_count()`. Empty palette becomes single air (0).
    pub fn from_palette_indices(
        kind: PaletteKind,
        palette: &[i32],
        indices: &[u16],
    ) -> Result<Self, PaletteError> {
        let expected = kind.entry_count();
        if indices.len() != expected {
            return Err(PaletteError::BadLength {
                expected,
                actual: indices.len(),
            });
        }
        if palette.is_empty() {
            return Ok(Self::Single(0));
        }
        if palette.len() == 1 {
            return Ok(Self::Single(palette[0]));
        }
        // Validate indices fit the palette.
        let max_idx = (palette.len() - 1) as u16;
        for &idx in indices {
            if idx > max_idx {
                return Err(PaletteError::IndexOutOfRange {
                    index: idx,
                    palette_len: palette.len(),
                });
            }
        }

        match kind.indirect_bits(palette.len()) {
            Some(bits) if bits > 0 => Ok(Self::Indirect {
                bits,
                palette: palette.to_vec(),
                indices: indices.to_vec(),
            }),
            Some(_) => Ok(Self::Single(palette[0])),
            None => {
                // Promote local indices to global ids.
                let values = indices
                    .iter()
                    .map(|&i| palette[i as usize] as u32)
                    .collect();
                Ok(Self::Global {
                    bits: kind.global_bits(),
                    values,
                })
            }
        }
    }

    /// Convenience: every cell is the same global id.
    pub fn single(global_id: i32) -> Self {
        Self::Single(global_id)
    }
}

/// Errors while building a network palette.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaletteError {
    #[error("paletted container length {actual} != expected {expected}")]
    BadLength { expected: usize, actual: usize },
    #[error("palette index {index} out of range for palette len {palette_len}")]
    IndexOutOfRange { index: u16, palette_len: usize },
}

/// `ceil(log2(n))` for n ≥ 1; 0 for n = 0.
pub fn bits_needed(n: usize) -> u8 {
    if n <= 1 {
        0
    } else {
        (usize::BITS - (n - 1).leading_zeros()) as u8
    }
}

/// Packs `values` with SimpleBitStorage (no cross-long packing).
///
/// `values_per_long = 64 / bits`; unused high bits of each long are zero.
pub fn pack_simple_bit_storage(bits: u8, values: impl IntoIterator<Item = u32>) -> Vec<i64> {
    debug_assert!((1..=32).contains(&bits));
    let bits = bits as u32;
    let values_per_long = 64 / bits;
    let mask = if bits == 32 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };

    let values: Vec<u32> = values.into_iter().collect();
    if values.is_empty() || values_per_long == 0 {
        return Vec::new();
    }
    let long_count = values.len().div_ceil(values_per_long as usize);
    let mut data = vec![0i64; long_count];
    for (i, &value) in values.iter().enumerate() {
        let cell = i / values_per_long as usize;
        let offset = ((i as u32) % values_per_long) * bits;
        let packed = (u64::from(value) & mask) << offset;
        data[cell] |= packed as i64;
    }
    data
}

/// Writes one paletted container: `bits: u8` + palette + fixed-size long array
/// (no long-array length prefix — 1.21.5+ / 26.2).
pub fn write_paletted_container(out: &mut Vec<u8>, container: &NetworkPalettedContainer) {
    match container {
        NetworkPalettedContainer::Single(id) => {
            out.push(0);
            out.extend_from_slice(&encode_var_i32(*id));
            // ZeroBitStorage: no longs.
        }
        NetworkPalettedContainer::Indirect {
            bits,
            palette,
            indices,
        } => {
            out.push(*bits);
            out.extend_from_slice(&encode_var_i32(palette.len() as i32));
            for &id in palette {
                out.extend_from_slice(&encode_var_i32(id));
            }
            let longs = pack_simple_bit_storage(*bits, indices.iter().map(|&i| u32::from(i)));
            for &word in &longs {
                out.extend_from_slice(&word.to_be_bytes());
            }
        }
        NetworkPalettedContainer::Global { bits, values } => {
            out.push(*bits);
            // GlobalPalette writes nothing for the palette list.
            let longs = pack_simple_bit_storage(*bits, values.iter().copied());
            for &word in &longs {
                out.extend_from_slice(&word.to_be_bytes());
            }
        }
    }
}

/// Number of longs SimpleBitStorage allocates for `entry_count` values at `bits`.
pub fn simple_bit_storage_long_count(bits: u8, entry_count: usize) -> usize {
    if bits == 0 || entry_count == 0 {
        return 0;
    }
    let values_per_long = 64 / bits as u32;
    entry_count.div_ceil(values_per_long as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_needed_basic() {
        assert_eq!(bits_needed(0), 0);
        assert_eq!(bits_needed(1), 0);
        assert_eq!(bits_needed(2), 1);
        assert_eq!(bits_needed(3), 2);
        assert_eq!(bits_needed(16), 4);
        assert_eq!(bits_needed(17), 5);
        assert_eq!(bits_needed(256), 8);
        assert_eq!(bits_needed(257), 9);
    }

    #[test]
    fn block_indirect_bits_force_four() {
        assert_eq!(PaletteKind::Blocks.indirect_bits(1), Some(0));
        assert_eq!(PaletteKind::Blocks.indirect_bits(2), Some(4));
        assert_eq!(PaletteKind::Blocks.indirect_bits(16), Some(4));
        assert_eq!(PaletteKind::Blocks.indirect_bits(17), Some(5));
        assert_eq!(PaletteKind::Blocks.indirect_bits(256), Some(8));
        assert_eq!(PaletteKind::Blocks.indirect_bits(257), None);
    }

    #[test]
    fn biome_indirect_bits() {
        assert_eq!(PaletteKind::Biomes.indirect_bits(2), Some(1));
        assert_eq!(PaletteKind::Biomes.indirect_bits(4), Some(2));
        assert_eq!(PaletteKind::Biomes.indirect_bits(8), Some(3));
        assert_eq!(PaletteKind::Biomes.indirect_bits(9), None);
    }

    #[test]
    fn pack_two_values_four_bits() {
        // values_per_long = 16; both fit in long 0 at offsets 0 and 4.
        let longs = pack_simple_bit_storage(4, [1u32, 2]);
        assert_eq!(longs.len(), 1);
        assert_eq!(longs[0] as u64 & 0xF, 1);
        assert_eq!((longs[0] as u64 >> 4) & 0xF, 2);
    }

    #[test]
    fn pack_4096_at_four_bits_is_256_longs() {
        let values = vec![0u32; 4096];
        let longs = pack_simple_bit_storage(4, values);
        assert_eq!(longs.len(), 256);
        assert_eq!(simple_bit_storage_long_count(4, 4096), 256);
    }

    #[test]
    fn single_container_wire_bytes() {
        let mut out = Vec::new();
        write_paletted_container(&mut out, &NetworkPalettedContainer::Single(1));
        assert_eq!(out, vec![0, 1]); // bits=0, varint 1
    }

    #[test]
    fn two_block_palette_uses_four_bits_and_data() {
        let palette = [0i32, 1];
        let mut indices = vec![0u16; 4096];
        indices[0] = 1;
        indices[1] = 1;
        let c =
            NetworkPalettedContainer::from_palette_indices(PaletteKind::Blocks, &palette, &indices)
                .expect("build");
        match &c {
            NetworkPalettedContainer::Indirect { bits, palette, .. } => {
                assert_eq!(*bits, 4);
                assert_eq!(palette, &[0, 1]);
            }
            other => panic!("expected Indirect, got {other:?}"),
        }
        let mut out = Vec::new();
        write_paletted_container(&mut out, &c);
        // bits(1) + palette_len varint(1) + two varint ids(1+1) + 256*8 data
        assert_eq!(out[0], 4);
        assert_eq!(out[1], 2); // palette len
        assert_eq!(out[2], 0); // air
        assert_eq!(out[3], 1); // stone
        assert_eq!(out.len(), 1 + 1 + 1 + 1 + 256 * 8);
    }
}
