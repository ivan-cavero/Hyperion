#![no_main]

use hyperion_protocol::{
    decode_encryption_response, decode_frame, decode_login_acknowledged, decode_login_start,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok((frame, _)) = decode_frame(input) {
        let _ = decode_login_start(&frame);
        let _ = decode_encryption_response(&frame);
        let _ = decode_login_acknowledged(&frame);
    }
});
