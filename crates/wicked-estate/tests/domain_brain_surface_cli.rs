//! CLI integration tests for the two net-new read surfaces the Domain-Brain build depends on
//! (`.product/DES-DOMAIN-BRAIN-CONTRACT.md`, Contract 2 §2 and §4):
//!
//! 1. `clusters --json --summary` — per-community `id` + FULL `members: [symbol_id]` list. Brain's
//!    domain-model engine attaches a domain label to EVERY member, so the summary read must carry
//!    the whole membership, not just the ≤5 `label_candidates`. This test locks that shape so the
//!    JSON cannot silently drop `members` again (the MCP `Communities` tool omits it — §2).
//! 2. `resolve <name> [--file F] [--kind K] --json` → `[{symbol_id, name, kind, file, line}]` — the
//!    first-class name→SymbolId surface (§4 #2) that a write path's precondition depends on.
//!
//! These drive the compiled binary as a subprocess against a temp on-disk DB, exercising the real
//! flag-parse → store-read → JSON-shape wiring — the exact seam brain mocks with a fake client.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_dbsurf_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

/// Run `wicked-estate <args> --db <db>`; assert success; return stdout.
fn run(cwd: &Path, db: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new(bin());
    cmd.current_dir(cwd);
    cmd.args(args);
    cmd.args(["--db", db.to_str().unwrap()]);
    let out = cmd.output().expect("spawn wicked-estate");
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

/// Index `files` (relative path → contents) under a fresh dir; return (dir, db_path).
fn index_files(tag: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let dir = fresh_dir(tag);
    for (rel, contents) in files {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, contents).unwrap();
    }
    let db = dir.join("g.db");
    run(&dir, &db, &["index", dir.to_str().unwrap()]);
    (dir, db)
}

// ── Surface 1: clusters --json --summary carries id + full members ────────────────────────────

#[test]
fn clusters_json_summary_emits_id_and_full_members() {
    // A densely connected call graph so Louvain finds a community with size >= 2.
    let src = "\
fn a() { b(); c(); }
fn b() { c(); a(); }
fn c() { a(); b(); }
";
    let (dir, db) = index_files("clusters_members", &[("src/a.rs", src)]);

    let stdout = run(&dir, &db, &["clusters", "--json", "--summary"]);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("clusters --json --summary is JSON");
    let arr = v.as_array().expect("top-level array of community objects");
    assert!(!arr.is_empty(), "at least one community detected: {stdout}");

    for (i, community) in arr.iter().enumerate() {
        // Contract §2 shape: {id, size, members, label_candidates, dominant_files, modularity_contribution}.
        assert_eq!(
            community["id"].as_u64(),
            Some(i as u64),
            "community `id` is the positional (largest-first) index"
        );

        let members = community["members"]
            .as_array()
            .expect("`members` is present and an array (Contract 2 §2 — the load-bearing field)");
        let size = community["size"].as_u64().expect("`size` present") as usize;
        assert_eq!(
            members.len(),
            size,
            "`members` is the FULL membership, not a ≤5 truncation: len must equal `size`"
        );
        assert!(size >= 2, "min_size default is 2");
        for m in members {
            assert!(m.is_string(), "each member is a SymbolId string, got {m}");
        }

        // label_candidates is the ≤5 top-PageRank subset — must NOT be confused with members.
        let labels = community["label_candidates"]
            .as_array()
            .expect("`label_candidates` present");
        assert!(labels.len() <= 5, "label_candidates is capped at 5");
        assert!(
            labels.len() <= members.len(),
            "label_candidates is a subset of members"
        );
        assert!(
            community["dominant_files"].is_array(),
            "dominant_files present"
        );
        assert!(
            community["modularity_contribution"].is_number(),
            "modularity_contribution present"
        );
    }
}

#[test]
fn clusters_json_summary_members_are_resolvable_symbol_ids() {
    // Every member SymbolId in the summary must round-trip through `annotations --symbol <id>`
    // (i.e. it is a real interned id, not a display name) — brain writes annotations keyed on it.
    let src = "\
fn a() { b(); c(); }
fn b() { c(); a(); }
fn c() { a(); b(); }
";
    let (dir, db) = index_files("clusters_resolvable", &[("src/a.rs", src)]);

    let v: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["clusters", "--json", "--summary"])).unwrap();
    let first = &v.as_array().unwrap()[0];
    let member = first["members"].as_array().unwrap()[0]
        .as_str()
        .unwrap()
        .to_string();

    // The id resolves: `annotations --symbol <id> --json` returns a `symbol` field echoing it.
    let detail = run(&dir, &db, &["annotations", "--symbol", &member, "--json"]);
    let dv: serde_json::Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(
        dv["symbol"].as_str(),
        Some(member.as_str()),
        "clusters member SymbolId is a real interned id usable on the --symbol write/read path"
    );
}

// ── Surface 2: resolve <name> [--file F] [--kind K] --json ────────────────────────────────────

#[test]
fn resolve_json_returns_symbol_id_name_kind_file_line() {
    let (dir, db) = index_files("resolve_basic", &[("src/a.rs", "fn target() {}\n")]);

    let stdout = run(&dir, &db, &["resolve", "target", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("resolve --json is JSON");
    let arr = v.as_array().expect("resolve --json is an array");
    assert_eq!(arr.len(), 1, "exactly one symbol named `target`: {stdout}");

    let r = &arr[0];
    // Contract §4 #2 shape: {symbol_id, name, kind, file, line}.
    assert!(
        r["symbol_id"].as_str().is_some_and(|s| !s.is_empty()),
        "symbol_id is a non-empty string"
    );
    assert_eq!(r["name"], "target");
    assert_eq!(r["kind"], "Function");
    assert!(
        r["file"].as_str().is_some_and(|s| s.ends_with("a.rs")),
        "file points at the source: {r}"
    );
    assert!(r["line"].as_u64().is_some(), "line is a number");

    // The resolved id must match what `nodes --json` reports for the same symbol (single source of truth).
    let nodes: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["nodes", "--json"])).unwrap();
    let node = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "target")
        .expect("target node present in nodes --json");
    assert_eq!(
        r["symbol_id"], node["symbol_id"],
        "resolve returns the same SymbolId as nodes --json (one identity authority)"
    );
}

#[test]
fn resolve_json_empty_for_unknown_name() {
    let (dir, db) = index_files("resolve_unknown", &[("src/a.rs", "fn target() {}\n")]);
    let v: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["resolve", "does_not_exist", "--json"])).unwrap();
    assert_eq!(
        v.as_array().unwrap().len(),
        0,
        "unknown name resolves to an empty array (not an error, not a fabricated id)"
    );
}

#[test]
fn resolve_kind_filter_narrows_results() {
    // Two symbols share the name `Thing`: a struct and a function. --kind disambiguates.
    let src = "\
struct Thing { x: i32 }
fn Thing() {}
";
    let (dir, db) = index_files("resolve_kind", &[("src/a.rs", src)]);

    // No filter → both name matches present.
    let all: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["resolve", "Thing", "--json"])).unwrap();
    assert!(
        all.as_array().unwrap().len() >= 2,
        "both `Thing` symbols resolve without a kind filter: {all}"
    );

    // --kind struct → only the struct (case-insensitive against the Debug kind form).
    let only: serde_json::Value = serde_json::from_str(&run(
        &dir,
        &db,
        &["resolve", "Thing", "--kind", "struct", "--json"],
    ))
    .unwrap();
    let structs = only.as_array().unwrap();
    assert!(!structs.is_empty(), "the struct resolves");
    for r in structs {
        assert_eq!(r["kind"], "Struct", "kind filter keeps only structs");
    }
}

#[test]
fn resolve_file_filter_narrows_to_one_location() {
    // Same name `dup` defined in two files; --file selects one location's SymbolId.
    let (dir, db) = index_files(
        "resolve_file",
        &[
            ("src/one.rs", "fn dup() {}\n"),
            ("src/two.rs", "fn dup() {}\n"),
        ],
    );

    let all: serde_json::Value =
        serde_json::from_str(&run(&dir, &db, &["resolve", "dup", "--json"])).unwrap();
    assert_eq!(
        all.as_array().unwrap().len(),
        2,
        "both `dup` definitions resolve without a file filter"
    );

    // Read one match's exact `file` value, then filter by it — deterministic, path-format agnostic.
    let target_file = all.as_array().unwrap()[0]["file"]
        .as_str()
        .unwrap()
        .to_string();
    let only: serde_json::Value = serde_json::from_str(&run(
        &dir,
        &db,
        &["resolve", "dup", "--file", &target_file, "--json"],
    ))
    .unwrap();
    let filtered = only.as_array().unwrap();
    assert_eq!(
        filtered.len(),
        1,
        "file filter narrows to a single location"
    );
    assert_eq!(filtered[0]["file"], target_file);
}
