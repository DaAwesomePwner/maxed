use super::{identity_key, publish_identity_key, publish_identity_key_with};
use std::{fs, io, path::Path};

fn unsupported_link_errors() -> Vec<io::Error> {
    let errors = vec![io::ErrorKind::Unsupported.into()];
    #[cfg(windows)]
    let errors = {
        let mut errors = errors;
        errors.extend([
            io::Error::from_raw_os_error(1),
            io::Error::from_raw_os_error(50),
        ]);
        errors
    };
    errors
}

#[test]
fn identity_key_unsupported_link_creates_and_reuses_key() {
    for error in unsupported_link_errors() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("identity.key");
        let staging = root.path().join("identity.key.pending");
        fs::write(&staging, [7; 32]).unwrap();
        publish_identity_key_with(&staging, &path, |_, _| Err(error)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), [7; 32]);
        assert!(!staging.exists());
        assert_eq!(identity_key(root.path()).unwrap(), [7; 32]);
    }
}

#[test]
fn identity_key_unsupported_link_preserves_destination_appearing_during_link() {
    for len in [0, 1, 32, 33] {
        for error in unsupported_link_errors() {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("identity.key");
            let staging = root.path().join("identity.key.pending");
            let original = vec![42; len];
            fs::write(&staging, [7; 32]).unwrap();
            let result = publish_identity_key_with(&staging, &path, |_, to| {
                fs::write(to, &original).unwrap();
                Err(error)
            });
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(fs::read(&staging).unwrap(), [7; 32]);
        }
    }
}

#[test]
fn identity_key_other_link_errors_do_not_fall_back() {
    for kind in [
        io::ErrorKind::AlreadyExists,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::NotFound,
        io::ErrorKind::Other,
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("identity.key");
        let staging = root.path().join("identity.key.pending");
        fs::write(&staging, [7; 32]).unwrap();
        let error =
            publish_identity_key_with(&staging, &path, |_, _| Err(kind.into())).unwrap_err();
        assert_eq!(error.kind(), kind);
        assert!(!path.exists());
        assert_eq!(fs::read(&staging).unwrap(), [7; 32]);
    }
}

fn assert_created(root: &Path) {
    let key = identity_key(root).unwrap();
    let saved = fs::read(root.join("identity.key")).unwrap();
    assert_eq!(saved.len(), 32);
    assert_eq!(saved, key);
    assert!(!root.join("identity.key.pending").exists());
    assert_eq!(identity_key(root).unwrap(), key);
}

#[test]
fn fresh_identity_key_is_published_and_reused() {
    let root = tempfile::tempdir().unwrap();
    assert_created(root.path());
}

#[test]
fn empty_identity_key_is_recovered() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("identity.key"), []).unwrap();
    assert_created(root.path());
}

#[test]
fn stale_identity_key_pending_is_replaced() {
    for len in [0, 7, 32] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("identity.key.pending"), vec![42; len]).unwrap();
        assert_created(root.path());
    }
}

#[test]
fn nonempty_invalid_identity_key_is_preserved() {
    for len in [1, 7, 31, 33] {
        let root = tempfile::tempdir().unwrap();
        let bytes = vec![42; len];
        let path = root.path().join("identity.key");
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            identity_key(root.path()).unwrap_err(),
            "Local identity key is invalid"
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert!(!root.path().join("identity.key.pending").exists());
    }
}

#[test]
fn existing_identity_key_is_unchanged_even_with_stale_pending() {
    let root = tempfile::tempdir().unwrap();
    let key = [42; 32];
    let path = root.path().join("identity.key");
    fs::write(&path, key).unwrap();
    fs::write(root.path().join("identity.key.pending"), [7; 32]).unwrap();
    assert_eq!(identity_key(root.path()).unwrap(), key);
    assert_eq!(fs::read(path).unwrap(), key);
}

#[test]
fn identity_key_publication_never_replaces_existing_destination() {
    for len in [1, 32, 33] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("identity.key");
        let staging = root.path().join("identity.key.pending");
        let original = vec![42; len];
        fs::write(&path, &original).unwrap();
        fs::write(&staging, [7; 32]).unwrap();
        assert_eq!(
            publish_identity_key(&staging, &path).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&staging).unwrap(), [7; 32]);
    }
}

#[cfg(windows)]
#[test]
fn identity_key_open_error_preserves_existing_key_and_pending() {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("identity.key");
    let staging = root.path().join("identity.key.pending");
    fs::write(&path, [42; 32]).unwrap();
    fs::write(&staging, [7; 32]).unwrap();
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    assert_eq!(
        identity_key(root.path()).unwrap_err(),
        "Could not read local identity key"
    );
    drop(locked);
    assert_eq!(fs::read(&path).unwrap(), [42; 32]);
    assert_eq!(fs::read(&staging).unwrap(), [7; 32]);
    assert_eq!(identity_key(root.path()).unwrap(), [42; 32]);
}
