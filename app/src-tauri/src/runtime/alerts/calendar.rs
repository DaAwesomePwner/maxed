//! Resolve each local midnight using the zone's rules at that date.
use super::AlertCalendar;
use chrono::{DateTime, Days, Local, Offset, TimeDelta, TimeZone, Utc};
use maxed_types::{TimeRange, Timestamp};

const UNAVAILABLE: &str = "Local calendar context is unavailable";

pub struct LocalAlertCalendar;

impl AlertCalendar for LocalAlertCalendar {
    fn seven_day_window(&self, at: Timestamp) -> Result<TimeRange, String> {
        // Chrono's Local UTC lookup unwraps a failed Windows zone lookup.
        // Keep that dependency failure off the capture pipeline's error path.
        std::panic::catch_unwind(|| seven_day_window(&Local, at))
            .unwrap_or_else(|_| Err(UNAVAILABLE.into()))
    }
}

pub(super) fn seven_day_window<T: TimeZone>(zone: &T, at: Timestamp) -> Result<TimeRange, String> {
    let micros = i64::try_from(at.0).map_err(|_| UNAVAILABLE)?;
    let utc = DateTime::<Utc>::from_timestamp_micros(micros).ok_or(UNAVAILABLE)?;
    let offset = zone.offset_from_utc_datetime(&utc.naive_utc()).fix();
    let date = utc
        .naive_utc()
        .checked_add_signed(TimeDelta::seconds(i64::from(offset.local_minus_utc())))
        .ok_or(UNAVAILABLE)?
        .date();
    let start = date.checked_sub_days(Days::new(6)).ok_or(UNAVAILABLE)?;
    let end = date.checked_add_days(Days::new(1)).ok_or(UNAVAILABLE)?;
    let midnight = |date: chrono::NaiveDate| -> Result<Timestamp, String> {
        let local = date.and_hms_opt(0, 0, 0).ok_or(UNAVAILABLE)?;
        // Platform zone transitions have whole-second precision. Usually this
        // resolves midnight in one lookup. In a gap, examine the remaining
        // seconds of this date in order, without assuming a one-hour jump or
        // skipping short valid intervals. Never cross into the next date.
        // Ambiguous local times always choose their earlier instant.
        let instant = (0..86_400)
            .find_map(|seconds| {
                local
                    .checked_add_signed(TimeDelta::seconds(seconds))
                    .and_then(|candidate| zone.from_local_datetime(&candidate).earliest())
            })
            .ok_or(UNAVAILABLE)?;
        let micros = u64::try_from(instant.timestamp_micros()).map_err(|_| UNAVAILABLE)?;
        Ok(Timestamp(micros))
    };
    let window = TimeRange {
        start: midnight(start)?,
        end: midnight(end)?,
    };
    if window.start > at || at >= window.end {
        return Err(UNAVAILABLE.into());
    }
    Ok(window)
}

#[cfg(test)]
mod alerts_history;
