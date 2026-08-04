//! Minecraft protocol primitives used by Hyperion.

mod compression;
mod configuration;
mod crypto;
mod error;
mod frame;
mod handshake;
mod login;
mod nbt;
mod play;
mod status;

pub use compression::{MAX_UNCOMPRESSED_LENGTH, compress_body, decompress_body};
pub use configuration::{
    ACCEPT_CODE_OF_CONDUCT_PACKET_ID, ACKNOWLEDGE_FINISH_CONFIGURATION_PACKET_ID,
    CLIENT_INFORMATION_PACKET_ID as CONFIGURATION_CLIENT_INFORMATION_PACKET_ID,
    CODE_OF_CONDUCT_PACKET_ID, COOKIE_RESPONSE_PACKET_ID, CUSTOM_CLICK_ACTION_PACKET_ID,
    CUSTOM_PAYLOAD_PACKET_ID, ClientInformation,
    DISCONNECT_PACKET_ID as CONFIGURATION_DISCONNECT_PACKET_ID, FINISH_CONFIGURATION_PACKET_ID,
    KEEP_ALIVE_PACKET_ID as CONFIGURATION_KEEP_ALIVE_PACKET_ID, KNOWN_PACKS_PACKET_ID, KnownPack,
    MAX_KNOWN_PACK_STRING_UTF16_UNITS, MAX_LOCALE_UTF16_UNITS, PONG_PACKET_ID,
    REGISTRY_DATA_PACKET_ID, RESOURCE_PACK_RESPONSE_PACKET_ID, RegistryData, RegistryEntry,
    SELECT_KNOWN_PACKS_PACKET_ID, TaggedRegistry, UPDATE_ENABLED_FEATURES_PACKET_ID,
    UPDATE_TAGS_PACKET_ID, decode_client_information, decode_finish_configuration_ack,
    decode_known_packs, encode_code_of_conduct_payload,
    encode_disconnect_payload as encode_configuration_disconnect_payload, encode_feature_flags,
    encode_feature_flags_payload, encode_finish_configuration, encode_finish_configuration_payload,
    encode_registry_data, encode_registry_data_payload, encode_select_known_packs,
    encode_select_known_packs_payload, encode_update_tags, encode_update_tags_payload,
};
pub use crypto::{
    Cfb8Stream, SHARED_SECRET_LENGTH, VERIFY_TOKEN_LENGTH, decrypt_pkcs1v15, generate_rsa_keypair,
    server_id_hash,
};
pub use error::ProtocolError;
pub use frame::{
    FrameSplit, MAX_PACKET_LENGTH, PacketFrame, decode_frame, decode_packet_data, decode_var_i32,
    encode_frame, encode_var_i32, split_frame,
};
pub use handshake::{
    HandshakeIntent, HandshakePacket, MAX_HANDSHAKE_SERVER_ADDRESS_UTF16_UNITS, decode_handshake,
};
pub use login::{
    ENCRYPTION_REQUEST_PACKET_ID, EncryptionRequest, EncryptionResponse, GameProfile,
    GameProfileProperty, LOGIN_ACKNOWLEDGED_PACKET_ID, LOGIN_DISCONNECT_PACKET_ID,
    LOGIN_SUCCESS_PACKET_ID, LoginStart, LoginSuccess, MAX_PROPERTIES,
    MAX_PROPERTY_NAME_UTF16_UNITS, MAX_PROPERTY_SIGNATURE_UTF16_UNITS,
    MAX_PROPERTY_VALUE_UTF16_UNITS, MAX_PUBLIC_KEY_LENGTH, MAX_SHARED_SECRET_LENGTH,
    MAX_USERNAME_UTF16_UNITS, MAX_VERIFY_TOKEN_LENGTH, SET_COMPRESSION_PACKET_ID, SetCompression,
    decode_encryption_response, decode_login_acknowledged, decode_login_start,
    encode_encryption_request, encode_encryption_request_payload, encode_login_disconnect,
    encode_login_disconnect_payload, encode_login_success, encode_login_success_payload,
    encode_set_compression, offline_mode_uuid,
};
pub use nbt::{
    NbtReader, NbtTag, decode_compound_tag, decode_named_tag, encode_compound_tag,
    encode_named_tag, encode_string_tag,
};
pub use play::{
    CHAT_MESSAGE_PACKET_ID, CHAT_SESSION_UPDATE_PACKET_ID, CHUNK_BATCH_FINISHED_PACKET_ID,
    CHUNK_BATCH_RECEIVED_PACKET_ID, CHUNK_BATCH_START_PACKET_ID, CLIENT_INFORMATION_PACKET_ID,
    CLIENT_TICK_END_PACKET_ID, CONFIRM_TELEPORTATION_PACKET_ID, ChatMessage, DISCONNECT_PACKET_ID,
    GAME_EVENT_PACKET_ID, KEEP_ALIVE_PACKET_ID, LEVEL_CHUNK_WITH_LIGHT_PACKET_ID, LOGIN_PACKET_ID,
    LoginPlay, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE, MOVE_PLAYER_POS_PACKET_ID,
    MOVE_PLAYER_POS_ROT_PACKET_ID, MOVE_PLAYER_ROT_PACKET_ID, MovePlayerPos, MovePlayerPosRot,
    MovePlayerRot, PING_PACKET_ID, PING_REQUEST_PACKET_ID, PLAYER_ABILITIES_PACKET_ID,
    PLAYER_COMMAND_PACKET_ID, PLAYER_INFO_UPDATE_PACKET_ID, PLAYER_INPUT_PACKET_ID,
    PLAYER_LOADED_PACKET_ID, PLAYER_POSITION_PACKET_ID, PlayerAbilities, PlayerInfoUpdate,
    SERVER_DATA_PACKET_ID, SERVERBOUND_KEEP_ALIVE_PACKET_ID, SET_CHUNK_CACHE_CENTER_PACKET_ID,
    SET_CHUNK_CACHE_RADIUS_PACKET_ID, SET_DEFAULT_SPAWN_POSITION_PACKET_ID,
    SET_SIMULATION_DISTANCE_PACKET_ID, SET_TICKING_STATE_PACKET_ID, SYSTEM_CHAT_MESSAGE_PACKET_ID,
    ServerData, TimeClock, UPDATE_TIME_PACKET_ID, decode_chat_message, decode_chunk_batch_received,
    decode_client_tick_end, decode_confirm_teleportation, decode_keep_alive,
    decode_move_player_pos, decode_move_player_pos_rot, decode_move_player_rot,
    decode_ping_request as decode_play_ping_request, decode_player_loaded,
    encode_chunk_batch_finished_payload, encode_chunk_batch_start_payload,
    encode_disconnect_payload, encode_empty_chunk_payload, encode_game_event_payload,
    encode_keep_alive_payload, encode_login_payload, encode_ping_payload,
    encode_player_abilities_payload, encode_player_info_update_payload,
    encode_player_position_payload, encode_server_data_payload,
    encode_set_chunk_cache_center_payload, encode_set_chunk_cache_radius_payload,
    encode_set_default_spawn_position_payload, encode_set_simulation_distance_payload,
    encode_set_ticking_state_payload, encode_system_chat_message_payload,
    encode_update_time_payload,
};
pub use status::{
    MAX_STATUS_RESPONSE_UTF16_UNITS, PingRequest, StatusDescription, StatusPlayer, StatusPlayers,
    StatusResponse, StatusVersion, decode_ping_request, decode_status_request,
    encode_pong_response, encode_status_response, encode_status_response_payload,
};

/// Protocol version used by Minecraft Java Edition 26.2.
pub const SUPPORTED_PROTOCOL_VERSION: i32 = 776;

/// Client connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Handshake,
    Status,
    Login,
    Configuration,
    Play,
}

#[cfg(test)]
mod tests {
    use super::ConnectionState;

    #[test]
    fn states_are_distinct() {
        assert_ne!(ConnectionState::Handshake, ConnectionState::Play);
    }
}
