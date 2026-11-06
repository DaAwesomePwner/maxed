use crate::{project::micros, StoreError, StoreReader};
use maxed_types::{HomeScope, HomeSummary};
use rusqlite::params;

static HOME_SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    crate::stats::with_dungeon_stats(&with_home_scope(include_str!("home.sql")))
});

/// Share Home's session/day boundaries, scoped observations and per-character fame delta.
pub(crate) fn with_home_scope(sql: &str) -> String {
    const SLOT: &str = "/*home_scope*/";
    debug_assert_eq!(sql.matches(SLOT).count(), 1);
    sql.replacen(SLOT, include_str!("home_scope.sql"), 1)
}

impl StoreReader {
    /// One statement owns scope, aggregates and bounded details in one read snapshot.
    /// Today boundaries are caller-supplied local midnights, including DST changes.
    pub fn home_summary(&self, scope: HomeScope) -> Result<HomeSummary, StoreError> {
        let (kind, session_id, start, end) = match scope {
            HomeScope::Session { id } => {
                if id.is_empty() || id.len() > 256 {
                    return Err(StoreError::InvalidEvent);
                }
                (0, Some(id), None, None)
            }
            HomeScope::Today { range } => {
                let start = micros(range.start)?;
                let end = micros(range.end)?;
                if start >= end {
                    return Err(StoreError::InvalidEvent);
                }
                (1, None, Some(start), Some(end))
            }
        };
        let json: Option<String> = self
            .conn
            .query_row(
                HOME_SQL.as_str(),
                params![kind, session_id, start, end],
                |row| row.get(0),
            )
            .map_err(|error| {
                // SQLite's JSON functions reject corrupt stored JSON before serde runs.
                if matches!(&error, rusqlite::Error::SqliteFailure(_, Some(message))
                if message.contains("malformed JSON"))
                {
                    StoreError::InvalidEvent
                } else {
                    StoreError::from(error)
                }
            })?;
        // Serde performs checked u32 count conversion and finite-float validation.
        Ok(serde_json::from_str::<HomeSummary>(
            &json.ok_or(StoreError::InvalidEvent)?,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[test]
    fn shared_dungeon_statistics_are_spliced_once() {
        assert!(!HOME_SQL.contains(crate::stats::DUNGEON_STATS_SLOT));
        assert_eq!(HOME_SQL.matches("dungeon_counts AS (").count(), 1);
    }

    #[test]
    fn home_still_one_statement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home.sqlite");
        let mut db = rusqlite::Connection::open(&path).unwrap();
        crate::migrate(&mut db).unwrap();
        let reader = crate::open_reader(&path).unwrap();
        reader.conn.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(|event| {
                if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
                    STATEMENTS.with(|s| s.borrow_mut().push(statement.sql().into_owned()));
                }
            }),
        );
        for scope in [
            HomeScope::Session {
                id: "absent".into(),
            },
            HomeScope::Today {
                range: maxed_types::TimeRange {
                    start: maxed_types::Timestamp(0),
                    end: maxed_types::Timestamp(1),
                },
            },
        ] {
            STATEMENTS.with(|s| s.borrow_mut().clear());
            assert_eq!(reader.home_summary(scope).unwrap().runs, 0);
            STATEMENTS.with(|s| {
                let statements = s.borrow();
                assert_eq!(statements.len(), 1, "{statements:?}");
                assert!(statements[0].contains("WITH scope AS"));
            });
        }
    }
}
