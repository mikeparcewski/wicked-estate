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
fn knowledge_proposal_submit_readonly_preserves_payload_and_facets() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    let payload = json!({
        "content": "The ledger is rebuilt nightly from the journal.",
        "class": "concept",
        "scope": "project:ledger",
        "source": "docs/ledger.md",
        "observed_at": "2026-10-11T12:00:00Z",
        "source_rev": "abc123"
    });
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({"kind_type": "knowledge", "payload": payload, "facets": {"repo": "ledger"}}),
        true,
    );
    let pid = result_payload(&resp)["id"].as_str().unwrap().to_string();
    let pending = mem
        .list_proposals(Some("knowledge"), Some(ProposalState::Pending))
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, pid);
    assert_eq!(pending[0].payload, payload);
    assert_eq!(pending[0].facets.get("repo"), Some("ledger"));
    assert_eq!(know.count(None, None).unwrap(), 0);
    assert_eq!(mem.count().unwrap(), 0);
    assert!(
        KnowledgeApi::recall(&mut know, "ledger", 500, None, 1)
            .unwrap()
            .is_empty()
    );
    for tool in ["proposal.approve", "proposal.reject"] {
        let resp = call(&store, &mut mem, &mut know, tool, json!({"id": pid}), true);
        assert_eq!(resp["error"]["code"], -32601);
    }
    let listed = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.list",
        json!({"kind_type": "knowledge", "state": "pending"}),
        true,
    );
    let listed = result_payload(&listed);
    assert_eq!(listed["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(listed["proposals"][0]["id"], pid);
}

#[test]
fn knowledge_proposal_submit_requires_source_and_content() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    for field in ["source", "content"] {
        for bad in [
            None,
            Some(json!(null)),
            Some(json!("")),
            Some(json!("  ")),
            Some(json!(7)),
        ] {
            let mut payload = json!({"content": "ledger fact", "source": "docs/ledger.md"});
            match bad {
                None => {
                    payload.as_object_mut().unwrap().remove(field);
                }
                Some(v) => {
                    payload[field] = v;
                }
            }
            let resp = call(
                &store,
                &mut mem,
                &mut know,
                "proposal.submit",
                json!({"kind_type": "knowledge", "payload": payload}),
                true,
            );
            assert_eq!(resp["error"]["code"], -32602, "{resp}");
            assert!(
                resp["error"]["message"].as_str().unwrap().contains(field),
                "{resp}"
            );
        }
    }
    assert!(mem.list_proposals(None, None).unwrap().is_empty());
    assert_eq!(know.count(None, None).unwrap(), 0);
}

#[test]
fn knowledge_proposal_approve_writes_recallable_node_with_source() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    let submitted = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({"kind_type": "knowledge", "facets": {"repo": "ledger"}, "payload": {
            "content": "The reconciliation ledger is rebuilt nightly from the journal.",
            "scope": "project:ledger", "source": "docs/ledger.md"
        }}),
        true,
    );
    let pid = result_payload(&submitted)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let approved = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.approve",
        json!({"id": pid}),
        false,
    );
    let approved = result_payload(&approved);
    assert_eq!(approved["outcome"], "promoted");
    let active_id = approved["active_id"].as_str().unwrap();
    let expected = wicked_estate_core::Symbol::synthetic("kchunk", format!("proposal:{pid}")).id();
    assert_eq!(active_id, expected.0);
    let node = know.node(&expected).unwrap().unwrap();
    assert_eq!(node.metadata["facets"], json!({"repo": "ledger"}));
    let node = wicked_estate_knowledge::KNode::from_node(&node).unwrap();
    assert_eq!(node.scope, "project:ledger");
    assert_eq!(node.source, "docs/ledger.md");
    let recalled = KnowledgeApi::recall(
        &mut know,
        "reconciliation ledger",
        500,
        Some("project:ledger"),
        1,
    )
    .unwrap();
    assert!(
        recalled
            .iter()
            .any(|r| r.node_id == active_id && r.source == "docs/ledger.md")
    );
    assert_eq!(know.count(None, None).unwrap(), 1);
    assert_eq!(mem.count().unwrap(), 0);
    let approved_queue = mem
        .list_proposals(Some("knowledge"), Some(ProposalState::Approved))
        .unwrap();
    assert_eq!(approved_queue.len(), 1);
    assert_eq!(approved_queue[0].id, pid);
    for tool in ["proposal.approve", "proposal.reject"] {
        let again = call(&store, &mut mem, &mut know, tool, json!({"id": pid}), false);
        assert!(
            again.get("error").is_some(),
            "decided proposals cannot be decided again: {again}"
        );
    }
}

#[test]
fn knowledge_proposal_retry_after_write_keeps_one_node() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    let pid = mem
        .submit_proposal(
            "knowledge",
            json!({"content": "retry ledger fact", "source": "docs/ledger.md"}),
            Default::default(),
            Default::default(),
            1,
        )
        .unwrap();
    // Inject a failure after writing the node but before the queue can mark it Approved.
    let failed = mem.approve_proposal_with_knowledge(&pid, 2, &mut |proposal, now| {
        KnowledgeApi::write_proposal(&mut know, proposal, now)?;
        anyhow::bail!("injected failure after knowledge write")
    });
    assert!(failed.is_err());
    assert_eq!(
        mem.list_proposals(Some("knowledge"), Some(ProposalState::Pending))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(know.count(None, None).unwrap(), 1);
    let retried = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.approve",
        json!({"id": pid}),
        false,
    );
    let retried = result_payload(&retried);
    assert_eq!(retried["outcome"], "promoted");
    assert_eq!(
        retried["active_id"],
        wicked_estate_core::Symbol::synthetic("kchunk", format!("proposal:{pid}"))
            .id()
            .0
    );
    assert_eq!(know.count(None, None).unwrap(), 1);
    assert_eq!(
        mem.list_proposals(Some("knowledge"), Some(ProposalState::Approved))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn knowledge_proposal_reject_writes_nothing() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    let submitted = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({"kind_type": "knowledge", "payload": {"content": "discarded ledger fact", "source": "docs/ledger.md"}}),
        true,
    );
    let pid = result_payload(&submitted)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let rejected = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.reject",
        json!({"id": pid}),
        false,
    );
    assert_eq!(result_payload(&rejected)["ok"], true);
    assert_eq!(know.count(None, None).unwrap(), 0);
    assert_eq!(mem.count().unwrap(), 0);
    assert!(
        KnowledgeApi::recall(&mut know, "discarded ledger", 500, None, 1)
            .unwrap()
            .is_empty()
    );
    let rejected_queue = mem
        .list_proposals(Some("knowledge"), Some(ProposalState::Rejected))
        .unwrap();
    assert_eq!(rejected_queue.len(), 1);
    assert_eq!(rejected_queue[0].id, pid);
    let approved = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.approve",
        json!({"id": pid}),
        false,
    );
    assert!(approved.get("error").is_some());
    assert_eq!(know.count(None, None).unwrap(), 0);
}

#[test]
fn knowledge_proposal_submit_validates_optional_fields_and_facets() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    for (field, bad) in [
        ("class", json!("wiki")),
        ("class", json!(null)),
        ("scope", json!(3)),
        ("source_rev", json!(false)),
        ("observed_at", json!(3)),
        ("observed_at", json!("not-a-date")),
        ("observed_at", json!("2026-02-30T12:00:00Z")),
    ] {
        let mut payload = json!({"content": "ledger fact", "source": "docs/ledger.md"});
        payload[field] = bad;
        let resp = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({"kind_type": "knowledge", "payload": payload}),
            true,
        );
        assert_eq!(resp["error"]["code"], -32602, "{resp}");
        assert!(
            resp["error"]["message"].as_str().unwrap().contains(field),
            "{resp}"
        );
    }
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({
            "kind_type": "knowledge", "payload": {"content": "ledger fact", "source": "docs/ledger.md"},
            "facets": {"repo": ""}
        }),
        true,
    );
    assert_eq!(resp["error"]["code"], -32602);
    assert!(mem.list_proposals(None, None).unwrap().is_empty());
}

#[test]
fn knowledge_proposal_approve_supports_all_classes() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    for class in ["doc", "section", "chunk", "concept"] {
        let submitted = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({
                "kind_type": "knowledge", "payload": {"content": "ledger fact", "source": "docs/ledger.md", "class": class}
            }),
            true,
        );
        let pid = result_payload(&submitted)["id"]
            .as_str()
            .unwrap()
            .to_string();
        let approved = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.approve",
            json!({"id": pid}),
            false,
        );
        let approved = result_payload(&approved);
        let active_id =
            wicked_estate_core::SymbolId(approved["active_id"].as_str().unwrap().to_string());
        let node = know.node(&active_id).unwrap().unwrap();
        assert_eq!(
            node.kind,
            wicked_estate_core::NodeKind::Other(format!("k{class}"))
        );
        let node = wicked_estate_knowledge::KNode::from_node(&node).unwrap();
        assert_eq!(node.scope, "");
        assert_eq!(node.source, "docs/ledger.md");
    }
    assert_eq!(know.count(None, None).unwrap(), 4);
}

#[test]
fn proposal_approve_memory_and_policy_routing_unchanged() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    for (kind, payload, outcome) in [
        (
            "memory",
            json!({"content": "codex needs workspace-write", "tier": "semantic"}),
            "promoted",
        ),
        (
            "policy:security",
            json!({"rule": "no secrets in logs", "severity": "error"}),
            "handed_off",
        ),
    ] {
        let submitted = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({"kind_type": kind, "payload": payload}),
            true,
        );
        let pid = result_payload(&submitted)["id"]
            .as_str()
            .unwrap()
            .to_string();
        let approved = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.approve",
            json!({"id": pid}),
            false,
        );
        let approved = result_payload(&approved);
        assert_eq!(approved["outcome"], outcome);
        if kind.starts_with("policy:") {
            assert_eq!(approved["payload"], payload);
        }
        let listed = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.list",
            json!({"kind_type": kind, "state": "approved"}),
            true,
        );
        let listed = result_payload(&listed);
        assert_eq!(listed["proposals"].as_array().unwrap().len(), 1);
        assert_eq!(listed["proposals"][0]["id"], pid);
    }
    assert_eq!(mem.count().unwrap(), 1);
    assert_eq!(know.count(None, None).unwrap(), 0);
    let recalled = MemoryApi::recall(
        &mem,
        &wicked_estate_memory_core::RecallQuery::new("codex workspace", "", 500, 1),
    )
    .unwrap();
    assert!(
        recalled
            .iter()
            .any(|r| r.content.contains("workspace-write"))
    );
    let listed = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.list",
        json!({"kind_type": "knowledge"}),
        true,
    );
    assert!(
        result_payload(&listed)["proposals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
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

#[test]
fn proposal_submit_validates_memory_tier_and_policy_severity_enums() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();

    // A drifted memory tier ("long_term"/"durable") fails loud at SUBMIT (so the worker corrects
    // now, not silently at approval).
    for bad in ["long_term", "durable"] {
        let resp = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({ "kind_type": "memory", "payload": { "content": "x", "tier": bad } }),
            false,
        );
        assert_eq!(
            resp["error"]["code"], -32602,
            "tier {bad:?} must be rejected; got {resp}"
        );
        assert!(
            resp["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("semantic"),
            "the error must name the valid tiers; got {resp}"
        );
    }

    // A drifted policy severity ("high"/"medium"/"warning") fails loud at SUBMIT.
    for bad in ["high", "medium", "warning"] {
        let resp = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({ "kind_type": "policy:security", "payload": { "rule": "x", "severity": bad } }),
            false,
        );
        assert_eq!(
            resp["error"]["code"], -32602,
            "severity {bad:?} must be rejected; got {resp}"
        );
        assert!(
            resp["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("info|warn|error|critical"),
            "the error must name the valid severities; got {resp}"
        );
    }

    // A PRESENT but NON-string tier/severity is invalid too (it must not bypass the guard and
    // fail only at approval).
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "memory", "payload": { "content": "x", "tier": 3 } }),
        false,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "a non-string tier must be rejected; got {resp}"
    );
    let resp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "policy:security", "payload": { "rule": "x", "severity": ["error"] } }),
        false,
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "a non-string severity must be rejected; got {resp}"
    );

    // A policy must name a VALID steering type — a bare "policy" or an invented suffix is
    // rejected at submit (it has no steering_type to land against otherwise).
    for bad_kind in [
        "policy",
        "policy:data-integrity",
        "policy:safety",
        "policy:consistency",
    ] {
        let resp = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({ "kind_type": bad_kind, "payload": { "rule": "x", "severity": "error" } }),
            false,
        );
        assert_eq!(
            resp["error"]["code"], -32602,
            "policy kind_type {bad_kind:?} must be rejected; got {resp}"
        );
    }
    // A valid policy:<type> submits fine.
    let okp = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "policy:architecture", "payload": { "rule": "x", "severity": "error" } }),
        false,
    );
    assert!(
        okp.get("error").is_none(),
        "a valid policy:<type> must submit; got {okp}"
    );

    // Valid values still submit fine.
    let ok = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "memory", "payload": { "content": "x", "tier": "semantic" } }),
        false,
    );
    assert!(
        ok.get("error").is_none(),
        "a valid tier must submit; got {ok}"
    );
    let ok = call(
        &store,
        &mut mem,
        &mut know,
        "proposal.submit",
        json!({ "kind_type": "policy:security", "payload": { "rule": "x", "severity": "error" } }),
        false,
    );
    assert!(
        ok.get("error").is_none(),
        "a valid severity must submit; got {ok}"
    );
}

#[test]
fn proposal_submit_validates_reach_per_kind() {
    let store = SqliteStore::in_memory().unwrap();
    let mut mem = MemoryEngine::in_memory().unwrap();
    let mut know = KnowledgeEngine::in_memory().unwrap();
    let cases = [
        (
            "memory",
            json!({"content": "prefers tables in replies", "reach": "operator"}),
            true,
        ),
        (
            "memory",
            json!({"content": "prefers tables in replies", "reach": "pattern"}),
            true,
        ),
        (
            "memory",
            json!({"content": "prefers tables in replies", "reach": "everywhere"}),
            false,
        ),
        (
            "policy:development",
            json!({"rule": "always rebase", "reach": "operator"}),
            true,
        ),
        (
            "policy:development",
            json!({"rule": "always rebase", "reach": "project"}),
            true,
        ),
        (
            "policy:development",
            json!({"rule": "always rebase", "reach": "pattern"}),
            false,
        ),
        (
            "policy:development",
            json!({"rule": "always rebase", "reach": 3}),
            false,
        ),
    ];
    for (kind_type, payload, ok) in cases {
        let resp = call(
            &store,
            &mut mem,
            &mut know,
            "proposal.submit",
            json!({"kind_type": kind_type, "payload": payload, "facets": {"project": "wicked"}}),
            true,
        );
        if ok {
            assert!(resp.get("error").is_none(), "{kind_type} {payload}: {resp}");
        } else {
            assert_eq!(
                resp["error"]["code"], -32602,
                "{kind_type} {payload}: {resp}"
            );
            assert!(
                resp["error"]["message"].as_str().unwrap().contains("reach"),
                "{resp}"
            );
        }
    }
}
