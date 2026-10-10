use crate::{AssetError, SheetInfo};
use maxed_types::SpriteRef;
use std::collections::BTreeMap;

pub(crate) fn safe_sheet(name: &str) -> bool {
    [
        "characters.png",
        "characters_masks.png",
        "groundTiles.png",
        "mapObjects.png",
        "enchantPips.png",
    ]
    .contains(&name)
}

/// Catalog refs for the extractor's own pip strip; present only when the strip
/// was extracted. Keys `enchantPip:1..4` map to the client's RarityIcon_1..4.
pub(crate) fn enchant_pips(sheets: &[SheetInfo]) -> Vec<SpriteRef> {
    use crate::unity::{PIP_COUNT, PIP_SHEET, PIP_SIZE};
    if !sheets
        .iter()
        .any(|s| s.name == PIP_SHEET && s.width == PIP_SIZE * PIP_COUNT && s.height == PIP_SIZE)
    {
        return vec![];
    }
    (1..=PIP_COUNT)
        .map(|n| SpriteRef {
            key: format!("enchantPip:{n}"),
            sheet: format!("{PIP_SHEET}.png"),
            x: (n - 1) * PIP_SIZE,
            y: 0,
            width: PIP_SIZE,
            height: PIP_SIZE,
            trim: None,
        })
        .collect()
}

struct Buffer<'a>(&'a [u8]);
impl Buffer<'_> {
    fn bytes(&self, p: usize, n: usize) -> Result<&[u8], AssetError> {
        self.0
            .get(p..p.checked_add(n).ok_or(AssetError::InvalidStructure)?)
            .ok_or(AssetError::InvalidStructure)
    }
    fn u16(&self, p: usize) -> Result<u16, AssetError> {
        Ok(u16::from_le_bytes(
            self.bytes(p, 2)?
                .try_into()
                .map_err(|_| AssetError::InvalidStructure)?,
        ))
    }
    fn u32(&self, p: usize) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(
            self.bytes(p, 4)?
                .try_into()
                .map_err(|_| AssetError::InvalidStructure)?,
        ))
    }
    fn u64(&self, p: usize) -> Result<u64, AssetError> {
        Ok(u64::from_le_bytes(
            self.bytes(p, 8)?
                .try_into()
                .map_err(|_| AssetError::InvalidStructure)?,
        ))
    }
    fn indirect(&self, p: usize) -> Result<usize, AssetError> {
        let off = self.u32(p)? as usize;
        if off == 0 {
            return Err(AssetError::InvalidStructure);
        }
        let target = p.checked_add(off).ok_or(AssetError::InvalidStructure)?;
        self.bytes(target, 4)?;
        Ok(target)
    }
    fn field(&self, t: usize, index: usize, size: usize) -> Result<Option<usize>, AssetError> {
        let offset = self.u32(t)? as i32;
        let vtable = i64::try_from(t)
            .map_err(|_| AssetError::InvalidStructure)?
            .checked_sub(i64::from(offset))
            .ok_or(AssetError::InvalidStructure)?;
        let vtable = usize::try_from(vtable).map_err(|_| AssetError::InvalidStructure)?;
        let len = self.u16(vtable)? as usize;
        let object_len = self.u16(vtable + 2)? as usize;
        if len < 4 || !len.is_multiple_of(2) || object_len < 4 {
            return Err(AssetError::InvalidStructure);
        }
        self.bytes(vtable, len)?;
        self.bytes(t, object_len)?;
        if 4 + index * 2 >= len {
            return Ok(None);
        }
        let relative = self.u16(vtable + 4 + index * 2)? as usize;
        if relative == 0 {
            return Ok(None);
        }
        if relative < 4 || relative.checked_add(size).is_none_or(|v| v > object_len) {
            return Err(AssetError::InvalidStructure);
        }
        Ok(Some(t + relative))
    }
    fn required(&self, t: usize, index: usize, size: usize) -> Result<usize, AssetError> {
        self.field(t, index, size)?
            .ok_or(AssetError::InvalidStructure)
    }
    fn string(&self, t: usize, index: usize) -> Result<String, AssetError> {
        let p = self.indirect(self.required(t, index, 4)?)?;
        let len = self.u32(p)? as usize;
        if len > 200 {
            return Err(AssetError::LimitExceeded);
        }
        if self.bytes(p + 4 + len, 1)? != [0] {
            return Err(AssetError::InvalidStructure);
        }
        std::str::from_utf8(self.bytes(p + 4, len)?)
            .map(str::to_owned)
            .map_err(|_| AssetError::InvalidStructure)
    }
    fn vector(&self, t: usize, index: usize, max: usize) -> Result<Vec<usize>, AssetError> {
        let Some(field) = self.field(t, index, 4)? else {
            return Ok(vec![]);
        };
        let p = self.indirect(field)?;
        let len = self.u32(p)? as usize;
        if len > max {
            return Err(AssetError::LimitExceeded);
        }
        self.bytes(p + 4, len * 4)?;
        (0..len).map(|i| self.indirect(p + 4 + i * 4)).collect()
    }
    fn scalar(&self, t: usize, index: usize) -> Result<u32, AssetError> {
        self.field(t, index, 4)?
            .map(|p| self.u32(p))
            .transpose()
            .map(|v| v.unwrap_or(0))
    }
    fn position(&self, p: usize) -> Result<u32, AssetError> {
        let v = f32::from_bits(self.u32(p)?);
        if !v.is_finite() || !(0.0..=16384.0).contains(&v) || v.fract() != 0.0 {
            return Err(AssetError::InvalidStructure);
        }
        Ok(v as u32)
    }
}

/// Fixed-depth FlatBuffer schema from assets/flattbuffer; never uses unchecked offsets.
pub fn parse_sprites(bytes: &[u8], sheets: &[SheetInfo]) -> Result<Vec<SpriteRef>, AssetError> {
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(AssetError::LimitExceeded);
    }
    let b = Buffer(bytes);
    let root = b.indirect(0)?;
    let mut out = BTreeMap::new();
    let mut count = 0;
    let mut retained = 0usize;
    for group in b.vector(root, 0, 4096)? {
        let name = b.string(group, 0)?;
        for sprite in b.vector(group, 2, 262144)? {
            count += 1;
            if count > 262144 {
                return Err(AssetError::LimitExceeded);
            }
            insert(
                &b,
                sprite,
                &name,
                b.scalar(sprite, 3)?,
                sheets,
                &mut out,
                &mut retained,
            )?;
        }
    }
    // Reference SpriteFlatBuffer.decodeSheet explicitly applies animated frames
    // in source order after static sprites; the last frame wins for (group,index).
    for animation in b.vector(root, 1, 262144)? {
        count += 1;
        if count > 262144 {
            return Err(AssetError::LimitExceeded);
        }
        let name = b.string(animation, 0)?;
        let index = b.scalar(animation, 1)?;
        let sprite = b.indirect(b.required(animation, 5, 4)?)?;
        insert(&b, sprite, &name, index, sheets, &mut out, &mut retained)?;
    }
    Ok(out.into_values().collect())
}
fn insert(
    b: &Buffer<'_>,
    sprite: usize,
    group: &str,
    index: u32,
    sheets: &[SheetInfo],
    out: &mut BTreeMap<String, SpriteRef>,
    retained: &mut usize,
) -> Result<(), AssetError> {
    let atlas = b
        .field(sprite, 7, 8)?
        .map(|p| b.u64(p))
        .transpose()?
        .unwrap_or(0);
    let name = *['g', 'c', 'm', 'o']
        .get(
            usize::try_from(atlas.checked_sub(1).ok_or(AssetError::InvalidStructure)?)
                .map_err(|_| AssetError::InvalidStructure)?,
        )
        .ok_or(AssetError::InvalidStructure)?;
    let name = match name {
        'g' => "groundTiles",
        'c' => "characters",
        'm' => "characters_masks",
        _ => "mapObjects",
    };
    let sheet = sheets
        .iter()
        .find(|s| s.name == name)
        .ok_or(AssetError::InvalidCatalog)?;
    let p = b.required(sprite, 0, 16)?;
    let (x, y, height, width) = (
        b.position(p)?,
        b.position(p + 4)?,
        b.position(p + 8)?,
        b.position(p + 12)?,
    );
    if width == 0
        || height == 0
        || x.checked_add(width).is_none_or(|end| end > sheet.width)
        || y.checked_add(height).is_none_or(|end| end > sheet.height)
    {
        return Err(AssetError::InvalidCatalog);
    }
    let key = format!("{group}:{index}");
    if !out.contains_key(&key) {
        *retained = retained
            .checked_add(key.len() * 2 + 224)
            .ok_or(AssetError::LimitExceeded)?;
        if *retained > 48 * 1024 * 1024 {
            return Err(AssetError::LimitExceeded);
        }
    }
    out.insert(
        key.clone(),
        SpriteRef {
            key,
            sheet: format!("{name}.png"),
            x,
            y,
            width,
            height,
            trim: None,
        },
    );
    Ok(())
}

pub(crate) fn trim_sheet<R: std::io::BufRead + std::io::Seek>(
    input: R,
    sheet: &str,
    sprites: &mut [SpriteRef],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), AssetError> {
    use maxed_types::SpriteTrim;
    use std::sync::atomic::Ordering;
    if cancel.load(Ordering::Relaxed) {
        return Err(AssetError::Cancelled);
    }
    let mut reader = png::Decoder::new(input)
        .read_info()
        .map_err(|_| AssetError::InvalidStructure)?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || width == 0
        || height == 0
        || width > 16384
        || height > 16384
    {
        return Err(AssetError::InvalidStructure);
    }
    let mut indices: Vec<usize> = sprites
        .iter()
        .enumerate()
        .filter_map(|(i, s)| (s.sheet == sheet).then_some(i))
        .collect();
    for &i in &indices {
        let s = &mut sprites[i];
        if s.width == 0
            || s.height == 0
            || s.x.checked_add(s.width).is_none_or(|end| end > width)
            || s.y.checked_add(s.height).is_none_or(|end| end > height)
        {
            return Err(AssetError::InvalidCatalog);
        }
        s.trim = None;
    }
    indices.sort_by_key(|&i| sprites[i].y);
    let mut next = 0;
    let mut active: Vec<usize> = Vec::new();
    for y in 0..height {
        if cancel.load(Ordering::Relaxed) {
            return Err(AssetError::Cancelled);
        }
        let row = reader
            .next_row()
            .map_err(|_| AssetError::InvalidStructure)?
            .ok_or(AssetError::InvalidStructure)?;
        let bytes = row.data();
        if bytes.len() != width as usize * 4 {
            return Err(AssetError::InvalidStructure);
        }
        active.retain(|&i| y < sprites[i].y + sprites[i].height);
        while next < indices.len() && sprites[indices[next]].y <= y {
            active.push(indices[next]);
            next += 1;
        }
        for &i in &active {
            let s = &mut sprites[i];
            for x in s.x..s.x + s.width {
                if bytes[x as usize * 4 + 3] == 0 {
                    continue;
                }
                let (rx, ry) = (x - s.x, y - s.y);
                match &mut s.trim {
                    None => {
                        s.trim = Some(SpriteTrim {
                            x: rx,
                            y: ry,
                            width: 1,
                            height: 1,
                        })
                    }
                    Some(t) => {
                        let right = (t.x + t.width).max(rx + 1);
                        let bottom = (t.y + t.height).max(ry + 1);
                        t.x = t.x.min(rx);
                        t.y = t.y.min(ry);
                        t.width = right - t.x;
                        t.height = bottom - t.y;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod trim_tests {
    use super::*;
    use maxed_types::SpriteTrim;
    use std::{io::Cursor, sync::atomic::AtomicBool};

    fn fixture() -> Vec<u8> {
        let mut rgba = vec![0u8; 10 * 8 * 4];
        for y in 3..6 {
            for x in 4..6 {
                rgba[(y * 10 + x) * 4 + 3] = 255;
            }
        }
        rgba[(3 * 10 + 4) * 4 + 3] = 1; // Partial alpha is visible too.
        rgba[3] = 255; // Neighbor outside the sprite must not affect its bounds.
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 10, 8);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&rgba)
                .unwrap();
        }
        bytes
    }
    fn reference(x: u32, y: u32, width: u32, height: u32) -> SpriteRef {
        SpriteRef {
            key: "padded".into(),
            sheet: "characters.png".into(),
            x,
            y,
            width,
            height,
            trim: None,
        }
    }
    #[test]
    fn trim_is_relative_tight_and_survives_catalog_serialization() {
        let mut refs = vec![reference(2, 1, 6, 6), reference(8, 6, 2, 2)];
        trim_sheet(
            Cursor::new(fixture()),
            "characters.png",
            &mut refs,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            refs[0].trim,
            Some(SpriteTrim {
                x: 2,
                y: 2,
                width: 2,
                height: 3
            })
        );
        assert_eq!(refs[1].trim, None); // Fully transparent, no zero-sized crop.
        assert_eq!(
            (refs[0].x, refs[0].y, refs[0].width, refs[0].height),
            (2, 1, 6, 6)
        );
        let json = serde_json::to_vec(&refs).unwrap();
        assert_eq!(
            serde_json::from_slice::<Vec<SpriteRef>>(&json).unwrap(),
            refs
        );
        let mut legacy = serde_json::to_value(&refs[0]).unwrap();
        legacy.as_object_mut().unwrap().remove("trim");
        assert_eq!(
            serde_json::from_value::<SpriteRef>(legacy).unwrap().trim,
            None
        );
    }
    #[test]
    fn trim_checks_bounds_and_cancellation() {
        assert!(trim_sheet(
            Cursor::new(fixture()),
            "characters.png",
            &mut [reference(9, 0, 2, 2)],
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(matches!(
            trim_sheet(
                Cursor::new(fixture()),
                "characters.png",
                &mut [reference(2, 1, 6, 6)],
                &AtomicBool::new(true)
            ),
            Err(AssetError::Cancelled)
        ));
    }
}
