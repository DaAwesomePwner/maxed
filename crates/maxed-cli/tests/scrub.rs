use maxed_cli::scrub::rewrite_ciphertext;
#[test]
fn ciphertext_patch_preserves_length_and_keystream() {
    let original = b"token";
    let replacement = b"xxxxx";
    let key_stream = [1, 2, 3, 4, 5];
    let cipher: Vec<u8> = original
        .iter()
        .zip(key_stream)
        .map(|(a, b)| a ^ b)
        .collect();
    let patched = rewrite_ciphertext(&cipher, original, replacement).unwrap();
    let decoded: Vec<u8> = patched.iter().zip(key_stream).map(|(a, b)| a ^ b).collect();
    assert_eq!(decoded, replacement);
    assert!(rewrite_ciphertext(&cipher, original, b"shorter!").is_err());
}
mod support;
use maxed_cli::{scrub::scrub, Pipeline};
use maxed_protocol::{
    key_bytes,
    privacy::{sensitive_spans, SecretKind},
    DecodeEvent, Rc4,
};
use maxed_recording::{PcapReader, PcapWriter};
use maxed_types::{Direction, RawFrame, Timestamp};
use std::{fs, path::Path};
use support::PrivateDir;
const C: Direction = Direction::ClientToServer;
const S: Direction = Direction::ServerToClient;

#[test]
fn scrub_keeps_quest_and_realm_score_numeric_bodies() {
    let d = PrivateDir::new();
    let packets = [
        (82, vec![0, 0, 0, 64, 1, 128, 1]),
        (169, 15_644i32.to_be_bytes().to_vec()),
        (82, vec![255, 255, 255, 255, 0]),
    ];
    let frames = vec![
        frame(0, S, 500, true, &[]),
        frame(1, S, 501, false, &encrypted(S, &packets)),
    ];
    let input = d.file("numeric.pcap");
    let output = d.file("scrubbed.pcap");
    write(&input, &frames);
    scrub(&input, &output).unwrap();
    let scrubbed = read(&output);
    assert_eq!(&scrubbed[1].bytes[54..], &frames[1].bytes[54..]);
    assert_eq!(
        fields(scrubbed),
        vec![
            (82, serde_json::json!({"objectId":64,"list":[64]})),
            (169, serde_json::json!({"score":15644})),
            (82, serde_json::json!({"objectId":-1,"list":[]}))
        ]
    );
}
fn string(b: &mut Vec<u8>, s: &[u8]) {
    b.extend_from_slice(&(s.len() as u16).to_be_bytes());
    b.extend_from_slice(s);
}
fn hello() -> Vec<u8> {
    let mut b = 7i32.to_be_bytes().to_vec();
    string(&mut b, b"7.0.0");
    string(&mut b, b"private-access-token");
    b.extend_from_slice(&123i32.to_be_bytes());
    string(&mut b, &[0xa1, 0xb2, 0xc3]);
    for s in [
        b"Unity".as_slice(),
        b"Steam",
        b"platform-secret",
        b"client-secret",
        b"user-secret",
    ] {
        string(&mut b, s);
    }
    b
}
fn text_body(name: &[u8], text: &[u8]) -> Vec<u8> {
    let mut b = vec![];
    string(&mut b, name);
    b.extend_from_slice(&7i32.to_be_bytes());
    b.extend_from_slice(&5i16.to_be_bytes());
    b.push(3);
    string(&mut b, if name.starts_with(b"#") { b"" } else { name });
    string(&mut b, text);
    string(&mut b, text);
    b.push(1);
    b.extend_from_slice(&12i32.to_be_bytes());
    b
}
fn status(name: &[u8]) -> Vec<u8> {
    let mut b = vec![7];
    b.extend_from_slice(&[0; 8]);
    b.push(1);
    b.push(31);
    string(&mut b, name);
    b.push(0);
    b
}
fn update(name: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 8];
    b.extend_from_slice(&[0, 0, 1]);
    b.extend_from_slice(&0x0300u16.to_be_bytes());
    b.extend(status(name));
    b.push(0);
    b
}
fn tick(name: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 14];
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend(status(name));
    b
}
fn reconnect() -> Vec<u8> {
    let mut b = vec![];
    string(&mut b, b"Nexus");
    string(&mut b, b"10.2.3.4");
    b.extend_from_slice(&2050u16.to_be_bytes());
    b.extend_from_slice(&[0; 8]);
    string(&mut b, &[0xde, 0xad, 0xbe, 0xef]);
    b
}
fn encrypted(dir: Direction, packets: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut rc4 = Rc4::new(&key_bytes(dir).unwrap()).unwrap();
    let mut out = vec![];
    for (id, plain) in packets {
        let mut b = plain.clone();
        rc4.apply(&mut b);
        out.extend_from_slice(&((b.len() + 5) as u32).to_be_bytes());
        out.push(*id);
        out.extend(b);
    }
    out
}
fn frame(index: u64, dir: Direction, seq: u32, syn: bool, payload: &[u8]) -> RawFrame {
    let mut b = vec![0; 54 + payload.len()];
    b[..12].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    b[12..14].copy_from_slice(&[8, 0]);
    b[14] = 0x45;
    b[16..18].copy_from_slice(&((40 + payload.len()) as u16).to_be_bytes());
    b[22] = 64;
    b[23] = 6;
    let (a, z, ports) = if dir == C {
        ([10, 0, 0, 1], [10, 0, 0, 2], [0xc3, 0x50, 8, 2])
    } else {
        ([10, 0, 0, 2], [10, 0, 0, 1], [8, 2, 0xc3, 0x50])
    };
    b[26..30].copy_from_slice(&a);
    b[30..34].copy_from_slice(&z);
    b[34..38].copy_from_slice(&ports);
    b[38..42].copy_from_slice(&seq.to_be_bytes());
    b[46] = 0x50;
    b[47] = if syn { 2 } else { 16 };
    b[54..].copy_from_slice(payload);
    RawFrame {
        index,
        at: Timestamp(100 + index * 25),
        original_len: b.len() as u32,
        bytes: b,
    }
}
fn write(path: &Path, frames: &[RawFrame]) {
    let mut w = PcapWriter::new(fs::File::create(path).unwrap()).unwrap();
    for f in frames {
        w.write_frame(f).unwrap();
    }
    w.finish().unwrap();
}
fn read(path: &Path) -> Vec<RawFrame> {
    let mut r = PcapReader::new(fs::File::open(path).unwrap()).unwrap();
    let mut out = vec![];
    while let Some(f) = r.next_frame().unwrap() {
        out.push(f);
    }
    out
}
fn fields(frames: Vec<RawFrame>) -> Vec<(u8, serde_json::Value)> {
    let mut p = Pipeline::default();
    let mut result = vec![];
    for f in frames {
        for e in p.push(f) {
            if let DecodeEvent::Packet(p) = e {
                result.push((p.id, p.packet.fields().unwrap()));
            }
        }
    }
    result
}
fn happy() -> Vec<RawFrame> {
    let c = encrypted(C, &[(74, hello()), (105, vec![])]);
    let s = encrypted(
        S,
        &[
            (42, update(b"Alice")),
            (10, tick(b"Alice")),
            (44, text_body(b"Alice", b"Alice has a secret-token")),
            (42, update("\u{732b}\u{4eba}".as_bytes())),
            (45, reconnect()),
        ],
    );
    vec![
        frame(0, C, 100, true, b""),
        frame(1, S, 500, true, b""),
        frame(2, C, 101, false, &c[..24]),
        frame(3, C, 125, false, &c[24..]),
        frame(4, C, 101, false, &c[..24]),
        frame(5, C, 120, false, &c[19..40]),
        frame(6, S, 501, false, &s),
        frame(7, S, 520, false, &s[19..65]),
    ]
}
fn assert_refused(frames: &[RawFrame]) {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    write(&input, frames);
    assert!(scrub(&input, &output).is_err());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
}
#[test]
fn split_tokens_retransmits_overlaps_names_keys_and_checksums_replay_correctly() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let original = happy();
    write(&input, &original);
    let before = fs::read(&input).unwrap();
    let report = scrub(&input, &output).unwrap();
    assert!(report.verified);
    assert_eq!(report.frames, 8);
    assert!(report.changed_spans >= 13);
    assert_eq!(fs::read(&input).unwrap(), before);
    let rewritten = read(&output);
    let decoded = fields(rewritten.clone());
    let old = fields(original.clone());
    assert_eq!(decoded.len(), old.len());
    let hello = &decoded.iter().find(|p| p.0 == 74).unwrap().1;
    assert_eq!(hello["accessToken"], "xxxxxxxxxxxxxxxxxxxx");
    assert_eq!(hello["key"], "000000");
    assert_eq!(hello["buildVersion"], "7.0.0");
    let names: Vec<_> = decoded
        .iter()
        .filter_map(|(id, v)| match id {
            42 => Some(v["newObjects"][0]["status"]["stats"][0]["stringStatValue"].clone()),
            10 => Some(v["status"][0]["stats"][0]["stringStatValue"].clone()),
            44 => Some(v["name"].clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names[0], names[1]);
    assert_eq!(names[1], names[2]);
    assert_ne!(names[0], "Alice");
    assert_eq!(names[3].as_str().unwrap().len(), 6);
    let reconnect = &decoded.iter().find(|p| p.0 == 45).unwrap().1;
    assert_eq!(reconnect["key"], "00000000");
    assert_eq!(reconnect["name"], "Nexus");
    assert_eq!(reconnect["host"], "10.2.3.4");
    assert_eq!(&rewritten[2].bytes[54..], &rewritten[4].bytes[54..]);
    assert_eq!(&rewritten[5].bytes[54..59], &rewritten[2].bytes[73..78]);
    for (a, b) in original.iter().zip(&rewritten) {
        assert_eq!(a.at, b.at);
        assert_eq!(a.original_len, b.original_len);
        assert_eq!(&a.bytes[34..50], &b.bytes[34..50]);
        assert_ne!(&a.bytes[..12], &b.bytes[..12]);
        assert_ne!(&a.bytes[26..34], &b.bytes[26..34]);
        assert_eq!(sum(&b.bytes[14..34]), 0);
        let mut pseudo = b.bytes[26..34].to_vec();
        pseudo.extend_from_slice(&[0, 6]);
        pseudo.extend_from_slice(&((b.bytes.len() - 34) as u16).to_be_bytes());
        pseudo.extend_from_slice(&b.bytes[34..]);
        assert_eq!(sum(&pseudo), 0);
    }
    let metadata = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&fs::read(metadata).unwrap()).unwrap();
    assert_eq!(meta["verified"], true);
    use sha2::{Digest, Sha256};
    let mut schema = Sha256::new();
    for source in [
        include_bytes!("../../maxed-protocol/src/defs.rs").as_slice(),
        include_bytes!("../../maxed-protocol/src/reader.rs"),
        include_bytes!("../../maxed-protocol/src/privacy.rs"),
        include_bytes!("../../maxed-protocol/src/notices.rs"),
        include_bytes!("../src/scrub.rs"),
    ] {
        schema.update(source);
    }
    assert_eq!(
        meta["schemaProfileSha256"],
        maxed_protocol::lower_hex(&schema.finalize())
    );
    // The pre-#55 profile used non-alphanumeric item boundaries.
    assert_ne!(
        meta["schemaProfileSha256"],
        "486ec7246e1fbfdcac99158025ab15e168d0050a74e5fcbcd8d637e5f9c20886"
    );
}
fn sum(b: &[u8]) -> u16 {
    let mut s: u32 = b
        .chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])))
        .sum();
    while s >> 16 != 0 {
        s = (s & 65535) + (s >> 16);
    }
    !(s as u16)
}
#[test]
fn trailing_packet_bytes_still_refuse_output() {
    for (id, body) in [
        (105, vec![1]),
        (74, {
            let mut b = hello();
            b.push(1);
            b
        }),
    ] {
        let wire = encrypted(C, &[(id, body)]);
        assert_refused(&[frame(0, C, 100, true, b""), frame(1, C, 101, false, &wire)]);
    }
}
#[test]
fn alignment_gaps_truncation_conflicting_overlap_and_partial_tail_refuse() {
    let wire = encrypted(C, &[(74, hello())]);
    assert_refused(&[frame(0, C, 101, false, &wire)]);
    assert_refused(&[
        frame(0, C, 100, true, b""),
        frame(1, C, 102, false, &wire[1..]),
    ]);
    assert_refused(&[
        frame(0, C, 100, true, b""),
        frame(1, C, 101, false, &wire[..wire.len() - 1]),
    ]);
    let mut conflict = wire.clone();
    conflict[22] ^= 1;
    assert_refused(&[
        frame(0, C, 100, true, b""),
        frame(1, C, 101, false, &wire),
        frame(2, C, 101, false, &conflict),
    ]);
    let mut truncated = frame(1, C, 101, false, &wire);
    truncated.original_len += 1;
    assert_refused(&[frame(0, C, 100, true, b""), truncated]);
}
#[test]
fn input_output_aliases_existing_outputs_and_empty_captures_refuse() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    write(&input, &happy());
    let before = fs::read(&input).unwrap();
    assert!(scrub(&input, &input).is_err());
    assert_eq!(fs::read(&input).unwrap(), before);
    let output = dir.write("output.pcap", b"sentinel");
    assert!(scrub(&input, &output).is_err());
    assert_eq!(fs::read(output).unwrap(), b"sentinel");
    assert_refused(&[]);
}
#[test]
fn unproven_dialogue_and_opaque_strings_are_filled_and_game_stats_kept() {
    for (id, body, field) in [
        (44, text_body(b"#Oryx", b"Alice defeated me"), "text"),
        (
            101,
            {
                let mut b = vec![0; 8];
                string(&mut b, b"secret");
                b
            },
            "str",
        ),
        (
            0,
            {
                let mut b = vec![0; 4];
                string(&mut b, b"secret");
                b
            },
            "errorDescription",
        ),
        (
            67,
            {
                let mut b = vec![1, 0];
                string(&mut b, b"secret");
                b
            },
            "message",
        ),
    ] {
        let (report, decoded) = scrub_packets(S, &[(id, body)]);
        assert!(report
            .diagnostics
            .fields
            .iter()
            .any(|(&(packet, _, name, treatment), &count)| packet == id
                && name == field
                && treatment == "filled-unclassified"
                && count == 1));
        assert!(decoded[0].1[field]
            .as_str()
            .unwrap()
            .bytes()
            .all(|b| b == b'x'));
    }
    for id in [6, 71, 72, 80, 121, 127, 128, 147, 155] {
        let mut b = tick(b"game-data");
        b[26] = id;
        let (_, decoded) = scrub_packets(S, &[(10, b)]);
        assert_eq!(
            decoded[0].1["status"][0]["stats"][0]["stringStatValue"],
            "game-data"
        );
    }
}
#[test]
fn all_credential_and_identity_spans_exclude_length_prefixes() {
    let b = hello();
    let spans = sensitive_spans(74, C, &b).unwrap();
    assert_eq!(spans.len(), 5);
    assert!(spans.iter().all(|s| s.kind == SecretKind::Credential));
    assert_eq!(
        &b[spans[0].offset..spans[0].offset + spans[0].len],
        b"private-access-token"
    );
    for id in [31, 62, 82] {
        let mut b = update(b"Identity");
        b[23] = id;
        let spans = sensitive_spans(42, S, &b).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, SecretKind::Identity);
    }
}
#[test]
fn fixed_width_namespace_exhaustion_refuses() {
    let packets: Vec<_> = b"0123456789abcdefghijklmnopqrstuvwxyz"
        .iter()
        .map(|b| (42, update(&[*b])))
        .collect();
    let wire = encrypted(S, &packets);
    assert_refused(&[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)]);
}
#[test]
fn reordered_segments_and_sequence_wrap_keep_the_same_plaintext() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let wire = encrypted(C, &[(74, hello())]);
    let syn = u32::MAX - 12;
    let frames = [
        frame(0, C, syn, true, b""),
        frame(1, C, syn.wrapping_add(25), false, &wire[24..]),
        frame(2, C, syn.wrapping_add(1), false, &wire[..30]),
    ];
    write(&input, &frames);
    assert!(scrub(&input, &output).unwrap().verified);
    assert_eq!(fields(read(&output)).len(), 1);
}
#[test]
fn new_connection_and_nonoverlapping_tuple_reuse_reset_both_ciphers() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let wire = encrypted(C, &[(74, hello())]);
    let frames = [
        frame(0, C, 100, true, b""),
        frame(1, C, 101, false, &wire),
        frame(2, C, 5000, true, b""),
        frame(3, C, 5001, false, &wire),
    ];
    write(&input, &frames);
    assert!(scrub(&input, &output).unwrap().verified);
    assert_eq!(fields(read(&output)).len(), 2);
}
#[test]
fn reused_tuple_ambiguous_old_sequence_ranges_refuse() {
    let wire = encrypted(C, &[(74, hello())]);
    assert_refused(&[
        frame(0, C, 100, true, b""),
        frame(1, C, 101, false, &wire),
        frame(2, C, 110, true, b""),
        frame(3, C, 111, false, &wire),
    ]);
}
#[test]
fn headers_padding_options_and_non_game_frames_are_not_unexamined_carriers() {
    for change in 0..5 {
        let mut frames = happy();
        let b = &mut frames[0].bytes;
        match change {
            0 => b[12] = 0x86,
            1 => b[20] = 0x20,
            2 => {
                b.push(77);
                frames[0].original_len += 1;
            }
            3 => b[14] = 0x46,
            _ => b[35] = 12,
        };
        assert_refused(&frames);
    }
}
#[test]
fn scrub_cli_is_dispatched_and_diagnostics_do_not_echo_values() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    write(&input, &happy());
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .arg("scrub")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    for marker in [
        "verified=true",
        "provisional=false",
        "filled_unclassified=0",
        "pseudonymized=",
        "kept_allowlisted=",
        "field=NAME_STAT",
    ] {
        assert!(stdout.contains(marker), "missing report marker: {marker}");
    }
    assert!(!String::from_utf8_lossy(&result.stdout).contains("Alice"));
}
#[test]
fn unknown_numeric_stat_is_safe_but_unknown_notification_layout_refuses() {
    let mut b = vec![0; 14];
    b.extend_from_slice(&1u16.to_be_bytes());
    b.push(7);
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&[1, 255, 1, 0]);
    assert!(maxed_protocol::decode_packet(10, S, &b).is_ok());
    assert!(sensitive_spans(10, S, &b).unwrap().is_empty());
    let (_, out) = scrub_packets(S, &[(10, b)]);
    assert_eq!(out[0].1["status"][0]["stats"][0]["statValue"], 1);
    let mut b = vec![11, 4];
    b.extend_from_slice(&[0; 6]);
    assert!(maxed_protocol::decode_packet(67, S, &b).is_ok());
    assert!(sensitive_spans(67, S, &b).is_err());
    let temp = PrivateDir::new();
    let input = temp.file("input.pcap");
    let output = temp.file("output.pcap");
    let wire = encrypted(S, &[(67, b)]);
    write(
        &input,
        &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
    );
    let error =
        maxed_cli::scrub::scrub_with_options(&input, &output, Default::default()).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("NOTIFICATION id=67 s2c field=extra"));
    assert!(message
        .contains("unknown ProgressBar extra flag bits; conditional payload extent unproven"));
    assert!(!output.exists());
}
#[test]
fn aliases_are_sorted_deterministic_and_avoid_original_fixed_width_codes() {
    let mut mappings = vec![];
    for names in [
        [b"Alice", b"00000", b"Aaron"],
        [b"Aaron", b"Alice", b"00000"],
    ] {
        let dir = PrivateDir::new();
        let input = dir.file("input.pcap");
        let output = dir.file("output.pcap");
        let packets: Vec<_> = names.iter().map(|n| (42, update(n.as_slice()))).collect();
        let wire = encrypted(S, &packets);
        write(
            &input,
            &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
        );
        assert!(scrub(&input, &output).unwrap().verified);
        let mut mapping = std::collections::BTreeMap::new();
        for (source, (_, packet)) in names.iter().zip(fields(read(&output))) {
            let alias = packet["newObjects"][0]["status"]["stats"][0]["stringStatValue"]
                .as_str()
                .unwrap()
                .to_owned();
            assert!(!names.iter().any(|n| n.as_slice() == alias.as_bytes()));
            mapping.insert(source.to_vec(), alias);
        }
        mappings.push(mapping);
    }
    assert_eq!(mappings[0], mappings[1]);
    assert_eq!(mappings[0][b"00000".as_slice()], "00001");
    assert_eq!(mappings[0][b"Aaron".as_slice()], "00002");
    assert_eq!(mappings[0][b"Alice".as_slice()], "00003");
}
#[test]
fn credentials_colliding_with_even_exact_public_literals_refuse() {
    let body = custom_hello(b"7.0.0", b"7.0.0", b"Unity", b"Steam");
    let error = scrub_failure(C, &[(74, body)]);
    assert!(error
        .to_string()
        .contains("field=buildVersion secret-class=credential"));
    assert!(!error.to_string().contains("7.0.0"));
}

#[test]
fn unambiguous_retired_generation_retransmit_is_scrubbed_too() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let wire = encrypted(C, &[(74, hello())]);
    let frames = [
        frame(0, C, 100, true, b""),
        frame(1, C, 101, false, &wire),
        frame(2, C, 5000, true, b""),
        frame(3, C, 5001, false, &wire),
        frame(4, C, 101, false, &wire),
    ];
    write(&input, &frames);
    assert!(scrub(&input, &output).unwrap().verified);
    let out = read(&output);
    assert_eq!(&out[1].bytes[54..], &out[4].bytes[54..]);
    assert_eq!(fields(out).len(), 2);
    let mut conflicting = frames.clone();
    conflicting[4].bytes[76] ^= 1;
    assert_refused(&conflicting);
}
#[test]
fn policy_unknown_move_and_typed_failures_are_explicit_and_safe() {
    use maxed_cli::scrub::{scrub_with_options, ScrubOptions};
    let dir = PrivateDir::new();
    let input = dir.file("policy.pcap");
    let output = dir.file("output.pcap");
    let packets = vec![
        (250, b"private-access-token Alice".to_vec()),
        (42, vec![1, 2]),
        (67, vec![127, 0]),
        (67, vec![127, 0]),
    ];
    let wire = encrypted(S, &packets);
    write(
        &input,
        &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
    );
    let error = scrub_with_options(&input, &output, ScrubOptions::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("refused:"));
    assert!(error.contains("1 packets outside"));
    assert!(error.contains("3 typed packets failed"));
    assert!(error.contains("UPDATE id=42 s2c x1"));
    assert!(error.contains("NOTIFICATION id=67 s2c x2"));
    assert!(error.contains("--allow-undecodable"));
    for secret in ["Alice", "private-access-token"] {
        assert!(!error.contains(secret));
    }
    assert!(!output.exists());
    let report = scrub_with_options(
        &input,
        &output,
        ScrubOptions {
            allow_undecodable: true,
        },
    )
    .unwrap();
    assert!(report.verified);
    let after = read(&output);
    let mut plain = after[1].bytes[54..].to_vec();
    let mut rc4 = Rc4::new(&key_bytes(S).unwrap()).unwrap();
    let mut offset = 0;
    for (_, body) in packets {
        rc4.apply(&mut plain[offset + 5..offset + 5 + body.len()]);
        assert!(plain[offset + 5..offset + 5 + body.len()]
            .iter()
            .all(|&b| b == 0xff));
        offset += 5 + body.len();
    }
}
#[test]
fn refusal_cli_exits_nonzero_without_printing_wire_values() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let wire = encrypted(
        S,
        &[
            (42, vec![1, 2]),
            (44, {
                let mut b = text_body(b"#Oryx", b"Alice private-access-token");
                b.push(0);
                b
            }),
        ],
    );
    write(
        &input,
        &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
    );
    for extra in [vec![], vec!["--allow-undecodable"]] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
            .arg("scrub")
            .arg(&input)
            .arg(&output)
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(error.contains("trailing bytes"));
        assert!(error.contains("TEXT id=44"));
        for secret in ["Alice", "private-access-token", "#Oryx"] {
            assert!(!error.contains(secret));
        }
        assert!(!output.exists());
    }
}
#[test]
fn unknown_bodies_and_move_headers_preserve_canonical_shape_and_recovery() {
    use maxed_cli::scrub::{scrub_with_options, ScrubOptions, ScrubReason};
    let dir = PrivateDir::new();
    let input = dir.file("moves.pcap");
    let output = dir.file("output.pcap");
    let moves: Vec<_> = (0i32..3)
        .map(|tick| {
            let mut b = tick.to_be_bytes().to_vec();
            b.extend_from_slice(&1234i32.to_be_bytes());
            b.extend_from_slice(&0i16.to_be_bytes());
            b.extend_from_slice(b"Alice secret-token");
            (62, b)
        })
        .collect();
    let mut packets = vec![
        (250, b"Alice private-access-token".to_vec()),
        // Deliberately wrong-direction PING: its length-only packet and the
        // informational WrongDirection diagnostic must both survive scrubbing.
        (8, b"secret ping".to_vec()),
    ];
    packets.extend(moves);
    packets.push((105, vec![]));
    let wire = encrypted(C, &packets);
    let frames = vec![frame(0, C, 100, true, b""), frame(1, C, 101, false, &wire)];
    write(&input, &frames);
    let before = maxed_cli::replay::replay(&input).unwrap();
    assert_eq!(before.decoded, packets.len() as u64);
    assert_eq!(before.unknown, 5);
    assert_eq!(before.unknown_id_packets, 1);
    assert_eq!(before.wrong_direction_packets, 1);
    assert_eq!(before.failures, 0);
    let report = scrub_with_options(&input, &output, ScrubOptions::default()).unwrap();
    assert_eq!(report.diagnostics.count(ScrubReason::OutsideSchema), 5);
    let out = read(&output);
    assert_eq!(fields(frames), fields(out.clone()));
    let mut rc4 = Rc4::new(&key_bytes(C).unwrap()).unwrap();
    let mut recovery = maxed_protocol::Alignment::new(C, false).unwrap();
    let mut offset = 54;
    for (id, original) in &packets {
        let cipher = &out[1].bytes[offset + 5..offset + 5 + original.len()];
        let mut plain = cipher.to_vec();
        rc4.apply(&mut plain);
        let prefix = if *id == 62 { 4 } else { 0 };
        if prefix > 0 {
            assert_eq!(&plain[..4], &original[..4]);
        }
        assert!(plain[prefix..].iter().all(|&b| b == 0xff));
        if *id == 62 {
            let recovered = recovery.consume(*id, cipher).unwrap();
            if original[..4] == 2i32.to_be_bytes() {
                assert_eq!(recovered.unwrap(), plain);
            } else {
                assert!(recovered.is_none());
            }
        }
        offset += 5 + original.len();
    }
    assert!(recovery.is_synced());
    assert_eq!(maxed_cli::replay::replay(&output).unwrap(), before);
}
#[test]
fn failed_newtick_keeps_alignment_header_with_explicit_opt_in() {
    use maxed_cli::scrub::{scrub_with_options, ScrubOptions};
    let dir = PrivateDir::new();
    let input = dir.file("ticks.pcap");
    let output = dir.file("output.pcap");
    let broken = vec![0, 0, 0, 0, 9];
    let mut valid = tick(b"Alice");
    valid[..4].copy_from_slice(&1i32.to_be_bytes());
    let wire = encrypted(S, &[(10, broken.clone()), (10, valid)]);
    write(
        &input,
        &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
    );
    assert!(
        scrub_with_options(
            &input,
            &output,
            ScrubOptions {
                allow_undecodable: true
            }
        )
        .unwrap()
        .verified
    );
    let out = read(&output);
    let mut b = out[1].bytes[59..64].to_vec();
    Rc4::new(&key_bytes(S).unwrap()).unwrap().apply(&mut b);
    assert_eq!(&b[..4], &broken[..4]);
    assert_eq!(b[4], 0xff);
    assert_eq!(fields(out).len(), 1);
}
#[test]
fn allow_flag_cli_reports_provisional_fixture_and_failures_always_exit_nonzero() {
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let wire = encrypted(S, &[(42, vec![1, 2])]);
    write(
        &input,
        &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
    );
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
            .arg("scrub")
            .arg(&input)
            .arg(&output)
            .args(extra)
            .output()
            .unwrap()
    };
    assert_eq!(run(&[]).status.code(), Some(2));
    let accepted = run(&["--allow-undecodable"]);
    assert_eq!(accepted.status.code(), Some(0));
    let stdout = String::from_utf8(accepted.stdout).unwrap();
    assert!(stdout.contains("1 typed packets failed"));
    assert!(stdout.contains("Provisional fixture"));
    assert!(stdout.contains("filled packets cannot validate decoding"));
    assert_eq!(run(&["--allow-undecodable"]).status.code(), Some(2));
    assert_eq!(run(&["--unknown-flag"]).status.code(), Some(2));
    let meta = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&fs::read(meta).unwrap()).unwrap();
    assert_eq!(meta["provisional"], true);
    assert_eq!(meta["remakeAfterP1UpdateLayoutFix"], true);
}
#[test]
fn midstream_and_unsupported_envelope_reports_are_counted_and_honest() {
    use maxed_cli::scrub::{scrub_with_options, ScrubOptions};
    let dir = PrivateDir::new();
    let input = dir.file("input.pcap");
    let output = dir.file("output.pcap");
    let wire = encrypted(C, &[(74, hello())]);
    let mut bad = frame(2, C, 400, false, b"secret");
    bad.bytes[14] = 0x46;
    write(
        &input,
        &[
            frame(0, C, 100, false, &wire),
            frame(1, S, 200, false, &wire),
            bad.clone(),
            bad,
        ],
    );
    let error = scrub_with_options(
        &input,
        &output,
        ScrubOptions {
            allow_undecodable: true,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("2 directions missing"));
    assert!(error.contains("2 unsupported"));
    assert!(error.contains("scan incomplete"));
    assert!(!error.contains("secret"));
    assert!(!output.exists());
}

fn scrub_packets(
    dir: Direction,
    packets: &[(u8, Vec<u8>)],
) -> (maxed_cli::scrub::ScrubReport, Vec<(u8, serde_json::Value)>) {
    let temp = PrivateDir::new();
    let input = temp.file("input.pcap");
    let output = temp.file("output.pcap");
    let wire = encrypted(dir, packets);
    // Realistic TCP payload sizes keep large synthetic policy matrices out of
    // the ordinary decoder's oversized-first-segment recovery gate.
    let mut frames = vec![frame(0, dir, 100, true, b"")];
    for (i, chunk) in wire.chunks(1000).enumerate() {
        frames.push(frame(
            i as u64 + 1,
            dir,
            101 + (i * 1000) as u32,
            false,
            chunk,
        ));
    }
    write(&input, &frames);
    let report = scrub(&input, &output).unwrap();
    assert!(report.verified);
    (report, fields(read(&output)))
}

#[test]
fn every_string_stat_policy_replays_and_reports_without_values() {
    use maxed_protocol::privacy::{FieldPolicy, STRING_STATS};
    // Independent ID set from RS StatData.isStringStat; missing/duplicate rows fail.
    let ids: Vec<_> = STRING_STATS.iter().map(|s| s.0).collect();
    assert_eq!(
        ids,
        [6, 31, 38, 54, 62, 71, 72, 80, 82, 115, 121, 127, 128, 147, 155]
    );
    for &(id, name, policy) in STRING_STATS {
        let mut body = update(b"private-value");
        body[23] = id;
        let (report, out) = scrub_packets(S, &[(42, body)]);
        let value = out[0].1["newObjects"][0]["status"]["stats"][0]["stringStatValue"]
            .as_str()
            .unwrap();
        match policy {
            FieldPolicy::Keep => assert_eq!(value, "private-value"),
            FieldPolicy::Pseudonym => {
                assert_ne!(value, "private-value");
                assert_eq!(value.len(), 13);
            }
            FieldPolicy::Fill => assert_eq!(value, "xxxxxxxxxxxxx"),
            FieldPolicy::FillUnclassified => {
                panic!("all existing string stats must have an explicit policy")
            }
        }
        assert!(report.diagnostics.fields.keys().any(|key| key.2 == name));
    }
}

#[test]
fn map_npc_dialogue_and_failure_templates_survive_verification() {
    let mut map = vec![0; 8];
    for value in ["Moonlight Village", "Moonlight Village", "Medusa"] {
        string(&mut map, value.as_bytes());
    }
    map.extend_from_slice(&[0; 21]);
    string(&mut map, b"7.0.0");
    map.extend_from_slice(&[0; 9]);
    string(&mut map, b"mod1;mod2;mod3;mod4;|S");
    map.extend_from_slice(&[0; 2]);
    let dialogue = text_body(
        b"#Kitsune Umi",
        b"This fully concludes the Moonlight Festival!",
    );
    let mut failure = vec![0; 4];
    string(&mut failure, b"Character not found");
    let (_, out) = scrub_packets(S, &[(92, map), (44, dialogue), (0, failure)]);
    assert_eq!(out[0].1["name"], "Moonlight Village");
    assert_eq!(out[0].1["displayName"], "Moonlight Village");
    assert_eq!(out[0].1["realmName"], "Medusa");
    assert_eq!(out[0].1["versionNumber"], "7.0.0");
    assert_eq!(out[0].1["dungeonModifiers"], "mod1");
    assert_eq!(out[0].1["dungeonModifiers4"], "mod4");
    assert_eq!(out[0].1["dungeonGrade"], "S");
    assert_eq!(out[1].1["name"], "#Kitsune Umi");
    assert_eq!(
        out[1].1["text"],
        "This fully concludes the Moonlight Festival!"
    );
    assert_eq!(out[1].1["cleanText"], out[1].1["text"]);
    assert_eq!(out[2].1["errorDescription"], "Character not found");
}

fn custom_hello(version: &[u8], token: &[u8], user: &[u8], play: &[u8]) -> Vec<u8> {
    let mut body = vec![0; 4];
    string(&mut body, version);
    string(&mut body, token);
    body.extend_from_slice(&[0; 4]);
    string(&mut body, b"");
    string(&mut body, user);
    string(&mut body, play);
    for _ in 0..3 {
        string(&mut body, b"");
    }
    body
}

#[test]
fn hello_formats_and_short_secret_and_replacement_collisions() {
    for (version, keep) in [
        ("8.12.3.45", true),
        ("8.12.token", false),
        ("8..2", false),
        (".8.2", false),
        ("8.2.", false),
    ] {
        let (_, out) = scrub_packets(
            C,
            &[(
                74,
                custom_hello(version.as_bytes(), b"xxx", b"Unity", b"Steam"),
            )],
        );
        assert_eq!(
            out[0].1["buildVersion"],
            if keep {
                version.to_owned()
            } else {
                "x".repeat(version.len())
            }
        );
        assert_eq!(out[0].1["accessToken"], "xxx");
    }
    let (_, out) = scrub_packets(
        C,
        &[(
            74,
            custom_hello(b"7.0.0", b"7", b"custom-user", b"custom-play"),
        )],
    );
    assert_eq!(out[0].1["userPlatform"], "xxxxxxxxxxx");
    assert_eq!(out[0].1["playPlatform"], "xxxxxxxxxxx");
    // A one-byte identity is a substring of the public destination, not a leak.
    let (_, out) = scrub_packets(S, &[(42, update(b"N")), (45, reconnect())]);
    assert_eq!(out[1].1["name"], "Nexus");
}

#[test]
fn unexplained_collisions_have_a_distinct_reason() {
    use maxed_cli::scrub::{scrub_with_options, ScrubOptions, ScrubReason};
    for name in [b"Nexus".as_slice(), b"10"] {
        let mut r = reconnect();
        if name == b"10" {
            r = vec![];
            string(&mut r, b"10");
            string(&mut r, b"10.2.3.4");
            r.extend_from_slice(&[0; 10]);
            string(&mut r, b"");
        }
        let temp = PrivateDir::new();
        let input = temp.file("input.pcap");
        let output = temp.file("output.pcap");
        let wire = encrypted(S, &[(42, update(name)), (45, r)]);
        write(
            &input,
            &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
        );
        let error = scrub_with_options(&input, &output, ScrubOptions::default()).unwrap_err();
        assert_eq!(error.diagnostics.count(ScrubReason::ResidualCollision), 1);
        assert!(error
            .to_string()
            .contains("unexplained secret-value collisions"));
        assert!(!output.exists());
    }
}

#[test]
fn death_identity_credentials_and_opaque_fields_are_filled() {
    let mut body = vec![];
    string(&mut body, b"account-private");
    body.push(1); // charId
    string(&mut body, b"killer-private");
    body.extend_from_slice(&[0; 4]);
    body.push(0); // totalFame
    body.push(1); // fameData count
    string(&mut body, b"achievement-private");
    body.extend_from_slice(&[1, 1]);
    string(&mut body, b"stats-private");
    let (report, out) = scrub_packets(S, &[(46, body)]);
    assert_eq!(out[0].1["accountId"], "xxxxxxxxxxxxxxx");
    assert_eq!(out[0].1["charId"], 1);
    assert_eq!(out[0].1["killedBy"], "xxxxxxxxxxxxxx");
    assert_eq!(
        out[0].1["fameData"][0]["achievement"],
        "xxxxxxxxxxxxxxxxxxx"
    );
    assert_eq!(out[0].1["stats"], "xxxxxxxxxxxxx");
    assert_eq!(report.diagnostics.field_count("filled-unclassified"), 3);
}

#[test]
fn malformed_utf8_unclassified_text_is_filled_without_losing_wire_length() {
    let mut body = vec![0; 8];
    string(&mut body, &[0xff, 0xfe]);
    let (report, out) = scrub_packets(S, &[(101, body)]);
    assert_eq!(out[0].1["str"], "xx");
    assert_eq!(report.diagnostics.field_count("filled-unclassified"), 1);
}

#[test]
fn all_unmapped_stat_ids_keep_numeric_values_in_update_and_newtick() {
    use maxed_protocol::privacy::EvidenceReason;
    let mut packets = vec![];
    let mut tick_id = 0i32;
    for stat_id in (0..=255).filter(|id| maxed_protocol::defs::stat_name(*id).is_none()) {
        for packet_id in [42, 10] {
            let mut status = vec![7];
            status.extend_from_slice(&[0; 8]);
            status.extend_from_slice(&[2, stat_id, 0x81, 0x02, 0x41, 31]);
            string(&mut status, b"Alice");
            status.push(0);
            let mut body = if packet_id == 10 {
                let mut b = vec![0; 14];
                b.extend_from_slice(&1u16.to_be_bytes());
                b
            } else {
                let mut b = vec![0; 8];
                b.extend_from_slice(&[0, 0, 1, 3, 0]);
                b
            };
            if packet_id == 10 {
                body[..4].copy_from_slice(&tick_id.to_be_bytes());
                tick_id += 1;
            }
            body.extend(status);
            if packet_id == 42 {
                body.push(0);
            }
            packets.push((packet_id, body));
        }
    }
    let (report, out) = scrub_packets(S, &packets);
    assert_eq!(report.diagnostics.field_evidence.len(), packets.len());
    for ((packet_id, _, evidence), count) in &report.diagnostics.field_evidence {
        assert!(matches!(packet_id, 42 | 10));
        assert!(evidence.stat_id.is_some());
        assert_eq!(evidence.field, "statTypeNum");
        assert_eq!(evidence.reason, EvidenceReason::UnmappedNumericStat);
        assert_eq!(*count, 1);
    }
    for (id, value) in out {
        let stats = if id == 10 {
            &value["status"][0]["stats"]
        } else {
            &value["newObjects"][0]["status"]["stats"]
        };
        assert_eq!(stats[0]["statValue"], 129);
        assert_eq!(stats[0]["statValueTwo"], -1);
        assert_eq!(stats[0]["statType"], serde_json::Value::Null);
        assert_eq!(stats[1]["stringStatValue"], "00000");
    }
    let message = report.diagnostics.to_string();
    assert!(message.contains("statTypeNum=255 reason=unmapped statTypeNum; compressed-integer primary/secondary values kept"));
    assert!(!message.contains("Alice"));
    assert!(!message.contains("129")); // numeric values never enter diagnostics
}

fn six_bit_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        result.push(ALPHABET[usize::from(a >> 2)] as char);
        result.push(ALPHABET[usize::from((a & 3) << 4 | b >> 4)] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[usize::from((b & 15) << 2 | c >> 6)] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[usize::from(c & 63)] as char
        } else {
            '='
        });
    }
    result
}
fn create_stats(value: &str) -> Vec<u8> {
    let mut body = vec![0; 8];
    string(&mut body, value.as_bytes());
    body
}
fn stat_snapshot(ids: &[u8], values: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1, 0, 0, 0];
    let mut mask = [0u8; 16];
    for id in ids {
        mask[usize::from(id / 8)] |= 1 << (id % 8);
    }
    bytes.extend(mask);
    bytes.extend(values);
    bytes
}

#[test]
fn create_success_keeps_only_fully_decoded_character_counter_snapshots() {
    let valid = [
        six_bit_encode(&[0; 4]),
        six_bit_encode(&stat_snapshot(&[], &[])),
        // RS order: shots fired (24), Void completions (41), Moonlight (71).
        six_bit_encode(&stat_snapshot(&[24, 41, 71], &[1, 0x81, 0x02, 0x41])),
        six_bit_encode(&stat_snapshot(&[24], &[0xbf, 0xff, 0xff, 0xff, 0x0f])), // i32::MAX
    ];
    let mut invalid = vec![
        "secret-player-text".to_owned(),
        "AAAAAB==".to_owned(),
        "AAAAAA".to_owned(),
        "AA==AAAA".to_owned(),
        "AAAAAA==junk".to_owned(),
        "AAAA+A==".to_owned(),
        six_bit_encode(&[0; 5]),                    // bytes after zero flag
        six_bit_encode(&[1; 4]),                    // missing presence map
        six_bit_encode(&stat_snapshot(&[24], &[])), // missing counter
        six_bit_encode(&stat_snapshot(&[24], &[0x80])), // truncated compressed integer
        six_bit_encode(&stat_snapshot(&[24], &[0x80, 0x80, 0x80, 0x80, 0x80, 0])),
        six_bit_encode(&stat_snapshot(&[24], &[0xbf, 0xff, 0xff, 0xff, 0x7f])), // overflow
        six_bit_encode(&stat_snapshot(&[24], b"Alice")), // trailing decoded bytes
    ];
    // A padding-bit alias decodes to the same bytes but is not a strict encoding.
    invalid.push("AAAAAA=A".to_owned());
    for value in &valid {
        let (report, out) = scrub_packets(S, &[(101, create_stats(value))]);
        assert_eq!(out[0].1["str"], *value);
        assert_eq!(report.diagnostics.field_count("kept-allowlisted"), 1);
    }
    for value in invalid {
        let (report, out) = scrub_packets(S, &[(101, create_stats(&value))]);
        assert_eq!(out[0].1["str"], "x".repeat(value.len()));
        assert_eq!(report.diagnostics.field_count("filled-unclassified"), 1);
    }
    // Independent coverage of every presence bit, including RS's five unused bits.
    let snapshots: Vec<_> = (0..128)
        .map(|id| six_bit_encode(&stat_snapshot(&[id], &[3])))
        .collect();
    let packets: Vec<_> = snapshots.iter().map(|s| (101, create_stats(s))).collect();
    let (_, out) = scrub_packets(S, &packets);
    for (id, ((_, packet), original)) in out.iter().zip(&snapshots).enumerate() {
        if [11, 54, 56, 57, 73].contains(&id) {
            assert_eq!(packet["str"], "x".repeat(original.len()));
        } else {
            assert_eq!(packet["str"], *original);
        }
    }
}

fn map_named(name: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 8];
    for value in [name, b"", b""] {
        string(&mut b, value);
    }
    b.extend_from_slice(&[0; 21]);
    string(&mut b, b"");
    b.extend_from_slice(&[0; 9]);
    string(&mut b, b"");
    b.extend_from_slice(&[0; 2]);
    b
}
fn scrub_failure(dir: Direction, packets: &[(u8, Vec<u8>)]) -> maxed_cli::scrub::ScrubFailure {
    let temp = PrivateDir::new();
    let input = temp.file("input.pcap");
    let output = temp.file("output.pcap");
    let wire = encrypted(dir, packets);
    write(
        &input,
        &[
            frame(0, dir, 100, true, b""),
            frame(1, dir, 101, false, &wire),
        ],
    );
    let error =
        maxed_cli::scrub::scrub_with_options(&input, &output, Default::default()).unwrap_err();
    assert!(!output.exists());
    error
}

#[test]
fn player_text_map_coincidences_are_informational_and_metadata_names_the_class() {
    use maxed_cli::scrub::{ScrubReason, SecretClass};
    let mut notification = vec![1, 0];
    string(&mut notification, b"Nexus");
    for source in [
        (44, text_body(b"Alice", b"Nexus")),
        (67, notification),
        (101, create_stats("Nexus")),
    ] {
        let temp = PrivateDir::new();
        let input = temp.file("input.pcap");
        let output = temp.file("output.pcap");
        let wire = encrypted(S, &[source, (92, map_named(b"Nexus"))]);
        write(
            &input,
            &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
        );
        let report = scrub(&input, &output).unwrap();
        assert!(report.verified);
        assert_eq!(
            report
                .diagnostics
                .count(ScrubReason::PublicLiteralCoincidence),
            1
        );
        assert_eq!(report.diagnostics.count(ScrubReason::ResidualCollision), 0);
        let collision = report.diagnostics.collisions.keys().next().unwrap();
        assert_eq!(collision.secret_class, SecretClass::PlayerText);
        assert_eq!(collision.field, "name");
        let message = report.diagnostics.to_string();
        assert!(message.contains("public-literal coincidence"));
        assert!(message.contains("MAPINFO id=92 s2c field=name secret-class=player-text"));
        assert!(!message.contains("Nexus"));
        assert!(!message.contains("Alice"));
        let out = fields(read(&output));
        assert_eq!(out[1].1["name"], "Nexus");
        let source_field = match out[0].0 {
            44 => "text",
            67 => "message",
            _ => "str",
        };
        assert_eq!(out[0].1[source_field], "xxxxx");
        let metadata = fs::read_dir(&temp.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "json"))
            .unwrap();
        let meta: serde_json::Value = serde_json::from_slice(&fs::read(metadata).unwrap()).unwrap();
        assert_eq!(
            meta["coverage"]["collisions"][0]["secretClass"],
            "player-text"
        );
        assert_eq!(
            meta["coverage"]["collisions"][0]["reason"],
            "public-literal coincidence"
        );
        assert_eq!(meta["coverage"]["collisions"][0]["count"], 1);
    }
}

#[test]
fn player_text_coincidences_in_reconnect_and_npc_dialogue_are_informational() {
    use maxed_cli::scrub::ScrubReason;
    for value in [b"Nexus".as_slice(), b"10.2.3.4"] {
        let (report, out) =
            scrub_packets(S, &[(44, text_body(b"Alice", value)), (45, reconnect())]);
        assert_eq!(
            report
                .diagnostics
                .count(ScrubReason::PublicLiteralCoincidence),
            1
        );
        assert_eq!(out[1].1["name"], "Nexus");
        assert_eq!(out[1].1["host"], "10.2.3.4");
    }
    let line = b"This fully concludes the Moonlight Festival!";
    let (report, out) = scrub_packets(
        S,
        &[
            (44, text_body(b"Alice", line)),
            (44, text_body(b"#Kitsune Umi", line)),
        ],
    );
    assert_eq!(
        report
            .diagnostics
            .count(ScrubReason::PublicLiteralCoincidence),
        2
    );
    assert_eq!(out[1].1["text"], String::from_utf8_lossy(line).as_ref());
    // Long player-text substrings in public data are coincidences too.
    let (report, _) = scrub_packets(
        S,
        &[
            (44, text_body(b"Alice", b"Nexus")),
            (92, map_named(b"The Nexus")),
        ],
    );
    assert_eq!(
        report
            .diagnostics
            .count(ScrubReason::PublicLiteralCoincidence),
        1
    );
}

#[test]
fn identity_and_account_id_collisions_cannot_be_downgraded_by_matching_player_text() {
    use maxed_cli::scrub::{ScrubReason, SecretClass};
    for stat_id in [31, 38, 54, 115] {
        let mut body = update(b"Nexus");
        body[23] = stat_id;
        let error = scrub_failure(
            S,
            &[
                (42, body),
                (44, text_body(b"Alice", b"Nexus")),
                (92, map_named(b"Nexus")),
            ],
        );
        assert_eq!(error.diagnostics.count(ScrubReason::ResidualCollision), 1);
        assert_eq!(
            error
                .diagnostics
                .count(ScrubReason::PublicLiteralCoincidence),
            1
        );
        assert!(error
            .diagnostics
            .collisions
            .keys()
            .any(|c| c.secret_class == SecretClass::Identity
                && c.reason == ScrubReason::ResidualCollision));
        assert!(error
            .to_string()
            .contains("field=name secret-class=identity"));
        assert!(!error.to_string().contains("Nexus"));
    }
    let mut reconnect = vec![];
    string(&mut reconnect, b"Realm");
    string(&mut reconnect, b"game.example");
    reconnect.extend_from_slice(&[0; 10]);
    string(&mut reconnect, b"Nexus");
    let error = scrub_failure(
        S,
        &[
            (45, reconnect),
            (44, text_body(b"Alice", b"Nexus")),
            (92, map_named(b"Nexus")),
        ],
    );
    assert_eq!(error.diagnostics.count(ScrubReason::ResidualCollision), 1);
    assert!(error
        .to_string()
        .contains("field=name secret-class=credential"));
}

fn typed_update(kind: u16, name: &[u8]) -> Vec<u8> {
    let mut body = update(name);
    body[11..13].copy_from_slice(&kind.to_be_bytes());
    body
}
fn packet_name(packet: &(u8, serde_json::Value)) -> &serde_json::Value {
    if packet.0 == 42 {
        &packet.1["newObjects"][0]["status"]["stats"][0]["stringStatValue"]
    } else {
        &packet.1["status"][0]["stats"][0]["stringStatValue"]
    }
}
#[test]
fn nonplayer_names_are_filled_and_map_server_coincidences_are_informational() {
    use maxed_cli::scrub::{ScrubReason, SecretClass};
    let (report, out) = scrub_packets(
        S,
        &[
            (92, map_named(b"Nexus")),
            (42, typed_update(0x0700, b"Nexus")),
            (10, tick(b"Nexus")),
            (45, reconnect()),
        ],
    );
    assert_eq!(packet_name(&out[1]), "xxxxx");
    assert_eq!(packet_name(&out[2]), "xxxxx");
    for id in [42, 10] {
        assert_eq!(
            report
                .diagnostics
                .fields
                .get(&(id, 1, "NAME_STAT", "filled-unclassified")),
            Some(&1)
        );
    }
    assert_eq!(report.diagnostics.count(ScrubReason::ResidualCollision), 0);
    assert_eq!(
        report
            .diagnostics
            .count(ScrubReason::PublicLiteralCoincidence),
        2
    );
    assert!(report
        .diagnostics
        .collisions
        .keys()
        .all(|c| matches!(c.id, 92 | 45)
            && c.field == "name"
            && c.secret_class == SecretClass::PlayerText));
}
#[test]
fn gravestone_names_and_chat_mentions_are_filled_without_public_literal_exemption() {
    use maxed_cli::scrub::ScrubReason;
    // Synthetic non-player grave: NAME_STAT plus GRAVE_ACCOUNT_ID.
    let mut grave = typed_update(0x0700, b"DeadPlayer");
    grave[22] = 2; // Number of stats in the object's status.
    grave.pop(); // Move the UPDATE drops count after the added stat.
    grave.push(115);
    string(&mut grave, b"GraveAccount");
    grave.extend_from_slice(&[0, 0]); // Stat secondary value, then drops count.
    let (report, out) = scrub_packets(
        S,
        &[
            (42, grave),
            (10, tick(b"DeadPlayer")),
            (44, text_body(b"Alice", b"DeadPlayer")),
        ],
    );
    assert_eq!(packet_name(&out[0]), "xxxxxxxxxx");
    assert_eq!(packet_name(&out[1]), "xxxxxxxxxx");
    assert_eq!(
        out[0].1["newObjects"][0]["status"]["stats"][1]["stringStatValue"],
        "xxxxxxxxxxxx"
    );
    assert_eq!(out[2].1["text"], "xxxxxxxxxx");
    assert_eq!(out[2].1["cleanText"], "xxxxxxxxxx");
    assert_eq!(
        report
            .diagnostics
            .count(ScrubReason::PublicLiteralCoincidence),
        0
    );
    assert!(report.diagnostics.collisions.is_empty());
    let rendered = format!("{out:?}{}", report.diagnostics);
    assert!(!rendered.contains("DeadPlayer"));
    assert!(!rendered.contains("GraveAccount"));
}
#[test]
fn every_supplied_player_class_is_pseudonymized_and_map_collisions_refuse() {
    use maxed_protocol::privacy::PLAYER_CLASSES;
    assert_eq!(
        PLAYER_CLASSES
            .iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>(),
        [
            768, 775, 782, 784, 785, 796, 797, 798, 799, 800, 801, 802, 803, 804, 805, 806, 817,
            818, 819
        ]
    );
    for &(kind, _) in PLAYER_CLASSES {
        let (_, out) = scrub_packets(
            S,
            &[(42, typed_update(kind, b"Alice")), (10, tick(b"Alice"))],
        );
        assert_eq!(packet_name(&out[0]), "00000");
        assert_eq!(packet_name(&out[1]), "00000");
    }
    let error = scrub_failure(
        S,
        &[(42, typed_update(819, b"Nexus")), (92, map_named(b"Nexus"))],
    );
    assert!(error.to_string().contains("secret-class=identity"));
}
#[test]
fn unknown_object_names_are_filled_and_not_promoted_by_later_type_evidence() {
    let (report, out) = scrub_packets(
        S,
        &[
            (10, tick(b"Alice")),
            (42, typed_update(768, b"Alice")),
            (92, map_named(b"Nexus")),
        ],
    );
    assert_eq!(packet_name(&out[0]), "xxxxx");
    assert_eq!(packet_name(&out[1]), "00000");
    assert!(report
        .diagnostics
        .to_string()
        .contains("object type unavailable; NAME_STAT filled as player-text"));
    assert_eq!(
        report
            .diagnostics
            .count(maxed_cli::scrub::ScrubReason::ResidualCollision),
        0
    );
}
#[test]
fn create_success_identifies_local_player_before_its_type_is_observed() {
    let mut create = create_stats("AAAAAA==");
    create[..4].copy_from_slice(&7i32.to_be_bytes());
    let (_, out) = scrub_packets(
        S,
        &[
            (101, create),
            (10, tick(b"Alice")),
            (42, typed_update(0x0700, b"Alice")),
        ],
    );
    assert_eq!(packet_name(&out[1]), "00000");
    assert_eq!(packet_name(&out[2]), "00000");
}
#[test]
fn guild_and_account_stats_remain_sensitive_on_nonplayers_and_unknown_objects() {
    for stat_id in [38, 54, 62, 82, 115] {
        let mut update = typed_update(0x0700, b"Secret");
        update[23] = stat_id;
        let mut tick = tick(b"Secret");
        tick[26] = stat_id;
        for packet in [(42, update), (10, tick)] {
            let (_, out) = scrub_packets(S, &[packet]);
            assert_eq!(
                packet_name(&out[0]),
                if matches!(stat_id, 62 | 82) {
                    "000000"
                } else {
                    "xxxxxx"
                }
            );
        }
    }
}
#[test]
fn object_context_is_cleared_by_drops_map_changes_and_connection_generations() {
    let mut drops = vec![0; 8];
    drops.extend_from_slice(&[0, 0, 0, 1, 7]);
    for reset in [(42, drops), (92, map_named(b"Realm")), (45, reconnect())] {
        let (_, out) = scrub_packets(
            S,
            &[
                (42, typed_update(768, b"Alice")),
                reset,
                (10, tick(b"Alice")),
            ],
        );
        assert_eq!(packet_name(&out[2]), "xxxxx");
    }
    let temp = PrivateDir::new();
    let input = temp.file("input.pcap");
    let output = temp.file("output.pcap");
    let first = encrypted(S, &[(42, typed_update(768, b"Alice"))]);
    let second = encrypted(S, &[(10, tick(b"Alice"))]);
    write(
        &input,
        &[
            frame(0, S, 100, true, b""),
            frame(1, S, 101, false, &first),
            frame(2, S, 5000, true, b""),
            frame(3, S, 5001, false, &second),
        ],
    );
    assert!(scrub(&input, &output).unwrap().verified);
    let out = fields(read(&output));
    assert_eq!(packet_name(&out[1]), "xxxxx");
}

#[test]
fn provisional_filled_updates_invalidate_object_type_context() {
    let mut nonfinite = update(b"Other");
    nonfinite[..4].copy_from_slice(&f32::NAN.to_be_bytes());
    for broken in [vec![1, 2], nonfinite] {
        let temp = PrivateDir::new();
        let input = temp.file("input.pcap");
        let output = temp.file("output.pcap");
        let wire = encrypted(
            S,
            &[
                (42, typed_update(768, b"Alice")),
                (42, broken),
                (10, tick(b"Alice")),
            ],
        );
        write(
            &input,
            &[frame(0, S, 100, true, b""), frame(1, S, 101, false, &wire)],
        );
        let report = maxed_cli::scrub::scrub_with_options(
            &input,
            &output,
            maxed_cli::scrub::ScrubOptions {
                allow_undecodable: true,
            },
        )
        .unwrap();
        assert!(report.verified);
        let out = fields(read(&output));
        assert_eq!(packet_name(&out[0]), "00000");
        assert_eq!(packet_name(out.last().unwrap()), "xxxxx");
    }
}

#[test]
fn unlisted_player_range_types_fill_names_in_update_and_newtick() {
    use maxed_protocol::privacy::EvidenceReason;
    for kind in [769, 820, 1023] {
        let (report, out) = scrub_packets(
            S,
            &[
                (42, typed_update(kind, b"FuturePlayer")),
                (10, tick(b"FuturePlayer")),
            ],
        );
        assert_eq!(packet_name(&out[0]), "xxxxxxxxxxxx");
        assert_eq!(packet_name(&out[1]), "xxxxxxxxxxxx");
        for id in [42, 10] {
            assert_eq!(
                report
                    .diagnostics
                    .fields
                    .get(&(id, 1, "NAME_STAT", "filled-unclassified")),
                Some(&1)
            );
            assert!(report
                .diagnostics
                .field_evidence
                .keys()
                .any(|&(packet, _, e)| packet == id
                    && e.stat_id == Some(31)
                    && e.reason == EvidenceReason::UnlistedPlayerRangeType));
        }
        let message = report.diagnostics.to_string();
        assert!(message.contains("unlisted player-range type"));
        assert!(!message.contains("FuturePlayer"));
    }
    // Types outside the range also fill names: no non-player name is allowlisted.
    for kind in [767, 1024] {
        let (_, out) = scrub_packets(
            S,
            &[
                (42, typed_update(kind, b"ObjectName")),
                (10, tick(b"ObjectName")),
            ],
        );
        assert_eq!(packet_name(&out[0]), "xxxxxxxxxx");
        assert_eq!(packet_name(&out[1]), "xxxxxxxxxx");
    }
    // CREATE_SUCCESS positively identifies the local player even for a new class.
    let mut create = create_stats("AAAAAA==");
    create[..4].copy_from_slice(&7i32.to_be_bytes());
    let (_, out) = scrub_packets(
        S,
        &[
            (101, create),
            (42, typed_update(820, b"Alice")),
            (10, tick(b"Alice")),
        ],
    );
    assert_eq!(packet_name(&out[1]), "00000");
    assert_eq!(packet_name(&out[2]), "00000");
}

#[test]
fn oryx_death_and_killed_keys_survive_scrubbing_with_only_killer_redacted() {
    for text in [
        r#"{"k":"stringlist.The_Plague_Doctor.death.0",}"#,
        r#"{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"SyntheticSecret",},}"#,
        r#"{"k":"stringlist.The_Plague_Doctor.death.0"}"#,
        r#"{"k":"stringlist.Cube_Deity.death.0"}{"k":"stringlist.Cube_Deity.death.0"}"#,
        r#"{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"SyntheticSecret"}}"#,
        r#"{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"SyntheticSecret"}}{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"SyntheticSecret"}}"#,
        r#"{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"\u0053yntheticSecret"}}{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"\u0053yntheticSecret"}}"#,
    ] {
        let dir = PrivateDir::new();
        let input = dir.file("synthetic.pcap");
        let output = dir.file("scrubbed.pcap");
        let wire = encrypted(S, &[(44, text_body(b"#Oryx the Mad God", text.as_bytes()))]);
        write(
            &input,
            &[frame(0, S, 100, true, &[]), frame(1, S, 101, false, &wire)],
        );
        scrub(&input, &output).unwrap();
        let rows = fields(read(&output));
        let expected = text
            .replace(
                r"\u0053yntheticSecret",
                &"x".repeat(r"\u0053yntheticSecret".len()),
            )
            .replace("SyntheticSecret", &"x".repeat("SyntheticSecret".len()));
        assert_eq!(rows[0].1["text"], expected);
        assert_eq!(rows[0].1["cleanText"], expected);
        assert!(!rows[0].1.to_string().contains("SyntheticSecret"));
        assert!(maxed_protocol::notices::parse_oryx_announcement(&expected).is_some());
    }
}
#[test]
fn oryx_preserved_key_still_rejects_a_known_identity() {
    let dir = PrivateDir::new();
    let input = dir.file("synthetic.pcap");
    let output = dir.file("scrubbed.pcap");
    let wire = encrypted(
        S,
        &[
            (42, update(b"SyntheticSecret")),
            (
                44,
                text_body(
                    b"#Oryx the Mad God",
                    br#"{"k":"stringlist.SyntheticSecret.death.0"}"#,
                ),
            ),
        ],
    );
    write(
        &input,
        &[frame(0, S, 100, true, &[]), frame(1, S, 101, false, &wire)],
    );
    assert!(scrub(&input, &output).is_err());
}

#[test]
fn oryx_escaped_killer_cannot_hide_a_name_collision_in_a_retained_key() {
    let dir = PrivateDir::new();
    let input = dir.file("synthetic.pcap");
    let output = dir.file("scrubbed.pcap");
    let text =
        br#"{"k":"stringlist.SyntheticSecret.killed.0","t":{"KILLER":"\u0053yntheticSecret"}}"#;
    let wire = encrypted(S, &[(44, text_body(b"#Oryx the Mad God", text))]);
    write(
        &input,
        &[frame(0, S, 100, true, &[]), frame(1, S, 101, false, &wire)],
    );
    assert!(scrub(&input, &output).is_err());
}
#[test]
fn oryx_unknown_fields_and_non_system_senders_stay_filled() {
    for (sender, text) in [
        (
            "#Oryx the Mad God",
            r#"{"k":"stringlist.The_Boss.death.0","extra":"SyntheticSecret"}"#,
        ),
        (
            "#Oryx the Mad God",
            r#"{"k":"stringlist.The_Boss.death.0"}{"k":"stringlist.Other.death.0"}"#,
        ),
        ("#Other", r#"{"k":"stringlist.The_Boss.death.0"}"#),
    ] {
        let dir = PrivateDir::new();
        let input = dir.file("synthetic.pcap");
        let output = dir.file("scrubbed.pcap");
        let wire = encrypted(S, &[(44, text_body(sender.as_bytes(), text.as_bytes()))]);
        write(
            &input,
            &[frame(0, S, 100, true, &[]), frame(1, S, 101, false, &wire)],
        );
        scrub(&input, &output).unwrap();
        assert_eq!(fields(read(&output))[0].1["text"], "x".repeat(text.len()));
    }
}

#[test]
fn unique_data_short_coincidences_are_reported_for_both_secret_classes() {
    for kind in [768, 0x0700] {
        let value = format!("AAIE{}Qzxy{}=", "a".repeat(250), "b".repeat(260));
        let mut data = tick(value.as_bytes());
        data[26] = 80;
        let (report, out) = scrub_packets(S, &[(42, typed_update(kind, b"Qzxy")), (10, data)]);
        assert!(report.verified);
        assert_eq!(packet_name(&out[1]), value.as_str());
        let summary = report.diagnostics.to_string();
        assert!(summary.contains("short opaque token interior"));
        assert!(summary.contains("field=UNIQUE_DATA_STRING"));
        assert_eq!(report.diagnostics.collisions.values().sum::<u64>(), 1);
    }
}

#[test]
fn unique_data_secret_tokens_and_long_substrings_still_refuse() {
    for secret in ["Qzxy", "Qz", "SyntheticSecret"] {
        for value in [secret.to_owned(), format!("AAIE,{secret},AAAA")] {
            let mut data = tick(value.as_bytes());
            data[26] = 80;
            let error = scrub_failure(S, &[(42, typed_update(768, secret.as_bytes())), (10, data)]);
            assert!(error.to_string().contains("secret-class=identity"));
        }
    }
    let mut data = tick(b"AAIESyntheticSecretAAAA");
    data[26] = 80;
    assert!(scrub_failure(
        S,
        &[(42, typed_update(768, b"SyntheticSecret")), (10, data)]
    )
    .to_string()
    .contains("secret-class=identity"));
}

fn unique_data(id: u8, value: &[u8]) -> (u8, Vec<u8>) {
    let mut data = if id == 42 { update(value) } else { tick(value) };
    data[if id == 42 { 23 } else { 26 }] = 80;
    (id, data)
}

#[test]
fn unique_data_base64_short_name_and_chat_interiors_are_ignored() {
    for id in [10, 42] {
        for (secret, value) in [
            ("f", "_f_"),
            ("ab", "-ab-"),
            ("xyz", "xyz="),
            ("Qz", "AAIE;Qz;AAAA"),
        ] {
            for source in [
                (44, text_body(b"SyntheticSpeaker", secret.as_bytes())),
                (42, typed_update(768, secret.as_bytes())),
            ] {
                let (report, out) = scrub_packets(S, &[source, unique_data(id, value.as_bytes())]);
                assert!(report.verified);
                assert_eq!(packet_name(&out[1]), value);
                assert!(report.diagnostics.collisions.is_empty());
            }
        }
    }
}

#[test]
fn unique_data_short_digit_chat_interiors_are_ignored() {
    for id in [10, 42] {
        for secret in [b"1".as_slice(), b"20"] {
            let (report, out) = scrub_packets(
                S,
                &[
                    (44, text_body(b"SyntheticSpeaker", secret)),
                    unique_data(id, b"AA1b_20_CC"),
                ],
            );
            assert!(report.verified);
            assert_eq!(packet_name(&out[1]), "AA1b_20_CC");
            assert!(report.diagnostics.collisions.is_empty());
        }
    }
}

#[test]
fn unique_data_base64_whole_items_still_refuse() {
    for id in [10, 42] {
        for secret in ["f", "ab", "xyz", "1", "20", "Qzxy", "QzxyAbc"] {
            for value in [
                secret.to_owned(),
                format!("AAAA,{secret},BBBB"),
                format!("{secret},BBBB"),
                format!("AAAA,{secret}"),
                format!("AA_{secret}_BB,{secret},CCCC"),
            ] {
                for source in [
                    (44, text_body(b"SyntheticSpeaker", secret.as_bytes())),
                    (42, typed_update(768, secret.as_bytes())),
                ] {
                    let error = scrub_failure(S, &[source, unique_data(id, value.as_bytes())]);
                    assert!(error.to_string().contains("field=UNIQUE_DATA_STRING"));
                    assert!(
                        error
                            .diagnostics
                            .count(maxed_cli::scrub::ScrubReason::ResidualCollision)
                            > 0
                    );
                }
            }
        }
    }
}

#[test]
fn unique_data_base64_name_interiors_are_reported() {
    for id in [10, 42] {
        for secret in ["Qzxy", "QzxyAbc"] {
            for value in [
                format!("AAAA,_{secret}_,BBBB"),
                format!("-{secret}-"),
                format!("AA_{secret}="),
            ] {
                for source in [
                    (44, text_body(b"SyntheticSpeaker", secret.as_bytes())),
                    (42, typed_update(768, secret.as_bytes())),
                ] {
                    let (report, out) =
                        scrub_packets(S, &[source, unique_data(id, value.as_bytes())]);
                    assert!(report.verified);
                    assert_eq!(packet_name(&out[1]), value.as_str());
                    assert_eq!(
                        report
                            .diagnostics
                            .count(maxed_cli::scrub::ScrubReason::OpaqueTokenCoincidence),
                        1
                    );
                    assert_eq!(report.diagnostics.collisions.values().sum::<u64>(), 1);
                    let summary = report.diagnostics.to_string();
                    assert!(summary.contains("field=UNIQUE_DATA_STRING"));
                    assert!(!summary.contains(secret));
                }
            }
        }
    }
}

#[test]
fn unique_data_base64_credentials_numeric_and_long_secrets_still_refuse() {
    for id in [10, 42] {
        for secret in ["f", "ab", "xyz", "Qzxy", "QzxyAbcd"] {
            let mut credential = vec![];
            string(&mut credential, b"Realm");
            string(&mut credential, b"game.example");
            credential.extend_from_slice(&[0; 10]);
            string(&mut credential, secret.as_bytes());
            let value = format!("AA_{secret}_BB");
            let error = scrub_failure(S, &[(45, credential), unique_data(id, value.as_bytes())]);
            assert!(error.to_string().contains("secret-class=credential"));
        }
        for secret in ["1", "12", "123", "1234", "1234567", "QzxyAbcd"] {
            for value in [format!("AA-{secret}-BB"), format!("AA{secret}BB")] {
                let error = scrub_failure(
                    S,
                    &[
                        (42, typed_update(768, secret.as_bytes())),
                        unique_data(id, value.as_bytes()),
                    ],
                );
                assert!(error.to_string().contains("secret-class=identity"));
                assert!(error.to_string().contains("field=UNIQUE_DATA_STRING"));
            }
        }
    }
}

#[test]
fn unique_data_base64_malformed_and_one_sided_name_matches_refuse() {
    for id in [10, 42] {
        for secret in ["Qzxy", "QzxyAbc", "SyntheticSecret"] {
            for value in [
                format!("AAIE;{secret};AAAA"),
                format!("{secret}_BBBB"),
                format!("AAAA_{secret}"),
                format!("AAAA,{secret}_BBBB"),
                format!("AAAA_{secret},BBBB"),
            ] {
                let error = scrub_failure(
                    S,
                    &[
                        (42, typed_update(768, secret.as_bytes())),
                        unique_data(id, value.as_bytes()),
                    ],
                );
                assert!(error.to_string().contains("secret-class=identity"));
            }
        }
    }
}

#[test]
fn unique_data_seven_byte_interior_is_admitted_but_eight_bytes_refuse() {
    let (report, _) = scrub_packets(
        S,
        &[
            (42, typed_update(768, b"QzxyAbc")),
            unique_data(10, b"AAIEQzxyAbcBBBB"),
        ],
    );
    assert!(report.verified);
    assert_eq!(
        report
            .diagnostics
            .count(maxed_cli::scrub::ScrubReason::OpaqueTokenCoincidence),
        1
    );
    let error = scrub_failure(
        S,
        &[
            (42, typed_update(768, b"QzxyAbcd")),
            unique_data(10, b"AAIEQzxyAbcdBBBB"),
        ],
    );
    assert!(error.to_string().contains("secret-class=identity"));
}

#[test]
fn unique_data_short_credential_interior_refuses() {
    let mut credential = vec![];
    string(&mut credential, b"Realm");
    string(&mut credential, b"game.example");
    credential.extend_from_slice(&[0; 10]);
    string(&mut credential, b"Qzxy");
    let error = scrub_failure(S, &[(45, credential), unique_data(10, b"AAIEQzxyBBBB")]);
    assert!(error.to_string().contains("secret-class=credential"));
}

#[test]
fn unique_data_numeric_identity_interiors_refuse() {
    for secret in [b"1234".as_slice(), b"1234567"] {
        for stat in [31, 38, 54, 115] {
            let mut identity = typed_update(768, secret);
            identity[23] = stat;
            let value = [b"AAIE".as_slice(), secret, b"BBBB"].concat();
            let error = scrub_failure(S, &[(42, identity), unique_data(10, &value)]);
            assert!(error.to_string().contains("secret-class=identity"));
        }
    }
}

#[test]
fn unique_data_mixed_interior_and_delimited_occurrences_refuse() {
    let error = scrub_failure(
        S,
        &[
            (42, typed_update(768, b"Qzxy")),
            unique_data(10, b"AAIEQzxyBBBB,Qzxy,AAAA"),
        ],
    );
    assert!(error.to_string().contains("secret-class=identity"));
}

#[test]
fn unique_data_one_sided_token_matches_refuse() {
    for value in [b"QzxyBBBB".as_slice(), b"AAIEQzxy"] {
        let error = scrub_failure(
            S,
            &[(42, typed_update(768, b"Qzxy")), unique_data(10, value)],
        );
        assert!(error.to_string().contains("secret-class=identity"));
    }
}

#[test]
fn unique_data_update_carrier_reports_the_same_coincidence_rule() {
    for kind in [768, 0x0700] {
        let (report, out) = scrub_packets(
            S,
            &[
                (42, typed_update(kind, b"Qzxy")),
                unique_data(42, b"AAIEQzxyBBBB"),
            ],
        );
        assert!(report.verified);
        assert_eq!(packet_name(&out[1]), "AAIEQzxyBBBB");
        assert_eq!(report.diagnostics.collisions.values().sum::<u64>(), 1);
        assert!(report
            .diagnostics
            .collisions
            .keys()
            .all(|c| c.id == 42 && c.field == "UNIQUE_DATA_STRING"));
        let summary = report.diagnostics.to_string();
        assert!(summary.contains("short opaque token interior"));
        assert!(summary.contains("non-numeric"));
        assert!(summary.contains("NEWTICK/UPDATE"));
    }
}

fn portal_notification(message: &str) -> Vec<u8> {
    let mut body = vec![8, 0];
    string(&mut body, message.as_bytes());
    body.extend_from_slice(&123i32.to_be_bytes());
    body
}

#[test]
fn portal_opener_reuses_name_alias_even_with_json_escapes_or_later_name_stat() {
    for message in [
        r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticOpener"}}"#,
        r#"{"k":"s.dungeon_opened_by","t":{"player":"syntheticopener"}}"#,
        r#"{"t":{"player":"\u0053yntheticOpener",},"k":"s.dungeon_opened_by",}"#,
    ] {
        let (report, out) = scrub_packets(
            S,
            &[
                (67, portal_notification(message)),
                (42, typed_update(768, b"SyntheticOpener")),
            ],
        );
        assert!(report.verified);
        let text = out[0].1["message"].as_str().unwrap();
        let notice = maxed_protocol::notices::parse_portal_announcement(text).unwrap();
        assert_eq!(notice.player, packet_name(&out[1]).as_str().unwrap());
        assert!(text.contains(packet_name(&out[1]).as_str().unwrap()));
        assert!(!format!("{out:?}").contains("SyntheticOpener"));
        assert!(report.diagnostics.collisions.is_empty());
    }
}

#[test]
fn portal_unknown_openers_are_consistent_and_unrecognized_templates_are_filled() {
    let message = r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticUnknown"}}"#;
    let (_, out) = scrub_packets(
        S,
        &[
            (67, portal_notification(message)),
            (67, portal_notification(message)),
        ],
    );
    assert_eq!(out[0], out[1]);
    let notice =
        maxed_protocol::notices::parse_portal_announcement(out[0].1["message"].as_str().unwrap())
            .unwrap();
    assert_eq!(notice.player, "0".repeat("SyntheticUnknown".len()));
    for message in [
        r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticUnknown","extra":"secret"}}"#,
        r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticUnknown","player":"Duplicate"}}"#,
        r#"{"k":"s.other","t":{"player":"SyntheticUnknown"}}"#,
    ] {
        let (_, out) = scrub_packets(S, &[(67, portal_notification(message))]);
        assert_eq!(out[0].1["message"], "x".repeat(message.len()));
    }
}

#[test]
fn portal_opener_refuses_ambiguous_case_insensitive_name_aliases() {
    let message = r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticOpener"}}"#;
    let error = scrub_failure(
        S,
        &[
            (42, typed_update(768, b"SyntheticOpener")),
            (42, typed_update(768, b"syntheticopener")),
            (67, portal_notification(message)),
        ],
    );
    assert_eq!(
        error
            .diagnostics
            .count(maxed_cli::scrub::ScrubReason::VerificationFailed),
        1
    );
}

fn death_notification(message: &str) -> Vec<u8> {
    let mut body = portal_notification(message);
    body[0] = 7;
    body
}

#[test]
fn death_notice_keeps_game_data_and_reuses_name_stat_alias() {
    for message in [
        r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Synthetic Boss","level":"20"}}"#,
        r#"{"k":"s.death","t":{"player":"syntheticvictim","level":"20","enemy":"Synthetic Boss",}}"#,
        r#"{"t":{"enemy":"Synthetic Boss","level":"20","player":"\u0053yntheticVictim",},"k":"s.death",}"#,
    ] {
        let (report, out) = scrub_packets(
            S,
            &[
                (67, death_notification(message)),
                (42, typed_update(768, b"SyntheticVictim")),
            ],
        );
        assert!(report.verified);
        let text = out[0].1["message"].as_str().unwrap();
        // The wire grammar permits trailing commas; normalize only this authored
        // test input to inspect the retained values with serde_json.
        let notice: serde_json::Value = serde_json::from_str(&text.replace(",}", "}")).unwrap();
        assert_eq!(notice["k"], "s.death");
        assert_eq!(&notice["t"]["player"], packet_name(&out[1]));
        assert_eq!(notice["t"]["enemy"], "Synthetic Boss");
        assert_eq!(notice["t"]["level"], "20");
        assert_eq!(text.len(), message.len());
        assert!(!format!("{out:?}")
            .to_ascii_lowercase()
            .contains("syntheticvictim"));
        assert!(report.diagnostics.collisions.is_empty());
    }
}

#[test]
fn death_notice_unknown_shapes_are_filled() {
    for message in [
        r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Boss","level":"20","extra":"secret"}}"#,
        r#"{"k":"s.death","t":{"player":"SyntheticVictim","player":"Duplicate","enemy":"Boss","level":"20"}}"#,
        r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Boss"}}"#,
        r#"{"k":"s.death","t":{"player":42,"enemy":"Boss","level":"20"}}"#,
        r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":{},"level":"20"}}"#,
        r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Boss","level":"secret"}}"#,
        r#"{"k":"s.death","extra":"secret","t":{"player":"SyntheticVictim","enemy":"Boss","level":"20"}}"#,
        r#"{"k":"s.other","t":{"player":"SyntheticVictim","enemy":"Boss","level":"20"}}"#,
    ] {
        let (report, out) = scrub_packets(S, &[(67, death_notification(message))]);
        assert!(report.verified);
        assert_eq!(out[0].1["message"], "x".repeat(message.len()));
    }
}

#[test]
fn death_notice_unmapped_or_ambiguous_victim_refuses() {
    let notice = (
        67,
        death_notification(
            r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Boss","level":"20"}}"#,
        ),
    );
    for names in [
        vec![],
        vec![b"SyntheticVictim".as_slice(), b"syntheticvictim"],
    ] {
        let mut packets: Vec<_> = names
            .into_iter()
            .map(|name| (42, typed_update(768, name)))
            .collect();
        packets.push(notice.clone());
        let error = scrub_failure(S, &packets);
        assert_eq!(
            error
                .diagnostics
                .count(maxed_cli::scrub::ScrubReason::VerificationFailed),
            1
        );
    }
}

#[test]
fn death_notice_retained_game_data_still_refuses_identity_collisions() {
    for enemy in ["SyntheticVictim", r"\u0053yntheticVictim"] {
        let message = format!(
            r#"{{"k":"s.death","t":{{"player":"SyntheticVictim","enemy":"{enemy}","level":"20"}}}}"#
        );
        let error = scrub_failure(
            S,
            &[
                (42, typed_update(768, b"SyntheticVictim")),
                (67, death_notification(&message)),
            ],
        );
        assert!(error.to_string().contains("secret-class=identity"));
    }
}

#[test]
fn death_notice_game_data_lists_player_text_coincidences() {
    use maxed_cli::scrub::{ScrubReason, SecretClass};
    for (chat, field) in [
        ("Cube", "deathEnemy"),
        ("cube", "deathEnemy"),
        ("20", "deathLevel"),
    ] {
        let (report, out) = scrub_packets(
            S,
            &[
                (42, typed_update(768, b"SyntheticVictim")),
                (44, text_body(b"SyntheticSpeaker", chat.as_bytes())),
                (
                    67,
                    death_notification(
                        r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Cube Deity","level":"20"}}"#,
                    ),
                ),
            ],
        );
        assert!(report.verified);
        assert_eq!(report.diagnostics.count(ScrubReason::ResidualCollision), 0);
        assert_eq!(
            report
                .diagnostics
                .count(ScrubReason::PublicLiteralCoincidence),
            1
        );
        let collision = report.diagnostics.collisions.keys().next().unwrap();
        assert_eq!(collision.field, field);
        assert_eq!(collision.secret_class, SecretClass::PlayerText);
        assert_eq!(collision.reason, ScrubReason::PublicLiteralCoincidence);
        let notice: serde_json::Value =
            serde_json::from_str(out[2].1["message"].as_str().unwrap()).unwrap();
        assert_eq!(notice["t"]["enemy"], "Cube Deity");
        assert_eq!(notice["t"]["level"], "20");
        assert_eq!(&notice["t"]["player"], packet_name(&out[0]));
        assert_eq!(out[1].1["text"], "x".repeat(chat.len()));
    }
}

#[test]
fn death_notice_game_data_refuses_case_insensitive_identity_even_with_chat() {
    use maxed_cli::scrub::{ScrubReason, SecretClass};
    for enemy in ["Boss SYNTHETICVICTIM", r"Boss \u0073yntheticvictim"] {
        let message = format!(
            r#"{{"k":"s.death","t":{{"player":"SyntheticVictim","enemy":"{enemy}","level":"20"}}}}"#
        );
        let error = scrub_failure(
            S,
            &[
                (42, typed_update(768, b"SyntheticVictim")),
                (44, text_body(b"SyntheticSpeaker", b"syntheticvictim")),
                (67, death_notification(&message)),
            ],
        );
        assert!(error
            .diagnostics
            .collisions
            .keys()
            .any(|c| c.field == "deathEnemy"
                && c.secret_class == SecretClass::Identity
                && c.reason == ScrubReason::ResidualCollision));
    }
}

#[test]
fn death_notice_game_data_refuses_credentials_even_with_chat() {
    use maxed_cli::scrub::{ScrubReason, SecretClass};
    for (secret, enemy, level, field) in [
        ("Cube", "Cube Deity", "20", "deathEnemy"),
        ("Cube", r"\u0043ube Deity", "20", "deathEnemy"),
        ("20", "Boss", "20", "deathLevel"),
    ] {
        let mut reconnect = vec![];
        string(&mut reconnect, b"Realm");
        string(&mut reconnect, b"game.example");
        reconnect.extend_from_slice(&[0; 10]);
        string(&mut reconnect, secret.as_bytes());
        let message = format!(
            r#"{{"k":"s.death","t":{{"player":"SyntheticVictim","enemy":"{enemy}","level":"{level}"}}}}"#
        );
        let error = scrub_failure(
            S,
            &[
                (42, typed_update(768, b"SyntheticVictim")),
                (45, reconnect),
                (44, text_body(b"SyntheticSpeaker", secret.as_bytes())),
                (67, death_notification(&message)),
            ],
        );
        assert!(error.diagnostics.collisions.keys().any(|c| c.field == field
            && c.secret_class == SecretClass::Credential
            && c.reason == ScrubReason::ResidualCollision));
    }
}

#[test]
fn death_notice_and_portal_templates_require_the_matching_notification_code() {
    for (code, message) in [
        (
            7,
            r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticVictim"}}"#,
        ),
        (
            8,
            r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Boss","level":"20"}}"#,
        ),
    ] {
        let mut notice = portal_notification(message);
        notice[0] = code;
        let (report, out) = scrub_packets(
            S,
            &[(42, typed_update(768, b"SyntheticVictim")), (67, notice)],
        );
        assert!(report.verified);
        assert_eq!(out[1].1["message"], "x".repeat(message.len()));
    }
}

#[test]
fn death_notice_changes_wave_two_scrub_profile() {
    // Historical change marker only. The current metadata/source hash equality
    // is guarded by split_tokens_retransmits_overlaps_names_keys_and_checksums_replay_correctly.
    use sha2::{Digest, Sha256};
    let mut schema = Sha256::new();
    for source in [
        include_bytes!("../../maxed-protocol/src/defs.rs").as_slice(),
        include_bytes!("../../maxed-protocol/src/reader.rs"),
        include_bytes!("../../maxed-protocol/src/privacy.rs"),
        include_bytes!("../../maxed-protocol/src/notices.rs"),
        include_bytes!("../src/scrub.rs"),
    ] {
        schema.update(source);
    }
    assert_ne!(
        maxed_protocol::lower_hex(&schema.finalize()),
        "919021c47ff91acb51290db4eff7fb62aab8e326d6414e26978757996fcd1ad8"
    );
}

#[test]
fn death_notice_escaped_structure_is_fully_filled() {
    let literal = r#"{"k":"s.death","t":{"player":"SyntheticVictim","enemy":"Boss","level":"20"}}"#;
    for (plain, escaped) in [
        (r#""k""#, r#""\u006b""#),
        (r#""t""#, r#""\u0074""#),
        (r#""player""#, r#""\u0070layer""#),
        (r#""enemy""#, r#""\u0065nemy""#),
        (r#""level""#, r#""\u006cevel""#),
        (r#""s.death""#, r#""\u0073.death""#),
    ] {
        let message = literal.replacen(plain, escaped, 1);
        let (report, out) = scrub_packets(
            S,
            &[
                (42, typed_update(768, b"SyntheticVictim")),
                (67, death_notification(&message)),
            ],
        );
        assert!(report.verified);
        assert_eq!(out[1].1["message"], "x".repeat(message.len()), "{plain}");
    }
}

#[test]
fn portal_notice_escaped_structure_is_fully_filled() {
    let literal = r#"{"k":"s.dungeon_opened_by","t":{"player":"SyntheticVictim"}}"#;
    for (plain, escaped) in [
        (r#""k""#, r#""\u006b""#),
        (r#""t""#, r#""\u0074""#),
        (r#""player""#, r#""\u0070layer""#),
        (r#""s.dungeon_opened_by""#, r#""\u0073.dungeon_opened_by""#),
    ] {
        let message = literal.replacen(plain, escaped, 1);
        let (report, out) = scrub_packets(
            S,
            &[
                (42, typed_update(768, b"SyntheticVictim")),
                (67, portal_notification(&message)),
            ],
        );
        assert!(report.verified);
        assert_eq!(out[1].1["message"], "x".repeat(message.len()), "{plain}");
        assert!(maxed_protocol::notices::parse_portal_announcement(&message).is_none());
    }
}

#[test]
fn oryx_notice_escaped_structure_is_fully_filled() {
    let literal = r#"{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"SyntheticVictim"}}"#;
    for (plain, escaped) in [
        (r#""k""#, r#""\u006b""#),
        (r#""t""#, r#""\u0074""#),
        (r#""KILLER""#, r#""\u004bILLER""#),
        ("stringlist.", r"\u0073tringlist."),
    ] {
        let message = literal.replacen(plain, escaped, 1);
        let (report, out) = scrub_packets(
            S,
            &[(44, text_body(b"#Oryx the Mad God", message.as_bytes()))],
        );
        assert!(report.verified);
        assert_eq!(out[0].1["text"], "x".repeat(message.len()), "{plain}");
        assert_eq!(out[0].1["cleanText"], "x".repeat(message.len()), "{plain}");
        assert!(maxed_protocol::notices::parse_oryx_announcement(&message).is_none());
    }
}

#[test]
fn notice_escaped_structure_cannot_certify_decoded_identity_or_credential_leaks() {
    for (name, secret, id, message, field) in [
        (
            "PLAYER",
            "player",
            7,
            r#"{"k":"s.death","t":{"\u0070layer":"PLAYER","enemy":"Boss","level":"20"}}"#,
            "message",
        ),
        (
            "PLAYER",
            "player",
            8,
            r#"{"k":"s.dungeon_opened_by","t":{"\u0070layer":"PLAYER"}}"#,
            "message",
        ),
        (
            "KILLER",
            "KILLER",
            44,
            r#"{"k":"stringlist.Hermit_God.killed.2","t":{"\u004bILLER":"KILLER"}}"#,
            "text",
        ),
    ] {
        for with_credential in [false, true] {
            let mut packets = vec![(42, typed_update(768, name.as_bytes()))];
            if with_credential {
                let mut reconnect = vec![];
                string(&mut reconnect, b"Realm");
                string(&mut reconnect, b"game.example");
                reconnect.extend_from_slice(&[0; 10]);
                string(&mut reconnect, secret.as_bytes());
                packets.push((45, reconnect));
            }
            let index = packets.len();
            packets.push(match id {
                7 => (67, death_notification(message)),
                8 => (67, portal_notification(message)),
                _ => (44, text_body(b"#Oryx the Mad God", message.as_bytes())),
            });
            let (report, out) = scrub_packets(S, &packets);
            // Certification is safe only because the whole unknown notice is
            // filled; no escaped key can decode to the source secret.
            assert!(report.verified);
            assert_eq!(out[index].1[field], "x".repeat(message.len()));
            if id == 44 {
                assert_eq!(out[index].1["cleanText"], "x".repeat(message.len()));
            }
            assert!(serde_json::from_str::<serde_json::Value>(
                out[index].1[field].as_str().unwrap()
            )
            .is_err());
        }
    }
}
