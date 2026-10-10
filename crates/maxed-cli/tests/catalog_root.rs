//! Full command coverage using a synthetic Unity v22 file, never client data.
use std::process::Command;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_maxed-cli"))
}

#[cfg(windows)]
mod windows {
    use super::command;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "maxed-catroot-command-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn align(out: &mut Vec<u8>) {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    fn string(out: &mut Vec<u8>, value: &str) {
        out.extend((value.len() as u32).to_le_bytes());
        out.extend(value.as_bytes());
        align(out);
    }
    fn synthetic_client() -> Vec<u8> {
        let texts = [
        ("equip", "<Objects><Object type='256' id='Sword'><Class>Equipment</Class><Projectile><Damage>100</Damage></Projectile></Object></Objects>"),
        ("enchantments", "<Enchantments/>"),
        ("players", "<Objects><Object type='100' id='Knight'><Class>Player</Class></Object></Objects>"),
        ("terrain", "<GroundTypes><Ground type='1' id='Lava'><MinDamage>5</MinDamage><MaxDamage>5</MaxDamage></Ground></GroundTypes>"),
    ];
        let mut payload = Vec::new();
        let mut spans = Vec::new();
        for (name, xml) in texts {
            let start = payload.len();
            let mut object = Vec::new();
            string(&mut object, name);
            object.extend((xml.len() as u32).to_le_bytes());
            object.extend(xml.as_bytes());
            spans.push((start, object.len()));
            payload.extend(object);
        }
        let mut metadata = b"6000.0.58f2\0".to_vec();
        metadata.extend(19u32.to_le_bytes()); // target platform
        metadata.push(0); // stripped type trees
        metadata.extend(1u32.to_le_bytes()); // one serialized type
        metadata.extend(49u32.to_le_bytes()); // TextAsset
        metadata.extend([0, 255, 255]);
        metadata.extend([0; 16]);
        metadata.extend((spans.len() as u32).to_le_bytes());
        for (index, (offset, size)) in spans.into_iter().enumerate() {
            align(&mut metadata);
            metadata.extend((index as u64 + 1).to_le_bytes());
            metadata.extend((offset as u64).to_le_bytes());
            metadata.extend((size as u32).to_le_bytes());
            metadata.extend(0u32.to_le_bytes());
        }
        metadata.extend([0; 13]); // scripts, externals, references, user string
        let offset = 48 + metadata.len();
        let mut bytes = vec![0; 48];
        bytes[8..12].copy_from_slice(&22u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
        bytes[24..32].copy_from_slice(&((offset + payload.len()) as u64).to_be_bytes());
        bytes[32..40].copy_from_slice(&(offset as u64).to_be_bytes());
        bytes.extend(metadata);
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn command_extracts_read_only_and_reports_manifest_hash_and_file_count() {
        let scratch = Scratch::new();
        let input = scratch.0.join("resources.assets");
        let source = synthetic_client();
        fs::write(&input, &source).unwrap();
        let root = scratch.0.join("root");
        let result = command()
            .arg("catalog-root")
            .arg("--client")
            .arg(&input)
            .arg("--output")
            .arg(&root)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let manifest: Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        let hash = format!(
            "{:x}",
            Sha256::digest(fs::read(root.join("catalog.json")).unwrap())
        );
        assert_eq!(manifest["catalogSha256"], hash);
        assert_eq!(
            String::from_utf8(result.stdout).unwrap().trim(),
            format!("catalogSha256={hash} files=8")
        );
        assert_eq!(
            fs::read(root.join("TileID.list")).unwrap(),
            b"1;;5,5;Lava\n"
        );
        assert_eq!(
            fs::read(root.join("ObjectID.list")).unwrap(),
            b"100;;Player;;;;;Knight\n256;;Equipment;;,100,100,0;;;Sword\n"
        );
        assert_eq!(fs::read(&input).unwrap(), source);
        let result = command()
            .arg("catalog-root")
            .arg("--client")
            .arg(&input)
            .arg("--output")
            .arg(&root)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&result.stderr).contains(scratch.0.to_str().unwrap()));
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
    }
}

#[test]
#[cfg(not(windows))]
fn unsupported_command_reports_windows_only_before_client_or_output_access() {
    let result = command()
        .args([
            "catalog-root",
            "--client",
            "missing-client",
            "--output",
            "../invalid-output",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).unwrap().trim(),
        "catalog-root v1 is Windows-only"
    );
}

#[test]
fn help_and_invalid_dispatch_describe_current_commands_without_layout_claim() {
    let help = command().arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("catalog-root --client <resources.assets> --output <new-dir>"));
    assert!(help.contains("catalog-root v1 is Windows-only"));
    assert!(help.contains("filled undecodable typed packets"));
    assert!(!help.contains("UPDATE layout"));
    let invalid = command()
        .args(["catalog-root", "--output", "missing"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
}
