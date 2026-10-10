//! Synthetic loot history through the shared native scheduler.
use super::history::{HistoryCaller, HistoryKey, INVALID};
use super::*;
use maxed_types::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Waker},
};

fn filter() -> DropFilter {
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

fn item_request() -> ItemHistoryRequest {
    ItemHistoryRequest {
        item_type: 3180,
        cursor: None,
        limit: 50,
    }
}

fn collection_request() -> CollectionRequest {
    CollectionRequest {
        dungeon: Some("Synthetic dungeon".into()),
        chase: vec![ChaseItem {
            item_type: 3180,
            dungeon: Some("Synthetic dungeon".into()),
            pinned_at: Timestamp(0),
        }],
    }
}

fn setup(seeded: bool) -> (tempfile::TempDir, Arc<CaptureRuntime>, DropView) {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = demo::demo_snapshot();
    let mut run = snapshot.run.unwrap();
    run.dungeon = "Synthetic dungeon".into();
    let mut bag = snapshot.drops.into_iter().next().unwrap();
    bag.run_id = Some(run.id.clone());
    bag.bag_rank = Some(255);
    bag.items = vec![3180];
    bag.contents = vec![DropItem {
        slot: 0,
        item_type: 3180,
        quantity: 1,
        taken_observed_at: None,
        enchant: EnchantView {
            state: EnchantState::Missing,
            slots: None,
            applied: None,
            ids: vec![],
        },
    }];
    if seeded {
        let mut db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
        maxed_store::migrate(&mut db).unwrap();
        let events = [
            EngineEvent::SessionUpdated(snapshot.session.unwrap()),
            EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
            EngineEvent::RunStarted(run),
            EngineEvent::BagDropped(bag.clone()),
        ];
        let events: Vec<_> = events
            .into_iter()
            .enumerate()
            .map(|(i, event)| EventEnvelope {
                fight_chart_omitted: false,
                stream_id: "synthetic-loot-history".into(),
                sequence: Sequence(Timestamp(i as u64 + 1)),
                at: bag.at,
                event,
            })
            .collect();
        maxed_store::apply_batch(&mut db, &events).unwrap();
    }
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(dir.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    (dir, runtime, bag)
}

fn ready<T: std::fmt::Debug>(reply: Result<HistoryReply<T>, String>) -> T {
    match reply.unwrap() {
        HistoryReply::Ready { value } => value,
        other => panic!("expected Ready, got {other:?}"),
    }
}

fn pending<F: Future>(future: Pin<&mut F>) {
    assert!(future
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

#[test]
fn loot_commands_return_ready_on_seeded_store() {
    let (_dir, runtime, bag) = setup(true);
    tauri::async_runtime::block_on(async {
        let page = ready(runtime.list_drops(filter()).await);
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].drop, bag);
        assert_eq!(page.rows[0].dungeon.as_deref(), Some("Synthetic dungeon"));
        let item = ready(runtime.get_item_history(item_request()).await);
        assert_eq!((item.totals.appearances, item.totals.bags), (1, 1));
        assert_eq!(item.bags, page);
        let stats = ready(
            runtime
                .get_loot_stats(LootStatsFilter { range: None })
                .await,
        );
        assert_eq!(stats.whites, 1);
        assert_eq!(stats.dungeons[0].best.as_ref().unwrap().drop_id, bag.id);
        let collection = ready(
            runtime
                .get_collection(collection_request(), vec![(3180, Some("UT".into()))])
                .await,
        );
        assert_eq!(collection.chase[0].seen, 1);
        assert_eq!(collection.chase[0].last_at, Some(bag.at));
        assert_eq!(
            collection.grid,
            vec![CollectionCell {
                item_type: 3180,
                tier: "UT".into(),
                count: 1,
                last_at: bag.at
            }]
        );
    });
}

#[test]
fn loot_commands_reject_invalid_keys_before_queueing() {
    let (_dir, runtime, _) = setup(true);
    let (send, entered) = bounded(1);
    let (release, wait) = bounded(1);
    *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
        send.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let mut active = Box::pin(super::history_tests::command(&runtime, 1));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut queued = Box::pin(runtime.list_drops(filter()));
    pending(queued.as_mut());
    let mut keys = vec![];
    for limit in [0, 51] {
        keys.push(HistoryKey::Drops(DropFilter { limit, ..filter() }));
        keys.push(HistoryKey::ItemHistory(ItemHistoryRequest {
            limit,
            ..item_request()
        }));
    }
    for (start, end) in [(2, 1), (1, 1), (0, i64::MAX as u64 + 1)] {
        let range = Some(TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        });
        keys.push(HistoryKey::Drops(DropFilter {
            range: range.clone(),
            ..filter()
        }));
        keys.push(HistoryKey::LootStats(LootStatsFilter { range }));
    }
    for name in [String::new(), "é".repeat(129)] {
        keys.push(HistoryKey::Drops(DropFilter {
            place: Some(LootPlace::Dungeon { name: name.clone() }),
            ..filter()
        }));
        keys.push(HistoryKey::Collection(
            CollectionRequest {
                dungeon: Some(name.clone()),
                ..collection_request()
            },
            vec![],
        ));
        let mut request = collection_request();
        request.chase[0].dungeon = Some(name);
        keys.push(HistoryKey::Collection(request, vec![]));
    }
    for cursor in [
        DropCursor {
            id: "é".repeat(129),
            at: Timestamp(0),
        },
        DropCursor {
            id: "valid".into(),
            at: Timestamp(i64::MAX as u64 + 1),
        },
    ] {
        keys.push(HistoryKey::Drops(DropFilter {
            cursor: Some(cursor.clone()),
            ..filter()
        }));
        keys.push(HistoryKey::ItemHistory(ItemHistoryRequest {
            cursor: Some(cursor),
            ..item_request()
        }));
    }
    let mut request = collection_request();
    request.chase = vec![request.chase[0].clone(); 101];
    keys.push(HistoryKey::Collection(request, vec![]));
    let mut request = collection_request();
    request.chase[0].pinned_at = Timestamp(i64::MAX as u64 + 1);
    keys.push(HistoryKey::Collection(request, vec![]));
    keys.push(HistoryKey::Collection(
        collection_request(),
        vec![(1, Some("x".repeat(17)))],
    ));
    keys.push(HistoryKey::Collection(
        collection_request(),
        vec![(1, None); 65_537],
    ));
    for key in keys {
        let error = tauri::async_runtime::block_on(runtime.submit_history(
            HistoryCaller::NativeTest,
            key,
            |_| -> Result<(), String> { panic!("invalid key reached worker") },
        ))
        .unwrap_err();
        assert_eq!(error, INVALID);
        let counts = runtime.shared.home.scheduler.lock().unwrap().counts();
        assert_eq!((counts.0, counts.1, counts.2), (1, 1, 0));
    }
    // Exercise the public wrappers too, using their independent MainWindow budget.
    tauri::async_runtime::block_on(async {
        assert_eq!(
            runtime
                .list_drops(DropFilter {
                    limit: 0,
                    ..filter()
                })
                .await
                .unwrap_err(),
            INVALID
        );
        assert_eq!(
            runtime
                .get_item_history(ItemHistoryRequest {
                    limit: 51,
                    ..item_request()
                })
                .await
                .unwrap_err(),
            INVALID
        );
        assert_eq!(
            runtime
                .get_loot_stats(LootStatsFilter {
                    range: Some(TimeRange {
                        start: Timestamp(1),
                        end: Timestamp(1)
                    })
                })
                .await
                .unwrap_err(),
            INVALID
        );
        assert_eq!(
            runtime
                .get_collection(
                    CollectionRequest {
                        dungeon: Some(String::new()),
                        ..collection_request()
                    },
                    vec![]
                )
                .await
                .unwrap_err(),
            INVALID
        );
    });
    // Invalid keys must neither enter the worker nor evict the valid pending read.
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(active));
    assert_eq!(ready(tauri::async_runtime::block_on(queued)).rows.len(), 1);
}

#[test]
fn loot_commands_on_absent_live_store_return_empty_values() {
    let (dir, runtime, _) = setup(false);
    tauri::async_runtime::block_on(async {
        let empty = DropPage {
            rows: vec![],
            next: None,
        };
        assert_eq!(ready(runtime.list_drops(filter()).await), empty);
        assert_eq!(
            ready(runtime.get_item_history(item_request()).await),
            ItemHistory {
                item_type: 3180,
                totals: ItemTotals {
                    appearances: 0,
                    bags: 0,
                    taken: 0,
                    first_at: None,
                    last_at: None
                },
                enchant_variants: vec![],
                by_dungeon: vec![],
                by_source: vec![],
                bags: empty,
            }
        );
        assert_eq!(
            ready(
                runtime
                    .get_loot_stats(LootStatsFilter { range: None })
                    .await
            ),
            LootStats {
                dungeons: vec![],
                sources: vec![],
                totals: vec![],
                whites: 0,
            }
        );
        let mut request = collection_request();
        request.chase.push(ChaseItem {
            item_type: 65535,
            dungeon: None,
            pinned_at: Timestamp(1),
        });
        let collection = ready(runtime.get_collection(request.clone(), vec![]).await);
        assert_eq!(
            collection,
            Collection {
                chase: request
                    .chase
                    .into_iter()
                    .map(|item| ChaseRow {
                        item,
                        seen: 0,
                        last_at: None,
                        dry_runs: 0,
                    })
                    .collect(),
                grid_dungeon: request.dungeon,
                grid: vec![]
            }
        );
    });
    assert!(!dir.path().join("live.sqlite3").exists());
}

#[test]
fn loot_reads_share_history_admission() {
    for abuse in [false, true] {
        let (_dir, runtime, _) = setup(true);
        let now = Instant::now();
        *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
        let (send, entered) = bounded(1);
        let (release, wait) = bounded(1);
        *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
            send.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
        }));
        let run_filter = RunFilter {
            character_id: None,
            dungeon: None,
            outcome: None,
            range: None,
            sort: RunSort::Newest,
            cursor: None,
            limit: 20,
        };
        let mut active = Box::pin(runtime.list_runs(run_filter.clone()));
        pending(active.as_mut());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        if abuse {
            for _ in 0..30 {
                assert_eq!(
                    tauri::async_runtime::block_on(runtime.list_drops(DropFilter {
                        limit: 0,
                        ..filter()
                    }))
                    .unwrap_err(),
                    INVALID
                );
            }
        }
        let mut queued = Box::pin(runtime.list_runs(RunFilter {
            limit: 10,
            ..run_filter
        }));
        pending(queued.as_mut());
        let mut loot = Box::pin(runtime.list_drops(filter()));
        pending(loot.as_mut());
        assert_eq!(
            runtime.shared.home.scheduler.lock().unwrap().counts(),
            if abuse { (1, 1, 1, 32) } else { (1, 1, 0, 3) }
        );
        if !abuse {
            assert_eq!(
                tauri::async_runtime::block_on(queued.as_mut()).unwrap(),
                HistoryReply::Superseded
            );
        }
        release.send(()).unwrap();
        ready(tauri::async_runtime::block_on(active));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        if abuse {
            assert_eq!(
                tauri::async_runtime::block_on(loot).unwrap(),
                HistoryReply::Busy
            );
            ready(tauri::async_runtime::block_on(queued));
        } else {
            assert_eq!(ready(tauri::async_runtime::block_on(loot)).rows.len(), 1);
        }
    }
}
