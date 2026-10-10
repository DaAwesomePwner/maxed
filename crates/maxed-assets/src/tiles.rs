//! Immutable PNG tile pack with a bounded sorted index read positionally.
//! Extraction retains one <=256x256 RGBA tile, never a decoded atlas.
use crate::{unity::cancelled, AssetError};
use maxed_types::SpriteRef;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

pub(crate) const PACK: &str = "tiles.pack";
const MAGIC: &[u8; 8] = b"MXTILE01";
const HEADER: u64 = 76;
const ENTRY: u64 = 64;
const MAX_TILES: usize = 262144;
pub(crate) const PACK_LIMIT: u64 = 512 * 1024 * 1024;
const SHEETS: [&str; 5] = [
    "characters.png",
    "characters_masks.png",
    "groundTiles.png",
    "mapObjects.png",
    "enchantPips.png",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TileKey {
    pub(crate) sheet: u32,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}
impl TileKey {
    pub fn from_sprite(s: &SpriteRef) -> Result<Self, AssetError> {
        Self::new(&s.sheet, s.x, s.y, s.width, s.height)
    }
    pub(crate) fn new(
        sheet: &str,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<Self, AssetError> {
        let sheet = SHEETS
            .iter()
            .position(|s| *s == sheet)
            .ok_or(AssetError::InvalidCatalog)? as u32;
        if width == 0
            || height == 0
            || width > 256
            || height > 256
            || x.checked_add(width).is_none_or(|end| end > 16384)
            || y.checked_add(height).is_none_or(|end| end > 16384)
        {
            return Err(AssetError::InvalidCatalog);
        }
        Ok(Self {
            sheet,
            x,
            y,
            width,
            height,
        })
    }
    pub(crate) fn sheet_name(self) -> &'static str {
        SHEETS[self.sheet as usize]
    }
    fn words(self) -> [u32; 5] {
        [self.sheet, self.x, self.y, self.width, self.height]
    }
    fn from_entry(bytes: &[u8; 64]) -> Result<Self, AssetError> {
        let mut words = [0; 5];
        for (word, bytes) in words.iter_mut().zip(bytes[..20].as_chunks::<4>().0.iter()) {
            *word = u32::from_le_bytes(*bytes);
        }
        Self::new(
            SHEETS
                .get(words[0] as usize)
                .ok_or(AssetError::CacheInvalid)?,
            words[1],
            words[2],
            words[3],
            words[4],
        )
    }
}

pub(crate) fn png_limit(width: u32, height: u32) -> usize {
    (width * height * 4 + height) as usize + 1024
}
fn keys(sprites: &[SpriteRef]) -> Result<Vec<TileKey>, AssetError> {
    if sprites.len() > MAX_TILES {
        return Err(AssetError::LimitExceeded);
    }
    // Oversized refs already render as fallbacks in SpriteImage. Do not encode
    // them, or let an otherwise valid large catalog rect defeat the tile bound.
    let mut keys = Vec::with_capacity(sprites.len());
    for s in sprites {
        if s.width <= 256 && s.height <= 256 {
            keys.push(TileKey::from_sprite(s)?);
        }
    }
    keys.sort_unstable();
    keys.dedup();
    // Cap total one-time work even for a catalog full of overlapping large refs.
    let rgba_bytes: u64 = keys
        .iter()
        .map(|k| u64::from(k.width) * u64::from(k.height) * 4)
        .sum();
    if rgba_bytes > 1024 * 1024 * 1024 {
        return Err(AssetError::LimitExceeded);
    }
    Ok(keys)
}

pub(crate) fn write_pack(
    root: &Path,
    fingerprint: &str,
    sprites: &[SpriteRef],
    cancel: &AtomicBool,
    mut row: impl FnMut(TileKey, u32, &mut [u8]) -> Result<(), AssetError>,
) -> Result<(), AssetError> {
    cancelled(cancel)?;
    if !valid_fingerprint(fingerprint) {
        return Err(AssetError::InvalidCatalog);
    }
    let keys = keys(sprites)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(PACK))?;
    file.write_all(MAGIC)?;
    file.write_all(fingerprint.as_bytes())?;
    file.write_all(&(keys.len() as u32).to_le_bytes())?;
    let mut offset = HEADER + ENTRY * keys.len() as u64;
    file.set_len(offset)?;
    // Keep the disk index in its original key order; visit source rows together.
    let mut extraction: Vec<_> = keys.into_iter().enumerate().collect();
    extraction.sort_unstable_by_key(|(_, k)| (k.sheet, k.y, k.x, k.width, k.height));
    for (i, key) in extraction {
        cancelled(cancel)?;
        let mut pixels = vec![0; (key.width * key.height * 4) as usize];
        for (y, bytes) in pixels.chunks_exact_mut(key.width as usize * 4).enumerate() {
            cancelled(cancel)?;
            row(key, y as u32, bytes)?;
        }
        // The fixed slice bounds encoded output, not encoder working memory:
        // png also grows an internal Vec while compressing the tile.
        let mut encoded = vec![0; png_limit(key.width, key.height)];
        let mut output = std::io::Cursor::new(encoded.as_mut_slice());
        {
            let mut encoder = png::Encoder::new(&mut output, key.width, key.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut writer = encoder.write_header().map_err(|_| AssetError::Io)?;
            writer
                .write_image_data(&pixels)
                .map_err(|_| AssetError::Io)?;
            writer.finish().map_err(|_| AssetError::Io)?;
        }
        let len = output.position() as usize;
        encoded.truncate(len);
        let end = offset
            .checked_add(len as u64)
            .filter(|n| *n <= PACK_LIMIT)
            .ok_or(AssetError::LimitExceeded)?;
        let mut entry = [0; 64];
        for (word, target) in key
            .words()
            .iter()
            .zip(entry[..20].as_chunks_mut::<4>().0.iter_mut())
        {
            target.copy_from_slice(&word.to_le_bytes());
        }
        entry[20..28].copy_from_slice(&offset.to_le_bytes());
        entry[28..32].copy_from_slice(&(len as u32).to_le_bytes());
        entry[32..].copy_from_slice(&Sha256::digest(&encoded));
        file.seek(SeekFrom::Start(HEADER + i as u64 * ENTRY))?;
        file.write_all(&entry)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&encoded)?;
        offset = end;
    }
    file.sync_all()?;
    Ok(())
}

pub(crate) fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn open_pack(root: &Path, fingerprint: &str) -> Result<(File, u32), AssetError> {
    if !valid_fingerprint(fingerprint) || crate::cache::link(root)? {
        return Err(AssetError::CacheInvalid);
    }
    let root = root.canonicalize()?;
    let path = root.join(PACK);
    if crate::cache::link(&path)? || path.canonicalize()?.parent() != Some(root.as_path()) {
        return Err(AssetError::CacheInvalid);
    }
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > PACK_LIMIT {
        return Err(AssetError::CacheInvalid);
    }
    let mut header = [0; HEADER as usize];
    file.read_exact(&mut header)?;
    let count = u32::from_le_bytes(header[72..].try_into().unwrap());
    if &header[..8] != MAGIC
        || &header[8..72] != fingerprint.as_bytes()
        || count as usize > MAX_TILES
        || HEADER + u64::from(count) * ENTRY > meta.len()
    {
        return Err(AssetError::CacheInvalid);
    }
    Ok((file, count))
}
fn read_index(file: &mut File, count: u32) -> Result<Vec<[u8; 64]>, AssetError> {
    // open_pack bounds count, so this single allocation/read is <=16 MiB.
    let mut index = vec![[0; 64]; count as usize];
    file.read_exact(index.as_flattened_mut())?;
    Ok(index)
}

/// An open immutable pack and scalar metadata per lease. Binary search uses
/// positional reads: no resident index, shared cursor, or delivery mutex.
#[derive(Debug)]
pub(crate) struct TilePack {
    file: File,
    count: u32,
    len: u64,
}
fn read_at(file: &File, mut bytes: &mut [u8], mut offset: u64) -> Result<(), AssetError> {
    while !bytes.is_empty() {
        #[cfg(windows)]
        let n = {
            use std::os::windows::fs::FileExt;
            file.seek_read(bytes, offset)?
        };
        #[cfg(unix)]
        let n = {
            use std::os::unix::fs::FileExt;
            file.read_at(bytes, offset)?
        };
        if n == 0 {
            return Err(AssetError::CacheInvalid);
        }
        offset += n as u64;
        bytes = &mut bytes[n..];
    }
    Ok(())
}
impl TilePack {
    pub(crate) fn open(
        root: &Path,
        fingerprint: &str,
        expected_len: u64,
    ) -> Result<Self, AssetError> {
        let (file, count) = open_pack(root, fingerprint)?;
        let len = file.metadata()?.len();
        if len != expected_len {
            return Err(AssetError::CacheInvalid);
        }
        // Check ordering/spans once with a bounded sequential buffer, then drop it.
        let mut reader = BufReader::with_capacity(65536, file);
        let mut previous = None;
        for _ in 0..count {
            let mut bytes = [0; 64];
            reader.read_exact(&mut bytes)?;
            let key = TileKey::from_entry(&bytes)?;
            if previous.is_some_and(|p| p >= key) {
                return Err(AssetError::CacheInvalid);
            }
            span(&bytes, key, count, len)?;
            previous = Some(key);
        }
        Ok(Self {
            file: reader.into_inner(),
            count,
            len,
        })
    }
    pub(crate) fn read(&self, key: TileKey) -> Result<Vec<u8>, AssetError> {
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            let mut entry = [0; 64];
            read_at(&self.file, &mut entry, HEADER + u64::from(mid) * ENTRY)?;
            match TileKey::from_entry(&entry)?.cmp(&key) {
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
                std::cmp::Ordering::Equal => {
                    let (offset, len) = span(&entry, key, self.count, self.len)?;
                    let mut png = vec![0; len];
                    read_at(&self.file, &mut png, offset)?;
                    if !png_header(&png, key) || Sha256::digest(&png)[..] != entry[32..] {
                        return Err(AssetError::CacheInvalid);
                    }
                    return Ok(png);
                }
            }
        }
        Err(AssetError::InvalidCatalog)
    }
}

fn span(
    entry: &[u8; 64],
    key: TileKey,
    count: u32,
    file_len: u64,
) -> Result<(u64, usize), AssetError> {
    let offset = u64::from_le_bytes(entry[20..28].try_into().unwrap());
    let len = u32::from_le_bytes(entry[28..32].try_into().unwrap()) as usize;
    if !(57..=png_limit(key.width, key.height)).contains(&len)
        || offset < HEADER + u64::from(count) * ENTRY
        || offset
            .checked_add(len as u64)
            .is_none_or(|end| end > file_len)
    {
        return Err(AssetError::CacheInvalid);
    }
    Ok((offset, len))
}
fn png_header(bytes: &[u8], key: TileKey) -> bool {
    bytes.len() >= 33
        && bytes[..8] == *b"\x89PNG\r\n\x1a\n"
        && bytes[8..16] == *b"\0\0\0\rIHDR"
        && bytes[16..20] == key.width.to_be_bytes()
        && bytes[20..24] == key.height.to_be_bytes()
        && bytes[24..29] == [8, 6, 0, 0, 0]
}

#[cfg(test)]
fn read_tile(root: &Path, fingerprint: &str, key: TileKey) -> Result<Vec<u8>, AssetError> {
    TilePack::open(root, fingerprint, std::fs::metadata(root.join(PACK))?.len())?.read(key)
}

pub(crate) fn validate_pack(
    root: &Path,
    fingerprint: &str,
    sprites: &[SpriteRef],
    cancel: &AtomicBool,
) -> Result<(), AssetError> {
    let keys = keys(sprites)?;
    let (mut file, count) = open_pack(root, fingerprint)?;
    if count as usize != keys.len() {
        return Err(AssetError::CacheInvalid);
    }
    let file_len = file.metadata()?.len();
    let index = read_index(&mut file, count)?;
    let mut spans = Vec::with_capacity(keys.len());
    for (key, bytes) in keys.into_iter().zip(&index) {
        cancelled(cancel)?;
        if TileKey::from_entry(bytes)? != key {
            return Err(AssetError::CacheInvalid);
        }
        let (offset, len) = span(bytes, key, count, file_len)?;
        spans.push((offset, len, key));
    }
    // Extraction is row-friendly while the lookup index retains its old order.
    // Validate every payload in disk order, rejecting gaps/overlaps as before.
    spans.sort_unstable_by_key(|s| s.0);
    let mut file = BufReader::with_capacity(65536, file);
    let mut end = HEADER + u64::from(count) * ENTRY;
    for (offset, len, key) in spans {
        cancelled(cancel)?;
        if offset != end {
            return Err(AssetError::CacheInvalid);
        }
        let mut header = [0; 33];
        file.read_exact(&mut header)?;
        if !png_header(&header, key) {
            return Err(AssetError::CacheInvalid);
        }
        file.seek_relative((len - header.len()) as i64)?;
        end += len as u64;
    }
    if end != file_len {
        return Err(AssetError::CacheInvalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::SpriteRef;
    use std::io::Cursor;

    fn sprite(x: u32, y: u32, width: u32, height: u32) -> SpriteRef {
        SpriteRef {
            key: format!("{x}:{y}:{width}:{height}"),
            sheet: "characters.png".into(),
            x,
            y,
            width,
            height,
            trim: None,
        }
    }
    #[test]
    fn tiles_preserve_rgba_deduplicate_and_reject_unknown_or_invalid_rectangles() {
        let dir = tempfile::tempdir().unwrap();
        let fingerprint = "a".repeat(64);
        let sprites = vec![
            sprite(3, 5, 8, 12),
            sprite(3, 5, 8, 12),
            sprite(15, 15, 1, 1),
            sprite(0, 16, 4, 4),
            sprite(16, 0, 2, 2),
        ];
        let pixel = |x: u32, y: u32| [x as u8, y as u8, (x + y) as u8, (x * y) as u8];
        let mut rows = 0;
        let mut order = Vec::new();
        write_pack(
            dir.path(),
            &fingerprint,
            &sprites,
            &AtomicBool::new(false),
            |key, y, row| {
                rows += 1;
                if y == 0 {
                    order.push((key.x, key.y));
                }
                for (i, rgba) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    rgba.copy_from_slice(&pixel(key.x + i as u32, key.y + y));
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(rows, 19); // Duplicate rectangles are encoded once.
        assert_eq!(order, [(16, 0), (3, 5), (15, 15), (0, 16)]);
        validate_pack(dir.path(), &fingerprint, &sprites, &AtomicBool::new(false)).unwrap();
        let mut catalog = maxed_types::demo::demo_catalog();
        catalog.client_fingerprint = fingerprint;
        catalog.sprites = sprites;
        for s in &catalog.sprites {
            let bytes = read_tile(
                dir.path(),
                &catalog.client_fingerprint,
                TileKey::from_sprite(s).unwrap(),
            )
            .unwrap();
            assert!(bytes.len() <= png_limit(s.width, s.height));
            let mut decoder = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
            let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
            let info = decoder.next_frame(&mut pixels).unwrap();
            assert_eq!((info.width, info.height), (s.width, s.height));
            for y in 0..s.height {
                for x in 0..s.width {
                    let at = ((y * s.width + x) * 4) as usize;
                    assert_eq!(&pixels[at..at + 4], &pixel(s.x + x, s.y + y));
                }
            }
        }
        assert!(read_tile(
            dir.path(),
            &"b".repeat(64),
            TileKey::from_sprite(&catalog.sprites[0]).unwrap()
        )
        .is_err());
        assert!(read_tile(
            dir.path(),
            &catalog.client_fingerprint,
            TileKey::from_sprite(&sprite(0, 0, 1, 1)).unwrap()
        )
        .is_err());
        for s in [
            sprite(0, 0, 0, 1),
            sprite(16384, 0, 1, 1),
            sprite(u32::MAX, 0, 1, 1),
            sprite(0, 0, 257, 1),
        ] {
            assert!(TileKey::from_sprite(&s).is_err());
        }
    }
    #[test]
    fn corrupt_spans_payloads_and_indices_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fingerprint = "a".repeat(64);
        let sprites = [sprite(1, 2, 8, 8)];
        let cancel = AtomicBool::new(false);
        write_pack(dir.path(), &fingerprint, &sprites, &cancel, |_, _, row| {
            row.fill(255);
            Ok(())
        })
        .unwrap();
        validate_pack(dir.path(), &fingerprint, &sprites, &cancel).unwrap();
        let path = dir.path().join(PACK);
        let original = std::fs::read(&path).unwrap();
        let key = TileKey::from_sprite(&sprites[0]).unwrap();
        for case in 0..5 {
            let mut bytes = original.clone();
            match case {
                0 => bytes[104..108].copy_from_slice(&u32::MAX.to_le_bytes()), // huge response length
                1 => bytes[96..104].copy_from_slice(&u64::MAX.to_le_bytes()),  // overflowing offset
                2 => bytes[72..76].copy_from_slice(&u32::MAX.to_le_bytes()),   // huge index
                3 => *bytes.last_mut().unwrap() ^= 1,                          // payload hash
                4 => bytes[76..80].copy_from_slice(&u32::MAX.to_le_bytes()),   // invalid sheet
                _ => unreachable!(),
            }
            std::fs::write(&path, bytes).unwrap();
            assert!(
                read_tile(dir.path(), &fingerprint, key).is_err(),
                "case {case}"
            );
        }
    }

    #[test]
    fn positional_delivery_is_concurrent_and_has_no_resident_index() {
        let dir = tempfile::tempdir().unwrap();
        let fp = "a".repeat(64);
        let sprites: Vec<_> = (0..64).map(|x| sprite(x * 8, 8, 8, 8)).collect();
        write_pack(
            dir.path(),
            &fp,
            &sprites,
            &AtomicBool::new(false),
            |key, y, row| {
                row.fill((key.x + y) as u8);
                Ok(())
            },
        )
        .unwrap();
        let pack = TilePack::open(
            dir.path(),
            &fp,
            std::fs::metadata(dir.path().join(PACK)).unwrap().len(),
        )
        .unwrap();
        assert!(std::mem::size_of::<TilePack>() <= 32);
        std::thread::scope(|scope| {
            for s in &sprites {
                let pack = &pack;
                scope.spawn(move || {
                    let key = TileKey::from_sprite(s).unwrap();
                    for _ in 0..20 {
                        let bytes = pack.read(key).unwrap();
                        let mut decoder =
                            png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
                        let mut rgba = [0; 256];
                        decoder.next_frame(&mut rgba).unwrap();
                        assert_eq!(rgba[0], s.x as u8);
                        assert_eq!(rgba[224], (s.x + 7) as u8);
                    }
                });
            }
        });
        println!(
            "resident tile index: 0 bytes; TilePack scalars/file: {} bytes",
            std::mem::size_of::<TilePack>()
        );
    }

    #[test]
    fn tile_extraction_checks_cancellation_between_rows() {
        let dir = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let mut rows = 0;
        let result = write_pack(
            dir.path(),
            &"a".repeat(64),
            &[sprite(0, 0, 8, 8)],
            &flag,
            |_, _, _| {
                rows += 1;
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            },
        );
        assert!(matches!(result, Err(AssetError::Cancelled)));
        assert_eq!(rows, 1);
    }

    #[test]
    #[ignore = "release-mode synthetic timing; run explicitly"]
    fn synthetic_catalog_scale_tile_pack_cost() {
        let dir = tempfile::tempdir().unwrap();
        // Similar count to the historical client catalog, synthetic 16x16 art.
        let sprites: Vec<_> = (0..66807)
            .map(|i| sprite((i % 512) * 16 + 8, (i / 512) * 16 + 8, 16, 16))
            .collect();
        let start = std::time::Instant::now();
        write_pack(
            dir.path(),
            &"a".repeat(64),
            &sprites,
            &AtomicBool::new(false),
            |key, y, row| {
                for (x, rgba) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    *rgba = [
                        (key.x + x as u32) as u8,
                        (key.y + y) as u8,
                        x as u8,
                        (x as u32 * y) as u8,
                    ];
                }
                Ok(())
            },
        )
        .unwrap();
        let elapsed = start.elapsed();
        let bytes = std::fs::metadata(dir.path().join(PACK)).unwrap().len();
        assert!(bytes <= HEADER + sprites.len() as u64 * (ENTRY + png_limit(16, 16) as u64));
        let validation = std::time::Instant::now();
        validate_pack(
            dir.path(),
            &"a".repeat(64),
            &sprites,
            &AtomicBool::new(false),
        )
        .unwrap();
        println!("validation {:?}; 66,807 synthetic 16x16 tiles: {elapsed:?}, {bytes} pack bytes; excludes Unity reads and atlas publication", validation.elapsed());
    }
    #[test]
    #[ignore = "release-mode synthetic timing; run explicitly"]
    fn synthetic_catalog_file_backed_extraction_cost() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("resources.assets");
        std::fs::write(
            &source,
            crate::tiles::synthetic_fixture::fixture_sized(None, 8200, 2104),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let mut sheets = crate::open_sheets(&source, &cancel).unwrap();
        let sprites: Vec<_> = (0..66807)
            .map(|i| sprite((i % 512) * 16 + 8, (i / 512) * 16 + 8, 16, 16))
            .collect();
        let start = std::time::Instant::now();
        write_pack(
            dir.path(),
            &"a".repeat(64),
            &sprites,
            &cancel,
            |key, y, row| sheets[0].read_span_row(key.x, key.y + y, row, &cancel),
        )
        .unwrap();
        let cold = start.elapsed();
        let start = std::time::Instant::now();
        // Same whole-file SHA-256 pass as generation validation.
        let mut file = File::open(dir.path().join(PACK)).unwrap();
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        std::hint::black_box(hash.finalize());
        validate_pack(dir.path(), &"a".repeat(64), &sprites, &cancel).unwrap();
        println!("66,807 file-backed synthetic tiles: cold {cold:?}, SHA-256 + validation {:?}, {} pack bytes; excludes source construction, atlas publication, trims and catalog JSON", start.elapsed(), file.metadata().unwrap().len());
    }
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/common/mod.rs"]
mod synthetic_fixture;
