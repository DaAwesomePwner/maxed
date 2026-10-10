#[path = "../../maxed-engine/tests/common/mod.rs"]
mod engine_common;
use engine_common::*;
use maxed_store::{apply_batch, migrate, open_reader};
use maxed_types::*;
use rusqlite::Connection;

#[test]
fn loot_retention_preserves_taken_and_reappearing_items() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retention");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let mut e = engine();
    let mut sequence = 0;
    let mut save = |events: Vec<EngineEvent>| {
        let events: Vec<_> = events
            .into_iter()
            .map(|event| {
                sequence += 1;
                EventEnvelope {
                    fight_chart_omitted: false,
                    stream_id: "loot-test".into(),
                    sequence: Sequence(Timestamp(sequence)),
                    at: Timestamp(sequence),
                    event,
                }
            })
            .collect();
        apply_batch(&mut db, &events).unwrap();
        apply_batch(&mut db, &events).unwrap(); // Exact delivery retry must not count twice.
    };
    save(apply(&mut e, 0, map("The Void")));
    save(apply(&mut e, 1000, create(1, 1)));
    save(apply(
        &mut e,
        2000,
        update(vec![player(), target()], vec![]),
    ));
    save(apply(
        &mut e,
        3000,
        update(
            vec![object(4, 1292, vec![stat(8, 200), stat(9, 200)])],
            vec![],
        ),
    ));
    save(apply(
        &mut e,
        4000,
        tick(vec![status(4, vec![stat(8, -1)])]),
    ));
    save(apply(
        &mut e,
        5000,
        tick(vec![status(4, vec![stat(8, 201)])]),
    ));
    save(apply(
        &mut e,
        6000,
        tick(vec![status(4, vec![stat(8, 200)])]),
    ));
    for item in 202u16..212 {
        save(apply(
            &mut e,
            7000 + u64::from(item),
            tick(vec![status(4, vec![stat(8, item.into())])]),
        ));
    }
    let snapshot = e.snapshot();
    let expected = snapshot.drops[0].clone();
    assert_eq!(expected.contents.len(), 13);
    assert_eq!(
        expected.contents[0].taken_observed_at,
        Some(Timestamp(4000))
    );
    assert_eq!(expected.contents[1].taken_observed_at, None);
    assert_eq!(
        expected.contents[2].taken_observed_at,
        Some(Timestamp(6000))
    );
    let run = snapshot.run.unwrap().id;
    let scope = HomeScope::Session {
        id: snapshot.session.unwrap().id,
    };
    drop(db);
    let reader = open_reader(&path).unwrap();
    assert_eq!(
        reader.get_run(&run).unwrap().unwrap().drops,
        vec![expected.clone()]
    );
    let home = reader.home_summary(scope).unwrap();
    assert_eq!(home.best_drop, Some(expected.clone()));
    assert_eq!(home.recent_details[0].loot, vec![expected.clone()]);
    drop(reader);
    let mut db = Connection::open(&path).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM drop_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 13);
    // Even a later incomplete projection cannot erase stored history or taken evidence.
    let mut stale = expected.clone();
    stale.contents.truncate(1);
    stale.contents[0].taken_observed_at = None;
    sequence += 1;
    apply_batch(
        &mut db,
        &[EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "loot-test".into(),
            sequence: Sequence(Timestamp(sequence)),
            at: Timestamp(9000),
            event: EngineEvent::DropUpdated(stale),
        }],
    )
    .unwrap();
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run(&run)
            .unwrap()
            .unwrap()
            .drops[0]
            .contents,
        expected.contents
    );
}
