//! Config-driven `ExtraEdgeExtractor` — drop-in DOMAIN edge/node injector.
//!
//! Lets a repo inject custom relationships (event-bus producer/consumer, command→agent dispatch,
//! capability links, framework hooks) into the code graph **without changing any core code**. Rules
//! are plain TOML; a new domain edge type is a new rule block, zero Rust.
//!
//! # Config schema
//!
//! Each rule block is a `[[rule]]` TOML array entry. `emit_node` and `emit_edge` are optional
//! subsections. In TOML basic strings, `\\` is one backslash and `\"` is a literal double-quote.
//!
//! ```toml
//! [[rule]]
//! name       = "event-bus-emit"          # human name → Provenance::Extractor(name)
//! file_glob  = "**/*.js"                 # glob filter; ** crosses path separators
//! pattern    = "emit\\([\"'](?P<topic>[\\w.]+)"  # regex — named captures become template vars
//!
//! [rule.emit_node]                        # optional — inject a synthetic node per match
//! id_template   = "topic:{topic}"        # {capture_name} expanded per match
//! label_capture = "topic"                # which capture is the human-readable label
//! kind          = "synthetic"            # "synthetic" | "other:<tag>"
//! node_scheme   = "event-bus-topic"      # Symbol::Synthetic scheme — SET THE SAME VALUE in
//!                                        # related rules so they share the same SymbolId
//!
//! [rule.emit_edge]                        # optional — inject an edge from file → synthetic node
//! kind               = "other:emits"     # "other:<tag>" → EdgeKind::Other("emits")
//! target_id_template = "topic:{topic}"   # must expand to the same id as emit_node.id_template
//! target_node_scheme = "event-bus-topic" # must match emit_node.node_scheme
//! # target_kind        = "file"          # optional — target the LITERAL file node whose
//! #                                      # repo-relative path is the expanded target_id_template
//! # source_id_template  = "agent:{name}" # optional — start the edge at a synthetic node
//! # source_node_scheme  = "agent"        # instead of the matched file's node
//!
//! [[rule]]
//! name      = "event-bus-consume"
//! file_glob = "**/*.js"
//! pattern   = "subscribe\\([\"'](?P<topic>[\\w.]+)"
//!
//! [rule.emit_node]
//! id_template   = "topic:{topic}"
//! label_capture = "topic"
//! kind          = "synthetic"
//! node_scheme   = "event-bus-topic"      # same scheme → same SymbolId as the emit rule
//!
//! [rule.emit_edge]
//! kind               = "other:consumes"
//! target_id_template = "topic:{topic}"
//! target_node_scheme = "event-bus-topic"
//! ```
//!
//! The above two rules make event-bus producer/consumer relationships traversable: both `emit` and
//! `subscribe` calls land on the **same** synthetic `topic:<name>` node (because both rules use
//! `node_scheme = "event-bus-topic"`), so a blast-radius query on a topic crosses the event-bus
//! boundary and reaches both producers and consumers.
//!
//! # Stable synthetic ids
//!
//! Synthetic node ids are:
//! ```text
//! Symbol::Synthetic { scheme: node_scheme (or rule.name), id: expanded_id_template }
//! ```
//! Because the id comes from the *captured value* (e.g. the topic name) and not from file path or
//! line number, **two files that emit/consume the same topic share the exact same `SymbolId`** —
//! which is exactly what lets the graph connect them. The `node_scheme` field is the mechanism:
//! set it to the same value in every rule that should contribute to the same logical node pool.

use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use wicked_estate_core::{
    Edge, EdgeKind, Language, Location, Node, NodeKind, Provenance, ResolutionTier, SourceFile,
    Span, Symbol, UnresolvedRef,
};

// ── TOML config schema ────────────────────────────────────────────────────────

/// How to construct a synthetic node id / label from a regex match.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeTemplate {
    /// Template string: literal text plus `{capture_name}` placeholders.
    /// Example: `"topic:{topic}"` → `"topic:orders.created"`.
    pub id_template: String,
    /// Which capture group provides the human-readable label for the node.
    pub label_capture: String,
    /// Node kind: `"synthetic"` or `"other:<tag>"` (e.g. `"other:event_topic"`).
    #[serde(default = "default_node_kind")]
    pub kind: String,
    /// Scheme for `Symbol::Synthetic { scheme, id }`.
    ///
    /// **Set the same value in every rule that should share a node** — the synthetic `SymbolId`
    /// is `Symbol::Synthetic { scheme: node_scheme, id: expanded_id_template }`. Defaults to
    /// the rule's `name` if absent. For event-bus rules: set `node_scheme = "event-bus-topic"`
    /// in both the emit and consume rules so they converge on the same topic node.
    #[serde(default)]
    pub node_scheme: Option<String>,
}

fn default_node_kind() -> String {
    "synthetic".to_string()
}

/// How to construct an edge per match. By default the edge runs from the **matched file's node**
/// to a **synthetic** target; both ends can be overridden:
///
/// - `source_id_template` + `source_node_scheme` — start the edge at a synthetic node instead of
///   the file node (e.g. an `archetype:{name}` node emitted by a sibling rule).
/// - `target_kind = "file"` — land the edge on the **literal file node** whose repo-relative path
///   is the expanded `target_id_template`. If that file is not in the graph, the edge dangles and
///   the indexer's dangling-edge prune removes it — a declared-but-missing target never fabricates
///   a relationship (the file-existence guard).
#[derive(Debug, Clone, Deserialize)]
pub struct EdgeTemplate {
    /// Edge kind: `"other:<tag>"` (e.g. `"other:emits"`, `"other:consumes"`).
    pub kind: String,
    /// `id_template` for the target node (must match `NodeTemplate::id_template` exactly when both
    /// are present so the same `SymbolId` is used as key). With `target_kind = "file"` this is the
    /// repo-relative path of the target file instead.
    pub target_id_template: String,
    /// Scheme for the target `Symbol::Synthetic`. Must match `NodeTemplate::node_scheme` (or the
    /// rule name if absent). Defaults to the rule's `name` when not set. Ignored when
    /// `target_kind = "file"`.
    #[serde(default)]
    pub target_node_scheme: Option<String>,
    /// Target kind: `"synthetic"` (default) or `"file"`. With `"file"`, the expanded
    /// `target_id_template` is a repo-relative path and the target is `Symbol::File { path }`.
    #[serde(default)]
    pub target_kind: Option<String>,
    /// When set, the edge's SOURCE is the synthetic node
    /// `Symbol::Synthetic { scheme: source_node_scheme (or rule name), id: expanded template }`
    /// instead of the matched file's node. The synthetic source must be emitted somewhere (this
    /// rule's or a sibling rule's `emit_node`) or the edge is pruned as dangling.
    #[serde(default)]
    pub source_id_template: Option<String>,
    /// Scheme for the synthetic source. Defaults to the rule's `name`. Only read when
    /// `source_id_template` is set.
    #[serde(default)]
    pub source_node_scheme: Option<String>,
}

/// One rule in the TOML config: a glob filter, a regex, and optional node/edge templates.
#[derive(Debug, Clone, Deserialize)]
pub struct EdgeRule {
    /// Rule name, used in `Provenance::Extractor(name)` and as the synthetic-id scheme.
    pub name: String,
    /// Glob pattern for which file paths this rule applies to.
    /// Simple glob: `*` matches within a segment, `**` crosses path separators.
    pub file_glob: String,
    /// Regex applied line-by-line (or over the whole file text) to extract captures.
    pub pattern: String,
    /// Optional synthetic node to inject per match.
    pub emit_node: Option<NodeTemplate>,
    /// Optional edge to inject per match (from the file node → the synthetic target).
    pub emit_edge: Option<EdgeTemplate>,
}

// ── Deserialization wrapper ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RulesFile {
    #[serde(rename = "rule", default)]
    rules: Vec<EdgeRule>,
}

// ── Compiled rule ─────────────────────────────────────────────────────────────

/// A compiled rule: the raw config plus a pre-compiled `Regex`.
struct CompiledRule {
    cfg: EdgeRule,
    re: Regex,
}

impl CompiledRule {
    fn try_from(cfg: EdgeRule) -> Result<Self, String> {
        let re = Regex::new(&cfg.pattern).map_err(|e| {
            format!(
                "rule {:?}: invalid pattern {:?}: {e}",
                cfg.name, cfg.pattern
            )
        })?;
        Ok(Self { cfg, re })
    }
}

// ── ExtraEdgeExtractor ────────────────────────────────────────────────────────

/// Config-driven extractor that injects DOMAIN edges/nodes (event-bus, dispatch, capabilities)
/// into the graph without any core code changes.
///
/// Build with [`ExtraEdgeExtractor::from_toml`]; run with [`ExtraEdgeExtractor::extract_extra`].
///
/// # Example
///
/// ```rust,ignore
/// let toml_src = r#"
/// [[rule]]
/// name      = "event-bus-emit"
/// file_glob = "**/*.js"
/// pattern   = 'emit\(["'"](?P<topic>[\w.]+)'
/// [rule.emit_node]
/// id_template   = "topic:{topic}"
/// label_capture = "topic"
/// kind          = "synthetic"
/// [rule.emit_edge]
/// kind               = "other:emits"
/// target_id_template = "topic:{topic}"
/// "#;
///
/// let extractor = ExtraEdgeExtractor::from_toml(toml_src).unwrap();
/// let file = SourceFile { path: "src/orders.js".into(), language: Language::new("javascript"), text: r#"bus.emit("orders.created", payload);"#.into() };
/// let extra = extractor.extract_extra(&file);
/// // extra.nodes: [synthetic topic:orders.created]
/// // extra.local_edges: [file → topic:orders.created  (Other("emits"))]
/// ```
pub struct ExtraEdgeExtractor {
    rules: Vec<CompiledRule>,
}

impl ExtraEdgeExtractor {
    /// Parse a TOML config string and compile all regex patterns.
    ///
    /// Returns `Err` if the TOML is invalid or any regex fails to compile.
    pub fn from_toml(src: &str) -> Result<Self, String> {
        Self::from_toml_named(&[("<inline>".to_string(), src.to_string())])
    }

    /// Parse SEVERAL TOML rule files (e.g. every `*.toml` under `.wicked-estate-extractors/`) into
    /// one extractor. `files` is `(label, contents)` — the label names the file in parse errors.
    /// Rules keep the given file order, then their in-file order.
    pub fn from_toml_named(files: &[(String, String)]) -> Result<Self, String> {
        let mut rules: Vec<EdgeRule> = Vec::new();
        for (label, src) in files {
            let file: RulesFile = toml::from_str(src)
                .map_err(|e| format!("extra-edge config parse error in {label}: {e}"))?;
            rules.extend(file.rules);
        }
        let rules = rules
            .into_iter()
            .map(CompiledRule::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { rules })
    }

    /// True when at least one rule's `file_glob` matches `path` — the cheap pre-filter callers use
    /// to decide whether a file needs the extra-edge pass at all.
    pub fn matches_path(&self, path: &str) -> bool {
        self.rules
            .iter()
            .any(|r| glob_matches(&r.cfg.file_glob, path))
    }

    /// Number of compiled rules (0 = nothing will ever match).
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Run all applicable rules over `file` and return the synthetic nodes + domain edges.
    ///
    /// The file node itself is **not** included in the returned nodes — it is the caller's
    /// responsibility to ensure it exists in the store. The returned nodes and edges can be
    /// merged into an `Extraction` or applied directly to a `GraphStore`.
    ///
    /// # Idempotency
    ///
    /// Synthetic node ids are derived purely from the rule name + the captured values, so running
    /// the same rule twice over the same file produces identical `SymbolId`s. Deduplication in the
    /// `GraphStore` (by `symbol` PK for nodes, by `dedup_key` for edges) makes the result
    /// idempotent.
    pub fn extract_extra(&self, file: &SourceFile) -> ExtraExtraction {
        let file_symbol = Symbol::file(&file.path).id();
        // Deduplicate synthetic nodes across rules within this file: id → Node.
        let mut node_map: HashMap<String, Node> = HashMap::new();
        let mut edges: Vec<Edge> = Vec::new();
        let mut unresolved_refs: Vec<UnresolvedRef> = Vec::new();

        for rule in &self.rules {
            if !glob_matches(&rule.cfg.file_glob, &file.path) {
                continue;
            }
            let provenance = Provenance::Extractor(rule.cfg.name.clone());
            for caps in rule.re.captures_iter(&file.text) {
                let bindings = capture_bindings(&rule.re, &caps);

                // ── Emit node ────────────────────────────────────────────────
                if let Some(ref nt) = rule.cfg.emit_node {
                    let node_id = expand_template(&nt.id_template, &bindings);
                    // Use explicit node_scheme when set; fall back to rule name.
                    // Setting the same node_scheme across related rules (emit + consume)
                    // ensures they converge on the same SymbolId for shared topics.
                    let scheme = nt.node_scheme.as_deref().unwrap_or(&rule.cfg.name);
                    let symbol_id = Symbol::synthetic(scheme, &node_id).id();
                    node_map.entry(node_id).or_insert_with(|| {
                        let label = bindings
                            .get(&nt.label_capture)
                            .cloned()
                            .unwrap_or_else(|| symbol_id.as_str().to_string());
                        let kind = parse_node_kind(&nt.kind);
                        let mut node = Node::new(
                            symbol_id.clone(),
                            kind,
                            label,
                            // Synthetic nodes have no language — use an empty tag.
                            Language::new("synthetic"),
                            Location::new(&file.path, Span::ZERO),
                        );
                        node.signature = Some(nt.id_template.clone());
                        node
                    });
                }

                // ── Emit edge ─────────────────────────────────────────────────
                if let Some(ref et) = rule.cfg.emit_edge {
                    let target_id = expand_template(&et.target_id_template, &bindings);
                    // Target: synthetic node (default) or — with target_kind = "file" — the
                    // literal file node at the expanded repo-relative path.
                    let target_symbol = if et.target_kind.as_deref() == Some("file") {
                        Symbol::file(&target_id).id()
                    } else {
                        // Use explicit target_node_scheme when set; fall back to rule name.
                        let target_scheme =
                            et.target_node_scheme.as_deref().unwrap_or(&rule.cfg.name);
                        Symbol::synthetic(target_scheme, &target_id).id()
                    };
                    // Source: the matched file's node (default) or a synthetic node when
                    // source_id_template is set (e.g. the archetype node a sibling rule emits).
                    let source_symbol = match &et.source_id_template {
                        Some(t) => {
                            let source_id = expand_template(t, &bindings);
                            let source_scheme =
                                et.source_node_scheme.as_deref().unwrap_or(&rule.cfg.name);
                            Symbol::synthetic(source_scheme, &source_id).id()
                        }
                        None => file_symbol.clone(),
                    };
                    let ek = parse_edge_kind(&et.kind);
                    let mut edge = Edge::new(
                        source_symbol,
                        target_symbol,
                        ek,
                        ResolutionTier::Heuristic,
                        &rule.cfg.name,
                    );
                    edge.provenance = provenance.clone();
                    // Location: byte 0 of the file (the whole-file match site).
                    edge.location = Some(Location::new(&file.path, Span::ZERO));
                    edges.push(edge);
                }

                // ── Emit bridge ref (rules-engine NodeKind) ─────────────────────────
                if let Some(ref nt) = rule.cfg.emit_node {
                    let is_rules_engine = matches!(
                        parse_node_kind(&nt.kind),
                        NodeKind::Rule
                            | NodeKind::RuleSet
                            | NodeKind::Condition
                            | NodeKind::Action
                            | NodeKind::Fact
                    );
                    if is_rules_engine {
                        let scheme = nt.node_scheme.as_deref().unwrap_or(&rule.cfg.name);
                        let uref = UnresolvedRef::new(
                            file_symbol.clone(),
                            format!("rules-engine:{scheme}"),
                            EdgeKind::InvokedBy,
                            Location::new(&file.path, Span::ZERO),
                        );
                        unresolved_refs.push(uref);
                    }
                }
            }
        }

        ExtraExtraction {
            nodes: node_map.into_values().collect(),
            edges,
            unresolved_refs,
        }
    }
}

/// The output of [`ExtraEdgeExtractor::extract_extra`]: synthetic nodes + domain edges.
#[derive(Debug, Default)]
pub struct ExtraExtraction {
    /// Synthetic / domain nodes emitted by the matching rules.
    pub nodes: Vec<Node>,
    /// Domain edges from the file node to the synthetic targets.
    pub edges: Vec<Edge>,
    /// UnresolvedRefs emitted by bridge rules (rules-engine kinds).
    /// These are passed to RulesBridgeResolver in the resolution phase.
    pub unresolved_refs: Vec<UnresolvedRef>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract named captures from a `regex::Captures` into a `HashMap<name, text>`.
fn capture_bindings(re: &Regex, caps: &regex::Captures<'_>) -> HashMap<String, String> {
    re.capture_names()
        .flatten()
        .filter_map(|name| {
            caps.name(name)
                .map(|m| (name.to_string(), m.as_str().to_string()))
        })
        .collect()
}

/// Expand `{capture_name}` placeholders in a template string.
fn expand_template(template: &str, bindings: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in bindings {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Parse a node-kind string from the config.
///
/// `"synthetic"` → `NodeKind::Synthetic`
/// `"other:<tag>"` → `NodeKind::Other("<tag>")`
/// W15 rules-engine kinds (`"rule"`, `"rule_set"`, `"condition"`, `"action"`, `"fact"`)
/// map to their first-class variants.
/// Anything else → `NodeKind::Synthetic` (safe default).
fn parse_node_kind(s: &str) -> NodeKind {
    if s == "synthetic" {
        return NodeKind::Synthetic;
    }
    if let Some(tag) = s.strip_prefix("other:") {
        return NodeKind::Other(tag.to_string());
    }
    match s {
        // W15 rules-engine NodeKinds — usable directly in cross-graph bridge configs.
        "rule" => NodeKind::Rule,
        "rule_set" => NodeKind::RuleSet,
        "condition" => NodeKind::Condition,
        "action" => NodeKind::Action,
        "fact" => NodeKind::Fact,
        _ => NodeKind::Synthetic,
    }
}

/// Parse an edge-kind string from the config.
///
/// `"other:<tag>"` → `EdgeKind::Other("<tag>")`
/// Built-in names (`"calls"`, `"imports"`, …) map to the corresponding variant.
/// W15 rules-engine names (`"governs"`, `"evaluates"`, `"produces"`, `"invoked_by"`)
/// map to their first-class variants so TOML rules bridge configs can use them directly.
fn parse_edge_kind(s: &str) -> EdgeKind {
    if let Some(tag) = s.strip_prefix("other:") {
        return EdgeKind::Other(tag.to_string());
    }
    match s {
        "calls" => EdgeKind::Calls,
        "imports" => EdgeKind::Imports,
        "contains" => EdgeKind::Contains,
        "references" => EdgeKind::References,
        "extends" => EdgeKind::Extends,
        "implements" => EdgeKind::Implements,
        // W15 rules-engine EdgeKinds — used in cross-graph bridge TOML configs.
        "governs" => EdgeKind::Governs,
        "evaluates" => EdgeKind::Evaluates,
        "produces" => EdgeKind::Produces,
        "invoked_by" => EdgeKind::InvokedBy,
        other => EdgeKind::Other(other.to_string()),
    }
}

/// Minimal glob matcher: supports `**` (crosses path separators) and `*` (within a segment).
/// Does **not** support `?`, character classes, or braces — callers needing those should pre-expand.
fn glob_matches(glob: &str, path: &str) -> bool {
    glob_match_inner(glob.as_bytes(), path.as_bytes())
}

fn glob_match_inner(pat: &[u8], text: &[u8]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some(b'*') => {
            if pat.get(1) == Some(&b'*') {
                // `**` — matches zero or more path components (any bytes including `/`).
                let rest = pat.get(2..).unwrap_or(b"");
                // Skip leading `/` after `**` if present.
                let rest = if rest.first() == Some(&b'/') {
                    &rest[1..]
                } else {
                    rest
                };
                // Try matching rest at every suffix of text.
                for i in 0..=text.len() {
                    if glob_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                false
            } else {
                // Single `*` — matches within a path segment (no `/`).
                let rest = &pat[1..];
                for i in 0..=text.len() {
                    // Single `*` does not cross `/`.
                    if i > 0 && text[i - 1] == b'/' {
                        break;
                    }
                    if glob_match_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                false
            }
        }
        Some(&b) => match text.first() {
            Some(&t) if t == b => glob_match_inner(&pat[1..], &text[1..]),
            _ => false,
        },
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── glob_matches ─────────────────────────────────────────────────────────

    #[test]
    fn glob_star_star_matches_nested_path() {
        assert!(glob_matches("**/*.js", "src/bus/orders.js"));
        assert!(glob_matches("**/*.ts", "deep/nested/path/file.ts"));
        assert!(!glob_matches("**/*.js", "src/foo.ts"));
    }

    #[test]
    fn glob_single_star_no_slash() {
        assert!(glob_matches("src/*.rs", "src/main.rs"));
        assert!(!glob_matches("src/*.rs", "src/sub/main.rs"));
    }

    #[test]
    fn glob_exact_match() {
        assert!(glob_matches("src/main.rs", "src/main.rs"));
        assert!(!glob_matches("src/main.rs", "src/lib.rs"));
    }

    // ── parse helpers ─────────────────────────────────────────────────────────

    #[test]
    fn parse_node_kind_synthetic() {
        assert_eq!(parse_node_kind("synthetic"), NodeKind::Synthetic);
    }

    #[test]
    fn parse_node_kind_other() {
        assert_eq!(
            parse_node_kind("other:event_topic"),
            NodeKind::Other("event_topic".to_string())
        );
    }

    #[test]
    fn parse_edge_kind_other() {
        assert_eq!(
            parse_edge_kind("other:emits"),
            EdgeKind::Other("emits".to_string())
        );
        assert_eq!(
            parse_edge_kind("other:consumes"),
            EdgeKind::Other("consumes".to_string())
        );
    }

    #[test]
    fn parse_edge_kind_builtin() {
        assert_eq!(parse_edge_kind("calls"), EdgeKind::Calls);
        assert_eq!(parse_edge_kind("imports"), EdgeKind::Imports);
    }

    // ── expand_template ───────────────────────────────────────────────────────

    #[test]
    fn expand_template_substitutes_captures() {
        let mut b = HashMap::new();
        b.insert("topic".to_string(), "orders.created".to_string());
        assert_eq!(expand_template("topic:{topic}", &b), "topic:orders.created");
        assert_eq!(expand_template("{topic}", &b), "orders.created");
    }

    #[test]
    fn expand_template_unknown_capture_left_as_is() {
        let b: HashMap<String, String> = HashMap::new();
        assert_eq!(expand_template("topic:{unknown}", &b), "topic:{unknown}");
    }

    // ── from_toml ─────────────────────────────────────────────────────────────

    #[test]
    fn from_toml_invalid_toml_returns_err() {
        assert!(ExtraEdgeExtractor::from_toml("not valid toml @@@").is_err());
    }

    #[test]
    fn from_toml_bad_regex_returns_err() {
        let src = r#"
[[rule]]
name      = "bad"
file_glob = "**/*.js"
pattern   = "(?P<unclosed"
"#;
        assert!(ExtraEdgeExtractor::from_toml(src).is_err());
    }

    #[test]
    fn from_toml_empty_rules_ok() {
        let ex = ExtraEdgeExtractor::from_toml("").unwrap();
        let sf = SourceFile {
            path: "x.js".into(),
            language: Language::new("javascript"),
            text: "anything".into(),
        };
        let out = ex.extract_extra(&sf);
        assert!(out.nodes.is_empty());
        assert!(out.edges.is_empty());
    }

    // ── extract_extra basic ───────────────────────────────────────────────────

    // TOML basic string: backslashes doubled, double-quotes escaped.
    // pattern value is: emit\(["'](?P<topic>[\w.]+)
    const EMIT_RULE: &str = "
[[rule]]
name      = \"event-bus-emit\"
file_glob = \"**/*.js\"
pattern   = \"emit\\\\([\\\"'](?P<topic>[\\\\w.]+)\"

[rule.emit_node]
id_template   = \"topic:{topic}\"
label_capture = \"topic\"
kind          = \"synthetic\"

[rule.emit_edge]
kind               = \"other:emits\"
target_id_template = \"topic:{topic}\"
";

    #[test]
    fn emit_rule_produces_node_and_edge() {
        let ex = ExtraEdgeExtractor::from_toml(EMIT_RULE).unwrap();
        let sf = SourceFile {
            path: "src/orders.js".into(),
            language: Language::new("javascript"),
            text: r#"bus.emit("orders.created", payload);"#.into(),
        };
        let out = ex.extract_extra(&sf);

        assert_eq!(out.nodes.len(), 1, "one synthetic topic node");
        let node = &out.nodes[0];
        assert_eq!(node.kind, NodeKind::Synthetic);
        assert_eq!(node.name, "orders.created");

        // The node id must be Symbol::synthetic("event-bus-emit", "topic:orders.created").
        let expected_id = Symbol::synthetic("event-bus-emit", "topic:orders.created").id();
        assert_eq!(node.symbol, expected_id);

        assert_eq!(out.edges.len(), 1, "one emits edge");
        let edge = &out.edges[0];
        assert_eq!(edge.kind, EdgeKind::Other("emits".to_string()));
        assert_eq!(edge.target, expected_id);
        // source must be the file node.
        assert_eq!(edge.source, Symbol::file("src/orders.js").id());
        // Provenance must be Extractor("event-bus-emit").
        assert_eq!(
            edge.provenance,
            Provenance::Extractor("event-bus-emit".to_string())
        );
    }

    #[test]
    fn file_glob_filters_non_matching_files() {
        let ex = ExtraEdgeExtractor::from_toml(EMIT_RULE).unwrap();
        let sf = SourceFile {
            path: "src/orders.py".into(),
            language: Language::new("python"),
            text: r#"bus.emit("orders.created", payload)"#.into(),
        };
        let out = ex.extract_extra(&sf);
        assert!(out.nodes.is_empty(), "glob **/*.js must reject .py files");
        assert!(out.edges.is_empty());
    }

    #[test]
    fn multiple_matches_produce_multiple_edges_deduped_nodes() {
        let ex = ExtraEdgeExtractor::from_toml(EMIT_RULE).unwrap();
        let sf = SourceFile {
            path: "src/app.js".into(),
            language: Language::new("javascript"),
            text: r#"
bus.emit("orders.created", a);
bus.emit("payments.processed", b);
bus.emit("orders.created", c); // duplicate topic
"#
            .into(),
        };
        let out = ex.extract_extra(&sf);
        // Two distinct topics → two synthetic nodes.
        assert_eq!(out.nodes.len(), 2, "two distinct topic nodes");
        // Three matches → three edges (same target node for the duplicate, but separate edges).
        assert_eq!(out.edges.len(), 3, "three emits edges");
    }

    // W15.13 — cross-graph rules bridge via first-class EdgeKind/NodeKind.
    const ODM_BRIDGE_RULE: &str = r#"
[[rule]]
name       = "ibm-odm-invoke"
file_glob  = "**/*.java"
pattern    = 'IlrContext\.execute\(\)|RulesRunner\.run\(\)|IlrSession\.execute\(\)'

[rule.emit_node]
id_template   = "odm:pricing-rules"
label_capture = ""
kind          = "rule_set"
node_scheme   = "ibm-odm"

[rule.emit_edge]
kind               = "invoked_by"
target_id_template = "odm:pricing-rules"
target_node_scheme = "ibm-odm"
"#;

    #[test]
    fn w15_rules_bridge_emits_invoked_by_edge_and_rule_set_node() {
        use wicked_estate_core::{EdgeKind, NodeKind};

        let ex = ExtraEdgeExtractor::from_toml(ODM_BRIDGE_RULE).unwrap();
        let sf = SourceFile {
            path: "src/PricingService.java".into(),
            language: Language::new("java"),
            text: "context.execute(pricingRules); ilrCtx.IlrContext.execute();".into(),
        };
        let out = ex.extract_extra(&sf);

        assert_eq!(out.nodes.len(), 1, "one RuleSet synthetic node");
        assert_eq!(
            out.nodes[0].kind,
            NodeKind::RuleSet,
            "node kind must be RuleSet"
        );

        assert_eq!(out.edges.len(), 1, "one InvokedBy edge");
        assert_eq!(
            out.edges[0].kind,
            EdgeKind::InvokedBy,
            "edge kind must be InvokedBy"
        );

        assert!(
            !out.unresolved_refs.is_empty(),
            "bridge rule must emit at least one UnresolvedRef"
        );
        assert!(
            out.unresolved_refs[0].raw_name.starts_with("rules-engine:"),
            "UnresolvedRef raw_name must start with 'rules-engine:'"
        );
    }

    // ── source/target overrides (archetype→playbook shape) ────────────────────

    // Two rules over one catalog file: rule 1 emits the synthetic archetype node (+ a contains
    // edge from the catalog file), rule 2 emits archetype-node → playbook FILE-node edges.
    const ARCHETYPE_RULES: &str = r#"
[[rule]]
name      = "archetype-declare"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_node]
id_template   = "archetype:{name}"
label_capture = "name"
kind          = "other:archetype"
node_scheme   = "archetype"

[rule.emit_edge]
kind               = "contains"
target_id_template = "archetype:{name}"
target_node_scheme = "archetype"

[[rule]]
name      = "archetype-playbook"
file_glob = ".claude-plugin/archetypes.json"
pattern   = '(?m)^ {4}"(?P<name>[a-z][a-z0-9_-]*)":\s*\{'

[rule.emit_edge]
kind               = "references"
source_id_template = "archetype:{name}"
source_node_scheme = "archetype"
target_kind        = "file"
target_id_template = "skills/archetype/refs/{name}.md"
"#;

    const CATALOG_JSON: &str = r#"{
  "archetypes": {
    "triage": {
      "phases": ["classify"]
    },
    "build": {
      "phases": ["plan", "implement"]
    }
  }
}"#;

    #[test]
    fn edge_source_override_uses_synthetic_node() {
        let ex = ExtraEdgeExtractor::from_toml(ARCHETYPE_RULES).unwrap();
        let sf = SourceFile {
            path: ".claude-plugin/archetypes.json".into(),
            language: Language::new("json"),
            text: CATALOG_JSON.into(),
        };
        let out = ex.extract_extra(&sf);

        // Rule 2's edges must start at the synthetic archetype node, not the file node.
        let playbook_edges: Vec<_> = out
            .edges
            .iter()
            .filter(|e| e.provenance == Provenance::Extractor("archetype-playbook".to_string()))
            .collect();
        assert_eq!(playbook_edges.len(), 2, "one playbook edge per archetype");
        let expected_src = Symbol::synthetic("archetype", "archetype:triage").id();
        assert!(
            playbook_edges.iter().any(|e| e.source == expected_src),
            "edge source must be the synthetic archetype node"
        );
    }

    #[test]
    fn edge_target_kind_file_targets_literal_file_node() {
        let ex = ExtraEdgeExtractor::from_toml(ARCHETYPE_RULES).unwrap();
        let sf = SourceFile {
            path: ".claude-plugin/archetypes.json".into(),
            language: Language::new("json"),
            text: CATALOG_JSON.into(),
        };
        let out = ex.extract_extra(&sf);

        let expected_tgt = Symbol::file("skills/archetype/refs/triage.md").id();
        assert!(
            out.edges.iter().any(|e| e.target == expected_tgt),
            "playbook edge must target the literal playbook FILE node, got {:?}",
            out.edges.iter().map(|e| &e.target).collect::<Vec<_>>()
        );
    }

    #[test]
    fn shared_scheme_rules_converge_and_catalog_contains_edge_lands() {
        let ex = ExtraEdgeExtractor::from_toml(ARCHETYPE_RULES).unwrap();
        let sf = SourceFile {
            path: ".claude-plugin/archetypes.json".into(),
            language: Language::new("json"),
            text: CATALOG_JSON.into(),
        };
        let out = ex.extract_extra(&sf);

        // Two archetype nodes (kind other:archetype), deduped across the two rules.
        assert_eq!(out.nodes.len(), 2, "one node per archetype key");
        assert!(
            out.nodes
                .iter()
                .all(|n| n.kind == NodeKind::Other("archetype".to_string()))
        );

        // Rule 1: catalog file → archetype node (Contains).
        let file_sym = Symbol::file(".claude-plugin/archetypes.json").id();
        let arch_sym = Symbol::synthetic("archetype", "archetype:build").id();
        assert!(
            out.edges.iter().any(|e| e.kind == EdgeKind::Contains
                && e.source == file_sym
                && e.target == arch_sym),
            "catalog file must contain the archetype node"
        );

        // Rule 2's source is exactly rule 1's emitted node id — the shared-scheme convergence.
        assert!(
            out.edges
                .iter()
                .any(|e| e.kind == EdgeKind::References && e.source == arch_sym),
            "playbook edge source must converge on rule 1's node"
        );
    }

    #[test]
    fn nested_keys_do_not_match_the_anchored_catalog_pattern() {
        let ex = ExtraEdgeExtractor::from_toml(ARCHETYPE_RULES).unwrap();
        let sf = SourceFile {
            path: ".claude-plugin/archetypes.json".into(),
            language: Language::new("json"),
            text: CATALOG_JSON.into(),
        };
        let out = ex.extract_extra(&sf);
        // "phases" is nested at 6 spaces — the 4-space anchor must not capture it.
        assert!(
            !out.nodes.iter().any(|n| n.name == "phases"),
            "nested keys must not become archetype nodes"
        );
    }

    // ── from_toml_named / matches_path ─────────────────────────────────────────

    #[test]
    fn from_toml_named_merges_rules_across_files() {
        let files = vec![
            ("a.toml".to_string(), EMIT_RULE.to_string()),
            ("b.toml".to_string(), ARCHETYPE_RULES.to_string()),
        ];
        let ex = ExtraEdgeExtractor::from_toml_named(&files).unwrap();
        assert_eq!(ex.rule_count(), 3, "1 rule from a.toml + 2 from b.toml");
        assert!(ex.matches_path("src/orders.js"));
        assert!(ex.matches_path(".claude-plugin/archetypes.json"));
        assert!(!ex.matches_path("src/orders.py"));
    }

    #[test]
    fn from_toml_named_parse_error_names_the_file() {
        let files = vec![
            ("good.toml".to_string(), EMIT_RULE.to_string()),
            ("bad.toml".to_string(), "not valid toml @@@".to_string()),
        ];
        let err = match ExtraEdgeExtractor::from_toml_named(&files) {
            Err(e) => e,
            Ok(_) => panic!("parse must fail on bad.toml"),
        };
        assert!(
            err.contains("bad.toml"),
            "error must name the offending file, got: {err}"
        );
    }

    #[test]
    fn parse_edge_kind_covers_all_w15_variants() {
        use wicked_estate_core::EdgeKind;
        assert_eq!(parse_edge_kind("governs"), EdgeKind::Governs);
        assert_eq!(parse_edge_kind("evaluates"), EdgeKind::Evaluates);
        assert_eq!(parse_edge_kind("produces"), EdgeKind::Produces);
        assert_eq!(parse_edge_kind("invoked_by"), EdgeKind::InvokedBy);
    }

    #[test]
    fn parse_node_kind_covers_all_w15_variants() {
        use wicked_estate_core::NodeKind;
        assert_eq!(parse_node_kind("rule"), NodeKind::Rule);
        assert_eq!(parse_node_kind("rule_set"), NodeKind::RuleSet);
        assert_eq!(parse_node_kind("condition"), NodeKind::Condition);
        assert_eq!(parse_node_kind("action"), NodeKind::Action);
        assert_eq!(parse_node_kind("fact"), NodeKind::Fact);
    }
}
