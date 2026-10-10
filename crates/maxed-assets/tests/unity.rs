mod common;
use common::fixture;
use maxed_assets::{checked_span, read_resources, AssetError};
use std::sync::atomic::AtomicBool;

#[test]
fn spans_reject_truncation_and_overflow() {
    assert_eq!(checked_span(2, 3, 5).unwrap(), 2..5);
    assert!(matches!(
        checked_span(2, 4, 5),
        Err(AssetError::InvalidStructure)
    ));
    assert!(matches!(
        checked_span(u64::MAX, 1, u64::MAX),
        Err(AssetError::InvalidStructure)
    ));
}

#[test]
fn cancellation_precedes_file_access() {
    assert!(matches!(
        read_resources(
            std::path::Path::new("missing/resources.assets"),
            &AtomicBool::new(true)
        ),
        Err(AssetError::Cancelled)
    ));
}

#[test]
fn truncated_headers_are_rejected() {
    for len in 0..48 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.assets");
        std::fs::write(&path, vec![0; len]).unwrap();
        assert!(read_resources(&path, &AtomicBool::new(false)).is_err());
    }
}

#[test]
#[ignore = "requires owner-approved installed client via MAXED_CLIENT_ASSETS"]
fn installed_client_spike() {
    let path = std::env::var_os("MAXED_CLIENT_ASSETS").expect("set MAXED_CLIENT_ASSETS");
    let info =
        maxed_assets::inspect_resources(std::path::Path::new(&path), &AtomicBool::new(false))
            .unwrap();
    eprintln!(
        "serialized_version={}, unity_version={}, type_tree={}, objects={}, text_assets={}",
        info.serialized_version, info.unity_version, info.type_tree, info.objects, info.text_assets
    );
    for text in &info.texts {
        if ["equip", "players", "enchantments", "spritesheetf"].contains(&text.name.as_str()) {
            eprintln!("{text:?}");
        }
    }
    for sheet in &info.sheets {
        eprintln!("{sheet:?}");
    }
    let assets =
        maxed_assets::read_text_resources(std::path::Path::new(&path), &AtomicBool::new(false))
            .unwrap();
    let sheets =
        maxed_assets::open_sheets(std::path::Path::new(&path), &AtomicBool::new(false)).unwrap();
    for required in ["equip", "players", "enchantments"] {
        assert!(assets.texts.iter().any(|text| text.name == required));
    }
    assert!(!assets.sprite_mapping.is_empty());
    assert_eq!(sheets.len(), 4);
    let mut pips =
        maxed_assets::locate_enchant_pips(std::path::Path::new(&path), &AtomicBool::new(false))
            .unwrap()
            .expect("RarityIcon_1..4 resolve through the GUI atlas");
    let rgba = pips.read_rgba(&AtomicBool::new(false)).unwrap();
    for pip in 0..4 {
        assert!(
            (0..16).any(|y| (0..16).any(|x| rgba[(y * 64 + pip * 16 + x) * 4 + 3] != 0)),
            "pip {} is transparent",
            pip + 1
        );
    }
    use sha2::Digest;
    eprintln!("enchant pips rgba sha256={:x}", sha2::Sha256::digest(&rgba));
    eprintln!(
        "texts={}, sheets={}, sprite_mapping_bytes={}",
        assets.texts.len(),
        sheets.len(),
        assets.sprite_mapping.len()
    );
}

#[test]
fn synthetic_text_and_four_texture_corners() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    std::fs::write(&path, fixture()).unwrap();
    let assets = read_resources(&path, &AtomicBool::new(false)).unwrap();
    assert_eq!(assets.texts[0].name, "equip");
    assert_eq!(assets.texts[0].bytes, b"<Objects/>");
    assert_eq!(
        assets.textures[0].rgba,
        [0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255]
    );
}

#[test]
fn streamed_rows_preserve_rgba_and_cancel_without_reading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    std::fs::write(&path, common::fixture_with_stream(Some("pixels.resS"))).unwrap();
    let pixels = [
        11, 22, 33, 0, 44, 55, 66, 127, 77, 88, 99, 255, 10, 20, 30, 128,
    ];
    std::fs::write(dir.path().join("pixels.resS"), pixels).unwrap();
    let mut sheets = maxed_assets::open_sheets(&path, &AtomicBool::new(false)).unwrap();
    let mut row = [0; 8];
    sheets[0]
        .read_row(0, &mut row, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(row, pixels[8..]);
    sheets[0]
        .read_row(1, &mut row, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(row, pixels[..8]);
    assert!(matches!(
        sheets[0].read_row(0, &mut row, &AtomicBool::new(true)),
        Err(AssetError::Cancelled)
    ));
}

#[test]
fn every_truncated_synthetic_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let bytes = fixture();
    for end in 0..bytes.len() {
        std::fs::write(&path, &bytes[..end]).unwrap();
        assert!(
            read_resources(&path, &AtomicBool::new(false)).is_err(),
            "length {end}"
        );
    }
}

#[test]
fn rejects_unknown_version_and_oversized_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let mut bytes = fixture();
    bytes[8..12].copy_from_slice(&23u32.to_be_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        read_resources(&path, &AtomicBool::new(false)),
        Err(AssetError::UnsupportedVersion)
    ));
    let mut bytes = fixture();
    bytes[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert!(read_resources(&path, &AtomicBool::new(false)).is_err());
}

#[test]
fn versioned_metadata_endian_and_type_tree_blobs() {
    for version in 17u32..=22 {
        for big in [false, true] {
            for tree in [false, true] {
                let put = |b: &mut Vec<u8>, n: u32| {
                    b.extend(if big {
                        n.to_be_bytes()
                    } else {
                        n.to_le_bytes()
                    })
                };
                let mut text = vec![];
                put(&mut text, 1);
                text.extend([b'x', 0, 0, 0]);
                put(&mut text, 2);
                text.extend(b"ok");
                let mut meta = b"6000.0.58f2\0".to_vec();
                put(&mut meta, 19);
                meta.push(u8::from(tree));
                put(&mut meta, 1);
                put(&mut meta, 49);
                meta.extend([0, 255, 255]);
                meta.extend([0; 16]);
                if tree {
                    put(&mut meta, 1);
                    put(&mut meta, 1);
                    meta.extend(vec![0; if version >= 19 { 32 } else { 24 }]);
                    meta.push(0);
                    if version >= 21 {
                        put(&mut meta, 0);
                    }
                }
                put(&mut meta, 1);
                while !meta.len().is_multiple_of(4) {
                    meta.push(0);
                }
                meta.extend([0; 8]);
                meta.extend(vec![0; if version == 22 { 8 } else { 4 }]);
                put(&mut meta, text.len() as u32);
                put(&mut meta, 0);
                put(&mut meta, 0);
                put(&mut meta, 0);
                if version >= 20 {
                    // A ref type MonoBehaviour still has ScriptID when its
                    // script index is -1. Nonzero hashes expose misalignment.
                    put(&mut meta, 1);
                    put(&mut meta, 114);
                    meta.extend([0, 255, 255]);
                    meta.extend([0x53; 16]);
                    meta.extend([0x48; 16]);
                    if tree {
                        put(&mut meta, 0);
                        put(&mut meta, 0);
                        if version >= 21 {
                            meta.extend(b"RefClass\0Namespace\0Assembly\0");
                        }
                    }
                }
                meta.push(0);
                let header = if version == 22 { 48 } else { 20 };
                let offset = header + meta.len();
                let size = offset + text.len();
                let mut bytes = vec![0; header];
                bytes[8..12].copy_from_slice(&version.to_be_bytes());
                bytes[16] = u8::from(big);
                if version == 22 {
                    bytes[20..24].copy_from_slice(&(meta.len() as u32).to_be_bytes());
                    bytes[24..32].copy_from_slice(&(size as u64).to_be_bytes());
                    bytes[32..40].copy_from_slice(&(offset as u64).to_be_bytes());
                } else {
                    bytes[..4].copy_from_slice(&(meta.len() as u32).to_be_bytes());
                    bytes[4..8].copy_from_slice(&(size as u32).to_be_bytes());
                    bytes[12..16].copy_from_slice(&(offset as u32).to_be_bytes());
                }
                bytes.extend(meta);
                bytes.extend(text);
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("resources.assets");
                std::fs::write(&path, bytes).unwrap();
                let result = read_resources(&path, &AtomicBool::new(false)).unwrap();
                assert_eq!(result.texts[0].name, "x");
                assert_eq!(result.texts[0].bytes, b"ok");
            }
        }
    }
}

#[test]
fn object_counts_and_type_indices_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let original = fixture();
    let type_count = 48 + b"6000.0.58f2\0".len() + 5;
    let object_count = type_count + 4 + 2 * 23;
    let first_object = (object_count + 4 + 3) & !3;
    for (at, value) in [
        (type_count, 1_000_001u32),
        (object_count, 1_000_001),
        (first_object + 20, 99),
    ] {
        let mut bytes = original.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(read_resources(&path, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn rejects_oversized_text_and_invalid_texture_dimensions_or_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let original = fixture();
    let offset = u64::from_be_bytes(original[32..40].try_into().unwrap()) as usize;
    let texture = offset + 26;
    for (at, value, limit) in [
        (offset + 12, 64u32 * 1024 * 1024 + 1, true),
        (texture + 20, 16385, true),
        (texture + 36, 999, false),
    ] {
        let mut bytes = original.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        let result = read_resources(&path, &AtomicBool::new(false));
        if limit {
            assert!(matches!(result, Err(AssetError::LimitExceeded)));
        } else {
            assert!(matches!(result, Err(AssetError::UnsupportedTexture)));
        }
    }
}
