//! `rules.recall` — faceted, severity-ordered recall of conformance `Rule` nodes (arch-R2).
//!
//! The deterministic-guardrail lane (wicked-governance in wicked-core) persists every
//! `ConformanceRule` as a native `NodeKind::Rule` node on the shared estate graph, keyed by the
//! synthetic symbol `wicked-apps synthetic conformance_rule/<id>:` with every field serialized
//! into `Node.metadata` (the retired `conformance-rules` wire schema — the contract garden and
//! the QE surfaces consume). Until this tool existed, that recall path (`recall_rules`) was
//! **Rust-only**: the sole MCP rules surface, [`crate::RulesInventory`], lists `RuleSet` engine
//! nodes, so ungrouped conformance rules were invisible to every non-Rust consumer.
//!
//! This tool ports `recall_rules`' facet semantics onto the read-only MCP surface:
//!
//! * `language` / `layer` / `framework` are **wildcard facets** — a rule with the facet ABSENT
//!   applies to all values of it (matches any query); a query that omits the facet matches all
//!   rules.
//! * `severity` / `rule_type` are **exact** matches.
//! * `scope` restricts to a scope subtree by canonical path prefix (the estate-wide
//!   `SymbolQuery::scope_prefix` predicate, e.g. `"wiki:architecture"`).
//! * Results are ordered severity-first (critical → error → warn → info) then rule id —
//!   deterministic, enforcement-ready.
//! * `retired` rules are withdrawn from recall (same funnel as the Rust-side `recall_rules`).
//!
//! Agent-behavior rules honored: R1 (empty result = successful response + diagnostic, NEVER
//! `isError`; invalid facet values likewise), R4 (output capped via `limit`, truncation is loud),
//! R5 (staleness note emitted for the MCP layer to enrich).
//!
//! There is deliberately **no** `rules.write` counterpart on the MCP surface: git-tracked docs are
//! the source of truth for rules, and promotion to an enforceable `Rule` node happens only via a
//! human-merged doc PR flowing through the governance ingest (`wicked-core rules ingest`) — an
//! agent must not author the gate that later judges its own runs (evaluator≠creator, ADR-012).

use serde::Deserialize;
use serde_json::{Value, json};
use wicked_estate_core::{
    GraphRead, NodeKind, Result, RetrievalResult, RetrievalTool, Symbol, SymbolQuery,
};

/// Symbol scheme wicked-apps-core mints synthetic (non-source) symbols under. Kept in sync with
/// `wicked_apps_core::SYMBOL_SCHEME` — the value is part of the persisted symbol id, so it is a
/// wire contract, not an implementation detail.
const WICKED_APPS_SCHEME: &str = "wicked-apps";
/// Symbol-namespace prefix for conformance-rule symbols (`conformance_rule/<id>`), matching
/// `wicked_governance::conformance::CONFORMANCE_RULE`.
const CONFORMANCE_RULE: &str = "conformance_rule";

/// Default result cap (R4) — rules are small JSON objects; 100 stays well under the ~25K budget.
const DEFAULT_LIMIT: usize = 100;
/// Hard result cap (R4).
const MAX_LIMIT: usize = 500;

const VALID_SEVERITIES: [&str; 4] = ["info", "warn", "error", "critical"];
const VALID_RULE_TYPES: [&str; 2] = ["pattern", "policy"];

/// Descending severity rank for recall ordering (mirrors `ConfSeverity::rank`). Unknown strings
/// (a producer newer than this reader) rank lowest rather than failing the whole recall.
fn severity_rank(severity: Option<&str>) -> u8 {
    match severity {
        Some("critical") => 4,
        Some("error") => 3,
        Some("warn") => 2,
        Some("info") => 1,
        _ => 0,
    }
}

/// The facet slice of a persisted `ConformanceRule` this tool filters/orders on. Deserialized
/// leniently from `Node.metadata` (strings, not the producer's enums): estate is a READER of the
/// conformance wire schema — strict validation lives at wicked-governance's fail-closed write
/// boundary, and a reader that hard-errors on a newer producer's vocabulary would take the whole
/// recall down with it.
#[derive(Debug, Deserialize)]
struct RuleView {
    id: String,
    #[serde(default)]
    rule_type: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    targets: TargetsView,
    #[serde(default)]
    retired: bool,
}

#[derive(Debug, Default, Deserialize)]
struct TargetsView {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    framework: Option<String>,
}

/// Wildcard facet match (ported from `recall_rules`): a query that omits the facet matches all
/// rules; a rule whose facet is ABSENT applies broadly and matches any query value.
fn facet_matches(rule_facet: &Option<String>, query: &Option<String>) -> bool {
    match query {
        None => true,
        Some(qv) => match rule_facet {
            None => true,
            Some(rv) => rv == qv,
        },
    }
}

fn opt_str(request: &Value, key: &str) -> Option<String> {
    request
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Faceted recall of conformance rules over MCP. See the module docs for semantics.
#[derive(Debug, Default)]
pub struct RulesRecall;

impl RetrievalTool for RulesRecall {
    fn name(&self) -> &str {
        "rules.recall"
    }

    fn description(&self) -> &str {
        "Recall the conformance rules (native Rule nodes, PAT-*/POL-* ids) that apply to a query \
         slice. language/layer/framework are wildcard facets (a rule without the facet applies to \
         all values), severity/rule_type are exact, scope restricts to a scope subtree by prefix. \
         Results are severity-ordered (critical\u{2192}info) then id. Read-only: rules are \
         authored via git-tracked docs + `wicked-core rules ingest`, never over MCP."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        // Facet extraction. Invalid enum values are an HONEST empty result + diagnostic, never
        // isError (R1) — same pattern as SearchEntity's missing-name handling.
        let severity = opt_str(request, "severity");
        if let Some(s) = &severity {
            if !VALID_SEVERITIES.contains(&s.as_str()) {
                return Ok(RetrievalResult {
                    content: json!({ "rules": [], "total": 0, "returned": 0 }),
                    diagnostics: vec![format!(
                        "rules.recall: invalid severity {s:?} — expected one of {VALID_SEVERITIES:?}"
                    )],
                });
            }
        }
        let rule_type = opt_str(request, "rule_type");
        if let Some(t) = &rule_type {
            if !VALID_RULE_TYPES.contains(&t.as_str()) {
                return Ok(RetrievalResult {
                    content: json!({ "rules": [], "total": 0, "returned": 0 }),
                    diagnostics: vec![format!(
                        "rules.recall: invalid rule_type {t:?} — expected one of {VALID_RULE_TYPES:?}"
                    )],
                });
            }
        }
        let language = opt_str(request, "language");
        let layer = opt_str(request, "layer");
        let framework = opt_str(request, "framework");
        let scope = opt_str(request, "scope");
        let limit = request
            .get("limit")
            .and_then(|v| v.as_u64())
            .map_or(DEFAULT_LIMIT, |n| (n as usize).min(MAX_LIMIT))
            .max(1);

        // Index-only Rule-node scan (no FTS, no traversal). `scope` pushes the estate-wide
        // subtree predicate into the store BEFORE any ranking, so scoped recall never leaks
        // another scope's rules.
        let nodes = store.find_symbols(&SymbolQuery {
            kinds: vec![NodeKind::Rule],
            scope_prefix: scope.clone(),
            ..Default::default()
        })?;

        let mut foreign = 0usize; // Rule nodes that are NOT conformance rules (e.g. W15 rules-engine artifacts)
        let mut undecodable = 0usize; // conformance-symbol nodes whose metadata failed to decode
        let mut retired = 0usize;
        let mut matched: Vec<(RuleView, Value)> = Vec::new();

        for node in nodes {
            // A shared estate store may hold other `NodeKind::Rule` nodes (estate's own rules-engine
            // extractors mint them from DRL/DMN/… sources). Only conformance rules carry the
            // `conformance_rule/<id>` synthetic symbol (node.name == rule id) — identify by that
            // round-trip and COUNT foreign nodes instead of failing on them.
            if node.symbol
                != Symbol::synthetic(
                    WICKED_APPS_SCHEME,
                    format!("{CONFORMANCE_RULE}/{}", node.name),
                )
                .id()
            {
                foreign += 1;
                continue;
            }
            let meta = Value::Object(node.metadata.clone());
            let view: RuleView = match serde_json::from_value(meta.clone()) {
                Ok(v) => v,
                Err(_) => {
                    undecodable += 1;
                    continue;
                }
            };
            if view.retired {
                retired += 1;
                continue;
            }
            if facet_matches(&view.targets.language, &language)
                && facet_matches(&view.targets.layer, &layer)
                && facet_matches(&view.targets.framework, &framework)
                && severity
                    .as_deref()
                    .is_none_or(|s| view.severity.as_deref() == Some(s))
                && rule_type
                    .as_deref()
                    .is_none_or(|t| view.rule_type.as_deref() == Some(t))
            {
                matched.push((view, meta));
            }
        }

        // Deterministic enforcement-ready ordering: severity rank desc, then id asc.
        matched.sort_by(|(a, _), (b, _)| {
            severity_rank(b.severity.as_deref())
                .cmp(&severity_rank(a.severity.as_deref()))
                .then_with(|| a.id.cmp(&b.id))
        });

        let total = matched.len();
        let mut diagnostics = vec![crate::staleness_note()];
        if total > limit {
            diagnostics.push(format!(
                "rules.recall: output capped at {limit} of {total} matched rules (R4) — narrow \
                 the facets or raise 'limit' (max {MAX_LIMIT})"
            ));
            matched.truncate(limit);
        }
        if undecodable > 0 {
            diagnostics.push(format!(
                "rules.recall: skipped {undecodable} conformance Rule node(s) whose metadata did \
                 not decode as a ConformanceRule — inspect them via SearchEntity/RetrieveEntity"
            ));
        }
        if total == 0 {
            diagnostics.push(format!(
                "rules.recall: no active conformance rules matched (facets: language={language:?}, \
                 layer={layer:?}, framework={framework:?}, severity={severity:?}, \
                 rule_type={rule_type:?}, scope={scope:?}). The graph holds {retired} retired \
                 conformance rule(s) and {foreign} non-conformance Rule node(s) (rules-engine \
                 artifacts — see RulesInventory). Rules are populated from git-tracked docs via \
                 `wicked-core rules ingest`, never over MCP."
            ));
        }

        let rules: Vec<Value> = matched.into_iter().map(|(_, meta)| meta).collect();
        let returned = rules.len();
        Ok(RetrievalResult {
            content: json!({ "rules": rules, "total": total, "returned": returned }),
            diagnostics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{GraphWrite, Language, Location, Node, Scope, Span};
    use wicked_estate_store::MemStore;

    fn conformance_node(
        id: &str,
        rule_type: &str,
        severity: &str,
        language: Option<&str>,
        retired: bool,
    ) -> Node {
        let symbol = Symbol::synthetic(WICKED_APPS_SCHEME, format!("{CONFORMANCE_RULE}/{id}")).id();
        let mut node = Node::new(
            symbol,
            NodeKind::Rule,
            id,
            Language::new(WICKED_APPS_SCHEME),
            Location::new(format!("{CONFORMANCE_RULE}/{id}"), Span::ZERO),
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
            "confidence": 0.72,
            "targets": targets,
            "provenance": { "source": "test", "source_kinds": [] },
            "retired": retired,
        });
        if let Value::Object(map) = meta {
            node.metadata = map;
        }
        node
    }

    /// A foreign Rule node — minted by a rules-engine EXTRACTOR (own scheme), not governance.
    fn foreign_rule_node() -> Node {
        Node::new(
            Symbol::synthetic("drl", "pricing-rule-7").id(),
            NodeKind::Rule,
            "pricing-rule-7",
            Language::new("drl"),
            Location::new("rules/pricing.drl", Span::ZERO),
        )
    }

    fn store_with(nodes: Vec<Node>) -> MemStore {
        let mut s = MemStore::new();
        s.begin_batch().unwrap();
        s.upsert_nodes(&nodes).unwrap();
        s.commit_batch().unwrap();
        s
    }

    #[test]
    fn recall_orders_severity_desc_then_id() {
        let store = store_with(vec![
            conformance_node("PAT-002", "pattern", "warn", None, false),
            conformance_node("POL-001", "policy", "critical", None, false),
            conformance_node("PAT-001", "pattern", "warn", None, false),
        ]);
        let res = RulesRecall.invoke(&store, &json!({})).unwrap();
        let ids: Vec<&str> = res.content["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["POL-001", "PAT-001", "PAT-002"],
            "critical first, then id-ordered within a severity"
        );
        assert_eq!(res.content["total"].as_u64().unwrap(), 3);
    }

    #[test]
    fn recall_language_facet_is_wildcard() {
        let store = store_with(vec![
            conformance_node("PAT-100", "pattern", "error", Some("rust"), false),
            conformance_node("PAT-101", "pattern", "error", Some("python"), false),
            conformance_node("PAT-102", "pattern", "error", None, false), // wildcard rule
        ]);
        let res = RulesRecall
            .invoke(&store, &json!({ "language": "rust" }))
            .unwrap();
        let ids: Vec<&str> = res.content["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["PAT-100", "PAT-102"],
            "exact language + facet-absent wildcard match; other languages excluded"
        );
    }

    #[test]
    fn recall_severity_and_rule_type_are_exact() {
        let store = store_with(vec![
            conformance_node("PAT-200", "pattern", "critical", None, false),
            conformance_node("POL-200", "policy", "critical", None, false),
            conformance_node("POL-201", "policy", "info", None, false),
        ]);
        let res = RulesRecall
            .invoke(
                &store,
                &json!({ "severity": "critical", "rule_type": "policy" }),
            )
            .unwrap();
        let ids: Vec<&str> = res.content["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["POL-200"]);
    }

    #[test]
    fn recall_skips_retired_and_counts_foreign() {
        let store = store_with(vec![
            conformance_node("PAT-300", "pattern", "warn", None, true), // retired
            foreign_rule_node(),
        ]);
        let res = RulesRecall.invoke(&store, &json!({})).unwrap();
        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        // Empty is a DIAGNOSTIC, and it reports both the retired and the foreign counts honestly.
        let diag = res.diagnostics.join("\n");
        assert!(
            diag.contains("1 retired") && diag.contains("1 non-conformance"),
            "diagnostic must report retired + foreign counts; got: {diag}"
        );
    }

    #[test]
    fn recall_empty_store_is_diagnostic_never_error() {
        let store = MemStore::new();
        let res = RulesRecall.invoke(&store, &json!({})).unwrap();
        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("rules.recall: no active conformance rules matched")),
            "empty result must carry the explanatory diagnostic (R1); got {:?}",
            res.diagnostics
        );
    }

    #[test]
    fn recall_invalid_severity_is_diagnostic_never_error() {
        let store = store_with(vec![conformance_node(
            "PAT-400", "pattern", "warn", None, false,
        )]);
        let res = RulesRecall
            .invoke(&store, &json!({ "severity": "fatal" }))
            .unwrap();
        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("invalid severity")),
            "invalid enum value must be named in a diagnostic; got {:?}",
            res.diagnostics
        );
    }

    #[test]
    fn recall_caps_output_loudly() {
        let nodes: Vec<Node> = (0..5)
            .map(|i| conformance_node(&format!("PAT-90{i}"), "pattern", "warn", None, false))
            .collect();
        let store = store_with(nodes);
        let res = RulesRecall.invoke(&store, &json!({ "limit": 2 })).unwrap();
        assert_eq!(res.content["total"].as_u64().unwrap(), 5, "total = matched");
        assert_eq!(
            res.content["returned"].as_u64().unwrap(),
            2,
            "returned = capped"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("capped")),
            "truncation must be loud (R4); got {:?}",
            res.diagnostics
        );
    }

    #[test]
    fn recall_scope_restricts_to_subtree() {
        let mut scoped = conformance_node("POL-500", "policy", "error", None, false);
        scoped.scope = Scope::parse("wiki:architecture");
        let unscoped = conformance_node("POL-501", "policy", "error", None, false);
        let store = store_with(vec![scoped, unscoped]);

        let res = RulesRecall
            .invoke(&store, &json!({ "scope": "wiki:architecture" }))
            .unwrap();
        let ids: Vec<&str> = res.content["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["POL-500"],
            "scope prefix must exclude root-scoped rules"
        );

        // No scope → both.
        let res_all = RulesRecall.invoke(&store, &json!({})).unwrap();
        assert_eq!(res_all.content["total"].as_u64().unwrap(), 2);
    }
}
