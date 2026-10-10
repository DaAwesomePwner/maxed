//! Compact per-run boss navigation and selected-player contribution.
use crate::{
    queries::{bounded, identifier, row_json},
    StoreError, StoreReader,
};
use maxed_types::RunFightStripRow;
use rusqlite::params;

const MAX_STRIP_BYTES: usize = 256 * 1024;
// Project only strip fields: full fights can contain megabytes of series/sources.
const FIGHTS: &str = "WITH strip AS (
    SELECT json_object(
        'fight_id',f.id,
        'target_type',json_extract(s.json,'$.target_type'),
        'target_name',json_extract(s.json,'$.target_name'),
        'boss_type',f.boss_type,
        'started',json_extract(s.json,'$.started'),
        'ended',json_extract(s.json,'$.ended'),
        'killed',json(CASE WHEN json_extract(f.json,'$.kill.provisional')=0
            OR json_extract(f.json,'$.end_reason')='killed' THEN 'true' ELSE 'false' END),
        'player_count',json_extract(s.json,'$.player_count'),
        'damage',NULL,'rank',NULL,'share',NULL) AS payload
    FROM fight_summaries s JOIN fights f ON f.id=s.fight_id
    WHERE s.run_id=?1 AND (json_extract(s.json,'$.target_kind') IN ('boss','event_boss')
        OR f.boss_type IS NOT NULL)
    ORDER BY s.started,s.fight_id LIMIT 64
) SELECT length(CAST(payload AS BLOB)),payload FROM strip";
const LOCAL: &str = "SELECT length(CAST(payload AS BLOB)),payload FROM (
    SELECT json_array(damage,rank,json_extract(json,'$.share')) AS payload
    FROM fight_players WHERE fight_id=?1 AND json_extract(json,'$.is_local')=1
        AND NOT EXISTS (
            SELECT 1 FROM fights f, json_each(f.json,'$.issues') issue
            WHERE f.id=?1 AND issue.value='missing_identity'
        ) LIMIT 2)";
const PLAYER: &str = "SELECT length(CAST(payload AS BLOB)),payload FROM (
    SELECT json_array(damage,rank,json_extract(json,'$.share')) AS payload
    FROM fight_players WHERE fight_id=?1 AND player_id=?2 LIMIT 1)";

impl StoreReader {
    pub fn run_fight_strip(
        &self,
        run_id: &str,
        player_id: Option<&str>,
    ) -> Result<Vec<RunFightStripRow>, StoreError> {
        identifier(run_id)?;
        if let Some(id) = player_id {
            identifier(id)?;
        }
        let tx = self.conn.unchecked_transaction()?;
        let mut bytes = 0;
        let mut result = Vec::<RunFightStripRow>::new();
        {
            let mut statement = tx.prepare(FIGHTS)?;
            let mut rows = statement.query([run_id])?;
            while let Some(row) = rows.next()? {
                result.push(row_json(row, 0, &mut bytes, MAX_STRIP_BYTES)?);
            }
        }
        {
            let mut statement = tx.prepare(if player_id.is_some() { PLAYER } else { LOCAL })?;
            for fight in &mut result {
                let mut rows = if let Some(id) = player_id {
                    statement.query(params![fight.fight_id, id])?
                } else {
                    statement.query([&fight.fight_id])?
                };
                if let Some(row) = rows.next()? {
                    let (damage, rank, share): (Option<f64>, Option<u32>, Option<f64>) =
                        row_json(row, 0, &mut bytes, MAX_STRIP_BYTES)?;
                    // A second local identity makes the selection ambiguous.
                    if rows.next()?.is_none() {
                        fight.damage = damage;
                        fight.rank = rank;
                        fight.share = share;
                    }
                }
            }
        }
        let result = bounded(result, MAX_STRIP_BYTES)?;
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_queries_use_indexed_lookups() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrate(&mut conn).unwrap();
        for (sql, args, table) in [
            (FIGHTS, vec!["demo-run"], "f"),
            (LOCAL, vec!["demo-fight"], "fight_players"),
            (PLAYER, vec!["demo-fight", "TestMage"], "fight_players"),
        ] {
            let plan = conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            assert!(
                plan.contains(&format!("SEARCH {table} USING INDEX")),
                "{plan}"
            );
            assert!(!plan.contains(&format!("SCAN {table}")), "{plan}");
            if sql == LOCAL {
                assert!(plan.contains("SEARCH f USING INDEX"), "{plan}");
                assert!(!plan.contains("SCAN f"), "{plan}");
            }
            if sql == FIGHTS {
                assert!(
                    plan.contains("SEARCH s USING INDEX fight_summaries_run"),
                    "{plan}"
                );
            }
        }
    }
}
