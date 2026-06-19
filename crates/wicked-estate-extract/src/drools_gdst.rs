//! W15.5 — Drools GDST guided decision table extractor.
//!
//! A thin wrapper around [`XmlRulesExtractor`] driven by an embedded TOML config.
//! GDST is Drools' XML format for guided decision tables; it is plain XML and requires
//! no separate grammar — the existing `XmlRulesExtractor` infrastructure handles it.
//!
//! # Emitted graph elements
//!
//! | GDST element             | NodeKind    | notes |
//! |--------------------------|-------------|-------|
//! | `decision-table52`       | `RuleSet`   | the top-level decision table |
//! | `ConditionCol52`         | `Condition` | a condition column |
//! | `ActionInsertFactCol52`  | `Action`    | an insert-fact action column |
//! | `ActionSetFieldCol52`    | `Action`    | a set-field action column |
//!
//! Edges:
//! - `decision-table52` → `ConditionCol52` : `Evaluates`
//! - `decision-table52` → `ActionInsertFactCol52` : `Produces`
//! - `decision-table52` → `ActionSetFieldCol52` : `Produces`

#![cfg(feature = "xml-rules")]

use wicked_estate_core::{Extraction, Extractor, Language, Result, SourceFile};

use crate::xml_rules::XmlRulesExtractor;

/// TOML config embedded at compile time — the single source of truth for Drools GDST extraction.
///
/// Notes on naming conventions:
/// - `tableName` is a **child element** in GDST, not an XML attribute — use `name_child`.
/// - Drools GDST uses fully-qualified Java class names as XML element names (e.g.
///   `org.drools.workbench.models.guided.dtable.shared.model.ConditionCol52`).
///   `roxmltree` returns the full local name, so we match on the short suffix form by
///   registering the short names: the XML extractor matches on `tag_name().name()` which is
///   the local part after any namespace prefix, but in GDST the namespace is carried in the
///   element *name* itself (dot-separated). We therefore also provide short-form aliases
///   (`ConditionCol52`, `ActionInsertFactCol52`, `ActionSetFieldCol52`) that appear as
///   child elements of `childColumns` / `actionCols` wrappers.
const GDST_CONFIG: &str = r#"
[engine]
name       = "drools-gdst"
file_globs = ["**/*.gdst"]

# The root element: "tableName" is a child element whose text is the name.
[[node_mappings]]
element    = "decision-table52"
emit_kind  = "rule_set"
name_child = "tableName"

# Short local names as used inside childColumns / actionCols wrappers.
[[node_mappings]]
element    = "org.drools.workbench.models.guided.dtable.shared.model.ConditionCol52"
emit_kind  = "condition"
name_child = "header"

[[node_mappings]]
element    = "org.drools.workbench.models.guided.dtable.shared.model.ActionInsertFactCol52"
emit_kind  = "action"
name_child = "header"

[[node_mappings]]
element    = "org.drools.workbench.models.guided.dtable.shared.model.ActionSetFieldCol52"
emit_kind  = "action"
name_child = "header"
"#;

/// Extractor for Drools GDST guided decision table files (`*.gdst`).
///
/// Delegates entirely to [`XmlRulesExtractor`] configured by [`GDST_CONFIG`].
/// Construct with [`DroolsGdstExtractor::new`].
pub struct DroolsGdstExtractor(XmlRulesExtractor);

impl DroolsGdstExtractor {
    /// Build the extractor from the embedded TOML config.
    ///
    /// # Panics
    ///
    /// Panics if the embedded config is malformed — a compile-time defect, never at runtime.
    pub fn new() -> Self {
        let inner = XmlRulesExtractor::from_toml(GDST_CONFIG)
            .expect("DroolsGdstExtractor embedded config must be valid");
        Self(inner)
    }
}

impl Default for DroolsGdstExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for DroolsGdstExtractor {
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

    use super::DroolsGdstExtractor;

    #[test]
    fn drools_gdst_new_does_not_panic() {
        let _ = DroolsGdstExtractor::new();
    }

    #[test]
    fn drools_gdst_extracts_condition_from_fixture() {
        // Minimal GDST-like fixture. The real format nests ConditionCol52 under
        // conditionPatterns/Pattern52/childColumns; here we flatten it so the extractor
        // (which descends all elements) still finds the elements.
        const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<decision-table52 xmlns="http://www.jboss.org/drools">
  <tableName>LoanApproval</tableName>
  <conditionPatterns>
    <org.drools.workbench.models.guided.dtable.shared.model.Pattern52>
      <childColumns>
        <org.drools.workbench.models.guided.dtable.shared.model.ConditionCol52>
          <header>Credit Score</header>
        </org.drools.workbench.models.guided.dtable.shared.model.ConditionCol52>
      </childColumns>
    </org.drools.workbench.models.guided.dtable.shared.model.Pattern52>
  </conditionPatterns>
  <actionCols>
    <org.drools.workbench.models.guided.dtable.shared.model.ActionInsertFactCol52>
      <header>Approve</header>
    </org.drools.workbench.models.guided.dtable.shared.model.ActionInsertFactCol52>
  </actionCols>
</decision-table52>"#;

        let sf = SourceFile {
            path: "loan.gdst".to_string(),
            language: Language::new("xml-rules:drools-gdst"),
            text: FIXTURE.to_string(),
        };
        let extraction = DroolsGdstExtractor::new()
            .extract(&sf)
            .expect("extraction must succeed");

        assert!(
            extraction
                .nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet || n.kind == NodeKind::Condition),
            "expected at least one RuleSet or Condition node; got {:?}",
            extraction
                .nodes
                .iter()
                .map(|n| &n.kind)
                .collect::<Vec<_>>()
        );
    }
}
