use crate::common;
use common::loot::*;
use maxed_store::StoreError;
use maxed_types::*;

fn filter(limit: u16) -> DropFilter {
    DropFilter {
        min_rank: None,
        place: None,
        item_type: None,
        enchanted_only: false,
        range: None,
        cursor: None,
        limit,
    }
}

fn ids(page: &DropPage) -> Vec<&str> {
    page.rows.iter().map(|row| row.drop.id.as_str()).collect()
}

#[test]
fn feed_is_newest_first_with_dungeon_and_outside_rows() {
    let mut t = LootDb::new();
    t.run("r1", "The Shatters", 100, Some(900), Some(Outcome::Cleared));
    t.bag(
        "b1",
        Some("r1"),
        200,
        Some(255),
        Some((1, false)),
        &[(0, 3180, None, no_enchant())],
    );
    t.bag("b2", None, 300, Some(1), None, &[]);
    let page = t.reader().list_drops(filter(50)).unwrap();
    assert_eq!(&ids(&page)[..2], ["b2", "b1"]);
    assert_eq!(page.rows[1].dungeon.as_deref(), Some("The Shatters"));
    assert_eq!(page.rows[0].dungeon, None);
}

#[test]
fn feed_keyset_equal_timestamps_no_duplicates() {
    let mut t = LootDb::new();
    for i in 0..7 {
        t.bag(&format!("e{i}"), None, 500, Some(1), None, &[]);
    }
    let reader = t.reader();
    let mut seen = vec![];
    let mut f = filter(3);
    loop {
        let page = reader.list_drops(f.clone()).unwrap();
        seen.extend(
            page.rows
                .iter()
                .map(|r| r.drop.id.clone())
                .filter(|id| id.starts_with('e')),
        );
        match page.next {
            Some(c) => f.cursor = Some(c),
            None => break,
        }
    }
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(seen.len(), 7);
    assert_eq!(sorted.len(), 7);
    assert_eq!(seen, ["e6", "e5", "e4", "e3", "e2", "e1", "e0"]);
}

#[test]
fn feed_drop_updated_keeps_position() {
    let mut t = LootDb::new();
    t.bag("older", None, 300, Some(1), None, &[]);
    t.bag(
        "updated",
        None,
        400,
        Some(1),
        None,
        &[(0, 10, None, no_enchant())],
    );
    t.bag("newer", None, 500, Some(1), None, &[]);
    let reader = t.reader();
    let first = reader.list_drops(filter(1)).unwrap();
    assert_eq!(ids(&first), ["newer"]);
    t.bag(
        "updated",
        None,
        600,
        Some(1),
        None,
        &[(1, 20, None, enchant(2, &[5, 9]))],
    );
    let mut f = filter(1);
    f.cursor = first.next;
    let second = reader.list_drops(f.clone()).unwrap();
    assert_eq!(ids(&second), ["updated"]);
    assert_eq!(second.rows[0].drop.at, Timestamp(400));
    assert_eq!(second.rows[0].drop.contents.len(), 2);
    f.cursor = second.next;
    let third = reader.list_drops(f).unwrap();
    assert_eq!(ids(&third), ["older"]);
    assert!(third.next.is_none());
    let all = reader.list_drops(filter(50)).unwrap();
    assert_eq!(
        all.rows
            .iter()
            .filter(|row| row.drop.id == "updated")
            .count(),
        1
    );
    assert_eq!(ids(&all), ["newer", "updated", "older"]);
}

#[test]
fn feed_filters_rank_place_item_enchant_range() {
    let mut t = LootDb::new();
    t.run("r1", "The Shatters", 100, Some(900), Some(Outcome::Cleared));
    t.bag(
        "white",
        Some("r1"),
        300,
        Some(255),
        Some((1, false)),
        &[(0, 3180, None, enchant(2, &[5, 9]))],
    );
    t.bag(
        "purple",
        None,
        200,
        Some(1),
        None,
        &[(0, 10, None, no_enchant())],
    );
    let reader = t.reader();
    let mut f = filter(50);
    f.min_rank = Some(255);
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["white"]);
    let mut f = filter(50);
    f.place = Some(LootPlace::Outside);
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["purple"]);
    let mut f = filter(50);
    f.place = Some(LootPlace::Dungeon {
        name: "The Shatters".into(),
    });
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["white"]);
    let mut f = filter(50);
    f.item_type = Some(10);
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["purple"]);
    let mut f = filter(50);
    f.enchanted_only = true;
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["white"]);
    let mut f = filter(50);
    f.range = Some(TimeRange {
        start: Timestamp(0),
        end: Timestamp(300),
    });
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["purple"]);
    let mut f = filter(50);
    f.min_rank = Some(255);
    f.place = Some(LootPlace::Dungeon {
        name: "The Shatters".into(),
    });
    f.item_type = Some(3180);
    f.enchanted_only = true;
    f.range = Some(TimeRange {
        start: Timestamp(300),
        end: Timestamp(301),
    });
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["white"]);
}

#[test]
fn feed_rejects_bad_limits_and_cursors() {
    let t = LootDb::new();
    let reader = t.reader();
    for limit in [0, 51, u16::MAX] {
        assert!(matches!(
            reader.list_drops(filter(limit)),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.item_history(ItemHistoryRequest {
                item_type: 10,
                cursor: None,
                limit
            }),
            Err(StoreError::InvalidEvent)
        ));
    }
    for id in [
        String::new(),
        "x".repeat(257),
        "é".repeat(129),
        "bad\nid".into(),
    ] {
        let cursor = DropCursor {
            at: Timestamp(1),
            id,
        };
        let mut f = filter(10);
        f.cursor = Some(cursor.clone());
        assert!(matches!(
            reader.list_drops(f),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.item_history(ItemHistoryRequest {
                item_type: 10,
                cursor: Some(cursor),
                limit: 10
            }),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn feed_rejects_bad_ranges_and_dungeons_and_binds_names() {
    let mut t = LootDb::new();
    let name = "Dungeon's ?1 ' OR 1=1 --";
    t.run("r1", name, 100, None, None);
    t.bag("matching", Some("r1"), 200, None, None, &[]);
    t.bag("outside", None, 200, None, None, &[]);
    let reader = t.reader();
    for (start, end) in [(1, 1), (2, 1), (0, u64::MAX)] {
        let mut f = filter(10);
        f.range = Some(TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        });
        assert!(matches!(
            reader.list_drops(f),
            Err(StoreError::InvalidEvent)
        ));
    }
    for name in [String::new(), "x".repeat(257), "é".repeat(129)] {
        let mut f = filter(10);
        f.place = Some(LootPlace::Dungeon { name });
        assert!(matches!(
            reader.list_drops(f),
            Err(StoreError::InvalidEvent)
        ));
    }
    let mut f = filter(10);
    f.place = Some(LootPlace::Dungeon { name: name.into() });
    assert_eq!(ids(&reader.list_drops(f).unwrap()), ["matching"]);
}

#[test]
fn item_history_counts_appearances_and_bags_once() {
    let mut t = LootDb::new();
    t.run("r1", "The Shatters", 100, Some(900), Some(Outcome::Cleared));
    t.bag(
        "b1",
        Some("r1"),
        200,
        Some(255),
        Some((1, false)),
        &[
            (0, 3180, Some(250), no_enchant()),
            (1, 3180, None, enchant(2, &[5, 9])),
            (2, 99, None, no_enchant()),
        ],
    );
    t.bag(
        "b2",
        None,
        300,
        Some(255),
        Some((2, true)),
        &[(0, 3180, None, enchant(2, &[5, 9]))],
    );
    let h = t
        .reader()
        .item_history(ItemHistoryRequest {
            item_type: 3180,
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(
        (h.totals.appearances, h.totals.bags, h.totals.taken),
        (3, 2, 1)
    );
    assert_eq!(
        (h.totals.first_at, h.totals.last_at),
        (Some(Timestamp(200)), Some(Timestamp(300)))
    );
    assert_eq!(h.bags.rows.len(), 2);
    let recorded = h
        .enchant_variants
        .iter()
        .find(|v| v.state == EnchantState::Recorded)
        .unwrap();
    assert_eq!((recorded.count, recorded.applied), (2, Some(2)));
    assert_eq!(recorded.ids, [5, 9]);
    assert_eq!(
        h.enchant_variants
            .iter()
            .find(|v| v.state == EnchantState::Missing)
            .unwrap()
            .count,
        1
    );
    assert!(h
        .by_dungeon
        .iter()
        .any(|p| p.dungeon.is_none() && p.count == 1));
    assert!(h
        .by_source
        .iter()
        .any(|s| s.source_enemy == Some(2) && s.estimated));
    assert_eq!(
        h.by_dungeon[0],
        PlaceCount {
            dungeon: Some("The Shatters".into()),
            count: 2,
            last_at: Timestamp(200),
            runs_since: Some(0),
        }
    );
    assert_eq!(
        h.by_source[0],
        SourceCount {
            source_enemy: Some(1),
            estimated: false,
            count: 2
        }
    );
}

#[test]
fn item_history_unknown_type_is_zero_not_error() {
    let t = LootDb::new();
    let h = t
        .reader()
        .item_history(ItemHistoryRequest {
            item_type: 65000,
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!((h.totals.appearances, h.totals.bags), (0, 0));
    assert!(h.totals.first_at.is_none() && h.bags.rows.is_empty() && h.bags.next.is_none());
    assert_eq!(h.totals.taken, 0);
    assert!(h.totals.last_at.is_none());
    assert!(h.enchant_variants.is_empty() && h.by_dungeon.is_empty() && h.by_source.is_empty());
}

#[test]
fn item_history_keeps_enchant_and_source_evidence_distinct() {
    let mut t = LootDb::new();
    for (i, state) in [
        EnchantState::Missing,
        EnchantState::Invalid,
        EnchantState::RecordedEmpty,
        EnchantState::Recorded,
    ]
    .into_iter()
    .enumerate()
    {
        for n in 0..2 {
            let mut e = enchant(n, &[i as i32, i32::from(n)]);
            e.state = state.clone();
            let source = match i {
                0 => None,
                1 => Some((7, false)),
                _ => Some((7, true)),
            };
            t.bag(
                &format!("b{i}-{n}"),
                None,
                100,
                None,
                source,
                &[(0, 10, None, e)],
            );
        }
    }
    let h = t
        .reader()
        .item_history(ItemHistoryRequest {
            item_type: 10,
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(h.enchant_variants.len(), 6);
    for state in [EnchantState::Missing, EnchantState::Invalid] {
        let v = h
            .enchant_variants
            .iter()
            .find(|v| v.state == state)
            .unwrap();
        assert_eq!((v.count, v.applied, v.ids.as_slice()), (2, None, &[][..]));
    }
    assert_eq!(h.by_source.len(), 3);
    assert!(h.by_source.contains(&SourceCount {
        source_enemy: None,
        estimated: false,
        count: 2
    }));
    assert!(h.by_source.contains(&SourceCount {
        source_enemy: Some(7),
        estimated: false,
        count: 2
    }));
    assert!(h.by_source.contains(&SourceCount {
        source_enemy: Some(7),
        estimated: true,
        count: 4
    }));
    let mut f = filter(50);
    f.enchanted_only = true;
    assert_eq!(ids(&t.reader().list_drops(f).unwrap()), ["b3-1"]);
}

#[test]
fn item_history_bounds_groups_and_pages_bags_without_paging_totals() {
    let mut t = LootDb::new();
    for i in 0..66u16 {
        let run = format!("r{i}");
        t.run(&run, &format!("Dungeon {i}"), 1, None, None);
        t.bag(
            &format!("b{i:02}"),
            Some(&run),
            100,
            None,
            Some((i, false)),
            &[(0, 10, None, enchant(1, &[i32::from(i)]))],
        );
    }
    let reader = t.reader();
    let mut cursor = None;
    let mut seen = vec![];
    loop {
        let h = reader
            .item_history(ItemHistoryRequest {
                item_type: 10,
                cursor,
                limit: 50,
            })
            .unwrap();
        assert_eq!((h.totals.appearances, h.totals.bags), (66, 66));
        assert_eq!(
            (
                h.enchant_variants.len(),
                h.by_dungeon.len(),
                h.by_source.len()
            ),
            (32, 64, 64)
        );
        assert!(h.bags.rows.len() <= 50);
        seen.extend(h.bags.rows.into_iter().map(|r| r.drop.id));
        cursor = h.bags.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), 66);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 66);
}

#[test]
fn feed_preserves_unknown_empty_and_partial_contents() {
    let mut t = LootDb::new();
    t.bag("unknown", None, 100, None, None, &[]);
    t.bag("empty", None, 200, None, None, &[]);
    t.bag(
        "partial",
        None,
        300,
        None,
        Some((7, true)),
        &[(0, 10, Some(400), no_enchant())],
    );
    let mut rows = t.reader().list_drops(filter(50)).unwrap().rows;
    for row in &mut rows {
        if row.drop.id != "empty" {
            row.drop.contents_known = false;
            t.put_drop(row.drop.clone());
        }
    }
    assert_eq!(t.reader().list_drops(filter(50)).unwrap().rows, rows);
}

#[test]
fn loot_replies_reject_oversize_data() {
    let mut t = LootDb::new();
    t.bag(
        "large",
        None,
        100,
        None,
        None,
        &[(0, 10, None, no_enchant())],
    );
    let mut drop = t
        .reader()
        .list_drops(filter(50))
        .unwrap()
        .rows
        .remove(0)
        .drop;
    drop.source_reason = Some("x".repeat(1024 * 1024));
    t.put_drop(drop);
    let reader = t.reader();
    assert!(matches!(
        reader.list_drops(filter(50)),
        Err(StoreError::Limit)
    ));
    assert!(matches!(
        reader.item_history(ItemHistoryRequest {
            item_type: 10,
            cursor: None,
            limit: 50
        }),
        Err(StoreError::Limit)
    ));
}

fn all_time() -> LootStatsFilter {
    LootStatsFilter { range: None }
}

#[test]
fn stats_do_not_multiply_bags_by_items() {
    let mut t = LootDb::new();
    t.run("r1", "The Shatters", 100, Some(900), Some(Outcome::Cleared));
    t.bag(
        "b1",
        Some("r1"),
        200,
        Some(255),
        Some((1, false)),
        &[
            (0, 1, None, no_enchant()),
            (1, 2, None, no_enchant()),
            (2, 3, None, no_enchant()),
        ],
    );
    let s = t.reader().loot_stats(all_time()).unwrap();
    let d = s
        .dungeons
        .iter()
        .find(|d| d.dungeon == "The Shatters")
        .unwrap();
    assert_eq!((d.runs_ended, d.clears, d.whites), (1, 1, 1));
    assert_eq!(d.bags.iter().map(|b| b.count).sum::<u32>(), 1);
}

#[test]
fn stats_exclude_open_runs_from_denominator() {
    let mut t = LootDb::new();
    t.run("r1", "Lost Halls", 100, Some(900), Some(Outcome::Died));
    t.run("r2", "Lost Halls", 1000, None, None);
    t.bag("b2", Some("r2"), 1100, Some(255), None, &[]);
    let s = t.reader().loot_stats(all_time()).unwrap();
    let d = s
        .dungeons
        .iter()
        .find(|d| d.dungeon == "Lost Halls")
        .unwrap();
    assert_eq!((d.runs_ended, d.clears, d.whites), (1, 0, 1));
}

#[test]
fn runs_since_last_white_counts_runs_started_after_it() {
    let mut t = LootDb::new();
    t.run("r1", "Shatters", 100, Some(200), Some(Outcome::Cleared));
    t.bag("w", Some("r1"), 150, Some(255), None, &[]);
    t.run("r2", "Shatters", 300, Some(400), Some(Outcome::Cleared));
    t.run("r3", "Shatters", 500, Some(600), Some(Outcome::Left));
    t.run("r4", "Nest", 700, Some(800), Some(Outcome::Cleared));
    let s = t.reader().loot_stats(all_time()).unwrap();
    assert_eq!(
        s.dungeons
            .iter()
            .find(|d| d.dungeon == "Shatters")
            .unwrap()
            .runs_since_last_white,
        Some(2)
    );
    assert_eq!(
        s.dungeons
            .iter()
            .find(|d| d.dungeon == "Nest")
            .unwrap()
            .runs_since_last_white,
        None
    );
}

#[test]
fn dry_gap_counts_runs_between_white_runs() {
    let mut t = LootDb::new();
    // Insert out of order; the dry gaps follow run starts, not insertion order.
    for i in [4, 2, 0, 3, 1] {
        let id = format!("r{i}");
        t.run(
            &id,
            "Shatters",
            i * 100,
            Some(i * 100 + 90),
            Some(Outcome::Left),
        );
        if i == 0 || i == 3 {
            t.bag(
                &format!("w{i}"),
                Some(&id),
                i * 100 + 50,
                Some(255),
                None,
                &[],
            );
        }
    }
    t.run("other", "Nest", 600, Some(690), Some(Outcome::Cleared));
    t.bag("other-white", Some("other"), 650, Some(255), None, &[]);
    let s = t.reader().loot_stats(all_time()).unwrap();
    let d = s.dungeons.iter().find(|d| d.dungeon == "Shatters").unwrap();
    assert_eq!(d.white_runs, 2);
    assert_eq!(d.avg_runs_per_white, Some(2.5));
    assert_eq!(d.longest_dry, Some(2));
}

#[test]
fn dry_gap_counts_a_run_with_several_whites_once() {
    let mut t = LootDb::new();
    t.run("r1", "Shatters", 100, Some(200), Some(Outcome::Cleared));
    t.bag("w1", Some("r1"), 150, Some(255), None, &[]);
    t.bag("w2", Some("r1"), 160, Some(255), None, &[]);
    let s = t.reader().loot_stats(all_time()).unwrap();
    let d = &s.dungeons[0];
    assert_eq!(d.whites, 2);
    assert_eq!(d.white_runs, 1);
    assert_eq!(d.avg_runs_per_white, Some(1.0));
    assert_eq!(d.longest_dry, Some(0));
}

#[test]
fn dry_gap_includes_leading_and_current_streaks() {
    let mut t = LootDb::new();
    for i in 0..8 {
        let id = format!("r{i}");
        t.run(
            &id,
            "Shatters",
            i * 100,
            Some(i * 100 + 90),
            Some(Outcome::Left),
        );
        if i == 3 {
            t.bag("w", Some(&id), i * 100 + 50, Some(255), None, &[]);
        }
        if i == 3 || i == 7 {
            let s = t.reader().loot_stats(all_time()).unwrap();
            let d = &s.dungeons[0];
            assert_eq!(d.white_runs, 1);
            assert_eq!(d.avg_runs_per_white, Some((i + 1) as f64));
            assert_eq!(d.longest_dry, Some(if i == 3 { 3 } else { 4 }));
        }
    }
}

#[test]
fn dry_gap_none_without_whites() {
    let mut t = LootDb::new();
    t.run("r1", "Shatters", 100, Some(200), Some(Outcome::Left));
    t.bag("purple", Some("r1"), 150, Some(3), None, &[]);
    t.run("open", "Shatters", 300, None, None);
    t.bag("open-white", Some("open"), 350, Some(255), None, &[]);
    t.bag("outside-white", None, 400, Some(255), None, &[]);
    let s = t.reader().loot_stats(all_time()).unwrap();
    let d = &s.dungeons[0];
    assert_eq!(d.white_runs, 0);
    assert_eq!(d.avg_runs_per_white, None);
    assert_eq!(d.longest_dry, None);
}

#[test]
fn dry_gap_ignores_the_range_filter() {
    let mut t = LootDb::new();
    for i in 0..5 {
        let id = format!("r{i}");
        t.run(
            &id,
            "Shatters",
            i * 100,
            Some(i * 100 + 90),
            Some(Outcome::Left),
        );
        if i == 0 || i == 3 {
            t.bag(
                &format!("w{i}"),
                Some(&id),
                i * 100 + 50,
                Some(255),
                None,
                &[],
            );
        }
    }
    t.run("open", "Shatters", 500, None, None);
    t.bag("open-white", Some("open"), 550, Some(255), None, &[]);
    let s = t
        .reader()
        .loot_stats(LootStatsFilter {
            range: Some(TimeRange {
                start: Timestamp(400),
                end: Timestamp(500),
            }),
        })
        .unwrap();
    let d = &s.dungeons[0];
    assert_eq!((d.runs_ended, d.whites), (1, 0));
    assert_eq!(d.white_runs, 2);
    assert_eq!(d.avg_runs_per_white, Some(2.5));
    assert_eq!(d.longest_dry, Some(2));
}

#[test]
fn runless_sources_have_no_rate() {
    let mut t = LootDb::new();
    t.bag("b", None, 100, Some(255), Some((42, false)), &[]);
    let s = t.reader().loot_stats(all_time()).unwrap();
    let src = s.sources.iter().find(|x| x.source_enemy == 42).unwrap();
    assert_eq!(
        (src.dungeon.as_deref(), src.runs_ended, src.whites),
        (None, None, 1)
    );
}

#[test]
fn stats_range_is_half_open() {
    let mut t = LootDb::new();
    t.run("start", "Boundary", 10, Some(100), Some(Outcome::Cleared));
    t.run("end", "Boundary", 110, Some(200), Some(Outcome::Died));
    t.bag(
        "start-bag",
        Some("start"),
        100,
        Some(255),
        Some((42, false)),
        &[],
    );
    t.bag(
        "end-bag",
        Some("end"),
        200,
        Some(255),
        Some((42, false)),
        &[],
    );
    let s = t
        .reader()
        .loot_stats(LootStatsFilter {
            range: Some(TimeRange {
                start: Timestamp(100),
                end: Timestamp(200),
            }),
        })
        .unwrap();
    assert_eq!(s.dungeons.len(), 1);
    let d = &s.dungeons[0];
    assert_eq!((d.runs_ended, d.clears, d.whites), (1, 1, 1));
    assert_eq!(
        d.bags,
        vec![BagTierCount {
            bag_type: Some(400),
            bag_rank: Some(255),
            count: 1
        }]
    );
    assert_eq!(d.best.as_ref().unwrap().drop_id, "start-bag");
    assert_eq!((s.sources[0].runs_ended, s.sources[0].whites), (Some(1), 1));
    assert_eq!(s.totals, d.bags);
    assert_eq!(s.whites, 1);
}

#[test]
fn stats_narrow_range_seeks_and_counts_only_in_range_bags() {
    let mut t = LootDb::new();
    t.run("r", "Narrow", 0, Some(501), Some(Outcome::Cleared));
    t.run(
        "max",
        "Narrow",
        0,
        Some(i64::MAX as u64),
        Some(Outcome::Cleared),
    );
    for at in [0, 499, 500, 501, 502, 10_000, i64::MAX as u64] {
        t.bag(
            &format!("bag-{at}"),
            Some("r"),
            at,
            Some(255),
            Some((42, false)),
            &[],
        );
    }
    let stats = t
        .reader()
        .loot_stats(LootStatsFilter {
            range: Some(TimeRange {
                start: Timestamp(500),
                end: Timestamp(502),
            }),
        })
        .unwrap();
    let expected = vec![BagTierCount {
        bag_type: Some(400),
        bag_rank: Some(255),
        count: 2,
    }];
    assert_eq!(stats.totals, expected);
    assert_eq!(stats.whites, 2);
    assert_eq!(stats.dungeons.len(), 1);
    assert_eq!(stats.dungeons[0].bags, expected);
    assert_eq!(stats.dungeons[0].runs_ended, 1);
    assert_eq!(stats.dungeons[0].best.as_ref().unwrap().drop_id, "bag-501");
    assert_eq!(stats.sources.len(), 1);
    assert_eq!(stats.sources[0].bags, expected);
    assert_eq!(stats.sources[0].runs_ended, Some(1));
    let all = t.reader().loot_stats(all_time()).unwrap();
    assert_eq!(all.whites, 7);
    assert_eq!(all.dungeons[0].runs_ended, 2);
    let before_max = t
        .reader()
        .loot_stats(LootStatsFilter {
            range: Some(TimeRange {
                start: Timestamp(0),
                end: Timestamp(i64::MAX as u64),
            }),
        })
        .unwrap();
    assert_eq!(before_max.whites, 6);
    assert_eq!(before_max.dungeons[0].runs_ended, 1);

    let sql = format!(
        "EXPLAIN QUERY PLAN {}",
        include_str!("../src/loot_stats.sql")
    );
    // Check both pre-ANALYZE and statistics-informed plans.
    for analyzed in [false, true] {
        if analyzed {
            t.db.execute_batch("ANALYZE").unwrap();
        }
        let mut statement = t.db.prepare(&sql).unwrap();
        let plan: Vec<String> = statement
            .query_map([500, 502], |row| row.get(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let seek = plan.iter().find(|line| {
            line.contains("SEARCH d USING COVERING INDEX drops_stats (at>? AND at<?)")
        });
        assert!(seek.is_some(), "{plan:#?}");
        println!(
            "loot stats range plan (analyzed={analyzed}): {}",
            seek.unwrap()
        );
        assert!(
            plan.iter()
                .any(|line| line.contains("SCAN d USING COVERING INDEX drops_stats")),
            "{plan:#?}"
        );
    }
}

#[test]
fn chase_dry_streaks() {
    let mut t = LootDb::new();
    t.run("r1", "Shatters", 100, Some(200), Some(Outcome::Cleared));
    t.bag(
        "b",
        Some("r1"),
        150,
        Some(255),
        None,
        &[(0, 3180, None, no_enchant())],
    );
    t.run("r2", "Shatters", 300, Some(400), Some(Outcome::Cleared));
    t.run("r3", "Nest", 500, Some(600), Some(Outcome::Cleared));
    let chase = vec![
        ChaseItem {
            item_type: 3180,
            dungeon: Some("Shatters".into()),
            pinned_at: Timestamp(50),
        },
        ChaseItem {
            item_type: 3180,
            dungeon: None,
            pinned_at: Timestamp(50),
        },
        ChaseItem {
            item_type: 7777,
            dungeon: Some("Nest".into()),
            pinned_at: Timestamp(450),
        },
    ];
    let c = t
        .reader()
        .collection(
            CollectionRequest {
                dungeon: None,
                chase,
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        c.chase
            .iter()
            .map(|r| (r.seen, r.dry_runs))
            .collect::<Vec<_>>(),
        vec![(1, 1), (1, 2), (0, 1)]
    );
    assert!(c.grid.is_empty() && c.grid_dungeon.is_none());
}

#[test]
fn collection_grid_is_seen_ut_st_only() {
    let mut t = LootDb::new();
    t.run("r1", "Shatters", 100, Some(200), Some(Outcome::Cleared));
    t.bag(
        "b",
        Some("r1"),
        150,
        Some(255),
        None,
        &[
            (0, 1, None, no_enchant()),
            (1, 2, None, no_enchant()),
            (2, 3, None, no_enchant()),
        ],
    );
    let tiers = vec![
        (1, Some("UT".to_string())),
        (2, Some("ST".to_string())),
        (3, Some("12".to_string())),
    ];
    let c = t
        .reader()
        .collection(
            CollectionRequest {
                dungeon: Some("Shatters".into()),
                chase: vec![],
            },
            &tiers,
        )
        .unwrap();
    assert_eq!(
        c.grid
            .iter()
            .map(|g| (g.item_type, g.tier.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "UT"), (2, "ST")]
    );
}

#[test]
fn collection_rejects_oversize_chase() {
    let t = LootDb::new();
    let chase = (0..101)
        .map(|i| ChaseItem {
            item_type: i,
            dungeon: None,
            pinned_at: Timestamp(0),
        })
        .collect();
    assert!(matches!(
        t.reader().collection(
            CollectionRequest {
                dungeon: None,
                chase
            },
            &[]
        ),
        Err(StoreError::InvalidEvent)
    ));
}

#[test]
fn item_history_orders_enchant_variants_by_count() {
    let mut t = LootDb::new();
    for (variant, count) in [(1, 1), (2, 3), (3, 2)] {
        for i in 0..count {
            t.bag(
                &format!("v{variant}-{i}"),
                None,
                100,
                None,
                None,
                &[(0, 10, None, enchant(1, &[variant]))],
            );
        }
    }
    let h = t
        .reader()
        .item_history(ItemHistoryRequest {
            item_type: 10,
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(
        h.enchant_variants
            .iter()
            .map(|v| (v.ids[0], v.count))
            .collect::<Vec<_>>(),
        [(2, 3), (3, 2), (1, 1)]
    );
}

#[test]
fn put_drop_advances_sequence_before_more_builder_events() {
    let mut t = LootDb::new();
    t.bag("first", None, 100, None, None, &[]);
    let mut drop = t
        .reader()
        .list_drops(filter(50))
        .unwrap()
        .rows
        .remove(0)
        .drop;
    drop.contents_known = false;
    t.put_drop(drop);
    t.bag("second", None, 200, None, None, &[]);
    t.run("later", "Dungeon", 200, Some(300), Some(Outcome::Left));
    assert_eq!(
        ids(&t.reader().list_drops(filter(50)).unwrap()),
        ["second", "first"]
    );
}

#[test]
fn stats_streak_ignores_range_and_excludes_equal_start_and_open_runs() {
    let mut t = LootDb::new();
    t.run(
        "white-run",
        "Dungeon",
        10,
        Some(100),
        Some(Outcome::Cleared),
    );
    t.bag("white", Some("white-run"), 50, Some(255), None, &[]);
    t.run("equal", "Dungeon", 50, Some(200), Some(Outcome::Unknown));
    t.run("after", "Dungeon", 51, Some(300), Some(Outcome::Died));
    t.run("open", "Dungeon", 52, None, None);
    let s = t
        .reader()
        .loot_stats(LootStatsFilter {
            range: Some(TimeRange {
                start: Timestamp(200),
                end: Timestamp(250),
            }),
        })
        .unwrap();
    let d = &s.dungeons[0];
    assert_eq!(
        (d.runs_ended, d.clears, d.whites, d.runs_since_last_white),
        (1, 0, 0, Some(1))
    );
    assert!(d.bags.is_empty() && d.best.is_none());
}

#[test]
fn stats_include_bag_only_dungeons_unknown_ranks_and_best_ties() {
    let mut t = LootDb::new();
    t.run("open", "Open", 1, None, None);
    for (id, at, rank) in [
        ("unknown", 100, None),
        ("ranked", 110, Some(20)),
        ("white-old", 120, Some(255)),
        ("white-new", 130, Some(255)),
    ] {
        t.bag(id, Some("open"), at, rank, Some((42, false)), &[]);
    }
    t.run("unranked", "Unranked", 1, None, None);
    t.bag("only-unknown", Some("unranked"), 150, None, None, &[]);
    t.run("empty-open", "Empty open", 1, None, None);
    t.bag("outside", None, 140, Some(255), Some((42, true)), &[]);
    let s = t.reader().loot_stats(all_time()).unwrap();
    assert_eq!(
        s.dungeons
            .iter()
            .map(|d| d.dungeon.as_str())
            .collect::<Vec<_>>(),
        ["Open", "Unranked"]
    );
    let d = &s.dungeons[0];
    assert_eq!(
        (d.runs_ended, d.whites, d.runs_since_last_white),
        (0, 2, Some(0))
    );
    assert_eq!(
        d.best,
        Some(BestBagRef {
            bag_type: Some(400),
            drop_id: "white-new".into(),
            run_id: Some("open".into()),
            bag_rank: 255,
            at: Timestamp(130)
        })
    );
    assert!(d.bags.contains(&BagTierCount {
        bag_type: Some(400),
        bag_rank: None,
        count: 1
    }));
    assert_eq!(
        d.bags.iter().map(|b| b.bag_rank).collect::<Vec<_>>(),
        [Some(255), Some(20), None]
    );
    assert!(s.dungeons[1].best.is_none());
    assert_eq!(s.sources[0].runs_ended, Some(0));
    assert_eq!(s.sources[1].runs_ended, None);
    assert_eq!(s.totals.iter().map(|b| b.count).sum::<u32>(), 6);
    assert_eq!(s.whites, 3);
}

#[test]
fn stats_caps_rows_without_capping_totals_or_source_denominators() {
    let mut t = LootDb::new();
    for i in 0..258u16 {
        let name = format!("Dungeon {i:03}");
        let run = format!("r{i}");
        t.run(&run, &name, 1, Some(200), Some(Outcome::Left));
        t.bag(
            &format!("b{i}"),
            Some(&run),
            100,
            Some(255),
            Some((i, false)),
            &[],
        );
    }
    t.run(
        "extra",
        "Dungeon 257",
        201,
        Some(300),
        Some(Outcome::Unknown),
    );
    let s = t.reader().loot_stats(all_time()).unwrap();
    assert_eq!(
        (s.dungeons.len(), s.sources.len(), s.whites),
        (128, 256, 258)
    );
    assert_eq!(s.dungeons[0].dungeon, "Dungeon 257");
    assert_eq!(s.dungeons[1].dungeon, "Dungeon 000");
    assert_eq!(s.sources.last().unwrap().source_enemy, 255);
    assert_eq!(s.sources.last().unwrap().runs_ended, Some(1));
    assert_eq!(s.totals[0].count, 258);
}

#[test]
fn chase_counts_appearances_and_strict_started_boundary_in_input_order() {
    let mut t = LootDb::new();
    let name = "Dungeon's ?1 ' OR 1=1 --";
    t.run("r1", name, 1, Some(100), Some(Outcome::Cleared));
    t.bag(
        "seen",
        Some("r1"),
        50,
        None,
        None,
        &[(0, 10, None, no_enchant()), (1, 10, None, no_enchant())],
    );
    t.bag(
        "outside",
        None,
        55,
        None,
        None,
        &[(0, 10, None, no_enchant())],
    );
    t.run("equal", name, 50, Some(200), Some(Outcome::Died));
    t.run("after", name, 51, Some(201), Some(Outcome::Unknown));
    t.run("open", name, 52, None, None);
    let chase = vec![
        ChaseItem {
            item_type: 10,
            dungeon: Some(name.into()),
            pinned_at: Timestamp(999),
        },
        ChaseItem {
            item_type: 99,
            dungeon: Some(name.into()),
            pinned_at: Timestamp(50),
        },
        ChaseItem {
            item_type: 10,
            dungeon: None,
            pinned_at: Timestamp(0),
        },
    ];
    let c = t
        .reader()
        .collection(
            CollectionRequest {
                dungeon: None,
                chase: chase.clone(),
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        c.chase.iter().map(|r| r.item.clone()).collect::<Vec<_>>(),
        chase
    );
    assert_eq!(
        c.chase
            .iter()
            .map(|r| (r.seen, r.last_at, r.dry_runs))
            .collect::<Vec<_>>(),
        [
            (2, Some(Timestamp(50)), 1),
            (0, None, 1),
            (3, Some(Timestamp(55)), 0)
        ]
    );
}

#[test]
fn collection_grid_caps_and_orders_without_duplicate_catalog_counts() {
    let mut t = LootDb::new();
    t.run("r", "Dungeon", 1, None, None);
    let mut tiers = vec![];
    for i in 0..260u16 {
        t.bag(
            &format!("b{i}"),
            Some("r"),
            100,
            Some(255),
            None,
            &[(0, i, None, no_enchant())],
        );
        tiers.push((i, Some(if i == 0 { "ST" } else { "UT" }.into())));
    }
    tiers.push((259, Some("UT".into())));
    t.bag(
        "repeat",
        Some("r"),
        200,
        None,
        None,
        &[(0, 259, None, no_enchant()), (1, 259, None, no_enchant())],
    );
    t.bag(
        "outside",
        None,
        300,
        None,
        None,
        &[(0, 259, None, no_enchant())],
    );
    let c = t
        .reader()
        .collection(
            CollectionRequest {
                dungeon: Some("Dungeon".into()),
                chase: vec![],
            },
            &tiers,
        )
        .unwrap();
    assert_eq!(c.grid_dungeon.as_deref(), Some("Dungeon"));
    assert_eq!(c.grid.len(), 256);
    assert_eq!(
        c.grid[0],
        CollectionCell {
            item_type: 259,
            tier: "UT".into(),
            count: 3,
            last_at: Timestamp(200),
        }
    );
    assert_eq!(c.grid[1].item_type, 1);
    assert_eq!(c.grid[255].item_type, 255);
}

#[test]
fn stats_and_collection_validate_inputs_and_keep_storage_errors() {
    let t = LootDb::new();
    let reader = t.reader();
    for (start, end) in [(1, 1), (2, 1), (0, u64::MAX)] {
        assert!(matches!(
            reader.loot_stats(LootStatsFilter {
                range: Some(TimeRange {
                    start: Timestamp(start),
                    end: Timestamp(end),
                })
            }),
            Err(StoreError::InvalidEvent)
        ));
    }
    for name in [String::new(), "x".repeat(257), "é".repeat(129)] {
        for grid in [true, false] {
            let request = if grid {
                CollectionRequest {
                    dungeon: Some(name.clone()),
                    chase: vec![],
                }
            } else {
                CollectionRequest {
                    dungeon: None,
                    chase: vec![ChaseItem {
                        item_type: 1,
                        dungeon: Some(name.clone()),
                        pinned_at: Timestamp(0),
                    }],
                }
            };
            assert!(matches!(
                reader.collection(request, &[]),
                Err(StoreError::InvalidEvent)
            ));
        }
    }
    let request = CollectionRequest {
        dungeon: None,
        chase: vec![ChaseItem {
            item_type: 1,
            dungeon: None,
            pinned_at: Timestamp(u64::MAX),
        }],
    };
    assert!(matches!(
        reader.collection(request, &[]),
        Err(StoreError::InvalidEvent)
    ));
    let request = CollectionRequest {
        dungeon: Some("é".repeat(128)),
        chase: (0..100)
            .map(|i| ChaseItem {
                item_type: i,
                dungeon: None,
                pinned_at: Timestamp(0),
            })
            .collect(),
    };
    assert_eq!(
        reader.collection(request.clone(), &[]).unwrap().chase.len(),
        100
    );
    for tiers in [vec![(1, Some("x".repeat(17)))], vec![(1, None); 65_537]] {
        assert!(matches!(
            reader.collection(request.clone(), &tiers),
            Err(StoreError::Limit)
        ));
    }
    assert!(reader.loot_stats(all_time()).unwrap().dungeons.is_empty());
    t.db.execute("ALTER TABLE drops RENAME TO unavailable_drops", [])
        .unwrap();
    assert!(matches!(reader.loot_stats(all_time()), Err(StoreError::Io)));
    assert!(matches!(
        reader.collection(request, &[]),
        Err(StoreError::Io)
    ));
}

#[test]
fn stats_rejects_oversize_payload() {
    let mut t = LootDb::new();
    t.run("r", "Dungeon", 1, Some(2), Some(Outcome::Left));
    t.db.execute(
        "UPDATE runs SET dungeon=?1 WHERE id='r'",
        ["x".repeat(1024 * 1024)],
    )
    .unwrap();
    assert!(matches!(
        t.reader().loot_stats(all_time()),
        Err(StoreError::Limit)
    ));
}

#[test]
fn stats_best_bag_uses_rank_then_time_then_id_across_sources() {
    let mut t = LootDb::new();
    t.run("r", "Dungeon", 0, None, None);
    for (id, at, rank, source) in [
        ("white:9", 9, 255, 1),
        ("white:10", 10, 255, 1),
        ("white:tie-z", 10, 255, 2),
        ("newer-low-rank", i64::MAX as u64, 20, 2),
    ] {
        t.bag(id, Some("r"), at, Some(rank), Some((source, false)), &[]);
    }
    let reader = t.reader();
    assert_eq!(
        reader.loot_stats(all_time()).unwrap().dungeons[0]
            .best
            .as_ref()
            .unwrap()
            .drop_id,
        "white:tie-z"
    );
    t.bag(
        "maximum-time",
        Some("r"),
        i64::MAX as u64,
        Some(255),
        None,
        &[],
    );
    let best = reader
        .loot_stats(all_time())
        .unwrap()
        .dungeons
        .remove(0)
        .best
        .unwrap();
    assert_eq!(
        (best.drop_id.as_str(), best.at),
        ("maximum-time", Timestamp(i64::MAX as u64))
    );
}

#[test]
fn summary_totals_filters_days_and_bounds() {
    let mut t = LootDb::new();
    t.run("a", "Synthetic A", 10, Some(250), Some(Outcome::Cleared));
    t.run("b", "Synthetic B", 20, Some(450), Some(Outcome::Cleared));
    t.run("open", "Synthetic A", 30, None, None);
    t.bag(
        "a1",
        Some("a"),
        100,
        Some(255),
        None,
        &[
            (0, 10, None, enchant(2, &[1, 2])),
            (1, 20, None, no_enchant()),
        ],
    );
    t.bag(
        "a2",
        Some("a"),
        200,
        Some(10),
        None,
        &[(0, 10, None, no_enchant())],
    );
    t.bag(
        "b1",
        Some("b"),
        300,
        Some(255),
        None,
        &[(0, 20, None, enchant(3, &[1, 2, 3]))],
    );
    t.bag("outside", None, 400, None, None, &[]);
    let reader = t.reader();
    let tiers = [(10, Some("UT".into())), (20, Some("ST".into()))];
    let request = DropSummaryRequest {
        include_items: true,
        filter: filter(0),
        days: vec![],
    };
    let summary = reader.drop_summary(request.clone(), &tiers).unwrap();
    assert_eq!(
        summary.totals,
        DropTotals {
            bags: 4,
            runs: 2,
            runs_ended: Some(2),
            whites: 2,
            ut_items: Some(2),
            st_items: Some(2),
            enchanted_items: Some(2),
            items_counted: true,
            best_enchants: Some(3),
            first_at: Some(Timestamp(100)),
            last_white: Some(LastWhite {
                drop_id: "b1".into(),
                run_id: Some("b".into()),
                dungeon: Some("Synthetic B".into()),
                at: Timestamp(300)
            }),
        }
    );
    for index in 0..3 {
        let mut f = filter(0);
        match index {
            0 => f.item_type = Some(10),
            1 => f.min_rank = Some(255),
            _ => f.enchanted_only = true,
        }
        let totals = reader
            .drop_summary(
                DropSummaryRequest {
                    include_items: true,
                    filter: f,
                    days: vec![],
                },
                &tiers,
            )
            .unwrap()
            .totals;
        assert_eq!((totals.bags, totals.runs_ended), (2, Some(2)));
    }
    let mut f = filter(0);
    f.place = Some(LootPlace::Outside);
    let totals = reader
        .drop_summary(
            DropSummaryRequest {
                include_items: true,
                filter: f,
                days: vec![],
            },
            &tiers,
        )
        .unwrap()
        .totals;
    assert_eq!((totals.bags, totals.runs_ended), (1, None));
    let mut f = filter(0);
    f.place = Some(LootPlace::Dungeon {
        name: "Synthetic A".into(),
    });
    f.range = Some(TimeRange {
        start: Timestamp(150),
        end: Timestamp(350),
    });
    let summary = reader
        .drop_summary(
            DropSummaryRequest {
                include_items: true,
                filter: f,
                days: vec![
                    TimeRange {
                        start: Timestamp(250),
                        end: Timestamp(400),
                    },
                    TimeRange {
                        start: Timestamp(0),
                        end: Timestamp(250),
                    },
                    TimeRange {
                        start: Timestamp(400),
                        end: Timestamp(500),
                    },
                ],
            },
            &tiers,
        )
        .unwrap();
    assert_eq!(
        (summary.totals.bags, summary.totals.runs_ended),
        (1, Some(1))
    );
    assert_eq!(
        summary
            .days
            .iter()
            .map(|d| (d.bags, d.runs_ended))
            .collect::<Vec<_>>(),
        [(0, Some(1)), (1, Some(0)), (0, Some(0))]
    );
    let mut invalid = request.clone();
    invalid.days = vec![
        TimeRange {
            start: Timestamp(0),
            end: Timestamp(1)
        };
        9
    ];
    assert!(matches!(
        reader.drop_summary(invalid, &[]),
        Err(StoreError::InvalidEvent)
    ));
    assert!(matches!(
        reader.drop_summary(request.clone(), &[(1, Some("x".repeat(17)))]),
        Err(StoreError::Limit)
    ));
    assert!(matches!(
        reader.drop_summary(request, &vec![(1, None); 65_537]),
        Err(StoreError::Limit)
    ));
    let legacy: DropFilter = serde_json::from_str(r#"{"min_rank":null,"place":null,"item_type":null,"enchanted_only":false,"range":null,"cursor":null,"limit":50}"#).unwrap();
    assert_eq!(legacy, filter(50));
}

#[test]
fn summary_empty_unknown_tiers_and_ignored_pagination() {
    let mut t = LootDb::new();
    let mut f = filter(0);
    f.cursor = Some(DropCursor {
        at: Timestamp(u64::MAX),
        id: String::new(),
    });
    let request = DropSummaryRequest {
        include_items: true,
        filter: f,
        days: vec![],
    };
    let empty = t.reader().drop_summary(request.clone(), &[]).unwrap();
    assert_eq!(
        empty.totals,
        DropTotals {
            runs_ended: Some(0),
            ut_items: Some(0),
            st_items: Some(0),
            enchanted_items: Some(0),
            items_counted: true,
            ..Default::default()
        }
    );
    t.bag(
        "a",
        None,
        100,
        Some(255),
        None,
        &[(0, 10, None, no_enchant())],
    );
    t.bag(
        "z",
        None,
        100,
        Some(255),
        None,
        &[(0, 20, None, enchant(2, &[1, 2]))],
    );
    let summary = t
        .reader()
        .drop_summary(
            request.clone(),
            &[(10, Some("UT".into())), (10, Some("ST".into()))],
        )
        .unwrap();
    assert_eq!(
        (
            summary.totals.bags,
            summary.totals.ut_items,
            summary.totals.st_items
        ),
        (2, Some(0), Some(1))
    );
    assert_eq!(
        summary.totals.last_white.unwrap(),
        LastWhite {
            drop_id: "z".into(),
            run_id: None,
            dungeon: None,
            at: Timestamp(100),
        }
    );
    for time in [
        TimeRange {
            start: Timestamp(1),
            end: Timestamp(1),
        },
        TimeRange {
            start: Timestamp(2),
            end: Timestamp(1),
        },
        TimeRange {
            start: Timestamp(0),
            end: Timestamp(u64::MAX),
        },
    ] {
        let mut bad = request.clone();
        bad.days.push(time.clone());
        assert!(matches!(
            t.reader().drop_summary(bad, &[]),
            Err(StoreError::InvalidEvent)
        ));
        let mut bad = request.clone();
        bad.filter.range = Some(time);
        assert!(matches!(
            t.reader().drop_summary(bad, &[]),
            Err(StoreError::InvalidEvent)
        ));
    }
    for name in [String::new(), "x".repeat(257)] {
        let mut bad = request.clone();
        bad.filter.place = Some(LootPlace::Dungeon { name });
        assert!(matches!(
            t.reader().drop_summary(bad, &[]),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn summary_enchants_cover_all_tiers_without_changing_bag_filters() {
    let mut t = LootDb::new();
    t.bag(
        "ut",
        None,
        100,
        Some(255),
        None,
        &[(0, 10, None, enchant(2, &[1, 2]))],
    );
    t.bag(
        "st",
        None,
        200,
        Some(10),
        None,
        &[(0, 20, None, enchant(3, &[1, 2, 3]))],
    );
    t.bag(
        "tiered",
        None,
        300,
        Some(10),
        None,
        &[(0, 30, None, enchant(4, &[1, 2, 3, 4]))],
    );
    t.bag(
        "unknown",
        None,
        400,
        None,
        None,
        &[(0, 40, None, enchant(5, &[1, 2, 3, 4, 5]))],
    );
    let tiers = [
        (10, Some("UT".into())),
        (20, Some("ST".into())),
        (30, Some("T10".into())),
        (40, None),
    ];
    let reader = t.reader();
    let mut f = filter(50);
    f.enchanted_only = true;
    let totals = reader
        .drop_summary(
            DropSummaryRequest {
                include_items: true,
                filter: f.clone(),
                days: vec![],
            },
            &tiers,
        )
        .unwrap()
        .totals;
    assert_eq!(
        (
            totals.bags,
            totals.ut_items,
            totals.st_items,
            totals.enchanted_items,
            totals.best_enchants
        ),
        (4, Some(1), Some(1), Some(4), Some(5))
    );
    f.item_type = Some(30);
    let totals = reader
        .drop_summary(
            DropSummaryRequest {
                include_items: true,
                filter: f,
                days: vec![],
            },
            &tiers,
        )
        .unwrap()
        .totals;
    assert_eq!(
        (totals.bags, totals.enchanted_items, totals.best_enchants),
        (1, Some(1), Some(4))
    );
    let totals = reader
        .drop_summary(
            DropSummaryRequest {
                include_items: true,
                filter: filter(50),
                days: vec![],
            },
            &[],
        )
        .unwrap()
        .totals;
    assert_eq!(
        (totals.bags, totals.enchanted_items, totals.best_enchants),
        (4, Some(4), Some(5))
    );
}

#[test]
fn summary_item_cap_is_per_block_and_can_be_disabled() {
    let mut t = LootDb::new();
    t.bag(
        "cap-0",
        None,
        0,
        Some(255),
        None,
        &[
            (0, 10, None, enchant(2, &[1, 2])),
            (1, 20, None, enchant(3, &[1, 2, 3])),
        ],
    );
    t.db.execute_batch(
        "WITH RECURSIVE n(i) AS (
        VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<10000
    ) INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json,source_type)
        SELECT printf('cap-%d',i),session_id,NULL,i,bag_type,bag_rank,
            json_set(json,'$.id',printf('cap-%d',i),'$.at',CAST(i AS TEXT)),source_type
        FROM n CROSS JOIN drops WHERE id='cap-0';
    INSERT INTO drop_items(drop_id,slot,item_type,quantity,json,taken_observed_at)
        SELECT d.id,i.slot,i.item_type,i.quantity,i.json,i.taken_observed_at
        FROM drops d CROSS JOIN drop_items i WHERE d.id<>'cap-0' AND i.drop_id='cap-0';",
    )
    .unwrap();
    let reader = t.reader();
    let tiers = [(10, Some("UT".into())), (20, Some("T5".into()))];
    let mut request = DropSummaryRequest {
        filter: filter(0),
        include_items: true,
        days: vec![
            TimeRange {
                start: Timestamp(0),
                end: Timestamp(10_000),
            },
            TimeRange {
                start: Timestamp(10_000),
                end: Timestamp(10_001),
            },
        ],
    };
    let summary = reader.drop_summary(request.clone(), &tiers).unwrap();
    assert_eq!(MAX_SUMMARY_ITEM_BAGS, 10_000);
    assert_eq!(
        (
            summary.totals.bags,
            summary.totals.runs,
            summary.totals.runs_ended,
            summary.totals.whites
        ),
        (10_001, 0, Some(0), 10_001)
    );
    assert_eq!(summary.totals.first_at, Some(Timestamp(0)));
    assert_eq!(
        summary.totals.last_white.as_ref().unwrap().at,
        Timestamp(10_000)
    );
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
    for (day, bags) in summary.days.iter().zip([10_000, 1]) {
        assert_eq!(day.bags, bags);
        assert!(day.items_counted);
        assert_eq!(
            (
                day.ut_items,
                day.st_items,
                day.enchanted_items,
                day.best_enchants
            ),
            (Some(bags), Some(0), Some(bags * 2), Some(3))
        );
    }
    request.filter.range = Some(request.days[0].clone());
    request.days = vec![TimeRange {
        start: Timestamp(20_000),
        end: Timestamp(20_001),
    }];
    let summary = reader.drop_summary(request.clone(), &tiers).unwrap();
    assert_eq!(summary.totals.bags, 10_000);
    assert!(summary.totals.items_counted);
    assert_eq!(summary.totals.enchanted_items, Some(20_000));
    assert!(summary.days[0].items_counted);
    assert_eq!(summary.days[0].ut_items, Some(0));
    request.include_items = false;
    // Invalid item JSON proves the skipped aggregate never evaluates item payloads.
    t.db.execute("UPDATE drop_items SET json='invalid synthetic JSON'", [])
        .unwrap();
    let summary = reader.drop_summary(request.clone(), &tiers).unwrap();
    for block in [&summary.totals, &summary.days[0]] {
        assert!(!block.items_counted);
        assert_eq!(
            (
                block.ut_items,
                block.st_items,
                block.enchanted_items,
                block.best_enchants
            ),
            (None, None, None, None)
        );
    }
    request.include_items = true;
    request.filter.range = None;
    request.days.clear();
    assert!(
        !reader
            .drop_summary(request, &tiers)
            .unwrap()
            .totals
            .items_counted
    );
}

#[test]
fn summary_legacy_request_defaults_to_counting_items() {
    let mut json = serde_json::json!({ "filter": filter(50), "days": [] });
    let legacy: DropSummaryRequest = serde_json::from_value(json.clone()).unwrap();
    assert!(legacy.include_items);
    json["include_items"] = false.into();
    let explicit: DropSummaryRequest = serde_json::from_value(json).unwrap();
    assert!(!explicit.include_items);
    let totals: DropTotals = serde_json::from_value(serde_json::json!({
        "bags": 0, "runs": 0, "runs_ended": 0, "whites": 0,
        "best_enchants": null, "last_white": null, "first_at": null
    }))
    .unwrap();
    assert_eq!(
        (totals.ut_items, totals.st_items, totals.enchanted_items),
        (None, None, None)
    );
    assert!(!totals.items_counted);
}

#[test]
fn detail_fields_preserve_legacy_json() {
    let place: PlaceCount =
        serde_json::from_str(r#"{"dungeon":"A","count":1,"last_at":"10"}"#).unwrap();
    let tier: BagTierCount = serde_json::from_str(r#"{"bag_rank":255,"count":1}"#).unwrap();
    let best: BestBagRef =
        serde_json::from_str(r#"{"drop_id":"b","run_id":null,"bag_rank":255,"at":"10"}"#).unwrap();
    for (value, field) in [
        (serde_json::to_value(place).unwrap(), "runs_since"),
        (serde_json::to_value(tier).unwrap(), "bag_type"),
        (serde_json::to_value(best).unwrap(), "bag_type"),
    ] {
        assert_eq!(value.get(field), Some(&serde_json::Value::Null));
    }
}

#[test]
fn detail_fields_count_ended_runs_after_each_places_last_seen() {
    let mut t = LootDb::new();
    t.run("a", "A", 0, Some(100), Some(Outcome::Cleared));
    t.bag(
        "a",
        Some("a"),
        50,
        Some(255),
        None,
        &[(0, 10, None, no_enchant())],
    );
    t.bag(
        "outside",
        None,
        70,
        None,
        None,
        &[(0, 10, None, no_enchant())],
    );
    for i in 0..3 {
        t.run(
            &format!("later{i}"),
            "A",
            101 + i * 100,
            Some(200 + i * 100),
            Some(Outcome::Cleared),
        );
    }
    t.run("other", "B", 101, Some(200), Some(Outcome::Cleared));
    t.run("equal", "A", 50, Some(100), Some(Outcome::Cleared));
    t.run("open", "A", 500, None, None);
    let h = t
        .reader()
        .item_history(ItemHistoryRequest {
            item_type: 10,
            cursor: None,
            limit: 50,
        })
        .unwrap();
    let places = serde_json::to_value(h.by_dungeon).unwrap();
    let a = places
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["dungeon"] == "A")
        .unwrap();
    assert_eq!(a["runs_since"], 3);
    let outside = places
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["dungeon"].is_null())
        .unwrap();
    assert_eq!(outside.get("runs_since"), Some(&serde_json::Value::Null));
    let sql = format!(
        "EXPLAIN QUERY PLAN {}",
        include_str!("../src/loot_item_places.sql")
    );
    for analyzed in [false, true] {
        if analyzed {
            t.db.execute_batch("ANALYZE").unwrap();
        }
        let plan =
            t.db.prepare(&sql)
                .unwrap()
                .query_map([10], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        assert!(
            plan.iter()
                .any(|step| step.starts_with("SEARCH streak_run USING")
                    && step.contains("INDEX")
                    && step.contains("dungeon=?")),
            "{plan:#?}"
        );
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN streak_run")),
            "{plan:#?}"
        );
    }
}

#[test]
fn detail_fields_stats_representative_types_and_best_actual_type() {
    let mut t = LootDb::new();
    t.run("a", "A", 0, Some(100), Some(Outcome::Cleared));
    t.run("b", "B", 0, Some(100), Some(Outcome::Cleared));
    for (id, run, at, bag_type, source, rank) in [
        ("a1", Some("a"), 10, 900, 1, Some(255)),
        ("a2", Some("a"), 20, 800, 1, Some(255)),
        ("a3", Some("a"), 30, 700, 2, Some(255)),
        ("b1", Some("b"), 40, 600, 1, Some(255)),
        ("outside", None, 50, 1100, 1, None),
    ] {
        let mut drop = demo::demo_snapshot().drops.remove(0);
        drop.id = id.into();
        drop.run_id = run.map(str::to_owned);
        drop.at = Timestamp(at);
        drop.bag_type = bag_type;
        drop.bag_rank = rank;
        drop.source_enemy = Some(source);
        t.put_drop(drop);
    }
    let stats = serde_json::to_value(
        t.reader()
            .loot_stats(LootStatsFilter { range: None })
            .unwrap(),
    )
    .unwrap();
    assert_eq!(stats["totals"][0]["bag_type"], 600);
    assert_eq!(stats["totals"][1]["bag_type"], 1100);
    let a = &stats["dungeons"][0];
    assert_eq!(a["dungeon"], "A");
    assert_eq!(a["bags"][0]["bag_type"], 700);
    assert_eq!(a["best"]["bag_type"], 700);
    let source = stats["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["dungeon"] == "A" && s["source_enemy"] == 1)
        .unwrap();
    assert_eq!(source["bags"][0]["bag_type"], 800);
}

#[test]
fn summary_outside_range_and_days_keep_counts_without_run_denominators() {
    let mut t = LootDb::new();
    t.run(
        "run",
        "Synthetic dungeon",
        1,
        Some(250),
        Some(Outcome::Cleared),
    );
    for (id, run, at) in [
        ("before", None, 99),
        ("first", None, 100),
        ("second", None, 200),
        ("end", None, 300),
        ("inside", Some("run"), 150),
    ] {
        t.bag(id, run, at, Some(255), None, &[(0, 10, None, no_enchant())]);
    }
    let mut filter = filter(0);
    filter.place = Some(LootPlace::Outside);
    filter.range = Some(TimeRange {
        start: Timestamp(100),
        end: Timestamp(300),
    });
    let summary = t
        .reader()
        .drop_summary(
            DropSummaryRequest {
                filter,
                include_items: true,
                days: vec![
                    TimeRange {
                        start: Timestamp(0),
                        end: Timestamp(200),
                    },
                    TimeRange {
                        start: Timestamp(200),
                        end: Timestamp(400),
                    },
                    TimeRange {
                        start: Timestamp(400),
                        end: Timestamp(500),
                    },
                ],
            },
            &[(10, Some("UT".into()))],
        )
        .unwrap();
    assert_eq!(
        (
            summary.totals.bags,
            summary.totals.whites,
            summary.totals.runs,
            summary.totals.runs_ended,
            summary.totals.ut_items
        ),
        (2, 2, 0, None, Some(2))
    );
    assert_eq!(
        summary
            .days
            .iter()
            .map(|day| (day.bags, day.runs_ended, day.ut_items))
            .collect::<Vec<_>>(),
        [(1, None, Some(1)), (1, None, Some(1)), (0, None, Some(0))]
    );
}
