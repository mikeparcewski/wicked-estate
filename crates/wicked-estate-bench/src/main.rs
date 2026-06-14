//! `wicked-estate-bench` binary entry point (W1.6 / W8.1).
//!
//! All benchmark logic lives in [`wicked_estate_bench::capability`]; this file is intentionally thin.
//!
//! Usage:
//!   cargo run -p wicked-estate-bench --bin wicked-estate-bench -- [path ...]
//!
//! With no arguments, benchmarks the workspace root. Pass one or more repo paths as arguments to
//! benchmark additional repositories.

use std::path::PathBuf;

use anyhow::Result;
use wicked_estate_bench::capability::{print_summary_table, run_benchmark};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let paths: Vec<PathBuf> = if args.is_empty() {
        let defaults = default_paths();
        if defaults.is_empty() {
            eprintln!("No paths provided and no defaults found on disk.");
            eprintln!("Pass one or more repo paths as arguments:");
            eprintln!(
                "  cargo run -p wicked-estate-bench --bin wicked-estate-bench -- /path/to/repo"
            );
            return Ok(());
        }
        eprintln!(
            "wicked-estate-bench: using {} default repo(s)",
            defaults.len()
        );
        for p in &defaults {
            eprintln!("  - {}", p.display());
        }
        defaults
    } else {
        args.iter()
            .map(PathBuf::from)
            .filter(|p| {
                if p.exists() {
                    true
                } else {
                    eprintln!("WARN: path does not exist, skipping: {}", p.display());
                    false
                }
            })
            .collect()
    };

    eprintln!();
    let report = run_benchmark(&paths, /* write_report = */ true)?;
    print_summary_table(&report.repos);

    // Machine-readable JSON to stdout.
    println!("{}", serde_json::to_string_pretty(&report)?);

    Ok(())
}

/// Build the default repo set when no paths are given: just the workspace root. Pass additional
/// repo paths as CLI arguments to benchmark them.
fn default_paths() -> Vec<PathBuf> {
    // wicked-estate-bench Cargo.toml is at crates/wicked-estate-bench; the workspace root is two levels up.
    let ws_root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    vec![ws_root].into_iter().filter(|p| p.exists()).collect()
}
