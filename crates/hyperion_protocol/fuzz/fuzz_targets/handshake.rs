#![no_main]

use hyperion_protocol::{decode_frame, decode_handshake};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok((frame, _)) = decode_frame(input) {
        let _ = decode_handshake(&frame);
    }
});
