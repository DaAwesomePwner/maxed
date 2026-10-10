mod support;
use maxed_cli::decode;
use maxed_recording::PcapWriter;
use maxed_types::{RawFrame, Timestamp};
use sha2::{Digest, Sha256};
use std::fs;
use support::PrivateDir;

#[test]
fn quest_and_realm_score_decode_as_typed_packets() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction::ServerToClient;
    let d = PrivateDir::new();
    let mut rc4 = Rc4::new(&key_bytes(ServerToClient).unwrap()).unwrap();
    let mut payload = vec![];
    for (id, mut body) in [
        (82u8, vec![0, 0, 0, 64, 1, 128, 1]),
        (169, 15_644i32.to_be_bytes().to_vec()),
    ] {
        payload.extend_from_slice(&((body.len() + 5) as u32).to_be_bytes());
        payload.push(id);
        rc4.apply(&mut body);
        payload.extend(body);
    }
    let mut w = PcapWriter::new(vec![]).unwrap();
    for mut f in [frame(0, 9, true, &[]), frame(1, 10, false, &payload)] {
        f.bytes[34..38].copy_from_slice(&[8, 2, 0xc3, 0x50]);
        w.write_frame(&f).unwrap();
    }
    let input = d.write("quest-score.pcap", w.finish().unwrap());
    let output = d.file("decoded.jsonl");
    let r = decode(&input, &output).unwrap();
    assert_eq!((r.decoded, r.unknown, r.failures), (2, 0, 0));
    let rows: Vec<serde_json::Value> = fs::read_to_string(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows[0]["f"], serde_json::json!({"objectId":64,"list":[64]}));
    assert_eq!(rows[1]["f"], serde_json::json!({"score":15644}));
}

fn frame(index: u64, seq: u32, syn: bool, payload: &[u8]) -> RawFrame {
    let mut b = vec![0; 54 + payload.len()];
    b[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    b[14] = 0x45;
    b[16..18].copy_from_slice(&((40 + payload.len()) as u16).to_be_bytes());
    b[23] = 6;
    b[26..30].copy_from_slice(&[127, 0, 0, 1]);
    b[30..34].copy_from_slice(&[127, 0, 0, 2]);
    b[34..38].copy_from_slice(&[0xc3, 0x50, 8, 2]);
    b[38..42].copy_from_slice(&seq.to_be_bytes());
    b[46] = 0x50;
    b[47] = if syn { 2 } else { 0x10 };
    b[54..].copy_from_slice(payload);
    RawFrame {
        index,
        at: Timestamp(index * 100),
        original_len: b.len() as u32,
        bytes: b,
    }
}
fn pcap() -> Vec<u8> {
    let mut w = PcapWriter::new(vec![]).unwrap();
    w.write_frame(&frame(0, 9, true, &[])).unwrap();
    // Empty ESCAPE followed by Unknown, then one malformed PLAYERHIT.
    w.write_frame(&frame(
        1,
        10,
        false,
        &[0, 0, 0, 5, 105, 0, 0, 0, 7, 250, 1, 2, 0, 0, 0, 6, 90, 3],
    ))
    .unwrap();
    w.finish().unwrap()
}
#[test]
fn synthetic_decode_counts_shape_and_private_provenance() {
    let d = PrivateDir::new();
    let bytes = pcap();
    let input = d.write("input.pcap", &bytes);
    let output = d.file("rust.jsonl");
    let r = decode(&input, &output).unwrap();
    assert_eq!((r.frames, r.decoded, r.unknown, r.failures), (2, 2, 1, 1));
    let text = fs::read_to_string(&output).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines[0],
        serde_json::json!({"i":0,"t":100,"dir":"c2s","id":105,"name":"ESCAPE","f":{}})
    );
    assert_eq!(lines[1]["i"], 1);
    assert_eq!(lines[1]["f"], serde_json::json!({"len":7}));
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(d.file("rust.jsonl.meta.json")).unwrap()).unwrap();
    assert_eq!(
        metadata,
        serde_json::json!({"inputSha256":maxed_protocol::canonical::lower_hex(&Sha256::digest(&bytes)),
            "fieldFailures":1,"trailingBytePackets":0,"unknownIdPackets":1,
            "wrongDirectionPackets":0,"unverifiedPackets":0,"truncated":false})
    );
}

#[test]
fn joined_replay_and_decode_commands_use_the_same_recording() {
    let d = PrivateDir::new();
    let input = d.write("input.pcap", pcap());
    let binary = env!("CARGO_BIN_EXE_maxed-cli");
    let replay = std::process::Command::new(binary)
        .arg("replay")
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(replay.status.code(), Some(0));
    assert!(String::from_utf8(replay.stdout)
        .unwrap()
        .contains("frames=2 decoded=2 unknown=1 failures=1"));
    for named in [false, true] {
        let output = d.file(if named {
            "named.jsonl"
        } else {
            "positional.jsonl"
        });
        let mut command = std::process::Command::new(binary);
        command.arg("decode").arg(&input);
        if named {
            command.arg("--output");
        }
        let result = command.arg(&output).output().unwrap();
        assert_eq!(result.status.code(), Some(0));
        assert!(String::from_utf8(result.stderr)
            .unwrap()
            .contains("frames=2 decoded=2 unknown=1 failures=1"));
        assert_eq!(fs::read_to_string(&output).unwrap().lines().count(), 2);
    }
    assert_eq!(
        fs::read(d.file("named.jsonl")).unwrap(),
        fs::read(d.file("positional.jsonl")).unwrap()
    );
}
#[test]
fn refuses_overwrite_same_input_and_tracked_destinations() {
    let d = PrivateDir::new();
    let input = d.write("input.pcap", pcap());
    let output = d.write("existing.jsonl", b"synthetic marker");
    assert!(decode(&input, &input).is_err());
    assert!(decode(&input, &output).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"synthetic marker");
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let outside = repo.join("outside.jsonl");
    let outside_metadata = repo.join("outside.jsonl.meta.json");
    assert!(!outside.exists());
    assert!(!outside_metadata.exists());
    for path in [
        repo.join("Cargo.toml"),
        repo.join("tests/fixtures/scrubbed/private.jsonl"),
        outside.clone(),
    ] {
        let error = decode(&input, &path).unwrap_err();
        assert!(matches!(error, maxed_cli::CliError::InvalidArguments));
        let error = error.to_string();
        assert!(!error.contains("input.pcap"));
        assert!(!error.contains("outside.jsonl"));
    }
    assert!(!outside.exists());
    assert!(!outside_metadata.exists());
}
#[test]
fn failed_decode_removes_only_its_own_partial_files() {
    let d = PrivateDir::new();
    let input = d.write("bad.pcap", b"synthetic invalid capture");
    let output = d.file("failed.jsonl");
    assert!(decode(&input, &output).is_err());
    assert!(!output.exists());
    assert!(!d.file("failed.jsonl.meta.json").exists());
    assert!(input.exists());
    let input = d.write("good.pcap", pcap());
    let metadata = d.write("failed.jsonl.meta.json", b"keep existing metadata");
    assert!(decode(&input, &output).is_err());
    assert!(!output.exists());
    assert_eq!(fs::read(metadata).unwrap(), b"keep existing metadata");
}

#[test]
fn nonfinite_export_counts_failure_and_continues_with_consecutive_indices() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction;
    let d = PrivateDir::new();
    let mut c = Rc4::new(&key_bytes(Direction::ClientToServer).unwrap()).unwrap();
    let mut payload = vec![0, 0, 0, 5, 105];
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut body = 1i32.to_be_bytes().to_vec();
        body.extend(value.to_be_bytes());
        body.extend(0f32.to_be_bytes());
        c.apply(&mut body);
        payload.extend(17u32.to_be_bytes());
        payload.push(103);
        payload.extend(body);
    }
    let mut body = vec![255; 6];
    c.apply(&mut body);
    payload.extend([0, 0, 0, 11, 90]);
    payload.extend(body);
    let mut w = PcapWriter::new(vec![]).unwrap();
    w.write_frame(&frame(0, 9, true, &[])).unwrap();
    w.write_frame(&frame(1, 10, false, &payload)).unwrap();
    let input = d.write("nonfinite.pcap", w.finish().unwrap());
    let output = d.file("nonfinite.jsonl");
    let report = decode(&input, &output).unwrap();
    assert_eq!((report.decoded, report.failures), (2, 3));
    let lines: Vec<serde_json::Value> = fs::read_to_string(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["i"], 0);
    assert_eq!(lines[1]["i"], 1);
    assert_eq!(lines[1]["f"]["objectId"], -1);
}

fn encrypted_packet(cipher: &mut maxed_protocol::Rc4, id: u8, mut body: Vec<u8>) -> Vec<u8> {
    cipher.apply(&mut body);
    let mut bytes = ((body.len() + 5) as u32).to_be_bytes().to_vec();
    bytes.push(id);
    bytes.extend(body);
    bytes
}

#[test]
fn replay_decode_and_metadata_separate_all_four_java_counters() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction;
    let d = PrivateDir::new();
    let mut cipher = Rc4::new(&key_bytes(Direction::ClientToServer).unwrap()).unwrap();
    let mut payload = Vec::new();
    for (id, body) in [
        (250, vec![1]),
        (67, vec![]),
        (120, vec![]),
        (1, vec![]),
        (105, vec![42]),
        (90, vec![1]),
        (90, vec![0; 6]),
    ] {
        payload.extend(encrypted_packet(&mut cipher, id, body));
    }
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    writer.write_frame(&frame(0, 9, true, &[])).unwrap();
    writer.write_frame(&frame(1, 10, false, &payload)).unwrap();
    let input = d.write("counters.pcap", writer.finish().unwrap());
    let output = d.file("counters.jsonl");
    let decoded = decode(&input, &output).unwrap();
    assert_eq!(maxed_cli::replay(&input).unwrap(), decoded);
    assert_eq!(
        (
            decoded.field_failures,
            decoded.trailing_byte_packets,
            decoded.unknown_id_packets,
            decoded.wrong_direction_packets
        ),
        (1, 1, 1, 2)
    );
    assert_eq!(
        (decoded.decoded, decoded.unknown, decoded.failures),
        (6, 4, 2)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(d.file("counters.jsonl.meta.json")).unwrap()).unwrap();
    for (key, count) in [
        ("fieldFailures", 1),
        ("trailingBytePackets", 1),
        ("unknownIdPackets", 1),
        ("wrongDirectionPackets", 2),
    ] {
        assert_eq!(metadata[key], count);
        assert!(decoded.to_string().contains(&format!("{key}={count}")));
    }
}

#[test]
fn truncated_last_record_retains_complete_prefix_metadata_and_cli_summary() {
    let d = PrivateDir::new();
    let prefix = pcap();
    let mut tail = PcapWriter::new(Vec::new()).unwrap();
    tail.write_frame(&frame(2, 28, false, &[0, 0, 0, 5, 105]))
        .unwrap();
    let tail = tail.finish().unwrap();
    let tail = &tail[24..];
    for cut in [1, 15, 16, tail.len() - 1] {
        let mut bytes = prefix.clone();
        bytes.extend(&tail[..cut]);
        let input = d.write(&format!("truncated-{cut}.pcap"), &bytes);
        let output = d.file(&format!("prefix-{cut}.jsonl"));
        let report = decode(&input, &output).unwrap();
        assert_eq!(report.frames, 2);
        assert_eq!(report.decoded, 2);
        assert!(report.truncated);
        assert_eq!(maxed_cli::replay(&input).unwrap(), report);
        assert_eq!(fs::read_to_string(output).unwrap().lines().count(), 2);
        let metadata: serde_json::Value = serde_json::from_slice(
            &fs::read(d.file(&format!("prefix-{cut}.jsonl.meta.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["truncated"], true);
        assert_eq!(
            metadata["inputSha256"],
            maxed_protocol::lower_hex(&Sha256::digest(&bytes))
        );
        if cut == 15 {
            let binary = env!("CARGO_BIN_EXE_maxed-cli");
            let result = std::process::Command::new(binary)
                .arg("replay")
                .arg(&input)
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
            assert!(String::from_utf8(result.stdout)
                .unwrap()
                .contains("frames=2 decoded=2"));
            assert!(String::from_utf8(result.stderr)
                .unwrap()
                .contains("Truncated final pcap record"));
            let result = std::process::Command::new(binary)
                .arg("decode")
                .arg(&input)
                .arg("--output")
                .arg(d.file("cli-prefix.jsonl"))
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
            let summary = String::from_utf8(result.stderr).unwrap();
            assert!(summary.contains("frames=2 decoded=2"));
            assert!(summary.contains("Truncated final pcap record"));
            assert_eq!(
                fs::read_to_string(d.file("cli-prefix.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
        }
    }
}

#[test]
fn corruption_before_eof_and_truncated_global_header_remain_strict_errors() {
    let d = PrivateDir::new();
    let mut bad = pcap();
    bad.extend([0; 8]); // seconds, micros
    bad.extend(u32::MAX.to_le_bytes()); // impossible included length
    bad.extend(u32::MAX.to_le_bytes());
    bad.extend(pcap()); // more data after the corrupt record
    for (name, bytes) in [("corrupt", bad), ("header", vec![0xd4, 0xc3])] {
        let input = d.write(&format!("{name}.pcap"), bytes);
        let output = d.file(&format!("{name}.jsonl"));
        assert!(maxed_cli::replay(&input).is_err());
        assert!(decode(&input, &output).is_err());
        assert!(!output.exists());
        assert!(!d.file(&format!("{name}.jsonl.meta.json")).exists());
    }
}

#[test]
fn malformed_update_and_notification_explanations_are_private_and_structural() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction;
    let d = PrivateDir::new();
    let mut cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    let mut update = vec![0; 9];
    update.extend([0, 1, 0x12, 0x34, 1]);
    update.extend([0; 8]);
    update.extend([2, 20, 1, 0, 31, 0, 80]);
    update.extend(b"SYNTHETIC_SECRET");
    let mut payload = encrypted_packet(&mut cipher, 42, update);
    payload.extend(encrypted_packet(
        &mut cipher,
        67,
        b"\x06\x00\x00\x40SYNTHETIC_SECRET".to_vec(),
    ));
    payload.extend(encrypted_packet(&mut cipher, 67, vec![255, 0]));
    payload.extend(encrypted_packet(&mut cipher, 120, vec![0; 6]));
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    for mut raw in [frame(0, 9, true, &[]), frame(1, 10, false, &payload)] {
        raw.bytes[34..38].copy_from_slice(&[8, 2, 0xc3, 0x50]);
        writer.write_frame(&raw).unwrap();
    }
    let input = d.write("explain.pcap", writer.finish().unwrap());
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .arg("decode")
        .arg(&input)
        .arg("--output")
        .arg(d.file("explain.jsonl"))
        .arg("--explain-failures")
        .arg(d.file("explain.txt"))
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = fs::read_to_string(d.file("explain.txt")).unwrap();
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("name="))
            .count(),
        3
    );
    assert!(text.contains("event=Opened fresh=true packetOrdinal=0 connId="));
    assert!(text.contains("event=Closed packetOrdinal=4 connId="));
    assert!(text.contains("name=UPDATE id=42 len=50 offset=34 error=Truncated stats=[(4660,20,int),(4660,31,string)] nextHex="));
    assert!(text.contains("name=NOTIFICATION id=67 len=25 offset=9 error=Truncated nextHex="));
    assert!(text.contains("name=NOTIFICATION id=67 len=7 offset=6 error=Definition nextHex="));
    assert!(!text.contains("SYNTHETIC_SECRET"));
    assert!(!text.contains(&maxed_protocol::lower_hex(b"SYNTHETIC_SECRET")));
    let summary = String::from_utf8(result.stderr).unwrap();
    assert!(summary.contains("fieldFailures=3"));
    let private_decode = maxed_cli::decode::decode_with_explanations;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(private_decode(
        &input,
        &d.file("refused.jsonl"),
        Some(&root.join("report.txt"))
    )
    .is_err());
    assert!(!d.file("refused.jsonl").exists());
    assert!(private_decode(&input, &d.file("refused.jsonl"), Some(&input)).is_err());
    assert!(private_decode(
        &input,
        &d.file("refused.jsonl"),
        Some(&d.file("explain.txt"))
    )
    .is_err());
    assert_eq!(fs::read_to_string(d.file("explain.txt")).unwrap(), text);
    assert!(!d.file("refused.jsonl").exists());
}

#[test]
fn extended_report_includes_inner_create_stats_and_preserves_rejected_packet_continuation() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction;
    let d = PrivateDir::new();
    let mut cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    let mut create = vec![0; 8];
    create.extend([0, 8]);
    create.extend(b"AAAAAAA="); // zero flag plus an unexplained byte
    let mut truncated = vec![0; 8];
    truncated.extend([0, 80]);
    truncated.extend(b"SYNTHETIC_SECRET");
    let mut update = vec![0; 9];
    update.push(63);
    update.extend([0x12; 100]);
    let mut payload = Vec::new();
    for (id, body) in [
        (101, create),
        (101, truncated),
        (114, vec![0x12, 0x34, 0x81, 1, 2, 3, 4, 5, 6, 0x80]),
        (42, update),
        (67, vec![255, 254, 0, 0]),
        (10, vec![0; 16]),
    ] {
        payload.extend(encrypted_packet(&mut cipher, id, body));
    }
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    for mut raw in [frame(0, 9, true, &[]), frame(1, 10, false, &payload)] {
        raw.bytes[34..38].copy_from_slice(&[8, 2, 0xc3, 0x50]);
        writer.write_frame(&raw).unwrap();
    }
    let input = d.write("extended.pcap", writer.finish().unwrap());
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .arg("decode")
        .arg(&input)
        .arg("--output")
        .arg(d.file("extended.jsonl"))
        .arg("--explain-failures")
        .arg(d.file("extended.txt"))
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = fs::read_to_string(d.file("extended.txt")).unwrap();
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("name="))
            .count(),
        5
    );
    for line in text.lines() {
        for field in [
            "packetOrdinal=",
            "connId=",
            "dir=",
            "timestampMicros=",
            "lastNewTickId=",
            "postRecovered=",
            "packetsSinceRecovered=",
        ] {
            assert!(line.contains(field), "missing {field} on {line}");
        }
    }
    assert!(text.contains("event=Closed packetOrdinal=6"));
    assert!(text.contains("lastNewTickId=0 postRecovered=false packetsSinceRecovered=none"));
    let ordinals: Vec<u64> = text
        .lines()
        .map(|line| {
            line.split("packetOrdinal=")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    assert!(ordinals.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(text.contains("outer=decoded base64Length=8 decodedLength=5"));
    assert!(text.contains("divergence=trailing@4"));
    assert!(text.contains("str.length@8:width=2:value=80"));
    assert!(text.contains("manaProgress@9:width=1:expected=ci:available=1:error=Truncated"));
    assert!(text.contains("bodyOffset=10 prefixHex="));
    assert!(text.contains("prefixEnd=74 prefixSchema=current"));
    assert!(text.contains("len=9 offset=6 error=Definition nextHex= effectByte=255 extraByte=254"));
    for secret in ["SYNTHETIC_SECRET", "AAAAAAA="] {
        assert!(!text.contains(secret));
        assert!(!text.contains(&maxed_protocol::lower_hex(secret.as_bytes())));
    }
    let lines: Vec<serde_json::Value> = fs::read_to_string(d.file("extended.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["name"], "CREATE_SUCCESS");
    assert_eq!(lines[1]["name"], "NEWTICK");
    assert_eq!(lines[1]["i"], 1);
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("fieldFailures=4"));
    // The opt-in report changes neither canonical output nor counters.
    let normal = decode(&input, &d.file("normal.jsonl")).unwrap();
    assert_eq!(normal.field_failures, 4);
    assert_eq!(
        fs::read(d.file("normal.jsonl")).unwrap(),
        fs::read(d.file("extended.jsonl")).unwrap()
    );
}

#[test]
fn syn_provenance_changes_trust_counts_and_metadata_but_not_canonical_jsonl() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction;
    let d = PrivateDir::new();
    let mut outputs = Vec::new();
    for syn_proven in [false, true] {
        let mut cipher = Rc4::new(&key_bytes(Direction::ClientToServer).unwrap()).unwrap();
        let mut nonfinite = vec![0; 4];
        nonfinite.extend(f32::NAN.to_be_bytes());
        nonfinite.extend([0; 4]);
        let mut payload = Vec::new();
        for (id, body) in [
            (105, vec![]),
            (103, nonfinite),
            (62, 0i32.to_be_bytes().to_vec()),
            (90, vec![0; 6]),
            (62, 1i32.to_be_bytes().to_vec()),
            (105, vec![]),
        ] {
            payload.extend(encrypted_packet(&mut cipher, id, body));
        }
        let mut writer = PcapWriter::new(Vec::new()).unwrap();
        let mut start = frame(0, 9, syn_proven, &[]);
        if !syn_proven {
            start.bytes[34..36].copy_from_slice(&50001u16.to_be_bytes());
            start.bytes[47] = 0x14; // stale RST, so the next direction starts by assumption
        }
        writer.write_frame(&start).unwrap();
        writer.write_frame(&frame(1, 10, false, &payload)).unwrap();
        let input = d.write(
            &format!("trust-{syn_proven}.pcap"),
            writer.finish().unwrap(),
        );
        let output = d.file(&format!("trust-{syn_proven}.jsonl"));
        let report = decode(&input, &output).unwrap();
        let expected = if syn_proven { 0 } else { 4 };
        assert_eq!(report.unverified_packets, expected);
        assert_eq!(report.decoded, 5); // the nonfinite packet is not exported
        assert_eq!(
            maxed_cli::replay(&input).unwrap().unverified_packets,
            expected
        );
        let metadata: serde_json::Value = serde_json::from_slice(
            &fs::read(d.file(&format!("trust-{syn_proven}.jsonl.meta.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["unverifiedPackets"], expected);
        for command in ["decode", "replay"] {
            let mut process = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"));
            process.arg(command).arg(&input);
            if command == "decode" {
                process
                    .arg("--output")
                    .arg(d.file(&format!("trust-cli-{syn_proven}.jsonl")));
            }
            let result = process.output().unwrap();
            assert!(result.status.success());
            let summary = if command == "decode" {
                result.stderr
            } else {
                result.stdout
            };
            assert!(String::from_utf8(summary)
                .unwrap()
                .contains(&format!("unverifiedPackets={expected}")));
        }
        outputs.push(fs::read(output).unwrap());
    }
    assert_eq!(outputs[0], outputs[1]);
}
