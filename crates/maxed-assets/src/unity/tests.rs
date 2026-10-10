use super::*;

#[test]
fn companion_paths_cannot_escape_client_directory() {
    let dir = tempfile::tempdir().unwrap();
    let resources = dir.path().join("resources.assets");
    let companion = dir.path().join("resources.assets.resS");
    std::fs::write(&companion, [0]).unwrap();
    assert_eq!(
        safe_companion(&resources, "resources.assets.resS").unwrap(),
        companion.canonicalize().unwrap()
    );
    for path in [
        "",
        "../outside.resS",
        "/absolute.resS",
        "C:/absolute.resS",
        "\\\\host\\share.resS",
        "nested\\..\\outside.resS",
        "foo:stream",
        "./resources.assets.resS",
    ] {
        assert!(safe_companion(&resources, path).is_err(), "{path}");
    }
    assert!(matches!(
        safe_companion(&resources, "missing.resS"),
        Err(AssetError::Io)
    ));
}

#[test]
fn integers_obey_byte_order_and_short_reads_fail() {
    for big in [false, true] {
        let mut bytes = Vec::new();
        bytes.extend(if big {
            42u16.to_be_bytes()
        } else {
            42u16.to_le_bytes()
        });
        bytes.extend(if big {
            123456u32.to_be_bytes()
        } else {
            123456u32.to_le_bytes()
        });
        bytes.extend(if big {
            u64::MAX.to_be_bytes()
        } else {
            u64::MAX.to_le_bytes()
        });
        let mut reader = crate::reader::Reader::new(&bytes, big);
        assert_eq!(reader.u16().unwrap(), 42);
        assert_eq!(reader.u32().unwrap(), 123456);
        assert_eq!(reader.u64().unwrap(), u64::MAX);
        assert!(reader.u8().is_err());
    }
}

#[test]
fn unterminated_strings_and_negative_lengths_fail() {
    let data = [b'a'; 4097];
    let mut r = crate::reader::Reader::new(&data, false);
    assert!(matches!(r.cstring(), Err(AssetError::LimitExceeded)));
    let data = u32::MAX.to_le_bytes();
    let mut r = crate::reader::Reader::new(&data, false);
    assert!(matches!(r.string(), Err(AssetError::LimitExceeded)));
}
