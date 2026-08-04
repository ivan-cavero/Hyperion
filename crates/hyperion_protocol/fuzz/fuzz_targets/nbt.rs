#![no_main]

use hyperion_protocol::decode_compound_tag;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let _ = decode_compound_tag(input);
});
