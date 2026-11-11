use crate::common;
use maxed_assets::{client_fingerprint, load_or_extract, AssetError, AssetPaths, AssetProgress};
use std::sync::atomic::{AtomicBool, Ordering};
fn setup() -> (tempfile::TempDir, AssetPaths) {
    let dir = tempfile::tempdir().unwrap();
    let resources = dir.path().join("resources.assets");
    std::fs::write(&resources, common::fixture()).unwrap();
    let paths = AssetPaths {
        resources,
        app_data: dir.path().join("app"),
    };
    (dir, paths)
}

#[test]
fn cancellation_precedes_any_filesystem_access() {
    let p = AssetPaths {
        resources: "missing/resources.assets".into(),
        app_data: "missing/app".into(),
    };
    assert!(matches!(
        load_or_extract(&p, &AtomicBool::new(true), &mut |_| {}),
        Err(AssetError::Cancelled)
    ));
}
#[test]
fn cache_round_trip_hashes_png_and_retains_prior_generation() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let first = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let mut phases = vec![];
    let second = load_or_extract(&p, &flag, &mut |x| phases.push(x)).unwrap();
    assert_eq!(first.catalog, second.catalog);
    assert_eq!(first.root, second.root);
    assert!(!phases.contains(&AssetProgress::Reading));
    let mut bytes = std::fs::read(&p.resources).unwrap();
    let last = bytes.len() - 1;
    bytes[last] = 0; // preserve size
    let at = bytes.windows(10).position(|v| v == b"<Objects/>").unwrap();
    bytes[at..at + 10].copy_from_slice(b"<Grounds/>");
    let old_mtime = std::fs::metadata(&p.resources).unwrap().modified().unwrap();
    std::fs::write(&p.resources, &bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&p.resources)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(old_mtime))
        .unwrap();
    let third = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    assert_ne!(first.root, third.root);
    assert!(first.root.join("catalog.json").is_file());
}

#[test]
fn loaded_catalog_keeps_all_data_without_parser_growth_capacity() {
    let (_dir, paths) = setup();
    let cancel = AtomicBool::new(false);
    let first = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
    let expected = (*first.catalog).clone();
    drop(first); // Ensure the next load really deserializes, with no live lease.
    let loaded = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
    assert_eq!(*loaded.catalog, expected);
    macro_rules! compact {
        ($($field:ident),+) => { $(assert_eq!(loaded.catalog.$field.capacity(), loaded.catalog.$field.len(),
            "{} retained parser growth capacity", stringify!($field));)+ };
    }
    compact!(
        items,
        objects,
        classes,
        enchants,
        dungeons,
        bags,
        sprites,
        tiles,
        crucible_bonuses
    );
    let shared = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
    assert!(std::sync::Arc::ptr_eq(&shared.catalog, &loaded.catalog));
}
#[test]
fn corrupt_catalog_and_png_are_never_served() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let cached = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    std::fs::write(cached.root.join("catalog.json"), b"{}").unwrap();
    assert!(matches!(
        load_or_extract(&p, &flag, &mut |_| {}),
        Err(AssetError::CacheInvalid)
    ));
}
#[test]
fn cancellation_cleans_only_its_own_stage() {
    for phase in [
        AssetProgress::Fingerprinting,
        AssetProgress::Reading,
        AssetProgress::Catalog,
        AssetProgress::Sprites,
        AssetProgress::Publishing,
    ] {
        let (_dir, p) = setup();
        let root = p.app_data.join("assets/v1");
        std::fs::create_dir_all(root.join("unrelated-stage")).unwrap();
        let flag = AtomicBool::new(false);
        let result = load_or_extract(&p, &flag, &mut |x| {
            if x == phase {
                flag.store(true, Ordering::Relaxed);
            }
        });
        assert!(matches!(result, Err(AssetError::Cancelled)));
        assert!(root.join("unrelated-stage").is_dir());
        assert!(!std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".stage-")));
    }
}
#[test]
fn publication_failure_preserves_pointer_and_prior_generation() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let cached = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let pointer = p.app_data.join("assets/v1/current");
    let bytes = std::fs::read(&pointer).unwrap();
    #[cfg(windows)]
    let _lock = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&pointer)
            .unwrap()
    };
    let mut data = std::fs::read(&p.resources).unwrap();
    let at = data.windows(10).position(|v| v == b"<Objects/>").unwrap();
    data[at..at + 10].copy_from_slice(b"<Grounds/>");
    std::fs::write(&p.resources, data).unwrap();
    #[cfg(windows)]
    {
        assert!(matches!(
            load_or_extract(&p, &flag, &mut |_| {}),
            Err(AssetError::CacheInvalid)
        ));
        drop(_lock);
        assert_eq!(std::fs::read(&pointer).unwrap(), bytes);
    }
    assert!(cached.root.is_dir());
}
#[test]
fn fingerprint_reads_bytes_not_metadata() {
    let (_d, p) = setup();
    let first = client_fingerprint(&p.resources).unwrap();
    let mut b = std::fs::read(&p.resources).unwrap();
    let at = b.windows(10).position(|v| v == b"<Objects/>").unwrap();
    b[at + 1] = b'G';
    std::fs::write(&p.resources, b).unwrap();
    assert_ne!(first, client_fingerprint(&p.resources).unwrap());
}

#[test]
fn companion_bytes_are_hashed_and_missing_or_escaping_companions_fail() {
    let (d, p) = setup();
    let stream = d.path().join("pixels.resS");
    std::fs::write(
        &p.resources,
        common::fixture_with_stream(Some("pixels.resS")),
    )
    .unwrap();
    assert!(client_fingerprint(&p.resources).is_err());
    std::fs::write(&stream, [0; 16]).unwrap();
    let first = client_fingerprint(&p.resources).unwrap();
    std::fs::write(&stream, [1; 16]).unwrap();
    assert_ne!(first, client_fingerprint(&p.resources).unwrap());
    for path in ["../pixels.resS", "C:/pixels.resS", "/pixels.resS"] {
        std::fs::write(&p.resources, common::fixture_with_stream(Some(path))).unwrap();
        assert!(client_fingerprint(&p.resources).is_err());
    }
}
#[test]
fn corrupted_png_and_manifest_are_rejected() {
    for file in ["characters.png", "tiles.pack", "manifest.json"] {
        let (_dir, p) = setup();
        let flag = AtomicBool::new(false);
        let cached = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
        std::fs::write(cached.root.join(file), b"broken").unwrap();
        assert!(matches!(
            load_or_extract(&p, &flag, &mut |_| {}),
            Err(AssetError::CacheInvalid)
        ));
    }
}
#[test]
fn patched_during_extraction_is_not_published() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let result = load_or_extract(&p, &flag, &mut |phase| {
        if phase == AssetProgress::Publishing {
            let mut b = std::fs::read(&p.resources).unwrap();
            let at = b.windows(10).position(|v| v == b"<Objects/>").unwrap();
            b[at + 1] = b'G';
            std::fs::write(&p.resources, b).unwrap();
        }
    });
    assert!(matches!(result, Err(AssetError::CacheInvalid)));
    assert!(!p.app_data.join("assets/v1/current").exists());
}
#[test]
fn pruning_waits_for_all_readers_and_leaves_unknown_directories() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let first = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let second_reader = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let old = first.root.clone();
    let mut b = std::fs::read(&p.resources).unwrap();
    let at = b.windows(10).position(|v| v == b"<Objects/>").unwrap();
    b[at + 1] = b'G';
    std::fs::write(&p.resources, b).unwrap();
    let current = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let unknown = current.root.parent().unwrap().join("unknown");
    std::fs::create_dir(&unknown).unwrap();
    let unknown_quarantine = current.root.parent().unwrap().join(".corrupt-unrelated");
    std::fs::create_dir(&unknown_quarantine).unwrap();
    let icons = current.root.parent().unwrap().join("toast-icons");
    let old_icons = icons.join(&first.catalog.client_fingerprint);
    let current_icons = icons.join(&current.catalog.client_fingerprint);
    std::fs::create_dir_all(&old_icons).unwrap();
    std::fs::create_dir_all(&current_icons).unwrap();
    drop(first);
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
        0
    );
    assert!(old.is_dir());
    assert!(old_icons.is_dir());
    drop(second_reader);
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
        1
    );
    assert!(!old.exists());
    assert!(!old_icons.exists());
    assert!(current_icons.is_dir());
    assert!(current.root.is_dir());
    assert!(unknown.is_dir());
    assert!(unknown_quarantine.is_dir());
}

#[test]
#[cfg(windows)]
fn corrupted_generation_recovers_only_after_all_readers_release() {
    for file in ["catalog.json", "characters.png", "manifest.json"] {
        let (_dir, p) = setup();
        let flag = AtomicBool::new(false);
        let cached = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
        let expected = (*cached.catalog).clone();
        let generation = cached.root.clone();
        let arc_reader = cached.catalog.clone();
        std::fs::write(generation.join(file), b"broken").unwrap();
        drop(cached);
        assert!(matches!(
            load_or_extract(&p, &flag, &mut |_| {}),
            Err(AssetError::CacheInvalid)
        ));
        drop(arc_reader);
        // Exercise the cross-process lease independently of the in-process map.
        use std::os::windows::fs::OpenOptionsExt;
        let os_reader = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(3)
            .open(generation.with_extension("lease"))
            .unwrap();
        assert!(matches!(
            load_or_extract(&p, &flag, &mut |_| {}),
            Err(AssetError::CacheInvalid)
        ));
        drop(os_reader);
        let mut phases = vec![];
        let repaired = load_or_extract(&p, &flag, &mut |phase| phases.push(phase)).unwrap();
        assert!(phases.contains(&AssetProgress::Reading));
        assert_eq!(*repaired.catalog, expected);
        assert_eq!(repaired.root, generation);
        let root = generation.parent().unwrap();
        let quarantines: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".corrupt-"))
            .collect();
        assert_eq!(quarantines.len(), 1);
        let quarantined = quarantines[0].path();
        assert_eq!(std::fs::read(quarantined.join(file)).unwrap(), b"broken");
        let manifest = std::fs::OpenOptions::new()
            .write(true)
            .open(generation.join("manifest.json"))
            .unwrap();
        // An older valid generation cannot authorize deleting a newer quarantine.
        manifest
            .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        drop(repaired);
        assert_eq!(
            maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
            0
        );
        manifest
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        let leased = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
        // A repaired live generation does not lease its old quarantine.
        assert_eq!(
            maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
            1
        );
        assert!(leased.root.is_dir());
        drop(leased);
        assert!(!quarantined.exists());
        assert!(generation.is_dir());
        assert_eq!(std::fs::read(&p.resources).unwrap(), common::fixture());
    }
}

#[test]
#[cfg(windows)]
fn invalid_generation_created_during_extraction_is_quarantined_at_publish() {
    let (_dir, p) = setup();
    let fingerprint = client_fingerprint(&p.resources).unwrap();
    let generation = p.app_data.join("assets/v1").join(&fingerprint);
    let repaired = load_or_extract(&p, &AtomicBool::new(false), &mut |phase| {
        if phase == AssetProgress::Publishing {
            std::fs::create_dir(&generation).unwrap();
            std::fs::write(generation.join("manifest.json"), b"interrupted writer").unwrap();
        }
    })
    .unwrap();
    assert_eq!(repaired.catalog.client_fingerprint, fingerprint);
    let quarantines: Vec<_> = std::fs::read_dir(generation.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".corrupt-"))
        .collect();
    assert_eq!(quarantines.len(), 1);
    assert_eq!(
        std::fs::read(quarantines[0].path().join("manifest.json")).unwrap(),
        b"interrupted writer"
    );
}

#[test]
#[cfg(windows)]
fn cancelled_repair_retains_quarantine_and_retry_succeeds() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let cached = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let generation = cached.root.clone();
    let expected = (*cached.catalog).clone();
    // A structurally valid manifest with an old format is invalidated too.
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(generation.join("manifest.json")).unwrap()).unwrap();
    manifest["version"] = serde_json::json!(0);
    std::fs::write(
        generation.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    drop(cached);
    let result = load_or_extract(&p, &flag, &mut |phase| {
        if phase == AssetProgress::Reading {
            flag.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(result, Err(AssetError::Cancelled)));
    assert!(!generation.exists());
    let root = generation.parent().unwrap();
    let quarantines: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".corrupt-"))
        .collect();
    assert_eq!(quarantines.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(quarantines[0].path().join("manifest.json")).unwrap()
        )
        .unwrap()["version"],
        0
    );
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &AtomicBool::new(false)).unwrap(),
        0
    );
    flag.store(false, Ordering::Relaxed);
    let repaired = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    assert_eq!(*repaired.catalog, expected);
    assert_eq!(repaired.root, generation);
    assert!(quarantines[0].path().exists());
}

#[test]
#[cfg(windows)]
fn successful_publish_prunes_v9_but_preserves_readers_and_inflight_extraction() {
    let (_dir, paths) = setup();
    let cancel = AtomicBool::new(false);
    let old = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
    let retired = old.root.clone();
    drop(old);
    // A reviewed synthetic v9 generation: same atlas/catalog contract, no pack.
    let manifest_path = retired.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = 9.into();
    manifest["files"]
        .as_object_mut()
        .unwrap()
        .remove("tiles.pack");
    std::fs::remove_file(retired.join("tiles.pack")).unwrap();
    std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    // Shared lease models another process still reading the v9 generation.
    use std::os::windows::fs::OpenOptionsExt;
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .open(retired.with_extension("lease"))
        .unwrap();
    let resources2 = paths.resources.with_file_name("second.assets");
    let mut bytes = common::fixture();
    let at = bytes.windows(10).position(|b| b == b"<Objects/>").unwrap();
    bytes[at..at + 10].copy_from_slice(b"<Grounds/>");
    std::fs::write(&resources2, bytes).unwrap();
    let second_paths = AssetPaths {
        resources: resources2,
        app_data: paths.app_data.clone(),
    };
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        load_or_extract(&second_paths, &AtomicBool::new(false), &mut |phase| {
            if phase == AssetProgress::Publishing {
                ready_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            }
        })
        .unwrap()
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    // Third generation publishes while the second has a completed staging dir
    // and a live shared lease. Pruning must leave both it and the v9 reader alone.
    let mut bytes = common::fixture();
    bytes[at..at + 10].copy_from_slice(b"<Sprites/>");
    std::fs::write(&paths.resources, bytes).unwrap();
    let third = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
    assert!(retired.is_dir());
    let stages: Vec<_> = std::fs::read_dir(third.root.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".stage-"))
        .collect();
    assert_eq!(stages.len(), 1);
    drop(reader);
    resume_tx.send(()).unwrap();
    let second = worker.join().unwrap();
    assert!(second.root.is_dir());
    assert!(third.root.is_dir()); // leased in-process
    second.prune_in_background();
    for _ in 0..200 {
        if !retired.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!retired.exists()); // retired by scheduled maintenance
}

#[test]
#[cfg(windows)]
fn partial_retirement_is_retried_and_other_entries_continue() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let current = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let root = current.root.parent().unwrap();
    let fp = "b".repeat(64);
    let old = root.join(&fp);
    std::fs::create_dir(&old).unwrap();
    // Pre-v9 schema: safely identified without deserializing its catalog.
    std::fs::write(
        old.join("manifest.json"),
        format!(r#"{{"version":8,"fingerprint":"{fp}","sheets":[],"files":{{}}}}"#),
    )
    .unwrap();
    // Model the retirement boundary, then an indexer opens the renamed tree.
    let retired_path = root.join(format!(".retired-{fp}-1-2-3"));
    std::fs::rename(&old, &retired_path).unwrap();
    let locked = retired_path.join("z-locked");
    std::fs::write(&locked, b"synthetic").unwrap();
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&locked)
        .unwrap();
    let other = root.join(format!(".retired-{}-1-2-3", "c".repeat(64)));
    std::fs::create_dir(&other).unwrap();
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
        1
    );
    assert!(!old.exists(), "renamed before the sharing violation");
    assert!(!other.exists(), "a failure does not abort the sweep");
    let retired = std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!(".retired-{fp}-"))
        })
        .unwrap()
        .path();
    assert!(retired.join("z-locked").exists());
    assert!(
        !retired.join("manifest.json").exists(),
        "first deletion made partial progress"
    );
    drop(handle);
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
        1
    );
    assert!(!retired.exists());
}

#[test]
#[cfg(windows)]
fn retires_pre_v9_generations_but_preserves_unknown_identity() {
    let (_dir, p) = setup();
    let flag = AtomicBool::new(false);
    let current = load_or_extract(&p, &flag, &mut |_| {}).unwrap();
    let root = current.root.parent().unwrap();
    for (digit, version) in [('b', 1), ('c', 8)] {
        let fp = digit.to_string().repeat(64);
        let path = root.join(&fp);
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("manifest.json"),
            format!(r#"{{"version":{version},"fingerprint":"{fp}","sheets":[],"files":{{}}}}"#),
        )
        .unwrap();
        std::fs::write(path.join("catalog.json"), b"old schema").unwrap();
    }
    let unknown = root.join("d".repeat(64));
    std::fs::create_dir(&unknown).unwrap();
    std::fs::write(unknown.join("manifest.json"), b"not a generation").unwrap();
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &flag).unwrap(),
        2
    );
    assert!(unknown.exists());
    assert!(current.root.exists());
}

#[test]
fn toast_uses_the_existing_delivery_without_reopening_manifest_metadata() {
    let (_dir, paths) = setup();
    let mut cache = load_or_extract(&paths, &AtomicBool::new(false), &mut |_| {}).unwrap();
    let mut catalog = maxed_types::demo::demo_catalog();
    catalog
        .client_fingerprint
        .clone_from(&cache.catalog.client_fingerprint);
    catalog.sprites = cache.catalog.sprites.clone();
    catalog.dungeons[0].portal_type = Some(catalog.objects[0].object_type);
    catalog.objects[0].sprite_key = Some(catalog.sprites[0].key.clone());
    let dungeon = catalog.dungeons[0].name.clone();
    cache.catalog = std::sync::Arc::new(catalog);
    std::fs::write(
        cache.root.join("manifest.json"),
        b"unavailable after publication",
    )
    .unwrap();
    let icon = maxed_assets::cached_toast_portal_icon(&cache, &dungeon).unwrap();
    assert!(std::fs::read(icon)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn concurrent_publishers_reuse_one_complete_generation_without_holding_validation_lock() {
    let (_dir, paths) = setup();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    load_or_extract(&paths, &AtomicBool::new(false), &mut |phase| {
                        if phase == AssetProgress::Publishing {
                            barrier.wait();
                        }
                    })
                    .unwrap()
                })
            })
            .collect();
        let caches: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(caches[0].root, caches[1].root);
        assert!(std::sync::Arc::ptr_eq(
            &caches[0].catalog,
            &caches[1].catalog
        ));
        let root = caches[0].root.parent().unwrap();
        assert!(std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().starts_with(".stage-")));
    });
}

#[test]
#[cfg(windows)]
fn held_toast_icon_keeps_retirement_retryable() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_dir, p) = setup();
    let cancel = AtomicBool::new(false);
    let current = load_or_extract(&p, &cancel, &mut |_| {}).unwrap();
    let root = current.root.parent().unwrap();
    let fp = "b".repeat(64);
    let old = root.join(&fp);
    std::fs::create_dir(&old).unwrap();
    std::fs::write(
        old.join("manifest.json"),
        format!(r#"{{"version":8,"fingerprint":"{fp}","sheets":[],"files":{{}}}}"#),
    )
    .unwrap();
    let icons = root.join("toast-icons").join(&fp);
    std::fs::create_dir_all(&icons).unwrap();
    let icon = icons.join("42.png");
    std::fs::write(&icon, b"synthetic").unwrap();
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&icon)
        .unwrap();
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &cancel).unwrap(),
        0
    );
    assert!(!old.exists());
    assert!(std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .any(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with(".retired-")));
    drop(handle);
    assert_eq!(
        maxed_assets::prune_unused_generations(&p.app_data, &cancel).unwrap(),
        1
    );
    assert!(!icons.exists());
    assert!(!std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .any(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with(".retired-")));
}

#[test]
fn character_cache_rejects_v11_and_failed_rebuild_preserves_prior_publication() {
    let (_dir, paths) = setup();
    let cancel = AtomicBool::new(false);
    let cached = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
    let pointer = paths.app_data.join("assets/v1/current");
    let previous_pointer = std::fs::read(&pointer).unwrap();
    let manifest_path = cached.root.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["version"], 14);
    manifest["version"] = 11.into();
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(matches!(
        load_or_extract(&paths, &cancel, &mut |_| {}),
        Err(AssetError::CacheInvalid)
    ));
    // A changed source needs a rebuild. Its failure must not publish an empty/neutral catalog.
    std::fs::write(&paths.resources, b"synthetic invalid Unity source").unwrap();
    assert!(load_or_extract(&paths, &cancel, &mut |_| {}).is_err());
    assert_eq!(std::fs::read(&pointer).unwrap(), previous_pointer);
    assert!(cached.root.join("catalog.json").is_file());
}

#[test]
#[cfg(windows)]
fn unused_v10_and_v11_generations_and_icons_are_pruned_but_never_loaded() {
    for version in [10, 11] {
        let (_dir, paths) = setup();
        let cancel = AtomicBool::new(false);
        let old = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
        let generation = old.root.clone();
        let fingerprint = old.catalog.client_fingerprint.clone();
        let icons = generation
            .parent()
            .unwrap()
            .join("toast-icons")
            .join(&fingerprint);
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("42.png"), b"synthetic icon").unwrap();
        let manifest_path = generation.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["version"] = version.into();
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        // A reader-held old generation must be refused, not loaded or repaired.
        assert!(matches!(
            load_or_extract(&paths, &cancel, &mut |_| {}),
            Err(AssetError::CacheInvalid)
        ));
        let mut bytes = common::fixture();
        let at = bytes.windows(10).position(|b| b == b"<Objects/>").unwrap();
        bytes[at..at + 10].copy_from_slice(b"<Grounds/>");
        std::fs::write(&paths.resources, bytes).unwrap();
        let current = load_or_extract(&paths, &cancel, &mut |_| {}).unwrap();
        assert_ne!(current.root, generation);
        assert_eq!(
            maxed_assets::prune_unused_generations(&paths.app_data, &cancel).unwrap(),
            0
        );
        assert!(generation.join("catalog.json").is_file());
        assert!(generation.join("characters.png").is_file());
        assert!(generation.join("tiles.pack").is_file());
        assert!(icons.is_dir());
        drop(old);
        assert_eq!(
            maxed_assets::prune_unused_generations(&paths.app_data, &cancel).unwrap(),
            1
        );
        assert!(!generation.exists());
        assert!(!icons.exists());
        assert!(current.root.join("catalog.json").is_file());
    }
}
