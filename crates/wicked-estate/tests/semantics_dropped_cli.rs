//! #438: a re-index keeps the semantic fields of every symbol it emits again, and says so on
//! stderr (`SEMANTICS-DROPPED`) for the ones it cannot keep.

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

#[test]
fn reindex_reports_dropped_symbols_and_keeps_the_rest() {
    let dir = std::env::temp_dir().join(format!("ci_semdrop_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.rs");
    fs::write(&file, "pub fn keep() {}\npub fn gone() {}\n").unwrap();
    let db = dir.join("g.db");
    let db = db.to_str().unwrap();
    assert!(run(&dir, &["index", ".", "--db", db]).status.success());

    for name in ["keep", "gone"] {
        let sym = symbol_of(&dir, db, name);
        let out = run(
            &dir,
            &[
                "semantics",
                &sym,
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
    let keep = symbol_of(&dir, db, "keep");

    fs::write(&file, "pub fn keep() {}\n").unwrap();
    let out = run(&dir, &["index", ".", "--db", db]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.lines()
            .any(|l| l == "SEMANTICS-DROPPED: 1 symbol(s) in a.rs"),
        "stderr: {err}"
    );

    let shown = run(&dir, &["semantics", &keep, "--db", db]);
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(text.contains("REQ-1") && text.contains("alice"), "{text}");
}
