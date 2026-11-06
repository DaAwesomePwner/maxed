//! Invented run-history scale fixture. No recording or owner history is read.
//! Writer seeds establish projections; SQL expands those exact DTO templates.
use super::{apply_batch, migrate, open_reader, StoreReader};
use maxed_types::*;
use rusqlite::{params, Connection};
use std::{path::PathBuf, time::Instant};

pub const DAY: u64 = 86_400_000_000;
pub const BASE: u64 = 1_790_830_800_000_000; // 2026-10-01 midnight, UTC-05:00.
pub const START: u64 = BASE + 60_000_000_000;
pub const RUN: &str = "demo-run";
pub const FIGHT: &str = "fight-00000";
pub const NAME: &str = "A\u{e9}\"\\\n";
pub const REASON: &str = "Synthetic estimate: missing projectile; \u{e9}\"\\\n";

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub name: &'static str,
    pub fights: usize,
    pub timeline: usize,
    pub bags: usize,
    pub large_fights: usize,
    pub large_players: usize,
    pub large_buckets: usize,
    pub players: usize,
    pub sources: usize,
    pub buckets: usize,
    pub story_budget: usize,
    pub fight_budget: usize,
}

pub const REALISTIC: Shape = Shape {
    name: "realistic",
    fights: 400,
    timeline: 600,
    bags: 20,
    large_fights: 20,
    large_players: 8,
    large_buckets: 120,
    players: 3_200,
    sources: 9_600,
    buckets: 5_440,
    story_budget: 1_560_576,
    fight_budget: 129_024,
};
pub const WORST: Shape = Shape {
    name: "worst-case",
    fights: 3_000,
    timeline: 5_000,
    bags: 200,
    large_fights: 10,
    large_players: 85,
    large_buckets: 1_800,
    players: 24_770,
    sources: 74_310,
    buckets: 41_920,
    story_budget: 12_455_936,
    fight_budget: 874_496,
};

pub struct Fixture {
    pub _dir: tempfile::TempDir,
    pub path: PathBuf,
    pub db: Connection,
    pub shape: Shape,
}

pub fn bytes(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

fn enchant(recorded: bool) -> EnchantView {
    EnchantView {
        state: if recorded {
            EnchantState::Recorded
        } else {
            EnchantState::Missing
        },
        slots: recorded.then_some(2),
        applied: recorded.then_some(2),
        ids: if recorded { vec![10, 20] } else { vec![] },
    }
}

fn detail(id: usize, players: usize, buckets: usize, large: bool) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = format!("fight-{id:05}");
    fight.target_id = id as i32 + 2;
    fight.boss_type = large.then_some(300);
    fight.started = Timestamp(START + 1_000_000);
    fight.ended = Some(Timestamp(START + 3_000_000));
    fight.first_hit = Some(fight.started);
    fight.last_hit = fight.ended;
    fight.end_reason = Some(FightEnd::MapChanged);
    fight.issues = vec![CoverageIssue::MissingProjectile];
    let template = fight.players.remove(0);
    fight.players = (0..players)
        .map(|i| {
            let mut player = template.clone();
            player.player_id = format!("player-{i:03}");
            player.object_id = i as i32 + 1;
            player.name = Some(format!("{NAME}{i:03}"));
            player.is_local = i == 0;
            player.rank = Some(i as u32 + 1);
            player.share = Some(1.0 / players as f64);
            player.hits = 6;
            player.biggest_hit = if i % 2 == 0 { None } else { Some(50.0) };
            player.sources = (0..3)
                .map(|s| {
                    let mut source = template.sources[0].clone();
                    source.item_type = (s != 2).then_some(200);
                    source.kind =
                        [SourceKind::Weapon, SourceKind::Ability, SourceKind::Unknown][s].clone();
                    source.ability = s == 1;
                    source.damage = 100.0;
                    source.biggest_hit = (s == 0).then_some(50.0);
                    source.estimated = s == 2;
                    source.reasons = if s == 2 { vec![REASON.into()] } else { vec![] };
                    source.enchant = enchant(s == 1);
                    source
                })
                .collect();
            player
        })
        .collect();
    fight.total_damage = 300.0 * players as f64;
    fight.buckets = (0..buckets)
        .map(|i| DamageBucket {
            from: Timestamp(START + 1_000_000 + i as u64 * 1_000),
            until: Timestamp(START + 1_000_000 + (i as u64 + 1) * 1_000),
            total: fight.total_damage / buckets as f64,
            local: Some(300.0 / buckets as f64),
            others_damage: None,
            others_presence: None,
        })
        .collect();
    fight
}

pub fn build(shape: Shape) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.sqlite");
    let mut db = Connection::open(&path).unwrap();
    let migration = Instant::now();
    migrate(&mut db).unwrap();
    eprintln!(
        "{} fresh schema migration (no legacy backfill): {:?}",
        shape.name,
        migration.elapsed()
    );
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    let snapshot = demo::demo_snapshot();
    let mut session = snapshot.session.unwrap();
    session.started = Timestamp(BASE - DAY);
    session.last_traffic = Timestamp(START + 5_000_000);
    session.ended = Some(session.last_traffic);
    let mut character = snapshot.character.unwrap();
    character.observed_at = Timestamp(BASE - DAY);
    let mut other = character.clone();
    other.id = "archive-character".into();
    let mut run = snapshot.run.unwrap();
    run.started = Timestamp(START);
    run.ended = Some(Timestamp(START + 5_000_000));
    run.completed_at = Some(Timestamp(START + 4_000_000));
    run.died_at = Some(Timestamp(START + 4_500_000));
    run.evidence = vec![EvidenceAt {
        at: run.completed_at.unwrap(),
        evidence: Evidence::Victory,
    }];
    run.evidence.extend([
        EvidenceAt {
            at: run.died_at.unwrap(),
            evidence: Evidence::DeathPacket,
        },
        EvidenceAt {
            at: run.ended.unwrap(),
            evidence: Evidence::ObservedExit,
        },
        EvidenceAt {
            at: Timestamp(START + 2_000_000),
            evidence: Evidence::CaptureGap,
        },
    ]);
    run.issues = vec![CoverageIssue::StreamGap];
    run.party_size = Some(shape.large_players as u32);
    let mut events = vec![
        EngineEvent::SessionUpdated(session),
        EngineEvent::CharacterUpdated(character),
        EngineEvent::CharacterUpdated(other),
        EngineEvent::RunStarted(run),
        EngineEvent::FightStarted(detail(0, shape.large_players, shape.large_buckets, true)),
        EngineEvent::FightStarted(detail(shape.large_fights, 8, 8, false)),
    ];
    for i in 0..shape.bags {
        let mut drop = snapshot.drops[0].clone();
        drop.id = format!("bag-{i:05}");
        drop.at = Timestamp(START + 3_000_000 + i as u64);
        drop.contents_known = i % 2 == 0;
        drop.contents = if drop.contents_known {
            (0..8)
                .map(|slot| DropItem {
                    slot,
                    item_type: 200 + u16::from(slot),
                    quantity: 1,
                    enchant: enchant(slot % 2 == 0),
                    taken_observed_at: None,
                })
                .collect()
        } else {
            vec![]
        };
        drop.items = drop.contents.iter().map(|d| d.item_type).collect();
        events.push(EngineEvent::BagDropped(drop));
    }
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-b3".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: Timestamp(START + 5_000_000),
            event,
        })
        .collect();
    apply_batch(&mut db, &events).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute_batch(
        "CREATE TEMP TABLE numbers(i INTEGER PRIMARY KEY);
        WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<9999)
        INSERT INTO numbers SELECT i FROM n;",
    )
    .unwrap();
    tx.execute(
        "INSERT INTO runs
        SELECT printf('archive-%05d',i),r.session_id,
        CASE WHEN i%2=0 THEN 'demo-character' ELSE 'archive-character' END,
        CASE WHEN i%4=0 THEN 'Other Sanctum' ELSE r.dungeon END,
        ?1+i*12000000,?1+i*12000000+1000000*(1+i%7),
        CASE WHEN i%3<>0 THEN ?1+i*12000000+1000000*(1+i%7) END,NULL,
        CASE WHEN i%3<>0 THEN 'cleared' ELSE 'left' END,0,NULL,
        json_set(r.json,'$.id',printf('archive-%05d',i),
            '$.character_id',CASE WHEN i%2=0 THEN 'demo-character' ELSE 'archive-character' END,
            '$.dungeon',CASE WHEN i%4=0 THEN 'Other Sanctum' ELSE r.dungeon END,
            '$.started',CAST(?1+i*12000000 AS TEXT),
            '$.ended',CAST(?1+i*12000000+1000000*(1+i%7) AS TEXT),
            '$.completed_at',CASE WHEN i%3<>0 THEN CAST(?1+i*12000000+1000000*(1+i%7) AS TEXT) END,
            '$.died_at',NULL,'$.outcome',CASE WHEN i%3<>0 THEN 'cleared' ELSE 'left' END,
            '$.evidence',json(CASE WHEN i%3<>0 THEN json_array(json_object('at',
                CAST(?1+i*12000000+1000000*(1+i%7) AS TEXT),'evidence','victory')) ELSE '[]' END))
        FROM numbers CROSS JOIN runs r WHERE r.id=?2 AND i BETWEEN 1 AND 9999",
        params![(BASE - DAY) as i64, RUN],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO run_evidence
        SELECT id,0,completed_at,'victory',json_extract(json,'$.evidence[0]')
        FROM runs WHERE id<>?1 AND outcome='cleared'",
        [RUN],
    )
    .unwrap();
    tx.execute("INSERT INTO fights
        SELECT printf('fight-%05d',i),f.run_id,f.session_id,f.started,f.ended,f.boss_type,
        f.local_dps,f.complete_capture,json_set(f.json,'$.id',printf('fight-%05d',i),'$.target_id',i+2)
        FROM numbers CROSS JOIN fights f
        WHERE i<?1 AND i NOT IN (0,?2)
        AND f.id=CASE WHEN i<?2 THEN 'fight-00000' ELSE printf('fight-%05d',?2) END",
        params![shape.fights as i64, shape.large_fights as i64]).unwrap();
    tx.execute(
        "INSERT INTO fight_summaries
        SELECT printf('fight-%05d',i),s.run_id,s.started,
        json_set(s.json,'$.id',printf('fight-%05d',i))
        FROM numbers CROSS JOIN fight_summaries s
        WHERE i<?1 AND i NOT IN (0,?2)
        AND s.fight_id=CASE WHEN i<?2 THEN 'fight-00000' ELSE printf('fight-%05d',?2) END",
        params![shape.fights as i64, shape.large_fights as i64],
    )
    .unwrap();
    tx.execute_batch(
        "DELETE FROM damage_sources; DELETE FROM fight_players;
        INSERT INTO fight_players SELECT f.id,p.value->>'$.player_id',p.value->>'$.object_id',
            p.value->>'$.damage',p.value->>'$.dps',p.value->>'$.rank',p.value
            FROM fights f,json_each(f.json,'$.players') p;
        INSERT INTO damage_sources SELECT p.fight_id,p.player_id,s.key,s.value->>'$.item_type',
            s.value->>'$.kind',s.value->>'$.damage',s.value->>'$.hits',s.value
            FROM fight_players p,json_each(p.json,'$.sources') s;
        DELETE FROM timeline WHERE kind='fight';
        INSERT INTO timeline SELECT 'fight:'||id,run_id,started,'fight',id,
            'Observed target damage; defeat is not inferred' FROM fights;",
    )
    .unwrap();
    let existing: i64 = tx
        .query_row("SELECT count(*) FROM timeline", [], |r| r.get(0))
        .unwrap();
    tx.execute(
        "INSERT INTO timeline SELECT printf('issue-%05d',i),?1,?2+i,
        'capture_issue',?1,?3 FROM numbers WHERE i<?4",
        params![
            RUN,
            (START + 2_000_000) as i64,
            REASON,
            shape.timeline as i64 - existing
        ],
    )
    .unwrap();
    tx.execute_batch("DROP TABLE numbers;").unwrap();
    tx.commit().unwrap();
    Fixture {
        _dir: dir,
        path,
        db,
        shape,
    }
}

pub fn cases() -> Vec<(&'static str, RunFilter)> {
    let base = RunFilter {
        character_id: None,
        dungeon: None,
        outcome: None,
        range: None,
        sort: RunSort::Newest,
        cursor: None,
        limit: 50,
    };
    let mut combined = base.clone();
    combined.dungeon = Some("Synthetic Sanctum".into());
    combined.outcome = Some(Outcome::Cleared);
    combined.range = Some(TimeRange {
        start: Timestamp(BASE),
        end: Timestamp(BASE + DAY),
    });
    let mut deep = base.clone();
    deep.cursor = Some(RunCursor {
        sort_value: (BASE - DAY + 1_000 * 12_000_000).to_string(),
        id: "archive-01000".into(),
    }); // 9,000 rows before cursor; no OFFSET in the reader.
    vec![
        ("newest", base.clone()),
        ("combined-day", combined),
        (
            "oldest",
            RunFilter {
                sort: RunSort::Oldest,
                ..base.clone()
            },
        ),
        (
            "duration-ascending",
            RunFilter {
                sort: RunSort::DurationAscending,
                ..base.clone()
            },
        ),
        (
            "duration-descending",
            RunFilter {
                sort: RunSort::DurationDescending,
                ..base
            },
        ),
        ("deep-keyset", deep),
    ]
}

impl Fixture {
    /// Counts come from persisted tables/JSON, never from the query output alone.
    pub fn validate(&self) -> StoreReader {
        let s = self.shape;
        let db = &self.db;
        let count = |sql: &str| db.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize;
        assert_eq!(count("SELECT count(*) FROM runs"), 10_000);
        assert_eq!(
            count("SELECT count(*) FROM runs WHERE id<>'demo-run'"),
            9_999
        );
        for (table, expected) in [
            ("fights", s.fights),
            ("fight_summaries", s.fights),
            ("timeline", s.timeline),
            ("drops", s.bags),
        ] {
            assert_eq!(count(&format!("SELECT count(*) FROM {table}")), expected);
            assert_eq!(
                count(&format!(
                    "SELECT count(*) FROM {table} WHERE run_id='demo-run'"
                )),
                expected
            );
        }
        assert_eq!(count("SELECT count(*) FROM fight_players"), s.players);
        assert_eq!(count("SELECT count(*) FROM damage_sources"), s.sources);
        assert_eq!(
            count("SELECT sum(json_array_length(json,'$.buckets')) FROM fights"),
            s.buckets
        );
        assert_eq!(
            count("SELECT sum(json_array_length(json,'$.players')) FROM fights"),
            s.players
        );
        assert_eq!(
            count("SELECT sum(json_array_length(json,'$.sources')) FROM fight_players"),
            s.sources
        );
        assert_eq!(
            count("SELECT count(*) FROM damage_sources WHERE json_extract(json,'$.estimated')=1"),
            s.sources / 3
        );
        assert_eq!(
            count("SELECT count(*) FROM drops WHERE json_extract(json,'$.contents_known')=0"),
            s.bags / 2
        );
        assert_eq!(count("SELECT count(*) FROM drop_items"), s.bags / 2 * 8);
        assert_eq!(
            count("SELECT count(*) FROM fights WHERE json_array_length(json,'$.buckets')>8"),
            s.large_fights
        );
        assert_eq!(count(&format!("SELECT count(*) FROM fights WHERE json_array_length(json,'$.buckets')={} AND json_array_length(json,'$.players')={}",s.large_buckets,s.large_players)),s.large_fights);
        assert_eq!(
            count(
                "SELECT count(*) FROM fight_players WHERE json_array_length(json,'$.sources')<>3"
            ),
            0
        );
        assert_eq!(count("SELECT count(*) FROM fights WHERE id<>json_extract(json,'$.id') OR run_id<>json_extract(json,'$.run_id') OR started<>CAST(json_extract(json,'$.started') AS INTEGER) OR ended<>CAST(json_extract(json,'$.ended') AS INTEGER)"),0);
        assert_eq!(count("SELECT count(*) FROM runs WHERE id<>json_extract(json,'$.id') OR dungeon<>json_extract(json,'$.dungeon') OR character_id<>json_extract(json,'$.character_id') OR outcome<>json_extract(json,'$.outcome') OR started<>CAST(json_extract(json,'$.started') AS INTEGER) OR ended IS NOT CAST(json_extract(json,'$.ended') AS INTEGER) OR completed_at IS NOT CAST(json_extract(json,'$.completed_at') AS INTEGER)"),0);
        assert_eq!(count("SELECT count(*) FROM fight_summaries s JOIN fights f ON f.id=s.fight_id WHERE s.fight_id<>json_extract(s.json,'$.id') OR s.run_id<>f.run_id OR s.started<>f.started OR json_extract(s.json,'$.player_count')<>json_array_length(f.json,'$.players')"),0);
        assert_eq!(count(&format!("SELECT count(*) FROM runs WHERE dungeon='Synthetic Sanctum' AND outcome='cleared' AND started>={BASE} AND started<{}",BASE+DAY)),1_401);
        assert_eq!(
            count(&format!(
                "SELECT count(*) FROM runs WHERE started>{}",
                BASE - DAY + 1_000 * 12_000_000
            )),
            9_000
        );
        assert_eq!(count("SELECT count(*) FROM fights WHERE json_array_length(json,'$.buckets')=8 AND json_array_length(json,'$.players')=8"), s.fights-s.large_fights);
        assert_eq!(
            count("SELECT count(*) FROM timeline WHERE kind='fight'"),
            s.fights
        );
        assert_eq!(
            count("SELECT count(*) FROM timeline WHERE kind='drop'"),
            s.bags
        );
        for kind in ["entered", "completion", "death", "exit"] {
            assert_eq!(
                count(&format!(
                    "SELECT count(*) FROM timeline WHERE kind='{kind}'"
                )),
                1
            );
        }
        assert_eq!(count("SELECT count(*) FROM pragma_foreign_key_check"), 0);
        // Nested detail is exactly the persisted player/source row, not just equal counts.
        assert_eq!(
            count(
                "SELECT count(*) FROM fights f,json_each(f.json,'$.players') p
            LEFT JOIN fight_players fp ON fp.fight_id=f.id AND fp.player_id=p.value->>'$.player_id'
            WHERE fp.json IS NULL OR fp.json<>p.value"
            ),
            0
        );
        assert_eq!(count("SELECT count(*) FROM fight_players p,json_each(p.json,'$.sources') s
            LEFT JOIN damage_sources ds ON ds.fight_id=p.fight_id AND ds.player_id=p.player_id AND ds.source_index=s.key
            WHERE ds.json IS NULL OR ds.json<>s.value"), 0);
        assert_eq!(
            s.story_budget,
            s.fights * 1_024 + s.timeline * 1_536 + s.bags * 8_192 + 65_536
        );
        assert_eq!(
            s.fight_budget,
            s.large_players * 4_096 + s.large_buckets * 256 + 65_536
        );
        assert!(s.story_budget < 32 * 1024 * 1024);
        assert!(s.fight_budget < 8 * 1024 * 1024);
        assert_eq!(NAME.len(), 6);
        assert_eq!(bytes(&NAME), 11); // Quotes, backslash and newline are escaped; \u{e9} stays UTF-8.
        assert_eq!(REASON.len(), 45);
        assert_eq!(bytes(&REASON), 50);
        let reader = open_reader(&self.path).unwrap();
        let story = reader.get_run(RUN).unwrap().unwrap();
        assert_eq!(
            (story.fights.len(), story.timeline.len(), story.drops.len()),
            (s.fights, s.timeline, s.bags)
        );
        assert!(story
            .fights
            .iter()
            .enumerate()
            .all(|(i, f)| f.ordinal == i as u32 + 1 && f.id == format!("fight-{i:05}")));
        for row in &story.fights {
            assert_eq!(row.id.len(), 11);
            assert!(row.target_name.is_none());
            assert!(bytes(row) < 1_024);
        }
        for row in &story.timeline {
            assert!(bytes(row) < 1_536);
        }
        for row in &story.drops {
            assert_eq!(row.id.len(), 9);
            assert!(bytes(row) < 8_192);
        }
        let mut header = story.clone();
        header.fights.clear();
        header.timeline.clear();
        header.drops.clear();
        header.boss_fights = None;
        header.events = None;
        assert!(bytes(&header) <= 65_536);
        // Keep the existing summary budget; full boss details have their own
        // existing fight budget, and the combined story retains its hard cap.
        let mut summary = story.clone();
        summary.boss_fights = None;
        assert!(bytes(&summary) <= s.story_budget);
        assert!(bytes(&story) <= 32 * 1024 * 1024);
        let expected_clears = (1..10_000).filter(|i| i % 3 != 0 && i % 4 != 0).count();
        assert!(expected_clears > 3_000);
        assert_eq!(
            story.comparison.as_ref().unwrap().previous_clears as usize,
            expected_clears
        );
        let expected_mean = (1..10_000)
            .filter(|i| i % 3 != 0 && i % 4 != 0)
            .map(|i| (1 + i % 7) as f64 * 1_000_000.0)
            .sum::<f64>()
            / expected_clears as f64;
        assert_eq!(
            story.comparison.as_ref().unwrap().mean_micros,
            expected_mean
        );
        let fight = reader.get_fight(FIGHT).unwrap().unwrap();
        // SQL expansion clones the large boss template for every large fight.
        let bosses = story.boss_fights.as_ref().unwrap();
        assert_eq!(bosses.len(), s.large_fights);
        for (i, boss) in bosses.iter().enumerate() {
            let mut expected = fight.clone();
            expected.id = format!("fight-{i:05}");
            expected.target_id = i as i32 + 2;
            assert_eq!(boss, &expected);
        }
        assert_eq!(story.boss_fights_limited, Some(false));
        assert_eq!(story.events.as_ref().unwrap(), &vec![]);
        assert_eq!(
            (fight.players.len(), fight.buckets.len()),
            (s.large_players, s.large_buckets)
        );
        assert!(bytes(&fight) <= s.fight_budget);
        for p in &fight.players {
            assert_eq!(p.sources.len(), 3);
            assert!(bytes(p) < 4_096);
            assert_eq!(p.name.as_ref().unwrap().len(), 9);
        }
        for b in &fight.buckets {
            assert!(bytes(b) < 256);
        }
        let player = reader.get_player_breakdown(FIGHT, 1).unwrap().unwrap();
        assert_eq!(player, fight.players[0]);
        assert!(bytes(&player) <= 4_096);
        for (name, filter) in cases() {
            let page = reader.list_runs(filter.clone()).unwrap();
            // Independent in-memory ordering over known generated headers.
            let mut expected: Vec<_> = (1..10_000)
                .map(|i| {
                    (
                        format!("archive-{i:05}"),
                        BASE - DAY + i * 12_000_000,
                        (1 + i % 7) * 1_000_000,
                        i,
                    )
                })
                .collect();
            expected.push((RUN.into(), START, 5_000_000, 10_000));
            if filter.dungeon.is_some() {
                expected.retain(|r| r.3 == 10_000 || (r.3 % 3 != 0 && r.3 % 4 != 0));
            }
            if let Some(range) = &filter.range {
                expected.retain(|r| r.1 >= range.start.0 && r.1 < range.end.0);
            }
            if filter.cursor.is_some() {
                expected.retain(|r| r.1 < BASE - DAY + 1_000 * 12_000_000);
            }
            expected.sort_by(|a, b| {
                let key = |r: &(String, u64, u64, u64)| {
                    if matches!(filter.sort, RunSort::Newest | RunSort::Oldest) {
                        r.1
                    } else {
                        r.2
                    }
                };
                let order = key(a).cmp(&key(b)).then_with(|| a.0.cmp(&b.0));
                if matches!(filter.sort, RunSort::Newest | RunSort::DurationDescending) {
                    order.reverse()
                } else {
                    order
                }
            });
            assert_eq!(
                page.rows.iter().map(|r| &r.id).collect::<Vec<_>>(),
                expected[..50].iter().map(|r| &r.0).collect::<Vec<_>>(),
                "{name}"
            );
            assert_eq!(page.next.as_ref().unwrap().id, page.rows[49].id);
            assert!(bytes(&page) <= 1024 * 1024);
            eprintln!(
                "{} list {name}: rows={} bytes={}",
                s.name,
                page.rows.len(),
                bytes(&page)
            );
        }
        eprintln!("{} counts: runs=10000 fights={} timeline={} bags={} players={} sources={} buckets={} eligible-clears={expected_clears}; bytes story={} fight={} player={}",
            s.name,s.fights,s.timeline,s.bags,s.players,s.sources,s.buckets,bytes(&story),bytes(&fight),bytes(&player));
        for label in cases()
            .into_iter()
            .map(|(label, _)| label)
            .chain(["story", "fight", "player"])
        {
            self.assert_plans(label, &self.planned_queries(label));
        }
        reader
    }
}

impl Fixture {
    /// Expected expanded reader SQL. The unit trace test compares every statement
    /// byte-for-byte with production, so this integration-test plan input cannot drift silently.
    pub fn planned_queries(&self, label: &str) -> Vec<String> {
        let completed_at = START + 4_000_000;
        match label {
            "story" => vec![
                format!("SELECT length(CAST(json AS BLOB)),json,length(CAST(entry_character_json AS BLOB)),entry_character_json FROM runs WHERE id='{RUN}'"),
                format!("SELECT length(CAST(json AS BLOB)),json FROM fight_summaries WHERE run_id='{RUN}' ORDER BY started,fight_id LIMIT 12001"),
                format!("SELECT length(CAST(json AS BLOB)),json FROM drops WHERE run_id='{RUN}' ORDER BY at,id LIMIT 2001"),
                format!("SELECT length(CAST(id AS BLOB))+coalesce(length(CAST(related_id AS BLOB)),0)+length(CAST(explanation AS BLOB))+length(CAST(kind AS BLOB)),id,at,kind,related_id,explanation FROM timeline WHERE run_id='{RUN}' AND (kind!='player_left' OR {completed_at} IS NULL OR at<{completed_at}) ORDER BY at,id LIMIT 20001"),
                format!("SELECT count(*),avg(completed_at-started) FROM runs WHERE dungeon='Synthetic Sanctum' AND outcome='cleared' AND started<{START} AND completed_at>started AND CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END='dungeon' AND EXISTS(SELECT 1 FROM run_evidence e WHERE e.run_id=runs.id AND e.kind IN ('victory','completion_dialogue'))"),
            ],
            "fight" => vec![format!("SELECT length(CAST(json AS BLOB)),json FROM fights WHERE id='{FIGHT}'")],
            "player" => vec![format!("SELECT length(CAST(json AS BLOB)),json FROM fight_players WHERE fight_id='{FIGHT}' AND object_id=1 LIMIT 2")],
            _ => {
                let key = match label {
                    "duration-ascending" => "coalesce((CASE WHEN ended>=started THEN ended-started END),9223372036854775807)",
                    "duration-descending" => "coalesce((CASE WHEN ended>=started THEN ended-started END),-1)",
                    _ => "started",
                };
                let direction = if matches!(label,"oldest"|"duration-ascending") { "ASC" } else { "DESC" };
                let predicates = match label {
                    "combined-day" => format!(" AND dungeon='Synthetic Sanctum' AND outcome='cleared' AND started>={BASE} AND started<{}",BASE+DAY),
                    "deep-keyset" => format!(" AND (started,id)<({},'archive-01000')",BASE-DAY+1_000*12_000_000),
                    _ => String::new(),
                };
                vec![format!("SELECT length(CAST(json AS BLOB)),json,{key} FROM runs WHERE 1=1{predicates} ORDER BY {key} {direction},id {direction} LIMIT 51")]
            }
        }
    }

    pub fn assert_plans(&self, label: &str, statements: &[String]) {
        assert_eq!(
            statements,
            self.planned_queries(label),
            "production SQL changed; review plan contract for {label}"
        );
        assert_eq!(statements.len(), if label == "story" { 5 } else { 1 });
        for sql in statements {
            assert!(!sql.to_ascii_uppercase().contains("OFFSET"));
            let mut stmt = self
                .db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let plan = stmt
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            eprintln!(
                "{} {label}: {} SELECTs; {sql}\n{plan}",
                self.shape.name,
                statements.len()
            );
            let index = if label == "story" {
                assert!(
                    !sql.contains("FROM fights")
                        && !sql.contains("fight_players")
                        && !sql.contains("damage_sources")
                );
                if sql.contains("FROM fight_summaries") {
                    "fight_summaries_run"
                } else if sql.contains("FROM drops") {
                    "drops_run"
                } else if sql.contains("FROM timeline") {
                    "timeline_run"
                } else if sql.contains("count(*)") {
                    assert!(plan.contains("pd_run_evidence_kind"));
                    "pd_runs_comparison"
                } else {
                    "sqlite_autoindex_runs_1"
                }
            } else {
                match label {
                    "newest" | "oldest" | "deep-keyset" => "runs_started",
                    "combined-day" => "runs_dungeon_outcome",
                    "duration-ascending" => "runs_duration_ascending",
                    "duration-descending" => "runs_duration_descending",
                    "fight" => "sqlite_autoindex_fights_1",
                    "player" => "fight_players_object",
                    _ => unreachable!(),
                }
            };
            assert!(plan.contains(index), "{label} requires {index}: {plan}");
            if matches!(
                label,
                "story" | "fight" | "player" | "deep-keyset" | "combined-day"
            ) {
                assert!(plan.contains("SEARCH"), "{label} must seek: {plan}");
                assert!(!plan.contains("SCAN"), "{label} must not scan: {plan}");
            }
            assert!(
                !plan.contains("TEMP B-TREE"),
                "{label}: unexpected temporary sort: {plan}"
            );
        }
    }
}
