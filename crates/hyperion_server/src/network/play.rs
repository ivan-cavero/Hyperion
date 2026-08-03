//! Play state: spawn sequence and the minimal in-game loop.
//!
//! After Configuration the server switches to Play and sends the spawn
//! sequence: Login (play), abilities, player info, world settings, the
//! single spawn chunk (void world) and a teleport. Then a keep-alive/chat
//! loop runs until the client disconnects.

use std::time::Duration;

use hyperion_protocol::{
    CHAT_MESSAGE_PACKET_ID, CHAT_SESSION_UPDATE_PACKET_ID, CHUNK_BATCH_FINISHED_PACKET_ID,
    CHUNK_BATCH_RECEIVED_PACKET_ID, CHUNK_BATCH_START_PACKET_ID, CLIENT_INFORMATION_PACKET_ID,
    CLIENT_TICK_END_PACKET_ID, CONFIRM_TELEPORTATION_PACKET_ID, GAME_EVENT_PACKET_ID, GameProfile,
    KEEP_ALIVE_PACKET_ID, LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, LOGIN_PACKET_ID, LoginPlay,
    MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE, MOVE_PLAYER_POS_PACKET_ID, MOVE_PLAYER_POS_ROT_PACKET_ID,
    MOVE_PLAYER_ROT_PACKET_ID, PLAYER_ABILITIES_PACKET_ID, PLAYER_COMMAND_PACKET_ID,
    PLAYER_INFO_UPDATE_PACKET_ID, PLAYER_INPUT_PACKET_ID, PLAYER_LOADED_PACKET_ID,
    PLAYER_POSITION_PACKET_ID, PlayerAbilities, PlayerInfoUpdate, SERVER_DATA_PACKET_ID,
    SERVERBOUND_KEEP_ALIVE_PACKET_ID, SET_CHUNK_CACHE_CENTER_PACKET_ID,
    SET_CHUNK_CACHE_RADIUS_PACKET_ID, SET_DEFAULT_SPAWN_POSITION_PACKET_ID,
    SET_SIMULATION_DISTANCE_PACKET_ID, SET_TICKING_STATE_PACKET_ID, SYSTEM_CHAT_MESSAGE_PACKET_ID,
    ServerData, TimeClock, UPDATE_TIME_PACKET_ID, decode_chat_message, decode_chunk_batch_received,
    decode_client_information, decode_client_tick_end, decode_confirm_teleportation,
    decode_keep_alive, decode_player_loaded, encode_chunk_batch_finished_payload,
    encode_chunk_batch_start_payload, encode_empty_chunk_payload, encode_game_event_payload,
    encode_keep_alive_payload, encode_login_payload, encode_player_abilities_payload,
    encode_player_info_update_payload, encode_player_position_payload, encode_server_data_payload,
    encode_set_chunk_cache_center_payload, encode_set_chunk_cache_radius_payload,
    encode_set_default_spawn_position_payload, encode_set_simulation_distance_payload,
    encode_set_ticking_state_payload, encode_system_chat_message_payload,
    encode_update_time_payload,
};
use tokio::time::{Instant, interval_at};
use tracing::{info, trace, warn};

use super::configuration::{OVERWORLD_DIMENSION_TYPE_ID, PLAINS_BIOME_ID, SECTION_COUNT};
use super::connection::{Connection, ConnectionError};
use crate::config::ServerConfig;

/// Keep-alive interval (vanilla uses 15 seconds).
const KEEP_ALIVE_INTERVAL_SECONDS: u64 = 10;
/// Game event "Start waiting for level chunks" (see Game Event, Play ID 38).
const GAME_EVENT_START_WAITING_FOR_LEVEL_CHUNKS: u8 = 13;
/// Creative game mode: flying in the void without fall damage.
const GAME_MODE_CREATIVE: u8 = 1;

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
    let spawn_y = config.spawn_y as f64 + 0.5;

    // 1. Login (play): the client leaves the loading screen once it arrives.
    // Entity id 0 is reserved ("not assigned yet") on the client and throws
    // IllegalStateException: Tried to access entity ID before ID assignment.
    let login = LoginPlay {
        entity_id: 1,
        hardcore: false,
        max_players: config.max_players,
        view_distance,
        simulation_distance: view_distance,
        reduced_debug_info: false,
        enable_respawn_screen: true,
        dimension_type: OVERWORLD_DIMENSION_TYPE_ID,
        dimension_name: "minecraft:overworld".to_owned(),
        hashed_seed: 0,
        game_mode: GAME_MODE_CREATIVE,
        previous_game_mode: -1,
        is_debug: false,
        is_flat: false,
        portal_cooldown: 0,
        sea_level: 63,
        online_mode: config.online_mode,
        enforces_secure_chat: false,
    };
    connection
        .write_frame(LOGIN_PACKET_ID, &encode_login_payload(&login)?)
        .await?;

    // 2. Player abilities: flying enabled.
    let abilities = PlayerAbilities {
        flags: 0x02 | 0x04, // flying + allow flying
        fly_speed: 0.05,
        fov_modifier: 0.1,
    };
    connection
        .write_frame(
            PLAYER_ABILITIES_PACKET_ID,
            &encode_player_abilities_payload(&abilities),
        )
        .await?;

    // 3. Player info update: the player itself.
    let info = [PlayerInfoUpdate {
        uuid: *profile.uuid.as_bytes(),
        name: username.clone(),
        game_mode: GAME_MODE_CREATIVE as i32,
        listed: true,
        ping: 0,
    }];
    connection
        .write_frame(
            PLAYER_INFO_UPDATE_PACKET_ID,
            &encode_player_info_update_payload(&info)?,
        )
        .await?;

    // 4. Server data (tab-list MOTD).
    let server_data = ServerData {
        motd: "Hyperion".to_owned(),
        icon: None,
    };
    connection
        .write_frame(
            SERVER_DATA_PACKET_ID,
            &encode_server_data_payload(&server_data)?,
        )
        .await?;

    // 5. World settings: chunk cache center, radius and simulation distance.
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
            &encode_set_simulation_distance_payload(view_distance),
        )
        .await?;

    // 6. Default spawn position.
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

    // 7. "Start waiting for level chunks" (vanilla sends this before chunks).
    connection
        .write_frame(
            GAME_EVENT_PACKET_ID,
            &encode_game_event_payload(GAME_EVENT_START_WAITING_FOR_LEVEL_CHUNKS, 0.0),
        )
        .await?;

    // 8. Time and ticking state.
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

    // 9. The single spawn chunk (void world) inside a chunk batch.
    connection
        .write_frame(
            CHUNK_BATCH_START_PACKET_ID,
            &encode_chunk_batch_start_payload(),
        )
        .await?;
    let chunk = encode_empty_chunk_payload(0, 0, SECTION_COUNT, true, PLAINS_BIOME_ID)?;
    connection
        .write_frame(LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, &chunk)
        .await?;
    connection
        .write_frame(
            CHUNK_BATCH_FINISHED_PACKET_ID,
            &encode_chunk_batch_finished_payload(1),
        )
        .await?;

    // 10. Teleport the player to the spawn point.
    connection
        .write_frame(
            PLAYER_POSITION_PACKET_ID,
            &encode_player_position_payload(0, 0.5, spawn_y, 0.5, 0.0, 0.0, 0),
        )
        .await?;

    info!(%username, "player spawned into the world");

    // Keep-alive + chat loop until the client disconnects.
    // First keep-alive after one interval, not immediately (interval() fires right away).
    let keep_alive_period = Duration::from_secs(KEEP_ALIVE_INTERVAL_SECONDS);
    let mut keep_alive = interval_at(Instant::now() + keep_alive_period, keep_alive_period);
    let mut next_keep_alive_id: i64 = 0;
    loop {
        tokio::select! {
            frame = connection.read_frame() => {
                let frame = frame?;
                match frame.packet_id {
                    SERVERBOUND_KEEP_ALIVE_PACKET_ID => {
                        let id = decode_keep_alive(&frame.payload)?;
                        trace!(%username, id, "keep-alive response");
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
                    other => warn!(%username, packet_id = other, "unhandled play packet"),
                }
            }
            _ = keep_alive.tick() => {
                next_keep_alive_id += 1;
                let id = next_keep_alive_id;
                connection
                    .write_frame(KEEP_ALIVE_PACKET_ID, &encode_keep_alive_payload(id))
                    .await?;
                trace!(%username, id, "keep-alive sent");
            }
        }
    }
}
