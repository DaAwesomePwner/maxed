//! Private canonical export; canonical fields may contain credentials and player text.
use crate::{CliError, Pipeline, ReplayReport};
use maxed_protocol::{canonical_line, write_jsonl, DecodeEvent, ProtocolError};
use maxed_recording::{PcapReader, RecordingError};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
};

pub(crate) fn metadata_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".meta.json");
    PathBuf::from(name)
}
fn private_absolute(path: &Path) -> Result<PathBuf, CliError> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(CliError::InvalidArguments);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    // Resolve the existing prefix before comparing paths. On Windows this also
    // normalizes ordinary drive paths and canonical verbatim paths consistently.
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .ok_or(CliError::InvalidArguments)?
                .to_owned(),
        );
        existing = existing.parent().ok_or(CliError::InvalidArguments)?;
    }
    let mut resolved = existing.canonicalize()?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    let root = repository_root()?.join("recordings");
    if !resolved.starts_with(&root) || resolved == root {
        return Err(CliError::InvalidArguments);
    }
    Ok(resolved)
}
pub(crate) fn private_existing(path: &Path) -> Result<PathBuf, CliError> {
    let absolute = private_absolute(path)?;
    let cwd = repository_root()?;
    let resolved = absolute.canonicalize()?;
    if !resolved.starts_with(cwd.join("recordings")) {
        return Err(CliError::InvalidArguments);
    }
    Ok(resolved)
}
fn output_path(path: &Path) -> Result<PathBuf, CliError> {
    let absolute = private_absolute(path)?;
    let cwd = repository_root()?;
    let root = cwd.join("recordings");
    // Walk existing ancestors before mkdir, so a symlink cannot redirect directory creation.
    let parent = absolute.parent().ok_or(CliError::InvalidArguments)?;
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or(CliError::InvalidArguments)?;
    }
    let actual = ancestor.canonicalize()?;
    if actual != cwd && !actual.starts_with(&root) {
        return Err(CliError::InvalidArguments);
    }
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(&root) {
        return Err(CliError::InvalidArguments);
    }
    Ok(absolute)
}
fn repository_root() -> Result<PathBuf, CliError> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}
struct PartialOutput {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    complete: bool,
}
impl PartialOutput {
    fn create(path: PathBuf) -> Result<Self, CliError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Self {
            path,
            writer: Some(BufWriter::new(file)),
            complete: false,
        })
    }
    fn writer(&mut self) -> Result<&mut BufWriter<File>, CliError> {
        self.writer.as_mut().ok_or(CliError::InvalidArguments)
    }
    fn flush(&mut self) -> Result<(), CliError> {
        self.writer()?.flush()?;
        Ok(())
    }
    fn commit(&mut self) {
        self.complete = true;
    }
}
impl Drop for PartialOutput {
    fn drop(&mut self) {
        self.writer.take();
        if !self.complete {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct HashingReader<'a> {
    file: File,
    hash: &'a mut Sha256,
}
impl Read for HashingReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
}
fn events(
    events: Vec<DecodeEvent>,
    report: &mut ReplayReport,
    out: &mut PartialOutput,
    index: &mut u64,
) -> Result<(), CliError> {
    for event in events {
        match event {
            DecodeEvent::Packet(packet) => {
                let line = match canonical_line(*index, &packet) {
                    Ok(line) => line,
                    Err(ProtocolError::NonFinite) => {
                        if !packet.alignment_verified {
                            report.unverified_packets = report.unverified_packets.saturating_add(1);
                        }
                        report.failures = report.failures.saturating_add(1);
                        eprintln!("decode diagnostic: NonFinite");
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                };
                report.observe(&DecodeEvent::Packet(packet));
                write_jsonl(out.writer()?, &line)?;
                *index = index.checked_add(1).ok_or(CliError::InvalidArguments)?;
            }
            diagnostic @ DecodeEvent::Diagnostic { kind, .. } => {
                report.observe(&diagnostic);
                eprintln!("decode diagnostic: {kind:?}");
            }
        }
    }
    Ok(())
}
pub fn decode(input: &Path, output: &Path) -> Result<ReplayReport, CliError> {
    decode_with_explanations(input, output, None)
}
pub fn decode_with_explanations(
    input: &Path,
    output: &Path,
    explanation: Option<&Path>,
) -> Result<ReplayReport, CliError> {
    let input_path = input.canonicalize()?;
    let explanation_path = explanation.map(output_path).transpose()?;
    let output_path = output_path(output)?;
    if input_path == output_path || output_path.canonicalize().is_ok_and(|p| p == input_path) {
        return Err(CliError::InvalidArguments);
    }
    let mut out = PartialOutput::create(output_path.clone())?;
    let mut meta = PartialOutput::create(metadata_path(&output_path))?;
    let mut explanation = explanation_path.map(PartialOutput::create).transpose()?;
    let mut hash = Sha256::new();
    let mut report = ReplayReport::default();
    let mut pipeline = Pipeline::default();
    pipeline.explain_failures(explanation.is_some());
    let mut index = 0;
    {
        let file = File::open(input_path)?;
        let mut reader = PcapReader::new(BufReader::new(HashingReader {
            file,
            hash: &mut hash,
        }))?;
        loop {
            let frame = match reader.next_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(RecordingError::Truncated) => {
                    report.truncated = true;
                    break;
                }
                Err(error) => return Err(error.into()),
            };
            report.frames = report
                .frames
                .checked_add(1)
                .ok_or(CliError::InvalidArguments)?;
            events(pipeline.push(frame), &mut report, &mut out, &mut index)?;
            if let Some(explanation) = &mut explanation {
                for failure in pipeline.take_failures() {
                    writeln!(explanation.writer()?, "{failure}")?;
                }
            }
        }
        events(pipeline.finish(), &mut report, &mut out, &mut index)?;
        if let Some(explanation) = &mut explanation {
            for entry in pipeline.take_failures() {
                writeln!(explanation.writer()?, "{entry}")?;
            }
        }
    }
    let digest = maxed_protocol::canonical::lower_hex(&hash.finalize());
    serde_json::to_writer(
        meta.writer()?,
        &serde_json::json!({
            "inputSha256":digest,
            "fieldFailures":report.field_failures,
            "trailingBytePackets":report.trailing_byte_packets,
            "unknownIdPackets":report.unknown_id_packets,
            "wrongDirectionPackets":report.wrong_direction_packets,
            "unverifiedPackets":report.unverified_packets,
            "truncated":report.truncated,
        }),
    )
    .map_err(|_| CliError::InvalidArguments)?;
    meta.writer()?.write_all(b"\n")?;
    out.flush()?;
    meta.flush()?;
    if let Some(explanation) = &mut explanation {
        explanation.flush()?;
        explanation.commit();
    }
    out.commit();
    meta.commit();
    Ok(report)
}
