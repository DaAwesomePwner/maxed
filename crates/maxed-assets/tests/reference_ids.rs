use std::{path::PathBuf, sync::atomic::AtomicBool};

#[test]
#[ignore = "requires MAXED_CLIENT_ASSETS and MAXED_ASSET_ORACLE"]
fn reference_lists_match_existing_java_oracle_byte_for_byte() {
    let client =
        PathBuf::from(std::env::var_os("MAXED_CLIENT_ASSETS").expect("MAXED_CLIENT_ASSETS"));
    let oracle = PathBuf::from(std::env::var_os("MAXED_ASSET_ORACLE").expect("MAXED_ASSET_ORACLE"));
    let cancel = AtomicBool::new(false);
    let assets = maxed_assets::read_text_resources(&client, &cancel).unwrap();
    let lists = maxed_assets::build_reference_id_lists(&assets.texts, &cancel).unwrap();
    for (name, actual) in [
        ("ObjectID.list", lists.object_ids),
        ("TileID.list", lists.tile_ids),
    ] {
        let expected = std::fs::read(oracle.join(name)).unwrap();
        // Report offsets and lengths, never dump complete client tables on failure.
        let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
        assert_eq!(mismatch, None, "{name}: first mismatching byte");
        assert_eq!(actual.len(), expected.len(), "{name}: byte length");
    }
}
