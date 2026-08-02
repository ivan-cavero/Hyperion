#![no_main]

use hyperion_protocol::{decode_frame, decode_ping_request, decode_status_request};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok((frame, _)) = decode_frame(input) {
        let _ = decode_status_request(&frame);
        let _ = decode_ping_request(&frame);
    }
});
