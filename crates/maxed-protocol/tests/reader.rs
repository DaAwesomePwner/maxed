use maxed_protocol::Reader;

#[test]
fn java_utf8_replacement_spans() {
    let cases: &[(&[u8], &str)] = &[
        (&[0xff], "\u{fffd}"),
        (&[0xed, 0xa0, 0x80], "\u{fffd}"),
        (&[0xed, 0xbf, 0xbf], "\u{fffd}"),
        (&[0xed, 0xa0], "\u{fffd}"),
        (&[0xe2, 0x82], "\u{fffd}"),
        (&[0xf0, 0x90, 0x80], "\u{fffd}"),
        (&[0xed, 0xa0, b'A'], "\u{fffd}A"),
        (&[0xe2, b'A', 0x82], "\u{fffd}A\u{fffd}"),
        (&[0xe0, 0x80, 0x80], "\u{fffd}\u{fffd}\u{fffd}"),
        (
            &[0xf4, 0x90, 0x80, 0x80],
            "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
        ),
        (&[0xed, 0x9f, 0xbf], "\u{d7ff}"),
        (&[0xf4, 0x8f, 0xbf, 0xbf], "\u{10ffff}"),
    ];
    for (bytes, expected) in cases {
        let mut wire = (bytes.len() as u16).to_be_bytes().to_vec();
        wire.extend_from_slice(bytes);
        assert_eq!(
            Reader::new(&wire).string().unwrap(),
            *expected,
            "{bytes:02x?}"
        );
    }
}

#[test]
fn compressed_sign_magnitude_and_overflow() {
    assert_eq!(Reader::new(&[0x41]).ci().unwrap(), -1);
    assert_eq!(Reader::new(&[0x80, 0x01]).ci().unwrap(), 64);
    assert_eq!(
        Reader::new(&[0xc0, 0x80, 0x80, 0x80, 0x10]).ci().unwrap(),
        i32::MIN
    );
    assert!(Reader::new(&[0x80]).ci().is_err());
    assert!(Reader::new(&[0x80, 0x80, 0x80, 0x80, 0x10]).ci().is_err());
    assert!(Reader::new(&[0xff; 6]).ci().is_err());
}

#[test]
fn fixed_width_signedness_and_endianness() {
    let mut r = Reader::new(&[0xff; 14]);
    assert_eq!(r.i8().unwrap(), -1);
    assert_eq!(r.u8().unwrap(), 255);
    assert_eq!(r.i16().unwrap(), -1);
    assert_eq!(r.u16().unwrap(), 65535);
    assert_eq!(r.i32().unwrap(), -1);
    assert_eq!(r.u32().unwrap(), u32::MAX);
    assert_eq!(r.remaining(), 0);
    assert_eq!(r.position(), 14);
    assert_eq!(Reader::new(&[0x12, 0x34]).u16().unwrap(), 0x1234);
    assert_eq!(
        Reader::new(&[0x12, 0x34, 0x56, 0x78]).u32().unwrap(),
        0x12345678
    );
}

#[test]
fn strings_use_utf8_byte_count_and_replace_malformed_input() {
    assert_eq!(Reader::new(&[0, 0]).string().unwrap(), "");
    assert_eq!(
        Reader::new(&[0, 3, 0xe2, 0x98, 0x83]).string().unwrap(),
        "\u{2603}"
    );
    assert_eq!(
        Reader::new(&[0, 1, 0xff]).string(),
        Ok("\u{fffd}".to_string())
    );
    assert!(Reader::new(&[0, 4, b'a']).string().is_err());
}

#[test]
fn blobs_are_bounded_and_uninterpreted() {
    assert_eq!(
        Reader::new(&[0, 3, 0, 0xff, 0x80]).bytes().unwrap(),
        [0, 0xff, 0x80]
    );
    assert!(Reader::new(&[0xff, 0xff]).bytes().is_err());
    assert!(Reader::new(&[0, 0]).bytes().unwrap().is_empty());
}

#[test]
fn collection_counts_are_checked_before_reading_or_allocating() {
    for (count, minimum) in [(-1, 1), (i32::MAX, 1), (1, usize::MAX), (0, 0), (1, 0)] {
        let mut r = Reader::new(&[0; 4]);
        let mut called = false;
        assert!(r
            .list(count, minimum, |r| {
                called = true;
                r.u8()
            })
            .is_err());
        assert!(!called);
        assert_eq!(r.position(), 0);
    }
    assert_eq!(Reader::new(&[1, 2]).list(2, 1, Reader::u8).unwrap(), [1, 2]);
    assert!(Reader::new(&[]).list(0, 1, Reader::u8).unwrap().is_empty());
    assert!(Reader::new(&[1]).list(1, 1, Reader::u32).is_err());
}

#[test]
fn floats_and_booleans_preserve_wire_meaning() {
    assert_eq!(Reader::new(&1.25f32.to_be_bytes()).f32().unwrap(), 1.25);
    assert_eq!(
        Reader::new(&(-0.0f32).to_be_bytes())
            .f32()
            .unwrap()
            .to_bits(),
        (-0.0f32).to_bits()
    );
    // Nonfinite rejection belongs to canonical serialization, not primitive reading.
    assert!(Reader::new(&f32::NAN.to_be_bytes()).f32().unwrap().is_nan());
    assert!(Reader::new(&[0; 3]).f32().is_err());
    for value in 0..=255 {
        assert_eq!(Reader::new(&[value]).bool().unwrap(), value != 0);
    }
}

#[test]
fn every_prefix_is_bounded_and_never_panics() {
    let samples: &[&[u8]] = &[
        &[0xff],
        &[0x12, 0x34],
        &[0x12, 0x34, 0x56, 0x78],
        &[0, 3, 0xe2, 0x98, 0x83],
        &[0, 3, 0, 0xff, 0x80],
        &[0xc0, 0x80, 0x80, 0x80, 0x10],
        &[0, 0],
    ];
    for sample in samples {
        for end in 0..=sample.len() {
            let bytes = &sample[..end];
            let _ = Reader::new(bytes).i8();
            let _ = Reader::new(bytes).u8();
            let _ = Reader::new(bytes).i16();
            let _ = Reader::new(bytes).u16();
            let _ = Reader::new(bytes).i32();
            let _ = Reader::new(bytes).u32();
            let _ = Reader::new(bytes).f32();
            let _ = Reader::new(bytes).bool();
            let _ = Reader::new(bytes).string();
            let _ = Reader::new(bytes).bytes();
            let _ = Reader::new(bytes).ci();
            let _ = Reader::new(bytes).list(i32::MAX, 1, Reader::u8);
            let mut r = Reader::new(bytes);
            assert!(r.take(usize::MAX).is_err());
            assert_eq!(r.position(), 0);
            r.take(end).unwrap();
            assert!(r.take(usize::MAX).is_err());
            assert_eq!(r.position(), end);
        }
    }
}
