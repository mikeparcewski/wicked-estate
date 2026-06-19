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
//! - top-level `allow`/`deny`/named rule → [`NodeKind::Rule`] (clauses sharing a name dedup)
//! - rule body `{ … }` (or `if …`)       → [`NodeKind::Condition`]
//! - rule head value (`:= v` / `= v` / `contains x`) → [`NodeKind::Action`]
//! - `input.*` / `data.*` references     → [`NodeKind::Fact`] (import targets excluded)
//!
//! Structural scanning runs over a comment-blanked, string-masked copy of the source (see
//! [`crate::rules_text`]) so keywords/braces inside comments or string literals can't mislead, and
//! the idiomatic multi-line `allow\n{ … }` rule form is recognized. All edges carry
//! [`ResolutionTier::Heuristic`]; all IDs use `Symbol::synthetic` (ADR-002). It captures rule
//! STRUCTURE, not full Rego datalog semantics.

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
    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // `content`: comments blanked. `scan`: also string literals masked. Structural matching runs
        // on `scan` (so `{`/`if`/`:=` inside a string or comment can't mislead); signatures slice
        // from `content`. Both are length-preserving, so offsets are interchangeable.
        let content = crate::rules_text::blank_hash_comments(&file.text);
        let scan = crate::rules_text::mask_strings(&content);
        let mut nodes = Vec::new();
        let mut local_edges = Vec::new();

        // 1. package → RuleSet
        let ruleset_sym = RE_PACKAGE.captures(&scan).map(|c| {
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

        // 2. Top-level rule-head candidates (column-0 lines). A candidate is a rule if its head line
        //    carries a marker, it is a `default` rule, OR a `{` opens on the next non-blank line
        //    (the idiomatic multi-line `allow\n{ … }` form). (start, head_end, name, rest, extent).
        let candidates: Vec<(usize, usize, String, String, bool)> = RE_RULE_HEAD
            .captures_iter(&scan)
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
                Some((m.start(), m.end(), name, rest, c.get(1).is_some()))
            })
            .collect();

        let heads: Vec<(usize, usize, String, String, usize)> = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, (start, head_end, name, rest, is_default))| {
                let extent_end = candidates.get(i + 1).map(|h| h.0).unwrap_or(scan.len());
                let next_line_brace = scan[*head_end..extent_end].trim_start().starts_with('{');
                let looks_like_rule = *is_default
                    || rest.contains('{')
                    || rest.contains(" if")
                    || rest.contains(":=")
                    || rest.contains('=')
                    || rest.contains("contains")
                    || rest.contains('[')
                    || next_line_brace;
                looks_like_rule.then(|| (*start, *head_end, name.clone(), rest.clone(), extent_end))
            })
            .collect();

        // 3. Per rule head: Rule + (Condition from body) + (Action from head value).
        let mut seen_rule = BTreeSet::new();
        for (idx, (start, head_end, name, _rest, extent_end)) in heads.iter().enumerate() {
            // Head line, strings intact, for signatures + value/condition detection.
            let head_line = content[*start..*head_end].trim();

            let rule_sym = Symbol::synthetic("rego", format!("{}::rule::{}", file.path, name)).id();
            if seen_rule.insert(name.clone()) {
                let mut rule_node = Node::new(
                    rule_sym.clone(),
                    NodeKind::Rule,
                    name.clone(),
                    Language::new(LANG),
                    Location::new(&file.path, byte_span(*start, *head_end)),
                );
                rule_node.signature = Some(head_line.to_string());
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
            let action_text = if let Some(p) = head_line.find(":=") {
                Some(value_before_brace(&head_line[p + 2..]))
            } else if let Some(p) = head_line.find("contains") {
                let seg = head_line[p..].split(" if").next().unwrap_or("");
                Some(value_before_brace(seg))
            } else {
                head_line.match_indices('=').find_map(|(p, _)| {
                    let bytes = head_line.as_bytes();
                    let after = bytes.get(p + 1).copied();
                    let before = p.checked_sub(1).and_then(|q| bytes.get(q).copied());
                    // Reject `==`, `!=`, `<=`, `>=` — only a single `=` (unification) is a value.
                    if matches!(after, Some(b'='))
                        || matches!(before, Some(b'=') | Some(b'!') | Some(b'<') | Some(b'>'))
                    {
                        None
                    } else {
                        Some(value_before_brace(&head_line[p + 1..]))
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

            // Condition: the brace body `{ … }` (string-aware match over `scan`, sliced from
            // `content`), else a one-line `if <expr>` form.
            let cond_text = if let Some(rel) = scan[*start..*extent_end].find('{') {
                let open = *start + rel;
                let end = crate::rules_text::match_brace_end(&scan, open);
                Some(content[open + 1..end].trim().to_string())
            } else {
                head_line
                    .find(" if")
                    .map(|p| head_line[p + 3..].trim().to_string())
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

        // 4. input.* / data.* references → Fact (deduped). Skip refs on `import` lines (an imported
        //    package is not an input fact). Facts are emitted regardless of a package; the Contains
        //    edge is added only when a RuleSet exists.
        let mut seen_fact = BTreeSet::new();
        for c in RE_REF.captures_iter(&scan) {
            let mo = c.get(1).unwrap();
            let line_start = scan[..mo.start()].rfind('\n').map(|n| n + 1).unwrap_or(0);
            if scan[line_start..].trim_start().starts_with("import") {
                continue;
            }
            let path = content[mo.start()..mo.end()].to_string();
            if !seen_fact.insert(path.clone()) {
                continue;
            }
            let fact_sym = Symbol::synthetic("rego", format!("{}::fact::{}", file.path, path)).id();
            let mut fact_node = Node::new(
                fact_sym.clone(),
                NodeKind::Fact,
                path.clone(),
                Language::new(LANG),
                Location::new(&file.path, Span::ZERO),
            );
            fact_node.signature = Some(path);
            nodes.push(fact_node);
            if let Some(rs) = &ruleset_sym {
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

    fn languages(&self) -> Vec<Language> {
        vec![Language::new(LANG)]
    }
}

/// The value text up to (but not including) a rule body `{`, trimmed.
fn value_before_brace(s: &str) -> String {
    s.split('{').next().unwrap_or("").trim().to_string()
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

    // ── Antagonist regression tests ──────────────────────────────────────────────────────────

    #[test]
    fn next_line_brace_rule_is_detected() {
        // Antagonist M5: idiomatic multi-line form with the brace on the next line.
        let src = "package p\n\nallow\n{\n    input.user == \"admin\"\n}\n";
        let ex = RegoRulesExtractor::new().extract(&rego(src)).unwrap();
        let n = |k: NodeKind| ex.nodes.iter().filter(|x| x.kind == k).count();
        assert_eq!(
            n(NodeKind::Rule),
            1,
            "the next-line-brace rule must be detected"
        );
        assert!(
            n(NodeKind::Condition) >= 1,
            "and its body becomes a Condition"
        );
    }

    #[test]
    fn brace_inside_string_does_not_truncate_condition() {
        // Antagonist M4: a `}` inside a string literal must not close the body early.
        let src = "package p\n\nallow if {\n    msg := \"has a } brace\"\n    input.user == \"admin\"\n}\n";
        let ex = RegoRulesExtractor::new().extract(&rego(src)).unwrap();
        let cond = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Condition)
            .expect("condition");
        assert!(
            cond.signature
                .as_deref()
                .unwrap_or("")
                .contains("input.user == "),
            "the real predicate after the string-brace must survive: {:?}",
            cond.signature
        );
    }

    #[test]
    fn import_paths_are_not_facts() {
        // Antagonist m1: `import data.lib.helpers` is not an input-document Fact.
        let src = "package p\n\nimport data.lib.helpers\n\nallow if {\n    input.ok == true\n}\n";
        let ex = RegoRulesExtractor::new().extract(&rego(src)).unwrap();
        let facts: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Fact)
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            !facts.contains(&"data.lib.helpers"),
            "import target leaked as Fact: {facts:?}"
        );
        assert!(
            facts.contains(&"input.ok"),
            "real input ref still a Fact: {facts:?}"
        );
    }

    #[test]
    fn facts_emitted_without_package() {
        // Antagonist m2: a package-less fragment still yields Facts (just no RuleSet edge).
        let src = "allow if {\n    input.user == \"admin\"\n}\n";
        let ex = RegoRulesExtractor::new().extract(&rego(src)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Fact && n.name == "input.user"),
            "facts must be emitted even without a package declaration"
        );
    }

    #[test]
    fn rule_keyword_inside_comment_is_ignored() {
        // A `#`-commented line that looks like a rule head must not become a Rule/Fact.
        let src =
            "package p\n\n# deny if { input.bad == true }\nallow if {\n    input.ok == true\n}\n";
        let ex = RegoRulesExtractor::new().extract(&rego(src)).unwrap();
        let rules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            rules,
            vec!["allow"],
            "commented `deny` must be ignored: {rules:?}"
        );
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Fact && n.name == "input.bad"),
            "commented input ref must not be a Fact"
        );
    }
}
