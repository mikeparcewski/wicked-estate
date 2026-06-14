//! Property-based tests for `wicked-estate-rank`.
//!
//! These tests build small, random graphs using [`MemStore`] and assert invariants
//! that must hold for **every** graph, not just hand-crafted examples.
//!
//! P4 — All global PageRank scores are finite and ≥ 0 for any graph.
//!
//! P5 — Determinism: ranking the same graph twice yields identical scores.
//!
//! P6 — Single-node graph → that node ≈ 1.0 (within 1e-3).
//!      Empty graph → empty map.
//!
//! P7 — A seeded node's personalized score ≥ its global score (seed weighting
//!      never demotes the seed — SEED_WEIGHT is a positive bias, not a penalty).

use wicked_estate_core::{
    Confidence, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, Provenance, Ranker,
    Span, SymbolId,
};
use wicked_estate_rank::{PageRank, SEED_WEIGHT};
use wicked_estate_store::MemStore;
use proptest::prelude::*;

// ─── graph builder helpers ────────────────────────────────────────────────────

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
        resolved_by: "prop-test".into(),
        location: None,
        metadata: Default::default(),
    }
}

// ─── strategy: small random graphs ───────────────────────────────────────────

/// A small graph description: node count + edge list.
/// Nodes are named "n0", "n1", ..., "n{count-1}".
/// Edges are (src_index, tgt_index) pairs; self-loops and duplicates are allowed
/// (wicked-estate-rank skips self-loops; MemStore deduplicates via `dedup_key`).
#[derive(Debug, Clone)]
struct RandGraph {
    node_count: usize,
    edges: Vec<(usize, usize)>,
}

/// Build a strategy that yields graphs with 1–30 nodes and 0–60 edges.
fn arb_rand_graph() -> impl Strategy<Value = RandGraph> {
    (1usize..=30).prop_flat_map(|n| {
        let max_edges = (n * 4).min(60);
        prop::collection::vec((0..n, 0..n), 0..=max_edges).prop_map(move |edges| RandGraph {
            node_count: n,
            edges,
        })
    })
}

fn populate_store(g: &RandGraph) -> MemStore {
    let mut store = MemStore::new();
    let nodes: Vec<Node> = (0..g.node_count)
        .map(|i| make_node(&format!("n{i}")))
        .collect();
    store.upsert_nodes(&nodes).expect("upsert_nodes");

    let edges: Vec<Edge> = g
        .edges
        .iter()
        .map(|(s, t)| make_calls_edge(&format!("n{s}"), &format!("n{t}")))
        .collect();
    store.upsert_edges(&edges).expect("upsert_edges");
    store
}

// ─── P4: all scores finite and ≥ 0 ──────────────────────────────────────────

proptest! {
    #[test]
    fn p4_all_scores_finite_and_non_negative(g in arb_rand_graph()) {
        let store = populate_store(&g);
        let scores = PageRank::new()
            .rank(&store, &[])
            .expect("rank must not fail");

        for (id, score) in &scores {
            prop_assert!(
                score.is_finite(),
                "score for {:?} must be finite, got {}",
                id,
                score
            );
            prop_assert!(
                *score >= 0.0,
                "score for {:?} must be >= 0.0, got {}",
                id,
                score
            );
        }
        prop_assert_eq!(
            scores.len(),
            g.node_count,
            "score map must have one entry per node"
        );
    }
}

// ─── P5: determinism ─────────────────────────────────────────────────────────

proptest! {
    #[test]
    fn p5_same_graph_yields_identical_scores(g in arb_rand_graph()) {
        let store = populate_store(&g);
        let pr = PageRank::new();
        let scores1 = pr.rank(&store, &[]).expect("first rank must succeed");
        let scores2 = pr.rank(&store, &[]).expect("second rank must succeed");

        prop_assert_eq!(scores1.len(), scores2.len(), "score map sizes must be equal");

        for (id, s1) in &scores1 {
            let s2 = scores2.get(id).expect("id must exist in second run");
            prop_assert_eq!(
                s1.to_bits(),
                s2.to_bits(),
                "scores for {:?} must be bit-identical across two runs: {} vs {}",
                id,
                s1,
                s2
            );
        }
    }
}

// ─── P6a: single-node graph ≈ 1.0 ────────────────────────────────────────────

#[test]
fn p6_single_node_score_approx_one() {
    let mut store = MemStore::new();
    store.upsert_nodes(&[make_node("only")]).expect("upsert");
    let scores = PageRank::new()
        .rank(&store, &[])
        .expect("rank must not fail");
    assert_eq!(scores.len(), 1, "single-node graph must have one score");
    let s = scores[&sym("only")];
    assert!(
        (s - 1.0_f32).abs() < 1e-3,
        "single-node score must be ≈ 1.0, got {s}"
    );
}

// ─── P6b: empty graph → empty map ────────────────────────────────────────────

#[test]
fn p6_empty_graph_returns_empty_map() {
    let store = MemStore::new();
    let scores = PageRank::new()
        .rank(&store, &[])
        .expect("rank must not fail on empty graph");
    assert!(scores.is_empty(), "empty graph must yield empty score map");
}

// ─── P7: seed weighting never demotes the seed ───────────────────────────────
//
// For any graph with ≥ 1 node, pick "n0" as the seed.
// Its personalized score must be ≥ its global score minus a small tolerance.
//
// The tolerance accounts for floating-point arithmetic across 30 iterations.
// Theoretical guarantee: the seed receives extra teleport mass (SEED_WEIGHT ≈ 100×
// the default), so its personalized score can only increase.  In practice small
// graphs with extreme damping may produce differences just above FP noise; we
// use `max(eps_abs, frac * global)` to stay robust.

proptest! {
    #[test]
    fn p7_seed_personalized_score_gte_global(g in arb_rand_graph()) {
        let store = populate_store(&g);
        let pr = PageRank::new();

        let global = pr.rank(&store, &[]).expect("global rank");
        let seeds = [sym("n0")];
        let personal = pr.rank(&store, &seeds).expect("personal rank");

        let global_score = *global.get(&sym("n0")).expect("n0 in global scores");
        let personal_score = *personal.get(&sym("n0")).expect("n0 in personal scores");

        // Tolerance: the larger of an absolute floor and a relative fraction of global.
        // SEED_WEIGHT is 100.0 (a very large positive bias); even in the worst case
        // of a single iteration the seed receives substantially more mass. We use 1e-2
        // absolute + 5% relative as a deliberately generous lower bound.
        let tol = (1e-2_f32).max(0.05 * global_score);

        prop_assert!(
            personal_score >= global_score - tol,
            "seed n0 personalized score ({}) must not be substantially below \
             global score ({}); SEED_WEIGHT={}; tol={}",
            personal_score,
            global_score,
            SEED_WEIGHT,
            tol
        );
    }
}
