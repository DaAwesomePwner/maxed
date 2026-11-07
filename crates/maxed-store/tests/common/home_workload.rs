//! Distributed, synthetic Home history. Uses the normal event projector, including run caches.
use maxed_store::{apply_batch, migrate, open_reader, StoreReader};
use maxed_types::*;
use rusqlite::{params, Connection};

pub const DAY: u64 = 86_400_000_000;
pub const BASE: u64 = 1_700_000_000_000_000;
pub const CHARACTER: &str = "synthetic-character-0";
pub const DUNGEON: &str = "Synthetic Dungeon 0";

pub struct Fixture {
    pub _dir: tempfile::TempDir,
    pub path: std::path::PathBuf,
    pub db: Connection,
    pub range: TimeRange,
}

impl Fixture {
    pub fn build(days: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic-home.sqlite");
        let mut db = Connection::open(&path).unwrap();
        migrate(&mut db).unwrap();
        db.pragma_update(None, "journal_mode", "WAL").unwrap();
        let snapshot = demo::demo_snapshot();
        let session = snapshot.session.unwrap();
        let mut initial = vec![EngineEvent::SessionUpdated(session.clone())];
        for i in 0..3 {
            let mut character = snapshot.character.clone().unwrap();
            character.id = format!("synthetic-character-{i}");
            character.account_id = Some(crate::common::account_scope_fixture::ACCOUNT.into());
            character.name = Some(format!("Synthetic {i}"));
            initial.push(EngineEvent::CharacterUpdated(character));
        }
        let mut sequence = 0;
        let mut send = |db: &mut Connection, events: Vec<EngineEvent>| {
            let events: Vec<_> = events
                .into_iter()
                .map(|event| {
                    sequence += 1;
                    EventEnvelope {
                        stream_id: "synthetic-home-history".into(),
                        sequence: Sequence(Timestamp(sequence)),
                        at: Timestamp(BASE),
                        fight_chart_omitted: false,
                        event,
                    }
                })
                .collect();
            apply_batch(db, &events).unwrap();
        };
        send(&mut db, initial);
        for day in 0..days {
            let mut events = Vec::new();
            for slot in 0..20 {
                let group = day * 20 + slot;
                let started = BASE + day * DAY + slot * 1_800_000_000;
                let character = format!("synthetic-character-{}", (group / 4) % 3);
                let realm = group % 5 == 0;
                let mut run = snapshot.run.clone().unwrap();
                run.id = format!("synthetic-run-{group:05}");
                run.character_id = Some(character.clone());
                run.dungeon = format!("Synthetic Dungeon {}", group % 4);
                run.started = Timestamp(started);
                run.ended = None;
                run.completed_at = None;
                run.outcome = None;
                run.evidence.clear();
                run.issues.clear();
                if realm {
                    db.execute("INSERT INTO map_visits(id,stream_id,session_id,character_id,map,kind,started,ended,source,json)
                        VALUES(?1,'synthetic-home-history',?2,?3,'Synthetic Realm','realm',?4,?5,'live','{}')",
                        params![format!("synthetic-visit-{group:05}"), session.id, character, started as i64, (started+1_800_000_000) as i64]).unwrap();
                } else {
                    events.push(EngineEvent::RunStarted(run.clone()));
                }
                for boss in 0..7 {
                    let index = group * 7 + boss;
                    let mut fight = snapshot.fights[0].clone();
                    fight.id = format!("synthetic-fight-{index:05}");
                    fight.owner_character_id = Some(character.clone());
                    fight.run_id = (!realm).then(|| run.id.clone());
                    let target = if boss < 4 {
                        group % 4 * 8 + boss
                    } else {
                        32 + (group + boss) % 32
                    } as u16;
                    fight.target_type = Some(target);
                    fight.boss_type = Some(target);
                    fight.target_kind = Some(if realm {
                        TargetKind::EventBoss
                    } else {
                        TargetKind::Boss
                    });
                    fight.started = Timestamp(started + 60_000_000 + boss * 45_000_000);
                    let killed = Timestamp(fight.started.0 + 30_000_000 + index % 7);
                    fight.ended = (index % 17 != 0).then_some(killed);
                    fight.first_hit = Some(fight.started);
                    fight.last_hit = Some(killed);
                    fight.kill = Some(KillView {
                        at: killed,
                        evidence: KillEvidence::HpZeroRemoved,
                        provisional: index % 11 == 0,
                    });
                    fight.issues.clear();
                    let player = fight.players[0].clone();
                    fight.players = (0..7)
                        .map(|p| {
                            let mut player = player.clone();
                            player.player_id = format!("SyntheticPlayer{p}");
                            player.name = Some(format!("Synthetic {p}"));
                            player.object_id = p + 1;
                            player.is_local = p == 0;
                            player.damage = 30_000.0 + f64::from(p) * 3000.0;
                            player.dps = (p != 0 || index % 13 != 0).then_some(
                                1000.0 + (index % 19) as f64 * 100.0 + f64::from(p) * 10.0,
                            );
                            player.rank = Some(p as u32 + 1);
                            player
                        })
                        .collect();
                    fight.total_damage =
                        fight.players.iter().map(|p| p.damage).sum::<f64>() + 12345.0;
                    fight.unattributed_damage = 12345.0;
                    // Retain a moderate chart payload as well as the seven-player JSON list.
                    fight.buckets = fight.buckets.iter().cycle().take(60).cloned().collect();
                    events.push(EngineEvent::FightUpdated(fight));
                }
                if !realm {
                    run.ended = Some(Timestamp(started + 600_000_000));
                    run.completed_at = Some(Timestamp(started + 500_000_000));
                    run.outcome = Some(if group % 9 == 0 {
                        Outcome::Left
                    } else {
                        Outcome::Cleared
                    });
                    events.push(EngineEvent::RunEnded(run));
                }
            }
            send(&mut db, events);
        }
        assert_eq!(
            db.query_row("SELECT count(*) FROM fights", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            days as i64 * 140
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM fight_players", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            days as i64 * 980
        );
        Self {
            _dir: dir,
            path,
            db,
            range: TimeRange {
                start: Timestamp(BASE + days / 2 * DAY),
                end: Timestamp(BASE + days * DAY),
            },
        }
    }

    pub fn reader(&self) -> StoreReader {
        open_reader(&self.path).unwrap()
    }
    pub fn boss_request(&self, targets: usize) -> BossHistoryRequest {
        BossHistoryRequest {
            range: Some(self.range.clone()),
            character_id: Some(CHARACTER.into()),
            target_types: Some((0..targets as u16).collect()),
        }
    }
    pub fn profile_request(&self) -> DungeonProfileRequest {
        DungeonProfileRequest {
            range: self.range.clone(),
            character_id: Some(CHARACTER.into()),
            dungeon: DUNGEON.into(),
        }
    }
}
