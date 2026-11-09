/// Inclusive batches avoid overflow at the last u16 character number.
pub(crate) fn next_batch(progress: Option<u16>) -> Option<std::ops::RangeInclusive<u16>> {
    let start = progress.map_or(Some(0), |n| n.checked_add(1))?;
    Some(start..=start.saturating_add(4095))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_scope_batches_resume_without_overflow() {
        assert_eq!(next_batch(None), Some(0..=4095));
        assert_eq!(next_batch(Some(4095)), Some(4096..=8191));
        assert_eq!(next_batch(Some(65534)), Some(65535..=65535));
        assert_eq!(next_batch(Some(65535)), None);
        let mut progress = None;
        let mut count = 0;
        while let Some(batch) = next_batch(progress) {
            progress = Some(*batch.end());
            count += batch.count();
        }
        assert_eq!(count, 65536);
    }
}
