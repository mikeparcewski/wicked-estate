//! `wicked-estate-rank` — Ranker impls: (personalized) PageRank over CALLS/IMPORTS (Wave 4.1).
//!
//! # Graph backend
//!
//! This crate uses [`petgraph`] (v0.6.5) as the graph engine.
//!
//! Both **global** (empty `seeds`) and **personalized** (non-empty `seeds`) PageRank run the same
//! biased-teleport power iteration **on top of the petgraph `DiGraph`** — `graph.node_indices()`,
//! `graph.edges_directed(…, Outgoing)`, and related petgraph primitives for all traversal; we build
//! no separate adjacency structure. With empty `seeds` the teleport vector is uniform, which is
//! exactly standard global PageRank; with seeds, each seed gets `SEED_WEIGHT` (~100×). The iteration
//! is dangling-node-safe (zero-out-degree mass redistributed, preserving row-stochasticity).
//!
//! We deliberately do **not** call `petgraph::algo::page_rank`: it is O(V·E) per iteration with no
//! epsilon early-stop and was MEASURED at ~60s on a 22.9k-node / 53.6k-edge graph (it dominated
//! index wall-clock — more than the entire parse phase). Our iteration is O(V+E) per step with L1
//! convergence and ranks the same graph in ~0.1s. Do not "optimize" this back to the library call.
//!
//! # Future centrality algorithms
//!
//! petgraph 0.6.5 does not yet ship a betweenness-centrality implementation.
//! When it does (tracked upstream), it will be accessible as
//! `petgraph::algo::betweenness_centrality` and can be exposed here as a companion
//! to `ranked_symbols` for hotspot analysis without any structural changes to this crate.
//!
//! # Public surface
//!
//! * [`PageRank`] — implements [`wicked_estate_core::Ranker`]; configurable damping + iteration budget.
//! * [`ranked_symbols`] — convenience wrapper: runs PageRank and returns the top-N symbols
//!   sorted by score descending.

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use wicked_estate_core::{EdgeKind, GraphRead, Ranker, Result, SymbolId};

pub mod community;
pub mod semantic_cluster;

pub use community::{CommunityParams, detect_communities, max_community_fraction, modularity};
pub use semantic_cluster::{
    ClusterAlgo, SemanticClusterParams, cosine_distance, semantic_clusters,
};

// ─── constants ──────────────────────────────────────────────────────────────

/// Default damping factor (Brin & Page 1998).
pub const DEFAULT_DAMPING: f32 = 0.85;

/// Default maximum power-iteration steps.
pub const DEFAULT_MAX_ITER: usize = 30;

/// L1 convergence threshold; iteration stops early when the rank-delta falls below this.
pub const DEFAULT_EPSILON: f32 = 1e-6;

/// Weight multiplier applied to seed nodes in the personalised teleport vector.
/// Matches the Aider repo-map pattern.
pub const SEED_WEIGHT: f32 = 100.0;

// ─── PageRank ────────────────────────────────────────────────────────────────

/// Personalised PageRank ranker over a [`GraphRead`] store.
///
/// Only [`EdgeKind::Calls`] and [`EdgeKind::Imports`] edges contribute to the
/// link structure. Other edge kinds (Contains, Defines, …) are ignored so that
/// structural / syntactic containment does not pollute the relevance signal.
///
/// # Algorithm backend
///
/// Internally the ranker builds a [`petgraph::graph::DiGraph`] from the store's nodes and filtered
/// edges, then runs biased-teleport power iteration directly on petgraph's node/edge iterators for
/// BOTH global (uniform teleport) and personalized (seed-weighted teleport) PageRank. It does NOT
/// call `petgraph::algo::page_rank` — that is O(V·E)/iteration and was measured ~60× slower here.
///
/// # Dangling nodes
///
/// A node with no out-edges (in the CALLS/IMPORTS sub-graph) is a *dangling node*.
/// On each iteration its accumulated mass is distributed uniformly across *all* nodes
/// rather than being lost, preserving the row-stochastic invariant.
///
/// # Personalised teleport
///
/// When `seeds` is non-empty the teleport vector assigns `SEED_WEIGHT` (≈100×) to
/// each seed and `1.0` to every other node, then normalises. With empty seeds the
/// vector is uniform, yielding standard global PageRank.
#[derive(Debug, Clone)]
pub struct PageRank {
    /// Damping factor d. Probability that the random surfer follows a link.
    pub damping: f32,
    /// Maximum number of power-iteration steps.
    pub max_iter: usize,
    /// L1 convergence threshold; iteration stops early when the rank-delta falls below this.
    pub epsilon: f32,
}

impl Default for PageRank {
    fn default() -> Self {
        Self {
            damping: DEFAULT_DAMPING,
            max_iter: DEFAULT_MAX_ITER,
            epsilon: DEFAULT_EPSILON,
        }
    }
}

impl PageRank {
    /// Create with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with explicit parameters.
    pub fn with_params(damping: f32, max_iter: usize, epsilon: f32) -> Self {
        Self {
            damping,
            max_iter,
            epsilon,
        }
    }
}

impl Ranker for PageRank {
    fn rank(&self, store: &dyn GraphRead, seeds: &[SymbolId]) -> Result<HashMap<SymbolId, f32>> {
        pagerank_inner(store, seeds, self.damping, self.max_iter, self.epsilon)
    }
}

// ─── graph construction ───────────────────────────────────────────────────────

/// Build a petgraph `DiGraph` from the store, filtered to CALLS + IMPORTS edges.
///
/// Returns:
/// - The directed graph (node weight = `SymbolId`, edge weight = `f32` placeholder).
/// - A map from `SymbolId` → `NodeIndex` for O(1) lookup.
/// - The ordered `Vec<SymbolId>` corresponding to node indices (index_of[i] == NodeIndex(i)).
fn build_graph(
    store: &dyn GraphRead,
) -> Result<(DiGraph<SymbolId, f32>, HashMap<SymbolId, NodeIndex>)> {
    let all_nodes = store.all_nodes()?;
    let all_edges = store.all_edges()?;

    // DiGraph<NodeWeight=SymbolId, EdgeWeight=f32>
    let mut graph: DiGraph<SymbolId, f32> =
        DiGraph::with_capacity(all_nodes.len(), all_edges.len());
    let mut node_index: HashMap<SymbolId, NodeIndex> = HashMap::with_capacity(all_nodes.len());

    for node in &all_nodes {
        let idx = graph.add_node(node.symbol.clone());
        node_index.insert(node.symbol.clone(), idx);
    }

    for edge in &all_edges {
        if !matches!(edge.kind, EdgeKind::Calls | EdgeKind::Imports) {
            continue;
        }
        let Some(&src) = node_index.get(&edge.source) else {
            continue;
        };
        let Some(&tgt) = node_index.get(&edge.target) else {
            continue;
        };
        if src == tgt {
            continue; // skip self-loops
        }
        // Edge direction: caller(source) → callee(target); callee receives rank from caller.
        graph.add_edge(src, tgt, 1.0_f32);
    }

    Ok((graph, node_index))
}

// ─── core algorithm ──────────────────────────────────────────────────────────

fn pagerank_inner(
    store: &dyn GraphRead,
    seeds: &[SymbolId],
    damping: f32,
    max_iter: usize,
    epsilon: f32,
) -> Result<HashMap<SymbolId, f32>> {
    let all_nodes = store.all_nodes()?;
    let n = all_nodes.len();

    if n == 0 {
        return Ok(HashMap::new());
    }

    let (graph, node_index) = build_graph(store)?;

    // ── power iteration on petgraph's graph — handles BOTH global and personalized ───
    //
    // We deliberately do NOT call `petgraph::algo::page_rank` for the global (empty-seeds)
    // case. MEASURED: it is O(V·E) per iteration with no epsilon early-stop and took ~60s on a
    // 22.9k-node / 53.6k-edge graph — it dominated index wall-clock (more than the entire
    // extract+write phase). The biased power iteration below is O(V+E) per iteration with
    // epsilon convergence; with empty `seeds` the teleport vector is uniform, which is exactly
    // standard global PageRank. Same petgraph data structure (node_indices/edges_directed),
    // correct algorithm, ~60× faster. (See docs/recon/footprint-speed.md.)

    let n_f = n as f32;

    // Build the personalised teleport vector using petgraph node indices.
    let mut teleport: Vec<f32> = vec![1.0_f32; n];
    for seed in seeds {
        if let Some(&idx) = node_index.get(seed) {
            teleport[idx.index()] = SEED_WEIGHT;
        }
    }
    let total: f32 = teleport.iter().sum();
    for v in &mut teleport {
        *v /= total;
    }

    // Pre-compute out-degrees using petgraph's edge iterator.
    let out_degrees: Vec<usize> = graph
        .node_indices()
        .map(|idx| graph.edges_directed(idx, Direction::Outgoing).count())
        .collect();

    let uniform = 1.0_f32 / n_f;
    let mut rank: Vec<f32> = vec![uniform; n];
    let mut next_rank: Vec<f32> = vec![0.0_f32; n];

    for _iter in 0..max_iter {
        // Accumulate dangling mass (petgraph: nodes with zero out-degree).
        let dangling_mass: f32 = graph
            .node_indices()
            .filter(|idx| out_degrees[idx.index()] == 0)
            .map(|idx| rank[idx.index()])
            .sum();

        // Distribute dangling mass + teleport bias.
        let base: f32 = (1.0 - damping) + damping * dangling_mass;
        for idx in graph.node_indices() {
            next_rank[idx.index()] = base * teleport[idx.index()];
        }

        // Distribute link mass using petgraph's edge traversal.
        for src in graph.node_indices() {
            let out_deg = out_degrees[src.index()];
            if out_deg == 0 {
                continue;
            }
            let contribution = damping * rank[src.index()] / out_deg as f32;
            for edge in graph.edges_directed(src, Direction::Outgoing) {
                next_rank[edge.target().index()] += contribution;
            }
        }

        // Check L1 convergence.
        let delta: f32 = rank
            .iter()
            .zip(next_rank.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        std::mem::swap(&mut rank, &mut next_rank);
        next_rank.fill(0.0);

        if delta < epsilon {
            break;
        }
    }

    // Build output map.
    let result: HashMap<SymbolId, f32> = graph
        .node_indices()
        .map(|idx| (graph[idx].clone(), rank[idx.index()]))
        .collect();

    Ok(result)
}

// ─── convenience function ────────────────────────────────────────────────────

/// Run personalised PageRank and return the top-`n` symbols sorted by score descending.
///
/// # Parameters
/// * `store`  — read-only graph (CALLS + IMPORTS edges are used)
/// * `seeds`  — symbols that should receive extra teleport weight (Aider repo-map pattern);
///   pass an empty slice for standard global PageRank
/// * `top_n`  — how many results to return; pass `usize::MAX` for all nodes
///
/// # Returns
/// `Vec<(SymbolId, f32)>` sorted high-score first, length ≤ `top_n`.
pub fn ranked_symbols(
    store: &dyn GraphRead,
    seeds: &[SymbolId],
    top_n: usize,
) -> Result<Vec<(SymbolId, f32)>> {
    let pr = PageRank::new();
    let scores = pr.rank(store, seeds)?;

    let mut pairs: Vec<(SymbolId, f32)> = scores.into_iter().collect();
    // Sort descending by score, then by symbol id for determinism on ties.
    pairs.sort_unstable_by(|(id_a, s_a), (id_b, s_b)| {
        s_b.partial_cmp(s_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| id_a.0.cmp(&id_b.0))
    });
    pairs.truncate(top_n);
    Ok(pairs)
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Confidence, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, Provenance,
        Span, SymbolId,
    };
    use wicked_estate_store::MemStore;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn sym(name: &str) -> SymbolId {
        SymbolId(name.to_string())
    }

    fn make_node(id: &str) -> Node {
        Node {
            symbol: sym(id),
            kind: NodeKind::Function,
            name: id.to_string(),
            language: Language("rust".into()),
            location: Location {
                file: "src/lib.rs".into(),
                span: Span {
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 1,
                },
            },
            signature: None,
            doc: None,
            metadata: Default::default(),
        }
    }

    fn make_calls_edge(src: &str, tgt: &str) -> Edge {
        Edge {
            source: sym(src),
            target: sym(tgt),
            kind: EdgeKind::Calls,
            confidence: Confidence::new(1.0),
            provenance: Provenance::Parsed,
            resolved_by: "test".into(),
            location: None,
            metadata: Default::default(),
        }
    }

    fn make_imports_edge(src: &str, tgt: &str) -> Edge {
        Edge {
            source: sym(src),
            target: sym(tgt),
            kind: EdgeKind::Imports,
            confidence: Confidence::new(1.0),
            provenance: Provenance::Parsed,
            resolved_by: "test".into(),
            location: None,
            metadata: Default::default(),
        }
    }

    // ── empty graph ───────────────────────────────────────────────────────────

    /// An empty store must return an empty map — no panic.
    #[test]
    fn empty_graph_returns_empty() {
        let store = MemStore::new();
        let scores = PageRank::new()
            .rank(&store, &[])
            .expect("rank must not fail on empty graph");
        assert!(scores.is_empty());
    }

    /// ranked_symbols on an empty store returns an empty vec — no panic.
    #[test]
    fn ranked_symbols_empty_graph() {
        let store = MemStore::new();
        let top = ranked_symbols(&store, &[], 10).expect("must not fail");
        assert!(top.is_empty());
    }

    // ── single node ───────────────────────────────────────────────────────────

    /// A single isolated node (no edges) must get a non-zero rank and not panic.
    #[test]
    fn single_node_no_panic() {
        let mut store = MemStore::new();
        store.upsert_nodes(&[make_node("alpha")]).unwrap();

        let scores = PageRank::new()
            .rank(&store, &[])
            .expect("rank must not fail");
        assert_eq!(scores.len(), 1);
        let score = scores[&sym("alpha")];
        assert!(
            score > 0.0,
            "single-node score must be positive, got {score}"
        );
        assert!(
            (score - 1.0_f32).abs() < 1e-4,
            "single-node score must sum to ~1, got {score}"
        );
    }

    // ── hub topology ─────────────────────────────────────────────────────────

    /// Build a "hub" graph:
    ///   caller_0, …, caller_N all call `core`.
    ///   `core` calls nothing.
    ///
    /// Global PageRank must rank `core` highest.
    #[test]
    fn hub_core_ranks_highest_globally() {
        let mut store = MemStore::new();
        let callers: Vec<String> = (0..8).map(|i| format!("caller_{i}")).collect();

        // nodes
        let mut nodes: Vec<Node> = callers.iter().map(|c| make_node(c)).collect();
        nodes.push(make_node("core"));
        store.upsert_nodes(&nodes).unwrap();

        // edges: every caller → core
        let edges: Vec<Edge> = callers.iter().map(|c| make_calls_edge(c, "core")).collect();
        store.upsert_edges(&edges).unwrap();

        let top = ranked_symbols(&store, &[], usize::MAX).expect("rank must succeed");
        assert!(!top.is_empty());
        assert_eq!(
            top[0].0,
            sym("core"),
            "core must rank #1; got {:?}",
            &top[..3.min(top.len())]
        );
    }

    // ── personalised PageRank ─────────────────────────────────────────────────

    /// When we seed on `caller_0`, its personalised rank must exceed the rank of
    /// `caller_1` (which receives equal rank in the global case).
    #[test]
    fn personalized_seed_shifts_mass_toward_seed() {
        let mut store = MemStore::new();

        // Two parallel callers both calling core.
        // In global PR they score equally; with seed on caller_0, caller_0 scores higher.
        for name in ["caller_0", "caller_1", "core"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[
                make_calls_edge("caller_0", "core"),
                make_calls_edge("caller_1", "core"),
            ])
            .unwrap();

        // Global: both callers tie.
        let global = PageRank::new().rank(&store, &[]).unwrap();
        let diff_global = (global[&sym("caller_0")] - global[&sym("caller_1")]).abs();
        assert!(
            diff_global < 1e-4,
            "global PR must treat callers equally, diff={diff_global}"
        );

        // Personalised: seed on caller_0.
        let seeds = [sym("caller_0")];
        let personal = PageRank::new().rank(&store, &seeds).unwrap();
        assert!(
            personal[&sym("caller_0")] > personal[&sym("caller_1")],
            "seeded caller_0 must outscore caller_1; scores: {} vs {}",
            personal[&sym("caller_0")],
            personal[&sym("caller_1")]
        );
    }

    /// Personalised PageRank with a seed on `caller_0` must also elevate `core`
    /// (the dependency of `caller_0`) relative to the global case.
    #[test]
    fn personalized_elevates_seed_neighborhood() {
        let mut store = MemStore::new();

        // caller_0 → core_0 (shared dep of caller_0)
        // caller_1 → core_1 (separate dep)
        for name in ["caller_0", "caller_1", "core_0", "core_1"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[
                make_calls_edge("caller_0", "core_0"),
                make_calls_edge("caller_1", "core_1"),
            ])
            .unwrap();

        let seeds = [sym("caller_0")];
        let personal = PageRank::new().rank(&store, &seeds).unwrap();

        // core_0 (neighbour of seed) must outscore core_1 (neighbour of non-seed).
        assert!(
            personal[&sym("core_0")] > personal[&sym("core_1")],
            "core_0 (seed neighbour) must outscore core_1; scores: {} vs {}",
            personal[&sym("core_0")],
            personal[&sym("core_1")]
        );
    }

    // ── imports edges count ───────────────────────────────────────────────────

    /// IMPORTS edges must contribute to rank the same way as CALLS.
    #[test]
    fn imports_edges_contribute_to_rank() {
        let mut store = MemStore::new();
        for name in ["mod_a", "mod_b", "util"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        // Both modules import util — util should rank highest.
        store
            .upsert_edges(&[
                make_imports_edge("mod_a", "util"),
                make_imports_edge("mod_b", "util"),
            ])
            .unwrap();

        let top = ranked_symbols(&store, &[], usize::MAX).unwrap();
        assert_eq!(top[0].0, sym("util"), "util (most imported) must rank #1");
    }

    // ── non-ranking edges are ignored ─────────────────────────────────────────

    /// Contains / Defines edges must not influence rank.
    #[test]
    fn non_call_import_edges_ignored() {
        use wicked_estate_core::GraphWrite;

        let mut store = MemStore::new();
        for name in ["file_a", "func_b"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        // Only a Contains edge — func_b should not get elevated rank.
        let contains_edge = Edge {
            source: sym("file_a"),
            target: sym("func_b"),
            kind: EdgeKind::Contains,
            confidence: Confidence::new(1.0),
            provenance: Provenance::Parsed,
            resolved_by: "test".into(),
            location: None,
            metadata: Default::default(),
        };
        store.upsert_edges(&[contains_edge]).unwrap();

        let scores = PageRank::new().rank(&store, &[]).unwrap();
        // With no CALLS/IMPORTS edges both nodes are dangling; ranks should be equal.
        let score_a = scores[&sym("file_a")];
        let score_b = scores[&sym("func_b")];
        assert!(
            (score_a - score_b).abs() < 1e-4,
            "Contains edges must not differentiate ranks: file_a={score_a}, func_b={score_b}"
        );
    }

    // ── ranked_symbols ordering and top_n ────────────────────────────────────

    /// ranked_symbols must be sorted descending and respect the top_n cap.
    #[test]
    fn ranked_symbols_sorted_descending_and_capped() {
        let mut store = MemStore::new();
        // 5 callers → hub
        let callers: Vec<_> = (0..5).map(|i| format!("fn_{i}")).collect();
        let mut nodes: Vec<Node> = callers.iter().map(|c| make_node(c)).collect();
        nodes.push(make_node("hub"));
        store.upsert_nodes(&nodes).unwrap();
        let edges: Vec<Edge> = callers.iter().map(|c| make_calls_edge(c, "hub")).collect();
        store.upsert_edges(&edges).unwrap();

        let top3 = ranked_symbols(&store, &[], 3).unwrap();
        assert_eq!(top3.len(), 3, "must return exactly 3 results");
        assert!(top3[0].1 >= top3[1].1, "results must be sorted descending");
        assert!(top3[1].1 >= top3[2].1, "results must be sorted descending");
        assert_eq!(top3[0].0, sym("hub"), "hub must be #1");
    }

    // ── rank sums to ~1.0 ────────────────────────────────────────────────────

    /// PageRank scores must sum to approximately 1.0 (stochastic invariant).
    #[test]
    fn rank_sums_to_one() {
        let mut store = MemStore::new();
        for name in ["a", "b", "c", "d"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[
                make_calls_edge("a", "b"),
                make_calls_edge("b", "c"),
                make_calls_edge("c", "d"),
                make_calls_edge("d", "a"),
            ])
            .unwrap();

        let scores = PageRank::new().rank(&store, &[]).unwrap();
        let total: f32 = scores.values().sum();
        assert!(
            (total - 1.0).abs() < 1e-4,
            "PageRank scores must sum to ~1.0, got {total}"
        );
    }

    // ── dangling nodes ────────────────────────────────────────────────────────

    /// Nodes that only receive links (no out-edges) must not cause panics or
    /// score-drain; the sum-to-one invariant still holds.
    #[test]
    fn dangling_nodes_no_panic_and_scores_sum_to_one() {
        let mut store = MemStore::new();
        // leaf has no out-edges; root points to it.
        for name in ["root", "leaf"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[make_calls_edge("root", "leaf")])
            .unwrap();

        let scores = PageRank::new().rank(&store, &[]).unwrap();
        let total: f32 = scores.values().sum();
        assert!(
            (total - 1.0).abs() < 1e-4,
            "sum must be ~1.0 with dangling node, got {total}"
        );
        // leaf is a sink — it should rank higher than root (receives the link).
        assert!(
            scores[&sym("leaf")] > scores[&sym("root")],
            "leaf (sink) must outrank root; leaf={}, root={}",
            scores[&sym("leaf")],
            scores[&sym("root")]
        );
    }
}
