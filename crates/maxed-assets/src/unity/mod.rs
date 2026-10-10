//! Bounded serialized-file reader derived from RealmShark's UnityPy reader.
//! Upstream: MIT, Copyright (c) 2019-2026 K0lb3; see UNITYPY-LICENSE.txt.
mod header;
mod localization;
mod object;
mod pips;
mod serialized;
mod stream;
pub use pips::{locate_enchant_pips, EnchantPips, PIP_COUNT, PIP_SHEET, PIP_SIZE};
pub use stream::{open_sheets, read_text_resources, SheetInfo, SheetReader};
#[cfg(test)]
mod tests;
mod text;
mod texture;

use crate::{checked_span, AssetError};
use header::Header;
use object::Object;
use serialized::Serialized;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use texture::TextureInfo;

pub(crate) const BUDGET: usize = 256 * 1024 * 1024;
const SHEETS: &[&str] = &[
    "characters",
    "characters_masks",
    "groundTiles",
    "mapObjects",
];

#[derive(Debug)]
pub struct TextAsset {
    pub name: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug)]
pub struct TextureAsset {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Debug)]
pub struct ExtractedAssets {
    pub english: std::collections::BTreeMap<String, String>,
    pub texts: Vec<TextAsset>,
    pub textures: Vec<TextureAsset>,
    pub sprite_mapping: Vec<u8>,
}

#[derive(Debug)]
pub struct ResourceInfo {
    pub serialized_version: u32,
    pub unity_version: String,
    pub type_tree: bool,
    pub objects: usize,
    pub text_assets: usize,
    pub texts: Vec<TextInfo>,
    pub sheets: Vec<TextureProbe>,
}

/// Diagnostic prefix fields only; not a claim that the texture layout decoded.
#[derive(Debug)]
pub struct TextureProbe {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub complete_image_size: u32,
    pub format: i32,
    pub mip_count: u32,
    /// Ten fixed metadata words following mip count; no pixel bytes.
    pub layout_words: [u32; 10],
    pub supported_layout: bool,
}

#[derive(Debug)]
pub struct TextInfo {
    pub name: String,
    pub bytes: usize,
    pub sha256: String,
}

pub(crate) fn cancelled(cancel: &AtomicBool) -> Result<(), AssetError> {
    if cancel.load(Ordering::Relaxed) {
        Err(AssetError::Cancelled)
    } else {
        Ok(())
    }
}

fn open(path: &Path, cancel: &AtomicBool) -> Result<(File, Header, Serialized), AssetError> {
    cancelled(cancel)?;
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut bytes = [0; 48];
    file.read_exact(&mut bytes)?;
    let header = Header::parse(&bytes, len)?;
    let metadata = read_span(&mut file, header.header_size, header.metadata_size as usize)?;
    let serialized = Serialized::parse(&metadata, &header, len, cancel)?;
    Ok((file, header, serialized))
}

fn read_span(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>, AssetError> {
    checked_span(offset, len as u64, file.metadata()?.len())?;
    if len > BUDGET {
        return Err(AssetError::LimitExceeded);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn texture_header(
    file: &mut File,
    object: &Object,
    big: bool,
    unity: &str,
) -> Result<TextureInfo, AssetError> {
    let bytes = read_span(file, object.offset, (object.size as usize).min(8192))?;
    let mut info = TextureInfo::parse(&bytes, big, object.size, unity)?;
    if info.image_bytes > 0 {
        let relative = info.inline_offset + u64::from(info.image_bytes);
        let remaining = u64::from(object.size)
            .checked_sub(relative)
            .ok_or(AssetError::InvalidStructure)?;
        if remaining > 8192 {
            return Err(AssetError::InvalidStructure);
        }
        let tail = read_span(file, object.offset + relative, remaining as usize)?;
        let mut r = crate::reader::Reader::new(&tail, big);
        info.stream_offset = r.u64()?;
        info.stream_bytes = r.u32()?;
        info.stream_path = r.string()?;
        if info.stream_bytes != 0 || !info.stream_path.is_empty() || r.pos != tail.len() {
            return Err(AssetError::InvalidStructure);
        }
    }
    Ok(info)
}

/// Inspects the selected sheets without allocating pixel payloads.
pub fn inspect_resources(path: &Path, cancel: &AtomicBool) -> Result<ResourceInfo, AssetError> {
    let (mut file, header, serialized) = open(path, cancel)?;
    let mut sheets = Vec::new();
    let mut texts = Vec::new();
    let mut retained_names = 0usize;
    for object in &serialized.objects {
        cancelled(cancel)?;
        if object.class == 49 {
            if object.size as usize > 64 * 1024 * 1024 + 8192 {
                return Err(AssetError::LimitExceeded);
            }
            let bytes = read_span(&mut file, object.offset, object.size as usize)?;
            let text = text::parse(bytes, header.big)?;
            retained_names = retained_names
                .checked_add(text.name.len() + 64 + std::mem::size_of::<TextInfo>())
                .ok_or(AssetError::LimitExceeded)?;
            if retained_names > BUDGET {
                return Err(AssetError::LimitExceeded);
            }
            texts.push(TextInfo {
                name: text.name,
                bytes: text.bytes.len(),
                sha256: format!("{:x}", Sha256::digest(&text.bytes)),
            });
        }
        if object.class == 28 {
            let bytes = read_span(&mut file, object.offset, (object.size as usize).min(8192))?;
            let mut r = crate::reader::Reader::new(&bytes, header.big);
            let name = r.string()?;
            if SHEETS.contains(&name.as_str()) {
                if sheets.iter().any(|s: &TextureProbe| s.name == name) {
                    return Err(AssetError::InvalidStructure);
                }
                if serialized.unity_version != "6000.0.58f2" {
                    return Err(AssetError::UnsupportedVersion);
                }
                r.boolean()?;
                r.align()?;
                let width = r.u32()?;
                let height = r.u32()?;
                let complete_image_size = r.u32()?;
                r.skip(4)?;
                let format = r.i32()?;
                let mip_count = r.u32()?;
                let mut layout_words = [0; 10];
                for word in &mut layout_words {
                    *word = r.u32()?;
                }
                let supported_layout = match texture_header(
                    &mut file,
                    object,
                    header.big,
                    &serialized.unity_version,
                ) {
                    Ok(_) => true,
                    Err(AssetError::UnsupportedTexture) => false,
                    Err(error) => return Err(error),
                };
                sheets.push(TextureProbe {
                    name,
                    width,
                    height,
                    complete_image_size,
                    format,
                    mip_count,
                    layout_words,
                    supported_layout,
                });
            }
        }
    }
    cancelled(cancel)?;
    Ok(ResourceInfo {
        serialized_version: header.version,
        unity_version: serialized.unity_version,
        type_tree: serialized.type_tree,
        objects: serialized.objects.len(),
        text_assets: serialized.objects.iter().filter(|o| o.class == 49).count(),
        texts,
        sheets,
    })
}

/// Bounded collecting convenience for small inputs. For the installed client,
/// use `read_text_resources` plus `open_sheets`, or `load_or_extract`; its sheets
/// exceed the aggregate collecting limit and must remain file-backed.
pub fn read_resources(path: &Path, cancel: &AtomicBool) -> Result<ExtractedAssets, AssetError> {
    let (mut file, header, serialized) = open(path, cancel)?;
    let mut out = ExtractedAssets {
        english: Default::default(),
        texts: Vec::new(),
        textures: Vec::new(),
        sprite_mapping: Vec::new(),
    };
    let mut retained = 0usize;
    for object in &serialized.objects {
        cancelled(cancel)?;
        match object.class {
            49 => {
                if object.size as usize > 64 * 1024 * 1024 + 8192 {
                    return Err(AssetError::LimitExceeded);
                }
                if retained
                    .checked_add(object.size as usize)
                    .is_none_or(|n| n > BUDGET)
                {
                    return Err(AssetError::LimitExceeded);
                }
                let bytes = read_span(&mut file, object.offset, object.size as usize)?;
                let text = text::parse(bytes, header.big)?;
                retained = retained
                    .checked_add(
                        text.bytes.capacity() + text.name.len() + std::mem::size_of::<TextAsset>(),
                    )
                    .ok_or(AssetError::LimitExceeded)?;
                if retained > BUDGET {
                    return Err(AssetError::LimitExceeded);
                }
                if text.name == "spritesheetf" {
                    if !out.sprite_mapping.is_empty() {
                        return Err(AssetError::InvalidStructure);
                    }
                    out.sprite_mapping = text.bytes;
                } else {
                    out.texts.push(text);
                }
            }
            114 => localization::read(
                &mut file,
                object,
                header.big,
                cancel,
                &mut out.english,
                &mut retained,
                BUDGET,
            )?,
            28 => {
                let bytes = read_span(&mut file, object.offset, (object.size as usize).min(8192))?;
                let name = crate::reader::Reader::new(&bytes, header.big).string()?;
                if !SHEETS.contains(&name.as_str()) {
                    continue;
                }
                let info =
                    texture_header(&mut file, object, header.big, &serialized.unity_version)?;
                if info.format != 4 {
                    return Err(AssetError::UnsupportedTexture);
                }
                let size = info.decoded_size()?;
                retained = retained
                    .checked_add(size)
                    .ok_or(AssetError::LimitExceeded)?;
                if retained > BUDGET {
                    return Err(AssetError::LimitExceeded);
                }
                let mut rgba = if info.image_bytes > 0 {
                    if size > info.image_bytes as usize {
                        return Err(AssetError::InvalidStructure);
                    }
                    read_span(&mut file, object.offset + info.inline_offset, size)?
                } else {
                    if size > info.stream_bytes as usize {
                        return Err(AssetError::InvalidStructure);
                    }
                    let companion = safe_companion(path, &info.stream_path)?;
                    let mut stream = File::open(companion)?;
                    checked_span(
                        info.stream_offset,
                        u64::from(info.stream_bytes),
                        stream.metadata()?.len(),
                    )?;
                    read_span(&mut stream, info.stream_offset, size)?
                };
                // Unity's first row is bottom-up. Normalize to PNG's top-down order.
                let row = info.width as usize * 4;
                for y in 0..info.height as usize / 2 {
                    let (upper, lower) = rgba.split_at_mut((info.height as usize - 1 - y) * row);
                    upper[y * row..(y + 1) * row].swap_with_slice(&mut lower[..row]);
                }
                out.textures.push(TextureAsset {
                    name: info.name,
                    width: info.width,
                    height: info.height,
                    rgba,
                });
            }
            _ => {}
        }
    }
    cancelled(cancel)?;
    Ok(out)
}

pub(crate) fn safe_companion(resources: &Path, relative: &str) -> Result<PathBuf, AssetError> {
    if relative.is_empty() || relative.contains([':', '\\']) {
        return Err(AssetError::InvalidStructure);
    }
    let path = Path::new(relative);
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(AssetError::InvalidStructure);
    }
    let root = resources
        .parent()
        .ok_or(AssetError::InvalidStructure)?
        .canonicalize()?;
    let target = root.join(path).canonicalize()?;
    if !target.starts_with(&root) {
        return Err(AssetError::InvalidStructure);
    }
    Ok(target)
}
