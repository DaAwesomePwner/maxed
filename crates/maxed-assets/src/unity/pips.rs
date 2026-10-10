//! Enchant pips: the client's `RarityIcon_1..4` sprites, resolved by name through
//! SpriteAtlas render data. The GUI atlas is repacked on game patches, so nothing
//! here depends on its hashed texture name or on fixed rectangles.
use super::*;
use crate::reader::Reader;

/// Cache sheet written by the extractor: pip `n` occupies x = (n - 1) * 16.
pub const PIP_SHEET: &str = "enchantPips";
pub const PIP_SIZE: u32 = 16;
pub const PIP_COUNT: u32 = 4;
const SPRITE: i32 = 213;
const SPRITE_ATLAS: i32 = 687_078_895;
const ATLAS_LIMIT: usize = 16 * 1024 * 1024;

type RenderKey = ([u8; 16], i64);
type PPtr = (i32, i64);

struct Atlas {
    sprites: Vec<(String, PPtr)>,
    render: Vec<(RenderKey, AtlasEntry)>,
}
#[derive(Clone, Copy)]
struct AtlasEntry {
    texture: PPtr,
    /// x, y (bottom-up texture coordinates), width, height. Other sprites may
    /// use fractional rects; only the pips' are required to be whole pixels.
    rect: [f32; 4],
    settings: u32,
}
struct SpriteHeader {
    name: String,
    key: RenderKey,
    atlas: PPtr,
}

/// File-backed location of the four pips inside one atlas texture.
pub struct EnchantPips {
    file: File,
    offset: u64,
    texture_width: u32,
    rects: [[u32; 4]; PIP_COUNT as usize],
    pub(crate) companion: Option<PathBuf>,
}

fn pptr(r: &mut Reader<'_>) -> Result<PPtr, AssetError> {
    Ok((r.i32()?, r.u64()? as i64))
}
fn key(r: &mut Reader<'_>) -> Result<RenderKey, AssetError> {
    let guid = r
        .take(16)?
        .try_into()
        .map_err(|_| AssetError::InvalidStructure)?;
    Ok((guid, r.u64()? as i64))
}
fn pixel(v: f32) -> Result<u32, AssetError> {
    if !v.is_finite() || !(0.0..=16384.0).contains(&v) || v.fract() != 0.0 {
        return Err(AssetError::InvalidStructure);
    }
    Ok(v as u32)
}

// Type trees are stripped; these field profiles were checked against the
// 6000.0.58f2 client (the atlas parse must end exactly at the object size).
fn parse_atlas(bytes: &[u8], big: bool) -> Result<Atlas, AssetError> {
    let mut r = Reader::new(bytes, big);
    r.string()?; // name
    let packed = r.count(65536)?;
    let mut pointers = Vec::with_capacity(packed);
    for _ in 0..packed {
        pointers.push(pptr(&mut r)?);
    }
    if r.count(65536)? != packed {
        return Err(AssetError::InvalidStructure);
    }
    let mut sprites = Vec::with_capacity(packed);
    for pointer in pointers {
        sprites.push((r.string()?, pointer));
    }
    let entries = r.count(65536)?;
    let mut render = Vec::with_capacity(entries);
    for _ in 0..entries {
        let key = key(&mut r)?;
        let texture = pptr(&mut r)?;
        pptr(&mut r)?; // alpha texture
        let mut rect = [0.0; 4];
        for v in &mut rect {
            *v = f32::from_bits(r.u32()?);
        }
        r.skip(8 + 8 + 16 + 4)?; // rect offset, atlas offset, uv transform, downscale
        let settings = r.u32()?;
        for _ in 0..r.count(64)? {
            pptr(&mut r)?;
            r.string()?;
        }
        render.push((
            key,
            AtlasEntry {
                texture,
                rect,
                settings,
            },
        ));
    }
    r.string()?; // tag
    r.boolean()?; // variant
    r.align()?;
    if r.pos != bytes.len() {
        return Err(AssetError::InvalidStructure);
    }
    Ok(Atlas { sprites, render })
}

fn parse_sprite(bytes: &[u8], big: bool) -> Result<SpriteHeader, AssetError> {
    let mut r = Reader::new(bytes, big);
    let name = r.string()?;
    r.skip(16 + 8 + 16 + 4 + 8 + 4)?; // rect, offset, border, pixels/unit, pivot, extrude
    r.boolean()?; // polygon
    r.align()?;
    let key = key(&mut r)?;
    for _ in 0..r.count(64)? {
        r.string()?; // atlas tags
    }
    let atlas = pptr(&mut r)?;
    Ok(SpriteHeader { name, key, atlas })
}

/// Locates the pips without reading pixels. `None` when this client has no
/// usable pip art (missing, ambiguous, rotated or differently sized sprites):
/// pips are decoration and must never block the catalog.
pub fn locate_enchant_pips(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<EnchantPips>, AssetError> {
    let (mut file, header, serialized) = open(path, cancel)?;
    match locate(path, &mut file, &header, &serialized, cancel) {
        Ok(pips) => Ok(Some(pips)),
        Err(e @ (AssetError::Cancelled | AssetError::Io)) => Err(e),
        Err(_) => Ok(None),
    }
}

fn locate(
    path: &Path,
    file: &mut File,
    header: &Header,
    serialized: &Serialized,
    cancel: &AtomicBool,
) -> Result<EnchantPips, AssetError> {
    if serialized.unity_version != "6000.0.58f2" {
        return Err(AssetError::UnsupportedVersion);
    }
    let by_id = |id: i64, class: i32| {
        serialized
            .objects
            .iter()
            .find(|o| o.path_id == id && o.class == class)
            .ok_or(AssetError::InvalidStructure)
    };
    let names: Vec<String> = (1..=PIP_COUNT).map(|n| format!("RarityIcon_{n}")).collect();
    // (atlas path id, sprite pointer, render entries of that atlas) per pip.
    let mut found: Vec<Option<(i64, PPtr, usize)>> = vec![None; names.len()];
    let mut atlases = Vec::new();
    for object in serialized
        .objects
        .iter()
        .filter(|o| o.class == SPRITE_ATLAS)
    {
        cancelled(cancel)?;
        if object.size as usize > ATLAS_LIMIT {
            return Err(AssetError::LimitExceeded);
        }
        // An atlas outside the checked profile cannot hold the pips we resolve;
        // skipping it keeps the pip lookup independent of unrelated atlases.
        let bytes = read_span(file, object.offset, object.size as usize)?;
        let Ok(atlas) = parse_atlas(&bytes, header.big) else {
            continue;
        };
        let mut used = false;
        for (name, pointer) in &atlas.sprites {
            if let Some(i) = names.iter().position(|n| n == name) {
                if found[i].is_some() {
                    return Err(AssetError::InvalidStructure);
                }
                found[i] = Some((object.path_id, *pointer, atlases.len()));
                used = true;
            }
        }
        if used {
            atlases.push(atlas.render);
        }
    }
    let mut entries = Vec::with_capacity(names.len());
    for (name, found) in names.iter().zip(found) {
        let (atlas_id, (file_id, sprite_id), atlas) = found.ok_or(AssetError::InvalidStructure)?;
        let object = by_id(sprite_id, SPRITE)?;
        if file_id != 0 {
            return Err(AssetError::InvalidStructure);
        }
        let sprite = parse_sprite(
            &read_span(file, object.offset, (object.size as usize).min(8192))?,
            header.big,
        )?;
        if &sprite.name != name || sprite.atlas != (0, atlas_id) {
            return Err(AssetError::InvalidStructure);
        }
        let entry = atlases[atlas]
            .iter()
            .find(|(k, _)| *k == sprite.key)
            .map(|(_, e)| *e)
            .ok_or(AssetError::InvalidStructure)?;
        // Packed sprites only; rotated packing would need a transform.
        if entry.settings & 1 == 0 || (entry.settings >> 2) & 0xf != 0 {
            return Err(AssetError::UnsupportedTexture);
        }
        entries.push(entry);
    }
    let texture = entries[0].texture;
    if texture.0 != 0 || entries.iter().any(|e| e.texture != texture) {
        return Err(AssetError::InvalidStructure);
    }
    let object = by_id(texture.1, 28)?;
    let info = texture_header(file, object, header.big, &serialized.unity_version)?;
    let mut rects = [[0; 4]; PIP_COUNT as usize];
    for (rect, entry) in rects.iter_mut().zip(&entries) {
        let [x, y, w, h] = entry.rect.map(pixel);
        let (x, y, w, h) = (x?, y?, w?, h?);
        if w != PIP_SIZE || h != PIP_SIZE || x + w > info.width || y + h > info.height {
            return Err(AssetError::UnsupportedTexture);
        }
        *rect = [x, y, w, h];
    }
    let (pixels, offset, companion) = if info.image_bytes > 0 {
        (File::open(path)?, object.offset + info.inline_offset, None)
    } else {
        let companion = safe_companion(path, &info.stream_path)?;
        (File::open(&companion)?, info.stream_offset, Some(companion))
    };
    checked_span(
        offset,
        info.decoded_size()? as u64,
        pixels.metadata()?.len(),
    )?;
    Ok(EnchantPips {
        file: pixels,
        offset,
        texture_width: info.width,
        rects,
        companion,
    })
}

impl EnchantPips {
    pub fn info(&self) -> SheetInfo {
        SheetInfo {
            name: PIP_SHEET.into(),
            width: PIP_SIZE * PIP_COUNT,
            height: PIP_SIZE,
        }
    }
    /// Top-down RGBA for the `PIP_COUNT * 16` x 16 strip.
    pub fn read_rgba(&mut self, cancel: &AtomicBool) -> Result<Vec<u8>, AssetError> {
        let stride = (PIP_SIZE * PIP_COUNT * 4) as usize;
        let row = (PIP_SIZE * 4) as usize;
        let mut out = vec![0; stride * PIP_SIZE as usize];
        for (i, [x, y, _, _]) in self.rects.iter().enumerate() {
            for r in 0..PIP_SIZE {
                cancelled(cancel)?;
                // Unity stores rows bottom-up; rect y is the bottom edge.
                let source =
                    u64::from(y + PIP_SIZE - 1 - r) * u64::from(self.texture_width) + u64::from(*x);
                self.file.seek(SeekFrom::Start(self.offset + source * 4))?;
                let at = r as usize * stride + i * row;
                self.file.read_exact(&mut out[at..at + row])?;
            }
        }
        Ok(out)
    }
    pub fn write_png(&mut self, output: &mut File, cancel: &AtomicBool) -> Result<(), AssetError> {
        let rgba = self.read_rgba(cancel)?;
        let info = self.info();
        let mut encoder = png::Encoder::new(&mut *output, info.width, info.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| AssetError::Io)?;
        writer.write_image_data(&rgba).map_err(|_| AssetError::Io)?;
        writer.finish().map_err(|_| AssetError::Io)?;
        output.sync_all()?;
        Ok(())
    }
}
