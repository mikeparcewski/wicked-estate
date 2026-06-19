//! W15.9 — Drools DRL (`.drl`) rules extractor (heuristic regex tier).
//!
//! Issue #21 scoped a full Drools ANTLR4 → tree-sitter port (estimated 2–5 weeks). This extractor
//! instead follows the proven [`OdmExtractor`](crate::OdmExtractor) IRL/BAL pattern — a heuristic
//! regex extractor that delivers testable DRL coverage now and slots into the same W15 rules graph:
//!
//! - `package com.example` → [`NodeKind::RuleSet`]
//! - `rule "Name" … when … then … end` → [`NodeKind::Rule`] + `when` → [`NodeKind::Condition`] +
//!   `then` → [`NodeKind::Action`]
//! - `declare Type … end` → [`NodeKind::Fact`]
//!
//! It captures rule STRUCTURE (the W15 node graph), not full DRL expression semantics. All edges
//! carry [`ResolutionTier::Heuristic`] (regex heuristics, not AST facts); all IDs use
//! `Symbol::synthetic` with stable logical keys (ADR-002). A future tree-sitter DRL grammar can
//! supersede this behind the same NodeKinds without changing downstream consumers.

use regex::Regex;
use std::sync::LazyLock;
use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
    Result, SourceFile, Span, Symbol,
};

const LANG: &str = "drools-drl";

/// `package com.example.rules` (DRL has no trailing `;`).
static RE_PACKAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*package\s+([\w.]+)").expect("RE_PACKAGE must compile"));
/// `rule "Name"` (quoted) or `rule Name` (bare identifier).
static RE_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*rule\s+(?:"([^"]+)"|([A-Za-z_][\w]*))"#).expect("RE_RULE must compile")
});
/// `declare TypeName` → a Fact type.
static RE_DECLARE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*declare\s+([A-Za-z_][\w.]*)").expect("RE_DECLARE must compile")
});
/// `end` on its own line — closes a rule / declare / query / function block.
static RE_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*end\b").expect("RE_END must compile"));
/// `when` on its own line — opens the LHS (conditions).
static RE_WHEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*when\b").expect("RE_WHEN must compile"));
/// `then` on its own line — opens the RHS (actions).
static RE_THEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*then\b").expect("RE_THEN must compile"));

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

/// Heuristic regex extractor for Drools DRL rule files.
pub struct DrlExtractor;

impl DrlExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DrlExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for DrlExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(LANG)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // `content`: comments blanked (keywords inside a comment are not rules).
        // `scan`: also string literals masked (a `when`/`then`/`end` inside a quoted value must not
        // split a rule). Both are length-preserving, so offsets found on `scan` index `content` for
        // human-readable signatures. All structural matching runs on `scan`.
        let content = crate::rules_text::blank_c_comments(&file.text);
        let scan = crate::rules_text::mask_strings(&content);
        // Names/signatures come from `content` (quoted rule names survive); body keyword/boundary
        // search runs on `scan` (a `when`/`then`/`end` inside a string can't split a rule).
        let text = content.as_str();
        let mut nodes = Vec::new();
        let mut local_edges = Vec::new();

        // 1. package → RuleSet
        let ruleset_sym = RE_PACKAGE.captures(text).map(|c| {
            let pkg = c[1].to_string();
            let sym = Symbol::synthetic("drl", format!("{}::package::{}", file.path, pkg)).id();
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

        // 2. rule "Name" … when … then … end
        for caps in RE_RULE.captures_iter(text) {
            let m = caps.get(0).unwrap();
            let name = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|x| x.as_str().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }

            // The rule body runs from the header to the first `end` line — but never past the next
            // `rule`/`declare` header, so a rule missing its `end` cannot swallow the following one.
            let header_end = m.end();
            let rest = &scan[header_end..];
            let mut body_len = RE_END.find(rest).map(|e| e.start()).unwrap_or(rest.len());
            if let Some(nr) = RE_RULE.find(rest) {
                body_len = body_len.min(nr.start());
            }
            if let Some(nd) = RE_DECLARE.find(rest) {
                body_len = body_len.min(nd.start());
            }
            let body = &rest[..body_len];

            let rule_sym = Symbol::synthetic("drl", format!("{}::rule::{}", file.path, name)).id();
            let mut rule_node = Node::new(
                rule_sym.clone(),
                NodeKind::Rule,
                name.clone(),
                Language::new(LANG),
                Location::new(&file.path, byte_span(m.start(), m.end())),
            );
            rule_node.signature = Some(format!("rule \"{name}\""));
            nodes.push(rule_node);

            if let Some(rs) = &ruleset_sym {
                local_edges.push(Edge::new(
                    rs.clone(),
                    rule_sym.clone(),
                    EdgeKind::Contains,
                    ResolutionTier::Heuristic,
                    "drools-drl",
                ));
            }

            let when_m = RE_WHEN.find(body);
            let then_m = RE_THEN.find(body);

            // when … (up to `then`) → Condition
            if let Some(w) = when_m {
                let cond_end = then_m.map(|t| t.start()).unwrap_or(body.len());
                let cond_text = content[header_end + w.end()..header_end + cond_end]
                    .trim()
                    .to_string();
                let cond_sym =
                    Symbol::synthetic("drl", format!("{}::condition::{}::when", file.path, name))
                        .id();
                let mut cn = Node::new(
                    cond_sym.clone(),
                    NodeKind::Condition,
                    format!("{name}::when"),
                    Language::new(LANG),
                    Location::new(&file.path, Span::ZERO),
                );
                cn.signature = Some(cond_text);
                nodes.push(cn);
                local_edges.push(Edge::new(
                    rule_sym.clone(),
                    cond_sym,
                    EdgeKind::Contains,
                    ResolutionTier::Heuristic,
                    "drools-drl",
                ));
            }

            // then … (to end) → Action
            if let Some(t) = then_m {
                let action_text = content[header_end + t.end()..header_end + body_len]
                    .trim()
                    .to_string();
                let act_sym =
                    Symbol::synthetic("drl", format!("{}::action::{}::then", file.path, name)).id();
                let mut an = Node::new(
                    act_sym.clone(),
                    NodeKind::Action,
                    format!("{name}::then"),
                    Language::new(LANG),
                    Location::new(&file.path, Span::ZERO),
                );
                an.signature = Some(action_text);
                nodes.push(an);
                local_edges.push(Edge::new(
                    rule_sym.clone(),
                    act_sym,
                    EdgeKind::Contains,
                    ResolutionTier::Heuristic,
                    "drools-drl",
                ));
            }
        }

        // 3. declare TypeName … end → Fact
        for caps in RE_DECLARE.captures_iter(text) {
            let m = caps.get(0).unwrap();
            let name = caps[1].to_string();
            let fact_sym = Symbol::synthetic("drl", format!("{}::fact::{}", file.path, name)).id();
            let mut fact_node = Node::new(
                fact_sym.clone(),
                NodeKind::Fact,
                name.clone(),
                Language::new(LANG),
                Location::new(&file.path, byte_span(m.start(), m.end())),
            );
            fact_node.signature = Some(format!("declare {name}"));
            nodes.push(fact_node);
            if let Some(rs) = &ruleset_sym {
                local_edges.push(Edge::new(
                    rs.clone(),
                    fact_sym,
                    EdgeKind::Contains,
                    ResolutionTier::Heuristic,
                    "drools-drl",
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

    fn drl(text: &str) -> SourceFile {
        SourceFile {
            path: "rules.drl".to_string(),
            language: Language::new(LANG),
            text: text.to_string(),
        }
    }

    const SAMPLE: &str = r#"package com.example.lending

import com.example.Applicant

declare Applicant
    score : int
    income : double
end

rule "Approve high score"
    salience 10
when
    $a : Applicant( score >= 700 )
then
    $a.setApproved(true);
    update($a);
end

rule "Reject low income"
when
    $a : Applicant( income < 20000 )
then
    $a.setApproved(false);
end
"#;

    #[test]
    fn package_emits_ruleset() {
        let ex = DrlExtractor::new().extract(&drl(SAMPLE)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet && n.name == "com.example.lending"),
            "expected a RuleSet for the package"
        );
    }

    #[test]
    fn each_rule_produces_rule_condition_action() {
        let ex = DrlExtractor::new().extract(&drl(SAMPLE)).unwrap();
        let rules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .map(|n| n.name.as_str())
            .collect();
        assert!(rules.contains(&"Approve high score"), "got {rules:?}");
        assert!(rules.contains(&"Reject low income"), "got {rules:?}");
        assert_eq!(
            ex.nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Condition)
                .count(),
            2,
            "expected one Condition (when) per rule"
        );
        assert_eq!(
            ex.nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Action)
                .count(),
            2,
            "expected one Action (then) per rule"
        );
    }

    #[test]
    fn declare_emits_fact() {
        let ex = DrlExtractor::new().extract(&drl(SAMPLE)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Fact && n.name == "Applicant"),
            "expected a Fact node for the declared type"
        );
    }

    #[test]
    fn ruleset_contains_rules_and_facts() {
        let ex = DrlExtractor::new().extract(&drl(SAMPLE)).unwrap();
        // RuleSet → Rule and RuleSet → Fact Contains edges, plus Rule → Condition/Action.
        let contains = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .count();
        // 2 rules + 1 fact from the ruleset, + 2 conditions + 2 actions from rules = 7.
        assert_eq!(contains, 7, "expected 7 Contains edges, got {contains}");
    }

    #[test]
    fn condition_and_action_carry_signatures() {
        let ex = DrlExtractor::new().extract(&drl(SAMPLE)).unwrap();
        let cond = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Condition && n.name == "Approve high score::when")
            .expect("condition node");
        assert!(
            cond.signature
                .as_deref()
                .unwrap_or("")
                .contains("score >= 700"),
            "condition should carry the when-clause text"
        );
    }

    #[test]
    fn rule_keyword_inside_comment_is_not_a_rule() {
        // Antagonist M1: a `rule "…"` inside a block/line comment must NOT become a Rule.
        let src = r#"package com.x

/* rule "Ghost In Block Comment"
   when then end */
// rule "Ghost In Line Comment"

rule "Real"
when
    $a : Account( open == true )
then
    $a.flag();
end
"#;
        let ex = DrlExtractor::new().extract(&drl(src)).unwrap();
        let rules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            rules,
            vec!["Real"],
            "only the real rule, no comment ghosts: {rules:?}"
        );
    }

    #[test]
    fn rule_missing_end_does_not_swallow_the_next_rule() {
        // Antagonist m3: a rule missing its `end` must not absorb the following rule's text.
        let src = r#"package com.x

rule "First"
when
    $a : Account( a > 1 )
then
    $a.first();

rule "Second"
when
    $b : Account( b > 2 )
then
    $b.second();
end
"#;
        let ex = DrlExtractor::new().extract(&drl(src)).unwrap();
        let first_cond = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Condition && n.name == "First::when")
            .expect("First condition");
        assert!(
            !first_cond
                .signature
                .as_deref()
                .unwrap_or("")
                .contains("Account( b > 2 )"),
            "First's condition must not swallow Second's body: {:?}",
            first_cond.signature
        );
        assert_eq!(
            ex.nodes.iter().filter(|n| n.kind == NodeKind::Rule).count(),
            2,
            "both rules detected"
        );
    }
}
