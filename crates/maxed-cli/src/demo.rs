//! Deterministic, entirely synthetic demo; no live capture, storage or notifications.
use crate::{CliError, Pipeline};
use maxed_capture::parse_frame;
use maxed_engine::{Engine, EngineConfig};
use maxed_protocol::{key_bytes, DecodeEvent, Diagnostic, Rc4};
use maxed_recording::{PcapReader, PcapWriter};
use maxed_types::{
    Catalog, Direction, EngineEvent, EventEnvelope, FightEnd, RawFrame, Sequence, Timestamp,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Read},
    path::Path,
    sync::Arc,
};

/// Create a new classic pcap. An existing path is never replaced, including on failure.
pub fn write_demo(output: &Path) -> Result<(), CliError> {
    let frames = synthetic_frames(story()?)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let result = (|| {
        let mut writer = PcapWriter::new(file)?;
        for frame in &frames {
            writer.write_frame(frame)?;
        }
        writer.finish()?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}

/// Replay through capture/stream/protocol and the real engine, using capture time.
/// The caller supplies a separate stream namespace and identity key; no wall clock,
/// database, raw packet export or external notification is involved.
pub fn replay_engine(
    input: &Path,
    catalog: Arc<Catalog>,
    config: EngineConfig,
) -> Result<Vec<EventEnvelope>, CliError> {
    replay_reader(BufReader::new(File::open(input)?), catalog, config)
}

fn replay_reader(
    reader: impl Read,
    catalog: Arc<Catalog>,
    config: EngineConfig,
) -> Result<Vec<EventEnvelope>, CliError> {
    let stream_id = config.stream_id.clone();
    let mut engine = Engine::new(config, catalog)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let mut reader = PcapReader::new(reader)?;
    let mut pipeline = Pipeline::default();
    let mut output = Vec::new();
    let mut at = Timestamp(0);
    while let Some(frame) = reader.next_frame()? {
        at = Timestamp(at.0.max(frame.at.0));
        // Only parsed game TCP payload is traffic; unrelated frames cannot keep
        // a session alive. apply() also observes decoded packet receipt time.
        if parse_frame(&frame)
            .ok()
            .flatten()
            .is_some_and(|s| !s.bytes.is_empty())
        {
            let events = engine.observe_traffic(frame.at);
            envelope(&mut engine, &stream_id, events, &mut output);
        }
        consume(&mut engine, &stream_id, pipeline.push(frame), &mut output);
    }
    consume(&mut engine, &stream_id, pipeline.finish(), &mut output);
    let events = engine.finish(at, FightEnd::CaptureStopped);
    envelope(&mut engine, &stream_id, events, &mut output);
    Ok(output)
}

fn consume(
    engine: &mut Engine,
    stream_id: &str,
    decoded: Vec<DecodeEvent>,
    output: &mut Vec<EventEnvelope>,
) {
    for event in decoded {
        let events = match event {
            DecodeEvent::Packet(p) if p.alignment_verified => engine.apply(p),
            DecodeEvent::Packet(p) => {
                engine.diagnostic(p.conn, p.dir, p.at, Diagnostic::Recovering)
            }
            DecodeEvent::Diagnostic {
                conn,
                dir,
                at,
                kind,
            } => engine.diagnostic(conn, dir, at, kind),
        };
        envelope(engine, stream_id, events, output);
    }
}

fn envelope(
    engine: &mut Engine,
    stream_id: &str,
    events: Vec<EngineEvent>,
    output: &mut Vec<EventEnvelope>,
) {
    let at = engine.snapshot().at;
    for event in events {
        let sequence = Sequence(Timestamp(output.len() as u64 + 1));
        output.push(EventEnvelope {
            fight_chart_omitted: false,
            stream_id: stream_id.into(),
            sequence,
            at,
            event,
        });
        engine.set_watermark(sequence);
    }
}

// Only the explicitly authored wire shapes used by this synthetic story are
// encoded here. Protocol definitions remain the authority for decoding.
#[derive(Default)]
struct Body(Vec<u8>);
impl Body {
    fn byte(&mut self, n: u8) {
        self.0.push(n);
    }
    fn short(&mut self, n: u16) {
        self.0.extend(n.to_be_bytes());
    }
    fn int(&mut self, n: i32) {
        self.0.extend(n.to_be_bytes());
    }
    fn pos(&mut self) {
        self.0.extend([0; 8]);
    }
    fn string(&mut self, s: &str) -> Result<(), CliError> {
        self.short(u16::try_from(s.len()).map_err(|_| maxed_protocol::ProtocolError::Length)?);
        self.0.extend(s.as_bytes());
        Ok(())
    }
    fn ci(&mut self, n: i32) {
        let mut remaining = n.unsigned_abs();
        let mut byte = (remaining & 63) as u8 | if n < 0 { 64 } else { 0 };
        remaining >>= 6;
        while remaining != 0 {
            self.byte(byte | 128);
            byte = (remaining & 127) as u8;
            remaining >>= 7;
        }
        self.byte(byte);
    }
}

struct WirePacket {
    at: u64,
    dir: Direction,
    id: u8,
    body: Body,
}
fn server(at: u64, id: u8, body: Body) -> WirePacket {
    WirePacket {
        at,
        dir: Direction::ServerToClient,
        id,
        body,
    }
}
fn map(at: u64, name: &str) -> Result<WirePacket, CliError> {
    let mut b = Body::default();
    b.int(64);
    b.int(64);
    b.string(name)?;
    b.string(name)?;
    b.string("")?;
    b.int(1);
    b.int(0);
    b.int(0); // seed, background, difficulty f32=0
    b.0.extend([0; 3]);
    b.short(85);
    b.int(0);
    b.string("synthetic-demo-v1")?;
    b.short(0);
    b.short(16);
    b.byte(0);
    b.int(0);
    b.string("")?;
    b.short(0);
    Ok(server(at, 92, b))
}
fn create(at: u64, counters: &str) -> Result<WirePacket, CliError> {
    let mut b = Body::default();
    b.int(1);
    b.int(1);
    b.string(counters)?;
    Ok(server(at, 101, b))
}
enum Stat<'a> {
    Number(u8, i32),
    Text(u8, &'a str),
}
fn status(b: &mut Body, id: i32, stats: &[Stat<'_>]) -> Result<(), CliError> {
    b.ci(id);
    b.pos();
    b.ci(i32::try_from(stats.len()).map_err(|_| maxed_protocol::ProtocolError::Length)?);
    for stat in stats {
        match stat {
            Stat::Number(id, n) => {
                b.byte(*id);
                b.ci(*n);
            }
            Stat::Text(id, s) => {
                b.byte(*id);
                b.string(s)?;
            }
        }
        b.ci(0);
    }
    Ok(())
}
type Object<'a> = (u16, i32, Vec<Stat<'a>>);
fn update(at: u64, objects: &[Object<'_>], drops: &[i32]) -> Result<WirePacket, CliError> {
    let mut b = Body::default();
    b.pos();
    b.byte(0);
    b.ci(0);
    b.ci(i32::try_from(objects.len()).map_err(|_| maxed_protocol::ProtocolError::Length)?);
    for (ty, id, stats) in objects {
        b.short(*ty);
        status(&mut b, *id, stats)?;
    }
    b.ci(i32::try_from(drops.len()).map_err(|_| maxed_protocol::ProtocolError::Length)?);
    for id in drops {
        b.ci(*id);
    }
    Ok(server(at, 42, b))
}
fn player(id: i32, name: &'static str, account: &'static str) -> Object<'static> {
    use Stat::{Number as N, Text as T};
    (
        100,
        id,
        vec![
            T(31, name),
            T(38, account),
            N(7, 20),
            N(20, 25),
            N(29, 0),
            N(96, 0),
            N(113, 1000),
            T(128, ""),
            T(155, ""),
            T(80, ",,,"),
            N(8, 256),
            N(9, -1),
            N(10, -1),
            N(11, -1),
            N(57, 100),
        ],
    )
}
fn enemy(ty: u16, id: i32) -> Object<'static> {
    use Stat::Number as N;
    (
        ty,
        id,
        vec![N(0, 10000), N(1, 10000), N(21, 0), N(29, 0), N(96, 0)],
    )
}
fn bag_stats() -> Vec<Stat<'static>> {
    let mut stats: Vec<_> = (8..16)
        .map(|s| Stat::Number(s, if s == 8 { 256 } else { -1 }))
        .collect();
    // Base64 of [0,2,4,1,0]: one recorded enchant, synthetic enchant ID 1.
    stats.push(Stat::Text(80, "AAIEAQA=,,,,,,,"));
    stats
}
fn tick(at: u64, id: i32, statuses: &[(i32, Vec<Stat<'_>>)]) -> Result<WirePacket, CliError> {
    let mut b = Body::default();
    b.int(id);
    b.int(200);
    b.int((at / 1000) as i32);
    b.short(0);
    b.short(u16::try_from(statuses.len()).map_err(|_| maxed_protocol::ProtocolError::Length)?);
    for (object, stats) in statuses {
        status(&mut b, *object, stats)?;
    }
    Ok(server(at, 10, b))
}
fn shoot(at: u64, owner: i32, bullet: u16, amount: u16) -> WirePacket {
    let mut b = Body::default();
    b.short(bullet);
    b.int(owner);
    b.int(256);
    b.pos();
    b.int(0);
    b.short(amount);
    b.int(0);
    b.byte(0);
    b.byte(1);
    b.int(0);
    server(at, 12, b)
}
fn damage(at: u64, target: i32, owner: i32, bullet: u16, amount: u16) -> WirePacket {
    let mut b = Body::default();
    b.int(target);
    b.byte(0);
    b.short(amount);
    b.byte(0);
    b.short(bullet);
    b.int(owner);
    server(at, 75, b)
}
fn story() -> Result<Vec<WirePacket>, CliError> {
    Ok(vec![
        map(100_000, "Nexus")?,
        map(1_000_000, "Realm")?,
        create(1_010_000, "AAAAAA==")?,
        update(
            1_020_000,
            &[player(1, "DemoHero", "DemoAccount"), enemy(258, 4)],
            &[],
        )?,
        tick(2_000_000, 0, &[])?,
        shoot(2_010_000, 1, 1, 50),
        damage(2_020_000, 4, 1, 1, 50),
        map(5_000_000, "Snake Pit")?,
        create(5_010_000, "AAAAAA==")?,
        update(
            5_020_000,
            &[
                player(1, "DemoHero", "DemoAccount"),
                player(3, "DemoAlly", "DemoAllyAccount"),
                enemy(257, 2),
            ],
            &[],
        )?,
        tick(6_000_000, 1, &[])?,
        shoot(6_010_000, 1, 10, 100),
        damage(6_020_000, 2, 1, 10, 100),
        tick(8_000_000, 2, &[(1, vec![Stat::Number(57, 125)])])?,
        shoot(8_010_000, 1, 11, 200),
        damage(8_020_000, 2, 1, 11, 200),
        update(9_000_000, &[(1292, 5, bag_stats())], &[2])?,
        tick(9_100_000, 3, &[(5, bag_stats())])?,
        tick(9_200_000, 4, &[(5, bag_stats())])?,
        server(10_000_000, 67, Body(vec![14, 0])),
        map(12_000_000, "Nexus")?,
    ])
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in bytes.chunks(2) {
        sum += u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

struct Connection {
    port: u16,
    client_seq: u32,
    server_seq: u32,
    client_rc4: Rc4,
    server_rc4: Rc4,
}
impl Connection {
    fn new(port: u16) -> Result<Self, CliError> {
        Ok(Self {
            port,
            client_seq: 1000,
            server_seq: 9000,
            client_rc4: Rc4::new(&key_bytes(Direction::ClientToServer)?)?,
            server_rc4: Rc4::new(&key_bytes(Direction::ServerToClient)?)?,
        })
    }
    fn handshake(&mut self, frames: &mut Vec<RawFrame>, at: u64) -> Result<(), CliError> {
        self.frame(frames, at, Direction::ClientToServer, 0x02, &[])?;
        self.frame(frames, at + 1000, Direction::ServerToClient, 0x12, &[])?;
        self.frame(frames, at + 2000, Direction::ClientToServer, 0x10, &[])
    }
    fn packet(&mut self, frames: &mut Vec<RawFrame>, packet: WirePacket) -> Result<(), CliError> {
        let mut body = packet.body.0;
        let cipher = match packet.dir {
            Direction::ClientToServer => &mut self.client_rc4,
            Direction::ServerToClient => &mut self.server_rc4,
        };
        cipher.apply(&mut body);
        let len = body
            .len()
            .checked_add(5)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(maxed_protocol::ProtocolError::Length)?;
        let mut bytes = len.to_be_bytes().to_vec();
        bytes.push(packet.id);
        bytes.extend(body);
        // Deliberately split packet bodies across TCP segments. Each packet
        // retains a fixed receipt time regardless of segmentation/retransmission.
        for chunk in bytes.chunks(37) {
            self.frame(frames, packet.at, packet.dir, 0x18, chunk)?;
        }
        Ok(())
    }
    fn frame(
        &mut self,
        frames: &mut Vec<RawFrame>,
        at: u64,
        dir: Direction,
        flags: u8,
        payload: &[u8],
    ) -> Result<(), CliError> {
        let ip_len = payload
            .len()
            .checked_add(40)
            .and_then(|n| u16::try_from(n).ok())
            .ok_or(maxed_protocol::ProtocolError::Length)?;
        let mut bytes = vec![0; usize::from(ip_len) + 14];
        let (src, dst, sport, dport, seq, ack) = match dir {
            Direction::ClientToServer => (
                [192, 0, 2, 10],
                [198, 51, 100, 20],
                self.port,
                2050,
                self.client_seq,
                self.server_seq,
            ),
            Direction::ServerToClient => (
                [198, 51, 100, 20],
                [192, 0, 2, 10],
                2050,
                self.port,
                self.server_seq,
                self.client_seq,
            ),
        };
        bytes[..6].copy_from_slice(&[2, 0, 0, 0, 0, 1]);
        bytes[6..12].copy_from_slice(&[2, 0, 0, 0, 0, 2]);
        bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        bytes[14] = 0x45;
        bytes[16..18].copy_from_slice(&ip_len.to_be_bytes());
        bytes[20] = 0x40;
        bytes[22] = 64;
        bytes[23] = 6;
        bytes[26..30].copy_from_slice(&src);
        bytes[30..34].copy_from_slice(&dst);
        let ip_check = checksum(&bytes[14..34]);
        bytes[24..26].copy_from_slice(&ip_check.to_be_bytes());
        bytes[34..36].copy_from_slice(&sport.to_be_bytes());
        bytes[36..38].copy_from_slice(&dport.to_be_bytes());
        bytes[38..42].copy_from_slice(&seq.to_be_bytes());
        if flags & 0x10 != 0 {
            bytes[42..46].copy_from_slice(&ack.to_be_bytes());
        }
        bytes[46] = 0x50;
        bytes[47] = flags;
        bytes[48..50].copy_from_slice(&65535u16.to_be_bytes());
        bytes[54..].copy_from_slice(payload);
        let mut pseudo = Vec::from(src);
        pseudo.extend(dst);
        pseudo.extend([0, 6]);
        pseudo.extend((ip_len - 20).to_be_bytes());
        pseudo.extend(&bytes[34..]);
        let tcp_check = checksum(&pseudo);
        bytes[50..52].copy_from_slice(&tcp_check.to_be_bytes());
        frames.push(RawFrame {
            index: frames.len() as u64,
            at: Timestamp(at),
            original_len: bytes.len() as u32,
            bytes,
        });
        let next = seq
            .wrapping_add(payload.len() as u32)
            .wrapping_add(u32::from(flags & 0x02 != 0));
        match dir {
            Direction::ClientToServer => self.client_seq = next,
            Direction::ServerToClient => self.server_seq = next,
        }
        Ok(())
    }
}
fn synthetic_frames(packets: Vec<WirePacket>) -> Result<Vec<RawFrame>, CliError> {
    let mut frames = Vec::new();
    let mut connection = Connection::new(40000)?;
    connection.handshake(&mut frames, 0)?;
    for packet in packets {
        connection.packet(&mut frames, packet)?;
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{CoverageIssue, Evidence, FightView, Outcome, RunView, SourceKind};

    fn catalog() -> Arc<Catalog> {
        Arc::new(
            serde_json::from_str(include_str!(
                "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap(),
        )
    }
    fn config() -> EngineConfig {
        EngineConfig {
            stream_id: "demo-v1".into(),
            identity_key: [7; 32],
            ..Default::default()
        }
    }
    fn pcap(mut frames: Vec<RawFrame>) -> Vec<u8> {
        let mut writer = PcapWriter::new(Vec::new()).unwrap();
        for (index, frame) in frames.iter_mut().enumerate() {
            frame.index = index as u64;
            writer.write_frame(frame).unwrap();
        }
        writer.finish().unwrap()
    }
    fn replay_frames(frames: Vec<RawFrame>) -> Vec<EventEnvelope> {
        replay_reader(pcap(frames).as_slice(), catalog(), config()).unwrap()
    }
    fn replay_packets(packets: Vec<WirePacket>) -> Vec<EventEnvelope> {
        replay_frames(synthetic_frames(packets).unwrap())
    }
    fn last_run(events: &[EventEnvelope]) -> &RunView {
        events
            .iter()
            .rev()
            .find_map(|e| match &e.event {
                EngineEvent::RunEnded(r) | EngineEvent::RunUpdated(r) => Some(r),
                _ => None,
            })
            .unwrap()
    }
    fn boss(events: &[EventEnvelope]) -> &FightView {
        events
            .iter()
            .rev()
            .find_map(|e| match &e.event {
                EngineEvent::FightEnded(f) if f.target_id == 2 => Some(f),
                _ => None,
            })
            .unwrap()
    }
    fn domain(events: &[EventEnvelope]) -> Vec<&EngineEvent> {
        events
            .iter()
            .filter_map(|e| match e.event {
                EngineEvent::SessionUpdated(_) => None,
                _ => Some(&e.event),
            })
            .collect()
    }

    #[test]
    fn replay_frame_gap_of_thirty_one_minutes_starts_a_second_session() {
        let mut frames = synthetic_frames(story().unwrap()).unwrap();
        for frame in &mut frames {
            if frame.at.0 >= 8_000_000 {
                frame.at.0 += 31 * 60 * 1_000_000;
            }
        }
        let events = replay_frames(frames);
        let sessions: std::collections::BTreeSet<_> = events
            .iter()
            .filter_map(|e| {
                if let EngineEvent::SessionUpdated(s) = &e.event {
                    Some(&s.id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(sessions.len(), 2);
        assert!(events
            .iter()
            .any(|e| matches!(&e.event, EngineEvent::FightEnded(f)
            if f.end_reason == Some(FightEnd::SessionIdle) && f.total_damage == 100.)));
        assert!(events
            .iter()
            .any(|e| matches!(&e.event, EngineEvent::RunEnded(r)
            if r.outcome == Some(Outcome::Unknown) && !r.complete_capture)));
    }

    #[test]
    fn replay_eof_mid_run_is_not_a_tcp_capture_gap() {
        let frames = synthetic_frames(story().unwrap())
            .unwrap()
            .into_iter()
            .filter(|frame| frame.at.0 <= 6_020_000)
            .collect();
        let events = replay_frames(frames);
        let run = last_run(&events);
        assert_eq!(run.outcome, Some(Outcome::Unknown));
        assert!(!run.issues.contains(&CoverageIssue::StreamGap));
        assert!(!run
            .evidence
            .iter()
            .any(|e| e.evidence == Evidence::CaptureGap));
        assert!(!boss(&events).issues.contains(&CoverageIssue::StreamGap));
        assert_eq!(boss(&events).end_reason, Some(FightEnd::CaptureStopped));
    }

    #[test]
    fn duplicate_and_reordered_tcp_segments_preserve_domain_events() {
        let base = synthetic_frames(story().unwrap()).unwrap();
        let expected = replay_frames(base.clone());
        let mut duplicates = Vec::new();
        for frame in &base {
            duplicates.push(frame.clone());
            duplicates.push(frame.clone());
        }
        assert_eq!(domain(&expected), domain(&replay_frames(duplicates)));
        let mut reordered = base;
        // Reverse the split segments of every packet, keeping capture time monotonic.
        let mut start = 0;
        while start < reordered.len() {
            let mut end = start + 1;
            while end < reordered.len() && reordered[end].at == reordered[start].at {
                end += 1;
            }
            reordered[start..end].reverse();
            start = end;
        }
        assert_eq!(domain(&expected), domain(&replay_frames(reordered)));
    }

    #[test]
    fn absent_update_keeps_target_identity_and_attribution_unknown() {
        let mut packets = story().unwrap();
        packets.retain(|p| p.at != 5_020_000);
        let events = replay_packets(packets);
        let f = boss(&events);
        assert_eq!(f.boss_type, None);
        assert_eq!(f.target_type, None);
        // With no object UPDATE the source is not known to be a player;
        // retain server damage as unattributed with explicit coverage issues.
        assert_eq!(f.total_damage, 300.);
        assert_eq!(f.unattributed_damage, 300.);
        assert!(f.players.is_empty());
        assert!(!f.complete_capture);
        assert!(f.issues.contains(&CoverageIssue::MissingIdentity));
        assert!(f.issues.contains(&CoverageIssue::MissingCatalog));
        assert!(!events.iter().any(|e| matches!(&e.event,
            EngineEvent::CharacterUpdated(c) if e.at.0 >= 5_000_000 && c.identity_verified)));
    }

    fn death() -> WirePacket {
        let mut b = Body::default();
        b.string("DemoAccount").unwrap();
        b.ci(1);
        b.string("Demo Serpent").unwrap();
        b.int(0);
        b.ci(125);
        b.ci(0);
        b.string("").unwrap();
        server(10_000_000, 46, b)
    }
    #[test]
    fn death_and_completion_order_preserves_both_facts() {
        for clear_first in [false, true] {
            let mut packets = story().unwrap();
            let index = packets.iter().position(|p| p.id == 67).unwrap();
            packets.insert(index + usize::from(clear_first), death());
            let events = replay_packets(packets);
            let r = last_run(&events);
            assert_eq!(
                r.outcome,
                Some(if clear_first {
                    Outcome::Cleared
                } else {
                    Outcome::Died
                })
            );
            assert_eq!(r.died_at, Some(Timestamp(10_000_000)));
            assert_eq!(r.completed_at, Some(Timestamp(10_000_000)));
            assert!(r.evidence.iter().any(|e| e.evidence == Evidence::Victory));
            assert!(r
                .evidence
                .iter()
                .any(|e| e.evidence == Evidence::DeathPacket));
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e.event, EngineEvent::PlayerDied { .. }))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn malformed_counter_cannot_turn_exit_into_completion() {
        let mut packets = story().unwrap();
        packets.retain(|p| p.id != 67);
        let index = packets.iter().position(|p| p.at == 5_010_000).unwrap();
        packets[index] = create(5_010_000, "not-base64").unwrap();
        packets.push(create(12_010_000, "not-base64").unwrap());
        packets.push(update(12_020_000, &[player(1, "DemoHero", "DemoAccount")], &[]).unwrap());
        let events = replay_packets(packets);
        let r = last_run(&events);
        assert_eq!(r.outcome, Some(Outcome::Left));
        assert_eq!(r.completed_at, None);
        assert!(!r
            .evidence
            .iter()
            .any(|e| matches!(e.evidence, Evidence::CompletionCounter { .. })));
    }

    #[test]
    fn delayed_contents_enrich_one_bag_without_inventing_empty_slots() {
        let mut packets = story().unwrap();
        let index = packets.iter().position(|p| p.at == 9_000_000).unwrap();
        packets[index] = update(9_000_000, &[(1292, 5, vec![])], &[2]).unwrap();
        let index = packets.iter().position(|p| p.at == 9_100_000).unwrap();
        packets[index] = tick(9_100_000, 3, &[(5, vec![Stat::Number(8, 256)])]).unwrap();
        let events = replay_packets(packets);
        let bags: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.event {
                EngineEvent::BagDropped(b) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(bags.len(), 1);
        assert!(!bags[0].contents_known);
        let updates: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.event {
                EngineEvent::DropUpdated(b) => Some((e.at, b)),
                _ => None,
            })
            .collect();
        assert!(updates.iter().all(|(_, b)| b.id == bags[0].id));
        assert!(updates
            .iter()
            .any(|(at, b)| at.0 == 9_100_000 && !b.contents_known && b.items == [256]));
        assert!(updates.iter().any(|(at, b)| at.0 == 9_200_000
            && b.contents_known
            && b.contents[0].enchant.ids == [1]));
    }

    #[test]
    fn capture_gap_then_fresh_connection_retains_incomplete_run() {
        let mut frames = synthetic_frames(story().unwrap()).unwrap();
        frames.retain(|f| f.at.0 < 9_000_000);
        // A truncated captured IP frame invalidates both active directions.
        let mut broken = frames.last().unwrap().clone();
        broken.at = Timestamp(8_030_000);
        broken.bytes.pop();
        frames.push(broken);
        let mut connection = Connection::new(40001).unwrap();
        connection.handshake(&mut frames, 11_990_000).unwrap();
        connection
            .packet(&mut frames, map(12_000_000, "Nexus").unwrap())
            .unwrap();
        connection
            .packet(&mut frames, create(12_010_000, "AAAAAA==").unwrap())
            .unwrap();
        connection
            .packet(
                &mut frames,
                update(12_020_000, &[player(1, "DemoHero", "DemoAccount")], &[]).unwrap(),
            )
            .unwrap();
        let events = replay_frames(frames);
        let r = last_run(&events);
        assert_eq!(r.outcome, Some(Outcome::Unknown));
        assert!(!r.complete_capture);
        assert!(r
            .evidence
            .iter()
            .any(|e| e.evidence == Evidence::CaptureGap));
        assert!(r.issues.contains(&CoverageIssue::StreamGap));
        assert!(events.iter().any(|e| matches!(&e.event, EngineEvent::MapEntered { name, .. } if e.at.0 == 12_000_000 && name == "Nexus")));
        assert!(events.iter().any(|e| matches!(&e.event, EngineEvent::CharacterUpdated(c) if e.at.0 == 12_020_000 && c.identity_verified)));
    }

    #[test]
    fn client_enemyhit_reconstruction_and_ally_source_cross_both_ciphers() {
        let mut packets = story().unwrap();
        let mut shot = Body::default();
        shot.int(8500);
        shot.short(12);
        shot.short(256);
        shot.byte(255);
        shot.pos();
        shot.int(0);
        shot.0.extend([0; 3]);
        shot.pos();
        let mut hit = Body::default();
        hit.int(8520);
        hit.short(12);
        hit.int(1);
        hit.int(2);
        hit.byte(0);
        hit.int(1);
        let index = packets.iter().position(|p| p.at == 9_000_000).unwrap();
        packets.splice(
            index..index,
            [
                WirePacket {
                    at: 8_500_000,
                    dir: Direction::ClientToServer,
                    id: 30,
                    body: shot,
                },
                WirePacket {
                    at: 8_520_000,
                    dir: Direction::ClientToServer,
                    id: 25,
                    body: hit,
                },
                shoot(8_600_000, 3, 13, 75),
                damage(8_620_000, 2, 3, 13, 75),
            ],
        );
        let events = replay_packets(packets);
        let f = boss(&events);
        assert_eq!(f.total_damage, 475.);
        assert_eq!(f.players.len(), 2);
        let local = f.players.iter().find(|p| p.is_local).unwrap();
        assert_eq!(local.damage, 400.);
        assert_eq!(local.hits, 3);
        assert!(local.sources.iter().any(|s| !s.estimated
            && s.kind == SourceKind::Weapon
            && s.damage == 100.
            && s.reasons == ["client-projectile-reconstruction"]));
        let ally = f.players.iter().find(|p| !p.is_local).unwrap();
        assert_eq!(ally.name.as_deref(), Some("DemoAlly"));
        assert_eq!(ally.damage, 75.);
        assert_eq!(ally.sources[0].item_type, Some(256));
        assert!(!ally.sources[0].estimated);
    }

    #[test]
    fn incomplete_pcap_and_invalid_engine_config_return_errors() {
        let mut bytes = pcap(synthetic_frames(story().unwrap()).unwrap());
        bytes.pop();
        assert!(matches!(
            replay_reader(bytes.as_slice(), catalog(), config()),
            Err(CliError::Recording(
                maxed_recording::RecordingError::Truncated
            ))
        ));
        let mut invalid = config();
        invalid.stream_id.clear();
        assert!(replay_reader([].as_slice(), catalog(), invalid).is_err());
    }
}
