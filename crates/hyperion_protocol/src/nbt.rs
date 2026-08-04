//! Network-NBT writer and streaming reader for protocol 776 (Minecraft 26.2).
//!
//! Since 1.20.2 the network NBT format is the standard NBT binary format
//! (big-endian numbers, unsigned-short length prefixes on strings) with one
//! difference: the root tag carries **no name** (no `u16` length prefix).
//! This matches the format vanilla exchanges for registry data and, since
//! 1.20.5, for text components.

use crate::ProtocolError;
use crate::frame::ByteWriter;

const TAG_END: u8 = 0x00;
const TAG_BYTE: u8 = 0x01;
const TAG_SHORT: u8 = 0x02;
const TAG_INT: u8 = 0x03;
const TAG_LONG: u8 = 0x04;
const TAG_FLOAT: u8 = 0x05;
const TAG_DOUBLE: u8 = 0x06;
const TAG_BYTE_ARRAY: u8 = 0x07;
const TAG_STRING: u8 = 0x08;
const TAG_LIST: u8 = 0x09;
const TAG_COMPOUND: u8 = 0x0a;
const TAG_INT_ARRAY: u8 = 0x0b;
const TAG_LONG_ARRAY: u8 = 0x0c;

const MAX_NBT_DEPTH: usize = 128;

/// A named NBT tag stored inside a compound.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// A homogeneous list (all elements share one tag type).
    List(Vec<NbtTag>),
    Compound(Vec<(String, NbtTag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

/// Encodes a root-level **String** tag (no root name), as used for plain
/// text components: `0x08` + u16 length + UTF-8.
pub fn encode_string_tag(content: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::new();
    writer.push_u8(TAG_STRING);
    push_u16_string(&mut writer, content)?;
    Ok(writer.into_bytes())
}

/// Encodes a root-level **Compound** tag (no root name): `0x0a` + named
/// children + `0x00`.
pub fn encode_compound_tag(entries: &[(String, NbtTag)]) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::new();
    writer.push_u8(TAG_COMPOUND);
    for (name, tag) in entries {
        write_named_tag(&mut writer, name, tag)?;
    }
    writer.push_u8(TAG_END);
    Ok(writer.into_bytes())
}

fn write_named_tag(writer: &mut ByteWriter, name: &str, tag: &NbtTag) -> Result<(), ProtocolError> {
    match tag {
        NbtTag::Byte(_) => writer.push_u8(TAG_BYTE),
        NbtTag::Short(_) => writer.push_u8(TAG_SHORT),
        NbtTag::Int(_) => writer.push_u8(TAG_INT),
        NbtTag::Long(_) => writer.push_u8(TAG_LONG),
        NbtTag::Float(_) => writer.push_u8(TAG_FLOAT),
        NbtTag::Double(_) => writer.push_u8(TAG_DOUBLE),
        NbtTag::ByteArray(_) => writer.push_u8(TAG_BYTE_ARRAY),
        NbtTag::String(_) => writer.push_u8(TAG_STRING),
        NbtTag::List(_) => writer.push_u8(TAG_LIST),
        NbtTag::Compound(_) => writer.push_u8(TAG_COMPOUND),
        NbtTag::IntArray(_) => writer.push_u8(TAG_INT_ARRAY),
        NbtTag::LongArray(_) => writer.push_u8(TAG_LONG_ARRAY),
    }
    push_u16_string(writer, name)?;
    write_tag_payload(writer, tag)
}

fn write_tag_payload(writer: &mut ByteWriter, tag: &NbtTag) -> Result<(), ProtocolError> {
    match tag {
        NbtTag::Byte(value) => writer.push_byte(*value),
        NbtTag::Short(value) => writer.push_bytes(&value.to_be_bytes()),
        NbtTag::Int(value) => writer.push_i32(*value),
        NbtTag::Long(value) => writer.push_i64(*value),
        NbtTag::Float(value) => writer.push_f32(*value),
        NbtTag::Double(value) => writer.push_f64(*value),
        NbtTag::ByteArray(value) => {
            writer.push_i32(value.len() as i32);
            writer.push_bytes(value);
        }
        NbtTag::String(value) => push_u16_string(writer, value)?,
        NbtTag::List(elements) => write_list_payload(writer, elements)?,
        NbtTag::Compound(tags) => {
            for (name, tag) in tags {
                write_named_tag(writer, name, tag)?;
            }
            writer.push_u8(TAG_END);
        }
        NbtTag::IntArray(value) => {
            writer.push_i32(value.len() as i32);
            for v in value {
                writer.push_i32(*v);
            }
        }
        NbtTag::LongArray(value) => {
            writer.push_i32(value.len() as i32);
            for v in value {
                writer.push_i64(*v);
            }
        }
    }
    Ok(())
}

/// Writes an NBT list payload: element-type byte + i32 big-endian count +
/// the elements themselves (no names). An empty list uses `TAG_END` (0x00)
/// as its element type, matching the vanilla convention.
fn write_list_payload(writer: &mut ByteWriter, elements: &[NbtTag]) -> Result<(), ProtocolError> {
    let element_type = elements.first().map(tag_type_byte).unwrap_or(TAG_END);
    writer.push_u8(element_type);
    writer.push_i32(elements.len() as i32);
    for element in elements {
        write_tag_payload(writer, element)?;
    }
    Ok(())
}

fn tag_type_byte(tag: &NbtTag) -> u8 {
    match tag {
        NbtTag::Byte(_) => TAG_BYTE,
        NbtTag::Short(_) => TAG_SHORT,
        NbtTag::Int(_) => TAG_INT,
        NbtTag::Long(_) => TAG_LONG,
        NbtTag::Float(_) => TAG_FLOAT,
        NbtTag::Double(_) => TAG_DOUBLE,
        NbtTag::ByteArray(_) => TAG_BYTE_ARRAY,
        NbtTag::String(_) => TAG_STRING,
        NbtTag::List(_) => TAG_LIST,
        NbtTag::Compound(_) => TAG_COMPOUND,
        NbtTag::IntArray(_) => TAG_INT_ARRAY,
        NbtTag::LongArray(_) => TAG_LONG_ARRAY,
    }
}

/// Writes an NBT string: unsigned-short big-endian byte length + UTF-8.
fn push_u16_string(writer: &mut ByteWriter, value: &str) -> Result<(), ProtocolError> {
    if value.len() > u16::MAX as usize {
        return Err(ProtocolError::StringTooLong);
    }
    writer.push_u16(value.len() as u16);
    writer.push_bytes(value.as_bytes());
    Ok(())
}

/// A streaming reader for network NBT binary data.
///
/// Reads from a `&[u8]` slice with an internal position cursor. All methods
/// return `ProtocolError` on malformed or truncated input with OOM protection
/// (bounds checks before allocation, depth limit).
pub struct NbtReader<'a> {
    data: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> NbtReader<'a> {
    /// Creates a new reader wrapping the given byte slice.
    pub const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            depth: 0,
        }
    }

    /// Returns the current read position.
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Returns the number of unread bytes.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Reads a single tag-type byte (advances by 1).
    pub fn read_tag_type(&mut self) -> Result<u8, ProtocolError> {
        let byte = self
            .data
            .get(self.pos)
            .copied()
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        self.pos += 1;
        Ok(byte)
    }

    /// Reads a named tag: tag-type byte + u16 name + payload.
    ///
    /// Returns an error if the tag type is `TAG_END`.
    pub fn read_named_tag(&mut self) -> Result<(String, NbtTag), ProtocolError> {
        let tag_type = self.read_tag_type()?;
        if tag_type == TAG_END {
            return Err(ProtocolError::InvalidPacketPayload);
        }
        let name = self.read_string()?;
        let tag = self.read_tag_payload(tag_type)?;
        Ok((name, tag))
    }

    /// Reads a compound payload: zero or more named tags followed by `TAG_END`.
    ///
    /// Enforces the nesting depth limit.
    pub fn read_compound(&mut self) -> Result<Vec<(String, NbtTag)>, ProtocolError> {
        if self.depth >= MAX_NBT_DEPTH {
            return Err(ProtocolError::NbtDepthExceeded);
        }
        self.depth += 1;

        let mut entries = Vec::new();
        loop {
            let tag_type = self.read_tag_type()?;
            if tag_type == TAG_END {
                break;
            }
            let name = self.read_string()?;
            let tag = self.read_tag_payload(tag_type)?;
            entries.push((name, tag));
        }

        self.depth -= 1;
        Ok(entries)
    }

    /// Reads a tag payload for the given tag type (no preceding type byte).
    ///
    /// Dispatches to the appropriate type-specific reader. For compound and
    /// list types this enforces the nesting depth limit.
    pub fn read_tag_payload(&mut self, tag_type: u8) -> Result<NbtTag, ProtocolError> {
        match tag_type {
            TAG_BYTE => {
                let value = self
                    .data
                    .get(self.pos)
                    .copied()
                    .ok_or(ProtocolError::UnexpectedEndOfInput)? as i8;
                self.pos += 1;
                Ok(NbtTag::Byte(value))
            }
            TAG_SHORT => {
                let bytes = self.read_fixed_bytes::<2>()?;
                Ok(NbtTag::Short(i16::from_be_bytes(bytes)))
            }
            TAG_INT => {
                let bytes = self.read_fixed_bytes::<4>()?;
                Ok(NbtTag::Int(i32::from_be_bytes(bytes)))
            }
            TAG_LONG => {
                let bytes = self.read_fixed_bytes::<8>()?;
                Ok(NbtTag::Long(i64::from_be_bytes(bytes)))
            }
            TAG_FLOAT => {
                let bytes = self.read_fixed_bytes::<4>()?;
                Ok(NbtTag::Float(f32::from_be_bytes(bytes)))
            }
            TAG_DOUBLE => {
                let bytes = self.read_fixed_bytes::<8>()?;
                Ok(NbtTag::Double(f64::from_be_bytes(bytes)))
            }
            TAG_BYTE_ARRAY => self.read_byte_array().map(NbtTag::ByteArray),
            TAG_STRING => self.read_string().map(NbtTag::String),
            TAG_LIST => self.read_list().map(NbtTag::List),
            TAG_COMPOUND => self.read_compound().map(NbtTag::Compound),
            TAG_INT_ARRAY => self.read_int_array().map(NbtTag::IntArray),
            TAG_LONG_ARRAY => self.read_long_array().map(NbtTag::LongArray),
            _ => Err(ProtocolError::InvalidPacketPayload),
        }
    }

    /// Reads a u16-length-prefixed UTF-8 string.
    pub fn read_string(&mut self) -> Result<String, ProtocolError> {
        let length = self.read_u16()?;
        if (length as usize) > self.remaining() {
            return Err(ProtocolError::UnexpectedEndOfInput);
        }
        let slice = &self.data[self.pos..self.pos + length as usize];
        self.pos += length as usize;
        let s = std::str::from_utf8(slice).map_err(|_| ProtocolError::InvalidUtf8)?;
        Ok(s.to_owned())
    }

    /// Reads a list payload: element-type byte + i32 count + elements.
    ///
    /// Enforces the nesting depth limit and applies OOM protection on the
    /// element count.
    pub fn read_list(&mut self) -> Result<Vec<NbtTag>, ProtocolError> {
        if self.depth >= MAX_NBT_DEPTH {
            return Err(ProtocolError::NbtDepthExceeded);
        }
        self.depth += 1;

        let element_type = self.read_tag_type()?;
        let count = self.read_i32()?;
        if count < 0 {
            return Err(ProtocolError::InvalidPacketPayload);
        }
        // Basic OOM guard: each element is at least 1 byte.
        if (count as usize) > self.remaining() {
            return Err(ProtocolError::UnexpectedEndOfInput);
        }

        let mut elements = Vec::with_capacity(count as usize);
        for _ in 0..count {
            elements.push(self.read_tag_payload(element_type)?);
        }

        self.depth -= 1;
        Ok(elements)
    }

    /// Reads a `TAG_Byte_Array` payload: i32 length + bytes.
    pub fn read_byte_array(&mut self) -> Result<Vec<u8>, ProtocolError> {
        let length = self.read_i32()?;
        if length < 0 {
            return Err(ProtocolError::InvalidPacketPayload);
        }
        let len = length as usize;
        if len > self.remaining() {
            return Err(ProtocolError::UnexpectedEndOfInput);
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes.to_vec())
    }

    /// Reads a `TAG_Int_Array` payload: i32 length + length × i32 values.
    pub fn read_int_array(&mut self) -> Result<Vec<i32>, ProtocolError> {
        let length = self.read_i32()?;
        if length < 0 {
            return Err(ProtocolError::InvalidPacketPayload);
        }
        let len = length as usize;
        let byte_len = len
            .checked_mul(4)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        if byte_len > self.remaining() {
            return Err(ProtocolError::UnexpectedEndOfInput);
        }
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            let bytes = self.read_fixed_bytes::<4>()?;
            values.push(i32::from_be_bytes(bytes));
        }
        Ok(values)
    }

    /// Reads a `TAG_Long_Array` payload: i32 length + length × i64 values.
    pub fn read_long_array(&mut self) -> Result<Vec<i64>, ProtocolError> {
        let length = self.read_i32()?;
        if length < 0 {
            return Err(ProtocolError::InvalidPacketPayload);
        }
        let len = length as usize;
        let byte_len = len
            .checked_mul(8)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        if byte_len > self.remaining() {
            return Err(ProtocolError::UnexpectedEndOfInput);
        }
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            let bytes = self.read_fixed_bytes::<8>()?;
            values.push(i64::from_be_bytes(bytes));
        }
        Ok(values)
    }

    // -- private helpers --

    /// Reads exactly `n` bytes and returns them as a fixed-size array.
    fn read_fixed_bytes<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        let end = self
            .pos
            .checked_add(N)
            .ok_or(ProtocolError::InvalidPacketPayload)?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or(ProtocolError::UnexpectedEndOfInput)?;
        self.pos = end;
        // SAFETY: slice is guaranteed to have length N because we just
        // checked `end - pos == N` and `data[pos..end]` exists.
        Ok(slice.try_into().unwrap_or_else(|_| unreachable!()))
    }

    /// Reads a big-endian u16.
    fn read_u16(&mut self) -> Result<u16, ProtocolError> {
        let bytes = self.read_fixed_bytes()?;
        Ok(u16::from_be_bytes(bytes))
    }

    /// Reads a big-endian i32.
    fn read_i32(&mut self) -> Result<i32, ProtocolError> {
        let bytes = self.read_fixed_bytes()?;
        Ok(i32::from_be_bytes(bytes))
    }
}

/// Decodes a root-level compound tag (no root name) from raw bytes.
///
/// Expects the input to start with `TAG_COMPOUND` (0x0a), followed by
/// named entries and a terminating `TAG_END`.
pub fn decode_compound_tag(input: &[u8]) -> Result<Vec<(String, NbtTag)>, ProtocolError> {
    let mut reader = NbtReader::new(input);
    let tag_type = reader.read_tag_type()?;
    if tag_type != TAG_COMPOUND {
        return Err(ProtocolError::InvalidPacketPayload);
    }
    reader.read_compound()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Writer round-trips (new types)
    // ------------------------------------------------------------------

    #[test]
    fn root_string_tag_has_no_name() {
        let encoded = encode_string_tag("hola").unwrap();
        assert_eq!(encoded, vec![0x08, 0x00, 0x04, b'h', b'o', b'l', b'a']);
    }

    #[test]
    fn root_compound_has_no_name() {
        let encoded = encode_compound_tag(&[("a".to_owned(), NbtTag::Int(5))]).unwrap();
        assert_eq!(
            encoded,
            vec![0x0a, 0x03, 0x00, 0x01, b'a', 0x00, 0x00, 0x00, 0x05, 0x00]
        );
    }

    #[test]
    fn list_of_strings_encodes_element_type_and_count() {
        let encoded = encode_compound_tag(&[(
            "infiniburn".to_owned(),
            NbtTag::List(vec![
                NbtTag::String("minecraft:netherrack".to_owned()),
                NbtTag::String("minecraft:magma_block".to_owned()),
            ]),
        )])
        .unwrap();
        assert_eq!(
            encoded,
            vec![
                0x0a, // root compound
                0x09, 0x00, 0x0a, // TAG_List, name length 10
                b'i', b'n', b'f', b'i', b'n', b'i', b'b', b'u', b'r', b'n',
                0x08, // element type: String
                0x00, 0x00, 0x00, 0x02, // count 2
                0x00, 0x14, // "minecraft:netherrack" length 20
                b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'n', b'e', b't', b'h',
                b'e', b'r', b'r', b'a', b'c', b'k', 0x00,
                0x15, // "minecraft:magma_block" length 21
                b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'm', b'a', b'g', b'm',
                b'a', b'_', b'b', b'l', b'o', b'c', b'k', 0x00, // end of root
            ]
        );
    }

    #[test]
    fn nested_compound_uses_u16_names() {
        let encoded = encode_compound_tag(&[(
            "dimension_type".to_owned(),
            NbtTag::Compound(vec![("fixed_time".to_owned(), NbtTag::Long(6_000))]),
        )])
        .unwrap();
        assert_eq!(
            encoded,
            vec![
                0x0a, // root compound
                0x0a, 0x00, 0x0e, // child compound, name length 14
                b'd', b'i', b'm', b'e', b'n', b's', b'i', b'o', b'n', b'_', b't', b'y', b'p', b'e',
                0x04, 0x00, 0x0a, // TAG_Long, name length 10 (fixed_time)
                b'f', b'i', b'x', b'e', b'd', b'_', b't', b'i', b'm', b'e', 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x17, 0x70, // 6000 BE
                0x00, // end of child
                0x00, // end of root
            ]
        );
    }

    // ------------------------------------------------------------------
    // Reader round-trips (round-trip encode-then-decode)
    // ------------------------------------------------------------------

    fn round_trip(entries: &[(String, NbtTag)]) {
        let encoded = encode_compound_tag(entries).unwrap();
        let decoded = decode_compound_tag(&encoded).unwrap();
        assert_eq!(decoded, entries);
    }

    #[test]
    fn round_trip_byte() {
        round_trip(&[("value".to_owned(), NbtTag::Byte(42))]);
    }

    #[test]
    fn round_trip_short() {
        round_trip(&[("value".to_owned(), NbtTag::Short(-32000))]);
    }

    #[test]
    fn round_trip_int() {
        round_trip(&[("value".to_owned(), NbtTag::Int(0x12345678))]);
    }

    #[test]
    fn round_trip_long() {
        round_trip(&[("value".to_owned(), NbtTag::Long(0x123456789abcdef))]);
    }

    #[test]
    fn round_trip_float() {
        round_trip(&[("value".to_owned(), NbtTag::Float(1.25))]);
    }

    #[test]
    fn round_trip_double() {
        round_trip(&[("value".to_owned(), NbtTag::Double(-2.5))]);
    }

    #[test]
    fn round_trip_string() {
        round_trip(&[("name".to_owned(), NbtTag::String("hello".to_owned()))]);
    }

    #[test]
    fn round_trip_byte_array() {
        round_trip(&[("data".to_owned(), NbtTag::ByteArray(vec![0, 1, 2, 255]))]);
    }

    #[test]
    fn round_trip_int_array() {
        round_trip(&[("data".to_owned(), NbtTag::IntArray(vec![1, -2, 3, -4]))]);
    }

    #[test]
    fn round_trip_long_array() {
        round_trip(&[("data".to_owned(), NbtTag::LongArray(vec![1, -2, 3]))]);
    }

    #[test]
    fn round_trip_empty_byte_array() {
        round_trip(&[("data".to_owned(), NbtTag::ByteArray(vec![]))]);
    }

    #[test]
    fn round_trip_empty_int_array() {
        round_trip(&[("data".to_owned(), NbtTag::IntArray(vec![]))]);
    }

    #[test]
    fn round_trip_empty_long_array() {
        round_trip(&[("data".to_owned(), NbtTag::LongArray(vec![]))]);
    }

    #[test]
    fn round_trip_empty_list() {
        round_trip(&[("items".to_owned(), NbtTag::List(vec![]))]);
    }

    #[test]
    fn round_trip_list_of_ints() {
        round_trip(&[(
            "nums".to_owned(),
            NbtTag::List(vec![NbtTag::Int(10), NbtTag::Int(20), NbtTag::Int(30)]),
        )]);
    }

    #[test]
    fn round_trip_nested_compound() {
        round_trip(&[(
            "outer".to_owned(),
            NbtTag::Compound(vec![("inner".to_owned(), NbtTag::Short(99))]),
        )]);
    }

    #[test]
    fn round_trip_mixed_compound() {
        round_trip(&[
            ("b".to_owned(), NbtTag::Byte(1)),
            ("s".to_owned(), NbtTag::String("text".to_owned())),
            ("ia".to_owned(), NbtTag::IntArray(vec![7, 8, 9])),
        ]);
    }

    // ------------------------------------------------------------------
    // decode_compound_tag
    // ------------------------------------------------------------------

    #[test]
    fn decode_compound_tag_rejects_non_compound() {
        let result = decode_compound_tag(&[TAG_BYTE, 0x00, 0x01, 42]);
        assert_eq!(result, Err(ProtocolError::InvalidPacketPayload));
    }

    #[test]
    fn decode_compound_tag_empty() {
        let encoded = encode_compound_tag(&[]).unwrap();
        // 0x0a + 0x00 = 2 bytes
        assert_eq!(encoded, vec![0x0a, 0x00]);
        let decoded = decode_compound_tag(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    // ------------------------------------------------------------------
    // OOM protection: truncated input
    // ------------------------------------------------------------------

    #[test]
    fn truncated_byte_array_returns_eof() {
        // ByteArray with length 10 but only 3 bytes follow
        let input = vec![
            0x0a, 0x07, 0x00, 0x02, b'd', b't', 0x00, 0x00, 0x00, 0x0a, 1, 2, 3,
        ];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::UnexpectedEndOfInput)
        );
    }

    #[test]
    fn truncated_int_array_returns_eof() {
        // IntArray with length 3 (needs 12 bytes) but only 4 bytes follow
        let input = vec![
            0x0a, 0x0b, 0x00, 0x02, b'd', b't', 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01,
            0x00,
        ];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::UnexpectedEndOfInput)
        );
    }

    #[test]
    fn truncated_long_array_returns_eof() {
        // LongArray with length 2 (needs 16 bytes) but only 4 bytes follow
        let input = vec![
            0x0a, 0x0c, 0x00, 0x02, b'd', b't', 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::UnexpectedEndOfInput)
        );
    }

    #[test]
    fn truncated_string_returns_eof() {
        // String with length 100 but no data follows
        let input = vec![0x0a, 0x08, 0x00, 0x02, b'd', b't', 0x01, 0x00];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::UnexpectedEndOfInput)
        );
    }

    #[test]
    fn truncated_list_returns_eof() {
        // List with count 100 but no data follows
        let input = vec![
            0x0a, 0x09, 0x00, 0x02, b'd', b't', 0x01, 0x00, 0x00, 0x00, 0x64,
        ];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::UnexpectedEndOfInput)
        );
    }

    // ------------------------------------------------------------------
    // OOM protection: negative lengths
    // ------------------------------------------------------------------

    #[test]
    fn negative_byte_array_length_rejected() {
        let input = vec![0x0a, 0x07, 0x00, 0x02, b'd', b't', 0xff, 0xff, 0xff, 0xff];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn negative_int_array_length_rejected() {
        let input = vec![0x0a, 0x0b, 0x00, 0x02, b'd', b't', 0xff, 0xff, 0xff, 0xff];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn negative_long_array_length_rejected() {
        let input = vec![0x0a, 0x0c, 0x00, 0x02, b'd', b't', 0xff, 0xff, 0xff, 0xff];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn negative_list_count_rejected() {
        let input = vec![
            0x0a, 0x09, 0x00, 0x02, b'd', b't', 0x01, 0xff, 0xff, 0xff, 0xff,
        ];
        assert_eq!(
            decode_compound_tag(&input),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    // ------------------------------------------------------------------
    // Depth limit
    // ------------------------------------------------------------------

    fn build_deep_nbt(depth: usize) -> NbtTag {
        let mut tag = NbtTag::Byte(1);
        for _ in 0..depth {
            tag = NbtTag::Compound(vec![("x".to_owned(), tag)]);
        }
        tag
    }

    #[test]
    fn depth_limit_exceeded() {
        let tag = build_deep_nbt(MAX_NBT_DEPTH);
        let encoded = encode_compound_tag(&[("root".to_owned(), tag)]).unwrap();
        let result = decode_compound_tag(&encoded);
        assert_eq!(result, Err(ProtocolError::NbtDepthExceeded));
    }

    #[test]
    fn depth_limit_at_boundary() {
        // MAX_NBT_DEPTH - 1 nesting levels should succeed
        // (root compound adds 1, so MAX_NBT_DEPTH - 1 nested compounds
        //  inside the root gives total depth = MAX_NBT_DEPTH)
        let tag = build_deep_nbt(MAX_NBT_DEPTH - 1);
        let encoded = encode_compound_tag(&[("root".to_owned(), tag)]).unwrap();
        let result = decode_compound_tag(&encoded);
        assert!(result.is_ok());
        let decoded = result.unwrap();
        assert_eq!(decoded.len(), 1);
    }

    // ------------------------------------------------------------------
    // NbtReader standalone
    // ------------------------------------------------------------------

    #[test]
    fn reader_pos_and_remaining() {
        let data = [0x0a, 0x00];
        let mut reader = NbtReader::new(&data);
        assert_eq!(reader.pos(), 0);
        assert_eq!(reader.remaining(), 2);
        reader.read_tag_type().unwrap();
        assert_eq!(reader.pos(), 1);
        assert_eq!(reader.remaining(), 1);
    }

    #[test]
    fn reader_read_tag_type_eof() {
        let mut reader = NbtReader::new(&[]);
        assert_eq!(
            reader.read_tag_type(),
            Err(ProtocolError::UnexpectedEndOfInput)
        );
    }

    #[test]
    fn reader_read_named_tag_on_end_returns_error() {
        let mut reader = NbtReader::new(&[TAG_END]);
        assert_eq!(
            reader.read_named_tag(),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn reader_unknown_tag_type_rejected() {
        let mut reader = NbtReader::new(&[0x0d]); // invalid tag type
        assert_eq!(
            reader.read_tag_payload(0x0d),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }
}
