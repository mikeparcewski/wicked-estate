//! `memory.recall scope_prefix` wire tests — the subtree-inclusive recall param must survive the
//! full MCP dispatch pipeline (`handle_request_unified`).
//!
//! Semantics under test (see `ScopeFilter` in `wicked-estate-memory`):
//! - no `scope_prefix` ⇒ existing inheritance behavior EXACTLY (a root-scoped recall does NOT see
//!   leaf-scoped memories) — the no-regression pin;
//! - `scope_prefix: ""` ⇒ the root subtree = every memory (root- and leaf-scoped both surface);
//! - `scope_prefix: "brain:test"` ⇒ only that subtree (not root — replace, not fuse — and not a
//!   sibling subtree);
//! - a present non-string `scope_prefix` ⇒ JSON-RPC -32602 (fail loud, same rule as
//!   memory.capture's scope).
//!
//! Uses real in-memory engines (no fixture-DB dependency), same pattern as `attribution.rs`.

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

/// Engines pre-loaded with one memory per scope: root, the `brain:test/doc:a` leaf (the migrated
/// brain-memory shape), and a `brain:other/doc:b` sibling. All three lexically match the recall
/// query, so any exclusion is the scope filter's doing.
fn seeded() -> (MemStore, MemoryEngine, KnowledgeEngine) {
    let store = MemStore::new();
    let xedge = Arc::new(XedgeStore::in_memory().unwrap());
    let mut memory = MemoryEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    let mut knowledge = KnowledgeEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    for (scope, content) in [
        ("", "root wicked garden fact"),
        (
            "brain:test/doc:a",
            "leaf wicked garden fact from the brain import",
        ),
        (
            "brain:other/doc:b",
            "sibling wicked garden fact in another subtree",
        ),
    ] {
        let resp = call(
            &store,
            &mut memory,
            &mut knowledge,
            1,
            "memory.capture",
            json!({"content": content, "kind": "fact", "tier": "semantic", "scope": scope}),
        );
        assert!(
            resp.get("error").is_none(),
            "seed capture at scope '{scope}' failed: {resp}"
        );
    }
    (store, memory, knowledge)
}

/// Run a memory.recall and return the items' `content` strings.
fn recall_contents(
    store: &MemStore,
    memory: &mut MemoryEngine,
    knowledge: &mut KnowledgeEngine,
    args: Value,
) -> Vec<String> {
    let resp = call(store, memory, knowledge, 2, "memory.recall", args);
    assert!(
        resp.get("error").is_none(),
        "memory.recall: unexpected JSON-RPC error: {resp}"
    );
    let inner = inner_json(&resp);
    inner["items"]
        .as_array()
        .expect("memory.recall must return an `items` array")
        .iter()
        .map(|i| i["content"].as_str().unwrap_or_default().to_string())
        .collect()
}

// ── the wire tests ────────────────────────────────────────────────────────────

/// No-regression pin: without `scope_prefix`, a root-scoped recall keeps the inheritance
/// semantics — the leaf-scoped memory stays INVISIBLE.
#[test]
fn recall_without_scope_prefix_keeps_inheritance_semantics() {
    let (store, mut memory, mut knowledge) = seeded();
    let contents = recall_contents(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"query": "wicked garden fact", "token_budget": 4000}),
    );
    assert!(
        contents.iter().any(|c| c.contains("root")),
        "root recall must see the root-scoped memory; got: {contents:?}"
    );
    assert!(
        !contents.iter().any(|c| c.contains("leaf")),
        "REGRESSION: root recall without scope_prefix returned a leaf-scoped memory: {contents:?}"
    );
}

/// `scope_prefix: ""` = the root subtree = everything: root- and leaf-scoped memories both
/// surface, and leaf items carry their own `brain:test/doc:a` scope on the wire.
#[test]
fn recall_with_empty_scope_prefix_sees_root_and_leaf() {
    let (store, mut memory, mut knowledge) = seeded();
    let resp = call(
        &store,
        &mut memory,
        &mut knowledge,
        2,
        "memory.recall",
        json!({"query": "wicked garden fact", "scope_prefix": "", "token_budget": 4000}),
    );
    assert!(
        resp.get("error").is_none(),
        "memory.recall: unexpected JSON-RPC error: {resp}"
    );
    let inner = inner_json(&resp);
    let items = inner["items"].as_array().expect("items array");
    for expect in ["root", "leaf", "sibling"] {
        assert!(
            items
                .iter()
                .any(|i| i["content"].as_str().unwrap_or_default().contains(expect)),
            "scope_prefix \"\" must surface the {expect}-scoped memory; items: {items:?}"
        );
    }
    assert!(
        items
            .iter()
            .any(|i| i["scope"].as_str() == Some("brain:test/doc:a")),
        "the leaf item must carry its own scope on the wire; items: {items:?}"
    );
}

/// `scope_prefix: "brain:test"` admits ONLY that subtree — not the root-scoped ancestor
/// (replace, not fuse) and not the `brain:other` sibling.
#[test]
fn recall_with_scope_prefix_sees_only_that_subtree() {
    let (store, mut memory, mut knowledge) = seeded();
    let contents = recall_contents(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"query": "wicked garden fact", "scope_prefix": "brain:test", "token_budget": 4000}),
    );
    assert!(
        contents.iter().any(|c| c.contains("leaf")),
        "subtree recall must see its leaf memory; got: {contents:?}"
    );
    assert!(
        !contents.iter().any(|c| c.contains("root")),
        "REPLACE VIOLATED: subtree recall returned the root-scoped memory: {contents:?}"
    );
    assert!(
        !contents.iter().any(|c| c.contains("sibling")),
        "SUBTREE ISOLATION VIOLATED: recall leaked brain:other: {contents:?}"
    );
}

/// A present NON-string `scope_prefix` is invalid params (-32602) — silently ignoring it would
/// answer from the wrong visibility set (the same fail-loud rule as memory.capture's scope).
#[test]
fn recall_rejects_non_string_scope_prefix() {
    let (store, mut memory, mut knowledge) = seeded();
    for bad in [
        json!(42),
        json!({"p": "x"}),
        json!(["brain:test"]),
        json!(true),
    ] {
        let resp = call(
            &store,
            &mut memory,
            &mut knowledge,
            3,
            "memory.recall",
            json!({"query": "wicked garden fact", "scope_prefix": bad}),
        );
        let err = resp
            .get("error")
            .unwrap_or_else(|| panic!("non-string scope_prefix {bad} must be rejected: {resp}"));
        assert_eq!(err["code"].as_i64().unwrap(), -32602, "scope_prefix {bad}");
    }
    // Explicit null stays valid — it means "omitted" (inheritance behavior), like capture's scope.
    let resp = call(
        &store,
        &mut memory,
        &mut knowledge,
        4,
        "memory.recall",
        json!({"query": "wicked garden fact", "scope_prefix": Value::Null}),
    );
    assert!(
        resp.get("error").is_none(),
        "null scope_prefix must be treated as omitted: {resp}"
    );
}
