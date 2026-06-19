//! W15.11 — FICO Blaze Advisor `.brl` (Structured Rule Language) **heuristic** extractor.
//!
//! Issue #24 scoped three Blaze tracks: `.brl` text (a regex Tier-3 bootstrap), `.rma` project XML,
//! and `.xls` decision tables. Only the `.brl` track is buildable from PUBLIC information: FICO's SRL
//! is documented (the W3C 2004 "Fair Isaac Blaze Advisor" paper + the FICO help portal) as an
//! English-like rule language. The `.rma` XML and `.xls` tracks need a proprietary schema for which
//! **no public specimen exists** (a GitHub code search returns zero FICO `.rma` files); the W15.2/
//! W15.3 ingest infra ([`XmlRulesExtractor`](crate::XmlRulesExtractor) /
//! [`ExcelRulesExtractor`](crate::ExcelRulesExtractor)) is built and proven on public formats and
//! can onboard those formats once a specimen is provided — see the issue-closure note.
//!
//! This extractor follows the [`OdmExtractor`](crate::OdmExtractor)/[`DrlExtractor`](crate::DrlExtractor)
//! heuristic pattern. SRL rules are brace-delimited with an English-like `if … then …` body:
//!
//! ```text
//! ruleset AccountRules {
//!   rule HighBalance {
//!     if customer.balance > 10000 then set customer.tier to "gold" ;
//!   }
//! }
//! ```
//!
//! Mapping:
//! - `ruleset`/`library <name>` (or file stem) → [`NodeKind::RuleSet`]
//! - `rule <name> { … }`                       → [`NodeKind::Rule`]
//! - `if`/`when`/`whenever … (to `then`)`      → [`NodeKind::Condition`]
//! - `then` / `set` / `assign` / `create …`    → [`NodeKind::Action`]
//!
//! Structural scanning runs over a comment-blanked, string-masked copy (see [`crate::rules_text`]) so
//! a `then`/`}` inside a comment or string literal can't split a rule; signatures are sliced from the
//! comment-blanked copy (string values intact). All edges carry [`ResolutionTier::Heuristic`]; IDs use
//! `Symbol::synthetic` (ADR-002). Pure regex — stays in the MIT core. As a Tier-3 bootstrap to
//! documented syntax, it captures rule STRUCTURE and will benefit from tuning against a real corpus.

use regex::Regex;
use std::sync::LazyLock;
use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
    Result, SourceFile, Span, Symbol,
};

const LANG: &str = "fico-blaze-brl";

/// `ruleset Name` / `library Name` — the rule container name (falls back to the file stem).
static RE_RULESET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:ruleset|library)\s+([A-Za-z_][\w.]*)").expect("RE_RULESET must compile")
});
/// `rule "Name" {` or `rule Name {` — a rule definition opening a brace body.
static RE_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\brule\s+(?:"([^"]+)"|([A-Za-z_][\w]*))\s*\{"#).expect("RE_RULE must compile")
});
/// The LHS opener — `if` / `when` / `whenever`.
static RE_IF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:if|whenever|when)\b").expect("RE_IF must compile"));
/// The LHS→RHS divider — `then`.
static RE_THEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bthen\b").expect("RE_THEN must compile"));

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

fn file_stem(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    base.strip_suffix(".brl").unwrap_or(base).to_string()
}

/// Heuristic regex extractor for FICO Blaze Advisor `.brl` SRL files.
pub struct BlazeBrlExtractor;

impl BlazeBrlExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlazeBrlExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for BlazeBrlExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(LANG)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // `content`: comments blanked (quoted rule names intact). `scan`: also string literals masked
        // (a `then`/`}` inside a quoted value can't split a rule). Rule names/signatures come from
        // `content`; brace matching + if/then location run on `scan`. Both length-preserving.
        let content = crate::rules_text::blank_c_comments(&file.text);
        let scan = crate::rules_text::mask_strings(&content);
        let lang = Language::new(LANG);
        let mut nodes = Vec::new();
        let mut local_edges = Vec::new();

        // 1. ruleset/library name (or file stem) → RuleSet
        let rs_name = RE_RULESET
            .captures(&content)
            .map(|c| c[1].to_string())
            .unwrap_or_else(|| file_stem(&file.path));
        let ruleset_sym =
            Symbol::synthetic("blaze", format!("{}::ruleset::{}", file.path, rs_name)).id();
        let mut rs_node = Node::new(
            ruleset_sym.clone(),
            NodeKind::RuleSet,
            &rs_name,
            lang.clone(),
            Location::new(&file.path, Span::ZERO),
        );
        rs_node.signature = Some(format!("ruleset {rs_name}"));
        nodes.push(rs_node);

        // 2. rule <name> { … } → Rule + Condition (if…) + Action (then…)
        for caps in RE_RULE.captures_iter(&content) {
            let m = caps.get(0).unwrap();
            let name = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|x| x.as_str().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // Body is between the `{` the match ends on and its string-aware matching `}`.
            let brace_open = m.end() - 1;
            let body_end = crate::rules_text::match_brace_end(&scan, brace_open);
            let body_base = brace_open + 1;
            let body_scan = &scan[body_base..body_end];

            let rule_sym =
                Symbol::synthetic("blaze", format!("{}::rule::{}", file.path, name)).id();
            let mut rule_node = Node::new(
                rule_sym.clone(),
                NodeKind::Rule,
                &name,
                lang.clone(),
                Location::new(&file.path, byte_span(m.start(), m.end())),
            );
            rule_node.signature = Some(format!("rule {name}"));
            nodes.push(rule_node);
            local_edges.push(Edge::new(
                ruleset_sym.clone(),
                rule_sym.clone(),
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "fico-blaze-brl",
            ));

            let if_m = RE_IF.find(body_scan);
            let then_m = RE_THEN.find(body_scan);

            // Condition: from the `if`/`when`/`whenever` opener up to `then` (or end of body).
            if let Some(iff) = if_m {
                // Guard against `then` preceding the if/when opener (malformed): a `then` start
                // before `iff.end()` would make `[iff.start()..end_rel]` an invalid range and panic.
                // Such a `then` is not the divider — fall back to the body end.
                let end_rel = then_m
                    .map(|t| t.start())
                    .filter(|&start| start >= iff.end())
                    .unwrap_or(body_scan.len());
                let cond = content[body_base + iff.start()..body_base + end_rel]
                    .trim()
                    .to_string();
                if !cond.is_empty() {
                    let csym =
                        Symbol::synthetic("blaze", format!("{}::condition::{}", file.path, name))
                            .id();
                    let mut cn = Node::new(
                        csym.clone(),
                        NodeKind::Condition,
                        format!("{name}::if"),
                        lang.clone(),
                        Location::new(
                            &file.path,
                            byte_span(body_base + iff.start(), body_base + end_rel),
                        ),
                    );
                    cn.signature = Some(cond);
                    nodes.push(cn);
                    local_edges.push(Edge::new(
                        rule_sym.clone(),
                        csym,
                        EdgeKind::Contains,
                        ResolutionTier::Heuristic,
                        "fico-blaze-brl",
                    ));
                }
            }

            // Action: everything after `then` (the set/assign/create statements).
            if let Some(t) = then_m {
                let action = content[body_base + t.end()..body_end]
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                if !action.is_empty() {
                    let asym =
                        Symbol::synthetic("blaze", format!("{}::action::{}", file.path, name)).id();
                    let mut an = Node::new(
                        asym.clone(),
                        NodeKind::Action,
                        format!("{name}::then"),
                        lang.clone(),
                        Location::new(&file.path, byte_span(body_base + t.end(), body_end)),
                    );
                    an.signature = Some(action);
                    nodes.push(an);
                    local_edges.push(Edge::new(
                        rule_sym.clone(),
                        asym,
                        EdgeKind::Contains,
                        ResolutionTier::Heuristic,
                        "fico-blaze-brl",
                    ));
                }
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

    fn brl(text: &str) -> SourceFile {
        SourceFile {
            path: "account.brl".to_string(),
            language: Language::new(LANG),
            text: text.to_string(),
        }
    }

    // Documented Blaze SRL shape (W3C 2004 paper + FICO help): English-like, brace-delimited.
    const SAMPLE: &str = r#"ruleset AccountRules {

  rule HighBalance {
    if customer.balance > 10000 then
      set customer.tier to "gold" ;
  }

  rule Overdrawn {
    whenever customer.balance < 0 then
      assign customer.status = "overdrawn" ;
  }
}
"#;

    #[test]
    fn ruleset_name_becomes_ruleset() {
        let ex = BlazeBrlExtractor::new().extract(&brl(SAMPLE)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet && n.name == "AccountRules"),
            "expected a RuleSet from the `ruleset` declaration"
        );
    }

    #[test]
    fn each_rule_yields_rule_condition_action() {
        let ex = BlazeBrlExtractor::new().extract(&brl(SAMPLE)).unwrap();
        let rules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .map(|n| n.name.as_str())
            .collect();
        assert!(rules.contains(&"HighBalance"), "got {rules:?}");
        assert!(rules.contains(&"Overdrawn"), "got {rules:?}");
        let n = |k: NodeKind| ex.nodes.iter().filter(|x| x.kind == k).count();
        assert_eq!(n(NodeKind::Condition), 2, "one if/whenever per rule");
        assert_eq!(n(NodeKind::Action), 2, "one then per rule");
    }

    #[test]
    fn condition_and_action_carry_text() {
        let ex = BlazeBrlExtractor::new().extract(&brl(SAMPLE)).unwrap();
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::Condition
                && n.signature
                    .as_deref()
                    .unwrap_or("")
                    .contains("customer.balance > 10000")),
            "condition should carry its if-clause"
        );
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::Action
                && n.signature
                    .as_deref()
                    .unwrap_or("")
                    .contains("set customer.tier")),
            "action should carry its then-clause"
        );
    }

    #[test]
    fn quoted_rule_names_and_no_ruleset_fall_back_to_stem() {
        // No `ruleset` decl → RuleSet named from the file stem; quoted rule name handled.
        let src = r#"rule "Risk Check" {
  if applicant.score < 500 then create flag ;
}
"#;
        let ex = BlazeBrlExtractor::new().extract(&brl(src)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet && n.name == "account"),
            "RuleSet should fall back to the file stem"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Rule && n.name == "Risk Check"),
            "quoted rule name should be captured"
        );
    }

    // ── Antagonist regression tests ──────────────────────────────────────────────────────────

    #[test]
    fn then_inside_a_string_does_not_truncate() {
        // Antagonist M2: the word `then` inside a quoted value must not split condition/action.
        let src = r#"ruleset R {
  rule S {
    if state is "then-pending" then set ok to true ;
  }
}
"#;
        let ex = BlazeBrlExtractor::new().extract(&brl(src)).unwrap();
        let cond = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Condition)
            .expect("condition");
        assert!(
            cond.signature.as_deref().unwrap_or("").contains("state is"),
            "condition keeps full text past the string `then`: {:?}",
            cond.signature
        );
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::Action
                && n.signature
                    .as_deref()
                    .unwrap_or("")
                    .contains("set ok to true")),
            "the real action after the real `then` is captured"
        );
    }

    #[test]
    fn brace_inside_a_string_does_not_drop_the_action() {
        // Antagonist M3: a `}` inside a quoted value must not close the rule body early.
        let src = r#"ruleset R {
  rule S {
    if note = "has a } here" then set flag to true ;
  }
}
"#;
        let ex = BlazeBrlExtractor::new().extract(&brl(src)).unwrap();
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::Action
                && n.signature
                    .as_deref()
                    .unwrap_or("")
                    .contains("set flag to true")),
            "action survives a closing brace embedded in a string literal"
        );
    }

    #[test]
    fn rule_inside_comment_is_ignored() {
        // A `rule` keyword inside a comment must not become a Rule.
        let src = r#"ruleset R {
  /* rule Ghost { if x then set y ; } */
  rule Real {
    if a > 1 then set b to 2 ;
  }
}
"#;
        let ex = BlazeBrlExtractor::new().extract(&brl(src)).unwrap();
        let rules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            rules,
            vec!["Real"],
            "only the real rule, no comment ghost: {rules:?}"
        );
    }

    #[test]
    fn then_before_if_does_not_panic() {
        // Same out-of-order class as the gemini #34 finding on DRL: `then` before the if/when opener
        // would make the condition range invalid and panic. Must not panic.
        let src = r#"ruleset R {
  rule Malformed {
    then set ok to true if applicant.score < 500 ;
  }
}
"#;
        let ex = BlazeBrlExtractor::new().extract(&brl(src)).unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Rule && n.name == "Malformed"),
            "the rule is still emitted without panicking"
        );
    }
}
