//! W15.2 — XML rules extractor (TOML path-config + `roxmltree` DOM traversal).
//!
//! Handles structured XML rules documents: DMN (Decision Model and Notation), Drools DRL
//! descriptors, BPMN decision tables, and any XML dialect that maps elements to rule-model nodes.
//! The mapping is **entirely data-driven** — a `XmlRulesConfig` TOML file describes which XML
//! element names emit which `NodeKind`s and which parent→child pairs emit `EdgeKind`s. No
//! per-dialect Rust code is required.
//!
//! # Config TOML example (DMN)
//! ```toml
//! [engine]
//! name       = "dmn"
//! file_globs = ["**/*.dmn"]
//!
//! [[node_mappings]]
//! element   = "definitions"
//! emit_kind = "rule_set"
//! name_attr = "name"
//!
//! [[node_mappings]]
//! element   = "decision"
//! emit_kind = "rule"
//! name_attr = "name"
//!
//! [[edge_mappings]]
//! parent_element = "definitions"
//! child_element  = "decision"
//! edge_kind      = "governs"
//! ```
//!
//! # Node identity
//! `Symbol::synthetic("xml-rules", "<file>::<element_path>::<name>")` — stable as long as the
//! element name attribute does not change (ADR-002).

#![cfg(feature = "xml-rules")]

use serde::Deserialize;
use wicked_estate_core::{
    Edge, EdgeKind, Error, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Symbol,
};

// ── Config structs (TOML-deserializable) ─────────────────────────────────────

/// Top-level config for an `XmlRulesExtractor` instance.
#[derive(Debug, Clone, Deserialize)]
pub struct XmlRulesConfig {
    pub engine: EngineConfig,
    pub node_mappings: Vec<NodeMapping>,
    #[serde(default)]
    pub edge_mappings: Vec<EdgeMapping>,
}

/// Engine identity + file-glob filter (informational — the extractor itself does not glob; that
/// is the indexer's job).
#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    pub name: String,
    pub file_globs: Vec<String>,
}

/// How a single XML element name is mapped to a graph `NodeKind`.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeMapping {
    /// XML element (local) name to match, e.g. `"decision"`.
    pub element: String,
    /// Target `NodeKind` as a snake_case string: `"rule"`, `"rule_set"`, `"condition"`,
    /// `"action"`, or `"fact"`.
    pub emit_kind: String,
    /// Attribute whose value is used as the node name (takes priority over `name_child` and
    /// `name_text`).
    pub name_attr: Option<String>,
    /// Extract the name from the text of a named child element (e.g. `name_child = "label"`
    /// reads the text content of the first `<label>` child). Checked after `name_attr` but
    /// before `name_text`.
    #[serde(default)]
    pub name_child: Option<String>,
    /// If `true` and `name_attr` / `name_child` are absent / unset, use the element's text
    /// content as the name.
    #[serde(default)]
    pub name_text: bool,
}

/// An edge emitted whenever `child_element` appears as a direct child of `parent_element`.
#[derive(Debug, Clone, Deserialize)]
pub struct EdgeMapping {
    pub parent_element: String,
    pub child_element: String,
    /// `EdgeKind` as a snake_case string: `"contains"`, `"governs"`, `"evaluates"`, `"produces"`.
    pub edge_kind: String,
}

// ── Kind parsing helpers ──────────────────────────────────────────────────────

fn parse_node_kind(s: &str) -> NodeKind {
    match s {
        "rule" => NodeKind::Rule,
        "rule_set" => NodeKind::RuleSet,
        "condition" => NodeKind::Condition,
        "action" => NodeKind::Action,
        "fact" => NodeKind::Fact,
        "class" => NodeKind::Class,
        "function" => NodeKind::Function,
        "module" => NodeKind::Module,
        other => NodeKind::Other(other.to_string()),
    }
}

fn parse_edge_kind(s: &str) -> EdgeKind {
    match s {
        "contains" => EdgeKind::Contains,
        "governs" => EdgeKind::Governs,
        "evaluates" => EdgeKind::Evaluates,
        "produces" => EdgeKind::Produces,
        "invoked_by" => EdgeKind::InvokedBy,
        "calls" => EdgeKind::Calls,
        "imports" => EdgeKind::Imports,
        other => EdgeKind::Other(other.to_string()),
    }
}

// ── Name resolution helper ────────────────────────────────────────────────────

/// Resolve a node's display name from a mapping + element node.
fn resolve_name(mapping: &NodeMapping, node: roxmltree::Node<'_, '_>, fallback: &str) -> String {
    if let Some(attr) = &mapping.name_attr {
        node.attribute(attr.as_str()).unwrap_or("").to_string()
    } else if let Some(child_elem) = &mapping.name_child {
        node.children()
            .find(|c| c.is_element() && c.tag_name().name() == child_elem.as_str())
            .and_then(|c| c.text())
            .unwrap_or("")
            .trim()
            .to_string()
    } else if mapping.name_text {
        node.text().unwrap_or("").trim().to_string()
    } else {
        fallback.to_string()
    }
}

// ── Extractor ─────────────────────────────────────────────────────────────────

/// A data-driven XML extractor for rules documents (DMN, Drools, BPMN, Salesforce Flow, …).
///
/// Build one from a deserialized [`XmlRulesConfig`]:
/// ```ignore
/// let cfg: XmlRulesConfig = toml::from_str(toml_source)?;
/// let extractor = XmlRulesExtractor::new(cfg);
/// ```
pub struct XmlRulesExtractor {
    config: XmlRulesConfig,
}

impl XmlRulesExtractor {
    pub fn new(config: XmlRulesConfig) -> Self {
        Self { config }
    }

    /// Parse a TOML string into an `XmlRulesExtractor` directly.
    pub fn from_toml(toml_source: &str) -> std::result::Result<Self, toml::de::Error> {
        let config: XmlRulesConfig = toml::from_str(toml_source)?;
        Ok(Self::new(config))
    }

    /// The engine name reported as the language.
    fn language(&self) -> Language {
        Language::new(format!("xml-rules:{}", self.config.engine.name))
    }
}

impl Extractor for XmlRulesExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![self.language()]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let doc =
            roxmltree::Document::parse(&file.text).map_err(|e| Error::Extraction(e.to_string()))?;

        let lang = self.language();
        let mut nodes: Vec<Node> = Vec::new();
        let mut local_edges: Vec<Edge> = Vec::new();

        // Index node_mappings by element name for O(1) lookup.
        let node_map: std::collections::HashMap<&str, &NodeMapping> = self
            .config
            .node_mappings
            .iter()
            .map(|m| (m.element.as_str(), m))
            .collect();

        // Walk all elements in document order, emitting nodes for mapped elements.
        for node in doc.descendants() {
            if !node.is_element() {
                continue;
            }
            let elem_name = node.tag_name().name();
            let Some(mapping) = node_map.get(elem_name) else {
                continue;
            };

            // Resolve the node's display name.
            let name = resolve_name(mapping, node, elem_name);

            if name.is_empty() {
                continue;
            }

            // Build an element path (ancestor chain, names only) for a stable id.
            let mut path_parts: Vec<String> = node
                .ancestors()
                .filter(|a| a.is_element())
                .map(|a| a.tag_name().name().to_string())
                .collect::<Vec<_>>();
            path_parts.reverse();
            path_parts.push(elem_name.to_string());
            let element_path = path_parts.join("/");

            let id_str = format!("{}::{}::{}", file.path, element_path, name);
            let symbol = Symbol::synthetic("xml-rules", &id_str).id();

            let location = Location::new(&file.path, Span::ZERO);
            let kind = parse_node_kind(&mapping.emit_kind);

            let graph_node = Node::new(symbol, kind, &name, lang.clone(), location);
            nodes.push(graph_node);
        }

        // Emit edges for parent→child element pairs defined in edge_mappings.
        // We do a second pass over the document to find containment relationships.
        for edge_mapping in &self.config.edge_mappings {
            for parent_node in doc.descendants() {
                if !parent_node.is_element() {
                    continue;
                }
                if parent_node.tag_name().name() != edge_mapping.parent_element {
                    continue;
                }

                // Resolve the parent symbol id.
                let parent_name =
                    if let Some(nm) = node_map.get(edge_mapping.parent_element.as_str()) {
                        let n = resolve_name(nm, parent_node, &edge_mapping.parent_element);
                        if n.is_empty() {
                            continue;
                        }
                        n
                    } else {
                        continue;
                    };

                // Build the parent's element path.
                let mut pp: Vec<String> = parent_node
                    .ancestors()
                    .filter(|a| a.is_element())
                    .map(|a| a.tag_name().name().to_string())
                    .collect::<Vec<_>>();
                pp.reverse();
                pp.push(edge_mapping.parent_element.clone());
                let parent_path = pp.join("/");
                let parent_id_str = format!("{}::{}::{}", file.path, parent_path, parent_name);
                let parent_sym = Symbol::synthetic("xml-rules", &parent_id_str).id();

                // Walk direct children matching the child element.
                for child in parent_node.children() {
                    if !child.is_element() {
                        continue;
                    }
                    if child.tag_name().name() != edge_mapping.child_element {
                        continue;
                    }

                    let child_nm = match node_map.get(edge_mapping.child_element.as_str()) {
                        Some(nm) => nm,
                        None => continue,
                    };

                    let child_name = resolve_name(child_nm, child, &edge_mapping.child_element);
                    if child_name.is_empty() {
                        continue;
                    }

                    // Build the child's element path.
                    let mut cp: Vec<String> = child
                        .ancestors()
                        .filter(|a| a.is_element())
                        .map(|a| a.tag_name().name().to_string())
                        .collect::<Vec<_>>();
                    cp.reverse();
                    cp.push(edge_mapping.child_element.clone());
                    let child_path = cp.join("/");
                    let child_id_str = format!("{}::{}::{}", file.path, child_path, child_name);
                    let child_sym = Symbol::synthetic("xml-rules", &child_id_str).id();

                    // Edge: source = parent (dependent / governer), target = child (dependency).
                    // "A governs B" → source=A (the rule set), target=B (the rule).
                    // This matches the edge-direction invariant: source is the dependent.
                    let edge = Edge::new(
                        parent_sym.clone(),
                        child_sym,
                        parse_edge_kind(&edge_mapping.edge_kind),
                        ResolutionTier::Parsed,
                        "xml-rules",
                    );
                    local_edges.push(edge);
                }
            }
        }

        Ok(Extraction {
            nodes,
            local_edges,
            refs: vec![],
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const DMN_CONFIG: &str = r#"
[engine]
name       = "dmn"
file_globs = ["**/*.dmn"]

[[node_mappings]]
element   = "definitions"
emit_kind = "rule_set"
name_attr = "name"

[[node_mappings]]
element   = "decision"
emit_kind = "rule"
name_attr = "name"

[[edge_mappings]]
parent_element = "definitions"
child_element  = "decision"
edge_kind      = "governs"
"#;

    const DMN_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions name="LoanDecision">
  <decision name="EligibilityCheck">
    <input name="creditScore"/>
    <output name="eligible"/>
  </decision>
</definitions>"#;

    #[test]
    fn extracts_rule_set_and_rule_from_dmn() {
        let extractor = XmlRulesExtractor::from_toml(DMN_CONFIG).expect("config must parse");

        let file = SourceFile {
            path: "rules/loan.dmn".to_string(),
            language: Language::new("xml-rules:dmn"),
            text: DMN_XML.to_string(),
        };

        let extraction = extractor.extract(&file).expect("extraction must succeed");

        // Exactly 2 nodes: 1 RuleSet + 1 Rule.
        assert_eq!(
            extraction.nodes.len(),
            2,
            "expected 2 nodes, got {}: {:?}",
            extraction.nodes.len(),
            extraction
                .nodes
                .iter()
                .map(|n| (&n.name, &n.kind))
                .collect::<Vec<_>>()
        );

        let rule_sets: Vec<_> = extraction
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::RuleSet)
            .collect();
        assert_eq!(rule_sets.len(), 1, "expected 1 RuleSet");
        assert_eq!(
            rule_sets[0].name, "LoanDecision",
            "RuleSet name must be 'LoanDecision'"
        );

        let rules: Vec<_> = extraction
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Rule)
            .collect();
        assert_eq!(rules.len(), 1, "expected 1 Rule");
        assert_eq!(
            rules[0].name, "EligibilityCheck",
            "Rule name must be 'EligibilityCheck'"
        );
    }

    #[test]
    fn emits_governs_edge_between_definitions_and_decision() {
        let extractor = XmlRulesExtractor::from_toml(DMN_CONFIG).expect("config must parse");

        let file = SourceFile {
            path: "rules/loan.dmn".to_string(),
            language: Language::new("xml-rules:dmn"),
            text: DMN_XML.to_string(),
        };

        let extraction = extractor.extract(&file).expect("extraction must succeed");

        assert_eq!(
            extraction.local_edges.len(),
            1,
            "expected 1 Governs edge, got {}",
            extraction.local_edges.len()
        );
        assert_eq!(
            extraction.local_edges[0].kind,
            EdgeKind::Governs,
            "edge kind must be Governs"
        );
    }

    #[test]
    fn node_ids_are_stable_and_synthetic() {
        let extractor = XmlRulesExtractor::from_toml(DMN_CONFIG).expect("config must parse");

        let file = SourceFile {
            path: "rules/loan.dmn".to_string(),
            language: Language::new("xml-rules:dmn"),
            text: DMN_XML.to_string(),
        };

        let extraction = extractor.extract(&file).expect("extraction must succeed");

        for node in &extraction.nodes {
            assert!(
                node.symbol.as_str().contains("xml-rules synthetic"),
                "symbol must be synthetic, got: {}",
                node.symbol
            );
            assert!(
                node.symbol.as_str().contains("rules/loan.dmn"),
                "symbol must encode the file path"
            );
        }
    }

    #[test]
    fn languages_returns_engine_scoped_name() {
        let extractor = XmlRulesExtractor::from_toml(DMN_CONFIG).expect("config must parse");
        let langs = extractor.languages();
        assert_eq!(langs.len(), 1);
        assert_eq!(langs[0].as_str(), "xml-rules:dmn");
    }

    #[test]
    fn empty_xml_produces_empty_extraction() {
        let extractor = XmlRulesExtractor::from_toml(DMN_CONFIG).expect("config must parse");

        let file = SourceFile {
            path: "rules/empty.dmn".to_string(),
            language: Language::new("xml-rules:dmn"),
            text: "<definitions name=\"\"></definitions>".to_string(),
        };

        let extraction = extractor.extract(&file).expect("extraction must succeed");
        // name="" → skipped; no nodes emitted.
        assert!(extraction.nodes.is_empty(), "empty name should be skipped");
        assert!(extraction.local_edges.is_empty());
    }

    #[test]
    fn name_child_resolves_from_child_element_text() {
        // Verify the name_child extension: name comes from a child element's text, not an attr.
        const CFG: &str = r#"
[engine]
name       = "test-child"
file_globs = ["**/*.xml"]

[[node_mappings]]
element    = "parent"
emit_kind  = "rule_set"
name_child = "label"
"#;

        const XML: &str = r#"<?xml version="1.0"?>
<root>
  <parent>
    <label>My Rule Set</label>
  </parent>
</root>"#;

        let extractor = XmlRulesExtractor::from_toml(CFG).expect("config must parse");
        let file = SourceFile {
            path: "test.xml".to_string(),
            language: Language::new("xml-rules:test-child"),
            text: XML.to_string(),
        };
        let extraction = extractor.extract(&file).expect("extraction must succeed");
        assert_eq!(extraction.nodes.len(), 1);
        assert_eq!(extraction.nodes[0].name, "My Rule Set");
        assert_eq!(extraction.nodes[0].kind, NodeKind::RuleSet);
    }
}
