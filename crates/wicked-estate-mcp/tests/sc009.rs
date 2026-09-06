//! SC-009: Integration test — all 4 v0.12.x fixture databases opened in a single unified call.
//!
//! Covers:
//! - SC-009: Opens all 4 fixture DBs and verifies 4 read operations each return non-empty results.
//! - DB compatibility (DoD §2.6): Each v0.12.x fixture opens without error (no migration required).
//! - Tools count: `tools/list` with all 4 stores open returns exactly 24 tools
//!   (11 estate + 6 memory + 7 knowledge; SemanticSearch absent — no embedder meta in fixtures).

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use wicked_estate_core::RetrievalTool;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_overlay::XedgeStore;
use wicked_estate_store::SqliteStore;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Copy a fixture file to the temp dir.  Silently skips if the source does not exist
/// (used for optional sidecars such as `.memext`).  Returns the destination path regardless.
fn copy_fixture(name: &str, dir: &tempfile::TempDir) -> PathBuf {
    let src = fixture_dir().join(name);
    let dst = dir.path().join(name);
    if src.exists() {
        std::fs::copy(&src, &dst)
            .unwrap_or_else(|e| panic!("failed to copy fixture '{name}': {e}"));
    }
    dst
}

/// Extract the inner JSON payload from an MCP tool-result response.
///
/// MCP wraps tool output as:
/// `{"result":{"content":[{"type":"text","text":"<JSON string>"}],"isError":false}}`
/// This helper parses `content[0].text` as JSON and returns it.
fn inner_json(resp: &Value) -> Value {
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("content[0].text must be a string; got: {resp}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("content[0].text is not valid JSON ({e}); text={text}"))
}

// ── SC-009 ────────────────────────────────────────────────────────────────────

/// SC-009: Open all 4 v0.12.x fixture stores and exercise one read operation per domain.
///
/// Fixture seed data (SEED.md):
/// - Estate: `seed_fn` → SearchEntity("seed_fn") must return non-empty matches.
/// - Memory: scope="fixture" content contains "seed" → memory.recall("seed") must return items.
/// - Knowledge: doc with "seed" → knowledge.recall("seed") must return items.
/// - XEdge: about-edge links knowledge doc to estate symbol at epoch 0 == 0 →
///   knowledge.recall_about_code(["ts-rust . . . seed_fixture/seed_fn()."]) must return items.
#[test]
fn sc009_all_four_stores_open_and_all_reads_return_nonempty() {
    // Copy fixtures to a fresh temp dir so WAL/SHM files never touch the originals.
    let dir = tempfile::tempdir().expect("create tempdir");

    let estate_path = copy_fixture("estate_v0120.db", &dir);
    let memory_path = copy_fixture("memory_v0121.db", &dir);
    let knowledge_path = copy_fixture("knowledge_v0121.db", &dir);
    let xedge_path = copy_fixture("xedge_v0121.db", &dir);
    // Optional MemoryEngine sidecar — copy if present.
    copy_fixture("memory_v0121.db.memext", &dir);

    // Open all 4 stores.
    let store =
        SqliteStore::open(&estate_path).expect("estate_v0120.db: SqliteStore::open must succeed");

    let xedge = Arc::new(
        XedgeStore::open(xedge_path.to_str().unwrap())
            .expect("xedge_v0121.db: XedgeStore::open must succeed"),
    );

    let mut memory = MemoryEngine::open(memory_path.to_str().unwrap())
        .expect("memory_v0121.db: MemoryEngine::open must succeed")
        .with_xedge_store(Arc::clone(&xedge));

    let mut knowledge = KnowledgeEngine::open(knowledge_path.to_str().unwrap())
        .expect("knowledge_v0121.db: KnowledgeEngine::open must succeed")
        .with_xedge_store(Arc::clone(&xedge));

    let ctx = McpContext::default();

    // ── 1. SearchEntity (estate store; no domain handles required) ────────────

    let req_search = json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "tools/call",
        "params": { "name": "SearchEntity", "arguments": { "name": "seed_fn" } }
    });
    let resp_search =
        handle_request_unified(&store, &req_search, &ctx, None, None::<&dyn RetrievalTool>);

    assert!(
        resp_search.get("error").is_none(),
        "SearchEntity: unexpected JSON-RPC error; response: {resp_search}"
    );
    assert!(
        !resp_search["result"]["isError"].as_bool().unwrap_or(true),
        "SearchEntity: isError must be false"
    );
    let search_inner = inner_json(&resp_search);
    let matches = search_inner["matches"]
        .as_array()
        .unwrap_or_else(|| panic!("SearchEntity: 'matches' must be an array; got: {search_inner}"));
    assert!(
        !matches.is_empty(),
        "SearchEntity('seed_fn'): expected non-empty matches in estate_v0120.db; got: {search_inner}"
    );

    // ── 2. memory.recall ──────────────────────────────────────────────────────

    let req_recall = json!({
        "jsonrpc": "2.0", "id": 2,
        "method": "tools/call",
        "params": {
            "name": "memory.recall",
            "arguments": { "query": "seed", "token_budget": 512 }
        }
    });
    // Block scope: DomainHandles holds mutable borrows; released at block end.
    let resp_recall = {
        let mut handles = DomainHandles {
            memory: &mut memory as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut knowledge as &mut dyn KnowledgeApi,
        };
        handle_request_unified(
            &store,
            &req_recall,
            &ctx,
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
        )
    };

    assert!(
        resp_recall.get("error").is_none(),
        "memory.recall: unexpected JSON-RPC error; response: {resp_recall}"
    );
    assert!(
        !resp_recall["result"]["isError"].as_bool().unwrap_or(true),
        "memory.recall: isError must be false"
    );
    let recall_inner = inner_json(&resp_recall);
    let recall_items = recall_inner["items"]
        .as_array()
        .unwrap_or_else(|| panic!("memory.recall: 'items' must be an array; got: {recall_inner}"));
    assert!(
        !recall_items.is_empty(),
        "memory.recall(query='seed'): expected non-empty items in memory_v0121.db; got: {recall_inner}"
    );

    // ── 3. knowledge.recall ───────────────────────────────────────────────────

    let req_know_recall = json!({
        "jsonrpc": "2.0", "id": 3,
        "method": "tools/call",
        "params": {
            "name": "knowledge.recall",
            "arguments": { "query": "seed", "token_budget": 512 }
        }
    });
    let resp_know_recall = {
        let mut handles = DomainHandles {
            memory: &mut memory as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut knowledge as &mut dyn KnowledgeApi,
        };
        handle_request_unified(
            &store,
            &req_know_recall,
            &ctx,
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
        )
    };

    assert!(
        resp_know_recall.get("error").is_none(),
        "knowledge.recall: unexpected JSON-RPC error; response: {resp_know_recall}"
    );
    assert!(
        !resp_know_recall["result"]["isError"]
            .as_bool()
            .unwrap_or(true),
        "knowledge.recall: isError must be false"
    );
    let know_recall_inner = inner_json(&resp_know_recall);
    let know_items = know_recall_inner["items"].as_array().unwrap_or_else(|| {
        panic!("knowledge.recall: 'items' must be array; got: {know_recall_inner}")
    });
    assert!(
        !know_items.is_empty(),
        "knowledge.recall(query='seed'): expected non-empty items in knowledge_v0121.db; got: {know_recall_inner}"
    );

    // ── 4. knowledge.recall_about_code ───────────────────────────────────────
    // The xedge fixture seeds an about-edge from the knowledge doc to the estate symbol.
    // Estate symbol epoch == 0; xedge tgt_epoch == 0 → edge resolves → non-empty result.

    let req_about = json!({
        "jsonrpc": "2.0", "id": 4,
        "method": "tools/call",
        "params": {
            "name": "knowledge.recall_about_code",
            "arguments": {
                "code_ids": ["ts-rust . . . seed_fixture/seed_fn()."]
            }
        }
    });
    let resp_about = {
        let mut handles = DomainHandles {
            memory: &mut memory as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut knowledge as &mut dyn KnowledgeApi,
        };
        handle_request_unified(
            &store,
            &req_about,
            &ctx,
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
        )
    };

    assert!(
        resp_about.get("error").is_none(),
        "knowledge.recall_about_code: unexpected JSON-RPC error; response: {resp_about}"
    );
    assert!(
        !resp_about["result"]["isError"].as_bool().unwrap_or(true),
        "knowledge.recall_about_code: isError must be false"
    );
    let about_inner = inner_json(&resp_about);
    let about_items = about_inner["items"].as_array().unwrap_or_else(|| {
        panic!("knowledge.recall_about_code: 'items' must be array; got: {about_inner}")
    });
    assert!(
        !about_items.is_empty(),
        "knowledge.recall_about_code: expected non-empty items \
         (xedge tgt_epoch=0 == estate symbol epoch=0, edge must resolve); got: {about_inner}"
    );

    // ── tools/list with all 4 stores open → exactly 24 tools ─────────────────
    // 11 estate (unconditional, incl. rules.recall) + 6 memory + 7 knowledge = 24.
    // SemanticSearch is absent: McpContext::default() has embedder_meta_id=None → dim-guard fails.

    let req_list = json!({
        "jsonrpc": "2.0", "id": 5,
        "method": "tools/list",
        "params": {}
    });
    let resp_list = {
        let mut handles = DomainHandles {
            memory: &mut memory as &mut dyn MemoryApi<Error = anyhow::Error>,
            knowledge: &mut knowledge as &mut dyn KnowledgeApi,
        };
        handle_request_unified(
            &store,
            &req_list,
            &ctx,
            Some(&mut handles),
            None::<&dyn RetrievalTool>,
        )
    };

    let tools = resp_list["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list: 'tools' must be array; got: {resp_list}"));
    assert_eq!(
        tools.len(),
        28,
        "tools/list with all 4 stores must return exactly 28 tools \
         (11 estate + 6 memory + 7 knowledge + 4 proposal); got {}: {:?}",
        tools.len(),
        tools
            .iter()
            .map(|t| t["name"].as_str().unwrap_or("?"))
            .collect::<Vec<_>>()
    );
}

// ── DB compatibility (DoD §2.6) ───────────────────────────────────────────────

/// DoD §2.6: Each v0.12.x fixture opens in the v0.13.0 engine without error.
/// No migration is required — the schema is forward-compatible.
#[test]
fn db_compat_all_fixture_stores_open_without_error() {
    let dir = tempfile::tempdir().expect("create tempdir");

    let estate_path = copy_fixture("estate_v0120.db", &dir);
    let memory_path = copy_fixture("memory_v0121.db", &dir);
    let knowledge_path = copy_fixture("knowledge_v0121.db", &dir);
    let xedge_path = copy_fixture("xedge_v0121.db", &dir);
    // Optional sidecar — copy if present so MemoryEngine::open reconciles it.
    copy_fixture("memory_v0121.db.memext", &dir);

    // estate_v0120.db — opened by the current SqliteStore (idempotent schema migrate).
    SqliteStore::open(&estate_path)
        .expect("estate_v0120.db: SqliteStore::open must succeed without error (DoD §2.6)");

    // memory_v0121.db — opened by MemoryEngine; crash-recovery reconcile runs at open.
    MemoryEngine::open(memory_path.to_str().unwrap())
        .expect("memory_v0121.db: MemoryEngine::open must succeed without error (DoD §2.6)");

    // knowledge_v0121.db — opened by KnowledgeEngine.
    KnowledgeEngine::open(knowledge_path.to_str().unwrap())
        .expect("knowledge_v0121.db: KnowledgeEngine::open must succeed without error (DoD §2.6)");

    // xedge_v0121.db — opened by XedgeStore (schema + meta row are idempotent).
    XedgeStore::open(xedge_path.to_str().unwrap())
        .expect("xedge_v0121.db: XedgeStore::open must succeed without error (DoD §2.6)");
}
