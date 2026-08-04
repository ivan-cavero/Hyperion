//! Play state: spawn sequence and the minimal in-game loop.
//!
//! After Configuration the server switches to Play and sends the spawn
//! sequence: Login (play), abilities, player info, world settings, the
//! spawn chunk (Anvil flat platform when `world_dir` is set) and a teleport.
//! Then a keep-alive/chat loop runs until the client disconnects.

use std::time::Duration;

use hyperion_protocol::{
    ABILITIES_CREATIVE, CHAT_MESSAGE_PACKET_ID, CHAT_SESSION_UPDATE_PACKET_ID,
    CHUNK_BATCH_FINISHED_PACKET_ID, CHUNK_BATCH_RECEIVED_PACKET_ID, CHUNK_BATCH_START_PACKET_ID,
    CLIENT_INFORMATION_PACKET_ID, CLIENT_TICK_END_PACKET_ID, CONFIRM_TELEPORTATION_PACKET_ID,
    CUSTOM_PAYLOAD_PLAY_PACKET_ID, DISCONNECT_PACKET_ID, GAME_EVENT_CHANGE_GAME_MODE,
    GAME_EVENT_PACKET_ID, GAME_EVENT_START_WAITING_FOR_LEVEL_CHUNKS, GameProfile,
    KEEP_ALIVE_PACKET_ID, LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, LOGIN_PACKET_ID, LoginPlay,
    MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE, MOVE_PLAYER_POS_PACKET_ID, MOVE_PLAYER_POS_ROT_PACKET_ID,
    MOVE_PLAYER_ROT_PACKET_ID, PING_PACKET_ID, PING_REQUEST_PACKET_ID, PLAYER_ABILITIES_PACKET_ID,
    PLAYER_COMMAND_PACKET_ID, PLAYER_INFO_UPDATE_PACKET_ID, PLAYER_INPUT_PACKET_ID,
    PLAYER_LOADED_PACKET_ID, PLAYER_POSITION_PACKET_ID, PlayerAbilities, PlayerInfoUpdate,
    SERVER_DATA_PACKET_ID, SERVERBOUND_KEEP_ALIVE_PACKET_ID, SET_CHUNK_CACHE_CENTER_PACKET_ID,
    SET_CHUNK_CACHE_RADIUS_PACKET_ID, SET_DEFAULT_SPAWN_POSITION_PACKET_ID,
    SET_HELD_ITEM_PACKET_ID, SET_SIMULATION_DISTANCE_PACKET_ID, SET_TICKING_STATE_PACKET_ID,
    SYSTEM_CHAT_MESSAGE_PACKET_ID, ServerData, TimeClock, UPDATE_TIME_PACKET_ID,
    decode_chat_message, decode_chunk_batch_received, decode_client_information,
    decode_client_tick_end, decode_confirm_teleportation, decode_keep_alive,
    decode_play_ping_request, decode_player_loaded, encode_brand_payload,
    encode_chunk_batch_finished_payload, encode_chunk_batch_start_payload,
    encode_disconnect_payload, encode_empty_chunk_payload, encode_game_event_payload,
    encode_keep_alive_payload, encode_login_payload, encode_ping_payload,
    encode_player_abilities_payload, encode_player_info_update_payload,
    encode_player_position_payload, encode_server_data_payload,
    encode_set_chunk_cache_center_payload, encode_set_chunk_cache_radius_payload,
    encode_set_default_spawn_position_payload, encode_set_held_item_payload,
    encode_set_simulation_distance_payload, encode_set_ticking_state_payload,
    encode_system_chat_message_payload, encode_update_time_payload,
};
use hyperion_world::{ChunkColumn, load_or_flat, snap_ground_y};
use tokio::time::{Instant, interval_at};
use tracing::{info, trace, warn};

use super::configuration::{OVERWORLD_DIMENSION_TYPE_ID, PLAINS_BIOME_ID, SECTION_COUNT};
use super::connection::{Connection, ConnectionError};
use crate::config::ServerConfig;

/// Creative game mode id (0 survival, 1 creative, 2 adventure, 3 spectator).
const GAME_MODE_CREATIVE: u8 = 1;
/// Brand string shown in F3 "Server brand" / debug.
const SERVER_BRAND: &str = "Hyperion";
/// Highest valid serverbound Play packet ID.
///
/// The 26.2 packets.json dump lists exactly 69 serverbound Play packets with
/// contiguous IDs 0..=68. Everything in that range is a legitimate vanilla
/// packet we may not implement yet; anything above it is a protocol desync
/// or a broken/malicious client and is worth a warning.
const MAX_SERVERBOUND_PLAY_PACKET_ID: i32 = 68;

/// Sends the spawn sequence and runs the keep-alive/chat loop.
pub(super) async fn serve_play(
    connection: &mut Connection,
    profile: GameProfile,
    config: &ServerConfig,
) -> Result<(), ConnectionError> {
    let username = profile.username.clone();
    info!(%username, uuid = %profile.uuid, "entering play state");

    let view_distance = config
        .view_distance
        .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
    let simulation_distance = config
        .simulation_distance
        .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
    let spawn_y = config.spawn_y as f64;

    // 1. Login (play): the client leaves the loading screen once it arrives.
    // Entity id 0 is reserved ("not assigned yet") on the client and throws
    // IllegalStateException: Tried to access entity ID before ID assignment.
    let login = LoginPlay {
        entity_id: 1,
        hardcore: false,
        max_players: config.max_players,
        view_distance,
        simulation_distance,
        reduced_debug_info: false,
        enable_respawn_screen: true,
        dimension_type: OVERWORLD_DIMENSION_TYPE_ID,
        dimension_name: "minecraft:overworld".to_owned(),
        hashed_seed: 0,
        game_mode: GAME_MODE_CREATIVE,
        previous_game_mode: -1,
        is_debug: false,
        is_flat: !config.world_dir.as_os_str().is_empty(),
        portal_cooldown: 0,
        sea_level: 63,
        online_mode: config.online_mode,
        enforces_secure_chat: false,
    };
    connection
        .write_frame(LOGIN_PACKET_ID, &encode_login_payload(&login)?)
        .await?;

    // 2. Brand (F3 "Server brand") — without this the client shows null.
    connection
        .write_frame(
            CUSTOM_PAYLOAD_PLAY_PACKET_ID,
            &encode_brand_payload(SERVER_BRAND)?,
        )
        .await?;

    // 3. Full creative abilities (invulnerable + fly + creative instant-break).
    // Missing 0x08 made the client open a survival inventory and mine slowly.
    let abilities = PlayerAbilities {
        flags: ABILITIES_CREATIVE,
        fly_speed: 0.05,
        fov_modifier: 0.1,
    };
    connection
        .write_frame(
            PLAYER_ABILITIES_PACKET_ID,
            &encode_player_abilities_payload(&abilities),
        )
        .await?;

    // 4. Explicit game-mode change (belt-and-suspenders with Login game_mode).
    connection
        .write_frame(
            GAME_EVENT_PACKET_ID,
            &encode_game_event_payload(GAME_EVENT_CHANGE_GAME_MODE, f32::from(GAME_MODE_CREATIVE)),
        )
        .await?;

    // 5. Player info update: the player itself (tab list + local game mode).
    let info = [PlayerInfoUpdate {
        uuid: *profile.uuid.as_bytes(),
        name: username.clone(),
        game_mode: i32::from(GAME_MODE_CREATIVE),
        listed: true,
        ping: 0,
    }];
    connection
        .write_frame(
            PLAYER_INFO_UPDATE_PACKET_ID,
            &encode_player_info_update_payload(&info)?,
        )
        .await?;

    // 6. Held item slot (hotbar 0).
    connection
        .write_frame(SET_HELD_ITEM_PACKET_ID, &encode_set_held_item_payload(0))
        .await?;

    // 7. Server data (tab-list MOTD) — same text as the multiplayer list.
    let server_data = ServerData {
        motd: config.motd.clone(),
        icon: None,
    };
    connection
        .write_frame(
            SERVER_DATA_PACKET_ID,
            &encode_server_data_payload(&server_data)?,
        )
        .await?;

    // 8. World settings: chunk cache center, radius and simulation distance.
    connection
        .write_frame(
            SET_CHUNK_CACHE_CENTER_PACKET_ID,
            &encode_set_chunk_cache_center_payload(0, 0),
        )
        .await?;
    connection
        .write_frame(
            SET_CHUNK_CACHE_RADIUS_PACKET_ID,
            &encode_set_chunk_cache_radius_payload(view_distance),
        )
        .await?;
    connection
        .write_frame(
            SET_SIMULATION_DISTANCE_PACKET_ID,
            &encode_set_simulation_distance_payload(simulation_distance),
        )
        .await?;

    // 9. Default spawn position.
    connection
        .write_frame(
            SET_DEFAULT_SPAWN_POSITION_PACKET_ID,
            &encode_set_default_spawn_position_payload(
                OVERWORLD_DIMENSION_TYPE_ID,
                0,
                config.spawn_y,
                0,
                0.0,
                0.0,
            ),
        )
        .await?;

    // 10. "Start waiting for level chunks" (vanilla sends this before chunks).
    connection
        .write_frame(
            GAME_EVENT_PACKET_ID,
            &encode_game_event_payload(GAME_EVENT_START_WAITING_FOR_LEVEL_CHUNKS, 0.0),
        )
        .await?;

    // 11. Time and ticking state.
    let clocks = [TimeClock {
        clock_id: 0,
        time: 6_000, // noon
        fractional_time: 0.0,
        rate: 1.0,
    }];
    connection
        .write_frame(
            UPDATE_TIME_PACKET_ID,
            &encode_update_time_payload(0, &clocks),
        )
        .await?;
    connection
        .write_frame(
            SET_TICKING_STATE_PACKET_ID,
            &encode_set_ticking_state_payload(20.0, false),
        )
        .await?;

    // 12. Chunk batch: flat platform for the whole view distance (not just 0,0).
    // A single chunk with vd=8 leaves a 16×16 island; missing neighbours make
    // solid faces cull against void and look "transparent".
    connection
        .write_frame(
            CHUNK_BATCH_START_PACKET_ID,
            &encode_chunk_batch_start_payload(),
        )
        .await?;
    let (chunk_payloads, feet_y) = spawn_chunk_payloads(config, view_distance)?;
    let batch_size = chunk_payloads.len() as i32;
    for payload in &chunk_payloads {
        connection
            .write_frame(LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, payload)
            .await?;
    }
    connection
        .write_frame(
            CHUNK_BATCH_FINISHED_PACKET_ID,
            &encode_chunk_batch_finished_payload(batch_size),
        )
        .await?;

    // 13. Teleport the player to the spawn point (feet on the platform).
    let teleport_y = if config.world_dir.as_os_str().is_empty() {
        spawn_y + 0.5
    } else {
        f64::from(feet_y)
    };
    connection
        .write_frame(
            PLAYER_POSITION_PACKET_ID,
            &encode_player_position_payload(0, 0.5, teleport_y, 0.5, 0.0, 0.0, 0),
        )
        .await?;

    info!(
        %username,
        feet_y = teleport_y,
        chunks = batch_size,
        brand = SERVER_BRAND,
        "player spawned into the world"
    );

    // Keep-alive + chat loop until the client disconnects.
    // First keep-alive after one interval, not immediately (interval() fires right away).
    let keep_alive_period = Duration::from_secs(config.keep_alive_interval_seconds);
    let keep_alive_timeout = Duration::from_secs(config.keep_alive_timeout_seconds);
    let mut keep_alive = interval_at(Instant::now() + keep_alive_period, keep_alive_period);
    let mut next_keep_alive_id: i64 = 0;
    let mut keep_alive_pending = false;
    let mut last_keep_alive_sent_at = Instant::now();
    loop {
        tokio::select! {
            frame = connection.read_frame() => {
                let frame = frame?;
                match frame.packet_id {
                    SERVERBOUND_KEEP_ALIVE_PACKET_ID => {
                        let id = decode_keep_alive(&frame.payload)?;
                        trace!(%username, id, "keep-alive response");
                        keep_alive_pending = false;
                    }
                    PING_REQUEST_PACKET_ID => {
                        // Vanilla answers the latency probe immediately.
                        let id = decode_play_ping_request(&frame.payload)?;
                        connection
                            .write_frame(PING_PACKET_ID, &encode_ping_payload(id))
                            .await?;
                        trace!(%username, id, "ping answered");
                    }
                    CHAT_MESSAGE_PACKET_ID => {
                        let chat = decode_chat_message(&frame.payload)?;
                        info!(%username, message = %chat.message, "chat message");
                        let echo = format!("{username}: {}", chat.message);
                        connection
                            .write_frame(
                                SYSTEM_CHAT_MESSAGE_PACKET_ID,
                                &encode_system_chat_message_payload(&echo, false)?,
                            )
                            .await?;
                    }
                    CLIENT_INFORMATION_PACKET_ID => {
                        decode_client_information(&frame)?;
                    }
                    CONFIRM_TELEPORTATION_PACKET_ID => {
                        decode_confirm_teleportation(&frame.payload)?;
                    }
                    CHUNK_BATCH_RECEIVED_PACKET_ID => {
                        decode_chunk_batch_received(&frame.payload)?;
                    }
                    PLAYER_LOADED_PACKET_ID => {
                        decode_player_loaded(&frame.payload)?;
                    }
                    CLIENT_TICK_END_PACKET_ID => {
                        decode_client_tick_end(&frame.payload)?;
                    }
                    MOVE_PLAYER_POS_PACKET_ID
                    | MOVE_PLAYER_POS_ROT_PACKET_ID
                    | MOVE_PLAYER_ROT_PACKET_ID
                    | PLAYER_COMMAND_PACKET_ID
                    | PLAYER_INPUT_PACKET_ID
                    | CHAT_SESSION_UPDATE_PACKET_ID => {
                        // Movement and chat sessions are ignored for now.
                    }
                    other @ 0..=MAX_SERVERBOUND_PLAY_PACKET_ID => {
                        // A legitimate vanilla packet we do not implement yet
                        // (inventory, abilities, entity actions, ...): the
                        // client sends these routinely, so keep quiet about
                        // them.
                        trace!(%username, packet_id = other, "ignoring unimplemented play packet");
                    }
                    other => warn!(%username, packet_id = other, "unhandled play packet out of range"),
                }
            }
            _ = keep_alive.tick() => {
                if keep_alive_pending && last_keep_alive_sent_at.elapsed() > keep_alive_timeout {
                    // Vanilla kicks clients that stay silent for two
                    // keep-alive intervals.
                    info!(%username, "keep-alive timed out, kicking");
                    connection
                        .write_frame(
                            DISCONNECT_PACKET_ID,
                            &encode_disconnect_payload("Timed out")?,
                        )
                        .await?;
                    return Ok(());
                }
                next_keep_alive_id += 1;
                let id = next_keep_alive_id;
                connection
                    .write_frame(KEEP_ALIVE_PACKET_ID, &encode_keep_alive_payload(id))
                    .await?;
                // The timeout clock starts with the first unanswered
                // keep-alive and is only reset by a response, so a client
                // that stops answering is kicked regardless of the probes
                // sent in between.
                if !keep_alive_pending {
                    last_keep_alive_sent_at = Instant::now();
                }
                keep_alive_pending = true;
                trace!(%username, id, "keep-alive sent");
            }
        }
    }
}

/// Builds `level_chunk_with_light` payloads for every chunk in a square of
/// half-side `view_distance` around (0, 0).
///
/// When `world_dir` is set, each column is loaded from Anvil or generated as a
/// flat stone platform (not always written back — only bootstrap ensures 0,0).
/// When empty (unit tests), a single void chunk is returned.
///
/// Returns `(payloads, feet_y)`.
fn spawn_chunk_payloads(
    config: &ServerConfig,
    view_distance: i32,
) -> Result<(Vec<Vec<u8>>, i32), ConnectionError> {
    if config.world_dir.as_os_str().is_empty() {
        let payload = encode_empty_chunk_payload(0, 0, SECTION_COUNT, true, PLAINS_BIOME_ID)?;
        return Ok((vec![payload], config.spawn_y));
    }

    let ground_y = snap_ground_y(config.spawn_y.saturating_sub(1));
    let radius = view_distance.max(1);
    // Cap the first-join batch so a huge view-distance does not stall the
    // accept loop (vd=8 → 17² = 289 chunks ≈ 15 MB of light-heavy payloads).
    let radius = radius.min(8);
    let mut payloads = Vec::with_capacity(((2 * radius + 1) as usize).pow(2));
    let mut feet_y = ground_y + 1;

    for chunk_z in -radius..=radius {
        for chunk_x in -radius..=radius {
            let column = match load_or_flat(&config.world_dir, chunk_x, chunk_z, ground_y) {
                Ok(col) => col,
                Err(error) => {
                    warn!(
                        world = %config.world_dir.display(),
                        chunk_x,
                        chunk_z,
                        %error,
                        "chunk load failed; using in-memory flat column"
                    );
                    ChunkColumn::flat(chunk_x, chunk_z, ground_y)
                }
            };
            if chunk_x == 0 && chunk_z == 0 {
                feet_y = column.surface_y;
            }
            let payload = column
                .encode_network_payload(true)
                .map_err(ConnectionError::from)?;
            payloads.push(payload);
        }
    }

    Ok((payloads, feet_y))
}
