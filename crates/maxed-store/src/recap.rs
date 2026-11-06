//! Historical records and journey context for saved runs and realm visits.
use crate::{
    journey,
    project::micros,
    queries::{bounded, fight_summaries, identifier, MAX_RUN_STORY_BYTES},
    records, run_screen, StoreError, StoreReader,
};
use maxed_types::{
    RunRecap, RunRecapRequest, Timestamp, VisitPeriod, VisitRecap, VisitRecapRequest,
};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

struct VisitFacts {
    session: String,
    character: Option<String>,
    map: String,
    instance: Option<String>,
    period: VisitPeriod,
    last_traffic: u64,
}

fn timestamp(value: i64) -> rusqlite::Result<Timestamp> {
    u64::try_from(value)
        .map(Timestamp)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

impl StoreReader {
    pub fn visit_recap(
        &self,
        request: &VisitRecapRequest,
    ) -> Result<Option<VisitRecap>, StoreError> {
        identifier(&request.visit_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let visit = tx
            .prepare_cached(include_str!("visit_recap.sql"))?
            .query_row([&request.visit_id], |row| {
                Ok(VisitFacts {
                    session: row.get(0)?,
                    character: row.get(1)?,
                    map: row.get(2)?,
                    instance: row.get(3)?,
                    period: VisitPeriod {
                        visit_id: request.visit_id.clone(),
                        started: timestamp(row.get(4)?)?,
                        ended: row.get::<_, Option<i64>>(5)?.map(timestamp).transpose()?,
                    },
                    last_traffic: timestamp(row.get(6)?)?.0,
                })
            })
            .optional()?;
        let Some(visit) = visit else {
            return Ok(None);
        };
        // Period collection is independent of the bounded journey inference.
        let periods = if let Some(key) = &visit.instance {
            tx.prepare_cached(include_str!("visit_periods.sql"))?
                .query_map(params![visit.session, key], |row| {
                    Ok(VisitPeriod {
                        visit_id: row.get(0)?,
                        started: timestamp(row.get(1)?)?,
                        ended: row.get::<_, Option<i64>>(2)?.map(timestamp).transpose()?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![visit.period]
        };
        // Clock regressions can overlap stays. Query their union once while
        // preserving every original period and its unknown departure in the DTO.
        let mut intervals: Vec<(u64, u64)> = Vec::new();
        for period in &periods {
            let start = period.started.0;
            let end = period.ended.map_or(visit.last_traffic, |at| at.0);
            if end <= start {
                continue;
            }
            if let Some(previous) = intervals.last_mut().filter(|previous| start <= previous.1) {
                previous.1 = previous.1.max(end);
            } else {
                intervals.push((start, end));
            }
        }
        let scope = serde_json::to_string(&intervals)?;
        let mut title = visit.map;
        for period in &periods {
            let castle: Option<String> = tx
                .prepare_cached(include_str!("visit_castle.sql"))?
                .query_row(
                    params![visit.session, micros(period.started)?, period.visit_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(castle) = castle else {
                continue;
            };
            if let Some(name) = journey::castle_realm_name(&tx, Some(&castle))? {
                title = name;
                break;
            }
        }
        let visits = journey::session_visits(&tx, &visit.session)?;
        let period_ids = periods
            .iter()
            .map(|period| period.visit_id.as_str())
            .collect::<HashSet<_>>();
        let mut chain = None;
        for (index, row) in visits.iter().enumerate() {
            if row.kind == "realm" && period_ids.contains(row.id.as_str()) {
                if let Some(range) =
                    journey::chain_range(&visits, index).filter(|range| range.start == index)
                {
                    chain = Some(journey::chain(&tx, &visit.session, &visits, range)?);
                    break;
                }
            }
        }
        let mut recap = VisitRecap {
            visit_id: request.visit_id.clone(),
            session_id: visit.session,
            character_id: visit.character,
            title,
            periods,
            fights: vec![],
            boss_fights: vec![],
            boss_fights_limited: false,
            announcements: vec![],
            announcements_limited: false,
            drops: vec![],
            drops_limited: false,
            fame_delta: None,
            best_dps: vec![],
            chain,
        };
        bounded(&recap, MAX_RUN_STORY_BYTES)?;
        let mut bytes = serde_json::to_vec(&recap)?.len();
        let args = params![recap.session_id, scope];
        recap.fights = fight_summaries(&tx, include_str!("visit_fights.sql"), args, &mut bytes)?;
        (recap.drops, recap.drops_limited) =
            run_screen::load_details(&tx, include_str!("visit_drops.sql"), args, &mut bytes)?;
        (recap.announcements, recap.announcements_limited) = run_screen::load_events(
            &tx,
            include_str!("visit_announcements.sql"),
            args,
            &mut bytes,
        )?;
        (recap.boss_fights, recap.boss_fights_limited) =
            run_screen::load_bosses(&tx, include_str!("visit_bosses.sql"), args, &mut bytes)?;
        recap.fame_delta = tx
            .prepare_cached(include_str!("visit_fame.sql"))?
            .query_row(
                params![recap.session_id, scope, recap.character_id],
                |row| row.get(0),
            )?;
        recap.best_dps = records::best_dps(&tx, include_str!("visit_records.sql"), args)?;
        let recap = bound_visit(recap)?;
        tx.commit()?;
        Ok(Some(recap))
    }

    pub fn run_recap(
        &self,
        request: &RunRecapRequest,
        chase_types: &[u16],
    ) -> Result<Option<RunRecap>, StoreError> {
        identifier(&request.run_id)?;
        // All statements share a snapshot even while the writer commits new runs.
        let tx = self.conn.unchecked_transaction()?;
        let run = tx
            .prepare_cached(include_str!("run_recap.sql"))?
            .query_row([&request.run_id], |row| {
                Ok(records::RunFacts {
                    session: row.get(0)?,
                    dungeon: row.get(1)?,
                    started: row.get(2)?,
                    cleared: row.get::<_, Option<bool>>(3)?.unwrap_or(false),
                    clear_micros: row.get(4)?,
                    has_white: row.get(5)?,
                })
            })
            .optional()?;
        let Some(run) = run else {
            return Ok(None);
        };
        let mut recap = records::run_records(&tx, &request.run_id, &run, chase_types)?;
        let visits = journey::session_visits(&tx, &run.session)?;
        if let Some(index) = visits
            .iter()
            .position(|visit| visit.run_id.as_deref() == Some(&request.run_id))
        {
            recap.in_a_row = journey::in_a_row(&visits, index);
            if let Some(range) =
                journey::chain_range(&visits, index).filter(|range| range.end == index + 1)
            {
                recap.chain = Some(journey::chain(&tx, &run.session, &visits, range)?);
            }
        }
        tx.commit()?;
        Ok(Some(recap))
    }
}

/// As with RunStory, retain summaries and signal optional detail omitted by the
/// final serialization budget (including escaping and aggregate expansion).
fn bound_visit(mut recap: VisitRecap) -> Result<VisitRecap, StoreError> {
    loop {
        match bounded(&recap, MAX_RUN_STORY_BYTES) {
            Ok(_) => return Ok(recap),
            Err(StoreError::Limit) if !recap.boss_fights.is_empty() => {
                recap.boss_fights.clear();
                recap.boss_fights_limited = true;
            }
            Err(StoreError::Limit) if !recap.announcements.is_empty() => {
                recap.announcements.clear();
                recap.announcements_limited = true;
            }
            Err(StoreError::Limit) if !recap.drops.is_empty() => {
                recap.drops.clear();
                recap.drops_limited = true;
            }
            Err(error) => return Err(error),
        }
    }
}
