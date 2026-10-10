//! Synthetic GUI atlas: the pips are found by sprite name through atlas render
//! data, never by texture name or fixed rectangles. No game art is used.
use maxed_assets::{load_or_extract, locate_enchant_pips, AssetPaths};
use std::sync::atomic::AtomicBool;

const TEXTURE: i32 = 28;
const TEXT: i32 = 49;
const SPRITE: i32 = 213;
const ATLAS: i32 = 687_078_895;
const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;

fn u32le(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn align(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}
fn string(out: &mut Vec<u8>, s: &str) {
    u32le(out, s.len() as u32);
    out.extend(s.as_bytes());
    align(out);
}
fn pptr(out: &mut Vec<u8>, id: i64) {
    u32le(out, 0);
    out.extend(id.to_le_bytes());
}
fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend(v.to_le_bytes());
    }
}
fn guid(n: u8) -> [u8; 16] {
    [n; 16]
}

/// Atlas texture pixel at (x, bottom-up storage row).
fn texel(x: u32, row: u32) -> [u8; 4] {
    // Pip 1 has padding; pip 2 is fully transparent. Other atlas pixels are opaque.
    let alpha = if x < 16 && row < 16 {
        if (2..6).contains(&x) && (6..12).contains(&row) {
            255
        } else {
            0
        }
    } else if (16..32).contains(&x) && (3..19).contains(&row) {
        0
    } else {
        255
    };
    [x as u8, row as u8, (x ^ row) as u8, alpha]
}

#[derive(Clone)]
struct Pip {
    name: String,
    listed: bool,
    rect: [f32; 4],
    settings: u32,
    atlas: i64,
}

struct Spec {
    pips: Vec<Pip>,
    duplicate_atlas: bool,
    companion: Option<&'static str>,
}
impl Default for Spec {
    fn default() -> Self {
        let at = [(0.0, 0.0), (16.0, 3.0), (40.0, 16.0), (48.0, 0.0)];
        Self {
            pips: (1..=4)
                .map(|n| Pip {
                    name: format!("RarityIcon_{n}"),
                    listed: true,
                    rect: [at[n - 1].0, at[n - 1].1, 16.0, 16.0],
                    settings: 3,
                    atlas: 0,
                })
                .collect(),
            duplicate_atlas: false,
            companion: None,
        }
    }
}

fn texture(companion: Option<&str>) -> (Vec<u8>, Vec<u8>) {
    let mut pixels = Vec::new();
    for row in 0..HEIGHT {
        for x in 0..WIDTH {
            pixels.extend(texel(x, row));
        }
    }
    let mut t = Vec::new();
    // A hashed, patch-specific name the extractor must not depend on.
    string(&mut t, "sactx-0-64x32-Uncompressed-GUI Atlas-0badc0de");
    t.extend([0; 4]);
    let size = pixels.len() as u32;
    for n in [WIDTH, HEIGHT, size, 0, 4, 1, 0, 0, 0, 0, 1, 2] {
        u32le(&mut t, n);
    }
    t.extend([0; 32]);
    u32le(&mut t, 0); // platform blob
    if let Some(path) = companion {
        u32le(&mut t, 0);
        t.extend(0u64.to_le_bytes());
        u32le(&mut t, size);
        string(&mut t, path);
        (t, pixels)
    } else {
        u32le(&mut t, size);
        t.extend(&pixels);
        t.extend([0; 16]);
        (t, vec![])
    }
}

fn sprite(name: &str, n: u8, atlas: i64) -> Vec<u8> {
    let mut s = Vec::new();
    string(&mut s, name);
    floats(
        &mut s,
        &[0.0, 0.0, 16.0, 16.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );
    floats(&mut s, &[100.0, 0.5, 0.5]);
    u32le(&mut s, 1); // extrude
    s.push(0); // polygon
    align(&mut s);
    s.extend(guid(n));
    s.extend(21_300_000i64.to_le_bytes());
    u32le(&mut s, 1);
    string(&mut s, "GUI Atlas");
    pptr(&mut s, atlas);
    s.extend([0; 40]); // render data and the rest, not read
    s
}

fn atlas(entries: &[(&str, i64, u8, [f32; 4], u32)], texture: i64) -> Vec<u8> {
    let mut a = Vec::new();
    string(&mut a, "GUI Atlas");
    u32le(&mut a, entries.len() as u32);
    for (_, id, ..) in entries {
        pptr(&mut a, *id);
    }
    u32le(&mut a, entries.len() as u32);
    for (name, ..) in entries {
        string(&mut a, name);
    }
    u32le(&mut a, entries.len() as u32);
    for (_, _, n, rect, settings) in entries {
        a.extend(guid(*n));
        a.extend(21_300_000i64.to_le_bytes());
        pptr(&mut a, texture);
        pptr(&mut a, 0);
        floats(&mut a, rect);
        floats(&mut a, &[0.0; 2]);
        floats(&mut a, &rect[..2]);
        floats(&mut a, &[100.0, rect[0], 100.0, rect[1]]);
        floats(&mut a, &[1.0]);
        u32le(&mut a, *settings);
        u32le(&mut a, 0); // secondary textures
    }
    string(&mut a, "GUI Atlas");
    a.push(0);
    align(&mut a);
    a
}

/// Serialized file (v22, little-endian) with path ids 1..=n in order.
fn serialized(objects: &[(i32, Vec<u8>)]) -> Vec<u8> {
    let mut classes: Vec<i32> = Vec::new();
    for (class, _) in objects {
        if !classes.contains(class) {
            classes.push(*class);
        }
    }
    let mut metadata = b"6000.0.58f2\0".to_vec();
    u32le(&mut metadata, 19);
    metadata.push(0);
    u32le(&mut metadata, classes.len() as u32);
    for class in &classes {
        metadata.extend(class.to_le_bytes());
        metadata.extend([0, 255, 255]);
        metadata.extend([0; 16]);
    }
    u32le(&mut metadata, objects.len() as u32);
    let mut offset = 0u64;
    for (i, (class, bytes)) in objects.iter().enumerate() {
        align(&mut metadata);
        metadata.extend((i as u64 + 1).to_le_bytes());
        metadata.extend(offset.to_le_bytes());
        u32le(&mut metadata, bytes.len() as u32);
        u32le(
            &mut metadata,
            classes.iter().position(|c| c == class).unwrap() as u32,
        );
        offset += bytes.len() as u64;
    }
    metadata.extend([0; 13]);
    let data = 48 + metadata.len();
    let len = data + offset as usize;
    let mut out = vec![0; 48];
    out[8..12].copy_from_slice(&22u32.to_be_bytes());
    out[20..24].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
    out[24..32].copy_from_slice(&(len as u64).to_be_bytes());
    out[32..40].copy_from_slice(&(data as u64).to_be_bytes());
    out.extend(metadata);
    for (_, bytes) in objects {
        out.extend(bytes);
    }
    out
}

/// Path ids: 1 text, 2 texture, 3..=6 pip sprites, 7 decoy sprite, 8 atlas,
/// 9 optional second atlas.
fn build(spec: &Spec) -> (Vec<u8>, Vec<u8>) {
    const ATLAS_ID: i64 = 8;
    let mut text = Vec::new();
    string(&mut text, "equip");
    u32le(&mut text, 10);
    text.extend(b"<Objects/>");
    let (texture, stream) = texture(spec.companion);
    let mut objects = vec![(TEXT, text), (TEXTURE, texture)];
    for (i, pip) in spec.pips.iter().enumerate() {
        let atlas = if pip.atlas == 0 { ATLAS_ID } else { pip.atlas };
        objects.push((SPRITE, sprite(&pip.name, i as u8 + 1, atlas)));
    }
    objects.push((SPRITE, sprite("toggle_bg", 9, ATLAS_ID)));
    let mut entries: Vec<(&str, i64, u8, [f32; 4], u32)> = spec
        .pips
        .iter()
        .enumerate()
        .filter(|(_, p)| p.listed)
        .map(|(i, p)| {
            (
                p.name.as_str(),
                i as i64 + 3,
                i as u8 + 1,
                p.rect,
                p.settings,
            )
        })
        .collect();
    // Unrelated sprites may have fractional, tightly packed rects.
    entries.insert(0, ("toggle_bg", 7, 9, [2.5, 20.0, 9.5, 4.0], 67));
    objects.push((ATLAS, atlas(&entries, 2)));
    if spec.duplicate_atlas {
        objects.push((ATLAS, atlas(&entries[1..2], 2)));
    }
    (serialized(&objects), stream)
}

fn write(spec: &Spec) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let (bytes, stream) = build(spec);
    std::fs::write(&path, bytes).unwrap();
    if let Some(name) = spec.companion {
        std::fs::write(dir.path().join(name), stream).unwrap();
    }
    (dir, path)
}

fn expected(spec: &Spec) -> Vec<u8> {
    let mut out = Vec::new();
    for r in 0..16u32 {
        for pip in &spec.pips {
            let [x, y, ..] = pip.rect.map(|v| v as u32);
            for c in 0..16 {
                out.extend(texel(x + c, y + 15 - r));
            }
        }
    }
    out
}

#[test]
fn pips_resolve_by_sprite_name_with_top_down_rows() {
    let spec = Spec::default();
    let (_dir, path) = write(&spec);
    let mut pips = locate_enchant_pips(&path, &AtomicBool::new(false))
        .unwrap()
        .expect("pips located");
    let info = pips.info();
    assert_eq!(
        (info.name.as_str(), info.width, info.height),
        ("enchantPips", 64, 16)
    );
    assert_eq!(
        pips.read_rgba(&AtomicBool::new(false)).unwrap(),
        expected(&spec)
    );
}

#[test]
fn streamed_atlas_texture_reads_from_companion() {
    let spec = Spec {
        companion: Some("resources.assets.resS"),
        ..Spec::default()
    };
    let (_dir, path) = write(&spec);
    let mut pips = locate_enchant_pips(&path, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(
        pips.read_rgba(&AtomicBool::new(false)).unwrap(),
        expected(&spec)
    );
    assert!(matches!(
        pips.read_rgba(&AtomicBool::new(true)),
        Err(maxed_assets::AssetError::Cancelled)
    ));
}

#[test]
fn unusable_pip_art_is_absent_rather_than_an_extraction_error() {
    type Change = fn(&mut Spec);
    let cases: [(&str, Change); 6] = [
        ("unlisted", |s| s.pips[3].listed = false),
        ("renamed", |s| s.pips[1].name = "RarityIcon_9".into()),
        ("rotated", |s| s.pips[2].settings = 3 | 1 << 2),
        ("resized", |s| s.pips[0].rect[2] = 32.0),
        ("foreign atlas", |s| s.pips[0].atlas = 9),
        ("listed twice", |s| s.duplicate_atlas = true),
    ];
    for (label, change) in cases {
        let mut spec = Spec::default();
        change(&mut spec);
        let (_dir, path) = write(&spec);
        assert!(
            locate_enchant_pips(&path, &AtomicBool::new(false))
                .unwrap()
                .is_none(),
            "{label}"
        );
    }
}

#[test]
fn cache_publishes_pip_sheet_and_catalog_refs() {
    let (dir, path) = write(&Spec::default());
    let paths = AssetPaths {
        resources: path,
        app_data: dir.path().join("app"),
    };
    let cached = load_or_extract(&paths, &AtomicBool::new(false), &mut |_| {}).unwrap();
    let pips: Vec<_> = cached
        .catalog
        .sprites
        .iter()
        .filter(|s| s.sheet == "enchantPips.png")
        .map(|s| (s.key.as_str(), s.x, s.y, s.width, s.height))
        .collect();
    assert_eq!(
        pips,
        [
            ("enchantPip:1", 0, 0, 16, 16),
            ("enchantPip:2", 16, 0, 16, 16),
            ("enchantPip:3", 32, 0, 16, 16),
            ("enchantPip:4", 48, 0, 16, 16),
        ]
    );
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(cached.root.join("enchantPips.png")).unwrap(),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut rgba).unwrap();
    assert_eq!(rgba, expected(&Spec::default()));
    // Compare served tiles against the independently published strip, including
    // every nonzero horizontal offset (not just the first pip).
    for sprite in cached
        .catalog
        .sprites
        .iter()
        .filter(|s| s.sheet == "enchantPips.png")
    {
        let tile = maxed_assets::sprite_delivery::read_sprite(
            &cached,
            maxed_assets::sprite_delivery::TileKey::from_sprite(sprite).unwrap(),
        )
        .unwrap();
        let mut tile = png::Decoder::new(std::io::Cursor::new(tile))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; tile.output_buffer_size().unwrap()];
        let info = tile.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (16, 16));
        for y in 0..16usize {
            let at = (y * 64 + sprite.x as usize) * 4;
            assert_eq!(
                &pixels[y * 64..(y + 1) * 64],
                &rgba[at..at + 64],
                "{} row {y}",
                sprite.key
            );
        }
    }

    let assert_trim = |catalog: &maxed_types::Catalog| {
        let sprite = |key| catalog.sprites.iter().find(|s| s.key == key).unwrap();
        // The atlas's bottom-up rows 6..12 become top-down rows 4..10.
        assert_eq!(
            sprite("enchantPip:1").trim,
            Some(maxed_types::SpriteTrim {
                x: 2,
                y: 4,
                width: 4,
                height: 6,
            })
        );
        assert_eq!(sprite("enchantPip:2").trim, None);
    };
    assert_trim(&cached.catalog);
    let published: maxed_types::Catalog =
        serde_json::from_slice(&std::fs::read(cached.root.join("catalog.json")).unwrap()).unwrap();
    assert_trim(&published);
    assert_eq!(published, *cached.catalog);
    // Release the lease so reload reads the published catalog from disk.
    let root = cached.root.clone();
    drop(cached);
    let mut phases = vec![];
    let again = load_or_extract(&paths, &AtomicBool::new(false), &mut |p| phases.push(p)).unwrap();
    assert_eq!(again.root, root);
    assert!(!phases.contains(&maxed_assets::AssetProgress::Reading));
    assert_trim(&again.catalog);
    assert_eq!(published, *again.catalog);
}

#[test]
fn cache_without_pips_has_no_pip_refs() {
    let mut spec = Spec::default();
    spec.pips[0].listed = false;
    let (dir, path) = write(&spec);
    let paths = AssetPaths {
        resources: path,
        app_data: dir.path().join("app"),
    };
    let cached = load_or_extract(&paths, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert!(cached
        .catalog
        .sprites
        .iter()
        .all(|s| s.sheet != "enchantPips.png"));
    assert!(!cached.root.join("enchantPips.png").exists());
}
