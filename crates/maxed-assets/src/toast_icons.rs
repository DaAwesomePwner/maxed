//! Best-effort portal artwork. Only synthetic images belong in repository tests.
use crate::cache::{exists_without_following, link};
use crate::sprite_delivery::{dungeon_portal_type, sprite_location, SPRITE_ORIGIN};
use maxed_types::{Catalog, SpriteRef};
use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const SIZE: u32 = 96;
const ICON_LIMIT: usize = 64 * 1024;
const DECODE_LIMIT: usize = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

/// `cache_root` contains fingerprint-named sheet generations. The caller keeps
/// its CachedCatalog lease alive until this returns. All failures are optional.
pub fn toast_portal_icon(catalog: &Catalog, cache_root: &Path, dungeon: &str) -> Option<PathBuf> {
    toast_icon(catalog, cache_root, dungeon, |tile| {
        crate::sprite_delivery::SpriteDelivery::open(
            &cache_root.join(&catalog.client_fingerprint),
            &catalog.client_fingerprint,
        )
        .ok()?
        .read(tile)
        .ok()
    })
}

/// Reuse the published lease's delivery for native notifications.
pub fn cached_toast_portal_icon(cache: &crate::CachedCatalog, dungeon: &str) -> Option<PathBuf> {
    toast_icon(&cache.catalog, cache.root.parent()?, dungeon, |tile| {
        cache.delivery.read(tile).ok()
    })
}
fn toast_icon(
    catalog: &Catalog,
    cache_root: &Path,
    dungeon: &str,
    read: impl FnOnce(crate::sprite_delivery::TileKey) -> Option<Vec<u8>>,
) -> Option<PathBuf> {
    let object_type = dungeon_portal_type(catalog, dungeon)?;
    let key = catalog
        .objects
        .iter()
        .find(|o| o.object_type == object_type)?
        .sprite_key
        .as_ref()?;
    let sprite = catalog.sprites.iter().find(|s| &s.key == key)?;
    // Validate path components even on a cache hit.
    let (_, tile) = sprite_location(&format!(
        "{SPRITE_ORIGIN}/{}/{}/{}-{}-{}-{}.png",
        catalog.client_fingerprint, sprite.sheet, sprite.x, sprite.y, sprite.width, sprite.height
    ))
    .ok()?;
    let directory = cache_root
        .join("toast-icons")
        .join(&catalog.client_fingerprint);
    let path = directory.join(format!("{object_type}.png"));
    checked_icon_path(cache_root, &path, true)?;
    if valid_icon(cache_root, &path) {
        return Some(path);
    }
    let sheet = read(tile)?;
    let bytes = crop_png(
        &sheet,
        &SpriteRef {
            x: 0,
            y: 0,
            ..sprite.clone()
        },
    )?;
    checked_icon_path(cache_root, &path, false)?;
    // Publish complete files only: a concurrent toast never sees a partial PNG.
    let temporary = directory.join(format!(
        ".{object_type}-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        checked_icon_path(cache_root, &temporary, false)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .ok()?;
        file.write_all(&bytes).ok()?;
        drop(file);
        if !publish_icon(cache_root, &temporary, &path) {
            return None;
        }
        Some(path)
    })();
    if checked_icon_path(cache_root, &temporary, false).is_some() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// WinRT requires a regular local path. UNC artwork stays text-only.
/// This string conversion deliberately works on non-Windows hosts too.
pub fn local_toast_icon_path(path: &Path) -> Option<&Path> {
    let path = path.to_str()?;
    if path.starts_with(r"\\?\UNC\") {
        return None;
    }
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    if path.starts_with(r"\\") || path.starts_with("//") {
        return None;
    }
    Some(Path::new(path))
}

/// Check each immediate parent against the canonical cache boundary, without
/// accepting links or Windows reparse points at either directory or file level.
fn checked_icon_path(cache_root: &Path, path: &Path, create: bool) -> Option<()> {
    let directory = path.parent()?;
    let icons = directory.parent()?;
    if icons.parent()? != cache_root {
        return None;
    }
    let mut canonical_parent = cache_root.canonicalize().ok()?;
    for directory in [icons, directory] {
        if !exists_without_following(directory).ok()? {
            if !create {
                return None;
            }
            match fs::create_dir(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
        }
        // Recheck even after create_dir: AlreadyExists is not evidence of safety.
        if link(directory).ok()? || !directory.is_dir() {
            return None;
        }
        let canonical = directory.canonicalize().ok()?;
        if canonical.parent()? != canonical_parent {
            return None;
        }
        canonical_parent = canonical;
    }
    if exists_without_following(path).ok()?
        && (link(path).ok()?
            || !path.is_file()
            || path.canonicalize().ok()?.parent()? != canonical_parent)
    {
        return None;
    }
    Some(())
}

fn publish_icon(cache_root: &Path, temporary: &Path, path: &Path) -> bool {
    if checked_icon_path(cache_root, temporary, false).is_none()
        || checked_icon_path(cache_root, path, false).is_none()
    {
        return false;
    }
    // Windows may reject replacement while another toast has the destination
    // open. Accept a concurrent writer's result only after the same validation.
    fs::rename(temporary, path).is_ok() || valid_icon(cache_root, path)
}

fn valid_icon(cache_root: &Path, path: &Path) -> bool {
    read_valid_icon(cache_root, path).is_some()
}

fn read_valid_icon(cache_root: &Path, path: &Path) -> Option<()> {
    checked_icon_path(cache_root, path, false)?;
    let file = fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || !(57..=ICON_LIMIT as u64).contains(&metadata.len()) {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(ICON_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > ICON_LIMIT
        || bytes.len() < 33
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || &bytes[8..16] != b"\0\0\0\rIHDR"
        || bytes[16..20] != SIZE.to_be_bytes()
        || bytes[20..24] != SIZE.to_be_bytes()
    {
        return None;
    }
    // Header checks alone miss truncated image data and missing IEND chunks.
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: ICON_LIMIT });
    let mut reader = decoder.read_info().ok()?;
    let length = reader.output_buffer_size()?;
    if length > ICON_LIMIT {
        return None;
    }
    reader.next_frame(&mut vec![0; length]).ok()?;
    reader.finish().ok()?;
    Some(())
}

fn crop_png(bytes: &[u8], sprite: &SpriteRef) -> Option<Vec<u8>> {
    if sprite.width == 0 || sprite.height == 0 {
        return None;
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: DECODE_LIMIT,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if sprite.x.checked_add(sprite.width)? > info.width
        || sprite.y.checked_add(sprite.height)? > info.height
    {
        return None;
    }
    let length = reader.output_buffer_size()?;
    if length > DECODE_LIMIT {
        return None;
    }
    let mut source = vec![0; length];
    let info = reader.next_frame(&mut source).ok()?;
    let channels = info.color_type.samples();
    let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let sx = sprite.x + (u64::from(x) * u64::from(sprite.width) / u64::from(SIZE)) as u32;
            let sy = sprite.y + (u64::from(y) * u64::from(sprite.height) / u64::from(SIZE)) as u32;
            let offset = (sy as usize * info.width as usize + sx as usize) * channels;
            let pixel = &source[offset..offset + channels];
            let rgba = match info.color_type {
                png::ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
                png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
                png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
                png::ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
                png::ColorType::Indexed => return None,
            };
            let offset = ((y * SIZE + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, SIZE, SIZE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&pixels).ok()?;
        writer.finish().ok()?;
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_rename_reuses_only_a_complete_icon_published_by_another_writer() {
        let dir = tempfile::tempdir().unwrap();
        let directory = dir.path().join("toast-icons").join("a".repeat(64));
        fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("42.png");
        let missing_temporary = directory.join("missing.tmp");
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 96, 96);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![128; 96 * 96 * 4])
                .unwrap();
        }
        fs::write(&destination, png).unwrap();
        assert!(publish_icon(dir.path(), &missing_temporary, &destination));
        fs::write(&destination, b"broken").unwrap();
        assert!(!publish_icon(dir.path(), &missing_temporary, &destination));
    }
}
