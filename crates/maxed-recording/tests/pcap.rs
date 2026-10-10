use maxed_recording::{PcapReader, PcapWriter, RecordingError};
use maxed_types::{RawFrame, Timestamp};

#[test]
fn preserves_original_timestamp_and_lengths() {
    let frame = RawFrame {
        index: 0,
        at: Timestamp(1_234_567),
        original_len: 60,
        bytes: vec![0; 54],
    };
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    writer.write_frame(&frame).unwrap();
    let file = writer.finish().unwrap();
    assert_eq!(&file[..4], &[0xd4, 0xc3, 0xb2, 0xa1]);
    let mut reader = PcapReader::new(file.as_slice()).unwrap();
    let got = reader.next_frame().unwrap().unwrap();
    assert_eq!(got.at, frame.at);
    assert_eq!(got.original_len, 60);
    assert_eq!(got.bytes, frame.bytes);
    assert!(reader.next_frame().unwrap().is_none());
}

#[test]
fn truncated_global_header_is_an_error() {
    for len in 0..24 {
        assert!(PcapReader::new(&[0u8; 24][..len]).is_err());
    }
}

fn recording() -> Vec<u8> {
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    writer
        .write_frame(&RawFrame {
            index: 9,
            at: Timestamp(2_000_001),
            original_len: 8,
            bytes: vec![1, 2, 3],
        })
        .unwrap();
    writer.finish().unwrap()
}

#[test]
fn partial_record_headers_and_bodies_are_errors_but_boundary_eof_is_normal() {
    let bytes = recording();
    assert!(PcapReader::new(&bytes[..24])
        .unwrap()
        .next_frame()
        .unwrap()
        .is_none());
    for size in 25..bytes.len() {
        assert!(matches!(
            PcapReader::new(&bytes[..size]).unwrap().next_frame(),
            Err(RecordingError::Truncated)
        ));
    }
}

#[test]
fn reads_big_endian_classic_microseconds() {
    let mut bytes = recording();
    bytes[..4].copy_from_slice(&[0xa1, 0xb2, 0xc3, 0xd4]);
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    for offset in (8..40).step_by(4) {
        bytes[offset..offset + 4].reverse();
    }
    let frame = PcapReader::new(bytes.as_slice())
        .unwrap()
        .next_frame()
        .unwrap()
        .unwrap();
    assert_eq!(frame.at, Timestamp(2_000_001));
    assert_eq!(frame.bytes, [1, 2, 3]);
    assert_eq!(frame.original_len, 8);
}

#[test]
fn rejects_formats_linktypes_and_record_bounds_before_allocating() {
    for magic in [
        [0x0a, 0x0d, 0x0d, 0x0a],
        [0x4d, 0x3c, 0xb2, 0xa1],
        [0xa1, 0xb2, 0x3c, 0x4d],
    ] {
        let mut bytes = recording();
        bytes[..4].copy_from_slice(&magic);
        assert!(matches!(
            PcapReader::new(bytes.as_slice()),
            Err(RecordingError::Header)
        ));
    }
    let mut bytes = recording();
    bytes[20..24].copy_from_slice(&101u32.to_le_bytes());
    assert!(matches!(
        PcapReader::new(bytes.as_slice()),
        Err(RecordingError::LinkType)
    ));
    for (offset, value) in [(32, u32::MAX), (32, 9), (36, 2), (28, 1_000_000)] {
        let mut bytes = recording();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(PcapReader::new(bytes.as_slice())
            .unwrap()
            .next_frame()
            .is_err());
    }
    let mut bytes = recording();
    bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        PcapReader::new(bytes.as_slice()),
        Err(RecordingError::Length)
    ));
    // Larger declared snaplen is common in other pcap writers. Allocation still
    // remains bounded to the capture pipeline's maximum frame size.
    bytes[16..20].copy_from_slice(&262_144u32.to_le_bytes());
    assert!(PcapReader::new(bytes.as_slice())
        .unwrap()
        .next_frame()
        .unwrap()
        .is_some());
    bytes[32..36].copy_from_slice(&65_536u32.to_le_bytes());
    bytes[36..40].copy_from_slice(&65_536u32.to_le_bytes());
    assert!(matches!(
        PcapReader::new(bytes.as_slice()).unwrap().next_frame(),
        Err(RecordingError::Length)
    ));
}

#[test]
fn writer_rejects_overflow_and_preserves_input_order() {
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    for frame in [
        RawFrame {
            index: 0,
            at: Timestamp(u64::MAX),
            original_len: 0,
            bytes: vec![],
        },
        RawFrame {
            index: 0,
            at: Timestamp(0),
            original_len: 0,
            bytes: vec![1],
        },
        RawFrame {
            index: 0,
            at: Timestamp(0),
            original_len: 65_536,
            bytes: vec![0; 65_536],
        },
    ] {
        assert!(writer.write_frame(&frame).is_err());
    }
    for at in [10, 1, 9] {
        writer
            .write_frame(&RawFrame {
                index: 999,
                at: Timestamp(at),
                original_len: 0,
                bytes: vec![],
            })
            .unwrap();
    }
    let file = writer.finish().unwrap();
    assert_eq!(file.len(), 24 + 3 * 16);
    let mut reader = PcapReader::new(file.as_slice()).unwrap();
    for (i, at) in [10, 1, 9].into_iter().enumerate() {
        let frame = reader.next_frame().unwrap().unwrap();
        assert_eq!(frame.index, i as u64);
        assert_eq!(frame.at, Timestamp(at));
    }
}

struct Interrupted<R>(R, bool);
impl<R: std::io::Read> std::io::Read for Interrupted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.1 = !self.1;
        if self.1 {
            Err(std::io::ErrorKind::Interrupted.into())
        } else {
            self.0.read(buf)
        }
    }
}
#[test]
fn interrupted_reads_retry() {
    let file = recording();
    let mut reader = PcapReader::new(Interrupted(file.as_slice(), false)).unwrap();
    assert_eq!(reader.next_frame().unwrap().unwrap().bytes, [1, 2, 3]);
    assert!(reader.next_frame().unwrap().is_none());
}

#[test]
fn maximum_microsecond_time_round_trips_and_flush_failures_surface() {
    let at = Timestamp(u64::from(u32::MAX) * 1_000_000 + 999_999);
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    writer
        .write_frame(&RawFrame {
            index: 0,
            at,
            original_len: 0,
            bytes: vec![],
        })
        .unwrap();
    let file = writer.finish().unwrap();
    assert_eq!(
        PcapReader::new(file.as_slice())
            .unwrap()
            .next_frame()
            .unwrap()
            .unwrap()
            .at,
        at
    );
    struct BadFlush;
    impl std::io::Write for BadFlush {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Other.into())
        }
    }
    assert!(matches!(
        PcapWriter::new(BadFlush).unwrap().finish(),
        Err(RecordingError::Io(_))
    ));
}
