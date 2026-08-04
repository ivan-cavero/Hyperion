//! Configuration state codecs (protocol 776, Minecraft 26.2).
//!
//! Every modern client (1.20.2+) passes through Configuration after Login
//! Acknowledged. The server must send registry data, feature flags, update
//! tags and Finish Configuration before the client moves on to Play.
//!
//! Flow implemented here:
//!   S→C Select Known Packs → C→S Known Packs (optional, negotiates NBT data)
//!   S→C Registry Data → S→C Update Enabled Features → S→C Update Tags
//!   S→C Finish Configuration → C→S Acknowledge Finish Configuration

use bytes::Bytes;

use crate::ProtocolError;
use crate::frame::{ByteWriter, PacketCursor, encode_var_i32};

/// Clientbound Finish Configuration.
pub const FINISH_CONFIGURATION_PACKET_ID: i32 = 3;
/// Clientbound Disconnect (configuration).
pub const DISCONNECT_PACKET_ID: i32 = 2;
/// Clientbound Registry Data.
pub const REGISTRY_DATA_PACKET_ID: i32 = 7;
/// Clientbound Update Enabled Features (feature flags).
pub const UPDATE_ENABLED_FEATURES_PACKET_ID: i32 = 12;
/// Clientbound Update Tags.
pub const UPDATE_TAGS_PACKET_ID: i32 = 13;
/// Clientbound Select Known Packs.
pub const SELECT_KNOWN_PACKS_PACKET_ID: i32 = 14;
/// Clientbound Code of Conduct (26.2).
pub const CODE_OF_CONDUCT_PACKET_ID: i32 = 19;

/// Serverbound Client Information.
pub const CLIENT_INFORMATION_PACKET_ID: i32 = 0;
/// Serverbound Cookie Response.
pub const COOKIE_RESPONSE_PACKET_ID: i32 = 1;
/// Serverbound Custom Payload.
pub const CUSTOM_PAYLOAD_PACKET_ID: i32 = 2;
/// Serverbound Acknowledge Finish Configuration.
pub const ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID: i32 = 3;
/// Serverbound Keep Alive (configuration).
pub const KEEP_ALIVE_PACKET_ID: i32 = 4;
/// Serverbound Pong.
pub const PONG_PACKET_ID: i32 = 5;
/// Serverbound Resource Pack Response.
pub const RESOURCE_PACK_RESPONSE_PACKET_ID: i32 = 6;
/// Serverbound Known Packs (`select_known_packs`).
pub const KNOWN_PACKS_PACKET_ID: i32 = 7;
/// Serverbound Custom Click Action.
pub const CUSTOM_CLICK_ACTION_PACKET_ID: i32 = 8;
/// Serverbound Accept Code of Conduct (26.2).
pub const ACCEPT_CODE_OF_CONDUCT_PACKET_ID: i32 = 9;

/// Maximum UTF-16 length of a known-pack component (vanilla packs are tiny,
/// but the wiki does not bound them tightly).
pub const MAX_KNOWN_PACK_STRING_UTF16_UNITS: usize = 64;
/// Maximum UTF-16 length of a locale string.
pub const MAX_LOCALE_UTF16_UNITS: usize = 16;

/// A pack known to the server, negotiated through Select Known Packs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

/// Client Information (configuration): client settings the server must know
/// before entering Play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInformation {
    pub locale: String,
    pub view_distance: u8,
    pub chat_mode: i32,
    pub chat_colors: bool,
    pub displayed_skin_parts: u8,
    pub main_hand: i32,
    pub enable_text_filtering: bool,
    pub allow_server_listings: bool,
    pub particle_status: i32,
}

/// A synchronized registry: id + its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryData {
    pub registry_id: String,
    pub entries: Vec<RegistryEntry>,
}

/// One registry entry: id plus optional NBT data.
///
/// When the client negotiated `minecraft:core` through Known Packs, `data`
/// may be `None` and the client fills in its bundled definitions. Otherwise
/// the NBT must be present (the client only has registry data from its own
/// data pack when no pack was negotiated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub id: String,
    pub data: Option<Vec<u8>>,
}

/// A tagged registry for Update Tags: tag names → entry ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedRegistry {
    pub registry_id: String,
    pub tags: Vec<(String, Vec<i32>)>,
}

/// Decodes the serverbound Known Packs packet.
pub fn decode_known_packs(frame: &crate::PacketFrame) -> Result<Vec<KnownPack>, ProtocolError> {
    let mut cursor = PacketCursor::new(&frame.payload);
    let count = cursor.read_var_i32()?;
    let count = usize::try_from(count).map_err(|_| ProtocolError::InvalidPacketPayload)?;
    // Each pack is at least three empty length-prefixed strings (1 byte each).
    // Reject before allocating so a hostile VarInt cannot OOM via `with_capacity`.
    const MIN_PACK_BYTES: usize = 3;
    if count
        .checked_mul(MIN_PACK_BYTES)
        .is_none_or(|needed| needed > cursor.remaining())
    {
        return Err(ProtocolError::InvalidPacketPayload);
    }
    let mut packs = Vec::with_capacity(count);
    for _ in 0..count {
        let namespace = cursor.read_string(MAX_KNOWN_PACK_STRING_UTF16_UNITS)?;
        let id = cursor.read_string(MAX_KNOWN_PACK_STRING_UTF16_UNITS)?;
        let version = cursor.read_string(MAX_KNOWN_PACK_STRING_UTF16_UNITS)?;
        packs.push(KnownPack {
            namespace,
            id,
            version,
        });
    }
    cursor.finish()?;
    Ok(packs)
}

/// Decodes the serverbound Client Information packet.
pub fn decode_client_information(
    frame: &crate::PacketFrame,
) -> Result<ClientInformation, ProtocolError> {
    let mut cursor = PacketCursor::new(&frame.payload);
    let client_information = ClientInformation {
        locale: cursor.read_string(MAX_LOCALE_UTF16_UNITS)?,
        view_distance: cursor.read_u8()?,
        chat_mode: cursor.read_var_i32()?,
        chat_colors: cursor.read_bool()?,
        displayed_skin_parts: cursor.read_u8()?,
        main_hand: cursor.read_var_i32()?,
        enable_text_filtering: cursor.read_bool()?,
        allow_server_listings: cursor.read_bool()?,
        particle_status: cursor.read_var_i32()?,
    };
    cursor.finish()?;
    Ok(client_information)
}

/// Validates an Acknowledge Finish Configuration packet (must be empty).
pub fn decode_finish_configuration_ack(frame: &crate::PacketFrame) -> Result<(), ProtocolError> {
    if frame.packet_id != ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID {
        return Err(ProtocolError::InvalidPacketId);
    }
    if frame.payload.is_empty() {
        Ok(())
    } else {
        Err(ProtocolError::InvalidPacketPayload)
    }
}

/// Encodes the payload of a Clientbound Registry Data packet.
///
/// Since 1.20.5 each packet carries a single registry: the registry ID
/// followed by its entries (entry ID + optional full NBT data). The entry
/// order defines the numeric IDs assigned by the client, starting at 0.
pub fn encode_registry_data_payload(registry: &RegistryData) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(64 + registry.entries.len() * 32);
    writer.push_string(&registry.registry_id, 32_767)?;
    writer.push_var_i32(registry.entries.len() as i32);
    for entry in &registry.entries {
        writer.push_string(&entry.id, 32_767)?;
        match &entry.data {
            Some(data) => {
                writer.push_bool(true);
                writer.push_bytes(data);
            }
            None => writer.push_bool(false),
        }
    }
    Ok(writer.into_bytes())
}

/// Encodes Clientbound Select Known Packs.
pub fn encode_select_known_packs_payload(packs: &[KnownPack]) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::new();
    writer.push_var_i32(packs.len() as i32);
    for pack in packs {
        writer.push_string(&pack.namespace, MAX_KNOWN_PACK_STRING_UTF16_UNITS)?;
        writer.push_string(&pack.id, MAX_KNOWN_PACK_STRING_UTF16_UNITS)?;
        writer.push_string(&pack.version, MAX_KNOWN_PACK_STRING_UTF16_UNITS)?;
    }
    Ok(writer.into_bytes())
}

/// Encodes Clientbound Update Enabled Features (feature flags).
pub fn encode_feature_flags_payload(features: &[&str]) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::new();
    writer.push_var_i32(features.len() as i32);
    for feature in features {
        writer.push_string(feature, 32_767)?;
    }
    Ok(writer.into_bytes())
}

/// Encodes Clientbound Update Tags.
pub fn encode_update_tags_payload(tags: &[TaggedRegistry]) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::new();
    writer.push_var_i32(tags.len() as i32);
    for registry in tags {
        writer.push_string(&registry.registry_id, 32_767)?;
        writer.push_var_i32(registry.tags.len() as i32);
        for (tag_name, entries) in &registry.tags {
            writer.push_string(tag_name, 32_767)?;
            writer.push_var_i32(entries.len() as i32);
            for entry in entries {
                writer.push_var_i32(*entry);
            }
        }
    }
    Ok(writer.into_bytes())
}

/// Encodes the Clientbound Code of Conduct payload: a single UTF-8 string
/// (the code of conduct text/URL), `ByteBufCodecs.STRING_UTF8` on the wire.
pub fn encode_code_of_conduct_payload(code_of_conduct: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::new();
    writer.push_string(code_of_conduct, 32_767)?;
    Ok(writer.into_bytes())
}

/// Encodes the (empty) Clientbound Finish Configuration payload.
pub fn encode_finish_configuration_payload() -> Vec<u8> {
    Vec::new()
}

/// Encodes a Clientbound Disconnect payload in configuration state.
/// `reason` must already be a JSON text component.
pub fn encode_disconnect_payload(reason: &str) -> Result<Vec<u8>, ProtocolError> {
    crate::frame::encode_string(reason, 262_144)
}

/// Convenience: encodes a complete Clientbound Finish Configuration frame.
pub fn encode_finish_configuration() -> Result<Bytes, ProtocolError> {
    Ok(Bytes::from(encode_var_i32(FINISH_CONFIGURATION_PACKET_ID)))
}

/// Convenience: encodes a complete Clientbound Registry Data frame.
pub fn encode_registry_data(registry: &RegistryData) -> Result<Bytes, ProtocolError> {
    let mut frame = encode_var_i32(REGISTRY_DATA_PACKET_ID);
    frame.extend_from_slice(&encode_registry_data_payload(registry)?);
    Ok(Bytes::from(frame))
}

/// Convenience: encodes a complete Clientbound Select Known Packs frame.
pub fn encode_select_known_packs(packs: &[KnownPack]) -> Result<Bytes, ProtocolError> {
    let mut frame = encode_var_i32(SELECT_KNOWN_PACKS_PACKET_ID);
    frame.extend_from_slice(&encode_select_known_packs_payload(packs)?);
    Ok(Bytes::from(frame))
}

/// Convenience: encodes a complete Clientbound Update Enabled Features frame.
pub fn encode_feature_flags(features: &[&str]) -> Result<Bytes, ProtocolError> {
    let mut frame = encode_var_i32(UPDATE_ENABLED_FEATURES_PACKET_ID);
    frame.extend_from_slice(&encode_feature_flags_payload(features)?);
    Ok(Bytes::from(frame))
}

/// Convenience: encodes a complete Clientbound Update Tags frame.
pub fn encode_update_tags(tags: &[TaggedRegistry]) -> Result<Bytes, ProtocolError> {
    let mut frame = encode_var_i32(UPDATE_TAGS_PACKET_ID);
    frame.extend_from_slice(&encode_update_tags_payload(tags)?);
    Ok(Bytes::from(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::encode_frame;

    #[test]
    fn known_packs_round_trip() {
        let packs = vec![KnownPack {
            namespace: "minecraft".to_owned(),
            id: "core".to_owned(),
            version: "26.2".to_owned(),
        }];
        let payload = encode_select_known_packs_payload(&packs).unwrap();
        let frame = crate::PacketFrame {
            packet_id: KNOWN_PACKS_PACKET_ID,
            payload: Bytes::from(payload),
        };
        assert_eq!(decode_known_packs(&frame).unwrap(), packs);
    }

    #[test]
    fn known_packs_rejects_huge_count_without_oom() {
        // Regression: CI fuzz found OOM via `Vec::with_capacity` on a hostile
        // VarInt count that exceeds the remaining payload
        // (artifact oom-4b4eaadd… / input after frame split).
        let frame = crate::PacketFrame {
            packet_id: KNOWN_PACKS_PACKET_ID,
            payload: Bytes::from(vec![0xfa, 0xff, 0xff, 0x7e, 0xff, 0xff, 0xff, 0xff, 0xff]),
        };
        assert_eq!(
            decode_known_packs(&frame),
            Err(ProtocolError::InvalidPacketPayload)
        );
    }

    #[test]
    fn client_information_round_trip() {
        let info = ClientInformation {
            locale: "es_ES".to_owned(),
            view_distance: 8,
            chat_mode: 0,
            chat_colors: true,
            displayed_skin_parts: 0x7f,
            main_hand: 1,
            enable_text_filtering: false,
            allow_server_listings: true,
            particle_status: 0,
        };
        let mut writer = ByteWriter::new();
        writer
            .push_string(&info.locale, MAX_LOCALE_UTF16_UNITS)
            .unwrap();
        writer.push_u8(info.view_distance);
        writer.push_var_i32(info.chat_mode);
        writer.push_bool(info.chat_colors);
        writer.push_u8(info.displayed_skin_parts);
        writer.push_var_i32(info.main_hand);
        writer.push_bool(info.enable_text_filtering);
        writer.push_bool(info.allow_server_listings);
        writer.push_var_i32(info.particle_status);
        let frame = crate::PacketFrame {
            packet_id: CLIENT_INFORMATION_PACKET_ID,
            payload: Bytes::from(writer.into_bytes()),
        };
        assert_eq!(decode_client_information(&frame).unwrap(), info);
    }

    #[test]
    fn registry_data_encodes_entries_with_optional_nbt() {
        let registry = RegistryData {
            registry_id: "minecraft:dimension_type".to_owned(),
            entries: vec![
                RegistryEntry {
                    id: "minecraft:overworld".to_owned(),
                    data: Some(crate::nbt::encode_string_tag("x").unwrap()),
                },
                RegistryEntry {
                    id: "minecraft:the_end".to_owned(),
                    data: None,
                },
            ],
        };
        let payload = encode_registry_data_payload(&registry).unwrap();
        let frame = crate::PacketFrame {
            packet_id: REGISTRY_DATA_PACKET_ID,
            payload: Bytes::from(payload),
        };
        // Verify by decoding the raw structure manually: id, entries.
        let mut cursor = PacketCursor::new(&frame.payload);
        assert_eq!(
            cursor.read_string(32_767).unwrap(),
            "minecraft:dimension_type"
        );
        assert_eq!(cursor.read_var_i32().unwrap(), 2);
        assert_eq!(cursor.read_string(32_767).unwrap(), "minecraft:overworld");
        assert!(cursor.read_bool().unwrap());
        assert_eq!(cursor.read_u8().unwrap(), 0x08); // NBT string tag
        let nbt_length = cursor.read_u16().unwrap() as usize;
        assert_eq!(cursor.read_fixed_bytes(nbt_length).unwrap(), b"x");
        assert_eq!(cursor.read_string(32_767).unwrap(), "minecraft:the_end");
        assert!(!cursor.read_bool().unwrap());
        assert_eq!(cursor.finish(), Ok(()));
    }

    #[test]
    fn feature_flags_and_tags_are_wire_sized() {
        let flags = encode_feature_flags_payload(&["minecraft:vanilla"]).unwrap();
        assert_eq!(
            flags,
            vec![
                0x01, 0x11, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'v', b'a',
                b'n', b'i', b'l', b'l', b'a'
            ]
        );

        let tags = vec![TaggedRegistry {
            registry_id: "minecraft:damage_type".to_owned(),
            tags: vec![("minecraft:is_fire".to_owned(), vec![1, 2, 3])],
        }];
        let payload = encode_update_tags_payload(&tags).unwrap();
        let mut cursor = PacketCursor::new(&payload);
        assert_eq!(cursor.read_var_i32().unwrap(), 1);
        assert_eq!(cursor.read_string(32_767).unwrap(), "minecraft:damage_type");
        assert_eq!(cursor.read_var_i32().unwrap(), 1);
        assert_eq!(cursor.read_string(32_767).unwrap(), "minecraft:is_fire");
        assert_eq!(cursor.read_var_i32().unwrap(), 3);
        assert_eq!(cursor.read_var_i32().unwrap(), 1);
        assert_eq!(cursor.read_var_i32().unwrap(), 2);
        assert_eq!(cursor.read_var_i32().unwrap(), 3);
        assert_eq!(cursor.finish(), Ok(()));
    }

    #[test]
    fn code_of_conduct_payload_is_a_var_int_string() {
        // Matches the client's `ByteBufCodecs.STRING_UTF8`: one UTF-8 string
        // with a VarInt length prefix (not the u16 length used inside NBT).
        let payload =
            encode_code_of_conduct_payload("https://aka.ms/MinecraftCodeOfConduct").unwrap();
        assert_eq!(payload[0] & 0x80, 0); // single-byte VarInt length
        let length = usize::from(payload[0]);
        assert_eq!(
            &payload[1..],
            "https://aka.ms/MinecraftCodeOfConduct".as_bytes()
        );
        assert_eq!(length, payload.len() - 1);

        // Round-trip through the crate's own VarInt string decoder.
        let (decoded_length, consumed) = crate::frame::decode_var_i32(&payload, 0, 5).unwrap();
        assert_eq!(decoded_length as usize, payload.len() - consumed);
    }

    #[test]
    fn finish_configuration_frame_is_valid() {
        let frame = encode_finish_configuration().unwrap();
        let decoded = crate::decode_packet_data(frame).unwrap();
        assert_eq!(decoded.packet_id, FINISH_CONFIGURATION_PACKET_ID);
        assert!(decoded.payload.is_empty());
        let _ = encode_frame(FINISH_CONFIGURATION_PACKET_ID, &[]).unwrap();
    }
}
