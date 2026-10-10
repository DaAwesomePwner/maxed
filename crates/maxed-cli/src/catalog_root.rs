//! Private, staged catalog bundle shared with RealmShark's domain exporter.
//!
//! Catalog-root v1 is Windows-only: publication relies on Windows' refusal to
//! rename over an existing directory. Other platforms fail before path validation,
//! client access or staging. The output's parent directory must already exist.
use crate::{
    domain_export::{canonical_catalog_bytes, catalog_sha256},
    CliError,
};
use maxed_assets::{AssetError, TextAsset};
use maxed_types::Catalog;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct CatalogRootReport {
    pub catalog_sha256: String,
    /// Includes manifest.json; the manifest's files map excludes itself.
    pub file_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema: &'static str,
    catalog_sha256: String,
    files: BTreeMap<String, String>,
}

fn asset_error(error: AssetError) -> CliError {
    // CliError's I/O display is sanitized, including when this error is nested.
    io::Error::other(error).into()
}

/// Extracts client TextAssets read-only. No cache, textures, JVM or network is
/// needed; sheets are opened only to validate Catalog sprite references.
/// Windows-only v1; unsupported platforms return a fixed error before any I/O.
pub fn catalog_root(client: &Path, output: &Path) -> Result<CatalogRootReport, CliError> {
    publish(output, |stage| {
        let cancel = AtomicBool::new(false);
        let fingerprint = maxed_assets::client_fingerprint(client).map_err(asset_error)?;
        let assets = maxed_assets::read_text_resources(client, &cancel).map_err(asset_error)?;
        let lists =
            maxed_assets::build_reference_id_lists(&assets.texts, &cancel).map_err(asset_error)?;
        let xml = reference_xml(&assets.texts)?;
        let sheets = maxed_assets::open_sheets(client, &cancel).map_err(asset_error)?;
        let infos = sheets
            .iter()
            .map(|sheet| sheet.info().clone())
            .collect::<Vec<_>>();
        let catalog = maxed_assets::build_catalog_with_sheets(assets, fingerprint.clone(), &infos)
            .map_err(asset_error)?;
        maxed_assets::validate_catalog(&catalog).map_err(asset_error)?;
        let report = write_bundle(stage, &catalog, &lists, &xml)?;
        // Refuse mixed generations if the client patches during extraction.
        if maxed_assets::client_fingerprint(client).map_err(asset_error)? != fingerprint {
            return Err(asset_error(AssetError::InvalidStructure));
        }
        Ok(report)
    })
}

// Preserve all XML so Java's dungeon rules see the same client definitions.
// Names follow UnityExtractor.checkDuplicates, including collisions with literal
// suffix names. No input-controlled path is used until it passes validation.
fn reference_xml(texts: &[TextAsset]) -> Result<BTreeMap<String, Vec<u8>>, CliError> {
    let mut names = BTreeSet::new();
    let mut folded = BTreeSet::new();
    let mut xml = BTreeMap::new();
    for text in texts {
        // UnityExtractor filters these names before reserving collision suffixes.
        if [
            "manifest_xml",
            "COPYING",
            "Errors",
            "ExplainUnzip",
            "cloth_bazaar",
            "Cursors",
            "Dialogs",
            "Keyboard",
            "LICENSE",
            "LineBreaking Following Characters",
            "LineBreaking Leading Characters",
            "manifest_json",
            "spritesheetf",
            "iso_4217",
            "data",
            "manifest",
            "BillingMode",
        ]
        .contains(&text.name.as_str())
        {
            continue;
        }
        if text.name.is_empty()
            || !text.name.is_ascii()
            || text.name.len() > 240
            || text
                .name
                .chars()
                .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
            || text.name.ends_with(['.', ' '])
            || reserved_name(&text.name)
        {
            return Err(asset_error(AssetError::InvalidStructure));
        }
        let mut name = text.name.clone();
        let mut suffix = 1;
        while !names.insert(name.clone()) {
            suffix += 1;
            name = format!("{}{suffix}", text.name);
        }
        if !folded.insert(name.to_ascii_lowercase()) {
            return Err(asset_error(AssetError::InvalidStructure));
        }
        let bytes = text
            .bytes
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&text.bytes);
        if !bytes
            .iter()
            .take(256)
            .copied()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|b| b == b'<')
        {
            continue;
        }
        xml.insert(format!("xml/{name}.xml"), text.bytes.clone());
    }
    for name in ["equip", "enchantments", "players"] {
        if !xml.contains_key(&format!("xml/{name}.xml")) {
            return Err(asset_error(AssetError::InvalidStructure));
        }
    }
    Ok(xml)
}

fn reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        })
}

fn write_bundle(
    stage: &Path,
    catalog: &Catalog,
    lists: &maxed_assets::ReferenceIdLists,
    xml: &BTreeMap<String, Vec<u8>>,
) -> Result<CatalogRootReport, CliError> {
    let catalog = canonical_catalog_bytes(catalog)?;
    let digest = catalog_sha256(&catalog);
    let mut files = BTreeMap::new();
    fs::create_dir(stage.join("xml"))?;
    for (name, bytes) in [
        ("catalog.json", catalog.as_slice()),
        ("ObjectID.list", lists.object_ids.as_slice()),
        ("TileID.list", lists.tile_ids.as_slice()),
    ]
    .into_iter()
    .chain(
        xml.iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
    ) {
        write_new(&stage.join(name), bytes)?;
        files.insert(name.to_owned(), catalog_sha256(bytes));
    }
    let count = files.len() + 1;
    let manifest = Manifest {
        schema: "maxed-catalog-root-v1",
        catalog_sha256: digest.clone(),
        files,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(io::Error::other)?;
    write_new(&stage.join("manifest.json"), &bytes)?;
    Ok(CatalogRootReport {
        catalog_sha256: digest,
        file_count: count,
    })
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

struct Stage {
    path: PathBuf,
    owned: bool,
}
impl Stage {
    #[cfg(windows)]
    fn publish(&mut self, output: &Path) -> Result<(), CliError> {
        // Windows refuses even an empty existing destination directory.
        fs::rename(&self.path, output)?;
        self.owned = false;
        Ok(())
    }
    #[cfg(not(windows))]
    fn publish(&mut self, _output: &Path) -> Result<(), CliError> {
        // Std rename on Unix can replace an empty directory. Fail closed without
        // a no-replace API rather than weaken the publication guarantee.
        Err(CliError::CatalogRootWindowsOnly)
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        // Only an invocation-owned sibling created by create_dir is removed.
        if self.owned {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn absent(path: &Path) -> Result<(), CliError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists).into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn publish(
    output: &Path,
    prepare: impl FnOnce(&Path) -> Result<CatalogRootReport, CliError>,
) -> Result<CatalogRootReport, CliError> {
    publish_for_platform(cfg!(windows), output, prepare)
}

// The explicit capability lets tests exercise fail-fast behavior on Windows too.
fn publish_for_platform(
    windows: bool,
    output: &Path,
    prepare: impl FnOnce(&Path) -> Result<CatalogRootReport, CliError>,
) -> Result<CatalogRootReport, CliError> {
    if !windows {
        return Err(CliError::CatalogRootWindowsOnly);
    }
    if output
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(CliError::InvalidArguments);
    }
    let output = crate::record::private_output_path(output)?;
    absent(&output)?;
    // Require an existing parent: resolve it before creating anything, including
    // symlink targets, then apply the repository private-path rule again.
    let parent = output
        .parent()
        .ok_or(CliError::InvalidArguments)?
        .canonicalize()?;
    let output = crate::record::private_output_path(
        &parent.join(output.file_name().ok_or(CliError::InvalidArguments)?),
    )?;
    absent(&output)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut stage = loop {
        let path = parent.join(format!(
            ".maxed-catalog-root-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => break Stage { path, owned: true },
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let report = prepare(&stage.path)?;
    absent(&output)?;
    stage.publish(&output)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain_export::export_domain;
    use serde_json::Value;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "maxed-catroot-tests-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/scrubbed/maxed-demo")
            .join(name)
    }
    fn synthetic(stage: &Path) -> Result<CatalogRootReport, CliError> {
        let catalog: Catalog = serde_json::from_slice(&fs::read(fixture("catalog.json"))?).unwrap();
        let texts = ["equip", "players", "enchantments"].map(|name| TextAsset {
            name: name.into(),
            bytes: b"<Objects/>\n".to_vec(),
        });
        let lists =
            maxed_assets::build_reference_id_lists(&texts, &AtomicBool::new(false)).unwrap();
        write_bundle(stage, &catalog, &lists, &reference_xml(&texts)?)
    }
    fn verify(root: &Path, report: &CatalogRootReport) {
        let manifest: Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest.as_object().unwrap().len(), 3);
        assert_eq!(manifest["schema"], "maxed-catalog-root-v1");
        assert_eq!(manifest["catalogSha256"], report.catalog_sha256);
        let files = manifest["files"].as_object().unwrap();
        assert_eq!(report.file_count, files.len() + 1);
        assert!(!files.contains_key("manifest.json"));
        let mut actual = BTreeSet::new();
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                for file in fs::read_dir(entry.path()).unwrap() {
                    let file = file.unwrap();
                    actual.insert(format!("xml/{}", file.file_name().to_str().unwrap()));
                }
            } else if entry.file_name() != "manifest.json" {
                actual.insert(entry.file_name().to_str().unwrap().to_owned());
            }
        }
        assert_eq!(actual, files.keys().cloned().collect());
        for (name, digest) in files {
            let bytes = fs::read(root.join(name)).unwrap();
            assert_eq!(
                digest.as_str().unwrap(),
                format!("{:x}", sha2::Sha256::digest(&bytes))
            );
        }
        assert_eq!(files["catalog.json"], manifest["catalogSha256"]);
    }
    use sha2::Digest;

    #[test]
    fn manifest_canonical_bytes_and_export_domain_hash_agree() {
        let scratch = Scratch::new();
        let root = scratch.0.join("root");
        fs::create_dir(&root).unwrap();
        let report = synthetic(&root).unwrap();
        verify(&root, &report);
        assert_eq!(report.file_count, 7);
        assert_eq!(
            fs::read(root.join("xml/equip.xml")).unwrap(),
            b"<Objects/>\n"
        );
        let bytes = fs::read(root.join("catalog.json")).unwrap();
        let mut parsed: Value = serde_json::from_slice(&bytes).unwrap();
        parsed.sort_all_objects();
        assert_eq!(bytes, serde_json::to_vec(&parsed).unwrap());
        assert_ne!(bytes.last(), Some(&b'\n'));
        let output = scratch.0.join("domain.jsonl");
        export_domain(&fixture("story.pcap"), &root.join("catalog.json"), &output).unwrap();
        let header: Value =
            serde_json::from_str(fs::read_to_string(&output).unwrap().lines().next().unwrap())
                .unwrap();
        assert_eq!(header["catalogSha256"], report.catalog_sha256);
        let metadata: Value =
            serde_json::from_slice(&fs::read(crate::decode::metadata_path(&output)).unwrap())
                .unwrap();
        assert_eq!(metadata["catalogSha256"], report.catalog_sha256);
        let mut changed = bytes;
        changed.push(b'\n');
        assert_ne!(catalog_sha256(&changed), report.catalog_sha256);
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 3);
    }
    #[test]
    fn unsupported_platform_refuses_before_output_validation_and_preparation() {
        let error = publish_for_platform(false, Path::new("../invalid-output"), |_| {
            panic!("must not read the client or write a stage")
        })
        .unwrap_err();
        assert!(matches!(error, CliError::CatalogRootWindowsOnly));
        assert_eq!(error.to_string(), "catalog-root v1 is Windows-only");
    }
    #[test]
    #[cfg(not(windows))]
    fn unsupported_platform_refuses_before_client_access() {
        let error =
            catalog_root(Path::new("missing-client"), Path::new("../invalid-output")).unwrap_err();
        assert!(matches!(error, CliError::CatalogRootWindowsOnly));
        assert_eq!(error.to_string(), "catalog-root v1 is Windows-only");
    }
    #[test]
    #[cfg(windows)]
    fn existing_output_is_refused_before_client_access_and_left_intact() {
        let scratch = Scratch::new();
        let root = scratch.0.join("root");
        fs::create_dir(&root).unwrap();
        assert!(catalog_root(Path::new("missing-client"), &root).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::write(root.join("sentinel"), b"unchanged").unwrap();
        assert!(publish(&root, |_| panic!("must not prepare")).is_err());
        assert_eq!(fs::read(root.join("sentinel")).unwrap(), b"unchanged");
        let file = scratch.0.join("file");
        fs::write(&file, b"existing file").unwrap();
        assert!(publish(&file, |_| panic!("must not prepare")).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"existing file");
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
    }
    #[test]
    #[cfg(windows)]
    fn failed_preparation_and_racing_destination_preserve_other_files() {
        let scratch = Scratch::new();
        let unrelated = scratch.0.join(".maxed-catalog-root-unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("sentinel"), b"keep").unwrap();
        let root = scratch.0.join("root");
        assert!(publish(&root, |stage| {
            fs::write(stage.join("partial"), b"partial")?;
            Err(io::Error::other("synthetic failure").into())
        })
        .is_err());
        assert!(!root.exists());
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
        assert!(publish(&root, |stage| {
            let report = synthetic(stage)?;
            fs::create_dir(&root)?;
            Ok(report)
        })
        .is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        assert_eq!(fs::read(unrelated.join("sentinel")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
    }
    #[test]
    #[cfg(windows)]
    fn repository_public_paths_and_traversal_are_rejected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog-root-public");
        assert!(matches!(
            publish(&root, |_| panic!("must not prepare")),
            Err(CliError::InvalidArguments)
        ));
        assert!(matches!(
            publish(Path::new("../escape"), |_| panic!("must not prepare")),
            Err(CliError::InvalidArguments)
        ));
    }
    #[test]
    fn canonical_catalog_sorts_nested_keys_without_rounding_numbers() {
        let mut catalog: Catalog =
            serde_json::from_slice(&fs::read(fixture("catalog.json")).unwrap()).unwrap();
        catalog.crucible_bonuses.push(maxed_types::CrucibleDef {
            id: "synthetic".into(),
            damage_multiplier: 1.23456789123,
        });
        let bytes = canonical_catalog_bytes(&catalog).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("{\"bags\":[{\"base_type\":"));
        assert!(text.contains(
            "\"crucible_bonuses\":[{\"damage_multiplier\":1.23456789123,\"id\":\"synthetic\"}]"
        ));
        assert!(text.contains("\"projectiles\":[{\"armor_piercing\":false,\"index\":0,"));
        assert!(!text.contains('\n'));
    }
    #[test]
    fn xml_bytes_names_and_missing_required_rules_are_checked() {
        let mut texts = ["equip", "enchantments", "players"]
            .map(|name| TextAsset {
                name: name.into(),
                bytes: b"\xef\xbb\xbf<Objects/>\r\n".to_vec(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let xml = reference_xml(&texts).unwrap();
        assert_eq!(xml["xml/players.xml"], texts[2].bytes);
        texts.push(TextAsset {
            name: "equip".into(),
            bytes: b"<Objects/>".to_vec(),
        });
        assert!(reference_xml(&texts)
            .unwrap()
            .contains_key("xml/equip2.xml"));
        texts[3].name = "../escape".into();
        assert!(reference_xml(&texts).is_err());
        for name in ["NUL", "CON.extra", "LPT1", "COM9"] {
            texts[3].name = name.into();
            assert!(reference_xml(&texts).is_err());
        }
        // A non-XML text reserves its filename before XML sniffing, as Java does.
        texts[3] = TextAsset {
            name: "equip".into(),
            bytes: b"{}".to_vec(),
        };
        texts.push(TextAsset {
            name: "equip".into(),
            bytes: b"<Objects/>".to_vec(),
        });
        let xml = reference_xml(&texts).unwrap();
        assert!(!xml.contains_key("xml/equip2.xml"));
        assert!(xml.contains_key("xml/equip3.xml"));
        assert!(reference_xml(&[]).is_err());
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "requires MAXED_CLIENT_ASSETS; writes only a private temporary bundle"]
    fn installed_client_catalog_root() {
        let client =
            PathBuf::from(std::env::var_os("MAXED_CLIENT_ASSETS").expect("MAXED_CLIENT_ASSETS"));
        let scratch = Scratch::new();
        let output = scratch.0.join("root");
        let report = catalog_root(&client, &output).unwrap();
        verify(&output, &report);
        assert!(report.file_count > 7);
        let texts = maxed_assets::read_text_resources(&client, &AtomicBool::new(false)).unwrap();
        for name in ["equip", "enchantments", "players"] {
            let original = texts.texts.iter().find(|text| text.name == name).unwrap();
            assert_eq!(
                fs::read(output.join(format!("xml/{name}.xml"))).unwrap(),
                original.bytes
            );
        }
        let export = scratch.0.join("domain.jsonl");
        export_domain(
            &fixture("story.pcap"),
            &output.join("catalog.json"),
            &export,
        )
        .unwrap();
        let header: Value =
            serde_json::from_str(fs::read_to_string(export).unwrap().lines().next().unwrap())
                .unwrap();
        assert_eq!(header["catalogSha256"], report.catalog_sha256);
    }
}
