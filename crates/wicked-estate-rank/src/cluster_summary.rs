//! Cluster-summary aggregation: turn raw community lists into agent-ready descriptions.
//!
//! # Purpose
//!
//! [`detect_communities`] returns `Vec<Vec<SymbolId>>` — useful for downstream algorithms but
//! opaque to an LLM agent. [`summarize_communities`] enriches each community with:
//!
//! - **`top_symbols`** — the highest-PageRank members (≤5), ranking done once over the whole store
//!   so scores are comparable across communities.
//! - **`dominant_files`** — the top ≤3 source paths / directories by member count, giving the
//!   agent a spatial anchor for the cluster.
//! - **`modularity_contribution`** — this community's exact term in the Newman–Girvan `Q_γ`
//!   formula: `l_c / m − γ·(d_c / 2m)²`, where `l_c` is the number of edges internal to the
//!   community, `d_c` is the total degree of its members (directed edges counted once each), and
//!   `m` is the total edge count. A positive value means the community has more internal edges
//!   than expected by chance; a large positive value indicates a cohesive cluster.
//!
//! # Modularity-contribution definition
//!
//! We compute the **per-community term** of the standard Newman–Girvan `Q_γ` formula, identical
//! to the formula used in [`community::modularity`]:
//!
//! ```text
//! contrib_c = l_c / m − γ · (d_c / (2m))²
//! ```
//!
//! This is **not** an approximation: summing all `contrib_c` over all communities in a partition
//! yields the global `Q_γ` returned by `modularity(store, communities, resolution)`. We choose
//! the per-community term (rather than computing a single-community modularity in isolation)
//! because it is additive, directly comparable across communities, and reflects each community's
//! share of the global modularity budget.
//!
//! # Determinism
//!
//! Output is fully deterministic: `top_symbols` ties are broken by `SymbolId` string order;
//! `dominant_files` ties are broken alphabetically; communities are sorted largest-first with the
//! lexicographically smallest `SymbolId` as a tiebreaker.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wicked_estate_core::{EdgeKind, GraphRead, Result, SymbolId};

use crate::ranked_symbols;

// ─── public types ─────────────────────────────────────────────────────────────

/// Agent-ready description of one community.
///
/// Produced by [`summarize_communities`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommunitySummary {
    /// Number of members in this community.
    pub size: usize,

    /// The community's highest-PageRank members (≤5), as label candidates.
    ///
    /// Ranked by global PageRank score descending; ties broken by `SymbolId` string ascending.
    pub top_symbols: Vec<String>,

    /// Top ≤3 source paths / directories by member count.
    ///
    /// Paths come directly from `node.location.file`; no directory-stripping is applied so both
    /// `src/foo/bar.rs` and `src/foo` are valid entries depending on what the nodes carry.
    /// Ties are broken alphabetically.
    pub dominant_files: Vec<String>,

    /// This community's term in `Q_γ = Σ_c [ l_c/m − γ·(d_c/2m)² ]`.
    ///
    /// See the module-level documentation for the exact definition. A positive value means the
    /// community is more internally connected than expected by chance.
    pub modularity_contribution: f64,
}

// ─── public entry point ────────────────────────────────────────────────────────

/// Summarise a community partition into agent-ready [`CommunitySummary`] structs.
///
/// # Parameters
/// * `store`       — read-only graph store (CALLS + IMPORTS edges + node metadata used).
/// * `communities` — a partition produced by [`detect_communities`] or any other source;
///   each inner `Vec` is one community.
/// * `resolution`  — the `γ` used when detecting the communities; passed through to the
///   modularity-contribution formula so contributions are self-consistent.
///
/// # Returns
/// One `CommunitySummary` per non-empty community, sorted **largest community first**; ties
/// broken by the lexicographically smallest `SymbolId` in each community.
///
/// Empty communities in the input are silently skipped. An empty `communities` slice (or all
/// empty communities) returns an empty `Vec`.
pub fn summarize_communities(
    store: &dyn GraphRead,
    communities: &[Vec<SymbolId>],
    resolution: f64,
) -> Result<Vec<CommunitySummary>> {
    // Fast-path: nothing to summarise.
    let non_empty: Vec<&Vec<SymbolId>> = communities.iter().filter(|c| !c.is_empty()).collect();
    if non_empty.is_empty() {
        return Ok(Vec::new());
    }

    // ── 1. Global PageRank over the whole store (one pass, shared across all communities) ──
    let pr_scores: HashMap<SymbolId, f32> = {
        let pairs = ranked_symbols(store, &[], usize::MAX)?;
        pairs.into_iter().collect()
    };

    // ── 2. Node metadata: file paths per symbol ───────────────────────────────────────────
    let all_nodes = store.all_nodes()?;
    let file_of: HashMap<&SymbolId, &str> = all_nodes
        .iter()
        .map(|n| (&n.symbol, n.location.file.as_str()))
        .collect();

    // ── 3. Modularity-contribution inputs ────────────────────────────────────────────────
    //
    // We need m (total edge count), l_c (internal edges per community), and d_c (total degree
    // per community) for each community c.  One pass over all CALLS/IMPORTS edges is enough.

    // Map each SymbolId → community index so we can classify edges.
    let mut comm_of: HashMap<&SymbolId, usize> = HashMap::new();
    for (ci, members) in non_empty.iter().enumerate() {
        for s in members.iter() {
            comm_of.insert(s, ci);
        }
    }

    let all_edges = store.all_edges()?;
    let mut m: f64 = 0.0;
    // degree: directed, so each edge contributes 1 to the source community's degree and 1 to
    // the target community's degree — matching community.rs `modularity()` exactly.
    let mut degree: Vec<f64> = vec![0.0; non_empty.len()];
    let mut internal: Vec<f64> = vec![0.0; non_empty.len()];

    for edge in &all_edges {
        if !matches!(edge.kind, EdgeKind::Calls | EdgeKind::Imports) {
            continue;
        }
        if edge.source == edge.target {
            continue;
        }
        let (Some(&cu), Some(&cv)) = (comm_of.get(&edge.source), comm_of.get(&edge.target)) else {
            continue;
        };
        m += 1.0;
        degree[cu] += 1.0;
        degree[cv] += 1.0;
        if cu == cv {
            internal[cu] += 1.0;
        }
    }

    // ── 4. Build one CommunitySummary per community ───────────────────────────────────────

    let mut summaries: Vec<CommunitySummary> = non_empty
        .iter()
        .enumerate()
        .map(|(ci, members)| {
            // top_symbols: collect (score, symbol) for this community, sort descending.
            let mut ranked: Vec<(f32, &SymbolId)> = members
                .iter()
                .map(|s| (pr_scores.get(s).copied().unwrap_or(0.0), s))
                .collect();
            ranked.sort_unstable_by(|(sa, ia), (sb, ib)| {
                sb.partial_cmp(sa)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| ia.0.cmp(&ib.0))
            });
            let top_symbols: Vec<String> =
                ranked.iter().take(5).map(|(_, s)| s.0.clone()).collect();

            // dominant_files: count members per file path, take top 3.
            let mut file_counts: HashMap<&str, usize> = HashMap::new();
            for s in members.iter() {
                if let Some(&path) = file_of.get(s) {
                    *file_counts.entry(path).or_insert(0) += 1;
                }
            }
            let mut file_vec: Vec<(&str, usize)> = file_counts.into_iter().collect();
            // Sort: count descending, then path ascending for determinism.
            file_vec.sort_unstable_by(|(pa, ca), (pb, cb)| cb.cmp(ca).then_with(|| pa.cmp(pb)));
            let dominant_files: Vec<String> = file_vec
                .iter()
                .take(3)
                .map(|(p, _)| (*p).to_string())
                .collect();

            // modularity_contribution: per-community term of Q_γ.
            let contrib = if m > 0.0 {
                internal[ci] / m - resolution * (degree[ci] / (2.0 * m)).powi(2)
            } else {
                0.0
            };

            CommunitySummary {
                size: members.len(),
                top_symbols,
                dominant_files,
                modularity_contribution: contrib,
            }
        })
        .collect();

    // ── 5. Sort: largest community first; tiebreak by smallest SymbolId in that community ─

    // Pre-compute the min SymbolId string for each community for the tiebreaker.
    let min_sym: Vec<&str> = non_empty
        .iter()
        .map(|members| members.iter().map(|s| s.0.as_str()).min().unwrap_or(""))
        .collect();

    // We need to sort summaries in tandem with their indices.
    let mut indexed: Vec<(usize, CommunitySummary)> = summaries.drain(..).enumerate().collect();
    indexed.sort_unstable_by(|(ia, sa), (ib, sb)| {
        sb.size
            .cmp(&sa.size)
            .then_with(|| min_sym[*ia].cmp(min_sym[*ib]))
    });
    summaries = indexed.into_iter().map(|(_, s)| s).collect();

    Ok(summaries)
}

// ─── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Confidence, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, Provenance,
        Span, SymbolId,
    };
    use wicked_estate_store::MemStore;

    // ─── helpers (mirror the pattern from community.rs tests) ────────────────

    fn sym(name: &str) -> SymbolId {
        SymbolId(name.to_string())
    }

    /// Build a node in a specific file path.
    fn make_node_in(id: &str, file: &str) -> Node {
        Node {
            symbol: sym(id),
            kind: NodeKind::Function,
            name: id.to_string(),
            language: Language("rust".into()),
            location: Location {
                file: file.into(),
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

    fn calls(src: &str, tgt: &str) -> Edge {
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

    /// Build a MemStore with two triangles in different directories, connected by one bridge.
    ///
    /// Triangle A: a0 ↔ a1 ↔ a2 ↔ a0  (all in "src/alpha/")
    /// Triangle B: b0 ↔ b1 ↔ b2 ↔ b0  (all in "src/beta/")
    /// Bridge: a2 → b0
    ///
    /// This mirrors the two-triangle topology from community.rs's `refine_splits_two_triangles`
    /// test but with distinct directories so `dominant_files` has unambiguous expected values.
    fn build_two_triangle_store() -> (MemStore, Vec<Vec<SymbolId>>) {
        let mut store = MemStore::new();

        // Triangle A
        for name in ["a0", "a1", "a2"] {
            store
                .upsert_nodes(&[make_node_in(name, "src/alpha/mod.rs")])
                .unwrap();
        }
        // Triangle B
        for name in ["b0", "b1", "b2"] {
            store
                .upsert_nodes(&[make_node_in(name, "src/beta/mod.rs")])
                .unwrap();
        }

        let edges = vec![
            // Triangle A (bidirectional as two directed edges to match call-graph conventions)
            calls("a0", "a1"),
            calls("a1", "a0"),
            calls("a1", "a2"),
            calls("a2", "a1"),
            calls("a0", "a2"),
            calls("a2", "a0"),
            // Triangle B
            calls("b0", "b1"),
            calls("b1", "b0"),
            calls("b1", "b2"),
            calls("b2", "b1"),
            calls("b0", "b2"),
            calls("b2", "b0"),
            // Bridge (single directed edge — weak inter-community link)
            calls("a2", "b0"),
        ];
        store.upsert_edges(&edges).unwrap();

        // Pre-built communities: one per triangle (mirrors what detect_communities produces).
        let comm_a = vec![sym("a0"), sym("a1"), sym("a2")];
        let comm_b = vec![sym("b0"), sym("b1"), sym("b2")];
        // Largest first — same size so sorted by min symbol: "a0" < "b0".
        let communities = vec![comm_a, comm_b];

        (store, communities)
    }

    // ─── tests ──────────────────────────────────────────────────────────────

    /// Empty store with empty communities → empty result, no panic.
    #[test]
    fn empty_store_empty_communities() {
        let store = MemStore::new();
        let result = summarize_communities(&store, &[], 1.0).expect("must not fail");
        assert!(result.is_empty(), "expected empty, got {result:?}");
    }

    /// Empty communities list even with a non-empty store → empty result.
    #[test]
    fn non_empty_store_empty_communities() {
        let mut store = MemStore::new();
        store
            .upsert_nodes(&[make_node_in("x", "src/lib.rs")])
            .unwrap();
        let result = summarize_communities(&store, &[], 1.0).expect("must not fail");
        assert!(result.is_empty());
    }

    /// Two-triangle topology: expect exactly 2 summaries.
    #[test]
    fn two_triangles_two_summaries() {
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).expect("must not fail");
        assert_eq!(
            summaries.len(),
            2,
            "expected 2 summaries, got {}",
            summaries.len()
        );
    }

    /// Community sizes must match the input communities.
    #[test]
    fn sizes_match_community_lengths() {
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).unwrap();
        for (i, s) in summaries.iter().enumerate() {
            assert_eq!(s.size, 3, "summary[{i}].size must be 3, got {}", s.size);
        }
    }

    /// `top_symbols` must be non-empty and contain only members of the right community.
    #[test]
    fn top_symbols_populated_and_from_correct_community() {
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).unwrap();

        let alpha_set: std::collections::HashSet<&str> = ["a0", "a1", "a2"].into();
        let beta_set: std::collections::HashSet<&str> = ["b0", "b1", "b2"].into();

        // summaries are sorted largest-first; both have size 3, tiebreak by min symbol.
        // "a0" < "b0" → alpha community is first.
        let alpha_summary = &summaries[0];
        let beta_summary = &summaries[1];

        assert!(
            !alpha_summary.top_symbols.is_empty(),
            "alpha top_symbols must not be empty"
        );
        for sym in &alpha_summary.top_symbols {
            assert!(
                alpha_set.contains(sym.as_str()),
                "alpha top_symbol '{sym}' is not an alpha member"
            );
        }

        assert!(
            !beta_summary.top_symbols.is_empty(),
            "beta top_symbols must not be empty"
        );
        for sym in &beta_summary.top_symbols {
            assert!(
                beta_set.contains(sym.as_str()),
                "beta top_symbol '{sym}' is not a beta member"
            );
        }
    }

    /// `dominant_files` must reflect the directories where each community lives.
    #[test]
    fn dominant_files_reflect_directories() {
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).unwrap();

        // Alpha community (summaries[0]): all nodes in "src/alpha/mod.rs"
        assert_eq!(
            summaries[0].dominant_files,
            vec!["src/alpha/mod.rs"],
            "alpha dominant_files must be src/alpha/mod.rs"
        );

        // Beta community (summaries[1]): all nodes in "src/beta/mod.rs"
        assert_eq!(
            summaries[1].dominant_files,
            vec!["src/beta/mod.rs"],
            "beta dominant_files must be src/beta/mod.rs"
        );
    }

    /// `modularity_contribution` must be positive for cohesive clusters.
    #[test]
    fn modularity_contribution_positive_for_cohesive_clusters() {
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).unwrap();

        for (i, s) in summaries.iter().enumerate() {
            assert!(
                s.modularity_contribution > 0.0,
                "summary[{i}].modularity_contribution must be > 0.0, got {}",
                s.modularity_contribution
            );
        }
    }

    /// Sum of per-community contributions must equal the global modularity (additive invariant).
    #[test]
    fn sum_of_contributions_equals_global_modularity() {
        use crate::modularity;
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).unwrap();

        let sum_contrib: f64 = summaries.iter().map(|s| s.modularity_contribution).sum();
        let global_q = modularity(&store, &communities, 1.0).unwrap();

        assert!(
            (sum_contrib - global_q).abs() < 1e-9,
            "sum of contributions {sum_contrib} must equal global modularity {global_q}"
        );
    }

    /// Output is deterministic: calling twice yields identical results.
    #[test]
    fn output_is_deterministic() {
        let (store, communities) = build_two_triangle_store();
        let r1 = summarize_communities(&store, &communities, 1.0).unwrap();
        let r2 = summarize_communities(&store, &communities, 1.0).unwrap();
        assert_eq!(r1, r2, "output must be deterministic");
    }

    /// Largest community comes first; equal-size communities tiebreak by min SymbolId.
    #[test]
    fn sorted_largest_first_with_tiebreak() {
        let (store, communities) = build_two_triangle_store();
        let summaries = summarize_communities(&store, &communities, 1.0).unwrap();
        // Both are size 3; alpha (min sym "a0") precedes beta (min sym "b0").
        assert_eq!(summaries[0].dominant_files[0], "src/alpha/mod.rs");
        assert_eq!(summaries[1].dominant_files[0], "src/beta/mod.rs");

        // Verify size-ordering invariant holds in general.
        for w in summaries.windows(2) {
            assert!(
                w[0].size >= w[1].size,
                "summaries must be sorted largest-first: {} vs {}",
                w[0].size,
                w[1].size
            );
        }
    }
}
