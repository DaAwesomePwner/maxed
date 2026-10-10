use maxed_protocol::Rc4;

#[test]
fn state_forks_and_reset_reproduce_keystream() {
    let fresh = Rc4::new(b"Key").unwrap();
    let mut first = fresh.clone();
    let mut text = b"Plaintext".to_vec();
    first.apply(&mut text);
    assert_eq!(text, [0xbb, 0xf3, 0x16, 0xe8, 0xd9, 0x40, 0xaf, 0x0a, 0xd3]);
    fresh.clone().apply(&mut text);
    assert_eq!(text, b"Plaintext");
    assert!(Rc4::new(&[]).is_err());
}

#[test]
fn directional_protocol_constants_are_exact_and_distinct() {
    use maxed_protocol::keys::{key_bytes, INCOMING_HEX, OUTGOING_HEX};
    use maxed_types::Direction;
    assert_eq!(INCOMING_HEX, "c91d9eec420160730d825604e0");
    assert_eq!(OUTGOING_HEX, "5a4d2016bc16dc64883194ffd9");
    let incoming = key_bytes(Direction::ServerToClient).unwrap();
    let outgoing = key_bytes(Direction::ClientToServer).unwrap();
    assert_eq!(
        incoming,
        [0xc9, 0x1d, 0x9e, 0xec, 0x42, 1, 0x60, 0x73, 0x0d, 0x82, 0x56, 4, 0xe0]
    );
    assert_eq!(
        outgoing,
        [0x5a, 0x4d, 0x20, 0x16, 0xbc, 0x16, 0xdc, 0x64, 0x88, 0x31, 0x94, 0xff, 0xd9]
    );
    let mut a = [0; 64];
    let mut b = [0; 64];
    Rc4::new(&incoming).unwrap().apply(&mut a);
    Rc4::new(&outgoing).unwrap().apply(&mut b);
    assert_ne!(a, b);
}

#[test]
fn skipping_matches_zero_application_and_offset_clones_are_independent() {
    for offset in [0, 1, 255, 256, 257, 4096] {
        let fresh = Rc4::new(b"synthetic-key").unwrap();
        let mut skipped = fresh.clone();
        skipped.skip(offset);
        let mut applied = fresh.clone();
        applied.apply(&mut vec![0; offset]);
        let mut fork = skipped.clone();
        let mut a = [0; 512];
        let mut b = [0; 512];
        let mut c = [0; 512];
        skipped.apply(&mut a);
        applied.apply(&mut b);
        fork.apply(&mut c);
        assert_eq!(a, b);
        assert_eq!(a, c);
        // Consuming the clones cannot alter the retained reset state.
        let mut reset = [0; 512];
        let mut expected = [0; 512];
        fresh.clone().apply(&mut reset);
        Rc4::new(b"synthetic-key").unwrap().apply(&mut expected);
        assert_eq!(reset, expected);
    }
}

#[test]
fn chunking_and_empty_calls_do_not_change_the_keystream() {
    let mut whole = Rc4::new(&[0xff]).unwrap();
    let mut chunks = whole.clone();
    let mut a = [0x80; 1024];
    let mut b = a;
    whole.apply(&mut a);
    chunks.apply(&mut []);
    chunks.skip(0);
    for chunk in b.chunks_mut(7) {
        chunks.apply(chunk);
    }
    assert_eq!(a, b);
}
