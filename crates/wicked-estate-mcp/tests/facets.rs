//! `memory.capture` `facets` / `memory.recall` `intent` wire tests (DES-MEM-FACETED-001 Phase 2 —
//! MCP surface). The orthogonal, intent-matching facet dimension must survive the full MCP dispatch
//! pipeline (`handle_request_unified`), be validated FAIL-LOUD at the wire (same rule as `scope`),
//! and stay fully back-compatible with unfaceted capture/recall.
//!
//! Semantics under test (see `facet_admits` in `wicked-estate-memory-core`):
//! - a faceted capture (`facets:{cli:codex}`) is admitted by an intent that carries the axis with
//!   the matching value (`intent:{cli:codex}`), and EXCLUDED by a mismatching (`{cli:claude}`) or an
//!   empty intent — a memory constraining an axis the intent lacks does not surface;
//! - an UNFACETED memory is admitted under every intent, empty included (specificity 0) — the
//!   no-regression pin;
//! - an invalid facet/intent (uppercase axis, empty value, or a non-object value) ⇒ JSON-RPC -32602
//!   (never silently accepted — a bad facet silently mis-routes recall);
//! - omitted / null `facets`/`intent` ⇒ empty ⇒ pre-Phase-2 behavior EXACTLY.
//!
//! Uses real in-memory engines (no fixture-DB dependency), same pattern as `scope_prefix.rs`.

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

/// Fresh in-memory engines (memory + a knowledge engine to satisfy `DomainHandles`; knowledge is
/// unused here). Both share an xedge store, mirroring the production wiring.
fn engines() -> (MemStore, MemoryEngine, KnowledgeEngine) {
    let store = MemStore::new();
    let xedge = Arc::new(XedgeStore::in_memory().unwrap());
    let memory = MemoryEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    let knowledge = KnowledgeEngine::in_memory()
        .unwrap()
        .with_xedge_store(Arc::clone(&xedge));
    (store, memory, knowledge)
}

/// Capture a memory at root scope, asserting it succeeded. Returns the response for inspection.
fn capture_ok(
    store: &MemStore,
    memory: &mut MemoryEngine,
    knowledge: &mut KnowledgeEngine,
    args: Value,
) -> Value {
    let resp = call(store, memory, knowledge, 1, "memory.capture", args.clone());
    assert!(
        resp.get("error").is_none(),
        "memory.capture must succeed for {args}; got {resp}"
    );
    resp
}

/// Run a memory.recall and return the items' `content` strings.
fn recall_contents(
    store: &MemStore,
    memory: &mut MemoryEngine,
    knowledge: &mut KnowledgeEngine,
    args: Value,
) -> Vec<String> {
    let resp = call(store, memory, knowledge, 2, "memory.recall", args.clone());
    assert!(
        resp.get("error").is_none(),
        "memory.recall: unexpected JSON-RPC error for {args}: {resp}"
    );
    let inner = inner_json(&resp);
    inner["items"]
        .as_array()
        .expect("memory.recall must return an `items` array")
        .iter()
        .map(|i| i["content"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Seed one faceted (`cli:codex`) and one unfaceted memory, lexically identical on the query token
/// `shibboleth`, so any exclusion is the facet filter's doing (not lexical miss).
fn seed_faceted(store: &MemStore, memory: &mut MemoryEngine, knowledge: &mut KnowledgeEngine) {
    capture_ok(
        store,
        memory,
        knowledge,
        json!({
            "content": "codex shibboleth needs workspace-write",
            "kind": "fact",
            "tier": "semantic",
            "facets": {"cli": "codex"}
        }),
    );
    capture_ok(
        store,
        memory,
        knowledge,
        json!({
            "content": "plain shibboleth unfaceted fact",
            "kind": "fact",
            "tier": "semantic"
        }),
    );
}

// ── R1/R4/R5 happy path: capture + intent-matched recall ────────────────────────

/// A faceted capture surfaces under a matching intent, and is EXCLUDED by a mismatching or empty
/// intent. The unfaceted memory surfaces under every intent (specificity 0) — the back-compat pin.
#[test]
fn recall_returns_faceted_memory_only_under_matching_intent() {
    let (store, mut memory, mut knowledge) = engines();
    seed_faceted(&store, &mut memory, &mut knowledge);

    // Matching intent: the codex memory AND the unfaceted memory both surface.
    let matched = recall_contents(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"query": "shibboleth", "token_budget": 4000, "intent": {"cli": "codex"}}),
    );
    assert!(
        matched.iter().any(|c| c.contains("codex")),
        "intent {{cli:codex}} must surface the codex-faceted memory; got {matched:?}"
    );
    assert!(
        matched.iter().any(|c| c.contains("unfaceted")),
        "the unfaceted memory must surface under any intent; got {matched:?}"
    );

    // Mismatching intent: the codex memory is EXCLUDED; the unfaceted one still surfaces.
    let mismatched = recall_contents(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"query": "shibboleth", "token_budget": 4000, "intent": {"cli": "claude"}}),
    );
    assert!(
        !mismatched.iter().any(|c| c.contains("codex")),
        "FACET LEAK: intent {{cli:claude}} returned the codex-faceted memory: {mismatched:?}"
    );
    assert!(
        mismatched.iter().any(|c| c.contains("unfaceted")),
        "the unfaceted memory must still surface under a mismatching intent; got {mismatched:?}"
    );

    // Empty (omitted) intent: a memory constraining an axis the intent lacks is EXCLUDED; only the
    // unfaceted memory surfaces.
    let no_intent = recall_contents(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"query": "shibboleth", "token_budget": 4000}),
    );
    assert!(
        !no_intent.iter().any(|c| c.contains("codex")),
        "FACET LEAK: an intent-less recall returned the codex-faceted memory: {no_intent:?}"
    );
    assert!(
        no_intent.iter().any(|c| c.contains("unfaceted")),
        "the unfaceted memory must surface with no intent (legacy behavior); got {no_intent:?}"
    );
}

// ── fail-loud validation (-32602) ───────────────────────────────────────────────

/// An invalid `facets` on capture is invalid params (-32602): an uppercase axis, an empty value, or
/// a present non-object value — never silently accepted.
#[test]
fn capture_rejects_invalid_facets() {
    let (store, mut memory, mut knowledge) = engines();
    for bad in [
        json!({"CLI": "codex"}),      // uppercase axis (^[a-z]… violated)
        json!({"cli": ""}),           // empty value
        json!({"cli.name": "codex"}), // '.' not in the axis charset
        json!(["cli", "codex"]),      // array, not an object
        json!("cli:codex"),           // string, not an object
        json!(42),                    // number
        json!(true),                  // bool
    ] {
        let resp = call(
            &store,
            &mut memory,
            &mut knowledge,
            3,
            "memory.capture",
            json!({"content": "x", "kind": "fact", "tier": "semantic", "facets": bad}),
        );
        let err = resp
            .get("error")
            .unwrap_or_else(|| panic!("invalid facets {bad} must be rejected: {resp}"));
        assert_eq!(err["code"].as_i64().unwrap(), -32602, "facets {bad}");
    }
}

/// An invalid `intent` on recall is invalid params (-32602) — the SAME fail-loud rule as capture's
/// facets (a bad intent silently mis-routes recall).
#[test]
fn recall_rejects_invalid_intent() {
    let (store, mut memory, mut knowledge) = engines();
    for bad in [
        json!({"CLI": "codex"}),
        json!({"cli": ""}),
        json!({"cli.name": "codex"}),
        json!(["cli", "codex"]),
        json!("cli:codex"),
        json!(42),
        json!(true),
    ] {
        let resp = call(
            &store,
            &mut memory,
            &mut knowledge,
            4,
            "memory.recall",
            json!({"query": "shibboleth", "intent": bad}),
        );
        let err = resp
            .get("error")
            .unwrap_or_else(|| panic!("invalid intent {bad} must be rejected: {resp}"));
        assert_eq!(err["code"].as_i64().unwrap(), -32602, "intent {bad}");
    }
}

// ── back-compat: no facets / null facets behave exactly as before ────────────────

/// Explicit `null` (and omission) for `facets`/`intent` is treated as empty — a valid, unfaceted
/// capture/recall round-trip, exactly the pre-Phase-2 behavior.
#[test]
fn null_and_omitted_facets_are_treated_as_empty() {
    let (store, mut memory, mut knowledge) = engines();

    // Capture with explicit null facets succeeds (== omitted == unfaceted).
    capture_ok(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"content": "legacy shibboleth memory", "kind": "fact", "tier": "semantic", "facets": Value::Null}),
    );

    // Recall with explicit null intent succeeds and surfaces the unfaceted memory (no filtering).
    let resp = call(
        &store,
        &mut memory,
        &mut knowledge,
        5,
        "memory.recall",
        json!({"query": "shibboleth", "token_budget": 4000, "intent": Value::Null}),
    );
    assert!(
        resp.get("error").is_none(),
        "null intent must be treated as omitted: {resp}"
    );
    let contents: Vec<String> = inner_json(&resp)["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|i| i["content"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        contents.iter().any(|c| c.contains("legacy")),
        "null-facets capture must be recallable with a null intent; got {contents:?}"
    );
}

/// Pure legacy round-trip: a capture with NO `facets` and a recall with NO `intent` behaves exactly
/// as before Phase 2 — the memory surfaces, unaffected by the new dimension.
#[test]
fn no_facets_no_intent_behaves_as_before() {
    let (store, mut memory, mut knowledge) = engines();
    capture_ok(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"content": "unfaceted shibboleth learning", "kind": "fact", "tier": "semantic"}),
    );
    let contents = recall_contents(
        &store,
        &mut memory,
        &mut knowledge,
        json!({"query": "shibboleth", "token_budget": 4000}),
    );
    assert!(
        contents
            .iter()
            .any(|c| c.contains("unfaceted shibboleth learning")),
        "a legacy (unfaceted) capture must be recallable with no intent; got {contents:?}"
    );
}
