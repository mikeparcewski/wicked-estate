//! Query + result types for the read side of the [`crate::GraphStore`] and retrieval tools.

use crate::edge::{Direction, Edge, EdgeKind};
use crate::node::{Language, Node, NodeKind};
use crate::symbol::SymbolId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A symbol search. Free-text ranking (BM25) is the store's job; this is the request shape.
#[derive(Debug, Clone, Default)]
pub struct SymbolQuery {
    /// Free-text query (BM25 over name/signature/doc in stores that support it).
    pub text: Option<String>,
    /// Exact simple-name match.
    pub exact_name: Option<String>,
    /// Restrict to these node kinds (empty = any).
    pub kinds: Vec<NodeKind>,
    pub language: Option<Language>,
    pub limit: Option<usize>,
    /// Restrict results to a scope subtree by canonical path prefix (e.g. `"org:acme"`), matching
    /// that scope and its descendants. `None` = all scopes. The predicate is pushed into the store
    /// SQL **before** any `LIMIT`, so top-k ranking never leaks across scopes (multi-tenant isolation).
    pub scope_prefix: Option<String>,
}

/// A **bounded** traversal request. We deliberately support only bounded reverse-reachability /
/// k-hop (the actual agent workload; see the design notes).
/// `max_depth` and `max_nodes` are required guard rails — unbounded whole-graph walks are out.
#[derive(Debug, Clone)]
pub struct TraversalSpec {
    pub direction: Direction,
    /// Edge kinds to follow (empty = all).
    pub edge_kinds: Vec<EdgeKind>,
    pub max_depth: u32,
    pub max_nodes: usize,
    /// Ignore edges below this confidence.
    pub min_confidence: f32,
}

impl TraversalSpec {
    /// Blast-radius preset: walk `Dependents` along `Calls` edges up to `max_depth`.
    pub fn blast_radius(max_depth: u32) -> Self {
        Self {
            direction: Direction::Dependents,
            // ALL edge kinds, not just Calls. The contract invariant is source=dependent /
            // target=dependency for EVERY edge, so a complete blast radius follows every
            // dependency kind backwards — `uses` (JCL step → dataset), `protects` (RACF profile →
            // asset), `accesses`, imports, references, type/heritage edges. Calls-only silently
            // under-reported estate + non-call dependents (an asset read as "nothing depends on
            // me"), the exact failure mode the engine must never have.
            edge_kinds: vec![],
            max_depth,
            max_nodes: 5_000,
            min_confidence: 0.0,
        }
    }
}

/// A subgraph returned by a traversal, with per-node distance from the start.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Subgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Distance from the start node, keyed by `SymbolId` string.
    pub depths: BTreeMap<String, u32>,
    /// True if a cap (`max_depth` / `max_nodes`) truncated the result.
    pub truncated: bool,
}

impl Subgraph {
    /// The dependent list of a blast-radius traversal, with import-transit File nodes cut
    /// (contains-aware rule; lane relative-imports Decision G, PER-1).
    ///
    /// Once File→File `Imports` edges exist, an all-kinds `Dependents` walk from a symbol
    /// reaches every TRANSITIVE IMPORTER FILE of the symbol's file — nodes that are not
    /// dependents of the symbol in any useful sense. The traversal itself is untouched (the
    /// locked "follow every dependency edge kind" decision, `TraversalSpec::blast_radius`);
    /// this classifies the RESULT:
    ///
    /// - **Non-File start**: keep a File node iff this subgraph holds a `Contains` edge from it
    ///   to a reached non-File node. The start's own containing file and every caller's
    ///   containing file pass (their `Contains` edge to the start/caller is walked, so it is in
    ///   `edges`); a File reached only through File→File import edges has no such edge here and
    ///   is dropped. This is exact HEAD-parity for symbol starts: before File→File edges
    ///   existed, every File in a dependents subgraph was reached via `Contains` — see
    ///   docs/recon/relative-imports.md Decision G (FEAS-1).
    /// - **File start** (`start_kind = Some(NodeKind::File)`): keep everything — the importing
    ///   files ARE the blast radius of a file.
    ///
    /// The start node itself is never returned. Kept Files' min-depths may shift when an import
    /// edge offers a shorter path; callers must not depend on File-row depths.
    pub fn code_dependents(&self, start: &SymbolId, start_kind: Option<&NodeKind>) -> Vec<&Node> {
        if matches!(start_kind, Some(NodeKind::File)) {
            return self.nodes.iter().filter(|n| &n.symbol != start).collect();
        }
        // Non-File nodes reached by the walk (the start included — its containing file's
        // Contains edge targets the start itself).
        let non_file: std::collections::HashSet<&str> = self
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| n.symbol.as_str())
            .collect();
        // Files with a Contains edge (in THIS subgraph) to a reached non-File node.
        let contains_holding: std::collections::HashSet<&str> = self
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains && non_file.contains(e.target.as_str()))
            .map(|e| e.source.as_str())
            .collect();
        self.nodes
            .iter()
            .filter(|n| {
                if &n.symbol == start {
                    return false;
                }
                match n.kind {
                    NodeKind::File => contains_holding.contains(n.symbol.as_str()),
                    _ => true,
                }
            })
            .collect()
    }
}

/// Aggregate counts for health / staleness / coverage reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_count: u64,
    pub edge_count: u64,
    pub file_count: u64,
    pub unresolved_ref_count: u64,
    pub nodes_by_kind: BTreeMap<String, u64>,
    pub edges_by_kind: BTreeMap<String, u64>,
    /// On-disk database size in bytes. Zero for in-memory stores.
    #[serde(default)]
    pub db_size_bytes: u64,
}

/// Result envelope for a [`crate::RetrievalTool`] invocation. `diagnostics` carries the
/// agent-behavior signals (staleness, coverage warnings, `GRAPH-FALLBACK:` markers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub content: serde_json::Value,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl RetrievalResult {
    pub fn new(content: serde_json::Value) -> Self {
        Self {
            content,
            diagnostics: Vec::new(),
        }
    }
}

#[cfg(test)]
mod code_dependents_tests {
    use super::*;
    use crate::edge::ResolutionTier;
    use crate::node::Location;
    use crate::symbol::Symbol;

    fn node(id: &SymbolId, kind: NodeKind, file: &str) -> Node {
        Node::new(
            id.clone(),
            kind,
            id.as_str(),
            Language::new("typescript"),
            Location::new(file, crate::node::Span::ZERO),
        )
    }

    fn edge(source: &SymbolId, target: &SymbolId, kind: EdgeKind) -> Edge {
        Edge::new(
            source.clone(),
            target.clone(),
            kind,
            ResolutionTier::Parsed,
            "test",
        )
    }

    /// The HEAD-shaped walk from a symbol `f` in FileB with a caller `g` in FileA, PLUS the new
    /// import edge FileA→FileB: the symbol start keeps `g`, FileB (contains f) and FileA
    /// (contains g) — exact HEAD parity — and drops an import-only transit File.
    #[test]
    fn symbol_start_keeps_contains_holding_files_drops_import_transit() {
        let f = Symbol::file("b.ts"); // FileB
        let file_b = f.id();
        let file_a = Symbol::file("a.ts").id();
        let file_t = Symbol::file("t.ts").id(); // transit importer: t.ts imports a.ts? no — imports b.ts
        let sym_f = SymbolId("f".into());
        let sym_g = SymbolId("g".into());

        let sub = Subgraph {
            nodes: vec![
                node(&sym_f, NodeKind::Function, "b.ts"),
                node(&sym_g, NodeKind::Function, "a.ts"),
                node(&file_b, NodeKind::File, "b.ts"),
                node(&file_a, NodeKind::File, "a.ts"),
                node(&file_t, NodeKind::File, "t.ts"),
            ],
            edges: vec![
                edge(&sym_g, &sym_f, EdgeKind::Calls),     // g calls f
                edge(&file_b, &sym_f, EdgeKind::Contains), // FileB contains f (the start)
                edge(&file_a, &sym_g, EdgeKind::Contains), // FileA contains g (a caller)
                edge(&file_a, &file_b, EdgeKind::Imports), // FileA imports FileB
                edge(&file_t, &file_b, EdgeKind::Imports), // t.ts imports FileB — transit only
            ],
            depths: Default::default(),
            truncated: false,
        };

        let deps = sub.code_dependents(&sym_f, Some(&NodeKind::Function));
        let ids: Vec<&str> = deps.iter().map(|n| n.symbol.as_str()).collect();
        assert!(ids.contains(&"g"), "caller kept: {ids:?}");
        assert!(
            ids.contains(&file_b.as_str()),
            "the start's containing file is a HEAD dependent and stays: {ids:?}"
        );
        assert!(
            ids.contains(&file_a.as_str()),
            "a caller's containing file is a HEAD dependent (Calls→Contains) and stays: {ids:?}"
        );
        assert!(
            !ids.contains(&file_t.as_str()),
            "an import-only transit File must be dropped: {ids:?}"
        );
        assert!(!ids.contains(&"f"), "the start itself is never returned");
    }

    /// A File start keeps every reached node — importing files ARE the file's blast radius.
    #[test]
    fn file_start_keeps_all_importers() {
        let file_b = Symbol::file("b.ts").id();
        let file_a = Symbol::file("a.ts").id();
        let file_t = Symbol::file("t.ts").id();
        let sub = Subgraph {
            nodes: vec![
                node(&file_b, NodeKind::File, "b.ts"),
                node(&file_a, NodeKind::File, "a.ts"),
                node(&file_t, NodeKind::File, "t.ts"),
            ],
            edges: vec![
                edge(&file_a, &file_b, EdgeKind::Imports),
                edge(&file_t, &file_a, EdgeKind::Imports), // transitive importer
            ],
            depths: Default::default(),
            truncated: false,
        };
        let deps = sub.code_dependents(&file_b, Some(&NodeKind::File));
        let ids: Vec<&str> = deps.iter().map(|n| n.symbol.as_str()).collect();
        assert_eq!(ids.len(), 2, "both importers kept: {ids:?}");
        assert!(ids.contains(&file_a.as_str()));
        assert!(ids.contains(&file_t.as_str()));
    }

    /// Unknown start kind (node not in the store) behaves as a non-File start.
    #[test]
    fn unknown_start_kind_uses_the_contains_rule() {
        let sym_f = SymbolId("f".into());
        let file_t = Symbol::file("t.ts").id();
        let sub = Subgraph {
            nodes: vec![
                node(&sym_f, NodeKind::Function, "b.ts"),
                node(&file_t, NodeKind::File, "t.ts"),
            ],
            edges: vec![edge(&file_t, &Symbol::file("b.ts").id(), EdgeKind::Imports)],
            depths: Default::default(),
            truncated: false,
        };
        let deps = sub.code_dependents(&sym_f, None);
        assert!(
            deps.is_empty(),
            "transit File dropped under the contains rule even without a start kind"
        );
    }
}
