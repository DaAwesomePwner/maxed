use super::*;
use crossbeam_channel::{unbounded, Sender};
use maxed_types::Timestamp;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};

enum Read {
    Frame(Result<Option<RawFrame>, CaptureError>),
    Gate(RawFrame, Sender<()>, Receiver<()>),
}
struct FakeDevice {
    frames: Receiver<Read>,
    drops: Arc<AtomicU32>,
    polls: Arc<AtomicUsize>,
    samples: Arc<AtomicUsize>,
}
impl Device for FakeDevice {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        let result = match self.frames.try_recv() {
            Ok(Read::Frame(result)) => result,
            Ok(Read::Gate(frame, entered, release)) => {
                entered.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(Some(frame))
            }
            Err(_) => Ok(None),
        };
        self.polls.fetch_add(1, Ordering::SeqCst);
        result
    }
    fn dropped_packets(&mut self) -> Result<Option<u32>, CaptureError> {
        let drops = self.drops.load(Ordering::SeqCst);
        self.samples.fetch_add(1, Ordering::SeqCst);
        Ok(Some(drops))
    }
}
struct FakeBackend(Vec<Option<FakeDevice>>);
impl Backend for FakeBackend {
    fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
        Ok((0..self.0.len())
            .map(|i| AdapterInfo {
                id: i.to_string(),
                description: "Synthetic".into(),
            })
            .collect())
    }
    fn open(&mut self, adapter: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        Ok(Box::new(
            self.0[adapter.id.parse::<usize>().unwrap()].take().unwrap(),
        ))
    }
}
struct Control {
    tx: Sender<Read>,
    drops: Arc<AtomicU32>,
    polls: Arc<AtomicUsize>,
    samples: Arc<AtomicUsize>,
}
fn setup(explicit: bool) -> (CaptureHandle, Vec<Control>, Arc<AtomicU64>) {
    let mut controls = Vec::new();
    let devices = (0..2)
        .map(|_| {
            let (tx, frames) = unbounded();
            let drops = Arc::new(AtomicU32::new(0));
            let polls = Arc::new(AtomicUsize::new(0));
            let samples = Arc::new(AtomicUsize::new(0));
            controls.push(Control {
                tx,
                drops: drops.clone(),
                polls: polls.clone(),
                samples: samples.clone(),
            });
            Some(FakeDevice {
                frames,
                drops,
                polls,
                samples,
            })
        })
        .collect();
    let time = Arc::new(AtomicU64::new(0));
    let clock = time.clone();
    let start = Instant::now();
    let handle = start_with_clock(
        CaptureConfig {
            adapter_id: explicit.then(|| "0".into()),
            ..Default::default()
        },
        || Ok(FakeBackend(devices)),
        move || start + Duration::from_millis(clock.load(Ordering::SeqCst)),
    )
    .unwrap();
    (handle, controls, time)
}
fn frame(marker: u64, game: bool) -> RawFrame {
    let mut bytes = vec![0; 54];
    bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    bytes[14] = 0x45;
    bytes[16..18].copy_from_slice(&40u16.to_be_bytes());
    bytes[23] = 6;
    bytes[34..38].copy_from_slice(&[0xc3, 0x50, 8, if game { 2 } else { 3 }]);
    bytes[46] = 0x50;
    bytes[47] = 2;
    RawFrame {
        index: 999,
        at: Timestamp(marker),
        original_len: 54,
        bytes,
    }
}
fn send(control: &Control, marker: u64) {
    control
        .tx
        .send(Read::Frame(Ok(Some(frame(marker, true)))))
        .unwrap();
}
fn receive(handle: &CaptureHandle, marker: u64, index: u64) {
    let got = handle.frames.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!((got.at, got.index), (Timestamp(marker), index));
}
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}
fn idle_rounds(control: &Control) {
    let before = control.polls.load(Ordering::SeqCst);
    wait(|| control.polls.load(Ordering::SeqCst) >= before + 3);
}
#[test]
fn switches_both_ways_with_gap_and_fresh_drop_baselines() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 1);
    receive(&handle, 1, 0);
    wait(|| c[0].samples.load(Ordering::SeqCst) == 1);
    c[1].drops.store(900, Ordering::SeqCst);
    idle_rounds(&c[0]);
    clock.store(4999, Ordering::SeqCst);
    send(&c[1], 2);
    idle_rounds(&c[0]);
    assert!(handle.frames.is_empty());
    clock.store(5000, Ordering::SeqCst);
    wait(|| c[1].samples.load(Ordering::SeqCst) == 1);
    send(&c[1], 3);
    receive(&handle, 3, 2);
    assert_eq!(handle.status().dropped_frames, 1);
    assert_eq!(handle.status().state, CaptureState::Recovering);
    c[1].drops.store(903, Ordering::SeqCst);
    clock.store(5250, Ordering::SeqCst);
    wait(|| handle.status().dropped_frames == 4);
    send(&c[1], 4);
    receive(&handle, 4, 6);
    idle_rounds(&c[1]);
    c[0].drops.store(700, Ordering::SeqCst);
    clock.store(10250, Ordering::SeqCst);
    send(&c[0], 5);
    wait(|| handle.status().dropped_frames == 5);
    send(&c[0], 6);
    receive(&handle, 6, 8);
    idle_rounds(&c[0]);
    clock.store(10500, Ordering::SeqCst);
    idle_rounds(&c[0]);
    assert_eq!(handle.status().dropped_frames, 5);
    send(&c[0], 7);
    receive(&handle, 7, 9);
    handle.stop().unwrap();
}
#[test]
fn explicit_selection_never_switches() {
    let (handle, c, clock) = setup(true);
    send(&c[0], 1);
    receive(&handle, 1, 0);
    clock.store(10000, Ordering::SeqCst);
    idle_rounds(&c[0]);
    assert_eq!(c[1].polls.load(Ordering::SeqCst), 0);
    send(&c[0], 2);
    receive(&handle, 2, 1);
    handle.stop().unwrap();
}
#[test]
fn non_game_candidate_does_not_switch_and_candidate_failure_is_nonfatal() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 1);
    receive(&handle, 1, 0);
    c[1].tx
        .send(Read::Frame(Ok(Some(frame(2, false)))))
        .unwrap();
    clock.store(5000, Ordering::SeqCst);
    idle_rounds(&c[1]);
    assert!(handle.frames.is_empty());
    assert_eq!(handle.status().dropped_frames, 0);
    c[1].tx
        .send(Read::Frame(Err(CaptureError::OpenFailed)))
        .unwrap();
    wait(|| c[1].tx.is_empty());
    idle_rounds(&c[0]);
    let polls = c[1].polls.load(Ordering::SeqCst);
    idle_rounds(&c[0]);
    assert_eq!(c[1].polls.load(Ordering::SeqCst), polls);
    send(&c[0], 3);
    receive(&handle, 3, 1);
    handle.stop().unwrap();
}
#[test]
fn selected_failure_drains_past_non_game_and_requires_fresh_game_evidence() {
    for fresh in [true, false] {
        let (handle, c, clock) = setup(false);
        send(&c[0], 10);
        receive(&handle, 10, 0);
        idle_rounds(&c[0]);
        clock.store(if fresh { 1000 } else { 1 }, Ordering::SeqCst);
        c[1].tx
            .send(Read::Frame(Ok(Some(frame(11, false)))))
            .unwrap();
        send(&c[1], 12);
        c[0].tx
            .send(Read::Frame(Err(CaptureError::OpenFailed)))
            .unwrap();
        if fresh {
            wait(|| handle.status().dropped_frames == 1);
            assert!(
                handle.frames.is_empty(),
                "drained evidence must be discarded"
            );
            send(&c[1], 13);
            receive(&handle, 13, 2);
            handle.stop().unwrap();
        } else {
            wait(|| handle.status().state == CaptureState::Failed);
            assert!(handle.frames.is_empty());
            assert_eq!(handle.stop(), Err(CaptureError::OpenFailed));
        }
    }
}

#[test]
fn candidate_evidence_before_silence_is_discarded_and_cannot_switch() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 1);
    receive(&handle, 1, 0);
    send(&c[1], 9);
    wait(|| c[1].tx.is_empty());
    idle_rounds(&c[1]);
    clock.store(1000, Ordering::SeqCst);
    send(&c[0], 2);
    receive(&handle, 2, 1);
    idle_rounds(&c[0]);
    clock.store(6000, Ordering::SeqCst);
    idle_rounds(&c[1]);
    assert!(handle.frames.is_empty());
    assert_eq!(handle.status().dropped_frames, 0);
    assert_eq!(c[1].samples.load(Ordering::SeqCst), 0);
    handle.stop().unwrap();
}

#[test]
fn mirrored_adapters_drain_during_traffic_and_never_flap_during_pause() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 1);
    receive(&handle, 1, 0);
    for marker in 2..20 {
        clock.store(marker * 100, Ordering::SeqCst);
        send(&c[0], marker);
        receive(&handle, marker, marker - 1);
        idle_rounds(&c[0]);
        clock.store(marker * 100 + 1, Ordering::SeqCst);
        send(&c[1], marker + 1);
        wait(|| c[1].tx.is_empty());
        idle_rounds(&c[1]);
    }
    idle_rounds(&c[0]);
    for time in [7000, 12000, 17000] {
        clock.store(time, Ordering::SeqCst);
        idle_rounds(&c[1]);
        assert!(handle.frames.is_empty());
        assert_eq!(handle.status().dropped_frames, 0);
        assert_eq!(c[1].samples.load(Ordering::SeqCst), 0);
    }
    handle.stop().unwrap();
}

#[test]
fn switched_adapter_discards_equal_and_older_wire_timestamps() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 10);
    receive(&handle, 10, 0);
    idle_rounds(&c[0]);
    clock.store(5000, Ordering::SeqCst);
    send(&c[1], 11);
    wait(|| handle.status().dropped_frames == 1);
    for marker in [1, 10, 9] {
        send(&c[1], marker);
    }
    idle_rounds(&c[1]);
    assert!(handle.frames.is_empty());
    send(&c[1], 12);
    receive(&handle, 12, 2);
    // The first accepted frame ends the guard, even before its timeout.
    send(&c[1], 12);
    receive(&handle, 12, 3);
    send(&c[1], 2);
    receive(&handle, 2, 4);
    idle_rounds(&c[1]);
    clock.store(10000, Ordering::SeqCst);
    send(&c[0], 1);
    wait(|| handle.status().dropped_frames == 2);
    clock.store(11000, Ordering::SeqCst);
    send(&c[0], 1);
    receive(&handle, 1, 6);
    handle.stop().unwrap();
}

#[test]
fn timestamp_guard_expires_after_one_second_without_a_newer_frame() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 100);
    receive(&handle, 100, 0);
    idle_rounds(&c[0]);
    clock.store(5000, Ordering::SeqCst);
    send(&c[1], 1); // Clock stepped backwards; arrival still qualifies.
    wait(|| handle.status().dropped_frames == 1);
    clock.store(5999, Ordering::SeqCst);
    send(&c[1], 2);
    idle_rounds(&c[1]);
    assert!(handle.frames.is_empty());
    clock.store(6000, Ordering::SeqCst);
    send(&c[1], 3);
    receive(&handle, 3, 2);
    send(&c[1], 3);
    receive(&handle, 3, 3);
    send(&c[1], 1);
    receive(&handle, 1, 4);
    assert_eq!(handle.status().dropped_frames, 1);
    handle.stop().unwrap();
}

#[test]
fn candidate_backlog_must_be_drained_across_bounded_passes_before_selection() {
    let (handle, c, clock) = setup(false);
    send(&c[0], 1);
    receive(&handle, 1, 0);
    idle_rounds(&c[0]);
    clock.store(5000, Ordering::SeqCst);
    let (entered, waiting) = bounded(1);
    let (release, gate) = bounded(1);
    c[1].tx
        .send(Read::Gate(frame(2, true), entered, gate))
        .unwrap();
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    // Fill the backlog while the first candidate read is held.
    for marker in 3..10 {
        send(&c[1], marker);
    }
    let (entered_ninth, waiting_ninth) = bounded(1);
    let (release_ninth, gate_ninth) = bounded(1);
    c[1].tx
        .send(Read::Gate(frame(10, true), entered_ninth, gate_ninth))
        .unwrap();
    for marker in 11..19 {
        send(&c[1], marker);
    }
    let selected_polls = c[0].polls.load(Ordering::SeqCst);
    release.send(()).unwrap();
    waiting_ninth.recv_timeout(Duration::from_secs(2)).unwrap();
    // The ninth candidate read is in a new pass: selected got another poll,
    // and a full eight-frame pass without an empty read could not switch.
    assert!(c[0].polls.load(Ordering::SeqCst) > selected_polls);
    assert_eq!(c[1].samples.load(Ordering::SeqCst), 0);
    assert_eq!(handle.status().dropped_frames, 0);
    assert!(handle.frames.is_empty());
    release_ninth.send(()).unwrap();
    wait(|| handle.status().dropped_frames == 1);
    assert!(c[1].tx.is_empty());
    assert!(handle.frames.is_empty());
    send(&c[1], 19);
    receive(&handle, 19, 2);
    handle.stop().unwrap();
}
