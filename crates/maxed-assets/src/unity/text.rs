use super::TextAsset;
use crate::{reader::Reader, AssetError};

pub(crate) fn parse(mut bytes: Vec<u8>, big: bool) -> Result<TextAsset, AssetError> {
    let mut r = Reader::new(&bytes, big);
    let name = r.string()?;
    let len = r.count(64 * 1024 * 1024)?;
    let start = r.pos;
    r.take(len)?;
    bytes.copy_within(start..start + len, 0);
    bytes.truncate(len);
    Ok(TextAsset { name, bytes })
}
