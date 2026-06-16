//! Community detection over the CALLS/IMPORTS graph.
//!
//! # Status / seam
//!
//! This module is the fixed seam for the community-detection work. [`detect_communities`] takes a
//! [`CommunityParams`] (resolution, hierarchical, package-bias knobs) and returns communities
//! largest-first. The current body is **connected-components (union-find)** — correct but coarse:
//! anything transitively connected collapses into one community, which is exactly why a large
//! codebase yields a single mega-community. The Louvain implementation (modularity optimisation
//! with a resolution parameter, optional hierarchical refinement, optional package bias) replaces
//! the body **behind this unchanged signature**.
//!
//! [`modularity`] and [`max_community_fraction`] are real now — they score *any* partition and are
//! independent of how the partition was produced, so the benchmark gate (no mega-community) and the
//! Louvain agent both build against them today.

use std::collections::HashMap;
use wicked_estate_core::{EdgeKind, GraphRead, Result, SymbolId};

// ─── parameters ────────────────────────────────────────────────────────────────

/// Tuning knobs for [`detect_communities`].
///
/// `min_size` / `include_singletons` are honoured today. `resolution`, `hierarchical`, and
/// `package_bias` are part of the seam the Louvain implementation fills in; the union-find stub
/// ignores them (a `γ` has no meaning for connected components).
#[derive(Debug, Clone)]
pub struct CommunityParams {
    /// Minimum community size to return. Communities smaller than this are dropped.
    pub min_size: usize,
    /// Include nodes that touch no CALLS/IMPORTS edge (each becomes a singleton).
    pub include_singletons: bool,
    /// Modularity resolution γ. `1.0` = standard modularity; `> 1.0` yields smaller, tighter
    /// communities (use to break up a mega-community); `< 1.0` yields coarser ones. Ignored by the
    /// union-find stub.
    pub resolution: f64,
    /// Run a second pass that re-partitions each large community at higher resolution, producing a
    /// two-level result flattened into the returned `Vec`. Ignored by the union-find stub.
    pub hierarchical: bool,
    /// Weight of synthetic same-directory (package) edges, as a fraction of the median real edge
    /// weight, added before optimisation so package structure informs the partition without forcing
    /// it. `0.0` disables. Ignored by the union-find stub.
    pub package_bias: f64,
}

impl Default for CommunityParams {
    fn default() -> Self {
        Self {
            min_size: 2,
            include_singletons: false,
            resolution: 1.0,
            hierarchical: false,
            package_bias: 0.0,
        }
    }
}

impl CommunityParams {
    /// Convenience constructor mirroring the historical `(min_size, include_singletons)` call.
    pub fn new(min_size: usize, include_singletons: bool) -> Self {
        Self {
            min_size,
            include_singletons,
            ..Default::default()
        }
    }
}

// ─── union-find (current backend) ────────────────────────────────────────────────

pub(crate) struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        if self.rank[rx] < self.rank[ry] {
            self.parent[rx] = ry;
        } else if self.rank[rx] > self.rank[ry] {
            self.parent[ry] = rx;
        } else {
            self.parent[ry] = rx;
            self.rank[rx] += 1;
        }
    }
}

// ─── detection ───────────────────────────────────────────────────────────────────

/// Detect communities in the CALLS/IMPORTS graph, largest-first.
///
/// Only [`EdgeKind::Calls`] and [`EdgeKind::Imports`] edges define membership. Singletons
/// (nodes touching no such edge) are excluded unless `params.include_singletons` is set.
/// Communities smaller than `params.min_size` are dropped.
pub fn detect_communities(
    store: &dyn GraphRead,
    params: &CommunityParams,
) -> Result<Vec<Vec<SymbolId>>> {
    let all_nodes = store.all_nodes()?;
    let all_edges = store.all_edges()?;

    if all_nodes.is_empty() {
        return Ok(Vec::new());
    }

    let id_to_idx: HashMap<SymbolId, usize> = all_nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.symbol.clone(), i))
        .collect();

    let mut uf = UnionFind::new(all_nodes.len());
    let mut has_edge: std::collections::HashSet<usize> =
        std::collections::HashSet::with_capacity(all_nodes.len());

    for edge in &all_edges {
        if !matches!(edge.kind, EdgeKind::Calls | EdgeKind::Imports) {
            continue;
        }
        let (Some(&si), Some(&ti)) = (id_to_idx.get(&edge.source), id_to_idx.get(&edge.target))
        else {
            continue;
        };
        if si == ti {
            continue;
        }
        uf.union(si, ti);
        has_edge.insert(si);
        has_edge.insert(ti);
    }

    let mut groups: HashMap<usize, Vec<SymbolId>> = HashMap::new();
    for (idx, node) in all_nodes.iter().enumerate() {
        if !params.include_singletons && !has_edge.contains(&idx) {
            continue;
        }
        let root = uf.find(idx);
        groups.entry(root).or_default().push(node.symbol.clone());
    }

    let mut communities: Vec<Vec<SymbolId>> = groups
        .into_values()
        .filter(|c| c.len() >= params.min_size)
        .collect();

    communities.sort_unstable_by_key(|c| std::cmp::Reverse(c.len()));

    Ok(communities)
}

// ─── partition quality (real; backend-independent) ───────────────────────────────

/// Newman–Girvan modularity `Q_γ` of a partition over the CALLS/IMPORTS graph, treated as
/// undirected and unweighted.
///
/// `Q_γ = Σ_c [ l_c / m − γ · (d_c / 2m)² ]` where `m` is the edge count, `l_c` the edges interior
/// to community `c`, and `d_c` the total degree of `c`'s members. Only edges whose **both**
/// endpoints appear in `communities` contribute (symbols filtered out by `min_size` are ignored).
/// Returns `0.0` for an empty graph. Range is roughly `[-0.5, 1.0]`; a good partition scores `> 0.3`.
pub fn modularity(
    store: &dyn GraphRead,
    communities: &[Vec<SymbolId>],
    resolution: f64,
) -> Result<f64> {
    // symbol -> community index
    let mut comm: HashMap<&SymbolId, usize> = HashMap::new();
    for (ci, members) in communities.iter().enumerate() {
        for s in members {
            comm.insert(s, ci);
        }
    }
    if comm.is_empty() {
        return Ok(0.0);
    }

    let all_edges = store.all_edges()?;
    let mut m: f64 = 0.0;
    let mut degree: HashMap<usize, f64> = HashMap::new();
    let mut internal: HashMap<usize, f64> = HashMap::new();

    for edge in &all_edges {
        if !matches!(edge.kind, EdgeKind::Calls | EdgeKind::Imports) {
            continue;
        }
        let (Some(&cu), Some(&cv)) = (comm.get(&edge.source), comm.get(&edge.target)) else {
            continue;
        };
        if edge.source == edge.target {
            continue;
        }
        m += 1.0;
        *degree.entry(cu).or_insert(0.0) += 1.0;
        *degree.entry(cv).or_insert(0.0) += 1.0;
        if cu == cv {
            *internal.entry(cu).or_insert(0.0) += 1.0;
        }
    }

    if m == 0.0 {
        return Ok(0.0);
    }

    let mut q = 0.0;
    for ci in 0..communities.len() {
        let l_c = internal.get(&ci).copied().unwrap_or(0.0);
        let d_c = degree.get(&ci).copied().unwrap_or(0.0);
        q += l_c / m - resolution * (d_c / (2.0 * m)).powi(2);
    }
    Ok(q)
}

/// Fraction of all partitioned symbols that fall in the single largest community.
///
/// The mega-community regression signal: connected-components on any connected graph returns `~1.0`
/// here; a healthy Louvain partition keeps it well under `0.3`. Returns `0.0` for an empty partition.
pub fn max_community_fraction(communities: &[Vec<SymbolId>]) -> f64 {
    let total: usize = communities.iter().map(|c| c.len()).sum();
    if total == 0 {
        return 0.0;
    }
    let largest = communities.iter().map(|c| c.len()).max().unwrap_or(0);
    largest as f64 / total as f64
}

// ─── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        Confidence, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, Provenance,
        Span,
    };
    use wicked_estate_store::MemStore;

    fn sym(name: &str) -> SymbolId {
        SymbolId(name.to_string())
    }

    fn make_node(id: &str) -> Node {
        make_node_in(id, "src/lib.rs")
    }

    /// Node whose source path is `file` — lets package-bias tests place symbols in different
    /// directories.
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

    #[test]
    fn detect_communities_two_components() {
        let mut store = MemStore::new();
        for name in ["A", "B", "C", "D", "E", "F"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[
                make_calls_edge("A", "B"),
                make_calls_edge("B", "C"),
                make_calls_edge("D", "E"),
            ])
            .unwrap();

        let communities = detect_communities(&store, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(communities.len(), 2, "expected 2 communities");
        assert_eq!(communities[0].len(), 3, "largest must have 3 symbols");
        assert_eq!(communities[1].len(), 2, "second must have 2 symbols");
        let flat: std::collections::HashSet<String> =
            communities[0].iter().map(|s| s.0.clone()).collect();
        assert!(flat.contains("A") && flat.contains("B") && flat.contains("C"));
    }

    #[test]
    fn detect_communities_min_size_filter() {
        let mut store = MemStore::new();
        for name in ["A", "B", "C", "D", "E", "F"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[
                make_calls_edge("A", "B"),
                make_calls_edge("B", "C"),
                make_calls_edge("D", "E"),
            ])
            .unwrap();

        let communities = detect_communities(&store, &CommunityParams::new(3, false)).unwrap();
        assert_eq!(communities.len(), 1, "only one community has size >= 3");
        assert_eq!(communities[0].len(), 3);
    }

    #[test]
    fn detect_communities_empty_graph() {
        let store = MemStore::new();
        let communities = detect_communities(&store, &CommunityParams::new(2, false)).unwrap();
        assert!(communities.is_empty());
    }

    #[test]
    fn modularity_two_cliques_is_high() {
        // Two triangles, no cross edges → a clean 2-community partition scores well above 0.3.
        let mut store = MemStore::new();
        for name in ["A", "B", "C", "D", "E", "F"] {
            store.upsert_nodes(&[make_node(name)]).unwrap();
        }
        store
            .upsert_edges(&[
                make_calls_edge("A", "B"),
                make_calls_edge("B", "C"),
                make_calls_edge("C", "A"),
                make_calls_edge("D", "E"),
                make_calls_edge("E", "F"),
                make_calls_edge("F", "D"),
            ])
            .unwrap();

        let communities = detect_communities(&store, &CommunityParams::new(2, false)).unwrap();
        let q = modularity(&store, &communities, 1.0).unwrap();
        assert!(
            q > 0.3,
            "two-clique partition modularity must exceed 0.3, got {q}"
        );
    }

    #[test]
    fn max_community_fraction_flags_mega_community() {
        // One community of 9 and one of 1 → 0.9 fraction.
        let big: Vec<SymbolId> = (0..9).map(|i| sym(&format!("n{i}"))).collect();
        let small = vec![sym("solo")];
        let frac = max_community_fraction(&[big, small]);
        assert!((frac - 0.9).abs() < 1e-9, "expected 0.9, got {frac}");
        assert_eq!(max_community_fraction(&[]), 0.0);
    }
}
