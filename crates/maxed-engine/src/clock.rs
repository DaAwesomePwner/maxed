use maxed_types::Timestamp;

pub(crate) fn monotonic(previous: u64, incoming: u64) -> (u64, bool) {
    (previous.max(incoming), incoming < previous)
}

pub fn idle_deadline(last: Timestamp) -> Timestamp {
    Timestamp(last.0.saturating_add(1_800_000_000))
}

pub(crate) fn millis(at: Timestamp) -> Timestamp {
    Timestamp(at.0 / 1_000 * 1_000)
}

pub fn dps(damage: f64, first: Timestamp, last: Timestamp) -> Option<f64> {
    let elapsed = last.0.checked_sub(first.0)?;
    if elapsed == 0 || !damage.is_finite() {
        return None;
    }
    let result = damage * 1_000_000.0 / elapsed as f64;
    result.is_finite().then_some(result)
}

pub fn share(damage: f64, total: f64) -> Option<f64> {
    let result = damage / total;
    (damage.is_finite() && total.is_finite() && total > 0.0 && result.is_finite()).then_some(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn regression() {
        assert_eq!(super::monotonic(20, 19), (20, true));
        assert_eq!(super::monotonic(20, 21), (21, false));
    }
}
