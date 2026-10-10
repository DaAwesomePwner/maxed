use maxed_cli::parity::{compare_lines, direction_lines};
use maxed_protocol::CanonicalLine;
use serde_json::json;

fn line(i: u64, dir: &str, id: u8, t: u64) -> CanonicalLine {
    CanonicalLine {
        i,
        t,
        dir: dir.into(),
        id,
        name: maxed_protocol::defs::known_name(id).into(),
        f: if id == 105 {
            json!({})
        } else {
            json!({"len":5})
        },
    }
}

#[test]
fn quest_and_score_typed_fields_keep_packet_parity_differences_visible() {
    for (id, body, expected_paths) in [
        (
            82,
            vec![0, 0, 0, 64, 1, 128, 1],
            vec!["/f/len", "/f/list", "/f/objectId"],
        ),
        (
            169,
            15_644i32.to_be_bytes().to_vec(),
            vec!["/f/len", "/f/score"],
        ),
    ] {
        let mut java = line(0, "s2c", id, 1);
        java.f = json!({"len":body.len()+5});
        let mut maxed = line(0, "s2c", id, 1);
        maxed.f = maxed_protocol::decode_packet(id, maxed_types::Direction::ServerToClient, &body)
            .unwrap()
            .0
            .fields()
            .unwrap();
        let result = compare_lines(&[java], &[maxed], 20);
        assert_eq!(result.unexplained, expected_paths.len() as u64);
        assert_eq!(
            result
                .diffs
                .iter()
                .map(|d| d.path.as_str())
                .collect::<Vec<_>>(),
            expected_paths
        );
    }
}
#[test]
fn per_direction_order_ignores_only_global_interleaving() {
    let a = vec![line(0, "c2s", 105, 1), line(1, "s2c", 250, 2)];
    let b = vec![line(0, "s2c", 250, 2), line(1, "c2s", 105, 1)];
    assert_eq!(direction_lines(&a, "s2c")[0].i, 1);
    let r = compare_lines(&a, &b, 20);
    assert_eq!(r.unexplained, 0);
    assert_eq!(r.compared, 2);
}
#[test]
fn differences_count_beyond_display_limit_without_resynchronizing() {
    let a = vec![
        line(0, "s2c", 250, 1),
        line(1, "s2c", 250, 2),
        line(2, "s2c", 250, 3),
    ];
    let b = vec![line(0, "s2c", 250, 1), line(1, "s2c", 250, 3)];
    let r = compare_lines(&a, &b, 1);
    assert_eq!(r.unexplained, 2);
    assert_eq!(r.diffs.len(), 1);
    assert_eq!(r.by_type["UNKNOWN"], 2);
}
#[test]
fn recursive_diffs_never_reveal_values_or_untrusted_keys() {
    let mut a = line(0, "c2s", 105, 1);
    let mut b = line(0, "c2s", 105, 1);
    a.f = json!({"objectId":1,"text":"synthetic-secret-a","untrusted-secret-key":"a"});
    b.f = json!({"objectId":2,"text":"synthetic-secret-b","untrusted-secret-key":"b"});
    let r = compare_lines(&[a], &[b], 20);
    assert_eq!(r.unexplained, 3);
    assert!(r.diffs.iter().all(|d| !d.path.contains("secret")));
    assert!(r.diffs.iter().any(|d| d.path == "/f/text"));
}
#[test]
fn exact_integers_and_numeric_float_equality() {
    let mut a = line(0, "s2c", 250, 1);
    let mut b = line(0, "s2c", 250, 1);
    a.f = json!({"x":1,"y":-0.0,"objectId":u64::MAX});
    b.f = json!({"x":1.0,"y":0,"objectId":u64::MAX});
    assert_eq!(compare_lines(&[a], &[b], 20).unexplained, 0);
    let mut a = line(0, "s2c", 250, 1);
    let mut b = line(0, "s2c", 250, 1);
    a.f = json!({"objectId":9007199254740992u64});
    b.f = json!({"objectId":9007199254740993u64});
    assert_eq!(compare_lines(&[a], &[b], 20).unexplained, 1);
}
mod support;
use maxed_cli::parity::{parity, parity_with_fixture, value_hash};
use std::fs;
use support::PrivateDir;
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const HEADER:&str="# Deliberate differences\n\n| id | area | Java behavior | Maxed behavior | reason | evidence |\n|---|---|---|---|---|---|\n";
fn exports(d: &PrivateDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let mut a = line(0, "c2s", 105, 1);
    let mut b = line(0, "c2s", 105, 1);
    a.f = json!({"text":"synthetic-a"});
    b.f = json!({"text":"synthetic-b"});
    let a = d.write(
        "java.jsonl",
        format!("{}\n", serde_json::to_string(&a).unwrap()),
    );
    let b = d.write(
        "rust.jsonl",
        format!("{}\n", serde_json::to_string(&b).unwrap()),
    );
    for name in ["java.jsonl.meta.json", "rust.jsonl.meta.json"] {
        d.write(name, json!({"inputSha256":HASH}).to_string());
    }
    (a, b)
}

#[test]
fn repository_documentary_quest_score_rows_do_not_suppress_packet_differences() {
    let d = PrivateDir::new();
    let (java, maxed) = exports(&d);
    let ledger = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/parity/deliberate-differences.md");
    let report = parity(&java, &maxed, Some(&ledger), 20).unwrap();
    assert_eq!((report.unexplained, report.suppressed), (1, 0));
}
fn rule() -> serde_json::Value {
    json!({"fixtureSha256":HASH,"dir":"c2s","ordinal":0,"packetId":105,"path":"/f/text","javaSha256":value_hash(Some(&json!("synthetic-a"))),"maxedSha256":value_hash(Some(&json!("synthetic-b")))})
}
fn ledger(d: &PrivateDir, id: &str, rule: &serde_json::Value) -> std::path::PathBuf {
    d.write("ledger.md",format!("{HEADER}| {id} | protocol | old | new | reviewed synthetic case | parity-rule: {rule} |\n"))
}
#[test]
fn narrow_ledger_suppression_requires_exact_provenance_values_and_cli_argument() {
    let d = PrivateDir::new();
    let (a, b) = exports(&d);
    let p = ledger(&d, "MAXED-0001", &rule());
    let r = parity(&a, &b, Some(&p), 20).unwrap();
    assert_eq!((r.unexplained, r.suppressed), (0, 1));
    assert!(r.by_type.is_empty());
    assert!(parity_with_fixture(&a, &b, Some(&p), 20, None).is_err());
    let (r, ids) = parity_with_fixture(&a, &b, Some(&p), 20, Some(HASH)).unwrap();
    assert_eq!(r.suppressed, 1);
    assert!(r.by_type.is_empty());
    assert_eq!(ids["MAXED-0001"], 1);
    assert!(parity_with_fixture(&a, &b, Some(&p), 20, Some(OTHER)).is_err());
    for (key, value) in [
        ("fixtureSha256", json!(OTHER)),
        ("ordinal", json!(1)),
        ("packetId", json!(250)),
        ("javaSha256", json!(OTHER)),
        ("maxedSha256", json!(OTHER)),
        ("path", json!("/t")),
    ] {
        let mut rule = rule();
        rule[key] = value;
        let p = ledger(&d, "MAXED-0001", &rule);
        let r = parity(&a, &b, Some(&p), 20).unwrap();
        assert_eq!((r.unexplained, r.suppressed), (1, 0));
    }
    let p = ledger(&d, "MAXED-0001", &rule());
    fs::remove_file(d.file("java.jsonl.meta.json")).unwrap();
    assert!(parity(&a, &b, Some(&p), 20).is_err());
    d.write(
        "java.jsonl.meta.json",
        json!({"inputSha256":OTHER}).to_string(),
    );
    assert!(parity(&a, &b, Some(&p), 20).is_err());
}
#[test]
fn malformed_ledger_rows_and_unknown_ids_are_rejected() {
    let d = PrivateDir::new();
    let (a, b) = exports(&d);
    for id in ["UNKNOWN-0001", "MAXED-*", "MAXED-12345", "MAXED-abcd"] {
        assert!(parity(&a, &b, Some(&ledger(&d, id, &rule())), 20).is_err());
    }
    for (key, value) in [
        ("path", json!("/f/*")),
        ("fixtureSha256", json!("*")),
        ("dir", json!("both")),
        ("ordinal", json!(-1)),
        ("packetId", json!(256)),
        ("extra", json!(true)),
    ] {
        let mut r = rule();
        r[key] = value;
        assert!(parity(&a, &b, Some(&ledger(&d, "MAXED-0001", &r)), 20).is_err());
    }
    let empty = d.write("ledger.md", HEADER);
    assert_eq!(parity(&a, &b, Some(&empty), 20).unwrap().suppressed, 0);
    let malformed = d.write(
        "ledger.md",
        format!("{HEADER}| MAXED-0001 | protocol | a | b | reason | parity-rule: {{}} |\n"),
    );
    assert!(parity(&a, &b, Some(&malformed), 20).is_err());
}
#[test]
fn invalid_json_indices_directions_and_oversized_lines_fail_safely() {
    let d = PrivateDir::new();
    let good = d.write(
        "good.jsonl",
        format!(
            "{}\n",
            serde_json::to_string(&line(0, "c2s", 105, 1)).unwrap()
        ),
    );
    for text in [
        "synthetic secret invalid json\n".to_string(),
        format!(
            "{}\n",
            serde_json::to_string(&line(1, "c2s", 105, 1)).unwrap()
        ),
        format!(
            "{}\n",
            serde_json::to_string(&line(0, "wrong", 105, 1)).unwrap()
        ),
        " ".repeat(maxed_cli::parity::MAX_CANONICAL_LINE_BYTES + 1),
        "\n".into(),
    ] {
        let bad = d.write("bad.jsonl", text);
        let error = parity(&good, &bad, None, 20).err().unwrap().to_string();
        assert!(!error.contains("synthetic secret"));
    }
}

#[test]
fn protocol_sized_newtick_expansion_above_one_mib_is_accepted() {
    use maxed_protocol::{canonical_line, decode_packet, write_jsonl, DecodedPacket, MAX_FRAME};
    use maxed_types::{ConnId, Direction, Timestamp};
    let d = PrivateDir::new();
    // Each numeric stat is three wire bytes but expands to five named fields.
    let mut body = vec![0; 14];
    body.extend(1u16.to_be_bytes());
    body.extend([0; 9]); // objectId plus position
    body.extend([0xa0, 0xb8, 0x02]); // compressed count = 20,000
    body.resize(body.len() + 20_000 * 3, 0);
    assert!(body.len() + 5 <= MAX_FRAME);
    let (packet, left) = decode_packet(10, Direction::ServerToClient, &body).unwrap();
    assert_eq!(left, 0);
    let decoded = DecodedPacket {
        alignment_verified: true,
        conn: ConnId(1),
        dir: Direction::ServerToClient,
        at: Timestamp(1),
        id: 10,
        len: body.len() + 5,
        packet,
    };
    let mut jsonl = vec![];
    write_jsonl(&mut jsonl, &canonical_line(0, &decoded).unwrap()).unwrap();
    assert!(jsonl.len() > 1_048_576);
    assert!(jsonl.len() < maxed_cli::parity::MAX_CANONICAL_LINE_BYTES);
    let a = d.write("large-java.jsonl", &jsonl);
    let b = d.write("large-rust.jsonl", &jsonl);
    let report = parity(&a, &b, None, 20).unwrap();
    assert_eq!((report.compared, report.unexplained), (1, 0));
}

#[test]
fn type_counts_deduplicate_each_side_but_retain_distinct_types() {
    let a = line(0, "s2c", 250, 1);
    let b = line(0, "s2c", 250, 2);
    let report = compare_lines(&[a], &[b], 20);
    assert_eq!(report.unexplained, 1);
    assert_eq!(report.by_type["UNKNOWN"], 1);
    let a = line(0, "s2c", 250, 1);
    let mut b = line(0, "s2c", 10, 1);
    b.name = "UNKNOWN".into(); // Isolate the ID difference.
    let report = compare_lines(&[a], &[b], 20);
    assert_eq!(report.unexplained, 1);
    assert_eq!(report.by_type["UNKNOWN"], 1);
    assert_eq!(report.by_type["NEWTICK"], 1);
}

#[test]
fn suppressed_and_unexplained_counts_remain_separate_for_the_same_type() {
    let d = PrivateDir::new();
    let (a, b) = exports(&d);
    let mut changed: CanonicalLine = serde_json::from_slice(&fs::read(&b).unwrap()).unwrap();
    changed.t += 1;
    fs::write(
        &b,
        format!("{}\n", serde_json::to_string(&changed).unwrap()),
    )
    .unwrap();
    let p = ledger(&d, "MAXED-0001", &rule());
    let (report, ids) = parity_with_fixture(&a, &b, Some(&p), 20, Some(HASH)).unwrap();
    assert_eq!((report.unexplained, report.suppressed), (1, 1));
    assert_eq!(report.by_type["ESCAPE"], 1);
    assert_eq!(ids["MAXED-0001"], 1);
}
#[test]
fn timestamp_enum_field_and_direction_differences_are_visible() {
    let mut a = line(0, "s2c", 0, 1);
    let mut b = line(0, "s2c", 0, 2);
    a.f = json!({"errorId":"IncorrectVersion"});
    b.f = json!({"errorId":4,"extra":true});
    let r = compare_lines(&[a], &[b], 20);
    assert_eq!(r.unexplained, 3);
    assert!(r.diffs.iter().any(|d| d.path == "/t"));
    let a = line(0, "s2c", 250, 1);
    let b = line(0, "c2s", 250, 1);
    assert_eq!(compare_lines(&[a], &[b], 20).unexplained, 2);
}
#[test]
fn command_exit_codes_and_default_output_do_not_expose_values() {
    let d = PrivateDir::new();
    let (a, b) = exports(&d);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .args(["parity"])
        .arg(&a)
        .arg(&b)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("/f/text"));
    assert!(!stdout.contains("synthetic-a"));
    assert!(!stdout.contains("synthetic-b"));
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .arg("parity")
        .arg(&a)
        .arg(&a)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    let bad = d.write("bad.jsonl", b"private invalid json\n");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .arg("parity")
        .arg(&a)
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(!String::from_utf8(result.stderr)
        .unwrap()
        .contains("private invalid"));
    let p = ledger(&d, "MAXED-0001", &rule());
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
        .arg("parity")
        .arg(&a)
        .arg(&b)
        .arg("--ledger")
        .arg(p)
        .args(["--fixture-sha256", HASH])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .contains("suppression=MAXED-0001 count=1"));
}

#[test]
fn counter_metadata_remains_compatible_with_exact_provenance_checks() {
    let d = PrivateDir::new();
    let (a, b) = exports(&d);
    let p = ledger(&d, "MAXED-0001", &rule());
    let mut metadata = json!({"inputSha256":HASH,"fieldFailures":37,"trailingBytePackets":2,
        "unknownIdPackets":3,"wrongDirectionPackets":4,"unverifiedPackets":5,"truncated":false});
    d.write(
        "rust.jsonl.meta.json",
        serde_json::to_vec(&metadata).unwrap(),
    );
    let (report, _) = parity_with_fixture(&a, &b, Some(&p), 20, Some(HASH)).unwrap();
    assert_eq!(report.suppressed, 1);
    metadata["fieldFailures"] = json!(-1);
    d.write(
        "rust.jsonl.meta.json",
        serde_json::to_vec(&metadata).unwrap(),
    );
    assert!(parity_with_fixture(&a, &b, Some(&p), 20, Some(HASH)).is_err());
    metadata["fieldFailures"] = json!(37);
    metadata["unexpected"] = json!(1);
    d.write(
        "rust.jsonl.meta.json",
        serde_json::to_vec(&metadata).unwrap(),
    );
    assert!(parity_with_fixture(&a, &b, Some(&p), 20, Some(HASH)).is_err());
}

#[test]
fn decode_metadata_is_accepted_by_fixture_ledger_parity() {
    use maxed_recording::PcapWriter;
    use maxed_types::{RawFrame, Timestamp};
    let d = PrivateDir::new();
    let mut writer = PcapWriter::new(vec![]).unwrap();
    for (index, seq, syn, payload) in [
        (0, 9u32, true, &[][..]),
        (1, 10, false, &[0, 0, 0, 5, 105][..]),
    ] {
        let mut bytes = vec![0; 54 + payload.len()];
        bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        bytes[14] = 0x45;
        bytes[16..18].copy_from_slice(&((40 + payload.len()) as u16).to_be_bytes());
        bytes[23] = 6;
        bytes[26..30].copy_from_slice(&[127, 0, 0, 1]);
        bytes[30..34].copy_from_slice(&[127, 0, 0, 2]);
        bytes[34..38].copy_from_slice(&[0xc3, 0x50, 8, 2]);
        bytes[38..42].copy_from_slice(&seq.to_be_bytes());
        bytes[46] = 0x50;
        bytes[47] = if syn { 2 } else { 0x10 };
        bytes[54..].copy_from_slice(payload);
        writer
            .write_frame(&RawFrame {
                index,
                at: Timestamp(index * 100),
                original_len: bytes.len() as u32,
                bytes,
            })
            .unwrap();
    }
    let input = d.write("input.pcap", writer.finish().unwrap());
    let rust = d.file("rust.jsonl");
    assert_eq!(maxed_cli::decode(&input, &rust).unwrap().decoded, 1);
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(d.file("rust.jsonl.meta.json")).unwrap()).unwrap();
    assert_eq!(metadata["unverifiedPackets"], 0);
    let hash = metadata["inputSha256"].as_str().unwrap();
    let mut java_line: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&rust).unwrap()).unwrap();
    assert_eq!(java_line["t"], 100);
    java_line["t"] = json!(101);
    let java = d.write("java.jsonl", format!("{java_line}\n"));
    // Old Java metadata and newly produced Maxed metadata must both be accepted.
    d.write(
        "java.jsonl.meta.json",
        json!({"inputSha256":hash}).to_string(),
    );
    let rule = json!({"fixtureSha256":hash,"dir":"c2s","ordinal":0,"packetId":105,
        "path":"/t","javaSha256":value_hash(Some(&json!(101))),"maxedSha256":value_hash(Some(&json!(100)))});
    let ledger = ledger(&d, "MAXED-0001", &rule);
    let (report, ids) = parity_with_fixture(&java, &rust, Some(&ledger), 20, Some(hash)).unwrap();
    assert_eq!((report.unexplained, report.suppressed), (0, 1));
    assert_eq!(ids["MAXED-0001"], 1);
    assert!(parity_with_fixture(&java, &rust, Some(&ledger), 20, Some(OTHER)).is_err());
}
