//! Synthetic retained appearances only; no owner captures or catalog assets.
mod common;
use crate::common::account_scope_fixture::ACCOUNT;
use common::loot::{no_enchant, LootDb};
use maxed_types::*;
use rusqlite::params;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

// The default test harness runs ignored tests concurrently. Keep measurements isolated.
static BENCHMARK: Mutex<()> = Mutex::new(());
const RARE_ITEM: u16 = 65_000;

fn workload(count: u32) -> LootDb {
    let mut t = LootDb::new();
    t.run("template", "Template", 0, Some(100), Some(Outcome::Cleared));
    t.bag(
        "template",
        Some("template"),
        50,
        Some(255),
        Some((1, false)),
        &[
            (0, 0, None, no_enchant()),
            (1, 100, None, no_enchant()),
            (2, 300, None, no_enchant()),
        ],
    );
    let tx = t.db.transaction().unwrap();
    tx.execute_batch("CREATE TEMP TABLE numbers(i INTEGER PRIMARY KEY)")
        .unwrap();
    tx.execute(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i+1<?1)
        INSERT INTO numbers SELECT i FROM n",
        [count],
    )
    .unwrap();
    tx.execute("INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,died_at,outcome,complete_capture,entry_character_json,json)
        SELECT printf('r%05d',i),session_id,NULL,printf('Dungeon-%02d',i%40),
            (i*?1/20000)*100,((i+1)*?1/20000)*100,((i+1)*?1/20000)*100,NULL,'cleared',1,NULL,
            json_set(json,'$.id',printf('r%05d',i),'$.character_id',NULL,'$.dungeon',printf('Dungeon-%02d',i%40),
                '$.started',CAST((i*?1/20000)*100 AS TEXT),'$.ended',CAST(((i+1)*?1/20000)*100 AS TEXT),
                '$.completed_at',CAST(((i+1)*?1/20000)*100 AS TEXT))
        FROM numbers CROSS JOIN runs WHERE runs.id='template' AND i<20000", [count]).unwrap();
    tx.execute(
        "INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json,source_type)
        SELECT printf('b%06d',i),session_id,printf('r%05d',i*20000/?1),i*100+50,bag_type,
            CASE WHEN i%97=0 THEN 255 ELSE 10 END,
            json_set(json,'$.id',printf('b%06d',i),'$.run_id',printf('r%05d',i*20000/?1),
                '$.at',CAST(i*100+50 AS TEXT),'$.bag_rank',CASE WHEN i%97=0 THEN 255 ELSE 10 END,
                '$.source_enemy',i%80,
                '$.items',json_array(i%100,100+i%200,CASE WHEN i<5 THEN ?2 ELSE 300+i%300 END),
                '$.contents[0].item_type',i%100,'$.contents[1].item_type',100+i%200,
                '$.contents[2].item_type',CASE WHEN i<5 THEN ?2 ELSE 300+i%300 END),i%80
        FROM numbers CROSS JOIN drops WHERE drops.id='template'",
        params![count, RARE_ITEM],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO drop_items(drop_id,slot,item_type,quantity,json,taken_observed_at)
        SELECT d.id,j.key,json_extract(j.value,'$.item_type'),1,j.value,NULL
        FROM drops d,json_each(d.json,'$.contents') j WHERE d.id<>'template'",
        [],
    )
    .unwrap();
    tx.execute_batch(
        "DELETE FROM drop_items WHERE drop_id='template';
        DELETE FROM drops WHERE id='template';
        DELETE FROM run_evidence WHERE run_id='template';
        DELETE FROM timeline WHERE run_id='template';
        DELETE FROM runs WHERE id='template';
        DROP TABLE numbers;",
    )
    .unwrap();
    tx.commit().unwrap();
    for (sql, expected) in [
        ("SELECT count(*) FROM drops", count),
        ("SELECT count(*) FROM drop_items", count * 3),
        ("SELECT count(*) FROM runs", 20_000),
        ("SELECT count(DISTINCT dungeon) FROM runs", 40),
        (
            "SELECT count(DISTINCT drop_id) FROM drop_items WHERE item_type=65000",
            5,
        ),
    ] {
        assert_eq!(
            t.db.query_row(sql, [], |r| r.get::<_, u32>(0)).unwrap(),
            expected
        );
    }
    t
}

fn feed() -> DropFilter {
    DropFilter {
        min_rank: None,
        place: None,
        item_type: None,
        enchanted_only: false,
        range: None,
        cursor: None,
        limit: 50,
    }
}

fn timed<T>(
    count: u32,
    timings: &mut Vec<(&'static str, Duration)>,
    name: &'static str,
    read: impl FnOnce() -> T,
) -> T {
    let start = Instant::now();
    let result = read();
    let elapsed = start.elapsed();
    eprintln!("loot {count} {name}: {elapsed:?}");
    timings.push((name, elapsed));
    result
}

fn benchmark(count: u32) {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let _guard = BENCHMARK.lock().unwrap_or_else(|e| e.into_inner());
    let t = workload(count);
    let reader = t.reader();
    let mut timings = vec![];
    let page = timed(count, &mut timings, "feed first", || {
        reader.list_drops(feed()).unwrap()
    });
    assert_eq!(page.rows.len(), 50);
    let mut f = feed();
    f.place = Some(LootPlace::Dungeon {
        name: "Dungeon-00".into(),
    });
    let page = timed(count, &mut timings, "feed dungeon", || {
        reader.list_drops(f).unwrap()
    });
    assert_eq!(page.rows.len(), 50);
    let mut f = feed();
    f.item_type = Some(0);
    let page = timed(count, &mut timings, "feed common item", || {
        reader.list_drops(f).unwrap()
    });
    assert_eq!(page.rows.len(), 50);
    let h = timed(count, &mut timings, "history common item", || {
        reader
            .item_history(ItemHistoryRequest {
                item_type: 0,
                cursor: None,
                limit: 50,
            })
            .unwrap()
    });
    assert_eq!(h.totals.bags, count / 100);
    let h = timed(count, &mut timings, "history rare old item", || {
        reader
            .item_history(ItemHistoryRequest {
                item_type: RARE_ITEM,
                cursor: None,
                limit: 50,
            })
            .unwrap()
    });
    assert_eq!(h.totals.bags, 5);
    assert_eq!(h.bags.rows.len(), 5);
    assert!(h.bags.rows.iter().all(|r| r.drop.at.0 <= 450));
    let mut f = feed();
    f.item_type = Some(RARE_ITEM);
    let page = timed(count, &mut timings, "feed rare old item", || {
        reader.list_drops(f).unwrap()
    });
    assert_eq!(page.rows.len(), 5);
    let s = timed(count, &mut timings, "stats all time", || {
        reader.loot_stats(LootStatsFilter { range: None }).unwrap()
    });
    assert_eq!(s.dungeons.len(), 40);
    assert_eq!(s.dungeons.iter().map(|d| d.runs_ended).sum::<u32>(), 20_000);
    assert_eq!(s.totals.iter().map(|b| b.count).sum::<u32>(), count);
    assert!(s.dungeons.iter().all(|d| d.runs_since_last_white.is_some()));
    let s = timed(count, &mut timings, "stats date range", || {
        reader
            .loot_stats(LootStatsFilter {
                range: Some(TimeRange {
                    start: Timestamp(u64::from(count) * 5),
                    end: Timestamp(u64::from(count) * 95),
                }),
            })
            .unwrap()
    });
    assert_eq!(s.dungeons.len(), 40);
    assert_eq!(s.dungeons.iter().map(|d| d.runs_ended).sum::<u32>(), 18_000);
    assert_eq!(
        s.totals.iter().map(|b| b.count).sum::<u32>(),
        count * 9 / 10
    );
    let request = CollectionRequest {
        dungeon: Some("Dungeon-00".into()),
        chase: (0..100)
            .map(|i| ChaseItem {
                item_type: i,
                dungeon: (i % 2 == 0).then(|| "Dungeon-00".into()),
                pinned_at: Timestamp(0),
            })
            .collect(),
    };
    let tiers = (0..600)
        .map(|i| (i, Some(if i % 2 == 0 { "UT" } else { "ST" }.into())))
        .collect::<Vec<_>>();
    // Rotate five samples of stats, summaries, collection, and item history reads.
    let mut samples: [Vec<Duration>; 6] = std::array::from_fn(|_| Vec::new());
    for round in 0..5 {
        for offset in 0..6 {
            let index = (round + offset) % 6;
            if index == 0 {
                let start = Instant::now();
                let stats = reader.loot_stats(LootStatsFilter { range: None }).unwrap();
                samples[index].push(start.elapsed());
                assert_eq!(stats.dungeons.len(), 40);
                assert_eq!(stats.totals.iter().map(|b| b.count).sum::<u32>(), count);
            } else if index == 3 {
                let start = Instant::now();
                let collection = reader.collection(request.clone(), &tiers).unwrap();
                samples[index].push(start.elapsed());
                assert_eq!(collection.chase.len(), 100);
                assert!(!collection.grid.is_empty());
            } else if index >= 4 {
                let item_type = if index == 4 { 0 } else { RARE_ITEM };
                let start = Instant::now();
                let history = reader
                    .item_history(ItemHistoryRequest {
                        item_type,
                        cursor: None,
                        limit: 50,
                    })
                    .unwrap();
                samples[index].push(start.elapsed());
                assert_eq!(
                    history.totals.bags,
                    if index == 4 { count / 100 } else { 5 }
                );
            } else {
                let mut filter = feed();
                if index == 2 {
                    filter.range = Some(TimeRange {
                        start: Timestamp(0),
                        end: Timestamp(1_000_000),
                    });
                }
                let start = Instant::now();
                let summary = reader
                    .drop_summary(
                        DropSummaryRequest {
                            include_items: true,
                            filter,
                            days: vec![],
                        },
                        &tiers,
                    )
                    .unwrap();
                samples[index].push(start.elapsed());
                if index == 1 {
                    assert_eq!(summary.totals.bags, count);
                    assert_eq!(summary.totals.runs, 20_000);
                    assert_eq!(summary.totals.runs_ended, Some(20_000));
                    assert!(!summary.totals.items_counted);
                    assert_eq!(
                        (
                            summary.totals.ut_items,
                            summary.totals.st_items,
                            summary.totals.enchanted_items,
                            summary.totals.best_enchants
                        ),
                        (None, None, None, None)
                    );
                } else {
                    assert_eq!(summary.totals.bags, 10_000);
                    assert!(summary.totals.items_counted);
                    assert_eq!(
                        summary.totals.ut_items.unwrap() + summary.totals.st_items.unwrap(),
                        29_995
                    );
                }
            }
        }
    }
    for (label, sample) in [
        "stats all time",
        "summary all time",
        "summary 10k range",
        "collection",
        "history common item",
        "history rare old item",
    ]
    .into_iter()
    .zip(&mut samples)
    {
        eprintln!("loot {count} {label} samples: {sample:?}");
        sample.sort_unstable();
    }
    let collection_median = samples[3][2];
    eprintln!(
        "loot {count} history medians: common={:?}, rare={:?}",
        samples[4][2], samples[5][2]
    );
    let stats_median = samples[0][2];
    let summary_median = samples[1][2];
    let range_median = samples[2][2];
    eprintln!("loot {count} medians: stats={stats_median:?}, summary={summary_median:?} ({:.3}x), summary_10k={range_median:?} ({:.3}x)",
        summary_median.as_secs_f64()/stats_median.as_secs_f64(), range_median.as_secs_f64()/stats_median.as_secs_f64());
    let passes = |elapsed: Duration| {
        elapsed <= Duration::from_millis(150)
            || elapsed.as_secs_f64() <= 1.2 * stats_median.as_secs_f64()
    };
    eprintln!(
        "loot {count} collection median={collection_median:?} ({:.3}x stats)",
        collection_median.as_secs_f64() / stats_median.as_secs_f64()
    );
    let c = timed(count, &mut timings, "collection 100 chase + grid", || {
        reader.collection(request, &tiers).unwrap()
    });
    assert_eq!(c.chase.len(), 100);
    assert!(!c.grid.is_empty());
    let target = Duration::from_millis(if count == 50_000 { 150 } else { 750 });
    let misses = timings
        .iter()
        .filter(|(_, elapsed)| *elapsed >= target)
        .collect::<Vec<_>>();
    if count == 50_000 {
        assert!(passes(summary_median), "50k summary median {summary_median:?} exceeds 150ms and 1.2x stats median {stats_median:?}");
        assert!(passes(range_median), "10k ranged summary median {range_median:?} exceeds 150ms and 1.2x stats median {stats_median:?}");
        assert!(
            misses.is_empty(),
            "50k reads exceeded {target:?}: {misses:?}"
        );
    } else if !misses.is_empty() {
        eprintln!("loot {count} reported target misses ({target:?}): {misses:?}");
    }
}

#[test]
#[ignore = "release performance gate: 50k synthetic drops"]
fn loot_50k() {
    benchmark(50_000);
}

#[test]
#[ignore = "release performance report: 500k synthetic drops"]
fn loot_500k() {
    benchmark(500_000);
}

#[test]
#[ignore = "release timing comparison: Home streak versus all-time Loot at 50k and 500k drops"]
fn home_streak_50k_500k() {
    if cfg!(debug_assertions) {
        panic!("timing requires --release");
    }
    let _guard = BENCHMARK.lock().unwrap_or_else(|e| e.into_inner());
    for count in [50_000, 500_000] {
        let t = workload(count);
        let reader = t.reader();
        let request = HomeDungeonStreakRequest {
            dungeon: "Dungeon-00".into(),
        };
        let mut streak_samples = Vec::new();
        let mut stats_samples = Vec::new();
        // Alternate order so warming one query does not always favor the other.
        for round in 0..6 {
            let mut streak = None;
            let mut stats = None;
            for offset in 0..2 {
                let start = Instant::now();
                if (round + offset) % 2 == 0 {
                    streak = Some(
                        reader
                            .home_dungeon_streak(request.clone(), Some(ACCOUNT))
                            .unwrap(),
                    );
                    streak_samples.push(start.elapsed());
                } else {
                    stats = Some(reader.loot_stats(LootStatsFilter { range: None }).unwrap());
                    stats_samples.push(start.elapsed());
                }
            }
            let streak = streak.unwrap();
            let stats = stats.unwrap();
            let row = stats
                .dungeons
                .iter()
                .find(|row| row.dungeon == request.dungeon)
                .unwrap();
            assert_eq!(streak.runs_ended, row.runs_ended);
            assert_eq!(streak.runs_since_last_white, row.runs_since_last_white);
            assert_eq!(streak.avg_runs_per_white, row.avg_runs_per_white);
        }
        eprintln!("home streak {count} samples: {streak_samples:?}; full loot: {stats_samples:?}");
        streak_samples.sort_unstable();
        stats_samples.sort_unstable();
        let streak_median = (streak_samples[2] + streak_samples[3]) / 2;
        let stats_median = (stats_samples[2] + stats_samples[3]) / 2;
        eprintln!("home streak {count} medians: scoped={streak_median:?}, full={stats_median:?}, speedup={:.2}x",
            stats_median.as_secs_f64() / streak_median.as_secs_f64());
    }
}

#[test]
fn summary_plan_uses_covering_bags_and_indexed_appearances() {
    let t = LootDb::new();
    let sql = include_str!("../src/drop_summary.sql")
        .replace("{predicates}", "1=1")
        .replace(
            "{runs_ended}",
            "(SELECT count(*) FROM runs WHERE ended IS NOT NULL)",
        )
        .replace("{tiers}", "?1")
        .replace("{include_items}", "1")
        .replace("{item_bag_cap}", &MAX_SUMMARY_ITEM_BAGS.to_string());
    let plan =
        t.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(["[]"], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    assert!(
        plan.iter()
            .any(|step| step.contains("USING COVERING INDEX drops_stats")),
        "{plan:?}"
    );
    assert!(
        plan.iter().any(
            |step| step.contains("SEARCH appearance USING INDEX") && step.contains("drop_id=?")
        ),
        "{plan:?}"
    );
    assert!(
        !plan
            .iter()
            .any(|step| step.starts_with("SCAN appearance") || step.contains("SCAN drop_items")),
        "{plan:?}"
    );
}
