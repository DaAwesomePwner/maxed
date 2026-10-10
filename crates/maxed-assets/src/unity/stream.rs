use super::*;
use std::io::Write;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SheetInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
}

/// File-backed pixels. No full-sheet allocation, even during PNG publication.
pub struct SheetReader {
    info: SheetInfo,
    file: File,
    offset: u64,
    span_band: [Vec<u8>; 2],
    span_start: [Option<u32>; 2],
    pub(crate) companion: Option<PathBuf>,
}

impl SheetReader {
    pub fn info(&self) -> &SheetInfo {
        &self.info
    }
    pub fn read_row(
        &mut self,
        y: u32,
        output: &mut [u8],
        cancel: &AtomicBool,
    ) -> Result<(), AssetError> {
        cancelled(cancel)?;
        let row_size = self.info.width as usize * 4;
        if y >= self.info.height || output.len() != row_size {
            return Err(AssetError::InvalidStructure);
        }
        let offset = self.offset + u64::from(self.info.height - 1 - y) * row_size as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(output)?;
        Ok(())
    }
    /// Read only the requested top-down span from file-backed Unity RGBA.
    pub(crate) fn read_span_row(
        &mut self,
        x: u32,
        y: u32,
        output: &mut [u8],
        cancel: &AtomicBool,
    ) -> Result<(), AssetError> {
        cancelled(cancel)?;
        if !output.len().is_multiple_of(4)
            || output.is_empty()
            || y >= self.info.height
            || u64::from(x) + (output.len() / 4) as u64 > u64::from(self.info.width)
        {
            return Err(AssetError::InvalidStructure);
        }
        // Cache two 16-row bands, <=2 MiB at the maximum sheet width. The
        // source is bottom-up; unaligned 16px tiles reuse both adjacent bands.
        let start = y / 16 * 16;
        let slot = (start / 16 % 2) as usize;
        let rows = 16.min(self.info.height - start);
        let stride = self.info.width as usize * 4;
        if self.span_start[slot] != Some(start) {
            self.span_start[slot] = None;
            self.span_band[slot].resize(rows as usize * stride, 0);
            let offset = self.offset + u64::from(self.info.height - start - rows) * stride as u64;
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.read_exact(&mut self.span_band[slot])?;
            self.span_start[slot] = Some(start);
        }
        let at = (rows - 1 - (y - start)) as usize * stride + x as usize * 4;
        output.copy_from_slice(&self.span_band[slot][at..at + output.len()]);
        Ok(())
    }
    pub fn write_png(&mut self, output: &mut File, cancel: &AtomicBool) -> Result<(), AssetError> {
        let mut encoder = png::Encoder::new(&mut *output, self.info.width, self.info.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|_| AssetError::Io)?;
        {
            let mut stream = writer.stream_writer().map_err(|_| AssetError::Io)?;
            let mut row = vec![0; self.info.width as usize * 4];
            for y in 0..self.info.height {
                self.read_row(y, &mut row, cancel)?;
                stream.write_all(&row)?;
            }
            stream.finish().map_err(|_| AssetError::Io)?;
        }
        writer.finish().map_err(|_| AssetError::Io)?;
        output.sync_all()?;
        Ok(())
    }
}

pub fn open_sheets(path: &Path, cancel: &AtomicBool) -> Result<Vec<SheetReader>, AssetError> {
    let (mut file, header, serialized) = open(path, cancel)?;
    let mut sheets = Vec::new();
    for object in &serialized.objects {
        cancelled(cancel)?;
        if object.class != 28 {
            continue;
        }
        let bytes = read_span(&mut file, object.offset, (object.size as usize).min(8192))?;
        let name = crate::reader::Reader::new(&bytes, header.big).string()?;
        if !SHEETS.contains(&name.as_str()) {
            continue;
        }
        if sheets.iter().any(|s: &SheetReader| s.info.name == name) {
            return Err(AssetError::InvalidStructure);
        }
        let info = texture_header(&mut file, object, header.big, &serialized.unity_version)?;
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
        sheets.push(SheetReader {
            info: SheetInfo {
                name,
                width: info.width,
                height: info.height,
            },
            file: pixels,
            span_band: [Vec::new(), Vec::new()],
            span_start: [None, None],
            offset,
            companion,
        });
    }
    sheets.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    Ok(sheets)
}

/// Reads only text payloads; pixel storage remains file-backed via `open_sheets`.
pub fn read_text_resources(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<ExtractedAssets, AssetError> {
    let (mut file, header, serialized) = open(path, cancel)?;
    let mut out = ExtractedAssets {
        english: Default::default(),
        texts: Vec::new(),
        textures: Vec::new(),
        sprite_mapping: Vec::new(),
    };
    let mut retained = 0usize;
    for object in serialized
        .objects
        .iter()
        .filter(|o| matches!(o.class, 49 | 114))
    {
        cancelled(cancel)?;
        if object.class == 114 {
            localization::read(
                &mut file,
                object,
                header.big,
                cancel,
                &mut out.english,
                &mut retained,
                128 * 1024 * 1024,
            )?;
            continue;
        }
        if object.size as usize > 64 * 1024 * 1024 + 8192 {
            return Err(AssetError::LimitExceeded);
        }
        retained = retained
            .checked_add(object.size as usize + 128)
            .ok_or(AssetError::LimitExceeded)?;
        // Reserve the other half of the extraction budget for parsing and catalog.
        if retained > 128 * 1024 * 1024 {
            return Err(AssetError::LimitExceeded);
        }
        let bytes = read_span(&mut file, object.offset, object.size as usize)?;
        let text = text::parse(bytes, header.big)?;
        if text.name == "spritesheetf" {
            if !out.sprite_mapping.is_empty() {
                return Err(AssetError::InvalidStructure);
            }
            out.sprite_mapping = text.bytes;
        } else {
            out.texts.push(text);
        }
    }
    cancelled(cancel)?;
    Ok(out)
}

#[cfg(test)]
mod band_tests {
    use super::*;
    #[test]
    fn unaligned_tiles_reuse_both_bands_and_short_last_band_is_top_down() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rgba");
        let (width, height) = (32u32, 29u32);
        let pixels: Vec<u8> = (0..height)
            .rev()
            .flat_map(|y| (0..width).flat_map(move |x| [x as u8, y as u8, 0, 255]))
            .collect();
        std::fs::write(&path, pixels).unwrap();
        let mut sheet = SheetReader {
            info: SheetInfo {
                name: "synthetic".into(),
                width,
                height,
            },
            file: File::open(&path).unwrap(),
            offset: 0,
            span_band: [Vec::new(), Vec::new()],
            span_start: [None, None],
            companion: None,
        };
        let cancel = AtomicBool::new(false);
        for x in [0, 16] {
            for y in 8..24 {
                let mut row = [0; 64];
                sheet.read_span_row(x, y, &mut row, &cancel).unwrap();
                for (i, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
                    assert_eq!(*pixel, [x as u8 + i as u8, y as u8, 0, 255]);
                }
            }
            assert_eq!(sheet.span_start, [Some(0), Some(16)]);
            // A cached second tile must not attempt any file read.
            sheet.file = File::open({
                let empty = dir.path().join("empty");
                std::fs::write(&empty, []).unwrap();
                empty
            })
            .unwrap();
        }
        let mut row = [0; 4];
        sheet.read_span_row(31, 28, &mut row, &cancel).unwrap();
        assert_eq!(row, [31, 28, 0, 255]);
        assert_eq!(sheet.span_band[1].len(), 13 * 32 * 4);
    }
}
