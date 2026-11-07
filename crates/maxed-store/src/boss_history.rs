//! Local boss performance and confirmed kill times across all characters.
use crate::{
    party_average::with_scoped_party_average,
    stats::{bounds, count, median},
    StoreError, StoreReader,
};
use maxed_types::{
    BossHistory, BossHistoryRequest, BossHistoryRow, HomeBossBaseline, HomeBossBaselines,
    StatsFilter, MAX_STATS_ROWS,
};
use rusqlite::{params, Connection, Row};
use std::sync::OnceLock;

// The templates remain independently explainable with optional filters. Production
// uses only these eight fixed variants; request values are always bound parameters.
const RANGE: &str =
    "/*range*/ (?1 IS NULL OR fights.started>=?1) AND (?2 IS NULL OR fights.started<?2)";
const TARGETS: &str =
    "/*targets*/ (?4 IS NULL OR target_type IN (SELECT value FROM json_each(?4)))";

fn sql(lean: bool, bounded: bool, targeted: bool) -> &'static str {
    static SQL: OnceLock<[String; 8]> = OnceLock::new();
    &SQL.get_or_init(|| {
        std::array::from_fn(|index| {
            let template = if index & 4 != 0 {
                include_str!("home_boss_baselines.sql").to_owned()
            } else {
                with_scoped_party_average(include_str!("boss_history.sql"))
            };
            debug_assert_eq!(template.matches(RANGE).count(), 1);
            debug_assert_eq!(template.matches(TARGETS).count(), 1);
            template
                .replace(
                    RANGE,
                    if index & 2 != 0 {
                        "fights.started>=?1 AND fights.started<?2"
                    } else {
                        "1"
                    },
                )
                .replace(
                    TARGETS,
                    if index & 1 != 0 {
                        "target_type IN (SELECT value FROM json_each(?4))"
                    } else {
                        "1"
                    },
                )
                .replace("/*fight_character*/", include_str!("fight_character.sql"))
        })
    })[usize::from(lean) * 4 + usize::from(bounded) * 2 + usize::from(targeted)]
}

fn read_rows<T>(
    conn: &Connection,
    request: BossHistoryRequest,
    lean: bool,
    account: Option<&str>,
    mut decode: impl FnMut(&Row<'_>) -> Result<T, StoreError>,
) -> Result<(Vec<T>, bool), StoreError> {
    let filter = StatsFilter {
        range: request.range,
        character_id: request.character_id,
    };
    let (lo, hi, character) = bounds(&filter)?;
    if request
        .target_types
        .as_ref()
        .is_some_and(|types| types.len() > 64)
    {
        return Err(StoreError::InvalidEvent);
    }
    let targets = request
        .target_types
        .map(|types| serde_json::to_string(&types))
        .transpose()?;
    let mut statement = conn.prepare_cached(sql(lean, lo.is_some(), targets.is_some()))?;
    // Without a target predicate, ?3 is the last slot; the earlier numbered
    // slots remain valid even in the all-time variant.
    let parameter_count = statement.parameter_count();
    let bindings = params![lo, hi, character, targets, account];
    let mut query = statement.query(&bindings[..parameter_count])?;
    let mut rows = Vec::new();
    while let Some(row) = query.next()? {
        rows.push(decode(row)?);
    }
    let truncated = rows.len() > MAX_STATS_ROWS;
    rows.truncate(MAX_STATS_ROWS);
    Ok((rows, truncated))
}

impl StoreReader {
    pub fn boss_history(&self, request: BossHistoryRequest) -> Result<BossHistory, StoreError> {
        let (rows, truncated) = read_rows(&self.conn, request, false, None, |row| {
            let mut durations: Vec<u64> = serde_json::from_str(&row.get::<_, String>(8)?)?;
            Ok(BossHistoryRow {
                target_type: row.get(0)?,
                target_name: row.get(1)?,
                fights: count(row.get(2)?)?,
                avg_dps: row.get(3)?,
                best_dps: row.get(4)?,
                best_fight_id: row.get(5)?,
                avg_rank: row.get(6)?,
                avg_party_dps: row.get(7)?,
                kills: u32::try_from(durations.len()).map_err(|_| StoreError::InvalidEvent)?,
                median_kill_micros: median(&mut durations),
            })
        })?;
        Ok(BossHistory { rows, truncated })
    }

    pub fn home_boss_baselines(
        &self,
        request: BossHistoryRequest,
        account: Option<&str>,
    ) -> Result<HomeBossBaselines, StoreError> {
        let (rows, truncated) = read_rows(&self.conn, request, true, account, |row| {
            let mut durations: Vec<u64> = serde_json::from_str(&row.get::<_, String>(4)?)?;
            Ok(HomeBossBaseline {
                target_type: row.get(0)?,
                fights: count(row.get(1)?)?,
                avg_dps: row.get(2)?,
                best_dps: row.get(3)?,
                kill_samples: u32::try_from(durations.len())
                    .map_err(|_| StoreError::InvalidEvent)?,
                median_kill_micros: median(&mut durations),
            })
        })?;
        Ok(HomeBossBaselines { rows, truncated })
    }
}
