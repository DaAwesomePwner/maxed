use crate::common::stats::{database, run, session, Stream};
use maxed_store::{open_reader, start_writer, StoreError, StoreWriter, WriterConfig};
use maxed_types::*;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;

#[test]
fn saved_run_flags_zero_damage_player_who_joined_between_boss_starts() {
    let (_dir, path, mut db) = database();
    let (mut stream, run, alpha) = seed(&mut db);
    let mut zero = profile(&run, "bravo", Some("SynthBravo"));
    zero.first_seen = Timestamp(run.started.0 + 60);
    zero.last_seen = Timestamp(run.started.0 + 65);
    let first = fight(&run, "party-fight", vec![player(&alpha, 100.)]);
    let mut second = fight(&run, "second-boss", vec![player(&alpha, 100.)]);
    second.started = Timestamp(run.started.0 + 70);
    second.first_hit = Some(second.started);
    second.last_hit = Some(Timestamp(run.started.0 + 80));
    second.ended = second.last_hit;
    assert!(first.started < zero.first_seen && zero.first_seen < second.started);
    stream.apply(
        &mut db,
        vec![
            EngineEvent::FightEnded(first),
            EngineEvent::PlayerObserved(zero),
            EngineEvent::FightEnded(second),
            end(&run),
        ],
    );
    let view = open_reader(&path)
        .unwrap()
        .get_party_run(&run.id)
        .unwrap()
        .unwrap();
    let member = view
        .members
        .iter()
        .find(|member| member.player_id == "bravo")
        .unwrap();
    assert_eq!(member.damage, Some(0.));
    assert!(member.flags.contains(&ContributionFlag::NoDamageSeen));
}

fn profile(run: &RunView, id: &str, name: Option<&str>) -> PlayerProfileView {
    PlayerProfileView {
        scope_id: run.id.clone(),
        run_id: Some(run.id.clone()),
        player_id: id.into(),
        object_id: 101,
        is_local: false,
        name: name.map(str::to_owned),
        class_id: Some(782),
        level: Some(20),
        stars: Some(12),
        fame: None,
        skin_id: None,
        guild: Some("Synthetic Guild".into()),
        stats: vec![],
        maxed: Some(6),
        maxed_stats: vec![],
        equipment: vec![EquipmentView {
            observed_empty: None,
            slot: 0,
            item_type: Some(1),
            enchant: EnchantView {
                state: EnchantState::Missing,
                slots: None,
                applied: None,
                ids: vec![],
            },
        }],
        first_seen: Timestamp(run.started.0 + 10),
        last_seen: Timestamp(run.started.0 + 20),
    }
}

fn player(profile: &PlayerProfileView, damage: f64) -> PlayerDamage {
    PlayerDamage {
        object_id: profile.object_id,
        player_id: profile.player_id.clone(),
        name: profile.name.clone(),
        class_id: profile.class_id,
        is_local: profile.is_local,
        damage,
        hits: 1,
        biggest_hit: Some(damage),
        deaths: 0,
        dps: None,
        share: None,
        rank: None,
        sources: vec![],
    }
}

fn fight(run: &RunView, id: &str, players: Vec<PlayerDamage>) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = id.into();
    fight.run_id = Some(run.id.clone());
    fight.scope_id = Some(run.id.clone());
    fight.session_id = run.session_id.clone();
    fight.started = Timestamp(run.started.0 + 30);
    fight.first_hit = Some(fight.started);
    fight.last_hit = Some(Timestamp(run.started.0 + 50));
    fight.ended = fight.last_hit;
    fight.end_reason = Some(FightEnd::Killed);
    fight.boss_type = None;
    fight.target_kind = Some(TargetKind::Boss);
    fight.total_damage = players.iter().map(|p| p.damage).sum();
    fight.players = players;
    fight.buckets.clear();
    fight
}

fn end(run: &RunView) -> EngineEvent {
    let mut run = run.clone();
    run.ended = Some(Timestamp(run.started.0 + 100));
    run.outcome = Some(Outcome::Cleared);
    EngineEvent::RunEnded(run)
}

fn left(run: &RunView, p: &PlayerProfileView) -> EngineEvent {
    EngineEvent::InstanceEvent(InstanceEventView {
        id: format!("{}-{}-left", run.id, p.player_id),
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        map: Some(run.dungeon.clone()),
        at: Timestamp(run.started.0 + 60),
        kind: InstanceEventKind::PlayerLeft {
            object_id: p.object_id,
            player_name: p.name.clone(),
            class_id: p.class_id,
        },
        provisional: false,
    })
}

fn seed(db: &mut Connection) -> (Stream, RunView, PlayerProfileView) {
    let mut stream = Stream::new("party-reads");
    let run = run("party-run", "party-session", "Synthetic Dungeon", 100);
    let alpha = profile(&run, "alpha", Some("SynthAlpha"));
    stream.apply(
        db,
        vec![
            session("party-session", 0, 10_000, None),
            EngineEvent::RunStarted(run.clone()),
            EngineEvent::PlayerObserved(alpha.clone()),
            EngineEvent::FightEnded(fight(&run, "party-fight", vec![player(&alpha, 100.)])),
            end(&run),
        ],
    );
    (stream, run, alpha)
}

fn writer(path: &std::path::Path) -> StoreWriter {
    start_writer(
        path.to_owned(),
        WriterConfig {
            stream_id: "party-reads".into(),
            flush_millis: 60_000,
            ..WriterConfig::default()
        },
    )
    .unwrap()
}

fn submit(writer: &StoreWriter, sequence: &mut u64, event: EngineEvent) {
    writer
        .try_submit(EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "party-reads".into(),
            sequence: Sequence(Timestamp(*sequence)),
            at: Timestamp(0),
            event,
        })
        .unwrap();
    *sequence += 1;
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn party_file_writes_preserve_open_runs_pending_alerts_and_watermarks() {
    use maxed_store::{party_write_file, PartyWrite};
    let (_dir, path, mut db) = database();
    let mut stream = Stream::new("cold-party");
    let run = run("open-party-run", "party-session", "Synthetic Dungeon", 100);
    stream.apply(
        &mut db,
        vec![
            session("party-session", 0, 10_000, None),
            EngineEvent::RunStarted(run.clone()),
        ],
    );
    db.execute_batch("INSERT INTO alerts(id,at,kind,json) VALUES('synthetic-alert',100,'white_bag','{}');
        INSERT INTO alert_occurrences(id,kind,source_key,source_stream_id,source_sequence,committed_at,loudness,state,alert_id)
        VALUES('synthetic-occurrence','white_bag','synthetic-source','cold-party',1,100,'toast','pending','synthetic-alert');").unwrap();
    let run_json = || {
        db.query_row("SELECT json FROM runs WHERE id=?1", [&run.id], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
    };
    let before = run_json();
    let mark = |key: &str, note: Option<String>| PartyWrite::Mark {
        key: key.into(),
        name: "SynthAlpha".into(),
        mark: Some(PartyMark::Watched),
        note,
    };
    assert!(party_write_file(&path, &mark("mismatched", None)).is_err());
    assert!(party_write_file(&path, &mark("synthalpha", Some("x".repeat(501)))).is_err());
    party_write_file(&path, &mark("synthalpha", Some("Synthetic note".into()))).unwrap();
    let read = || {
        open_reader(&path)
            .unwrap()
            .get_party_player("synthalpha", None)
            .unwrap()
    };
    assert_eq!(read().unwrap().row.mark, Some(PartyMark::Watched));
    party_write_file(
        &path,
        &PartyWrite::Clear(ClearPartyHistory {
            marks_and_notes: false,
        }),
    )
    .unwrap();
    assert_eq!(read().unwrap().row.note.as_deref(), Some("Synthetic note"));
    party_write_file(
        &path,
        &PartyWrite::Clear(ClearPartyHistory {
            marks_and_notes: true,
        }),
    )
    .unwrap();
    assert!(read().is_none());
    assert_eq!(run_json(), before);
    assert_eq!(
        db.query_row(
            "SELECT state FROM alert_occurrences WHERE id='synthetic-occurrence'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "pending"
    );
    assert_eq!(crate::common::receipts::heads(&db).len() as i64, 1);
    assert_eq!(crate::common::receipts::head(&db, "cold-party").unwrap(), 2);
}

#[test]
fn annotations_known_unknown_and_key_limit() {
    let (_dir, path, mut db) = database();
    seed(&mut db);
    db.execute("INSERT INTO party_run_members SELECT key,run_id,'alpha-rejoin',class_id,joined,departed_at,left_early,died,damage,share,boss_share FROM party_run_members", []).unwrap();
    let writer = writer(&path);
    writer
        .set_party_mark(
            "synthalpha",
            "SynthAlpha",
            Some(PartyMark::Watched),
            Some("Synthetic note"),
        )
        .unwrap();
    let reader = open_reader(&path).unwrap();
    assert_eq!(
        reader
            .party_annotations(&["synthalpha".into(), "synthunknown".into()])
            .unwrap(),
        vec![PartyAnnotation {
            key: "synthalpha".into(),
            mark: Some(PartyMark::Watched),
            note: Some("Synthetic note".into()),
            runs_together: 1
        }]
    );
    assert!(reader.party_annotations(&[]).unwrap().is_empty());
    assert_eq!(
        reader
            .party_annotations(&vec!["synthalpha".into(); 512])
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        reader.party_annotations(&vec!["synthalpha".into(); 513]),
        Err(StoreError::Limit)
    ));
    writer.shutdown().unwrap();
}

#[test]
fn players_page_every_sort_without_duplicates_or_gaps() {
    let (_dir, path, mut db) = database();
    let mut stream = Stream::new("party-reads");
    stream.apply(&mut db, vec![session("party-session", 0, 10_000, None)]);
    for n in 0..3 {
        let run = run(
            &format!("run-{n}"),
            "party-session",
            "Synthetic Dungeon",
            100 + n * 1000,
        );
        let mut events = vec![EngineEvent::RunStarted(run.clone())];
        let mut players = vec![];
        for i in 0..137 {
            if n > i % 3 {
                continue;
            }
            let mut p = profile(&run, &format!("p-{i}"), Some(&format!("Synth{i:03}")));
            p.object_id = i as i32;
            p.last_seen = Timestamp(run.started.0 + i);
            events.push(EngineEvent::PlayerObserved(p.clone()));
            let mut damage = player(&p, (i % 7) as f64);
            damage.deaths = u32::from(i.is_multiple_of(2));
            players.push(damage);
            if i.is_multiple_of(5) {
                events.push(left(&run, &p));
            }
        }
        events.push(EngineEvent::FightEnded(fight(
            &run,
            &format!("fight-{n}"),
            players,
        )));
        events.push(end(&run));
        stream.apply(&mut db, events);
    }
    // Repeating thirds straddle the first page; NULL shares sort last.
    db.execute(
        "UPDATE party_run_members SET share=CASE WHEN key>='synth120' THEN NULL ELSE 1.0/3 END",
        [],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    for sort in [
        PartyPlayerSort::LastSeen,
        PartyPlayerSort::RunsTogether,
        PartyPlayerSort::Deaths,
        PartyPlayerSort::LeftEarly,
        PartyPlayerSort::AvgShare,
    ] {
        let first = reader
            .list_party_players("", PartyPlayerFilter::All, sort.clone(), None)
            .unwrap();
        assert_eq!(first.rows.len(), 100);
        let next = first.next.as_ref().unwrap();
        let encoded = serde_json::to_string(next).unwrap();
        assert!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap()["sort_value"].is_string()
        );
        let next: PartyPlayersCursor = serde_json::from_str(&encoded).unwrap();
        if sort == PartyPlayerSort::AvgShare {
            let value: f64 = db
                .query_row(
                    "SELECT avg(share) FROM (
                        SELECT sum(share) AS share FROM party_run_members WHERE key=?1 GROUP BY run_id)",
                    [&next.key],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(next.sort_value, value.to_bits().to_string());
        }
        let second = reader
            .list_party_players("", PartyPlayerFilter::All, sort.clone(), Some(&next))
            .unwrap();
        assert_eq!(second.rows.len(), 37);
        assert!(second.next.is_none());
        let rows: Vec<_> = first.rows.into_iter().chain(second.rows).collect();
        assert_eq!(
            rows.iter().map(|r| &r.key).collect::<BTreeSet<_>>().len(),
            137
        );
        let value = |r: &PartyPlayerRow| match sort {
            PartyPlayerSort::LastSeen => r.last_seen.0 as f64,
            PartyPlayerSort::RunsTogether => f64::from(r.runs_together),
            PartyPlayerSort::Deaths => f64::from(r.deaths),
            PartyPlayerSort::LeftEarly => f64::from(r.left_early),
            PartyPlayerSort::AvgShare => r.avg_share.unwrap_or(-1.),
        };
        assert!(rows.windows(2).all(|w| value(&w[0]) > value(&w[1])
            || (value(&w[0]) == value(&w[1]) && w[0].key < w[1].key)));
        for row in rows {
            let i: u32 = row.key.trim_start_matches("synth").parse().unwrap();
            assert_eq!(row.runs_together, i % 3 + 1);
            assert_eq!(row.deaths, if i.is_multiple_of(2) { i % 3 + 1 } else { 0 });
            assert_eq!(
                row.left_early,
                if i.is_multiple_of(5) { i % 3 + 1 } else { 0 }
            );
        }
    }
}

#[test]
fn avg_share_combines_rejoins_per_run_and_pages_with_unknown_shares() {
    let (_dir, path, mut db) = database();
    let (mut stream, first, _) = seed(&mut db);
    for (id, started) in [("second-run", 1000), ("unknown-run", 2000)] {
        stream.apply(
            &mut db,
            vec![EngineEvent::RunStarted(run(
                id,
                "party-session",
                "Synthetic Dungeon",
                started,
            ))],
        );
    }
    db.execute("DELETE FROM party_run_members", []).unwrap();
    for (key, run_id, player_id, share) in [
        ("synthalpha", first.id.as_str(), "alpha", Some(0.1)),
        ("synthalpha", first.id.as_str(), "alpha-rejoin", Some(0.1)),
        ("synthalpha", "second-run", "alpha", Some(0.4)),
        ("synthalpha", "second-run", "alpha-rejoin", None),
        ("synthalpha", "unknown-run", "alpha", None),
        ("synthalpha", "unknown-run", "alpha-rejoin", None),
        ("synthbravo", first.id.as_str(), "bravo", Some(0.25)),
        ("synthunknown", "unknown-run", "unknown", None),
        ("synthunknown", "unknown-run", "unknown-rejoin", None),
    ] {
        db.execute(
            "INSERT INTO party_run_members(key,run_id,player_id,left_early,died,damage,share)
             VALUES(?1,?2,?3,0,0,0,?4)",
            params![key, run_id, player_id, share],
        )
        .unwrap();
    }
    // Put the combined average on the page boundary so its actual cursor is exercised.
    for i in 0..99 {
        db.execute(
            "INSERT INTO party_run_members(key,run_id,player_id,left_early,died,damage,share)
             VALUES(?1,?2,?1,0,0,0,0.5)",
            params![format!("synthfiller{i:03}"), first.id],
        )
        .unwrap();
    }
    db.execute(
        "INSERT OR IGNORE INTO party_players(key,name,first_seen,last_seen,updated)
         SELECT DISTINCT key,key,0,0,0 FROM party_run_members",
        [],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let detail = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert!((detail.row.avg_share.unwrap() - 0.3).abs() < 1e-12);
    assert_eq!(detail.row.runs_together, 3);
    assert_eq!(detail.runs.len(), 3);
    assert_eq!(detail.runs[0].share, None);
    assert_eq!(detail.runs[1].share, Some(0.4));
    assert_eq!(detail.runs[2].share, Some(0.2));

    let first = reader
        .list_party_players("", PartyPlayerFilter::All, PartyPlayerSort::AvgShare, None)
        .unwrap();
    assert_eq!(first.rows.len(), 100);
    let alpha = first.rows.last().unwrap();
    assert_eq!(alpha.key, "synthalpha");
    assert!((alpha.avg_share.unwrap() - 0.3).abs() < 1e-12);
    let cursor = first.next.unwrap();
    assert_eq!(cursor.key, alpha.key);
    assert!((f64::from_bits(cursor.sort_value.parse().unwrap()) - 0.3).abs() < 1e-12);
    let second = reader
        .list_party_players(
            "",
            PartyPlayerFilter::All,
            PartyPlayerSort::AvgShare,
            Some(&cursor),
        )
        .unwrap();
    assert!(second.next.is_none());
    assert_eq!(second.rows.len(), 2);
    assert_eq!(second.rows[0].key, "synthbravo");
    assert_eq!(second.rows[0].avg_share, Some(0.25));
    assert_eq!(second.rows[1].key, "synthunknown");
    assert_eq!(second.rows[1].avg_share, None);
}

#[test]
fn substring_search_escapes_wildcards_and_filters_annotations() {
    let (_dir, path, mut db) = database();
    seed(&mut db);
    let writer = writer(&path);
    for (key, name, mark, note) in [
        ("synth%alpha", "Synth%Alpha", Some(PartyMark::Watched), None),
        (
            "synth_alpha",
            "Synth_Alpha",
            Some(PartyMark::Banned),
            Some("Synthetic note"),
        ),
        ("synth\\alpha", "Synth\\Alpha", None, Some("Synthetic note")),
        ("synthblank", "SynthBlank", None, Some("")),
    ] {
        writer.set_party_mark(key, name, mark, note).unwrap();
    }
    let reader = open_reader(&path).unwrap();
    for (query, expected) in [
        ("%", "synth%alpha"),
        ("_", "synth_alpha"),
        ("\\", "synth\\alpha"),
        ("%aLP", "synth%alpha"),
    ] {
        let page = reader
            .list_party_players(
                query,
                PartyPlayerFilter::All,
                PartyPlayerSort::LastSeen,
                None,
            )
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].key, expected);
    }
    for (filter, expected) in [
        (PartyPlayerFilter::All, 5),
        (PartyPlayerFilter::Watched, 1),
        (PartyPlayerFilter::Banned, 1),
        (PartyPlayerFilter::WithNotes, 2),
    ] {
        assert_eq!(
            reader
                .list_party_players("", filter, PartyPlayerSort::LastSeen, None)
                .unwrap()
                .rows
                .len(),
            expected
        );
    }
    writer.shutdown().unwrap();
}

#[test]
fn player_runs_page_distinct_runs_and_latest_profile() {
    let (_dir, path, mut db) = database();
    let (mut stream, first, mut alpha) = seed(&mut db);
    for i in 0..24 {
        let mut run = first.clone();
        run.id = format!("run-{i:02}");
        run.started = Timestamp(1000 + i / 2);
        alpha.scope_id = run.id.clone();
        alpha.run_id = Some(run.id.clone());
        alpha.first_seen = run.started;
        alpha.last_seen = Timestamp(2000 + i);
        stream.apply(
            &mut db,
            vec![
                EngineEvent::RunStarted(run.clone()),
                EngineEvent::PlayerObserved(alpha.clone()),
                EngineEvent::FightEnded(fight(
                    &run,
                    &format!("fight-{i}"),
                    vec![player(&alpha, 100.)],
                )),
                end(&run),
            ],
        );
    }
    // Split the run's share across joins: per-run averaging stays 1, per-row is 25/26.
    db.execute(
        "UPDATE party_run_members SET share=0.5 WHERE run_id='run-23'",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO party_run_members SELECT key,run_id,'alpha-rejoin',class_id,joined,departed_at,1,1,damage,share,boss_share FROM party_run_members WHERE run_id='run-23'", []).unwrap();
    let reader = open_reader(&path).unwrap();
    let first = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(first.row.runs_together, 25);
    assert_eq!(first.row.deaths, 1);
    assert_eq!(first.row.left_early, 1);
    assert_eq!(first.row.avg_share, Some(1.));
    assert_eq!(first.row.last_class_id, alpha.class_id);
    assert_eq!(first.row.last_maxed, alpha.maxed);
    assert_eq!(first.guild, alpha.guild);
    assert_eq!(first.last_profile, Some(alpha));
    assert_eq!(first.first_seen, Timestamp(110));
    assert_eq!(first.runs.len(), 20);
    let second = reader
        .get_party_player("synthalpha", first.next.as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(second.runs.len(), 5);
    assert!(second.next.is_none());
    let runs: Vec<_> = first.runs.into_iter().chain(second.runs).collect();
    assert_eq!(
        runs.iter()
            .map(|r| &r.run_id)
            .collect::<BTreeSet<_>>()
            .len(),
        25
    );
    assert!(runs.windows(2).all(|w| w[0].started >= w[1].started));
    assert_eq!(
        runs.iter().find(|r| r.run_id == "run-23").unwrap().share,
        Some(1.)
    );
    assert!(reader
        .get_party_player("synthunknown", None)
        .unwrap()
        .is_none());
    db.execute("UPDATE party_players SET last_profile_json=NULL", [])
        .unwrap();
    assert_eq!(
        reader
            .get_party_player("synthalpha", None)
            .unwrap()
            .unwrap()
            .row
            .last_class_id,
        Some(782)
    );
}

#[test]
fn saved_run_unions_profiles_damage_local_and_merges_names_before_flags() {
    let (_dir, path, mut db) = database();
    let (mut stream, run, alpha) = seed(&mut db);
    let bravo = profile(&run, "bravo", Some("SynthBravo"));
    let mut local = profile(&run, "local", Some("SynthLocal"));
    local.is_local = true;
    let mut rejoin = alpha.clone();
    rejoin.player_id = "alpha-rejoin".into();
    rejoin.name = Some("SYNTHALPHA,suffix".into());
    rejoin.last_seen = Timestamp(190);
    rejoin.class_id = Some(800);
    rejoin.maxed = Some(8);
    rejoin.equipment[0].item_type = Some(2);
    let nameless = profile(&run, "nameless", None);
    let late = {
        let mut p = profile(&run, "late", Some("SynthLate"));
        p.first_seen = Timestamp(180);
        p
    };
    stream.apply(
        &mut db,
        vec![
            EngineEvent::PlayerObserved(bravo),
            EngineEvent::PlayerObserved(local.clone()),
            EngineEvent::PlayerObserved(rejoin.clone()),
            EngineEvent::PlayerObserved(nameless),
            EngineEvent::PlayerObserved(late),
            left(&run, &rejoin),
            EngineEvent::FightEnded(fight(
                &run,
                "party-fight",
                vec![
                    player(&alpha, 100.),
                    player(&rejoin, 50.),
                    player(&local, 150.),
                ],
            )),
            end(&run),
        ],
    );
    let reader = open_reader(&path).unwrap();
    let view = reader.get_party_run(&run.id).unwrap().unwrap();
    assert!(view.gear_recorded);
    assert!(!view.live);
    assert_eq!(view.area_kind, AreaKind::Dungeon);
    assert_eq!(view.map, Some(run.dungeon));
    assert_eq!(view.members.len(), 5);
    assert!(view
        .members
        .iter()
        .all(|p| p.present.is_none() && p.parse.is_none()));
    let merged = view
        .members
        .iter()
        .find(|p| p.name_key.as_deref() == Some("synthalpha"))
        .unwrap();
    assert_eq!(merged.damage, Some(150.));
    assert_eq!(merged.share, Some(0.5));
    assert_eq!(merged.boss_share, Some(0.5));
    assert_eq!(merged.class_id, rejoin.class_id);
    assert_eq!(merged.equipment, rejoin.equipment);
    assert_eq!(merged.maxed, Some(8));
    assert_eq!(merged.name.as_deref(), Some("SYNTHALPHA"));
    assert!(merged.flags.contains(&ContributionFlag::LeftEarly));
    assert!(!merged.flags.contains(&ContributionFlag::NoDamageSeen));
    let zero = view
        .members
        .iter()
        .find(|p| p.player_id == "bravo")
        .unwrap();
    assert_eq!(zero.damage, Some(0.));
    assert!(zero.flags.contains(&ContributionFlag::NoDamageSeen));
    assert!(!view
        .members
        .iter()
        .find(|p| p.player_id == "late")
        .unwrap()
        .flags
        .contains(&ContributionFlag::NoDamageSeen));
    assert_eq!(
        view.members.iter().find(|p| p.is_local).unwrap().damage,
        Some(150.)
    );
    assert!(view.members.iter().any(|p| p.name_key.is_none()));
    assert!(reader.get_party_run("missing-run").unwrap().is_none());
}

#[test]
fn legacy_run_keeps_damage_flags_and_has_no_gear() {
    let (_dir, path, mut db) = database();
    let (mut stream, run, alpha) = seed(&mut db);
    db.execute("DELETE FROM player_profiles", []).unwrap();
    stream.apply(&mut db, vec![left(&run, &alpha), end(&run)]);
    let view = open_reader(&path)
        .unwrap()
        .get_party_run(&run.id)
        .unwrap()
        .unwrap();
    assert!(!view.gear_recorded);
    assert_eq!(view.members.len(), 1);
    assert_eq!(view.members[0].damage, Some(100.));
    assert!(view.members[0].equipment.is_empty());
    assert_eq!(view.members[0].level, None);
    assert_eq!(view.members[0].maxed, None);
    assert!(view.members[0].flags.contains(&ContributionFlag::LeftEarly));
}

#[test]
fn marks_create_update_clear_and_validate_character_count() {
    let (_dir, path, _db) = database();
    let writer = writer(&path);
    let reader = open_reader(&path).unwrap();
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    writer
        .set_party_mark(
            "synthalpha",
            "SynthAlpha",
            Some(PartyMark::Watched),
            Some(&"é".repeat(500)),
        )
        .unwrap();
    let saved = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert!(saved.first_seen.0 >= before);
    assert_eq!(saved.first_seen, saved.row.last_seen);
    assert_eq!(saved.row.note.as_ref().unwrap().chars().count(), 500);
    assert!(matches!(
        writer.set_party_mark(
            "synthalpha",
            "SynthAlpha",
            Some(PartyMark::Banned),
            Some(&"é".repeat(501))
        ),
        Err(StoreError::InvalidEvent)
    ));
    assert_eq!(
        reader
            .get_party_player("synthalpha", None)
            .unwrap()
            .unwrap()
            .row
            .mark,
        Some(PartyMark::Watched)
    );
    writer
        .set_party_mark(
            "synthalpha",
            "SynthAlpha",
            Some(PartyMark::Banned),
            Some("Synthetic note"),
        )
        .unwrap();
    assert_eq!(
        reader
            .get_party_player("synthalpha", None)
            .unwrap()
            .unwrap()
            .row
            .mark,
        Some(PartyMark::Banned)
    );
    writer
        .set_party_mark("synthalpha", "SynthAlpha", None, None)
        .unwrap();
    let saved = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(saved.row.mark, None);
    assert_eq!(saved.row.note, None);
    assert!(saved.runs.is_empty());
    writer.shutdown().unwrap();
}

#[test]
fn clear_options_keep_local_profiles_damage_chat_and_events() {
    for marks_and_notes in [false, true] {
        let (_dir, path, mut db) = database();
        let (mut stream, run, alpha) = seed(&mut db);
        let mut local = profile(&run, "local", Some("SynthLocal"));
        local.is_local = true;
        let bravo = profile(&run, "bravo", Some("SynthBravo"));
        stream.apply(
            &mut db,
            vec![
                EngineEvent::PlayerObserved(local.clone()),
                EngineEvent::PlayerObserved(bravo),
                left(&run, &alpha),
            ],
        );
        for p in [&alpha, &local] {
            db.execute(
                "INSERT INTO player_timelines VALUES(?1,?2,?3)",
                params![
                    p.scope_id,
                    p.player_id,
                    serde_json::json!({"is_local":p.is_local}).to_string()
                ],
            )
            .unwrap();
        }
        // Same player ID in a different, local scope must not be removed by a player-ID-only join.
        let mut other_local = local.clone();
        other_local.scope_id = "other-scope".into();
        other_local.player_id = alpha.player_id.clone();
        stream.apply(
            &mut db,
            vec![EngineEvent::PlayerObserved(other_local.clone())],
        );
        db.execute(
            "INSERT INTO player_timelines VALUES(?1,?2,'{\"is_local\":true}')",
            params![other_local.scope_id, other_local.player_id],
        )
        .unwrap();
        let writer = writer(&path);
        writer
            .set_party_mark(
                "synthalpha",
                "SynthAlpha",
                Some(PartyMark::Watched),
                Some("Synthetic note"),
            )
            .unwrap();
        writer
            .set_party_mark("synthnote", "SynthNote", None, Some("Synthetic note"))
            .unwrap();
        db.execute("INSERT INTO chat_messages(id,session_id,run_id,at,channel,sender_kind,sender,is_local,text,json)
            VALUES('synthetic-chat','party-session','party-run',150,'public','player','SynthAlpha',0,'Synthetic message','{}')", []).unwrap();
        let tables = [
            "pd_run_players",
            "fights",
            "fight_players",
            "runs",
            "chat_messages",
            "instance_events",
        ];
        let before: Vec<_> = tables.iter().map(|t| count(&db, t)).collect();
        writer
            .clear_party(&ClearPartyHistory { marks_and_notes })
            .unwrap();
        assert_eq!(count(&db, "party_run_members"), 0);
        assert_eq!(
            count(&db, "party_players"),
            if marks_and_notes { 0 } else { 2 }
        );
        for table in ["player_profiles", "player_gear_log", "player_timelines"] {
            assert_eq!(count(&db, table), 2, "{table}");
        }
        assert_eq!(
            tables.iter().map(|t| count(&db, t)).collect::<Vec<_>>(),
            before
        );
        if !marks_and_notes {
            let saved = open_reader(&path)
                .unwrap()
                .get_party_player("synthalpha", None)
                .unwrap()
                .unwrap();
            assert_eq!(saved.row.mark, Some(PartyMark::Watched));
            assert_eq!(saved.row.note.as_deref(), Some("Synthetic note"));
            assert_eq!(saved.last_profile, None);
            assert_eq!(saved.row.last_map, None);
            assert_eq!(saved.row.runs_together, 0);
            assert_eq!(saved.row.last_class_id, None);
        }
        writer.shutdown().unwrap();
    }
}

#[test]
fn mark_flushes_pending_profile_and_preserves_history_fields() {
    let (_dir, path, mut db) = database();
    let (stream, run, _) = seed(&mut db);
    let writer = writer(&path);
    let mut sequence = stream.next_sequence();
    let bravo = profile(&run, "bravo", Some("SynthBravo"));
    submit(
        &writer,
        &mut sequence,
        EngineEvent::PlayerObserved(bravo.clone()),
    );
    writer
        .set_party_mark("synthbravo", "SynthBravo", Some(PartyMark::Watched), None)
        .unwrap();
    let saved = open_reader(&path)
        .unwrap()
        .get_party_player("synthbravo", None)
        .unwrap()
        .unwrap();
    assert_eq!(saved.first_seen, bravo.first_seen);
    assert_eq!(saved.row.last_seen, bravo.last_seen);
    assert_eq!(saved.last_profile, Some(bravo));
    assert_eq!(saved.row.mark, Some(PartyMark::Watched));
    writer.shutdown().unwrap();
}

#[test]
fn clear_rolls_back_all_tables_when_a_delete_fails() {
    let (_dir, path, mut db) = database();
    seed(&mut db);
    let writer = writer(&path);
    writer
        .set_party_mark("synthalpha", "SynthAlpha", Some(PartyMark::Watched), None)
        .unwrap();
    db.execute_batch(
        "CREATE TRIGGER synthetic_clear_failure BEFORE DELETE ON player_profiles
        BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;",
    )
    .unwrap();
    let tables = [
        "party_players",
        "party_run_members",
        "player_profiles",
        "player_gear_log",
    ];
    let before: Vec<_> = tables.iter().map(|t| count(&db, t)).collect();
    assert!(writer
        .clear_party(&ClearPartyHistory {
            marks_and_notes: true
        })
        .is_err());
    assert_eq!(
        tables.iter().map(|t| count(&db, t)).collect::<Vec<_>>(),
        before
    );
    db.execute_batch("DROP TRIGGER synthetic_clear_failure")
        .unwrap();
    writer
        .clear_party(&ClearPartyHistory {
            marks_and_notes: true,
        })
        .unwrap();
    assert!(tables.iter().all(|t| count(&db, t) == 0));
    writer.shutdown().unwrap();
}

#[test]
fn saved_run_boss_predicate_and_end_gate_flags_after_merging() {
    for (kind, boss_type, ended, is_boss) in [
        (TargetKind::Boss, None, true, true),
        (TargetKind::EventBoss, None, true, true),
        (TargetKind::Ordinary, Some(900), true, true),
        (TargetKind::Ordinary, None, true, false),
        (TargetKind::Boss, None, false, true),
    ] {
        let (_dir, path, mut db) = database();
        let (mut stream, run, alpha) = seed(&mut db);
        let mut trash = fight(&run, "party-fight", vec![player(&alpha, 60.)]);
        trash.target_kind = Some(TargetKind::Ordinary);
        let mut rejoin = profile(&run, "alpha-rejoin", Some("SYNTHALPHA,suffix"));
        rejoin.last_seen = Timestamp(190);
        let mut local = profile(&run, "local", Some("SynthLocal"));
        local.is_local = true;
        let zero = profile(&run, "bravo", Some("SynthBravo"));
        let mut boss = fight(
            &run,
            "boss-fight",
            vec![player(&rejoin, 40.), player(&local, 900.)],
        );
        boss.target_kind = Some(kind);
        boss.boss_type = boss_type;
        if !ended {
            boss.ended = None;
            boss.end_reason = None;
        }
        stream.apply(
            &mut db,
            vec![
                EngineEvent::PlayerObserved(rejoin.clone()),
                EngineEvent::PlayerObserved(zero),
                EngineEvent::FightEnded(trash),
                EngineEvent::FightUpdated(boss),
                end(&run),
            ],
        );
        let reader = open_reader(&path).unwrap();
        let view = reader.get_party_run(&run.id).unwrap().unwrap();
        let merged = view
            .members
            .iter()
            .find(|p| p.name_key.as_deref() == Some("synthalpha"))
            .unwrap();
        assert_eq!(merged.damage, Some(100.));
        // 40/940 boss share is too high for MostlyTrash, even though one identity has zero boss damage.
        assert!(!merged.flags.contains(&ContributionFlag::MostlyTrash));
        let zero = view
            .members
            .iter()
            .find(|p| p.player_id == "bravo")
            .unwrap();
        assert_eq!(
            zero.flags.contains(&ContributionFlag::NoDamageSeen),
            is_boss
        );
        // Lower the observed boss contribution to exercise the ended-boss gate.
        let mut boss = fight(
            &run,
            "boss-fight",
            vec![player(&rejoin, 1.), player(&local, 900.)],
        );
        boss.target_kind = Some(if is_boss {
            TargetKind::Boss
        } else {
            TargetKind::Ordinary
        });
        if !ended {
            boss.ended = None;
            boss.end_reason = None;
        }
        stream.apply(&mut db, vec![EngineEvent::FightUpdated(boss)]);
        let view = reader.get_party_run(&run.id).unwrap().unwrap();
        let merged = view
            .members
            .iter()
            .find(|p| p.name_key.as_deref() == Some("synthalpha"))
            .unwrap();
        assert_eq!(
            merged.flags.contains(&ContributionFlag::MostlyTrash),
            is_boss && ended
        );
    }
}

#[test]
fn budgets_cover_cumulative_reads_and_duplicated_output_fields() {
    let (_dir, path, mut db) = database();
    let (mut stream, run, mut alpha) = seed(&mut db);
    alpha.guild = Some("x".repeat(600_000));
    let text = serde_json::to_string(&alpha).unwrap();
    db.execute("UPDATE party_players SET last_profile_json=?1", [&text])
        .unwrap();
    let reader = open_reader(&path).unwrap();
    // The input profile fits, but guild appears both in the view and in last_profile.
    assert!(matches!(
        reader.get_party_player("synthalpha", None),
        Err(StoreError::Limit)
    ));
    let bravo = profile(&run, "bravo", Some("SynthBravo"));
    stream.apply(&mut db, vec![EngineEvent::PlayerObserved(bravo.clone())]);
    let mut bravo = bravo;
    bravo.guild = alpha.guild.clone();
    db.execute(
        "UPDATE party_players SET last_profile_json=?1 WHERE key='synthbravo'",
        [serde_json::to_string(&bravo).unwrap()],
    )
    .unwrap();
    let page = reader
        .list_party_players("", PartyPlayerFilter::All, PartyPlayerSort::LastSeen, None)
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(page
        .rows
        .iter()
        .all(|p| p.last_class_id == Some(782) && p.last_maxed == Some(6)));
    db.execute(
        "UPDATE player_profiles SET json=?1 WHERE player_id='alpha'",
        [&text],
    )
    .unwrap();
    db.execute(
        "UPDATE player_profiles SET json=?1 WHERE player_id='bravo'",
        [serde_json::to_string(&bravo).unwrap()],
    )
    .unwrap();
    assert!(matches!(
        reader.get_party_run(&run.id),
        Err(StoreError::Limit)
    ));
}

#[test]
fn clear_flushes_pending_profiles_and_open_run_end_rebuilds_only_that_run() {
    let (_dir, path, mut db) = database();
    let (stream, _, _) = seed(&mut db);
    let writer = writer(&path);
    let mut sequence = stream.next_sequence();
    let run = run("open-run", "party-session", "Synthetic Dungeon", 2000);
    let alpha = profile(&run, "alpha", Some("SynthAlpha,suffix"));
    let mut local = profile(&run, "local", Some("SynthLocal"));
    local.is_local = true;
    writer
        .set_party_mark(
            "synthbravo",
            "SynthBravo",
            Some(PartyMark::Watched),
            Some("Synthetic note"),
        )
        .unwrap();
    submit(&writer, &mut sequence, EngineEvent::RunStarted(run.clone()));
    submit(
        &writer,
        &mut sequence,
        EngineEvent::PlayerObserved(local.clone()),
    );
    submit(
        &writer,
        &mut sequence,
        EngineEvent::PlayerObserved(alpha.clone()),
    );
    submit(
        &writer,
        &mut sequence,
        EngineEvent::FightEnded(fight(&run, "open-fight", vec![player(&alpha, 100.)])),
    );
    writer
        .clear_party(&ClearPartyHistory {
            marks_and_notes: false,
        })
        .unwrap();
    assert_eq!(count(&db, "player_profiles"), 1);
    assert_eq!(count(&db, "party_run_members"), 0);
    submit(&writer, &mut sequence, end(&run));
    writer.flush().unwrap();
    assert_eq!(count(&db, "party_run_members"), 1);
    assert_eq!(
        db.query_row("SELECT run_id FROM party_run_members", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "open-run"
    );
    let reader = open_reader(&path).unwrap();
    let page = reader
        .list_party_players("", PartyPlayerFilter::All, PartyPlayerSort::LastSeen, None)
        .unwrap();
    let rebuilt = page.rows.iter().find(|p| p.key == "synthalpha").unwrap();
    assert_eq!(rebuilt.name, "SynthAlpha");
    assert_eq!(rebuilt.last_seen, run.started);
    assert_eq!(rebuilt.last_map.as_deref(), Some("Synthetic Dungeon"));
    assert_eq!(rebuilt.runs_together, 1);
    let saved = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(saved.first_seen, run.started);
    assert_eq!(saved.last_profile, None);
    assert_eq!(
        db.query_row(
            "SELECT updated FROM party_players WHERE key='synthalpha'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        run.started.0 as i64
    );
    let watched = page.rows.iter().find(|p| p.key == "synthbravo").unwrap();
    assert_eq!(watched.mark, Some(PartyMark::Watched));
    assert_eq!(watched.note.as_deref(), Some("Synthetic note"));
    let local_json: String = db
        .query_row("SELECT json FROM player_profiles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<PlayerProfileView>(&local_json).unwrap(),
        local
    );
    writer.shutdown().unwrap();
}

#[test]
fn clear_removes_remote_timelines_without_profiles() {
    let (_dir, path, db) = database();
    db.execute_batch(
        "INSERT INTO player_timelines VALUES
        ('synthetic-scope','remote','{\"is_local\":false}'),
        ('synthetic-scope','local','{\"is_local\":true}');",
    )
    .unwrap();
    let writer = writer(&path);
    writer
        .clear_party(&ClearPartyHistory {
            marks_and_notes: false,
        })
        .unwrap();
    let players: Vec<String> = db
        .prepare("SELECT player_id FROM player_timelines")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(players, vec!["local"]);
    writer.shutdown().unwrap();
}

#[test]
fn saved_run_departures_survive_clear() {
    let (_dir, path, mut db) = database();
    let (mut stream, run, alpha) = seed(&mut db);
    stream.apply(&mut db, vec![left(&run, &alpha), end(&run)]);
    let reader = open_reader(&path).unwrap();
    let before = reader.get_party_run(&run.id).unwrap().unwrap();
    assert!(before.members[0]
        .flags
        .contains(&ContributionFlag::LeftEarly));
    let writer = writer(&path);
    writer
        .clear_party(&ClearPartyHistory {
            marks_and_notes: true,
        })
        .unwrap();
    assert_eq!(count(&db, "party_run_members"), 0);
    let after = reader.get_party_run(&run.id).unwrap().unwrap();
    assert_eq!(after.members[0].departed_at, before.members[0].departed_at);
    assert!(after.members[0]
        .flags
        .contains(&ContributionFlag::LeftEarly));
    writer.shutdown().unwrap();
}

#[test]
fn saved_run_reads_zero_damage_departures_without_materialized_members() {
    let (_dir, path, mut db) = database();
    let (mut stream, run, _) = seed(&mut db);
    let mut bravo = profile(&run, "bravo", Some("SynthBravo"));
    bravo.object_id = 202;
    stream.apply(
        &mut db,
        vec![
            EngineEvent::PlayerObserved(bravo.clone()),
            left(&run, &bravo),
        ],
    );
    db.execute("DELETE FROM party_run_members", []).unwrap();
    let view = open_reader(&path)
        .unwrap()
        .get_party_run(&run.id)
        .unwrap()
        .unwrap();
    let member = view
        .members
        .iter()
        .find(|p| p.player_id == "bravo")
        .unwrap();
    assert_eq!(member.departed_at, Some(Timestamp(160)));
    assert!(member.flags.contains(&ContributionFlag::LeftEarly));
    assert_eq!(
        count(&db, "party_run_members"),
        0,
        "read must not rebuild history"
    );
}

#[test]
fn marks_require_the_canonical_name_key() {
    let (_dir, path, db) = database();
    let writer = writer(&path);
    for key in ["SynthAlpha", "synthbravo", "synthalpha,suffix"] {
        assert!(matches!(
            writer.set_party_mark(key, "SynthAlpha,suffix", Some(PartyMark::Watched), None),
            Err(StoreError::InvalidEvent)
        ));
    }
    assert_eq!(count(&db, "party_players"), 0);
    writer
        .set_party_mark(
            "synthalpha",
            "SynthAlpha,fe14",
            Some(PartyMark::Watched),
            None,
        )
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT name FROM party_players WHERE key='synthalpha'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "SynthAlpha"
    );
    writer.shutdown().unwrap();
}

#[test]
fn mark_created_player_accepts_successively_newer_replayed_profiles() {
    let (_dir, path, mut db) = database();
    let writer = writer(&path);
    writer
        .set_party_mark(
            "synthalpha",
            "synthalpha",
            Some(PartyMark::Watched),
            Some("Synthetic note"),
        )
        .unwrap();
    let reader = open_reader(&path).unwrap();
    let marked = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert!(marked.last_profile.is_none());
    writer.shutdown().unwrap();

    let (mut stream, run, alpha) = seed(&mut db);
    assert!(alpha.last_seen < marked.row.last_seen);
    let observed = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(observed.row.name, "SynthAlpha");
    assert_eq!(observed.row.last_map, Some(run.dungeon));
    assert_eq!(observed.last_profile, Some(alpha.clone()));
    assert_eq!(observed.first_seen, alpha.first_seen);
    assert_eq!(observed.row.last_seen, marked.row.last_seen);
    assert_eq!(observed.row.mark, Some(PartyMark::Watched));
    assert_eq!(observed.row.note.as_deref(), Some("Synthetic note"));
    assert_eq!(
        db.query_row(
            "SELECT last_scope_id FROM party_players WHERE key='synthalpha'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        alpha.scope_id
    );
    let mut newer = alpha.clone();
    newer.last_seen = Timestamp(alpha.last_seen.0 + 10);
    newer.maxed = Some(8);
    assert!(newer.last_seen < marked.row.last_seen);
    stream.apply(&mut db, vec![EngineEvent::PlayerObserved(newer.clone())]);
    let observed = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(observed.last_profile, Some(newer));
    assert_eq!(observed.row.last_maxed, Some(8));
    assert_eq!(observed.row.last_seen, marked.row.last_seen);
    stream.apply(&mut db, vec![EngineEvent::PlayerObserved(alpha)]);
    assert_eq!(
        reader
            .get_party_player("synthalpha", None)
            .unwrap()
            .unwrap(),
        observed
    );
}

#[test]
fn players_cursor_rejects_invalid_or_nonfinite_bit_strings() {
    let (_dir, path, _db) = database();
    let reader = open_reader(&path).unwrap();
    for value in ["", "0.3333333333333333", "-1", "00", "18446744073709551616"]
        .into_iter()
        .map(str::to_owned)
        .chain([f64::NAN, f64::INFINITY, f64::NEG_INFINITY].map(|f| f.to_bits().to_string()))
    {
        let cursor = PartyPlayersCursor {
            sort_value: value,
            key: "synthalpha".into(),
        };
        assert!(matches!(
            reader.list_party_players(
                "",
                PartyPlayerFilter::All,
                PartyPlayerSort::AvgShare,
                Some(&cursor)
            ),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn party_reads_enforce_one_mib_before_decoding_and_on_output() {
    let (_dir, path, mut db) = database();
    seed(&mut db);
    let reader = open_reader(&path).unwrap();
    let huge = "x".repeat(1024 * 1024 + 1);
    db.execute("UPDATE party_players SET name=?1", [&huge])
        .unwrap();
    assert!(matches!(
        reader.list_party_players("", PartyPlayerFilter::All, PartyPlayerSort::LastSeen, None),
        Err(StoreError::Limit)
    ));
    assert!(matches!(
        reader.get_party_player("synthalpha", None),
        Err(StoreError::Limit)
    ));
    db.execute(
        "UPDATE party_players SET name='SynthAlpha',last_profile_json=?1",
        [&huge],
    )
    .unwrap();
    assert!(matches!(
        reader.get_party_player("synthalpha", None),
        Err(StoreError::Limit)
    ));
    db.execute("UPDATE player_profiles SET json=?1", [&huge])
        .unwrap();
    assert!(matches!(
        reader.get_party_run("party-run"),
        Err(StoreError::Limit)
    ));
    db.execute("UPDATE party_players SET key=?1", [&huge])
        .unwrap();
    assert!(matches!(
        reader.party_annotations(&[huge]),
        Err(StoreError::Limit)
    ));
}
