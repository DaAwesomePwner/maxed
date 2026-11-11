//! D-cache: immutable, content-addressed generations, prepared before publication.
//! Unlike RealmShark's cache this uses app data and hashes all parsed inputs.
use crate::unity::cancelled;
use crate::{
    locate_enchant_pips, open_sheets, read_text_resources, validate_catalog, AssetError, SheetInfo,
};
use maxed_types::Catalog;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

const CATALOG_LIMIT: u64 = 32 * 1024 * 1024;
const CACHE_VERSION: u32 = 14;
// Reviewed retirement layouts: v9 has catalog/atlases without a tile pack;
// v10 adds the pack, v11 changes localization only, and v12 adds optional catalog
// metadata; v13 adds subattack projectile references; v14 adds optional enchant
// weapon-damage metadata. All retain Catalog v1 and the same manifest/hash
// integrity rules.
// On the next cache bump, review its layout before extending this explicit range.
const RETIRABLE_VERSIONS: std::ops::RangeInclusive<u32> = 9..=14;
static NEXT: AtomicU64 = AtomicU64::new(0);
type Leases = BTreeMap<PathBuf, Weak<Catalog>>;
static LEASES: OnceLock<Mutex<Leases>> = OnceLock::new();
fn leases() -> Result<std::sync::MutexGuard<'static, Leases>, AssetError> {
    LEASES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| AssetError::CacheInvalid)
}

pub struct AssetPaths {
    pub resources: PathBuf,
    pub app_data: PathBuf,
}
#[derive(Clone, Debug)]
pub struct CachedCatalog {
    pub catalog: Arc<Catalog>,
    pub root: PathBuf,
    _lease: Arc<File>,
    pub(crate) delivery: Arc<crate::sprite_delivery::SpriteDelivery>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetProgress {
    Fingerprinting,
    Reading,
    Catalog,
    Sprites,
    Publishing,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    fingerprint: String,
    sheets: Vec<SheetInfo>,
    files: BTreeMap<String, FileStamp>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileStamp {
    bytes: u64,
    sha256: String,
}

fn valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<FileStamp, AssetError> {
    cancelled(cancel)?;
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        cancelled(cancel)?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes
            .checked_add(n as u64)
            .ok_or(AssetError::LimitExceeded)?;
        if bytes > 16 * 1024 * 1024 * 1024 {
            return Err(AssetError::LimitExceeded);
        }
        hash.update(&buffer[..n]);
    }
    Ok(FileStamp {
        bytes,
        sha256: format!("{:x}", hash.finalize()),
    })
}
pub fn client_fingerprint(resources: &Path) -> Result<String, AssetError> {
    fingerprint(resources, &AtomicBool::new(false))
}
fn fingerprint(resources: &Path, cancel: &AtomicBool) -> Result<String, AssetError> {
    cancelled(cancel)?;
    let root = resources
        .parent()
        .ok_or(AssetError::InvalidStructure)?
        .canonicalize()?;
    let source = resources.canonicalize()?;
    if !source.starts_with(&root) {
        return Err(AssetError::InvalidStructure);
    }
    let mut inputs = BTreeMap::new();
    inputs.insert(
        source
            .strip_prefix(&root)
            .map_err(|_| AssetError::InvalidStructure)?
            .to_path_buf(),
        source,
    );
    let pips = locate_enchant_pips(resources, cancel)?.and_then(|p| p.companion);
    let sheets = open_sheets(resources, cancel)?;
    for path in sheets.into_iter().filter_map(|s| s.companion).chain(pips) {
        inputs.insert(
            path.strip_prefix(&root)
                .map_err(|_| AssetError::InvalidStructure)?
                .to_path_buf(),
            path,
        );
    }
    let mut hash = fingerprint_prefix(
        CACHE_VERSION,
        include_bytes!("rules.rs"),
        include_bytes!("../../maxed-types/src/dungeon_meta.rs"),
    );
    hash.update((inputs.len() as u32).to_le_bytes());
    for (relative, path) in inputs {
        let name = relative
            .to_str()
            .ok_or(AssetError::InvalidStructure)?
            .replace('\\', "/");
        let stamp = hash_file(&path, cancel)?;
        hash.update((name.len() as u32).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(stamp.bytes.to_le_bytes());
        hash.update(stamp.sha256.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}
// Hashing the rules and dungeon metadata sources invalidates generations when a table is
// edited without a manual version bump. Cache format changes have their own tag.
fn fingerprint_prefix(cache_version: u32, rules: &[u8], dungeon_meta: &[u8]) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(b"Maxed-assets\0reader-2\0catalog-1\0cache-version\0");
    hash.update(cache_version.to_le_bytes());
    hash.update(b"rules-sha256\0");
    hash.update(Sha256::digest(rules));
    hash.update(b"dungeon-meta-sha256\0");
    hash.update(Sha256::digest(dungeon_meta));
    hash
}

pub(crate) fn link(path: &Path) -> Result<bool, AssetError> {
    let m = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(m.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(m.file_type().is_symlink())
    }
}
pub(crate) fn exists_without_following(path: &Path) -> Result<bool, AssetError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(AssetError::CacheInvalid),
    }
}
fn cache_root(app_data: &Path) -> Result<PathBuf, AssetError> {
    fs::create_dir_all(app_data)?;
    let app = app_data.canonicalize()?;
    let mut current = app.clone();
    for name in ["assets", "v1"] {
        current.push(name);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        };
        if link(&current)? || !current.is_dir() {
            return Err(AssetError::CacheInvalid);
        }
    }
    let root = current.canonicalize()?;
    if !root.starts_with(&app) {
        return Err(AssetError::CacheInvalid);
    }
    Ok(root)
}
fn unique(root: &Path, prefix: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    root.join(format!(
        ".{prefix}-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
struct Stage {
    path: PathBuf,
    root: PathBuf,
}
impl Drop for Stage {
    fn drop(&mut self) {
        if self.path.parent() == Some(self.root.as_path())
            && self
                .path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".stage-"))
            && matches!(link(&self.path), Ok(false))
            && self
                .path
                .canonicalize()
                .ok()
                .is_some_and(|p| p.parent() == Some(self.root.as_path()))
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
fn write_json(path: &Path, value: &impl Serialize, max: u64) -> Result<(), AssetError> {
    struct Limited<W> {
        writer: W,
        left: u64,
    }
    impl<W: Write> Write for Limited<W> {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if b.len() as u64 > self.left {
                return Err(std::io::Error::other("cache size limit"));
            }
            let n = self.writer.write(b)?;
            self.left -= n as u64;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.writer.flush()
        }
    }
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut out = Limited {
        writer: BufWriter::new(&file),
        left: max,
    };
    serde_json::to_writer(&mut out, value).map_err(|_| AssetError::CacheInvalid)?;
    out.flush()?;
    file.sync_all()?;
    Ok(())
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path, max: u64) -> Result<T, AssetError> {
    if link(path)? {
        return Err(AssetError::CacheInvalid);
    }
    let file = File::open(path)?;
    if file.metadata()?.len() > max {
        return Err(AssetError::CacheInvalid);
    }
    serde_json::from_reader(BufReader::new(file.take(max + 1)))
        .map_err(|_| AssetError::CacheInvalid)
}
fn validate_generation(
    path: &Path,
    fingerprint: &str,
    cancel: &AtomicBool,
) -> Result<Catalog, AssetError> {
    validate_generation_version(path, fingerprint, cancel, false)
}
// Older reviewed layouts are accepted only for retirement, never for loading.
// v9 retains its distinct no-tile-pack integrity rules.
fn validate_generation_version(
    path: &Path,
    fingerprint: &str,
    cancel: &AtomicBool,
    retiring: bool,
) -> Result<Catalog, AssetError> {
    let check = || -> Result<Catalog, AssetError> {
        if link(path)? || !path.is_dir() {
            return Err(AssetError::CacheInvalid);
        }
        let manifest: Manifest = read_json(&path.join("manifest.json"), 65536)?;
        let legacy = retiring && manifest.version == 9;
        let supported_version = if retiring {
            RETIRABLE_VERSIONS.contains(&manifest.version)
        } else {
            manifest.version == CACHE_VERSION
        };
        if !supported_version
            || manifest.fingerprint != fingerprint
            || manifest.files.len() > 7
            || manifest.sheets.len() > 5
            || !manifest.files.contains_key("catalog.json")
            || (!legacy && !manifest.files.contains_key(crate::tiles::PACK))
        {
            return Err(AssetError::CacheInvalid);
        }
        let mut sheets = BTreeMap::new();
        for sheet in &manifest.sheets {
            let name = format!("{}.png", sheet.name);
            if !crate::sprites::safe_sheet(&name)
                || sheet.width == 0
                || sheet.height == 0
                || sheet.width > 16384
                || sheet.height > 16384
                || sheets.insert(name, sheet).is_some()
            {
                return Err(AssetError::CacheInvalid);
            }
        }
        if manifest.files.len() != sheets.len() + if legacy { 1 } else { 2 } {
            return Err(AssetError::CacheInvalid);
        }
        for (name, stamp) in &manifest.files {
            if name != "catalog.json"
                && (legacy || name != crate::tiles::PACK)
                && !sheets.contains_key(name)
                || !valid_hash(&stamp.sha256)
                || link(&path.join(name))?
            {
                return Err(AssetError::CacheInvalid);
            }
            let limit = if name == "catalog.json" {
                CATALOG_LIMIT
            } else if name == crate::tiles::PACK {
                crate::tiles::PACK_LIMIT
            } else {
                256 * 1024 * 1024
            };
            if stamp.bytes > limit {
                return Err(AssetError::CacheInvalid);
            }
            let actual = hash_file(&path.join(name), cancel)?;
            if actual.bytes != stamp.bytes || actual.sha256 != stamp.sha256 {
                return Err(AssetError::CacheInvalid);
            }
            if let Some(sheet) = sheets.get(name) {
                let decoder = png::Decoder::new(BufReader::new(File::open(path.join(name))?));
                let reader = decoder.read_info().map_err(|_| AssetError::CacheInvalid)?;
                let info = reader.info();
                if info.width != sheet.width
                    || info.height != sheet.height
                    || info.color_type != png::ColorType::Rgba
                    || info.bit_depth != png::BitDepth::Eight
                {
                    return Err(AssetError::CacheInvalid);
                }
            }
        }
        let mut entries = 0;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            entries += 1;
            if entries > 8
                || entry.file_name() != "manifest.json"
                    && !manifest
                        .files
                        .contains_key(entry.file_name().to_string_lossy().as_ref())
            {
                return Err(AssetError::CacheInvalid);
            }
        }
        if entries != manifest.files.len() + 1 {
            return Err(AssetError::CacheInvalid);
        }
        let c: Catalog = read_json(&path.join("catalog.json"), CATALOG_LIMIT)?;
        validate_catalog(&c)?;
        if c.client_fingerprint != fingerprint {
            return Err(AssetError::CacheInvalid);
        }
        for sprite in &c.sprites {
            let s = sheets.get(&sprite.sheet).ok_or(AssetError::CacheInvalid)?;
            if sprite.x + sprite.width > s.width || sprite.y + sprite.height > s.height {
                return Err(AssetError::CacheInvalid);
            }
        }
        if !legacy {
            crate::tiles::validate_pack(path, fingerprint, &c.sprites, cancel)?;
        }
        Ok(c)
    };
    check().map_err(|e| {
        if matches!(e, AssetError::Cancelled) {
            e
        } else {
            AssetError::CacheInvalid
        }
    })
}
fn pointer(root: &Path, fingerprint: &str) -> Result<(), AssetError> {
    let target = root.join("current");
    if exists_without_following(&target)? {
        if link(&target)? || !target.is_file() {
            return Err(AssetError::CacheInvalid);
        }
        if fs::metadata(&target)?.len() == 64
            && fs::read(&target).is_ok_and(|v| v == fingerprint.as_bytes())
        {
            return Ok(());
        }
    }
    let temp = unique(root, "pointer");
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(fingerprint.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &target)
    })()
    .map_err(|_| AssetError::CacheInvalid);
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn lease(root: PathBuf, mut catalog: Catalog, file: File) -> Result<CachedCatalog, AssetError> {
    let delivery = Arc::new(crate::sprite_delivery::SpriteDelivery::open(
        &root,
        &catalog.client_fingerprint,
    )?);
    let mut leases = leases()?;
    let catalog = if let Some(existing) = leases.get(&root).and_then(Weak::upgrade) {
        if *existing != catalog {
            return Err(AssetError::CacheInvalid);
        }
        existing
    } else {
        // JSON sequences grow geometrically while parsing. A published catalog
        // is immutable, so keeping that spare capacity for its whole lifetime
        // buys nothing (the production sprite vector can be nearly half empty).
        catalog.items.shrink_to_fit();
        catalog.objects.shrink_to_fit();
        catalog.classes.shrink_to_fit();
        catalog.enchants.shrink_to_fit();
        catalog.dungeons.shrink_to_fit();
        catalog.bags.shrink_to_fit();
        catalog.sprites.shrink_to_fit();
        catalog.tiles.shrink_to_fit();
        catalog.crucible_bonuses.shrink_to_fit();
        Arc::new(catalog)
    };
    leases.retain(|_, v| v.strong_count() > 0);
    leases.insert(root.clone(), Arc::downgrade(&catalog));
    drop(leases);
    Ok(CachedCatalog {
        delivery,
        catalog,
        root,
        _lease: Arc::new(file),
    })
}

fn generation_lease(root: &Path, fingerprint: &str, exclusive: bool) -> Result<File, AssetError> {
    let path = root.join(format!("{fingerprint}.lease"));
    if exists_without_following(&path)?
        && (link(&path)? || !path.is_file() || fs::metadata(&path)?.len() != 0)
    {
        return Err(AssetError::CacheInvalid);
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(if exclusive { 0 } else { 3 });
    }
    #[cfg(not(windows))]
    if exclusive {
        return Err(AssetError::CacheInvalid);
    }
    options.open(path).map_err(|_| AssetError::CacheInvalid)
}
/// Caller holds a shared OS lease. Never move a generation
/// while an Arc reader or another process retains a lease. Revalidate after the
/// exclusive acquisition: another process may have repaired it in the gap.
fn existing_or_quarantine(
    root: &Path,
    fingerprint: &str,
    file_lease: &mut Option<File>,
    cancel: &AtomicBool,
) -> Result<Option<Catalog>, AssetError> {
    let generation = root.join(fingerprint);
    if !exists_without_following(&generation)? {
        return Ok(None);
    }
    match validate_generation(&generation, fingerprint, cancel) {
        Ok(c) => return Ok(Some(c)),
        Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
        Err(_) => {}
    }
    let exclusive = {
        let active = leases()?;
        if active
            .get(&generation)
            .is_some_and(|v| v.strong_count() > 0)
        {
            return Err(AssetError::CacheInvalid);
        }
        drop(file_lease.take());
        generation_lease(root, fingerprint, true)?
    };
    let result = match validate_generation(&generation, fingerprint, cancel) {
        Ok(c) => Some(c),
        Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
        Err(_) => {
            cancelled(cancel)?;
            // Cache-root child only. Reparse points and non-directories are never
            // quarantined or recursively removed, including during a race.
            if link(&generation)?
                || !generation.is_dir()
                || generation.canonicalize()?.parent() != Some(root)
            {
                return Err(AssetError::CacheInvalid);
            }
            fs::rename(&generation, unique(root, &format!("corrupt-{fingerprint}")))
                .map_err(|_| AssetError::CacheInvalid)?;
            None
        }
    };
    drop(exclusive);
    *file_lease = Some(generation_lease(root, fingerprint, false)?);
    Ok(result)
}

pub fn load_or_extract(
    paths: &AssetPaths,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(AssetProgress),
) -> Result<CachedCatalog, AssetError> {
    cancelled(cancel)?;
    progress(AssetProgress::Fingerprinting);
    cancelled(cancel)?;
    let fingerprint = fingerprint(&paths.resources, cancel)?;
    let root = cache_root(&paths.app_data)?;
    let generation = root.join(&fingerprint);
    if exists_without_following(&generation)? && link(&generation)? {
        return Err(AssetError::CacheInvalid);
    }
    let mut file_lease = Some(generation_lease(&root, &fingerprint, false)?);
    if let Some(c) = existing_or_quarantine(&root, &fingerprint, &mut file_lease, cancel)? {
        cancelled(cancel)?;
        pointer(&root, &fingerprint)?;
        return lease(generation, c, file_lease.ok_or(AssetError::CacheInvalid)?);
    }
    let stage = Stage {
        path: unique(&root, "stage"),
        root: root.clone(),
    };
    fs::create_dir(&stage.path)?;
    progress(AssetProgress::Reading);
    cancelled(cancel)?;
    let assets = read_text_resources(&paths.resources, cancel)?;
    let mut sheets = open_sheets(&paths.resources, cancel)?;
    let mut pips = locate_enchant_pips(&paths.resources, cancel)?;
    let mut infos = sheets.iter().map(|s| s.info().clone()).collect::<Vec<_>>();
    infos.extend(pips.as_ref().map(|p| p.info()));
    progress(AssetProgress::Catalog);
    cancelled(cancel)?;
    let mut catalog =
        crate::catalog::build_cancellable(assets, fingerprint.clone(), &infos, cancel)?;
    cancelled(cancel)?;
    progress(AssetProgress::Sprites);
    cancelled(cancel)?;
    for sheet in &mut sheets {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.path.join(format!("{}.png", sheet.info().name)))?;
        sheet.write_png(&mut file, cancel)?;
    }
    if let Some(pips) = &mut pips {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.path.join(format!("{}.png", pips.info().name)))?;
        pips.write_png(&mut file, cancel)?;
    }
    let pip_pixels = pips.as_mut().map(|p| p.read_rgba(cancel)).transpose()?;
    crate::tiles::write_pack(
        &stage.path,
        &fingerprint,
        &catalog.sprites,
        cancel,
        |key, y, output| {
            if key.sheet_name() == "enchantPips.png" {
                let pixels = pip_pixels.as_ref().ok_or(AssetError::InvalidCatalog)?;
                let at = ((key.y + y) * 64 + key.x) as usize * 4;
                if key.x + key.width > 64 || key.y + key.height > 16 {
                    return Err(AssetError::InvalidCatalog);
                }
                output.copy_from_slice(
                    pixels
                        .get(at..at + output.len())
                        .ok_or(AssetError::InvalidCatalog)?,
                );
                Ok(())
            } else {
                let sheet = sheets
                    .iter_mut()
                    .find(|s| key.sheet_name().strip_suffix(".png") == Some(s.info().name.as_str()))
                    .ok_or(AssetError::InvalidCatalog)?;
                if key.y + key.height > sheet.info().height {
                    return Err(AssetError::InvalidCatalog);
                }
                sheet.read_span_row(key.x, key.y + y, output, cancel)
            }
        },
    )?;
    drop(pips);
    drop(sheets);
    for info in &infos {
        cancelled(cancel)?;
        let name = format!("{}.png", info.name);
        let input = std::io::BufReader::new(fs::File::open(stage.path.join(&name))?);
        crate::sprites::trim_sheet(input, &name, &mut catalog.sprites, cancel)?;
    }
    crate::catalog::validate_catalog(&catalog)?;
    write_json(&stage.path.join("catalog.json"), &catalog, CATALOG_LIMIT)?;
    drop(catalog);
    let mut files = BTreeMap::new();
    files.insert(
        "catalog.json".into(),
        hash_file(&stage.path.join("catalog.json"), cancel)?,
    );
    files.insert(
        crate::tiles::PACK.into(),
        hash_file(&stage.path.join(crate::tiles::PACK), cancel)?,
    );
    for info in &infos {
        let name = format!("{}.png", info.name);
        files.insert(name.clone(), hash_file(&stage.path.join(&name), cancel)?);
    }
    write_json(
        &stage.path.join("manifest.json"),
        &Manifest {
            version: CACHE_VERSION,
            fingerprint: fingerprint.clone(),
            sheets: infos,
            files,
        },
        65536,
    )?;
    let catalog = validate_generation(&stage.path, &fingerprint, cancel)?;
    progress(AssetProgress::Publishing);
    cancelled(cancel)?;
    if self::fingerprint(&paths.resources, cancel)? != fingerprint {
        return Err(AssetError::CacheInvalid);
    }
    let catalog = loop {
        cancelled(cancel)?;
        if let Some(existing) =
            existing_or_quarantine(&root, &fingerprint, &mut file_lease, cancel)?
        {
            break existing;
        }
        // Serialize only the existence check/rename, never validation. A
        // concurrent publisher's complete generation is checked on the next pass.
        let active = leases()?;
        if exists_without_following(&generation)? {
            drop(active);
            continue;
        }
        match fs::rename(&stage.path, &generation) {
            Ok(()) => break catalog,
            Err(_) if exists_without_following(&generation)? => {
                drop(active);
                continue;
            }
            Err(_) => return Err(AssetError::CacheInvalid),
        }
    };
    pointer(&root, &fingerprint)?;
    lease(
        generation,
        catalog,
        file_lease.ok_or(AssetError::CacheInvalid)?,
    )
}
impl CachedCatalog {
    /// Schedule maintenance only after the caller has received/published this lease.
    /// No filesystem sweep or join runs on the catalog/capture readiness path.
    pub fn prune_in_background(&self) {
        let Some(app_data) = self.root.ancestors().nth(3) else {
            return;
        };
        let app_data = app_data.to_owned();
        let published = self.clone();
        if std::thread::Builder::new()
            .name("asset-prune".into())
            .spawn(move || {
                let _published = published;
                if let Err(error) = prune_unused_generations(&app_data, &AtomicBool::new(false)) {
                    tracing::warn!(?error, "asset generation pruning deferred");
                }
            })
            .is_err()
        {
            tracing::warn!("asset pruning worker unavailable");
        }
    }
}

// Only invocation-generated names are eligible. Timestamp is quarantine time,
// not the old directory mtime (which rename preserves).
fn quarantine_identity(name: &str) -> Option<(&str, u128)> {
    let mut parts = name.strip_prefix(".corrupt-")?.split('-');
    let fingerprint = parts.next()?;
    if !valid_hash(fingerprint) {
        return None;
    }
    parts.next()?.parse::<u32>().ok()?;
    let nanos = parts.next()?.parse::<u128>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((fingerprint, nanos))
}

/// Explicit maintenance. Shared OS leases protect readers and extractors;
/// the in-process mutex is held only for eligibility/exclusive-lease acquisition.
/// Retired directories are retryable even after partially successful deletion.
pub fn prune_unused_generations(app_data: &Path, cancel: &AtomicBool) -> Result<usize, AssetError> {
    cancelled(cancel)?;
    let root = cache_root(app_data)?;
    // Serialize sweeps across processes without blocking a catalog publisher.
    let Ok(_sweep) = generation_lease(&root, "maintenance", true) else {
        return Ok(0);
    };
    let current = current_generation(&root)?;
    let mut entries = Vec::new();
    for (i, entry) in fs::read_dir(&root)?.enumerate() {
        cancelled(cancel)?;
        if i >= 4096 {
            return Err(AssetError::LimitExceeded);
        }
        match entry {
            Ok(entry) => {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if (valid_hash(name) && name != current)
                    || quarantine_identity(name).is_some()
                    || retired_identity(name).is_some()
                {
                    entries.push((name.to_owned(), entry.path()));
                }
            }
            Err(_) => tracing::warn!("asset prune entry enumeration deferred"),
        }
    }
    // Only quarantines need a publication timestamp. The current generation was
    // already validated by publication; never hash it again for maintenance.
    let newest = if entries
        .iter()
        .any(|(n, _)| quarantine_identity(n).is_some())
    {
        let path = root.join(&current);
        read_json::<Manifest>(&path.join("manifest.json"), 65536)
            .ok()
            .filter(|m| m.version == CACHE_VERSION && m.fingerprint == current)
            .and_then(|_| {
                fs::metadata(path.join("manifest.json"))
                    .ok()?
                    .modified()
                    .ok()
            })
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
    } else {
        None
    };
    let mut removed = 0;
    for (name, path) in entries {
        cancelled(cancel)?;
        let quarantine = quarantine_identity(&name);
        let retired = retired_identity(&name);
        if quarantine.is_some_and(|(_, timestamp)| !newest.is_some_and(|n| timestamp < n)) {
            continue;
        }
        let fingerprint = quarantine.or(retired).map_or(name.as_str(), |(fp, _)| fp);
        // Quarantines/retired dirs have their own exclusive lease. They cannot
        // be live generations, even if their repaired fingerprint is in use.
        let lease_name = if quarantine.is_some() || retired.is_some() {
            &name
        } else {
            fingerprint
        };
        let exclusive = {
            let active = leases()?;
            if quarantine.is_none()
                && retired.is_none()
                && active.get(&path).is_some_and(|v| v.strong_count() > 0)
            {
                continue;
            }
            generation_lease(&root, lease_name, true)
        };
        let Ok(_exclusive) = exclusive else { continue };
        // A different process may have published after enumeration. Once we
        // own this exclusive lease it cannot publish this fingerprint again.
        if quarantine.is_none() && retired.is_none() {
            match current_generation(&root) {
                Ok(current) if current != name => {}
                Ok(_) => continue,
                Err(error) => {
                    tracing::warn!(?error, "asset prune pointer check deferred");
                    continue;
                }
            }
        }
        let result = (|| {
            safe_tree(&root, &path)?;
            if quarantine.is_none() && retired.is_none() {
                let manifest: Manifest = read_json(&path.join("manifest.json"), 65536)?;
                if manifest.fingerprint != fingerprint || manifest.version > CACHE_VERSION {
                    return Err(AssetError::CacheInvalid);
                }
                // Old catalog schemas need not deserialize with today's DTOs.
                // Their safe generation identity and no-reparse tree suffice.
                if manifest.version >= 9 {
                    validate_generation_version(&path, fingerprint, cancel, true)?;
                }
            }
            cancelled(cancel)?;
            remove_generation(&root, &path, fingerprint)
        })();
        match result {
            Ok(()) => removed += 1,
            Err(AssetError::Cancelled) => return Err(AssetError::Cancelled),
            Err(error) => tracing::warn!(?error, "asset prune entry deferred"),
        }
    }
    Ok(removed)
}
fn current_generation(root: &Path) -> Result<String, AssetError> {
    let path = root.join("current");
    if link(&path)? || fs::metadata(&path)?.len() != 64 {
        return Err(AssetError::CacheInvalid);
    }
    let current = fs::read_to_string(path)?;
    if !valid_hash(&current) {
        return Err(AssetError::CacheInvalid);
    }
    Ok(current)
}
fn retired_identity(name: &str) -> Option<(&str, u128)> {
    let suffix = name.strip_prefix(".retired-")?;
    let (fp, rest) = suffix.split_once('-')?;
    if !valid_hash(fp) {
        return None;
    }
    let mut parts = rest.split('-');
    parts.next()?.parse::<u32>().ok()?;
    let nanos = parts.next()?.parse::<u128>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((fp, nanos))
}
/// Check every descendant, including incomplete retired trees, without following
/// links/reparse points. A bounded traversal refuses unexpected oversized trees.
fn safe_tree(root: &Path, path: &Path) -> Result<(), AssetError> {
    if link(path)?
        || !path.is_dir()
        || path.canonicalize()?.parent() != Some(root.canonicalize()?.as_path())
    {
        return Err(AssetError::CacheInvalid);
    }
    let mut pending = vec![path.to_owned()];
    let mut count = 0;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            count += 1;
            if count > 4096 {
                return Err(AssetError::LimitExceeded);
            }
            let entry = entry?;
            if link(&entry.path())? {
                return Err(AssetError::CacheInvalid);
            }
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else if !entry.file_type()?.is_file() {
                return Err(AssetError::CacheInvalid);
            }
        }
    }
    Ok(())
}
/// Caller holds the exclusive generation/retirement lease. Rename before any
/// deletion so a Windows sharing violation leaves a recognizable retry target.
fn remove_generation(root: &Path, path: &Path, fingerprint: &str) -> Result<(), AssetError> {
    safe_tree(root, path)?;
    let retired = if path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(retired_identity)
        .is_some()
    {
        path.to_owned()
    } else {
        let retired = unique(root, &format!("retired-{fingerprint}"));
        fs::rename(path, &retired)?;
        retired
    };
    if !exists_without_following(&root.join(fingerprint)).unwrap_or(true) {
        let icons = root.join("toast-icons");
        let generation_icons = icons.join(fingerprint);
        if link(&icons).is_ok_and(|linked| !linked) && generation_icons.exists() {
            // A retry owns the retirement lease, not the original generation.
            // Do not race a repaired generation/extractor when moving its icons.
            let _icon_lease = if path.file_name().and_then(|n| n.to_str()) == Some(fingerprint) {
                None // caller already holds the original exclusive lease
            } else {
                let active = leases()?;
                if active
                    .get(&root.join(fingerprint))
                    .is_some_and(|v| v.strong_count() > 0)
                {
                    return Err(AssetError::CacheInvalid);
                }
                Some(generation_lease(root, fingerprint, true)?)
            };
            safe_tree(&icons.canonicalize()?, &generation_icons)?;
            // Keep failed icon cleanup under the same retryable retirement name.
            fs::rename(generation_icons, retired.join("toast-icons"))?;
        }
    }
    fs::remove_dir_all(&retired)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn retirement_fixture(version: u32, with_pack: bool) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let fingerprint = "a".repeat(64);
        let cancel = AtomicBool::new(false);
        let catalog = crate::build_catalog(
            crate::ExtractedAssets {
                english: Default::default(),
                texts: vec![],
                textures: vec![],
                sprite_mapping: vec![],
            },
            fingerprint.clone(),
        )
        .unwrap();
        write_json(&dir.path().join("catalog.json"), &catalog, CATALOG_LIMIT).unwrap();
        let mut files = BTreeMap::new();
        files.insert(
            "catalog.json".into(),
            hash_file(&dir.path().join("catalog.json"), &cancel).unwrap(),
        );
        if with_pack {
            crate::tiles::write_pack(
                dir.path(),
                &fingerprint,
                &[],
                &cancel,
                |_, _, _| unreachable!(),
            )
            .unwrap();
            files.insert(
                crate::tiles::PACK.into(),
                hash_file(&dir.path().join(crate::tiles::PACK), &cancel).unwrap(),
            );
        }
        write_json(
            &dir.path().join("manifest.json"),
            &Manifest {
                version,
                fingerprint: fingerprint.clone(),
                sheets: vec![],
                files,
            },
            65536,
        )
        .unwrap();
        (dir, fingerprint)
    }

    #[test]
    fn v11_generation_is_valid_only_for_retirement() {
        let (dir, fingerprint) = retirement_fixture(11, true);
        let cancel = AtomicBool::new(false);
        assert!(matches!(
            validate_generation(dir.path(), &fingerprint, &cancel),
            Err(AssetError::CacheInvalid)
        ));
        assert!(validate_generation_version(dir.path(), &fingerprint, &cancel, true).is_ok());
        // Older layouts still undergo file-integrity validation before deletion.
        fs::write(
            dir.path().join(crate::tiles::PACK),
            b"corrupt synthetic pack",
        )
        .unwrap();
        assert!(matches!(
            validate_generation_version(dir.path(), &fingerprint, &cancel, true),
            Err(AssetError::CacheInvalid)
        ));
    }

    #[test]
    fn retirement_versions_cover_current_version_and_reject_unreviewed_layouts() {
        // A CACHE_VERSION bump must explicitly extend the reviewed retirement set.
        // Exercise the real validator, including its distinct v9 no-pack layout.
        for version in 8..=CACHE_VERSION + 1 {
            let (dir, fingerprint) = retirement_fixture(version, version != 9);
            let cancel = AtomicBool::new(false);
            assert_eq!(
                validate_generation_version(dir.path(), &fingerprint, &cancel, true).is_ok(),
                (9..=CACHE_VERSION).contains(&version),
                "retirement version {version} requires a layout review",
            );
            assert_eq!(
                validate_generation(dir.path(), &fingerprint, &cancel).is_ok(),
                version == CACHE_VERSION
            );
        }
        for (version, with_pack) in [(9, true), (10, false), (11, false), (CACHE_VERSION, false)] {
            let (dir, fingerprint) = retirement_fixture(version, with_pack);
            assert!(validate_generation_version(
                dir.path(),
                &fingerprint,
                &AtomicBool::new(false),
                true
            )
            .is_err());
        }
    }

    #[test]
    fn weapon_enchant_metadata_invalidates_version_thirteen_fingerprints() {
        assert_eq!(CACHE_VERSION, 14);
        assert_ne!(
            fingerprint_prefix(
                CACHE_VERSION,
                include_bytes!("rules.rs"),
                include_bytes!("../../maxed-types/src/dungeon_meta.rs")
            )
            .finalize(),
            fingerprint_prefix(
                13,
                include_bytes!("rules.rs"),
                include_bytes!("../../maxed-types/src/dungeon_meta.rs")
            )
            .finalize()
        );
    }

    #[test]
    fn removing_a_generation_prunes_only_its_icons() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let retired = "a".repeat(64);
        let active = "b".repeat(64);
        for name in [&retired, &active] {
            fs::create_dir(root.join(name)).unwrap();
            let icons = root.join("toast-icons").join(name);
            fs::create_dir_all(&icons).unwrap();
            fs::write(icons.join("42.png"), b"synthetic icon").unwrap();
        }
        let unknown = root.join("toast-icons/unknown");
        fs::create_dir(&unknown).unwrap();
        remove_generation(root, &root.join(&retired), &retired).unwrap();
        assert!(!root.join(&retired).exists());
        assert!(!root.join("toast-icons").join(&retired).exists());
        assert!(root.join(&active).is_dir());
        assert!(root
            .join("toast-icons")
            .join(&active)
            .join("42.png")
            .is_file());
        assert!(unknown.is_dir());

        // A retired quarantine can share a fingerprint with a repaired, current generation.
        let quarantine = root.join(format!(".corrupt-{active}-1-2-3"));
        fs::create_dir(&quarantine).unwrap();
        remove_generation(root, &quarantine, &active).unwrap();
        assert!(root
            .join("toast-icons")
            .join(&active)
            .join("42.png")
            .is_file());
    }

    #[test]
    fn cache_format_rules_and_dungeon_metadata_changes_invalidate_fingerprints() {
        let original = fingerprint_prefix(CACHE_VERSION, b"rules v1", b"dungeons v1").finalize();
        assert_ne!(
            original,
            fingerprint_prefix(CACHE_VERSION + 1, b"rules v1", b"dungeons v1").finalize()
        );
        assert_ne!(
            original,
            fingerprint_prefix(CACHE_VERSION, b"rules v2", b"dungeons v1").finalize()
        );
        assert_ne!(
            original,
            fingerprint_prefix(CACHE_VERSION, b"rules v1", b"dungeons v2").finalize()
        );
        assert!(quarantine_identity(".corrupt-unrelated").is_none());
        assert!(quarantine_identity(&format!(".corrupt-{}-1-2-3-extra", "a".repeat(64))).is_none());
    }
}
