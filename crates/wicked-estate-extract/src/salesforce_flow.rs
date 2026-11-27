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
    use wicked_estate_core::{NodeKind, Symbol, SymbolId};

    #[test]
    fn config_parses_without_panic() {
        // Verifies the embedded TOML is valid; new() would panic if not.
        let _ = SalesforceFlowExtractor::new();
    }

    /// #663 round 1 (codex): a node's id was its file, its ancestors' tag names and its own
    /// name, so the two `Account.Status__c` conditions under different outcomes of the fixture
    /// got one id, and the store kept one. Every node keeps its own id.
    #[test]
    fn same_named_conditions_under_different_rules_keep_their_own_ids() {
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
        let conditions = ex
            .nodes
            .iter()
            .filter(|n| n.name == "Account.Status__c")
            .count();
        assert_eq!(conditions, 2);
        // Each edge ends at a node the extraction emitted, and each Account.Status__c condition
        // hangs off its own rule.
        assert!(!ex.local_edges.is_empty());
        for e in &ex.local_edges {
            assert!(ids.contains(&e.source) && ids.contains(&e.target), "{e:?}");
        }
        let keyed = keyed_ids(&ex);
        assert!(
            keyed.contains_key("Is Active Premium/Account.Status__c"),
            "{keyed:?}"
        );
        assert!(
            keyed.contains_key("Is Inactive/Account.Status__c"),
            "{keyed:?}"
        );
    }

    const FIXTURE: &str =
        include_str!("../tests/fixtures/salesforce_flow/account_validation.flow-meta.xml");

    fn flow(text: &str) -> Extraction {
        SalesforceFlowExtractor::new()
            .extract(&SourceFile {
                path: "account_validation.flow-meta.xml".into(),
                language: Language::new("xml-rules:salesforce-flow"),
                text: text.into(),
            })
            .unwrap()
    }

    /// Each node's id keyed by its name, and a condition's by its rule's name too.
    fn keyed_ids(ex: &Extraction) -> std::collections::BTreeMap<String, SymbolId> {
        let name = |id: &SymbolId| {
            ex.nodes
                .iter()
                .find(|n| &n.symbol == id)
                .map(|n| n.name.clone())
                .unwrap_or_default()
        };
        let mut out = std::collections::BTreeMap::new();
        for n in &ex.nodes {
            let parent = ex
                .local_edges
                .iter()
                .find(|e| e.target == n.symbol && n.kind == NodeKind::Condition)
                .map(|e| format!("{}/", name(&e.source)))
                .unwrap_or_default();
            out.insert(format!("{parent}{}", n.name), n.symbol.clone());
        }
        out
    }

    /// #669 codex-check: requirement annotations are restored by exact id after a re-index,
    /// so a node whose id was already unique keeps the id earlier releases gave it (its
    /// ancestors' tags and its name). Only the same-named conditions that shared one id get
    /// new, name-qualified ids.
    #[test]
    fn unique_ids_keep_their_released_shape_669() {
        let ex = flow(FIXTURE);
        let keyed = keyed_ids(&ex);
        let released = |path: &str, name: &str| {
            Symbol::synthetic(
                "xml-rules",
                format!("account_validation.flow-meta.xml::{path}::{name}"),
            )
            .id()
        };
        assert_eq!(
            keyed["Is Active Premium"],
            released("Flow/decisions/rules/rules", "Is Active Premium")
        );
        assert_eq!(
            keyed["Check Account Status"],
            released("Flow/decisions/decisions", "Check Account Status")
        );
        assert_eq!(
            keyed["Is Active Premium/Account.Type"],
            released("Flow/decisions/rules/conditions/conditions", "Account.Type")
        );
        let shared = released(
            "Flow/decisions/rules/conditions/conditions",
            "Account.Status__c",
        );
        assert_ne!(keyed["Is Active Premium/Account.Status__c"], shared);
        assert_ne!(keyed["Is Inactive/Account.Status__c"], shared);
    }

    /// estate#667 round 1 (ADR-002): a node's id does not depend on unrelated siblings. Adding
    /// a rule before the others and swapping the two outcomes leaves every existing node,
    /// including each Account.Status__c condition, with the id it had.
    #[test]
    fn ids_survive_an_inserted_rule_and_reordered_outcomes_667() {
        let start = FIXTURE.find("        <rules>").unwrap();
        let second = FIXTURE[start + 1..].find("        <rules>").unwrap() + start + 1;
        let end = FIXTURE.find("    </decisions>").unwrap();
        let (first_rule, second_rule) = (&FIXTURE[start..second], &FIXTURE[second..end]);
        let inserted = "        <rules>\n            <name>Is_New</name>\n            \
            <label>Is New</label>\n            <conditions>\n                \
            <leftValueReference>Account.Status__c</leftValueReference>\n            \
            </conditions>\n        </rules>\n";
        let edited = format!(
            "{}{inserted}{second_rule}{first_rule}{}",
            &FIXTURE[..start],
            &FIXTURE[end..]
        );
        let before = keyed_ids(&flow(FIXTURE));
        let after = keyed_ids(&flow(&edited));
        assert_eq!(after.len(), before.len() + 2, "{after:?}");
        for (key, id) in &before {
            assert_eq!(after.get(key), Some(id), "{key} keeps its id");
        }
    }

    #[test]
    fn languages_reports_salesforce_flow() {
        let extractor = SalesforceFlowExtractor::new();
        let langs = extractor.languages();
        assert_eq!(langs.len(), 1);
        assert_eq!(langs[0].as_str(), "xml-rules:salesforce-flow");
    }
}
