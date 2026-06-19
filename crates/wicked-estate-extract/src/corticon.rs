//! W15.10 — Progress Corticon (`.ers` rulesheet / `.ecore` vocabulary) extractor.
//!
//! Corticon assets are EMF/XMI XML. The element structure here is taken from **real** specimens in
//! the official `corticon/corticon-classic-samples` repository (not invented) — e.g. a rulesheet is:
//!
//! ```xml
//! <com.corticon.rulesemf.assetmodel:RulesheetAsset …>
//!   <ruleset vocabulary="Maintenance.ecore#/">
//!     <rule>
//!       <condition><opaqueExpression expression="plane.totalMiles.mod(5000) &lt; 250"/></condition>
//!       <action><opaqueExpression expression="plane.maintenance += Maintenance.new[…]"/></action>
//!     </rule>
//!     <filter><opaqueExpression expression="plane.totalMiles > 0"/></filter>
//!   </ruleset>
//! </com.corticon.rulesemf.assetmodel:RulesheetAsset>
//! ```
//!
//! Mapping (a dedicated roxmltree pass — the generic [`XmlRulesExtractor`](crate::XmlRulesExtractor)
//! element→kind config can't name a node from a *grandchild* attribute, nor emit Corticon's nameless
//! positional `<rule>` rows):
//!
//! - `RulesheetAsset` root             → [`NodeKind::RuleSet`] (named from the file stem)
//! - `ruleset/@vocabulary`             → [`NodeKind::Fact`] (the referenced `.ecore` vocabulary)
//! - each `<rule>` with logic          → [`NodeKind::Rule`] (positional)
//! - `<condition>/<opaqueExpression>`  → [`NodeKind::Condition`] (signature = the expression)
//! - `<action>/<opaqueExpression>`     → [`NodeKind::Action`]
//! - `<filter>/<opaqueExpression>`     → [`NodeKind::Condition`]
//!
//! For an EMF `.ecore` vocabulary file (a public OMG/Eclipse standard): each `EClass` `eClassifier`
//! → [`NodeKind::Fact`]. All edges carry [`ResolutionTier::Heuristic`]; IDs use `Symbol::synthetic`.
//! Stays in the MIT core — pure roxmltree (MIT/Apache), no proprietary/GPL dependency.

#![cfg(feature = "xml-rules")]

use wicked_estate_core::{
    Edge, EdgeKind, Error, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Symbol,
};

const LANG: &str = "progress-corticon";

/// First `<opaqueExpression>` child's `expression` attribute, the human-readable rule logic.
fn opaque_expression(node: roxmltree::Node<'_, '_>) -> Option<String> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == "opaqueExpression")
        .and_then(|c| c.attribute("expression"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn file_stem(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    base.strip_suffix(".ers")
        .or_else(|| base.strip_suffix(".erf"))
        .or_else(|| base.strip_suffix(".ecore"))
        .unwrap_or(base)
        .to_string()
}

/// Extractor for Progress Corticon `.ers`/`.erf` rulesheets and `.ecore` vocabularies.
pub struct CorticonExtractor;

impl CorticonExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CorticonExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for CorticonExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new(LANG)]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let doc =
            roxmltree::Document::parse(&file.text).map_err(|e| Error::Extraction(e.to_string()))?;
        let lang = Language::new(LANG);
        let loc = || Location::new(&file.path, Span::ZERO);
        let mut nodes = Vec::new();
        let mut local_edges = Vec::new();

        let root = doc.root_element();
        let root_local = root.tag_name().name();

        // ── .ecore vocabulary: each EClass eClassifier → Fact ────────────────────────────────
        if root_local == "EPackage" || file.path.ends_with(".ecore") {
            for n in doc.descendants().filter(|n| n.is_element()) {
                if n.tag_name().name() != "eClassifiers" {
                    continue;
                }
                // EMF: xsi:type="ecore:EClass" carries the entity; attribute lookup is
                // namespace-agnostic on the local `type`/`name`.
                let is_class = n
                    .attributes()
                    .any(|a| a.name() == "type" && a.value().ends_with("EClass"));
                let Some(name) = n.attribute("name") else {
                    continue;
                };
                if !is_class {
                    continue;
                }
                let sym =
                    Symbol::synthetic("corticon", format!("{}::fact::{}", file.path, name)).id();
                let mut node = Node::new(sym, NodeKind::Fact, name, lang.clone(), loc());
                node.signature = Some(format!("entity {name}"));
                nodes.push(node);
            }
            return Ok(Extraction {
                nodes,
                local_edges,
                refs: Vec::new(),
            });
        }

        // ── .ers / .erf rulesheet: RulesheetAsset → RuleSet ───────────────────────────────────
        let ruleset_name = file_stem(&file.path);
        let ruleset_sym = Symbol::synthetic(
            "corticon",
            format!("{}::ruleset::{}", file.path, ruleset_name),
        )
        .id();
        let mut rs_node = Node::new(
            ruleset_sym.clone(),
            NodeKind::RuleSet,
            &ruleset_name,
            lang.clone(),
            loc(),
        );
        rs_node.signature = Some(format!("Corticon rulesheet {ruleset_name}"));
        nodes.push(rs_node);

        // ruleset/@vocabulary → Fact (the referenced .ecore vocabulary). Iterate ALL <ruleset>
        // elements (deduped) so a multi-ruleset file does not lose vocabularies after the first.
        let mut seen_vocab = std::collections::BTreeSet::new();
        for ruleset in doc
            .descendants()
            .filter(|n| n.tag_name().name() == "ruleset")
        {
            let Some(vocab) = ruleset.attribute("vocabulary") else {
                continue;
            };
            // e.g. "Maintenance.ecore#/" → "Maintenance.ecore"
            let vocab_name = vocab.split('#').next().unwrap_or(vocab).trim();
            if vocab_name.is_empty() || !seen_vocab.insert(vocab_name.to_string()) {
                continue;
            }
            let fsym =
                Symbol::synthetic("corticon", format!("{}::fact::{}", file.path, vocab_name)).id();
            let mut fnode = Node::new(
                fsym.clone(),
                NodeKind::Fact,
                vocab_name,
                lang.clone(),
                loc(),
            );
            fnode.signature = Some(format!("vocabulary {vocab_name}"));
            nodes.push(fnode);
            local_edges.push(Edge::new(
                ruleset_sym.clone(),
                fsym,
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "corticon",
            ));
        }

        // Each <rule> that carries condition/action logic → Rule (positional).
        let mut rule_ord = 0usize;
        for rule in doc.descendants().filter(|n| n.tag_name().name() == "rule") {
            let conditions: Vec<_> = rule
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "condition")
                .collect();
            let actions: Vec<_> = rule
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "action")
                .collect();
            if conditions.is_empty() && actions.is_empty() {
                continue; // empty placeholder <rule/> (the header column) — no logic, skip
            }
            rule_ord += 1;
            let rule_name = format!("rule#{rule_ord}");
            let rule_sym =
                Symbol::synthetic("corticon", format!("{}::rule::{}", file.path, rule_name)).id();
            let mut rule_node = Node::new(
                rule_sym.clone(),
                NodeKind::Rule,
                &rule_name,
                lang.clone(),
                loc(),
            );
            rule_node.signature = Some(format!("Corticon {rule_name}"));
            nodes.push(rule_node);
            local_edges.push(Edge::new(
                ruleset_sym.clone(),
                rule_sym.clone(),
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "corticon",
            ));

            for (i, cond) in conditions.iter().enumerate() {
                let expr = opaque_expression(*cond)
                    .unwrap_or_else(|| format!("{rule_name} condition {i}"));
                let csym = Symbol::synthetic(
                    "corticon",
                    format!("{}::condition::{}::{}", file.path, rule_name, i),
                )
                .id();
                let mut cnode = Node::new(
                    csym.clone(),
                    NodeKind::Condition,
                    format!("{rule_name}::cond{i}"),
                    lang.clone(),
                    loc(),
                );
                cnode.signature = Some(expr);
                nodes.push(cnode);
                local_edges.push(Edge::new(
                    rule_sym.clone(),
                    csym,
                    EdgeKind::Evaluates,
                    ResolutionTier::Heuristic,
                    "corticon",
                ));
            }
            for (i, act) in actions.iter().enumerate() {
                let expr =
                    opaque_expression(*act).unwrap_or_else(|| format!("{rule_name} action {i}"));
                let asym = Symbol::synthetic(
                    "corticon",
                    format!("{}::action::{}::{}", file.path, rule_name, i),
                )
                .id();
                let mut anode = Node::new(
                    asym.clone(),
                    NodeKind::Action,
                    format!("{rule_name}::act{i}"),
                    lang.clone(),
                    loc(),
                );
                anode.signature = Some(expr);
                nodes.push(anode);
                local_edges.push(Edge::new(
                    rule_sym.clone(),
                    asym,
                    EdgeKind::Produces,
                    ResolutionTier::Heuristic,
                    "corticon",
                ));
            }
        }

        // Rulesheet-level <filter> expressions → Condition on the RuleSet.
        for (i, filter) in doc
            .descendants()
            .filter(|n| n.tag_name().name() == "filter")
            .enumerate()
        {
            let Some(expr) = opaque_expression(filter) else {
                continue;
            };
            let fsym = Symbol::synthetic("corticon", format!("{}::filter::{}", file.path, i)).id();
            let mut fnode = Node::new(
                fsym.clone(),
                NodeKind::Condition,
                format!("filter#{i}"),
                lang.clone(),
                loc(),
            );
            fnode.signature = Some(expr);
            nodes.push(fnode);
            local_edges.push(Edge::new(
                ruleset_sym.clone(),
                fsym,
                EdgeKind::Contains,
                ResolutionTier::Heuristic,
                "corticon",
            ));
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

    fn sf(path: &str, text: &str) -> SourceFile {
        SourceFile {
            path: path.to_string(),
            language: Language::new(LANG),
            text: text.to_string(),
        }
    }

    // Structure copied from a REAL specimen:
    // corticon/corticon-classic-samples → Airplane maintenance/.../Maintenance_Change_Tires.ers
    const ERS: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<com.corticon.rulesemf.assetmodel:RulesheetAsset xmlns:com.corticon.rulesemf.assetmodel="http:///com/corticon/rulesemf/assetmodel.ecore" majorVersionNumber="2">
  <languageCode>en_US</languageCode>
  <ruleset vocabulary="Maintenance.ecore#/">
    <rule/>
    <rule documentingRuleStatements="#//@ruleset/@ruleStatements.0">
      <condition>
        <opaqueExpression expression="plane.totalMiles.mod ( 5000) &lt; 250"/>
        <viewExpressions lhs="plane.totalMiles.mod ( 5000)" rhs="&lt; 250"/>
      </condition>
      <action>
        <opaqueExpression expression="plane.maintenance += Maintenance.new[description='Change Tires']"/>
      </action>
    </rule>
    <filter>
      <opaqueExpression expression="plane.totalMiles > 0"/>
    </filter>
  </ruleset>
</com.corticon.rulesemf.assetmodel:RulesheetAsset>"##;

    const ECORE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ecore:EPackage xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:ecore="http://www.eclipse.org/emf/2002/Ecore" name="Maintenance">
  <eClassifiers xsi:type="ecore:EClass" name="Aircraft">
    <eStructuralFeatures xsi:type="ecore:EAttribute" name="totalMiles"/>
  </eClassifiers>
  <eClassifiers xsi:type="ecore:EClass" name="Maintenance">
    <eStructuralFeatures xsi:type="ecore:EAttribute" name="estimatedCost"/>
  </eClassifiers>
</ecore:EPackage>"#;

    #[test]
    fn ers_root_is_ruleset() {
        let ex = CorticonExtractor::new()
            .extract(&sf("Maintenance_Change_Tires.ers", ERS))
            .unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::RuleSet && n.name == "Maintenance_Change_Tires"),
            "expected a RuleSet named from the .ers file stem"
        );
    }

    #[test]
    fn ers_emits_rule_condition_action() {
        let ex = CorticonExtractor::new()
            .extract(&sf("tires.ers", ERS))
            .unwrap();
        let n = |k: NodeKind| ex.nodes.iter().filter(|x| x.kind == k).count();
        assert_eq!(
            n(NodeKind::Rule),
            1,
            "one logic-bearing rule (the empty <rule/> is skipped)"
        );
        // one rule condition + one rulesheet filter = 2 Condition nodes
        assert_eq!(n(NodeKind::Condition), 2, "rule condition + filter");
        assert_eq!(n(NodeKind::Action), 1, "one action");
    }

    #[test]
    fn ers_condition_carries_expression() {
        let ex = CorticonExtractor::new()
            .extract(&sf("tires.ers", ERS))
            .unwrap();
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::Condition
                && n.signature
                    .as_deref()
                    .unwrap_or("")
                    .contains("totalMiles.mod")),
            "a Condition should carry its opaqueExpression text"
        );
    }

    #[test]
    fn ers_vocabulary_reference_is_a_fact() {
        let ex = CorticonExtractor::new()
            .extract(&sf("tires.ers", ERS))
            .unwrap();
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Fact && n.name == "Maintenance.ecore"),
            "the referenced .ecore vocabulary should be a Fact"
        );
    }

    #[test]
    fn ecore_classes_become_facts() {
        let ex = CorticonExtractor::new()
            .extract(&sf("Maintenance.ecore", ECORE))
            .unwrap();
        let facts: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Fact)
            .map(|n| n.name.as_str())
            .collect();
        assert!(facts.contains(&"Aircraft"), "got {facts:?}");
        assert!(facts.contains(&"Maintenance"), "got {facts:?}");
    }
}
