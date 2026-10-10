use thiserror::Error;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionCounts {
    pub by_index: Vec<Option<i32>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CompletionError {
    #[error("Invalid completion encoding")]
    InvalidEncoding,
    #[error("Truncated completion snapshot")]
    Truncated,
    #[error("Unsupported completion structure")]
    Unsupported,
    #[error("Trailing completion data")]
    Trailing,
}
// RealmShark RealmCharacterStats.parseStats and updateStats, CharacterStatistics.
// Wire values follow this explicit reference order, NOT numeric bit order.
const ORDER: &[usize] = &[
    24, 25, 26, 27, 28, 29, 30, 31, 16, 17, 18, 19, 20, 21, 22, 23, 8, 9, 10, 12, 13, 14, 15, 0, 1,
    2, 3, 4, 5, 6, 7, 58, 59, 60, 61, 62, 63, 48, 49, 50, 51, 52, 53, 55, 40, 41, 42, 43, 44, 45,
    46, 47, 32, 33, 34, 35, 36, 37, 38, 39, 88, 89, 90, 91, 92, 93, 94, 95, 80, 81, 82, 83, 84, 85,
    86, 87, 72, 74, 75, 76, 77, 78, 79, 64, 65, 66, 67, 68, 69, 70, 71, 120, 121, 122, 123, 124,
    125, 126, 127, 112, 113, 114, 115, 116, 117, 118, 119, 104, 105, 106, 107, 108, 109, 110, 111,
    96, 97, 98, 99, 100, 101, 102, 103,
];
const DUNGEONS: &[usize] = &[
    23, 120, 35, 62, 67, 48, 3, 5, 46, 34, 40, 32, 7, 59, 37, 15, 1, 33, 4, 102, 68, 88, 58, 89,
    103, 90, 70, 2, 43, 38, 39, 55, 91, 6, 45, 92, 0, 101, 71, 52, 14, 93, 94, 95, 44, 21, 121, 42,
    49, 123, 80, 81, 47, 8, 125, 9, 10, 69, 60, 51, 82, 53, 63, 122, 66, 41, 13, 50, 22, 83, 98,
    99, 100, 84, 61,
];

pub fn decode_completion_counts(encoded: &str) -> Result<CompletionCounts, CompletionError> {
    use CompletionError::*;
    let bytes = crate::encoding::decode(encoded, 8192).ok_or(InvalidEncoding)?;
    let flag = bytes.get(..4).ok_or(Truncated)?;
    let mut values = [0; 128];
    let mut pos = 4;
    if flag != [0, 0, 0, 0] {
        let bits = bytes.get(4..20).ok_or(Truncated)?;
        pos = 20;
        for i in 0..128 {
            if bits[i / 8] & (1 << (i % 8)) != 0 && !ORDER.contains(&i) {
                return Err(Unsupported);
            }
        }
        for &i in ORDER {
            if bits[i / 8] & (1 << (i % 8)) != 0 {
                values[i] = compressed(&bytes, &mut pos)?;
            }
        }
    }
    if pos != bytes.len() {
        return Err(Trailing);
    }
    Ok(CompletionCounts {
        by_index: DUNGEONS.iter().map(|i| Some(values[*i])).collect(),
    })
}

// PcStatsDecoder.readCompressedInt: first byte has sign and six value bits.
fn compressed(bytes: &[u8], pos: &mut usize) -> Result<i32, CompletionError> {
    use CompletionError::*;
    let mut b = *bytes.get(*pos).ok_or(Truncated)?;
    *pos += 1;
    let negative = b & 64 != 0;
    let mut n = i64::from(b & 63);
    let mut shift = 6;
    while b & 128 != 0 {
        if shift > 27 {
            return Err(Unsupported);
        }
        b = *bytes.get(*pos).ok_or(Truncated)?;
        *pos += 1;
        n |= i64::from(b & 127) << shift;
        shift += 7;
    }
    if negative {
        n = -n;
    }
    i32::try_from(n).map_err(|_| Unsupported)
}
