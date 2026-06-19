//! W15.6 — Salesforce Flow extractor.
//!
//! Salesforce Flow XML uses **child elements** for names instead of XML attributes:
//!
//! ```xml
//! <decisions>
//!     <name>Check_Account_Type</name>
//!     <label>Check Account Type</label>
//!     <rules>
//!         <name>Is_Premium</name>
//!         <label>Is Premium Account</label>
//!         <conditions>
//!             <leftValueReference>Account.Type</leftValueReference>
//!         </conditions>
//!     </rules>
//! </decisions>
//! ```
//!
//! This module is a thin wrapper around [`XmlRulesExtractor`] with an embedded TOML config that
//! maps Salesforce Flow XML elements to the rules-model graph using the `name_child` extension
//! added to [`NodeMapping`] in W15.6.
//!
//! Gate: requires the `xml-rules` feature.

#![cfg(feature = "xml-rules")]

use wicked_estate_core::{Extraction, Extractor, Language, Result, SourceFile};

use crate::xml_rules::{XmlRulesConfig, XmlRulesExtractor};

// ── Embedded TOML config ──────────────────────────────────────────────────────

const FLOW_CONFIG: &str = r#"
[engine]
name       = "salesforce-flow"
file_globs = ["**/*.flow-meta.xml", "**/*.flow"]

[[node_mappings]]
element    = "Flow"
emit_kind  = "rule_set"
name_child = "label"

[[node_mappings]]
element    = "decisions"
emit_kind  = "rule_set"
name_child = "label"

[[node_mappings]]
element    = "rules"
emit_kind  = "rule"
name_child = "label"

[[node_mappings]]
element    = "conditions"
emit_kind  = "condition"
name_child = "leftValueReference"

[[node_mappings]]
element    = "actionCalls"
emit_kind  = "action"
name_child = "label"

[[edge_mappings]]
parent_element = "decisions"
child_element  = "rules"
edge_kind      = "contains"

[[edge_mappings]]
parent_element = "rules"
child_element  = "conditions"
edge_kind      = "evaluates"
"#;

// ── Extractor ─────────────────────────────────────────────────────────────────

/// Extracts a rules-model graph from Salesforce Flow metadata XML (`.flow-meta.xml` / `.flow`).
///
/// Delegates entirely to [`XmlRulesExtractor`] with an embedded TOML config. No per-element
/// Rust logic — rules as data (CLAUDE.md).
pub struct SalesforceFlowExtractor(XmlRulesExtractor);

impl SalesforceFlowExtractor {
    /// Build a new extractor from the embedded config. Panics if the embedded TOML is malformed
    /// (that would be a programming error, not a runtime error).
    pub fn new() -> Self {
        let cfg: XmlRulesConfig =
            toml::from_str(FLOW_CONFIG).expect("embedded Salesforce Flow TOML config is valid");
        Self(XmlRulesExtractor::new(cfg))
    }
}

impl Default for SalesforceFlowExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for SalesforceFlowExtractor {
    fn languages(&self) -> Vec<Language> {
        self.0.languages()
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        self.0.extract(file)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parses_without_panic() {
        // Verifies the embedded TOML is valid; new() would panic if not.
        let _ = SalesforceFlowExtractor::new();
    }

    #[test]
    fn languages_reports_salesforce_flow() {
        let extractor = SalesforceFlowExtractor::new();
        let langs = extractor.languages();
        assert_eq!(langs.len(), 1);
        assert_eq!(langs[0].as_str(), "xml-rules:salesforce-flow");
    }
}
