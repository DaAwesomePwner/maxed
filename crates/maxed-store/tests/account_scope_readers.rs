use crate::common::envelope;
use maxed_store::{apply_batch, migrate, open_reader, StoreReader};
use maxed_types::*;
use rusqlite::{params, Connection};

const A: &str = "account-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "account-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SECOND: u64 = 1_000_000;
const DUNGEON: &str = "Synthetic Account Dungeon";

struct Fixture {
    dir: tempfile::TempDir,
    db: Connection,
    sequence: u64,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Connection::open(dir.path().join("history.sqlite")).unwrap();
        migrate(&mut db).unwrap();
        let mut f = Self {
            dir,
            db,
            sequence: 0,
        };
        f.push(EngineEvent::SessionUpdated(PlaySessionView {
            id: "shared".into(),
            started: Timestamp(0),
            last_traffic: Timestamp(1000 * SECOND),
            ended: Some(Timestamp(1000 * SECOND)),
            issues: vec![CoverageIssue::StreamGap],
        }));
        for (id, account) in [
            ("a-7", Some(A)),
            ("a-8", Some(A)),
            ("b-7", Some(B)),
            ("legacy", None),
        ] {
            let mut c = demo::demo_snapshot().character.unwrap();
            c.id = id.into();
            c.account_id = account.map(str::to_owned);
            f.push(EngineEvent::CharacterUpdated(c));
        }
        f
    }
    fn push(&mut self, event: EngineEvent) {
        self.sequence += 1;
        apply_batch(&mut self.db, &[envelope(self.sequence, event)]).unwrap();
    }
    fn reader(&self) -> StoreReader {
        open_reader(&self.dir.path().join("history.sqlite")).unwrap()
    }
    fn run(&mut self, id: &str, character: &str, at: u64, duration: u64) {
        let mut r = demo::demo_snapshot().run.unwrap();
        r.id = id.into();
        r.session_id = "shared".into();
        r.character_id = Some(character.into());
        r.dungeon = DUNGEON.into();
        r.started = Timestamp(at * SECOND);
        r.completed_at = Some(Timestamp((at + duration) * SECOND));
        r.ended = r.completed_at;
        r.outcome = Some(Outcome::Cleared);
        r.area_kind = AreaKind::Dungeon;
        r.evidence = vec![EvidenceAt {
            at: r.completed_at.unwrap(),
            evidence: Evidence::Victory,
        }];
        r.issues.clear();
        self.push(EngineEvent::RunStarted(r));
    }
    fn fight(&mut self, id: &str, run: Option<&str>, owner: &str, at: u64, dps: f64, boss: u16) {
        let mut f = demo::demo_snapshot().fights.remove(0);
        f.id = id.into();
        f.session_id = "shared".into();
        f.run_id = run.map(str::to_owned);
        f.owner_character_id = Some(owner.into());
        f.started = Timestamp(at * SECOND);
        f.first_hit = Some(f.started);
        f.last_hit = Some(Timestamp((at + 30) * SECOND));
        f.ended = f.last_hit;
        f.local_active_micros = Some(30 * SECOND);
        f.complete_capture = true;
        f.issues.clear();
        f.boss_type = Some(boss);
        f.target_type = Some(boss);
        f.target_kind = Some(TargetKind::Boss);
        f.kill = Some(KillView {
            at: f.ended.unwrap(),
            evidence: KillEvidence::HpZeroRemoved,
            provisional: false,
        });
        f.players.retain(|p| p.is_local);
        f.players[0].player_id = owner.into();
        f.players[0].dps = Some(dps);
        f.players[0].damage = dps * 30.0;
        self.push(EngineEvent::FightEnded(f));
    }
    fn drop(&mut self, id: &str, run: Option<&str>, owner: &str, at: u64, item: u16) {
        let mut d = demo::demo_snapshot().drops.remove(0);
        d.id = id.into();
        d.session_id = "shared".into();
        d.run_id = run.map(str::to_owned);
        d.owner_character_id = Some(owner.into());
        d.at = Timestamp(at * SECOND);
        d.bag_rank = Some(255);
        d.items.truncate(1);
        d.items[0] = item;
        d.contents.truncate(1);
        d.contents[0].item_type = item;
        self.push(EngineEvent::BagDropped(d));
    }
    fn fame(&mut self, owner: &str, at: u64, fame: i32) {
        self.push(EngineEvent::FameChanged {
            at: Timestamp(at * SECOND),
            character_id: owner.into(),
            fame,
        });
    }
}
fn session() -> HomeScope {
    HomeScope::Session {
        id: "shared".into(),
    }
}
fn today() -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(1000 * SECOND),
        },
    }
}
fn profile() -> DungeonProfileRequest {
    DungeonProfileRequest {
        dungeon: DUNGEON.into(),
        character_id: None,
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(1000 * SECOND),
        },
    }
}

#[test]
fn account_scope_home_observations_fame_hud_and_unknown_notice() {
    let mut f = Fixture::new();
    f.run("a1", "a-7", 10, 100);
    f.run("b1", "b-7", 150, 1);
    f.run("a2", "a-8", 300, 90);
    f.fight("af", Some("a1"), "a-7", 20, 100., 42);
    f.fight("bf", Some("b1"), "b-7", 150, 9999., 43);
    f.fight("ar", None, "a-8", 400, 200., 42);
    f.drop("ad", Some("a1"), "a-7", 40, 100);
    f.drop("bd", None, "b-7", 200, 200);
    f.drop("ar", None, "a-8", 410, 101);
    f.fame("a-7", 0, 100);
    f.fame("b-7", 100, 1000);
    f.fame("a-7", 300, 130);
    f.fame("a-8", 400, 50);
    f.fame("a-8", 600, 80);
    f.fame("b-7", 700, 9000);
    // Open spans are clipped to recorded session end; unknown spans contribute nothing.
    for (seq, start, end, owner, account) in [
        (1, 0, Some(300), Some("a-7"), Some(A)),
        (2, 300, Some(600), Some("b-7"), Some(B)),
        (3, 600, Some(800), None, None),
        (4, 800, None, Some("a-8"), Some(A)),
    ] {
        f.db.execute(
            "INSERT INTO identity_spans VALUES('synthetic-spans',?1,'shared',?2,?3,?4,?5)",
            params![
                seq,
                (start * SECOND) as i64,
                end.map(|e| (e * SECOND) as i64),
                owner,
                account
            ],
        )
        .unwrap();
    }
    let reader = f.reader();
    for scope in [session(), today()] {
        let h = reader.home_summary(scope.clone(), Some(A)).unwrap();
        assert_eq!((h.runs, h.clears, h.fame_delta), (2, 2, Some(60)));
        assert_eq!(h.fame.len(), 4);
        assert_eq!(h.fame_characters.len(), 2);
        assert_eq!(h.top_fight.unwrap().id, "ar");
        assert!(!h.unattributed_history);
        assert!(h.recent_runs.iter().all(|r| r.id.starts_with('a')));
        assert!(h.issues.contains(&CoverageIssue::StreamGap));
        let loot = reader
            .scope_loot(scope.clone(), Some(A), None, 20, &[])
            .unwrap();
        assert_eq!(loot.total_bags, 2);
        assert!(loot.items.iter().all(|i| i.item_type != 200));
        let hud = reader
            .home_hud(
                scope.clone(),
                Some(A),
                &[(100, Some("UT".into())), (200, Some("ST".into()))],
                &[(101, PotionStat::Life)],
            )
            .unwrap();
        assert_eq!(hud.whites, 2);
        assert_eq!(hud.combat.unwrap().fights, 2);
        assert_eq!(hud.ut_st, vec![100]);
        assert_eq!(hud.potions[0].count, 1);
        assert_eq!(hud.fame_per_hour, Some(432.)); // 60 fame / 500 owned seconds.
        let empty = reader.home_summary(scope.clone(), None).unwrap();
        assert_eq!((empty.runs, empty.clears, empty.deaths), (0, 0, 0));
        assert!(empty.fame.is_empty());
        assert!(reader
            .home_hud(scope.clone(), None, &[], &[])
            .unwrap()
            .combat
            .is_none());
        assert_eq!(
            reader
                .scope_loot(scope, None, None, 20, &[])
                .unwrap()
                .total_bags,
            0
        );
    }
    let clipped = HomeScope::Today {
        range: TimeRange {
            start: Timestamp(100 * SECOND),
            end: Timestamp(900 * SECOND),
        },
    };
    assert_eq!(
        reader
            .home_hud(clipped, Some(A), &[], &[])
            .unwrap()
            .fame_per_hour,
        Some(360.)
    );
    f.run("unknown-in-scope", "legacy", 50, 1);
    f.fight(
        "unknown-in-scope",
        Some("unknown-in-scope"),
        "legacy",
        51,
        99999.,
        44,
    );
    for scope in [session(), today()] {
        let summary = reader.home_summary(scope.clone(), Some(A)).unwrap();
        assert_eq!((summary.runs, summary.clears), (2, 2));
        assert_eq!(summary.top_fight.unwrap().id, "ar");
        assert_eq!(
            reader
                .home_hud(scope, Some(A), &[], &[])
                .unwrap()
                .combat
                .unwrap()
                .fights,
            2
        );
    }
    f.run("unknown", "legacy", 2000, 1); // Outside both requested scopes, still a database-wide notice.
    assert!(
        reader
            .home_summary(session(), Some(A))
            .unwrap()
            .unattributed_history
    );
    assert!(
        reader
            .home_summary(today(), None)
            .unwrap()
            .unattributed_history
    );
}

#[test]
fn account_scope_bests_records_streak_and_learned_bosses() {
    let mut f = Fixture::new();
    f.run("unknown-fast", "legacy", 2, 1);
    f.fight(
        "unknown-fast",
        Some("unknown-fast"),
        "legacy",
        3,
        99999.,
        44,
    );
    f.drop("unknown-white", Some("unknown-fast"), "legacy", 4, 77);
    f.run("b-fast", "b-7", 1, 1);
    f.drop("b-white", Some("b-fast"), "b-7", 2, 77);
    for (id, owner, start, duration) in [
        ("a1", "a-7", 100, 100),
        ("a2", "a-8", 300, 90),
        ("a3", "a-7", 500, 80),
        ("a4", "a-8", 700, 70),
    ] {
        f.run(id, owner, start, duration);
        f.fight(id, Some(id), owner, start + 1, 100., 42);
    }
    f.fight("b-boss", Some("b-fast"), "b-7", 2, 9000., 43);
    f.drop("a-white", Some("a4"), "a-8", 750, 77);
    let reader = f.reader();
    let comparison = reader.get_run("a4").unwrap().unwrap().comparison.unwrap();
    assert_eq!(comparison.previous_clears, 3);
    assert_eq!(comparison.mean_micros, 90.0 * SECOND as f64);
    let best = reader.dungeon_bests(Some(A)).unwrap();
    assert_eq!(best.len(), 1);
    assert_eq!(best[0].best_clear_micros, 70 * SECOND);
    assert!(reader.dungeon_bests(None).unwrap().is_empty());
    let recap = reader
        .run_recap(
            &RunRecapRequest {
                run_id: "a4".into(),
            },
            &[77],
        )
        .unwrap()
        .unwrap();
    assert_eq!(recap.clear_number, Some(4));
    assert_eq!(recap.pb_clear.unwrap().previous_best_micros, 80 * SECOND);
    assert!(recap.milestones.contains(&RecapMilestone::FirstWhite));
    assert!(recap
        .milestones
        .iter()
        .any(|m| matches!(m,RecapMilestone::NewItems{item_types,..} if item_types==&[77])));
    assert_eq!(recap.dry_streak_end.unwrap().runs, 3);
    let streak = reader
        .home_dungeon_streak(
            HomeDungeonStreakRequest {
                dungeon: DUNGEON.into(),
            },
            Some(A),
        )
        .unwrap();
    assert_eq!(streak.runs_ended, 4);
    assert_eq!(streak.avg_runs_per_white, Some(4.));
    assert_eq!(
        reader
            .home_dungeon_streak(
                HomeDungeonStreakRequest {
                    dungeon: DUNGEON.into()
                },
                None
            )
            .unwrap()
            .runs_ended,
        0
    );
    let p = reader.dungeon_profile(profile(), Some(A)).unwrap();
    assert_eq!(p.clears, 4);
    assert_eq!(p.bosses.len(), 1);
    assert_eq!(p.bosses[0].target_type, 42);
    assert_eq!(p.bosses[0].kill_samples, 4);
    assert_eq!(reader.dungeon_profile(profile(), None).unwrap().clears, 0);
    let req = BossHistoryRequest {
        range: None,
        character_id: None,
        target_types: None,
    };
    let bosses = reader.home_boss_baselines(req.clone(), Some(A)).unwrap();
    assert_eq!(bosses.rows.len(), 1);
    assert_eq!(bosses.rows[0].target_type, 42);
    assert_eq!(bosses.rows[0].best_dps, Some(100.));
    assert!(reader
        .home_boss_baselines(req, None)
        .unwrap()
        .rows
        .is_empty());
    f.run("unknown", "legacy", 900, 1);
    let unknown = reader
        .run_recap(
            &RunRecapRequest {
                run_id: "unknown".into(),
            },
            &[77],
        )
        .unwrap()
        .unwrap();
    assert_eq!(unknown.clear_number, None);
    assert_eq!(unknown.pb_clear, None);
    assert_eq!(unknown.dry_streak_end, None);
    assert!(unknown.milestones.is_empty());
    f.db.execute("UPDATE runs SET character_id=NULL WHERE id='unknown'", [])
        .unwrap();
    let unknown = reader
        .run_recap(
            &RunRecapRequest {
                run_id: "unknown".into(),
            },
            &[],
        )
        .unwrap()
        .unwrap();
    assert_eq!(unknown.clear_number, None);
    assert_eq!(unknown.in_a_row, 0);
    assert!(reader
        .get_run("unknown")
        .unwrap()
        .unwrap()
        .comparison
        .is_none());
}

#[test]
fn account_scope_unknown_runless_notice_and_observation_owner_precedence() {
    let mut f = Fixture::new();
    f.run("a", "a-7", 10, 100);
    f.fight("foreign", Some("a"), "b-7", 20, 9000., 42);
    f.drop("foreign", Some("a"), "b-7", 30, 77);
    let reader = f.reader();
    let h = reader.home_summary(session(), Some(A)).unwrap();
    assert_eq!(h.runs, 1);
    assert!(h.top_fight.is_none());
    assert!(h.best_drop.is_none());
    assert_eq!(h.recent_details[0].observed_drop_count, 0);
    assert!(reader
        .run_recap(&RunRecapRequest { run_id: "a".into() }, &[77])
        .unwrap()
        .unwrap()
        .dry_streak_end
        .is_none());
    assert!(!h.unattributed_history);
    f.fight("legacy-fight", None, "legacy", 40, 5000., 42);
    assert!(
        reader
            .home_summary(today(), Some(A))
            .unwrap()
            .unattributed_history
    );
    f.db.execute(
        "UPDATE observation_characters SET character_id='a-7' WHERE observation_id='legacy-fight'",
        [],
    )
    .unwrap();
    assert!(
        !reader
            .home_summary(today(), Some(A))
            .unwrap()
            .unattributed_history
    ); // recovered observation ownership clears the notice
    f.drop("legacy-drop", None, "legacy", 50, 88);
    assert!(
        reader
            .home_summary(today(), Some(A))
            .unwrap()
            .unattributed_history
    );
    assert_eq!(
        reader
            .scope_loot(today(), Some(A), None, 10, &[])
            .unwrap()
            .total_bags,
        0
    );
    f.db.execute(
        "UPDATE observation_characters SET character_id='a-7' WHERE observation_id='legacy-drop'",
        [],
    )
    .unwrap();
    f.db.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,json) VALUES('unowned','shared',NULL,0,1,'{}')",[]).unwrap();
    assert!(
        reader
            .home_summary(today(), Some(A))
            .unwrap()
            .unattributed_history
    );
}

#[test]
fn account_scope_deaths_and_separate_sessions_do_not_borrow_ownership() {
    let mut f = Fixture::new();
    f.run("a", "a-7", 10, 100);
    f.run("b", "b-7", 20, 1);
    for (owner, run) in [
        (Some("a-7"), None),
        (Some("b-7"), None),
        (Some("b-7"), Some("a")),
        (None, Some("a")),
        (None, None),
    ] {
        f.push(EngineEvent::PlayerDied {
            at: Timestamp(200 * SECOND),
            character_id: owner.map(str::to_owned),
            run_id: run.map(str::to_owned),
        });
    }
    f.push(EngineEvent::SessionUpdated(PlaySessionView {
        id: "other".into(),
        started: Timestamp(0),
        last_traffic: Timestamp(1000 * SECOND),
        ended: Some(Timestamp(1000 * SECOND)),
        issues: vec![],
    }));
    f.run("other-a", "a-8", 300, 80);
    f.db.execute("UPDATE runs SET session_id='other',json=json_set(json,'$.session_id','other') WHERE id='other-a'",[]).unwrap();
    let reader = f.reader();
    let shared = reader.home_summary(session(), Some(A)).unwrap();
    assert_eq!((shared.runs, shared.deaths), (1, 2));
    let day = reader.home_summary(today(), Some(A)).unwrap();
    assert_eq!((day.runs, day.deaths), (2, 2));
    assert_eq!(
        reader
            .home_summary(HomeScope::Session { id: "other".into() }, Some(B))
            .unwrap()
            .runs,
        0
    );
}

#[test]
fn account_scope_realm_periods_stop_at_intervening_account() {
    let f = Fixture::new();
    for (id, character, at) in [
        ("first", "a-7", 10),
        ("boundary", "b-7", 20),
        ("last", "a-8", 30),
    ] {
        f.db.execute("INSERT INTO map_visits(id,stream_id,session_id,character_id,map,kind,realm_instance_key,started,ended,source,json) VALUES(?1,'synthetic','shared',?2,'Synthetic Realm','realm','same-instance',?3,?3+5,'live','{}')",params![id,character,at]).unwrap();
    }
    let reader = f.reader();
    let recap = reader
        .visit_recap(&VisitRecapRequest {
            visit_id: "last".into(),
        })
        .unwrap()
        .unwrap();
    assert_eq!(recap.periods.len(), 1);
    assert_eq!(recap.periods[0].visit_id, "last");
    assert!(recap.chain.is_none());
}

#[test]
fn account_scope_profile_dps_ignores_foreign_fights_in_owned_runs() {
    let mut f = Fixture::new();
    for (id, start) in [("a1", 100), ("a2", 300), ("a3", 500)] {
        f.run(id, "a-7", start, 100);
        f.fight(id, Some(id), "a-7", start + 1, 100., 42);
        f.fight(
            &format!("foreign-{id}"),
            Some(id),
            "b-7",
            start + 50,
            9000.,
            43,
        );
    }
    let reader = f.reader();
    for legacy in [false, true] {
        if legacy {
            f.db.execute("DELETE FROM pd_run_players", []).unwrap();
        }
        let p = reader.dungeon_profile(profile(), Some(A)).unwrap();
        assert_eq!(p.run_dps_samples, 3);
        assert_eq!(p.usual_run_dps, Some(100.));
        assert_eq!(p.bosses.len(), 1);
        assert_eq!(p.bosses[0].target_type, 42);
    }
}
