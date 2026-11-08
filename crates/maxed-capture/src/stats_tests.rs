use super::*;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use tracing::{span, Event, Metadata, Subscriber};

struct Warnings(Arc<AtomicUsize>);
impl Subscriber for Warnings {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, event: &Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}
struct Samples(VecDeque<Result<Option<u32>, CaptureError>>);
impl Device for Samples {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        Ok(None)
    }
    fn dropped_packets(&mut self) -> Result<Option<u32>, CaptureError> {
        self.0.pop_front().unwrap()
    }
}

#[test]
fn stats_warn_once_per_failure_episode() {
    let count = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(count.clone()), || {
        let mut stats_failed = false;
        let mut device = Samples(
            [
                Err(CaptureError::OpenFailed),
                Err(CaptureError::OpenFailed),
                Ok(None),
                Err(CaptureError::OpenFailed),
                Ok(Some(7)),
                Err(CaptureError::OpenFailed),
                Err(CaptureError::OpenFailed),
            ]
            .into(),
        );
        assert_eq!(read_drops(&mut device, &mut stats_failed), None);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        for _ in 0..3 {
            assert_eq!(read_drops(&mut device, &mut stats_failed), None);
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
        assert_eq!(read_drops(&mut device, &mut stats_failed), Some(7));
        for _ in 0..2 {
            assert_eq!(read_drops(&mut device, &mut stats_failed), None);
            assert_eq!(count.load(Ordering::SeqCst), 2);
        }
    });
}
