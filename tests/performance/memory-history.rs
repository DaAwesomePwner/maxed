//! Test-only exporter of the exact P4 invented workload; no app launch.
//! Run with `cargo run --offline -p maxed-app --example memory-history --
//! realistic|worst <fresh temporary database filename>`.
#![allow(dead_code)]
use maxed_store::{apply_batch, migrate, open_reader, StoreReader};
use sha2::{Digest, Sha256};
#[path = "../../crates/maxed-store/tests/common/runs_workload.rs"]
mod runs_workload;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("Use realistic|worst and a fresh temporary database filename".into());
    }
    let shape = match args[0].as_str() {
        "realistic" => runs_workload::REALISTIC,
        "worst" => runs_workload::WORST,
        _ => return Err("Unknown synthetic shape".into()),
    };
    let output = std::path::Path::new(&args[1]);
    let parent = output
        .parent()
        .ok_or("Missing output parent")?
        .canonicalize()?;
    let temp = std::env::temp_dir().canonicalize()?;
    if !parent.starts_with(temp) || output.exists() {
        return Err("Output must be a new file below the temporary directory".into());
    }
    let fixture = runs_workload::build(shape);
    // VACUUM INTO includes committed WAL pages and makes a self-contained copy.
    fixture.db.execute("VACUUM INTO ?1", [&args[1]])?;
    let manifest = serde_json::json!({
        "generator": "maxed-p4-runs-workload-v1", "shape": args[0],
        "fights": shape.fights, "timeline": shape.timeline, "bags": shape.bags,
        "players": shape.players, "buckets": shape.buckets,
        "sha256": format!("{:x}", Sha256::digest(std::fs::read(output)?))
    });
    std::fs::write(
        output.with_extension("json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!(
        "shape={} fights={} timeline={} bags={} players={} buckets={}",
        shape.name, shape.fights, shape.timeline, shape.bags, shape.players, shape.buckets
    );
    Ok(())
}
