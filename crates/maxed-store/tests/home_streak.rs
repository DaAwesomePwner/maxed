use crate::common::account_scope_fixture::ACCOUNT;
use crate::common::loot::LootDb;
use maxed_store::StoreError;
use maxed_types::*;

fn request(dungeon: &str) -> HomeDungeonStreakRequest {
    HomeDungeonStreakRequest {
        dungeon: dungeon.into(),
    }
}

#[test]
fn matches_all_time_loot_rows_on_mixed_history() {
    let mut t = LootDb::new();
    for (id, dungeon, start, end) in [
        ("dry-a", "Synthetic Dry", 10, Some(20)),
        ("dry-b", "Synthetic Dry", 10, Some(30)),
        ("open-before", "Synthetic Open", 10, Some(20)),
        ("open-white", "Synthetic Open", 30, None),
        ("many-white", "Synthetic Many", 10, Some(30)),
        ("same-as-white", "Synthetic Many", 25, Some(40)),
        ("after-white", "Synthetic Many", 26, Some(40)),
        ("after-open", "Synthetic Many", 50, None),
        ("latest-before", "Synthetic Latest", 10, Some(20)),
        ("latest-white", "Synthetic Latest", 30, Some(40)),
    ] {
        t.run(id, dungeon, start, end, end.map(|_| Outcome::Cleared));
    }
    for (id, run, at, rank) in [
        ("ordinary", "dry-a", 15, 10),
        ("open", "open-white", 35, 255),
        ("many-a", "many-white", 15, 255),
        ("many-b", "many-white", 25, 255),
        ("latest", "latest-white", 35, 255),
    ] {
        t.bag(id, Some(run), at, Some(rank), None, &[]);
    }
    // An unassigned realm white cannot affect any dungeon.
    t.bag("realm", None, 100, Some(255), None, &[]);
    let reader = t.reader();
    let stats = reader.loot_stats(LootStatsFilter { range: None }).unwrap();
    assert_eq!(stats.dungeons.len(), 4);
    for row in stats.dungeons {
        let streak = reader
            .home_dungeon_streak(request(&row.dungeon), Some(ACCOUNT))
            .unwrap();
        assert_eq!(streak.dungeon, row.dungeon);
        assert_eq!(streak.runs_ended, row.runs_ended);
        assert_eq!(streak.runs_since_last_white, row.runs_since_last_white);
        assert_eq!(streak.avg_runs_per_white, row.avg_runs_per_white);
        let (ended, dry, average, last) = match row.dungeon.as_str() {
            "Synthetic Dry" => (2, None, None, None),
            "Synthetic Open" => (1, Some(0), None, Some(Timestamp(35))),
            "Synthetic Many" => (3, Some(1), Some(3.0), Some(Timestamp(25))),
            "Synthetic Latest" => (2, Some(0), Some(2.0), Some(Timestamp(35))),
            _ => unreachable!(),
        };
        assert_eq!(
            (
                streak.runs_ended,
                streak.runs_since_last_white,
                streak.avg_runs_per_white,
                streak.last_white_at
            ),
            (ended, dry, average, last)
        );
    }
}

#[test]
fn unknown_dungeon_is_empty() {
    let t = LootDb::new();
    assert_eq!(
        t.reader()
            .home_dungeon_streak(request("Synthetic Unknown"), Some(ACCOUNT))
            .unwrap(),
        HomeDungeonStreak {
            dungeon: "Synthetic Unknown".into(),
            ..Default::default()
        }
    );
}

#[test]
fn rejects_invalid_dungeon_identifiers() {
    let t = LootDb::new();
    for dungeon in [String::new(), "x".repeat(257), "bad\0dungeon".into()] {
        assert!(matches!(
            t.reader()
                .home_dungeon_streak(request(&dungeon), Some(ACCOUNT)),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn plan_searches_only_dungeon_runs_and_white_membership() {
    let t = LootDb::new();
    let sql = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/home_streak.sql"))
        .unwrap();
    let sql = crate::common::account_sql::with_accounts(&sql);
    let plan =
        t.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(rusqlite::params!["Synthetic Dry", ACCOUNT], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    assert!(
        plan.iter()
            .any(|s| s.contains("SEARCH r USING INDEX runs_loot_stats (dungeon=?)")),
        "{plan:?}"
    );
    assert!(
        plan.iter()
            .any(|s| s.contains("SEARCH d USING INDEX drops_white_runs (run_id=?)")),
        "{plan:?}"
    );
    assert!(
        !plan
            .iter()
            .any(|s| s.starts_with("SCAN r") || s.starts_with("SCAN d")),
        "{plan:?}"
    );
}
