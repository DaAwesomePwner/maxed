// URL-safe six-bit alphabet: RealmShark PcStatsDecoder.sixBitStringToBytes.
// Unlike the reference, reject malformed alphabet, padding, and unused bits.
pub(crate) fn decode(value: &str, limit: usize) -> Option<Vec<u8>> {
    if value.len() > limit || !value.is_ascii() {
        return None;
    }
    let unpadded = value.trim_end_matches('=');
    let padding = value.len() - unpadded.len();
    if padding > 2 || (padding > 0 && !value.len().is_multiple_of(4)) || unpadded.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(unpadded.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in unpadded.bytes() {
        let n = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(n);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if acc != 0 {
        return None;
    }
    Some(out)
}
