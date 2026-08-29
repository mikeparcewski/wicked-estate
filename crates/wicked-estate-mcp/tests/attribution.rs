//! S4 attribution gate tests — verify that `source` (knowledge.recall) and `scope`
//! (memory.recall) survive the full MCP dispatch pipeline and appear on the wire.
//!
//! These tests exercise the combined-domain `handle_request_unified` path (the 24-tool server)
//! using in-memory engines so there is no fixture-DB dependency.

use std::sync::Arc;

use serde_json::{Value, json};
use wicked_estate_core::RetrievalTool;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_overlay::XedgeStore;
use wicked_estate_store::MemStore;

// ── helpers ───────────────────────────────────────────────────────────────────

fn empty_store() -> MemStore {
    MemStore::new()
}

fn call(
    store: &MemStore,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    knowledge: &mut dyn KnowledgeApi,
    id: u64,
    name: &str,
    args: Value,
) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": args}
    });
    let mut handles = DomainHandles { memory, knowledge };
    handle_request_unified(
        store,
        &req,
        &McpContext::default(),
        Some(&mut handles),
        None::<&dyn RetrievalTool>,
    )
}

/// Parse `result.content[0].text` as JSON.
fn inner_json(resp: &Value) -> Value {
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("content[0].text must be a string");
    serde_json::from_str(text).expect("content[0].text must be valid JSON")
}

// ── knowledge.recall: source field on the wire ────────────────────────────────

/// S4 gate: `knowledge.recall` items returned by the combined-server MCP path must carry a
/// non-empty `source` field for a document ingested with a known source.
#[test]
fn knowledge_recall_items_carry_source_on_wire() {
    let store = empty_store();
    let xedge = Arc::new(XedgeStore::in_memory().unwrap());
    let mut memory = MemoryEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    let mut knowledge = KnowledgeEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));

    // Ingest a document with a known, non-empty source.
    let _ing = call(
        &store,
        &mut memory,
        &mut knowledge,
        1,
        "knowledge.ingest",
        json!({
            "title": "Caching design",
            "chunks": ["The cache uses LRU eviction and a 60-second TTL."],
            "scope": "project:cache",
            "source": "docs/cache.md"
        }),
    );

    // Recall: the items array must contain a `source` field matching the ingested value.
    let resp = call(
        &store,
        &mut memory,
        &mut knowledge,
        2,
        "knowledge.recall",
        json!({"query": "how does the cache evict entries", "token_budget": 2000}),
    );

    assert!(
        resp.get("error").is_none(),
        "knowledge.recall: unexpected JSON-RPC error: {resp}"
    );
    assert!(
        !resp["result"]["isError"].as_bool().unwrap_or(true),
        "knowledge.recall: isError must be false"
    );

    let inner = inner_json(&resp);
    let items = inner["items"]
        .as_array()
        .expect("knowledge.recall must return an `items` array");
    assert!(
        !items.is_empty(),
        "knowledge.recall must return at least one item for the ingested doc"
    );

    // Every returned item must carry a `source` key.
    for (i, item) in items.iter().enumerate() {
        assert!(
            item.get("source").is_some(),
            "knowledge.recall item[{i}] is missing the `source` field (S4 regression): {item}"
        );
    }

    // At least one item must carry the exact ingested source.
    assert!(
        items
            .iter()
            .any(|item| item["source"].as_str() == Some("docs/cache.md")),
        "knowledge.recall: no item carries source='docs/cache.md'; items: {items:?}"
    );
}

// ── memory.recall: scope field is the item's own scope, not the query scope ───

/// S4 gate: `memory.recall` items returned by the combined-server MCP path must carry the
/// ITEM'S own `scope`, not the query scope.
///
/// Scope inheritance direction: a memory at scope `"org:acme"` is visible when the QUERY scope is
/// `"org:acme/agent:claude"` (because `"org:acme"` is an ancestor of `"org:acme/agent:claude"`).
/// This lets us write a memory at the org level and query at the agent level — the returned item
/// scope must be `"org:acme"`, NOT `"org:acme/agent:claude"` (the query scope).
#[test]
fn memory_recall_items_carry_item_scope_not_query_scope() {
    let store = empty_store();
    let xedge = Arc::new(XedgeStore::in_memory().unwrap());
    let mut memory = MemoryEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    let mut knowledge = KnowledgeEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));

    // Capture a memory at the ORG-level scope (broader than the query scope).
    // Scope inheritance: "org:acme" is an ancestor of "org:acme/agent:claude", so a query at the
    // agent scope will surface this org-level memory.
    let item_scope = "org:acme";
    let query_scope = "org:acme/agent:claude"; // more specific — differs from item_scope

    let _cap = call(
        &store,
        &mut memory,
        &mut knowledge,
        1,
        "memory.capture",
        json!({
            "content": "the billing service charges customers via Stripe webhooks",
            "kind": "fact",
            "tier": "semantic",
            "scope": item_scope
        }),
    );

    // Recall at the AGENT scope: the org-level memory must surface (ancestor inheritance).
    let resp = call(
        &store,
        &mut memory,
        &mut knowledge,
        2,
        "memory.recall",
        json!({
            "query": "how does billing charge customers",
            "scope": query_scope,
            "token_budget": 2000
        }),
    );

    assert!(
        resp.get("error").is_none(),
        "memory.recall: unexpected JSON-RPC error: {resp}"
    );
    assert!(
        !resp["result"]["isError"].as_bool().unwrap_or(true),
        "memory.recall: isError must be false"
    );

    let inner = inner_json(&resp);
    let items = inner["items"]
        .as_array()
        .expect("memory.recall must return an `items` array");
    assert!(
        !items.is_empty(),
        "memory.recall must return at least one item (ancestor inheritance: org:acme visible from org:acme/agent:claude)"
    );

    // Every returned item must carry a `scope` key.
    for (i, item) in items.iter().enumerate() {
        assert!(
            item.get("scope").is_some(),
            "memory.recall item[{i}] is missing the `scope` field (S4 regression): {item}"
        );
    }

    // The item's scope must be the ITEM's own scope ("org:acme"), NOT the query scope.
    // This is the falsifier for the old bug where `scope` was hardcoded to the query scope.
    assert!(
        items
            .iter()
            .any(|item| item["scope"].as_str() == Some(item_scope)),
        "memory.recall: no item carries the item's own scope='{item_scope}'; \
         items: {items:?} (the old bug returned query_scope='{query_scope}' instead)"
    );
}
