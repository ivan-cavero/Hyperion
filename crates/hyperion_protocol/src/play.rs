//! Play-state codecs for protocol 776 (Minecraft 26.2).
//!
//! Payload encoders for the "spawn into the world" sequence and payload
//! decoders for the minimum traffic a vanilla client sends after joining.
//! All packet IDs and field orders were verified against the protocol dump
//! for 26.2.
//!
//! The chunk encoder produces an all-air chunk with full-brightness sky
//! light: enough for the vanilla client to finish loading the world and
//! render the void.

use crate::ProtocolError;
use crate::frame::{ByteWriter, PacketCursor, encode_var_i32};
use crate::nbt::encode_string_tag;

// --- Clientbound (server -> client) packet IDs, Play state ---

/// ID of the clientbound `chunk_batch_finished` packet.
pub const CHUNK_BATCH_FINISHED_PACKET_ID: i32 = 11;
/// ID of the clientbound `chunk_batch_start` packet.
pub const CHUNK_BATCH_START_PACKET_ID: i32 = 12;
/// ID of the clientbound Play `custom_payload` packet (plugin message / brand).
pub const CUSTOM_PAYLOAD_PLAY_PACKET_ID: i32 = 24;
/// ID of the clientbound `disconnect` packet (Play state).
pub const DISCONNECT_PACKET_ID: i32 = 32;
/// ID of the clientbound `game_event` / `game_state_change` packet.
pub const GAME_EVENT_PACKET_ID: i32 = 38;
/// ID of the clientbound `keep_alive` packet (Play state).
pub const KEEP_ALIVE_PACKET_ID: i32 = 44;
/// ID of the clientbound `ping` packet (Play state).
pub const PING_PACKET_ID: i32 = 61;
/// ID of the clientbound `unload_chunk` packet.
pub const UNLOAD_CHUNK_PACKET_ID: i32 = 37;
/// ID of the clientbound `level_chunk_with_light` / `map_chunk` packet.
pub const LEVEL_CHUNK_WITH_LIGHT_PACKET_ID: i32 = 45;
/// ID of the clientbound `login` packet (Play state).
pub const LOGIN_PACKET_ID: i32 = 49;
/// ID of the clientbound `player_abilities` packet.
pub const PLAYER_ABILITIES_PACKET_ID: i32 = 64;
/// ID of the clientbound `player_info_update` packet.
pub const PLAYER_INFO_UPDATE_PACKET_ID: i32 = 70;
/// ID of the clientbound `player_position` packet.
pub const PLAYER_POSITION_PACKET_ID: i32 = 72;
/// ID of the clientbound `server_data` packet.
pub const SERVER_DATA_PACKET_ID: i32 = 86;
/// ID of the clientbound `set_chunk_cache_center` / `update_view_position` packet.
pub const SET_CHUNK_CACHE_CENTER_PACKET_ID: i32 = 94;
/// ID of the clientbound `set_chunk_cache_radius` / `update_view_distance` packet.
pub const SET_CHUNK_CACHE_RADIUS_PACKET_ID: i32 = 95;
/// ID of the clientbound `set_default_spawn_position` packet.
pub const SET_DEFAULT_SPAWN_POSITION_PACKET_ID: i32 = 97;
/// ID of the clientbound `set_held_item` / `held_item_slot` packet.
pub const SET_HELD_ITEM_PACKET_ID: i32 = 105;
/// ID of the clientbound `set_simulation_distance` packet.
pub const SET_SIMULATION_DISTANCE_PACKET_ID: i32 = 111;
/// ID of the clientbound `set_time` packet.
pub const UPDATE_TIME_PACKET_ID: i32 = 113;
/// ID of the clientbound `system_chat` packet.
pub const SYSTEM_CHAT_MESSAGE_PACKET_ID: i32 = 121;
/// ID of the clientbound `set_ticking_state` packet.
pub const SET_TICKING_STATE_PACKET_ID: i32 = 127;

/// Game event reason: change game mode (`game_state_change` reason 3).
pub const GAME_EVENT_CHANGE_GAME_MODE: u8 = 3;
/// Game event reason: start waiting for level chunks (reason 13).
pub const GAME_EVENT_START_WAITING_FOR_LEVEL_CHUNKS: u8 = 13;

/// Player abilities flag: invulnerable.
pub const ABILITY_INVULNERABLE: i8 = 0x01;
/// Player abilities flag: currently flying.
pub const ABILITY_FLYING: i8 = 0x02;
/// Player abilities flag: may fly.
pub const ABILITY_ALLOW_FLYING: i8 = 0x04;
/// Player abilities flag: creative instant-break / creative inventory.
pub const ABILITY_CREATIVE_MODE: i8 = 0x08;
/// Full creative ability set (invulnerable + flying + allow fly + creative).
pub const ABILITIES_CREATIVE: i8 =
    ABILITY_INVULNERABLE | ABILITY_FLYING | ABILITY_ALLOW_FLYING | ABILITY_CREATIVE_MODE;

// --- Serverbound (client -> server) packet IDs, Play state ---

/// ID of the serverbound `confirm_teleportation` packet.
pub const CONFIRM_TELEPORTATION_PACKET_ID: i32 = 0;
/// ID of the serverbound `chat` packet.
pub const CHAT_MESSAGE_PACKET_ID: i32 = 9;
/// ID of the serverbound `chat_session_update` packet.
pub const CHAT_SESSION_UPDATE_PACKET_ID: i32 = 10;
/// ID of the serverbound `chunk_batch_received` packet.
pub const CHUNK_BATCH_RECEIVED_PACKET_ID: i32 = 11;
/// ID of the serverbound `client_tick_end` packet.
pub const CLIENT_TICK_END_PACKET_ID: i32 = 13;
/// ID of the serverbound `client_information` packet (Play state).
pub const CLIENT_INFORMATION_PACKET_ID: i32 = 14;
/// ID of the serverbound `keep_alive` packet (Play state).
pub const SERVERBOUND_KEEP_ALIVE_PACKET_ID: i32 = 28;
/// ID of the serverbound `ping_request` packet.
pub const PING_REQUEST_PACKET_ID: i32 = 38;
/// ID of the serverbound `move_player_pos` packet.
pub const MOVE_PLAYER_POS_PACKET_ID: i32 = 30;
/// ID of the serverbound `move_player_pos_rot` packet.
pub const MOVE_PLAYER_POS_ROT_PACKET_ID: i32 = 31;
/// ID of the serverbound `move_player_rot` packet.
pub const MOVE_PLAYER_ROT_PACKET_ID: i32 = 32;
/// ID of the serverbound `player_command` packet.
pub const PLAYER_COMMAND_PACKET_ID: i32 = 42;
/// ID of the serverbound `player_input` packet.
pub const PLAYER_INPUT_PACKET_ID: i32 = 43;
/// ID of the serverbound `player_loaded` packet.
pub const PLAYER_LOADED_PACKET_ID: i32 = 44;

/// Minimum and maximum view distance accepted by vanilla clients.
pub const MIN_VIEW_DISTANCE: i32 = 2;
pub const MAX_VIEW_DISTANCE: i32 = 32;

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Fields of the clientbound Play `login` packet.
#[derive(Debug, Clone)]
pub struct LoginPlay {
    pub entity_id: i32,
    pub hardcore: bool,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub enable_respawn_screen: bool,
    pub dimension_type: i32,
    pub dimension_name: String,
    pub hashed_seed: i64,
    pub game_mode: u8,
    pub previous_game_mode: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub portal_cooldown: i32,
    pub sea_level: i32,
    pub online_mode: bool,
    pub enforces_secure_chat: bool,
}

/// One entry of the clientbound `player_info_update` packet.
#[derive(Debug, Clone)]
pub struct PlayerInfoUpdate {
    pub uuid: [u8; 16],
    pub name: String,
    pub game_mode: i32,
    pub listed: bool,
    pub ping: i32,
}

/// Fields of the clientbound `player_abilities` packet.
#[derive(Debug, Clone)]
pub struct PlayerAbilities {
    pub flags: i8,
    pub fly_speed: f32,
    pub fov_modifier: f32,
}

/// Fields of the clientbound `server_data` packet.
#[derive(Debug, Clone)]
pub struct ServerData {
    pub motd: String,
    pub icon: Option<Vec<u8>>,
}

/// Fields of the serverbound `chat` packet (26.2 layout).
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub message: String,
    pub timestamp: i64,
    pub salt: i64,
    pub signature: Option<Vec<u8>>,
    pub message_count: i32,
    pub acknowledged: [u8; 3],
    pub checksum: i8,
}

/// Fields of the serverbound `move_player_pos` packet.
#[derive(Debug, Clone, PartialEq)]
pub struct MovePlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flags: i32,
}

/// Fields of the serverbound `move_player_pos_rot` packet.
#[derive(Debug, Clone, PartialEq)]
pub struct MovePlayerPosRot {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: i32,
}

/// Fields of the serverbound `move_player_rot` packet.
#[derive(Debug, Clone, PartialEq)]
pub struct MovePlayerRot {
    pub yaw: f32,
    pub pitch: f32,
    pub flags: i32,
}

// ---------------------------------------------------------------------------
// Encoders
// ---------------------------------------------------------------------------

/// Encodes the payload of the clientbound Play `login` packet (ID 49).
///
/// Field order matches 26.2 `ClientboundLoginPacket` + `CommonPlayerSpawnInfo`:
/// playerId (Int, **must be ≠ 0**), hardcore, levels, maxPlayers, chunkRadius,
/// simulationDistance, reducedDebugInfo, showDeathScreen, doLimitedCrafting,
/// spawn info (dimensionType holder VarInt, dimension key, seed, gameType,
/// previousGameType, isDebug, isFlat, optional death location, portalCooldown,
/// seaLevel), onlineMode, enforcesSecureChat.
pub fn encode_login_payload(login: &LoginPlay) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(128);
    // Client treats id 0 as "not yet assigned" and crashes in addEntity.
    debug_assert!(login.entity_id != 0, "player entity id must be non-zero");
    writer.push_i32(login.entity_id);
    writer.push_bool(login.hardcore);
    writer.push_var_i32(1); // one dimension
    writer.push_string("minecraft:overworld", 32767)?;
    writer.push_var_i32(login.max_players);
    writer.push_var_i32(login.view_distance);
    writer.push_var_i32(login.simulation_distance);
    writer.push_bool(login.reduced_debug_info);
    writer.push_bool(login.enable_respawn_screen);
    writer.push_bool(false); // do limited crafting
    writer.push_var_i32(login.dimension_type);
    writer.push_string(&login.dimension_name, 32767)?;
    writer.push_i64(login.hashed_seed);
    writer.push_u8(login.game_mode);
    writer.push_byte(login.previous_game_mode);
    writer.push_bool(login.is_debug);
    writer.push_bool(login.is_flat);
    writer.push_bool(false); // has death location
    writer.push_var_i32(login.portal_cooldown);
    writer.push_var_i32(login.sea_level);
    // 26.2 field order: online mode BEFORE enforces secure chat.
    writer.push_bool(login.online_mode);
    writer.push_bool(login.enforces_secure_chat);
    Ok(writer.into_bytes())
}

/// Encodes the payload of the clientbound `player_abilities` packet (ID 64).
///
/// Second float is walking speed on the wire (vanilla still uses ~0.1 for
/// creative FOV/walk); kept as [`PlayerAbilities::fov_modifier`] for call-site
/// compatibility with older docs that called it FOV modifier.
pub fn encode_player_abilities_payload(abilities: &PlayerAbilities) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(9);
    writer.push_byte(abilities.flags);
    writer.push_f32(abilities.fly_speed);
    writer.push_f32(abilities.fov_modifier);
    writer.into_bytes()
}

/// Encodes a Play `custom_payload` (plugin message) packet payload.
///
/// Layout: `channel: Identifier` + channel-specific bytes (no outer length).
pub fn encode_custom_payload(channel: &str, data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(channel.len() + data.len() + 8);
    writer.push_string(channel, 32767)?;
    writer.push_bytes(data);
    Ok(writer.into_bytes())
}

/// Encodes the standard `minecraft:brand` plugin message data + full payload.
///
/// F3 "Server brand" line reads this string (e.g. `"Hyperion"`). Without it the
/// client shows `null` / vanilla default.
pub fn encode_brand_payload(brand: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut data = ByteWriter::with_capacity(brand.len() + 5);
    data.push_string(brand, 32767)?;
    encode_custom_payload("minecraft:brand", &data.into_bytes())
}

/// Encodes the clientbound `set_held_item` payload (hotbar slot 0..=8).
pub fn encode_set_held_item_payload(slot: i32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(5);
    writer.push_var_i32(slot);
    writer.into_bytes()
}

/// Encodes the clientbound `unload_chunk` payload (protocol 776 / 26.2).
///
/// Wire order is **Z then X** (matches the 26.1/26.2 packet dump).
pub fn encode_unload_chunk_payload(chunk_x: i32, chunk_z: i32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(8);
    writer.push_i32(chunk_z);
    writer.push_i32(chunk_x);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `player_info_update` packet (ID 70).
///
/// Every entry carries the Add Player, Update Game Mode, Update Listed and
/// Update Latency actions (mask 0x1D), with no game profile properties.
pub fn encode_player_info_update_payload(
    players: &[PlayerInfoUpdate],
) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(64 + players.len() * 48);
    const ACTIONS: u8 = 0x01 | 0x04 | 0x08 | 0x10;
    writer.push_u8(ACTIONS);
    writer.push_var_i32(
        i32::try_from(players.len()).map_err(|_| ProtocolError::InvalidPacketPayload)?,
    );
    for player in players {
        writer.push_bytes(&player.uuid);
        writer.push_string(&player.name, 16)?;
        writer.push_var_i32(0); // game profile properties
        writer.push_var_i32(player.game_mode);
        writer.push_bool(player.listed);
        writer.push_var_i32(player.ping);
    }
    Ok(writer.into_bytes())
}

/// Encodes the payload of the clientbound `keep_alive` packet (ID 44).
pub fn encode_keep_alive_payload(id: i64) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(8);
    writer.push_i64(id);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `ping` packet (ID 61).
///
/// The payload echoes the value of the serverbound `ping_request` packet,
/// exactly like `keep_alive`.
pub fn encode_ping_payload(id: i64) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(8);
    writer.push_i64(id);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `system_chat` packet (ID 121).
///
/// `content` is a plain-text chat component: a network NBT string tag.
pub fn encode_system_chat_message_payload(
    content: &str,
    overlay: bool,
) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(content.len() + 5);
    writer.push_bytes(&encode_string_tag(content)?);
    writer.push_bool(overlay);
    Ok(writer.into_bytes())
}

/// Encodes the payload of the clientbound `player_position` packet (ID 72).
pub fn encode_player_position_payload(
    teleport_id: i32,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    flags: i32,
) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(61);
    writer.push_var_i32(teleport_id);
    writer.push_f64(x);
    writer.push_f64(y);
    writer.push_f64(z);
    writer.push_f64(0.0); // velocity x
    writer.push_f64(0.0); // velocity y
    writer.push_f64(0.0); // velocity z
    writer.push_f32(yaw);
    writer.push_f32(pitch);
    writer.push_i32(flags);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `set_chunk_cache_center` packet (ID 94).
///
/// Chunk coordinates are **VarInts** (not fixed i32). Encoding them as i32
/// makes the client report "found 6 bytes extra" for center (0, 0).
pub fn encode_set_chunk_cache_center_payload(chunk_x: i32, chunk_z: i32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(10);
    writer.push_var_i32(chunk_x);
    writer.push_var_i32(chunk_z);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `set_chunk_cache_radius` packet (ID 95).
pub fn encode_set_chunk_cache_radius_payload(view_distance: i32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(5);
    writer.push_var_i32(view_distance);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `set_simulation_distance` packet (ID 111).
pub fn encode_set_simulation_distance_payload(simulation_distance: i32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(5);
    writer.push_var_i32(simulation_distance);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `set_default_spawn_position` packet (ID 97).
pub fn encode_set_default_spawn_position_payload(
    dimension: i32,
    x: i32,
    y: i32,
    z: i32,
    yaw: f32,
    pitch: f32,
) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(24);
    writer.push_var_i32(dimension);
    writer.push_position(x, y, z);
    writer.push_f32(yaw);
    writer.push_f32(pitch);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `set_ticking_state` packet (ID 127).
pub fn encode_set_ticking_state_payload(tick_rate: f32, frozen: bool) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(5);
    writer.push_f32(tick_rate);
    writer.push_bool(frozen);
    writer.into_bytes()
}

/// A single clock entry of the clientbound `set_time` packet (ID 113).
#[derive(Debug, Clone, Copy)]
pub struct TimeClock {
    pub clock_id: i32,
    pub time: i64,
    pub fractional_time: f32,
    pub rate: f32,
}

/// Encodes the payload of the clientbound `set_time` packet (ID 113).
pub fn encode_update_time_payload(world_age: i64, clocks: &[TimeClock]) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(16 + clocks.len() * 20);
    writer.push_i64(world_age);
    writer.push_var_i32(clocks.len() as i32);
    for clock in clocks {
        writer.push_var_i32(clock.clock_id);
        writer.push_var_i64(clock.time);
        writer.push_f32(clock.fractional_time);
        writer.push_f32(clock.rate);
    }
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `game_event` packet (ID 38).
pub fn encode_game_event_payload(event: u8, value: f32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(5);
    writer.push_u8(event);
    writer.push_f32(value);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound `server_data` packet (ID 86).
pub fn encode_server_data_payload(server_data: &ServerData) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(64);
    writer.push_bytes(&encode_string_tag(&server_data.motd)?);
    writer.push_bool(server_data.icon.is_some());
    if let Some(icon) = &server_data.icon {
        writer.push_byte_array(icon, 32767)?;
    }
    Ok(writer.into_bytes())
}

/// Encodes the payload of the clientbound `chunk_batch_start` packet (ID 12).
pub fn encode_chunk_batch_start_payload() -> Vec<u8> {
    Vec::new()
}

/// Encodes the payload of the clientbound `chunk_batch_finished` packet (ID 11).
pub fn encode_chunk_batch_finished_payload(batch_size: i32) -> Vec<u8> {
    let mut writer = ByteWriter::with_capacity(5);
    writer.push_var_i32(batch_size);
    writer.into_bytes()
}

/// Encodes the payload of the clientbound Play `disconnect` packet (ID 32).
pub fn encode_disconnect_payload(reason: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = ByteWriter::with_capacity(reason.len() + 5);
    writer.push_bytes(&encode_string_tag(reason)?);
    Ok(writer.into_bytes())
}

/// Number of longs in a 256-column heightmap at 9 bits/entry
/// (ceil(log2(world_height + 1)) for height 384 → 9 bits; 256 cols → 36 longs).
pub const HEIGHTMAP_LONG_COUNT: i32 = 36;
/// Bits per heightmap entry for a 384-block-tall world.
pub const HEIGHTMAP_BITS: u32 = 9;
/// `Heightmap.Types.WORLD_SURFACE` network id.
pub const HEIGHTMAP_WORLD_SURFACE: i32 = 1;
/// `Heightmap.Types.MOTION_BLOCKING` network id.
pub const HEIGHTMAP_MOTION_BLOCKING: i32 = 4;
/// Length of one light section (16³ nibbles = 2048 bytes).
const LIGHT_ARRAY_LENGTH: usize = 2048;
/// Full-bright sky/block light section — static so hot encode paths do not
/// allocate 2 KiB per light section (vanilla/Paper reuse buffers too).
static FULL_BRIGHT_LIGHT: [u8; LIGHT_ARRAY_LENGTH] = [0xff; LIGHT_ARRAY_LENGTH];
/// Light engine sections for a 24-section world: chunk sections + 2 borders.
/// Overworld min section Y=-4 → light sections -5..=20 (26 total).
fn light_section_count(block_section_count: i32) -> i32 {
    block_section_count + 2
}

/// One section in the network `level_chunk_with_light` section buffer.
///
/// Phase 2.1: single-valued block + biome palettes only (enough for flat /
/// void columns). Multi-valued palettes land with real worldgen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkChunkSection {
    /// Non-air block count (0..=4096). Client uses this for culling.
    pub non_air_count: i16,
    /// Fluid count (0 for solid/air-only sections).
    pub fluid_count: i16,
    /// Global block-state palette id filling the whole section (0 = air).
    pub block_state_id: i32,
    /// Global biome id filling the whole section.
    pub biome_id: i32,
}

impl NetworkChunkSection {
    /// All-air section with the given biome.
    pub const fn air(biome_id: i32) -> Self {
        Self {
            non_air_count: 0,
            fluid_count: 0,
            block_state_id: 0,
            biome_id,
        }
    }

    /// Solid single-block section (non-air count = 4096).
    pub const fn solid(block_state_id: i32, biome_id: i32) -> Self {
        Self {
            non_air_count: 4096,
            fluid_count: 0,
            block_state_id,
            biome_id,
        }
    }
}

/// One heightmap entry for the network chunk packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkHeightmap {
    /// `Heightmap.Types` network enum id.
    pub type_id: i32,
    /// Packed long array (`HEIGHTMAP_LONG_COUNT` entries for overworld).
    pub data: Vec<i64>,
}

/// Encodes a single-valued paletted container (BPE = 0):
/// `bits: u8 = 0` + `value: VarInt` + **no** data array (ZeroBitStorage).
///
/// As of 1.21.5 the data-array length is **not** written; size is implied
/// by bits-per-entry (0 → empty). Single-value palettes also have **no**
/// length-prefixed palette list — just the one global palette id.
fn write_single_valued_palette(out: &mut Vec<u8>, global_id: i32) {
    out.push(0); // bits per entry
    out.extend_from_slice(&encode_var_i32(global_id));
    // ZeroBitStorage: writeFixedSizeLongArray of length 0 → nothing.
}

/// Encodes one section for protocol 776 / 26.2:
/// `blockCount: short`, `fluidCount: short`, block palette, biome palette.
fn write_section(out: &mut Vec<u8>, section: &NetworkChunkSection) {
    out.extend_from_slice(&section.non_air_count.to_be_bytes());
    out.extend_from_slice(&section.fluid_count.to_be_bytes());
    write_single_valued_palette(out, section.block_state_id);
    write_single_valued_palette(out, section.biome_id);
}

/// Writes a BitSet as `VarInt(longCount) + longCount × i64` (vanilla format).
fn write_bit_set(writer: &mut ByteWriter, bits: u64, long_count: i32) {
    writer.push_var_i32(long_count);
    if long_count > 0 {
        writer.push_i64(bits as i64);
        for _ in 1..long_count {
            writer.push_i64(0);
        }
    }
}

/// Packs 256 height values (one per column, X then Z) into the network
/// heightmap long array.
///
/// Heightmaps use **compact** bit packing that **crosses long boundaries**
/// (`256 × 9 / 64 = 36` longs exactly). This differs from post-1.16 block
/// palettes, which do not pack across longs.
///
/// Values are absolute world Y for the highest matching block (vanilla
/// `Heightmap` stores the Y of the first empty block above the surface for
/// some types; callers choose the semantics).
pub fn pack_heightmap_values(heights: &[u16; 256]) -> Vec<i64> {
    let bits = HEIGHTMAP_BITS as usize;
    let long_count = HEIGHTMAP_LONG_COUNT as usize;
    let mut longs = vec![0i64; long_count];
    let mask = (1u64 << bits) - 1;
    for (index, &height) in heights.iter().enumerate() {
        let bit_index = index * bits;
        let long_index = bit_index / 64;
        let offset = bit_index % 64;
        let value = u64::from(height) & mask;
        longs[long_index] |= (value << offset) as i64;
        // Spill into the next long when the 9-bit field crosses a boundary.
        let bits_in_first = 64 - offset;
        if bits_in_first < bits {
            longs[long_index + 1] |= (value >> bits_in_first) as i64;
        }
    }
    longs
}

/// Encodes the payload of the clientbound `level_chunk_with_light` packet
/// (ID 45) for an arbitrary column described by single-value sections.
///
/// Format verified against 26.2 (`LevelChunkSection` / `PalettedContainer` /
/// `Heightmap.Types` bytecode + wiki Chunk format ≥ 1.21.5):
/// - Heightmaps use **enum ids**, not resource-location strings.
/// - Sections include **fluid count**.
/// - Single-value palettes are `bits=0 + VarInt value` (no palette length,
///   no data-array length).
/// - Light section count = block sections + 2.
pub fn encode_chunk_payload(
    chunk_x: i32,
    chunk_z: i32,
    heightmaps: &[NetworkHeightmap],
    sections: &[NetworkChunkSection],
    has_sky_light: bool,
) -> Result<Vec<u8>, ProtocolError> {
    let section_count = sections.len() as i32;
    let mut writer = ByteWriter::with_capacity(64 * 1024);

    writer.push_i32(chunk_x);
    writer.push_i32(chunk_z);

    writer.push_var_i32(heightmaps.len() as i32);
    for heightmap in heightmaps {
        writer.push_var_i32(heightmap.type_id);
        writer.push_var_i32(heightmap.data.len() as i32);
        for &value in &heightmap.data {
            writer.push_i64(value);
        }
    }

    // Section buffer (NOT length-prefixed inside; outer size is a VarInt).
    let mut data = Vec::with_capacity(sections.len() * 12);
    for section in sections {
        write_section(&mut data, section);
    }
    writer.push_byte_array(&data, usize::MAX)?;

    writer.push_var_i32(0); // block entities

    // Light data (ClientboundLightUpdatePacketData).
    let light_sections = light_section_count(section_count);
    if has_sky_light {
        // All light sections present and non-empty.
        let mask = if light_sections >= 64 {
            u64::MAX
        } else {
            (1u64 << light_sections) - 1
        };
        write_bit_set(&mut writer, mask, 1); // skyYMask
        write_bit_set(&mut writer, 0, 0); // blockYMask
        write_bit_set(&mut writer, 0, 0); // emptySkyYMask
        write_bit_set(&mut writer, 0, 0); // emptyBlockYMask
        writer.push_var_i32(light_sections); // sky light arrays
        for _ in 0..light_sections {
            writer.push_var_i32(LIGHT_ARRAY_LENGTH as i32);
            writer.push_bytes(&FULL_BRIGHT_LIGHT);
        }
        writer.push_var_i32(0); // block light arrays
    } else {
        write_bit_set(&mut writer, 0, 0);
        write_bit_set(&mut writer, 0, 0);
        write_bit_set(&mut writer, 0, 0);
        write_bit_set(&mut writer, 0, 0);
        writer.push_var_i32(0);
        writer.push_var_i32(0);
    }

    Ok(writer.into_bytes())
}

/// Encodes an all-air chunk with full-brightness sky light (void world).
///
/// Convenience wrapper around [`encode_chunk_payload`].
pub fn encode_empty_chunk_payload(
    chunk_x: i32,
    chunk_z: i32,
    section_count: i32,
    has_sky_light: bool,
    biome_id: i32,
) -> Result<Vec<u8>, ProtocolError> {
    let zero_heightmap = vec![0i64; HEIGHTMAP_LONG_COUNT as usize];
    let heightmaps = [
        NetworkHeightmap {
            type_id: HEIGHTMAP_WORLD_SURFACE,
            data: zero_heightmap.clone(),
        },
        NetworkHeightmap {
            type_id: HEIGHTMAP_MOTION_BLOCKING,
            data: zero_heightmap,
        },
    ];
    let sections: Vec<NetworkChunkSection> = (0..section_count)
        .map(|_| NetworkChunkSection::air(biome_id))
        .collect();
    encode_chunk_payload(chunk_x, chunk_z, &heightmaps, &sections, has_sky_light)
}

// ---------------------------------------------------------------------------
// Decoders
// ---------------------------------------------------------------------------

/// Decodes the payload of the serverbound `confirm_teleportation` packet
/// (ID 0), returning the teleport ID.
pub fn decode_confirm_teleportation(payload: &[u8]) -> Result<i32, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    let teleport_id = cursor.read_var_i32()?;
    cursor.finish()?;
    Ok(teleport_id)
}

/// Decodes the payload of the serverbound Play `keep_alive` packet (ID 28),
/// returning the keep-alive ID.
pub fn decode_keep_alive(payload: &[u8]) -> Result<i64, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    let id = cursor.read_i64()?;
    cursor.finish()?;
    Ok(id)
}

/// Decodes the payload of the serverbound Play `ping_request` packet (ID 38),
/// returning the payload to echo back in the clientbound `ping` packet.
pub fn decode_ping_request(payload: &[u8]) -> Result<i64, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    let id = cursor.read_i64()?;
    cursor.finish()?;
    Ok(id)
}

/// Decodes the payload of the serverbound `chat` packet (ID 9).
pub fn decode_chat_message(payload: &[u8]) -> Result<ChatMessage, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    let message = cursor.read_string(256)?;
    let timestamp = cursor.read_i64()?;
    let salt = cursor.read_i64()?;
    let signature = if cursor.read_bool()? {
        Some(cursor.read_fixed_bytes(256)?)
    } else {
        None
    };
    let message_count = cursor.read_var_i32()?;
    let acknowledged: [u8; 3] = cursor
        .read_fixed_bytes(3)?
        .try_into()
        .map_err(|_| ProtocolError::InvalidPacketPayload)?;
    let checksum = cursor.read_i8()?;
    cursor.finish()?;
    Ok(ChatMessage {
        message,
        timestamp,
        salt,
        signature,
        message_count,
        acknowledged,
        checksum,
    })
}

/// Decodes the payload of the serverbound `move_player_pos` packet (ID 30).
pub fn decode_move_player_pos(payload: &[u8]) -> Result<MovePlayerPos, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    Ok(MovePlayerPos {
        x: cursor.read_f64()?,
        y: cursor.read_f64()?,
        z: cursor.read_f64()?,
        flags: cursor.read_var_i32()?,
    })
}

/// Decodes the payload of the serverbound `move_player_pos_rot` packet (ID 31).
pub fn decode_move_player_pos_rot(payload: &[u8]) -> Result<MovePlayerPosRot, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    Ok(MovePlayerPosRot {
        x: cursor.read_f64()?,
        y: cursor.read_f64()?,
        z: cursor.read_f64()?,
        yaw: cursor.read_f32()?,
        pitch: cursor.read_f32()?,
        flags: cursor.read_var_i32()?,
    })
}

/// Decodes the payload of the serverbound `move_player_rot` packet (ID 32).
pub fn decode_move_player_rot(payload: &[u8]) -> Result<MovePlayerRot, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    Ok(MovePlayerRot {
        yaw: cursor.read_f32()?,
        pitch: cursor.read_f32()?,
        flags: cursor.read_var_i32()?,
    })
}

/// Decodes the payload of the serverbound `player_loaded` packet (ID 44).
pub fn decode_player_loaded(payload: &[u8]) -> Result<(), ProtocolError> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(ProtocolError::InvalidPacketPayload)
    }
}

/// Decodes the payload of the serverbound `chunk_batch_received` packet
/// (ID 11), returning the chunk batch size.
pub fn decode_chunk_batch_received(payload: &[u8]) -> Result<f32, ProtocolError> {
    let mut cursor = PacketCursor::new(payload);
    cursor.read_f32()
}

/// Decodes the payload of the serverbound `client_tick_end` packet (ID 13).
pub fn decode_client_tick_end(payload: &[u8]) -> Result<(), ProtocolError> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(ProtocolError::InvalidPacketPayload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{decode_frame, encode_frame};

    fn round_trip_frame(packet_id: i32, payload: &[u8]) -> crate::PacketFrame {
        let frame = encode_frame(packet_id, payload).expect("frame encodes");
        let (decoded, consumed) = decode_frame(&frame).expect("frame decodes");
        assert_eq!(consumed, frame.len());
        decoded
    }

    #[test]
    fn keep_alive_round_trip() {
        let payload = encode_keep_alive_payload(0x1122_3344_5566_7788);
        let frame = round_trip_frame(KEEP_ALIVE_PACKET_ID, &payload);
        assert_eq!(frame.packet_id, KEEP_ALIVE_PACKET_ID);
        assert_eq!(
            decode_keep_alive(&frame.payload).expect("decodes"),
            0x1122_3344_5566_7788
        );
    }

    #[test]
    fn ping_request_echo_round_trip() {
        let payload = encode_ping_payload(0x0102_0304_0506_0708);
        let frame = round_trip_frame(PING_PACKET_ID, &payload);
        assert_eq!(frame.packet_id, PING_PACKET_ID);
        assert_eq!(
            decode_ping_request(&frame.payload).expect("decodes"),
            0x0102_0304_0506_0708
        );
    }

    #[test]
    fn confirm_teleportation_round_trip() {
        let mut writer = ByteWriter::new();
        writer.push_var_i32(42);
        let frame = round_trip_frame(CONFIRM_TELEPORTATION_PACKET_ID, &writer.into_bytes());
        assert_eq!(
            decode_confirm_teleportation(&frame.payload).expect("decodes"),
            42
        );
    }

    #[test]
    fn decoders_reject_payloads_with_trailing_bytes() {
        // A strict decoder must refuse any payload that does not match the
        // packet's exact field layout, like vanilla does.
        let keep_alive = encode_keep_alive_payload(7)
            .into_iter()
            .chain([0xff])
            .collect::<Vec<_>>();
        assert!(matches!(
            decode_keep_alive(&keep_alive),
            Err(ProtocolError::InvalidPacketPayload)
        ));

        let ping = encode_ping_payload(7)
            .into_iter()
            .chain([0xff])
            .collect::<Vec<_>>();
        assert!(matches!(
            decode_ping_request(&ping),
            Err(ProtocolError::InvalidPacketPayload)
        ));

        let teleport = [0x2a, 0xff]; // varint 42 + one stray byte
        assert!(matches!(
            decode_confirm_teleportation(&teleport),
            Err(ProtocolError::InvalidPacketPayload)
        ));

        // Valid chat body (message "hi" + 4 i64-ish fields) + one stray byte.
        let mut writer = ByteWriter::new();
        writer.push_string("hi", 256).expect("string fits");
        writer.push_i64(1);
        writer.push_i64(2);
        writer.push_bool(false);
        writer.push_var_i32(0);
        writer.push_bytes(&[0, 0, 0, 0]); // acknowledged + checksum
        let mut chat = writer.into_bytes();
        chat.push(0x99);
        assert!(matches!(
            decode_chat_message(&chat),
            Err(ProtocolError::InvalidPacketPayload)
        ));
    }

    #[test]
    fn chat_round_trip_without_signature() {
        let mut writer = ByteWriter::new();
        writer.push_string("hello", 256).expect("string fits");
        writer.push_i64(1_700_000_000_000);
        writer.push_i64(7);
        writer.push_bool(false); // no signature
        writer.push_var_i32(0); // message count
        writer.push_bytes(&[0, 0, 0]); // acknowledged bitset
        writer.push_byte(0); // checksum
        let frame = round_trip_frame(CHAT_MESSAGE_PACKET_ID, &writer.into_bytes());
        let chat = decode_chat_message(&frame.payload).expect("decodes");
        assert_eq!(chat.message, "hello");
        assert_eq!(chat.timestamp, 1_700_000_000_000);
        assert_eq!(chat.salt, 7);
        assert!(chat.signature.is_none());
        assert_eq!(chat.message_count, 0);
        assert_eq!(chat.acknowledged, [0, 0, 0]);
        assert_eq!(chat.checksum, 0);
    }

    #[test]
    fn chat_round_trip_with_signature() {
        let mut writer = ByteWriter::new();
        writer.push_string("signed", 256).expect("string fits");
        writer.push_i64(1);
        writer.push_i64(2);
        writer.push_bool(true);
        writer.push_bytes(&[0xAA; 256]); // signature
        writer.push_var_i32(3);
        writer.push_bytes(&[0x10, 0, 0]);
        writer.push_byte(-1);
        let frame = round_trip_frame(CHAT_MESSAGE_PACKET_ID, &writer.into_bytes());
        let chat = decode_chat_message(&frame.payload).expect("decodes");
        assert_eq!(chat.message, "signed");
        assert_eq!(chat.signature, Some(vec![0xAA; 256]));
        assert_eq!(chat.message_count, 3);
        assert_eq!(chat.checksum, -1);
    }

    #[test]
    fn move_player_decoders() {
        let mut pos = ByteWriter::new();
        pos.push_f64(1.5);
        pos.push_f64(2.5);
        pos.push_f64(3.5);
        pos.push_var_i32(0);
        let frame = round_trip_frame(MOVE_PLAYER_POS_PACKET_ID, &pos.into_bytes());
        assert_eq!(
            decode_move_player_pos(&frame.payload).expect("decodes"),
            MovePlayerPos {
                x: 1.5,
                y: 2.5,
                z: 3.5,
                flags: 0
            }
        );

        let mut pos_rot = ByteWriter::new();
        pos_rot.push_f64(1.0);
        pos_rot.push_f64(2.0);
        pos_rot.push_f64(3.0);
        pos_rot.push_f32(45.0);
        pos_rot.push_f32(-30.0);
        pos_rot.push_var_i32(1);
        let frame = round_trip_frame(MOVE_PLAYER_POS_ROT_PACKET_ID, &pos_rot.into_bytes());
        assert_eq!(
            decode_move_player_pos_rot(&frame.payload).expect("decodes"),
            MovePlayerPosRot {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 45.0,
                pitch: -30.0,
                flags: 1
            }
        );

        let mut rot = ByteWriter::new();
        rot.push_f32(90.0);
        rot.push_f32(10.0);
        rot.push_var_i32(2);
        let frame = round_trip_frame(MOVE_PLAYER_ROT_PACKET_ID, &rot.into_bytes());
        assert_eq!(
            decode_move_player_rot(&frame.payload).expect("decodes"),
            MovePlayerRot {
                yaw: 90.0,
                pitch: 10.0,
                flags: 2
            }
        );
    }

    #[test]
    fn empty_packets_decode() {
        let frame = round_trip_frame(PLAYER_LOADED_PACKET_ID, &[]);
        assert!(decode_player_loaded(&frame.payload).is_ok());
        let frame = round_trip_frame(CLIENT_TICK_END_PACKET_ID, &[]);
        assert!(decode_client_tick_end(&frame.payload).is_ok());
    }

    #[test]
    fn player_position_payload_length() {
        let payload = encode_player_position_payload(1, 0.0, 64.0, 0.0, 0.0, 0.0, 0);
        // VarInt teleport id (1) + 6 doubles (48) + yaw + pitch (8) + flags (4).
        assert_eq!(payload.len(), 61);
    }

    #[test]
    fn set_chunk_cache_center_uses_varints() {
        // (0, 0) → two single-byte VarInts. Fixed i32 would be 8 bytes and
        // the vanilla client rejects it with "found 6 bytes extra".
        let payload = encode_set_chunk_cache_center_payload(0, 0);
        assert_eq!(payload, vec![0x00, 0x00]);

        let payload = encode_set_chunk_cache_center_payload(1, -1);
        assert_eq!(payload[0], 0x01);
        // -1 as VarInt is five 0xFF bytes ending with 0x0F… actually -1 is
        // 0x7F with zigzag? No, Minecraft VarInt is signed two's complement
        // in 7-bit groups: -1 = 0xFFFFFFFF as five bytes 0xff 0xff 0xff 0xff 0x0f.
        assert_eq!(&payload[1..], &[0xff, 0xff, 0xff, 0xff, 0x0f]);
    }

    #[test]
    fn login_payload_smoke() {
        let login = LoginPlay {
            entity_id: 1,
            hardcore: false,
            max_players: 20,
            view_distance: 8,
            simulation_distance: 8,
            reduced_debug_info: false,
            enable_respawn_screen: true,
            dimension_type: 0,
            dimension_name: "minecraft:overworld".to_string(),
            hashed_seed: 0,
            game_mode: 1,
            previous_game_mode: -1,
            is_debug: false,
            is_flat: false,
            portal_cooldown: 0,
            sea_level: 63,
            online_mode: true,
            enforces_secure_chat: false,
        };
        let payload = encode_login_payload(&login).expect("encodes");
        assert!(payload.len() > 50);
        // playerId is a big-endian i32 at the start.
        assert_eq!(&payload[..4], &[0, 0, 0, 1]);
        let frame = round_trip_frame(LOGIN_PACKET_ID, &payload);
        assert_eq!(frame.packet_id, LOGIN_PACKET_ID);
    }

    #[test]
    fn player_info_update_smoke() {
        let player = PlayerInfoUpdate {
            uuid: [1; 16],
            name: "Hyperion".to_string(),
            game_mode: 1,
            listed: true,
            ping: 0,
        };
        let payload = encode_player_info_update_payload(&[player]).expect("encodes");
        assert!(payload.len() > 30);
        let frame = round_trip_frame(PLAYER_INFO_UPDATE_PACKET_ID, &payload);
        assert_eq!(frame.packet_id, PLAYER_INFO_UPDATE_PACKET_ID);
    }

    #[test]
    fn empty_chunk_payload_shape() {
        let payload = encode_empty_chunk_payload(1, -2, 24, true, 0).expect("encodes");
        // Chunk coordinates are the first eight bytes, big-endian.
        assert_eq!(&payload[..4], &[0, 0, 0, 1]);
        assert_eq!(&payload[4..8], &[0xFF, 0xFF, 0xFF, 0xFE]);
        // 26 light sections × 2048 bytes ≈ 53 KiB of light alone.
        assert!(
            payload.len() > 50_000 && payload.len() < 80_000,
            "chunk payload is {} bytes",
            payload.len()
        );
        let dark = encode_empty_chunk_payload(0, 0, 24, false, 0).expect("encodes");
        assert!(dark.len() < 2_000, "dark chunk is {} bytes", dark.len());
    }

    #[test]
    fn empty_section_is_two_shorts_and_two_single_value_palettes() {
        let mut section = Vec::new();
        write_section(&mut section, &NetworkChunkSection::air(0));
        // blockCount=0, fluidCount=0, blocks: 0x00 + varint 0, biomes: 0x00 + varint 0
        assert_eq!(section, vec![0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn brand_payload_starts_with_channel_and_brand() {
        let payload = encode_brand_payload("Hyperion").expect("brand");
        // Channel "minecraft:brand" as a protocol string, then brand string.
        assert!(payload.len() > 20);
        // Contains both ASCII strings.
        let as_str = String::from_utf8_lossy(&payload);
        assert!(as_str.contains("minecraft:brand"));
        assert!(as_str.contains("Hyperion"));
    }

    #[test]
    fn pack_heightmap_constant_surface() {
        let heights = [64u16; 256];
        let packed = pack_heightmap_values(&heights);
        assert_eq!(packed.len(), HEIGHTMAP_LONG_COUNT as usize);
        // Compact packing: first entry is the low 9 bits of long 0.
        let bits = HEIGHTMAP_BITS;
        let mask = (1u64 << bits) - 1;
        assert_eq!((packed[0] as u64) & mask, 64);
        // Entry 1 starts at bit 9.
        assert_eq!((packed[0] as u64 >> 9) & mask, 64);
        // 256 × 9 = 2304 bits = exactly 36 longs — last long fully used.
        assert_ne!(packed[35], 0);
    }

    #[test]
    fn solid_chunk_payload_larger_than_empty_dark() {
        let sections: Vec<_> = (0..24)
            .map(|i| {
                if i < 8 {
                    NetworkChunkSection::solid(1, 0)
                } else {
                    NetworkChunkSection::air(0)
                }
            })
            .collect();
        let heights = [64u16; 256];
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
        let payload = encode_chunk_payload(0, 0, &heightmaps, &sections, true).expect("encodes");
        assert!(payload.len() > 50_000);
        assert_eq!(&payload[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn server_data_and_system_chat_round_trip() {
        let server_data = ServerData {
            motd: "A Hyperion server".to_string(),
            icon: None,
        };
        let payload = encode_server_data_payload(&server_data).expect("encodes");
        let frame = round_trip_frame(SERVER_DATA_PACKET_ID, &payload);
        assert_eq!(frame.packet_id, SERVER_DATA_PACKET_ID);

        let chat = encode_system_chat_message_payload("Hello, world!", false).expect("encodes");
        let frame = round_trip_frame(SYSTEM_CHAT_MESSAGE_PACKET_ID, &chat);
        assert_eq!(frame.packet_id, SYSTEM_CHAT_MESSAGE_PACKET_ID);
    }

    #[test]
    fn time_and_batch_payloads() {
        let clocks = [TimeClock {
            clock_id: 0,
            time: 100,
            fractional_time: 0.0,
            rate: 1.0,
        }];
        let payload = encode_update_time_payload(2000, &clocks);
        let frame = round_trip_frame(UPDATE_TIME_PACKET_ID, &payload);
        assert_eq!(frame.packet_id, UPDATE_TIME_PACKET_ID);

        let start = encode_chunk_batch_start_payload();
        let frame = round_trip_frame(CHUNK_BATCH_START_PACKET_ID, &start);
        assert_eq!(frame.packet_id, CHUNK_BATCH_START_PACKET_ID);

        let finished = encode_chunk_batch_finished_payload(1);
        let frame = round_trip_frame(CHUNK_BATCH_FINISHED_PACKET_ID, &finished);
        assert_eq!(frame.packet_id, CHUNK_BATCH_FINISHED_PACKET_ID);
    }
}
