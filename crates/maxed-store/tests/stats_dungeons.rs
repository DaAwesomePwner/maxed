use crate::common;
use common::stats::*;
use maxed_types::*;

fn all() -> StatsFilter {
    StatsFilter {
        range: None,
        character_id: None,
    }
}

#[test]
fn lifetime_rows_count_time_clears_and_deaths() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    let mut cleared = run("r1", "s1", "Synthetic Halls", MIN);
    cleared.character_id = Some("synth-character".into());
    cleared.ended = Some(Timestamp(11 * MIN));
    cleared.completed_at = Some(Timestamp(10 * MIN));
    cleared.outcome = Some(Outcome::Cleared);
    cleared.evidence = vec![EvidenceAt {
        at: Timestamp(10 * MIN),
        evidence: Evidence::Victory,
    }];
    let mut died = run("r2", "s1", "Synthetic Halls", 20 * MIN);
    died.ended = Some(Timestamp(23 * MIN));
    died.died_at = Some(Timestamp(23 * MIN));
    died.outcome = Some(Outcome::Died);
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 30 * MIN, Some(30 * MIN)),
            character("synth-character", "SynthAlpha", 0),
            fame("synth-character", 100, MIN),
            EngineEvent::RunEnded(cleared),
            EngineEvent::RunEnded(died),
        ],
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    let result = reader.stats_dungeons(all()).unwrap();
    assert!(!result.truncated);
    let row = &result.rows[0];
    assert_eq!(
        (row.dungeon.as_str(), row.runs, row.clears, row.deaths),
        ("Synthetic Halls", 2, 1, 1)
    );
    assert_eq!(row.observed_clears, 1);
    assert_eq!(row.best_clear_micros, Some(9 * MIN));
    assert_eq!(row.total_micros, 13 * MIN);
    assert_eq!(row.last_started, Timestamp(20 * MIN));
    assert_eq!(result.characters[0].name.as_deref(), Some("SynthAlpha"));
    // Character and range filters.
    let mine = reader
        .stats_dungeons(StatsFilter {
            range: None,
            character_id: Some("synth-character".into()),
        })
        .unwrap();
    assert_eq!(mine.rows[0].runs, 1);
    let late = reader
        .stats_dungeons(StatsFilter {
            range: Some(TimeRange {
                start: Timestamp(15 * MIN),
                end: Timestamp(60 * MIN),
            }),
            character_id: None,
        })
        .unwrap();
    assert_eq!(late.rows[0].runs, 1);
}

#[test]
fn dungeons_are_bounded_and_report_truncation() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    let mut events = vec![session("s1", 0, 0, None)];
    for i in 0..201u64 {
        let mut r = run(&format!("r{i}"), "s1", &format!("Synthetic {i:03}"), i);
        r.ended = Some(Timestamp(i + 1));
        events.push(EngineEvent::RunEnded(r));
    }
    s.apply(&mut db, events);
    let result = maxed_store::open_reader(&path)
        .unwrap()
        .stats_dungeons(all())
        .unwrap();
    assert_eq!(result.rows.len(), 200);
    assert!(result.truncated);
}

#[test]
fn invalid_filters_are_rejected() {
    let (_dir, path, _db) = database();
    let reader = maxed_store::open_reader(&path).unwrap();
    for filter in [
        StatsFilter {
            range: Some(TimeRange {
                start: Timestamp(2),
                end: Timestamp(1),
            }),
            character_id: None,
        },
        StatsFilter {
            range: None,
            character_id: Some(String::new()),
        },
        StatsFilter {
            range: None,
            character_id: Some("é".repeat(129)),
        },
    ] {
        assert!(reader.stats_dungeons(filter).is_err());
    }
}
