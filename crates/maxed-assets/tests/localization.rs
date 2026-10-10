use maxed_assets::{build_catalog, read_resources, read_text_resources, AssetError};
use std::sync::atomic::AtomicBool;

fn word(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn align(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}
fn string(out: &mut Vec<u8>, value: &str) {
    word(out, value.len() as u32);
    out.extend(value.as_bytes());
    align(out);
}
fn source() -> Vec<u8> {
    let mut out = vec![0; 28];
    string(&mut out, "I2Languages");
    out.extend([0; 12]);
    word(&mut out, 3);
    for (key, value) in [
        ("test.Fake_Boss", "Fake Boss"),
        ("test.Empty", ""),
        ("test.Event", "Grand Sphinx"),
    ] {
        string(&mut out, key);
        word(&mut out, 0);
        word(&mut out, 2);
        string(&mut out, "Foreign name");
        string(&mut out, value);
        word(&mut out, 2);
        out.extend([0; 4]);
        word(&mut out, 0);
    }
    out.extend([0; 12]);
    word(&mut out, 2);
    for code in ["fr", "en"] {
        string(&mut out, code);
        string(&mut out, code);
        word(&mut out, 0);
    }
    out
}
fn fixture(xml: &str, source: Option<&[u8]>) -> Vec<u8> {
    let mut text = Vec::new();
    string(&mut text, "synthetic");
    word(&mut text, xml.len() as u32);
    text.extend(xml.as_bytes());
    let mut metadata = b"6000.0.58f2\0".to_vec();
    word(&mut metadata, 19);
    metadata.push(0);
    word(&mut metadata, 2);
    for class in [49, 114] {
        word(&mut metadata, class);
        metadata.extend([0, 255, 255]);
        if class == 114 {
            metadata.extend([0; 16]);
        }
        metadata.extend([0; 16]);
    }
    word(&mut metadata, if source.is_some() { 2 } else { 1 });
    let mut objects = vec![text.as_slice()];
    objects.extend(source);
    let mut offset = 0u64;
    for (index, bytes) in objects.iter().enumerate() {
        align(&mut metadata);
        metadata.extend((index as u64 + 1).to_le_bytes());
        metadata.extend(offset.to_le_bytes());
        word(&mut metadata, bytes.len() as u32);
        word(&mut metadata, index as u32);
        offset += bytes.len() as u64;
    }
    metadata.extend([0; 13]);
    let start = 48 + metadata.len();
    let mut out = vec![0; 48];
    out[8..12].copy_from_slice(&22u32.to_be_bytes());
    out[20..24].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
    out[24..32].copy_from_slice(&(start as u64 + offset).to_be_bytes());
    out[32..40].copy_from_slice(&(start as u64).to_be_bytes());
    out.extend(metadata);
    for bytes in objects {
        out.extend(bytes);
    }
    out
}

#[test]
fn localized_names_and_fallbacks_reach_all_catalog_consumers() {
    let xml = r#"<Objects>
      <Object type="1" id="Fallback boss"><DisplayId>{test.Fake_Boss}</DisplayId><Class>Equipment</Class></Object>
      <Object type="2" id="Missing"><DisplayId>{test.Missing}</DisplayId></Object>
      <Object type="3" id="Empty"><DisplayId>{test.Empty}</DisplayId></Object>
      <Object type="4" id="Unused"><DisplayId>Plain Name</DisplayId></Object>
      <Object type="5" id="Fallback portal"><DisplayId>{test.Fake_Boss}</DisplayId><Class>Portal</Class><DungeonName>Synthetic Dungeon</DungeonName><DungeonPortal/></Object>
      <Object type="6" id="Fallback event"><DisplayId>{test.Event}</DisplayId></Object>
      <Enchantment type="1" id="Fallback enchant"><DisplayId>{test.Fake_Boss}</DisplayId></Enchantment>
    </Objects>"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let valid = source();
    let malformed = &valid[..valid.len() - 1];
    for source in [Some(valid.as_slice()), None, Some(malformed)] {
        std::fs::write(&path, fixture(xml, source)).unwrap();
        for read in [read_resources, read_text_resources] {
            let assets = read(&path, &AtomicBool::new(false)).unwrap();
            let c = build_catalog(assets, "a".repeat(64)).unwrap();
            let resolved = source == Some(valid.as_slice());
            assert_eq!(
                c.objects[0].name,
                if resolved {
                    "Fake Boss"
                } else {
                    "Fallback boss"
                }
            );
            assert_eq!(c.objects[1].name, "Missing");
            assert_eq!(c.objects[2].name, "Empty");
            assert_eq!(c.objects[3].name, "Plain Name");
            assert_eq!(c.items[0].name, c.objects[0].name);
            assert_eq!(
                c.enchants[0].name,
                if resolved {
                    "Fake Boss"
                } else {
                    "Fallback enchant"
                }
            );
            assert_eq!(c.objects[5].event_boss, Some(resolved));
            let dungeon = c
                .dungeons
                .iter()
                .find(|d| d.name == "Synthetic Dungeon")
                .unwrap();
            assert!(dungeon.aliases.iter().any(|s| s
                == if resolved {
                    "Fake Boss"
                } else {
                    "Fallback portal"
                }));
        }
    }
    std::fs::write(
        &path,
        fixture(
            r#"<Objects><Object type="1"><DisplayId>{test.Missing}</DisplayId></Object></Objects>"#,
            None,
        ),
    )
    .unwrap();
    let assets = read_text_resources(&path, &AtomicBool::new(false)).unwrap();
    assert!(matches!(
        build_catalog(assets, "a".repeat(64)),
        Err(AssetError::InvalidCatalog)
    ));
}

#[test]
fn changed_localization_layout_and_unrelated_behaviours_use_fallbacks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let mut source = source();
    // Term count follows the MonoBehaviour prefix, name and source settings.
    source[56..60].copy_from_slice(&65537u32.to_le_bytes());
    let mut unrelated = vec![0; 28];
    word(&mut unrelated, 4097);
    let xml = r#"<Objects><Object type="1" id="Fallback boss"><DisplayId>{test.Fake_Boss}</DisplayId></Object></Objects>"#;
    for bytes in [&source, &unrelated] {
        std::fs::write(&path, fixture(xml, Some(bytes))).unwrap();
        for read in [read_resources, read_text_resources] {
            let assets = read(&path, &AtomicBool::new(false)).unwrap();
            assert!(assets.english.is_empty());
            let catalog = build_catalog(assets, "a".repeat(64)).unwrap();
            assert_eq!(catalog.objects[0].name, "Fallback boss");
        }
    }
}

#[test]
#[ignore = "requires MAXED_CLIENT_ASSETS"]
fn installed_client_localized_names() {
    let client = std::path::PathBuf::from(
        std::env::var_os("MAXED_CLIENT_ASSETS").expect("MAXED_CLIENT_ASSETS"),
    );
    let assets = read_text_resources(&client, &AtomicBool::new(false)).unwrap();
    let sheets = maxed_assets::open_sheets(&client, &AtomicBool::new(false)).unwrap();
    let infos = sheets.iter().map(|s| s.info().clone()).collect::<Vec<_>>();
    let c = maxed_assets::build_catalog_with_sheets(assets, "a".repeat(64), &infos).unwrap();
    assert!(c.objects.iter().all(|o| !o.name.starts_with('{')));
    assert!(c.items.iter().all(|o| !o.name.starts_with('{')));
    assert!(c.enchants.iter().all(|o| !o.name.starts_with('{')));
    assert_eq!(
        c.objects
            .iter()
            .find(|o| o.object_type == 24108)
            .unwrap()
            .name,
        "Golden Oryx Effigy"
    );
}
