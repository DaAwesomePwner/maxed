use std::{fs::File, io::BufReader, path::PathBuf, sync::atomic::AtomicBool};

#[test]
#[ignore = "requires MAXED_CLIENT_ASSETS and MAXED_ASSET_ORACLE"]
fn xml_mapping_and_catalog_ids_match_oracle() {
    let client =
        PathBuf::from(std::env::var_os("MAXED_CLIENT_ASSETS").expect("MAXED_CLIENT_ASSETS"));
    let oracle = PathBuf::from(std::env::var_os("MAXED_ASSET_ORACLE").expect("MAXED_ASSET_ORACLE"));
    let cancel = AtomicBool::new(false);
    let assets = maxed_assets::read_text_resources(&client, &cancel).unwrap();
    assert_eq!(
        assets.sprite_mapping,
        std::fs::read(oracle.join("flatbuffer/spritesheetf")).unwrap()
    );
    let mut compared = 0;
    let mut names = std::collections::BTreeMap::new();
    for text in &assets.texts {
        assert!(!text.name.contains(['/', '\\', ':']));
        let count = names.entry(&text.name).or_insert(0);
        *count += 1;
        let name = if *count == 1 {
            text.name.clone()
        } else {
            format!("{}{count}", text.name)
        };
        let path = oracle.join("xml").join(format!("{name}.xml"));
        if path.is_file() {
            assert_eq!(text.bytes, std::fs::read(path).unwrap(), "text {name}");
            compared += 1;
        }
    }
    assert!(compared > 200);
    let sheets = maxed_assets::open_sheets(&client, &cancel).unwrap();
    let infos = sheets.iter().map(|s| s.info().clone()).collect::<Vec<_>>();
    let c = maxed_assets::build_catalog_with_sheets(assets, "a".repeat(64), &infos).unwrap();
    // equip.xml's Necrotic Skull has its scaling Activate inside Ability.
    let skull = c
        .items
        .iter()
        .find(|item| item.object_type == 0xaaa)
        .unwrap();
    let scaling = skull.scaling.as_ref().unwrap();
    assert_eq!(
        (scaling.stat_id, scaling.minimum, scaling.damage_per_stat),
        (27, 55, 3.0)
    );
    assert_eq!(scaling.origin_item, None);
    let list_bytes = std::fs::read(oracle.join("ObjectID.list")).unwrap();
    let lists = String::from_utf8_lossy(&list_bytes);
    let ids = lists
        .lines()
        .filter_map(|s| u16::try_from(s.split(';').next().unwrap().parse::<i64>().unwrap()).ok())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids, c.objects.iter().map(|o| o.object_type).collect());
    let mut definitions = std::collections::BTreeMap::new();
    for line in lists.lines() {
        let fields = line.split(';').collect::<Vec<_>>();
        if let Ok(id) = fields[0].parse::<u16>() {
            definitions.insert(id, fields);
        }
    }
    for object in &c.objects {
        let fields = &definitions[&object.object_type];
        assert_eq!(
            object.is_player,
            fields[2] == "Player",
            "player {}",
            object.object_type
        );
        assert_eq!(
            object.is_boss,
            fields[6].split(',').any(|v| v.trim() == "BOSS"),
            "boss {}",
            object.object_type
        );
        assert_eq!(
            object.is_miniboss,
            fields[6].split(',').any(|v| v.trim() == "MINIBOSS"),
            "miniboss {}",
            object.object_type
        );
        let expected = if fields[1].is_empty() {
            fields[7]
        } else {
            fields[1]
        };
        // Java's ID list uses the Windows default charset; compare names only
        // when no replacement/charset conversion is involved. XML itself matched exactly.
        if object.name.is_ascii() && expected.is_ascii() {
            assert_eq!(
                object.name.trim(),
                expected.trim(),
                "name {}",
                object.object_type
            );
        }
    }
    let tile_bytes = std::fs::read(oracle.join("TileID.list")).unwrap();
    let tiles = String::from_utf8_lossy(&tile_bytes);
    let mut tile_damage = std::collections::BTreeMap::new();
    for line in tiles.lines() {
        let fields = line.split(';').collect::<Vec<_>>();
        if !fields[2].is_empty() {
            let pair = fields[2]
                .split(',')
                .map(|s| s.parse::<i32>().unwrap())
                .collect::<Vec<_>>();
            tile_damage.insert(fields[0].parse::<u16>().unwrap(), (pair[0], pair[1]));
        }
    }
    let tiles = tiles
        .lines()
        .filter_map(|s| {
            let f = s.split(';').collect::<Vec<_>>();
            (!f[2].is_empty()).then(|| f[0].parse::<u16>().unwrap())
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(tiles, c.tiles.iter().map(|t| t.tile_type).collect());
    for tile in &c.tiles {
        assert_eq!(
            (tile.min_damage, tile.max_damage),
            tile_damage[&tile.tile_type]
        );
    }
    eprintln!("oracle XML files={compared}, objects={}, items={}, classes={}, enchants={}, sprites={}, damaging tiles={}",c.objects.len(),c.items.len(),c.classes.len(),c.enchants.len(),c.sprites.len(),c.tiles.len());
}

#[test]
#[ignore = "requires MAXED_CLIENT_ASSETS and MAXED_ASSET_ORACLE"]
fn every_sheet_pixel_matches_or_reports_exact_regions() {
    use std::io::Write;
    let client =
        PathBuf::from(std::env::var_os("MAXED_CLIENT_ASSETS").expect("MAXED_CLIENT_ASSETS"));
    let oracle = PathBuf::from(std::env::var_os("MAXED_ASSET_ORACLE").expect("MAXED_ASSET_ORACLE"));
    let reports = tempfile::Builder::new()
        .prefix("maxed-asset-pixel-regions-")
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("Exact difference reports: {}", reports.display());
    let cancel = AtomicBool::new(false);
    let mut sheets = maxed_assets::open_sheets(&client, &cancel).unwrap();
    assert_eq!(sheets.len(), 4);
    let mut unexpected = 0;
    for sheet in &mut sheets {
        let info = sheet.info().clone();
        let mut report = std::io::BufWriter::new(
            File::create(reports.join(format!("{}.jsonl", info.name))).unwrap(),
        );
        let mut decoder = png::Decoder::new(BufReader::new(
            File::open(oracle.join("sprites").join(format!("{}.png", info.name))).unwrap(),
        ));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut png = decoder.read_info().unwrap();
        assert_eq!(
            (png.info().width, png.info().height),
            (info.width, info.height)
        );
        assert_eq!(
            png.output_color_type(),
            (png::ColorType::Rgba, png::BitDepth::Eight)
        );
        let mut row = vec![0; info.width as usize * 4];
        let mut differences = [0usize; 3];
        for y in 0..info.height {
            sheet.read_row(y, &mut row, &cancel).unwrap();
            let expected = png.next_row().unwrap().unwrap();
            let mut run = None;
            for (x, (a, b)) in row
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.data().as_chunks::<4>().0.iter())
                .enumerate()
            {
                // Check every pixel, including native == oracle: equality is
                // insufficient when the Java transform requires a change.
                let class = difference_class(a, b);
                if class == 2 && differences[2] == 0 {
                    eprintln!("{} first unexpected pixel ({x},{y}): native={a:?}, oracle={b:?}, modeled={:?}", info.name, java_png_model(*a));
                }
                if a != b || class == 2 {
                    differences[class] += 1;
                    if run.is_none() {
                        run = Some(x);
                    }
                } else if let Some(start) = run.take() {
                    writeln!(report, "{{\"y\":{y},\"x_start\":{start},\"x_end\":{x}}}").unwrap();
                }
            }
            if let Some(start) = run {
                writeln!(
                    report,
                    "{{\"y\":{y},\"x_start\":{start},\"x_end\":{}}}",
                    info.width
                )
                .unwrap();
            }
        }
        report.flush().unwrap();
        assert!(png.next_row().unwrap().is_none());
        eprintln!("{}: pixels={}, differences [modeled transparent, modeled partial-alpha, unexpected]={:?}", info.name, u64::from(info.width) * u64::from(info.height), differences);
        unexpected += differences[2];
    }
    // D-sprite-alpha: opaque pixels must be identical; every other oracle
    // pixel must equal the exact test-only Java model, with no tolerance bucket.
    assert_eq!(unexpected, 0);
}

#[test]
#[ignore = "requires MAXED_CLIENT_ASSETS; creates only a private temporary cache"]
fn installed_cache_miss_hit_and_native_png_round_trip() {
    let client =
        PathBuf::from(std::env::var_os("MAXED_CLIENT_ASSETS").expect("MAXED_CLIENT_ASSETS"));
    let temp = tempfile::tempdir().unwrap();
    let paths = maxed_assets::AssetPaths {
        resources: client.clone(),
        app_data: temp.path().join("cache"),
    };
    let cancel = AtomicBool::new(false);
    let start = std::time::Instant::now();
    let first =
        maxed_assets::load_or_extract(&paths, &cancel, &mut |p| eprintln!("{p:?}")).unwrap();
    eprintln!("cache miss elapsed={:?}", start.elapsed());
    let start = std::time::Instant::now();
    let second = maxed_assets::load_or_extract(&paths, &cancel, &mut |p| {
        assert_ne!(p, maxed_assets::AssetProgress::Reading)
    })
    .unwrap();
    eprintln!("cache hit elapsed={:?}", start.elapsed());
    assert_eq!(first.catalog, second.catalog);
    for mut sheet in maxed_assets::open_sheets(&client, &cancel).unwrap() {
        let info = sheet.info().clone();
        let mut png = png::Decoder::new(BufReader::new(
            File::open(first.root.join(format!("{}.png", info.name))).unwrap(),
        ))
        .read_info()
        .unwrap();
        let mut row = vec![0; info.width as usize * 4];
        for y in 0..info.height {
            sheet.read_row(y, &mut row, &cancel).unwrap();
            assert_eq!(
                row,
                png.next_row().unwrap().unwrap().data(),
                "{} row {}",
                info.name,
                y
            );
        }
        assert!(png.next_row().unwrap().is_none());
    }
}

/// D-sprite-alpha, test-only mathematical model of RealmShark's sheet writer:
/// UnityExtractor.java:92-106 uses an RGBA byte raster, a premultiplied sRGB
/// ComponentColorModel, drawImage into a cleared TYPE_INT_ARGB, then ImageIO PNG.
/// ImageBuffer.getSprite only reads/crops those PNGs; it is not the sheet writer.
///
/// The custom raster first goes through ComponentColorModel.getRGB. Its byte
/// extractComponent uses f32 arithmetic and does NOT clamp before packed-ARGB
/// ORs. Straight RGB > alpha therefore spills into adjacent channels and alpha.
/// SrcOver onto transparent IntArgb then quantizes through Java2D's 8-bit
/// premultiply/unpremultiply tables. ImageIO preserves the resulting ARGB bytes.
/// These equations model that behavior, not a generic premultiplication rule.
/// Sources: OpenJDK ComponentColorModel.extractComponent/getRGB and Java2D
/// loops/AlphaMath.c initAlphaTables; independent API-only fixture is in
/// common/JavaPngVectors.java. Production always keeps the original RGBA.
fn java_png_model(rgba: [u8; 4]) -> [u8; 4] {
    if rgba[3] == 0 {
        return [0; 4];
    }
    if rgba[3] == 255 {
        return rgba;
    }
    let expand = |c: u8| -> u32 {
        ((f32::from(c) / 255.0) * (255.0 / f32::from(rgba[3])) * 255.0 + 0.5) as u32
    };
    let packed = (u32::from(rgba[3]) << 24)
        | (expand(rgba[0]) << 16)
        | (expand(rgba[1]) << 8)
        | expand(rgba[2]);
    let alpha = (packed >> 24) as u8;
    let quantize = |c: u8| -> u8 {
        // Exact fixed-point Java2D table entries, evaluated in u64 to keep
        // intermediate arithmetic bounded without overflow. No float tolerance.
        let a = u64::from(alpha);
        let premultiplied = (u64::from(c) * a * 65793 + (1 << 23)) >> 24;
        let reciprocal = ((255 << 24) + a / 2) / a;
        ((premultiplied * reciprocal + (1 << 23)) >> 24).min(255) as u8
    };
    [
        quantize((packed >> 16) as u8),
        quantize((packed >> 8) as u8),
        quantize(packed as u8),
        alpha,
    ]
}

// Classify all pixels, not just native/oracle differences. In particular alpha
// errors and arbitrary transparent RGB must not be hidden by an expected bucket.
fn difference_class(native: &[u8; 4], oracle: &[u8; 4]) -> usize {
    if *oracle != java_png_model(*native) {
        2
    } else if native[3] == 0 {
        0
    } else {
        1
    }
}

#[test]
fn java_model_matches_independent_awt_png_fixture() {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    // Every byte value at every alpha, isolated channels plus mixed channels
    // that exercise packed-channel spill. Same synthetic inputs as the Java
    // generator; expected digest comes from AWT drawImage + ImageIO round trip.
    for pattern in 0..4 {
        for alpha in 0..=255u8 {
            for c in 0..=255u8 {
                let rgba = match pattern {
                    0 => [c, 0, 0, alpha],
                    1 => [0, c, 0, alpha],
                    2 => [0, 0, c, alpha],
                    _ => [
                        c,
                        255 - c,
                        c.wrapping_mul(73).wrapping_add(alpha.wrapping_mul(19)),
                        alpha,
                    ],
                };
                digest.update(java_png_model(rgba));
            }
        }
    }
    // OpenJDK 17.0.20.1, Windows software AWT pipeline, 262144 pixels.
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "4aa41606dcc71600ef5ae7cb7c4ebd911764ea2cec46e4368da572d45b821e29"
    );
}

#[test]
fn java_model_explains_channel_spill_and_rejects_unmodeled_changes() {
    for (native, oracle) in [
        ([240, 0, 238, 128], [221, 2, 217, 129]),
        ([63, 0, 0, 3], [233, 0, 0, 23]),
        ([104, 94, 90, 32], [66, 240, 204, 35]),
        ([83, 187, 91, 64], [75, 231, 106, 65]),
        ([250, 128, 77, 0], [0, 0, 0, 0]),
        ([250, 128, 77, 255], [250, 128, 77, 255]),
    ] {
        assert_eq!(java_png_model(native), oracle);
        assert_ne!(difference_class(&native, &oracle), 2);
        // Every possible single-channel deviation must be unexpected, including
        // alpha and RGB at alpha zero. No partial-alpha exemption remains.
        for channel in 0..4 {
            for value in 0..=255u8 {
                if value != oracle[channel] {
                    let mut wrong = oracle;
                    wrong[channel] = value;
                    assert_eq!(difference_class(&native, &wrong), 2);
                }
            }
        }
    }
    let native = [240, 0, 238, 128];
    assert_eq!(difference_class(&native, &native), 2);
}
