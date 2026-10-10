use crate::{checked_span, reader::Reader, AssetError};

pub(crate) struct Header {
    pub version: u32,
    pub metadata_size: u64,
    pub data_offset: u64,
    pub header_size: u64,
    pub big: bool,
}

impl Header {
    pub fn parse(bytes: &[u8], file_length: u64) -> Result<Self, AssetError> {
        let mut r = Reader::new(bytes, true);
        let mut metadata_size = u64::from(r.u32()?);
        let mut file_size = u64::from(r.u32()?);
        let version = r.u32()?;
        if !(17..=22).contains(&version) {
            return Err(AssetError::UnsupportedVersion);
        }
        let mut data_offset = u64::from(r.u32()?);
        let big = r.boolean()?;
        r.skip(3)?;
        if version >= 22 {
            metadata_size = u64::from(r.u32()?);
            file_size = r.u64()?;
            data_offset = r.u64()?;
            r.skip(8)?;
        }
        let header_size = r.pos as u64;
        if file_size != file_length || data_offset < header_size {
            return Err(AssetError::InvalidStructure);
        }
        checked_span(header_size, metadata_size, data_offset)?;
        checked_span(data_offset, 0, file_length)?;
        if metadata_size > 64 * 1024 * 1024 {
            return Err(AssetError::LimitExceeded);
        }
        Ok(Self {
            version,
            metadata_size,
            data_offset,
            header_size,
            big,
        })
    }
}
