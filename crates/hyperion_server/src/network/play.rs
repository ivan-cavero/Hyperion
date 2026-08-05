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
    SYSTEM_CHAT_MESSAGE_PACKET_ID, ServerData, TimeClock, UNLOAD_CHUNK_PACKET_ID,
    UPDATE_TIME_PACKET_ID, decode_chat_message, decode_chunk_batch_received,
    decode_client_information, decode_client_tick_end, decode_confirm_teleportation,
    decode_keep_alive, decode_move_player_pos, decode_move_player_pos_rot,
    decode_play_ping_request, decode_player_loaded, encode_brand_payload,
    encode_chunk_batch_finished_payload, encode_chunk_batch_start_payload,
    encode_disconnect_payload, encode_empty_chunk_payload, encode_game_event_payload,
    encode_keep_alive_payload, encode_login_payload, encode_ping_payload,
    encode_player_abilities_payload, encode_player_info_update_payload,
    encode_player_position_payload, encode_server_data_payload,
    encode_set_chunk_cache_center_payload, encode_set_chunk_cache_radius_payload,
    encode_set_default_spawn_position_payload, encode_set_held_item_payload,
    encode_set_simulation_distance_payload, encode_set_ticking_state_payload,
    encode_system_chat_message_payload, encode_unload_chunk_payload, encode_update_time_payload,
};
use tokio::time::{Instant, interval_at};
use tracing::{info, trace, warn};

use super::chunk_view::{ChunkView, ViewUpdate};
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
        hashed_seed: config.level_seed,
        game_mode: GAME_MODE_CREATIVE,
        previous_game_mode: -1,
        is_debug: false,
        // Own-core surface terrain (Phase 2.4), not a superflat preset.
        is_flat: false,
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

    // 9–13. World open: compute spawn height FIRST, then tell the client where
    // spawn is, then stream chunks, then teleport. Never leave the player at
    // (0,0,0) while terrain is still generating.
    let world_seed = config.level_seed as u64;
    let (mut chunk_view, feet_y, initial_count) =
        ChunkView::spawn(config.world_dir.clone(), view_distance, world_seed)
            .map_err(ConnectionError::from)?;
    // Clamp: never advertise void / bedrock spawn.
    let feet_y = feet_y.clamp(16, 300);
    let teleport_y = if chunk_view.streaming_enabled() {
        f64::from(feet_y)
    } else {
        spawn_y.max(64.0) + 0.5
    };

    connection
        .write_frame(
            SET_DEFAULT_SPAWN_POSITION_PACKET_ID,
            &encode_set_default_spawn_position_payload(
                OVERWORLD_DIMENSION_TYPE_ID,
                0,
                feet_y,
                0,
                0.0,
                0.0,
            ),
        )
        .await?;

    // "Start waiting for level chunks" (vanilla sends this before chunks).
    connection
        .write_frame(
            GAME_EVENT_PACKET_ID,
            &encode_game_event_payload(GAME_EVENT_START_WAITING_FOR_LEVEL_CHUNKS, 0.0),
        )
        .await?;

    // Time and ticking state.
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

    // Chunk batch — spawn chunk first so the client has ground under the
    // teleport, then the rest of the view. Scaffold gen is fast; density
    // Play uses terrain-only detail.
    connection
        .write_frame(
            CHUNK_BATCH_START_PACKET_ID,
            &encode_chunk_batch_start_payload(),
        )
        .await?;
    let batch_size = if chunk_view.streaming_enabled() {
        // 1) Origin column first
        if let Some(payload) = chunk_view.payload(0, 0) {
            connection
                .write_frame(LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, &payload)
                .await?;
        }
        // 2) Remaining columns (skip 0,0 already sent)
        let mut sent = 1i32;
        for (chunk_x, chunk_z) in chunk_view.initial_coords() {
            if chunk_x == 0 && chunk_z == 0 {
                continue;
            }
            let payload = chunk_view
                .payload(chunk_x, chunk_z)
                .expect("streaming view has network cache");
            connection
                .write_frame(LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, &payload)
                .await?;
            sent += 1;
            // Flush every few chunks so the client can start rendering instead
            // of waiting for the whole view distance.
            if sent % 9 == 0 {
                connection.flush().await?;
            }
        }
        sent
    } else {
        let payload = encode_empty_chunk_payload(0, 0, SECTION_COUNT, true, PLAINS_BIOME_ID)?;
        connection
            .write_frame(LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, &payload)
            .await?;
        1
    };
    let _ = initial_count;
    connection
        .write_frame(
            CHUNK_BATCH_FINISHED_PACKET_ID,
            &encode_chunk_batch_finished_payload(batch_size),
        )
        .await?;

    // Teleport onto solid ground (after spawn chunk exists).
    connection
        .write_frame(
            PLAYER_POSITION_PACKET_ID,
            &encode_player_position_payload(0, 0.5, teleport_y, 0.5, 0.0, 0.0, 0),
        )
        .await?;
    connection.flush().await?;

    // Lazy disk: a small budget so join stays fast; rest drains on later ticks.
    let persisted = chunk_view.flush_dirty_budget(16);
    if persisted > 0 {
        trace!(persisted, "lazy anvil flush after spawn");
    }

    info!(
        %username,
        feet_y = teleport_y,
        chunks = batch_size,
        streaming = chunk_view.streaming_enabled(),
        brand = SERVER_BRAND,
        "player spawned into the world"
    );

    // Keep-alive + chat + chunk streaming until the client disconnects.
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
                        connection.flush().await?;
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
                        connection.flush().await?;
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
                    MOVE_PLAYER_POS_PACKET_ID => {
                        let mov = decode_move_player_pos(&frame.payload)?;
                        let update = chunk_view.on_position(mov.x, mov.z)?;
                        apply_view_update(connection, &mut chunk_view, update).await?;
                    }
                    MOVE_PLAYER_POS_ROT_PACKET_ID => {
                        let mov = decode_move_player_pos_rot(&frame.payload)?;
                        let update = chunk_view.on_position(mov.x, mov.z)?;
                        apply_view_update(connection, &mut chunk_view, update).await?;
                    }
                    MOVE_PLAYER_ROT_PACKET_ID
                    | PLAYER_COMMAND_PACKET_ID
                    | PLAYER_INPUT_PACKET_ID
                    | CHAT_SESSION_UPDATE_PACKET_ID => {
                        // Look direction / inputs: no world side-effects yet.
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
                    connection.flush().await?;
                    return Ok(());
                }
                next_keep_alive_id += 1;
                let id = next_keep_alive_id;
                connection
                    .write_frame(KEEP_ALIVE_PACKET_ID, &encode_keep_alive_payload(id))
                    .await?;
                connection.flush().await?;
                // Opportunistic disk drain while idle enough to send keep-alives.
                let _ = chunk_view.flush_dirty_budget(32);
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

/// Applies a [`ViewUpdate`]: center packet, unload far columns, batch-send new ones.
async fn apply_view_update(
    connection: &mut Connection,
    view: &mut ChunkView,
    update: ViewUpdate,
) -> Result<(), ConnectionError> {
    if update.new_center.is_none() && update.to_send.is_empty() && update.to_unload.is_empty() {
        return Ok(());
    }

    if let Some((cx, cz)) = update.new_center {
        connection
            .write_frame(
                SET_CHUNK_CACHE_CENTER_PACKET_ID,
                &encode_set_chunk_cache_center_payload(cx, cz),
            )
            .await?;
        trace!(cx, cz, loaded = view.loaded_count(), "chunk cache center");
    }

    for (chunk_x, chunk_z) in update.to_unload {
        connection
            .write_frame(
                UNLOAD_CHUNK_PACKET_ID,
                &encode_unload_chunk_payload(chunk_x, chunk_z),
            )
            .await?;
    }

    if !update.to_send.is_empty() {
        let batch_size = update.to_send.len() as i32;
        connection
            .write_frame(
                CHUNK_BATCH_START_PACKET_ID,
                &encode_chunk_batch_start_payload(),
            )
            .await?;
        for (chunk_x, chunk_z) in &update.to_send {
            let payload = view
                .payload(*chunk_x, *chunk_z)
                .expect("streaming view has network cache");
            connection
                .write_frame(LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, &payload)
                .await?;
        }
        connection
            .write_frame(
                CHUNK_BATCH_FINISHED_PACKET_ID,
                &encode_chunk_batch_finished_payload(batch_size),
            )
            .await?;
        // One flush per movement strip — not per chunk.
        connection.flush().await?;
        // Drain a few dirty columns without blocking exploration.
        let _ = view.flush_dirty_budget(8);
        trace!(sent = batch_size, "streamed new chunks");
    }

    Ok(())
}
