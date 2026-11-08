use maxed_cli::{
    domain_compare::{domain_parity, DomainError},
    domain_export::{export_domain, six_significant},
    parity::value_hash,
    CliError,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir();
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "domain-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
    fn export(&self, file: &str) -> PathBuf {
        let output = self.path(file);
        export_domain(&fixture("story.pcap"), &fixture("catalog.json"), &output).unwrap();
        output
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn fixture(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-demo")
        .join(file)
}
fn read(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn write(path: &Path, rows: &[Value]) {
    let text: String = rows.iter().map(|v| format!("{v}\n")).collect();
    fs::write(path, text).unwrap();
}
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
}
fn compare(java: &Path, rust: &Path) -> std::process::Output {
    command()
        .arg("parity-domain")
        .arg(java)
        .arg(rust)
        .output()
        .unwrap()
}
fn rule(rows: &[Value], path: &str, old: Option<&Value>, new: Option<&Value>) -> Value {
    json!({"fixtureSha256":rows[0]["inputSha256"], "catalogSha256":rows[0]["catalogSha256"],
        "kind":"fight", "recordKey":"1/0", "path":path,
        "javaSha256":value_hash(old), "maxedSha256":value_hash(new)})
}
fn ledger(path: &Path, rule: &Value) {
    fs::write(path, format!("| id | area | Java behavior | Maxed behavior | reason | evidence |\n|---|---|---|---|---|---|\n| MAXED-0001 | synthetic | reference | changed | test only | domain-rule:{rule} |\n")).unwrap();
}

#[test]
fn reconstruction_coverage_is_exported_and_comparison_only_echoes_enum_names() {
    let s = Scratch::new();
    let output = s.path("coverage.jsonl");
    let result = command()
        .arg("export-domain")
        .arg(fixture("story.pcap"))
        .arg("--catalog")
        .arg(fixture("catalog.json"))
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success());
    let metadata = s.path("coverage.jsonl.meta.json");
    let mut meta: Value = serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    let stderr = String::from_utf8(result.stderr).unwrap();
    for reason in maxed_engine::ReconstructionReason::ALL {
        let name = reason.name();
        let count = meta["reconstruction"][name].as_u64().unwrap();
        assert!(stderr.contains(&format!("coverage=reconstruction.{name} count={count}")));
    }
    meta["reconstruction"]["missingAttack"] = json!(7);
    meta["reconstruction"]["packet-derived-secret"] = json!(999);
    fs::write(&metadata, serde_json::to_vec(&meta).unwrap()).unwrap();
    let report = domain_parity(&output, &output, None).unwrap();
    assert_eq!(report.coverage["rust.reconstruction.missingAttack"], 7);
    assert!(!report
        .coverage
        .keys()
        .any(|k| k.contains("packet-derived-secret")));
    meta["reconstruction"]["missingAttack"] = json!("not-a-counter");
    fs::write(&metadata, serde_json::to_vec(&meta).unwrap()).unwrap();
    assert!(matches!(
        domain_parity(&output, &output, None),
        Err(CliError::Domain(DomainError::Metadata))
    ));
}

#[test]
fn catalog_hash_tracks_exact_file_bytes_in_headers_metadata_and_comparison() {
    let s = Scratch::new();
    let original = fs::read(fixture("catalog.json")).unwrap();
    let mut newline = original.clone();
    newline.push(b'\n');
    let mut space = newline.clone();
    *space.last_mut().unwrap() = b' ';
    let variants = [original.clone(), original, newline, space];
    let mut exports = Vec::new();
    let mut hashes = Vec::new();
    for (index, bytes) in variants.iter().enumerate() {
        let catalog = s.path(&format!("catalog-{index}.json"));
        let output = s.path(&format!("export-{index}.jsonl"));
        fs::write(&catalog, bytes).unwrap();
        export_domain(&fixture("story.pcap"), &catalog, &output).unwrap();
        let rows = read(&output);
        let expected = format!("{:x}", Sha256::digest(bytes));
        assert_eq!(rows[0]["catalogSha256"], expected);
        let metadata: Value = serde_json::from_slice(
            &fs::read(s.path(&format!("export-{index}.jsonl.meta.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["catalogSha256"], expected);
        hashes.push(expected);
        exports.push(output);
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
    assert_ne!(hashes[0], hashes[3]);
    assert_ne!(hashes[2], hashes[3]); // One whitespace byte changed.
    assert_eq!(
        domain_parity(&exports[0], &exports[1], None)
            .unwrap()
            .unexplained,
        0
    );
    for output in &exports[2..] {
        assert_eq!(read(&exports[0])[1..], read(output)[1..]);
        assert!(matches!(
            domain_parity(&exports[0], output, None),
            Err(CliError::Domain(DomainError::CatalogMismatch))
        ));
    }
}

#[test]
fn demo_exports_are_deterministic_and_matching_commands_exit_zero() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let result = command()
        .arg("export-domain")
        .arg(fixture("story.pcap"))
        .arg("--catalog")
        .arg(fixture("catalog.json"))
        .arg("--output")
        .arg(&b)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    assert_eq!(
        fs::read(s.path("a.jsonl.meta.json")).unwrap(),
        fs::read(s.path("b.jsonl.meta.json")).unwrap()
    );
    let rows = read(&a);
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[1]["key"], "1/0");
    assert_eq!(rows[2]["key"], "2/0");
    assert_eq!(rows[3]["kind"], "encounter");
    assert_eq!(rows[4]["value"]["targets"], 1);
    assert_eq!(rows[4]["value"]["damage"], 300.0);
    assert_eq!(rows[4]["value"]["dps"], 150.0);
    assert!(rows[3]["value"]["dps"].is_null());
    assert_eq!(rows[5]["value"]["fightCount"], 1);
    assert_eq!(rows[5]["value"]["dropCount"], 1);
    assert_eq!(rows[5]["value"]["outcome"], "cleared");
    assert_eq!(rows[5]["value"]["started"], 5_000_000);
    let text = fs::read_to_string(&a).unwrap();
    for absent in [
        "DemoHero",
        "DemoAccount",
        "player_id",
        "stream_id",
        "session_id",
        "character_id",
    ] {
        assert!(!text.contains(absent));
    }
    let report = domain_parity(&a, &b, None).unwrap();
    assert_eq!(
        (report.compared, report.unexplained, report.suppressed),
        (5, 0, 0)
    );
    assert_eq!(report.coverage["rust.metadataAvailable"], 1);
    assert_eq!(report.coverage["rust.fieldFailures"], 0);
    assert!(compare(&a, &b).status.success());
}

#[test]
fn changed_damage_evidence_boundary_and_missing_row_exit_nonzero() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("changed.jsonl");
    let original = read(&a);
    for case in 0..4 {
        let mut rows = original.clone();
        match case {
            0 => rows[2]["value"]["damage"] = json!(301),
            1 => rows[5]["value"]["evidence"][0]["kind"] = json!("capture_gap"),
            2 => rows[5]["value"]["ended"] = json!(12_001_000),
            _ => {
                rows.remove(1);
            }
        }
        write(&b, &rows);
        assert_eq!(compare(&a, &b).status.code(), Some(1));
        let report = domain_parity(&a, &b, None).unwrap();
        assert_eq!(report.unexplained, 1);
        assert_eq!(report.by_kind.values().sum::<u64>(), 1);
        assert_eq!(report.by_field.values().sum::<u64>(), 1);
        assert_eq!(report.coverage["rust.metadataAvailable"], 0);
    }
}

#[test]
fn invalid_headers_records_duplicates_clocks_and_nonfinite_fail_distinctly() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("bad.jsonl");
    let original = read(&a);
    for case in 0..8 {
        let mut rows = original.clone();
        match case {
            0 => rows[0]["inputSha256"] = json!("a".repeat(64)),
            1 => rows[0]["catalogSha256"] = json!("a".repeat(64)),
            2 => rows[0]["clock"] = json!("wall-clock"),
            3 => {
                rows.insert(2, rows[1].clone());
            }
            4 => {
                rows[1]["value"]
                    .as_object_mut()
                    .unwrap()
                    .remove("unattributed");
            }
            5 => rows[1]["value"]["privateUnexpectedKey"] = json!("secret-value"),
            6 => rows.swap(1, 2),
            _ => rows[1]["key"] = json!("01/0"),
        }
        write(&b, &rows);
        let error = domain_parity(&a, &b, None).unwrap_err();
        assert!(match (case, error) {
            (0, CliError::Domain(DomainError::InputMismatch))
            | (1, CliError::Domain(DomainError::CatalogMismatch))
            | (2, CliError::Domain(DomainError::Clock))
            | (3, CliError::Domain(DomainError::Duplicate))
            | (4 | 5 | 7, CliError::Domain(DomainError::Record))
            | (6, CliError::Domain(DomainError::Order)) => true,
            (_, e) => panic!("unexpected error: {e}"),
        });
        let result = compare(&a, &b);
        assert_eq!(result.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("secret-value"));
    }
    let text = fs::read_to_string(&a)
        .unwrap()
        .replacen("\"damage\":50.0", "\"damage\":1e999", 1);
    assert!(text.contains("1e999"));
    fs::write(&b, text).unwrap();
    assert!(matches!(
        domain_parity(&a, &b, None),
        Err(CliError::Domain(DomainError::NonFinite))
    ));
}

#[test]
fn ledger_suppresses_only_the_exact_fixture_catalog_row_path_and_value() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let rules = s.path("ledger.md");
    let original = read(&a);
    let mut rows = original.clone();
    rows[1]["value"]["damage"] = json!(51.0);
    write(&b, &rows);
    let exact = rule(
        &rows,
        "/value/damage",
        Some(&original[1]["value"]["damage"]),
        Some(&rows[1]["value"]["damage"]),
    );
    ledger(&rules, &exact);
    let report = domain_parity(&a, &b, Some(&rules)).unwrap();
    assert_eq!((report.unexplained, report.suppressed), (0, 1));
    assert_eq!(report.applied_ids["MAXED-0001"], 1);
    let result = command()
        .arg("parity-domain")
        .arg(&a)
        .arg(&b)
        .arg("--ledger")
        .arg(&rules)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("suppression=MAXED-0001 count=1"));
    for (field, value) in [
        ("fixtureSha256", json!("a".repeat(64))),
        ("catalogSha256", json!("a".repeat(64))),
        ("recordKey", json!("2/0")),
        ("path", json!("/value/unattributed")),
        ("javaSha256", json!("a".repeat(64))),
        ("maxedSha256", json!("a".repeat(64))),
    ] {
        let mut changed = exact.clone();
        changed[field] = value;
        ledger(&rules, &changed);
        assert_eq!(domain_parity(&a, &b, Some(&rules)).unwrap().unexplained, 1);
    }
    for path in [
        "*",
        "/value/private",
        "/value/damage/0",
        "/value/players/00/damage",
        "/value/players/0/sources/0/private",
    ] {
        let mut invalid = exact.clone();
        invalid["path"] = json!(path);
        ledger(&rules, &invalid);
        assert!(domain_parity(&a, &b, Some(&rules)).is_err());
    }
    let mut invalid = exact.clone();
    invalid["extra"] = json!(true);
    ledger(&rules, &invalid);
    assert!(domain_parity(&a, &b, Some(&rules)).is_err());
    // Whole missing rows use the full record and the distinct missing sentinel.
    let mut rows = original.clone();
    rows.remove(1);
    write(&b, &rows);
    ledger(&rules, &rule(&original, "", Some(&original[1]), None));
    assert_eq!(domain_parity(&a, &b, Some(&rules)).unwrap().suppressed, 1);
}

#[test]
fn existing_packet_ledger_validation_remains_strict_with_mixed_rows() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let rows = read(&a);
    let rules = s.path("ledger.md");
    let packet = s.path("packets.jsonl");
    fs::write(&packet, "").unwrap();
    ledger(&rules, &rule(&rows, "/value/damage", None, None));
    // Domain rows cannot require packet private metadata or suppress packet diffs.
    assert_eq!(
        maxed_cli::parity(&packet, &packet, Some(&rules), 20)
            .unwrap()
            .unexplained,
        0
    );
    let mut mixed = fs::read_to_string(&rules).unwrap();
    mixed.push_str(
        "| MAXED-0002 | packet | old | new | synthetic | parity-rule:{\"unknown\":true} |\n",
    );
    fs::write(&rules, mixed).unwrap();
    assert!(maxed_cli::parity(&packet, &packet, Some(&rules), 20).is_err());
    assert!(domain_parity(&a, &a, Some(&rules)).is_err());
}

#[test]
fn create_new_cleanup_and_truncated_input_preserve_existing_files() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let bytes = fs::read(&a).unwrap();
    assert!(export_domain(&fixture("story.pcap"), &fixture("catalog.json"), &a).is_err());
    assert_eq!(fs::read(&a).unwrap(), bytes);
    let output = s.path("new.jsonl");
    let metadata = s.path("new.jsonl.meta.json");
    fs::write(&metadata, "owned-by-someone-else").unwrap();
    assert!(export_domain(&fixture("story.pcap"), &fixture("catalog.json"), &output).is_err());
    assert!(!output.exists());
    assert_eq!(
        fs::read_to_string(metadata).unwrap(),
        "owned-by-someone-else"
    );
    let input = s.path("truncated.pcap");
    let mut bytes = fs::read(fixture("story.pcap")).unwrap();
    bytes.pop();
    fs::write(&input, bytes).unwrap();
    assert!(export_domain(&input, &fixture("catalog.json"), &output).is_err());
    assert!(!output.exists());
}

#[test]
fn exact_binary_half_even_f64_rounding_and_negative_zero() {
    for (input, output) in [
        (1.234565, 1.23456),
        (1.234575, 1.23457),
        // The next binary value lies above the decimal tie rather than below it.
        (f64::from_bits(1.234575_f64.to_bits() + 1), 1.23458),
        (123456.5, 123456.),
        (123457.5, 123458.),
        (-1.234565, -1.23456),
        (999999.5, 1e6),
        (1.234565e-100, 1.23456e-100),
        (1.234565e100, 1.23456e100),
        (2.675, 2.675),
        (f64::from_bits(1), f64::from_bits(1)),
        (-0.0, 0.0),
    ] {
        assert_eq!(six_significant(input).unwrap().to_bits(), output.to_bits());
    }
    assert!(six_significant(f64::INFINITY).is_err());
    assert!(six_significant(f64::NAN).is_err());
}

#[test]
fn field_counts_aggregate_indices_but_diffs_and_suppressions_keep_exact_paths() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let rules = s.path("ledger.md");
    let mut baseline = read(&a);
    let mut player = baseline[1]["value"]["players"][0].clone();
    let mut source = player["sources"][0].clone();
    source["itemType"] = json!(257);
    player["sources"].as_array_mut().unwrap().push(source);
    let mut other = player.clone();
    other["objectId"] = json!(3);
    baseline[1]["value"]["players"] = json!([player, other]);
    write(&a, &baseline);
    let paths = [
        "/value/players/0/sources/0/damage",
        "/value/players/0/sources/1/damage",
        "/value/players/1/sources/1/damage",
    ];
    let mut changed = baseline.clone();
    for path in paths {
        *changed[1].pointer_mut(path).unwrap() = json!(51.0);
    }
    write(&b, &changed);
    let report = domain_parity(&a, &b, None).unwrap();
    assert_eq!(report.unexplained, 3);
    assert_eq!(report.by_field.len(), 1);
    assert_eq!(report.by_field["fight/value/players/*/sources/*/damage"], 3);
    assert_eq!(
        report
            .diffs
            .iter()
            .map(|d| d.path.as_str())
            .collect::<Vec<_>>(),
        paths
    );
    ledger(
        &rules,
        &rule(
            &baseline,
            paths[0],
            baseline[1].pointer(paths[0]),
            changed[1].pointer(paths[0]),
        ),
    );
    let report = domain_parity(&a, &b, Some(&rules)).unwrap();
    assert_eq!((report.unexplained, report.suppressed), (2, 1));
    assert_eq!(report.by_field["fight/value/players/*/sources/*/damage"], 2);
    assert_eq!(
        report
            .diffs
            .iter()
            .map(|d| d.path.as_str())
            .collect::<Vec<_>>(),
        paths[1..]
    );
}

#[test]
fn duplicate_json_members_and_missing_nulls_are_malformed_but_availability_is_a_difference() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let text = fs::read_to_string(&a).unwrap().replacen(
        "\"damage\":50.0",
        "\"damage\":50.0,\"damage\":50.0",
        1,
    );
    fs::write(&b, text).unwrap();
    assert!(matches!(
        domain_parity(&a, &b, None),
        Err(CliError::Domain(DomainError::Record))
    ));
    let mut rows = read(&a);
    rows[1]["value"]["players"] = Value::Null;
    write(&b, &rows);
    let report = domain_parity(&a, &b, None).unwrap();
    assert_eq!(report.unexplained, 1);
    assert_eq!(report.diffs[0].path, "/value/players");
    rows[1]["value"].as_object_mut().unwrap().remove("players");
    write(&b, &rows);
    assert!(matches!(
        domain_parity(&a, &b, None),
        Err(CliError::Domain(DomainError::Record))
    ));
}

#[test]
fn matching_partial_or_empty_exports_are_parity_only_and_metadata_hashes_are_checked() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let mut rows = read(&a);
    rows[1]["value"]["completeCapture"] = json!(false);
    write(&b, &rows);
    let report = domain_parity(&b, &b, None).unwrap();
    assert_eq!(report.unexplained, 0);
    assert_eq!(report.coverage["rust.fight.incompleteOrUnknown"], 1);
    assert_eq!(report.coverage["rust.metadataAvailable"], 0);
    write(&b, &rows[..1]);
    let report = domain_parity(&b, &b, None).unwrap();
    assert_eq!(report.coverage["rust.records"], 0);
    assert_eq!(report.compared, 0);
    fs::write(
        s.path("b.jsonl.meta.json"),
        json!({"inputSha256":"0".repeat(64), "catalogSha256":rows[0]["catalogSha256"]}).to_string(),
    )
    .unwrap();
    assert!(matches!(
        domain_parity(&b, &b, None),
        Err(CliError::Domain(DomainError::Metadata))
    ));
}

#[test]
fn scoped_coverage_is_named_while_damage_outcome_and_other_evidence_still_compare() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let mut java = read(&a);
    let fight = java.iter_mut().find(|v| v["kind"] == "fight").unwrap();
    fight["value"]["completeCapture"] = json!(false);
    let mut rows = java.clone();
    let fight = rows.iter_mut().find(|v| v["kind"] == "fight").unwrap();
    fight["value"]["completeCapture"] = json!(true);
    let run = java.iter_mut().find(|v| v["kind"] == "run").unwrap();
    run["value"]["evidence"].as_array_mut().unwrap().insert(0, json!({
        "at":1000, "kind":"capture_gap", "rule":null, "before":null, "after":null, "delayMicros":null
    }));
    write(&a, &java);
    write(&b, &rows);
    let report = domain_parity(&a, &b, None).unwrap();
    assert_eq!(report.unexplained, 0);
    assert_eq!(report.applied_ids["D-coverage-scope"], 2);
    assert_eq!(report.coverage["D-coverage-scope.fields"], 2);
    let fight = rows.iter_mut().find(|v| v["kind"] == "fight").unwrap();
    fight["value"]["damage"] = json!(123456);
    let run = rows.iter_mut().find(|v| v["kind"] == "run").unwrap();
    run["value"]["outcome"] = json!("unknown");
    run["value"]["evidence"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "at":20_000_000, "kind":"victory", "rule":null, "before":null, "after":null, "delayMicros":null
        }));
    write(&b, &rows);
    let report = domain_parity(&a, &b, None).unwrap();
    assert!(report.by_field.contains_key("fight/value/damage"));
    assert!(report.by_field.contains_key("run/value/outcome"));
    assert!(report
        .by_field
        .keys()
        .any(|k| k.starts_with("run/value/evidence")));
    assert_eq!(report.applied_ids["D-coverage-scope"], 1);
}

#[test]
fn reverse_completeness_differences_remain_unexplained_or_require_exact_ledger_rules() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let original = read(&a);
    for kind in ["fight", "run"] {
        let mut java = original.clone();
        let row = java.iter_mut().find(|v| v["kind"] == kind).unwrap();
        row["value"]["completeCapture"] = json!(true);
        let mut rust = java.clone();
        let row = rust.iter_mut().find(|v| v["kind"] == kind).unwrap();
        row["value"]["completeCapture"] = json!(false);
        write(&a, &java);
        write(&b, &rust);
        let report = domain_parity(&a, &b, None).unwrap();
        assert_eq!(report.unexplained, 1);
        assert_eq!(report.suppressed, 0);
        assert_eq!(report.diffs[0].path, "/value/completeCapture");
        assert_eq!(report.diffs[0].kind, kind);
        if kind == "fight" {
            let path = s.path("coverage-ledger.md");
            ledger(
                &path,
                &rule(
                    &java,
                    "/value/completeCapture",
                    Some(&json!(true)),
                    Some(&json!(false)),
                ),
            );
            let report = domain_parity(&a, &b, Some(&path)).unwrap();
            assert_eq!(report.unexplained, 0);
            assert_eq!(report.applied_ids["MAXED-0001"], 1);
            assert!(!report.applied_ids.contains_key("D-coverage-scope"));
        }
        assert_eq!(
            domain_parity(&b, &a, None).unwrap().applied_ids["D-coverage-scope"],
            1
        );
    }
}

#[test]
fn capture_gap_classification_requires_rust_to_be_a_subset_including_multiplicity() {
    let s = Scratch::new();
    let a = s.export("a.jsonl");
    let b = s.path("b.jsonl");
    let original = read(&a);
    let gap = |at| {
        json!({
            "at":at, "kind":"capture_gap", "rule":null, "before":null, "after":null, "delayMicros":null
        })
    };
    for (java_gaps, rust_gaps, deliberate) in [
        (vec![gap(1000), gap(2000)], vec![gap(2000)], true),
        (vec![gap(1000)], vec![gap(1000), gap(2000)], false),
        (vec![gap(1000)], vec![gap(2000)], false),
        (vec![gap(1000)], vec![gap(1000), gap(1000)], false),
    ] {
        for (path, gaps) in [(&a, java_gaps), (&b, rust_gaps)] {
            let mut rows = original.clone();
            let run = rows.iter_mut().find(|v| v["kind"] == "run").unwrap();
            run["value"]["evidence"] = json!(gaps);
            write(path, &rows);
        }
        let report = domain_parity(&a, &b, None).unwrap();
        if deliberate {
            assert_eq!(report.unexplained, 0);
            assert_eq!(report.applied_ids["D-coverage-scope"], 1);
        } else {
            assert!(report.unexplained > 0);
            assert_eq!(report.suppressed, 0);
            assert!(report
                .diffs
                .iter()
                .all(|d| d.path.starts_with("/value/evidence")));
        }
    }
}

#[test]
fn export_rejects_repository_output_before_creating_files() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("forbidden-domain.jsonl");
    assert!(matches!(
        export_domain(&fixture("story.pcap"), &fixture("catalog.json"), &output),
        Err(CliError::InvalidArguments)
    ));
    assert!(!output.exists());
}

#[test]
fn export_hash_matches_decoded_recording_bytes() {
    let scratch = Scratch::new();
    let output = scratch.export("hash.jsonl");
    let bytes = fs::read(fixture("story.pcap")).unwrap();
    let expected = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(read(&output)[0]["inputSha256"], expected);
}
