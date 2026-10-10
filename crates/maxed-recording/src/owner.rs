//! Windows-only recording seam. Construct and drive the owner on a disk worker,
//! never on the capture/UI thread. The caller resolves the per-user app-data path.
use crate::{PcapWriter, PCAP_HEADER};
use maxed_types::RawFrame;
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, Metadata, OpenOptions},
    hash::{BuildHasher, RandomState},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering},
        mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc,
    },
    time::{Duration, Instant},
};

pub const FILE_HEADER_BYTES: u64 = PCAP_HEADER.len() as u64;
pub const FRAME_HEADER_BYTES: u64 = 16;
pub const MAX_FRAME_BYTES: usize = 65_535;
pub const SEGMENT_TARGET_BYTES: u64 = 64_000_000;
pub const MAX_QUOTA_BYTES: u64 = 1_000_000_000;
/// Payload bytes reserved by producers and queued for the disk worker: 16 MiB.
pub const QUEUE_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
/// Secondary bound limits metadata overhead even for empty frames.
pub const QUEUE_CAPACITY: usize = 16_384;
const MAX_INVENTORY: usize = 4096;
const CLOSED_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const CLOSED_CHECKS_PER_FRAME: usize = 4;
const REPARSE_POINT: u32 = 0x400;
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// Fixed, path-free failures suitable for mapping to native status DTOs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[repr(u8)]
pub enum RecordingFailure {
    #[error("Recording directory must be a resolved app-data recordings child")]
    Location = 2,
    #[error("Recording namespace is unavailable or already in use")]
    Lease,
    #[error("Recording I/O failed; summaries remain available")]
    Io,
    #[error("Recording quota cannot hold this frame; summaries remain available")]
    LimitTooSmall,
    #[error("Recording queue is full; summaries remain available")]
    QueueFull,
    #[error("Recording frame is invalid")]
    InvalidFrame,
    #[error("Recording inventory exceeds its supported bound")]
    Inventory,
    #[error("Recording has stopped")]
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingStatus {
    Recording,
    Stopped,
    Failed(RecordingFailure),
}

#[derive(Clone)]
pub struct RecordingMonitor(Arc<QueueState>);
struct QueueState {
    state: AtomicU8,
    queued_bytes: AtomicUsize,
    dropped_frames: AtomicU64,
}
impl RecordingMonitor {
    pub fn status(&self) -> RecordingStatus {
        match self.0.state.load(Ordering::Acquire) {
            0 => RecordingStatus::Recording,
            1 => RecordingStatus::Stopped,
            n => RecordingStatus::Failed(match n {
                2 => RecordingFailure::Location,
                3 => RecordingFailure::Lease,
                4 => RecordingFailure::Io,
                5 => RecordingFailure::LimitTooSmall,
                6 => RecordingFailure::QueueFull,
                7 => RecordingFailure::InvalidFrame,
                8 => RecordingFailure::Inventory,
                _ => RecordingFailure::Stopped,
            }),
        }
    }

    fn fail(&self, failure: RecordingFailure) -> RecordingFailure {
        let _ =
            self.0
                .state
                .compare_exchange(0, failure as u8, Ordering::AcqRel, Ordering::Acquire);
        match self.status() {
            RecordingStatus::Failed(first) => first,
            _ => failure,
        }
    }

    fn check(&self) -> Result<(), RecordingFailure> {
        match self.status() {
            RecordingStatus::Recording => Ok(()),
            RecordingStatus::Stopped => Err(RecordingFailure::Stopped),
            RecordingStatus::Failed(failure) => Err(failure),
        }
    }

    pub fn dropped_frames(&self) -> u64 {
        self.0.dropped_frames.load(Ordering::Acquire)
    }

    pub fn queued_bytes(&self) -> usize {
        self.0.queued_bytes.load(Ordering::Acquire)
    }

    fn dropped(&self) {
        let _ = self
            .0
            .dropped_frames
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                Some(n.saturating_add(1))
            });
    }
}

pub struct RecordingInput {
    sender: SyncSender<RawFrame>,
    monitor: RecordingMonitor,
}

/// The receiver is moved exactly once into the disk worker's owner.
pub struct RecordingPending {
    receiver: Receiver<RawFrame>,
    monitor: RecordingMonitor,
}

pub fn recording_queue() -> (RecordingInput, RecordingPending) {
    let (sender, receiver) = sync_channel(QUEUE_CAPACITY);
    let monitor = RecordingMonitor(Arc::new(QueueState {
        state: AtomicU8::new(0),
        queued_bytes: AtomicUsize::new(0),
        dropped_frames: AtomicU64::new(0),
    }));
    (
        RecordingInput {
            sender,
            monitor: monitor.clone(),
        },
        RecordingPending { receiver, monitor },
    )
}

impl RecordingInput {
    pub fn monitor(&self) -> RecordingMonitor {
        self.monitor.clone()
    }

    /// No disk access, blocking send, or unbounded frame copy. Call independently
    /// of decoding/store delivery: an error retires only this recording sink.
    /// Queue pressure returns Ok after counting a drop; the sink remains healthy.
    pub fn try_record(&self, frame: &RawFrame) -> Result<(), RecordingFailure> {
        self.monitor.check()?;
        if frame.bytes.len() > MAX_FRAME_BYTES
            || frame.bytes.len() as u64 > u64::from(frame.original_len)
            || frame.at.0 / 1_000_000 > u64::from(u32::MAX)
        {
            return Err(self.monitor.fail(RecordingFailure::InvalidFrame));
        }
        let bytes = frame.bytes.len();
        if self
            .monitor
            .0
            .queued_bytes
            .try_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                queued
                    .checked_add(bytes)
                    .filter(|total| *total <= QUEUE_BYTE_CAPACITY)
            })
            .is_err()
        {
            self.monitor.dropped();
            return Ok(());
        }
        match self.sender.try_send(frame.clone()) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.monitor
                    .0
                    .queued_bytes
                    .fetch_sub(bytes, Ordering::AcqRel);
                match error {
                    TrySendError::Full(_) => {
                        self.monitor.dropped();
                        Ok(())
                    }
                    TrySendError::Disconnected(_) => {
                        Err(self.monitor.fail(RecordingFailure::Stopped))
                    }
                }
            }
        }
    }
}

struct Closed {
    path: PathBuf,
    bytes: u64,
    stamp: (u64, u64),
}

fn stamp(metadata: &Metadata) -> (u64, u64) {
    (metadata.creation_time(), metadata.last_write_time())
}
fn sidecar(path: &Path) -> PathBuf {
    path.with_extension("owner")
}
fn identity(path: &Path, created: u64) -> Result<String, RecordingFailure> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(RecordingFailure::Inventory)?;
    Ok(format!("MAXED-RECORDING-2\n{stem}\n{created}\n"))
}
fn validate_sidecar(path: &Path, created: u64) -> Result<(), RecordingFailure> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(sidecar(path))
        .map_err(|_| RecordingFailure::Inventory)?;
    let meta = file.metadata().map_err(|_| RecordingFailure::Io)?;
    if !plain(&meta) || !meta.is_file() || meta.len() > 4096 {
        return Err(RecordingFailure::Inventory);
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(4096)
        .read_to_end(&mut bytes)
        .map_err(|_| RecordingFailure::Io)?;
    if bytes != identity(path, created)?.as_bytes() {
        return Err(RecordingFailure::Inventory);
    }
    Ok(())
}
// Complete prefixes only: an incomplete header/payload or excessive scan remains
// untouched and blocks admission. At most 64 MB and 4,000,000 frame headers.
fn validate_prefix(path: &Path) -> Result<(), RecordingFailure> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| RecordingFailure::Io)?;
    let meta = file.metadata().map_err(|_| RecordingFailure::Io)?;
    if !plain(&meta) || meta.len() > SEGMENT_TARGET_BYTES {
        return Err(RecordingFailure::Inventory);
    }
    let mut reader = crate::PcapReader::new(file.take(SEGMENT_TARGET_BYTES + 1))
        .map_err(|_| RecordingFailure::Inventory)?;
    for _ in 0..4_000_000 {
        if reader
            .next_frame()
            .map_err(|_| RecordingFailure::Inventory)?
            .is_none()
        {
            return Ok(());
        }
    }
    Err(RecordingFailure::Inventory)
}

/// Filesystem seam used to exercise blocked writes, disk-full and sync failure.
/// Production delegates directly to File. It never owns capture or store work.
pub trait RecordingDisk: Send + Sync {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<Metadata> {
        fs::symlink_metadata(path)
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<fs::ReadDir> {
        fs::read_dir(path)
    }
    fn create_segment(&self, path: &Path) -> std::io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }
    fn metadata(&self, file: &File) -> std::io::Result<Metadata> {
        file.metadata()
    }
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<usize> {
        file.write(bytes)
    }
    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        file.flush()
    }
    fn sync(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
}
pub struct SystemRecordingDisk;
impl RecordingDisk for SystemRecordingDisk {}
struct SegmentFile {
    file: File,
    disk: Arc<dyn RecordingDisk>,
}
impl Write for SegmentFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.disk.write(&mut self.file, bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.disk.flush(&mut self.file)
    }
}
struct Active {
    writer: PcapWriter<SegmentFile>,
    path: PathBuf,
    bytes: u64,
    ceiling: u64,
}

/// Sole namespace owner. No Clone, shared file handles, or independent cleaner.
/// Drop is crash-like (the .active file is recoverable); finish flushes and syncs.
pub struct RecordingOwner {
    active: Option<Active>,
    closed: VecDeque<Closed>,
    closed_bytes: u64,
    next_id: u64,
    root: PathBuf,
    limit: u64,
    pending: RecordingPending,
    disk: Arc<dyn RecordingDisk>,
    next_closed_check: Instant,
    closed_check_cursor: Option<usize>,
    // Directory handles deny rename/replacement while this owner exists.
    _directories: Vec<File>,
    // Last field: all recording file handles drop before the lease is released.
    _lease: File,
}

fn plain(metadata: &Metadata) -> bool {
    metadata.file_attributes() & REPARSE_POINT == 0
}

fn validate_root(root: &Path) -> Result<(), RecordingFailure> {
    if !root.is_absolute()
        || root.file_name().is_none_or(|name| name != "recordings")
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(RecordingFailure::Location);
    }
    // The caller must resolve/create app-data first. Reject directory redirection
    // before creating the child or touching its lease/inventory.
    for ancestor in root.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| RecordingFailure::Location)?;
        if !metadata.is_dir() || !plain(&metadata) {
            return Err(RecordingFailure::Location);
        }
    }
    match fs::create_dir(root) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(RecordingFailure::Io),
    }
    let metadata = fs::symlink_metadata(root).map_err(|_| RecordingFailure::Location)?;
    if !metadata.is_dir() || !plain(&metadata) {
        return Err(RecordingFailure::Location);
    }
    Ok(())
}

fn owned_id(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?.strip_prefix("maxed-v2-")?;
    let (digits, random) = stem.split_once('-')?;
    if digits.len() != 20
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || random.len() != 32
        || !random
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    digits.parse().ok()
}
fn inventory_paths(
    root: &Path,
    disk: &dyn RecordingDisk,
) -> Result<BTreeMap<PathBuf, Metadata>, RecordingFailure> {
    let mut paths = BTreeMap::new();
    for (index, entry) in disk
        .read_dir(root)
        .map_err(|_| RecordingFailure::Io)?
        .enumerate()
    {
        if index >= MAX_INVENTORY {
            return Err(RecordingFailure::Inventory);
        }
        let path = entry.map_err(|_| RecordingFailure::Io)?.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| RecordingFailure::Io)?;
        if !meta.is_file() || !plain(&meta) {
            return Err(RecordingFailure::Location);
        }
        if path
            .file_name()
            .is_none_or(|n| n != ".maxed-recording-v1.lease")
            && (owned_id(&path).is_none()
                || path.extension().is_none_or(|e| {
                    e != "owner" && e != "pcap" && e != "partial" && e != "interrupted"
                }))
        {
            return Err(RecordingFailure::Inventory);
        }
        paths.insert(path, meta);
    }
    Ok(paths)
}
fn guard_directories(root: &Path) -> Result<Vec<File>, RecordingFailure> {
    root.ancestors()
        .take(2)
        .map(|path| {
            let file = OpenOptions::new()
                .access_mode(0)
                .share_mode(3)
                .custom_flags(OPEN_REPARSE_POINT | 0x0200_0000)
                .open(path)
                .map_err(|_| RecordingFailure::Location)?;
            let meta = file.metadata().map_err(|_| RecordingFailure::Location)?;
            if !meta.is_dir() || !plain(&meta) {
                return Err(RecordingFailure::Location);
            }
            Ok(file)
        })
        .collect()
}

impl RecordingOwner {
    /// `root` is the trusted native caller's resolved per-user app-data
    /// `recordings` child, never a WebView path or the checkout's recordings.
    /// Call only on the recording worker. Initialization errors are also published
    /// through the input monitor; they must not abort capture or summary startup.
    pub fn open(
        root: PathBuf,
        limit: u64,
        pending: RecordingPending,
    ) -> Result<Self, RecordingFailure> {
        Self::open_with_disk(root, limit, pending, Arc::new(SystemRecordingDisk))
    }

    pub fn open_with_disk(
        root: PathBuf,
        limit: u64,
        pending: RecordingPending,
        disk: Arc<dyn RecordingDisk>,
    ) -> Result<Self, RecordingFailure> {
        let monitor = pending.monitor.clone();
        Self::open_inner(root, limit, pending, disk).map_err(|failure| monitor.fail(failure))
    }

    fn open_inner(
        root: PathBuf,
        limit: u64,
        pending: RecordingPending,
        disk: Arc<dyn RecordingDisk>,
    ) -> Result<Self, RecordingFailure> {
        pending.monitor.check()?;
        if !(FILE_HEADER_BYTES + FRAME_HEADER_BYTES + 1..=MAX_QUOTA_BYTES).contains(&limit) {
            return Err(RecordingFailure::LimitTooSmall);
        }
        validate_root(&root)?;
        let directories = guard_directories(&root)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(OPEN_REPARSE_POINT)
            .open(root.join(".maxed-recording-v1.lease"))
            .map_err(|_| RecordingFailure::Lease)?;
        let lease_metadata = lease.metadata().map_err(|_| RecordingFailure::Lease)?;
        if !plain(&lease_metadata) || lease_metadata.len() != 0 {
            return Err(RecordingFailure::Lease);
        }
        let mut inventory = Vec::new();
        let mut total = 0u64;
        let mut paths = inventory_paths(&root, disk.as_ref())?;
        paths.remove(&root.join(".maxed-recording-v1.lease"));
        let segments: Vec<_> = paths
            .keys()
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e == "pcap" || e == "partial" || e == "interrupted")
            })
            .cloned()
            .collect();
        let mut sequences = std::collections::BTreeSet::new();
        let mut recovery_bytes = 0u64;
        // Validate the complete inventory before any recovery rename or eviction.
        for path in segments {
            let id = owned_id(&path).ok_or(RecordingFailure::Inventory)?;
            if !sequences.insert(id) {
                return Err(RecordingFailure::Inventory);
            }
            let metadata = paths.remove(&path).ok_or(RecordingFailure::Inventory)?;
            paths
                .remove(&sidecar(&path))
                .ok_or(RecordingFailure::Inventory)?;
            validate_sidecar(&path, metadata.creation_time())?;
            if path.extension().is_some_and(|e| e == "partial") {
                recovery_bytes = recovery_bytes
                    .checked_add(metadata.len())
                    .ok_or(RecordingFailure::Inventory)?;
                // One aggregate 64 MB recovery budget per admission; retain and
                // block larger ambiguous inventories for explicit local repair.
                if recovery_bytes > SEGMENT_TARGET_BYTES {
                    return Err(RecordingFailure::Inventory);
                }
                validate_prefix(&path)?;
            }
            total = total
                .checked_add(metadata.len())
                .ok_or(RecordingFailure::Inventory)?;
            inventory.push((
                id,
                Closed {
                    path,
                    bytes: metadata.len(),
                    stamp: stamp(&metadata),
                },
            ));
        }
        if !paths.is_empty() {
            return Err(RecordingFailure::Inventory);
        }
        for (_, closed) in &mut inventory {
            if closed.path.extension().is_some_and(|e| e == "partial") {
                let interrupted = closed.path.with_extension("interrupted");
                fs::rename(&closed.path, &interrupted).map_err(|_| RecordingFailure::Io)?;
                closed.path = interrupted;
            }
        }
        inventory.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
        let next_id = inventory
            .last()
            .map_or(Some(0), |(id, _)| id.checked_add(1))
            .ok_or(RecordingFailure::Inventory)?;
        let mut owner = Self {
            active: None,
            closed: inventory.into_iter().map(|(_, file)| file).collect(),
            closed_bytes: total,
            next_id,
            root,
            limit,
            pending,
            next_closed_check: disk.now() + CLOSED_CHECK_INTERVAL,
            closed_check_cursor: None,
            disk,
            _directories: directories,
            _lease: lease,
        };
        owner.make_room(0, false)?;
        Ok(owner)
    }

    /// Logical bytes reserved/written, including all recoverable files. On a
    /// partial I/O failure this may conservatively exceed bytes actually written.
    pub fn accounted_bytes(&self) -> u64 {
        self.closed_bytes
            .saturating_add(self.active.as_ref().map_or(0, |a| a.bytes))
    }

    fn needs_room(&self, extra: u64, new_segment: bool) -> Result<bool, RecordingFailure> {
        let bytes = self
            .accounted_bytes()
            .checked_add(extra)
            .ok_or(RecordingFailure::Inventory)?;
        let entries = self
            .closed
            .len()
            .checked_add(usize::from(self.active.is_some()))
            .and_then(|n| n.checked_add(usize::from(new_segment)))
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(1))
            .ok_or(RecordingFailure::Inventory)?;
        Ok(bytes > self.limit || entries > MAX_INVENTORY)
    }

    fn make_room(&mut self, extra: u64, new_segment: bool) -> Result<(), RecordingFailure> {
        if extra > self.limit {
            return Err(RecordingFailure::LimitTooSmall);
        }
        if !self.needs_room(extra, new_segment)? {
            return Ok(());
        }
        // A complete scan precedes deletion, never every ordinary frame.
        self.validate_inventory()?;
        while self.needs_room(extra, new_segment)? {
            let file = self.closed.front().ok_or(RecordingFailure::LimitTooSmall)?;
            let metadata = fs::symlink_metadata(&file.path).map_err(|_| RecordingFailure::Io)?;
            if !metadata.is_file()
                || !plain(&metadata)
                || metadata.len() != file.bytes
                || stamp(&metadata) != file.stamp
            {
                return Err(RecordingFailure::Io);
            }
            validate_sidecar(&file.path, metadata.creation_time())?;
            fs::remove_file(&file.path).map_err(|_| RecordingFailure::Io)?;
            fs::remove_file(sidecar(&file.path)).map_err(|_| RecordingFailure::Io)?;
            self.closed_bytes = self
                .closed_bytes
                .checked_sub(file.bytes)
                .ok_or(RecordingFailure::Inventory)?;
            self.closed.pop_front();
        }
        Ok(())
    }

    fn validate_inventory(&mut self) -> Result<(), RecordingFailure> {
        let mut paths = inventory_paths(&self.root, self.disk.as_ref())?;
        paths.remove(&self.root.join(".maxed-recording-v1.lease"));
        for closed in &self.closed {
            let meta = paths.remove(&closed.path).ok_or(RecordingFailure::Io)?;
            if meta.len() != closed.bytes || stamp(&meta) != closed.stamp {
                return Err(RecordingFailure::Io);
            }
            validate_sidecar(&closed.path, meta.creation_time())?;
            paths
                .remove(&sidecar(&closed.path))
                .ok_or(RecordingFailure::Inventory)?;
        }
        if let Some(active) = &self.active {
            let meta = paths.remove(&active.path).ok_or(RecordingFailure::Io)?;
            validate_sidecar(&active.path, meta.creation_time())?;
            paths
                .remove(&sidecar(&active.path))
                .ok_or(RecordingFailure::Inventory)?;
        }
        if !paths.is_empty() {
            return Err(RecordingFailure::Inventory);
        }
        self.next_closed_check = self.disk.now() + CLOSED_CHECK_INTERVAL;
        self.closed_check_cursor = None;
        Ok(())
    }

    fn check_closed_metadata(&mut self) -> Result<(), RecordingFailure> {
        let cursor = match self.closed_check_cursor {
            Some(cursor) => cursor,
            None if self.disk.now() < self.next_closed_check => return Ok(()),
            None => 0,
        };
        // Spread each sweep over frames, using only already-tracked data paths.
        // No directory walk or sidecar reopen belongs on the periodic path.
        let end = (cursor + CLOSED_CHECKS_PER_FRAME).min(self.closed.len());
        for index in cursor..end {
            let closed = &self.closed[index];
            let meta = self
                .disk
                .symlink_metadata(&closed.path)
                .map_err(|_| RecordingFailure::Io)?;
            if !meta.is_file()
                || !plain(&meta)
                || meta.len() != closed.bytes
                || stamp(&meta) != closed.stamp
            {
                return Err(RecordingFailure::Io);
            }
        }
        if end == self.closed.len() {
            // Start the next sweep no sooner than five seconds after this one
            // completes, including when a slow capture spreads it over time.
            self.next_closed_check = self.disk.now() + CLOSED_CHECK_INTERVAL;
            self.closed_check_cursor = None;
        } else {
            self.closed_check_cursor = Some(end);
        }
        Ok(())
    }

    fn close_active(&mut self) -> Result<(), RecordingFailure> {
        self.validate_inventory()?;
        if let Some(active) = self.active.take() {
            // Preserve accounting even if flush/sync/rename fails. Such failure
            // permanently closes admission; a new owner inventories actual bytes.
            self.closed_bytes = self
                .closed_bytes
                .checked_add(active.bytes)
                .ok_or(RecordingFailure::Inventory)?;
            let file = active.writer.finish().map_err(|_| RecordingFailure::Io)?;
            file.disk
                .sync(&file.file)
                .map_err(|_| RecordingFailure::Io)?;
            drop(file);
            let closed = active.path.with_extension("pcap");
            self.disk
                .rename(&active.path, &closed)
                .map_err(|_| RecordingFailure::Io)?;
            self.closed.push_back(Closed {
                path: closed.clone(),
                bytes: active.bytes,
                stamp: stamp(&fs::metadata(&closed).map_err(|_| RecordingFailure::Io)?),
            });
        }
        Ok(())
    }

    fn write_frame(&mut self, frame: &RawFrame) -> Result<(), RecordingFailure> {
        // The active handle denies external writes/deletion. Its length check
        // is constant work independent of the number of retained segments.
        if let Some(active) = &self.active {
            let file = active.writer.get_ref();
            let meta = self
                .disk
                .metadata(&file.file)
                .map_err(|_| RecordingFailure::Io)?;
            if !meta.is_file() || !plain(&meta) || meta.len() != active.bytes {
                return Err(RecordingFailure::Io);
            }
        }
        self.check_closed_metadata()?;
        let bytes = FRAME_HEADER_BYTES
            .checked_add(frame.bytes.len() as u64)
            .ok_or(RecordingFailure::Inventory)?;
        let first_bytes = FILE_HEADER_BYTES
            .checked_add(bytes)
            .ok_or(RecordingFailure::Inventory)?;
        if first_bytes > self.limit {
            return Err(RecordingFailure::LimitTooSmall);
        }
        if self.active.as_ref().is_some_and(|a| {
            a.bytes
                .checked_add(bytes)
                .is_none_or(|total| total > a.ceiling)
        }) {
            self.close_active()?;
        }
        if self.active.is_none() {
            self.make_room(first_bytes, true)?;
            let id = self.next_id;
            self.next_id = id.checked_add(1).ok_or(RecordingFailure::Inventory)?;
            // RandomState supplies process-local random seeds; these IDs are not
            // secrets and never derive from character, account, server or time.
            let random = RandomState::new();
            let nonce = format!(
                "{:016x}{:016x}",
                random.hash_one((id, 0u8)),
                random.hash_one((id, 1u8))
            );
            let path = self.root.join(format!("maxed-v2-{id:020}-{nonce}.partial"));
            let file = self
                .disk
                .create_segment(&path)
                .map_err(|_| RecordingFailure::Io)?;
            let created = file
                .metadata()
                .map_err(|_| RecordingFailure::Io)?
                .creation_time();
            let mut ownership = OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .custom_flags(OPEN_REPARSE_POINT)
                .open(sidecar(&path))
                .map_err(|_| RecordingFailure::Io)?;
            ownership
                .write_all(identity(&path, created)?.as_bytes())
                .map_err(|_| RecordingFailure::Io)?;
            ownership.sync_all().map_err(|_| RecordingFailure::Io)?;
            drop(ownership);
            let ceiling = SEGMENT_TARGET_BYTES.min(
                self.limit
                    .checked_sub(self.closed_bytes)
                    .ok_or(RecordingFailure::Inventory)?,
            );
            let reserved = self
                .closed_bytes
                .checked_add(FILE_HEADER_BYTES)
                .ok_or(RecordingFailure::Inventory)?;
            let writer = PcapWriter::new(SegmentFile {
                file,
                disk: self.disk.clone(),
            })
            .map_err(|_| {
                self.closed_bytes = reserved;
                RecordingFailure::Io
            })?;
            self.active = Some(Active {
                writer,
                path,
                bytes: FILE_HEADER_BYTES,
                ceiling,
            });
        }
        let active = self.active.as_mut().ok_or(RecordingFailure::Io)?;
        active.bytes = active
            .bytes
            .checked_add(bytes)
            .ok_or(RecordingFailure::Inventory)?;
        active
            .writer
            .write_frame(frame)
            .map_err(|_| RecordingFailure::Io)
    }

    /// Blocking receive belongs exclusively on the disk worker. False means all
    /// producers disconnected; finish then durably closes the final segment.
    pub fn process_next(&mut self) -> Result<bool, RecordingFailure> {
        let frame = loop {
            self.pending.monitor.check()?;
            match self
                .pending
                .receiver
                .recv_timeout(Duration::from_millis(100))
            {
                Ok(frame) => break frame,
                Err(RecvTimeoutError::Disconnected) => return Ok(false),
                Err(RecvTimeoutError::Timeout) => continue,
            }
        };
        self.pending
            .monitor
            .0
            .queued_bytes
            .fetch_sub(frame.bytes.len(), Ordering::AcqRel);
        self.pending.monitor.check()?;
        self.write_frame(&frame)
            .map_err(|failure| self.pending.monitor.fail(failure))?;
        Ok(true)
    }

    pub fn run(mut self) -> Result<(), RecordingFailure> {
        while self.process_next()? {}
        self.finish()
    }

    pub fn finish(mut self) -> Result<(), RecordingFailure> {
        self.pending.monitor.check()?;
        self.close_active()
            .map_err(|failure| self.pending.monitor.fail(failure))?;
        let _ = self.pending.monitor.0.state.compare_exchange(
            0,
            1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.pending.monitor.check().or_else(|failure| {
            if self.pending.monitor.status() == RecordingStatus::Stopped {
                Ok(())
            } else {
                Err(failure)
            }
        })
    }
}

impl Drop for RecordingOwner {
    fn drop(&mut self) {
        let _ = self.pending.monitor.0.state.compare_exchange(
            0,
            1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}
