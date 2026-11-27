//! W15.4 — Camunda DMN extractor.
//!
//! A thin wrapper around [`XmlRulesExtractor`] driven by an embedded TOML config.
//! Adding or adjusting element→node-kind mappings is a config change, zero Rust change.
//!
//! # Emitted graph elements
//!
//! | DMN element    | NodeKind          | notes |
//! |----------------|-------------------|-------|
//! | `definitions`  | `RuleSet`         | the DMN model root |
//! | `decision`     | `Rule`            | a named decision |
//! | `decisionTable`| `RuleSet`         | the table owned by a decision |
//! | `rule`         | `Rule`            | one row in a decision table |
//! | `input`        | `Condition`       | input column (test condition) |
//! | `output`       | `Action`          | output column (result action) |
//!
//! Edges:
//! - `definitions` → `decision` : `Governs`
//! - `decision` → `decisionTable` : `Contains`
//! - `decisionTable` → `rule` : `Contains`
//! - `decisionTable` → `input` : `Evaluates`
//! - `decisionTable` → `output` : `Produces`

#![cfg(feature = "xml-rules")]

use wicked_estate_core::{Extraction, Extractor, Language, Result, SourceFile};

use crate::xml_rules::XmlRulesExtractor;

/// TOML config embedded at compile time — the single source of truth for Camunda DMN extraction.
/// Namespace-agnostic: we match on local element names only (roxmltree strips the namespace).
const DMN_CONFIG: &str = r#"
[engine]
name       = "camunda-dmn"
file_globs = ["**/*.dmn"]

# ── Node mappings ─────────────────────────────────────────────────────────────

[[node_mappings]]
element   = "definitions"
emit_kind = "rule_set"
name_attr = "name"
id_attr   = "id"

[[node_mappings]]
element   = "decision"
emit_kind = "rule"
name_attr = "name"
id_attr   = "id"

[[node_mappings]]
element              = "decisionTable"
emit_kind            = "rule_set"
name_attr            = "id"
id_attr              = "id"

[[node_mappings]]
element   = "rule"
emit_kind = "rule"
name_attr = "id"
id_attr   = "id"

[[node_mappings]]
element   = "input"
emit_kind = "condition"
name_attr = "label"
id_attr   = "id"

[[node_mappings]]
element   = "output"
emit_kind = "action"
name_attr = "label"
id_attr   = "id"

# ── Edge mappings ─────────────────────────────────────────────────────────────

[[edge_mappings]]
parent_element = "definitions"
child_element  = "decision"
edge_kind      = "governs"

[[edge_mappings]]
parent_element = "decision"
child_element  = "decisionTable"
edge_kind      = "contains"

[[edge_mappings]]
parent_element = "decisionTable"
child_element  = "rule"
edge_kind      = "contains"

[[edge_mappings]]
parent_element = "decisionTable"
child_element  = "input"
edge_kind      = "evaluates"

[[edge_mappings]]
parent_element = "decisionTable"
child_element  = "output"
edge_kind      = "produces"
"#;

/// Extractor for Camunda DMN 1.x decision model files (`*.dmn`).
///
/// Delegates entirely to [`XmlRulesExtractor`] configured by [`DMN_CONFIG`].
/// Construct with [`CamundaDmnExtractor::new`].
pub struct CamundaDmnExtractor(XmlRulesExtractor);

impl CamundaDmnExtractor {
    /// Build the extractor from the embedded TOML config.
    ///
    /// # Panics
    ///
    /// Panics if the embedded config is malformed — a compile-time defect, never at runtime.
    pub fn new() -> Self {
        let inner = XmlRulesExtractor::from_toml(DMN_CONFIG)
            .expect("CamundaDmnExtractor embedded config must be valid");
        Self(inner)
    }
}

impl Default for CamundaDmnExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for CamundaDmnExtractor {
    fn languages(&self) -> Vec<Language> {
        self.0.languages()
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        self.0.extract(file)
    }
}

#[cfg(test)]
mod tests {
    use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile};

    use super::CamundaDmnExtractor;

    #[test]
    fn camunda_dmn_new_does_not_panic() {
        let _ = CamundaDmnExtractor::new();
    }

    #[test]
    fn camunda_dmn_extracts_decision_table_from_fixture() {
        const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="https://www.omg.org/spec/DMN/20191111/MODEL/"
             name="LoanApproval" id="loan-approval">
  <decision name="LoanDecision" id="loan-decision">
    <decisionTable id="loan-dt">
      <input label="Income"/>
      <output label="Approved"/>
      <rule id="rule-1">
        <inputEntry id="ie-1"><text>&gt;50000</text></inputEntry>
        <outputEntry id="oe-1"><text>true</text></outputEntry>
      </rule>
    </decisionTable>
  </decision>
</definitions>"#;

        let sf = SourceFile {
            path: "loan.dmn".to_string(),
            language: Language::new("xml-rules:camunda-dmn"),
            text: FIXTURE.to_string(),
        };
        let extraction = CamundaDmnExtractor::new()
            .extract(&sf)
            .expect("extraction must succeed");

        assert!(
            extraction.nodes.iter().any(|n| n.kind == NodeKind::RuleSet),
            "expected at least one RuleSet node"
        );
        assert!(
            extraction.nodes.iter().any(|n| n.kind == NodeKind::Rule),
            "expected at least one Rule node"
        );
    }

    fn ids(text: &str) -> Vec<(String, String)> {
        CamundaDmnExtractor::new()
            .extract(&SourceFile {
                path: "loans.dmn".to_string(),
                language: Language::new("xml-rules:camunda-dmn"),
                text: text.to_string(),
            })
            .expect("extraction must succeed")
            .nodes
            .into_iter()
            .map(|n| (n.name, n.symbol.0))
            .collect()
    }

    fn named<'a>(ids: &'a [(String, String)], name: &str) -> Vec<&'a str> {
        ids.iter()
            .filter(|(n, _)| n == name)
            .map(|(_, id)| id.as_str())
            .collect()
    }

    fn model(body: &str) -> String {
        format!(
            "<definitions xmlns=\"https://www.omg.org/spec/DMN/20191111/MODEL/\" \
             name=\"Loans\" id=\"loans\">\n{body}</definitions>\n"
        )
    }

    /// #673 (DES-675): two inputs with one label in one table are two nodes, each keyed by its
    /// DMN `id`, and inserting a third before them changes neither id.
    #[test]
    fn same_label_inputs_keep_their_own_ids_when_one_is_inserted_673() {
        let table = |inputs: &str| {
            model(&format!(
                "  <decision name=\"Eligibility\" id=\"elig\">\n    \
                 <decisionTable id=\"t1\">\n{inputs}    </decisionTable>\n  </decision>\n"
            ))
        };
        let two =
            "      <input id=\"in1\" label=\"Age\"/>\n      <input id=\"in2\" label=\"Age\"/>\n";
        let before = ids(&table(two));
        let before = named(&before, "Age");
        assert_eq!(before.len(), 2, "{before:?}");
        assert_ne!(before[0], before[1]);
        let three = format!("      <input id=\"in0\" label=\"Age\"/>\n{two}");
        let after = ids(&table(&three));
        let after = named(&after, "Age");
        assert_eq!(after.len(), 3, "{after:?}");
        assert!(
            before.iter().all(|id| after.contains(id)),
            "{before:?} -> {after:?}"
        );
    }

    /// DES-675 (c): a DMN `id` repeated in one file (a malformed model) never gives two
    /// elements one id: the first keeps it, the later one is keyed under its own table.
    #[test]
    fn a_repeated_dmn_id_never_merges_two_elements_675() {
        let got = ids(&model(
            "  <decision name=\"A\" id=\"a\">\n    <decisionTable id=\"ta\">\n      \
             <input id=\"dup\" label=\"Age\"/>\n    </decisionTable>\n  </decision>\n  \
             <decision name=\"B\" id=\"b\">\n    <decisionTable id=\"tb\">\n      \
             <input id=\"dup\" label=\"Age\"/>\n    </decisionTable>\n  </decision>\n",
        ));
        let age = named(&got, "Age");
        assert_eq!(age.len(), 2, "{got:?}");
        assert_ne!(age[0], age[1]);
    }

    /// DES-675 (d): a DMN input without an `id` is keyed under the nearest element that has
    /// one, so same-labelled inputs of two tables stay two nodes.
    #[test]
    fn an_input_without_an_id_is_keyed_under_its_table_675() {
        let got = ids(&model(
            "  <decision name=\"A\" id=\"a\">\n    <decisionTable id=\"ta\">\n      \
             <input label=\"Age\"/>\n    </decisionTable>\n  </decision>\n  \
             <decision name=\"B\" id=\"b\">\n    <decisionTable id=\"tb\">\n      \
             <input label=\"Age\"/>\n    </decisionTable>\n  </decision>\n",
        ));
        let age = named(&got, "Age");
        assert_eq!(age.len(), 2, "{got:?}");
        assert!(
            age.iter()
                .any(|id| id.contains("decisionTable#ta/input:Age")),
            "{age:?}"
        );
    }
}
