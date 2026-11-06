//! Shared party DPS averages for saved fights.
use crate::{stats::count, StoreError};
use maxed_types::PartyAverage;
use rusqlite::{params_from_iter, Transaction};
use std::collections::HashMap;

const PARTY_AVERAGE_SLOT: &str = "/*party_average*/";
const SCOPED_PARTY_AVERAGE_SLOT: &str = "/*party_average_scoped*/";

/// The host must define `party_scope(fight_id)` before this slot.
/// Filter player rows before grouping so unrelated history is never aggregated.
pub(crate) fn with_scoped_party_average(sql: &str) -> String {
    debug_assert_eq!(sql.matches(SCOPED_PARTY_AVERAGE_SLOT).count(), 1);
    sql.replacen(
        SCOPED_PARTY_AVERAGE_SLOT,
        include_str!("party_average_scoped.sql"),
        1,
    )
}

/// Splice the shared party-average CTE into a host statement exactly once.
pub(crate) fn with_party_average(sql: &str) -> String {
    debug_assert_eq!(sql.matches(PARTY_AVERAGE_SLOT).count(), 1);
    sql.replacen(PARTY_AVERAGE_SLOT, include_str!("party_average.sql"), 1)
}

pub(crate) fn party_averages(
    tx: &Transaction<'_>,
    fight_ids: &[&str],
) -> Result<HashMap<String, PartyAverage>, StoreError> {
    if fight_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; fight_ids.len()].join(",");
    let sql = with_party_average(&format!(
        "WITH /*party_average*/
         SELECT fight_id, avg_dps, players FROM party_avg
         WHERE fight_id IN ({placeholders})"
    ));
    let mut statement = tx.prepare_cached(&sql)?;
    let rows = statement.query_map(params_from_iter(fight_ids), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, avg_dps, players) = row?;
        if !avg_dps.is_finite() {
            return Err(StoreError::InvalidEvent);
        }
        Ok((
            id,
            PartyAverage {
                avg_dps,
                players: count(players)?,
            },
        ))
    })
    .collect()
}
