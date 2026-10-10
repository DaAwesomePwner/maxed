//! Validated local tile delivery shared by the UI and notification artwork.
use maxed_types::Catalog;
use std::{fs::File, io::Read, path::Path};

pub const ASSET_ERROR: &str =
    "Client assets could not be loaded. Select a supported resources.assets file and retry.";
pub const SPRITE_ORIGIN: &str = "http://maxed-sprite.localhost";
pub use crate::tiles::TileKey;

/// RunView.dungeon is the canonical name, or the raw map name when the
/// catalog had no definition; aliases cover the latter.
pub fn dungeon_portal_type(catalog: &Catalog, dungeon: &str) -> Option<u16> {
    catalog
        .dungeons
        .iter()
        .find(|d| d.name == dungeon)
        .or_else(|| {
            catalog
                .dungeons
                .iter()
                .find(|d| d.aliases.iter().any(|a| a == dungeon))
        })
        .and_then(|d| d.portal_type)
}

/// Strict canonical route; user input never becomes a filesystem path.
pub fn sprite_location(uri: &str) -> Result<(&str, TileKey), String> {
    let rest = uri
        .strip_prefix("http://maxed-sprite.localhost/")
        .or_else(|| uri.strip_prefix("maxed-sprite://localhost/"))
        .ok_or(ASSET_ERROR)?;
    let (fingerprint, rest) = rest.split_once('/').ok_or(ASSET_ERROR)?;
    if !crate::tiles::valid_fingerprint(fingerprint) {
        return Err(ASSET_ERROR.into());
    }
    let (sheet, rect) = rest.split_once('/').ok_or(ASSET_ERROR)?;
    let mut parts = rect.strip_suffix(".png").ok_or(ASSET_ERROR)?.split('-');
    let mut values = [0u32; 4];
    for value in &mut values {
        let text = parts.next().ok_or(ASSET_ERROR)?;
        if text.is_empty()
            || text.len() > 5
            || !text.bytes().all(|b| b.is_ascii_digit())
            || (text.len() > 1 && text.starts_with('0'))
        {
            return Err(ASSET_ERROR.into());
        }
        *value = text.parse().map_err(|_| ASSET_ERROR)?;
    }
    if parts.next().is_some() {
        return Err(ASSET_ERROR.into());
    }
    let key =
        TileKey::new(sheet, values[0], values[1], values[2], values[3]).map_err(|_| ASSET_ERROR)?;
    Ok((fingerprint, key))
}

fn local_file(root: &Path, name: &str, limit: u64) -> Result<Vec<u8>, String> {
    let root = root.canonicalize().map_err(|_| ASSET_ERROR)?;
    let path = root.join(name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| ASSET_ERROR)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(ASSET_ERROR.into());
        }
    }
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || path.canonicalize().map_err(|_| ASSET_ERROR)?.parent() != Some(root.as_path())
    {
        return Err(ASSET_ERROR.into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| ASSET_ERROR)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ASSET_ERROR)?;
    if bytes.len() as u64 > limit {
        return Err(ASSET_ERROR.into());
    }
    Ok(bytes)
}
/// Cached with the generation lease. Manifest/path checks happen once; every
/// payload still gets its SHA-256 and PNG dimension check on delivery.
#[derive(Debug)]
pub(crate) struct SpriteDelivery {
    pack: crate::tiles::TilePack,
}
impl SpriteDelivery {
    pub(crate) fn open(root: &Path, fingerprint: &str) -> Result<Self, crate::AssetError> {
        #[derive(serde::Deserialize)]
        struct Stamp {
            bytes: u64,
        }
        #[derive(serde::Deserialize)]
        struct Manifest {
            fingerprint: String,
            files: std::collections::BTreeMap<String, Stamp>,
        }
        let manifest: Manifest = serde_json::from_slice(
            &local_file(root, "manifest.json", 65536)
                .map_err(|_| crate::AssetError::CacheInvalid)?,
        )
        .map_err(|_| crate::AssetError::CacheInvalid)?;
        if manifest.fingerprint != fingerprint {
            return Err(crate::AssetError::CacheInvalid);
        }
        let len = manifest
            .files
            .get(crate::tiles::PACK)
            .ok_or(crate::AssetError::CacheInvalid)?
            .bytes;
        Ok(Self {
            pack: crate::tiles::TilePack::open(root, fingerprint, len)?,
        })
    }
    pub(crate) fn read(&self, tile: TileKey) -> Result<Vec<u8>, String> {
        self.pack.read(tile).map_err(|_| ASSET_ERROR.into())
    }
}
/// A pack hit is sufficient: only validated catalog keys enter a published pack.
/// A changed fingerprint gets a new lease and a fresh immutable index.
pub fn read_sprite(cache: &crate::CachedCatalog, tile: TileKey) -> Result<Vec<u8>, String> {
    cache.delivery.read(tile)
}
