//! FINDING-077 (#79): a requirement cannot be marked validated by nobody.
//!
//! `requirement_validated` was a bare flag. A caller could assert that a symbol satisfies its
//! requirement with nothing recording who decided that, so a consumer's evaluator≠creator rule —
//! "structurally can't self-grade" — had nothing to check. wicked-core#131 observed the consequence
//! at scale: 46 distinct strings written as the requirement of 34,897 nodes, every one
//! self-validated, coverage 1.0, every gate green, and the resulting "requirements" were file-name
//! titles over reference lists.
//!
//! The store's job is not to decide whether a self-validated claim is acceptable — that is the
//! consumer's policy. Its job is to make the question answerable.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn indexed(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ci_valauth_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/a.rs"), "pub fn target() {}\n").unwrap();
    let db = dir.join("g.db");
    let out = Command::new(bin())
        .current_dir(&dir)
        .args(["index", "."])
        .args(["--db", db.to_str().unwrap()])
        .output()
        .expect("spawn");
    assert!(out.status.success(), "index failed: {out:?}");
    (dir, db)
}

fn semantics(cwd: &Path, db: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .arg("semantics")
        .args(args)
        .args(["--db", db.to_str().unwrap()])
        .output()
        .expect("spawn")
}

/// The symbol id the indexer minted for `target()`, read back from the graph.
fn target_symbol(cwd: &Path, db: &Path) -> String {
    let out = Command::new(bin())
        .current_dir(cwd)
        .args(["resolve", "target", "--json"])
        .args(["--db", db.to_str().unwrap()])
        .output()
        .expect("spawn");
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    v.get(0)
        .and_then(|e| e.get("symbol_id").or_else(|| e.get("symbol")))
        .and_then(|s| s.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| panic!("could not resolve `target` from: {text}"))
}

#[test]
fn validating_without_naming_an_actor_is_refused() {
    let (dir, db) = indexed("noactor");
    let sym = target_symbol(&dir, &db);
    let out = semantics(
        &dir,
        &db,
        &[&sym, "--requirement", "REQ-1", "--validated", "true"],
    );
    assert!(
        !out.status.success(),
        "a requirement was marked validated with no actor named — the claim this refuses to accept"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("--validated-by"),
        "the refusal must say how to comply, not just that it refused: {err}"
    );
}

#[test]
fn a_named_actor_is_stored_and_read_back() {
    let (dir, db) = indexed("named");
    let sym = target_symbol(&dir, &db);
    let set = semantics(
        &dir,
        &db,
        &[
            &sym,
            "--requirement",
            "REQ-1",
            "--validated",
            "true",
            "--validated-by",
            "reviewer@example.invalid",
        ],
    );
    assert!(set.status.success(), "set failed: {set:?}");

    let show = semantics(&dir, &db, &[&sym]);
    let text = String::from_utf8_lossy(&show.stdout);
    assert!(
        text.contains("reviewer@example.invalid"),
        "the validating actor must be readable back — a claim you cannot attribute is the defect: {text}"
    );
}

/// Setting a requirement WITHOUT validating it needs no actor: nothing is being asserted true.
#[test]
fn recording_a_requirement_alone_needs_no_actor() {
    let (dir, db) = indexed("reqonly");
    let sym = target_symbol(&dir, &db);
    let out = semantics(&dir, &db, &[&sym, "--requirement", "REQ-1"]);
    assert!(
        out.status.success(),
        "recording a requirement is not a validation claim and must not demand an actor: {out:?}"
    );
}
