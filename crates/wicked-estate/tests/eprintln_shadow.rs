//! estate#333 (wicked-core#957): std's `eprintln!` panics when stderr refuses the write, and a Node
//! host hands the process a non-blocking stderr pipe — the thread that logs dies. Every LIBRARY crate
//! a host can link shadows the macro at its root (see e.g. wicked-estate-store's lib.rs). This guard
//! fails when a library crate's sources call `eprintln!` without that shadow IN SCOPE (defined before
//! the root's first `mod`, uncommented, gated only by `cfg(not(test))`), so a new crate, a new call or
//! a moved shadow cannot bring the panic back. Binaries (`main.rs` and the modules only it declares)
//! and benchmark crates keep std's macro.

use std::fs;
use std::path::{Path, PathBuf};

const SHADOW: &str = "macro_rules! eprintln {";
/// Not linked by any host: benchmark harness only.
const EXEMPT: &[&str] = &["wicked-estate-bench"];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            // `src/bin/*` are separate binary crates.
            if p.file_name().is_some_and(|n| n == "bin")
                && p.parent().is_some_and(|d| d.ends_with("src"))
            {
                continue;
            }
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `mod x;` declarations (any visibility) in a crate root, by name.
fn declared_mods(root: &str) -> Vec<String> {
    root.lines()
        .filter_map(|l| {
            let t = l.trim();
            let t = t
                .strip_prefix("pub(crate) ")
                .or_else(|| t.strip_prefix("pub "))
                .unwrap_or(t);
            t.strip_prefix("mod ")
                .and_then(|r| r.strip_suffix(';'))
                .map(|m| m.trim().to_owned())
        })
        .collect()
}

/// The modules ONLY `main.rs` declares are the binary's; one `lib.rs` also declares is the library's.
fn bin_only_modules(src: &Path, lib: &str) -> Vec<String> {
    let Ok(main) = fs::read_to_string(src.join("main.rs")) else {
        return Vec::new();
    };
    let in_lib = declared_mods(lib);
    declared_mods(&main)
        .into_iter()
        .filter(|m| !in_lib.contains(m))
        .collect()
}

/// A call of the macro, however spaced (`eprintln!(`, `eprintln !(`), outside a line comment.
fn calls_eprintln(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("//") {
        return false;
    }
    let mut rest = t;
    while let Some(i) = rest.find("eprintln") {
        if rest[i + "eprintln".len()..].trim_start().starts_with('!') {
            return true;
        }
        rest = &rest[i + 1..];
    }
    false
}

/// The shadow is live at the crate root: defined on an uncommented line, gated only by
/// `cfg(not(test))` (optionally with the features its calls need), and BEFORE the first `mod`
/// declaration (textual macro scope).
fn shadow_in_scope(lib: &str) -> bool {
    let lines: Vec<&str> = lib.lines().collect();
    let Some(at) = lines
        .iter()
        .position(|l| l.trim_start().starts_with(SHADOW))
    else {
        return false;
    };
    let first_mod = lines.iter().position(|l| {
        let t = l.trim_start();
        let t = t
            .strip_prefix("pub(crate) ")
            .or_else(|| t.strip_prefix("pub "))
            .unwrap_or(t);
        t.starts_with("mod ")
    });
    let gated_ok = lines[..at]
        .iter()
        .rev()
        .take_while(|l| l.trim_start().starts_with("#["))
        // Gated only off unit tests, optionally narrowed to the features its calls need.
        .all(|l| !l.contains("cfg(") || l.contains("not(test)"));
    gated_ok && first_mod.is_none_or(|m| at < m)
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
        let lib = fs::read_to_string(src.join("lib.rs")).unwrap();
        let bin = bin_only_modules(&src, &lib);
        let mut files = Vec::new();
        rs_files(&src, &mut files);
        let logs = files.iter().any(|f| {
            let stem = f.file_stem().unwrap().to_string_lossy();
            let binary = f.file_name().is_some_and(|n| n == "main.rs")
                || (f.parent() == Some(src.as_path()) && bin.iter().any(|m| *m == stem));
            !binary && fs::read_to_string(f).unwrap().lines().any(calls_eprintln)
        });
        if logs && !shadow_in_scope(&lib) {
            missing.push(name);
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "library crates calling eprintln! without the estate#333 shadow in scope: {missing:?}"
    );
}

#[test]
fn the_scope_check_rejects_a_shadow_that_does_not_cover_the_modules() {
    let ok = "//! doc\n#[cfg(not(test))]\n#[allow(unused_macros)]\nmacro_rules! eprintln {\n}\npub mod a;\n";
    assert!(shadow_in_scope(ok));
    assert!(
        !shadow_in_scope("pub mod a;\n#[cfg(not(test))]\nmacro_rules! eprintln {\n}\n"),
        "below a mod"
    );
    assert!(
        !shadow_in_scope("// macro_rules! eprintln {\npub mod a;\n"),
        "commented out"
    );
    assert!(
        !shadow_in_scope("#[cfg(feature = \"x\")]\nmacro_rules! eprintln {\n}\npub mod a;\n"),
        "feature-gated"
    );
    assert!(calls_eprintln("    eprintln !(\"x\");"));
    assert!(!calls_eprintln("    // eprintln!(\"x\");"));
    let lib = "pub mod shared;\n";
    assert!(declared_mods(lib).contains(&"shared".to_owned()));
}
