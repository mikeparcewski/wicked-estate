//! wicked-estate#351 — the CLI states the same per-query freshness as MCP.
//!
//! The CLI bridge (`traverse`) and the MCP server both call `freshness::FreshnessProbe::annotate`
//! after the tool runs. These tests pin the parity (same `freshness` block for the same request)
//! and the CLI-only surfaces: `blast-radius --json` and its text rows, and `stats --json`'s
//! working-tree counters.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use wicked_estate_core::{GraphRead, SymbolQuery};
use wicked_estate_mcp::{McpContext, annotate_tool_response, handle_request_unified};
use wicked_estate_store::SqliteStore;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const LIB: &str = "pub fn leaf() {}\n\npub fn caller() {\n    leaf();\n}\n";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?}");
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .args(args)
        .env("WICKED_ESTATE_EMIT_PROGRAM", "wicked-bus-absent-freshness")
        .env("WICKED_ESTATE_EMIT_DEADLETTER", dir.join("emit.ndjson"))
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env_remove("WICKED_OTEL_ENDPOINT")
        .env_remove("WICKED_OTEL_HEADERS")
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .output()
        .expect("spawn wicked-estate")
}

fn indexed(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_fresh_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let s = Scratch(d);
    fs::create_dir_all(s.join("src")).unwrap();
    fs::write(s.join("src/lib.rs"), LIB).unwrap();
    fs::write(s.join(".gitignore"), "*.db*\nemit.ndjson\n").unwrap();
    git(&s, &["init", "-q", "."]);
    git(&s, &["add", "-A"]);
    git(&s, &["commit", "-qm", "base"]);
    let out = run(&s, &["index", ".", "--db", "graph.db"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    s
}

fn leaf_id(dir: &Path) -> String {
    let store = SqliteStore::open(dir.join("graph.db")).unwrap();
    store
        .find_symbols(&SymbolQuery {
            exact_name: Some("leaf".into()),
            ..Default::default()
        })
        .unwrap()[0]
        .symbol
        .0
        .clone()
}

fn json_out(dir: &Path, args: &[&str]) -> (Value, String) {
    let out = run(dir, args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        serde_json::from_slice(&out.stdout).expect("one JSON document"),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The MCP server's path, in process: the handler, then the probe the request loop runs.
fn mcp_traverse(dir: &Path, id: &str) -> Value {
    let db = dir.join("graph.db");
    let store = SqliteStore::open(&db).unwrap();
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "TraverseGraph",
                    "arguments": {"symbol": id, "direction": "dependents", "depth": 4} },
    });
    let mut resp = handle_request_unified(&store, &req, &McpContext::default(), None, None);
    let mut probe = wicked_estate::freshness::FreshnessProbe::open(db.to_str().unwrap());
    annotate_tool_response(&mut resp, "TraverseGraph", &mut probe);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap()
}

#[test]
fn cli_and_mcp_state_the_same_freshness() {
    let s = indexed("parity");
    let id = leaf_id(&s);
    let cli_args = [
        "traverse",
        id.as_str(),
        "--direction",
        "dependents",
        "--depth",
        "4",
        "--json",
        "--db",
        "graph.db",
    ];
    let (cli, _) = json_out(&s, &cli_args);
    assert_eq!(cli["freshness"], mcp_traverse(&s, &id)["freshness"]);
    assert_eq!(cli["freshness"]["commits_behind"], json!(0), "{cli}");

    // Edited: both transports mark the same file, the same way.
    fs::write(s.join("src/lib.rs"), format!("{LIB}// edited\n")).unwrap();
    let (cli, err) = json_out(&s, &cli_args);
    let mcp = mcp_traverse(&s, &id);
    assert_eq!(cli["freshness"], mcp["freshness"]);
    assert_eq!(
        cli["freshness"]["stale_files"],
        json!([{"file": "src/lib.rs", "status": "modified"}])
    );
    assert!(err.contains("differ from the index"), "{err}");
    assert!(
        !err.contains(wicked_estate_retrieve::STALENESS_PLACEHOLDER),
        "{err}"
    );
}

#[test]
fn blast_radius_marks_drifted_rows_in_json_and_text() {
    let s = indexed("br");
    fs::write(s.join("src/lib.rs"), format!("{LIB}// edited\n")).unwrap();
    let (doc, err) = json_out(&s, &["blast-radius", "leaf", "--json", "--db", "graph.db"]);
    let rows = doc["dependents"].as_array().unwrap();
    assert!(!rows.is_empty(), "{doc}");
    assert!(
        rows.iter().all(|r| r["stale"] == json!("modified")),
        "{doc}"
    );
    assert_eq!(doc["freshness"]["stale_files_total"], json!(1));
    assert!(err.contains("STALENESS:"), "{err}");
    let out = run(&s, &["blast-radius", "leaf", "--db", "graph.db"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("[stale: modified]"), "{text}");
    assert!(text.contains("differ from the index"), "{text}");
}

#[test]
fn stats_json_counts_dirty_and_unindexed_files() {
    let s = indexed("stats");
    let (doc, _) = json_out(&s, &["stats", "--json", "--db", "graph.db"]);
    assert_eq!(doc["staleness"]["dirty_indexed_files"], json!(0), "{doc}");
    assert_eq!(doc["staleness"]["unindexed_files"], json!(0), "{doc}");
    fs::write(s.join("src/lib.rs"), format!("{LIB}// edited\n")).unwrap();
    fs::write(s.join("src/new.py"), "def f():\n    pass\n").unwrap();
    fs::write(s.join("notes.txt"), "not source\n").unwrap();
    // A source file inside a NEW directory counts too (git reports `? newmod/` unless asked
    // for every untracked file — #372 review).
    fs::create_dir_all(s.join("newmod")).unwrap();
    fs::write(s.join("newmod/caller.rs"), "pub fn c() { }\n").unwrap();
    let (doc, _) = json_out(&s, &["stats", "--json", "--db", "graph.db"]);
    assert_eq!(doc["staleness"]["dirty_indexed_files"], json!(1), "{doc}");
    assert_eq!(doc["staleness"]["unindexed_files"], json!(2), "{doc}");
    // Every text path states this answer's freshness — a name miss included (#372 review).
    let out = run(
        &s,
        &["blast-radius", "no_such_symbol_xyz", "--db", "graph.db"],
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("no symbol named"), "{text}");
    assert!(
        text.contains("2 new source file(s) not in the graph"),
        "{text}"
    );
}
