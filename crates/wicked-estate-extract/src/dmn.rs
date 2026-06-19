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

[[node_mappings]]
element   = "decision"
emit_kind = "rule"
name_attr = "name"

[[node_mappings]]
element              = "decisionTable"
emit_kind            = "rule_set"
name_attr            = "id"

[[node_mappings]]
element   = "rule"
emit_kind = "rule"
name_attr = "id"

[[node_mappings]]
element   = "input"
emit_kind = "condition"
name_attr = "label"

[[node_mappings]]
element   = "output"
emit_kind = "action"
name_attr = "label"

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
    /// Fails only if the embedded config is malformed (compile-time defect — never at runtime).
    pub fn new() -> Result<Self> {
        let inner = XmlRulesExtractor::from_toml(DMN_CONFIG)
            .map_err(|e| wicked_estate_core::Error::Extraction(e.to_string()))?;
        Ok(Self(inner))
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
