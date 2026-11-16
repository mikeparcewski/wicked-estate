// SPDX-License-Identifier: MIT
//! #438: a re-index keeps the semantic fields of every symbol it emits again, and says so on
//! stderr (`SEMANTICS-DROPPED`) for the ones it cannot keep — including a symbol lost when a whole
//! annotated file is deleted (a deletion-only run).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn")
}

fn symbol_of(cwd: &Path, db: &str, name: &str) -> String {
    let out = run(cwd, &["resolve", name, "--json", "--db", db]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("resolve json");
    v[0].get("symbol_id")
        .or_else(|| v[0].get("symbol"))
        .and_then(|s| s.as_str())
        .expect("symbol id")
        .to_string()
}

fn annotate(cwd: &Path, db: &str, sym: &str) {
    let out = run(
        cwd,
        &[
            "semantics",
            sym,
            "--requirement",
            "REQ-1",
            "--validated",
            "true",
            "--validated-by",
            "alice",
            "--db",
            db,
        ],
    );
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn reindex_reports_dropped_symbols_and_keeps_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let file = dir.join("a.rs");
    fs::write(&file, "pub fn keep() {}\npub fn gone() {}\n").unwrap();
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    assert!(run(dir, &["index", ".", "--db", db]).status.success());

    for name in ["keep", "gone"] {
        annotate(dir, db, &symbol_of(dir, db, name));
    }
    let keep = symbol_of(dir, db, "keep");

    fs::write(&file, "pub fn keep() {}\n").unwrap();
    let out = run(dir, &["index", ".", "--db", db]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.lines()
            .any(|l| l == "SEMANTICS-DROPPED: 1 symbol(s) in a.rs"),
        "stderr: {err}"
    );

    let shown = run(dir, &["semantics", &keep, "--db", db]);
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(text.contains("REQ-1") && text.contains("alice"), "{text}");
}

/// Deleting a whole annotated file is a deletion-only run (nothing else changed): it must still
/// remove the file inside the write batch and report every annotated symbol it drops. On the
/// pre-fix code a deletion-only run returned early and printed nothing.
#[test]
fn deleting_an_annotated_file_reports_dropped_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    fs::write(dir.join("a.rs"), "pub fn keep() {}\n").unwrap();
    fs::write(dir.join("b.rs"), "pub fn doomed() {}\n").unwrap();
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    assert!(run(dir, &["index", ".", "--db", db]).status.success());
    annotate(dir, db, &symbol_of(dir, db, "doomed"));

    // Delete b.rs only; a.rs is byte-for-byte unchanged → a deletion-only re-index.
    fs::remove_file(dir.join("b.rs")).unwrap();
    let out = run(dir, &["index", ".", "--db", db]);
    assert!(out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.lines()
            .any(|l| l == "SEMANTICS-DROPPED: 1 symbol(s) in b.rs"),
        "stderr: {err}"
    );
}
