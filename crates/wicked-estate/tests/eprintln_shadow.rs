//! estate#333 (wicked-core#957): std's `eprintln!` panics when stderr refuses the write, and a Node
//! host hands the process a non-blocking stderr pipe — the thread that logs dies. Every LIBRARY crate
//! a host can link shadows the macro at its root (see e.g. wicked-estate-store's lib.rs). This guard
//! fails when a library crate's sources call `eprintln!` without that shadow, so a new crate or a new
//! call cannot bring the panic back. Binaries (`main.rs` and the modules only it declares) and
//! benchmark crates keep std's macro.

use std::fs;
use std::path::{Path, PathBuf};

const SHADOW: &str = "macro_rules! eprintln {";
/// Not linked by any host: benchmark harness only.
const EXEMPT: &[&str] = &["wicked-estate-bench"];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "bin") {
                continue;
            }
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The modules `main.rs` declares are the binary's, not the library's.
fn bin_modules(src: &Path) -> Vec<String> {
    let Ok(main) = fs::read_to_string(src.join("main.rs")) else {
        return Vec::new();
    };
    main.lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("mod ")
                .and_then(|r| r.strip_suffix(';'))
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn every_library_crate_that_logs_to_stderr_shadows_eprintln() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut missing = Vec::new();
    for e in fs::read_dir(crates).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let src = e.path().join("src");
        if EXEMPT.contains(&name.as_str()) || !src.join("lib.rs").exists() {
            continue;
        }
        let bin = bin_modules(&src);
        let mut files = Vec::new();
        rs_files(&src, &mut files);
        let logs = files.iter().any(|f| {
            let stem = f.file_stem().unwrap().to_string_lossy();
            if f.file_name().is_some_and(|n| n == "main.rs")
                || (f.parent() == Some(src.as_path()) && bin.iter().any(|m| *m == stem))
            {
                return false;
            }
            fs::read_to_string(f).unwrap().lines().any(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && t.contains("eprintln!")
            })
        });
        if logs
            && !fs::read_to_string(src.join("lib.rs"))
                .unwrap()
                .contains(SHADOW)
        {
            missing.push(name);
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "library crates calling eprintln! without the estate#333 shadow: {missing:?}"
    );
}
