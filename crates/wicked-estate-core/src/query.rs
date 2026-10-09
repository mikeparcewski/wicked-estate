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
    /// True if the result is INCOMPLETE for any reason — i.e. exactly
    /// `node_cap_reached || depth_horizon_reached`. This is the one field a naive caller may read
    /// and still be correct; the two cause flags below say WHICH cap bit, never WHETHER.
    ///
    /// Maintained by [`Subgraph::mark_node_cap`] / [`Subgraph::mark_depth_horizon`] /
    /// [`Subgraph::absorb_truncation`] — set those, never this, so the invariant holds by
    /// construction. [`Subgraph::truncation_invariant_holds`] pins it in the conformance kit.
    pub truncated: bool,
    /// The `max_nodes` cap dropped at least one reachable node.
    #[serde(default)]
    pub node_cap_reached: bool,
    /// The `max_depth` horizon declined to expand at least one node that had an unreached
    /// neighbour — i.e. real dependencies/dependents exist BEYOND the returned set.
    ///
    /// Historically every `traverse` impl derived `truncated` from the node cap alone, so a deep,
    /// narrow graph (a 20-hop COBOL `PERFORM` chain: few nodes, many hops) was cut silently and
    /// reported complete — wicked-estate#190. Deriving `truncated` from BOTH is the fix.
    ///
    /// When `node_cap_reached` is also set, this flag is a LOWER BOUND, not exact (#225): nodes the
    /// node cap declined are never expanded or probed, so a horizon past them can go unreported.
    /// `truncated` stays exact either way. Raise `max_nodes` until the node cap clears to read the
    /// depth cause exactly.
    #[serde(default)]
    pub depth_horizon_reached: bool,
}

impl Subgraph {
    /// Record that the `max_nodes` cap bit, keeping the `truncated` invariant.
    pub fn mark_node_cap(&mut self) {
        self.node_cap_reached = true;
        self.truncated = true;
    }

    /// Record that the `max_depth` horizon cut the result, keeping the `truncated` invariant.
    pub fn mark_depth_horizon(&mut self) {
        self.depth_horizon_reached = true;
        self.truncated = true;
    }

    /// Set both causes at once from an impl that computed them, deriving `truncated` as their OR.
    /// The single place a `traverse` implementation should establish incompleteness.
    pub fn with_caps(mut self, node_cap_reached: bool, depth_horizon_reached: bool) -> Self {
        if node_cap_reached {
            self.mark_node_cap();
        }
        if depth_horizon_reached {
            self.mark_depth_horizon();
        }
        self
    }

    /// Fold another subgraph's incompleteness into this one — the union-of-traversals case
    /// (`traverse_multi`, the overlay's cross-graph merge). ORs all three fields, so a cause lost
    /// here cannot make the union look complete.
    pub fn absorb_truncation(&mut self, other: &Subgraph) {
        // A backend that set only the legacy `truncated` bit (or a row deserialized from an
        // older schema, where the cause fields default to false) must still propagate. Before
        // wicked-estate#190 that bit meant the node cap, so it is folded in as one; copying the
        // bare bit would leave both causes false and break the invariant below.
        let legacy_only =
            other.truncated && !other.node_cap_reached && !other.depth_horizon_reached;
        if other.node_cap_reached || legacy_only {
            self.mark_node_cap();
        }
        if other.depth_horizon_reached {
            self.mark_depth_horizon();
        }
    }

    /// `truncated == node_cap_reached || depth_horizon_reached`. Asserted by the conformance kit.
    pub fn truncation_invariant_holds(&self) -> bool {
        self.truncated == (self.node_cap_reached || self.depth_horizon_reached)
    }

    /// The shortest hop sequence from `from` to whichever of `targets` is reached first,
    /// following `source → target` (the dependency orientation), or `None` when this
    /// subgraph holds no such route.
    ///
    /// Only edges whose endpoints are **both nodes of this subgraph** are walked. Backends
    /// differ in which frontier-exterior edges they induce — `SqliteStore` returns every
    /// neighbour of every reached node, including hops leaving the `max_depth` frontier to
    /// symbols it does not return as nodes, while `MemStore` stops at the frontier — so an
    /// unfiltered walk over [`Subgraph::edges`] would answer differently per backend and
    /// could emit a hop with no `Node` to denormalize.
    ///
    /// Deterministic: the adjacency is built in a stable order, so the same subgraph always
    /// yields the same route. `targets` is the whole resolved candidate set; passing a
    /// one-element slice is the ordinary case. An empty `targets`, or a `from` that is not a
    /// node here, yields `None`; `from` among `targets` yields `Some(vec![])`.
    pub fn shortest_path(&self, from: &SymbolId, targets: &[SymbolId]) -> Option<Vec<Edge>> {
        let node_set: std::collections::HashSet<&SymbolId> =
            self.nodes.iter().map(|n| &n.symbol).collect();
        if !node_set.contains(from) {
            return None;
        }
        let goals: std::collections::HashSet<&SymbolId> =
            targets.iter().filter(|t| node_set.contains(*t)).collect();
        if goals.is_empty() {
            return None;
        }
        if goals.contains(from) {
            return Some(Vec::new());
        }

        // Stable adjacency (D4): sort by (source, target, kind) so BFS visitation order is a
        // property of the data, never of a hash seed. Admit an edge only when both endpoints
        // are subgraph nodes (D1a).
        let mut admissible: Vec<&Edge> = self
            .edges
            .iter()
            .filter(|e| node_set.contains(&e.source) && node_set.contains(&e.target))
            .collect();
        // The kind tie-break uses `EdgeKind`'s derived `Ord` rather than formatting both
        // kinds per comparison, which heap-allocated twice for every comparison in a sort
        // over every admissible edge. Dropping the tie-break instead would be cheaper still
        // and would quietly lose the determinism this sort exists for: parallel edges
        // sharing both endpoints would then order by however the backend returned them.
        admissible.sort_by(|a, b| {
            a.source
                .0
                .cmp(&b.source.0)
                .then_with(|| a.target.0.cmp(&b.target.0))
                .then_with(|| a.kind.cmp(&b.kind))
        });
        let mut adjacency: std::collections::BTreeMap<&SymbolId, Vec<&Edge>> =
            std::collections::BTreeMap::new();
        for e in admissible {
            adjacency.entry(&e.source).or_default().push(e);
        }

        // BFS from `from`, recording the edge each node was first reached by.
        let mut came_from: std::collections::HashMap<&SymbolId, &Edge> =
            std::collections::HashMap::new();
        let mut seen: std::collections::HashSet<&SymbolId> = std::collections::HashSet::new();
        seen.insert(from);
        let mut queue: std::collections::VecDeque<&SymbolId> = std::collections::VecDeque::new();
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            for e in adjacency.get(current).into_iter().flatten() {
                let next = &e.target;
                if !seen.insert(next) {
                    continue;
                }
                came_from.insert(next, e);
                if goals.contains(next) {
                    // Walk the predecessor chain back to `from`, then reverse.
                    let mut hops: Vec<Edge> = Vec::new();
                    let mut cursor = next;
                    while let Some(edge) = came_from.get(cursor) {
                        hops.push((*edge).clone());
                        cursor = &edge.source;
                        if cursor == from {
                            break;
                        }
                    }
                    hops.reverse();
                    return Some(hops);
                }
                queue.push_back(next);
            }
        }
        None
    }

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

    /// Re-derive `depth_horizon_reached` for the [`Subgraph::code_dependents`] projection of a
    /// blast-radius walk, so the flag describes the rows the caller actually returns.
    ///
    /// The store's horizon probe runs on the raw all-edge-kinds walk. From a code-symbol start,
    /// the walk very often leaves the horizon only through File→File `Imports` edges, and
    /// `code_dependents` drops import-transit Files. A deeper walk then returns the identical
    /// set, so the flag was a false "more dependents exist". On a 905-file TypeScript repo that
    /// was 426 of 434 flags (98.2%).
    ///
    /// The check replays the deeper walk itself, in one call through the store seam: a
    /// [`GraphRead::traverse_multi`](crate::traits::GraphRead::traverse_multi) seeded with every
    /// frontier node (depth == `spec.max_depth`), up to [`HORIZON_EXTENSION_DEPTH`] further hops
    /// and `spec.max_nodes` nodes. Any node at a greater depth is reached through a frontier
    /// node, so this extension sees exactly what a deeper walk would add. The projection is then
    /// re-applied to the union of the two walks. The cut stays reported when the union keeps a
    /// row this walk does not: a new non-File node, a new File that sources a non-`Imports`
    /// edge, or a visited File the projection dropped that now sources one (for example a File
    /// reached only by an import that also calls a frontier node). The flag is cleared only when
    /// the extension finished inside its own bounds and added nothing to the projection; if the
    /// extension was itself cut, the flag stays, because a false alarm is preferred to a false
    /// "complete".
    ///
    /// The flag is left untouched when:
    /// - the start is a File or an Import, because their importers ARE the blast radius;
    /// - `max_depth` is 0, because then the frontier is the start, which `depths` does not hold;
    /// - the walk is not a `Dependents` walk;
    /// - the node cap also cut the walk, because then the frontier itself is incomplete and
    ///   `truncated` stays true anyway.
    pub fn refine_code_dependents_horizon(
        &mut self,
        store: &dyn crate::traits::GraphRead,
        start_kind: Option<&NodeKind>,
        spec: &TraversalSpec,
    ) -> crate::error::Result<()> {
        if !self.depth_horizon_reached
            || self.node_cap_reached
            || spec.max_depth == 0
            || spec.direction != Direction::Dependents
            || matches!(start_kind, Some(NodeKind::File | NodeKind::Import))
        {
            return Ok(());
        }
        let frontier: Vec<SymbolId> = self
            .depths
            .iter()
            .filter(|(_, d)| **d == spec.max_depth)
            .map(|(id, _)| SymbolId(id.clone()))
            .collect();
        let extension_spec = TraversalSpec {
            max_depth: HORIZON_EXTENSION_DEPTH,
            ..spec.clone()
        };
        let extension = store.traverse_multi(&frontier, &extension_spec)?;

        // Files kept by the projection: sources of a non-`Imports` edge, in this walk or in the
        // extension (the same rule as `code_dependents`, over the union of the two walks).
        let non_import_sources = |edges: &[Edge]| -> std::collections::HashSet<String> {
            edges
                .iter()
                .filter(|e| e.kind != EdgeKind::Imports)
                .map(|e| e.source.0.clone())
                .collect()
        };
        let kept_now = non_import_sources(&self.edges);
        let mut kept_union = kept_now.clone();
        kept_union.extend(non_import_sources(&extension.edges));
        let visited: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.symbol.as_str()).collect();

        let adds_a_row = extension.nodes.iter().any(|n| {
            let id = n.symbol.as_str();
            let kept = n.kind != NodeKind::File || kept_union.contains(id);
            kept && (!visited.contains(id) || (n.kind == NodeKind::File && !kept_now.contains(id)))
        }) || self.nodes.iter().any(|n| {
            // A visited File the projection drops today, made a row by an extension edge.
            let id = n.symbol.as_str();
            n.kind == NodeKind::File && !kept_now.contains(id) && kept_union.contains(id)
        });
        if adds_a_row || extension.truncated {
            // A deeper walk returns more rows, or the extension was cut before it could prove
            // otherwise: the cut stays reported.
            return Ok(());
        }
        self.depth_horizon_reached = false;
        self.truncated = self.node_cap_reached;
        Ok(())
    }
}

/// How many hops past the requested depth [`Subgraph::refine_code_dependents_horizon`] looks
/// before it gives up proving that a depth cut adds no blast-radius rows. Past this, the cut
/// stays reported.
pub const HORIZON_EXTENSION_DEPTH: u32 = 32;

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
mod absorb_truncation_tests {
    use super::*;

    /// A legacy subgraph (only `truncated` set, both causes false, as an older backend or an
    /// older serialized row produces) must keep the folded result's invariant: the legacy bit
    /// meant the node cap, so the union reports a node-cap cut.
    #[test]
    fn a_legacy_truncated_only_subgraph_folds_in_as_a_node_cap_cut() {
        let legacy = Subgraph {
            truncated: true,
            ..Default::default()
        };
        let mut acc = Subgraph::default();
        acc.absorb_truncation(&legacy);
        assert!(acc.truncated && acc.node_cap_reached && !acc.depth_horizon_reached);
        assert!(acc.truncation_invariant_holds(), "{acc:?}");
    }

    #[test]
    fn each_cause_folds_in_as_itself() {
        let mut acc = Subgraph::default();
        acc.absorb_truncation(&Subgraph::default().with_caps(false, true));
        assert!(acc.depth_horizon_reached && !acc.node_cap_reached && acc.truncated);
        acc.absorb_truncation(&Subgraph::default().with_caps(true, false));
        assert!(acc.depth_horizon_reached && acc.node_cap_reached);
        assert!(acc.truncation_invariant_holds());
        let mut clean = Subgraph::default();
        clean.absorb_truncation(&Subgraph::default());
        assert!(!clean.truncated && clean.truncation_invariant_holds());
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

#[cfg(test)]
mod shortest_path_tests {
    use super::*;
    use crate::edge::ResolutionTier;
    use crate::node::Location;

    fn sym(s: &str) -> SymbolId {
        SymbolId(s.into())
    }

    fn node(id: &SymbolId) -> Node {
        Node::new(
            id.clone(),
            NodeKind::Function,
            id.as_str(),
            Language::new("rust"),
            Location::new("a.rs", crate::node::Span::ZERO),
        )
    }

    fn edge(source: &SymbolId, target: &SymbolId) -> Edge {
        Edge::new(
            source.clone(),
            target.clone(),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
    }

    /// Build a subgraph whose node set is exactly `node_ids` and whose edges are `pairs`.
    /// `pairs` may name an endpoint absent from `node_ids` — that is the frontier-exterior
    /// shape `SqliteStore` produces and D1a filters.
    fn subgraph(node_ids: &[&str], pairs: &[(&str, &str)]) -> Subgraph {
        Subgraph {
            nodes: node_ids.iter().map(|i| node(&sym(i))).collect(),
            edges: pairs.iter().map(|(s, t)| edge(&sym(s), &sym(t))).collect(),
            depths: BTreeMap::new(),
            ..Default::default()
        }
    }

    fn hop_ids(hops: &[Edge]) -> Vec<(String, String)> {
        hops.iter()
            .map(|e| (e.source.0.clone(), e.target.0.clone()))
            .collect()
    }

    #[test]
    fn chain_returns_hops_in_order() {
        let g = subgraph(&["A", "B", "C", "D"], &[("A", "B"), ("B", "C"), ("C", "D")]);
        let hops = g
            .shortest_path(&sym("A"), &[sym("D")])
            .expect("route exists");
        assert_eq!(
            hop_ids(&hops),
            vec![
                ("A".to_string(), "B".to_string()),
                ("B".to_string(), "C".to_string()),
                ("C".to_string(), "D".to_string()),
            ]
        );
    }

    #[test]
    fn diamond_is_two_hops_and_deterministic() {
        let g = subgraph(
            &["A", "B", "C", "D"],
            &[("A", "B"), ("B", "D"), ("A", "C"), ("C", "D")],
        );
        let first = g
            .shortest_path(&sym("A"), &[sym("D")])
            .expect("route exists");
        let second = g
            .shortest_path(&sym("A"), &[sym("D")])
            .expect("route exists");
        assert_eq!(
            first.len(),
            2,
            "shortest route through a diamond is two hops"
        );
        assert_eq!(
            hop_ids(&first),
            hop_ids(&second),
            "the same subgraph must yield the identical hop sequence (D4)"
        );
    }

    #[test]
    fn same_start_and_target_is_zero_hops() {
        let g = subgraph(&["A", "B"], &[("A", "B")]);
        assert_eq!(g.shortest_path(&sym("A"), &[sym("A")]), Some(vec![]));
    }

    #[test]
    fn no_route_returns_none() {
        let g = subgraph(&["A", "B", "Z"], &[("A", "B")]);
        assert_eq!(g.shortest_path(&sym("A"), &[sym("Z")]), None);
    }

    /// D1a: `SqliteStore` induces edges to symbols outside the returned node set. A hop whose
    /// endpoint is not a subgraph node is inadmissible, so `E` is unreachable — while the
    /// route to `D`, whose endpoints are all nodes, is unaffected.
    #[test]
    fn edge_to_non_node_endpoint_is_inadmissible() {
        let g = subgraph(
            &["A", "B", "C", "D"],
            &[("A", "B"), ("B", "C"), ("C", "D"), ("D", "E")],
        );
        assert_eq!(
            g.shortest_path(&sym("A"), &[sym("E")]),
            None,
            "E is not a subgraph node, so the D→E hop must not be walked"
        );
        assert_eq!(
            g.shortest_path(&sym("A"), &[sym("D")]).map(|h| h.len()),
            Some(3),
            "filtering the frontier-exterior edge must not disturb the real route"
        );
    }

    /// The kind tie-break, which is the sole consumer of `EdgeKind`'s derived `Ord`.
    ///
    /// It decides only between two admissible edges sharing BOTH endpoints with different
    /// kinds — a source that both calls and references the same target. Every other fixture
    /// builds `Calls` edges only, so without this shape the tie-break key could be deleted
    /// (and the spine derive with it) and nothing would notice.
    #[test]
    fn parallel_edges_of_different_kinds_resolve_deterministically() {
        let mut g = subgraph(&["A", "B"], &[]);
        let mut calls = edge(&sym("A"), &sym("B"));
        calls.kind = EdgeKind::Calls;
        let mut references = edge(&sym("A"), &sym("B"));
        references.kind = EdgeKind::References;

        // Both insertion orders must pick the same edge.
        g.edges = vec![references.clone(), calls.clone()];
        let first = g.shortest_path(&sym("A"), &[sym("B")]).expect("route");
        g.edges = vec![calls, references];
        let second = g.shortest_path(&sym("A"), &[sym("B")]).expect("route");

        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].kind, second[0].kind,
            "which of two parallel edges the route carries must be a property of the data; \
             `Calls` sorts before `References` by declaration order in EdgeKind"
        );
        assert_eq!(
            first[0].kind,
            EdgeKind::Calls,
            "declaration order decides, and `Calls` is declared first — if this fails \
             because EdgeKind's variants were reordered, that reordering silently changed \
             which parallel edge every path query returns"
        );
    }

    /// The adjacency sort's reason to exist: the route must not depend on the order the
    /// backend happened to return `edges` in. Two subgraphs identical except for edge and
    /// node order must give the identical hop sequence.
    ///
    /// The diamond-determinism test above cannot catch this — it calls `shortest_path`
    /// twice on the *same* `Subgraph` value, which any pure function satisfies. Deleting or
    /// reversing the sort leaves that test green and this one red.
    #[test]
    fn hop_sequence_is_independent_of_edge_and_node_order() {
        // A diamond with two equally short routes: the tie is decided by visitation order,
        // so it is exactly where an unsorted adjacency shows through.
        let pairs = [("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")];
        let ids = ["A", "B", "C", "D"];

        let forward = subgraph(&ids, &pairs);
        let mut reversed_pairs = pairs;
        reversed_pairs.reverse();
        let mut reversed_ids = ids;
        reversed_ids.reverse();
        let reversed = subgraph(&reversed_ids, &reversed_pairs);

        let a = forward
            .shortest_path(&sym("A"), &[sym("D")])
            .expect("route exists");
        let b = reversed
            .shortest_path(&sym("A"), &[sym("D")])
            .expect("route exists");
        assert_eq!(
            hop_ids(&a),
            hop_ids(&b),
            "the same graph in a different edge/node order must yield the same route; \
             without the adjacency sort the tie flips with input order, and the two \
             backends return edges in different orders"
        );
    }

    /// The slice signature exists so one BFS can stop at whichever candidate is reached
    /// first (D2); a per-candidate loop would answer a different question.
    #[test]
    fn nearest_target_candidate_wins() {
        let g = subgraph(&["A", "B", "C", "D"], &[("A", "B"), ("B", "C"), ("C", "D")]);
        let hops = g
            .shortest_path(&sym("A"), &[sym("D"), sym("B")])
            .expect("route exists");
        assert_eq!(hop_ids(&hops), vec![("A".to_string(), "B".to_string())]);
    }
}
