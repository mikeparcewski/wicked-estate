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
//! All edges carry [`ResolutionTier::Heuristic`]; IDs use `Symbol::synthetic` (ADR-002). Pure regex
//! — stays in the MIT core. As a Tier-3 bootstrap to documented syntax, the extractor captures rule
//! STRUCTURE and will benefit from tuning against a real `.brl` corpus.

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

/// Given the byte index of an opening `{`, return (inner_content_trimmed, end_byte_after `}`).
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
                    return (text[open + 1..i].trim().to_string(), i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (text[open..].trim().to_string(), text.len())
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
        let text = &file.text;
        let lang = Language::new(LANG);
        let mut nodes = Vec::new();
        let mut local_edges = Vec::new();

        // 1. ruleset/library name (or file stem) → RuleSet
        let rs_name = RE_RULESET
            .captures(text)
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
            // Brace body starts at the `{` that the RE_RULE match ends on.
            let brace_open = m.end() - 1;
            let (body, _) = brace_block(text, brace_open);

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

            let if_m = RE_IF.find(&body);
            let then_m = RE_THEN.find(&body);

            // Condition: from the `if`/`when`/`whenever` opener up to `then` (or end of body).
            if let Some(iff) = if_m {
                let end = then_m.map(|t| t.start()).unwrap_or(body.len());
                let cond = body[iff.start()..end].trim().to_string();
                if !cond.is_empty() {
                    let csym =
                        Symbol::synthetic("blaze", format!("{}::condition::{}", file.path, name))
                            .id();
                    let mut cn = Node::new(
                        csym.clone(),
                        NodeKind::Condition,
                        format!("{name}::if"),
                        lang.clone(),
                        Location::new(&file.path, Span::ZERO),
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
                let action = body[t.end()..]
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
                        Location::new(&file.path, Span::ZERO),
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
}
