//! estate#406: explicit cleaning of legacy memory/knowledge stores, using temporary files only.
use std::path::{Path, PathBuf};
use std::process::Command;

use wicked_estate::{RedactionOutcome, ensure_redacted};
use wicked_estate_core::{GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, Symbol};
use wicked_estate_store::{GraphStoreMutExt, MemStore, SqliteStore};

const TEXT: &str = "fixture DB_PASSWORD=hunter2-example AKIAIOSFODNN7EXAMPLE";

fn old_store(path: &Path) {
    let mut store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    let mut node = Node::new(
        Symbol::synthetic("mem", "legacy").id(),
        NodeKind::Other("memory".into()),
        "fixture",
        Language::new("memory"),
        Location::new("mem", Span::ZERO),
    );
    node.metadata.insert("content".into(), TEXT.into());
    store.upsert_nodes(&[node.clone()]).unwrap();
    drop(store);
    // Simulate an old writer: bypass today's write backstop, including the contribution copy.
    node.name = TEXT.into();
    node.metadata.insert(
        "payload".into(),
        serde_json::json!({"content": "password: \"hunter2-example\""})
            .to_string()
            .into(),
    );
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "UPDATE nodes SET name=?1, data=?2",
        rusqlite::params![TEXT, serde_json::to_string(&node).unwrap()],
    )
    .unwrap();
    conn.execute(
        "UPDATE node_files SET data=?1",
        [serde_json::to_string(&node).unwrap()],
    )
    .unwrap();
    conn.execute("UPDATE nodes_fts SET name=?1", [TEXT])
        .unwrap();
    conn.execute("DELETE FROM meta WHERE k='text_redaction_epoch'", [])
        .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM nodes_fts WHERE nodes_fts MATCH 'AKIAIOSFODNN7EXAMPLE'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
}

fn assert_clean(path: &Path) {
    for path in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
    ] {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("read fixture: {e}"),
        };
        for value in ["hunter2-example", "AKIAIOSFODNN7EXAMPLE"] {
            assert!(!bytes.windows(value.len()).any(|w| w == value.as_bytes()));
        }
    }
    let conn = rusqlite::Connection::open(path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM nodes_fts WHERE nodes_fts MATCH 'AKIAIOSFODNN7EXAMPLE'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "the old FTS segments must be gone");
}

#[test]
fn ensure_redacted_cleans_text_even_with_current_code_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    old_store(&path);
    let mut store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    let RedactionOutcome::Migrated(report) = ensure_redacted(&mut store).unwrap() else {
        panic!("legacy text must migrate despite the fresh code epoch");
    };
    assert!(report.nodes_rewritten > 0);
    assert!(report.wal_truncated);
    let node = store
        .get_node(&Symbol::synthetic("mem", "legacy").id())
        .unwrap()
        .unwrap();
    assert!(!node.name.contains("hunter2-example"));
    assert_clean(&path);
    assert_eq!(
        ensure_redacted(&mut store).unwrap(),
        RedactionOutcome::Current
    );
    drop(store);
    assert_clean(&path);
}

#[test]
fn redact_store_cli_cleans_text_and_second_run_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge.db");
    old_store(&path);
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
            .args(["redact-store", "--db", path.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let output = String::from_utf8(first.stdout).unwrap();
    assert!(output.contains("node rows rewritten:"));
    assert!(!output.contains("hunter2-example"));
    assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
    assert_clean(&path);
    let second = run();
    assert!(second.status.success());
    assert!(
        String::from_utf8(second.stdout)
            .unwrap()
            .contains("already at the current")
    );
}

#[test]
fn unsupported_text_migration_is_an_error() {
    let mut store = MemStore::new();
    store
        .upsert_nodes(&[Node::new(
            Symbol::synthetic("mem", "legacy").id(),
            NodeKind::Other("memory".into()),
            TEXT,
            Language::new("memory"),
            Location::new("mem", Span::ZERO),
        )])
        .unwrap();
    store.meta_set_key("redaction_epoch", "1");
    assert!(ensure_redacted(&mut store).is_err());
}
