//! Community-quality metrics for the benchmark — the **mega-community regression gate**.
//!
//! Scores a community partition produced by `wicked-estate-rank` so the benchmark can assert the
//! engine never regresses to a single giant community (the union-find failure mode). All four
//! numbers come from the backend-independent scoring functions in `wicked-estate-rank`
//! (`modularity`, `max_community_fraction`), so this module measures — it does not re-implement.

use serde::{Deserialize, Serialize};
use wicked_estate_core::{GraphRead, Result};
use wicked_estate_rank::{CommunityParams, detect_communities, max_community_fraction, modularity};

/// Quality summary of one community partition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommunityMetrics {
    /// Newman–Girvan modularity `Q_γ` of the partition (higher is better; > 0.3 is healthy).
    pub modularity: f64,
    /// Number of communities returned (after `min_size` filtering).
    pub community_count: usize,
    /// Fraction of partitioned symbols in the single largest community. The mega-community signal:
    /// connected-components returns ~1.0; a healthy Louvain partition stays well under 0.3.
    pub max_community_fraction: f64,
    /// Fraction of all nodes that fell into no returned community (singletons / sub-`min_size`).
    pub singleton_rate: f64,
    /// Total nodes in the store.
    pub node_count: usize,
}

/// Compute [`CommunityMetrics`] for `store` under `params`.
pub fn community_metrics(
    store: &dyn GraphRead,
    params: &CommunityParams,
) -> Result<CommunityMetrics> {
    let node_count = store.all_nodes()?.len();
    let communities = detect_communities(store, params)?;
    let covered: usize = communities.iter().map(|c| c.len()).sum();
    let q = modularity(store, &communities, params.resolution)?;
    let frac = max_community_fraction(&communities);
    let singleton_rate = if node_count == 0 {
        0.0
    } else {
        (node_count - covered) as f64 / node_count as f64
    };
    Ok(CommunityMetrics {
        modularity: q,
        community_count: communities.len(),
        max_community_fraction: frac,
        singleton_rate,
        node_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Confidence, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, Provenance,
        Span, SymbolId,
    };
    use wicked_estate_store::MemStore;

    fn node(id: &str) -> Node {
        Node {
            symbol: SymbolId(id.into()),
            kind: NodeKind::Function,
            name: id.into(),
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
            scope: Default::default(),
        }
    }

    fn calls(a: &str, b: &str) -> Edge {
        Edge {
            source: SymbolId(a.into()),
            target: SymbolId(b.into()),
            kind: EdgeKind::Calls,
            confidence: Confidence::new(1.0),
            provenance: Provenance::Parsed,
            resolved_by: "test".into(),
            evidence_count: 0,
            location: None,
            metadata: Default::default(),
        }
    }

    fn two_clique_store() -> MemStore {
        let mut s = MemStore::new();
        for n in ["A", "B", "C", "D", "E", "F"] {
            s.upsert_nodes(&[node(n)]).unwrap();
        }
        // Two triangles, no cross edges.
        s.upsert_edges(&[
            calls("A", "B"),
            calls("B", "C"),
            calls("C", "A"),
            calls("D", "E"),
            calls("E", "F"),
            calls("F", "D"),
        ])
        .unwrap();
        s
    }

    #[test]
    fn community_metrics_two_cliques() {
        let s = two_clique_store();
        let m = community_metrics(&s, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(m.community_count, 2);
        assert!(
            m.modularity > 0.3,
            "modularity {} must exceed 0.3",
            m.modularity
        );
        assert!(
            (m.max_community_fraction - 0.5).abs() < 1e-9,
            "two equal cliques → 0.5, got {}",
            m.max_community_fraction
        );
        assert_eq!(m.singleton_rate, 0.0);
        assert_eq!(m.node_count, 6);
    }

    #[test]
    fn community_metrics_empty_store() {
        let s = MemStore::new();
        let m = community_metrics(&s, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(m.community_count, 0);
        assert_eq!(m.modularity, 0.0);
        assert_eq!(m.max_community_fraction, 0.0);
        assert_eq!(m.singleton_rate, 0.0);
        assert_eq!(m.node_count, 0);
    }

    #[test]
    fn community_metrics_with_singletons() {
        let mut s = two_clique_store();
        // Two isolated nodes touch no edge → singletons, excluded from communities.
        s.upsert_nodes(&[node("iso1"), node("iso2")]).unwrap();
        let m = community_metrics(&s, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(m.node_count, 8);
        assert!(
            m.singleton_rate > 0.0,
            "isolated nodes must raise singleton_rate"
        );
        assert!(
            (m.singleton_rate - 0.25).abs() < 1e-9,
            "2 of 8 isolated → 0.25"
        );
    }

    #[test]
    fn mega_community_gate_holds_on_clustered_graph() {
        // The benchmark's core assertion: a clustered (but connected) graph must NOT collapse to
        // one community. Ring of 4 triangles joined by single bridges.
        let mut s = MemStore::new();
        let mut names: Vec<String> = Vec::new();
        for c in 0..4 {
            let m: Vec<String> = (0..3).map(|i| format!("c{c}n{i}")).collect();
            names.extend(m.clone());
            for n in &m {
                s.upsert_nodes(&[node(n)]).unwrap();
            }
            s.upsert_edges(&[
                calls(&m[0], &m[1]),
                calls(&m[1], &m[2]),
                calls(&m[2], &m[0]),
            ])
            .unwrap();
        }
        for c in 0..4 {
            s.upsert_edges(&[calls(&format!("c{c}n0"), &format!("c{}n0", (c + 1) % 4))])
                .unwrap();
        }
        let m = community_metrics(&s, &CommunityParams::new(2, false)).unwrap();
        assert!(
            m.max_community_fraction < 0.30,
            "mega-community gate: largest fraction {} must be < 0.30",
            m.max_community_fraction
        );
    }
}
