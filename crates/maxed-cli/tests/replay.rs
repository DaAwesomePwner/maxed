use maxed_cli::Pipeline;
use maxed_protocol::{canonical_line, write_jsonl, DecodeEvent, Diagnostic};
use maxed_recording::{PcapReader, PcapWriter};
use maxed_types::{RawFrame, Timestamp};

#[test]
fn command_help_and_invalid_arguments_list_all_joined_commands() {
    for args in [vec!["--help"], vec![], vec!["record"]] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
            .args(&args)
            .output()
            .unwrap();
        let help = args == ["--help"];
        assert_eq!(result.status.code(), Some(if help { 0 } else { 2 }));
        let text = String::from_utf8(if help { result.stdout } else { result.stderr }).unwrap();
        for command in ["record", "replay", "decode", "parity"] {
            assert!(text.contains(&format!("maxed-cli {command} ")));
        }
    }
}

fn frame(index: u64, seq: u32, syn: bool, bytes: &[u8]) -> RawFrame {
    let mut raw = vec![0; 54 + bytes.len()];
    raw[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    raw[14] = 0x45;
    raw[16..18].copy_from_slice(&((40 + bytes.len()) as u16).to_be_bytes());
    raw[23] = 6;
    raw[26..30].copy_from_slice(&[127, 0, 0, 1]);
    raw[30..34].copy_from_slice(&[127, 0, 0, 2]);
    raw[34..38].copy_from_slice(&[0xc3, 0x50, 8, 2]);
    raw[38..42].copy_from_slice(&seq.to_be_bytes());
    raw[46] = 0x50;
    raw[47] = if syn { 2 } else { 0x10 };
    raw[54..].copy_from_slice(bytes);
    RawFrame {
        index,
        at: Timestamp(index * 100),
        original_len: raw.len() as u32,
        bytes: raw,
    }
}
fn canonical(frames: Vec<RawFrame>) -> Vec<u8> {
    let mut pipeline = Pipeline::default();
    let mut events = Vec::new();
    for frame in frames {
        events.extend(pipeline.push(frame));
    }
    events.extend(pipeline.finish());
    let mut output = Vec::new();
    let mut index = 0;
    for event in events {
        if let DecodeEvent::Packet(packet) = event {
            write_jsonl(&mut output, &canonical_line(index, &packet).unwrap()).unwrap();
            index += 1;
        }
    }
    assert_eq!(index, 2);
    output
}

#[test]
fn direct_and_classic_pcap_paths_emit_identical_canonical_output() {
    let payload = [0, 0, 0, 5, 105, 0, 0, 0, 7, 250, 0xaa, 0xbb];
    let frames = vec![
        frame(0, 9, true, b""),
        frame(1, 14, false, &payload[4..]),
        frame(2, 10, false, &payload[..6]),
    ];
    let direct = canonical(frames.clone());
    let mut writer = PcapWriter::new(Vec::new()).unwrap();
    for frame in frames {
        writer.write_frame(&frame).unwrap();
    }
    let file = writer.finish().unwrap();
    let mut reader = PcapReader::new(file.as_slice()).unwrap();
    let mut replay = Vec::new();
    while let Some(frame) = reader.next_frame().unwrap() {
        replay.push(frame);
    }
    assert_eq!(canonical(replay), direct);
}

#[test]
fn lost_or_truncated_frames_invalidate_before_more_ciphertext() {
    for truncated in [false, true] {
        let mut pipeline = Pipeline::default();
        pipeline.push(frame(0, 9, true, b""));
        let mut next = frame(if truncated { 1 } else { 2 }, 10, false, &[0, 0, 0, 5, 105]);
        if truncated {
            next.bytes.pop();
        }
        let events = pipeline.push(next);
        assert!(events.iter().any(|event| matches!(
            event,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::Recovering,
                ..
            }
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event, DecodeEvent::Packet(_))));
        pipeline.finish();
        assert!(pipeline.finish().is_empty());
    }
}

#[test]
fn private_outputs_refuse_repository_sources_and_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in [
        "Cargo.toml",
        "crates/maxed-cli/raw.jsonl",
        "tests/fixtures/scrubbed/raw.pcap",
    ] {
        assert!(maxed_cli::record::private_output_path(&root.join(path)).is_err());
    }
    assert!(
        maxed_cli::record::private_output_path(&root.join("recordings/synthetic.pcap")).is_ok()
    );
}

#[test]
fn unsupported_frames_are_counted_without_interrupting_ipv4_decode() {
    for ether_type in [0x86ddu16, 0x8100] {
        let mut pipeline = Pipeline::default();
        pipeline.push(frame(0, 9, true, b""));
        let packet = [0, 0, 0, 5, 105];
        assert!(pipeline
            .push(frame(1, 10, false, &packet))
            .iter()
            .any(|e| matches!(e, DecodeEvent::Packet(_))));
        let mut unsupported = frame(2, 0, false, b"");
        unsupported.bytes[12..14].copy_from_slice(&ether_type.to_be_bytes());
        if ether_type == 0x8100 {
            unsupported.bytes[16..18].copy_from_slice(&0x88a8u16.to_be_bytes());
            unsupported.bytes[20..22].copy_from_slice(&0x8100u16.to_be_bytes());
        }
        assert!(pipeline.push(unsupported).is_empty());
        assert_eq!(pipeline.unsupported_frames(), 1);
        let events = pipeline.push(frame(3, 15, false, &packet));
        assert!(events.iter().any(|e| matches!(e, DecodeEvent::Packet(_))));
        assert!(!events.iter().any(|e| matches!(
            e,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::Recovering,
                ..
            }
        )));
    }
}

#[test]
fn fragmented_or_invalid_ipv4_still_invalidates_streams() {
    for (offset, value) in [(14, 0x44), (20, 0x20)] {
        let mut pipeline = Pipeline::default();
        pipeline.push(frame(0, 9, true, b""));
        let mut malformed = frame(1, 10, false, &[0, 0, 0, 5, 105]);
        malformed.bytes[offset] = value;
        let events = pipeline.push(malformed);
        assert!(events.iter().any(|e| matches!(
            e,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::Recovering,
                ..
            }
        )));
        assert!(!events.iter().any(|e| matches!(e, DecodeEvent::Packet(_))));
        assert_eq!(pipeline.unsupported_frames(), 0);
    }
}
