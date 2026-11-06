use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn read_manifest(path: &Path) -> toml::Value {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("Cannot read {}: {error}", path.display()))
        .parse()
        .unwrap_or_else(|error| panic!("Cannot parse {}: {error}", path.display()))
}

fn register_path(
    path: &Path,
    declaration: &Path,
    registered: &mut BTreeSet<PathBuf>,
    failures: &mut Vec<String>,
) {
    if path.is_file() {
        registered.insert(path.canonicalize().unwrap());
    } else {
        failures.push(format!(
            "{} declared in {} does not exist; restore the file or remove/correct its registration",
            path.display(),
            declaration.display(),
        ));
    }
}

#[test]
fn workspace_integration_suites_are_registered() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace_manifest = read_manifest(&workspace.join("Cargo.toml"));
    let members = workspace_manifest["workspace"]["members"]
        .as_array()
        .expect("workspace.members must list the packages checked by the registration guard");
    let mut failures = Vec::new();

    for member in members {
        // Reading members from the manifest includes new packages automatically.
        // Unsupported member patterns fail here rather than silently skipping packages.
        let package = workspace.join(member.as_str().expect("workspace member must be a path"));
        let manifest_path = package.join("Cargo.toml");
        let manifest = read_manifest(&manifest_path);
        if manifest["package"]
            .get("autotests")
            .and_then(toml::Value::as_bool)
            != Some(false)
        {
            continue;
        }

        let mut registered = BTreeSet::new();
        if let Some(targets) = manifest.get("test").and_then(toml::Value::as_array) {
            for target in targets {
                let path = target
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_else(|| {
                        panic!(
                            "Declare an explicit path for each [[test]] in {}",
                            manifest_path.display()
                        )
                    });
                register_path(
                    &package.join(path),
                    &manifest_path,
                    &mut registered,
                    &mut failures,
                );
            }
        }

        let root = package.join("tests/integration/main.rs");
        if root.is_file() {
            let source = fs::read_to_string(&root).unwrap();
            for line in source.lines() {
                let Some(attribute) = line.trim().strip_prefix("#[path") else {
                    continue;
                };
                let path = attribute
                    .trim_start()
                    .strip_prefix('=')
                    .and_then(|value| value.trim().strip_suffix(']'))
                    .and_then(|value| value.trim().strip_prefix('"'))
                    .and_then(|value| value.strip_suffix('"'))
                    .unwrap_or_else(|| {
                        panic!(
                            "Use a one-line #[path = \"...\"] declaration in {}: {line}",
                            root.display()
                        )
                    });
                register_path(
                    &root.parent().unwrap().join(path),
                    &root,
                    &mut registered,
                    &mut failures,
                );
            }
        }

        let tests = package.join("tests");
        if !tests.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&tests).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
                && !registered.contains(&path.canonicalize().unwrap())
            {
                failures.push(format!(
                    "{} is unregistered with autotests = false; add a #[path = \"../{}\"] module to {} or a [[test]] path in {}",
                    path.display(),
                    path.file_name().unwrap().to_string_lossy(),
                    root.display(),
                    manifest_path.display(),
                ));
            }
        }
    }

    failures.sort();
    assert!(
        failures.is_empty(),
        "Integration test registration failed:\n{}",
        failures.join("\n")
    );
}
