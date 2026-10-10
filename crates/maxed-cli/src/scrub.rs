//! Whole-file fixture certification. No wire values are logged or serialized here.
use crate::{CliError, Pipeline};
use maxed_capture::parse_frame;
use maxed_protocol::{
    decode_packet, key_bytes,
    privacy::{
        inventory_with_context, FieldEvidence, FieldPolicy, Inventory, ObjectContext, SecretKind,
    },
    DecodeEvent, Diagnostic, Rc4,
};
use maxed_recording::{PcapReader, PcapWriter};
use maxed_types::{Direction, FlowKey, RawFrame};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Opt-in is required for provisional fixtures containing broken typed layouts.
/// Filled bodies cannot be used to validate decoding of those packets against
/// the original recording; regenerate such fixtures if decoding support changes.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrubOptions {
    pub allow_undecodable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScrubReason {
    OutsideSchema,
    UndecodableTyped,
    UnclassifiedFields,
    TrailingBytes,
    UnsupportedEnvelope,
    MissingSyn,
    MissingBytes,
    AmbiguousSequence,
    ConflictingOverlap,
    InvalidFraming,
    NamespaceExhausted,
    ResidualCollision,
    PublicLiteralCoincidence,
    OpaqueTokenCoincidence,
    VerificationFailed,
    ResourceLimit,
    InvalidArguments,
    Io,
    InvalidRecording,
    EmptyRecording,
    UnresolvedAlignment,
}
impl ScrubReason {
    fn label(self) -> &'static str {
        match self {
            Self::OutsideSchema => {
                "packets outside the typed schema set (eligible for body filler)"
            }
            Self::UndecodableTyped => "typed packets failed field decoding",
            Self::UnclassifiedFields => "typed packets contain structurally uncertain fields",
            Self::TrailingBytes => "typed packets contain trailing bytes",
            Self::UnsupportedEnvelope => "unsupported or truncated frame envelopes",
            Self::MissingSyn => {
                "directions missing a captured SYN; alignment and whole-stream coverage unproven"
            }
            Self::MissingBytes => "streams contain unobserved bytes",
            Self::AmbiguousSequence => "ambiguous sequence ranges",
            Self::ConflictingOverlap => "conflicting retransmissions or overlaps",
            Self::InvalidFraming => "streams contain incomplete or invalid packet framing",
            Self::NamespaceExhausted => "pseudonym or endpoint alias namespaces exhausted",
            Self::OpaqueTokenCoincidence => {
                "short opaque token interior (non-numeric, 4-7 bytes; UNIQUE_DATA_STRING in NEWTICK/UPDATE; exact/comma-delimited matches refuse)"
            }
            Self::PublicLiteralCoincidence => "public-literal coincidence",
            Self::ResidualCollision => "unexplained secret-value collisions in retained fields",
            Self::VerificationFailed => "output verification failures",
            Self::ResourceLimit => "recording admission resource limits exceeded",
            Self::InvalidArguments => {
                "invalid arguments, existing destination, or unsafe output location"
            }
            Self::Io => "file I/O failures",
            Self::InvalidRecording => "invalid or truncated pcap recordings",
            Self::EmptyRecording => "recordings contain no complete packets",
            Self::UnresolvedAlignment => "replay streams have unresolved alignment or recovery",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecretClass {
    Credential,
    Identity,
    PlayerText,
}
impl SecretClass {
    fn label(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::Identity => "identity",
            Self::PlayerText => "player-text",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CollisionEvidence {
    pub id: u8,
    pub direction: usize,
    pub field: &'static str,
    pub secret_class: SecretClass,
    pub reason: ScrubReason,
}
/// Metadata only: IDs, schema-authored names/directions, reasons and counts.
#[derive(Clone, Debug, Default)]
pub struct ScrubDiagnostics {
    pub packets: BTreeMap<ScrubReason, BTreeMap<(u8, usize), u64>>,
    pub items: BTreeMap<ScrubReason, u64>,
    pub scan_complete: bool,
    /// Packet ID, direction, schema field name, treatment; never wire values.
    pub fields: BTreeMap<(u8, usize, &'static str, &'static str), u64>,
    pub field_evidence: BTreeMap<(u8, usize, FieldEvidence), u64>,
    /// Counts retained field/class matches, deduplicated across equal source values.
    pub collisions: BTreeMap<CollisionEvidence, u64>,
}
impl ScrubDiagnostics {
    fn packet(&mut self, reason: ScrubReason, id: u8, d: usize) {
        *self
            .packets
            .entry(reason)
            .or_default()
            .entry((id, d))
            .or_default() += 1;
    }
    fn item(&mut self, reason: ScrubReason) {
        *self.items.entry(reason).or_default() += 1;
    }
    pub fn count(&self, reason: ScrubReason) -> u64 {
        self.items.get(&reason).copied().unwrap_or(0)
            + self.packets.get(&reason).map_or(0, |m| m.values().sum())
    }
    pub fn field_count(&self, treatment: &str) -> u64 {
        self.fields
            .iter()
            .filter(|(key, _)| key.3 == treatment)
            .map(|(_, count)| count)
            .sum()
    }
    fn merge(&mut self, other: Self) {
        self.scan_complete |= other.scan_complete;
        for (key, count) in other.collisions {
            *self.collisions.entry(key).or_default() += count;
        }
        for (key, count) in other.field_evidence {
            *self.field_evidence.entry(key).or_default() += count;
        }
        for (key, count) in other.fields {
            *self.fields.entry(key).or_default() += count;
        }
        for (reason, count) in other.items {
            *self.items.entry(reason).or_default() += count;
        }
        for (reason, packets) in other.packets {
            for (key, count) in packets {
                *self
                    .packets
                    .entry(reason)
                    .or_default()
                    .entry(key)
                    .or_default() += count;
            }
        }
    }
    fn rows(&self) -> serde_json::Value {
        let mut rows = vec![];
        for reason in self
            .items
            .keys()
            .chain(self.packets.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            let packets:Vec<_>=self.packets.get(&reason).into_iter().flat_map(|p|p.iter()).map(|(&(id,d),&count)|serde_json::json!({"id":id,"name":maxed_protocol::defs::known_name(id),"direction":dir_name(d),"count":count})).collect();
            rows.push(serde_json::json!({"reason":reason.label(),"count":self.count(reason),"packets":packets}));
        }
        let fields: Vec<_> = self.fields.iter().map(|(&(id, d, name, treatment), &count)|
            serde_json::json!({"id":id,"packet":maxed_protocol::defs::known_name(id),"direction":dir_name(d),"field":name,"treatment":treatment,"count":count})).collect();
        let evidence: Vec<_> = self.field_evidence.iter().map(|(&(id, d, item), &count)|
            serde_json::json!({"id":id,"packet":maxed_protocol::defs::known_name(id),"direction":dir_name(d),"field":item.field,"statTypeNum":item.stat_id,"reason":item.reason.label(),"refuses":item.reason.refuses(),"count":count})).collect();
        let collisions: Vec<_> = self.collisions.iter().map(|(item, count)|
            serde_json::json!({"id":item.id,"packet":maxed_protocol::defs::known_name(item.id),"direction":dir_name(item.direction),"field":item.field,"secretClass":item.secret_class.label(),"reason":item.reason.label(),"count":count})).collect();
        serde_json::json!({"scanComplete":self.scan_complete,"reasons":rows,"fields":fields,"fieldEvidence":evidence,"collisions":collisions})
    }
}
fn dir_name(d: usize) -> &'static str {
    if d == 0 {
        "c2s"
    } else {
        "s2c"
    }
}
impl std::fmt::Display for ScrubDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for reason in self
            .items
            .keys()
            .chain(self.packets.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if !first {
                write!(f, "; ")?;
            }
            first = false;
            write!(f, "{} {}", self.count(reason), reason.label())?;
            if let Some(packets) = self.packets.get(&reason) {
                write!(f, " (")?;
                for (i, (&(id, d), count)) in packets.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(
                        f,
                        "{} id={} {} x{}",
                        maxed_protocol::defs::known_name(id),
                        id,
                        dir_name(d),
                        count
                    )?;
                }
                write!(f, ")")?;
            }
        }
        for (&(id, d, name, treatment), count) in &self.fields {
            if !first {
                write!(f, "; ")?;
            }
            first = false;
            write!(
                f,
                "{} {} {} id={} {} field={}",
                count,
                treatment,
                maxed_protocol::defs::known_name(id),
                id,
                dir_name(d),
                name
            )?;
        }
        for (&(id, d, item), count) in &self.field_evidence {
            if !first {
                write!(f, "; ")?;
            }
            first = false;
            write!(
                f,
                "{} {} id={} {} field={}",
                count,
                maxed_protocol::defs::known_name(id),
                id,
                dir_name(d),
                item.field
            )?;
            if let Some(stat_id) = item.stat_id {
                write!(f, " statTypeNum={stat_id}")?;
            }
            write!(
                f,
                " reason={} refuses={}",
                item.reason.label(),
                item.reason.refuses()
            )?;
        }
        for (item, count) in &self.collisions {
            if !first {
                write!(f, "; ")?;
            }
            first = false;
            write!(
                f,
                "{} {} {} id={} {} field={} secret-class={}",
                count,
                item.reason.label(),
                maxed_protocol::defs::known_name(item.id),
                item.id,
                dir_name(item.direction),
                item.field,
                item.secret_class.label()
            )?;
        }
        if !self.scan_complete {
            write!(
                f,
                "; scan incomplete: packet counts cover only fully framed, decryptable streams"
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, thiserror::Error)]
#[error("refused: {diagnostics}{guidance}")]
pub struct ScrubFailure {
    pub diagnostics: Box<ScrubDiagnostics>,
    guidance: &'static str,
}
impl ScrubFailure {
    fn new(reason: ScrubReason) -> Self {
        let mut diagnostics = ScrubDiagnostics::default();
        diagnostics.item(reason);
        Self {
            diagnostics: Box::new(diagnostics),
            guidance: "",
        }
    }
}
impl From<CliError> for ScrubFailure {
    fn from(e: CliError) -> Self {
        Self::new(match e {
            CliError::InvalidArguments => ScrubReason::InvalidArguments,
            CliError::Io(_) => ScrubReason::Io,
            CliError::Recording(_) => ScrubReason::InvalidRecording,
            CliError::Parse(_) => ScrubReason::UnsupportedEnvelope,
            _ => ScrubReason::VerificationFailed,
        })
    }
}
impl From<std::io::Error> for ScrubFailure {
    fn from(_: std::io::Error) -> Self {
        Self::new(ScrubReason::Io)
    }
}
impl From<maxed_recording::RecordingError> for ScrubFailure {
    fn from(_: maxed_recording::RecordingError) -> Self {
        Self::new(ScrubReason::InvalidRecording)
    }
}
impl From<maxed_capture::ParseError> for ScrubFailure {
    fn from(_: maxed_capture::ParseError) -> Self {
        Self::new(ScrubReason::UnsupportedEnvelope)
    }
}
impl From<maxed_protocol::ProtocolError> for ScrubFailure {
    fn from(_: maxed_protocol::ProtocolError) -> Self {
        Self::new(ScrubReason::VerificationFailed)
    }
}
#[derive(Debug)]
pub struct ScrubReport {
    pub frames: u64,
    pub changed_spans: u64,
    pub verified: bool,
    pub diagnostics: ScrubDiagnostics,
}
fn refuse<T>(reason: ScrubReason) -> Result<T, ScrubFailure> {
    Err(ScrubFailure::new(reason))
}
pub fn rewrite_ciphertext(
    cipher: &[u8],
    old: &[u8],
    replacement: &[u8],
) -> Result<Vec<u8>, CliError> {
    if cipher.len() != old.len() || old.len() != replacement.len() {
        return Err(CliError::InvalidArguments);
    }
    Ok(cipher
        .iter()
        .zip(old)
        .zip(replacement)
        .map(|((&c, &o), &r)| c ^ o ^ r)
        .collect())
}

// Explicit resource limits refuse admission, never discard unexamined bytes.
const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_STREAM: usize = 16 * 1024 * 1024;
const MAX_FRAMES: usize = 250_000;
struct Occurrence {
    frame: usize,
    raw: usize,
    start: usize,
    len: usize,
}
struct Half {
    syn: u32,
    bytes: Vec<u8>,
    seen: Vec<bool>,
    occurrences: Vec<Occurrence>,
}
impl Half {
    fn new(syn: u32) -> Self {
        Self {
            syn,
            bytes: vec![],
            seen: vec![],
            occurrences: vec![],
        }
    }
    fn offset(&self, seq: u32) -> Option<usize> {
        let offset = seq.wrapping_sub(self.syn.wrapping_add(1));
        (offset < MAX_STREAM as u32).then_some(offset as usize)
    }
    fn overlaps(&self, seq: u32, len: usize) -> bool {
        let start = i64::from(seq.wrapping_sub(self.syn.wrapping_add(1)) as i32);
        start < self.bytes.len() as i64 && start + len as i64 > 0
    }
}
struct Generation {
    flow: FlowKey,
    halves: [Option<Half>; 2],
}
fn di(dir: Direction) -> usize {
    usize::from(dir == Direction::ServerToClient)
}
fn direction(i: usize) -> Direction {
    if i == 0 {
        Direction::ClientToServer
    } else {
        Direction::ServerToClient
    }
}

/// Strict supported envelope: Ethernet/IPv4/TCP, known TCP options, no opaque padding.
fn envelope(frame: &RawFrame) -> Result<(), ScrubFailure> {
    let b = &frame.bytes;
    if b.len() != frame.original_len as usize
        || b.len() < 54
        || b[12..14] != [8, 0]
        || b[14] != 0x45
    {
        return refuse(ScrubReason::UnsupportedEnvelope);
    }
    let total = usize::from(u16::from_be_bytes([b[16], b[17]]));
    if total < 40
        || total + 14 > b.len()
        || b[23] != 6
        || u16::from_be_bytes([b[20], b[21]]) & 0x3fff != 0
    {
        return refuse(ScrubReason::UnsupportedEnvelope);
    }
    if b[14 + total..].iter().any(|&v| v != 0) {
        return refuse(ScrubReason::UnsupportedEnvelope);
    }
    let head = usize::from(b[46] >> 4) * 4;
    if head < 20 || head + 20 > total || b[46] & 15 != 0 || b[47] & 0xc0 != 0 {
        return refuse(ScrubReason::UnsupportedEnvelope);
    }
    let mut p = 54;
    while p < 34 + head {
        match b[p] {
            0 => {
                if b[p..34 + head].iter().any(|&v| v != 0) {
                    return refuse(ScrubReason::UnsupportedEnvelope);
                }
                break;
            }
            1 => p += 1,
            kind => {
                let Some(&len) = b.get(p + 1) else {
                    return refuse(ScrubReason::UnsupportedEnvelope);
                };
                let len = usize::from(len);
                if p + len > 34 + head
                    || !matches!(
                        (kind, len),
                        (2, 4) | (3, 3) | (4, 2) | (5, 10 | 18 | 26 | 34) | (8, 10)
                    )
                {
                    return refuse(ScrubReason::UnsupportedEnvelope);
                }
                p += len;
            }
        }
    }
    Ok(())
}
fn assemble(frames: &[RawFrame]) -> Result<Vec<Generation>, ScrubFailure> {
    let mut diagnostics = ScrubDiagnostics::default();
    let mut seen = HashMap::new();
    for frame in frames {
        if envelope(frame).is_err() {
            diagnostics.item(ScrubReason::UnsupportedEnvelope);
            continue;
        }
        match parse_frame(frame) {
            Ok(Some(seg)) => {
                if seg.syn {
                    seen.insert((seg.flow, di(seg.dir)), true);
                } else if !seg.bytes.is_empty() && !seen.contains_key(&(seg.flow, di(seg.dir))) {
                    diagnostics.item(ScrubReason::MissingSyn);
                    seen.insert((seg.flow, di(seg.dir)), false);
                }
            }
            _ => diagnostics.item(ScrubReason::UnsupportedEnvelope),
        }
    }
    if !diagnostics.items.is_empty() {
        return Err(ScrubFailure {diagnostics:Box::new(diagnostics),guidance:"; record before connecting; unsupported frames require a supported Ethernet/IPv4/TCP capture"});
    }
    let mut generations: Vec<Generation> = vec![];
    let mut active = HashMap::<FlowKey, usize>::new();
    for (fi, frame) in frames.iter().enumerate() {
        envelope(frame)?;
        let Some(seg) = parse_frame(frame)? else {
            return refuse(ScrubReason::AmbiguousSequence);
        };
        let d = di(seg.dir);
        let old = active.get(&seg.flow).copied();
        let fresh = old.is_none()
            || old.is_some_and(|i| {
                seg.syn
                    && generations[i].halves[d]
                        .as_ref()
                        .is_some_and(|h| h.syn != seg.seq)
            });
        if fresh {
            if !seg.syn {
                return refuse(ScrubReason::MissingSyn);
            }
            generations.push(Generation {
                flow: seg.flow,
                halves: [None, None],
            });
            active.insert(seg.flow, generations.len() - 1);
        }
        let mut i = *active.get(&seg.flow).ok_or(CliError::InvalidArguments)?;
        if !seg.bytes.is_empty() {
            let seq = seg.seq.wrapping_add(u32::from(seg.syn));
            let retired: Vec<_> = generations
                .iter()
                .enumerate()
                .filter_map(|(j, g)| {
                    (j != i
                        && g.flow == seg.flow
                        && g.halves[d]
                            .as_ref()
                            .is_some_and(|h| h.overlaps(seq, seg.bytes.len())))
                    .then_some(j)
                })
                .collect();
            if !retired.is_empty() {
                // A delayed copy belongs to an older generation only when the
                // active sequence namespace cannot also contain it. Compare the
                // complete captured range; never extend/fill a retired stream.
                if seg.syn
                    || retired.len() != 1
                    || generations[i].halves[d]
                        .as_ref()
                        .is_none_or(|h| h.offset(seq).is_some())
                {
                    return refuse(ScrubReason::AmbiguousSequence);
                }
                i = retired[0];
                let h = generations[i].halves[d]
                    .as_ref()
                    .ok_or(CliError::InvalidArguments)?;
                let start = h
                    .offset(seq)
                    .ok_or_else(|| ScrubFailure::new(ScrubReason::AmbiguousSequence))?;
                let end = start
                    .checked_add(seg.bytes.len())
                    .ok_or_else(|| ScrubFailure::new(ScrubReason::ResourceLimit))?;
                if h.bytes.get(start..end) != Some(seg.bytes.as_slice())
                    || h.seen
                        .get(start..end)
                        .is_none_or(|seen| seen.contains(&false))
                {
                    return refuse(ScrubReason::AmbiguousSequence);
                }
            }
        }
        let slot = &mut generations[i].halves[d];
        if slot.is_none() {
            if !seg.syn {
                if seg.bytes.is_empty() {
                    continue;
                }
                return refuse(ScrubReason::AmbiguousSequence);
            }
            *slot = Some(Half::new(seg.seq));
        }
        let half = slot.as_mut().ok_or(CliError::InvalidArguments)?;
        if seg.bytes.is_empty() {
            continue;
        }
        let start = half
            .offset(seg.seq.wrapping_add(u32::from(seg.syn)))
            .ok_or_else(|| ScrubFailure::new(ScrubReason::AmbiguousSequence))?;
        let end = start
            .checked_add(seg.bytes.len())
            .filter(|&n| n <= MAX_STREAM)
            .ok_or_else(|| ScrubFailure::new(ScrubReason::ResourceLimit))?;
        half.bytes.resize(half.bytes.len().max(end), 0);
        half.seen.resize(half.seen.len().max(end), false);
        for (n, &b) in seg.bytes.iter().enumerate() {
            if half.seen[start + n] && half.bytes[start + n] != b {
                return refuse(ScrubReason::ConflictingOverlap);
            }
            half.bytes[start + n] = b;
            half.seen[start + n] = true;
        }
        half.occurrences.push(Occurrence {
            frame: fi,
            raw: seg.origin.offset,
            start,
            len: seg.bytes.len(),
        });
    }
    if generations
        .iter()
        .flat_map(|g| g.halves.iter().flatten())
        .any(|h| h.seen.contains(&false))
    {
        return refuse(ScrubReason::MissingBytes);
    }
    Ok(generations)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Treatment {
    Fields,
    OutsideSchema,
    Undecodable,
}
struct Body {
    treatment: Treatment,
    generation: usize,
    d: usize,
    start: usize,
    id: u8,
    plain: Vec<u8>,
    expected: Vec<u8>,
    inventory: Inventory,
}
fn bodies(generations: &[Generation]) -> Result<Vec<Body>, ScrubFailure> {
    let mut result = vec![];
    for (g, generation) in generations.iter().enumerate() {
        for (d, half) in generation.halves.iter().enumerate() {
            let Some(half) = half else {
                continue;
            };
            let mut cipher = Rc4::new(&key_bytes(direction(d))?)?;
            let mut start = 0;
            while start < half.bytes.len() {
                let b = &half.bytes[start..];
                if b.len() < 5 {
                    return refuse(ScrubReason::InvalidFraming);
                }
                let len = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
                if !(5..=maxed_protocol::MAX_FRAME).contains(&len) || len > b.len() {
                    return refuse(ScrubReason::InvalidFraming);
                }
                let id = b[4];
                let mut plain = b[5..len].to_vec();
                cipher.apply(&mut plain);
                let inventory = Inventory::default();
                result.push(Body {
                    treatment: Treatment::Fields,
                    generation: g,
                    d,
                    start: start + 5,
                    id,
                    expected: plain.clone(),
                    plain,
                    inventory,
                });
                start += len;
            }
        }
    }
    if result.is_empty() {
        return refuse(ScrubReason::EmptyRecording);
    }
    Ok(result)
}
fn classify(
    plans: &mut [Body],
    options: ScrubOptions,
    diagnostics: &mut ScrubDiagnostics,
) -> Result<(), ScrubFailure> {
    let mut rejected = false;
    let mut contexts = HashMap::<usize, ObjectContext>::new();
    for p in plans {
        let context = contexts.entry(p.generation).or_default();
        let reason = match decode_packet(p.id, direction(p.d), &p.plain) {
            Ok((maxed_protocol::Packet::Unknown { .. }, _)) => {
                p.treatment = Treatment::OutsideSchema;
                Some(ScrubReason::OutsideSchema)
            }
            Err(_) => {
                p.treatment = Treatment::Undecodable;
                Some(ScrubReason::UndecodableTyped)
            }
            Ok((_, left)) if left != 0 => {
                rejected = true;
                Some(ScrubReason::TrailingBytes)
            }
            Ok((packet, _)) if packet.fields().is_err() => {
                p.treatment = Treatment::Undecodable;
                Some(ScrubReason::UndecodableTyped)
            }
            Ok((packet, _)) => {
                match inventory_with_context(p.id, direction(p.d), &p.plain, context) {
                    Ok(i) => {
                        let structural = i.evidence.iter().any(|item| item.reason.refuses());
                        for &item in &i.evidence {
                            *diagnostics
                                .field_evidence
                                .entry((p.id, p.d, item))
                                .or_default() += 1;
                        }
                        for field in &i.treatments {
                            if field.len == 0 {
                                continue;
                            }
                            let treatment = match field.policy {
                                FieldPolicy::FillUnclassified => "filled-unclassified",
                                FieldPolicy::Pseudonym => "pseudonymized",
                                FieldPolicy::Keep => "kept-allowlisted",
                                FieldPolicy::Fill => "filled-sensitive",
                            };
                            *diagnostics
                                .fields
                                .entry((p.id, p.d, field.name, treatment))
                                .or_default() += 1;
                        }
                        p.inventory = i;
                        context.observe(&packet);
                        if structural {
                            rejected = true;
                            Some(ScrubReason::UnclassifiedFields)
                        } else {
                            None
                        }
                    }
                    Err(_) => {
                        rejected = true;
                        Some(ScrubReason::UnclassifiedFields)
                    }
                }
            }
        };
        // A filled lifecycle packet cannot safely carry object-type knowledge
        // forward, including packets whose fields fail canonical serialization.
        if p.treatment == Treatment::Undecodable && p.d == 1 && matches!(p.id, 42 | 92 | 45 | 101) {
            context.clear();
        }
        if let Some(reason) = reason {
            diagnostics.packet(reason, p.id, p.d);
        }
    }
    diagnostics.scan_complete = true;
    if rejected
        || (!options.allow_undecodable && diagnostics.count(ScrubReason::UndecodableTyped) > 0)
    {
        return Err(ScrubFailure {
            diagnostics: Box::new(diagnostics.clone()),
            guidance: if diagnostics.count(ScrubReason::UndecodableTyped) > 0
                && !options.allow_undecodable
            {
                "; typed decode failures require --allow-undecodable; provisional fixtures contain filled undecodable packets"
            } else if options.allow_undecodable {
                "; --allow-undecodable does not permit structural uncertainty or trailing bytes"
            } else {
                ""
            },
        });
    }
    Ok(())
}
// RealmShark outgoing/MovePacket.java reads tickId as its first readInt().
// Both MOVE and NEWTICK tick headers are needed by the existing RC4 aligner.
fn alignment_prefix(p: &Body) -> usize {
    if matches!((p.id, p.d), (62, 0) | (10, 1)) && p.plain.len() >= 4 {
        4
    } else {
        0
    }
}
fn aliases(values: BTreeSet<Vec<u8>>) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, ScrubFailure> {
    let mut groups = BTreeMap::<usize, Vec<Vec<u8>>>::new();
    for v in &values {
        if !v.is_empty() {
            groups.entry(v.len()).or_default().push(v.clone());
        }
    }
    let mut result = BTreeMap::new();
    for (len, originals) in groups {
        let mut code = vec![b'0'; len];
        let mut exhausted = false;
        for original in originals {
            loop {
                if exhausted {
                    return refuse(ScrubReason::NamespaceExhausted);
                }
                let candidate = code.clone();
                exhausted = !increment(&mut code);
                if !values.contains(&candidate) {
                    result.insert(original, candidate);
                    break;
                }
            }
        }
    }
    Ok(result)
}
fn increment(code: &mut [u8]) -> bool {
    for b in code.iter_mut().rev() {
        match *b {
            b'0'..=b'8' | b'a'..=b'y' => {
                *b += 1;
                return true;
            }
            b'9' => {
                *b = b'a';
                return true;
            }
            _ => *b = b'0',
        }
    }
    false
}
fn json_identity(body: &Body, offset: usize) -> bool {
    body.inventory
        .treatments
        .iter()
        .any(|field| field.offset == offset && matches!(field.name, "portalPlayer" | "deathPlayer"))
}
fn identity_value(body: &Body, offset: usize, len: usize) -> Result<Vec<u8>, ScrubFailure> {
    let value = &body.plain[offset..offset + len];
    if json_identity(body, offset) {
        Ok(serde_json::from_slice::<String>(value)
            .map_err(|_| maxed_protocol::ProtocolError::Definition)?
            .into_bytes())
    } else {
        Ok(value.to_vec())
    }
}
fn redact(bodies: &mut [Body]) -> Result<u64, ScrubFailure> {
    let mut originals = BTreeSet::new();
    for body in bodies.iter() {
        for s in &body.inventory.spans {
            if s.kind == SecretKind::Identity {
                originals.insert(identity_value(body, s.offset, s.len)?);
            }
        }
    }
    // Decoded notice identities join the same sorted, length-preserving namespace
    // as NAME_STAT, even if no player object (or only a later one) was observed.
    let names = aliases(originals)?;
    let mut player_aliases = BTreeMap::<Vec<u8>, BTreeSet<Vec<u8>>>::new();
    for body in bodies.iter() {
        for field in body
            .inventory
            .treatments
            .iter()
            .filter(|field| field.name == "NAME_STAT" && field.policy == FieldPolicy::Pseudonym)
        {
            let original = &body.plain[field.offset..field.offset + field.len];
            if let Some(alias) = names.get(original) {
                player_aliases
                    .entry(original.to_ascii_lowercase())
                    .or_default()
                    .insert(alias.clone());
            }
        }
    }
    let mut changed = 0;
    for body in bodies {
        if body.treatment != Treatment::Fields {
            let prefix = alignment_prefix(body);
            body.expected[prefix..].fill(0xff);
            changed += u64::from(prefix < body.plain.len());
            continue;
        }
        for s in &body.inventory.spans {
            if s.len == 0 {
                continue;
            }
            let replacement = match s.kind {
                SecretKind::Identity => {
                    let identity = identity_value(body, s.offset, s.len)?;
                    let alias = if json_identity(body, s.offset) {
                        // Own-pop suppression compares ASCII case-insensitively. A
                        // unique observed player alias preserves that comparison;
                        // conflicting NAME_STAT spellings cannot be certified.
                        match player_aliases.get(&identity.to_ascii_lowercase()) {
                            Some(aliases) if aliases.len() != 1 => {
                                return refuse(ScrubReason::VerificationFailed)
                            }
                            Some(aliases) => aliases.first(),
                            None if body.inventory.treatments.iter().any(|field| {
                                field.offset == s.offset && field.name == "deathPlayer"
                            }) =>
                            {
                                // A death must identify an observed player; invented
                                // aliases would silently lose departure cancellation.
                                return refuse(ScrubReason::VerificationFailed);
                            }
                            None => names.get(&identity),
                        }
                    } else {
                        names.get(&identity)
                    }
                    .ok_or(maxed_protocol::ProtocolError::Definition)?;
                    if json_identity(body, s.offset) {
                        let mut encoded = serde_json::to_vec(
                            std::str::from_utf8(alias)
                                .map_err(|_| maxed_protocol::ProtocolError::Definition)?,
                        )
                        .map_err(|_| maxed_protocol::ProtocolError::Definition)?;
                        if encoded.len() > s.len {
                            return refuse(ScrubReason::VerificationFailed);
                        }
                        encoded.resize(s.len, b' ');
                        encoded
                    } else {
                        alias.clone()
                    }
                }
                _ if body.inventory.binary.contains(&s.offset) => {
                    vec![0; s.len]
                }
                _ => vec![b'x'; s.len],
            };
            // Already-filler input is safe: verification checks replacement provenance.
            body.expected[s.offset..s.offset + s.len].copy_from_slice(&replacement);
            changed += 1;
        }
    }
    Ok(changed)
}
fn patch(
    frames: &mut [RawFrame],
    generations: &[Generation],
    bodies: &[Body],
) -> Result<(), ScrubFailure> {
    for body in bodies {
        let half = generations[body.generation].halves[body.d]
            .as_ref()
            .ok_or(CliError::InvalidArguments)?;
        let cipher = rewrite_ciphertext(
            &half.bytes[body.start..body.start + body.plain.len()],
            &body.plain,
            &body.expected,
        )?;
        for occurrence in &half.occurrences {
            let left = body.start.max(occurrence.start);
            let right = (body.start + body.plain.len()).min(occurrence.start + occurrence.len);
            if left < right {
                let raw = occurrence.raw + left - occurrence.start;
                frames[occurrence.frame].bytes[raw..raw + right - left]
                    .copy_from_slice(&cipher[left - body.start..right - body.start]);
            }
        }
    }
    Ok(())
}
fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(2) {
        sum += u32::from(u16::from_be_bytes([chunk[0], *chunk.get(1).unwrap_or(&0)]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 65535) + (sum >> 16);
    }
    !(sum as u16)
}
fn tcp_sum(b: &[u8]) -> u16 {
    let total = usize::from(u16::from_be_bytes([b[16], b[17]]));
    let mut bytes = b[26..34].to_vec();
    bytes.extend_from_slice(&[0, 6]);
    bytes.extend_from_slice(&((total - 20) as u16).to_be_bytes());
    bytes.extend_from_slice(&b[34..14 + total]);
    checksum(&bytes)
}
fn normalize(frames: &mut [RawFrame]) -> Result<(), ScrubFailure> {
    let mut ips = BTreeSet::new();
    let mut macs = BTreeSet::new();
    for f in frames.iter() {
        ips.insert(f.bytes[26..30].to_vec());
        ips.insert(f.bytes[30..34].to_vec());
        macs.insert(f.bytes[..6].to_vec());
        macs.insert(f.bytes[6..12].to_vec());
    }
    let mut ip_map = BTreeMap::new();
    let mut candidates = (1u8..=254)
        .flat_map(|n| {
            [
                [192, 0, 2, n].to_vec(),
                [198, 51, 100, n].to_vec(),
                [203, 0, 113, n].to_vec(),
            ]
        })
        .filter(|v| !ips.contains(v));
    for ip in &ips {
        ip_map.insert(
            ip.clone(),
            candidates
                .next()
                .ok_or(maxed_protocol::ProtocolError::Definition)?,
        );
    }
    let mut mac_map = BTreeMap::new();
    let mut next = 1u32;
    for mac in &macs {
        loop {
            let mut value = vec![2, 0];
            value.extend_from_slice(&next.to_be_bytes());
            next = next
                .checked_add(1)
                .ok_or(maxed_protocol::ProtocolError::Length)?;
            if !macs.contains(&value) {
                mac_map.insert(mac.clone(), value);
                break;
            }
        }
    }
    for f in frames {
        let b = &mut f.bytes;
        for start in [26, 30] {
            let value = ip_map
                .get(&b[start..start + 4])
                .ok_or(CliError::InvalidArguments)?;
            b[start..start + 4].copy_from_slice(value);
        }
        for start in [0, 6] {
            let value = mac_map
                .get(&b[start..start + 6])
                .ok_or(CliError::InvalidArguments)?;
            b[start..start + 6].copy_from_slice(value);
        }
        b[24..26].fill(0);
        let sum = checksum(&b[14..34]);
        b[24..26].copy_from_slice(&sum.to_be_bytes());
        b[50..52].fill(0);
        let sum = tcp_sum(b);
        b[50..52].copy_from_slice(&sum.to_be_bytes());
    }
    Ok(())
}
struct ReplayCheck {
    lines: Vec<maxed_protocol::CanonicalLine>,
    failures: usize,
    // Java-compatible classification diagnostics do not discard decoded packets.
    // Preserve their counts across scrubbing as well as the canonical fields.
    classification_counts: [usize; 2],
}
fn canonical(frames: &[RawFrame]) -> Result<ReplayCheck, ScrubFailure> {
    fn observe(
        events: Vec<DecodeEvent>,
        check: &mut ReplayCheck,
        teardown: bool,
    ) -> Result<(), ScrubFailure> {
        for event in events {
            match event {
                DecodeEvent::Packet(p) => {
                    match maxed_protocol::canonical_line(check.lines.len() as u64, &p) {
                        Ok(line) => check.lines.push(line),
                        Err(_) => check.failures += 1,
                    }
                }
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::BadFields,
                    ..
                } => check.failures += 1,
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::UnknownId,
                    ..
                } => check.classification_counts[0] += 1,
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::WrongDirection,
                    ..
                } => check.classification_counts[1] += 1,
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::Closed,
                    ..
                } => {}
                // Companion metadata only; the original diagnostic above still
                // determines certification and contributes to the totals.
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::Packet { .. },
                    ..
                } => {}
                // Only finish() produces normal teardown cancellation. A
                // cancellation while decoding input still fails certification.
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::Cancelled,
                    ..
                } if teardown => {}
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::Recovering | Diagnostic::Recovered,
                    ..
                } => return refuse(ScrubReason::UnresolvedAlignment),
                DecodeEvent::Diagnostic { .. } => return refuse(ScrubReason::VerificationFailed),
            }
        }
        Ok(())
    }
    let mut pipeline = Pipeline::default();
    let mut check = ReplayCheck {
        lines: vec![],
        failures: 0,
        classification_counts: [0; 2],
    };
    for frame in frames {
        observe(pipeline.push(frame.clone()), &mut check, false)?;
    }
    observe(pipeline.finish(), &mut check, true)?;
    Ok(check)
}
fn verify(
    original: &[RawFrame],
    output: &[RawFrame],
    plans: &[Body],
    diagnostics: &mut ScrubDiagnostics,
) -> Result<(), ScrubFailure> {
    if original.len() != output.len() {
        return refuse(ScrubReason::VerificationFailed);
    }
    for (a, b) in original.iter().zip(output) {
        if a.index != b.index
            || a.at != b.at
            || a.original_len != b.original_len
            || a.bytes.len() != b.bytes.len()
        {
            return refuse(ScrubReason::VerificationFailed);
        }
        envelope(b)?;
        let tcp_end = 34 + usize::from(b.bytes[46] >> 4) * 4;
        // Only endpoints and checksums may change in the envelope. In
        // particular, sequence/ack numbers and TCP options stay byte-identical.
        if a.bytes[12..24] != b.bytes[12..24]
            || a.bytes[34..50] != b.bytes[34..50]
            || a.bytes[52..tcp_end] != b.bytes[52..tcp_end]
        {
            return refuse(ScrubReason::VerificationFailed);
        }
        if checksum(&b.bytes[14..34]) != 0 || tcp_sum(&b.bytes) != 0 {
            return refuse(ScrubReason::VerificationFailed);
        }
    }
    // Independently reassemble the file that was re-opened from disk. This also
    // rechecks every overlapping/retransmitted ciphertext occurrence for equality.
    let streams = assemble(output)?;
    let decoded = bodies(&streams)?;
    if decoded.len() != plans.len() {
        return refuse(ScrubReason::VerificationFailed);
    }
    let mut secrets: BTreeSet<(SecretClass, Vec<u8>)> = plans
        .iter()
        .flat_map(|p| {
            p.inventory.spans.iter().filter(|s| s.len > 0).map(|s| {
                let class = match s.kind {
                    SecretKind::Identity => SecretClass::Identity,
                    SecretKind::Credential
                        if p.inventory.treatments.iter().any(|field| {
                            field.offset == s.offset
                                && matches!(
                                    field.name,
                                    "accountId"
                                        | "ACCOUNT_ID_STAT"
                                        | "OWNER_ACCOUNT_ID_STAT"
                                        | "GRAVE_ACCOUNT_ID"
                                )
                        }) =>
                    {
                        SecretClass::Identity
                    }
                    SecretKind::Credential => SecretClass::Credential,
                    SecretKind::Unclassified if p.inventory.binary.contains(&s.offset) => {
                        SecretClass::Credential
                    }
                    SecretKind::PlayerText | SecretKind::Unclassified => SecretClass::PlayerText,
                };
                (class, p.plain[s.offset..s.offset + s.len].to_vec())
            })
        })
        .collect();
    // KILLER is an identity even when JSON escapes conceal its wire spelling.
    // Retained announcement keys must be checked against the decoded value too.
    for p in plans {
        for field in p
            .inventory
            .treatments
            .iter()
            .filter(|f| matches!(f.name, "portalPlayer" | "deathPlayer"))
        {
            secrets.insert((
                SecretClass::Identity,
                identity_value(p, field.offset, field.len)?,
            ));
        }
        for field in p.inventory.treatments.iter().filter(|f| f.name == "KILLER") {
            if let Some(value) = p
                .plain
                .get(field.offset.saturating_sub(1)..field.offset + field.len + 1)
            {
                if let Ok(name) = serde_json::from_slice::<String>(value) {
                    secrets.insert((SecretClass::Identity, name.into_bytes()));
                }
            }
        }
    }
    for (actual, expected) in decoded.iter().zip(plans) {
        if actual.id != expected.id
            || actual.d != expected.d
            || actual.generation != expected.generation
            || actual.start != expected.start
            || actual.plain != expected.expected
        {
            return refuse(ScrubReason::VerificationFailed);
        }
        for field in &expected.inventory.treatments {
            // Replacement bytes cannot be retained source secrets. Kept fields
            // must still be checked, including exact public literals.
            if field.policy != FieldPolicy::Keep {
                continue;
            }
            let value = &actual.plain[field.offset..field.offset + field.len];
            let decoded_game_data = if matches!(field.name, "deathEnemy" | "deathLevel") {
                Some(
                    serde_json::from_slice::<String>(value)
                        .map_err(|_| maxed_protocol::ProtocolError::Definition)?,
                )
            } else {
                None
            };
            let opaque = field.name == "UNIQUE_DATA_STRING";
            let classes: BTreeSet<_> = secrets
                .iter()
                .filter(|(class, secret)| {
                    residual_collision(value, secret)
                        || decoded_game_data.as_ref().is_some_and(|v| {
                            if *class == SecretClass::Credential {
                                residual_collision(v.as_bytes(), secret)
                            } else {
                                // Match NAME_STAT's ASCII-insensitive identity
                                // semantics, and list chat coincidences under
                                // the same spelling comparison. Credentials
                                // retain their exact-byte matching semantics.
                                residual_collision_ignore_ascii_case(v.as_bytes(), secret)
                            }
                        })
                        || (opaque && delimited_collision(value, secret))
                        // Short credential and numeric identity values refuse
                        // on any substring, stricter than the old punctuation-
                        // bounded rule. Digit-only chat still uses the text floor.
                        || (opaque
                            && !secret.is_empty()
                            && secret.len() < 4
                            && (*class == SecretClass::Credential
                                || (*class == SecretClass::Identity
                                    && secret.iter().all(u8::is_ascii_digit)))
                            && value.windows(secret.len()).any(|part| part == secret))
                })
                .map(|(class, secret)| {
                    (
                        *class,
                        opaque
                            && *class != SecretClass::Credential
                            && secret.len() < 8
                            && opaque_token_interior(value, secret),
                    )
                })
                .collect();
            for (class, coincidence) in classes {
                let public_server_field = expected.d == 1
                    && (expected.id == 92
                        || (expected.id == 45 && matches!(field.name, "name" | "host"))
                        || (expected.id == 44
                            && matches!(field.name, "name" | "text" | "cleanText")));
                let reason = if coincidence {
                    ScrubReason::OpaqueTokenCoincidence
                } else if class == SecretClass::PlayerText
                    && (public_server_field || field.public_literal)
                {
                    ScrubReason::PublicLiteralCoincidence
                } else {
                    ScrubReason::ResidualCollision
                };
                diagnostics.packet(reason, expected.id, expected.d);
                *diagnostics
                    .collisions
                    .entry(CollisionEvidence {
                        id: expected.id,
                        direction: expected.d,
                        field: field.name,
                        secret_class: class,
                        reason,
                    })
                    .or_default() += 1;
            }
        }
    }
    if diagnostics.count(ScrubReason::ResidualCollision) > 0 {
        return Err(ScrubFailure {
            diagnostics: Box::new(diagnostics.clone()),
            guidance: "",
        });
    }
    let before = canonical(original)?;
    let after = canonical(output)?;
    let failed = plans
        .iter()
        .filter(|p| p.treatment == Treatment::Undecodable)
        .count();
    if before.failures != failed
        || after.failures != failed
        || before.lines.len() + failed != plans.len()
        || before.lines.len() != after.lines.len()
        || before.classification_counts != after.classification_counts
    {
        return refuse(ScrubReason::VerificationFailed);
    }
    // Exact field-pair multiset avoids quadratic matching on large recordings.
    // Keys remain in memory only; neither values nor hashes enter diagnostics.
    let mut expected_fields = BTreeMap::<(u8, usize, String, String), usize>::new();
    for p in plans {
        if p.treatment == Treatment::Undecodable {
            // Filler must remain undecodable, never invent a valid typed event.
            if decode_packet(p.id, direction(p.d), &p.expected)
                .is_ok_and(|(v, _)| v.fields().is_ok())
            {
                return refuse(ScrubReason::VerificationFailed);
            }
            continue;
        }
        let old = decode_packet(p.id, direction(p.d), &p.plain)?.0.fields()?;
        let new = decode_packet(p.id, direction(p.d), &p.expected)?
            .0
            .fields()?;
        *expected_fields
            .entry((p.id, p.d, old.to_string(), new.to_string()))
            .or_default() += 1;
    }
    for (old, new) in before.lines.iter().zip(&after.lines) {
        if old.i != new.i
            || old.t != new.t
            || old.dir != new.dir
            || old.id != new.id
            || old.name != new.name
        {
            return refuse(ScrubReason::VerificationFailed);
        }
        let key = (
            old.id,
            usize::from(old.dir == "s2c"),
            old.f.to_string(),
            new.f.to_string(),
        );
        let Some(count) = expected_fields.get_mut(&key) else {
            return refuse(ScrubReason::VerificationFailed);
        };
        if *count == 0 {
            return refuse(ScrubReason::VerificationFailed);
        }
        *count -= 1;
    }
    if expected_fields.values().any(|&n| n != 0) {
        return refuse(ScrubReason::VerificationFailed);
    }
    Ok(())
}
// UNIQUE_DATA_STRING is a comma-separated inventory of URL-safe six-bit item
// blobs (enchants::slots/parse_enchant), not a complete decoded text schema. Keep
// its bytes for enchant/export parity in both NEWTICK and UPDATE newObjects:
// the stat carries the same data class in either packet. Only 4-7-byte,
// non-numeric name-like identity/player-text matches strictly inside a base64
// item are coincidences. Shorter chat (including digits) and non-numeric names
// are ignored except for whole-item equality; credentials, numeric identity
// values, longer substrings and whole items refuse.
// Every admitted coincidence is reported by field/class, never by secret value.
fn opaque_items(field: &[u8]) -> impl Iterator<Item = &[u8]> {
    field.split(|&byte| byte == b',')
}

fn opaque_token_interior(field: &[u8], secret: &[u8]) -> bool {
    secret.iter().all(u8::is_ascii_alphanumeric)
        && secret.iter().any(u8::is_ascii_alphabetic)
        && opaque_items(field).all(|item| {
            let base64 = item
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'='));
            item.windows(secret.len())
                .enumerate()
                .filter(|(_, part)| *part == secret)
                .all(|(at, _)| base64 && at > 0 && at + secret.len() < item.len())
        })
}
fn delimited_collision(field: &[u8], secret: &[u8]) -> bool {
    !secret.is_empty() && opaque_items(field).any(|item| item == secret)
}
// Short values are common substrings of harmless game data. Only exact field
// equality is evidence for them; longer values also receive a substring scan.
fn residual_collision(field: &[u8], secret: &[u8]) -> bool {
    if secret.len() < 4 {
        !secret.is_empty() && field == secret
    } else {
        field.windows(secret.len()).any(|window| window == secret)
    }
}

fn residual_collision_ignore_ascii_case(field: &[u8], secret: &[u8]) -> bool {
    if secret.len() < 4 {
        !secret.is_empty() && field.eq_ignore_ascii_case(secret)
    } else {
        field
            .windows(secret.len())
            .any(|window| window.eq_ignore_ascii_case(secret))
    }
}

// A hard link publishes the verified file atomically and fails if any destination
// already exists, including one created after the initial argument check.
fn publish(temp: &Path, output: &Path) -> std::io::Result<()> {
    fs::hard_link(temp, output)
}

struct HashingReader<'a> {
    file: File,
    hash: &'a mut Sha256,
    count: u64,
}
impl Read for HashingReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(bytes)?;
        self.count += n as u64;
        if self.count > MAX_FILE {
            return Err(std::io::Error::other(
                "Recording admission size limit exceeded",
            ));
        }
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
}
fn read_frames(path: &Path) -> Result<(Vec<RawFrame>, String), ScrubFailure> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE {
        return refuse(ScrubReason::ResourceLimit);
    }
    let mut hash = Sha256::new();
    let mut reader = PcapReader::new(HashingReader {
        file,
        hash: &mut hash,
        count: 0,
    })?;
    let mut frames = vec![];
    while let Some(frame) = reader.next_frame()? {
        if frames.len() >= MAX_FRAMES {
            return refuse(ScrubReason::ResourceLimit);
        }
        frames.push(frame);
    }
    // Hash precisely the same read that produced the certified frame inventory.
    Ok((frames, maxed_protocol::lower_hex(&hash.finalize())))
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct OwnedFile {
    path: PathBuf,
    committed: bool,
}
impl Drop for OwnedFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn create_private(root: &Path, extension: &str) -> Result<(OwnedFile, File), ScrubFailure> {
    for _ in 0..100 {
        let path = root.join(format!(
            "scrub-{}-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                return Ok((
                    OwnedFile {
                        path,
                        committed: false,
                    },
                    file,
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    refuse(ScrubReason::VerificationFailed)
}
/// Writes only after whole-recording admission and verifies a private temporary
/// file before publishing. Existing destinations (including aliases) are refused.
pub fn scrub(input: &Path, output: &Path) -> Result<ScrubReport, CliError> {
    scrub_with_options(input, output, ScrubOptions::default())
        .map_err(|error| CliError::Io(std::io::Error::other(error)))
}
/// Detailed, privacy-safe admission results. CLI uses this entry point so no
/// refusal is collapsed into a generic protocol or I/O message.
pub fn scrub_with_options(
    input: &Path,
    output: &Path,
    options: ScrubOptions,
) -> Result<ScrubReport, ScrubFailure> {
    let mut diagnostics = ScrubDiagnostics::default();
    scrub_inner(input, output, options, &mut diagnostics).map_err(|mut error| {
        if error.diagnostics.scan_complete {
            return error;
        }
        diagnostics.merge(*error.diagnostics);
        error.diagnostics = Box::new(diagnostics);
        error
    })
}
fn scrub_inner(
    input: &Path,
    output: &Path,
    options: ScrubOptions,
    diagnostics: &mut ScrubDiagnostics,
) -> Result<ScrubReport, ScrubFailure> {
    let input = input.canonicalize()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    let output = parent.join(output.file_name().ok_or(CliError::InvalidArguments)?);
    if output == input || output.symlink_metadata().is_ok() {
        return Err(CliError::InvalidArguments.into());
    }
    let (original, input_hash) = read_frames(&input)?;
    let generations = assemble(&original)?;
    let mut plans = bodies(&generations)?;
    classify(&mut plans, options, diagnostics)?;
    let changed_spans = redact(&mut plans)?;
    let mut frames = original.clone();
    patch(&mut frames, &generations, &plans)?;
    normalize(&mut frames)?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let private = repo.join("recordings");
    // Never follow a recordings junction/symlink to a public directory.
    if private
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(CliError::InvalidArguments.into());
    }
    fs::create_dir_all(&private)?;
    if private.canonicalize()? != private {
        return Err(CliError::InvalidArguments.into());
    }
    let staging = if parent.starts_with(&private) {
        &parent
    } else {
        &private
    };
    let (temp, file) = create_private(staging, "pcap")?;
    let mut writer = PcapWriter::new(file)?;
    for frame in &frames {
        writer.write_frame(frame)?;
    }
    writer.finish()?.sync_all()?;
    let (reread, output_hash) = read_frames(&temp.path)?;
    if reread.len() != frames.len() || reread.iter().zip(&frames).any(|(a, b)| a.bytes != b.bytes) {
        return refuse(ScrubReason::VerificationFailed);
    }
    verify(&original, &reread, &plans, diagnostics)?;
    let mut schema = Sha256::new();
    schema.update(include_bytes!("../../maxed-protocol/src/defs.rs"));
    schema.update(include_bytes!("../../maxed-protocol/src/reader.rs"));
    schema.update(include_bytes!("../../maxed-protocol/src/privacy.rs"));
    schema.update(include_bytes!("../../maxed-protocol/src/notices.rs"));
    schema.update(include_bytes!("scrub.rs"));
    let profile_hash = maxed_protocol::lower_hex(&schema.finalize());
    let (mut meta, mut file) = create_private(staging, "meta.json")?;
    serde_json::to_writer(&mut file,&serde_json::json!({"schemaProfileSha256":profile_hash,"inputSha256":input_hash,"outputSha256":output_hash,"version":env!("CARGO_PKG_VERSION"),"frames":frames.len(),"packets":plans.len(),"changedSpans":changed_spans,"filledUnclassified":diagnostics.field_count("filled-unclassified"),"pseudonymized":diagnostics.field_count("pseudonymized"),"keptAllowlisted":diagnostics.field_count("kept-allowlisted"),"verified":true,"allowUndecodable":options.allow_undecodable,"provisional":diagnostics.count(ScrubReason::UndecodableTyped)>0,"remakeAfterP1UpdateLayoutFix":diagnostics.count(ScrubReason::UndecodableTyped)>0,"coverage":diagnostics.rows()})).map_err(|_|CliError::InvalidArguments)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    publish(&temp.path, &output)?;
    // OwnedFile removes the private staging link on every exit.
    meta.committed = true;
    Ok(ScrubReport {
        frames: frames.len() as u64,
        changed_spans,
        verified: true,
        diagnostics: diagnostics.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_cannot_clobber_a_destination_created_after_preflight() {
        let root = std::env::temp_dir();
        let (source, mut file) = create_private(&root, "publish-test").unwrap();
        file.write_all(b"verified capture").unwrap();
        drop(file);
        let destination = source.path.with_extension("destination-test");
        assert!(!destination.exists());
        let guard = OwnedFile {
            path: destination.clone(),
            committed: false,
        };
        let mut racing = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .unwrap();
        racing.write_all(b"racing sentinel").unwrap();
        drop(racing);
        assert!(publish(&source.path, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"racing sentinel");
        assert_eq!(fs::read(&source.path).unwrap(), b"verified capture");
        drop(guard);
        publish(&source.path, &destination).unwrap();
        let _guard = OwnedFile {
            path: destination.clone(),
            committed: false,
        };
        assert_eq!(fs::read(&destination).unwrap(), b"verified capture");
    }
}
