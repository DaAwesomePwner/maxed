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

// Synthetic CRUCIBLE_RESPONSE bodies shaped like the owner-verified layout:
// i16 count, big-endian i32 ids, i16 count, u16-length UTF-8 JSON strings.
const CRUCIBLE_JSON: &str = r#"{"array":[{"id":"1001","title":"Synthetic Crucible","desc":"Synthetic\tseason","crucible":{"start":1,"end":2,"leavelimitation":null,"seasonalonly":null,"trials":[],"restrictions":[{"type":2,"stat":"VIT","value":1.3,"percent":1},{"type":6,"healMethods":"Pets","value":0.7}],"bonuses":[{"type":1,"amount":1.15,"percent":1},{"type":2,"amount":1.1,"fame":1,"percent":1}]}}]}"#;
const RITUAL_JSON: &str = r#"{"array":[{"id":"2002","title":"1","desc":"Synthetic","bloodRitual":{"start":1,"end":2,"leavelimitation":null,"seasonalonly":null,"bonuses":[{"type":4,"amount":0.8,"percent":1},{"type":5,"amount":1.24,"percent":1},{"type":3,"amount":1,"percent":1}]}}]}"#;

fn crucible_body(ids: &[i32], jsons: &[&str]) -> Vec<u8> {
    let mut body = (ids.len() as i16).to_be_bytes().to_vec();
    for id in ids {
        body.extend(id.to_be_bytes());
    }
    body.extend((jsons.len() as i16).to_be_bytes());
    for json in jsons {
        body.extend((json.len() as u16).to_be_bytes());
        body.extend(json.as_bytes());
    }
    body
}

#[test]
fn crucible_response_decodes_ids_and_json_strings() {
    let body = crucible_body(&[0, 2], &[CRUCIBLE_JSON, RITUAL_JSON]);
    let (packet, _) = decode_packet(183, ServerToClient, &body).unwrap();
    assert_eq!(packet.name(), "CRUCIBLE_RESPONSE");
    let Packet::CrucibleResponse(response) = &packet else {
        panic!("typed CRUCIBLE_RESPONSE required")
    };
    assert_eq!(response.crucibleIds, [0, 2]);
    assert_eq!(response.crucibleJsons, [CRUCIBLE_JSON, RITUAL_JSON]);
    // Prefixes fail, trailing bytes are reported and the inventory keeps config.
    check(
        183,
        &body,
        json!({"crucibleIds": [0, 2], "crucibleJsons": [CRUCIBLE_JSON, RITUAL_JSON]}),
    );
    check(
        183,
        &crucible_body(&[], &[]),
        json!({"crucibleIds": [], "crucibleJsons": []}),
    );
    let inventory = maxed_protocol::privacy::inventory(183, ServerToClient, &body).unwrap();
    assert_eq!(inventory.treatments.len(), 2);
    for field in &inventory.treatments {
        assert_eq!(field.name, "crucibleJsons");
        assert_eq!(field.policy, maxed_protocol::privacy::FieldPolicy::Keep);
        assert!(field.public_literal);
    }
}

#[test]
fn crucible_response_rejects_bad_counts_and_lengths() {
    for (ids, jsons) in [(-1i16, 0i16), (0, -1), (65, 0), (0, 65), (i16::MIN, 0)] {
        let mut body = ids.to_be_bytes().to_vec();
        body.extend(vec![0; 4 * usize::try_from(ids.max(0)).unwrap()]);
        body.extend(jsons.to_be_bytes());
        body.extend(vec![0; 2 * usize::try_from(jsons.max(0)).unwrap()]);
        assert!(
            matches!(
                decode_packet(183, ServerToClient, &body),
                Err(ProtocolError::Definition)
            ),
            "ids={ids} jsons={jsons}"
        );
    }
    // A count larger than the remaining body cannot allocate or read past it.
    assert!(matches!(
        decode_packet(183, ServerToClient, &[0, 64, 0, 0, 0, 0]),
        Err(ProtocolError::Length)
    ));
    // A string length running past the body is truncated, not clamped.
    assert!(matches!(
        decode_packet(183, ServerToClient, &[0, 0, 0, 1, 0, 9, b'{', b'}']),
        Err(ProtocolError::Truncated)
    ));
    let maximum = crucible_body(&[7; 64], &["{}"; 64]);
    let (packet, left) = decode_packet(183, ServerToClient, &maximum).unwrap();
    assert_eq!(left, 0);
    assert_eq!(packet.fields().unwrap()["crucibleIds"], json!(vec![7; 64]));
}

#[test]
fn crucible_json_outside_the_config_allowlist_is_filled() {
    use maxed_protocol::privacy::{inventory, FieldPolicy, SecretKind};
    let progress = CRUCIBLE_JSON.replace(r#""trials":[]"#, r#""trials":[{"progress":3}]"#);
    let escaped = CRUCIBLE_JSON.replace("Synthetic Crucible", r"Synthetic \u0043rucible");
    let extra_top = CRUCIBLE_JSON.replacen('{', r#"{"accountId":"1","#, 1);
    for json in [
        progress.as_str(),
        escaped.as_str(),
        extra_top.as_str(),
        r#"{"array":{}}"#,
        r#"{"array":[[[[[[[[[[]]]]]]]]]]}"#,
        r#"["array"]"#,
        "not json",
        "",
    ] {
        let body = crucible_body(&[0], &[RITUAL_JSON, json]);
        let (packet, left) = decode_packet(183, ServerToClient, &body).unwrap();
        assert_eq!(left, 0);
        assert_eq!(packet.fields().unwrap()["crucibleJsons"][1], json);
        let inventory = inventory(183, ServerToClient, &body).unwrap();
        assert_eq!(inventory.treatments[0].policy, FieldPolicy::Keep);
        assert_eq!(
            inventory.treatments[1].policy,
            FieldPolicy::FillUnclassified,
            "{json}"
        );
        assert_eq!(inventory.treatments[1].name, "crucibleJsons");
        assert_eq!(inventory.spans.len(), 1);
        assert_eq!(inventory.spans[0].kind, SecretKind::Unclassified);
        assert_eq!(inventory.spans[0].len, json.len());
    }
}
