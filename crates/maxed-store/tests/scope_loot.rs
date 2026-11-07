use crate::common;
use crate::common::account_scope_fixture::ACCOUNT;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loot.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    crate::common::account_scope_fixture::apply_batch(&mut db, &story_events()).unwrap();
    db.execute("DELETE FROM drop_items", []).unwrap();
    db.execute("DELETE FROM drops", []).unwrap();
    (dir, path, db)
}
fn session() -> HomeScope {
    HomeScope::Session {
        id: "demo-session".into(),
    }
}
fn today(start: u64, end: u64) -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        },
    }
}
fn put(db: &mut Connection, id: &str, at: u64, items: &[(u16, u8)]) {
    let mut drop = demo::demo_snapshot().drops.remove(0);
    drop.id = id.into();
    drop.run_id = None;
    drop.at = Timestamp(at);
    drop.contents_known = true;
    drop.contents = items
        .iter()
        .enumerate()
        .map(|(slot, (item_type, applied))| DropItem {
            slot: slot as u8,
            item_type: *item_type,
            quantity: 99,
            taken_observed_at: Some(Timestamp(at + 1)),
            enchant: EnchantView {
                state: EnchantState::Recorded,
                applied: Some(*applied),
                slots: Some(*applied),
                ids: (0..i32::from(*applied)).collect(),
            },
        })
        .collect();
    let head: i64 = common::receipts::head(db, "test-stream").unwrap();
    crate::common::account_scope_fixture::apply_batch(
        db,
        &[envelope(
            u64::try_from(head).unwrap() + 1,
            EngineEvent::BagDropped(drop),
        )],
    )
    .unwrap();
}
#[test]
fn scope_loot_counts_appearances_including_runless_bags_and_excludes_end_boundary() {
    let (_dir, path, mut db) = fixture();
    put(&mut db, "a", 100, &[(1, 1), (1, 4), (2, 2)]);
    put(&mut db, "a", 100, &[(1, 1), (1, 4), (2, 2)]); // revision, not another appearance
    put(&mut db, "empty", 101, &[]);
    put(&mut db, "outside", 200, &[(1, 3)]);
    let tiers = vec![(1, Some("UT".into())), (2, Some("ST".into()))];
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let page = reader
        .scope_loot(today(100, 200), Some(ACCOUNT), None, 20, &tiers)
        .unwrap();
    assert_eq!(page.total_bags, 2);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].item_type, 1);
    assert_eq!(page.items[0].count, 2);
    assert_eq!(page.items[0].tier.as_deref(), Some("UT"));
    assert_eq!(
        page.items[0].best_enchant.as_ref().unwrap().applied,
        Some(4)
    );
    assert_eq!(page.items[0].last_at, Timestamp(100));
    assert_eq!(
        reader
            .scope_loot(session(), Some(ACCOUNT), None, 20, &tiers)
            .unwrap()
            .total_bags,
        3
    );
    assert_eq!(
        reader
            .scope_loot(
                HomeScope::Session {
                    id: "absent".into()
                },
                Some(ACCOUNT),
                None,
                20,
                &tiers
            )
            .unwrap()
            .total_bags,
        0
    );
}
#[test]
fn scope_loot_pages_large_today_without_duplicates_and_rejects_bad_cursors() {
    let (_dir, path, mut db) = fixture();
    for i in 0..257u16 {
        put(&mut db, &format!("bag-{i}"), 100, &[(i, 1)]);
    }
    let tiers = vec![(256, Some("UT".into())), (255, Some("ST".into()))];
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut cursor = None;
    let mut seen = Vec::new();
    loop {
        let page = reader
            .scope_loot(today(0, 1000), Some(ACCOUNT), cursor, 32, &tiers)
            .unwrap();
        assert!(page.items.len() <= 32);
        assert_eq!(page.total_bags, 257);
        seen.extend(page.items.iter().map(|item| item.item_type));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(&seen[..2], &[256, 255]);
    seen.sort_unstable();
    assert_eq!(seen, (0..257).collect::<Vec<u16>>());
    for limit in [0, 101, u16::MAX] {
        assert!(reader
            .scope_loot(session(), Some(ACCOUNT), None, limit, &tiers)
            .is_err());
    }
    for cursor in ["garbage".to_string(), "x".repeat(2049)] {
        assert!(reader
            .scope_loot(session(), Some(ACCOUNT), Some(cursor), 20, &tiers)
            .is_err());
    }
    let next = reader
        .scope_loot(today(0, 1000), Some(ACCOUNT), None, 1, &tiers)
        .unwrap()
        .next
        .unwrap();
    assert!(reader
        .scope_loot(session(), Some(ACCOUNT), Some(next), 20, &tiers)
        .is_err());
    assert!(reader
        .scope_loot(today(200, 100), Some(ACCOUNT), None, 20, &tiers)
        .is_err());
}
#[test]
fn scope_loot_missing_metadata_and_invalid_enchants_are_null_not_common() {
    let (_dir, path, mut db) = fixture();
    put(&mut db, "missing", 100, &[(1, 1)]);
    db.execute(
        "UPDATE drop_items SET json=json_set(json,'$.enchant.state','missing')",
        [],
    )
    .unwrap();
    let page = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .scope_loot(session(), Some(ACCOUNT), None, 100, &[])
        .unwrap();
    assert_eq!(page.items[0].tier, None);
    assert_eq!(page.items[0].best_enchant, None);
    for applied in ["null", "-1", "5", "1.5", "\"1\""] {
        db.execute(
            "UPDATE drop_items SET json=json_set(json,'$.enchant.state','recorded','$.enchant.applied',json(?1))",
            [applied],
        ).unwrap();
        let page = crate::common::account_scope_fixture::open_reader(&path)
            .unwrap()
            .scope_loot(session(), Some(ACCOUNT), None, 100, &[])
            .unwrap();
        assert_eq!(page.items[0].best_enchant, None);
    }
    db.execute("UPDATE drop_items SET json=json_set(json,'$.enchant.state','recorded_empty','$.enchant.applied',0,'$.enchant.ids',json('[]'))", []).unwrap();
    assert_eq!(
        crate::common::account_scope_fixture::open_reader(&path)
            .unwrap()
            .scope_loot(session(), Some(ACCOUNT), None, 100, &[])
            .unwrap()
            .items[0]
            .best_enchant
            .as_ref()
            .unwrap()
            .applied,
        Some(0)
    );
}

#[test]
fn scope_loot_deduplicates_catalog_metadata_and_bounds_query_inputs() {
    let (_dir, path, mut db) = fixture();
    put(&mut db, "bag", 100, &[(1, 1)]);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let tiers = vec![(1, Some("UT".into())), (1, Some("UT".into()))];
    let page = reader
        .scope_loot(session(), Some(ACCOUNT), None, 1, &tiers)
        .unwrap();
    assert_eq!(page.items[0].count, 1);
    assert_eq!(page.next, None);
    for scope in [
        HomeScope::Session { id: String::new() },
        HomeScope::Session {
            id: "x".repeat(257),
        },
        HomeScope::Session {
            id: "bad\0id".into(),
        },
        today(100, 100),
        today(0, u64::MAX),
    ] {
        assert!(matches!(
            reader.scope_loot(scope, Some(ACCOUNT), None, 20, &[]),
            Err(StoreError::InvalidEvent)
        ));
    }
    for priority in [0, 4] {
        let cursor =
            serde_json::json!({ "scope": session(), "priority": priority, "item_type": 1 })
                .to_string();
        assert!(matches!(
            reader.scope_loot(session(), Some(ACCOUNT), Some(cursor), 20, &[]),
            Err(StoreError::InvalidEvent)
        ));
    }
    for tiers in [vec![(1, Some("x".repeat(17)))], vec![(1, None); 65_537]] {
        assert!(matches!(
            reader.scope_loot(session(), Some(ACCOUNT), None, 20, &tiers),
            Err(StoreError::Limit)
        ));
    }
}
#[test]
fn scope_loot_bounds_json_before_returning_or_deserializing_it() {
    let (_dir, path, mut db) = fixture();
    put(&mut db, "large", 100, &[(1, 1)]);
    let ids = serde_json::to_string(&vec![1; 600_000]).unwrap();
    db.execute(
        "UPDATE drop_items SET json=json_set(json,'$.enchant.ids',json(?1))",
        [ids],
    )
    .unwrap();
    assert!(matches!(
        crate::common::account_scope_fixture::open_reader(&path)
            .unwrap()
            .scope_loot(session(), Some(ACCOUNT), None, 100, &[]),
        Err(StoreError::Limit)
    ));
}
