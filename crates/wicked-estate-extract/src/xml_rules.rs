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
//! Extractors built `with_id_attr`/`with_id_child` key elements on the format's own identifier
//! (a DMN `id`, a Salesforce Flow API `<name>`): `Symbol::synthetic("xml-rules",
//! "<file>::<tag>#<id>")`. It survives renames, moves and inserted same-named siblings
//! (ADR-002). An identifier used twice in one file identifies neither element.
//!
//! An element without one is keyed under its nearest keyed ancestor:
//! `"<anchor key>/<unmapped tags>/<tag>:<name>"`. Two such elements with one key (two Flow
//! conditions on one field under one rule) each get `~<digest of the element's text>`, never a
//! position, so an inserted sibling moves no id; byte-identical elements are one node.
//!
//! With no keyed ancestor at all (a format without ids, such as GDST), the id is the 0.37 form,
//! `"<file>::<ancestor tags>/<tag>::<name>"`, where same-named elements are one node (#675).

#![cfg(feature = "xml-rules")]

use serde::Deserialize;
use wicked_estate_core::{
    Edge, EdgeKind, Error, Extraction, Extractor, Language, Location, Node, NodeKind,
    ResolutionTier, Result, SourceFile, Span, Symbol, SymbolId,
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

/// Where an element carries the format's own stable identifier (#675).
#[derive(Debug, Clone)]
enum SourceId {
    /// An attribute (DMN `id`).
    Attr(String),
    /// A child element's text (Salesforce Flow `<name>`).
    Child(String),
}

impl SourceId {
    /// The identifier of `node`, trimmed, if it has a non-empty one.
    fn read(&self, node: roxmltree::Node<'_, '_>) -> Option<String> {
        let raw = match self {
            SourceId::Attr(attr) => node.attribute(attr.as_str()),
            SourceId::Child(child) => node
                .children()
                .find(|c| c.is_element() && c.tag_name().name() == child.as_str())
                .and_then(|c| c.text()),
        };
        raw.map(str::trim)
            .filter(|i| !i.is_empty())
            .map(str::to_string)
    }
}

/// FNV-1a over the element's parsed content: a stable digest (no new dependency, and the same on
/// every platform and toolchain, unlike `DefaultHasher`). It hashes the element's structure (tag
/// names, attributes and text, each with a marker byte), so:
/// - the text of a leaf element (one with no child elements) is a value and always counts, even
///   when it is only whitespace, so `A B` and `A  B`, or `<x></x>` and `<x> </x>`, stay apart
///   (#679, #684);
/// - whitespace-only text beside child elements is indentation and is skipped, so re-indenting a
///   file keeps every id (ADR-002: a reformat leaves identity unchanged);
/// - attributes are hashed in sorted order, because XML gives their order no meaning;
/// - element and attribute names are hashed with their namespace URI, so `camunda:x` and `other:x`
///   differ;
/// - line ends arrive normalized by the parser (XML 1.0 §2.11), so a CRLF checkout keeps every id.
///
/// The walk uses an explicit stack, so a deeply nested element cannot overflow the call stack.
fn content_digest(el: roxmltree::Node<'_, '_>) -> u64 {
    fn feed(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= u64::from(*b);
            *h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    // (node, close, beside_elements): `close` ends an element opened earlier, and
    // `beside_elements` says the node's parent also has child elements.
    let mut stack = vec![(el, false, false)];
    while let Some((n, close, beside_elements)) = stack.pop() {
        if close {
            feed(&mut h, &[5]);
        } else if n.is_element() {
            // A name is its namespace URI plus its local name: `camunda:x` and `other:x` differ,
            // while rebinding a prefix to the same URI changes nothing.
            feed(&mut h, &[1]);
            feed(&mut h, n.tag_name().namespace().unwrap_or("").as_bytes());
            feed(&mut h, &[6]);
            feed(&mut h, n.tag_name().name().as_bytes());
            let mut attrs: Vec<_> = n
                .attributes()
                .map(|a| (a.namespace(), a.name(), a.value()))
                .collect();
            attrs.sort_unstable();
            for (namespace, name, value) in attrs {
                feed(&mut h, &[2]);
                feed(&mut h, namespace.unwrap_or("").as_bytes());
                feed(&mut h, &[6]);
                feed(&mut h, name.as_bytes());
                feed(&mut h, &[3]);
                feed(&mut h, value.as_bytes());
            }
            stack.push((n, true, false));
            let has_elements = n.children().any(|c| c.is_element());
            let mut child = n.last_child();
            while let Some(c) = child {
                stack.push((c, false, has_elements));
                child = c.prev_sibling();
            }
        } else if n.is_text() {
            let text = n.text().unwrap_or("");
            if !(beside_elements && text.trim().is_empty()) {
                feed(&mut h, &[4]);
                feed(&mut h, text.as_bytes());
            }
        }
    }
    h
}

/// One mapped element's provisional key in `element_ids`.
struct Keyed<'a, 'input> {
    node: roxmltree::Node<'a, 'input>,
    name: String,
    key: String,
    /// Has a source id, or sits under an element that has one.
    anchored: bool,
    /// The key is the element's own source id.
    own: bool,
}

/// The display name and id of every mapped element with a name (see "Node identity" above).
/// The node and edge passes both read it, so they agree. Keys depend only on ancestors, so
/// they are computed one depth at a time, outermost first.
fn element_ids(
    doc: &roxmltree::Document<'_>,
    file: &str,
    node_map: &std::collections::HashMap<&str, &NodeMapping>,
    source_ids: &std::collections::HashMap<String, SourceId>,
) -> std::collections::HashMap<roxmltree::NodeId, (String, SymbolId)> {
    use std::collections::{BTreeMap, HashMap};
    let mapped: Vec<roxmltree::Node<'_, '_>> = doc
        .descendants()
        .filter(|n| n.is_element() && node_map.contains_key(n.tag_name().name()))
        .collect();
    let own_key = |n: roxmltree::Node<'_, '_>| {
        let tag = n.tag_name().name();
        source_ids
            .get(tag)
            .and_then(|s| s.read(n))
            .map(|id| format!("{tag}#{id}"))
    };
    // A source id used twice in the file identifies neither element: both take the fallback, so
    // inserting a duplicate never hands an existing element's id to the newcomer.
    let mut uses_of_own: HashMap<String, usize> = HashMap::new();
    for n in &mapped {
        if let Some(k) = own_key(*n) {
            *uses_of_own.entry(k).or_default() += 1;
        }
    }
    let mut by_depth: BTreeMap<usize, Vec<roxmltree::Node<'_, '_>>> = BTreeMap::new();
    for n in mapped {
        by_depth.entry(n.ancestors().count()).or_default().push(n);
    }
    // Final keys of anchored elements (those with a source id, or under one).
    let mut anchors: HashMap<roxmltree::NodeId, String> = HashMap::new();
    let mut ids = HashMap::new();
    for level in by_depth.into_values() {
        let mut keyed: Vec<Keyed<'_, '_>> = Vec::new();
        for n in level {
            let tag = n.tag_name().name();
            let name = resolve_name(node_map[tag], n, tag);
            if let Some(own) = own_key(n).filter(|k| uses_of_own[k] == 1) {
                keyed.push(Keyed {
                    node: n,
                    name,
                    key: own,
                    anchored: true,
                    own: true,
                });
                continue;
            }
            // The nearest anchored ancestor, and the unmapped elements in between.
            let mut between: Vec<&str> = Vec::new();
            let mut anchor = None;
            for a in n.ancestors().skip(1).filter(|a| a.is_element()) {
                if let Some(k) = anchors.get(&a.id()) {
                    anchor = Some(k.clone());
                    break;
                }
                between.push(a.tag_name().name());
            }
            match anchor {
                Some(anchor) => {
                    between.reverse();
                    let mut parts = vec![anchor];
                    parts.extend(between.iter().map(|t| t.to_string()));
                    parts.push(format!("{tag}:{name}"));
                    keyed.push(Keyed {
                        node: n,
                        name,
                        key: parts.join("/"),
                        anchored: true,
                        own: false,
                    });
                }
                None => {
                    // No ids anywhere above (GDST): the 0.37 form.
                    let mut tags: Vec<&str> = n
                        .ancestors()
                        .filter(|a| a.is_element())
                        .map(|a| a.tag_name().name())
                        .collect();
                    tags.reverse();
                    tags.push(tag);
                    let key = format!("{}::{name}", tags.join("/"));
                    keyed.push(Keyed {
                        node: n,
                        name,
                        key,
                        anchored: false,
                        own: false,
                    });
                }
            }
        }
        // Two anchored elements with one key (two conditions on one field under one rule) are
        // told apart by their content, never by position. Identical content is one node.
        let mut uses: HashMap<String, usize> = HashMap::new();
        for k in keyed.iter().filter(|k| k.anchored && !k.own) {
            *uses.entry(k.key.clone()).or_default() += 1;
        }
        for Keyed {
            node: n,
            name,
            key,
            anchored,
            own,
        } in keyed
        {
            let key = if anchored && !own && uses[&key] > 1 {
                format!("{key}~{:016x}", content_digest(n))
            } else {
                key
            };
            if anchored {
                anchors.insert(n.id(), key.clone());
            }
            if !name.is_empty() {
                let symbol = Symbol::synthetic("xml-rules", format!("{file}::{key}")).id();
                ids.insert(n.id(), (name, symbol));
            }
        }
    }
    ids
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
    /// `edge_mappings[i].edge_kind`, parsed ONCE at construction by the one core parser (#361).
    /// `new` is infallible (public API), so a bad kind is kept as its error — naming the mapping —
    /// and surfaced by `extract` the moment that mapping would emit, instead of minting a tag no
    /// built-in query matches.
    edge_kinds: Vec<std::result::Result<EdgeKind, String>>,
    /// Elements whose node id is the format's own identifier (`with_id_attr`/`with_id_child`).
    source_ids: std::collections::HashMap<String, SourceId>,
}

impl XmlRulesExtractor {
    pub fn new(config: XmlRulesConfig) -> Self {
        let edge_kinds = config
            .edge_mappings
            .iter()
            .map(|m| {
                EdgeKind::parse_user(&m.edge_kind).map_err(|e| {
                    format!(
                        "xml-rules {}: edge_mapping {} -> {}: edge_kind: {e}",
                        config.engine.name, m.parent_element, m.child_element
                    )
                })
            })
            .collect();
        Self {
            config,
            edge_kinds,
            source_ids: std::collections::HashMap::new(),
        }
    }

    /// Key `element`'s node id on its `attr` attribute, the format's own stable identifier (DMN
    /// `id`). See "Node identity" above.
    pub fn with_id_attr(mut self, element: &str, attr: &str) -> Self {
        self.source_ids
            .insert(element.to_string(), SourceId::Attr(attr.to_string()));
        self
    }

    /// Key `element`'s node id on the text of its `child` element, the format's own stable
    /// identifier (Salesforce Flow `<name>`). See "Node identity" above.
    pub fn with_id_child(mut self, element: &str, child: &str) -> Self {
        self.source_ids
            .insert(element.to_string(), SourceId::Child(child.to_string()));
        self
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

        let ids = element_ids(&doc, &file.path, &node_map, &self.source_ids);

        // Walk all elements in document order, emitting nodes for mapped, named elements. Elements
        // that share an id (identical content) are one node, emitted once.
        let mut emitted = std::collections::HashSet::new();
        for node in doc.descendants() {
            let Some((name, symbol)) = ids.get(&node.id()) else {
                continue;
            };
            if !emitted.insert(symbol.clone()) {
                continue;
            }
            let Some(mapping) = node_map.get(node.tag_name().name()) else {
                continue;
            };
            let location = Location::new(&file.path, Span::ZERO);
            let kind = parse_node_kind(&mapping.emit_kind);
            nodes.push(Node::new(
                symbol.clone(),
                kind,
                name,
                lang.clone(),
                location,
            ));
        }

        // Emit edges for parent→child element pairs defined in edge_mappings, each once.
        let mut linked = std::collections::HashSet::new();
        for (i, (edge_mapping, edge_kind)) in self
            .config
            .edge_mappings
            .iter()
            .zip(&self.edge_kinds)
            .enumerate()
        {
            for parent_node in doc.descendants() {
                if !parent_node.is_element()
                    || parent_node.tag_name().name() != edge_mapping.parent_element
                {
                    continue;
                }
                let Some((_, parent_sym)) = ids.get(&parent_node.id()) else {
                    continue;
                };
                for child in parent_node.children() {
                    if !child.is_element() || child.tag_name().name() != edge_mapping.child_element
                    {
                        continue;
                    }
                    let Some((_, child_sym)) = ids.get(&child.id()) else {
                        continue;
                    };
                    if !linked.insert((i, parent_sym.clone(), child_sym.clone())) {
                        continue;
                    }
                    // Edge: source = parent (dependent / governer), target = child (dependency).
                    // "A governs B" → source=A (the rule set), target=B (the rule).
                    // This matches the edge-direction invariant: source is the dependent.
                    local_edges.push(Edge::new(
                        parent_sym.clone(),
                        child_sym.clone(),
                        // Parsed at construction (#361); a refused kind fails here, loudly.
                        edge_kind.clone().map_err(Error::Extraction)?,
                        ResolutionTier::Parsed,
                        "xml-rules",
                    ));
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

    /// #361: `edge_kind` goes through the one core parser at construction. A near-spelling of a
    /// built-in (`Governs`) fails the extraction naming the mapping — before #361 it minted the
    /// tag `Governs`, which no built-in query matches.
    #[test]
    fn a_near_spelled_edge_kind_fails_naming_the_mapping() {
        let cfg = DMN_CONFIG.replace(
            "edge_kind      = \"governs\"",
            "edge_kind      = \"Governs\"",
        );
        assert_ne!(cfg, DMN_CONFIG, "fixture edit must apply");
        let extractor = XmlRulesExtractor::from_toml(&cfg).expect("config must parse");
        let file = SourceFile {
            path: "rules/loan.dmn".to_string(),
            language: Language::new("xml-rules:dmn"),
            text: DMN_XML.to_string(),
        };
        let err = extractor
            .extract(&file)
            .expect_err("a near-spelled kind must not mint a tag")
            .to_string();
        assert!(
            err.contains("dmn") && err.contains("did you mean \"governs\""),
            "{err}"
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
