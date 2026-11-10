use rusqlite::{
    params,
    trace::{TraceEvent, TraceEventCodes},
    Connection,
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

// Historical migration tests exercise the confirmed path, just as installed
// legacy histories must. No production path may upgrade these implicitly.
pub fn upgrade_after_confirmation(
    path: &std::path::Path,
) -> Result<(), maxed_store::backup::BackupError> {
    use maxed_store::{backup::*, StoreError};
    struct Space;
    impl BackupEnvironment for Space {
        fn available_space(&self, _: &std::path::Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            100
        }
    }
    match maxed_store::history_preflight(path) {
        Err(StoreError::HistoryUpgradeRequired) => HistoryUpgrade::open(path)?
            .confirm_and_upgrade(&Space, &std::sync::atomic::AtomicBool::new(false)),
        result => {
            result?;
            maxed_store::upgrade_history(path).map_err(Into::into)
        }
    }
}

// Installed scripts and their original LF-byte checksums are immutable.
const SCRIPTS: [(&str, &str); 19] = [
    (
        include_str!("../../migrations/001_initial.sql"),
        "b3aadbd6e99610c6da10b9c9ab8d942778894b9fab7def4efa5623b39b512c77",
    ),
    (
        include_str!("../../migrations/002_fight_summaries.sql"),
        "78f71e3a1fc55cc5c36afee962dbbb0291557a5046a5ec4db0f17f995108a219",
    ),
    (
        include_str!("../../migrations/003_loot_and_sessions.sql"),
        "2ad88d9d7beb92d67f5811dac107b43a1e1308d675beaea6be6a8ee921a911ca",
    ),
    (
        include_str!("../../migrations/004_alerts.sql"),
        "067fc53495aa86ea80fa7c495fbb0ff76a6acf44bae97652fd88730186558cc7",
    ),
    (
        include_str!("../../migrations/005_instance.sql"),
        "d75f71d68c277c57ccf65a44bcccf9cc7666c17f2ad97eb19a5ce71283f3e744",
    ),
    (
        include_str!("../../migrations/006_summary_target_kinds.sql"),
        "6ed13f1aceeccf7d71235f25135dbc39990edcb859e9c6410d2dac72b7f58f0e",
    ),
    (
        include_str!("../../migrations/007_pb_lookup_indexes.sql"),
        "da883d54ebb1ee76fcd75bb3f42f7f8655ea3d2537bfbb1ecc0156af3ea4befc",
    ),
    (
        include_str!("../../migrations/008_loot_browsing.sql"),
        "431ac95bce7d35c603ab47ffddc29379e83afb828b7a40f7747e28c04b0777d9",
    ),
    (
        include_str!("../../migrations/009_chat.sql"),
        "eea49090fa43463b9a6f7706e0aedf61fd6e20bdd4a1bc40ce50a0ae2b61c552",
    ),
    (
        include_str!("../../migrations/010_map_visits.sql"),
        "53f8413a07f07e3f5b561592530f7e45c7880c5d0f009d9b5bf04436d4d122a6",
    ),
    (
        include_str!("../../migrations/011_exclude_junk_bags.sql"),
        "929a8a5dcb973ea595923eecd041ebc5108bc88055ee191ede92cde28651ef0c",
    ),
    (
        include_str!("../../migrations/012_event_fts.sql"),
        "2aafb83767267ab4cd48eb013269485561dc77ca5b615f71250116a0c8f73312",
    ),
    (
        include_str!("../../migrations/013_player_profiles.sql"),
        "320e1ee873092433a96479e0bc542b8e68e8ed53126468d7444a6cf6af3a4165",
    ),
    (
        include_str!("../../migrations/014_party.sql"),
        "a286f20d91d47ec9a7fc50032070cbd0acf4a13735935c1bdb089fd2e803cc62",
    ),
    (
        include_str!("../../migrations/015_loot_stats_indexes.sql"),
        "17945317c8cad16082269178d79bc65d80f9d5387765d7ccd7ef6b71597efca8",
    ),
    (
        include_str!("../../migrations/016_maintenance_receipts.sql"),
        "56c70ccc3c7bd6d80765025d4ed128916e000c45ca2259da19209a9357ee3cb0",
    ),
    (
        include_str!("../../migrations/017_accounts.sql"),
        "2cf6c0bc4a3ad5b8f902dc67bb1b8a62331d1398e9e5a7ad1bc9c6a04de11154",
    ),
    (
        include_str!("../../migrations/018_account_reads.sql"),
        "142e85b5974102bc16677af856ba36bcd5ab4679856fe004462ca7051de4c012",
    ),
    (
        include_str!("../../migrations/019_attribution_notice.sql"),
        "c25f711e938a91fcca77ac2cbe674e2602d0d5502c3c52ff700fa7c6cf91a4b4",
    ),
];

pub fn install(db: &Connection, version: usize) {
    assert!((1..=SCRIPTS.len()).contains(&version));
    db.pragma_update(None, "foreign_keys", true).unwrap();
    // Install in one transaction, as production migrations do. Autocommitting
    // every statement syncs the disk once per statement, which is slow on CI.
    db.execute_batch("BEGIN").unwrap();
    for (i, (script, checksum)) in SCRIPTS.iter().take(version).enumerate() {
        assert_eq!(hash(script), *checksum);
        db.execute_batch(script).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            params![i as i64 + 1, checksum],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", version as i64)
        .unwrap();
    db.execute_batch("COMMIT").unwrap();
}

pub fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub fn schema(db: &Connection) -> Vec<(String, String, String, String)> {
    db.prepare("SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_schema ORDER BY type,name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

pub fn assert_installed(db: &Connection, version: usize) {
    // Fingerprints include every table, index, trigger, autoindex and FTS shadow
    // object, with its owner and SQL, before any latest-schema code runs.
    let expected = match version {
        12 => "8692974e58c969522cb49429b4a9be6cf2f583ad93b3b9e6dafc48e1e43b9312",
        14 => "c0296eed5a2092decf7bdcbf777ffdf7ec5c3987dca4fec1d553afdf94cb1322",
        15 => "ff3a762487bb24f928a59fa06e38e96a9c06c2ebae22b9535622d58f8b40b9da",
        _ => panic!("no installed object fingerprint for version {version}"),
    };
    let objects = schema(db)
        .into_iter()
        .map(|(kind, name, table, sql)| format!("{kind}|{name}|{table}|{sql}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(hash(&objects), expected);
    assert_eq!(user_version(db), version as i64);
    assert_eq!(
        checksums(db),
        SCRIPTS
            .iter()
            .take(version)
            .enumerate()
            .map(|(i, (_, sum))| (i as i64 + 1, sum.to_string()))
            .collect::<Vec<_>>()
    );
}

pub fn user_version(db: &Connection) -> i64 {
    db.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

pub fn checksums(db: &Connection) -> Vec<(i64, String)> {
    db.prepare("SELECT version,checksum FROM schema_migrations ORDER BY version")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

pub fn assert_integrity(db: &Connection) {
    let results: Vec<String> = db
        .prepare("PRAGMA integrity_check")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(results, ["ok"]);
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

// Small synthetic fixtures only. Hash sorted, typed rows to compare every saved
// byte without exposing identity fields if an assertion fails.
pub fn history(db: &Connection) -> BTreeMap<String, String> {
    let mut snapshots: BTreeMap<_, _> = schema(db)
        .into_iter()
        .filter(|(kind, name, _, _)| kind == "table" && name != "schema_migrations")
        .map(|(_, name, _, _)| {
            let columns = if name == "characters" {
                "id,observed_at,json"
            } else {
                "*"
            };
            let digest = row_hash(
                db,
                &format!("SELECT {columns} FROM \"{}\"", name.replace('"', "\"\"")),
            );
            (name, digest)
        })
        .collect();
    // Keep legacy payload hashes comparable, while still detecting any mutation
    // of the new membership column in rollback/idempotency snapshots.
    if db.prepare("SELECT account_id FROM characters").is_ok() {
        snapshots.insert(
            "characters.account_id".into(),
            row_hash(db, "SELECT id,account_id FROM characters"),
        );
    }
    snapshots
}

fn row_hash(db: &Connection, sql: &str) -> String {
    let mut stmt = db.prepare(sql).unwrap();
    let columns = stmt.column_count();
    let mut rows = stmt
        .query_map([], |r| {
            (0..columns)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .map(|r| format!("{:?}", r.unwrap()))
        .collect::<Vec<_>>();
    rows.sort();
    hash(&rows.join("\n"))
}

/// The sole allowed modification of an installed object by 017 is this exact
/// nullable column; every other byte of every installed schema object survives.
pub fn account_schema_preserves(
    object: &(String, String, String, String),
    after: &[(String, String, String, String)],
) -> bool {
    let mut expected = object.clone();
    if object.0 == "table" && object.1 == "characters" {
        expected.3 = format!(
            "{}, account_id TEXT REFERENCES accounts(id))",
            object.3.strip_suffix(')').unwrap()
        );
    }
    after.contains(&expected)
}

thread_local! {
    static TRACE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn trace_sql<T>(
    db: &mut Connection,
    action: impl FnOnce(&mut Connection) -> T,
) -> (T, Vec<String>) {
    TRACE.with(|trace| trace.borrow_mut().clear());
    db.trace_v2(
        TraceEventCodes::SQLITE_TRACE_STMT,
        Some(|event| {
            if let TraceEvent::Stmt(stmt, _) = event {
                TRACE.with(|trace| trace.borrow_mut().push(stmt.expanded_sql().unwrap()));
            }
        }),
    );
    let result = action(db);
    db.trace_v2(TraceEventCodes::empty(), None);
    (
        result,
        TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut())),
    )
}

fn normalized(sql: &str) -> String {
    sql.lines()
        .map(|line| line.split("--").next().unwrap())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(';')
        .to_uppercase()
}

/// Conservative DDL allowlist: no CTAS, populated-table index, or ALTER (including
/// ADD COLUMN CHECK/REFERENCES, which can scan implicitly). New-table checks/FKs
/// are safe. Future migrations must explicitly extend this policy and its tests.
pub fn schema_only(sql: &str, existing: &BTreeSet<String>, created: &mut BTreeSet<String>) -> bool {
    let normalized = normalized(sql);
    let tokens: Vec<_> = normalized
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.contains(&"SELECT") || tokens.contains(&"AS") {
        return false;
    }
    match tokens.as_slice() {
        ["CREATE", "TABLE", name, ..] if !existing.contains(*name) && !created.contains(*name) => {
            created.insert(name.to_string());
            true
        }
        ["CREATE", "INDEX", _, "ON", table, ..]
        | ["CREATE", "UNIQUE", "INDEX", _, "ON", table, ..] => created.contains(*table),
        _ => false,
    }
}

/// Attribute each executed script by its following checksum insertion, rather
/// than exempting an entire v12 upgrade because 014 has a historical backfill.
/// The same helper can cover direct and successive installed-release upgrades.
pub fn assert_additive_upgrade(
    trace: &[String],
    installed: usize,
    target: usize,
    objects: &[(String, String, String, String)],
) -> BTreeMap<usize, Vec<String>> {
    let existing = objects
        .iter()
        .map(|(_, name, _, _)| name.to_uppercase())
        .collect();
    let mut created = BTreeSet::new();
    let mut scripts = BTreeMap::new();
    let mut pending: Vec<String> = vec![];
    let mut next = installed + 1;
    for sql in trace {
        let sql = normalized(sql);
        if sql.is_empty() {
            continue;
        }
        if let Some(registry) = sql.strip_prefix("INSERT INTO SCHEMA_MIGRATIONS VALUES(") {
            let version: usize = registry.split(',').next().unwrap().parse().unwrap();
            assert_eq!(
                version, next,
                "each script must have one ordered checksum insertion"
            );
            if version == 17 {
                // Match the complete reviewed 017 script, including its sole
                // ALTER, populated-character index and read-only ownership views.
                let baseline: Vec<_> =
                    normalized(include_str!("../../migrations/017_accounts.sql"))
                        .split(';')
                        .map(normalized)
                        .filter(|sql| !sql.is_empty())
                        .collect();
                assert_eq!(
                    pending, baseline,
                    "unexpected work attributed to migration 017"
                );
            } else if version == 18 {
                let baseline: Vec<_> =
                    normalized(include_str!("../../migrations/018_account_reads.sql"))
                        .split(';')
                        .map(normalized)
                        .filter(|sql| !sql.is_empty())
                        .collect();
                assert_eq!(
                    pending, baseline,
                    "unexpected work attributed to migration 018"
                );
            } else if version == 19 {
                assert_eq!(
                    normalized(&pending.join("; ")),
                    normalized(include_str!("../../migrations/019_attribution_notice.sql")),
                    "unexpected work attributed to migration 019"
                );
            } else if version == 20 {
                // CAST(id AS BLOB) measures UTF-8 bytes; the conservative generic
                // checker rejects AS to forbid CREATE TABLE AS SELECT.
                assert_eq!(
                    normalized(&pending.join("; ")),
                    normalized(include_str!(
                        "../../migrations/020_crucible_definitions.sql"
                    )),
                    "unexpected work attributed to migration 020"
                );
            } else if version >= 16 {
                assert!(!pending.is_empty(), "missing additive DDL trace");
                for statement in &pending {
                    assert!(
                        schema_only(statement, &existing, &mut created),
                        "migration {version} is not schema-only DDL: {statement}"
                    );
                }
            } else if version >= 13 {
                let baseline: Vec<_> = normalized(SCRIPTS[version - 1].0)
                    .split(';')
                    .map(normalized)
                    .filter(|sql| !sql.is_empty())
                    .collect();
                assert_eq!(
                    pending, baseline,
                    "unexpected work attributed to migration {version}"
                );
            }
            scripts.insert(version, std::mem::take(&mut pending));
            next += 1;
        } else if next == installed + 1
            && pending.is_empty()
            && ([
                "PRAGMA FOREIGN_KEYS=1",
                "PRAGMA FOREIGN_KEYS",
                "BEGIN IMMEDIATE",
                "PRAGMA USER_VERSION",
            ]
            .contains(&sql.as_str())
                || sql
                    == format!(
                        "SELECT VERSION,CHECKSUM FROM SCHEMA_MIGRATIONS ORDER BY VERSION LIMIT {}",
                        target + 1
                    ))
        {
            // Existing bounded registry validation and connection/transaction setup.
        } else if next == target + 1 {
            assert!(
                sql == format!("PRAGMA USER_VERSION={target}") || sql == "COMMIT",
                "unexpected post-migration SQL: {sql}"
            );
        } else {
            pending.push(sql);
        }
    }
    assert_eq!(next, target + 1);
    assert!(pending.is_empty());
    assert_eq!(trace.last().map(|s| normalized(s)), Some("COMMIT".into()));
    scripts
}
