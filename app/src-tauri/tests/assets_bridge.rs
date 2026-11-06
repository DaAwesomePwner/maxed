use crate::fixture;
use maxed_app::assets_bridge::*;
use maxed_assets::{AssetError, AssetPaths, AssetProgress, CachedCatalog};
use maxed_types::{AssetCheck, Catalog, SpriteRef};
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

fn prepared() -> (tempfile::TempDir, AssetPaths, CachedCatalog) {
    let dir = tempfile::tempdir().unwrap();
    let paths = AssetPaths {
        resources: dir.path().join("resources.assets"),
        app_data: dir.path().join("app"),
    };
    fs::write(&paths.resources, fixture::fixture()).unwrap();
    let cache =
        maxed_assets::load_or_extract(&paths, &AtomicBool::new(false), &mut |_| {}).unwrap();
    (dir, paths, cache)
}
struct Fixed(CachedCatalog);
impl AssetLoader for Fixed {
    fn load(
        &self,
        _: &AssetPaths,
        _: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        progress(AssetProgress::Publishing);
        Ok(self.0.clone())
    }
}
fn publish(bridge: &AssetBridge, cache: &CachedCatalog) {
    let op = bridge.begin().unwrap();
    bridge.commit(&op, cache.clone(), || Ok(())).unwrap();
}

#[test]
fn extraction_does_not_write_client_and_only_commit_sets_ready() {
    let (_dir, paths, cache) = prepared();
    let before = fs::read(&paths.resources).unwrap();
    let bridge = AssetBridge::default();
    let op = bridge.begin().unwrap();
    let (_, loaded) = bridge
        .prepare(&paths.resources, &paths.app_data, &op)
        .unwrap();
    assert_ne!(bridge.status().assets, AssetCheck::Ready);
    assert_eq!(
        loaded.catalog.client_fingerprint,
        cache.catalog.client_fingerprint
    );
    bridge.commit(&op, loaded, || Ok(())).unwrap();
    assert_eq!(bridge.status().assets, AssetCheck::Ready);
    assert!(!bridge.status().live_confirmed);
    assert_eq!(fs::read(&paths.resources).unwrap(), before);
}

#[test]
fn cancellation_replacement_shutdown_and_failed_commit_keep_last_good_cache() {
    let (_dir, paths, cache) = prepared();
    let bridge = AssetBridge::new(Arc::new(Fixed(cache.clone())));
    publish(&bridge, &cache);
    let retained = bridge.cache().unwrap();
    let old = bridge.begin().unwrap();
    let newer = bridge.begin().unwrap();
    assert!(old.cancel.load(std::sync::atomic::Ordering::Acquire));
    assert!(bridge
        .prepare(&paths.resources, &paths.app_data, &old)
        .is_err());
    assert!(bridge
        .commit(&old, cache.clone(), || panic!("stale persist"))
        .is_err());
    assert!(bridge
        .commit(&newer, cache.clone(), || Err("Settings unavailable".into()))
        .is_err());
    bridge.cancel(true);
    assert!(bridge
        .commit(&newer, cache.clone(), || panic!("shutdown persist"))
        .is_err());
    assert!(bridge.begin().is_err());
    assert_eq!(bridge.cache().unwrap().catalog, retained.catalog);
    assert!(retained.root.join("catalog.json").is_file());
    bridge.resume();
    publish(&bridge, &cache);
}

#[test]
fn idle_cancel_preserves_ready_failed_unsupported_and_discovery_guidance() {
    let (_dir, paths, cache) = prepared();
    let bridge = AssetBridge::default();
    bridge.discovery_message("Multiple game clients found. Select a client.");
    let guidance = bridge.status();
    bridge.cancel(false);
    assert_eq!(bridge.status(), guidance);
    publish(&bridge, &cache);
    let ready = bridge.status();
    bridge.cancel(false);
    assert_eq!(bridge.status(), ready);
    let op = bridge.begin().unwrap();
    bridge.failed(&op);
    let failed = bridge.status();
    bridge.cancel(false);
    assert_eq!(bridge.status(), failed);
    let mut bytes = fixture::fixture();
    bytes[8..12].copy_from_slice(&99u32.to_be_bytes());
    fs::write(&paths.resources, bytes).unwrap();
    let op = bridge.begin().unwrap();
    assert!(bridge
        .prepare(&paths.resources, &paths.app_data, &op)
        .is_err());
    let unsupported = bridge.status();
    assert_eq!(unsupported.assets, AssetCheck::Unsupported);
    bridge.cancel(false);
    assert_eq!(bridge.status(), unsupported);
}

struct Failure(Mutex<Option<AssetError>>);
impl AssetLoader for Failure {
    fn load(
        &self,
        _: &AssetPaths,
        _: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        progress(AssetProgress::Reading);
        Err(self.0.lock().unwrap().take().unwrap())
    }
}
#[test]
fn missing_unsupported_read_permission_and_stale_cache_fail_safely() {
    let (_dir, paths, cache) = prepared();
    for error in [
        AssetError::UnsupportedVersion,
        AssetError::UnsupportedTexture,
        AssetError::Io,
        AssetError::InvalidStructure,
        AssetError::CacheInvalid,
        AssetError::Cancelled,
    ] {
        let unsupported = matches!(
            error,
            AssetError::UnsupportedVersion | AssetError::UnsupportedTexture
        );
        let bridge = AssetBridge::new(Arc::new(Failure(Mutex::new(Some(error)))));
        publish(&bridge, &cache);
        let op = bridge.begin().unwrap();
        assert_eq!(
            bridge
                .prepare(&paths.resources, &paths.app_data, &op)
                .unwrap_err(),
            ASSET_ERROR
        );
        assert_eq!(
            bridge.status().assets,
            if unsupported {
                AssetCheck::Unsupported
            } else {
                AssetCheck::Failed
            }
        );
        assert!(bridge.status().extraction_stage.is_none());
        assert_eq!(bridge.cache().unwrap().catalog, cache.catalog);
    }
    let bridge = AssetBridge::default();
    let op = bridge.begin().unwrap();
    assert!(bridge
        .prepare(
            &paths.resources.with_file_name("missing.assets"),
            &paths.app_data,
            &op
        )
        .is_err());
    assert!(resources_path(std::path::Path::new("resources.assets")).is_err());
    assert!(resources_path(std::path::Path::new(r"\\server\share\resources.assets")).is_err());
}

#[test]
fn actual_unsupported_reader_and_unreadable_input_preserve_published_cache() {
    let (_dir, paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let original = fs::read(&paths.resources).unwrap();
    let mut unsupported = original.clone();
    unsupported[8..12].copy_from_slice(&99u32.to_be_bytes());
    fs::write(&paths.resources, unsupported).unwrap();
    let op = bridge.begin().unwrap();
    assert_eq!(
        bridge
            .prepare(&paths.resources, &paths.app_data, &op)
            .unwrap_err(),
        ASSET_ERROR
    );
    assert_eq!(bridge.status().assets, AssetCheck::Unsupported);
    assert_eq!(bridge.cache().unwrap().catalog, cache.catalog);
    fs::write(&paths.resources, original).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let _locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&paths.resources)
            .unwrap();
        let op = bridge.begin().unwrap();
        assert_eq!(
            bridge
                .prepare(&paths.resources, &paths.app_data, &op)
                .unwrap_err(),
            ASSET_ERROR
        );
        assert_eq!(bridge.status().assets, AssetCheck::Failed);
        assert_eq!(bridge.cache().unwrap().catalog, cache.catalog);
    }
}

struct Late {
    cache: CachedCatalog,
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}
impl AssetLoader for Late {
    fn load(
        &self,
        _: &AssetPaths,
        _: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        progress(AssetProgress::Publishing);
        Ok(self.cache.clone())
    }
}
#[test]
fn late_progress_and_result_cannot_publish_over_a_replacement() {
    let (_dir, paths, cache) = prepared();
    let (entered, wait) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    let bridge = Arc::new(AssetBridge::new(Arc::new(Late {
        cache: cache.clone(),
        entered,
        release: gate,
    })));
    publish(&bridge, &cache);
    let op = bridge.begin().unwrap();
    let worker_bridge = bridge.clone();
    let worker =
        std::thread::spawn(move || worker_bridge.prepare(&paths.resources, &paths.app_data, &op));
    wait.recv().unwrap();
    let replacement = bridge.begin().unwrap();
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap().unwrap_err(), CANCELLED);
    assert_eq!(bridge.status().assets, AssetCheck::Finding);
    assert!(bridge.status().extraction_stage.is_none());
    bridge.commit(&replacement, cache, || Ok(())).unwrap();
}

#[test]
fn changed_client_fingerprint_keeps_old_lease_and_revokes_old_urls() {
    let (_dir, paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let old_url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        cache.catalog.client_fingerprint
    );
    assert!(bridge.sprite(&old_url).is_ok());
    let mut bytes = fs::read(&paths.resources).unwrap();
    let at = bytes.windows(10).position(|v| v == b"<Objects/>").unwrap();
    bytes[at..at + 10].copy_from_slice(b"<Grounds/>");
    fs::write(&paths.resources, bytes).unwrap();
    let op = bridge.begin().unwrap();
    let (_, next) = bridge
        .prepare(&paths.resources, &paths.app_data, &op)
        .unwrap();
    assert_ne!(
        next.catalog.client_fingerprint,
        cache.catalog.client_fingerprint
    );
    bridge.commit(&op, next, || Ok(())).unwrap();
    assert!(bridge.sprite(&old_url).is_err());
    assert!(cache.root.join("catalog.json").is_file());
}

#[test]
fn production_origin_and_malformed_encoded_traversal_and_absolute_urls() {
    let hash = "a".repeat(64);
    assert_eq!(
        sprite_location(&format!(
            "{SPRITE_ORIGIN}/{hash}/characters.png/1-0-1-1.png"
        ))
        .unwrap()
        .1,
        maxed_assets::sprite_delivery::TileKey::from_sprite(&SpriteRef {
            key: "test".into(),
            sheet: "characters.png".into(),
            x: 1,
            y: 0,
            width: 1,
            height: 1,
            trim: None
        })
        .unwrap()
    );
    assert_eq!(
        sprite_location(&format!(
            "{SPRITE_ORIGIN}/{hash}/enchantPips.png/0-0-16-16.png"
        ))
        .unwrap()
        .1,
        maxed_assets::sprite_delivery::TileKey::from_sprite(&SpriteRef {
            key: "test".into(),
            sheet: "enchantPips.png".into(),
            x: 0,
            y: 0,
            width: 16,
            height: 16,
            trim: None
        })
        .unwrap()
    );
    for suffix in [
        "../characters.png/1-0-1-1.png",
        "%2e%2e/characters.png/1-0-1-1.png",
        "%252e%252e/characters.png/1-0-1-1.png",
        "C:/secret.png",
        "\\secret.png",
        "characters.png?x=1",
        "characters.png#x",
        "characters.png/extra",
        "characters.svg",
        "%63haracters.png",
        "characters.png%00",
        "characters.png",
        "characters.png/0-0-0-8.png",
        "characters.png/0-0-257-8.png",
        "characters.png/16384-0-1-1.png",
        "characters.png/0-16384-1-1.png",
        "characters.png/01-0-1-1.png",
        "characters.png/1-0-1-1.png?x=1",
        "characters.png/1-0-1-1.png#x",
        "characters.png/%31-0-1-1.png",
        "characters.png/1-0-1-1.png/extra",
    ] {
        assert!(
            sprite_location(&format!("{SPRITE_ORIGIN}/{hash}/{suffix}")).is_err(),
            "{suffix}"
        );
    }
    for url in [
        format!("https://maxed-sprite.localhost/{hash}/characters.png/1-0-1-1.png"),
        format!("http://evil.localhost/{hash}/characters.png/1-0-1-1.png"),
        format!("{SPRITE_ORIGIN}/bad/characters.png/1-0-1-1.png"),
        format!("{SPRITE_ORIGIN}:80/{hash}/characters.png/1-0-1-1.png"),
    ] {
        assert!(sprite_location(&url).is_err());
    }
}

#[test]
fn sprite_delivery_caches_generation_metadata_but_checks_each_payload() {
    let (_dir, _paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        cache.catalog.client_fingerprint
    );
    assert!(bridge
        .sprite(&url)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    let unknown = sprite_location(&url.replace("characters.png", "groundTiles.png"))
        .unwrap()
        .1;
    assert!(read_sprite(&cache, unknown).is_err());
    let path = cache.root.join("tiles.pack");
    let original = fs::read(&path).unwrap();
    fs::write(&path, vec![b'<'; original.len()]).unwrap();
    assert!(bridge.sprite(&url).is_err());
    fs::write(&path, original).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(cache.root.join("manifest.json")).unwrap()).unwrap();
    manifest["fingerprint"] = "b".repeat(64).into();
    fs::write(
        cache.root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    // Active leases retain already-validated metadata and their open pack.
    // Tampered manifests are rejected on reload, not reparsed on every tile.
    assert!(bridge.sprite(&url).is_ok());
    assert!(maxed_assets::load_or_extract(&_paths, &AtomicBool::new(false), &mut |_| {}).is_err());
    fs::remove_file(path).unwrap();
    assert!(bridge.sprite(&url).is_ok());
}

#[test]
fn sprite_protocol_caches_only_valid_fingerprinted_images() {
    let (_dir, paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        cache.catalog.client_fingerprint
    );
    let expected = bridge.sprite(&url).unwrap();
    for _ in 0..3 {
        let response = bridge.sprite_response(
            true,
            &tauri::http::Method::GET,
            &url,
            Some("http://tauri.localhost"),
        );
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), &expected);
        assert_eq!(
            response.headers()["Cache-Control"],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(response.headers()["X-Content-Type-Options"], "nosniff");
        assert_eq!(
            response.headers()["Access-Control-Allow-Origin"],
            "http://tauri.localhost"
        );
        assert_eq!(response.headers()["Vary"], "Origin");
    }
    for (main, method, uri) in [
        (false, tauri::http::Method::GET, url.as_str()),
        (true, tauri::http::Method::POST, url.as_str()),
        (true, tauri::http::Method::GET, "invalid"),
    ] {
        let response = bridge.sprite_response(main, &method, uri, Some("http://tauri.localhost"));
        assert_eq!(response.status(), 404);
        assert_eq!(response.headers()["Cache-Control"], "no-store");
    }
    let mut bytes = fs::read(&paths.resources).unwrap();
    let at = bytes
        .windows(10)
        .position(|value| value == b"<Objects/>")
        .unwrap();
    bytes[at..at + 10].copy_from_slice(b"<Grounds/>");
    fs::write(&paths.resources, bytes).unwrap();
    let op = bridge.begin().unwrap();
    let (_, next) = bridge
        .prepare(&paths.resources, &paths.app_data, &op)
        .unwrap();
    let next_url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        next.catalog.client_fingerprint
    );
    assert_ne!(url, next_url);
    bridge.commit(&op, next, || Ok(())).unwrap();
    assert_eq!(
        bridge
            .sprite_response(true, &tauri::http::Method::GET, &url, None)
            .status(),
        404
    );
    assert_eq!(
        bridge
            .sprite_response(true, &tauri::http::Method::GET, &next_url, None)
            .status(),
        200
    );
}

#[test]
fn sprite_allows_app_origin() {
    let (_dir, _paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        cache.catalog.client_fingerprint
    );
    let origin = "http://tauri.localhost";
    let response = bridge.sprite_response(true, &tauri::http::Method::GET, &url, Some(origin));
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["Access-Control-Allow-Origin"], origin);
    assert_eq!(response.headers()["Vary"], "Origin");
}

#[test]
fn sprite_rejects_foreign_or_missing_origin() {
    let (_dir, _paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        cache.catalog.client_fingerprint
    );
    for origin in [Some("http://evil.localhost"), None] {
        let response = bridge.sprite_response(true, &tauri::http::Method::GET, &url, origin);
        assert_eq!(response.status(), 200);
        assert!(response
            .headers()
            .get("Access-Control-Allow-Origin")
            .is_none());
        assert_eq!(response.headers()["Vary"], "Origin");
    }
}

#[test]
fn failed_sprite_has_no_cors() {
    let (_dir, _paths, cache) = prepared();
    let bridge = AssetBridge::default();
    publish(&bridge, &cache);
    let url = format!(
        "{SPRITE_ORIGIN}/{}/characters.png/1-0-1-1.png",
        cache.catalog.client_fingerprint
    );
    for (main, method, uri) in [
        (false, tauri::http::Method::GET, url.as_str()),
        (true, tauri::http::Method::POST, url.as_str()),
        (true, tauri::http::Method::GET, "invalid"),
    ] {
        let response = bridge.sprite_response(main, &method, uri, Some("http://tauri.localhost"));
        assert_eq!(response.status(), 404);
        assert!(response
            .headers()
            .get("Access-Control-Allow-Origin")
            .is_none());
        assert!(response.headers().get("Vary").is_none());
    }
}

#[test]
fn app_origin_allowlist_is_exact() {
    assert!(app_origin_allowed("http://tauri.localhost"));
    assert_eq!(
        app_origin_allowed("http://127.0.0.1:1420"),
        cfg!(debug_assertions)
    );
    for origin in [
        "",
        "null",
        "*",
        "http://evil.localhost",
        "https://tauri.localhost",
        "http://tauri.localhost/",
        "http://tauri.localhost:80",
        "http://tauri.localhost.evil",
        "http://localhost:1420",
        "http://127.0.0.1:1421",
        "http://127.0.0.1:1420/",
    ] {
        assert!(
            !app_origin_allowed(origin),
            "unexpected allowed origin: {origin}"
        );
    }
}

#[test]
fn visuals_are_bounded_deduplicated_and_unknown_is_null() {
    let mut catalog: Catalog = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
    ))
    .unwrap();
    let object = catalog.objects.first().unwrap().object_type;
    catalog.sprites.push(SpriteRef {
        key: "synthetic".into(),
        sheet: "characters.png".into(),
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        trim: None,
    });
    catalog.objects[0].sprite_key = Some("synthetic".into());
    let page = visuals(&catalog, &[object, object, 65535]).unwrap();
    assert_eq!(page.objects.len(), 2);
    assert!(page
        .objects
        .iter()
        .find(|v| v.object_type == object)
        .unwrap()
        .sprite
        .is_some());
    let unknown = page
        .objects
        .iter()
        .find(|v| v.object_type == 65535)
        .unwrap();
    assert!(unknown.name.is_none() && unknown.sprite.is_none() && unknown.bag.is_none());
    assert_eq!(visuals(&catalog, &[0; 129]).unwrap().objects.len(), 1);
    assert!(visuals(&catalog, &(0..129).collect::<Vec<_>>()).is_err());
    assert!(visuals(&catalog, &[]).unwrap().objects.is_empty());
}

#[test]
fn visual_item_tier_optional_and_catalog_only() {
    let mut catalog = maxed_types::demo::demo_catalog();
    let item_type = catalog.items[0].object_type;
    for tier in [Some("UT"), Some("ST"), Some("14"), None] {
        catalog.items[0].tier = tier.map(str::to_owned);
        let page = visuals(&catalog, &[item_type]).unwrap();
        assert_eq!(page.objects.len(), 1);
        assert_eq!(page.client_fingerprint, catalog.client_fingerprint);
        assert_eq!(page.objects[0].item_tier.as_deref(), tier);
    }
    catalog.items.clear();
    let object = catalog.objects[0].object_type;
    let bag = catalog.bags[0].object_type;
    for value in visuals(&catalog, &[object, bag, 65535]).unwrap().objects {
        assert_eq!(value.item_tier, None);
    }
}

#[test]
fn dungeon_portal_type_resolves_names_and_aliases_and_missing_is_none() {
    let mut catalog: Catalog = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
    ))
    .unwrap();
    // Catalogs extracted before portal mapping deserialize without one.
    assert_eq!(dungeon_portal_type(&catalog, "Snake Pit"), None);
    let pit = catalog
        .dungeons
        .iter_mut()
        .find(|d| d.name == "Snake Pit")
        .unwrap();
    pit.aliases.push("Snake Pit (Display)".into());
    pit.portal_type = Some(1803);
    assert_eq!(dungeon_portal_type(&catalog, "Snake Pit"), Some(1803));
    assert_eq!(
        dungeon_portal_type(&catalog, "Snake Pit (Display)"),
        Some(1803)
    );
    assert_eq!(dungeon_portal_type(&catalog, "Nexus"), None);
    assert_eq!(dungeon_portal_type(&catalog, "Unknown Map"), None);
    assert_eq!(dungeon_portal_type(&catalog, ""), None);
}

#[test]
fn search_items_orders_prefix_first() {
    let mut catalog: Catalog = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
    ))
    .unwrap();
    let template = catalog.items.first().unwrap().clone();
    catalog.items = [
        (1, "Sword of Acclaim"),
        (2, "Acclaim Sword"),
        (3, "Dagger"),
        (7, "Acclaim Axe"),
        (6, "Acclaim Axe"),
        (5, "Acclaim Bow"),
        (4, "Xacclaim"),
    ]
    .into_iter()
    .map(|(object_type, name)| maxed_types::ItemDef {
        weapon_firing: None,
        potion_stat: None,
        object_type,
        name: name.into(),
        tier: Some("UT".into()),
        ..template.clone()
    })
    .collect();
    let hits = search_items(&catalog, "  aCcLaIm  ", 25).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.object_type).collect::<Vec<_>>(),
        vec![6, 7, 5, 2, 4, 1]
    );
    assert_eq!(hits[0].name, "Acclaim Axe");
    assert_eq!(hits[0].tier.as_deref(), Some("UT"));
    assert_eq!(search_items(&catalog, "acclaim", 1).unwrap(), hits[..1]);
    assert!(search_items(&catalog, "missing", 25).unwrap().is_empty());
}

#[test]
fn search_items_rejects_bad_queries() {
    let (_dir, _paths, cache) = prepared();
    let bridge = AssetBridge::default();
    assert!(bridge.search_items("sword", 10).is_err());
    publish(&bridge, &cache);
    for query in [
        "".into(),
        "   ".into(),
        "x".repeat(65),
        "a\u{0007}".into(),
        "a\nb".into(),
    ] {
        assert_eq!(
            search_items(&cache.catalog, &query, 10).unwrap_err(),
            "Invalid search."
        );
        assert_eq!(
            bridge.search_items(&query, 10).unwrap_err(),
            "Invalid search."
        );
    }
    for limit in [0, 26, 255] {
        assert_eq!(
            search_items(&cache.catalog, "sword", limit).unwrap_err(),
            "Invalid search."
        );
    }
    for query in ["x".repeat(64), "é".repeat(64)] {
        assert!(search_items(&cache.catalog, &query, 25).is_ok());
    }
    assert_eq!(
        bridge.search_items("sword", 25).unwrap(),
        search_items(&cache.catalog, "sword", 25).unwrap()
    );
}

#[test]
fn dungeon_picker_options_retain_aliases_and_exclude_non_dungeons() {
    let mut catalog = maxed_types::demo::demo_catalog();
    let dungeon = catalog
        .dungeons
        .iter_mut()
        .find(|d| d.kind == maxed_types::AreaKind::Dungeon)
        .unwrap();
    dungeon.aliases.push("Synthetic alias".into());
    let name = dungeon.name.clone();
    let options = catalog_dungeon_options(&catalog);
    assert!(options
        .iter()
        .all(|d| d.kind == maxed_types::AreaKind::Dungeon));
    assert!(options
        .iter()
        .find(|d| d.name == name)
        .unwrap()
        .aliases
        .contains(&"Synthetic alias".into()));
    assert_eq!(
        options.iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
        catalog_dungeons(&catalog)
    );
}

fn detail_catalog() -> Catalog {
    serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
    ))
    .unwrap()
}

#[test]
fn item_details_keep_unknown_rows_and_deduplicate() {
    let catalog = detail_catalog();
    let item = catalog.items[0].object_type;
    let page = item_details(&catalog, &[item, item, 65535], &[1, 1, 999_999]).unwrap();
    assert_eq!(page.client_fingerprint, catalog.client_fingerprint);
    assert_eq!(
        page.items
            .iter()
            .map(|row| row.object_type)
            .collect::<Vec<_>>(),
        [item, 65535]
    );
    assert_eq!(
        page.enchants.iter().map(|row| row.id).collect::<Vec<_>>(),
        [1, 999_999]
    );
    let unknown = &page.items[1];
    assert!(
        unknown.name.is_none()
            && unknown.tier.is_none()
            && unknown.slot_type.is_none()
            && unknown.slot_label.is_none()
            && unknown.soulbound.is_none()
    );
    assert_eq!(page.enchants[0].name.as_deref(), Some("Demo Enchant"));
    assert!(!page.enchants[0].description.as_ref().unwrap().is_empty());
    assert!(page.enchants[1].name.is_none() && page.enchants[1].description.is_none());
}

#[test]
fn item_details_are_bounded() {
    let catalog = detail_catalog();
    assert_eq!(
        item_details(&catalog, &[0; 129], &[1; 129])
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        item_details(&catalog, &(0..129).collect::<Vec<_>>(), &[]).unwrap_err(),
        "Request at most 128 unique object types."
    );
    assert_eq!(
        item_details(&catalog, &[], &(0..129).collect::<Vec<_>>()).unwrap_err(),
        "Request at most 128 unique enchant IDs."
    );
    let empty = item_details(&catalog, &[], &[]).unwrap();
    assert!(empty.items.is_empty() && empty.enchants.is_empty());
}

#[test]
fn slot_labels_match_the_client_table() {
    for (slot, label) in [
        (1, "Sword"),
        (6, "Leather Armor"),
        (25, "Star"),
        (31, "Sigil"),
    ] {
        assert_eq!(slot_label(slot), Some(label));
    }
    assert_eq!(slot_label(0), None);
    assert_eq!(slot_label(32), None);
    let mut catalog = detail_catalog();
    let item = catalog.items[0].object_type;
    for (slot, label) in [(17, Some("Staff")), (40, None)] {
        catalog.items[0].slot_type = Some(slot);
        let page = item_details(&catalog, &[item], &[]).unwrap();
        assert_eq!(page.items[0].slot_type, Some(slot));
        assert_eq!(page.items[0].slot_label.as_deref(), label);
    }
}

#[test]
fn item_details_carry_name_tier_and_soulbound() {
    let mut catalog = detail_catalog();
    catalog.items[0].tier = Some("UT".into());
    catalog.items[0].soulbound = Some(true);
    catalog.enchants[0].description.clear();
    let page = item_details(
        &catalog,
        &[catalog.items[0].object_type],
        &[catalog.enchants[0].id],
    )
    .unwrap();
    assert_eq!(page.items[0].name.as_ref(), Some(&catalog.items[0].name));
    assert_eq!(page.items[0].tier.as_deref(), Some("UT"));
    assert_eq!(page.items[0].soulbound, Some(true));
    assert_eq!(page.enchants[0].description, None);
}
