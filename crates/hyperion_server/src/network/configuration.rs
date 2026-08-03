//! Configuration state (protocol 776, Minecraft 26.2).
//!
//! Vanilla sequence (protocol FAQ + Registries wiki):
//!
//! 1. **S→C** Feature Flags  
//! 2. **S→C** Select Known Packs (`minecraft:core@26.2`)  
//! 3. **C→S** Select Known Packs (intersection)  
//! 4. **S→C** Registry Data × N  
//! 5. **S→C** Update Tags — full expanded tag set (never from known packs)  
//! 6. **S→C** Code of Conduct → **C→S** Accept  
//! 7. **S→C** Finish Configuration → **C→S** Acknowledge  
//!
//! When `minecraft:core` is negotiated we omit per-entry NBT (`data: None`)
//! and the client loads definitions from its own jar — same as vanilla.
//! That is required because network codecs reject unknown datapack-only
//! fields (features, carvers, …) if we push raw datapack JSON as NBT.
//!
//! If the client rejects core we fall back to packed network NBT from
//! `join_data/registry_nbt.bin` (best-effort).

use hyperion_protocol::{
    ACCEPT_CODE_OF_CONDUCT_PACKET_ID, CODE_OF_CONDUCT_PACKET_ID,
    CONFIGURATION_CLIENT_INFORMATION_PACKET_ID, CONFIGURATION_KEEP_ALIVE_PACKET_ID,
    COOKIE_RESPONSE_PACKET_ID, CUSTOM_CLICK_ACTION_PACKET_ID, CUSTOM_PAYLOAD_PACKET_ID,
    FINISH_CONFIGURATION_PACKET_ID, KNOWN_PACKS_PACKET_ID, KnownPack, ProtocolError,
    REGISTRY_DATA_PACKET_ID, RESOURCE_PACK_RESPONSE_PACKET_ID, RegistryData, RegistryEntry,
    SELECT_KNOWN_PACKS_PACKET_ID, TaggedRegistry, UPDATE_ENABLED_FEATURES_PACKET_ID,
    UPDATE_TAGS_PACKET_ID, decode_client_information, decode_finish_configuration_ack,
    decode_known_packs, encode_code_of_conduct_payload, encode_feature_flags_payload,
    encode_finish_configuration_payload, encode_registry_data_payload,
    encode_select_known_packs_payload, encode_update_tags_payload,
};
use tracing::{debug, info, trace, warn};

use super::connection::{Connection, ConnectionError};
use super::join_data::{
    CORE_KNOWN_PACK, VANILLA_REGISTRIES, VANILLA_TAGS, registry_entry_nbt,
};
use crate::config::ServerConfig;

/// Sections in a default world column (Y in -64..=320).
pub(super) const SECTION_COUNT: i32 = 24;

/// `minecraft:overworld` index in our dimension_type listing (overworld first).
pub(super) const OVERWORLD_DIMENSION_TYPE_ID: i32 = 0;

/// `minecraft:plains` index in our biome listing (plains first).
pub(super) const PLAINS_BIOME_ID: i32 = 0;

const MAX_CONFIGURATION_DRAIN: usize = 32;
const CODE_OF_CONDUCT_TEXT: &str = "https://aka.ms/MinecraftCodeOfConduct";

/// Runs Configuration end-to-end until the client is ready for Play.
pub(super) async fn serve_configuration(
    connection: &mut Connection,
    _config: &ServerConfig,
) -> Result<(), ConnectionError> {
    // --- 1. Feature flags -------------------------------------------------
    connection
        .write_frame(
            UPDATE_ENABLED_FEATURES_PACKET_ID,
            &encode_feature_flags_payload(&["minecraft:vanilla"])?,
        )
        .await?;

    // --- 2–3. Known packs negotiation ------------------------------------
    let (ns, id, version) = CORE_KNOWN_PACK;
    let offered = [KnownPack {
        namespace: ns.to_owned(),
        id: id.to_owned(),
        version: version.to_owned(),
    }];
    connection
        .write_frame(
            SELECT_KNOWN_PACKS_PACKET_ID,
            &encode_select_known_packs_payload(&offered)?,
        )
        .await?;
    debug!(namespace = ns, id, version, "offered known packs");

    let client_packs = drain_until_known_packs(connection).await?;
    // Accept any minecraft:core version the client reports (some loaders
    // may differ only in the version string).
    let core_negotiated = client_packs
        .iter()
        .any(|pack| pack.namespace == ns && pack.id == id);
    info!(?client_packs, core_negotiated, "known packs response from client");

    // --- 4. Registry Data ------------------------------------------------
    for (registry_id, entry_paths) in VANILLA_REGISTRIES {
        let mut entries = Vec::with_capacity(entry_paths.len());
        for path in *entry_paths {
            let data = if core_negotiated {
                None
            } else {
                let nbt = registry_entry_nbt(registry_id, path).ok_or_else(|| {
                    warn!(%registry_id, %path, "missing packed NBT for registry entry");
                    ConnectionError::Protocol(ProtocolError::InvalidPacketPayload)
                })?;
                Some(nbt.to_vec())
            };
            entries.push(RegistryEntry {
                id: format!("minecraft:{path}"),
                data,
            });
        }
        let registry = RegistryData {
            registry_id: (*registry_id).to_owned(),
            entries,
        };
        connection
            .write_frame(
                REGISTRY_DATA_PACKET_ID,
                &encode_registry_data_payload(&registry)?,
            )
            .await?;
    }
    debug!(
        registries = VANILLA_REGISTRIES.len(),
        core_negotiated,
        "registry data sent"
    );

    // --- 5. Update Tags (complete set — never from known packs) ----------
    // Incomplete tags were the main cause of
    // `Failed to load registries due to errors` after Finish Configuration:
    // datapack definitions reference block/timeline/damage tags that must
    // resolve when the client freezes the registry set.
    let tags = build_update_tags();
    connection
        .write_frame(UPDATE_TAGS_PACKET_ID, &encode_update_tags_payload(&tags)?)
        .await?;
    debug!(registries = tags.len(), "update tags sent");

    // --- 6. Code of Conduct (26.2) ----------------------------------------
    connection
        .write_frame(
            CODE_OF_CONDUCT_PACKET_ID,
            &encode_code_of_conduct_payload(CODE_OF_CONDUCT_TEXT)?,
        )
        .await?;
    drain_until_code_of_conduct(connection).await?;
    trace!("code of conduct accepted");

    // --- 7. Finish Configuration -----------------------------------------
    connection
        .write_frame(
            FINISH_CONFIGURATION_PACKET_ID,
            &encode_finish_configuration_payload(),
        )
        .await?;
    let response = connection.read_frame().await?;
    decode_finish_configuration_ack(&response)?;
    info!("configuration finished");
    Ok(())
}

fn build_update_tags() -> Vec<TaggedRegistry> {
    VANILLA_TAGS
        .iter()
        .map(|(registry_id, tags)| TaggedRegistry {
            registry_id: (*registry_id).to_owned(),
            tags: tags
                .iter()
                .map(|(name, ids)| ((*name).to_owned(), ids.to_vec()))
                .collect(),
        })
        .collect()
}

async fn drain_until_known_packs(
    connection: &mut Connection,
) -> Result<Vec<KnownPack>, ConnectionError> {
    for _ in 0..MAX_CONFIGURATION_DRAIN {
        let packet = connection.read_frame().await?;
        match packet.packet_id {
            KNOWN_PACKS_PACKET_ID => return decode_known_packs(&packet).map_err(Into::into),
            CONFIGURATION_CLIENT_INFORMATION_PACKET_ID => {
                let _ = decode_client_information(&packet)?;
            }
            COOKIE_RESPONSE_PACKET_ID
            | CUSTOM_PAYLOAD_PACKET_ID
            | CONFIGURATION_KEEP_ALIVE_PACKET_ID
            | RESOURCE_PACK_RESPONSE_PACKET_ID
            | CUSTOM_CLICK_ACTION_PACKET_ID => {}
            other => {
                warn!(packet_id = other, "unexpected packet while awaiting known packs");
                return Err(ConnectionError::Protocol(ProtocolError::InvalidPacketId));
            }
        }
    }
    Err(ConnectionError::Protocol(ProtocolError::InvalidPacketPayload))
}

async fn drain_until_code_of_conduct(connection: &mut Connection) -> Result<(), ConnectionError> {
    for _ in 0..MAX_CONFIGURATION_DRAIN {
        let packet = connection.read_frame().await?;
        match packet.packet_id {
            ACCEPT_CODE_OF_CONDUCT_PACKET_ID => return Ok(()),
            CONFIGURATION_CLIENT_INFORMATION_PACKET_ID => {
                let _ = decode_client_information(&packet)?;
            }
            KNOWN_PACKS_PACKET_ID
            | COOKIE_RESPONSE_PACKET_ID
            | CUSTOM_PAYLOAD_PACKET_ID
            | CONFIGURATION_KEEP_ALIVE_PACKET_ID
            | RESOURCE_PACK_RESPONSE_PACKET_ID
            | CUSTOM_CLICK_ACTION_PACKET_ID => {}
            other => {
                warn!(
                    packet_id = other,
                    "unexpected packet while awaiting code of conduct"
                );
                return Err(ConnectionError::Protocol(ProtocolError::InvalidPacketId));
            }
        }
    }
    Err(ConnectionError::Protocol(ProtocolError::InvalidPacketPayload))
}
