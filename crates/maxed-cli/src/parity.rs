//! Metadata-only comparison of private canonical exports. No packet values in reports.
use crate::CliError;
use maxed_protocol::{
    canonical::lower_hex,
    defs::{known_name, schema_field},
    CanonicalLine,
};
use serde::Deserialize;
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};
/// A 200,000-byte frame can expand heavily when three-byte stats become named
/// JSON objects. Allow a conservative 48x expansion plus envelope/newline space;
/// this also covers string escaping (up to 6x) and numeric formatting.
pub const MAX_CANONICAL_LINE_BYTES: usize = maxed_protocol::MAX_FRAME * 48 + 1024;
const MAX_LEDGER: u64 = 1_048_576;

pub struct Diff {
    pub dir: String,
    pub ordinal: u64,
    pub id: Option<u8>,
    pub path: String,
}
pub struct ParityReport {
    pub compared: u64,
    pub unexplained: u64,
    pub suppressed: u64,
    pub by_type: BTreeMap<String, u64>,
    pub diffs: Vec<Diff>,
}
impl ParityReport {
    fn empty() -> Self {
        Self {
            compared: 0,
            unexplained: 0,
            suppressed: 0,
            by_type: BTreeMap::new(),
            diffs: vec![],
        }
    }
}
pub fn direction_lines<'a>(lines: &'a [CanonicalLine], dir: &str) -> Vec<&'a CanonicalLine> {
    lines.iter().filter(|line| line.dir == dir).collect()
}

// Decimal comparison avoids lossy integer-to-f64 casts (especially above 2^53).
fn numeric(n: &Number) -> (bool, String, i32) {
    let s = n.to_string();
    let (negative, s) = if let Some(s) = s.strip_prefix('-') {
        (true, s)
    } else {
        (false, s.as_str())
    };
    let (mantissa, exponent) = s
        .split_once(['e', 'E'])
        .map_or((s, 0), |(a, b)| (a, b.parse::<i32>().unwrap_or(0)));
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len() as i32);
    let mut digits = mantissa
        .replace('.', "")
        .trim_start_matches('0')
        .to_string();
    let mut power = exponent - fraction;
    if digits.is_empty() {
        return (false, "0".into(), 0);
    }
    while digits.ends_with('0') {
        digits.pop();
        power += 1;
    }
    (negative, digits, power)
}
pub(crate) fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => numeric(a) == numeric(b),
        _ => a == b,
    }
}
type Emit<'a> = dyn FnMut(&str, Option<&Value>, Option<&Value>, bool) + 'a;
fn walk(a: Option<&Value>, b: Option<&Value>, path: &str, emit: &mut Emit<'_>) {
    match (a, b) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for key in keys {
                if let Some(field) = schema_field(key) {
                    walk(a.get(key), b.get(key), &format!("{path}/{field}"), emit);
                } else if a.get(key) != b.get(key) {
                    emit(path, a.get(key), b.get(key), false);
                }
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            for n in 0..a.len().max(b.len()) {
                walk(a.get(n), b.get(n), &format!("{path}/{n}"), emit);
            }
        }
        (Some(a), Some(b)) if equal(a, b) => {}
        _ => emit(path, a, b, true),
    }
}
fn line_value(line: &CanonicalLine) -> Value {
    serde_json::json!({"t":line.t,"id":line.id,"name":line.name,"f":line.f})
}
fn compare(
    java: &[CanonicalLine],
    rust: &[CanonicalLine],
    limit: usize,
    rules: &[Rule],
) -> (ParityReport, BTreeMap<String, u64>) {
    let mut report = ParityReport::empty();
    let mut suppressed_ids = BTreeMap::new();
    for dir in ["c2s", "s2c"] {
        let a = direction_lines(java, dir);
        let b = direction_lines(rust, dir);
        for ordinal in 0..a.len().max(b.len()) {
            let left = a.get(ordinal).copied();
            let right = b.get(ordinal).copied();
            report.compared += 1;
            let types: BTreeSet<_> = [left, right]
                .into_iter()
                .flatten()
                .map(|line| known_name(line.id))
                .collect();
            let id = left.or(right).map(|l| l.id);
            let mut emit = |path: &str, av: Option<&Value>, bv: Option<&Value>, eligible: bool| {
                let rule = if eligible {
                    rules.iter().find(|r| {
                        r.dir == dir
                            && r.ordinal == ordinal as u64
                            && Some(r.packet_id) == id
                            && r.path == path
                            && left.is_none_or(|l| l.id == r.packet_id)
                            && right.is_none_or(|l| l.id == r.packet_id)
                            && r.java_sha256 == value_hash(av)
                            && r.maxed_sha256 == value_hash(bv)
                    })
                } else {
                    None
                };
                if let Some(rule) = rule {
                    report.suppressed += 1;
                    *suppressed_ids.entry(rule.id.clone()).or_insert(0) += 1;
                } else {
                    report.unexplained += 1;
                    for name in &types {
                        *report.by_type.entry((*name).into()).or_insert(0) += 1;
                    }
                    if report.diffs.len() < limit {
                        report.diffs.push(Diff {
                            dir: dir.into(),
                            ordinal: ordinal as u64,
                            id,
                            path: path.into(),
                        });
                    }
                }
            };
            match (left, right) {
                (Some(a), Some(b)) => {
                    for (path, x, y) in [
                        ("/t", Value::from(a.t), Value::from(b.t)),
                        ("/id", Value::from(a.id), Value::from(b.id)),
                        (
                            "/name",
                            Value::from(a.name.clone()),
                            Value::from(b.name.clone()),
                        ),
                    ] {
                        if !equal(&x, &y) {
                            emit(path, Some(&x), Some(&y), true);
                        }
                    }
                    walk(Some(&a.f), Some(&b.f), "/f", &mut emit);
                }
                _ => {
                    let av = left.map(line_value);
                    let bv = right.map(line_value);
                    emit("", av.as_ref(), bv.as_ref(), true);
                }
            }
        }
    }
    (report, suppressed_ids)
}
pub fn compare_lines(java: &[CanonicalLine], rust: &[CanonicalLine], limit: usize) -> ParityReport {
    compare(java, rust, limit, &[]).0
}

fn valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_path(path: &str) -> bool {
    if matches!(path, "" | "/t" | "/id" | "/name" | "/f") {
        return true;
    }
    let Some(tail) = path.strip_prefix("/f/") else {
        return false;
    };
    tail.split('/').all(|p| {
        schema_field(p).is_some()
            || (!p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<usize>().is_ok()
                && (p == "0" || !p.starts_with('0')))
    })
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rule {
    #[serde(skip)]
    id: String,
    fixture_sha256: String,
    dir: String,
    ordinal: u64,
    packet_id: u8,
    path: String,
    java_sha256: String,
    maxed_sha256: String,
}
fn ledger(path: Option<&Path>) -> Result<Vec<Rule>, CliError> {
    Ok(ledger_all(path)?.0)
}
pub(crate) fn domain_rules(
    path: Option<&Path>,
) -> Result<Vec<crate::domain_compare::DomainRule>, CliError> {
    Ok(ledger_all(path)?.1)
}
type LedgerRules = (Vec<Rule>, Vec<crate::domain_compare::DomainRule>);
fn ledger_all(path: Option<&Path>) -> Result<LedgerRules, CliError> {
    let Some(path) = path else {
        return Ok((vec![], vec![]));
    };
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_LEDGER {
        return Err(CliError::InvalidArguments);
    }
    let mut text = String::new();
    file.take(MAX_LEDGER + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_LEDGER {
        return Err(CliError::InvalidArguments);
    }
    let mut header = false;
    let mut rules = vec![];
    let mut domain = vec![];
    let mut domain_selectors = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut selectors = BTreeSet::new();
    for line in text.lines().map(str::trim).filter(|s| s.starts_with('|')) {
        let cells: Vec<_> = line
            .strip_prefix('|')
            .and_then(|s| s.strip_suffix('|'))
            .ok_or(CliError::InvalidArguments)?
            .split('|')
            .map(str::trim)
            .collect();
        if !header {
            if cells
                != [
                    "id",
                    "area",
                    "Java behavior",
                    "Maxed behavior",
                    "reason",
                    "evidence",
                ]
            {
                return Err(CliError::InvalidArguments);
            }
            header = true;
            continue;
        }
        if cells.len() != 6 {
            return Err(CliError::InvalidArguments);
        }
        if cells
            .iter()
            .all(|c| c.len() >= 3 && c.bytes().all(|b| b == b'-' || b == b':'))
        {
            continue;
        }
        let id = cells[0];
        if id.len() != 10
            || !id.starts_with("MAXED-")
            || !id[6..].bytes().all(|b| b.is_ascii_digit())
            || !ids.insert(id.to_string())
            || cells.iter().any(|c| c.is_empty())
        {
            return Err(CliError::InvalidArguments);
        }
        if let Some(text) = cells[5].strip_prefix("domain-rule:") {
            let rule = crate::domain_compare::DomainRule::parse(text.trim(), id)?;
            let (fixture, catalog, kind, key, path) = rule.selector();
            if !domain_selectors.insert((
                fixture.to_owned(),
                catalog.to_owned(),
                kind.to_owned(),
                key.to_owned(),
                path.to_owned(),
            )) {
                return Err(CliError::InvalidArguments);
            }
            domain.push(rule);
            continue;
        }
        let rule_text = cells[5]
            .strip_prefix("parity-rule:")
            .ok_or(CliError::InvalidArguments)?
            .trim();
        let mut rule: Rule =
            serde_json::from_str(rule_text).map_err(|_| CliError::InvalidArguments)?;
        rule.id = id.into();
        if !valid_hash(&rule.fixture_sha256)
            || !valid_hash(&rule.java_sha256)
            || !valid_hash(&rule.maxed_sha256)
            || !matches!(rule.dir.as_str(), "c2s" | "s2c")
            || !valid_path(&rule.path)
            || !selectors.insert((
                rule.fixture_sha256.clone(),
                rule.dir.clone(),
                rule.ordinal,
                rule.packet_id,
                rule.path.clone(),
            ))
        {
            return Err(CliError::InvalidArguments);
        }
        rules.push(rule);
    }
    if !header {
        return Err(CliError::InvalidArguments);
    }
    Ok((rules, domain))
}

/// Hash compact, sorted-key JSON. Missing values hash the distinct non-JSON token `missing`.
pub fn value_hash(value: Option<&Value>) -> String {
    let bytes = match value {
        Some(v) => v.to_string().into_bytes(),
        None => b"missing".to_vec(),
    };
    lower_hex(&Sha256::digest(bytes))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    input_sha256: String,
    #[serde(default, rename = "fieldFailures")]
    _field_failures: u64,
    #[serde(default, rename = "trailingBytePackets")]
    _trailing_byte_packets: u64,
    #[serde(default, rename = "unknownIdPackets")]
    _unknown_id_packets: u64,
    #[serde(default, rename = "wrongDirectionPackets")]
    _wrong_direction_packets: u64,
    #[serde(default, rename = "unverifiedPackets")]
    _unverified_packets: u64,
    #[serde(default, rename = "truncated")]
    _truncated: bool,
}
fn metadata(path: &Path) -> Result<String, CliError> {
    let path = crate::decode::metadata_path(path);
    crate::decode::private_existing(&path)?;
    let file = File::open(path)?;
    if file.metadata()?.len() > 4096 {
        return Err(CliError::InvalidArguments);
    }
    let mut bytes = vec![];
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(CliError::InvalidArguments);
    }
    let m: Metadata = serde_json::from_slice(&bytes).map_err(|_| CliError::InvalidArguments)?;
    if !valid_hash(&m.input_sha256) {
        return Err(CliError::InvalidArguments);
    }
    Ok(m.input_sha256)
}
fn read_lines(path: &Path) -> Result<Vec<CanonicalLine>, CliError> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut lines = vec![];
    loop {
        let mut bytes = Vec::new();
        let n = (&mut reader)
            .take((MAX_CANONICAL_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if n == 0 {
            break;
        }
        if n > MAX_CANONICAL_LINE_BYTES || bytes.last() != Some(&b'\n') {
            return Err(CliError::InvalidArguments);
        }
        let line: CanonicalLine =
            serde_json::from_slice(&bytes).map_err(|_| CliError::InvalidArguments)?;
        if line.i != lines.len() as u64
            || !matches!(line.dir.as_str(), "c2s" | "s2c")
            || !line.f.is_object()
        {
            return Err(CliError::InvalidArguments);
        }
        lines.push(line);
    }
    Ok(lines)
}
fn run(
    java: &Path,
    rust: &Path,
    ledger_path: Option<&Path>,
    limit: usize,
    fixture: Option<&str>,
    explicit_required: bool,
) -> Result<(ParityReport, BTreeMap<String, u64>), CliError> {
    let mut rules = ledger(ledger_path)?;
    if !rules.is_empty() || fixture.is_some() {
        if explicit_required && fixture.is_none() {
            return Err(CliError::InvalidArguments);
        }
        let a = metadata(java)?;
        let b = metadata(rust)?;
        if a != b || fixture.is_some_and(|f| !valid_hash(f) || f != a) {
            return Err(CliError::InvalidArguments);
        }
        // Rules for other fixtures are valid ledger entries, but cannot suppress this comparison.
        rules.retain(|r| r.fixture_sha256 == a);
    }
    let a = read_lines(java)?;
    let b = read_lines(rust)?;
    Ok(compare(&a, &b, limit, &rules))
}
pub fn parity(
    java: &Path,
    rust: &Path,
    ledger: Option<&Path>,
    limit: usize,
) -> Result<ParityReport, CliError> {
    Ok(run(java, rust, ledger, limit, None, false)?.0)
}
/// CLI wrapper: reviewed ledger rules additionally require an explicit fixture digest.
pub fn parity_with_fixture(
    java: &Path,
    rust: &Path,
    ledger: Option<&Path>,
    limit: usize,
    fixture: Option<&str>,
) -> Result<(ParityReport, BTreeMap<String, u64>), CliError> {
    run(java, rust, ledger, limit, fixture, true)
}
