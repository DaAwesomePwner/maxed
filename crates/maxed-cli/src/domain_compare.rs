//! Strict domain-schema comparison and fixture-bound, exact-value ledger rules.
//! Diagnostics contain only schema paths and ordinals, never field values.
use crate::{parity::value_hash, CliError};
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Invalid domain header")]
    Header,
    #[error("Domain input hashes differ")]
    InputMismatch,
    #[error("Domain catalog hashes differ")]
    CatalogMismatch,
    #[error("Unsupported domain clock version")]
    Clock,
    #[error("Malformed domain record")]
    Record,
    #[error("Duplicate domain record key")]
    Duplicate,
    #[error("Domain records are not in canonical order")]
    Order,
    #[error("Nonfinite domain number")]
    NonFinite,
    #[error("Invalid domain catalog")]
    Catalog,
    #[error("Domain projection has no verified map boundary")]
    Projection,
    #[error("Invalid domain metadata")]
    Metadata,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    schema: String,
    input_sha256: String,
    catalog_sha256: String,
    clock: String,
}

#[derive(Debug, Default)]
pub struct DomainParityReport {
    pub compared: u64,
    pub unexplained: u64,
    pub suppressed: u64,
    pub by_kind: BTreeMap<String, u64>,
    pub by_field: BTreeMap<String, u64>,
    pub applied_ids: BTreeMap<String, u64>,
    pub diffs: Vec<DomainDiff>,
    /// Numeric coverage diagnostics, including explicit metadata availability.
    pub coverage: BTreeMap<String, u64>,
}
#[derive(Debug)]
pub struct DomainDiff {
    pub kind: String,
    pub key: String,
    pub path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DomainRule {
    #[serde(skip)]
    pub(crate) id: String,
    fixture_sha256: String,
    catalog_sha256: String,
    kind: String,
    record_key: String,
    path: String,
    java_sha256: String,
    maxed_sha256: String,
}
impl DomainRule {
    pub(crate) fn parse(text: &str, id: &str) -> Result<Self, CliError> {
        let mut rule: Self = serde_json::from_str(text).map_err(|_| CliError::InvalidArguments)?;
        if ![
            &rule.fixture_sha256,
            &rule.catalog_sha256,
            &rule.java_sha256,
            &rule.maxed_sha256,
        ]
        .into_iter()
        .all(|s| valid_hash(s))
            || record_order(&rule.kind, &rule.record_key).is_none()
            || !valid_path(&rule.kind, &rule.path)
        {
            return Err(CliError::InvalidArguments);
        }
        rule.id = id.into();
        Ok(rule)
    }
    pub(crate) fn selector(&self) -> (&str, &str, &str, &str, &str) {
        (
            &self.fixture_sha256,
            &self.catalog_sha256,
            &self.kind,
            &self.record_key,
            &self.path,
        )
    }
}

fn valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn ordinal(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 1 && s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        None
    } else {
        s.parse().ok()
    }
}
fn record_order(kind: &str, key: &str) -> Option<(u8, u64, u64)> {
    match kind {
        "fight" => {
            let (map, target) = key.split_once('/')?;
            Some((0, ordinal(map)?, ordinal(target)?))
        }
        "encounter" => Some((1, ordinal(key)?, 0)),
        "run" => Some((2, ordinal(key)?, 0)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Object(&'static [(&'static str, Shape)]),
    Array(&'static Shape),
    Float,
    UInt,
    Int,
    Time,
    Bool,
    Text,
    Enum(&'static [&'static str]),
}
const SOURCE: Shape = Shape::Object(&[
    (
        "kind",
        Shape::Enum(&[
            "weapon",
            "ability",
            "summon",
            "item_effect",
            "other",
            "unknown",
        ]),
    ),
    ("itemType", Shape::UInt),
    (
        "enchantState",
        Shape::Enum(&["missing", "invalid", "recorded_empty", "recorded"]),
    ),
    ("enchantIds", Shape::Array(&Shape::Int)),
    ("damage", Shape::Float),
    ("hits", Shape::UInt),
    ("biggestHit", Shape::Float),
    ("estimated", Shape::Bool),
]);
const PLAYER: Shape = Shape::Object(&[
    ("objectId", Shape::Int),
    ("local", Shape::Bool),
    ("damage", Shape::Float),
    ("hits", Shape::UInt),
    ("biggestHit", Shape::Float),
    ("dps", Shape::Float),
    ("share", Shape::Float),
    ("sources", Shape::Array(&SOURCE)),
]);
const EVIDENCE: Shape = Shape::Object(&[
    ("at", Shape::Time),
    (
        "kind",
        Shape::Enum(&[
            "victory",
            "completion_dialogue",
            "completion_counter",
            "death_packet",
            "observed_exit",
            "capture_gap",
            "app_closed",
        ]),
    ),
    ("rule", Shape::Text),
    ("before", Shape::Int),
    ("after", Shape::Int),
    ("delayMicros", Shape::UInt),
]);
const FIGHT: Shape = Shape::Object(&[
    ("map", Shape::Text),
    ("targetType", Shape::UInt),
    ("targetOrdinal", Shape::UInt),
    ("firstHit", Shape::Time),
    ("lastHit", Shape::Time),
    ("damage", Shape::Float),
    ("unattributed", Shape::Float),
    ("completeCapture", Shape::Bool),
    ("players", Shape::Array(&PLAYER)),
]);
const ENCOUNTER: Shape = Shape::Object(&[
    ("map", Shape::Text),
    ("firstHit", Shape::Time),
    ("lastHit", Shape::Time),
    ("damage", Shape::Float),
    ("unattributed", Shape::Float),
    ("dps", Shape::Float),
    ("targets", Shape::UInt),
]);
const RUN: Shape = Shape::Object(&[
    ("map", Shape::Text),
    ("started", Shape::Time),
    ("ended", Shape::Time),
    ("completedAt", Shape::Time),
    ("diedAt", Shape::Time),
    (
        "outcome",
        Shape::Enum(&["cleared", "died", "left", "unknown"]),
    ),
    ("evidence", Shape::Array(&EVIDENCE)),
    ("completeCapture", Shape::Bool),
    ("fightCount", Shape::UInt),
    ("dropCount", Shape::UInt),
]);
fn shape(kind: &str) -> Option<Shape> {
    match kind {
        "fight" => Some(FIGHT),
        "encounter" => Some(ENCOUNTER),
        "run" => Some(RUN),
        _ => None,
    }
}
fn validate(value: &Value, shape: Shape) -> bool {
    // Explicit null preserves unavailable Java fields. Objects/array elements
    // themselves must still have the declared shape when present.
    if value.is_null() {
        return true;
    }
    match shape {
        Shape::Object(fields) => value.as_object().is_some_and(|v| {
            v.len() == fields.len()
                && fields
                    .iter()
                    .all(|(k, s)| v.get(*k).is_some_and(|v| validate(v, *s)))
        }),
        Shape::Array(s) => value
            .as_array()
            .is_some_and(|a| a.iter().all(|v| !v.is_null() && validate(v, *s))),
        Shape::Float => value.as_f64().is_some_and(f64::is_finite),
        Shape::UInt => value.as_u64().is_some(),
        Shape::Int => value.as_i64().is_some(),
        Shape::Time => value.as_u64().is_some_and(|t| t % 1000 == 0),
        Shape::Bool => value.is_boolean(),
        Shape::Text => value.is_string(),
        Shape::Enum(choices) => value.as_str().is_some_and(|s| choices.contains(&s)),
    }
}
fn valid_path(kind: &str, path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let Some(mut schema) = shape(kind) else {
        return false;
    };
    let mut parts = path.split('/');
    if parts.next() != Some("") || parts.next() != Some("value") {
        return false;
    }
    for part in parts {
        schema = match schema {
            Shape::Object(fields) => match fields.iter().find(|(k, _)| *k == part) {
                Some((_, s)) => *s,
                None => return false,
            },
            Shape::Array(s) if ordinal(part).is_some() => *s,
            _ => return false,
        };
    }
    true
}

type SourceKey<'a> = (
    Option<&'a str>,
    Option<u64>,
    Option<&'a str>,
    Option<Vec<i64>>,
);
fn source_key(v: &Value) -> SourceKey<'_> {
    (
        v["kind"].as_str(),
        v["itemType"].as_u64(),
        v["enchantState"].as_str(),
        v["enchantIds"]
            .as_array()
            .map(|ids| ids.iter().filter_map(Value::as_i64).collect()),
    )
}
fn ordered_values(kind: &str, value: &Value) -> bool {
    if kind == "fight" {
        if let Some(players) = value["players"].as_array() {
            if !players
                .windows(2)
                .all(|p| p[0]["objectId"].as_i64() <= p[1]["objectId"].as_i64())
            {
                return false;
            }
            for player in players {
                if let Some(sources) = player["sources"].as_array() {
                    if !sources
                        .windows(2)
                        .all(|s| source_key(&s[0]) <= source_key(&s[1]))
                    {
                        return false;
                    }
                }
            }
        }
    } else if kind == "run" {
        if let Some(evidence) = value["evidence"].as_array() {
            return evidence.windows(2).all(|e| {
                (e[0]["at"].as_u64(), e[0]["kind"].as_str())
                    <= (e[1]["at"].as_u64(), e[1]["kind"].as_str())
            });
        }
    }
    true
}

struct Document {
    header: Header,
    records: BTreeMap<(u8, u64, u64), Value>,
}
// serde_json::Value otherwise silently accepts repeated object members. In a
// canonical comparison, ambiguity in a header or value is a malformed document.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("number out of range"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(v)) = seq.next_element()? {
                    values.push(v);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, UniqueValue(v))) = map.next_entry::<String, UniqueValue>()? {
                    if values.insert(key, v).is_some() {
                        return Err(de::Error::custom("duplicate JSON member"));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
fn read_line(reader: &mut impl BufRead) -> Result<Option<Value>, CliError> {
    let mut bytes = Vec::new();
    let max = crate::parity::MAX_CANONICAL_LINE_BYTES;
    let n = reader.take(max as u64 + 1).read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if n > max || bytes.last() != Some(&b'\n') {
        return Err(DomainError::Record.into());
    }
    serde_json::from_slice::<UniqueValue>(&bytes)
        .map(|v| Some(v.0))
        .map_err(|e| {
            if e.to_string().contains("number out of range") {
                DomainError::NonFinite.into()
            } else {
                DomainError::Record.into()
            }
        })
}
fn document(path: &Path) -> Result<Document, CliError> {
    let mut reader = BufReader::new(File::open(path)?);
    let header: Header =
        serde_json::from_value(read_line(&mut reader)?.ok_or(DomainError::Header)?)
            .map_err(|_| DomainError::Header)?;
    if header.schema != "maxed-domain-v1"
        || !valid_hash(&header.input_sha256)
        || !valid_hash(&header.catalog_sha256)
    {
        return Err(DomainError::Header.into());
    }
    if header.clock != "tick-receipt-ms-v1" {
        return Err(DomainError::Clock.into());
    }
    let mut records = BTreeMap::new();
    let mut previous = None;
    while let Some(record) = read_line(&mut reader)? {
        if !record
            .as_object()
            .is_some_and(|r| r.len() == 3 && r.contains_key("value"))
        {
            return Err(DomainError::Record.into());
        }
        let kind = record["kind"].as_str().ok_or(DomainError::Record)?;
        let key = record["key"].as_str().ok_or(DomainError::Record)?;
        let order = record_order(kind, key).ok_or(DomainError::Record)?;
        if !record["value"].is_object()
            || !validate(&record["value"], shape(kind).ok_or(DomainError::Record)?)
            || kind == "fight" && record["value"]["targetOrdinal"].as_u64() != Some(order.2)
        {
            return Err(DomainError::Record.into());
        }
        if !ordered_values(kind, &record["value"]) {
            return Err(DomainError::Order.into());
        }
        if records.contains_key(&order) {
            return Err(DomainError::Duplicate.into());
        }
        if previous.is_some_and(|p| p >= order) {
            return Err(DomainError::Order.into());
        }
        previous = Some(order);
        records.insert(order, record);
    }
    Ok(Document { header, records })
}

fn coverage(
    path: &Path,
    doc: &Document,
    side: &str,
    report: &mut DomainParityReport,
) -> Result<(), CliError> {
    let metadata = crate::decode::metadata_path(path);
    let file = match File::open(metadata) {
        Ok(file) => Some(file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    report.coverage.insert(
        format!("{side}.metadataAvailable"),
        u64::from(file.is_some()),
    );
    if let Some(file) = file {
        let mut bytes = Vec::new();
        file.take(1_048_577).read_to_end(&mut bytes)?;
        if bytes.len() > 1_048_576 {
            return Err(DomainError::Metadata.into());
        }
        let meta: Value = serde_json::from_slice(&bytes).map_err(|_| DomainError::Metadata)?;
        if meta["inputSha256"] != doc.header.input_sha256
            || meta["catalogSha256"] != doc.header.catalog_sha256
        {
            return Err(DomainError::Metadata.into());
        }
        for key in [
            "fieldFailures",
            "trailingBytePackets",
            "unknownIdPackets",
            "wrongDirectionPackets",
            "unverifiedPackets",
            "decodeFailures",
            "unsupportedFrames",
            "unmappedFights",
        ] {
            if let Some(value) = meta.get(key) {
                report.coverage.insert(
                    format!("{side}.{key}"),
                    value.as_u64().ok_or(DomainError::Metadata)?,
                );
            }
        }
        if let Some(value) = meta.get("truncated") {
            report.coverage.insert(
                format!("{side}.truncated"),
                u64::from(value.as_bool().ok_or(DomainError::Metadata)?),
            );
        }
        if let Some(issues) = meta.get("issues") {
            let issues = issues.as_object().ok_or(DomainError::Metadata)?;
            // Never echo arbitrary metadata object keys into diagnostics.
            for key in [
                "midstream",
                "decode_failure",
                "stream_gap",
                "missing_identity",
                "missing_catalog",
                "missing_stats",
                "missing_projectile",
                "ambiguous_hit",
                "resource_limit",
                "clock_regression",
            ] {
                if let Some(value) = issues.get(key) {
                    report.coverage.insert(
                        format!("{side}.issue.{key}"),
                        value.as_u64().ok_or(DomainError::Metadata)?,
                    );
                }
            }
        }
        if let Some(counts) = meta.get("reconstruction") {
            let counts = counts.as_object().ok_or(DomainError::Metadata)?;
            // Only engine-authored enum labels may enter diagnostics.
            for reason in maxed_engine::ReconstructionReason::ALL {
                let key = reason.name();
                if let Some(value) = counts.get(key) {
                    report.coverage.insert(
                        format!("{side}.reconstruction.{key}"),
                        value.as_u64().ok_or(DomainError::Metadata)?,
                    );
                }
            }
        }
    }
    for kind in ["fight", "run"] {
        let incomplete = doc
            .records
            .values()
            .filter(|r| r["kind"] == kind && r["value"]["completeCapture"] != true)
            .count();
        report.coverage.insert(
            format!("{side}.{kind}.incompleteOrUnknown"),
            incomplete as u64,
        );
    }
    report
        .coverage
        .insert(format!("{side}.records"), doc.records.len() as u64);
    Ok(())
}

type Emit<'a> = dyn FnMut(&str, Option<&Value>, Option<&Value>) + 'a;

fn scoped_coverage_difference(report: &mut DomainParityReport) {
    report.suppressed += 1;
    *report
        .applied_ids
        .entry("D-coverage-scope".into())
        .or_default() += 1;
    *report
        .coverage
        .entry("D-coverage-scope.fields".into())
        .or_default() += 1;
}
fn walk(a: Option<&Value>, b: Option<&Value>, path: &str, emit: &mut Emit<'_>) {
    match (a, b) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                walk(a.get(key), b.get(key), &format!("{path}/{key}"), emit);
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            for i in 0..a.len().max(b.len()) {
                walk(a.get(i), b.get(i), &format!("{path}/{i}"), emit);
            }
        }
        (Some(a), Some(b)) if crate::parity::equal(a, b) => {}
        _ => emit(path, a, b),
    }
}

/// Compare canonical domain records. Zero differences is numerical parity only;
/// coverage availability and omissions are always reported separately.
pub fn domain_parity(
    java: &Path,
    rust: &Path,
    ledger: Option<&Path>,
) -> Result<DomainParityReport, CliError> {
    let a = document(java)?;
    let b = document(rust)?;
    if a.header.input_sha256 != b.header.input_sha256 {
        return Err(DomainError::InputMismatch.into());
    }
    if a.header.catalog_sha256 != b.header.catalog_sha256 {
        return Err(DomainError::CatalogMismatch.into());
    }
    let rules = crate::parity::domain_rules(ledger)?;
    let mut report = DomainParityReport::default();
    coverage(java, &a, "java", &mut report)?;
    coverage(rust, &b, "rust", &mut report)?;
    for order in a
        .records
        .keys()
        .chain(b.records.keys())
        .collect::<BTreeSet<_>>()
    {
        let av = a.records.get(order);
        let bv = b.records.get(order);
        let row = av.or(bv).ok_or(DomainError::Record)?;
        let kind = row["kind"].as_str().ok_or(DomainError::Record)?;
        let key = row["key"].as_str().ok_or(DomainError::Record)?;
        report.compared += 1;
        let mut av = av.cloned();
        let mut bv = bv.cloned();
        if kind == "run" {
            if let (Some(a), Some(b)) = (&mut av, &mut bv) {
                if let (Some(a), Some(b)) = (
                    a["value"]["evidence"].as_array_mut(),
                    b["value"]["evidence"].as_array_mut(),
                ) {
                    let gaps = |rows: &Vec<Value>| {
                        rows.iter()
                            .filter(|v| v["kind"] == "capture_gap")
                            .cloned()
                            .collect::<Vec<_>>()
                    };
                    let other = |rows: &Vec<Value>| {
                        rows.iter()
                            .filter(|v| v["kind"] != "capture_gap")
                            .cloned()
                            .collect::<Vec<_>>()
                    };
                    // Only pure gap changes are classified. Mixed changes keep
                    // their original paths and exact ledger-rule comparisons.
                    let java_gaps = gaps(a);
                    let rust_gaps = gaps(b);
                    let mut remaining = java_gaps.clone();
                    let subset = rust_gaps.iter().all(|gap| {
                        if let Some(index) = remaining.iter().position(|v| v == gap) {
                            remaining.remove(index);
                            true
                        } else {
                            false
                        }
                    });
                    if java_gaps != rust_gaps && subset && other(a) == other(b) {
                        scoped_coverage_difference(&mut report);
                        a.retain(|v| v["kind"] != "capture_gap");
                        b.retain(|v| v["kind"] != "capture_gap");
                    }
                }
            }
        }
        let mut emit = |path: &str, x: Option<&Value>, y: Option<&Value>| {
            if matches!(kind, "fight" | "run")
                && path == "/value/completeCapture"
                && x.and_then(Value::as_bool) == Some(false)
                && y.and_then(Value::as_bool) == Some(true)
            {
                scoped_coverage_difference(&mut report);
            } else if let Some(rule) = rules.iter().find(|r| {
                r.fixture_sha256 == a.header.input_sha256
                    && r.catalog_sha256 == a.header.catalog_sha256
                    && r.kind == kind
                    && r.record_key == key
                    && r.path == path
                    && r.java_sha256 == value_hash(x)
                    && r.maxed_sha256 == value_hash(y)
            }) {
                report.suppressed += 1;
                *report.applied_ids.entry(rule.id.clone()).or_default() += 1;
            } else {
                report.unexplained += 1;
                *report.by_kind.entry(kind.into()).or_default() += 1;
                // Aggregate schema fields across array positions. Exact pointers
                // remain unchanged for the ledger above and detailed diffs below.
                let field = path
                    .split('/')
                    .map(|part| if ordinal(part).is_some() { "*" } else { part })
                    .collect::<Vec<_>>()
                    .join("/");
                *report.by_field.entry(format!("{kind}{field}")).or_default() += 1;
                if report.diffs.len() < 100 {
                    report.diffs.push(DomainDiff {
                        kind: kind.into(),
                        key: key.into(),
                        path: path.into(),
                    });
                }
            }
        };
        match (av.as_ref(), bv.as_ref()) {
            (Some(av), Some(bv)) => walk(av.get("value"), bv.get("value"), "/value", &mut emit),
            (av, bv) => emit("", av, bv),
        }
    }
    Ok(report)
}
