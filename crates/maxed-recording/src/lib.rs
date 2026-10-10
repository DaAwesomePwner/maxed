#![forbid(unsafe_code)]

mod pcap;
pub use pcap::{PcapReader, PcapWriter, RecordingError, PCAP_HEADER};

#[cfg(windows)]
mod owner;
#[cfg(windows)]
pub use owner::{
    recording_queue, RecordingDisk, RecordingFailure, RecordingInput, RecordingMonitor,
    RecordingOwner, RecordingPending, RecordingStatus, SystemRecordingDisk, FILE_HEADER_BYTES,
    FRAME_HEADER_BYTES, MAX_FRAME_BYTES, MAX_QUOTA_BYTES, QUEUE_BYTE_CAPACITY, QUEUE_CAPACITY,
    SEGMENT_TARGET_BYTES,
};
