//! Minimal network-NBT writer for protocol 776 (Minecraft 26.2).
//!
//! Since 1.20.2 the network NBT format is the standard NBT binary format
//! (big-endian numbers, unsigned-short length prefixes on strings) with one
//! difference: the root tag carries **no name** (no `u16` length prefix).
//! This matches the format vanilla exchanges for registry data and, since
//! 1.20.5, for text components.
//!
//! Only the tags Hyperion needs today are implemented: Byte, Int, Long,
//! Float, Double, String, Compound and List (homogeneous elements). A full
//! NBT reader/writer is a later milestone (see ROADMAP: "NBT propio").

use crate::ProtocolError;
use crate::frame::ByteWriter;

const TAG_BYTE: u8 = 0x01;
const TAG_INT: u8 = 0x03;
const TAG_LONG: u8 = 0x04;
const TAG_FLOAT: u8 = 0x05;
const TAG_DOUBLE: u8 = 0x06;
const TAG_STRING: u8 = 0x08;
const TAG_LIST: u8 = 0x09;
const TAG_COMPOUND: u8 = 0x0a;
const TAG_END: u8 = 0x00;

/// A named NBT tag stored inside a compound.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtTag {
    Byte(i8),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    /// A homogeneous list (all elements share one tag type).
    List(Vec<NbtTag>),
    Compound(Vec<(String, NbtTag)>),
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
        NbtTag::Int(_) => writer.push_u8(TAG_INT),
        NbtTag::Long(_) => writer.push_u8(TAG_LONG),
        NbtTag::Float(_) => writer.push_u8(TAG_FLOAT),
        NbtTag::Double(_) => writer.push_u8(TAG_DOUBLE),
        NbtTag::String(_) => writer.push_u8(TAG_STRING),
        NbtTag::List(_) => writer.push_u8(TAG_LIST),
        NbtTag::Compound(_) => writer.push_u8(TAG_COMPOUND),
    }
    push_u16_string(writer, name)?;
    write_tag_payload(writer, tag)
}

fn write_tag_payload(writer: &mut ByteWriter, tag: &NbtTag) -> Result<(), ProtocolError> {
    match tag {
        NbtTag::Byte(value) => writer.push_byte(*value),
        NbtTag::Int(value) => writer.push_i32(*value),
        NbtTag::Long(value) => writer.push_i64(*value),
        NbtTag::Float(value) => writer.push_f32(*value),
        NbtTag::Double(value) => writer.push_f64(*value),
        NbtTag::String(value) => push_u16_string(writer, value)?,
        NbtTag::List(elements) => write_list_payload(writer, elements)?,
        NbtTag::Compound(tags) => {
            for (name, tag) in tags {
                write_named_tag(writer, name, tag)?;
            }
            writer.push_u8(TAG_END);
        }
    }
    Ok(())
}

/// Writes an NBT list payload: element-type byte + u32 big-endian count +
/// the elements themselves (no names). An empty list uses `TAG_END` (0x00)
/// as its element type, matching the vanilla convention.
fn write_list_payload(writer: &mut ByteWriter, elements: &[NbtTag]) -> Result<(), ProtocolError> {
    let element_type = elements.first().map(tag_type_byte).unwrap_or(TAG_END);
    writer.push_u8(element_type);
    writer.push_u32(elements.len() as u32);
    for element in elements {
        write_tag_payload(writer, element)?;
    }
    Ok(())
}

fn tag_type_byte(tag: &NbtTag) -> u8 {
    match tag {
        NbtTag::Byte(_) => TAG_BYTE,
        NbtTag::Int(_) => TAG_INT,
        NbtTag::Long(_) => TAG_LONG,
        NbtTag::Float(_) => TAG_FLOAT,
        NbtTag::Double(_) => TAG_DOUBLE,
        NbtTag::String(_) => TAG_STRING,
        NbtTag::List(_) => TAG_LIST,
        NbtTag::Compound(_) => TAG_COMPOUND,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_string_tag_has_no_name() {
        let encoded = encode_string_tag("hola").unwrap();
        // 0x08 (type) + u16 length 4 + "hola" — no name prefix.
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
                0x04, 0x00, 0x0a, // TAG_Long, name length 10 (ixed_time)
                b'f', b'i', b'x', b'e', b'd', b'_', b't', b'i', b'm', b'e', 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x17, 0x70, // 6000 BE
                0x00, // end of child
                0x00, // end of root
            ]
        );
    }
}
