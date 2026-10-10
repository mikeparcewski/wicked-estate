//! `wicked-estate-bench` binary entry point (W1.6 / W8.1).
//!
//! All benchmark logic lives in [`wicked_estate_bench::capability`]; this file is intentionally thin.
//!
//! Usage:
//!   wicked-estate-bench [--no-report] [path ...]   capability benchmark (index speed, footprint,
//!                                                   resolution diagnostics); JSON on stdout
//!   wicked-estate-bench --recall                    memory recall@5 gate (exit 1 below the gate)
//!
//! With no paths, benchmarks the workspace root. `--no-report` skips rewriting the committed
//! Markdown report. An unknown flag or a path that does not exist is a usage error (exit 2), and a
//! requested repo that fails to benchmark fails the run (exit 1) — a receipt never silently drops
//! an input (BENCH-02). There is no golden-set / agent A/B runner here.

use std::path::PathBuf;

use anyhow::Result;
use wicked_estate_bench::capability::{print_summary_table, run_benchmark};
use wicked_estate_bench::memory_recall::{GATE, run_memory_recall_bench};

fn main() -> Result<()> {
    // Hermeticity pin (ADR-010 / D16): the bench is the repo's truth oracle, and a plugin
    // override on the dev machine would silently move built-in-language baselines. Pin the
    // plugins dir to a fresh empty temp dir BEFORE any indexing — unconditional, so bench
    // numbers never depend on who runs them. This binary is its own process, so setting the
    // env before the first registry access is OnceLock-safe.
    let plugin_pin = std::env::temp_dir().join(format!("we-bench-plugins-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&plugin_pin);
    // SAFETY: single-threaded at this point — first statement of main, before any spawn.
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &plugin_pin);
    }
    eprintln!("bench: WICKED_ESTATE_PLUGINS pinned to empty dir for hermetic baselines");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage =
        "usage: wicked-estate-bench [--no-report] [path ...]\n       wicked-estate-bench --recall";
    let mut recall = false;
    let mut write_report = true;
    let mut path_args: Vec<PathBuf> = Vec::new();
    for a in &args {
        match a.as_str() {
            "--recall" => recall = true,
            "--no-report" => write_report = false,
            "-h" | "--help" => {
                println!("{usage}");
                return Ok(());
            }
            flag if flag.starts_with('-') => {
                eprintln!("{usage}\nunknown flag {flag:?}");
                std::process::exit(2);
            }
            path => {
                let p = PathBuf::from(path);
                if !p.exists() {
                    eprintln!("{usage}\npath does not exist: {}", p.display());
                    std::process::exit(2);
                }
                path_args.push(p);
            }
        }
    }
    if recall && (!path_args.is_empty() || !write_report) {
        eprintln!("{usage}\n--recall takes no paths and no --no-report");
        std::process::exit(2);
    }

    if recall {
        let k = 5;
        eprintln!("wicked-estate-bench: running memory recall@{k} benchmark ...");
        let report = run_memory_recall_bench(k)?;
        eprintln!(
            "recall@{k}: {}/{} = {:.2} (gate >= {GATE}) — {}",
            report.hits,
            report.total,
            report.recall_at_k,
            if report.pass { "PASS" } else { "FAIL" },
        );
        println!("{}", serde_json::to_string_pretty(&report)?);
        if !report.pass {
            std::process::exit(1);
        }
        return Ok(());
    }

    let paths: Vec<PathBuf> = if path_args.is_empty() {
        let defaults = default_paths();
        if defaults.is_empty() {
            eprintln!("No paths provided and no defaults found on disk.");
            eprintln!("Pass one or more repo paths as arguments:");
            eprintln!(
                "  cargo run -p wicked-estate-bench --bin wicked-estate-bench -- /path/to/repo"
            );
            std::process::exit(2);
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
        path_args
    };

    eprintln!();
    let report = run_benchmark(&paths, write_report)?;
    print_summary_table(&report.repos);

    // Machine-readable JSON to stdout.
    println!("{}", serde_json::to_string_pretty(&report)?);

    // Corpus admission: every requested repo must be measured.
    if report.repos.len() != paths.len() {
        eprintln!(
            "wicked-estate-bench: measured {} of {} requested repo(s) — failing the run",
            report.repos.len(),
            paths.len()
        );
        std::process::exit(1);
    }
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
