//! Local packaging validation utility. No keys or bytes are printed, no network.
#![forbid(unsafe_code)]
mod verification;
use std::{fs::File, io::Read, path::Path};

fn read_bounded(path: &Path, bound: usize) -> Result<Vec<u8>, ()> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| ())?
        .take(bound as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > bound {
        return Err(());
    }
    Ok(bytes)
}

fn run() -> Result<(), ()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(());
    }
    let config: serde_json::Value =
        serde_json::from_slice(&read_bounded(Path::new(&args[0]), 65_536)?).map_err(|_| ())?;
    let key = verification::public_key(
        config
            .pointer("/plugins/updater/pubkey")
            .and_then(|v| v.as_str())
            .ok_or(())?,
    )
    .map_err(|_| ())?;
    let version = semver::Version::parse(config.get("version").and_then(|v| v.as_str()).ok_or(())?)
        .map_err(|_| ())?;
    let bytes = read_bounded(Path::new(&args[1]), 20_000_000)?;
    let signature = read_bounded(Path::new(&args[2]), 8192)?;
    verification::verify(
        &bytes,
        std::str::from_utf8(&signature).map_err(|_| ())?,
        &key,
        &version,
    )
    .map_err(|_| ())
}

fn main() {
    if run().is_err() {
        eprintln!(
            "Signed package verification failed (configuration, key, signature, version or size)."
        );
        std::process::exit(1);
    }
}
