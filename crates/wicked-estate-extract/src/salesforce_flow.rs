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
        let mut inner = XmlRulesExtractor::new(cfg);
        // A Flow element's API `<name>` is unique in the flow and survives label edits (#675).
        for element in ["decisions", "rules", "actionCalls"] {
            inner = inner.with_id_child(element, "name");
        }
        Self(inner)
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

    /// #675 (from #663 round 1, codex): the two `Account.Status__c` conditions under different
    /// outcomes of the fixture got one id, and the store kept one. Each is keyed under its rule's
    /// API name, so every node keeps its own id.
    #[test]
    fn same_named_conditions_under_different_rules_keep_their_own_ids_675() {
        let file = SourceFile {
            path: "account_validation.flow-meta.xml".into(),
            language: Language::new("xml-rules:salesforce-flow"),
            text: include_str!(
                "../tests/fixtures/salesforce_flow/account_validation.flow-meta.xml"
            )
            .into(),
        };
        let ex = SalesforceFlowExtractor::new().extract(&file).unwrap();
        let ids: std::collections::HashSet<_> = ex.nodes.iter().map(|n| &n.symbol).collect();
        assert_eq!(ids.len(), ex.nodes.len(), "one id per node");
        let status: Vec<&str> = ex
            .nodes
            .iter()
            .filter(|n| n.name == "Account.Status__c")
            .map(|n| n.symbol.as_str())
            .collect();
        assert_eq!(status.len(), 2);
        assert!(
            status
                .iter()
                .any(|id| id.contains("rules#Is_Active_Premium/")),
            "{status:?}"
        );
        assert!(
            status.iter().any(|id| id.contains("rules#Is_Inactive/")),
            "{status:?}"
        );
        for e in &ex.local_edges {
            assert!(ids.contains(&e.source) && ids.contains(&e.target), "{e:?}");
        }
    }

    fn flow_ids(conditions: &[(&str, &str)]) -> Vec<String> {
        let conditions: String = conditions
            .iter()
            .map(|(field, op)| {
                format!(
                    "<conditions><leftValueReference>{field}</leftValueReference>\
                     <operator>{op}</operator></conditions>"
                )
            })
            .collect();
        let text = format!(
            "<Flow xmlns=\"http://soap.sforce.com/2006/04/metadata\"><label>F</label>\
             <decisions><name>D</name><label>D</label><rules><name>R</name><label>R</label>\
             {conditions}</rules></decisions></Flow>"
        );
        let file = SourceFile {
            path: "f.flow-meta.xml".into(),
            language: Language::new("xml-rules:salesforce-flow"),
            text,
        };
        let ex = SalesforceFlowExtractor::new().extract(&file).unwrap();
        ex.nodes
            .into_iter()
            .filter(|n| n.name == "Amount")
            .map(|n| n.symbol.0)
            .collect()
    }

    /// #679: two conditions on one field that differ only in whitespace inside a literal are
    /// two nodes; the digest that tells them apart keeps whitespace.
    #[test]
    fn conditions_differing_only_in_literal_whitespace_stay_two_nodes_679() {
        let ids = flow_ids(&[("Amount", "A B"), ("Amount", "A  B")]);
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert_ne!(ids[0], ids[1]);
    }

    /// #679: the digest keeps whitespace but reads line ends as XML does, so a CRLF (or CR)
    /// checkout of the same file keeps every id.
    #[test]
    fn a_crlf_checkout_keeps_the_ids_679() {
        let lf = flow_ids(&[("Amount", "A\nB"), ("Amount", "A B")]);
        assert_eq!(lf.len(), 2, "{lf:?}");
        assert_eq!(flow_ids(&[("Amount", "A\r\nB"), ("Amount", "A B")]), lf);
        assert_eq!(flow_ids(&[("Amount", "A\rB"), ("Amount", "A B")]), lf);
    }

    /// #684 round 1 (codex P2): re-indenting the file keeps every id (ADR-002), while the
    /// literals still tell the two conditions apart.
    #[test]
    fn re_indenting_keeps_the_ids_679() {
        let compact = flow_ids(&[("Amount", "A B"), ("Amount", "A  B")]);
        assert_eq!(compact.len(), 2, "{compact:?}");
        let indented = |sep: &str| {
            let c = |op: &str| {
                format!(
                    "<conditions>{sep}<leftValueReference>Amount</leftValueReference>\
                     {sep}<operator>{op}</operator>{sep}</conditions>"
                )
            };
            let text = format!(
                "<Flow xmlns=\"http://soap.sforce.com/2006/04/metadata\"><label>F</label>\
                 <decisions><name>D</name><label>D</label><rules><name>R</name><label>R</label>\
                 {sep}{}{sep}{}{sep}</rules></decisions></Flow>",
                c("A B"),
                c("A  B")
            );
            let ex = SalesforceFlowExtractor::new()
                .extract(&SourceFile {
                    path: "f.flow-meta.xml".into(),
                    language: Language::new("xml-rules:salesforce-flow"),
                    text,
                })
                .unwrap();
            ex.nodes
                .into_iter()
                .filter(|n| n.name == "Amount")
                .map(|n| n.symbol.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(indented(""), compact);
        assert_eq!(indented("\n    "), compact);
        assert_eq!(indented("\r\n\t\t"), compact);
    }

    /// Round 1 (agy P2-2): two conditions on one field under one rule (a range) are two
    /// nodes, told apart by content, and inserting a third condition moves neither.
    #[test]
    fn two_conditions_on_one_field_stay_two_nodes_675() {
        let before = flow_ids(&[("Amount", "GreaterThan"), ("Amount", "LessThan")]);
        assert_eq!(before.len(), 2, "{before:?}");
        assert_ne!(before[0], before[1]);
        let after = flow_ids(&[
            ("Amount", "EqualTo"),
            ("Amount", "GreaterThan"),
            ("Amount", "LessThan"),
        ]);
        assert_eq!(after.len(), 3, "{after:?}");
        assert!(
            before.iter().all(|id| after.contains(id)),
            "{before:?} -> {after:?}"
        );
    }
}
