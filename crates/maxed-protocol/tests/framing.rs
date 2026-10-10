use maxed_protocol::{Framer, FramerOutput, ProtocolError};

fn completed(output: FramerOutput) -> Vec<Vec<u8>> {
    assert!(output.error.is_none());
    output.frames
}

#[test]
fn split_header_and_coalesced_packets() {
    let mut f = Framer::default();
    assert!(completed(f.push(&[0, 0])).is_empty());
    assert_eq!(
        completed(f.push(&[0, 5, 105, 0, 0, 0, 5, 105])),
        vec![vec![0, 0, 0, 5, 105]; 2]
    );
    assert!(Framer::default().push(&[0, 0, 0, 4]).error.is_some());
}

#[test]
fn bounds_reset_and_all_fragment_positions() {
    for len in [0u32, 1, 4, 200001, u32::MAX] {
        let mut f = Framer::default();
        assert!(f.push(&len.to_be_bytes()).error.is_some());
        assert_eq!(completed(f.push(&[0, 0, 0, 5, 105])).len(), 1);
    }
    let mut max = vec![0; 200000];
    max[..4].copy_from_slice(&200000u32.to_be_bytes());
    assert_eq!(completed(Framer::default().push(&max)), vec![max]);
    let packet = [0, 0, 0, 8, 250, 1, 2, 3];
    for split in 0..packet.len() {
        let mut f = Framer::default();
        assert!(completed(f.push(&packet[..split])).is_empty());
        assert_eq!(completed(f.push(&packet[split..])), vec![packet.to_vec()]);
        completed(f.push(&packet[..3]));
        f.reset();
        assert_eq!(completed(f.push(&packet)), vec![packet.to_vec()]);
    }
}

#[test]
fn completed_frames_survive_a_later_impossible_length() {
    let first = [0, 0, 0, 6, 250, 7];
    for split in 0..first.len() {
        let mut f = Framer::default();
        assert!(completed(f.push(&first[..split])).is_empty());
        let mut chunk = first[split..].to_vec();
        chunk.extend([0, 0, 0, 5, 105]);
        chunk.extend(4u32.to_be_bytes());
        let output = f.push(&chunk);
        assert_eq!(output.frames, vec![first.to_vec(), vec![0, 0, 0, 5, 105]]);
        assert_eq!(output.error, Some(ProtocolError::Framing));
        assert_eq!(completed(f.push(&first)), vec![first.to_vec()]);
    }
}
