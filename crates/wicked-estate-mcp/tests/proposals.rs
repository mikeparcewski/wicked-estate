//! Proposal-queue MCP integration tests (DES-MEM-FACETED-001 §5.0), driven through the
//! read-only-aware unified dispatch (`handle_request_unified_ro`).
//!
//! The keystone here is the deliberate `--readonly` asymmetry: `proposal.submit` is a SAFE write
//! (an inert `pending` node, never recalled/applied until approved), so a read-only worker CAN
//! submit; `proposal.approve` / `proposal.reject` mutate the active store / decide the queue, so a
//! read-only worker is REFUSED. These run against a REAL `MemoryEngine` so the submit write truly
//! lands under `--readonly` (the store handle is read-write regardless of the readonly flag).

use serde_json::{Value, json};
use wicked_estate_core::RetrievalTool;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified_ro};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::{MemoryApi, ProposalState};
use wicked_estate_store::SqliteStore;

/// Issue one `tools/call` through the unified dispatch with the given `read_only` flag.
fn call(
    store: &SqliteStore,
    mem: &mut MemoryEngine,
    know: &mut KnowledgeEngine,
    tool: &str,
    args: Value,
    read_only: bool,
) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": { "name": tool, "arguments": args }
    });
    let mut handles = DomainHandles {
        memory: mem as &mut dyn MemoryApi<Error = anyhow::Error>,
        knowledge: know as &mut dyn KnowledgeApi,
    };
    handle_request_unified_ro(
        store,
        &req,
        &McpContext::default(),
        Some(&mut handles),
        None::<&dyn RetrievalTool>,
        read_only,
    )
}

/// Unwrap the inner JSON payload from a successful MCP tool result (`result.content[0].text`).
fn result_payload(resp: &Value) -> Value {
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a text content result, got: {resp}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("result text is not JSON ({e}): {text}"))
}

#[test]
fn readonly_allows_proposal_submit_but_refuses_approve_and_reject() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();

    // ── 1. proposal.submit under --readonly SUCCEEDS (the safe-write exception) ──
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({
            "kind_type": "memory",
            "payload": { "content": "safe write under readonly", "tier": "semantic" }
        }),
        true, // read_only
    );
    assert!(
        resp.get("error").is_none(),
        "proposal.submit must be allowed under --readonly (it is an inert safe-write); got {resp}"
    );
    let pid = result_payload(&resp)["id"]
        .as_str()
        .expect("submit returns an id")
        .to_string();
    assert!(!pid.is_empty());

    // The write REALLY landed under --readonly: the engine now holds exactly one pending proposal.
    let pending = mem
        .list_proposals(None, Some(ProposalState::Pending))
        .unwrap();
    assert_eq!(
        pending.len(),
        1,
        "the pending proposal must be persisted even though the worker is --readonly"
    );
    assert_eq!(pending[0].id, pid);

    // ── 2. proposal.approve + proposal.reject under --readonly are REFUSED (-32601) ──
    for write_tool in ["proposal.approve", "proposal.reject"] {
        let resp = call(
            &store,
            &mut mem,
            &mut know,
            write_tool,
            json!({ "id": pid }),
            true,
        );
        assert_eq!(
            resp["error"]["code"], -32601,
            "{write_tool} must be refused under --readonly with -32601; got {resp}"
        );
        assert!(
            resp.get("result").is_none() || resp["result"].is_null(),
            "{write_tool} refused under --readonly must NOT produce a tool result; got {resp}"
        );
    }

    // The refusal is a true backstop: the proposal is still pending (approve/reject never ran).
    assert_eq!(
        mem.list_proposals(None, Some(ProposalState::Pending))
            .unwrap()
            .len(),
        1,
        "a refused approve/reject must leave the proposal pending"
    );

    // ── 3. Falsifier: WITHOUT --readonly, approve runs and promotes the proposal ──
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.approve",
        json!({ "id": pid }),
        false, // read_only = false
    );
    assert!(
        resp.get("error").is_none(),
        "without --readonly proposal.approve must reach the engine; got {resp}"
    );
    let payload = result_payload(&resp);
    assert_eq!(
        payload["outcome"], "promoted",
        "approving a memory proposal must promote it; got {payload}"
    );
    assert!(
        payload["active_id"].as_str().is_some_and(|s| !s.is_empty()),
        "a promoted memory proposal returns an active_id; got {payload}"
    );
    // And the proposal is no longer pending.
    assert!(
        mem.list_proposals(None, Some(ProposalState::Pending))
            .unwrap()
            .is_empty(),
        "after approval there are no pending proposals"
    );
}

#[test]
fn proposal_submit_stamps_provenance_from_env_not_args() {
    // Provenance is stamped by the server from WICKED_RUN_* env, NEVER from caller args. A caller
    // "provenance" arg is ignored; the env values (if any) are authoritative. This test sets none,
    // so provenance is the empty map — and an attacker-supplied provenance arg does not leak in.
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();

    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({
            "kind_type": "policy:security",
            "payload": { "rule": "no secrets in logs" },
            "provenance": { "run_agent": "attacker-forged" }
        }),
        false,
    );
    assert!(
        resp.get("error").is_none(),
        "submit must succeed; got {resp}"
    );
    let pid = result_payload(&resp)["id"].as_str().unwrap().to_string();

    let p = mem
        .list_proposals(Some("policy:security"), None)
        .unwrap()
        .into_iter()
        .find(|p| p.id == pid)
        .expect("the submitted proposal");
    assert!(
        !p.provenance.values().any(|v| v == "attacker-forged"),
        "caller-supplied provenance must NEVER be stored; got {:?}",
        p.provenance
    );
}

#[test]
fn proposal_submit_rejects_invalid_kind_type_and_missing_payload() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();

    // invalid kind_type (uppercase) ⇒ -32602
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "Memory", "payload": { "content": "x" } }),
        false,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "invalid kind_type must fail loud; got {resp}"
    );

    // missing payload ⇒ -32602
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "memory" }),
        false,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "missing payload must fail loud; got {resp}"
    );
}
