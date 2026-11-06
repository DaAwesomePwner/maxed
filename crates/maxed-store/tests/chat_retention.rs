use crate::common;
use common::chat::*;
use maxed_store::ChatMaintenance;
use maxed_types::*;

const DAY: u64 = 86_400_000_000;

fn db_with(messages: &[(u64, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let rows: Vec<_> = messages
        .iter()
        .enumerate()
        .map(|(i, (at, text))| {
            (
                message(i as u64 + 1, ChatChannel::Public, "SynthAlpha", text, *at),
                None,
            )
        })
        .collect();
    apply_chat(&mut db, &rows);
    (dir, path)
}

fn open(path: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).unwrap()
}

#[test]
fn prune_deletes_only_rows_older_than_the_cutoff() {
    let (_dir, path) = db_with(&[(10 * DAY, "old"), (100 * DAY, "new")]);
    let deleted = maxed_store::maintain_chat_file(
        &path,
        ChatMaintenance::Prune {
            before: Timestamp(50 * DAY),
        },
    )
    .unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(count(&open(&path)), 1);
    let fts: i64 = open(&path)
        .query_row(
            "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH '\"old\"'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts, 0);
}

#[test]
fn clock_far_behind_or_ahead_is_safe() {
    let (_dir, path) = db_with(&[(10 * DAY, "a"), (20 * DAY, "b")]);
    assert_eq!(
        maxed_store::maintain_chat_file(
            &path,
            ChatMaintenance::Prune {
                before: Timestamp(0)
            }
        )
        .unwrap(),
        0
    );
    assert_eq!(count(&open(&path)), 2);
    // A cutoff beyond every row deletes all of them, never more than exist.
    assert_eq!(
        maxed_store::maintain_chat_file(
            &path,
            ChatMaintenance::Prune {
                before: Timestamp(u64::MAX / 2)
            }
        )
        .unwrap(),
        2
    );
}

#[test]
fn prune_works_in_bounded_batches() {
    let rows: Vec<(u64, &str)> = (0..1_500).map(|i| (DAY + i, "x")).collect();
    let (_dir, path) = db_with(&rows);
    let first = maxed_store::maintain_chat_file(
        &path,
        ChatMaintenance::Prune {
            before: Timestamp(10 * DAY),
        },
    )
    .unwrap();
    assert_eq!(first, u64::from(maxed_store::CHAT_PRUNE_BATCH));
    let second = maxed_store::maintain_chat_file(
        &path,
        ChatMaintenance::Prune {
            before: Timestamp(10 * DAY),
        },
    )
    .unwrap();
    assert_eq!(second, 500);
}

#[test]
fn clear_removes_chat_and_chat_alerts_only() {
    let (_dir, path) = db_with(&[(10 * DAY, "a")]);
    {
        let db = open(&path);
        db.execute("INSERT INTO alerts VALUES('w',NULL,1,'whisper','{}')", [])
            .unwrap();
        db.execute(
            "INSERT INTO alerts VALUES('k',NULL,1,'key_popped','{}')",
            [],
        )
        .unwrap();
    }
    while maxed_store::maintain_chat_file(&path, ChatMaintenance::Clear).unwrap() > 0 {}
    let db = open(&path);
    assert_eq!(count(&db), 0);
    let kinds: Vec<String> = db
        .prepare("SELECT kind FROM alerts")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kinds, ["key_popped"]);
}

#[test]
fn prune_also_removes_old_chat_alerts() {
    let (_dir, path) = db_with(&[]);
    {
        let db = open(&path);
        db.execute(
            "INSERT INTO alerts VALUES('old',NULL,?1,'whisper','{}')",
            [DAY as i64],
        )
        .unwrap();
        db.execute(
            "INSERT INTO alerts VALUES('new',NULL,?1,'whisper','{}')",
            [(100 * DAY) as i64],
        )
        .unwrap();
    }
    maxed_store::maintain_chat_file(
        &path,
        ChatMaintenance::Prune {
            before: Timestamp(50 * DAY),
        },
    )
    .unwrap();
    let left: Vec<String> = open(&path)
        .prepare("SELECT id FROM alerts")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(left, ["new"]);
}

#[test]
fn cutoff_is_exclusive() {
    let (_dir, path) = db_with(&[(DAY - 1, "older"), (DAY, "boundary")]);
    assert_eq!(
        maxed_store::maintain_chat_file(
            &path,
            ChatMaintenance::Prune {
                before: Timestamp(DAY)
            }
        )
        .unwrap(),
        1
    );
    assert_eq!(count(&open(&path)), 1);
}

#[test]
fn clear_works_in_bounded_batches() {
    let rows: Vec<(u64, &str)> = (0..5_001).map(|i| (DAY + i, "synthetic")).collect();
    let (_dir, path) = db_with(&rows);
    assert_eq!(
        maxed_store::maintain_chat_file(&path, ChatMaintenance::Clear).unwrap(),
        u64::from(maxed_store::CHAT_CLEAR_BATCH)
    );
    assert_eq!(count(&open(&path)), 1);
    assert_eq!(
        maxed_store::maintain_chat_file(&path, ChatMaintenance::Clear).unwrap(),
        1
    );
    assert_eq!(
        maxed_store::maintain_chat_file(&path, ChatMaintenance::Clear).unwrap(),
        0
    );
    let fts: i64 = open(&path)
        .query_row(
            "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH 'synthetic'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts, 0);
}

#[test]
fn clear_and_cutoff_above_sqlite_range_include_the_largest_stored_timestamp() {
    for op in [
        ChatMaintenance::Clear,
        ChatMaintenance::Prune {
            before: Timestamp(u64::MAX),
        },
    ] {
        let (_dir, path) = db_with(&[(i64::MAX as u64, "synthetic")]);
        open(&path)
            .execute(
                "INSERT INTO alerts VALUES('last',NULL,?1,'whisper','{}')",
                [i64::MAX],
            )
            .unwrap();
        assert_eq!(maxed_store::maintain_chat_file(&path, op).unwrap(), 1);
        assert_eq!(count(&open(&path)), 0);
        let alerts: i64 = open(&path)
            .query_row("SELECT count(*) FROM alerts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(alerts, 0);
    }
}

#[test]
fn maintenance_preserves_pending_deliveries_and_unrelated_history() {
    for op in [
        ChatMaintenance::Clear,
        ChatMaintenance::Prune {
            before: Timestamp(2 * DAY),
        },
    ] {
        let (_dir, path) = db_with(&[]);
        let db = open(&path);
        for (id, kind, state) in [
            ("whisper", "whisper", "attempted"),
            ("guild", "guild_message", "suppressed"),
            ("party", "party_message", "attempted"),
            ("pending", "whisper", "pending"),
            ("key", "key_popped", "attempted"),
        ] {
            db.execute(
                "INSERT INTO alerts VALUES(?1,NULL,?2,?3,'{}')",
                rusqlite::params![id, DAY as i64, kind],
            )
            .unwrap();
            db.execute(
                "INSERT INTO alert_occurrences VALUES(?1,?2,?1,'test-stream',1,?3,'toast',?4,?1)",
                rusqlite::params![id, kind, DAY as i64, state],
            )
            .unwrap();
        }
        assert_eq!(maxed_store::maintain_chat_file(&path, op).unwrap(), 3);
        for table in ["alerts", "alert_occurrences"] {
            let left: Vec<String> = db
                .prepare(&format!("SELECT id FROM {table} ORDER BY id"))
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(left, ["key", "pending"]);
        }
        let sessions: i64 = db
            .query_row("SELECT count(*) FROM play_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sessions, 1);
    }
}

#[test]
fn maintenance_bounds_orphan_and_linked_occurrences_and_keeps_looping_without_chat() {
    for (op, batch) in [
        (ChatMaintenance::Clear, maxed_store::CHAT_CLEAR_BATCH),
        (
            ChatMaintenance::Prune {
                before: Timestamp(2 * DAY),
            },
            maxed_store::CHAT_PRUNE_BATCH,
        ),
    ] {
        let (_dir, path) = db_with(&[]);
        let mut db = open(&path);
        let tx = db.transaction().unwrap();
        for n in 0..batch + 1 {
            for kind in ["whisper", "guild_message", "party_message"] {
                tx.execute("INSERT INTO alert_occurrences VALUES(?1,?2,?1,'test-stream',1,?3,'off','suppressed',NULL)",
                    rusqlite::params![format!("orphan-{kind}-{n}"), kind, DAY as i64]).unwrap();
            }
            let id = format!("linked-{n}");
            tx.execute(
                "INSERT INTO alerts VALUES(?1,NULL,?2,'whisper','{}')",
                rusqlite::params![id, DAY as i64],
            )
            .unwrap();
            tx.execute("INSERT INTO alert_occurrences VALUES(?1,'whisper',?1,'test-stream',1,?2,'toast','attempted',?1)", rusqlite::params![id, DAY as i64]).unwrap();
        }
        for (id, kind, state, at) in [
            ("key", "key_popped", "suppressed", DAY),
            ("boundary", "whisper", "suppressed", 2 * DAY),
            ("future", "whisper", "suppressed", i64::MAX as u64),
        ] {
            tx.execute(
                "INSERT INTO alert_occurrences VALUES(?1,?2,?1,'test-stream',1,?3,'off',?4,NULL)",
                rusqlite::params![id, kind, at as i64, state],
            )
            .unwrap();
        }
        tx.execute(
            "INSERT INTO alert_occurrences VALUES('undated','whisper','undated',NULL,NULL,NULL,'off','suppressed',NULL)",
            [],
        ).unwrap();
        tx.commit().unwrap();
        let counts = || -> (i64, i64) {
            (
                db.query_row("SELECT count(*) FROM alerts", [], |r| r.get(0))
                    .unwrap(),
                db.query_row("SELECT count(*) FROM alert_occurrences", [], |r| r.get(0))
                    .unwrap(),
            )
        };
        let mut calls = 0;
        loop {
            let before = counts();
            let n = maxed_store::maintain_chat_file(&path, op).unwrap();
            let after = counts();
            assert!(before.0 - after.0 <= i64::from(batch));
            assert!(before.1 - after.1 <= i64::from(batch));
            calls += 1;
            assert!(calls <= 10, "maintenance must finish");
            if n < u64::from(batch) {
                break;
            }
        }
        assert!(calls > 1, "occurrences alone must keep maintenance running");
        assert_eq!(counts().0, 0);
        let left: Vec<String> = db
            .prepare("SELECT id FROM alert_occurrences ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            left,
            if matches!(op, ChatMaintenance::Clear) {
                vec!["key"]
            } else {
                vec!["boundary", "future", "key", "undated"]
            }
        );
    }
}

#[test]
fn maintenance_bounds_alerts_without_chat_or_occurrences() {
    for (op, batch) in [
        (ChatMaintenance::Clear, maxed_store::CHAT_CLEAR_BATCH),
        (
            ChatMaintenance::Prune {
                before: Timestamp(2 * DAY),
            },
            maxed_store::CHAT_PRUNE_BATCH,
        ),
    ] {
        let (_dir, path) = db_with(&[]);
        let mut db = open(&path);
        let tx = db.transaction().unwrap();
        for n in 0..batch + 1 {
            tx.execute(
                "INSERT INTO alerts VALUES(?1,NULL,1,'whisper','{}')",
                [format!("alert-{n}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert_eq!(
            maxed_store::maintain_chat_file(&path, op).unwrap(),
            u64::from(batch)
        );
        let left: i64 = db
            .query_row("SELECT count(*) FROM alerts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1);
        assert_eq!(maxed_store::maintain_chat_file(&path, op).unwrap(), 1);
        assert_eq!(maxed_store::maintain_chat_file(&path, op).unwrap(), 0);
    }
}
