//! Executes the real package entry point and Tauri signer with disposable keys.
//! Only the expensive package-build body is replaced by a reached-build sentinel.
#![cfg(all(windows, feature = "updater-signing-tool"))]
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Key {
    secret: String,
    public: String,
}

fn generate(cli: &Path, directory: &Path, name: &str) -> Key {
    let path = directory.join(name);
    let result = Command::new("node")
        .arg(cli)
        .args([
            "signer",
            "generate",
            "--ci",
            "--password",
            "p7-test-password",
            "--write-keys",
        ])
        .arg(&path)
        .env_remove("TAURI_SIGNING_PRIVATE_KEY")
        .env_remove("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
        .env_remove("TAURI_SIGNING_PRIVATE_KEY_PATH")
        .output()
        .unwrap();
    assert!(result.status.success(), "disposable key generation failed");
    let key = Key {
        secret: fs::read_to_string(&path).unwrap(),
        public: fs::read_to_string(directory.join(format!("{name}.pub"))).unwrap(),
    };
    fs::remove_file(path).unwrap();
    key
}

#[test]
fn signed_package_entry_rejects_missing_wrong_key_wrong_password_and_unresolved_public_key() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    // Node's entry-point resolver does not accept Rust's Windows extended prefix.
    let repo = PathBuf::from(repo.to_string_lossy().trim_start_matches(r"\\?\"));
    let real_cli = repo.join("app/ui/node_modules/@tauri-apps/cli/tauri.js");
    let fixture = tempfile::Builder::new()
        .prefix("p7-package-proof-")
        .tempdir_in(repo.join("target"))
        .unwrap();
    let root = fixture.path();
    let signer_key = generate(&real_cli, root, "disposable-a");
    let other_key = generate(&real_cli, root, "disposable-b");
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::create_dir_all(root.join("app/src-tauri")).unwrap();
    fs::create_dir_all(root.join("app/ui/node_modules/@tauri-apps/cli")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::copy(
        repo.join("scripts/build-release.ps1"),
        root.join("scripts/build-release.ps1"),
    )
    .unwrap();
    // Test-only shim forwards signer calls to the actual locked CLI. There is no
    // signer mock and no production injection/environment bypass in the script.
    fs::write(root.join("app/ui/node_modules/@tauri-apps/cli/tauri.js"), format!(
        "if (process.argv[2] === 'signer') {{ require({}); }} else {{ console.log('S2_PACKAGE_BUILD_REACHED'); process.exit(79); }}",
        serde_json::to_string(&real_cli.to_string_lossy().replace("\\\\?\\", "")).unwrap(),
    )).unwrap();
    fs::write(
        root.join("bin/cargo.cmd"),
        format!(
            "@echo off\r\n:next\r\nif \"%~1\"==\"--\" goto verify\r\nif \"%~1\"==\"\" exit /b 2\r\nshift\r\ngoto next\r\n:verify\r\nshift\r\n\"{}\" \"%~1\" \"%~2\" \"%~3\"\r\nexit /b %errorlevel%\r\n",
            env!("CARGO_BIN_EXE_maxed-update-verify")
        ),
    )
    .unwrap();
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![root.join("bin")];
    paths.extend(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).unwrap();
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join("app/src-tauri/tauri.conf.json")).unwrap())
            .unwrap();
    config["bundle"]["createUpdaterArtifacts"] = true.into();
    for (name, private, public, password, expected) in [
        (
            "missing",
            None,
            signer_key.public.as_str(),
            "p7-test-password",
            "require TAURI_SIGNING_PRIVATE_KEY",
        ),
        (
            "wrong key",
            Some(other_key.secret.as_str()),
            signer_key.public.as_str(),
            "p7-test-password",
            "does not match the configured",
        ),
        (
            "wrong password",
            Some(signer_key.secret.as_str()),
            signer_key.public.as_str(),
            "wrong-test-password",
            "key/password preflight failed",
        ),
        (
            "unresolved public",
            Some(signer_key.secret.as_str()),
            "",
            "p7-test-password",
            "resolved plugins.updater.pubkey",
        ),
        (
            "matching key",
            Some(signer_key.secret.as_str()),
            signer_key.public.as_str(),
            "p7-test-password",
            "S2_PACKAGE_BUILD_REACHED",
        ),
    ] {
        config["plugins"]["updater"]["pubkey"] = public.into();
        fs::write(
            root.join("app/src-tauri/tauri.conf.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let mut command = Command::new("powershell");
        command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(root.join("scripts/build-release.ps1"))
            .args(["-Package", "-Offline"])
            .env("PATH", &path)
            .env("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", password)
            .env_remove("TAURI_SIGNING_PRIVATE_KEY_PATH");
        if let Some(private) = private {
            command.env("TAURI_SIGNING_PRIVATE_KEY", private);
        } else {
            command.env_remove("TAURI_SIGNING_PRIVATE_KEY");
        }
        let output = command.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success(),
            "{name}: must not report package success"
        );
        assert!(
            text.contains(expected),
            "{name}: expected guard or positive sentinel missing"
        );
        assert_eq!(
            text.contains("S2_PACKAGE_BUILD_REACHED"),
            name == "matching key",
            "{name}: unexpected package invocation"
        );
        assert!(
            !text.contains(&signer_key.secret) && !text.contains(&other_key.secret),
            "private key leaked"
        );
        if root.join("target").exists() {
            assert_eq!(
                fs::read_dir(root.join("target")).unwrap().count(),
                0,
                "challenge cleanup failed"
            );
        }
    }
}
