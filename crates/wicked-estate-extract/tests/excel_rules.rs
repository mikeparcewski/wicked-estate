//! Integration tests for the Excel/XLSX decision table extractor (W15.3).
//!
//! The test fixture `tests/fixtures/excel/decision_rules.xlsx` is a hand-crafted minimal XLSX
//! workbook with one sheet named "DecisionRules":
//!
//!   Row 1 (header): rule_name | condition       | action
//!   Row 2 (data):   Rule_A    | age > 18        | approve
//!   Row 3 (data):   Rule_B    | income < 50000  | deny
//!
//! Expected extraction: 1 RuleSet + 2 Rules + 2 Conditions + 2 Actions = 7 nodes total,
//! plus 2 Governs edges (RuleSet→Rule) + 4 Contains edges (Rule→child) = 6 edges.
//!
//! The extractor opens workbooks from the filesystem path in `SourceFile.path`.  Tests pass the
//! absolute path to the fixture file; `SourceFile.text` is unused for binary XLSX.

#![cfg(feature = "excel-rules")]

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::{
    ColumnConfig, ColumnRole, ExcelEngineConfig, ExcelRulesConfig, ExcelRulesExtractor,
    SheetConfig,
};

/// Returns the absolute path to a named fixture file.
fn fixture_path(name: &str) -> String {
    format!(
        "{}/tests/fixtures/excel/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Build a `SourceFile` for the given fixture.  `text` is empty — the extractor reads from
/// `path` directly for binary XLSX files.
fn fixture_file(name: &str) -> SourceFile {
    SourceFile {
        path: fixture_path(name),
        language: Language::new("xlsx"),
        text: String::new(),
    }
}

fn make_config() -> ExcelRulesConfig {
    ExcelRulesConfig {
        engine: ExcelEngineConfig {
            name: "test-engine".into(),
            file_globs: vec!["*.xlsx".into()],
        },
        sheets: vec![SheetConfig {
            sheet_name: Some("DecisionRules".into()),
            header_row: 0,
            ruleset_name: "TestRuleSet".into(),
            columns: vec![
                ColumnConfig {
                    index: 0,
                    role: ColumnRole::RuleName,
                },
                ColumnConfig {
                    index: 1,
                    role: ColumnRole::Condition,
                },
                ColumnConfig {
                    index: 2,
                    role: ColumnRole::Action,
                },
            ],
        }],
    }
}

#[test]
fn extracts_ruleset_node() {
    let ex = ExcelRulesExtractor::new(make_config())
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed");

    let ruleset_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::RuleSet)
        .collect();
    assert_eq!(
        ruleset_nodes.len(),
        1,
        "expected exactly 1 RuleSet node, got {}: {:#?}",
        ruleset_nodes.len(),
        ex.nodes
    );
    assert_eq!(ruleset_nodes[0].name, "TestRuleSet");
}

#[test]
fn extracts_two_rule_nodes() {
    let ex = ExcelRulesExtractor::new(make_config())
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed");

    let rule_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Rule)
        .collect();
    assert_eq!(
        rule_nodes.len(),
        2,
        "expected 2 Rule nodes (one per data row), got {}: {:#?}",
        rule_nodes.len(),
        rule_nodes
    );
    let names: Vec<&str> = rule_nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"Rule_A"), "Rule_A missing; got {names:?}");
    assert!(names.contains(&"Rule_B"), "Rule_B missing; got {names:?}");
}

#[test]
fn extracts_condition_and_action_nodes() {
    let ex = ExcelRulesExtractor::new(make_config())
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed");

    let cond_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Condition)
        .collect();
    assert_eq!(
        cond_nodes.len(),
        2,
        "expected 2 Condition nodes, got {}",
        cond_nodes.len()
    );

    let act_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Action)
        .collect();
    assert_eq!(
        act_nodes.len(),
        2,
        "expected 2 Action nodes, got {}",
        act_nodes.len()
    );
}

#[test]
fn total_node_count_is_seven() {
    // 1 RuleSet + 2 Rules + 2 Conditions + 2 Actions = 7
    let ex = ExcelRulesExtractor::new(make_config())
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed");

    assert_eq!(
        ex.nodes.len(),
        7,
        "1 RuleSet + 2 Rules + 2 Conditions + 2 Actions = 7, got {}: {:#?}",
        ex.nodes.len(),
        ex.nodes.iter().map(|n| (&n.kind, &n.name)).collect::<Vec<_>>()
    );
}

#[test]
fn emits_governs_edges_from_ruleset() {
    let ex = ExcelRulesExtractor::new(make_config())
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed");

    let governs: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Governs)
        .collect();
    assert_eq!(
        governs.len(),
        2,
        "expected 2 Governs edges (one per Rule), got {}",
        governs.len()
    );
}

#[test]
fn emits_contains_edges_from_rules() {
    let ex = ExcelRulesExtractor::new(make_config())
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed");

    let contains: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .collect();
    assert_eq!(
        contains.len(),
        4,
        "expected 4 Contains edges (2 conditions + 2 actions), got {}",
        contains.len()
    );
}

#[test]
fn extractor_languages_returns_xlsx() {
    let extractor = ExcelRulesExtractor::new(make_config());
    let langs = extractor.languages();
    assert!(
        langs.iter().any(|l| l.as_str() == "xlsx"),
        "languages() must include 'xlsx'"
    );
}

#[test]
fn first_sheet_used_when_sheet_name_is_none() {
    // Override sheet_name to None — should still pick up the first (and only) sheet.
    let mut cfg = make_config();
    cfg.sheets[0].sheet_name = None;

    let ex = ExcelRulesExtractor::new(cfg)
        .extract(&fixture_file("decision_rules.xlsx"))
        .expect("extraction must succeed with sheet_name=None");

    assert_eq!(
        ex.nodes.iter().filter(|n| n.kind == NodeKind::Rule).count(),
        2,
        "should still find 2 Rule nodes when sheet_name=None"
    );
}
