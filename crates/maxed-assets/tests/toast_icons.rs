use maxed_assets::toast_portal_icon;
use maxed_types::{Catalog, SpriteRef};
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor};

fn prepared() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = maxed_types::demo::demo_catalog();
    catalog.client_fingerprint = "a".repeat(64);
    catalog.dungeons[0].name = "../../synthetic dungeon".into();
    catalog.dungeons[0].aliases = vec!["Synthetic alias".into()];
    catalog.dungeons[0].portal_type = Some(4242);
    catalog.objects[0].object_type = 4242;
    catalog.objects[0].sprite_key = Some("synthetic:0".into());
    catalog.sprites = vec![SpriteRef {
        key: "synthetic:0".into(),
        sheet: "mapObjects.png".into(),
        x: 1,
        y: 1,
        width: 2,
        height: 2,
        trim: None,
    }];
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let pixels = [
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 0, 64,
        ];
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    let root = dir.path().join(&catalog.client_fingerprint);
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "fingerprint": catalog.client_fingerprint,
            "files": {"tiles.pack": {"bytes": bytes.len() + 140}}
        }))
        .unwrap(),
    )
    .unwrap();
    let mut pack = b"MXTILE01".to_vec();
    pack.extend(catalog.client_fingerprint.as_bytes());
    pack.extend(1u32.to_le_bytes());
    for word in [3u32, 1, 1, 2, 2] {
        pack.extend(word.to_le_bytes());
    }
    pack.extend(140u64.to_le_bytes());
    pack.extend((bytes.len() as u32).to_le_bytes());
    pack.extend(Sha256::digest(&bytes));
    pack.extend(bytes);
    fs::write(root.join("tiles.pack"), pack).unwrap();
    (dir, catalog)
}

#[test]
fn crop_and_nearest_neighbour_preserve_every_pixel_and_alpha() {
    let (dir, catalog) = prepared();
    let path = toast_portal_icon(&catalog, dir.path(), "Synthetic alias").unwrap();
    let mut reader = png::Decoder::new(Cursor::new(fs::read(path).unwrap()))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(
        (info.width, info.height, info.color_type),
        (96, 96, png::ColorType::Rgba)
    );
    for y in 0..96 {
        for x in 0..96 {
            let expected = match (x < 48, y < 48) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 128],
                (true, false) => [0, 0, 255, 0],
                (false, false) => [255, 255, 0, 64],
            };
            assert_eq!(&pixels[(y * 96 + x) * 4..(y * 96 + x + 1) * 4], expected);
        }
    }
}

#[test]
fn cache_uses_only_fingerprint_and_numeric_type_and_reuses_file() {
    let (dir, catalog) = prepared();
    let path = toast_portal_icon(&catalog, dir.path(), "../../synthetic dungeon").unwrap();
    assert_eq!(
        path,
        dir.path()
            .join("toast-icons")
            .join("a".repeat(64))
            .join("4242.png")
    );
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    // A cache hit must not read or decode the source again.
    fs::remove_file(dir.path().join("a".repeat(64)).join("tiles.pack")).unwrap();
    assert_eq!(
        toast_portal_icon(&catalog, dir.path(), "Synthetic alias"),
        Some(path.clone())
    );
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
}

#[test]
fn missing_or_invalid_assets_are_optional() {
    for case in 0..12 {
        let (dir, mut catalog) = prepared();
        let root = dir.path().join(&catalog.client_fingerprint);
        match case {
            0 => catalog.dungeons.clear(),
            1 => catalog.dungeons[0].portal_type = None,
            2 => catalog.sprites[0].x = 4,
            3 => catalog.sprites[0].width = 0,
            4 => catalog.sprites[0].x = u32::MAX,
            5 => {
                fs::write(root.join("tiles.pack"), b"invalid").unwrap();
            }
            6 => {
                fs::write(root.join("manifest.json"), b"{}").unwrap();
            }
            7 => {
                fs::write(dir.path().join("toast-icons"), b"not a directory").unwrap();
            }
            8 => catalog.client_fingerprint = "../unsafe".into(),
            9 => catalog.objects.clear(),
            10 => catalog.sprites.clear(),
            11 => {
                // Keep signature, IHDR and manifest length valid; fail PNG decoding.
                let path = root.join("tiles.pack");
                let mut bytes = fs::read(&path).unwrap();
                let idat = bytes.windows(4).position(|b| b == b"IDAT").unwrap();
                bytes[idat + 4] ^= 0xff;
                fs::write(path, bytes).unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            toast_portal_icon(&catalog, dir.path(), "Synthetic alias"),
            None,
            "case {case}"
        );
    }
}

#[test]
fn corrupt_cached_icons_are_rebuilt_as_complete_96_pixel_pngs() {
    for case in 0..5 {
        let (dir, catalog) = prepared();
        let path = toast_portal_icon(&catalog, dir.path(), "Synthetic alias").unwrap();
        let valid = fs::read(&path).unwrap();
        let mut damaged = valid.clone();
        match case {
            0 => damaged.truncate(16),
            1 => damaged.truncate(damaged.len() - 12), // Missing IEND, intact IHDR.
            2 => damaged[19] = 1,                      // Wrong IHDR width.
            3 => {
                let idat = damaged.windows(4).position(|b| b == b"IDAT").unwrap();
                damaged[idat + 4] ^= 0xff;
            }
            4 => damaged.resize(128 * 1024, 0),
            _ => unreachable!(),
        }
        fs::write(&path, damaged).unwrap();
        assert_eq!(
            toast_portal_icon(&catalog, dir.path(), "Synthetic alias"),
            Some(path.clone())
        );
        let rebuilt = fs::read(&path).unwrap();
        assert_eq!(rebuilt, valid, "case {case} was not rebuilt");
        let mut reader = png::Decoder::new(Cursor::new(rebuilt)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (96, 96));
        reader.finish().unwrap();
    }
}

#[test]
fn toast_paths_strip_verbatim_drive_prefixes_and_reject_unc() {
    use std::path::Path;
    for (input, expected) in [
        (r"\\?\C:\cache\42.png", Some(r"C:\cache\42.png")),
        (r"C:\cache\42.png", Some(r"C:\cache\42.png")),
        ("/cache/42.png", Some("/cache/42.png")),
        (r"\\?\UNC\server\share\42.png", None),
        (r"\\server\share\42.png", None),
        ("//server/share/42.png", None),
    ] {
        assert_eq!(
            maxed_assets::local_toast_icon_path(Path::new(input)),
            expected.map(Path::new)
        );
    }
}

#[cfg(unix)]
#[test]
fn linked_icon_cache_paths_never_read_or_write_the_target() {
    use std::os::unix::fs::symlink;
    for level in 0..3 {
        for cached in [false, true] {
            let (dir, catalog) = prepared();
            let outside = tempfile::tempdir().unwrap();
            let icons = dir.path().join("toast-icons");
            let generation = icons.join(&catalog.client_fingerprint);
            let icon = generation.join("4242.png");
            let outside_icon = if level == 0 {
                outside
                    .path()
                    .join(&catalog.client_fingerprint)
                    .join("4242.png")
            } else {
                outside.path().join("4242.png")
            };
            if cached {
                let (clean, clean_catalog) = prepared();
                let valid =
                    toast_portal_icon(&clean_catalog, clean.path(), "Synthetic alias").unwrap();
                fs::create_dir_all(outside_icon.parent().unwrap()).unwrap();
                fs::copy(valid, &outside_icon).unwrap();
            }
            let before = cached.then(|| fs::read(&outside_icon).unwrap());
            match level {
                0 => symlink(outside.path(), &icons).unwrap(),
                1 => {
                    fs::create_dir(&icons).unwrap();
                    symlink(outside.path(), &generation).unwrap();
                }
                2 => {
                    fs::create_dir_all(&generation).unwrap();
                    symlink(&outside_icon, &icon).unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(
                toast_portal_icon(&catalog, dir.path(), "Synthetic alias"),
                None,
                "level {level}, cached {cached}"
            );
            if let Some(before) = before {
                assert_eq!(fs::read(&outside_icon).unwrap(), before);
                assert_eq!(
                    fs::read_dir(outside_icon.parent().unwrap())
                        .unwrap()
                        .count(),
                    1
                );
            } else {
                assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
            }
            let substituted = match level {
                0 => &icons,
                1 => &generation,
                _ => &icon,
            };
            assert!(fs::symlink_metadata(substituted)
                .unwrap()
                .file_type()
                .is_symlink());
            fs::remove_file(substituted).unwrap();
            let normal = toast_portal_icon(&catalog, dir.path(), "Synthetic alias").unwrap();
            assert_eq!(normal, icon);
            assert!(normal.is_file());
        }
    }
}

#[cfg(windows)]
#[test]
fn junction_icon_directories_never_write_the_target() {
    for fingerprint_link in [false, true] {
        let (dir, catalog) = prepared();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"untouched").unwrap();
        let icons = dir.path().join("toast-icons");
        let junction = if fingerprint_link {
            fs::create_dir(&icons).unwrap();
            icons.join(&catalog.client_fingerprint)
        } else {
            icons
        };
        let output = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "synthetic junction creation failed"
        );
        assert_eq!(
            toast_portal_icon(&catalog, dir.path(), "Synthetic alias"),
            None
        );
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"untouched");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
        fs::remove_dir(junction).unwrap();
        assert!(toast_portal_icon(&catalog, dir.path(), "Synthetic alias")
            .unwrap()
            .is_file());
    }
}
