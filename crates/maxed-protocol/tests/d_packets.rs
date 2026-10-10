use maxed_protocol::{decode_packet, privacy::sensitive_spans, Packet, ProtocolError};
use maxed_types::Direction::{ClientToServer, ServerToClient};
use serde_json::{json, Value};

fn check(id: u8, body: &[u8], fields: Value) {
    let (packet, remaining) = decode_packet(id, ServerToClient, body).unwrap();
    assert_eq!(packet.fields().unwrap(), fields);
    assert_eq!(remaining, 0);
    assert!(sensitive_spans(id, ServerToClient, body)
        .unwrap()
        .is_empty());
    for end in 0..body.len() {
        assert!(
            decode_packet(id, ServerToClient, &body[..end]).is_err(),
            "id {id}, prefix {end}"
        );
    }
    assert!(matches!(
        decode_packet(id, ClientToServer, body).unwrap().0,
        Packet::Unknown { .. }
    ));
    let mut extended = body.to_vec();
    extended.push(0);
    assert_eq!(decode_packet(id, ServerToClient, &extended).unwrap().1, 1);
    assert!(sensitive_spans(id, ServerToClient, &extended).is_err());
}

#[test]
fn quest_obj_id_decodes_target_and_compressed_list() {
    check(
        82,
        &[255, 255, 255, 255, 0],
        json!({"objectId": -1, "list": []}),
    );
    check(82, &[0, 0, 0, 7, 1, 7], json!({"objectId": 7, "list": [7]}));
    check(
        82,
        &[0, 0, 0, 64, 3, 128, 1, 128, 2, 255, 1],
        json!({"objectId": 64, "list": [64, 128, -127]}),
    );
    let mut maximum = vec![0, 0, 0, 7, 128, 1];
    maximum.extend([7; 64]);
    check(82, &maximum, json!({"objectId": 7, "list": vec![7; 64]}));
}

#[test]
fn quest_obj_id_rejects_negative_or_oversized_count() {
    for count in [&[65][..], &[129, 1], &[128, 2]] {
        let mut body = vec![0; 4];
        body.extend_from_slice(count);
        assert!(matches!(
            decode_packet(82, ServerToClient, &body),
            Err(ProtocolError::Definition)
        ));
    }
}

#[test]
fn realm_score_update_is_one_big_endian_i32() {
    for score in [-1i32, 0, 15_644, i32::MIN, i32::MAX] {
        check(169, &score.to_be_bytes(), json!({"score": score}));
    }
}

#[test]
fn show_effect_stays_unknown() {
    assert!(matches!(
        decode_packet(11, ServerToClient, &[1, 0]).unwrap().0,
        Packet::Unknown { id: 11, .. }
    ));
}
