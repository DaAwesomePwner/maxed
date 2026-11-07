//! All-time dry streak for the current dungeon, without the full Loot report.
use crate::{
    queries::identifier,
    stats::{count, unsigned},
    StoreError, StoreReader,
};
use maxed_types::{HomeDungeonStreak, HomeDungeonStreakRequest, Timestamp};

impl StoreReader {
    pub fn home_dungeon_streak(
        &self,
        request: HomeDungeonStreakRequest,
        account: Option<&str>,
    ) -> Result<HomeDungeonStreak, StoreError> {
        identifier(&request.dungeon)?;
        let (ended, last, dry, average): (i64, Option<i64>, Option<i64>, Option<f64>) = self
            .conn
            .prepare_cached(&crate::account_sql::with_accounts(include_str!(
                "home_streak.sql"
            )))?
            .query_row(rusqlite::params![request.dungeon, account], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
        Ok(HomeDungeonStreak {
            dungeon: request.dungeon,
            runs_ended: count(ended)?,
            runs_since_last_white: dry.map(count).transpose()?,
            avg_runs_per_white: average,
            last_white_at: last.map(unsigned).transpose()?.map(Timestamp),
        })
    }
}
