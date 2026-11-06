#![cfg(windows)]

use maxed_recording::{
    recording_queue, PcapReader, RecordingFailure, RecordingInput, RecordingOwner, RecordingStatus,
    FILE_HEADER_BYTES, FRAME_HEADER_BYTES, MAX_FRAME_BYTES, MAX_QUOTA_BYTES, PCAP_HEADER,
    QUEUE_BYTE_CAPACITY, QUEUE_CAPACITY, SEGMENT_TARGET_BYTES,
};
use maxed_types::{RawFrame, Timestamp};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn quota_never_deletes_active_or_foreign_files() {
    let temp = Temp::new();
    let foreign = temp.seed(3, "pcap", 100);
    fs::remove_file(foreign.with_extension("owner")).unwrap();
    let (_, pending) = recording_queue();
    assert!(RecordingOwner::open(temp.root(), 41, pending).is_err());
    assert!(foreign.exists(), "a reserved filename is not ownership");
}

#[test]
fn recording_lease_and_crash_recovery() {
    let temp = Temp::new();
    let (input, mut owner) = temp.open(100);
    send(&input, &mut owner, 1);
    drop(owner);
    let path = segments(&temp.root(), "partial")[0].clone();
    fs::write(&path, b"ambiguous interrupted bytes").unwrap();
    let (_, pending) = recording_queue();
    assert!(RecordingOwner::open(temp.root(), 41, pending).is_err());
    assert_eq!(fs::read(path).unwrap(), b"ambiguous interrupted bytes");
}

#[test]
fn quota_oldest_first_exact_boundaries() {
    for cap in [49, 50, 51] {
        let temp = Temp::new();
        let (input, mut owner) = temp.open(cap);
        input.try_record(&frame(10)).unwrap();
        assert_eq!(owner.process_next().is_ok(), cap >= 50);
        assert!(owner.accounted_bytes() <= cap);
    }
}

#[test]
fn external_growth_blocks_finalization_even_without_eviction() {
    let temp = Temp::new();
    let (input, mut owner) = temp.open(1000);
    send(&input, &mut owner, 1);
    owner.finish().unwrap();
    let closed = segments(&temp.root(), "pcap")[0].clone();
    let (input, mut owner) = temp.open(1000);
    OpenOptions::new()
        .append(true)
        .open(&closed)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    send(&input, &mut owner, 1);
    assert_eq!(owner.finish(), Err(RecordingFailure::Io));
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "maxed-s3-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("recordings")
    }
    fn open(&self, limit: u64) -> (RecordingInput, RecordingOwner) {
        let (input, pending) = recording_queue();
        let owner = RecordingOwner::open(self.root(), limit, pending).unwrap();
        (input, owner)
    }
    fn seed(&self, id: u64, extension: &str, size: u64) -> PathBuf {
        fs::create_dir_all(self.root()).unwrap();
        let path = self
            .root()
            .join(format!("maxed-v2-{id:020}-{id:032x}.{extension}"));
        let mut file = File::create(&path).unwrap();
        file.write_all(&PCAP_HEADER).unwrap();
        if size >= 40 {
            let payload = size - 40;
            file.write_all(&0u32.to_le_bytes()).unwrap();
            file.write_all(&0u32.to_le_bytes()).unwrap();
            file.write_all(&(payload as u32).to_le_bytes()).unwrap();
            file.write_all(&(payload as u32).to_le_bytes()).unwrap();
        }
        file.set_len(size).unwrap();
        let created = file.metadata().unwrap().creation_time();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        fs::write(
            path.with_extension("owner"),
            format!("MAXED-RECORDING-2\n{stem}\n{created}\n"),
        )
        .unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn frame(size: usize) -> RawFrame {
    RawFrame {
        index: 0,
        at: Timestamp(1_234_567),
        original_len: size as u32,
        bytes: vec![42; size],
    }
}
fn send(input: &RecordingInput, owner: &mut RecordingOwner, size: usize) {
    input.try_record(&frame(size)).unwrap();
    assert!(owner.process_next().unwrap());
}
fn segments(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == extension))
        .collect();
    paths.sort();
    paths
}

#[test]
fn accounts_actual_maximum_and_both_headers() {
    let temp = Temp::new();
    let expected = FILE_HEADER_BYTES + FRAME_HEADER_BYTES + MAX_FRAME_BYTES as u64;
    assert_eq!(expected, 65_575);
    let (input, mut owner) = temp.open(expected);
    send(&input, &mut owner, MAX_FRAME_BYTES);
    assert_eq!(owner.accounted_bytes(), expected);
    owner.finish().unwrap();
    let bytes = fs::read(&segments(&temp.root(), "pcap")[0]).unwrap();
    assert_eq!(bytes.len() as u64, expected);
    let mut reader = PcapReader::new(bytes.as_slice()).unwrap();
    let got = reader.next_frame().unwrap().unwrap();
    assert_eq!(got.bytes, frame(MAX_FRAME_BYTES).bytes);
    assert!(reader.next_frame().unwrap().is_none());
    assert_eq!(input.monitor().status(), RecordingStatus::Stopped);
}

#[test]
fn real_64_million_byte_segment_boundary_rolls_before_the_next_frame() {
    let temp = Temp::new();
    let (input, mut owner) = temp.open(2 * SEGMENT_TARGET_BYTES);
    let frame_bytes = FRAME_HEADER_BYTES + MAX_FRAME_BYTES as u64;
    let count = (SEGMENT_TARGET_BYTES - FILE_HEADER_BYTES) / frame_bytes;
    for _ in 0..count {
        send(&input, &mut owner, MAX_FRAME_BYTES);
    }
    let filled = FILE_HEADER_BYTES + count * frame_bytes;
    assert_eq!(segments(&temp.root(), "partial").len(), 1);
    assert!(segments(&temp.root(), "pcap").is_empty());
    send(&input, &mut owner, MAX_FRAME_BYTES);
    let closed = segments(&temp.root(), "pcap");
    assert_eq!(closed.len(), 1);
    assert_eq!(fs::metadata(&closed[0]).unwrap().len(), filled);
    assert!(filled <= SEGMENT_TARGET_BYTES);
    assert_eq!(
        owner.accounted_bytes(),
        filled + FILE_HEADER_BYTES + frame_bytes
    );
    owner.finish().unwrap();
}

#[test]
fn oldest_closed_and_recoverable_count_toward_admission_active_is_never_deleted() {
    let temp = Temp::new();
    let old = temp.seed(4, "pcap", 70);
    let recovered = temp.seed(5, "partial", 80);
    let newest = temp.seed(6, "pcap", 30);
    let (input, mut owner) = temp.open(200);
    assert_eq!(owner.accounted_bytes(), 180);
    send(&input, &mut owner, 40); // 80 bytes needs eviction of the oldest 70.
    assert!(!old.exists());
    assert!(recovered.with_extension("interrupted").exists());
    assert!(newest.exists());
    assert_eq!(owner.accounted_bytes(), 190);
    // Remaining quota was 90 when opening: rollover before growing past it.
    send(&input, &mut owner, 1);
    assert!(!recovered.exists());
    assert!(newest.exists());
    let closed = segments(&temp.root(), "pcap");
    assert_eq!(closed.len(), 2);
    assert_eq!(fs::metadata(&closed[1]).unwrap().len(), 80);
    assert_eq!(owner.accounted_bytes(), 151);
    owner.finish().unwrap();
}

#[test]
fn quota_smaller_than_segment_target_rotates_without_exceeding_cap() {
    let temp = Temp::new();
    let (input, mut owner) = temp.open(100);
    for _ in 0..4 {
        send(&input, &mut owner, 60); // Exactly 100 bytes per segment.
        assert_eq!(owner.accounted_bytes(), 100);
        assert_eq!(segments(&temp.root(), "partial").len(), 1);
        assert!(segments(&temp.root(), "pcap").is_empty());
    }
    owner.finish().unwrap();
    assert_eq!(segments(&temp.root(), "pcap").len(), 1);
}

#[test]
fn too_small_frame_limit_is_visible_and_does_not_delete_existing_data() {
    for limit in [41, 65_574] {
        let temp = Temp::new();
        let (input, mut owner) = temp.open(limit);
        input.try_record(&frame(MAX_FRAME_BYTES)).unwrap();
        assert_eq!(owner.process_next(), Err(RecordingFailure::LimitTooSmall));
        assert_eq!(
            input.monitor().status(),
            RecordingStatus::Failed(RecordingFailure::LimitTooSmall)
        );
        assert!(segments(&temp.root(), "partial").is_empty());
    }
    let temp = Temp::new();
    let (input, mut owner) = temp.open(60);
    send(&input, &mut owner, 10);
    let active = segments(&temp.root(), "partial")[0].clone();
    input.try_record(&frame(100)).unwrap();
    assert_eq!(owner.process_next(), Err(RecordingFailure::LimitTooSmall));
    assert!(active.exists());
    assert_eq!(owner.accounted_bytes(), 50);
}

#[test]
fn two_separate_leases_in_one_process_exclude_recording_and_cleanup() {
    let temp = Temp::new();
    let old = temp.seed(1, "pcap", 100);
    let (input, mut owner) = temp.open(200);
    send(&input, &mut owner, 10);
    let (_, pending) = recording_queue();
    assert!(matches!(
        RecordingOwner::open(temp.root(), 41, pending),
        Err(RecordingFailure::Lease)
    ));
    assert!(old.exists());
    let lease_path = temp.root().join(".maxed-recording-v1.lease");
    assert!(fs::remove_file(&lease_path).is_err());
    let active = segments(&temp.root(), "partial")[0].clone();
    assert!(fs::remove_file(&active).is_err());
    owner.finish().unwrap();
    let (_, pending) = recording_queue();
    let next = RecordingOwner::open(temp.root(), 41, pending).unwrap();
    assert!(!old.exists());
    assert_eq!(next.accounted_bytes(), 0);
}

#[test]
fn cleanup_uses_exact_owned_names_only_and_leaves_foreign_files_and_directories() {
    let temp = Temp::new();
    let old = temp.seed(1, "pcap", 100);
    for name in [
        "foreign.pcap",
        "maxed-v1-1.pcap",
        "maxed-v1-00000000000000000002.pcap.bak",
    ] {
        fs::write(temp.root().join(name), b"foreign").unwrap();
    }
    fs::create_dir(temp.root().join("foreign-directory")).unwrap();
    let (_, pending) = recording_queue();
    assert!(RecordingOwner::open(temp.root(), 41, pending).is_err());
    assert!(old.exists());
    assert_eq!(fs::read_dir(temp.root()).unwrap().count(), 7);
}

#[test]
fn crash_like_drop_recovers_exact_disk_bytes_on_next_lease() {
    let temp = Temp::new();
    let (input, mut owner) = temp.open(100);
    send(&input, &mut owner, 10);
    drop(owner);
    let active = segments(&temp.root(), "partial")[0].clone();
    assert_eq!(fs::metadata(&active).unwrap().len(), 50);
    let (input, mut next) = temp.open(100);
    assert_eq!(next.accounted_bytes(), 50);
    send(&input, &mut next, 40);
    assert!(!active.exists());
    assert_eq!(next.accounted_bytes(), 80);
    next.finish().unwrap();
}

#[test]
fn burst_drops_frames_without_stopping_and_resumes_after_drain() {
    let temp = Temp::new();
    let (input, mut owner) = temp.open(MAX_QUOTA_BYTES);
    let monitor = input.monitor();
    let admitted = QUEUE_BYTE_CAPACITY / MAX_FRAME_BYTES;
    // The disk worker is deliberately not driven during this 5,000-frame burst.
    for _ in 0..5000 {
        input.try_record(&frame(MAX_FRAME_BYTES)).unwrap();
        assert!(monitor.queued_bytes() <= QUEUE_BYTE_CAPACITY);
    }
    assert_eq!(monitor.queued_bytes(), admitted * MAX_FRAME_BYTES);
    assert_eq!(monitor.dropped_frames(), (5000 - admitted) as u64);
    assert_eq!(monitor.status(), RecordingStatus::Recording);
    for _ in 0..admitted {
        assert!(owner.process_next().unwrap());
    }
    assert_eq!(monitor.queued_bytes(), 0);
    input.try_record(&frame(17)).unwrap();
    drop(input);
    owner.run().unwrap();
    assert_eq!(monitor.status(), RecordingStatus::Stopped);
    assert_eq!(monitor.dropped_frames(), (5000 - admitted) as u64);
    let bytes = fs::read(&segments(&temp.root(), "pcap")[0]).unwrap();
    let mut reader = PcapReader::new(bytes.as_slice()).unwrap();
    for _ in 0..admitted {
        assert_eq!(
            reader.next_frame().unwrap().unwrap().bytes.len(),
            MAX_FRAME_BYTES
        );
    }
    assert_eq!(reader.next_frame().unwrap().unwrap().bytes.len(), 17);
    assert!(reader.next_frame().unwrap().is_none());
}

#[test]
fn delete_failure_preserves_file_and_reports_recording_only_failure() {
    let temp = Temp::new();
    let old = temp.seed(0, "pcap", 100);
    let (input, mut owner) = temp.open(100);
    let held = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&old)
        .unwrap();
    input.try_record(&frame(1)).unwrap();
    assert_eq!(owner.process_next(), Err(RecordingFailure::Io));
    assert_eq!(
        input.monitor().status(),
        RecordingStatus::Failed(RecordingFailure::Io)
    );
    assert!(old.exists());
    assert!(segments(&temp.root(), "partial").is_empty());
    drop(held);
}

#[test]
fn invalid_frames_fail_before_queue_allocation_or_file_creation() {
    for bad in [
        frame(MAX_FRAME_BYTES + 1),
        RawFrame {
            original_len: 0,
            ..frame(1)
        },
        RawFrame {
            at: Timestamp((u64::from(u32::MAX) + 1) * 1_000_000),
            ..frame(1)
        },
    ] {
        let temp = Temp::new();
        let (input, mut owner) = temp.open(MAX_QUOTA_BYTES);
        assert_eq!(input.try_record(&bad), Err(RecordingFailure::InvalidFrame));
        assert_eq!(owner.process_next(), Err(RecordingFailure::InvalidFrame));
        assert_eq!(owner.accounted_bytes(), 0);
    }
}

#[test]
fn worker_drains_and_syncs_after_all_inputs_disconnect() {
    let temp = Temp::new();
    let (input, owner) = temp.open(1000);
    let monitor = input.monitor();
    input.try_record(&frame(10)).unwrap();
    input.try_record(&frame(20)).unwrap();
    drop(input);
    std::thread::spawn(move || owner.run())
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(monitor.status(), RecordingStatus::Stopped);
    assert!(segments(&temp.root(), "partial").is_empty());
    assert_eq!(
        fs::metadata(&segments(&temp.root(), "pcap")[0])
            .unwrap()
            .len(),
        86
    );
}

#[test]
fn invalid_locations_and_limits_publish_initialization_failure() {
    let temp = Temp::new();
    for path in [
        PathBuf::from("recordings"),
        temp.0.join("wrong-child"),
        temp.0.join("..\\recordings"),
    ] {
        let (input, pending) = recording_queue();
        assert!(matches!(
            RecordingOwner::open(path, 100, pending),
            Err(RecordingFailure::Location)
        ));
        assert_eq!(
            input.monitor().status(),
            RecordingStatus::Failed(RecordingFailure::Location)
        );
    }
    for limit in [0, MAX_QUOTA_BYTES + 1] {
        let (input, pending) = recording_queue();
        assert!(matches!(
            RecordingOwner::open(temp.root(), limit, pending),
            Err(RecordingFailure::LimitTooSmall)
        ));
        assert_eq!(
            input.monitor().status(),
            RecordingStatus::Failed(RecordingFailure::LimitTooSmall)
        );
    }
}

#[test]
fn reserved_directory_is_never_treated_as_owned_closed_file() {
    let temp = Temp::new();
    let suspicious = temp.root().join("maxed-v1-00000000000000000000.pcap");
    fs::create_dir_all(&suspicious).unwrap();
    let (_, pending) = recording_queue();
    assert!(matches!(
        RecordingOwner::open(temp.root(), 41, pending),
        Err(RecordingFailure::Location)
    ));
    assert!(suspicious.is_dir());
}

#[test]
fn invalid_input_wakes_an_idle_worker_and_releases_the_lease() {
    let temp = Temp::new();
    let (input, owner) = temp.open(100);
    let (done, result) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        done.send(owner.run()).unwrap();
    });
    assert_eq!(
        input.try_record(&frame(MAX_FRAME_BYTES + 1)),
        Err(RecordingFailure::InvalidFrame)
    );
    assert_eq!(
        result
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        Err(RecordingFailure::InvalidFrame)
    );
    worker.join().unwrap();
    let (_input, owner) = temp.open(100);
    owner.finish().unwrap();
}

#[test]
fn active_creation_failure_cannot_overwrite_an_existing_entry() {
    use maxed_recording::{RecordingDisk, SystemRecordingDisk};
    use std::sync::{Arc, Mutex};
    struct Collision(Mutex<Option<PathBuf>>);
    impl RecordingDisk for Collision {
        fn create_segment(&self, path: &Path) -> std::io::Result<File> {
            // Race in a file after inventory, immediately before create_new.
            fs::write(path, b"unrelated late file")?;
            *self.0.lock().unwrap() = Some(path.into());
            SystemRecordingDisk.create_segment(path)
        }
    }
    let temp = Temp::new();
    let disk = Arc::new(Collision(Mutex::new(None)));
    let (input, pending) = recording_queue();
    let mut owner =
        RecordingOwner::open_with_disk(temp.root(), 100, pending, disk.clone()).unwrap();
    input.try_record(&frame(10)).unwrap();
    assert_eq!(owner.process_next(), Err(RecordingFailure::Io));
    let collision = disk.0.lock().unwrap().clone().unwrap();
    assert_eq!(fs::read(&collision).unwrap(), b"unrelated late file");
    assert!(!collision.with_extension("owner").exists());
    assert_eq!(
        input.monitor().status(),
        RecordingStatus::Failed(RecordingFailure::Io)
    );
}

#[test]
fn changed_closed_file_stops_cleanup_without_deleting_it() {
    let temp = Temp::new();
    let old = temp.seed(0, "pcap", 100);
    let (input, mut owner) = temp.open(100);
    File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_len(101)
        .unwrap();
    input.try_record(&frame(1)).unwrap();
    assert_eq!(owner.process_next(), Err(RecordingFailure::Io));
    assert_eq!(fs::metadata(old).unwrap().len(), 101);
}

#[test]
fn inventory_bound_fails_without_cleanup_or_unbounded_collection() {
    let temp = Temp::new();
    for id in 0..4097 {
        temp.seed(id, "pcap", 24);
    }
    let (input, pending) = recording_queue();
    assert!(matches!(
        RecordingOwner::open(temp.root(), 41, pending),
        Err(RecordingFailure::Inventory)
    ));
    assert_eq!(
        input.monitor().status(),
        RecordingStatus::Failed(RecordingFailure::Inventory)
    );
    assert_eq!(segments(&temp.root(), "pcap").len(), 4097);
}

#[test]
fn junction_root_and_reserved_entry_are_rejected_without_following_them() {
    let temp = Temp::new();
    let target = temp.0.join("junction-target");
    fs::create_dir(&target).unwrap();
    let sentinel = target.join("sentinel");
    fs::write(&sentinel, b"untouched").unwrap();
    let junction = |path: &Path| {
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(path)
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction creation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    junction(&temp.root());
    let (_, pending) = recording_queue();
    assert!(matches!(
        RecordingOwner::open(temp.root(), 41, pending),
        Err(RecordingFailure::Location)
    ));
    assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
    fs::remove_dir(temp.root()).unwrap();
    fs::create_dir(temp.root()).unwrap();
    let entry = temp.root().join("maxed-v1-00000000000000000000.active");
    junction(&entry);
    let (_, pending) = recording_queue();
    assert!(matches!(
        RecordingOwner::open(temp.root(), 41, pending),
        Err(RecordingFailure::Location)
    ));
    assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
    fs::remove_dir(entry).unwrap();
}

#[test]
fn frame_count_bound_drops_before_open_and_finalizes_admitted_frames() {
    let temp = Temp::new();
    let (input, pending) = recording_queue();
    for _ in 0..QUEUE_CAPACITY {
        input.try_record(&frame(1)).unwrap();
    }
    input.try_record(&frame(1)).unwrap();
    assert_eq!(input.monitor().dropped_frames(), 1);
    assert_eq!(input.monitor().queued_bytes(), QUEUE_CAPACITY);
    assert_eq!(input.monitor().status(), RecordingStatus::Recording);
    let monitor = input.monitor();
    drop(input);
    let owner = RecordingOwner::open(temp.root(), 1_000_000, pending).unwrap();
    owner.run().unwrap();
    assert_eq!(monitor.status(), RecordingStatus::Stopped);
    assert!(segments(&temp.root(), "partial").is_empty());
    let bytes = fs::read(&segments(&temp.root(), "pcap")[0]).unwrap();
    let mut reader = PcapReader::new(bytes.as_slice()).unwrap();
    for _ in 0..QUEUE_CAPACITY {
        assert_eq!(reader.next_frame().unwrap().unwrap().bytes, frame(1).bytes);
    }
    assert!(reader.next_frame().unwrap().is_none());
}

#[test]
fn queue_full_drain_reports_delete_failure_instead_of_finalized_prefix() {
    let temp = Temp::new();
    let old = temp.seed(0, "pcap", 100);
    let (input, owner) = temp.open(100);
    let held = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&old)
        .unwrap();
    for _ in 0..QUEUE_CAPACITY {
        input.try_record(&frame(1)).unwrap();
    }
    input.try_record(&frame(1)).unwrap();
    assert_eq!(input.monitor().dropped_frames(), 1);
    assert_eq!(input.monitor().queued_bytes(), QUEUE_CAPACITY);
    assert_eq!(input.monitor().status(), RecordingStatus::Recording);
    let monitor = input.monitor();
    drop(input);
    assert_eq!(owner.run(), Err(RecordingFailure::Io));
    assert_eq!(
        monitor.status(),
        RecordingStatus::Failed(RecordingFailure::Io)
    );
    assert!(old.exists());
    assert!(segments(&temp.root(), "partial").is_empty());
    drop(held);
}

#[test]
fn queue_full_finish_reports_rename_failure_and_retains_recoverable_prefix() {
    use maxed_recording::RecordingDisk;
    use std::sync::Arc;
    struct RenameFailure(AtomicU64);
    impl RecordingDisk for RenameFailure {
        fn rename(&self, _: &Path, _: &Path) -> std::io::Result<()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(std::io::Error::from_raw_os_error(5))
        }
    }
    let temp = Temp::new();
    let disk = Arc::new(RenameFailure(AtomicU64::new(0)));
    let (input, pending) = recording_queue();
    let owner =
        RecordingOwner::open_with_disk(temp.root(), 1_000_000, pending, disk.clone()).unwrap();
    for _ in 0..QUEUE_CAPACITY {
        input.try_record(&frame(1)).unwrap();
    }
    input.try_record(&frame(1)).unwrap();
    assert_eq!(input.monitor().dropped_frames(), 1);
    assert_eq!(input.monitor().queued_bytes(), QUEUE_CAPACITY);
    assert_eq!(input.monitor().status(), RecordingStatus::Recording);
    let monitor = input.monitor();
    drop(input);
    assert_eq!(owner.run(), Err(RecordingFailure::Io));
    assert_eq!(
        disk.0.load(Ordering::Relaxed),
        1,
        "final rename was attempted"
    );
    assert_eq!(
        monitor.status(),
        RecordingStatus::Failed(RecordingFailure::Io)
    );
    assert!(segments(&temp.root(), "pcap").is_empty());
    let partial = segments(&temp.root(), "partial")[0].clone();
    let mut reader = PcapReader::new(File::open(&partial).unwrap()).unwrap();
    for _ in 0..QUEUE_CAPACITY {
        assert_eq!(reader.next_frame().unwrap().unwrap().bytes, frame(1).bytes);
    }
    assert!(reader.next_frame().unwrap().is_none());
    drop(reader);
    let (_input, owner) = temp.open(1_000_000);
    owner.finish().unwrap();
    assert!(partial.with_extension("interrupted").exists());
}

#[test]
fn unrecordable_limits_preserve_all_existing_recordings_before_inventory_cleanup() {
    let temp = Temp::new();
    let closed = temp.seed(0, "pcap", 100);
    let recoverable = temp.seed(1, "partial", 80);
    let closed_bytes = fs::read(&closed).unwrap();
    let recoverable_bytes = fs::read(&recoverable).unwrap();
    let minimum = FILE_HEADER_BYTES + FRAME_HEADER_BYTES + 1;
    for limit in 1..minimum {
        let (input, pending) = recording_queue();
        assert!(matches!(
            RecordingOwner::open(temp.root(), limit, pending),
            Err(RecordingFailure::LimitTooSmall)
        ));
        assert_eq!(
            input.monitor().status(),
            RecordingStatus::Failed(RecordingFailure::LimitTooSmall)
        );
        assert_eq!(fs::read(&closed).unwrap(), closed_bytes);
        assert_eq!(fs::read(&recoverable).unwrap(), recoverable_bytes);
        assert!(!temp.root().join(".maxed-recording-v1.lease").exists());
    }
    let (input, mut owner) = temp.open(minimum);
    send(&input, &mut owner, 1);
    assert_eq!(owner.accounted_bytes(), minimum);
    owner.finish().unwrap();
}

#[test]
fn sidecar_size_identity_and_orphans_block_before_cleanup() {
    for mode in 0..4 {
        let temp = Temp::new();
        let path = temp.seed(0, "pcap", 100);
        let sidecar = path.with_extension("owner");
        match mode {
            0 => fs::write(&sidecar, vec![b'x'; 4097]).unwrap(),
            1 => fs::write(&sidecar, b"untrusted").unwrap(),
            2 => fs::remove_file(&path).unwrap(),
            _ => {
                fs::rename(&sidecar, sidecar.with_extension("wrong")).unwrap();
            }
        }
        let (_, pending) = recording_queue();
        assert!(RecordingOwner::open(temp.root(), 41, pending).is_err());
        assert_eq!(path.exists(), mode != 2);
    }
}

#[test]
fn recovery_is_bounded_and_retains_truncated_prefixes() {
    for size in [0, 23, 25, SEGMENT_TARGET_BYTES + 1] {
        let temp = Temp::new();
        let path = temp.seed(0, "partial", size);
        let (_, pending) = recording_queue();
        assert!(RecordingOwner::open(temp.root(), MAX_QUOTA_BYTES, pending).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), size);
    }
}

#[test]
fn recording_namespace_lease_cross_process() {
    let temp = Temp::new();
    let (_input, owner) = temp.open(100);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "owner::recording_namespace_child_probe",
            "--nocapture",
        ])
        .env("MAXED_A3_SYNTHETIC_LEASE_CHILD", temp.root())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
        "child lease probe was not selected: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    owner.finish().unwrap();
    let (_input, owner) = temp.open(100);
    owner.finish().unwrap();
}

#[test]
fn recording_namespace_child_probe() {
    let Some(root) = std::env::var_os("MAXED_A3_SYNTHETIC_LEASE_CHILD") else {
        return;
    };
    let (_, pending) = recording_queue();
    assert!(matches!(
        RecordingOwner::open(root.into(), 100, pending),
        Err(RecordingFailure::Lease)
    ));
}

struct FailingDisk {
    phase: u8,
}
impl maxed_recording::RecordingDisk for FailingDisk {
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<usize> {
        if self.phase == 3 && file.metadata()?.len() >= FILE_HEADER_BYTES {
            if file.metadata()?.len() == FILE_HEADER_BYTES {
                return file.write(&bytes[..5.min(bytes.len())]);
            }
            return Err(std::io::Error::from_raw_os_error(112));
        }
        if self.phase == 0 {
            return Err(std::io::Error::from_raw_os_error(112));
        }
        file.write(bytes)
    }
    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        if self.phase == 1 {
            return Err(std::io::Error::from_raw_os_error(29));
        }
        file.flush()
    }
    fn sync(&self, file: &File) -> std::io::Result<()> {
        if self.phase == 2 {
            return Err(std::io::Error::from_raw_os_error(1117));
        }
        file.sync_all()
    }
}
#[test]
fn disk_full_flush_and_sync_failures_preserve_partial_and_release_lease() {
    for phase in 0..4 {
        let temp = Temp::new();
        let (input, pending) = recording_queue();
        let mut owner = RecordingOwner::open_with_disk(
            temp.root(),
            100,
            pending,
            std::sync::Arc::new(FailingDisk { phase }),
        )
        .unwrap();
        input.try_record(&frame(1)).unwrap();
        if phase == 0 || phase == 3 {
            assert_eq!(owner.process_next(), Err(RecordingFailure::Io));
            drop(owner);
        } else {
            assert!(owner.process_next().unwrap());
            assert_eq!(owner.finish(), Err(RecordingFailure::Io));
        }
        assert_eq!(
            input.monitor().status(),
            RecordingStatus::Failed(RecordingFailure::Io)
        );
        assert!(segments(&temp.root(), "pcap").is_empty());
        assert_eq!(segments(&temp.root(), "partial").len(), 1);
        let (_, pending) = recording_queue();
        let result = RecordingOwner::open(temp.root(), 100, pending);
        if phase == 0 || phase == 3 {
            assert!(matches!(result, Err(RecordingFailure::Inventory)));
        } else {
            result.unwrap().finish().unwrap();
        }
    }
}

struct PeriodicDisk {
    epoch: std::time::Instant,
    seconds: AtomicU64,
    scans: AtomicU64,
    handles: AtomicU64,
    closed_stats: AtomicU64,
}
impl PeriodicDisk {
    fn new() -> Self {
        Self {
            epoch: std::time::Instant::now(),
            seconds: AtomicU64::new(0),
            scans: AtomicU64::new(0),
            handles: AtomicU64::new(0),
            closed_stats: AtomicU64::new(0),
        }
    }
}
impl maxed_recording::RecordingDisk for PeriodicDisk {
    fn now(&self) -> std::time::Instant {
        self.epoch + std::time::Duration::from_secs(self.seconds.load(Ordering::Relaxed))
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<fs::ReadDir> {
        self.scans.fetch_add(1, Ordering::Relaxed);
        fs::read_dir(path)
    }
    fn metadata(&self, file: &File) -> std::io::Result<fs::Metadata> {
        self.handles.fetch_add(1, Ordering::Relaxed);
        file.metadata()
    }
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
        assert!(path.extension().is_some_and(|e| e == "pcap"));
        self.closed_stats.fetch_add(1, Ordering::Relaxed);
        fs::symlink_metadata(path)
    }
}

#[test]
fn many_segments_do_not_add_per_frame_inventory_work() {
    use std::sync::Arc;
    for retained in [0, 1000] {
        let temp = Temp::new();
        for id in 0..retained {
            temp.seed(id, "pcap", 24);
        }
        let disk = Arc::new(PeriodicDisk::new());
        let (input, pending) = recording_queue();
        let mut owner =
            RecordingOwner::open_with_disk(temp.root(), MAX_QUOTA_BYTES, pending, disk.clone())
                .unwrap();
        let scans = disk.scans.load(Ordering::Relaxed);
        // 1,792 frames including segment creation and three timed sweeps. The
        // injected monotonic clock makes both the five-second gate and operation
        // bounds deterministic, without sleeping or racing worker scheduling.
        for second in [0, 4, 5, 9, 10, 14, 15] {
            disk.seconds.store(second, Ordering::Relaxed);
            let before = disk.closed_stats.load(Ordering::Relaxed);
            for _ in 0..8 {
                for _ in 0..32 {
                    input.try_record(&frame(1)).unwrap();
                }
                for _ in 0..32 {
                    let closed_before = disk.closed_stats.load(Ordering::Relaxed);
                    let active_before = disk.handles.load(Ordering::Relaxed);
                    assert!(owner.process_next().unwrap());
                    assert!(disk.closed_stats.load(Ordering::Relaxed) - closed_before <= 4);
                    assert!(disk.handles.load(Ordering::Relaxed) - active_before <= 1);
                    assert_eq!(disk.scans.load(Ordering::Relaxed), scans);
                }
            }
            let expected = if [5, 10, 15].contains(&second) {
                retained
            } else {
                0
            };
            assert_eq!(
                disk.closed_stats.load(Ordering::Relaxed) - before,
                expected,
                "each sweep visits every tracked entry once, with no early restart"
            );
            assert_eq!(input.monitor().status(), RecordingStatus::Recording);
        }
        assert_eq!(disk.handles.load(Ordering::Relaxed), 1791);
        assert_eq!(disk.closed_stats.load(Ordering::Relaxed), 3 * retained);
        owner.finish().unwrap();
        assert_eq!(
            disk.scans.load(Ordering::Relaxed),
            scans + 1,
            "finish fully revalidates"
        );
        let paths = segments(&temp.root(), "pcap");
        assert_eq!(paths.len(), retained as usize + 1);
        let mut reader = PcapReader::new(File::open(paths.last().unwrap()).unwrap()).unwrap();
        for _ in 0..1792 {
            assert!(reader.next_frame().unwrap().is_some());
        }
        assert!(reader.next_frame().unwrap().is_none());
    }
}

#[test]
fn entry_limit_evicts_oldest_closed_pairs_below_byte_quota_across_starts() {
    let temp = Temp::new();
    let oldest = temp.seed(0, "pcap", 24);
    let second = temp.seed(1, "pcap", 24);
    let newest = temp.seed(2046, "pcap", 24);
    for id in 2..2046 {
        temp.seed(id, "pcap", 24);
    }
    for expected_evicted in [&oldest, &second] {
        let (input, mut owner) = temp.open(MAX_QUOTA_BYTES);
        send(&input, &mut owner, 1);
        assert!(!expected_evicted.exists());
        assert!(!expected_evicted.with_extension("owner").exists());
        assert!(newest.exists());
        assert_eq!(segments(&temp.root(), "partial").len(), 1);
        assert_eq!(fs::read_dir(temp.root()).unwrap().count(), 4095);
        assert!(owner.accounted_bytes() < MAX_QUOTA_BYTES);
        owner.finish().unwrap();
        assert_eq!(segments(&temp.root(), "pcap").len(), 2047);
    }
}

#[test]
fn external_growth_and_timestamp_changes_are_detected_by_the_timed_metadata_check() {
    use std::sync::Arc;
    for length_change in [false, true] {
        let temp = Temp::new();
        let closed = temp.seed(0, "pcap", 24);
        let disk = Arc::new(PeriodicDisk::new());
        let (input, pending) = recording_queue();
        let mut owner =
            RecordingOwner::open_with_disk(temp.root(), MAX_QUOTA_BYTES, pending, disk.clone())
                .unwrap();
        send(&input, &mut owner, 1);
        if length_change {
            OpenOptions::new()
                .append(true)
                .open(&closed)
                .unwrap()
                .write_all(b"changed")
                .unwrap();
        } else {
            File::options()
                .write(true)
                .open(&closed)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
                )
                .unwrap();
        }
        disk.seconds.store(4, Ordering::Relaxed);
        for _ in 0..256 {
            send(&input, &mut owner, 1);
        }
        assert_eq!(disk.closed_stats.load(Ordering::Relaxed), 0);
        disk.seconds.store(5, Ordering::Relaxed);
        input.try_record(&frame(1)).unwrap();
        assert_eq!(owner.process_next(), Err(RecordingFailure::Io));
        assert_eq!(disk.closed_stats.load(Ordering::Relaxed), 1);
        assert_eq!(
            fs::metadata(closed).unwrap().len(),
            if length_change { 31 } else { 24 }
        );
    }
}
