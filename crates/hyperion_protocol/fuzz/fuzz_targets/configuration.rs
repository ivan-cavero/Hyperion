#![no_main]

use hyperion_protocol::{
    decode_client_information, decode_finish_configuration_ack, decode_frame, decode_known_packs,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok((frame, _)) = decode_frame(input) {
        let _ = decode_known_packs(&frame);
        let _ = decode_client_information(&frame);
        let _ = decode_finish_configuration_ack(&frame);
    }
});
