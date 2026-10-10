use crate::{CliError, Pipeline, ReplayReport};
use maxed_capture::{start_capture, CaptureConfig};
use maxed_protocol::{DecodeEvent, Diagnostic, Packet};
use maxed_recording::{PcapWriter, PCAP_HEADER};
use std::{
    cell::Cell,
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Resolve the parent before opening. Repository exports are restricted to the
/// explicit private recordings directory; scrub admission is a separate path.
pub fn private_output_path(path: &Path) -> Result<PathBuf, CliError> {
    let name = path.file_name().ok_or(CliError::InvalidArguments)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut missing = Vec::new();
    let mut existing = parent;
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .ok_or(CliError::InvalidArguments)?
                .to_owned(),
        );
        existing = existing
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
    }
    let mut parent = fs::canonicalize(existing)?;
    for name in missing.into_iter().rev() {
        parent.push(name);
    }
    for ancestor in parent.ancestors() {
        if ancestor.join(".git").exists() {
            let private = ancestor.join("recordings");
            if !parent.starts_with(&private) {
                return Err(CliError::InvalidArguments);
            }
            break;
        }
    }
    Ok(parent.join(name))
}

pub fn record(path: &Path, config: CaptureConfig, stop: &AtomicBool) -> Result<(), CliError> {
    let path = private_output_path(path)?;
    fs::create_dir_all(path.parent().ok_or(CliError::InvalidArguments)?)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let mut progress = RecordingProgress::default();
    // Count beneath the buffer and keep File available after finish errors.
    let mut writer = match PcapWriter::new(BufWriter::new(progress.count_writes(&file))) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
    };
    let handle = match start_capture(config) {
        Ok(handle) => handle,
        Err(error) => return finish_recording(&path, writer, &file, progress, Some(error.into())),
    };
    let receiver = handle.frames.clone();
    let mut pipeline = Pipeline::default();
    let mut failure = None;
    let mut live = false;
    while !stop.load(Ordering::Acquire) {
        let status = handle.status();
        if status.dropped_frames != 0 {
            failure = Some(CliError::FrameLoss);
            break;
        }
        if capture_failed(&status.state) {
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(frame) => {
                let status = handle.status();
                if status.dropped_frames != 0 {
                    failure = Some(CliError::FrameLoss);
                    break;
                }
                if capture_failed(&status.state) {
                    break;
                }
                if let Err(error) = write_next_frame(&mut writer, &mut progress, &frame) {
                    failure = Some(error);
                    break;
                }
                let mut report = ReplayReport::default();
                let events = pipeline.push(frame);
                let had_events = !events.is_empty();
                for event in events {
                    report.observe(&event);
                    match event {
                        DecodeEvent::Packet(packet)
                            if !matches!(packet.packet, Packet::Unknown { .. }) =>
                        {
                            live = true
                        }
                        DecodeEvent::Diagnostic {
                            kind: Diagnostic::Recovering,
                            ..
                        } => live = false,
                        _ => {}
                    }
                }
                if had_events {
                    handle.report_decode(
                        live,
                        report.unknown.min(u64::from(u32::MAX)) as u32,
                        report.failures.min(u64::from(u32::MAX)) as u32,
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = handle.status();
    let stopped = handle.stop();
    if failure.is_none() {
        failure = if status.dropped_frames != 0 {
            Some(CliError::FrameLoss)
        } else {
            stopped.err().map(CliError::from)
        };
    }
    // Drain only after a normal stop. Any loss or worker failure freezes the
    // output at its last complete record, before the discontinuity.
    if failure.is_none() {
        for frame in receiver.try_iter() {
            if let Err(error) = write_next_frame(&mut writer, &mut progress, &frame) {
                failure = Some(error);
                break;
            }
        }
    }
    finish_recording(&path, writer, &file, progress, failure)
}

fn capture_failed(state: &maxed_types::CaptureState) -> bool {
    use maxed_types::CaptureState;
    matches!(
        state,
        CaptureState::Failed
            | CaptureState::NpcapMissing
            | CaptureState::NpcapIncompatible
            | CaptureState::PermissionDenied
    )
}

struct RecordingProgress {
    written: u64,
    complete_len: u64,
    file_bytes: Rc<Cell<u64>>,
    pending_ends: VecDeque<u64>,
    persisted_end: u64,
}

impl Default for RecordingProgress {
    fn default() -> Self {
        Self {
            written: 0,
            complete_len: PCAP_HEADER.len() as u64,
            file_bytes: Rc::new(Cell::new(0)),
            pending_ends: VecDeque::new(),
            persisted_end: 0,
        }
    }
}

impl RecordingProgress {
    fn count_writes<W: Write>(&self, inner: W) -> CountingWriter<W> {
        CountingWriter {
            inner,
            file_bytes: self.file_bytes.clone(),
        }
    }

    fn update_persisted(&mut self) {
        // Only buffered records need individual offsets; retain one boundary
        // for the entire prefix already written to the file.
        while self
            .pending_ends
            .front()
            .is_some_and(|end| *end <= self.file_bytes.get())
        {
            self.persisted_end = self.pending_ends.pop_front().unwrap();
        }
    }
}

struct CountingWriter<W> {
    inner: W,
    file_bytes: Rc<Cell<u64>>,
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.file_bytes.set(self.file_bytes.get() + written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn write_next_frame<W: Write>(
    writer: &mut PcapWriter<W>,
    progress: &mut RecordingProgress,
    frame: &maxed_types::RawFrame,
) -> Result<(), CliError> {
    if frame.index != progress.written {
        return Err(CliError::FrameLoss);
    }
    let result = writer.write_frame(frame);
    if result.is_ok() {
        progress.written = progress.written.saturating_add(1);
        progress.complete_len += 16 + frame.bytes.len() as u64;
        progress.pending_ends.push_back(progress.complete_len);
    }
    progress.update_persisted();
    result.map_err(CliError::from)
}

fn finish_recording<W: Write>(
    path: &Path,
    writer: PcapWriter<W>,
    file: &fs::File,
    mut progress: RecordingProgress,
    failure: Option<CliError>,
) -> Result<(), CliError> {
    // Always flush and close, even when the capture loop failed.
    let result = writer
        .finish()
        .map(|_| ())
        .map_err(CliError::from)
        .and_then(|()| failure.map_or(Ok(()), Err));
    if result.is_err() {
        // finish has dropped the buffered writer before repair, including any
        // final flush attempted by Drop. Retain only complete records whose
        // bytes reached the file, even when later records stayed buffered.
        progress.update_persisted();
        let retained = progress.persisted_end > PCAP_HEADER.len() as u64
            && file.set_len(progress.persisted_end).is_ok();
        if !retained {
            let _ = fs::remove_file(path);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_capture::CaptureError;
    use maxed_recording::PcapReader;
    use maxed_types::{RawFrame, Timestamp};
    use std::sync::atomic::AtomicU64;

    fn output() -> (PathBuf, fs::File) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "maxed-record-test-{}-{}.pcap",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        (path, file)
    }
    fn frame(index: u64) -> RawFrame {
        RawFrame {
            index,
            at: Timestamp(1),
            original_len: 3,
            bytes: vec![1, 2, 3],
        }
    }

    #[test]
    fn discontinuity_retains_only_complete_frames_before_gap() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer = PcapWriter::new(BufWriter::new(progress.count_writes(&file))).unwrap();
        write_next_frame(&mut writer, &mut progress, &frame(0)).unwrap();
        let error = write_next_frame(&mut writer, &mut progress, &frame(2)).unwrap_err();
        assert!(matches!(error, CliError::FrameLoss));
        assert!(error.to_string().contains("frame loss"));
        assert!(finish_recording(&path, writer, &file, progress, Some(error)).is_err());
        let mut reader = PcapReader::new(fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(reader.next_frame().unwrap().unwrap().bytes, [1, 2, 3]);
        assert!(reader.next_frame().unwrap().is_none());
        drop(reader);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn drop_and_worker_failure_keep_prefix_but_remove_header_only_output() {
        for loss in [false, true] {
            for count in [0, 1] {
                let (path, file) = output();
                let mut progress = RecordingProgress::default();
                let mut writer =
                    PcapWriter::new(BufWriter::new(progress.count_writes(&file))).unwrap();
                for index in 0..count {
                    write_next_frame(&mut writer, &mut progress, &frame(index)).unwrap();
                }
                let error = if loss {
                    CliError::FrameLoss
                } else {
                    CaptureError::OpenFailed.into()
                };
                assert!(finish_recording(&path, writer, &file, progress, Some(error)).is_err());
                assert_eq!(path.exists(), count != 0);
                if count != 0 {
                    let mut reader = PcapReader::new(fs::File::open(&path).unwrap()).unwrap();
                    assert!(reader.next_frame().unwrap().is_some());
                    assert!(reader.next_frame().unwrap().is_none());
                    drop(reader);
                    fs::remove_file(path).unwrap();
                }
            }
        }
    }

    struct FailingWriter<'a> {
        file: &'a fs::File,
        remaining: usize,
        fail_flush: bool,
    }

    impl Write for FailingWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Err(std::io::Error::other("injected write failure"));
            }
            let len = bytes.len().min(self.remaining);
            let written = self.file.write(&bytes[..len])?;
            self.remaining -= written;
            Ok(written)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            if self.fail_flush {
                return Err(std::io::Error::other("injected flush failure"));
            }
            self.file.flush()
        }
    }

    fn assert_complete_prefix(path: &Path) {
        assert_eq!(fs::metadata(path).unwrap().len(), 24 + 16 + 3);
        let mut reader = PcapReader::new(fs::File::open(path).unwrap()).unwrap();
        let retained = reader.next_frame().unwrap().unwrap();
        assert_eq!(retained.bytes, frame(0).bytes);
        assert_eq!(retained.at, frame(0).at);
        assert!(reader.next_frame().unwrap().is_none());
        assert_eq!(crate::replay(path).unwrap().frames, 1);
    }

    #[test]
    fn partial_record_header_or_body_is_truncated_after_write_failure() {
        for partial in [1, 15, 16, 17, 18] {
            let (path, file) = output();
            let mut progress = RecordingProgress::default();
            let mut writer = PcapWriter::new(progress.count_writes(FailingWriter {
                file: &file,
                remaining: 24 + 19 + partial,
                fail_flush: false,
            }))
            .unwrap();
            write_next_frame(&mut writer, &mut progress, &frame(0)).unwrap();
            let error = write_next_frame(&mut writer, &mut progress, &frame(1)).unwrap_err();
            assert_eq!(
                fs::metadata(&path).unwrap().len(),
                (24 + 19 + partial) as u64
            );
            assert!(finish_recording(&path, writer, &file, progress, Some(error)).is_err());
            assert_complete_prefix(&path);
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn flush_failure_keeps_a_complete_prefix() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer = PcapWriter::new(progress.count_writes(FailingWriter {
            file: &file,
            remaining: usize::MAX,
            fail_flush: true,
        }))
        .unwrap();
        write_next_frame(&mut writer, &mut progress, &frame(0)).unwrap();
        assert!(finish_recording(&path, writer, &file, progress, None).is_err());
        assert_complete_prefix(&path);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_buffer_flush_retains_complete_on_disk_prefix() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer = PcapWriter::new(BufWriter::new(progress.count_writes(FailingWriter {
            file: &file,
            remaining: 24 + 19 + 17,
            fail_flush: false,
        })))
        .unwrap();
        for index in 0..2 {
            write_next_frame(&mut writer, &mut progress, &frame(index)).unwrap();
        }
        assert!(finish_recording(&path, writer, &file, progress, None).is_err());
        assert_complete_prefix(&path);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn buffered_write_failing_partway_through_a_later_frame_keeps_prefix() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer = PcapWriter::new(BufWriter::with_capacity(
            64,
            progress.count_writes(FailingWriter {
                file: &file,
                remaining: 24 + 19 + 17,
                fail_flush: false,
            }),
        ))
        .unwrap();
        for index in 0..2 {
            write_next_frame(&mut writer, &mut progress, &frame(index)).unwrap();
        }
        assert_eq!(progress.file_bytes.get(), 0);
        // The third record forces the buffer to flush, failing in record two.
        let error = write_next_frame(&mut writer, &mut progress, &frame(2)).unwrap_err();
        assert_eq!(progress.file_bytes.get(), 24 + 19 + 17);
        assert_eq!(progress.persisted_end, 24 + 19);
        assert!(finish_recording(&path, writer, &file, progress, Some(error)).is_err());
        assert_complete_prefix(&path);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn persisted_offsets_are_pruned_during_buffered_recording() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer =
            PcapWriter::new(BufWriter::with_capacity(64, progress.count_writes(&file))).unwrap();
        for index in 0..1000 {
            write_next_frame(&mut writer, &mut progress, &frame(index)).unwrap();
            assert!(progress.pending_ends.len() <= 4);
        }
        assert!(progress.persisted_end > 24 + 19 * 990);
        finish_recording(&path, writer, &file, progress, None).unwrap();
        assert_eq!(crate::replay(&path).unwrap().frames, 1000);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn partial_first_record_is_removed_after_buffer_flush_failure() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer = PcapWriter::new(BufWriter::new(progress.count_writes(FailingWriter {
            file: &file,
            remaining: 24 + 17,
            fail_flush: false,
        })))
        .unwrap();
        write_next_frame(&mut writer, &mut progress, &frame(0)).unwrap();
        assert!(finish_recording(&path, writer, &file, progress, None).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_failure_does_not_replace_original_error() {
        let (path, file) = output();
        let progress = RecordingProgress::default();
        let writer = PcapWriter::new(BufWriter::new(progress.count_writes(&file))).unwrap();
        let missing_path = path.with_extension("missing");
        assert!(!missing_path.exists());
        let error = finish_recording(
            &missing_path,
            writer,
            &file,
            progress,
            Some(CliError::FrameLoss),
        )
        .unwrap_err();
        assert!(matches!(error, CliError::FrameLoss));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_truncation_removes_file() {
        let (path, file) = output();
        let mut progress = RecordingProgress::default();
        let mut writer = PcapWriter::new(progress.count_writes(FailingWriter {
            file: &file,
            remaining: 24 + 19 + 17,
            fail_flush: false,
        }))
        .unwrap();
        write_next_frame(&mut writer, &mut progress, &frame(0)).unwrap();
        let error = write_next_frame(&mut writer, &mut progress, &frame(1)).unwrap_err();
        let read_only = fs::File::open(&path).unwrap();
        assert!(finish_recording(&path, writer, &read_only, progress, Some(error)).is_err());
        assert!(!path.exists());
    }
}
