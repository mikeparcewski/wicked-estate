//! Regression tests for issue #102 — the MCP response cache must never serve stale
//! memory/knowledge results.
//!
//! The server caches `tools/call` responses in two levels (L1 process HashMap plus the L2
//! `cache` table in the GRAPH store), both invalidated only by graph-store changes (the
//! `graph_version` and a graph.db mtime watch). Before the fix, `memory.*` / `knowledge.*`
//! responses entered
//! that cache too, so a `memory.capture` / `knowledge.ingest` never invalidated a previously
//! cached `memory.recall` / `knowledge.recall` with identical args — recalls were stale
//! forever, and repeated write-shaped calls were silently swallowed (never reached the
//! engine).
//!
//! These tests spawn the REAL binary over stdio (the cache lives in `main.rs`'s request
//! loop; in-process `handle_request_unified` tests bypass it — which is exactly how the bug
//! escaped) against scratch stores in a tempdir.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

// ── MCP subprocess wrapper (mirrors tests/env_vars.rs) ───────────────────────

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpChild {
    fn spawn_with_envs(binary: &str, envs: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(binary);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("failed to spawn wicked-estate-mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        McpChild {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{req}").expect("write to child stdin");
        self.stdin.flush().expect("flush child stdin");
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .expect("read from child stdout");
        assert!(
            n > 0,
            "server stdout closed before a response arrived (process may have crashed)"
        );
        serde_json::from_str(line.trim())
            .unwrap_or_else(|e| panic!("server sent invalid JSON ({e})\nraw: {line:?}"))
    }

    /// `tools/call` returning the tool's inner JSON payload (parsed from content[0].text).
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert!(
            resp.get("error").is_none(),
            "tool {name} returned JSON-RPC error: {resp}"
        );
        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(false),
            "tool {name} returned isError=true: {resp}"
        );
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tool {name}: no content[0].text in {resp}"));
        serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("tool {name}: inner payload not JSON ({e}): {text}"))
    }

    fn finish(self) {
        let McpChild {
            mut child,
            stdin,
            reader,
            ..
        } = self;
        drop(stdin);
        drop(reader);
        child.kill().ok();
        child.wait().ok();
    }
}

fn spawn_all_domains(tmp: &tempfile::TempDir) -> McpChild {
    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let mut mcp = McpChild::spawn_with_envs(
        binary,
        &[
            (
                "WICKED_ESTATE_DB",
                tmp.path().join("estate.db").to_str().unwrap(),
            ),
            (
                "WICKED_MEMORY_DB",
                tmp.path().join("memory.db").to_str().unwrap(),
            ),
            (
                "WICKED_KNOWLEDGE_DB",
                tmp.path().join("knowledge.db").to_str().unwrap(),
            ),
            (
                "WICKED_XEDGE_DB",
                tmp.path().join("xedge.db").to_str().unwrap(),
            ),
        ],
    );
    let init = mcp.request(
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "cache-staleness-test", "version": "0" }
        }),
    );
    assert!(init.get("result").is_some(), "initialize failed: {init}");
    mcp
}

// ── #102 regression: capture → recall → capture NEW → identical recall sees it ──

/// The exact broken sequence from the garden context stack (hooks capture, then recall):
/// a second `memory.recall` with byte-identical arguments MUST reflect a `memory.capture`
/// that happened in between. Before the fix the second recall was served from the response
/// cache (keyed on the graph version, which memory writes never bump) and the new memory
/// was invisible forever.
#[test]
fn memory_capture_between_identical_recalls_is_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = spawn_all_domains(&tmp);

    let capture_a = mcp.tool(
        "memory.capture",
        json!({ "content": "the deploy target is the blue cluster", "kind": "fact", "tier": "semantic" }),
    );
    assert!(
        capture_a.get("memory_id").is_some(),
        "capture A must return memory_id: {capture_a}"
    );

    // Identical args for both recalls — this is what keys the response cache.
    let recall_args = json!({ "query": "deploy target cluster", "token_budget": 2000 });

    let first = mcp.tool("memory.recall", recall_args.clone());
    let first_items = first["items"].as_array().expect("items array").clone();
    assert!(
        first_items
            .iter()
            .any(|i| i["content"].as_str().unwrap_or("").contains("blue")),
        "first recall must see the blue-cluster memory: {first_items:?}"
    );

    // Capture a NEW memory, then repeat the recall with IDENTICAL args.
    let capture_b = mcp.tool(
        "memory.capture",
        json!({ "content": "the deploy target moved to the green cluster", "kind": "fact", "tier": "semantic" }),
    );
    let new_id = capture_b["memory_id"]
        .as_str()
        .expect("capture B must return memory_id")
        .to_string();

    let second = mcp.tool("memory.recall", recall_args);
    let second_items = second["items"].as_array().expect("items array");
    assert!(
        second_items
            .iter()
            .any(|i| i["memory_id"].as_str() == Some(new_id.as_str())),
        "#102: identical recall after a new capture MUST include the new memory {new_id}; \
         got items: {second_items:?}"
    );

    mcp.finish();
}

/// Same shape for the knowledge domain: ingest → recall → ingest NEW doc → identical recall
/// must surface the newly ingested content.
#[test]
fn knowledge_ingest_between_identical_recalls_is_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = spawn_all_domains(&tmp);

    mcp.tool(
        "knowledge.ingest",
        json!({
            "title": "Rate limiting v1",
            "chunks": ["The API rate limit is 100 requests per second."],
            "scope": "project:api",
            "source": "docs/rate-limit-v1.md"
        }),
    );

    let recall_args =
        json!({ "query": "API rate limit requests per second", "token_budget": 2000 });

    let first = mcp.tool("knowledge.recall", recall_args.clone());
    assert!(
        first["items"]
            .as_array()
            .expect("items array")
            .iter()
            .any(|i| i["body_snippet"].as_str().unwrap_or("").contains("100")),
        "first recall must see the v1 chunk: {first}"
    );

    mcp.tool(
        "knowledge.ingest",
        json!({
            "title": "Rate limiting v2",
            "chunks": ["The API rate limit was raised to 500 requests per second."],
            "scope": "project:api",
            "source": "docs/rate-limit-v2.md"
        }),
    );

    let second = mcp.tool("knowledge.recall", recall_args);
    assert!(
        second["items"]
            .as_array()
            .expect("items array")
            .iter()
            .any(|i| i["body_snippet"].as_str().unwrap_or("").contains("500")),
        "#102: identical recall after a new ingest MUST surface the v2 chunk; got: {second}"
    );

    mcp.finish();
}

/// Write-shaped domain tools must EXECUTE every time. Before the fix, a repeated
/// `memory.capture` / `knowledge.ingest` with identical args was answered from the response
/// cache — the write never reached the engine (observed as accidental "ingest dedup" in the
/// S3 bench harness; that dedup is explicitly re-homed to callers/engine consolidation).
#[test]
fn repeated_identical_writes_reach_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = spawn_all_domains(&tmp);

    let args = json!({ "content": "idempotency probe", "kind": "fact", "tier": "semantic" });
    let id1 = mcp.tool("memory.capture", args.clone())["memory_id"]
        .as_str()
        .expect("memory_id")
        .to_string();
    let id2 = mcp.tool("memory.capture", args)["memory_id"]
        .as_str()
        .expect("memory_id")
        .to_string();
    assert_ne!(
        id1, id2,
        "#102: a second identical memory.capture must create a second memory, \
         not be swallowed by the response cache"
    );

    let ingest_args =
        json!({ "title": "probe doc", "chunks": ["probe chunk"], "scope": "", "source": "" });
    let doc1 = mcp.tool("knowledge.ingest", ingest_args.clone())["doc_id"]
        .as_str()
        .expect("doc_id")
        .to_string();
    let doc2 = mcp.tool("knowledge.ingest", ingest_args)["doc_id"]
        .as_str()
        .expect("doc_id")
        .to_string();
    assert_ne!(
        doc1, doc2,
        "#102: a second identical knowledge.ingest must reach the engine (uuid-v7 identity)"
    );

    mcp.finish();
}

/// The graph-tool response cache must SURVIVE the fix: after a graph tool call its response
/// is persisted in the graph store's L2 `cache` table, while domain tool calls leave no
/// cache rows. Asserted directly against the on-disk store (WAL allows a concurrent reader).
#[test]
fn graph_tools_still_cached_domain_tools_never() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = spawn_all_domains(&tmp);

    let graph_args = json!({ "name": "nonexistent_symbol_xyz" });
    // Graph tool call — response may be an isError result on an empty graph; the cache
    // stores the response either way, so don't assert on payload success here.
    let resp = mcp.request(
        "tools/call",
        json!({ "name": "SearchEntity", "arguments": graph_args }),
    );
    assert!(
        resp.get("result").is_some() || resp.get("error").is_some(),
        "SearchEntity must respond: {resp}"
    );

    let recall_args = json!({ "query": "anything at all", "token_budget": 2000 });
    mcp.tool("memory.recall", recall_args.clone());
    mcp.tool("knowledge.recall", recall_args.clone());
    mcp.finish();

    // Inspect the graph store's L2 cache directly. Keys are "{tool}/{args_json}" with args
    // serialized exactly as the server does (Value::to_string on params.arguments).
    let store =
        wicked_estate_store::SqliteStore::open(tmp.path().join("estate.db").to_str().unwrap())
            .expect("open estate store");
    let graph_key = format!("SearchEntity/{graph_args}");
    assert!(
        store.cache_get(&graph_key).expect("cache_get").is_some(),
        "graph tool response must still be cached under {graph_key:?} (#102 must not \
         disable graph-tool caching)"
    );
    for tool in ["memory.recall", "knowledge.recall"] {
        let key = format!("{tool}/{recall_args}");
        assert!(
            store.cache_get(&key).expect("cache_get").is_none(),
            "#102: domain tool response must NEVER be cached, found L2 row for {key:?}"
        );
    }
}
