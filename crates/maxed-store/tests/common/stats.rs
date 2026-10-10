//! Synthetic streams for Stats tests. Identities are synthetic.
use maxed_types::*;
use rusqlite::Connection;

pub const MIN: u64 = 60_000_000;

pub fn database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stats.sqlite");
    let mut db = Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    (dir, path, db)
}

/// One event stream with contiguous sequences. Start every stream with `session(..)`.
pub struct Stream {
    pub id: String,
    next: u64,
}
impl Stream {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            next: 1,
        }
    }
    pub fn apply(&mut self, db: &mut Connection, events: Vec<EngineEvent>) {
        let envelopes: Vec<_> = events
            .into_iter()
            .map(|event| {
                let sequence = self.next;
                self.next += 1;
                EventEnvelope {
                    fight_chart_omitted: false,
                    stream_id: self.id.clone(),
                    sequence: Sequence(Timestamp(sequence)),
                    at: Timestamp(0),
                    event,
                }
            })
            .collect();
        maxed_store::apply_batch(db, &envelopes).unwrap();
    }
    pub fn next_sequence(&self) -> u64 {
        self.next
    }
}

pub fn session(id: &str, started: u64, last_traffic: u64, ended: Option<u64>) -> EngineEvent {
    EngineEvent::SessionUpdated(PlaySessionView {
        id: id.into(),
        started: Timestamp(started),
        last_traffic: Timestamp(last_traffic),
        ended: ended.map(Timestamp),
        issues: vec![],
    })
}

pub fn character_view(id: &str, name: &str, at: u64) -> CharacterView {
    CharacterView {
        id: id.into(),
        name: Some(name.into()),
        class_id: Some(782),
        level: Some(20),
        maxed: None,
        fame: None,
        stats: vec![],
        equipment: vec![],
        observed_at: Timestamp(at),
        identity_verified: true,
        died_at: None,
        issues: vec![],
        stat_panel: None,
    }
}

pub fn character(id: &str, name: &str, at: u64) -> EngineEvent {
    EngineEvent::CharacterUpdated(character_view(id, name, at))
}

const V9_SCRIPTS: [&str; 9] = [
    include_str!("../../migrations/001_initial.sql"),
    include_str!("../../migrations/002_fight_summaries.sql"),
    include_str!("../../migrations/003_loot_and_sessions.sql"),
    include_str!("../../migrations/004_alerts.sql"),
    include_str!("../../migrations/005_instance.sql"),
    include_str!("../../migrations/006_summary_target_kinds.sql"),
    include_str!("../../migrations/007_pb_lookup_indexes.sql"),
    include_str!("../../migrations/008_loot_browsing.sql"),
    include_str!("../../migrations/009_chat.sql"),
];

/// A schema-9 database as Logs shipped it, for upgrade tests and the migration benchmark.
pub fn v9_database(path: &std::path::Path) -> Connection {
    use sha2::Digest;
    let db = Connection::open(path).unwrap();
    for (index, script) in V9_SCRIPTS.iter().enumerate() {
        db.execute_batch(script).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            rusqlite::params![
                index as i64 + 1,
                format!("{:x}", sha2::Sha256::digest(script.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 9).unwrap();
    db
}

pub fn map(name: &str, kind: AreaKind, at: u64) -> EngineEvent {
    EngineEvent::MapEntered {
        realm_instance_key: None,
        at: Timestamp(at),
        name: name.into(),
        kind,
    }
}

pub fn fame(character: &str, fame: i32, at: u64) -> EngineEvent {
    EngineEvent::FameChanged {
        at: Timestamp(at),
        character_id: character.into(),
        fame,
    }
}

pub fn run(id: &str, session: &str, dungeon: &str, started: u64) -> RunView {
    RunView {
        id: id.into(),
        session_id: session.into(),
        character_id: None,
        dungeon: dungeon.into(),
        area_kind: AreaKind::Dungeon,
        started: Timestamp(started),
        ended: None,
        completed_at: None,
        died_at: None,
        outcome: None,
        evidence: vec![],
        party_size: None,
        party_size_estimated: true,
        complete_capture: true,
        issues: vec![],
        context: None,
    }
}

/// Every visit ordered by start. Asserts the JSON copy agrees with the columns.
pub fn visits(db: &Connection) -> Vec<MapVisitView> {
    let mut statement = db
        .prepare("SELECT json,id,session_id,character_id,run_id,map,started,ended FROM map_visits ORDER BY started,id")
        .unwrap();
    statement
        .query_map([], |r| {
            let view: MapVisitView = serde_json::from_str(&r.get::<_, String>(0)?).unwrap();
            assert_eq!(view.id, r.get::<_, String>(1)?);
            assert_eq!(view.session_id, r.get::<_, String>(2)?);
            assert_eq!(view.character_id, r.get::<_, Option<String>>(3)?);
            assert_eq!(view.run_id, r.get::<_, Option<String>>(4)?);
            assert_eq!(view.map, r.get::<_, String>(5)?);
            assert_eq!(view.started.0 as i64, r.get::<_, i64>(6)?);
            assert_eq!(view.ended.map(|t| t.0 as i64), r.get::<_, Option<i64>>(7)?);
            Ok(view)
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
