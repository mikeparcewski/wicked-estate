//! `path_between` against real backends.
//!
//! The seam lives in `wicked-estate-core`, which has no store dependency, so its tests live
//! here — the first crate that has both `MemStore` and `SqliteStore`.
//!
//! Several tests run the *same graph* through both stores and compare. That is not
//! belt-and-braces: the two backends induce different frontier edges and order
//! `find_symbols` differently, so a seam that leaned on either would answer differently
//! depending on which store the caller opened.

use wicked_estate_core::path::{Unresolved, path_between};
use wicked_estate_core::{
    Direction, Edge, EdgeKind, GraphRead, GraphStats, Language, Location, Node, NodeKind,
    ResolutionTier, Span, Subgraph, SymbolId, SymbolQuery, TraversalSpec,
};
use wicked_estate_store::{GraphStoreMutExt, MemStore, SqliteStore};

fn sym(s: &str) -> SymbolId {
    SymbolId(s.into())
}

/// A node whose NAME is `name` and whose id is `id` — distinct, so a multi-match name
/// (two ids sharing one name) is expressible.
fn named_node(id: &str, name: &str) -> Node {
    Node::new(
        sym(id),
        NodeKind::Function,
        name,
        Language::new("rust"),
        Location::new("src/lib.rs", Span::ZERO),
    )
}

fn node(id: &str) -> Node {
    named_node(id, id)
}

fn edge(source: &str, target: &str) -> Edge {
    Edge::new(
        sym(source),
        sym(target),
        EdgeKind::Calls,
        ResolutionTier::Parsed,
        "test",
    )
}

fn load(store: &mut dyn GraphStoreMutExt, nodes: &[Node], edges: &[Edge]) {
    store.begin_batch().expect("begin");
    store.upsert_nodes(nodes).expect("nodes");
    store.upsert_edges(edges).expect("edges");
    store.commit_batch().expect("commit");
}

/// A straight chain `a0 → a1 → … → an`, each node's name equal to its id.
fn chain(len: usize) -> (Vec<Node>, Vec<Edge>) {
    let nodes: Vec<Node> = (0..=len).map(|i| node(&format!("a{i}"))).collect();
    let edges: Vec<Edge> = (0..len)
        .map(|i| edge(&format!("a{i}"), &format!("a{}", i + 1)))
        .collect();
    (nodes, edges)
}

fn mem(nodes: &[Node], edges: &[Edge]) -> MemStore {
    let mut s = MemStore::new();
    load(&mut s, nodes, edges);
    s
}

fn sqlite(nodes: &[Node], edges: &[Edge]) -> SqliteStore {
    let mut s = SqliteStore::in_memory().expect("open sqlite");
    load(&mut s, nodes, edges);
    s
}

fn hop_ids(hops: &[Edge]) -> Vec<(String, String)> {
    hops.iter()
        .map(|e| (e.source.0.clone(), e.target.0.clone()))
        .collect()
}

// ── the basic route ──────────────────────────────────────────────────────────

#[test]
fn chain_returns_two_hops_with_confidence_intact() {
    let (nodes, edges) = chain(2);
    let store = mem(&nodes, &edges);
    let r = path_between(&store, "a0", "a2", 8, 5_000).expect("query");
    assert!(r.found);
    assert_eq!(
        hop_ids(&r.hops),
        vec![
            ("a0".to_string(), "a1".to_string()),
            ("a1".to_string(), "a2".to_string())
        ]
    );
    for hop in &r.hops {
        assert_eq!(
            hop.confidence.get(),
            ResolutionTier::Parsed.default_confidence().get(),
            "each hop keeps the confidence its resolution tier gave it (R7)"
        );
        assert!(!hop.resolved_by.is_empty(), "provenance rides on every hop");
    }
}

// ── one traversal per resolved start, never one per node ─────────────────────

/// Delegates every read to an inner store while counting the two calls that matter.
struct CountingStore<'a> {
    inner: &'a MemStore,
    traverse_calls: std::cell::Cell<usize>,
    neighbor_calls: std::cell::Cell<usize>,
}

impl GraphRead for CountingStore<'_> {
    fn capabilities(&self) -> wicked_estate_core::StoreCapabilities {
        self.inner.capabilities()
    }
    fn get_node(&self, id: &SymbolId) -> wicked_estate_core::Result<Option<Node>> {
        self.inner.get_node(id)
    }
    fn find_symbols(&self, q: &SymbolQuery) -> wicked_estate_core::Result<Vec<Node>> {
        self.inner.find_symbols(q)
    }
    fn neighbors(&self, id: &SymbolId, dir: Direction) -> wicked_estate_core::Result<Vec<Edge>> {
        self.neighbor_calls.set(self.neighbor_calls.get() + 1);
        self.inner.neighbors(id, dir)
    }
    fn traverse(
        &self,
        start: &SymbolId,
        spec: &TraversalSpec,
    ) -> wicked_estate_core::Result<Subgraph> {
        self.traverse_calls.set(self.traverse_calls.get() + 1);
        self.inner.traverse(start, spec)
    }
    fn all_nodes(&self) -> wicked_estate_core::Result<Vec<Node>> {
        self.inner.all_nodes()
    }
    fn all_edges(&self) -> wicked_estate_core::Result<Vec<Edge>> {
        self.inner.all_edges()
    }
    fn unresolved_refs_for_name(
        &self,
        name: &str,
    ) -> wicked_estate_core::Result<Vec<wicked_estate_core::UnresolvedRef>> {
        self.inner.unresolved_refs_for_name(name)
    }
    fn file_digest(&self, f: &str) -> wicked_estate_core::Result<Option<String>> {
        self.inner.file_digest(f)
    }
    fn indexed_files(&self) -> wicked_estate_core::Result<Vec<String>> {
        // MemStore has an inherent `indexed_files` that shadows the trait method.
        GraphRead::indexed_files(self.inner)
    }
    fn file_git_sha(&self, f: &str) -> wicked_estate_core::Result<Option<String>> {
        self.inner.file_git_sha(f)
    }
    fn repo_info(&self) -> wicked_estate_core::Result<Option<wicked_estate_core::RepoInfo>> {
        self.inner.repo_info()
    }
    fn edge_history(
        &self,
        f: &str,
    ) -> wicked_estate_core::Result<Vec<wicked_estate_core::HistoricalEdge>> {
        self.inner.edge_history(f)
    }
    fn file_content(&self, f: &str) -> wicked_estate_core::Result<Option<String>> {
        self.inner.file_content(f)
    }
    fn symbol_source(&self, n: &Node) -> wicked_estate_core::Result<Option<String>> {
        self.inner.symbol_source(n)
    }
    fn changes_since(&self, c: u64) -> wicked_estate_core::Result<Vec<wicked_estate_core::Change>> {
        self.inner.changes_since(c)
    }
    fn node_semantics(
        &self,
        s: &SymbolId,
    ) -> wicked_estate_core::Result<Option<wicked_estate_core::NodeSemantics>> {
        self.inner.node_semantics(s)
    }
    fn find_by_requirement(&self, r: &str) -> wicked_estate_core::Result<Vec<Node>> {
        self.inner.find_by_requirement(r)
    }
    fn annotations(
        &self,
        s: &SymbolId,
    ) -> wicked_estate_core::Result<Vec<wicked_estate_core::Annotation>> {
        self.inner.annotations(s)
    }
    fn annotations_by_type(
        &self,
        t: &str,
    ) -> wicked_estate_core::Result<Vec<(SymbolId, wicked_estate_core::Annotation)>> {
        self.inner.annotations_by_type(t)
    }
    fn annotations_stale_since(
        &self,
        c: i64,
    ) -> wicked_estate_core::Result<Vec<(SymbolId, wicked_estate_core::Annotation)>> {
        self.inner.annotations_stale_since(c)
    }
    fn symbol_epoch(&self, id: &SymbolId) -> wicked_estate_core::Result<Option<u64>> {
        self.inner.symbol_epoch(id)
    }
    fn edge_supports(
        &self,
        source: &SymbolId,
        target: &SymbolId,
        kind: &wicked_estate_core::EdgeKind,
    ) -> wicked_estate_core::Result<Vec<wicked_estate_core::EdgeSupport>> {
        self.inner.edge_supports(source, target, kind)
    }
    fn support_generation(
        &self,
        owner: &wicked_estate_core::SupportOwner,
    ) -> wicked_estate_core::Result<Option<u64>> {
        self.inner.support_generation(owner)
    }
    fn support_owners(
        &self,
    ) -> wicked_estate_core::Result<Vec<wicked_estate_core::SupportOwnerState>> {
        self.inner.support_owners()
    }
    fn stats(&self) -> wicked_estate_core::Result<GraphStats> {
        self.inner.stats()
    }
}

#[test]
fn single_match_name_issues_exactly_one_traverse_and_no_neighbors() {
    let (nodes, edges) = chain(3);
    let inner = mem(&nodes, &edges);
    let counting = CountingStore {
        inner: &inner,
        traverse_calls: std::cell::Cell::new(0),
        neighbor_calls: std::cell::Cell::new(0),
    };
    let r = path_between(&counting, "a0", "a3", 8, 5_000).expect("query");
    assert!(r.found);
    assert_eq!(
        counting.traverse_calls.get(),
        1,
        "one resolved start symbol must cost exactly one traversal"
    );
    assert_eq!(
        counting.neighbor_calls.get(),
        0,
        "a per-node neighbors() walk is the N-statements-per-node anti-pattern"
    );
}

// ── multi-match on either side ───────────────────────────────────────────────

/// Two symbols share the name `start`; only one of them reaches the target.
#[test]
fn multi_match_from_returns_the_route_that_exists() {
    let nodes = vec![
        named_node("s_reaches", "start"),
        named_node("s_dead_end", "start"),
        node("mid"),
        node("goal"),
    ];
    let edges = vec![edge("s_reaches", "mid"), edge("mid", "goal")];
    let store = mem(&nodes, &edges);
    let r = path_between(&store, "start", "goal", 8, 5_000).expect("query");
    assert!(r.found);
    assert_eq!(r.hops.len(), 2);
    assert_eq!(r.hops[0].source, sym("s_reaches"));
}

/// Two symbols share the name `goal`, at different distances; the nearer wins, stably.
#[test]
fn multi_match_to_picks_the_nearer_and_is_stable() {
    let nodes = vec![
        node("start"),
        node("mid"),
        named_node("g_near", "goal"),
        named_node("g_far", "goal"),
    ];
    let edges = vec![
        edge("start", "g_near"),
        edge("start", "mid"),
        edge("mid", "g_far"),
    ];
    let store = mem(&nodes, &edges);
    let first = path_between(&store, "start", "goal", 8, 5_000).expect("query");
    let second = path_between(&store, "start", "goal", 8, 5_000).expect("query");
    assert_eq!(first.hops.len(), 1, "the nearer candidate wins");
    assert_eq!(first.hops[0].target, sym("g_near"));
    assert_eq!(hop_ids(&first.hops), hop_ids(&second.hops), "and stably");
}

// ── resolution: name, then id, then honest failure ───────────────────────────

#[test]
fn symbol_id_resolves_through_the_id_fallback() {
    // The node's NAME is "display", but an agent holding its id passes the id.
    let nodes = vec![named_node("mod::display#1", "display"), node("goal")];
    let edges = vec![edge("mod::display#1", "goal")];
    let store = mem(&nodes, &edges);

    let by_name = path_between(&store, "display", "goal", 8, 5_000).expect("query");
    let by_id = path_between(&store, "mod::display#1", "goal", 8, 5_000).expect("query");
    assert!(
        by_id.found,
        "a SymbolId must resolve when it names no symbol"
    );
    assert_eq!(hop_ids(&by_name.hops), hop_ids(&by_id.hops));
}

#[test]
fn unresolvable_from_reports_which_side_failed() {
    let (nodes, edges) = chain(2);
    let store = mem(&nodes, &edges);
    let r = path_between(&store, "no_such_symbol", "a2", 8, 5_000).expect("query");
    assert!(!r.found);
    assert!(r.hops.is_empty());
    assert_eq!(
        r.unresolved,
        Some(Unresolved::From),
        "an unresolvable input must not read like a proven absence (R3)"
    );
}

#[test]
fn unresolvable_to_reports_the_to_side() {
    let (nodes, edges) = chain(2);
    let store = mem(&nodes, &edges);
    let r = path_between(&store, "a0", "no_such_symbol", 8, 5_000).expect("query");
    assert_eq!(r.unresolved, Some(Unresolved::To));
}

#[test]
fn resolvable_pair_with_no_route_is_a_proven_absence() {
    let nodes = vec![node("x"), node("y")];
    let store = mem(&nodes, &[]);
    let r = path_between(&store, "x", "y", 8, 5_000).expect("query");
    assert!(!r.found);
    assert_eq!(r.unresolved, None, "both ends exist; nothing connects them");
    assert!(
        !r.depth_bounded && !r.node_bounded,
        "and nothing was cut off"
    );
}

// ── endpoints: rendered without a store lookup ───────────────────────────────

#[test]
fn endpoints_covers_both_ends_of_every_hop() {
    let (nodes, edges) = chain(3);
    let store = mem(&nodes, &edges);
    let r = path_between(&store, "a0", "a3", 8, 5_000).expect("query");
    assert!(r.found);
    let have: std::collections::HashSet<&str> =
        r.endpoints.iter().map(|n| n.symbol.as_str()).collect();
    for hop in &r.hops {
        assert!(
            have.contains(hop.source.as_str()),
            "hop source {} missing from endpoints — the renderer would fall back to get_node",
            hop.source.0
        );
        assert!(
            have.contains(hop.target.as_str()),
            "hop target {} missing from endpoints",
            hop.target.0
        );
    }
}

// ── the bound flags ──────────────────────────────────────────────────────────

/// D1b's falsifier pair, re-pointed by #230 at the EXACT cause (`Subgraph::depth_horizon_reached`,
/// wicked-estate#222): `depth_bounded` is true only when something lay BEYOND the horizon —
/// whether or not a route was found — and false when the walk saw everything, even if a node sat
/// exactly at the bound. The old "frontier touched" heuristic reported a found 3-hop route at
/// depth 3 as bounded; an implementation computing "no route and the frontier was reached" still
/// fails the found-and-bounded case. Both backends agree.
#[test]
fn depth_bounded_is_exact_true_only_when_something_lies_beyond_the_horizon() {
    let (nodes, edges) = chain(4);
    for (label, r) in [
        (
            "mem",
            path_between(&mem(&nodes, &edges), "a0", "a3", 3, 5_000).expect("mem"),
        ),
        (
            "sqlite",
            path_between(&sqlite(&nodes, &edges), "a0", "a3", 3, 5_000).expect("sqlite"),
        ),
    ] {
        assert!(r.found, "{label}: a 3-hop route is reachable at depth 3");
        assert_eq!(r.hops.len(), 3, "{label}");
        assert!(
            r.depth_bounded,
            "{label}: a4 lies beyond the horizon, so the flag is true despite the found route"
        );
    }
    let (nodes, edges) = chain(3);
    for (label, r) in [
        (
            "mem",
            path_between(&mem(&nodes, &edges), "a0", "a3", 3, 5_000).expect("mem"),
        ),
        (
            "sqlite",
            path_between(&sqlite(&nodes, &edges), "a0", "a3", 3, 5_000).expect("sqlite"),
        ),
    ] {
        assert!(r.found, "{label}");
        assert!(
            !r.depth_bounded,
            "{label}: a3 sits AT the bound but nothing lies beyond it — the walk saw everything"
        );
    }
    let (nodes, edges) = chain(3);
    let inside = path_between(&mem(&nodes, &edges), "a0", "a3", 8, 5_000).expect("query");
    assert!(inside.found && !inside.depth_bounded);
}

/// #230: both operands resolving to one candidate is a zero-hop route decided BEFORE any
/// traverse — no bound can be set, and the endpoint is the node itself.
#[test]
fn a_shared_candidate_is_a_zero_hop_route_with_no_bounds() {
    let (nodes, edges) = chain(2);
    let r = path_between(&mem(&nodes, &edges), "a1", "a1", 1, 1).expect("query");
    assert!(r.found && r.hops.is_empty(), "{r:?}");
    assert!(!r.depth_bounded && !r.node_bounded, "{r:?}");
    assert_eq!(r.endpoints.len(), 1);
    assert_eq!(r.endpoints[0].symbol.as_str(), "a1");
}

/// #228 item 2: `path_between` against LIVE Postgres, in the `postgres-conformance` job (which
/// runs this crate's whole suite under `--features postgres`). Skips without `TEST_POSTGRES_URL`.
/// Ids carry a per-run tag so a shared database never hands this run another run's rows.
#[cfg(feature = "postgres")]
#[test]
fn path_between_on_postgres_matches_the_embedded_backends() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) if !u.is_empty() => u,
        _ => {
            eprintln!("path_backends: TEST_POSTGRES_URL not set — skipping the Postgres case");
            return;
        }
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tag = format!("pb{}-{nanos}", std::process::id());
    let id = |i: usize| format!("{tag}-a{i}");
    let nodes: Vec<Node> = (0..=4).map(|i| named_node(&id(i), &id(i))).collect();
    let edges: Vec<Edge> = (0..4).map(|i| edge(&id(i), &id(i + 1))).collect();
    let mut pg = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    load(&mut pg, &nodes, &edges);
    let sq = sqlite(&nodes, &edges);

    let p = path_between(&pg, &id(0), &id(3), 3, 5_000).expect("postgres");
    let s = path_between(&sq, &id(0), &id(3), 3, 5_000).expect("sqlite");
    assert!(p.found && s.found, "postgres={p:?} sqlite={s:?}");
    assert_eq!(
        hop_ids(&p.hops),
        hop_ids(&s.hops),
        "the route is backend-independent"
    );
    assert_eq!(
        (p.depth_bounded, p.node_bounded),
        (s.depth_bounded, s.node_bounded),
        "the bound flags agree: postgres={p:?} sqlite={s:?}"
    );
    assert!(p.depth_bounded, "a4 lies beyond depth 3 on Postgres too");

    let absent = path_between(&pg, &id(0), &id(4), 2, 5_000).expect("postgres");
    assert!(!absent.found && absent.depth_bounded, "{absent:?}");

    // The other half of the exactness claim: a route found AT the bound with nothing beyond it
    // is not bounded — a second, shorter chain so the first one's a4 cannot sit past the horizon.
    let id3 = |i: usize| format!("{tag}-b{i}");
    let nodes3: Vec<Node> = (0..=3).map(|i| named_node(&id3(i), &id3(i))).collect();
    let edges3: Vec<Edge> = (0..3).map(|i| edge(&id3(i), &id3(i + 1))).collect();
    load(&mut pg, &nodes3, &edges3);
    let exact = path_between(&pg, &id3(0), &id3(3), 3, 5_000).expect("postgres");
    assert!(
        exact.found && !exact.depth_bounded,
        "b3 sits AT the bound with nothing beyond it: {exact:?}"
    );
}

#[test]
fn chain_longer_than_max_depth_is_a_bounded_absence_on_both_backends() {
    let (nodes, edges) = chain(6);
    let m = path_between(&mem(&nodes, &edges), "a0", "a6", 3, 5_000).expect("mem");
    let s = path_between(&sqlite(&nodes, &edges), "a0", "a6", 3, 5_000).expect("sqlite");

    for (label, r) in [("mem", &m), ("sqlite", &s)] {
        assert!(
            !r.found,
            "{label}: a 6-hop chain is out of reach at depth 3"
        );
        assert!(r.hops.is_empty(), "{label}");
        assert!(
            r.depth_bounded,
            "{label}: the absence is bounded, not proven"
        );
    }
    assert_eq!(hop_ids(&m.hops), hop_ids(&s.hops), "hops must match");
    assert_eq!(m.found, s.found, "found must match");
    assert_eq!(
        m.depth_bounded, s.depth_bounded,
        "depth_bounded must match across backends"
    );
}

/// The shape on which the two stores' own candidate ordering diverges: a multi-match name
/// with two equally short routes, inserted so SQLite's autoincrement row order differs from
/// `SymbolId` string order. Without the seam's own sort, each backend picks a different
/// winner and this is the only test that notices.
#[test]
fn multi_match_equal_routes_agree_across_backends() {
    // Insertion order puts "zzz" first, so its sid is lower than "aaa"'s — the reverse of
    // string order.
    let nodes = vec![
        named_node("zzz_start", "start"),
        named_node("aaa_start", "start"),
        node("goal"),
    ];
    let edges = vec![edge("zzz_start", "goal"), edge("aaa_start", "goal")];

    let m = path_between(&mem(&nodes, &edges), "start", "goal", 8, 5_000).expect("mem");
    let s = path_between(&sqlite(&nodes, &edges), "start", "goal", 8, 5_000).expect("sqlite");

    assert!(m.found && s.found);
    assert_eq!(m.hops.len(), 1);
    assert_eq!(
        hop_ids(&m.hops),
        hop_ids(&s.hops),
        "two equally short routes must resolve to the same one on both backends; \
         MemStore sorts find_symbols by symbol string while SqliteStore orders by row id, \
         so without the seam's own candidate sort these diverge"
    );
}

#[test]
fn node_budget_exhaustion_sets_node_bounded() {
    // A hub with many leaves: a depth-1 walk reaches more nodes than the budget allows.
    let mut nodes = vec![node("hub")];
    let mut edges = Vec::new();
    for i in 0..40 {
        nodes.push(node(&format!("leaf{i}")));
        edges.push(edge("hub", &format!("leaf{i}")));
    }
    nodes.push(node("unrelated"));
    let store = mem(&nodes, &edges);

    let r = path_between(&store, "hub", "unrelated", 4, 5).expect("query");
    assert!(!r.found);
    assert!(
        r.node_bounded,
        "the node budget bound the walk, so the absence is not proven"
    );
}

/// Both flags are the disjunction across candidate traversals, not the winning candidate's
/// value. An implementation folding one with `|=` and reading the other off the winner
/// passes every other test in this file while emitting a proven-absence signal for a
/// bounded search (R3).
#[test]
fn depth_bounded_is_a_disjunction_across_candidates() {
    // Two symbols named "start". One sits on a long chain (its walk hits the frontier);
    // the other is isolated. Neither reaches "goal". The bounded candidate sorts FIRST so
    // that a "take the last candidate's flag" implementation answers false and this test
    // fails — otherwise it would pass by luck and prove nothing.
    let mut nodes = vec![
        named_node("a_long", "start"),
        named_node("z_alone", "start"),
    ];
    let mut edges = Vec::new();
    let mut prev = "a_long".to_string();
    for i in 0..6 {
        let next = format!("c{i}");
        nodes.push(node(&next));
        edges.push(edge(&prev, &next));
        prev = next;
    }
    nodes.push(node("goal"));
    let store = mem(&nodes, &edges);

    let r = path_between(&store, "start", "goal", 3, 5_000).expect("query");
    assert!(!r.found);
    assert!(
        r.depth_bounded,
        "one candidate's walk reached its frontier; reporting only the other candidate's \
         flag would present a bounded absence as proven"
    );
}

#[test]
fn node_bounded_is_a_disjunction_across_candidates() {
    // Two symbols named "start". One is a hub whose walk exhausts a tiny node budget; the
    // other is isolated. Neither reaches "goal". The bounded candidate sorts FIRST, for the
    // same falsifier reason as the depth_bounded case.
    let mut nodes = vec![named_node("a_hub", "start"), named_node("z_alone", "start")];
    let mut edges = Vec::new();
    for i in 0..40 {
        nodes.push(node(&format!("leaf{i}")));
        edges.push(edge("a_hub", &format!("leaf{i}")));
    }
    nodes.push(node("goal"));
    let store = mem(&nodes, &edges);

    let r = path_between(&store, "start", "goal", 4, 5).expect("query");
    assert!(!r.found);
    assert!(
        r.node_bounded,
        "one candidate's walk was node-capped; the flag is a disjunction, not the winner's"
    );
}

/// Endpoint completeness on **SqliteStore**, on a route that terminates at the depth
/// frontier — the shape the admission rule exists for.
///
/// The MemStore version of this test cannot catch a SQLite-side regression: SqliteStore is
/// the backend that induces edges to symbols it does not return as nodes, so it is the one
/// where a missing endpoint would ship the bare-id fallback — a hop with no name, file or
/// line, which is the whole value of the feature.
#[test]
fn endpoints_are_complete_on_sqlite_at_the_depth_frontier() {
    let (nodes, edges) = chain(6);
    let store = sqlite(&nodes, &edges);
    // depth 3 on a 6-hop chain: the route to a3 ends exactly at the frontier, and the
    // traversal has induced edges beyond it.
    let r = path_between(&store, "a0", "a3", 3, 5_000).expect("query");
    assert!(r.found, "a3 is three hops out");
    assert!(r.depth_bounded, "the walk reached its frontier");

    let have: std::collections::HashSet<&str> =
        r.endpoints.iter().map(|n| n.symbol.as_str()).collect();
    for hop in &r.hops {
        assert!(
            have.contains(hop.source.as_str()) && have.contains(hop.target.as_str()),
            "every hop endpoint must be in `endpoints` on SqliteStore too; missing one \
             ships a hop with no name, file or line"
        );
    }
    for n in &r.endpoints {
        assert!(!n.name.is_empty(), "endpoint nodes carry their name");
        assert!(!n.location.file.is_empty(), "and their file");
    }
}

/// Stability that is about something: the same graph inserted in two different orders must
/// give the same route. Calling the same function twice on one store proves only that it is
/// a function.
#[test]
fn multi_match_winner_is_stable_across_insertion_order() {
    let forward = vec![
        named_node("g_near", "goal"),
        named_node("g_far", "goal"),
        node("start"),
        node("mid"),
    ];
    let mut reversed = forward.clone();
    reversed.reverse();
    let edges = vec![
        edge("start", "g_near"),
        edge("start", "mid"),
        edge("mid", "g_far"),
    ];

    let a = path_between(&mem(&forward, &edges), "start", "goal", 8, 5_000).expect("a");
    let b = path_between(&mem(&reversed, &edges), "start", "goal", 8, 5_000).expect("b");
    assert_eq!(
        hop_ids(&a.hops),
        hop_ids(&b.hops),
        "insertion order must not decide the winner"
    );
    assert_eq!(a.hops[0].target, sym("g_near"), "and the nearer one wins");
}

/// `from == to` is a found route of zero hops at the seam, distinct from an absence.
#[test]
fn same_symbol_is_found_with_zero_hops_at_the_seam() {
    let (nodes, edges) = chain(2);
    let store = mem(&nodes, &edges);
    let r = path_between(&store, "a0", "a0", 8, 5_000).expect("query");
    assert!(r.found, "the identity route exists");
    assert!(r.hops.is_empty());
    assert_eq!(r.unresolved, None);
    assert!(!r.depth_bounded && !r.node_bounded);
}

/// The cross-`from`-candidate comparison, which nothing else observes.
///
/// Every other multi-match test has either one routing candidate or two of equal length, so
/// `hops.len() < current.len()` and "first candidate that finds anything wins" answer
/// identically. Here the EARLIER-sorting candidate reaches the goal by the longer route, so
/// taking the first would return a 3-hop detour while a 1-hop route exists.
#[test]
fn shortest_route_wins_across_from_candidates() {
    // "a_long" sorts before "z_short"; only the second has the short route.
    let nodes = vec![
        named_node("a_long", "start"),
        named_node("z_short", "start"),
        node("mid1"),
        node("mid2"),
        node("goal"),
    ];
    let edges = vec![
        edge("a_long", "mid1"),
        edge("mid1", "mid2"),
        edge("mid2", "goal"),
        edge("z_short", "goal"),
    ];

    for (label, hops) in [
        (
            "mem",
            path_between(&mem(&nodes, &edges), "start", "goal", 8, 5_000),
        ),
        (
            "sqlite",
            path_between(&sqlite(&nodes, &edges), "start", "goal", 8, 5_000),
        ),
    ] {
        let r = hops.expect("query");
        assert!(r.found, "{label}");
        assert_eq!(
            r.hops.len(),
            1,
            "{label}: the shortest route across candidates is 1 hop, not the \
             earlier-sorting candidate's 3-hop detour"
        );
        assert_eq!(r.hops[0].source, sym("z_short"), "{label}");
    }
}

/// #207's synthetic value-flow slots are not name-addressable (the 0.17.0 contract), and Path
/// must not resolve one by name either. A real `a0 → a1`, a value slot that is ALSO named `a1`,
/// and a value-only name: the value-only name is an unresolved operand, and the exact
/// `SymbolId` of a slot still addresses it.
#[test]
fn value_flow_slots_never_resolve_by_name() {
    let slot_a1 = "value synthetic x:local:a1:";
    let slot_only = "value synthetic x:local:only_val:";
    let mut nodes = vec![node("a0"), node("a1")];
    nodes.push(named_node(slot_a1, "a1").with_value_role("local"));
    nodes.push(named_node(slot_only, "only_val").with_value_role("local"));
    let mut edges = vec![edge("a0", "a1")];
    edges.push(Edge::new(
        sym(slot_a1),
        sym(slot_only),
        EdgeKind::Other("flows_to".into()),
        ResolutionTier::Parsed,
        "test",
    ));
    for (label, store) in [
        ("mem", Box::new(mem(&nodes, &edges)) as Box<dyn GraphRead>),
        (
            "sqlite",
            Box::new(sqlite(&nodes, &edges)) as Box<dyn GraphRead>,
        ),
    ] {
        let r = path_between(store.as_ref(), "a1", "only_val", 8, 1000).unwrap();
        assert!(
            !r.found,
            "{label}: a value-only name must not resolve: {r:?}"
        );
        assert_eq!(
            r.unresolved,
            Some(Unresolved::To),
            "{label}: a bare name resolved a value slot"
        );
        let r = path_between(store.as_ref(), slot_a1, slot_only, 8, 1000).unwrap();
        assert!(
            r.found,
            "{label}: the exact SymbolId must still address a value slot"
        );
        let r = path_between(store.as_ref(), "a0", "a1", 8, 1000).unwrap();
        assert_eq!(
            hop_ids(&r.hops),
            vec![("a0".to_string(), "a1".to_string())],
            "{label}: the real a1 still resolves, and only it"
        );
    }
}

/// A depth cut is not a node-budget cut. `Subgraph::truncated` is true for either cause, so
/// reading it for `node_bounded` reported "the walk exhausted its node budget (5000 nodes)"
/// on a 7-node chain cut at depth 2.
#[test]
fn a_depth_cut_is_not_reported_as_a_node_budget_cut() {
    let (nodes, edges) = chain(6);
    for (label, store) in [
        ("mem", Box::new(mem(&nodes, &edges)) as Box<dyn GraphRead>),
        (
            "sqlite",
            Box::new(sqlite(&nodes, &edges)) as Box<dyn GraphRead>,
        ),
    ] {
        let r = path_between(store.as_ref(), "a0", "a6", 2, 5000).unwrap();
        assert!(!r.found, "{label}");
        assert!(
            r.depth_bounded,
            "{label}: the depth frontier bound the walk"
        );
        assert!(
            !r.node_bounded,
            "{label}: 7 nodes never exhaust a 5000-node budget"
        );
    }
}
