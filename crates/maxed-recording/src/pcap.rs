use maxed_types::{RawFrame, Timestamp};
use std::io::{self, Read, Write};

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("Recording I/O failed")]
    Io(#[from] io::Error),
    #[error("Unsupported or invalid classic microsecond pcap header")]
    Header,
    #[error("Invalid recording frame length")]
    Length,
    #[error("Invalid recording timestamp")]
    Timestamp,
    #[error("Recording must use Ethernet link type")]
    LinkType,
    #[error("Truncated recording")]
    Truncated,
}

pub const PCAP_HEADER: [u8; 24] = [
    0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0, 0, 1, 0, 0, 0,
];

pub struct PcapWriter<W: Write> {
    writer: W,
}
impl<W: Write> PcapWriter<W> {
    #[cfg(windows)]
    pub(crate) fn get_ref(&self) -> &W {
        &self.writer
    }
    pub fn new(mut writer: W) -> Result<Self, RecordingError> {
        writer.write_all(&PCAP_HEADER)?;
        Ok(Self { writer })
    }
    pub fn write_frame(&mut self, frame: &RawFrame) -> Result<(), RecordingError> {
        let seconds =
            u32::try_from(frame.at.0 / 1_000_000).map_err(|_| RecordingError::Timestamp)?;
        let micros = (frame.at.0 % 1_000_000) as u32;
        let len = u32::try_from(frame.bytes.len()).map_err(|_| RecordingError::Length)?;
        if len > 65_535 || len > frame.original_len {
            return Err(RecordingError::Length);
        }
        let mut header = [0u8; 16];
        for (out, value) in
            header
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip([seconds, micros, len, frame.original_len])
        {
            *out = value.to_le_bytes();
        }
        self.writer.write_all(&header)?;
        self.writer.write_all(&frame.bytes)?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<W, RecordingError> {
        self.writer.flush()?;
        Ok(self.writer)
    }
}

pub struct PcapReader<R: Read> {
    reader: R,
    little: bool,
    snaplen: u32,
    index: u64,
}
fn read_exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<(), RecordingError> {
    reader.read_exact(bytes).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            RecordingError::Truncated
        } else {
            RecordingError::Io(e)
        }
    })
}
fn number(bytes: &[u8], little: bool) -> u32 {
    let array = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if little {
        u32::from_le_bytes(array)
    } else {
        u32::from_be_bytes(array)
    }
}
impl<R: Read> PcapReader<R> {
    pub fn new(mut reader: R) -> Result<Self, RecordingError> {
        let mut header = [0u8; 24];
        read_exact(&mut reader, &mut header)?;
        let little = match header[..4] {
            [0xd4, 0xc3, 0xb2, 0xa1] => true,
            [0xa1, 0xb2, 0xc3, 0xd4] => false,
            _ => return Err(RecordingError::Header),
        };
        let version = if little { [2, 0, 4, 0] } else { [0, 2, 0, 4] };
        if header[4..8] != version {
            return Err(RecordingError::Header);
        }
        let snaplen = number(&header[16..20], little);
        if snaplen == 0 {
            return Err(RecordingError::Length);
        }
        if number(&header[20..24], little) != 1 {
            return Err(RecordingError::LinkType);
        }
        Ok(Self {
            reader,
            little,
            snaplen,
            index: 0,
        })
    }
    pub fn next_frame(&mut self) -> Result<Option<RawFrame>, RecordingError> {
        let mut header = [0u8; 16];
        loop {
            match self.reader.read(&mut header[..1]) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        read_exact(&mut self.reader, &mut header[1..])?;
        let seconds = number(&header[..4], self.little);
        let micros = number(&header[4..8], self.little);
        let len = number(&header[8..12], self.little);
        let original_len = number(&header[12..16], self.little);
        if len > self.snaplen || len > 65_535 || len > original_len {
            return Err(RecordingError::Length);
        }
        if micros >= 1_000_000 {
            return Err(RecordingError::Timestamp);
        }
        let mut bytes = vec![0; len as usize];
        read_exact(&mut self.reader, &mut bytes)?;
        let index = self.index;
        self.index = self.index.checked_add(1).ok_or(RecordingError::Length)?;
        Ok(Some(RawFrame {
            index,
            at: Timestamp(u64::from(seconds) * 1_000_000 + u64::from(micros)),
            original_len,
            bytes,
        }))
    }
}
