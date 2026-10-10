//! Explicit synthetic transition rules; never changes the process time zone.
use super::*;
use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime};

#[derive(Clone)]
struct TransitionZone {
    transition: NaiveDateTime,
    before: FixedOffset,
    after: FixedOffset,
}
impl TimeZone for TransitionZone {
    type Offset = FixedOffset;
    fn from_offset(_: &Self::Offset) -> Self {
        unreachable!("fixture only converts through its zone")
    }
    fn offset_from_local_date(&self, date: &NaiveDate) -> LocalResult<Self::Offset> {
        self.offset_from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
    }
    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Self::Offset> {
        let valid = |offset: FixedOffset| {
            let utc = *local - TimeDelta::seconds(i64::from(offset.local_minus_utc()));
            self.offset_from_utc_datetime(&utc) == offset
        };
        match (valid(self.before), valid(self.after)) {
            (true, true) => LocalResult::Ambiguous(self.before, self.after),
            (true, false) => LocalResult::Single(self.before),
            (false, true) => LocalResult::Single(self.after),
            _ => LocalResult::None,
        }
    }
    fn offset_from_utc_date(&self, date: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
    }
    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
        if utc < &self.transition {
            self.before
        } else {
            self.after
        }
    }
}
fn utc(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}
fn stamp(text: &str) -> Timestamp {
    Timestamp(utc(text).timestamp_micros() as u64)
}
fn zone(transition: &str, before: i32, after: i32) -> TransitionZone {
    TransitionZone {
        transition: utc(transition).naive_utc(),
        before: FixedOffset::east_opt(before * 3600).unwrap(),
        after: FixedOffset::east_opt(after * 3600).unwrap(),
    }
}

#[test]
fn seven_days_resolves_both_midnights_across_dst() {
    for (zone, event, start, end, hours) in [
        (
            zone("2026-03-08T08:00:00Z", -6, -5),
            "2026-03-10T00:00:00Z",
            "2026-03-03T06:00:00Z",
            "2026-03-10T05:00:00Z",
            167,
        ),
        (
            zone("2026-11-01T07:00:00Z", -5, -6),
            "2026-11-03T00:00:00Z",
            "2026-10-27T05:00:00Z",
            "2026-11-03T06:00:00Z",
            169,
        ),
    ] {
        let window = seven_day_window(&zone, stamp(event)).unwrap();
        assert_eq!(
            window,
            TimeRange {
                start: stamp(start),
                end: stamp(end)
            }
        );
        assert_eq!(window.end.0 - window.start.0, hours * 3_600_000_000);
    }
}

#[test]
fn ambiguous_midnight_is_earliest_and_missing_midnight_is_unavailable() {
    let repeated = zone("2026-11-01T01:00:00Z", 0, -1);
    let window = seven_day_window(&repeated, stamp("2026-11-07T12:00:00Z")).unwrap();
    assert_eq!(window.start, stamp("2026-11-01T00:00:00Z"));
    // This offset jump skips the entire local March 8, not just midnight.
    let missing = zone("2026-03-08T12:00:00Z", -12, 12);
    assert!(seven_day_window(&missing, stamp("2026-03-14T00:00:00Z")).is_err());
    assert!(seven_day_window(&missing, stamp("2026-03-07T18:00:00Z")).is_err());
    assert!(seven_day_window(&Utc, Timestamp(u64::MAX)).is_err());
    assert!(seven_day_window(&Utc, Timestamp(i64::MAX as u64)).is_err());
    assert!(seven_day_window(&Utc, Timestamp(0)).is_err());
}

#[test]
fn midnight_gap_resolves_to_first_valid_instant_on_same_date() {
    for seconds in [37, 3600] {
        let missing_midnight = TransitionZone {
            transition: utc("2026-03-08T00:00:00Z").naive_utc(),
            before: FixedOffset::east_opt(0).unwrap(),
            after: FixedOffset::east_opt(seconds).unwrap(),
        };
        let boundary = stamp("2026-03-08T00:00:00Z");
        let window = seven_day_window(&missing_midnight, stamp("2026-03-14T12:00:00Z")).unwrap();
        assert_eq!(window.start, boundary);
        let previous = seven_day_window(&missing_midnight, Timestamp(boundary.0 - 1)).unwrap();
        assert_eq!(previous.end, boundary);
        // Prove this is the first representable local second, including a gap
        // whose endpoint is not aligned to a whole minute.
        let midnight = utc("2026-03-08T00:00:00Z").naive_utc();
        assert!(missing_midnight
            .from_local_datetime(&(midnight + TimeDelta::seconds(i64::from(seconds) - 1)))
            .earliest()
            .is_none());
        assert_eq!(
            missing_midnight
                .from_local_datetime(&(midnight + TimeDelta::seconds(i64::from(seconds))))
                .earliest()
                .unwrap()
                .timestamp_micros() as u64,
            boundary.0
        );
    }
}

#[test]
fn ordinary_dates_use_event_day_and_microsecond_clock() {
    let at = stamp("2001-05-07T23:59:59.999999Z");
    assert_eq!(
        seven_day_window(&Utc, at).unwrap(),
        TimeRange {
            start: stamp("2001-05-01T00:00:00Z"),
            end: stamp("2001-05-08T00:00:00Z"),
        }
    );
}
