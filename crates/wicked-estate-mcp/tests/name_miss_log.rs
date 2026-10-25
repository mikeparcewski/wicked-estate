//! wicked-estate#358 — the MCP host logs every symbol-name miss to `search_misses`.
//!
//! The log is written in `main.rs`'s request loop (the tools only hold `&dyn GraphRead`), so
//! these tests drive the REAL binary over stdio. A miss — fresh or replayed from the response
//! cache — writes one row under the server's session id; a hit writes nothing; a `--readonly`
//! server writes nothing.

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
    fn spawn(dir: &Path, extra_args: &[&str]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
        cmd.args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("WICKED_ESTATE_DB", dir.join("estate.db"))
            .env("WICKED_MEMORY_DB", dir.join("memory.db"))
            .env("WICKED_KNOWLEDGE_DB", dir.join("knowledge.db"))
            .env("WICKED_XEDGE_DB", dir.join("xedge.db"))
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
                "clientInfo": { "name": "name-miss-log-test", "version": "0" }
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

/// A graph holding one function, `create_order`.
fn seed(dir: &Path) {
    let mut store = SqliteStore::open(dir.join("estate.db")).unwrap();
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

fn misses(dir: &Path) -> Vec<(String, Option<String>)> {
    SqliteStore::open_readonly(dir.join("estate.db"))
        .unwrap()
        .search_misses(100, None)
        .unwrap()
        .into_iter()
        .map(|m| (m.query, m.session_id))
        .collect()
}

#[test]
fn every_miss_is_logged_under_the_session_and_a_hit_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let mut mcp = McpChild::spawn(tmp.path(), &[]);

    let hit = mcp.tool("SearchEntity", json!({"name": "create_order"}));
    assert_eq!(hit["resolved"], json!(true), "{hit}");

    let miss = mcp.tool("SearchEntity", json!({"name": "creat_ordr"}));
    assert_eq!(miss["resolved"], json!(false), "{miss}");
    assert_eq!(
        miss["suggestions"][0]["name"],
        json!("create_order"),
        "{miss}"
    );
    // Byte-identical arguments: served from the response cache, still a miss to log.
    let again = mcp.tool("SearchEntity", json!({"name": "creat_ordr"}));
    assert_eq!(again["resolved"], json!(false));
    let br = mcp.tool("BlastRadius", json!({"symbol": "Create_Order"}));
    assert_eq!(br["resolved"], json!(false), "{br}");
    mcp.finish();

    let rows = misses(tmp.path());
    let queries: Vec<&str> = rows.iter().map(|(q, _)| q.as_str()).collect();
    assert_eq!(queries.len(), 3, "{rows:?}");
    assert_eq!(queries.iter().filter(|q| **q == "creat_ordr").count(), 2);
    assert!(queries.contains(&"Create_Order"));
    let sessions: std::collections::HashSet<_> = rows.iter().map(|(_, s)| s.clone()).collect();
    assert_eq!(sessions.len(), 1, "one server = one session: {rows:?}");
    assert!(
        rows[0].1.as_deref().is_some_and(|s| s.starts_with("mcp-")),
        "{rows:?}"
    );
}

#[test]
fn a_readonly_server_reports_the_miss_but_logs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let mut mcp = McpChild::spawn(tmp.path(), &["--readonly"]);
    let miss = mcp.tool("SearchEntity", json!({"name": "creat_ordr"}));
    assert_eq!(miss["resolved"], json!(false), "{miss}");
    mcp.finish();
    assert!(misses(tmp.path()).is_empty());
}

#[test]
fn name_miss_query_reads_only_a_miss() {
    let wrap = |content: Value| {
        json!({"jsonrpc": "2.0", "id": 1, "result": {
            "content": [{"type": "text", "text": content.to_string()}], "isError": false
        }})
    };
    assert_eq!(
        wicked_estate_mcp::name_miss_query(&wrap(json!({"resolved": false, "query": "x"}))),
        Some("x".to_string())
    );
    assert_eq!(
        wicked_estate_mcp::name_miss_query(&wrap(json!({"resolved": true, "query": "x"}))),
        None
    );
    assert_eq!(
        wicked_estate_mcp::name_miss_query(&wrap(json!({"matches": []}))),
        None
    );
    assert_eq!(wicked_estate_mcp::name_miss_query(&json!(null)), None);
}
