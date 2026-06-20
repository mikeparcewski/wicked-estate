//! Community detection over the CALLS/IMPORTS graph — multi-level **Louvain** with a resolution
//! parameter, optional hierarchical refinement, and optional package bias.
//!
//! # Why Louvain (not connected components)
//!
//! The previous backend was union-find (connected components): anything transitively connected
//! collapsed into one community, so a large codebase produced a single 64K-node mega-community.
//! Louvain instead maximises modularity `Q_γ`, so a graph that is *connected* but has cluster
//! structure (e.g. two cliques joined by one bridge) is split into the clusters. The resolution
//! `γ` tunes granularity; the optional second hierarchical pass breaks up any community that still
//! has internal structure.
//!
//! # Determinism
//!
//! No RNG. Nodes are processed in `SymbolId` order; neighbour-community candidates are scanned in
//! community-id order with a strict-improvement tie-break; aggregated super-nodes are renumbered by
//! first occurrence. Same input → same output, every run (the tests depend on it).
//!
//! [`modularity`] and [`max_community_fraction`] score *any* partition and are independent of how
//! it was produced (the benchmark gate and the comparison tests use them directly).

use std::collections::HashMap;
use wicked_estate_core::{EdgeKind, GraphRead, Node, Result, SymbolId};

// ─── parameters ────────────────────────────────────────────────────────────────

/// Tuning knobs for [`detect_communities`].
#[derive(Debug, Clone)]
pub struct CommunityParams {
    /// Minimum community size to return. Smaller communities are dropped.
    pub min_size: usize,
    /// Include nodes that touch no CALLS/IMPORTS edge (each becomes a singleton).
    pub include_singletons: bool,
    /// Modularity resolution γ. `1.0` = standard modularity; `> 1.0` yields smaller, tighter
    /// communities (use to break up a mega-community); `< 1.0` yields coarser ones.
    pub resolution: f64,
    /// After the flat partition, re-run Louvain at `resolution * 2.0` inside each community with
    /// internal structure, flattening the refined sub-communities into the result.
    pub hierarchical: bool,
    /// Weight of synthetic same-directory (package) edges, as a fraction of the median real edge
    /// weight, added before optimisation so package structure biases — but does not force — the
    /// partition. `0.0` disables. Added as a ring within each package (O(package size)).
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

/// Communities below this size are not worth a hierarchical refinement attempt (a clique never
/// splits; only communities with real substructure do).
const HIER_MIN_REFINE: usize = 4;

// ─── weighted graph for Louvain ──────────────────────────────────────────────────

/// Undirected weighted graph. `adj` is symmetric (each edge stored both directions), sorted by
/// neighbour id for determinism. `self_loop[i]` is the weight of `i`'s self-loop (accumulated by
/// aggregation). `k[i]` is the weighted degree (`Σ adj weights + 2·self_loop`). `m` is the total
/// edge weight (`= ½ Σ k`), held constant across aggregation levels.
struct Graph {
    adj: Vec<Vec<(usize, f64)>>,
    self_loop: Vec<f64>,
    k: Vec<f64>,
    m: f64,
}

/// Build a [`Graph`] from `n` nodes and canonical (`u <= v`) weighted edges. Parallel edges are
/// summed; `u == v` becomes a self-loop.
fn graph_from_edges(n: usize, edges: &[(usize, usize, f64)]) -> Graph {
    let mut adj_map: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
    let mut self_loop = vec![0.0; n];
    for &(u, v, w) in edges {
        if u == v {
            self_loop[u] += w;
        } else {
            *adj_map[u].entry(v).or_insert(0.0) += w;
            *adj_map[v].entry(u).or_insert(0.0) += w;
        }
    }
    let mut adj = Vec::with_capacity(n);
    let mut k = vec![0.0; n];
    let mut total = 0.0;
    for i in 0..n {
        let mut v: Vec<(usize, f64)> = adj_map[i].iter().map(|(&j, &w)| (j, w)).collect();
        v.sort_unstable_by_key(|&(j, _)| j);
        let deg: f64 = v.iter().map(|&(_, w)| w).sum::<f64>() + 2.0 * self_loop[i];
        k[i] = deg;
        total += deg;
        adj.push(v);
    }
    Graph {
        adj,
        self_loop,
        k,
        m: total / 2.0,
    }
}

/// One Louvain level: local node moving to modularity-`γ` optimum, to convergence.
///
/// Returns the community label per node (renumbered `0..K` by first occurrence) and whether any
/// node changed community.
fn one_level(g: &Graph, gamma: f64) -> (Vec<usize>, bool) {
    let n = g.adj.len();
    let mut comm: Vec<usize> = (0..n).collect();
    if g.m == 0.0 {
        return ((0..n).collect(), false);
    }
    let two_m = 2.0 * g.m;
    let mut sigma_tot: Vec<f64> = g.k.clone();
    let mut improved_any = false;

    loop {
        let mut moved = false;
        for i in 0..n {
            let ci = comm[i];
            // Weight from i to each neighbouring community.
            let mut w_to: HashMap<usize, f64> = HashMap::new();
            for &(j, w) in &g.adj[i] {
                if j == i {
                    continue;
                }
                *w_to.entry(comm[j]).or_insert(0.0) += w;
            }
            // Remove i from its current community.
            sigma_tot[ci] -= g.k[i];
            let w_i_ci = w_to.get(&ci).copied().unwrap_or(0.0);
            let mut best_comm = ci;
            let mut best_gain = w_i_ci - gamma * sigma_tot[ci] * g.k[i] / two_m;

            // Deterministic scan: candidates in community-id order, strict improvement to move.
            let mut cands: Vec<(usize, f64)> = w_to.iter().map(|(&c, &w)| (c, w)).collect();
            cands.sort_unstable_by_key(|&(c, _)| c);
            for (c, w_ic) in cands {
                if c == ci {
                    continue;
                }
                let gain = w_ic - gamma * sigma_tot[c] * g.k[i] / two_m;
                if gain > best_gain + 1e-12 {
                    best_gain = gain;
                    best_comm = c;
                }
            }

            sigma_tot[best_comm] += g.k[i];
            if best_comm != ci {
                comm[i] = best_comm;
                moved = true;
                improved_any = true;
            }
        }
        if !moved {
            break;
        }
    }

    // Renumber to 0..K by first occurrence (deterministic).
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut labels = vec![0usize; n];
    for (i, label) in labels.iter_mut().enumerate() {
        let next = remap.len();
        *label = *remap.entry(comm[i]).or_insert(next);
    }
    (labels, improved_any)
}

/// Aggregate `g` by `labels` (range `0..k`) into a `k`-super-node graph: intra-community edges
/// become self-loops, inter-community edges sum.
fn aggregate(g: &Graph, labels: &[usize], k: usize) -> Graph {
    let mut adj_map: Vec<HashMap<usize, f64>> = vec![HashMap::new(); k];
    let mut self_loop = vec![0.0; k];
    for (i, &sl) in g.self_loop.iter().enumerate() {
        self_loop[labels[i]] += sl;
    }
    for i in 0..g.adj.len() {
        for &(j, w) in &g.adj[i] {
            if j < i {
                continue; // each undirected pair once
            }
            let (ci, cj) = (labels[i], labels[j]);
            if ci == cj {
                self_loop[ci] += w;
            } else {
                *adj_map[ci].entry(cj).or_insert(0.0) += w;
                *adj_map[cj].entry(ci).or_insert(0.0) += w;
            }
        }
    }
    let mut adj = Vec::with_capacity(k);
    let mut kdeg = vec![0.0; k];
    for c in 0..k {
        let mut v: Vec<(usize, f64)> = adj_map[c].iter().map(|(&n, &w)| (n, w)).collect();
        v.sort_unstable_by_key(|&(n, _)| n);
        kdeg[c] = v.iter().map(|&(_, w)| w).sum::<f64>() + 2.0 * self_loop[c];
        adj.push(v);
    }
    Graph {
        adj,
        self_loop,
        k: kdeg,
        m: g.m, // total weight preserved by aggregation
    }
}

/// Full multi-level Louvain. Returns a community label per original node.
fn louvain(g0: Graph, gamma: f64) -> Vec<usize> {
    let n0 = g0.adj.len();
    let mut comm: Vec<usize> = (0..n0).collect();
    let mut g = g0;
    loop {
        if g.m == 0.0 {
            break;
        }
        let (labels, improved) = one_level(&g, gamma);
        for c in comm.iter_mut() {
            *c = labels[*c];
        }
        let k = labels.iter().copied().max().map(|m| m + 1).unwrap_or(0);
        if !improved || k == g.adj.len() {
            break;
        }
        g = aggregate(&g, &labels, k);
    }
    comm
}

// ─── public entry point ──────────────────────────────────────────────────────────

fn package_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Collect canonical (`u <= v`) weighted edges from the store's CALLS/IMPORTS edges, plus optional
/// synthetic package-ring edges. Returns the edge list and the set of node indices that touch at
/// least one *real* edge (used for the singleton filter — synthetic edges don't count).
fn collect_edges(
    nodes: &[Node],
    idx: &HashMap<SymbolId, usize>,
    edges: &[wicked_estate_core::Edge],
    package_bias: f64,
) -> (Vec<(usize, usize, f64)>, Vec<bool>) {
    let n = nodes.len();
    let mut acc: HashMap<(usize, usize), f64> = HashMap::new();
    let mut has_real_edge = vec![false; n];

    for e in edges {
        if !matches!(e.kind, EdgeKind::Calls | EdgeKind::Imports) {
            continue;
        }
        let (Some(&a), Some(&b)) = (idx.get(&e.source), idx.get(&e.target)) else {
            continue;
        };
        if a == b {
            continue;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        *acc.entry(key).or_insert(0.0) += 1.0;
        has_real_edge[a] = true;
        has_real_edge[b] = true;
    }

    // Package bias: a ring per directory (median real weight is 1.0, so weight = package_bias).
    if package_bias > 0.0 {
        let mut by_pkg: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, node) in nodes.iter().enumerate() {
            by_pkg
                .entry(package_of(&node.location.file))
                .or_default()
                .push(i);
        }
        for members in by_pkg.values() {
            if members.len() < 2 {
                continue;
            }
            // members are already in node-sorted order (nodes is sorted); ring them.
            for w in 0..members.len() {
                let a = members[w];
                let b = members[(w + 1) % members.len()];
                if a == b {
                    continue;
                }
                let key = if a < b { (a, b) } else { (b, a) };
                *acc.entry(key).or_insert(0.0) += package_bias;
                if members.len() == 2 {
                    break; // a 2-ring is a single edge; avoid adding it twice
                }
            }
        }
    }

    let edge_list: Vec<(usize, usize, f64)> =
        acc.into_iter().map(|((u, v), w)| (u, v, w)).collect();
    (edge_list, has_real_edge)
}

/// Induced sub-partition: run Louvain on the subgraph induced by `members` (real + package edges
/// among them) at `gamma`, returning sub-communities as lists of original node indices.
fn refine(all_edges: &[(usize, usize, f64)], members: &[usize], gamma: f64) -> Vec<Vec<usize>> {
    let local_idx: HashMap<usize, usize> =
        members.iter().enumerate().map(|(i, &m)| (m, i)).collect();
    let sub_edges: Vec<(usize, usize, f64)> = all_edges
        .iter()
        .filter_map(|&(u, v, w)| match (local_idx.get(&u), local_idx.get(&v)) {
            (Some(&lu), Some(&lv)) => Some((lu, lv, w)),
            _ => None,
        })
        .collect();
    let sub = graph_from_edges(members.len(), &sub_edges);
    let labels = louvain(sub, gamma);
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (local, &orig) in members.iter().enumerate() {
        groups.entry(labels[local]).or_default().push(orig);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_unstable_by_key(|g| g.iter().copied().min().unwrap_or(0));
    out
}

/// Detect communities in the CALLS/IMPORTS graph via multi-level Louvain, largest-first.
///
/// Only [`EdgeKind::Calls`] and [`EdgeKind::Imports`] edges define membership. Singletons (nodes
/// touching no such edge) are excluded unless `params.include_singletons`. Communities smaller than
/// `params.min_size` are dropped. See [`CommunityParams`] for `resolution`, `hierarchical`, and
/// `package_bias`.
pub fn detect_communities(
    store: &dyn GraphRead,
    params: &CommunityParams,
) -> Result<Vec<Vec<SymbolId>>> {
    let mut nodes = store.all_nodes()?;
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    // Deterministic node ordering.
    nodes.sort_unstable_by(|a, b| a.symbol.0.cmp(&b.symbol.0));
    let idx: HashMap<SymbolId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.symbol.clone(), i))
        .collect();

    let all_edges = store.all_edges()?;
    let (edge_list, has_real_edge) = collect_edges(&nodes, &idx, &all_edges, params.package_bias);

    let g0 = graph_from_edges(nodes.len(), &edge_list);
    let flat = louvain(g0, params.resolution);

    // Group node indices by flat community (deterministic: keyed scan).
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, &c) in flat.iter().enumerate() {
        groups.entry(c).or_default().push(i);
    }
    let mut group_list: Vec<Vec<usize>> = groups.into_values().collect();
    group_list.sort_unstable_by_key(|g| g.iter().copied().min().unwrap_or(0));

    // Optional hierarchical refinement of communities with internal structure.
    let refined: Vec<Vec<usize>> = if params.hierarchical {
        let mut out = Vec::new();
        for members in group_list {
            if members.len() >= HIER_MIN_REFINE {
                let sub = refine(&edge_list, &members, params.resolution * 2.0);
                // Accept the refinement only if it found GENUINE substructure: ≥2 sub-communities
                // of real size. A cohesive community (e.g. a clique) fragments into singletons at
                // the higher resolution — that is shattering, not structure, so we keep it whole.
                let nontrivial = sub.iter().filter(|g| g.len() >= 2).count();
                if nontrivial >= 2 {
                    out.extend(sub);
                    continue;
                }
            }
            out.push(members);
        }
        out
    } else {
        group_list
    };

    // Materialise SymbolIds, apply singleton + min_size filters, sort largest first.
    let mut communities: Vec<Vec<SymbolId>> = refined
        .into_iter()
        .map(|members| {
            members
                .into_iter()
                .filter(|&i| params.include_singletons || has_real_edge[i])
                .map(|i| nodes[i].symbol.clone())
                .collect::<Vec<_>>()
        })
        .filter(|c| c.len() >= params.min_size)
        .collect();

    communities.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| a.first().map(|s| &s.0).cmp(&b.first().map(|s| &s.0)))
    });
    Ok(communities)
}

// ─── partition quality (real; backend-independent) ───────────────────────────────

/// Newman–Girvan modularity `Q_γ` of a partition over the CALLS/IMPORTS graph, treated as
/// undirected and unweighted.
///
/// `Q_γ = Σ_c [ l_c / m − γ · (d_c / 2m)² ]` where `m` is the edge count, `l_c` the edges interior
/// to community `c`, and `d_c` the total degree of `c`'s members. Only edges whose **both**
/// endpoints appear in `communities` contribute. Returns `0.0` for an empty graph. Range is roughly
/// `[-0.5, 1.0]`; a good partition scores `> 0.3`.
pub fn modularity(
    store: &dyn GraphRead,
    communities: &[Vec<SymbolId>],
    resolution: f64,
) -> Result<f64> {
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
            scope: Default::default(),
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

    /// Build a store from node names and (src,tgt) call edges.
    fn store_of(names: &[&str], edges: &[(&str, &str)]) -> MemStore {
        let mut s = MemStore::new();
        for n in names {
            s.upsert_nodes(&[make_node(n)]).unwrap();
        }
        let es: Vec<Edge> = edges.iter().map(|&(a, b)| calls(a, b)).collect();
        s.upsert_edges(&es).unwrap();
        s
    }

    /// A clique over `names` (all undirected pairs as call edges).
    fn clique(names: &[&str]) -> Vec<(String, String)> {
        let mut e = Vec::new();
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                e.push((names[i].to_string(), names[j].to_string()));
            }
        }
        e
    }

    // ── union-find comparison helper (was the old backend) ────────────────────────

    fn connected_components(store: &MemStore, min_size: usize) -> Vec<Vec<SymbolId>> {
        let nodes = store.all_nodes().unwrap();
        let edges = store.all_edges().unwrap();
        let idx: HashMap<SymbolId, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.symbol.clone(), i))
            .collect();
        let mut parent: Vec<usize> = (0..nodes.len()).collect();
        fn find(p: &mut [usize], x: usize) -> usize {
            let mut r = x;
            while p[r] != r {
                r = p[r];
            }
            let mut c = x;
            while p[c] != r {
                let n = p[c];
                p[c] = r;
                c = n;
            }
            r
        }
        let mut touched = vec![false; nodes.len()];
        for e in &edges {
            if !matches!(e.kind, EdgeKind::Calls | EdgeKind::Imports) {
                continue;
            }
            if let (Some(&a), Some(&b)) = (idx.get(&e.source), idx.get(&e.target)) {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
                touched[a] = true;
                touched[b] = true;
            }
        }
        let mut groups: HashMap<usize, Vec<SymbolId>> = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            if !touched[i] {
                continue;
            }
            let r = find(&mut parent, i);
            groups.entry(r).or_default().push(n.symbol.clone());
        }
        groups
            .into_values()
            .filter(|c| c.len() >= min_size)
            .collect()
    }

    // ── basic behaviour (carried from the union-find era) ─────────────────────────

    #[test]
    fn detect_communities_two_components() {
        let s = store_of(
            &["A", "B", "C", "D", "E", "F"],
            &[("A", "B"), ("B", "C"), ("D", "E")],
        );
        let c = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(c.len(), 2, "expected 2 communities, got {}", c.len());
        let sizes: Vec<usize> = c.iter().map(|x| x.len()).collect();
        assert_eq!(sizes, vec![3, 2], "sizes largest-first");
    }

    #[test]
    fn detect_communities_min_size_filter() {
        let s = store_of(
            &["A", "B", "C", "D", "E", "F"],
            &[("A", "B"), ("B", "C"), ("D", "E")],
        );
        let c = detect_communities(&s, &CommunityParams::new(3, false)).unwrap();
        assert_eq!(c.len(), 1, "only one community has size >= 3");
        assert_eq!(c[0].len(), 3);
    }

    #[test]
    fn detect_communities_empty_graph() {
        let s = MemStore::new();
        let c = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        assert!(c.is_empty());
    }

    // ── the headline fix: connected graph does NOT collapse to one community ──────

    #[test]
    fn two_cliques_one_bridge_splits() {
        // Two 4-cliques joined by ONE bridge edge (A3—B0).
        let names = ["A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3"];
        let mut edges = clique(&["A0", "A1", "A2", "A3"]);
        edges.extend(clique(&["B0", "B1", "B2", "B3"]));
        edges.push(("A3".into(), "B0".into()));
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&names, &edge_refs);

        // Union-find merges everything (it's connected) → 1 community.
        let uf = connected_components(&s, 2);
        assert_eq!(
            uf.len(),
            1,
            "connected-components must merge the bridged cliques"
        );

        // Louvain splits into the two cliques.
        let lv = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(
            lv.len(),
            2,
            "Louvain must split bridged cliques into 2, got {}",
            lv.len()
        );
        assert!(max_community_fraction(&lv) < 0.75);
    }

    #[test]
    fn louvain_modularity_ge_unionfind() {
        let names = ["A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3"];
        let mut edges = clique(&["A0", "A1", "A2", "A3"]);
        edges.extend(clique(&["B0", "B1", "B2", "B3"]));
        edges.push(("A3".into(), "B0".into()));
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&names, &edge_refs);

        let lv = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        let uf = connected_components(&s, 2);
        let q_lv = modularity(&s, &lv, 1.0).unwrap();
        let q_uf = modularity(&s, &uf, 1.0).unwrap();
        assert!(
            q_lv >= q_uf - 1e-9,
            "Louvain modularity {q_lv} must be >= union-find {q_uf}"
        );
    }

    #[test]
    fn resolution_controls_granularity() {
        let names = ["A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3"];
        let mut edges = clique(&["A0", "A1", "A2", "A3"]);
        edges.extend(clique(&["B0", "B1", "B2", "B3"]));
        edges.push(("A3".into(), "B0".into()));
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&names, &edge_refs);

        let lo = detect_communities(
            &s,
            &CommunityParams {
                resolution: 1.0,
                ..CommunityParams::new(2, false)
            },
        )
        .unwrap();
        let hi = detect_communities(
            &s,
            &CommunityParams {
                resolution: 2.0,
                ..CommunityParams::new(2, false)
            },
        )
        .unwrap();
        assert!(
            hi.len() >= lo.len(),
            "higher resolution must not yield fewer communities ({} vs {})",
            hi.len(),
            lo.len()
        );
    }

    #[test]
    fn star_graph_hub_not_isolated() {
        let leaves: Vec<String> = (0..10).map(|i| format!("leaf{i}")).collect();
        let mut names = vec!["hub".to_string()];
        names.extend(leaves.iter().cloned());
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let edges: Vec<(&str, &str)> = leaves.iter().map(|l| ("hub", l.as_str())).collect();
        let s = store_of(&name_refs, &edges);

        let c = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        let hub_comm = c.iter().find(|cm| cm.iter().any(|x| x.0 == "hub"));
        assert!(hub_comm.is_some(), "hub must be in a returned community");
        assert!(
            hub_comm.unwrap().len() >= 2,
            "hub must share a community with >=1 leaf"
        );
    }

    #[test]
    fn mega_community_gate_ring_of_cliques() {
        // 5 cliques of 5, joined in a ring by single bridges → connected, but clustered.
        let mut names: Vec<String> = Vec::new();
        let mut edges: Vec<(String, String)> = Vec::new();
        for c in 0..5 {
            let members: Vec<String> = (0..5).map(|i| format!("c{c}_n{i}")).collect();
            names.extend(members.iter().cloned());
            edges.extend(clique(
                &members.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            ));
        }
        // ring bridges: c0_n0—c1_n0—...—c4_n0—c0_n0
        for c in 0..5 {
            edges.push((format!("c{c}_n0"), format!("c{}_n0", (c + 1) % 5)));
        }
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&name_refs, &edge_refs);

        let c = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        let frac = max_community_fraction(&c);
        assert!(
            frac < 0.30,
            "mega-community gate: max fraction {frac} must be < 0.30"
        );
    }

    #[test]
    fn refine_splits_two_triangles() {
        // The refinement primitive: a community that is two triangles joined by one bridge must
        // split into the two triangles. This is the mechanism hierarchical mode applies to a
        // flat community that still has internal structure.
        let edges = vec![
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (3, 4, 1.0),
            (3, 5, 1.0),
            (4, 5, 1.0),
            (2, 3, 1.0), // bridge
        ];
        let members: Vec<usize> = (0..6).collect();
        let sub = refine(&edges, &members, 1.0);
        assert_eq!(
            sub.len(),
            2,
            "refine must split two bridged triangles into 2"
        );
        let mut sizes: Vec<usize> = sub.iter().map(|g| g.len()).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, vec![3, 3]);
    }

    #[test]
    fn hierarchical_is_safe_no_shatter() {
        // Ring of 5 cliques: flat Louvain is already optimal, so each clique would only *shatter*
        // at the higher refinement resolution. The shatter guard must keep communities whole:
        // hierarchical must not drop nodes, must not grow the largest community, and must not
        // collapse to nothing.
        let mut names: Vec<String> = Vec::new();
        let mut edges: Vec<(String, String)> = Vec::new();
        for c in 0..5 {
            let members: Vec<String> = (0..5).map(|i| format!("c{c}_n{i}")).collect();
            names.extend(members.iter().cloned());
            edges.extend(clique(
                &members.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            ));
        }
        for c in 0..5 {
            edges.push((format!("c{c}_n0"), format!("c{}_n0", (c + 1) % 5)));
        }
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&name_refs, &edge_refs);

        let flat = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        let hier = detect_communities(
            &s,
            &CommunityParams {
                hierarchical: true,
                ..CommunityParams::new(2, false)
            },
        )
        .unwrap();

        let flat_nodes: usize = flat.iter().map(|c| c.len()).sum();
        let hier_nodes: usize = hier.iter().map(|c| c.len()).sum();
        assert_eq!(
            hier_nodes, flat_nodes,
            "hierarchical must not drop nodes by shattering"
        );
        let flat_max = flat.iter().map(|c| c.len()).max().unwrap_or(0);
        let hier_max = hier.iter().map(|c| c.len()).max().unwrap_or(0);
        assert!(
            hier_max <= flat_max,
            "hierarchical must not grow the largest community"
        );
        assert!(
            !hier.is_empty(),
            "hierarchical must not collapse to nothing"
        );
        assert!(
            max_community_fraction(&hier) < 0.30,
            "mega-community gate holds under hierarchical"
        );
    }

    // ── package bias ──────────────────────────────────────────────────────────────

    #[test]
    fn package_bias_does_not_merge_disconnected_cliques() {
        // Two cliques, no cross edges, both in the same directory. Bias must NOT merge them
        // (they're structurally separate; package membership only biases, never forces).
        let mut s = MemStore::new();
        for n in ["A0", "A1", "A2", "B0", "B1", "B2"] {
            s.upsert_nodes(&[make_node_in(n, "src/pkg/mod.rs")])
                .unwrap();
        }
        let a = clique(&["A0", "A1", "A2"]);
        let b = clique(&["B0", "B1", "B2"]);
        let es: Vec<Edge> = a.iter().chain(b.iter()).map(|(x, y)| calls(x, y)).collect();
        s.upsert_edges(&es).unwrap();

        let c = detect_communities(
            &s,
            &CommunityParams {
                package_bias: 0.5,
                ..CommunityParams::new(2, false)
            },
        )
        .unwrap();
        assert_eq!(
            c.len(),
            2,
            "package bias must not merge two disconnected cliques"
        );
    }

    #[test]
    fn package_bias_keeps_bridged_cliques_separate() {
        // Two cliques + 1 cross edge, same package, modest bias → still 2 communities
        // (membership ≠ forced merge).
        let mut s = MemStore::new();
        for n in ["A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3"] {
            s.upsert_nodes(&[make_node_in(n, "src/pkg/mod.rs")])
                .unwrap();
        }
        let mut edges = clique(&["A0", "A1", "A2", "A3"]);
        edges.extend(clique(&["B0", "B1", "B2", "B3"]));
        edges.push(("A3".into(), "B0".into()));
        let es: Vec<Edge> = edges.iter().map(|(x, y)| calls(x, y)).collect();
        s.upsert_edges(&es).unwrap();

        let c = detect_communities(
            &s,
            &CommunityParams {
                package_bias: 0.3,
                ..CommunityParams::new(2, false)
            },
        )
        .unwrap();
        assert_eq!(c.len(), 2, "modest package bias must not force a merge");
    }

    // ── modularity / max_community_fraction unit tests ────────────────────────────

    #[test]
    fn modularity_two_cliques_is_high() {
        let mut edges = clique(&["A", "B", "C"]);
        edges.extend(clique(&["D", "E", "F"]));
        let name_refs = ["A", "B", "C", "D", "E", "F"];
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&name_refs, &edge_refs);
        let c = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        let q = modularity(&s, &c, 1.0).unwrap();
        assert!(q > 0.3, "two-clique modularity must exceed 0.3, got {q}");
    }

    #[test]
    fn modularity_good_beats_bad_partition() {
        let mut edges = clique(&["A", "B", "C"]);
        edges.extend(clique(&["D", "E", "F"]));
        let name_refs = ["A", "B", "C", "D", "E", "F"];
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&name_refs, &edge_refs);
        let good = vec![
            vec![sym("A"), sym("B"), sym("C")],
            vec![sym("D"), sym("E"), sym("F")],
        ];
        let bad = vec![vec![
            sym("A"),
            sym("B"),
            sym("C"),
            sym("D"),
            sym("E"),
            sym("F"),
        ]];
        assert!(modularity(&s, &good, 1.0).unwrap() > modularity(&s, &bad, 1.0).unwrap());
    }

    #[test]
    fn max_community_fraction_flags_mega_community() {
        let big: Vec<SymbolId> = (0..9).map(|i| sym(&format!("n{i}"))).collect();
        let small = vec![sym("solo")];
        let frac = max_community_fraction(&[big, small]);
        assert!((frac - 0.9).abs() < 1e-9, "expected 0.9, got {frac}");
        assert_eq!(max_community_fraction(&[]), 0.0);
    }

    // ── determinism ─────────────────────────────────────────────────────────────

    #[test]
    fn deterministic_output() {
        let names = ["A0", "A1", "A2", "A3", "B0", "B1", "B2", "B3"];
        let mut edges = clique(&["A0", "A1", "A2", "A3"]);
        edges.extend(clique(&["B0", "B1", "B2", "B3"]));
        edges.push(("A3".into(), "B0".into()));
        let edge_refs: Vec<(&str, &str)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let s = store_of(&names, &edge_refs);
        let r1 = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        let r2 = detect_communities(&s, &CommunityParams::new(2, false)).unwrap();
        assert_eq!(r1, r2, "Louvain output must be deterministic");
    }
}
