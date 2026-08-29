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
    /// - **Non-File start**: keep a File node iff this subgraph holds ANY non-`Imports` edge
    ///   whose source is that File. The start's own containing file and every caller's
    ///   containing file pass (their `Contains` edge to the start/caller is walked, so it is in
    ///   `edges`); a file with FILE-SCOPE call sites (a test file whose top-level code calls
    ///   the start — the ref's `from` is the File symbol itself) passes via its `Calls` edge;
    ///   a File reached only through File→File import edges has no such edge here and is
    ///   dropped. This is exact pre-File→File-edge parity for symbol starts: every File in a
    ///   HEAD dependents subgraph was reached through some non-`Imports` edge it is the source
    ///   of, and that edge is always collected once its target is visited — see
    ///   docs/recon/relative-imports.md Decision G (FEAS-1; the first contains-only cut of
    ///   this rule dropped file-scope callers, caught by the cross-binary §5 gate).
    /// - **File or Import start** (`start_kind = Some(File | Import)`): keep everything — the
    ///   importing files ARE the blast radius of a file or of a dependency (Import) node. An
    ///   Import start MUST NOT use the non-File rule: in a Dependents walk from an Import node
    ///   every reached File's only subgraph source-edges are `Imports`, so the filter would
    ///   silently zero the result — a regression against HEAD on untouched pre-upgrade DBs
    ///   (`blast-radius react` on studio returned 95 importer Files; round-1
    ///   REV1-IMPORT-START).
    ///
    /// The start node itself is never returned. Kept Files' min-depths may shift when an import
    /// edge offers a shorter path; callers must not depend on File-row depths.
    pub fn code_dependents(&self, start: &SymbolId, start_kind: Option<&NodeKind>) -> Vec<&Node> {
        if matches!(start_kind, Some(NodeKind::File | NodeKind::Import)) {
            return self.nodes.iter().filter(|n| &n.symbol != start).collect();
        }
        // Files that are the SOURCE of any walked non-Imports edge (Contains to a reached
        // symbol, a file-scope Calls/References site, an estate `uses`/`accesses` edge …).
        // Every edge in `self.edges` was collected because its target is a visited node, so
        // source membership here is exactly "this File depends on something reached by a
        // dependency kind other than a file import".
        let dependency_files: std::collections::HashSet<&str> = self
            .edges
            .iter()
            .filter(|e| e.kind != EdgeKind::Imports)
            .map(|e| e.source.as_str())
            .collect();
        self.nodes
            .iter()
            .filter(|n| {
                if &n.symbol == start {
                    return false;
                }
                match n.kind {
                    NodeKind::File => dependency_files.contains(n.symbol.as_str()),
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

    /// An Import start keeps every reached node — the importing files ARE the blast radius of
    /// a dependency node (round-1 REV1-IMPORT-START). Under the non-File rule this subgraph
    /// returns NOTHING: every File's only source-edges here are `Imports`, so `blast-radius
    /// react` on an untouched pre-upgrade DB went from 95 importer Files (HEAD) to zero.
    #[test]
    fn import_start_keeps_importer_files() {
        let imp = SymbolId("import/react/".into());
        let file_a = Symbol::file("a.ts").id();
        let file_t = Symbol::file("t.ts").id();
        let sub = Subgraph {
            nodes: vec![
                node(&imp, NodeKind::Import, "a.ts"),
                node(&file_a, NodeKind::File, "a.ts"),
                node(&file_t, NodeKind::File, "t.ts"),
            ],
            edges: vec![
                // a.ts imports react (File → Import node, as the extractor emits it)
                edge(&file_a, &imp, EdgeKind::Imports),
                // t.ts imports a.ts (the lane's File→File edge) — transitive importer
                edge(&file_t, &file_a, EdgeKind::Imports),
            ],
            depths: Default::default(),
            truncated: false,
        };
        let deps = sub.code_dependents(&imp, Some(&NodeKind::Import));
        let ids: Vec<&str> = deps.iter().map(|n| n.symbol.as_str()).collect();
        assert_eq!(ids.len(), 2, "both importer Files kept: {ids:?}");
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

    /// A file whose FILE-SCOPE code calls the start (the extractor attributes top-level call
    /// sites to the File symbol itself — every test file does this) is a genuine dependent at
    /// HEAD via its Calls edge and must be KEPT; the first contains-only rule dropped it
    /// (caught by the cross-binary §5 gate on wicked-studio: 27 test files vanished from
    /// apiBase's blast radius).
    #[test]
    fn symbol_start_keeps_file_scope_caller_files() {
        let sym_f = SymbolId("f".into());
        let file_test = Symbol::file("tests/x.test.ts").id();
        let file_transit = Symbol::file("t.ts").id();
        let sub = Subgraph {
            nodes: vec![
                node(&sym_f, NodeKind::Function, "b.ts"),
                node(&file_test, NodeKind::File, "tests/x.test.ts"),
                node(&file_transit, NodeKind::File, "t.ts"),
            ],
            edges: vec![
                // top-level `apiBase()` in the test file: Calls with the FILE as source
                edge(&file_test, &sym_f, EdgeKind::Calls),
                // and the same file also imports the start's file — must not demote it
                edge(&file_test, &Symbol::file("b.ts").id(), EdgeKind::Imports),
                edge(&file_transit, &Symbol::file("b.ts").id(), EdgeKind::Imports),
            ],
            depths: Default::default(),
            truncated: false,
        };
        let deps = sub.code_dependents(&sym_f, Some(&NodeKind::Function));
        let ids: Vec<&str> = deps.iter().map(|n| n.symbol.as_str()).collect();
        assert!(
            ids.contains(&file_test.as_str()),
            "a file-scope caller File is a real dependent and stays: {ids:?}"
        );
        assert!(
            !ids.contains(&file_transit.as_str()),
            "an import-only transit File is still dropped: {ids:?}"
        );
    }
}
