#![forbid(unsafe_code)]

use maxed_cli::{decode, parity::parity_with_fixture, record, replay, CliError};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const USAGE: &str = "Usage:
  maxed-cli make-demo <new-output.pcap>
  maxed-cli catalog-root --client <resources.assets> --output <new-dir>
  maxed-cli export-domain <input.pcap> --catalog <catalog.json> --output <new.jsonl>
  maxed-cli parity-domain <java.jsonl> <rust.jsonl> [--ledger <path>]
  maxed-cli record <private.pcap>
  maxed-cli scrub <input.pcap> <output.pcap> [--allow-undecodable]
  maxed-cli replay <input.pcap>
  maxed-cli decode <input.pcap> --output <private.jsonl> [--explain-failures <private.txt>]
  maxed-cli parity <java.jsonl> <rust.jsonl> [--ledger <path>] [--fixture-sha256 <hash>] [--limit <count>]

catalog-root v1 is Windows-only; the output parent directory must already exist.
--allow-undecodable creates provisional fixtures containing filled undecodable typed packets.";

fn run() -> Result<i32, CliError> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|s| s.to_str()) {
        Some("catalog-root")
            if args.len() == 5 && args[1] == "--client" && args[3] == "--output" =>
        {
            let report =
                maxed_cli::catalog_root::catalog_root(Path::new(&args[2]), Path::new(&args[4]))?;
            println!(
                "catalogSha256={} files={}",
                report.catalog_sha256, report.file_count
            );
            Ok(0)
        }
        Some("export-domain")
            if args.len() == 6 && args[2] == "--catalog" && args[4] == "--output" =>
        {
            maxed_cli::domain_export::export_domain(
                Path::new(&args[1]),
                Path::new(&args[3]),
                Path::new(&args[5]),
            )?;
            Ok(0)
        }
        Some("parity-domain") if args.len() == 3 || args.len() == 5 && args[3] == "--ledger" => {
            let ledger = args.get(4).map(Path::new);
            let report = maxed_cli::domain_compare::domain_parity(
                Path::new(&args[1]),
                Path::new(&args[2]),
                ledger,
            )?;
            println!(
                "compared={} unexplained={} suppressed={}",
                report.compared, report.unexplained, report.suppressed
            );
            for (kind, count) in &report.by_kind {
                println!("kind={kind} differences={count}");
            }
            for (field, count) in &report.by_field {
                println!("field={field} differences={count}");
            }
            for diff in &report.diffs {
                println!("kind={} key={} path={}", diff.kind, diff.key, diff.path);
            }
            for (id, count) in &report.applied_ids {
                println!("suppression={id} count={count}");
            }
            for (field, count) in &report.coverage {
                println!("coverage={field} count={count}");
            }
            println!("Coverage is reported separately; matching exports do not establish complete capture.");
            Ok(i32::from(report.unexplained != 0))
        }
        Some("make-demo") if args.len() == 2 => {
            maxed_cli::demo::write_demo(Path::new(&args[1]))?;
            Ok(0)
        }
        Some("--help" | "-h") if args.len() == 1 => {
            println!("{USAGE}");
            Ok(0)
        }
        Some("record") if args.len() == 2 => {
            let stop = Arc::new(AtomicBool::new(false));
            let signal = stop.clone();
            handler_installed(ctrlc::set_handler(move || {
                signal.store(true, Ordering::Release)
            }))?;
            record(
                Path::new(&args[1]),
                maxed_capture::CaptureConfig::default(),
                &stop,
            )?;
            Ok(0)
        }
        Some("replay") if args.len() == 2 => {
            let report = replay(Path::new(&args[1]))?;
            println!("{report}");
            if report.truncated {
                eprintln!("Truncated final pcap record; complete prefix processed");
            }
            Ok(i32::from(report.truncated))
        }
        Some("decode")
            if args.len() == 6 && args[2] == "--output" && args[4] == "--explain-failures" =>
        {
            let report = maxed_cli::decode::decode_with_explanations(
                Path::new(&args[1]),
                Path::new(&args[3]),
                Some(Path::new(&args[5])),
            )?;
            eprintln!("{report}");
            if report.truncated {
                eprintln!("Truncated final pcap record; complete prefix processed");
            }
            Ok(i32::from(report.truncated))
        }
        Some("decode") if args.len() == 3 || (args.len() == 4 && args[2] == "--output") => {
            let output = if args.len() == 3 { &args[2] } else { &args[3] };
            let report = decode(Path::new(&args[1]), Path::new(output))?;
            eprintln!("{report}");
            if report.truncated {
                eprintln!("Truncated final pcap record; complete prefix processed");
            }
            Ok(i32::from(report.truncated))
        }
        Some("scrub")
            if args.len() == 3 || (args.len() == 4 && args[3] == "--allow-undecodable") =>
        {
            let options = maxed_cli::scrub::ScrubOptions {
                allow_undecodable: args.len() == 4,
            };
            match maxed_cli::scrub::scrub_with_options(
                Path::new(&args[1]),
                Path::new(&args[2]),
                options,
            ) {
                Ok(report) => {
                    println!(
                        "frames={} changed_spans={} verified={} provisional={} filled_unclassified={} pseudonymized={} kept_allowlisted={}",
                        report.frames, report.changed_spans, report.verified,
                        report.diagnostics.count(maxed_cli::scrub::ScrubReason::UndecodableTyped) > 0,
                        report.diagnostics.field_count("filled-unclassified"),
                        report.diagnostics.field_count("pseudonymized"),
                        report.diagnostics.field_count("kept-allowlisted")
                    );
                    if !report.diagnostics.packets.is_empty()
                        || !report.diagnostics.fields.is_empty()
                    {
                        println!("{}", report.diagnostics);
                    }
                    if report
                        .diagnostics
                        .count(maxed_cli::scrub::ScrubReason::UndecodableTyped)
                        > 0
                    {
                        println!("Provisional fixture: typed decode failures were filled; filled packets cannot validate decoding of those packets.");
                    }
                    Ok(if report.verified { 0 } else { 2 })
                }
                Err(error) => {
                    eprintln!("{error}");
                    Ok(2)
                }
            }
        }
        Some("parity") if args.len() >= 3 => {
            let mut ledger: Option<PathBuf> = None;
            let mut fixture: Option<String> = None;
            let mut limit = 20;
            let mut seen_limit = false;
            let mut i = 3;
            while i < args.len() {
                let value = args.get(i + 1).ok_or(CliError::InvalidArguments)?;
                match args[i].to_str() {
                    Some("--ledger") if ledger.is_none() => ledger = Some(PathBuf::from(value)),
                    Some("--fixture-sha256") if fixture.is_none() => {
                        fixture = Some(value.to_str().ok_or(CliError::InvalidArguments)?.into())
                    }
                    Some("--limit") if !seen_limit => {
                        limit = value
                            .to_str()
                            .ok_or(CliError::InvalidArguments)?
                            .parse()
                            .map_err(|_| CliError::InvalidArguments)?;
                        seen_limit = true;
                    }
                    _ => return Err(CliError::InvalidArguments),
                }
                i += 2;
            }
            let (report, ids) = parity_with_fixture(
                Path::new(&args[1]),
                Path::new(&args[2]),
                ledger.as_deref(),
                limit,
                fixture.as_deref(),
            )?;
            println!(
                "compared={} unexplained={} suppressed={}",
                report.compared, report.unexplained, report.suppressed
            );
            for (name, count) in &report.by_type {
                println!("type={name} differences={count}");
            }
            for diff in &report.diffs {
                println!(
                    "dir={} ordinal={} id={} path={}",
                    diff.dir,
                    diff.ordinal,
                    diff.id
                        .map_or_else(|| "missing".into(), |id| id.to_string()),
                    diff.path
                );
            }
            for (id, count) in ids {
                println!("suppression={id} count={count}");
            }
            Ok(if report.unexplained == 0 { 0 } else { 1 })
        }
        _ => Err(CliError::InvalidArguments),
    }
}

fn handler_installed(result: Result<(), ctrlc::Error>) -> Result<(), CliError> {
    result.map_err(|_| {
        CliError::SignalHandler(std::io::Error::other("Could not install Ctrl+C handler"))
    })
}

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            if matches!(error, CliError::InvalidArguments) {
                eprintln!("{USAGE}");
            }
            // Display is deliberately sanitized; never print the anyhow source chain.
            let error = anyhow::Error::new(error);
            eprintln!("{error}");
            2
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_install_failure_is_an_io_error_with_accurate_context() {
        let error = handler_installed(Err(ctrlc::Error::MultipleHandlers)).unwrap_err();
        assert_eq!(error.to_string(), "Could not install Ctrl+C handler");
        assert!(
            matches!(error, CliError::SignalHandler(source) if source.kind() == std::io::ErrorKind::Other)
        );
        assert!(handler_installed(Ok(())).is_ok());
    }
}
