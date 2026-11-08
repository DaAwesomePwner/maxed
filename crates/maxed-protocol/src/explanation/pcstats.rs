//! RealmCharacterStats.decode / PcStatsDecoder structural projection only.
use crate::{
    privacy::{six_bit_bytes, CHARACTER_STAT_ORDER as ORDER},
    Reader,
};
use std::fmt::Write;

/// Reports successful outer CREATE_SUCCESS packets too: the inner format may diverge.
pub fn explain_create_success(
    value: &str,
    framed_length: usize,
    alignment_verified: bool,
) -> String {
    format!(
        "name=CREATE_SUCCESS id=101 len={framed_length} outer=decoded{}",
        shape_with_trust(value.as_bytes(), alignment_verified)
    )
}

// Reuse wire decoding while keeping snapshot admission in character_stats.
fn base64(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > usize::from(u16::MAX) {
        return None;
    }
    six_bit_bytes(std::str::from_utf8(bytes).ok()?)
}

#[cfg(test)]
fn shape(bytes: &[u8]) -> String {
    shape_with_trust(bytes, true)
}

// Diagnostic recognition only, not snapshot admission or field semantics.
// Both observed 18- and 25-byte suffixes fit these conservative growth bounds.
fn extension_fields(tail: &[u8]) -> Option<usize> {
    if tail.is_empty() || tail.len() > 64 {
        return None;
    }
    let mut reader = Reader::new(tail);
    let mut fields = 0;
    while reader.remaining() != 0 {
        if fields == 32 {
            return None;
        }
        reader.ci().ok()?;
        fields += 1;
    }
    Some(fields)
}

pub(super) fn shape_with_trust(bytes: &[u8], alignment_verified: bool) -> String {
    let mut out = format!(" base64Length={}", bytes.len());
    if !alignment_verified {
        out.push_str(" prefixHex=redacted-unverified");
    }
    let Some(data) = base64(bytes) else {
        out.push_str(" decodedLength=unknown divergence=base64_encoding");
        return out;
    };
    let _ = write!(
        out,
        " decodedLength={} structure=RealmSharkPcStats",
        data.len()
    );
    if !alignment_verified {
        return out;
    }
    let mut reader = Reader::new(&data);
    let Ok(flag) = reader.take(4) else {
        let _ = write!(
            out,
            " divergence=flag_truncated@0:expected=4 tailHex={}",
            crate::lower_hex(&data)
        );
        return out;
    };
    let flag = u32::from_le_bytes([flag[0], flag[1], flag[2], flag[3]]);
    let nonzero = flag != 0;
    let _ = write!(out, " flag@0:width=4:present={nonzero}:leU32={flag} bitmapWidth=16 bitmapBitOrder=lsb0-per-byte");
    let mut unknown = false;
    if nonzero {
        let Ok(mask) = reader.take(16) else {
            let _ = write!(
                out,
                " divergence=presence_truncated@4:expected=16:available={} tailHex={}",
                reader.remaining(),
                crate::lower_hex(&data[reader.position()..])
            );
            return out;
        };
        let present = |id: u8| mask[usize::from(id / 8)] & (1 << (id % 8)) != 0;
        let bits: Vec<_> = (0..128).filter(|&id| present(id)).collect();
        let unsupported: Vec<_> = bits
            .iter()
            .copied()
            .filter(|id| !ORDER.contains(id))
            .collect();
        unknown = !unsupported.is_empty();
        let _ = write!(
            out,
            " presenceBits={bits:?} unsupportedBits={unsupported:?} fields=["
        );
        for &id in ORDER {
            if present(id) {
                let start = reader.position();
                let result = reader.ci();
                let _ = write!(
                    out,
                    "tag={id}@{start}:varintWidth={}",
                    reader.position() - start
                );
                match result {
                    Ok(value) => {
                        let _ = write!(out, ":value={value};");
                    }
                    Err(error) => {
                        let _ = write!(
                            out,
                            ";] divergence=counter_{error:?}@{start}:tag={id} tailHex={}",
                            crate::lower_hex(&data[start..])
                        );
                        return out;
                    }
                }
            }
        }
        out.push(']');
    }
    let _ = write!(
        out,
        " consumed={} trailing={}",
        reader.position(),
        reader.remaining()
    );
    // Only a fully decoded, supported presence section can precede an extension.
    // Discard values and expose only its bounded shape, even in opt-in reports.
    if nonzero && !unknown {
        if let Some(fields) = extension_fields(&data[reader.position()..]) {
            let _ = write!(
                out,
                " extension=unmodeled_varints:bytes={}:fields={fields} divergence=none",
                reader.remaining()
            );
            return out;
        }
    }
    if unknown {
        out.push_str(" divergence=unsupported_presence_bits@4");
    } else if reader.remaining() != 0 {
        let _ = write!(out, " divergence=trailing@{}", reader.position());
    } else {
        out.push_str(" divergence=none");
    }
    // The owner authorized the decoded PcStats tail. It remains unclassified:
    // showing bytes here does not admit it in the production snapshot decoder.
    let _ = write!(
        out,
        " tailHex={}",
        crate::lower_hex(&data[reader.position()..])
    );
    if reader.remaining() != 0 {
        out.push_str(" tailCandidateVarints=[");
        for _ in 0..128 {
            if reader.remaining() == 0 {
                break;
            }
            let start = reader.position();
            let result = reader.ci();
            let _ = write!(out, "@{start}:width={};", reader.position() - start);
            if let Err(error) = result {
                let _ = write!(out, "error={error:?};");
                break;
            }
        }
        let _ = write!(out, "] unclassifiedRemaining={}", reader.remaining());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let a = chunk[0];
            let b = chunk.get(1).copied().unwrap_or(0);
            let c = chunk.get(2).copied().unwrap_or(0);
            for index in [a >> 2, (a & 3) << 4 | b >> 4] {
                out.push(ALPHABET[usize::from(index)] as char);
            }
            out.push(if chunk.len() > 1 {
                ALPHABET[usize::from((b & 15) << 2 | c >> 6)] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[usize::from(c & 63)] as char
            } else {
                '='
            });
        }
        out
    }

    // Invented counters only: 67 present fields consume 112 bytes including
    // the header/bitmap, matching issue #41's shape without captured data.
    fn current_snapshot() -> Vec<u8> {
        let mut data = vec![0; 20];
        data[0] = 1;
        for (index, &id) in ORDER.iter().take(67).enumerate() {
            data[4 + usize::from(id / 8)] |= 1 << (id % 8);
            if index < 25 {
                data.extend([0x81, 1]);
            } else {
                data.push(1);
            }
        }
        assert_eq!(data.len(), 112);
        data
    }

    #[test]
    fn current_extension_reports_shape_without_values_or_divergence() {
        let mut data = current_snapshot();
        data.extend([2; 14]);
        data.extend([0x82, 1, 0x83, 1]);
        let encoded = encode(&data);
        let text = explain_create_success(&encoded, 191, true);
        assert!(text.contains("decodedLength=130"));
        assert!(text.contains("consumed=112 trailing=18 extension=unmodeled_varints:bytes=18:fields=16 divergence=none"));
        assert!(!text.contains("tailHex="));
        assert!(!text.contains("value=66"));
        assert!(!crate::privacy::character_stats(&encoded));
        let untrusted = explain_create_success(&encoded, 191, false);
        assert!(!untrusted.contains("extension="));
        assert!(!untrusted.contains("value="));
    }

    #[test]
    fn extension_bounds_are_inclusive() {
        let mut data = current_snapshot();
        data.extend([0x81, 1].repeat(32));
        assert!(shape(encode(&data).as_bytes())
            .contains("extension=unmodeled_varints:bytes=64:fields=32 divergence=none"));
    }

    #[test]
    fn malformed_oversize_and_unsupported_extensions_remain_divergences() {
        for tail in [
            vec![0x80],
            vec![0xff; 5],
            [0x80, 0x80, 0x80, 0x80, 1].repeat(13), // 65 bytes, only 13 fields
            vec![1; 33],                            // within byte bound, beyond field bound
        ] {
            let mut data = current_snapshot();
            data.extend(tail);
            let text = shape(encode(&data).as_bytes());
            assert!(text.contains("divergence=trailing@112"));
            assert!(!text.contains("extension="));
        }
        let mut data = current_snapshot();
        data[5] |= 8; // unsupported bit 11 must not be hidden by a valid suffix
        data.extend([1; 18]);
        assert!(shape(encode(&data).as_bytes()).contains("divergence=unsupported_presence_bits@4"));
        let data = current_snapshot();
        assert!(shape(encode(&data[..111]).as_bytes()).contains("divergence=counter_Truncated"));
    }

    #[test]
    fn presence_field_order_values_widths_and_divergence() {
        let mut data = vec![0; 20];
        data[0] = 1;
        data[4] = 1; // tag 0 comes after tag 24 in Java
        data[7] = 1;
        data.extend([0x81, 1, 42]);
        let encoded = encode(&data);
        assert_eq!(base64(encoded.as_bytes()).unwrap(), data);
        let text = shape(encoded.as_bytes());
        assert!(text.contains("decodedLength=23"));
        assert!(text.contains("presenceBits=[0, 24]"));
        assert!(text.contains(
            "fields=[tag=24@20:varintWidth=2:value=65;tag=0@22:varintWidth=1:value=42;]"
        ));
        assert!(text.contains("divergence=none"));
        assert!(text.contains(
            "flag@0:width=4:present=true:leU32=1 bitmapWidth=16 bitmapBitOrder=lsb0-per-byte"
        ));
        assert!(!text.contains(&encoded));
        data.push(0x80);
        let text = shape(encode(&data).as_bytes());
        assert!(text.contains("divergence=trailing@23"));
        assert!(text.contains("tailHex=80"));
        assert!(text.contains("error=Truncated"));
        data[5] |= 8; // unsupported bit 11
        assert!(shape(encode(&data).as_bytes()).contains("unsupportedBits=[11]"));
        assert!(shape(encode(&data[..21]).as_bytes())
            .contains("divergence=counter_Truncated@20:tag=24"));
    }

    #[test]
    fn strict_encoding_and_partial_structures() {
        for invalid in ["", "AA", "AB==", "AAA=AAAA", "AA+_", "SECRET!"] {
            assert!(shape(invalid.as_bytes()).contains("divergence=base64_encoding"));
        }
        assert!(shape(b"AAAAAA==").contains("divergence=none"));
        assert!(shape(b"AA==").contains("divergence=flag_truncated@0"));
        assert!(shape(b"AQAAAA==").contains("divergence=presence_truncated@4"));
        assert!(shape(b"AAAAAAA=").contains("divergence=trailing@4"));
        for byte in 0..=255 {
            let bytes = [byte, byte, byte, byte];
            assert_eq!(base64(encode(&bytes).as_bytes()).unwrap(), bytes);
        }
    }

    #[test]
    fn header_is_little_endian_and_counter_values_preserve_sign() {
        let mut data = vec![0; 20];
        data[..4].copy_from_slice(&[4, 3, 2, 1]);
        data[7] = 1;
        data.push(0x41); // sign-magnitude -1
        data.extend([0xa5, 0x5a]);
        let text = shape(encode(&data).as_bytes());
        assert!(text.contains("leU32=16909060"));
        assert!(text.contains("tag=24@20:varintWidth=1:value=-1;"));
        assert!(text.contains("extension=unmodeled_varints:bytes=2:fields=1 divergence=none"));
        assert!(!crate::privacy::character_stats(&encode(&data)));
    }

    #[test]
    fn diagnostic_order_and_supported_bits_match_strict_snapshot_policy() {
        for id in 0..128 {
            let mut data = vec![0; 20];
            data[0] = 1;
            data[4 + id / 8] = 1 << (id % 8);
            data.push(1);
            let encoded = encode(&data);
            assert_eq!(
                crate::privacy::character_stats(&encoded),
                ORDER.contains(&(id as u8))
            );
        }
        // Every prefix must be bounded and tolerate malformed compressed integers.
        for byte in [0, 1, 127, 128, 255] {
            let bytes = [byte; 160];
            for end in 0..=bytes.len() {
                let _ = shape(encode(&bytes[..end]).as_bytes());
            }
        }
    }

    #[test]
    fn repeatable_varint_tail_is_not_promoted_to_a_complete_counter_snapshot() {
        let mut data = vec![0; 20];
        data[0] = 1;
        data[7] = 1; // only shots_fired is present in this synthetic snapshot
        data.push(1);
        let complete = encode(&data);
        assert!(crate::privacy::character_stats(&complete));
        assert!(shape(complete.as_bytes()).contains("divergence=none"));

        // The historical 25-byte suffix uses the same diagnostic recognition,
        // without admitting its unknown semantics to the snapshot decoder.
        data.extend([1; 11]);
        for _ in 0..7 {
            data.extend([0x81, 1]);
        }
        let extended = encode(&data);
        assert!(!crate::privacy::character_stats(&extended));
        let text = shape(extended.as_bytes());
        assert!(text.contains("consumed=21 trailing=25 extension=unmodeled_varints:bytes=25:fields=18 divergence=none"));
        assert!(!text.contains("tailHex="));
        assert!(!text.contains(&extended));
    }
}
