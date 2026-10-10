use maxed_capture::npcap_candidates;
// FFI layout and fake-loader coverage: cargo test -p maxed-capture --lib

#[test]
fn system_npcap_precedes_default_search() {
    let paths = npcap_candidates(std::path::Path::new("C:/Windows"));
    assert_eq!(
        paths[0],
        std::path::PathBuf::from("C:/Windows/System32/Npcap/wpcap.dll")
    );
    assert_eq!(paths[1], std::path::PathBuf::from("wpcap.dll"));
}
