//! estate#595: a test build (`--features codegraph-fixture`) writes a `--codegraph` store from a
//! recorded codegraph database, through the real `index` route, so consumers' CI reads a store
//! shaped exactly as the import writes it. `WICKED_ESTATE_CODEGRAPH_RECORDED` names the SQL; the
//! run says so on stderr (`CODEGRAPH-FIXTURE:`). Release builds leave the hook out, and
//! `scripts/smoke-binary.py` checks the shipped binaries for it.

use std::path::{Path, PathBuf};
use std::process::Command;
use wicked_estate_core::GraphRead;
use wicked_estate_store::SqliteStore;

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

#[test]
fn a_test_build_writes_a_codegraph_store_from_recorded_sql() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    copy(&fixture_dir().join("repo"), &repo);
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let db = tmp.path().join("g.db");
    // No bundle is read: the recorded database stands in for the run.
    let bundle = tmp.path().join("no-bundle");
    let out = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .arg("index")
        .arg(&repo)
        .arg("--db")
        .arg(&db)
        .arg("--codegraph")
        .arg(&bundle)
        .env("WICKED_HOME", &home)
        .env(
            "WICKED_ESTATE_CODEGRAPH_RECORDED",
            fixture_dir().join("codegraph-1.6.4.sql"),
        )
        .env_remove("WICKED_ESTATE_DB")
        .output()
        .unwrap();
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
    let store = SqliteStore::open(&db).unwrap();
    let bound = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by.starts_with("codegraph:"))
        .count();
    assert!(bound > 0, "the store holds codegraph-bound edges");
}
