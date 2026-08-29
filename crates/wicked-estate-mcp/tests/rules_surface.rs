//! The rules MCP surface — arch-R2 (`rules.recall` works over the wire) and arch-R8/ADR-012
//! (the authorship contract: there is deliberately NO rule-mutation tool on the MCP surface).
//!
//! The ADR-012 half is a CONFORMANCE test on the actual advertised tool registry, not on a
//! hand-maintained list: an agent must be structurally unable to author (write / retire / update)
//! the deterministic rules that later gate its own runs. Rules enter the graph only via
//! git-tracked docs promoted through a human-merged PR and `wicked-core rules ingest`
//! (evaluator≠creator extended to guardrail authorship). If someone adds a mutating rules tool,
//! these tests are the tripwire.

use serde_json::{Value, json};
use wicked_estate_core::{
    GraphWrite, Language, Location, Node, NodeKind, RetrievalTool, Span, Symbol,
};
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_store::MemStore;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a conformance-rule node exactly as wicked-governance persists it: native
/// `NodeKind::Rule`, symbol `wicked-apps synthetic conformance_rule/<id>:`, the serialized rule
/// riding in `Node.metadata` (the retired `conformance-rules` wire schema).
fn conformance_rule_node(
    id: &str,
    rule_type: &str,
    severity: &str,
    language: Option<&str>,
) -> Node {
    let symbol = Symbol::synthetic("wicked-apps", format!("conformance_rule/{id}")).id();
    let mut node = Node::new(
        symbol,
        NodeKind::Rule,
        id,
        Language::new("wicked-apps"),
        Location::new(format!("conformance_rule/{id}"), Span::ZERO),
    );
    let mut targets = serde_json::Map::new();
    if let Some(l) = language {
        targets.insert("language".into(), l.into());
    }
    let meta = json!({
        "id": id,
        "rule_type": rule_type,
        "statement": format!("statement for {id}"),
        "severity": severity,
        "confidence": 0.9,
        "targets": targets,
        "provenance": { "source": "markdown", "ref": "docs/adr/ADR-012-rule-authorship.md", "source_kinds": ["doc"] },
        "retired": false,
    });
    if let Value::Object(map) = meta {
        node.metadata = map;
    }
    node
}

fn seeded_store() -> MemStore {
    let mut s = MemStore::new();
    s.begin_batch().unwrap();
    s.upsert_nodes(&[
        conformance_rule_node("POL-001", "policy", "critical", None),
        conformance_rule_node("PAT-002", "pattern", "warn", Some("rust")),
        conformance_rule_node("PAT-003", "pattern", "warn", Some("python")),
    ])
    .unwrap();
    s.commit_batch().unwrap();
    s
}

fn call_estate_only(store: &MemStore, id: u64, method: &str, params: Value) -> Value {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    handle_request_unified(
        store,
        &req,
        &McpContext::default(),
        None,
        None::<&dyn RetrievalTool>,
    )
}

/// tools/list names with ALL domain handles active — the fullest advertised surface.
fn full_tool_roster() -> Vec<String> {
    let store = MemStore::new();
    let mut memory = MemoryEngine::in_memory().unwrap();
    let mut knowledge = KnowledgeEngine::in_memory().unwrap();
    let mut handles = DomainHandles {
        memory: &mut memory as &mut dyn MemoryApi<Error = anyhow::Error>,
        knowledge: &mut knowledge as &mut dyn KnowledgeApi,
    };
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} });
    let resp = handle_request_unified(
        &store,
        &req,
        &McpContext::default(),
        Some(&mut handles),
        None::<&dyn RetrievalTool>,
    );
    resp["result"]["tools"]
        .as_array()
        .expect("tools/list must return an array")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

// ── arch-R2: rules.recall over the wire ──────────────────────────────────────

/// AW-4 acceptance: `rules.recall` works over MCP against a test store — non-Rust consumers can
/// list/recall Rule nodes, severity-ordered, isError=false.
#[test]
fn rules_recall_over_mcp_returns_severity_ordered_rules() {
    let store = seeded_store();
    let resp = call_estate_only(
        &store,
        10,
        "tools/call",
        json!({ "name": "rules.recall", "arguments": {} }),
    );

    assert!(
        !resp["result"]["isError"].as_bool().unwrap_or(true),
        "isError must be false: {resp}"
    );
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");
    let ids: Vec<&str> = parsed["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec!["POL-001", "PAT-002", "PAT-003"],
        "critical first, then id order within a severity"
    );
    // The full wire schema rides through — statement + provenance are what garden/CI cite.
    let first = &parsed["rules"][0];
    assert_eq!(
        first["statement"].as_str().unwrap(),
        "statement for POL-001"
    );
    assert_eq!(first["severity"].as_str().unwrap(), "critical");
}

/// AW-4 acceptance: facets filter over the wire (wildcard language + exact severity).
#[test]
fn rules_recall_over_mcp_applies_facets() {
    let store = seeded_store();
    let resp = call_estate_only(
        &store,
        11,
        "tools/call",
        json!({ "name": "rules.recall", "arguments": { "language": "rust", "severity": "warn" } }),
    );
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    let ids: Vec<&str> = parsed["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec!["PAT-002"],
        "language=rust excludes the python rule; severity=warn excludes the critical policy"
    );
}

/// AW-4 acceptance: an empty result is a DIAGNOSTIC through the MCP envelope, never isError (R1).
#[test]
fn rules_recall_over_mcp_empty_is_diagnostic_never_error() {
    let store = MemStore::new(); // no rules at all
    let resp = call_estate_only(
        &store,
        12,
        "tools/call",
        json!({ "name": "rules.recall", "arguments": {} }),
    );
    assert!(
        !resp["result"]["isError"].as_bool().unwrap_or(true),
        "empty recall must NOT be isError (R1): {resp}"
    );
    let content = resp["result"]["content"].as_array().unwrap();
    // Diagnostics ride as the second text block in the MCP envelope.
    let diag_text = content
        .get(1)
        .and_then(|c| c["text"].as_str())
        .unwrap_or_default();
    assert!(
        diag_text.contains("no active conformance rules matched"),
        "the diagnostic must explain the empty result; got: {diag_text:?}"
    );
}

// ── arch-R8 / ADR-012: NO rule-mutation tool on the MCP surface ───────────────

/// The authorship-contract conformance test, asserted on the ACTUAL advertised tool registry:
/// `rules.recall` is present, and no advertised tool can mutate rules. Mutating verbs are matched
/// against every rules-ish tool name so a future `rules.write` / `rules.register` /
/// `RulesUpdate` … trips this test, whatever its spelling.
#[test]
fn tool_registry_exposes_no_rule_mutation_tool() {
    let roster = full_tool_roster();

    assert!(
        roster.iter().any(|n| n == "rules.recall"),
        "rules.recall must be advertised; roster: {roster:?}"
    );

    const MUTATING_VERBS: [&str; 10] = [
        "write", "register", "retire", "upsert", "update", "delete", "create", "mutate", "ingest",
        "set",
    ];
    for name in &roster {
        let lower = name.to_lowercase();
        if !lower.contains("rule") {
            continue;
        }
        for verb in MUTATING_VERBS {
            assert!(
                !lower.contains(verb),
                "ADR-012 violation: advertised tool '{name}' looks like a rule-mutation surface — \
                 rules are authored ONLY via git-tracked docs promoted through a human-merged PR \
                 (`wicked-core rules ingest`); there must be no MCP write path"
            );
        }
    }

    // The rules-ish read surface is EXACTLY these two.
    let rules_tools: Vec<&String> = roster
        .iter()
        .filter(|n| n.to_lowercase().contains("rule"))
        .collect();
    assert_eq!(
        rules_tools.len(),
        2,
        "exactly RulesInventory + rules.recall; got {rules_tools:?}"
    );
}

/// Belt and braces: a tools/call for the tool ADR-012 forbids must come back "unknown tool" —
/// with and without domain handles — never reach any handler.
#[test]
fn rules_write_call_is_unknown_tool() {
    let store = seeded_store();
    for name in ["rules.write", "rules.register", "rules.retire"] {
        let resp = call_estate_only(
            &store,
            13,
            "tools/call",
            json!({ "name": name, "arguments": { "id": "PAT-999", "statement": "evil" } }),
        );
        let err = resp
            .get("error")
            .unwrap_or_else(|| panic!("'{name}' must be a JSON-RPC unknown-tool error: {resp}"));
        assert_eq!(err["code"].as_i64().unwrap(), -32602, "tool {name}");
    }
    // And the seeded rules are untouched: recall still returns exactly the three seeds.
    let resp = call_estate_only(
        &store,
        14,
        "tools/call",
        json!({ "name": "rules.recall", "arguments": {} }),
    );
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["total"].as_u64().unwrap(), 3);
}
