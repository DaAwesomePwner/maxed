use crate::{checked_span, reader::Reader, AssetError};

/// Metadata only: no decoded pixels and no private absolute paths.
#[derive(Debug)]
pub(crate) struct TextureInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub image_bytes: u32,
    pub stream_bytes: u32,
    pub(crate) inline_offset: u64,
    pub(crate) stream_offset: u64,
    pub(crate) stream_path: String,
}

impl TextureInfo {
    pub(crate) fn parse(
        bytes: &[u8],
        big: bool,
        object_size: u32,
        unity: &str,
    ) -> Result<Self, AssetError> {
        // Type trees are stripped in this client. This named-field profile is
        // validated by exact payload/trailer bounds and the pixel oracle test.
        // Do not apply the profile to other builds without that validation.
        if unity != "6000.0.58f2" {
            return Err(AssetError::UnsupportedVersion);
        }
        let mut r = Reader::new(bytes, big);
        let name = r.string()?;
        r.boolean()?; // alpha channel optional
        r.align()?;
        let width = r.u32()?;
        let height = r.u32()?;
        let complete_size = r.u32()?;
        let stripped_mips = r.u32()?;
        let format = r.i32()?;
        let mip_count = r.u32()?;
        for _ in 0..3 {
            // readable, preprocessed, ignore mipmap limit
            r.boolean()?;
        }
        r.align()?;
        r.string()?; // mipmap limit group name (aligned string)
        r.boolean()?; // streaming mipmaps
        r.align()?;
        r.skip(4)?; // mip priority
        let image_count = r.u32()?;
        let dimension = r.u32()?;
        if image_count != 1 || dimension != 2 {
            return Err(AssetError::UnsupportedTexture);
        }
        r.skip(32)?; // GL settings, lightmap format, color space
        let platform_size = r.count(4096)?;
        r.skip(platform_size)?;
        r.align()?;
        let image_bytes = r.u32()?;
        let inline_offset = r.pos as u64;
        checked_span(
            inline_offset,
            u64::from(image_bytes),
            u64::from(object_size),
        )?;
        // For inline payloads, the stream descriptor follows the pixels and is
        // checked by the object reader separately, without buffering that payload.
        let (stream_offset, stream_bytes, stream_path) = if image_bytes == 0 {
            (r.u64()?, r.u32()?, r.string()?)
        } else {
            (0, 0, String::new())
        };
        if image_bytes == 0 && r.pos as u64 != u64::from(object_size) {
            return Err(AssetError::InvalidStructure);
        }
        if width == 0 || height == 0 {
            return Err(AssetError::InvalidStructure);
        }
        if width > 16384 || height > 16384 {
            return Err(AssetError::LimitExceeded);
        }
        let expected = u64::from(width) * u64::from(height) * 4;
        if format != 4 || mip_count != 1 || stripped_mips != 0 {
            return Err(AssetError::UnsupportedTexture);
        }
        if u64::from(complete_size) != expected
            || u64::from(image_bytes.max(stream_bytes)) != expected
        {
            return Err(AssetError::InvalidStructure);
        }
        Ok(Self {
            name,
            width,
            height,
            format,
            image_bytes,
            stream_bytes,
            inline_offset,
            stream_offset,
            stream_path,
        })
    }
    pub(crate) fn decoded_size(&self) -> Result<usize, AssetError> {
        (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(AssetError::LimitExceeded)
    }
}
