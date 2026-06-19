//! W15.12 — IBM ODM BAL/IRL regex extractor (bootstrap tier).
//!
//! No public grammar exists for ODM BAL or IRL. This extractor uses heuristic
//! regex patterns to identify rule names, conditions, and actions — sufficient
//! for reverse-engineering structural maps of ODM rule projects.
//!
//! Supported:
//! - IRL `rule "Name" { when {...} then {...} }` — primary target
//! - BAL `rule "Name"` / `conditions` / `actions` blocks — best-effort
//! - `package com.example...` → package-level RuleSet node
//!
//! All edges carry `ResolutionTier::Heuristic` (regex heuristics, not AST facts).
//! All IDs use `Symbol::synthetic` with stable logical keys (ADR-002).

use regex::Regex;
use std::sync::LazyLock;
use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
    Result, SourceFile, Span, Symbol,
};

// ── Compiled regexes ──────────────────────────────────────────────────────────

/// Matches `package com.example.pkg;`
static RE_PACKAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*package\s+([\w.]+)\s*;").expect("RE_PACKAGE must compile")
});

/// Matches `rule "RuleName" {` in IRL
static RE_IRL_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*rule\s+"([^"]+)"\s*\{"#).expect("RE_IRL_RULE must compile")
});

/// Matches `when {` block opener in IRL
static RE_IRL_WHEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*when\s*\{").expect("RE_IRL_WHEN must compile")
});

/// Matches `then {` block opener in IRL
static RE_IRL_THEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*then\s*\{").expect("RE_IRL_THEN must compile")
});

/// Matches `rule "RuleName"` in BAL (no `{` follows immediately)
static RE_BAL_RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*rule\s+"([^"]+)"\s*$"#).expect("RE_BAL_RULE must compile")
});

/// Matches the `conditions` section header in BAL
static RE_BAL_CONDITIONS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*conditions\s*$").expect("RE_BAL_CONDITIONS must compile")
});

/// Matches the `actions` section header in BAL
static RE_BAL_ACTIONS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*actions\s*$").expect("RE_BAL_ACTIONS must compile")
});

// ── Extractor ─────────────────────────────────────────────────────────────────

/// Extractor for IBM ODM BAL and IRL rule files. Grammar-less; uses heuristic
/// regex patterns to identify rule structure.
pub struct OdmExtractor;

impl OdmExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OdmExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for OdmExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![
            Language::new("ibm-odm-irl"),
            Language::new("ibm-odm-bal"),
        ]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        // Detect format by extension or content heuristic.
        // IRL files typically have `rule "Name" {` patterns; BAL files use `conditions` / `actions`.
        if file.path.ends_with(".irl") || RE_IRL_RULE.is_match(&file.text) {
            extract_irl(file)
        } else {
            extract_bal(file)
        }
    }
}

// ── IRL extractor ─────────────────────────────────────────────────────────────

fn extract_irl(file: &SourceFile) -> Result<Extraction> {
    let mut nodes = Vec::new();
    let mut local_edges = Vec::new();
    let text = &file.text;

    // 1. Package → RuleSet node
    let ruleset_sym = if let Some(caps) = RE_PACKAGE.captures(text) {
        let pkg = caps[1].to_string();
        let sym = Symbol::synthetic("odm", format!("{}::package::{}", file.path, pkg)).id();
        let mut node = Node::new(
            sym.clone(),
            NodeKind::RuleSet,
            pkg.clone(),
            Language::new("ibm-odm-irl"),
            Location::new(&file.path, Span::ZERO),
        );
        node.signature = Some(format!("package {pkg}"));
        nodes.push(node);
        Some(sym)
    } else {
        None
    };

    // 2. Parse each `rule "Name" { when {...} then {...} }` block.
    // We walk the text character by character to find matching braces for each rule body.
    let mut search_start = 0usize;
    while let Some(rule_caps) = RE_IRL_RULE.captures(&text[search_start..]) {
        let full_match = rule_caps.get(0).unwrap();
        let abs_rule_start = search_start + full_match.start();
        let abs_rule_end = search_start + full_match.end();

        let rule_name = rule_caps[1].to_string();
        let rule_sym =
            Symbol::synthetic("odm", format!("{}::rule::{}", file.path, rule_name)).id();

        let rule_loc = Location::new(&file.path, byte_span(abs_rule_start, abs_rule_end));
        let mut rule_node = Node::new(
            rule_sym.clone(),
            NodeKind::Rule,
            rule_name.clone(),
            Language::new("ibm-odm-irl"),
            rule_loc,
        );
        rule_node.signature = Some(format!("rule \"{rule_name}\""));
        nodes.push(rule_node);

        // RuleSet → Rule (Contains)
        if let Some(rs) = &ruleset_sym {
            local_edges.push(Edge::new(
                rs.clone(),
                rule_sym.clone(),
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "odm-irl",
            ));
        }

        // Find the outer `{` that opens the rule body (just after the `{` at end of rule header).
        // abs_rule_end points past the `{`; the rule body starts there.
        let body_text = &text[abs_rule_end..];

        // Extract `when { ... }` block content
        if let Some(when_content) = extract_block(body_text, &RE_IRL_WHEN) {
            let cond_name = format!("{rule_name}::when");
            let cond_sym =
                Symbol::synthetic("odm", format!("{}::condition::{}", file.path, cond_name)).id();
            let mut cond_node = Node::new(
                cond_sym.clone(),
                NodeKind::Condition,
                cond_name,
                Language::new("ibm-odm-irl"),
                Location::new(&file.path, Span::ZERO),
            );
            cond_node.signature = Some(when_content.trim().to_string());
            nodes.push(cond_node);

            local_edges.push(Edge::new(
                rule_sym.clone(),
                cond_sym,
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "odm-irl",
            ));
        }

        // Extract `then { ... }` block content
        if let Some(then_content) = extract_block(body_text, &RE_IRL_THEN) {
            let action_name = format!("{rule_name}::then");
            let action_sym =
                Symbol::synthetic("odm", format!("{}::action::{}", file.path, action_name)).id();
            let mut action_node = Node::new(
                action_sym.clone(),
                NodeKind::Action,
                action_name,
                Language::new("ibm-odm-irl"),
                Location::new(&file.path, Span::ZERO),
            );
            action_node.signature = Some(then_content.trim().to_string());
            nodes.push(action_node);

            local_edges.push(Edge::new(
                rule_sym.clone(),
                action_sym,
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "odm-irl",
            ));
        }

        // Advance past this rule header so we find the next one
        search_start = abs_rule_end;
    }

    Ok(Extraction {
        nodes,
        local_edges,
        refs: Vec::new(),
    })
}

/// Extract the content of a `keyword { ... }` block. Returns the text inside the braces.
fn extract_block(text: &str, opener_re: &Regex) -> Option<String> {
    let m = opener_re.find(text)?;
    // Find the `{` at the end of the opener match
    let after_open = &text[m.end()..];
    let mut depth = 1i32;
    let mut end = 0usize;
    for (i, c) in after_open.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(after_open[..end].to_string())
}

// ── BAL extractor ─────────────────────────────────────────────────────────────

const MAX_ITEMS_PER_BLOCK: usize = 5;

fn extract_bal(file: &SourceFile) -> Result<Extraction> {
    let mut nodes = Vec::new();
    let mut local_edges = Vec::new();
    let text = &file.text;
    let lines: Vec<&str> = text.lines().collect();

    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim();

        // Match `rule "Name"` — bare (no `{`)
        if let Some(caps) = RE_BAL_RULE.captures(line) {
            let rule_name = caps[1].to_string();
            let rule_sym =
                Symbol::synthetic("odm", format!("{}::rule::{}", file.path, rule_name)).id();

            let rule_loc = Location::new(&file.path, line_span(i));
            let mut rule_node = Node::new(
                rule_sym.clone(),
                NodeKind::Rule,
                rule_name.clone(),
                Language::new("ibm-odm-bal"),
                rule_loc,
            );
            rule_node.signature = Some(format!("rule \"{rule_name}\""));
            nodes.push(rule_node);

            // Scan ahead for `conditions` and `actions` blocks within this rule.
            let mut j = i + 1;
            while j < lines.len() {
                let inner = lines[j].trim();

                // Stop at the start of the next rule
                if RE_BAL_RULE.is_match(inner) {
                    break;
                }

                if RE_BAL_CONDITIONS.is_match(inner) {
                    // Collect condition lines until `actions`, next `rule`, or end of text.
                    j += 1;
                    let mut count = 0usize;
                    while j < lines.len() && count < MAX_ITEMS_PER_BLOCK {
                        let cline = lines[j].trim();
                        if cline.is_empty()
                            || RE_BAL_ACTIONS.is_match(cline)
                            || RE_BAL_RULE.is_match(cline)
                        {
                            break;
                        }
                        // Each non-empty condition line → one Condition node
                        let cond_text = cline.trim_end_matches(';').trim().to_string();
                        if !cond_text.is_empty() {
                            let cond_name =
                                format!("{}::condition::{}", rule_name, sanitize(&cond_text));
                            let cond_sym = Symbol::synthetic(
                                "odm",
                                format!("{}::condition::{}", file.path, cond_name),
                            )
                            .id();
                            let mut cond_node = Node::new(
                                cond_sym.clone(),
                                NodeKind::Condition,
                                cond_name,
                                Language::new("ibm-odm-bal"),
                                Location::new(&file.path, Span::ZERO),
                            );
                            cond_node.signature = Some(cond_text);
                            nodes.push(cond_node);

                            local_edges.push(Edge::new(
                                rule_sym.clone(),
                                cond_sym,
                                EdgeKind::Contains,
                                ResolutionTier::Heuristic,
                                "odm-bal",
                            ));
                            count += 1;
                        }
                        j += 1;
                    }
                    continue;
                }

                if RE_BAL_ACTIONS.is_match(inner) {
                    // Collect action lines until next `rule` or end of text.
                    j += 1;
                    let mut count = 0usize;
                    while j < lines.len() && count < MAX_ITEMS_PER_BLOCK {
                        let aline = lines[j].trim();
                        if aline.is_empty() || RE_BAL_RULE.is_match(aline) {
                            break;
                        }
                        let action_text = aline.trim_end_matches(';').trim().to_string();
                        if !action_text.is_empty() {
                            let action_name =
                                format!("{}::action::{}", rule_name, sanitize(&action_text));
                            let action_sym = Symbol::synthetic(
                                "odm",
                                format!("{}::action::{}", file.path, action_name),
                            )
                            .id();
                            let mut action_node = Node::new(
                                action_sym.clone(),
                                NodeKind::Action,
                                action_name,
                                Language::new("ibm-odm-bal"),
                                Location::new(&file.path, Span::ZERO),
                            );
                            action_node.signature = Some(action_text);
                            nodes.push(action_node);

                            local_edges.push(Edge::new(
                                rule_sym.clone(),
                                action_sym,
                                EdgeKind::Contains,
                                ResolutionTier::Heuristic,
                                "odm-bal",
                            ));
                            count += 1;
                        }
                        j += 1;
                    }
                    continue;
                }

                j += 1;
            }
            i = j;
            continue;
        }

        i += 1;
    }

    Ok(Extraction {
        nodes,
        local_edges,
        refs: Vec::new(),
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// One-line span anchor (byte offsets set to 0; line numbers are the stable part).
fn line_span(line: usize) -> Span {
    let l = line as u32;
    Span {
        start_byte: 0,
        end_byte: 0,
        start_line: l,
        start_col: 0,
        end_line: l,
        end_col: 0,
    }
}

/// Byte-range span (for IRL, where we have byte offsets from regex matches).
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

/// Produce a stable, compact identifier fragment from free-text (strip special chars, truncate).
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    // Truncate to keep IDs from growing unboundedly.
    cleaned.chars().take(40).collect()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn irl_file(text: &str) -> SourceFile {
        SourceFile {
            path: "test.irl".to_string(),
            language: Language::new("ibm-odm-irl"),
            text: text.to_string(),
        }
    }

    fn bal_file(text: &str) -> SourceFile {
        SourceFile {
            path: "test.brl".to_string(),
            language: Language::new("ibm-odm-bal"),
            text: text.to_string(),
        }
    }

    #[test]
    fn irl_package_emits_ruleset() {
        let ex = OdmExtractor::new()
            .extract(&irl_file(
                r#"package com.example.lending;
rule "CheckScore" {
  when { Customer c(score >= 700); }
  then { c.approve(); }
}
"#,
            ))
            .unwrap();

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet
                    && n.name == "com.example.lending"),
            "expected a RuleSet node for the package"
        );
    }

    #[test]
    fn irl_single_rule_produces_rule_condition_action() {
        let ex = OdmExtractor::new()
            .extract(&irl_file(
                r#"package com.example;
rule "MyRule" {
  when { Foo f(x > 1); }
  then { f.doIt(); }
}
"#,
            ))
            .unwrap();

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Rule && n.name == "MyRule"),
            "expected Rule node"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Condition),
            "expected Condition node"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Action),
            "expected Action node"
        );
    }

    #[test]
    fn irl_contains_edges_emitted() {
        let ex = OdmExtractor::new()
            .extract(&irl_file(
                r#"package com.example;
rule "R" {
  when { X x(a > 0); }
  then { x.run(); }
}
"#,
            ))
            .unwrap();

        let contains: Vec<_> = ex
            .local_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect();
        assert!(
            contains.len() >= 3,
            "expected ≥3 Contains edges (ruleset→rule, rule→cond, rule→action), got {}",
            contains.len()
        );
    }

    #[test]
    fn bal_rule_emits_rule_node() {
        let ex = OdmExtractor::new()
            .extract(&bal_file(
                r#"rule "ApprovalRule"
  conditions
    the score is not less than 700 ;
  actions
    set the decision to "approved" ;
"#,
            ))
            .unwrap();

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Rule
                    && n.name == "ApprovalRule"),
            "expected Rule node for BAL"
        );
    }

    #[test]
    fn bal_conditions_and_actions_emitted() {
        let ex = OdmExtractor::new()
            .extract(&bal_file(
                r#"rule "TestRule"
  conditions
    the score is not less than 700 ;
  actions
    set the decision to "approved" ;
"#,
            ))
            .unwrap();

        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Condition),
            "expected Condition node"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Action),
            "expected Action node"
        );
    }

    #[test]
    fn odm_extractor_languages() {
        let langs = OdmExtractor::new().languages();
        assert_eq!(langs.len(), 2);
        assert!(langs.contains(&Language::new("ibm-odm-irl")));
        assert!(langs.contains(&Language::new("ibm-odm-bal")));
    }
}
