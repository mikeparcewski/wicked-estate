//! estate#595: a test build (`--features codegraph-fixture`) writes a `--codegraph` store from a
//! recorded codegraph database, through the real `index` route, so consumers' CI reads a store
//! shaped exactly as the import writes it. `WICKED_ESTATE_CODEGRAPH_RECORDED` names the SQL; the
//! run says so on stderr (`CODEGRAPH-FIXTURE:`). Release builds leave the hook out, and
//! `scripts/smoke-binary.py` checks the shipped binaries for it.
#![cfg(feature = "codegraph-fixture")]

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use wicked_estate_core::GraphRead;
use wicked_estate_mcp::{McpContext, handle_request_unified};
use wicked_estate_store::SqliteStore;

const HELPER: &str = "ts-typescript . . . src/lib/util/helper().";
const GET: &str = "ts-typescript . . . src/svc/UserService#get().";
/// Minted by the import: estate's Java extractor has no node for a record.
const POINT: &str = "ts-java . . . src/p/Shapes/Shapes#Point#";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codegraph")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

fn run(tmp: &Path, args: &[&std::ffi::OsStr]) -> Output {
    let home = tmp.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(tmp.join("no-plugins")).unwrap();
    Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .args(args)
        .env("WICKED_HOME", &home)
        // No ambient extraction plugin changes what the recorded facts map onto.
        .env("WICKED_ESTATE_PLUGINS", tmp.join("no-plugins"))
        .env_remove("WICKED_ESTATE_PLUGIN_OVERRIDE")
        .env(
            "WICKED_ESTATE_CODEGRAPH_RECORDED",
            fixture_dir().join("codegraph-1.6.4.sql"),
        )
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-codegraph-recorded",
        )
        .env("WICKED_ESTATE_EMIT_DEADLETTER", tmp.join("emit.ndjson"))
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .env_remove("WICKED_OTEL_ENDPOINT")
        .output()
        .unwrap()
}

/// The fixture repo indexed with `--codegraph` and the recorded database: the store's path.
fn indexed(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    copy(&fixture_dir().join("repo"), &repo);
    let db = tmp.join("g.db");
    // No bundle is read: the recorded database stands in for the run.
    let bundle = tmp.join("no-bundle");
    let out = run(
        tmp,
        &[
            "index".as_ref(),
            repo.as_os_str(),
            "--db".as_ref(),
            db.as_os_str(),
            "--codegraph".as_ref(),
            bundle.as_os_str(),
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "index failed: {stderr}");
    assert!(
        stderr.contains("CODEGRAPH-FIXTURE:"),
        "the run says its facts are recorded: {stderr}"
    );
    assert!(
        !stderr.contains("CODEGRAPH-FALLBACK"),
        "no fallback to estate's pipeline: {stderr}"
    );
    db
}

fn tool(store: &SqliteStore, name: &str, args: Value) -> String {
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": args },
    });
    let resp = handle_request_unified(store, &req, &McpContext::default(), None, None);
    assert_eq!(resp["result"]["isError"], json!(false), "{name}: {resp}");
    resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_test_build_writes_a_codegraph_store_from_recorded_sql() {
    let tmp = tempfile::tempdir().unwrap();
    let db = indexed(tmp.path());
    let store = SqliteStore::open(&db).unwrap();
    let bound = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by.starts_with("codegraph:"))
        .count();
    assert!(bound > 0, "the store holds codegraph-bound edges");
}

/// Estate's own readers on that store: blast-radius follows a codegraph-bound call, and
/// ContextBundle and FetchContent answer for a node only the import made.
#[test]
fn estate_readers_answer_on_the_recorded_codegraph_store() {
    let tmp = tempfile::tempdir().unwrap();
    let db = indexed(tmp.path());

    let out = run(
        tmp.path(),
        &[
            "blast-radius".as_ref(),
            "--id".as_ref(),
            HELPER.as_ref(),
            "--json".as_ref(),
            "--db".as_ref(),
            db.as_os_str(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    let dependents: Vec<&str> = doc["dependents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert!(dependents.contains(&GET), "helper <- get: {doc}");

    let store = SqliteStore::open(&db).unwrap();
    assert!(
        store
            .get_node(&wicked_estate_core::SymbolId(POINT.into()))
            .unwrap()
            .is_some(),
        "the import minted Point"
    );
    let bundle = tool(&store, "ContextBundle", json!({ "symbol": POINT }));
    assert!(
        bundle.contains(POINT),
        "ContextBundle seeds on Point: {bundle}"
    );
    // The node, with no error, and its source: a minted node has a byte span (estate#590).
    let content: Value =
        serde_json::from_str(&tool(&store, "FetchContent", json!({ "symbol": POINT }))).unwrap();
    assert_eq!(content["found"], json!(true), "{content}");
    assert_eq!(content["symbol"], json!(POINT), "{content}");
    assert_eq!(content["name"], json!("Point"), "{content}");
    assert_eq!(
        content["source"],
        json!("record Point(int a) { int g() { return a; } }"),
        "{content}"
    );
}
