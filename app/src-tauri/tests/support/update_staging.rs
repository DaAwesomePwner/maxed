use super::{staging::Namespace, Error};

#[test]
fn cross_process_install_is_busy() {
    if let Some(root) = std::env::var_os("MAXED_SYNTHETIC_UPDATE_ROOT") {
        assert!(matches!(
            Namespace::acquire(std::path::Path::new(&root)),
            Err(Error::Busy)
        ));
        std::process::exit(73);
    }
    let root = tempfile::tempdir().unwrap();
    let (_staged, path) = Namespace::acquire(root.path())
        .unwrap()
        .stage(b"MZ synthetic")
        .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "updater::staging_tests::cross_process_install_is_busy",
        ])
        .env("MAXED_SYNTHETIC_UPDATE_ROOT", root.path())
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(73),
        "child must reach the Busy assertion"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"MZ synthetic");
}

#[cfg(windows)]
#[test]
fn inherited_acl_namespace_is_usable_but_foreign_entries_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let namespace = root.path().join("updates-v1");
    std::fs::create_dir(&namespace).unwrap();
    let owned = namespace.join("stage-v1-abcdefghijkl");
    std::fs::create_dir(&owned).unwrap();
    std::fs::write(owned.join("installer.exe"), b"MZ stale").unwrap();
    let lease = Namespace::acquire(root.path()).unwrap();
    assert!(!owned.exists());
    drop(lease);
    let foreign = namespace.join("keep.txt");
    std::fs::write(&foreign, b"keep").unwrap();
    assert!(matches!(
        Namespace::acquire(root.path()),
        Err(Error::Install)
    ));
    assert_eq!(std::fs::read(foreign).unwrap(), b"keep");
}

#[cfg(windows)]
#[test]
fn staging_junction_is_refused_without_touching_its_target() {
    let root = tempfile::tempdir().unwrap();
    drop(Namespace::acquire(root.path()).unwrap());
    let target = root.path().join("foreign");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("installer.exe"), b"keep").unwrap();
    let junction = root.path().join("updates-v1").join("stage-v1-abcdefghijkl");
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic junction creation failed"
    );
    assert!(matches!(
        Namespace::acquire(root.path()),
        Err(Error::Install)
    ));
    assert_eq!(
        std::fs::read(target.join("installer.exe")).unwrap(),
        b"keep"
    );
    std::fs::remove_dir(junction).unwrap();
}

#[test]
fn namespace_lease_blocks_another_install_without_touching_active_staging() {
    let root = tempfile::tempdir().unwrap();
    let (staged, path) = Namespace::acquire(root.path())
        .unwrap()
        .stage(b"MZ invented")
        .unwrap();
    assert!(matches!(Namespace::acquire(root.path()), Err(Error::Busy)));
    assert_eq!(std::fs::read(&path).unwrap(), b"MZ invented");
    let parent = staged.path().to_owned();
    staged.close().unwrap();
    assert!(!parent.exists());
    assert!(Namespace::acquire(root.path()).is_ok());
}

#[test]
fn only_stale_owned_files_are_removed_after_lease_acquisition() {
    let root = tempfile::tempdir().unwrap();
    let lease = Namespace::acquire(root.path()).unwrap();
    let namespace = root.path().join("updates-v1");
    let stale = namespace.join("stage-v1-abcdefghijkl");
    std::fs::create_dir(&stale).unwrap();
    std::fs::write(stale.join("installer.exe"), b"MZ stale").unwrap();
    let foreign = namespace.join("stage-v1-123456789abc");
    std::fs::create_dir(&foreign).unwrap();
    std::fs::write(foreign.join("unowned.txt"), b"keep").unwrap();
    assert!(matches!(Namespace::acquire(root.path()), Err(Error::Busy)));
    assert!(stale.exists());
    drop(lease);
    assert!(matches!(
        Namespace::acquire(root.path()),
        Err(Error::Install)
    ));
    assert!(
        stale.exists(),
        "foreign entries block cleanup before any deletion"
    );
    assert_eq!(std::fs::read(foreign.join("unowned.txt")).unwrap(), b"keep");
    std::fs::remove_file(foreign.join("unowned.txt")).unwrap();
    std::fs::remove_dir(&foreign).unwrap();
    let _lease = Namespace::acquire(root.path()).unwrap();
    assert!(!stale.exists());
}

#[cfg(windows)]
#[test]
fn immutable_staged_handle_denies_write_delete_and_parent_replacement() {
    let root = tempfile::tempdir().unwrap();
    let (staged, path) = Namespace::acquire(root.path())
        .unwrap()
        .stage(b"MZ invented")
        .unwrap();
    assert!(std::fs::write(&path, b"tampered").is_err());
    assert!(std::fs::remove_file(&path).is_err());
    assert!(std::fs::rename(staged.path(), root.path().join("replaced")).is_err());
    assert!(std::fs::rename(
        root.path().join("updates-v1"),
        root.path().join("replaced-root")
    )
    .is_err());
    assert_eq!(staged.read().unwrap(), b"MZ invented");
}
