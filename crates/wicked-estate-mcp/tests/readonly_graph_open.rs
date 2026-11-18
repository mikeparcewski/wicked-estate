//! wicked-crew#1497: a `--readonly` server opens its graph `SQLITE_OPEN_READONLY`.
//!
//! A launcher hands a worker a graph it may read and must not change, in a directory the worker
//! cannot write. Before, the read-only server still opened the graph like a writer (WAL pragma,
//! schema DDL), so in such a directory every pooled connection failed and the server answered
//! "no readable graph" for a graph that was right there, and in a writable directory it quietly
//! switched a DELETE-journal graph to WAL. These drive the REAL binary over stdio.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use wicked_estate_core::{GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId};
use wicked_estate_store::SqliteStore;

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpChild {
    /// The server over `graph`, with every other store pointed into `side` (empty: served in memory).
    fn spawn(graph: &Path, side: &Path, extra_args: &[&str]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
        cmd.args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("WICKED_ESTATE_DB", graph)
            .env("WICKED_HOME", side)
            .env("WICKED_MEMORY_DB", side.join("memory.db"))
            .env("WICKED_KNOWLEDGE_DB", side.join("knowledge.db"))
            .env("WICKED_XEDGE_DB", side.join("xedge.db"))
            .env_remove("WICKED_OTEL_ENDPOINT")
            .env_remove("WICKED_STORE_URL")
            .env_remove("WICKED_RUNTIME");
        let mut child = cmd.spawn().expect("spawn wicked-estate-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut me = McpChild {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        };
        let init = me.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "readonly-graph-open-test", "version": "0" }
            }),
        );
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        me
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{req}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).unwrap();
        assert!(n > 0, "server closed stdout");
        serde_json::from_str(line.trim()).unwrap()
    }

    /// `tools/call` → the tool's JSON payload.
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(resp["result"]["isError"], json!(false), "{resp}");
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn finish(self) {
        let McpChild {
            mut child, stdin, ..
        } = self;
        drop(stdin);
        child.wait().ok();
    }
}

/// A graph holding one function, `create_order`, left in `journal` mode with no sidecar files.
fn seed(db: &Path, journal: &str) {
    {
        let mut store = SqliteStore::open(db).unwrap();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[Node::new(
                SymbolId("rust . . . src/lib.rs/create_order().".into()),
                NodeKind::Function,
                "create_order",
                Language::new("rust"),
                Location::new("src/lib.rs", Span::ZERO),
            )])
            .unwrap();
        store.commit_batch().unwrap();
    }
    let conn = rusqlite::Connection::open(db).unwrap();
    let mode: String = conn
        .query_row(&format!("PRAGMA journal_mode={journal}"), [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, journal, "seed journal mode");
}

fn journal_mode(db: &Path) -> String {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap()
}

#[cfg(unix)]
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn finds_create_order(mcp: &mut McpChild) {
    let hit = mcp.tool("SearchEntity", json!({"name": "create_order"}));
    assert_eq!(hit["resolved"], json!(true), "{hit}");
    assert_eq!(hit["matches"][0]["name"], json!("create_order"), "{hit}");
}

/// The graph a worker is handed read-only: a DELETE-journal graph in a directory nobody can write.
#[cfg(unix)]
#[test]
fn a_delete_journal_graph_in_a_read_only_directory_answers_and_is_left_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("graph");
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("code-graph.db");
    seed(&db, "delete");
    let lock = dir.join("code-graph.db.writer.lock");
    let _ = std::fs::remove_file(&lock);
    let before_entries = entries(&dir);
    let before_bytes = std::fs::read(&db).unwrap();
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let mut mcp = McpChild::spawn(&db, side.path(), &["--readonly"]);
    let hit = mcp.tool("SearchEntity", json!({"name": "create_order"}));
    let br = mcp.tool("BlastRadius", json!({"symbol": "create_order"}));
    mcp.finish();

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(hit["resolved"], json!(true), "the graph must answer: {hit}");
    assert_eq!(br["resolved"], json!(true), "{br}");
    assert_eq!(
        entries(&dir),
        before_entries,
        "nothing created beside the graph"
    );
    assert!(
        std::fs::read(&db).unwrap() == before_bytes,
        "the graph's bytes are unchanged"
    );
    assert_eq!(journal_mode(&db), "delete");
}

/// SAYS-VS-DOES: `--readonly` writes nothing even where it could: a DELETE-journal graph in a
/// writable directory is not switched to WAL and gains no sidecar.
#[test]
fn a_read_only_server_never_converts_or_writes_a_writable_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let db = tmp.path().join("code-graph.db");
    seed(&db, "delete");
    let before_bytes = std::fs::read(&db).unwrap();

    let mut mcp = McpChild::spawn(&db, side.path(), &["--readonly"]);
    finds_create_order(&mut mcp);
    // A miss too: a read-only server logs no miss and caches no response into the graph.
    let miss = mcp.tool("SearchEntity", json!({"name": "creat_ordr"}));
    assert_eq!(miss["resolved"], json!(false), "{miss}");
    mcp.finish();

    assert_eq!(
        journal_mode(&db),
        "delete",
        "a read-only server switched the graph to WAL"
    );
    assert!(
        std::fs::read(&db).unwrap() == before_bytes,
        "a read-only server wrote the graph"
    );
    assert!(!tmp.path().join("code-graph.db-wal").exists());
}

/// A WAL graph (what a writer leaves) still answers read-only, without its database file being
/// written; a write-mode server still opens its graph as a writer (it switches a DELETE graph to WAL).
/// SQLite may create a WAL graph's `-shm` beside it for a reader, so only the database file is
/// compared here.
#[test]
fn a_wal_graph_still_answers_read_only_and_a_writer_still_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let db = tmp.path().join("estate.db");
    seed(&db, "wal");
    let before = std::fs::read(&db).unwrap();

    let mut ro = McpChild::spawn(&db, side.path(), &["--readonly"]);
    finds_create_order(&mut ro);
    ro.finish();
    assert!(
        std::fs::read(&db).unwrap() == before,
        "a read-only server wrote a WAL graph's database file"
    );
    assert_eq!(journal_mode(&db), "wal");

    let writable = tmp.path().join("writer.db");
    seed(&writable, "delete");
    let mut rw = McpChild::spawn(&writable, side.path(), &[]);
    finds_create_order(&mut rw);
    rw.finish();
    assert_eq!(
        journal_mode(&writable),
        "wal",
        "a write-mode server opens its graph as a writer, as before"
    );
}

/// SAYS-VS-DOES and SILENT-OK: a graph written by an older build is REFUSED, not migrated and not
/// answered from: every graph tool names the reason (initialize and tools/list carry it too), and
/// the graph is byte-identical afterwards. Its owner migrates it on its next write.
#[test]
fn a_graph_from_an_older_build_is_refused_with_its_reason_and_never_written() {
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let db = tmp.path().join("estate.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/estate_v0120.db"),
        &db,
    )
    .unwrap();
    // Delivered as crew delivers a graph: DELETE journal mode (the fixture is WAL, and SQLite may create a WAL
    // graph's sidecars even for a read-only reader). The schema is untouched: a journal switch migrates nothing.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
    }
    let before = std::fs::read(&db).unwrap();
    let before_mode = journal_mode(&db);

    let mut mcp = McpChild::spawn(&db, side.path(), &["--readonly"]);
    let list = mcp.request("tools/list", json!({}));
    let resp = mcp.request(
        "tools/call",
        json!({ "name": "SearchEntity", "arguments": { "name": "anything" } }),
    );
    // The domain read that checks code links against the graph is refused too (codex r2 #1).
    let neighbors = mcp.request(
        "tools/call",
        json!({ "name": "knowledge.neighbors", "arguments": { "id": "sym_x", "store": "estate" } }),
    );
    mcp.finish();

    let tools = list["result"]["tools"].as_array().unwrap();
    for name in ["SearchEntity", "SemanticSearch"] {
        assert!(
            tools.iter().any(|t| t["name"] == json!(name)),
            "{name} stays advertised (HIDE): {list}"
        );
    }
    let n_err = neighbors["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("knowledge.neighbors must be refused: {neighbors}"));
    assert!(n_err.contains("refused"), "{n_err}");
    let reason = list["result"]["_meta"]["wicked-estate/graph"]["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("tools/list names why the graph is refused: {list}"));
    assert!(reason.contains("schema predates this build"), "{reason}");
    let err = resp["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("{resp}"));
    assert!(
        err.contains("refused") && err.contains("re-index or migrate"),
        "{err}"
    );
    assert!(
        std::fs::read(&db).unwrap() == before,
        "a read-only server wrote an old graph"
    );
    assert_eq!(journal_mode(&db), before_mode);
    for side_file in ["estate.db-wal", "estate.db-shm", "estate.db-journal"] {
        assert!(!tmp.path().join(side_file).exists(), "{side_file} created");
    }
}

/// codex r2 #5: SQLite opens a `file:` URI itself, so a read-only server refuses one rather than
/// let a writer open reach the graph. The graph is byte-identical and keeps its journal mode.
#[test]
fn a_file_uri_graph_is_refused_by_a_read_only_server_and_never_written() {
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let db = tmp.path().join("estate.db");
    seed(&db, "delete");
    let before = std::fs::read(&db).unwrap();
    let uri = std::path::PathBuf::from(format!("file:{}", db.display()));

    let mut mcp = McpChild::spawn(&uri, side.path(), &["--readonly"]);
    let resp = mcp.request(
        "tools/call",
        json!({ "name": "SearchEntity", "arguments": { "name": "alpha" } }),
    );
    mcp.finish();

    let err = resp["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("a file: URI graph must be refused: {resp}"));
    assert!(err.contains("file:") && err.contains("refused"), "{err}");
    assert!(
        std::fs::read(&db).unwrap() == before,
        "the graph was written"
    );
    assert_eq!(journal_mode(&db), "delete");
    for side_file in ["estate.db-wal", "estate.db-shm", "estate.db-journal"] {
        assert!(!tmp.path().join(side_file).exists(), "{side_file} created");
    }
}

/// wicked-estate#491: refusing an older WAL graph (the fixture as an older build left it, no
/// sidecars) creates no `-wal` or `-shm` beside it: the currency check reads the database file
/// alone through an `immutable` connection before SQLite's WAL machinery touches the directory.
#[test]
fn refusing_an_older_wal_graph_creates_no_file_beside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let db = tmp.path().join("estate.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/estate_v0120.db"),
        &db,
    )
    .unwrap();
    let head = std::fs::read(&db).unwrap();
    assert_eq!(head[18], 2, "the fixture is a WAL graph");
    let before = head;

    let mut mcp = McpChild::spawn(&db, side.path(), &["--readonly"]);
    let resp = mcp.request(
        "tools/call",
        json!({ "name": "SearchEntity", "arguments": { "name": "anything" } }),
    );
    mcp.finish();

    let err = resp["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("an older WAL graph must be refused: {resp}"));
    assert!(err.contains("refused"), "{err}");
    assert!(
        std::fs::read(&db).unwrap() == before,
        "the graph was written"
    );
    for side_file in ["estate.db-wal", "estate.db-shm", "estate.db-journal"] {
        assert!(!tmp.path().join(side_file).exists(), "{side_file} created");
    }
}

/// wicked-estate#491: a graph the server cannot reach (its directory may not be searched) is
/// refused with the reason, never served as an empty graph that answers "nothing found".
#[cfg(unix)]
#[test]
fn a_graph_behind_an_unsearchable_directory_is_refused_not_served_empty() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let side = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("locked");
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("estate.db");
    seed(&db, "delete");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    // A process that can search it anyway (root) cannot test this.
    let searchable = std::fs::metadata(&db).is_ok();

    let mut mcp = McpChild::spawn(&db, side.path(), &["--readonly"]);
    let resp = mcp.request(
        "tools/call",
        json!({ "name": "SearchEntity", "arguments": { "name": "alpha" } }),
    );
    mcp.finish();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if searchable {
        eprintln!("skipped: this process can search a 0000 directory");
        return;
    }

    let err = resp["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("an unreachable graph must be refused, not empty: {resp}"));
    assert!(err.contains("refused"), "{err}");
}
