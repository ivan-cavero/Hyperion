#![no_main]

use hyperion_protocol::{
    decode_chat_message, decode_chunk_batch_received, decode_client_tick_end,
    decode_confirm_teleportation, decode_frame, decode_keep_alive, decode_move_player_pos,
    decode_move_player_pos_rot, decode_move_player_rot, decode_play_ping_request,
    decode_player_loaded,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok((frame, _)) = decode_frame(input) {
        let _ = decode_confirm_teleportation(&frame.payload);
        let _ = decode_keep_alive(&frame.payload);
        let _ = decode_play_ping_request(&frame.payload);
        let _ = decode_chat_message(&frame.payload);
        let _ = decode_move_player_pos(&frame.payload);
        let _ = decode_move_player_pos_rot(&frame.payload);
        let _ = decode_move_player_rot(&frame.payload);
        let _ = decode_player_loaded(&frame.payload);
        let _ = decode_chunk_batch_received(&frame.payload);
        let _ = decode_client_tick_end(&frame.payload);
    }
});
