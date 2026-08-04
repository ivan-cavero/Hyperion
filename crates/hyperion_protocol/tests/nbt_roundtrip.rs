use hyperion_protocol::{
    NbtTag, ProtocolError, decode_compound_tag, encode_compound_tag, encode_string_tag,
};

// ============================================================
// Round-trip helper
// ============================================================

fn round_trip(entries: &[(String, NbtTag)]) {
    let encoded = encode_compound_tag(entries).unwrap();
    let decoded = decode_compound_tag(&encoded).unwrap();
    assert_eq!(decoded, entries, "decode must match original");
    let re_encoded = encode_compound_tag(&decoded).unwrap();
    assert_eq!(
        re_encoded, encoded,
        "re-encode must produce identical bytes"
    );
}

fn name(s: &str) -> String {
    s.to_string()
}

// ============================================================
// 1. Each of the 12 tag types individually
// ============================================================

#[test]
fn t01_round_trip_byte() {
    round_trip(&[(name("v"), NbtTag::Byte(0))]);
    round_trip(&[(name("v"), NbtTag::Byte(127))]);
    round_trip(&[(name("v"), NbtTag::Byte(-128))]);
}

#[test]
fn t02_round_trip_short() {
    round_trip(&[(name("v"), NbtTag::Short(0))]);
    round_trip(&[(name("v"), NbtTag::Short(32767))]);
    round_trip(&[(name("v"), NbtTag::Short(-32768))]);
}

#[test]
fn t03_round_trip_int() {
    round_trip(&[(name("v"), NbtTag::Int(0))]);
    round_trip(&[(name("v"), NbtTag::Int(2147483647))]);
    round_trip(&[(name("v"), NbtTag::Int(-2147483648))]);
}

#[test]
fn t04_round_trip_long() {
    round_trip(&[(name("v"), NbtTag::Long(0))]);
    round_trip(&[(name("v"), NbtTag::Long(9223372036854775807))]);
    round_trip(&[(name("v"), NbtTag::Long(-9223372036854775808))]);
}

#[test]
fn t05_round_trip_float() {
    round_trip(&[(name("v"), NbtTag::Float(0.0))]);
    round_trip(&[(name("v"), NbtTag::Float(-1.5))]);
    round_trip(&[(name("v"), NbtTag::Float(1.25))]);
}

#[test]
fn t06_round_trip_double() {
    round_trip(&[(name("v"), NbtTag::Double(0.0))]);
    round_trip(&[(name("v"), NbtTag::Double(-1.5))]);
    round_trip(&[(name("v"), NbtTag::Double(2.5))]);
}

#[test]
fn t07_round_trip_byte_array() {
    round_trip(&[(name("v"), NbtTag::ByteArray(vec![]))]);
    round_trip(&[(name("v"), NbtTag::ByteArray(vec![1, 2, 3]))]);
    let big: Vec<u8> = (0..=255u8).collect();
    round_trip(&[(name("v"), NbtTag::ByteArray(big))]);
}

#[test]
fn t08_round_trip_string() {
    round_trip(&[(name("v"), NbtTag::String("".into()))]);
    round_trip(&[(name("v"), NbtTag::String("hello".into()))]);
    round_trip(&[(name("v"), NbtTag::String("héllo 𝄞 world".into()))]);
}

#[test]
fn t09_round_trip_list() {
    round_trip(&[(name("v"), NbtTag::List(vec![]))]);
    round_trip(&[(
        name("v"),
        NbtTag::List(vec![NbtTag::Int(1), NbtTag::Int(2), NbtTag::Int(3)]),
    )]);
    round_trip(&[(
        name("v"),
        NbtTag::List(vec![NbtTag::String("a".into()), NbtTag::String("b".into())]),
    )]);
    round_trip(&[(
        name("v"),
        NbtTag::List(vec![
            NbtTag::Compound(vec![(name("x"), NbtTag::Int(1))]),
            NbtTag::Compound(vec![(name("x"), NbtTag::Int(2))]),
        ]),
    )]);
}

#[test]
fn t10_round_trip_compound() {
    round_trip(&[(name("v"), NbtTag::Compound(vec![]))]);
    round_trip(&[(
        name("v"),
        NbtTag::Compound(vec![(name("a"), NbtTag::Int(1))]),
    )]);
    round_trip(&[(
        name("v"),
        NbtTag::Compound(vec![(
            name("a"),
            NbtTag::Compound(vec![(name("b"), NbtTag::Int(2))]),
        )]),
    )]);
}

#[test]
fn t11_round_trip_int_array() {
    round_trip(&[(name("v"), NbtTag::IntArray(vec![]))]);
    round_trip(&[(name("v"), NbtTag::IntArray(vec![1, 2, 3]))]);
    round_trip(&[(
        name("v"),
        NbtTag::IntArray(vec![-1, -2, -3, 0, 2147483647, -2147483648]),
    )]);
}

#[test]
fn t12_round_trip_long_array() {
    round_trip(&[(name("v"), NbtTag::LongArray(vec![]))]);
    round_trip(&[(name("v"), NbtTag::LongArray(vec![1, 2, 3]))]);
    round_trip(&[(
        name("v"),
        NbtTag::LongArray(vec![
            -1,
            -2,
            -3,
            0,
            9223372036854775807,
            -9223372036854775808,
        ]),
    )]);
}

// ============================================================
// 2. Mixed compound with all types together
// ============================================================

#[test]
fn t13_round_trip_mixed_all_types() {
    round_trip(&[
        (name("byte"), NbtTag::Byte(127)),
        (name("short"), NbtTag::Short(-32768)),
        (name("int"), NbtTag::Int(2147483647)),
        (name("long"), NbtTag::Long(9223372036854775807)),
        (name("float"), NbtTag::Float(1.25)),
        (name("double"), NbtTag::Double(-2.5)),
        (name("byte_array"), NbtTag::ByteArray(vec![0xde, 0xad])),
        (name("string"), NbtTag::String("hello".into())),
        (
            name("list"),
            NbtTag::List(vec![NbtTag::Int(1), NbtTag::Int(2)]),
        ),
        (
            name("compound"),
            NbtTag::Compound(vec![(name("inner"), NbtTag::Byte(1))]),
        ),
        (name("int_array"), NbtTag::IntArray(vec![1, 2, 3])),
        (name("long_array"), NbtTag::LongArray(vec![4, 5, 6])),
    ]);
}

// ============================================================
// 3. Nested structures
// ============================================================

#[test]
fn t14_round_trip_three_level_nested() {
    round_trip(&[(
        name("a"),
        NbtTag::Compound(vec![(
            name("b"),
            NbtTag::Compound(vec![(name("c"), NbtTag::Int(42))]),
        )]),
    )]);
}

#[test]
fn t15_round_trip_list_of_compounds_with_lists() {
    round_trip(&[(
        name("data"),
        NbtTag::List(vec![
            NbtTag::Compound(vec![(
                name("nums"),
                NbtTag::List(vec![NbtTag::Int(1), NbtTag::Int(2)]),
            )]),
            NbtTag::Compound(vec![(
                name("nums"),
                NbtTag::List(vec![NbtTag::Int(3), NbtTag::Int(4)]),
            )]),
        ]),
    )]);
}

#[test]
fn t16_round_trip_deep_nesting_16() {
    let mut tag = NbtTag::Byte(1);
    for _ in 0..16 {
        tag = NbtTag::Compound(vec![(name("x"), tag)]);
    }
    round_trip(&[(name("root"), tag)]);
}

// ============================================================
// 4. Empty variants
// ============================================================

#[test]
fn t17_round_trip_empty_compound() {
    round_trip(&[]);
}

#[test]
fn t18_round_trip_empty_variants() {
    round_trip(&[(name("e"), NbtTag::ByteArray(vec![]))]);
    round_trip(&[(name("e"), NbtTag::IntArray(vec![]))]);
    round_trip(&[(name("e"), NbtTag::LongArray(vec![]))]);
    round_trip(&[(name("e"), NbtTag::List(vec![]))]);
    round_trip(&[(name("e"), NbtTag::Compound(vec![]))]);
    round_trip(&[(name("e"), NbtTag::String("".into()))]);
}

// ============================================================
// 5. Hostile / robustness tests
// ============================================================

#[test]
fn h01_hostile_empty_input() {
    assert!(decode_compound_tag(&[]).is_err());
}

#[test]
fn h02_hostile_just_compound_tag() {
    assert!(decode_compound_tag(&[0x0a]).is_err());
}

#[test]
fn h03_hostile_unknown_tag_type_0x0d() {
    let input = vec![0x0a, 0x0d, 0x00, 0x01, b'x', 0x00];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn h04_hostile_unknown_tag_type_0xff() {
    let input = vec![0x0a, 0xff, 0x00, 0x01, b'x', 0x00];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn h05_hostile_negative_byte_array_length() {
    // ByteArray with i32::MIN as length
    let input = vec![0x0a, 0x07, 0x00, 0x01, b'x', 0x80, 0x00, 0x00, 0x00];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn h06_hostile_negative_int_array_length() {
    let input = vec![0x0a, 0x0b, 0x00, 0x01, b'x', 0x80, 0x00, 0x00, 0x00];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn h07_hostile_negative_long_array_length() {
    let input = vec![0x0a, 0x0c, 0x00, 0x01, b'x', 0x80, 0x00, 0x00, 0x00];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn h08_hostile_negative_list_count() {
    let input = vec![0x0a, 0x09, 0x00, 0x01, b'x', 0x01, 0xff, 0xff, 0xff, 0xff];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

#[test]
fn h09_hostile_depth_exceeds_128() {
    fn build_deep(depth: usize) -> NbtTag {
        let mut tag = NbtTag::Byte(1);
        for _ in 0..depth {
            tag = NbtTag::Compound(vec![(name("x"), tag)]);
        }
        tag
    }
    // 128 nested compounds -> total depth 129 >= 128 -> exceeds
    let tag = build_deep(128);
    let encoded = encode_compound_tag(&[(name("root"), tag)]).unwrap();
    assert_eq!(
        decode_compound_tag(&encoded),
        Err(ProtocolError::NbtDepthExceeded)
    );
}

#[test]
fn h10_hostile_i32_max_list_count() {
    // List with count = i32::MAX but no data follows
    // Bytes: root_compound, TAG_List, name_len=1, 'x', elem_type=BYTE, count=i32::MAX
    let input = vec![0x0a, 0x09, 0x00, 0x01, b'x', 0x01, 0x7f, 0xff, 0xff, 0xff];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::UnexpectedEndOfInput)
    );
}

#[test]
fn h11_hostile_u16_max_string_length() {
    // String with length = u16::MAX but no data follows
    let input = vec![0x0a, 0x08, 0x00, 0x01, b'x', 0xff, 0xff];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::UnexpectedEndOfInput)
    );
}

#[test]
fn h12_hostile_truncated_last_byte_each_type() {
    let cases = vec![
        NbtTag::Byte(42),
        NbtTag::Short(42),
        NbtTag::Int(42),
        NbtTag::Long(42),
        NbtTag::Float(1.25),
        NbtTag::Double(-2.5),
        NbtTag::ByteArray(vec![1, 2, 3]),
        NbtTag::String("hello".into()),
        NbtTag::List(vec![NbtTag::Int(1), NbtTag::Int(2)]),
        NbtTag::Compound(vec![(name("inner"), NbtTag::Int(1))]),
        NbtTag::IntArray(vec![1, 2, 3]),
        NbtTag::LongArray(vec![1, 2, 3]),
    ];
    for (i, tag) in cases.iter().enumerate() {
        let encoded = encode_compound_tag(&[(name("v"), tag.clone())]).unwrap();
        let truncated = &encoded[..encoded.len() - 1];
        assert!(
            decode_compound_tag(truncated).is_err(),
            "truncated last byte failed for tag type {}",
            i
        );
    }
}

#[test]
fn h13_hostile_truncated_byte_array_mid_data() {
    // ByteArray with length 10 but only 3 bytes of data
    let input = vec![
        0x0a, 0x07, 0x00, 0x02, b'd', b't', 0x00, 0x00, 0x00, 0x0a, 1, 2, 3,
    ];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::UnexpectedEndOfInput)
    );
}

#[test]
fn h14_hostile_truncated_string_mid_data() {
    // String with length 100 but no data
    let input = vec![0x0a, 0x08, 0x00, 0x02, b'd', b't', 0x00, 0x64];
    // u16 length 100 = 0x0064, but only 0 bytes follow
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::UnexpectedEndOfInput)
    );
}

#[test]
fn h15_hostile_truncated_list_mid_data() {
    // List with count 100 but no data
    let input = vec![
        0x0a, 0x09, 0x00, 0x02, b'd', b't', 0x01, 0x00, 0x00, 0x00, 0x64,
    ];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::UnexpectedEndOfInput)
    );
}

#[test]
fn h16_hostile_not_a_compound() {
    // First byte is TAG_BYTE not TAG_COMPOUND
    let input = vec![0x01, 0x00, 0x01, b'x', 42, 0x00];
    assert_eq!(
        decode_compound_tag(&input),
        Err(ProtocolError::InvalidPacketPayload)
    );
}

// ============================================================
// 6. Fuzz-lite: 1000 random malformed inputs, no panic
// ============================================================

#[test]
fn fuzz_lite_never_panics() {
    // Simple xorshift64 PRNG (no external crate needed)
    struct XorShift64(u64);
    impl XorShift64 {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    let mut rng = XorShift64(42);
    for _ in 0..1000 {
        let len = (rng.next() % 200) as usize;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(rng.next() as u8);
        }
        // Must not panic — Err is perfectly fine
        let _ = decode_compound_tag(&bytes);
    }
}

// ============================================================
// 7. String tag encoding/decoding
// ============================================================

#[test]
fn s01_round_trip_string_tag() {
    let cases = vec!["", "hello", "héllo 𝄞 world"];
    for s in &cases {
        let encoded = encode_string_tag(s).unwrap();
        // Manually decode: first byte is TAG_STRING, then u16 length, then data
        assert_eq!(encoded[0], 0x08, "string tag type byte");
        let len = u16::from_be_bytes([encoded[1], encoded[2]]);
        let payload = &encoded[3..];
        assert_eq!(len as usize, payload.len());
        let decoded = std::str::from_utf8(payload).unwrap();
        assert_eq!(decoded, *s);
    }
}
