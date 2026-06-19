//! W15.8 — OPA/Rego (`.rego`) **rules-layer** extractor (heuristic regex tier).
//!
//! Issue #22 asked for a Rego tree-sitter grammar emitting W15 rules nodes. The grammar half is
//! already covered: `.rego` is parsed by the production `arborium-rego` tree-sitter grammar
//! (see [`crate::treesitter`]), which yields the *code* graph (rules-as-functions, imports). What
//! that path does NOT do is map a policy into the **W15 rules graph** (`RuleSet`/`Rule`/`Condition`/
//! `Action`/`Fact`) — and the numeric AC ("≥1 Rule, ≥1 Condition, ≥1 RuleSet extracted") is about
//! that graph.
//!
//! This extractor fills exactly that gap, following the proven [`OdmExtractor`](crate::OdmExtractor)
//! / [`DrlExtractor`](crate::DrlExtractor) heuristic pattern. It runs as a **supplementary** pass on
//! top of the tree-sitter parse (the same way [`CicsSqlExtractor`](crate::CicsSqlExtractor) layers
//! EXEC CICS/SQL onto COBOL), so a `.rego` file keeps its code symbols AND gains rules nodes that
//! `RulesInventory` can surface. Mapping:
//!
//! - `package authz`                     → [`NodeKind::RuleSet`]
//! - top-level `allow`/`deny`/named rule → [`NodeKind::Rule`]
//! - rule body `{ … }` (or `if …`)       → [`NodeKind::Condition`]
//! - rule head value (`:= v` / `= v` / `contains x`) → [`NodeKind::Action`]
//! - `input.*` / `data.*` references     → [`NodeKind::Fact`]
//!
//! All edges carry [`ResolutionTier::Heuristic`]; all IDs use `Symbol::synthetic` (ADR-002). It
//! captures rule STRUCTURE, not full Rego datalog semantics.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
    Result, SourceFile, Span, Symbol,
};

const LANG: &str = "opa-rego";

/// `package authz.rbac` — one per file → the RuleSet.
static RE_PACKAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^package[ \t]+([A-Za-z_][\w.]*)").expect("RE_PACKAGE must compile")
});
/// A top-level rule head: column-0 identifier (optionally `default`-prefixed), capturing the rest of
/// the head line. Body statements are indented, so column-0 anchoring isolates rule definitions.
/// `package`/`import` lines also match this shape and are filtered in code.
static RE_RULE_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(default[ \t]+)?([a-z_]\w*)([^\n]*)$").expect("RE_RULE_HEAD must compile")
});
/// `input.user.role`, `data.roles[_]` — input/document references → Fact paths.
static RE_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b((?:input|data)(?:\.[A-Za-z_]\w*)+)").expect("RE_REF must compile")
});

fn byte_span(start: usize, end: usize) -> Span {
    Span {
        start_byte: start as u32,
        end_byte: end as u32,
        start_line: 0,
        start_col: 0,
        end_line: 0,
        end_col: 0,
    }
}

/// Given the byte index of an opening `{` in `text`, return (content_without_braces, end_byte_after
/// matching `}`). Falls back to end-of-text if unbalanced.
fn brace_block(text: &str, open: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let content = text[open + 1..i].trim().to_string();
                    return (content, i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (text[open..].trim().to_string(), text.len())
}

/// Heuristic regex extractor for the OPA/Rego rules layer.
pub struct RegoRulesExtractor;

impl RegoRulesExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RegoRulesExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for RegoRulesExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(LANG)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let text = &file.text;
        let mut nodes = Vec::new();
        let mut local_edges = Vec::new();

        // 1. package → RuleSet
        let ruleset_sym = RE_PACKAGE.captures(text).map(|c| {
            let pkg = c[1].to_string();
            let sym = Symbol::synthetic("rego", format!("{}::package::{}", file.path, pkg)).id();
            let mut n = Node::new(
                sym.clone(),
                NodeKind::RuleSet,
                pkg.clone(),
                Language::new(LANG),
                Location::new(&file.path, Span::ZERO),
            );
            n.signature = Some(format!("package {pkg}"));
            nodes.push(n);
            sym
        });

        // 2. Collect top-level rule-head match positions (skipping package/import keywords).
        let heads: Vec<(usize, usize, String, String)> = RE_RULE_HEAD
            .captures_iter(text)
            .filter_map(|c| {
                let m = c.get(0).unwrap();
                let name = c.get(2).unwrap().as_str().to_string();
                if matches!(
                    name.as_str(),
                    "package" | "import" | "else" | "some" | "every"
                ) {
                    return None;
                }
                let rest = c.get(3).map(|r| r.as_str().to_string()).unwrap_or_default();
                // A rule head either has a body (`{`), an `if`, an assignment, a `contains`, or a
                // ref/partial (`[`). A bare top-level identifier with none of these is not a rule.
                let looks_like_rule = rest.contains('{')
                    || rest.contains(" if")
                    || rest.contains(":=")
                    || rest.contains('=')
                    || rest.contains("contains")
                    || rest.contains('[')
                    || c.get(1).is_some(); // `default <name>` is always a rule
                if !looks_like_rule {
                    return None;
                }
                Some((m.start(), m.end(), name, rest))
            })
            .collect();

        // 3. Per rule head: Rule + (Condition from body) + (Action from head value).
        let mut seen_rule = BTreeSet::new();
        for (idx, (start, head_end, name, rest)) in heads.iter().enumerate() {
            let extent_end = heads.get(idx + 1).map(|h| h.0).unwrap_or(text.len());

            // Dedup rules sharing a name (Rego allows multiple `allow { … }` clauses) into one Rule
            // node; each clause still contributes its own Condition/Action below.
            let rule_sym = Symbol::synthetic("rego", format!("{}::rule::{}", file.path, name)).id();
            if seen_rule.insert(name.clone()) {
                let mut rule_node = Node::new(
                    rule_sym.clone(),
                    NodeKind::Rule,
                    name.clone(),
                    Language::new(LANG),
                    Location::new(&file.path, byte_span(*start, *head_end)),
                );
                rule_node.signature = Some(format!("{name}{rest}").trim().to_string());
                nodes.push(rule_node);
                if let Some(rs) = &ruleset_sym {
                    local_edges.push(Edge::new(
                        rs.clone(),
                        rule_sym.clone(),
                        EdgeKind::Contains,
                        ResolutionTier::Heuristic,
                        "opa-rego",
                    ));
                }
            }

            // Action: explicit head value (`:= v` / standalone `= v`) or `contains x`.
            let action_text = if let Some(p) = rest.find(":=") {
                Some(
                    rest[p + 2..]
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                )
            } else if let Some(p) = rest.find("contains") {
                Some(
                    rest[p..]
                        .split(" if")
                        .next()
                        .unwrap_or("")
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                )
            } else {
                // standalone `=` (unification), not `==`
                rest.match_indices('=').find_map(|(p, _)| {
                    let after = rest.as_bytes().get(p + 1).copied();
                    let before = if p > 0 {
                        rest.as_bytes().get(p - 1).copied()
                    } else {
                        None
                    };
                    if after == Some(b'=') || before == Some(b'=') || before == Some(b'!') {
                        None
                    } else {
                        Some(
                            rest[p + 1..]
                                .split('{')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                        )
                    }
                })
            };
            if let Some(at) = action_text {
                if !at.is_empty() {
                    let act_sym = Symbol::synthetic(
                        "rego",
                        format!("{}::action::{}::{}", file.path, name, idx),
                    )
                    .id();
                    let mut an = Node::new(
                        act_sym.clone(),
                        NodeKind::Action,
                        format!("{name}::value"),
                        Language::new(LANG),
                        Location::new(&file.path, Span::ZERO),
                    );
                    an.signature = Some(at);
                    nodes.push(an);
                    local_edges.push(Edge::new(
                        rule_sym.clone(),
                        act_sym,
                        EdgeKind::Contains,
                        ResolutionTier::Heuristic,
                        "opa-rego",
                    ));
                }
            }

            // Condition: the brace body `{ … }`, else a one-line `if <expr>` form.
            let cond_text = if let Some(rel) = text[*start..extent_end].find('{') {
                let (content, _) = brace_block(text, *start + rel);
                Some(content)
            } else {
                rest.find(" if").map(|p| rest[p + 3..].trim().to_string())
            };
            if let Some(ct) = cond_text {
                if !ct.is_empty() {
                    let cond_sym = Symbol::synthetic(
                        "rego",
                        format!("{}::condition::{}::{}", file.path, name, idx),
                    )
                    .id();
                    let mut cn = Node::new(
                        cond_sym.clone(),
                        NodeKind::Condition,
                        format!("{name}::body"),
                        Language::new(LANG),
                        Location::new(&file.path, Span::ZERO),
                    );
                    cn.signature = Some(ct);
                    nodes.push(cn);
                    local_edges.push(Edge::new(
                        rule_sym.clone(),
                        cond_sym,
                        EdgeKind::Contains,
                        ResolutionTier::Heuristic,
                        "opa-rego",
                    ));
                }
            }
        }

        // 4. input.* / data.* references → Fact (deduped by full dotted path).
        if let Some(rs) = &ruleset_sym {
            let mut seen_fact = BTreeSet::new();
            for c in RE_REF.captures_iter(text) {
                let path = c[1].to_string();
                if !seen_fact.insert(path.clone()) {
                    continue;
                }
                let fact_sym =
                    Symbol::synthetic("rego", format!("{}::fact::{}", file.path, path)).id();
                let mut fact_node = Node::new(
                    fact_sym.clone(),
                    NodeKind::Fact,
                    path.clone(),
                    Language::new(LANG),
                    Location::new(&file.path, Span::ZERO),
                );
                fact_node.signature = Some(path);
                nodes.push(fact_node);
                local_edges.push(Edge::new(
                    rs.clone(),
                    fact_sym,
                    EdgeKind::Contains,
                    ResolutionTier::Heuristic,
                    "opa-rego",
                ));
            }
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rego(text: &str) -> SourceFile {
        SourceFile {
            path: "authz.rego".to_string(),
            language: Language::new(LANG),
            text: text.to_string(),
        }
    }

    // AC fixture: an OPA policy with `allow` + `deny` rules and `input.*` references.
    const SAMPLE: &str = r#"package authz.rbac

import future.keywords.if
import future.keywords.contains

default allow := false

allow if {
    input.user.role == "admin"
}

allow if {
    input.method == "GET"
    input.path == "/public"
}

deny contains msg if {
    input.user.banned == true
    msg := "user is banned"
}
"#;

    #[test]
    fn package_emits_ruleset() {
        let ex = RegoRulesExtractor::new().extract(&rego(SAMPLE)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet && n.name == "authz.rbac"),
            "expected a RuleSet for the package"
        );
    }

    #[test]
    fn allow_and_deny_become_rules() {
        let ex = RegoRulesExtractor::new().extract(&rego(SAMPLE)).unwrap();
        let rules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .map(|n| n.name.as_str())
            .collect();
        assert!(rules.contains(&"allow"), "got {rules:?}");
        assert!(rules.contains(&"deny"), "got {rules:?}");
        // The two `allow` clauses dedup to a single Rule node.
        assert_eq!(rules.iter().filter(|r| **r == "allow").count(), 1);
    }

    #[test]
    fn ac_minimum_counts_met() {
        // Issue #22 AC: ≥1 Rule, ≥1 Condition, ≥1 RuleSet extracted.
        let ex = RegoRulesExtractor::new().extract(&rego(SAMPLE)).unwrap();
        let n = |k: NodeKind| ex.nodes.iter().filter(|x| x.kind == k).count();
        assert!(n(NodeKind::RuleSet) >= 1, "RuleSet >= 1");
        assert!(n(NodeKind::Rule) >= 1, "Rule >= 1");
        assert!(n(NodeKind::Condition) >= 1, "Condition >= 1");
    }

    #[test]
    fn input_refs_become_facts() {
        let ex = RegoRulesExtractor::new().extract(&rego(SAMPLE)).unwrap();
        let facts: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Fact)
            .map(|n| n.name.as_str())
            .collect();
        assert!(facts.contains(&"input.user.role"), "got {facts:?}");
        assert!(facts.contains(&"input.method"), "got {facts:?}");
    }

    #[test]
    fn default_rule_has_action_no_condition() {
        let ex = RegoRulesExtractor::new().extract(&rego(SAMPLE)).unwrap();
        // `default allow := false` contributes an Action (value `false`).
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Action && n.signature.as_deref() == Some("false")),
            "expected an Action carrying the default value `false`"
        );
    }

    #[test]
    fn condition_carries_body_text() {
        let ex = RegoRulesExtractor::new().extract(&rego(SAMPLE)).unwrap();
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::Condition
                && n.signature
                    .as_deref()
                    .unwrap_or("")
                    .contains("input.user.role")),
            "a Condition should carry its rule body text"
        );
    }
}
