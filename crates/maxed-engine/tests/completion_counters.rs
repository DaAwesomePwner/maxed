use crate::common;
use common::*;
use maxed_engine::*;
use maxed_protocol::{defs::CreateSuccess, Packet};
use maxed_types::*;
fn encode(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut bits = 0;
    let mut n = 0u32;
    for &b in bytes {
        n = (n << 8) | u32::from(b);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(A[((n >> bits) & 63) as usize] as char);
        }
        n &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(A[(n << (6 - bits)) as usize] as char);
    }
    while !out.len().is_multiple_of(4) {
        out.push('=');
    }
    out
}
fn encoded(bit: usize, value: u8) -> String {
    let mut b = vec![1, 0, 0, 0];
    b.resize(20, 0);
    b[4 + bit / 8] |= 1 << (bit % 8);
    b.push(value);
    encode(&b)
}
fn counts(n: u8) -> Packet {
    Packet::CreateSuccess(CreateSuccess {
        objectId: 1,
        charId: 1,
        str: encoded(41, n),
    })
}
#[test]
fn each_known_completion_index_and_unknown_future_bits() {
    // RealmCharacterStats.updateStats ordering, independently pinned expected bit mapping.
    let bits = [
        23, 120, 35, 62, 67, 48, 3, 5, 46, 34, 40, 32, 7, 59, 37, 15, 1, 33, 4, 102, 68, 88, 58,
        89, 103, 90, 70, 2, 43, 38, 39, 55, 91, 6, 45, 92, 0, 101, 71, 52, 14, 93, 94, 95, 44, 21,
        121, 42, 49, 123, 80, 81, 47, 8, 125, 9, 10, 69, 60, 51, 82, 53, 63, 122, 66, 41, 13, 50,
        22, 83, 98, 99, 100, 84, 61,
    ];
    for (index, bit) in bits.into_iter().enumerate() {
        let c = decode_completion_counts(&encoded(bit, 9)).unwrap();
        assert_eq!(c.by_index[index], Some(9));
        assert_eq!(c.by_index.iter().flatten().sum::<i32>(), 9);
    }
    for bit in [11, 54, 56, 57, 73] {
        assert_eq!(
            decode_completion_counts(&encoded(bit, 1)),
            Err(CompletionError::Unsupported)
        );
    }
    let mut raw = vec![1, 0, 0, 0];
    raw.resize(20, 0);
    raw[4 + 41 / 8] = 1 << (41 % 8);
    raw.extend([0x80]);
    assert_eq!(
        decode_completion_counts(&encode(&raw)),
        Err(CompletionError::Truncated)
    );
}
#[test]
fn only_next_area_same_verified_character_plus_one_revises_run() {
    for delta in [0, 1, 2] {
        for pre_map in [false, true] {
            let mut e = engine();
            setup(&mut e);
            apply(&mut e, 5000, tick(vec![]));
            if pre_map {
                connection(&mut e, 2, 6000, counts(delta));
            }
            connection(&mut e, 2, 7000, map("Nexus"));
            if !pre_map {
                connection(&mut e, 2, 8000, counts(delta));
            }
            let events = connection(&mut e, 2, 9000, update(vec![player()], vec![]));
            let revised = events.iter().any(
                |v| matches!(v,EngineEvent::RunUpdated(r) if r.outcome==Some(Outcome::Cleared)),
            );
            assert_eq!(revised, delta == 1, "delta {delta}, before map {pre_map}");
        }
    }
}
#[test]
fn counter_rejects_receipt_after_deadline_and_intervening_area() {
    for late in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 5000, tick(vec![]));
        apply(&mut e, 6000, map("Nexus"));
        if !late {
            apply(&mut e, 7000, map("Realm"));
        }
        let at = if late { 30_005_001 } else { 8000 };
        apply(&mut e, at, counts(1));
        let events = apply(&mut e, at + 1000, update(vec![player()], vec![]));
        assert!(!events
            .iter()
            .any(|v| matches!(v,EngineEvent::RunUpdated(r) if r.outcome==Some(Outcome::Cleared))));
    }
}
#[test]
fn next_create_before_map_on_same_connection_cannot_relabel_prior_run() {
    let mut e = engine();
    setup(&mut e);
    let id = e.snapshot().run.unwrap().character_id;
    apply(&mut e, 3000, counts(1));
    assert_eq!(e.snapshot().run.unwrap().character_id, id);
    apply(&mut e, 4000, map("Nexus"));
    let events = apply(&mut e, 5000, update(vec![player()], vec![]));
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::RunUpdated(r) if r.outcome==Some(Outcome::Cleared))));
}
