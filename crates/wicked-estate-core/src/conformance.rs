//! A reusable conformance suite for [`GraphStore`] implementations.
//!
//! Every store (MemStore now; SQLite + SurrealDB at Wave 1.5) must pass [`graph_store_suite`].
//! Beyond CRUD it pins the **edge-direction invariant** and **bounded reverse-reachability**,
//! which is the contract blast-radius depends on. Call it from a `#[test]` in the store crate.

use crate::annotation::{Annotation, AnnotationClass, classify};
use crate::change::ChangeOp;
use crate::edge::{Direction, Edge, EdgeKind, ResolutionTier};
use crate::node::{Language, Location, Node, NodeKind, Span};
use crate::query::{SymbolQuery, TraversalSpec};
use crate::refs::UnresolvedRef;
use crate::repo::RepoInfo;
use crate::semantics::ValidationClaim;
use crate::symbol::{Descriptor, Symbol};
use crate::traits::GraphStore;

fn sym(name: &str) -> crate::symbol::SymbolId {
    Symbol::global("test", None, vec![Descriptor::method(name, None)]).id()
}

fn func_node(name: &str) -> Node {
    Node::new(
        sym(name),
        NodeKind::Function,
        name,
        Language::new("rust"),
        Location::new("src/lib.rs", Span::ZERO),
    )
}

fn calls(a: &str, b: &str) -> Edge {
    // "a calls b" → dependent=a (source), dependency=b (target).
    Edge::new(
        sym(a),
        sym(b),
        EdgeKind::Calls,
        ResolutionTier::Scip,
        "conformance",
    )
}

/// Independent union-of-`traverse` fold — the reference `traverse_multi` is checked against. Defined
/// HERE (not by calling `traverse_multi`) so a backend's specialization is compared to the slow
/// per-seed path, and the trait default's own wiring (incl. the drop-seeds step) is exercised.
fn union_of_traverse<S: crate::traits::GraphRead>(
    store: &S,
    starts: &[crate::symbol::SymbolId],
    spec: &TraversalSpec,
) -> crate::query::Subgraph {
    let mut nodes = Vec::new();
    let mut node_seen = std::collections::HashSet::new();
    let mut edges = Vec::new();
    let mut edge_seen = std::collections::HashSet::new();
    let mut depths = std::collections::BTreeMap::new();
    let mut acc = crate::query::Subgraph::default();
    for s in starts {
        let sub = store.traverse(s, spec).expect("traverse");
        // Fold BOTH causes (#190) — the kit's own fold used to inherit the node-cap blind spot.
        acc.absorb_truncation(&sub);
        for n in sub.nodes {
            if node_seen.insert(n.symbol.0.clone()) {
                nodes.push(n);
            }
        }
        for e in sub.edges {
            if edge_seen.insert(e.dedup_key()) {
                edges.push(e);
            }
        }
        for (k, v) in sub.depths {
            depths
                .entry(k)
                .and_modify(|d: &mut u32| *d = (*d).min(v))
                .or_insert(v);
        }
    }
    for s in starts {
        depths.remove(&s.0);
    }
    crate::query::Subgraph {
        nodes,
        edges,
        depths,
        ..acc
    }
}

/// Conformance: `traverse_multi(starts)` returns the IDENTICAL subgraph as the union of
/// `traverse(start)` over each seed — node set (by symbol), edge set (by dedup key), and the
/// min-depth map with ALL seeds excluded. The fixture has TEETH: a cross-reachable seed
/// (`tm_s2` reachable from `tm_s1`), a node reached from BOTH seeds (`tm_m1` — min-depth dedup),
/// and a multi-hop path (`tm_m1 → tm_leaf`) — so a backend that specializes `traverse_multi`
/// (e.g. SqliteStore's multi-seed CTE) is verified against the slow fold, not merely itself.
/// Untruncated (generous caps): the cap-interaction between per-seed and total limits is out of
/// scope here. Run on a FRESH store.
pub fn traverse_multi_matches_union_of_traverse<S: GraphStore>(store: &mut S) {
    let nodes = [
        func_node("tm_s1"),
        func_node("tm_s2"),
        func_node("tm_m1"),
        func_node("tm_leaf"),
    ];
    // tm_s1→tm_m1, tm_s1→tm_s2 (cross-reachable seed), tm_s2→tm_m1 (shared), tm_s2→tm_leaf,
    // tm_m1→tm_leaf (multi-hop).
    let edges = [
        calls("tm_s1", "tm_m1"),
        calls("tm_s1", "tm_s2"),
        calls("tm_s2", "tm_m1"),
        calls("tm_s2", "tm_leaf"),
        calls("tm_m1", "tm_leaf"),
    ];
    store.begin_batch().expect("begin");
    store.upsert_nodes(&nodes).expect("upsert nodes");
    store.upsert_edges(&edges).expect("upsert edges");
    store.commit_batch().expect("commit");

    let seeds = [sym("tm_s1"), sym("tm_s2")];

    for dir in [Direction::Dependencies, Direction::Both] {
        let mut spec = TraversalSpec::blast_radius(8);
        spec.direction = dir;
        spec.max_depth = 8;
        spec.max_nodes = 1000;
        spec.min_confidence = 0.0;
        spec.edge_kinds = vec![];

        let got = store.traverse_multi(&seeds, &spec).expect("traverse_multi");
        let want = union_of_traverse(store, &seeds, &spec);

        assert_eq!(
            got.depths, want.depths,
            "traverse_multi depths must equal union-of-traverse ({dir:?})"
        );
        let got_syms: std::collections::BTreeSet<_> =
            got.nodes.iter().map(|n| n.symbol.0.clone()).collect();
        let want_syms: std::collections::BTreeSet<_> =
            want.nodes.iter().map(|n| n.symbol.0.clone()).collect();
        assert_eq!(
            got_syms, want_syms,
            "traverse_multi node set must equal union-of-traverse ({dir:?})"
        );
        let got_edges: std::collections::BTreeSet<_> =
            got.edges.iter().map(|e| e.dedup_key()).collect();
        let want_edges: std::collections::BTreeSet<_> =
            want.edges.iter().map(|e| e.dedup_key()).collect();
        assert_eq!(
            got_edges, want_edges,
            "traverse_multi edge set must equal union-of-traverse ({dir:?})"
        );
        // The honesty triple must agree too — on THIS fixture, whose generous caps cut nothing, so
        // an override that invents a cut (or drops the causes) fails here. Equality is NOT an
        // invariant once a horizon bites: the union-of-traverse fold can over-report a cut a
        // native multi-seed walk correctly rules out (an edge from a seed's horizon node to
        // ANOTHER seed is unseen per-seed but inside the union). Over-reporting is the safe
        // direction; a horizon fixture here would pin a difference, not a defect.
        assert_eq!(
            (
                got.truncated,
                got.node_cap_reached,
                got.depth_horizon_reached
            ),
            (
                want.truncated,
                want.node_cap_reached,
                want.depth_horizon_reached
            ),
            "traverse_multi truncation causes must equal union-of-traverse ({dir:?})"
        );
        assert!(
            got.truncation_invariant_holds() && want.truncation_invariant_holds(),
            "truncated == node_cap_reached || depth_horizon_reached ({dir:?})"
        );
    }

    // Hardcoded discriminator (Dependencies) — catches a bug SHARED by the fold and the override:
    // the cross-reachable seed tm_s2 must be EXCLUDED from depths; tm_m1 at min-depth 1 (both
    // seeds); tm_leaf at min-depth 1 (tm_s2→tm_leaf beats tm_s1→tm_m1→tm_leaf).
    let mut spec = TraversalSpec::blast_radius(8);
    spec.direction = Direction::Dependencies;
    spec.max_depth = 8;
    spec.max_nodes = 1000;
    spec.min_confidence = 0.0;
    spec.edge_kinds = vec![];
    let got = store.traverse_multi(&seeds, &spec).expect("traverse_multi");
    assert_eq!(
        got.depths.get(sym("tm_m1").as_str()),
        Some(&1),
        "tm_m1 reached from both seeds at depth 1"
    );
    assert_eq!(
        got.depths.get(sym("tm_leaf").as_str()),
        Some(&1),
        "tm_leaf at min-depth 1 (tm_s2 → tm_leaf)"
    );
    assert!(
        !got.depths.contains_key(sym("tm_s2").as_str()),
        "cross-reachable SEED tm_s2 must be excluded from depths"
    );
    assert!(
        !got.depths.contains_key(sym("tm_s1").as_str()),
        "seed tm_s1 must be excluded from depths"
    );
}

/// Conformance: a bounded `traverse` cut by the **depth horizon** must say so.
///
/// This is wicked-estate#190. Every store derived `Subgraph::truncated` from the `max_nodes` cap
/// alone, so a deep, narrow graph — the legacy-estate shape: a long COBOL `PERFORM` / JCL step
/// chain has FEW nodes and MANY hops, so the node cap never trips — came back cut and labelled
/// complete, on both the CLI and the MCP transports. The honesty fields callers trust are worse
/// than useless when a cap they do not cover exists (agent rule R3).
///
/// Fixture: a 7-node `Calls` chain `dh_00 → dh_01 → … → dh_06` (so `dh_00` DEPENDS ON all six),
/// walked `Dependencies` from `dh_00`. Each case pins the two causes INDEPENDENTLY plus the
/// `truncated == node_cap_reached || depth_horizon_reached` invariant:
///
/// 1. **Depth cut only** — `max_depth = 2`, `max_nodes = 1000` (far more than the chain):
///    `depth_horizon_reached`, NOT `node_cap_reached`. This is the case that failed everywhere.
/// 2. **Complete** — `max_depth = 16`, `max_nodes = 1000`: all three false. Guards the opposite
///    failure (a probe that always fires is as dishonest as one that never does).
/// 3. **Node cut only** — `max_depth = 16`, `max_nodes = 2`: `node_cap_reached`, NOT
///    `depth_horizon_reached`. The recursion ran to the chain's end, so no node was left
///    unexpanded — the two causes must not be aliases for each other.
/// 4. **Cap-dropped neighbour** — a node the node cap declined is not "beyond the horizon"; a
///    BFS that forgets it was seen would mislabel a node-cap cut as a depth cut.
///
/// Run on any store; the `dh_*` symbols are disjoint from the other fixtures, so
/// [`graph_store_suite`] calls it inline.
pub fn traverse_reports_depth_horizon<S: GraphStore>(store: &mut S) {
    const N: usize = 7;
    let names: Vec<String> = (0..N).map(|i| format!("dh_{i:02}")).collect();
    let nodes: Vec<Node> = names.iter().map(|n| func_node(n)).collect();
    let edges: Vec<Edge> = (0..N - 1)
        .map(|i| calls(&names[i], &names[i + 1]))
        .collect();
    store.begin_batch().expect("begin_batch");
    store.upsert_nodes(&nodes).expect("upsert_nodes");
    store.upsert_edges(&edges).expect("upsert_edges");
    store.commit_batch().expect("commit_batch");

    let start = sym(&names[0]);
    let spec = |max_depth: u32, max_nodes: usize| TraversalSpec {
        direction: Direction::Dependencies,
        edge_kinds: vec![],
        max_depth,
        max_nodes,
        min_confidence: 0.0,
    };

    // --- case 1: the depth horizon cut the result, the node cap did not ---
    let sub = store
        .traverse(&start, &spec(2, 1000))
        .expect("traverse d=2");
    assert!(
        sub.truncation_invariant_holds(),
        "truncated must equal node_cap_reached || depth_horizon_reached (depth-cut case): {sub:?}"
    );
    assert_eq!(
        sub.depths.len(),
        2,
        "depth 2 over a 7-chain reaches exactly dh_01, dh_02 — got {:?}",
        sub.depths
    );
    assert!(
        sub.depth_horizon_reached,
        "max_depth=2 left dh_02 unexpanded with dh_03 unreached — the depth horizon CUT the \
         result and must be reported (wicked-estate#190)"
    );
    assert!(
        !sub.node_cap_reached,
        "max_nodes=1000 over a 7-node chain cannot have capped anything"
    );
    assert!(
        sub.truncated,
        "a depth-cut result is INCOMPLETE; `truncated` is the field a naive caller reads"
    );

    // --- case 2: the whole chain fits — nothing was cut ---
    let sub = store
        .traverse(&start, &spec(16, 1000))
        .expect("traverse d=16");
    assert!(
        sub.truncation_invariant_holds(),
        "invariant (complete case)"
    );
    assert_eq!(
        sub.depths.len(),
        N - 1,
        "depth 16 reaches the whole chain — got {:?}",
        sub.depths
    );
    assert!(
        !sub.depth_horizon_reached,
        "the walk ran past the chain's end; no node was left unexpanded"
    );
    assert!(!sub.node_cap_reached, "max_nodes=1000 did not bite");
    assert!(
        !sub.truncated,
        "a complete result must NOT claim truncation (a probe that always fires is as \
         dishonest as one that never fires)"
    );

    // --- case 3: the node cap cut the result, the depth horizon did not ---
    let sub = store.traverse(&start, &spec(16, 2)).expect("traverse n=2");
    assert!(
        sub.truncation_invariant_holds(),
        "invariant (node-cap case)"
    );
    assert!(
        sub.node_cap_reached,
        "max_nodes=2 over a 6-dependency chain must report the node cap"
    );
    assert!(
        !sub.depth_horizon_reached,
        "the recursion ran to the chain's end at max_depth=16 — the depth horizon is NOT the \
         cause here; the two flags must not alias"
    );
    assert!(sub.truncated, "a node-capped result is incomplete");

    // --- case 4: a node the CAP dropped is not "beyond the horizon" ---
    // `nc_b → nc_a`, `nc_c → nc_a`, `nc_c → nc_b`; Dependents of `nc_a` at max_depth=1. Both
    // dependents sit at depth 1, so nothing lies beyond the horizon. With max_nodes=2 a BFS store
    // may drop `nc_c` for the cap; probing the horizon node `nc_b` must then not count the
    // cap-dropped `nc_c` as an unreached neighbour — that would label a node-cap cut as a depth
    // cut and tell the caller to "raise depth", which cannot help. (Stores differ on WHETHER this
    // fixture trips the cap — MemStore counts the start against `max_nodes`, PostgresStore
    // fetches `max_nodes + 1` — so only the cause is pinned, never `node_cap_reached`.)
    store.begin_batch().expect("begin_batch");
    store
        .upsert_nodes(&[func_node("nc_a"), func_node("nc_b"), func_node("nc_c")])
        .expect("upsert nc nodes");
    store
        .upsert_edges(&[
            calls("nc_b", "nc_a"),
            calls("nc_c", "nc_a"),
            calls("nc_c", "nc_b"),
        ])
        .expect("upsert nc edges");
    store.commit_batch().expect("commit_batch");
    let sub = store
        .traverse(
            &sym("nc_a"),
            &TraversalSpec {
                direction: Direction::Dependents,
                edge_kinds: vec![],
                max_depth: 1,
                max_nodes: 2,
                min_confidence: 0.0,
            },
        )
        .expect("traverse nc");
    assert!(
        sub.truncation_invariant_holds(),
        "invariant (cap-dropped case)"
    );
    assert!(
        !sub.depth_horizon_reached,
        "every dependent of nc_a is at depth 1 — a node dropped by the NODE CAP is not beyond the \
         depth horizon, and must not be reported as a depth cut"
    );
}

/// Run the full contract against a fresh, empty store. Panics on the first violation.
pub fn graph_store_suite<S: GraphStore>(store: &mut S) {
    // Fixture: a → b → c  (a calls b, b calls c).
    let nodes = [func_node("a"), func_node("b"), func_node("c")];
    let edges = [calls("a", "b"), calls("b", "c")];

    store.begin_batch().expect("begin_batch");
    store.upsert_nodes(&nodes).expect("upsert_nodes");
    store.upsert_edges(&edges).expect("upsert_edges");
    store.commit_batch().expect("commit_batch");

    // --- idempotency: re-upserting the same edges must not create duplicates ---
    store.upsert_edges(&edges).expect("re-upsert edges");

    // --- evidence_count round-trips (brain consolidation) ---
    // Re-upsert a→b carrying evidence_count=7. Same (source,target,kind) + confidence, so this
    // UPDATES the existing edge in place (no new row — edge_count stays 2). Asserted on read below;
    // every backend MUST round-trip the first-class field. b→c never sets it → stays the honest 0.
    store
        .upsert_edges(&[calls("a", "b").with_evidence_count(7)])
        .expect("re-upsert a->b with evidence_count");

    // --- stats ---
    let stats = store.stats().expect("stats");
    assert_eq!(
        stats.node_count, 3,
        "expected 3 nodes, got {}",
        stats.node_count
    );
    assert_eq!(
        stats.edge_count, 2,
        "edges must be deduped to 2, got {}",
        stats.edge_count
    );

    // --- get_node round-trips ---
    let got = store
        .get_node(&sym("b"))
        .expect("get_node")
        .expect("node b exists");
    assert_eq!(got.symbol, sym("b"));
    assert_eq!(got.name, "b");
    assert!(
        store
            .get_node(&sym("zzz"))
            .expect("get_node missing")
            .is_none()
    );

    // --- EDGE-DIRECTION INVARIANT ---
    // b's dependents = symbols that depend on b = edges where target==b → {a}.
    let dependents = store
        .neighbors(&sym("b"), Direction::Dependents)
        .expect("dependents");
    assert_eq!(dependents.len(), 1, "b has exactly one dependent");
    assert_eq!(dependents[0].source, sym("a"), "a is the dependent of b");
    assert_eq!(dependents[0].target, sym("b"));
    assert_eq!(
        dependents[0].evidence_count, 7,
        "evidence_count must round-trip through the store (a->b carries 7)"
    );

    // b's dependencies = what b depends on = edges where source==b → {c}.
    let deps = store
        .neighbors(&sym("b"), Direction::Dependencies)
        .expect("dependencies");
    assert_eq!(deps.len(), 1, "b has exactly one dependency");
    assert_eq!(deps[0].target, sym("c"), "c is the dependency of b");
    assert_eq!(
        deps[0].evidence_count, 0,
        "an edge that never set evidence_count round-trips as the honest 0"
    );

    // --- BLAST-RADIUS via bounded reverse-reachability ---
    // "what breaks if I change c?" → c's transitive dependents = {b (depth 1), a (depth 2)}.
    let blast = store
        .traverse(&sym("c"), &TraversalSpec::blast_radius(8))
        .expect("traverse");
    assert!(
        blast.depths.contains_key(sym("b").as_str()),
        "b is in c's blast radius"
    );
    assert!(
        blast.depths.contains_key(sym("a").as_str()),
        "a is in c's blast radius"
    );
    assert_eq!(blast.depths[sym("b").as_str()], 1, "b is one hop from c");
    assert_eq!(blast.depths[sym("a").as_str()], 2, "a is two hops from c");

    // depth cap is honored: depth 1 reaches b but not a.
    let mut shallow = TraversalSpec::blast_radius(1);
    shallow.max_nodes = 5_000;
    let near = store
        .traverse(&sym("c"), &shallow)
        .expect("shallow traverse");
    assert!(near.depths.contains_key(sym("b").as_str()));
    assert!(
        !near.depths.contains_key(sym("a").as_str()),
        "depth cap must exclude a"
    );

    // --- symbol search ---
    let q = SymbolQuery {
        exact_name: Some("a".to_string()),
        ..Default::default()
    };
    let found = store.find_symbols(&q).expect("find_symbols");
    assert_eq!(found.len(), 1, "exact-name search finds exactly one");
    assert_eq!(found[0].name, "a");

    // --- bulk accessors for global analytics ---
    assert_eq!(store.all_nodes().expect("all_nodes").len(), 3);
    assert_eq!(store.all_edges().expect("all_edges").len(), 2);

    // --- capabilities are reported (drives retrieval fallbacks; must not panic) ---
    let _caps = store.capabilities();

    // --- unresolved refs: round-trip + stats counter ---
    // Simulate a ref the resolver could not bind (e.g. a call to a symbol named "ghost"
    // that has no matching definition in the index).
    let ghost_ref = UnresolvedRef::new(
        sym("a"),
        "ghost",
        EdgeKind::Calls,
        Location::new("src/lib.rs", Span::ZERO),
    );
    // A second ghost ref in a DIFFERENT file: remove_file (checked further down) must drop
    // only src/lib.rs's row and keep this one (rows are per unresolved reference —
    // docs/ENGINE-CONTRACT.md §2.1).
    let ghost_ref_other = UnresolvedRef::new(
        sym("a"),
        "ghost",
        EdgeKind::Calls,
        Location::new("src/other.rs", Span::ZERO),
    );
    store
        .upsert_unresolved_refs(&[ghost_ref, ghost_ref_other])
        .expect("upsert_unresolved_refs");

    let found = store
        .unresolved_refs_for_name("ghost")
        .expect("unresolved_refs_for_name");
    assert_eq!(found.len(), 2, "two unresolved refs for 'ghost' (per site)");
    assert!(found.iter().all(|r| r.raw_name == "ghost"));
    assert!(found.iter().all(|r| r.from == sym("a")));

    let stats_after = store.stats().expect("stats after unresolved upsert");
    assert_eq!(
        stats_after.unresolved_ref_count, 2,
        "stats must reflect the stored unresolved refs"
    );

    // A name with no refs returns an empty vec, not an error.
    let none = store
        .unresolved_refs_for_name("no_such_name")
        .expect("empty lookup ok");
    assert!(none.is_empty(), "no unresolved refs for unknown name");

    // --- unresolved refs: NON-ZERO span round-trip (admissibility F-B) ---
    // Exact-site identity is byte-exact: the store must persist and return the ref's
    // start_line + start_byte + end_byte (the persisted subset — cols/end_line are not part of
    // the contract), so two same-line sites are distinguishable without on-disk adjudication.
    let spanned_ref = UnresolvedRef::new(
        sym("a"),
        "spanned_ghost",
        EdgeKind::Calls,
        Location::new(
            "src/lib.rs",
            Span {
                start_line: 7,
                start_byte: 120,
                end_byte: 133,
                ..Span::ZERO
            },
        ),
    );
    store
        .upsert_unresolved_refs(std::slice::from_ref(&spanned_ref))
        .expect("upsert spanned unresolved ref");
    let found = store
        .unresolved_refs_for_name("spanned_ghost")
        .expect("unresolved_refs_for_name spanned");
    assert_eq!(found.len(), 1, "one spanned ref expected");
    assert_eq!(
        found[0].location.span.start_line, 7,
        "start_line round-trips"
    );
    assert_eq!(
        found[0].location.span.start_byte, 120,
        "start_byte round-trips"
    );
    assert_eq!(found[0].location.span.end_byte, 133, "end_byte round-trips");

    // --- Wave 2.6: file digest round-trip ---
    // set_file_digest / file_digest must survive an upsert (second write overwrites first).
    store
        .set_file_digest("f.rs", "abc123")
        .expect("set_file_digest");
    let got = store.file_digest("f.rs").expect("file_digest");
    assert_eq!(
        got,
        Some("abc123".to_string()),
        "file_digest must return stored value"
    );

    // Overwrite with a new digest.
    store
        .set_file_digest("f.rs", "def456")
        .expect("set_file_digest overwrite");
    let got2 = store
        .file_digest("f.rs")
        .expect("file_digest after overwrite");
    assert_eq!(
        got2,
        Some("def456".to_string()),
        "overwritten digest must be returned"
    );

    // Unknown file returns None (not an error).
    let missing = store
        .file_digest("no_such.rs")
        .expect("file_digest missing");
    assert!(missing.is_none(), "unknown file digest must be None");

    // --- Wave 11.1: file content round-trip ---
    // set_file_content / file_content must survive an upsert (second write overwrites first).
    store
        .set_file_content("src/lib.rs", "fn hello() {}")
        .expect("set_file_content");
    let got_content = store.file_content("src/lib.rs").expect("file_content");
    assert_eq!(
        got_content,
        Some("fn hello() {}".to_string()),
        "file_content must return stored text"
    );

    // Overwrite with new content.
    store
        .set_file_content("src/lib.rs", "fn world() {}")
        .expect("set_file_content overwrite");
    let got_content2 = store
        .file_content("src/lib.rs")
        .expect("file_content after overwrite");
    assert_eq!(
        got_content2,
        Some("fn world() {}".to_string()),
        "overwritten content must be returned"
    );

    // Unknown file returns None.
    let missing_content = store
        .file_content("no_such.rs")
        .expect("file_content missing");
    assert!(
        missing_content.is_none(),
        "file_content for unknown file must be None"
    );

    // --- Wave 11.1: symbol_source slice extraction ---
    // Insert a node with a non-zero span pointing into content we control.
    // "hello" starts at byte 3 and ends at byte 8 in "fn hello() {}" (0-indexed).
    let source_text = "fn hello() {}";
    store
        .set_file_content("src/content_test.rs", source_text)
        .expect("set content for slice test");
    let span_node = Node::new(
        sym("content_sym"),
        NodeKind::Function,
        "hello",
        Language::new("rust"),
        Location::new(
            "src/content_test.rs",
            Span {
                start_byte: 3,
                end_byte: 8,
                start_line: 0,
                start_col: 3,
                end_line: 0,
                end_col: 8,
            },
        ),
    );
    store
        .upsert_nodes(std::slice::from_ref(&span_node))
        .expect("upsert span_node");
    let slice = store.symbol_source(&span_node).expect("symbol_source");
    assert_eq!(
        slice,
        Some("hello".to_string()),
        "symbol_source must return the byte slice"
    );

    // A node with Span::ZERO returns None.
    let zero_node = func_node("a"); // location.file = "src/lib.rs", span = ZERO
    let zero_slice = store
        .symbol_source(&zero_node)
        .expect("symbol_source zero span");
    assert!(
        zero_slice.is_none(),
        "symbol_source for Span::ZERO must return None"
    );

    // --- FINDING-067: indexed_files reports what the INDEXER wrote, never a node's location ---
    // `index_path`'s delete-sweep removes every path this returns that is not on disk. So the one
    // property that keeps the sweep safe is: a path may appear here only because the indexer put it
    // here (`set_file_digest` / `set_file_content`), never merely because some node's
    // `location.file` says so.
    //
    // That distinction is not academic. An orchestrator sharing a store keeps its domain objects as
    // nodes with synthetic `location.file` values — `agent_session/<id>`, `work_unit/<id>`. A
    // backend answering this from nodes classifies all of them as deleted source files; in
    // production that swept 833 operational nodes in one transaction, including the session that
    // issued the index.
    store
        .set_file_digest("src/lib.rs", "deadbeef")
        .expect("set_file_digest");
    let indexed = store.indexed_files().expect("indexed_files");
    assert!(
        indexed.contains(&"src/lib.rs".to_string()),
        "a path with a stored digest must be reported; got {indexed:?}"
    );

    // BOTH file-writing calls count, not just `set_file_digest`. The backends disagreed on this:
    // `SqliteStore` keeps one `files` table that both calls write, while `MemStore` and
    // `SurrealStore` route content to a separate map/table. A content-recorded path invisible here
    // is never considered by the delete-sweep, so it lingers forever after being deleted on disk.
    store
        .set_file_content("src/content_only.rs", "pub fn only_content() {}\n")
        .expect("set_file_content");
    let indexed = store.indexed_files().expect("indexed_files after content");
    assert!(
        indexed.contains(&"src/content_only.rs".to_string()),
        "a path recorded via set_file_content must be reported too; got {indexed:?}"
    );

    // A node whose location was never written through any file-writing call. This is the exact
    // shape of the rows that were destroyed, and it must be invisible here.
    let foreign_path = "agent_session/conformance-1";
    store
        .upsert_nodes(&[Node::new(
            sym("conformance_foreign"),
            NodeKind::Other("agent_session".to_string()),
            "conformance-1",
            Language::new("none"),
            Location::new(foreign_path, Span::ZERO),
        )])
        .expect("upsert foreign node");
    let indexed = store
        .indexed_files()
        .expect("indexed_files after foreign node");
    assert!(
        !indexed.iter().any(|p| p == foreign_path),
        "indexed_files must never report a path that only exists as a node location; got {indexed:?}"
    );

    // --- Wave 2.6: remove_file removes that file's nodes ---
    // The fixture nodes all have location.file == "src/lib.rs" (set by func_node above).
    // After remove_file("src/lib.rs") none of them should remain. Scope (incr-integrity lane):
    // this unconditional removal covers every NON-Import node kind; a shared `NodeKind::Import`
    // node with survivor edges is instead kept + re-homed — pinned by the shared-Import section
    // at the end of this suite.
    store.remove_file("src/lib.rs").expect("remove_file");
    let remaining = store.all_nodes().expect("all_nodes after remove_file");
    assert!(
        remaining.iter().all(|n| n.location.file != "src/lib.rs"),
        "remove_file must remove all (non-Import) nodes whose location.file matches; remaining: {:?}",
        remaining
            .iter()
            .map(|n| &n.location.file)
            .collect::<Vec<_>>()
    );

    // remove_file also drops the file's unresolved rows — the per-file delete path that keeps
    // unresolved accounting per resolve pass (docs/ENGINE-CONTRACT.md §2.1): the src/lib.rs
    // ghost row is gone, the src/other.rs one survives.
    let ghost_after = store
        .unresolved_refs_for_name("ghost")
        .expect("unresolved_refs_for_name after remove_file");
    assert_eq!(
        ghost_after.len(),
        1,
        "remove_file must drop only the removed file's unresolved rows"
    );
    assert_eq!(ghost_after[0].location.file, "src/other.rs");
    assert_eq!(
        store
            .stats()
            .expect("stats after remove_file")
            .unresolved_ref_count,
        1,
        "stats must reflect the per-file unresolved delete"
    );

    // --- prune_dangling_edges: removes edges to missing nodes; keeps valid edges ---
    //
    // After remove_file("src/lib.rs") above, nodes a/b/c are gone but their edges may
    // still linger in the store (SQLite deletes edges by `file` column, and the fixture
    // edges carry file=''; MemStore purges by source-node membership).  We record the
    // edge count BEFORE inserting new nodes/edges so the relative delta is store-agnostic.

    // Insert two new nodes in a fresh file so they survive remove_file.
    let p_node = Node::new(
        sym("p"),
        NodeKind::Function,
        "p",
        Language::new("rust"),
        Location::new("src/other.rs", Span::ZERO),
    );
    let q_node = Node::new(
        sym("q"),
        NodeKind::Function,
        "q",
        Language::new("rust"),
        Location::new("src/other.rs", Span::ZERO),
    );
    store.upsert_nodes(&[p_node, q_node]).expect("upsert p, q");

    // Snapshot the existing edge count; we are about to add exactly two more.
    let edges_before_new = store
        .all_edges()
        .expect("all_edges before new inserts")
        .len();

    // Edge p → q: valid (both nodes exist).
    let valid_edge = calls("p", "q");
    // Edge p → ghost_target: dangling (ghost_target never inserted as a node).
    let dangling_edge = Edge::new(
        sym("p"),
        sym("ghost_target"),
        EdgeKind::Calls,
        ResolutionTier::Scip,
        "conformance",
    );
    store
        .upsert_edges(&[valid_edge, dangling_edge])
        .expect("upsert edges for prune test");

    // Total edges now = pre-existing + 2 new.
    let before_prune = store.all_edges().expect("all_edges before prune").len();
    assert_eq!(
        before_prune,
        edges_before_new + 2,
        "must have added exactly 2 edges"
    );

    let pruned = store.prune_dangling_edges().expect("prune_dangling_edges");
    // Every edge whose source or target is not in the current node set is removed.
    // At minimum the 1 dangling ghost_target edge must be pruned (any pre-existing
    // fixture danglers are also pruned — we don't assert their count here).
    assert!(
        pruned >= 1,
        "prune_dangling_edges must remove at least the ghost edge; pruned={pruned}"
    );

    // After pruning: the p→q edge must still exist; ghost edge must be gone.
    let after_edges = store.all_edges().expect("all_edges after prune");
    let has_pq = after_edges
        .iter()
        .any(|e| e.source == sym("p") && e.target == sym("q"));
    let has_ghost = after_edges.iter().any(|e| e.target == sym("ghost_target"));
    assert!(has_pq, "valid p→q edge must survive prune");
    assert!(
        !has_ghost,
        "dangling p→ghost_target edge must be removed by prune"
    );

    // ── Wave 7 (a): file_git_sha correctness ────────────────────────────────
    // `echo -n hello | git hash-object --stdin` = b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0
    // This pins the SHA1 blob computation against the known `git hash-object` value so any
    // backend that computes it independently stays consistent with git.
    store
        .set_file_content("conformance_git_sha.rs", "hello")
        .expect("set_file_content for git_sha check");
    let sha = store
        .file_git_sha("conformance_git_sha.rs")
        .expect("file_git_sha must not error")
        .expect("file_git_sha must be Some after set_file_content");
    assert_eq!(
        sha, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0",
        "file_git_sha for content \"hello\" must equal the known git hash-object value"
    );

    // ── Wave 7.1 (b): changes_since returns logged changes in seq order ──────
    store
        .log_change(ChangeOp::Upsert, "conformance_a.rs")
        .expect("log_change upsert");
    store
        .log_change(ChangeOp::Upsert, "conformance_b.rs")
        .expect("log_change upsert 2");
    store
        .log_change(ChangeOp::Remove, "conformance_c.rs")
        .expect("log_change remove");

    let all_changes = store.changes_since(0).expect("changes_since(0)");
    // Must contain at least the 3 we just logged (prior test phases may have logged more).
    assert!(
        all_changes.len() >= 3,
        "changes_since(0) must return at least the 3 logged changes; got {}",
        all_changes.len()
    );
    // Must be in ascending seq order.
    for w in all_changes.windows(2) {
        assert!(
            w[0].seq < w[1].seq,
            "changes_since must return rows in ascending seq order; got {:?} then {:?}",
            w[0].seq,
            w[1].seq
        );
    }
    // The three we logged must be present at the tail (they were appended last).
    let tail = &all_changes[all_changes.len() - 3..];
    assert_eq!(
        tail[0].target, "conformance_a.rs",
        "first logged change target mismatch"
    );
    assert_eq!(
        tail[1].target, "conformance_b.rs",
        "second logged change target mismatch"
    );
    assert_eq!(
        tail[2].target, "conformance_c.rs",
        "third logged change target mismatch"
    );
    assert_eq!(
        tail[2].op,
        ChangeOp::Remove,
        "third logged change must be Remove"
    );

    // Resume: changes_since(tail[1].seq) must return only the third.
    let resumed = store
        .changes_since(tail[1].seq)
        .expect("changes_since resume");
    assert!(
        !resumed.is_empty(),
        "changes_since(seq of second-last) must return at least the last change"
    );
    assert_eq!(
        resumed.last().expect("at least one").target,
        "conformance_c.rs",
        "resumed cursor must include the third logged change"
    );

    // ── Wave 7 (c): repo_info round-trip ────────────────────────────────────
    // Before set: must be None.
    let no_info = store.repo_info().expect("repo_info before set");
    assert!(no_info.is_none(), "repo_info must be None when never set");

    let info = RepoInfo {
        commit: Some("deadbeef".to_string()),
        branch: Some("conformance".to_string()),
        remote: Some("https://example.com/repo".to_string()),
        dirty: true,
    };
    store.set_repo_info(&info).expect("set_repo_info");
    let got_info = store
        .repo_info()
        .expect("repo_info after set")
        .expect("must be Some");
    assert_eq!(
        got_info.commit,
        Some("deadbeef".to_string()),
        "repo_info commit mismatch"
    );
    assert_eq!(
        got_info.branch,
        Some("conformance".to_string()),
        "repo_info branch mismatch"
    );
    assert_eq!(
        got_info.remote,
        Some("https://example.com/repo".to_string()),
        "repo_info remote mismatch"
    );
    assert!(got_info.dirty, "repo_info dirty flag mismatch");

    // ── Wave 7 (d): edge_history archives superseded edges on remove_file ───
    // Set up: one file, one node, one edge. Capture v1 git_sha. Remove file.
    // Assert: edge_history for that file contains the superseded edge tagged with v1 sha.
    let v1_content = "fn conformance_fn() {}";
    store
        .set_file_content("conformance_hist.rs", v1_content)
        .expect("set v1 content for history test");
    let v1_sha = store
        .file_git_sha("conformance_hist.rs")
        .expect("file_git_sha v1")
        .expect("v1 sha must be Some");

    // Insert a node in conformance_hist.rs and a node to call (uses src/other.rs from above).
    let hist_node = Node::new(
        sym("conformance_hist_fn"),
        NodeKind::Function,
        "conformance_hist_fn",
        Language::new("rust"),
        Location::new("conformance_hist.rs", Span::ZERO),
    );
    store.upsert_nodes(&[hist_node]).expect("upsert hist_node");

    // Edge: conformance_hist_fn → q (q was upserted above in src/other.rs).
    // The edge MUST carry a location pointing at the file being removed so the SQLite
    // archival (SELECT ... WHERE file=?) and MemStore archival (filter by location.file)
    // can both find it during remove_file.
    let hist_edge = Edge::new(
        sym("conformance_hist_fn"),
        sym("q"),
        EdgeKind::Calls,
        ResolutionTier::Scip,
        "conformance",
    )
    .with_location(Location::new("conformance_hist.rs", Span::ZERO));
    store.upsert_edges(&[hist_edge]).expect("upsert hist_edge");

    // Remove the file: this must archive the edge into edge_history before deleting live data.
    store
        .remove_file("conformance_hist.rs")
        .expect("remove conformance_hist.rs");

    let history = store
        .edge_history("conformance_hist.rs")
        .expect("edge_history after remove_file");
    assert!(
        !history.is_empty(),
        "edge_history must be non-empty after remove_file archived an edge"
    );
    // The archived entry must carry v1's git_sha.
    let found_v1 = history.iter().any(|h| h.git_sha == v1_sha);
    assert!(
        found_v1,
        "archived edge must be tagged with the v1 git_sha ({v1_sha}); history: {:?}",
        history.iter().map(|h| &h.git_sha).collect::<Vec<_>>()
    );
    // The archived edge must have the right source.
    let has_source = history
        .iter()
        .any(|h| h.edge.source == sym("conformance_hist_fn"));
    assert!(
        has_source,
        "archived edge must have conformance_hist_fn as source"
    );

    // ── Semantic linking ─────────────────────────────────────────────────────
    // Re-insert a fresh node "sem_fn" in a file that hasn't been wiped by remove_file.
    let sem_node = Node::new(
        sym("sem_fn"),
        NodeKind::Function,
        "sem_fn",
        Language::new("rust"),
        Location::new("src/sem.rs", Span::ZERO),
    );
    store.upsert_nodes(&[sem_node]).expect("upsert sem_fn");

    // Before any annotation: node_semantics returns None (no row in the semantics store).
    let before = store
        .node_semantics(&sym("sem_fn"))
        .expect("node_semantics before annotation");
    assert!(
        before.is_none(),
        "node_semantics must be None before any annotation is set"
    );

    // Full write: set all three fields.
    store
        .set_node_semantics(
            &sym("sem_fn"),
            Some("what it is"),
            Some("REQ-1"),
            Some(&ValidationClaim::new(true, "conformance-actor").expect("named actor")),
        )
        .expect("set_node_semantics full");

    let full = store
        .node_semantics(&sym("sem_fn"))
        .expect("node_semantics after full write")
        .expect("must be Some after annotation");
    assert_eq!(
        full.description,
        Some("what it is".to_string()),
        "description must be stored"
    );
    assert_eq!(
        full.requirement,
        Some("REQ-1".to_string()),
        "requirement must be stored"
    );
    assert!(full.requirement_validated, "validated flag must be true");
    // A validated requirement must carry WHO validated it. A store that keeps the flag and drops the
    // author reintroduces the unattributable claim `ValidationClaim` exists to prevent (#79).
    assert_eq!(
        full.requirement_validated_by.as_deref(),
        Some("conformance-actor"),
        "the validating actor must be stored alongside the flag"
    );
    assert!(
        full.requirement_validated_at.is_some_and(|t| t > 0),
        "the store must stamp when the claim was made, got {:?}",
        full.requirement_validated_at
    );

    // PARTIAL update: change only description — requirement and validated must be unchanged.
    store
        .set_node_semantics(&sym("sem_fn"), Some("updated desc"), None, None)
        .expect("set_node_semantics partial");

    let partial = store
        .node_semantics(&sym("sem_fn"))
        .expect("node_semantics after partial update")
        .expect("must still be Some");
    assert_eq!(
        partial.description,
        Some("updated desc".to_string()),
        "description must reflect partial update"
    );
    assert_eq!(
        partial.requirement,
        Some("REQ-1".to_string()),
        "requirement must be unchanged by partial update"
    );
    assert!(
        partial.requirement_validated,
        "validated flag must be unchanged by partial update"
    );

    // find_by_requirement returns the annotated node.
    let by_req = store
        .find_by_requirement("REQ-1")
        .expect("find_by_requirement");
    assert!(
        by_req.iter().any(|n| n.symbol == sym("sem_fn")),
        "find_by_requirement(\"REQ-1\") must return sem_fn"
    );

    // set_node_semantics on an absent symbol is a no-op (must not error).
    store
        .set_node_semantics(
            &sym("no_such_symbol"),
            Some("desc"),
            Some("REQ-X"),
            Some(&ValidationClaim::new(false, "conformance-retractor").expect("named actor")),
        )
        .expect("set_node_semantics on absent symbol must be a no-op");

    // ── Typed annotations ─────────────────────────────────────────────────────
    // Every GraphStore must round-trip typed key/value annotations, support many per symbol,
    // filter by type, treat custom types identically to known ones, default untyped→"note", and
    // scope deletes by (type, key). Uses fresh nodes in files not wiped by earlier remove_file.
    let ann_a = Node::new(
        sym("ann_a"),
        NodeKind::Function,
        "ann_a",
        Language::new("rust"),
        Location::new("src/ann.rs", Span::ZERO),
    );
    let ann_b = Node::new(
        sym("ann_b"),
        NodeKind::Function,
        "ann_b",
        Language::new("rust"),
        Location::new("src/ann.rs", Span::ZERO),
    );
    store
        .upsert_nodes(&[ann_a, ann_b])
        .expect("upsert annotation nodes");

    // Before any annotation: annotations() is empty (not an error).
    let empty = store
        .annotations(&sym("ann_a"))
        .expect("annotations before any write");
    assert!(
        empty.is_empty(),
        "annotations must be empty before any write"
    );

    // (1) Typed round-trip: write an assumption, read it back with all fields intact.
    //
    // FRACTIONAL-CONFIDENCE PRECISION CONTRACT (cross-backend). `Annotation.confidence` is `f64`
    // in core. The SQLite default stores it in a `REAL` column, which in SQLite is an 8-byte
    // IEEE-754 double — so a fraction round-trips (near-)exactly. The Postgres backend stores it
    // in `REAL`, which in Postgres is a 4-byte single (`f32`) — so the same value narrows f64→f32
    // on write and widens back on read. `0.6` is chosen precisely because it is NOT representable
    // exactly in f32: it reads back as `0.6000000238…` on Postgres (error ≈ 2.4e-8), whereas on
    // SQLite it is exact. Asserting with a tight `1e-9` tolerance (as a naive round-trip test
    // would) silently passes on SQLite but FAILS on Postgres — the exact narrowing the conformance
    // kit must make explicit rather than hide. We therefore assert with an f32-epsilon-scale
    // tolerance that holds on BOTH backends, and pin the precision expectation here as the
    // single source of truth. (Edge `Confidence` is already `f32` in core, so edges round-trip
    // losslessly on every backend; only this annotation field narrows.)
    const CONFIDENCE_RT_TOL: f64 = 1e-6; // > f32 machine epsilon (~1.19e-7); holds for f64 and f32 stores.
    store
        .annotate(
            &sym("ann_a"),
            Annotation::new("assumption", "thread-safety", "assumed Send+Sync")
                .with_confidence(0.6)
                .with_provenance("manual")
                .with_author("alice"),
        )
        .expect("annotate assumption");
    let got = store
        .annotations(&sym("ann_a"))
        .expect("annotations after assumption");
    assert_eq!(got.len(), 1, "exactly one annotation on ann_a so far");
    assert_eq!(got[0].r#type, "assumption", "type must round-trip");
    assert_eq!(got[0].key, "thread-safety");
    assert_eq!(got[0].value, "assumed Send+Sync");
    assert!(
        (got[0].confidence - 0.6).abs() < CONFIDENCE_RT_TOL,
        "fractional confidence 0.6 must round-trip within f32 tolerance \
         (SQLite REAL=f64 is near-exact; Postgres REAL=f32 narrows to ~0.60000002); got {}",
        got[0].confidence
    );
    assert_eq!(got[0].provenance, "manual", "provenance must round-trip");
    assert_eq!(got[0].author, "alice", "author must round-trip");
    assert_eq!(
        classify(&got[0].r#type),
        AnnotationClass::Assumption,
        "type classifies correctly"
    );

    // (2) Multiple annotations per symbol (bare INSERT, not upsert) — including a duplicate key.
    store
        .annotate(
            &sym("ann_a"),
            Annotation::note("thread-safety", "see PR #12"),
        )
        .expect("annotate note with duplicate key");
    store
        .annotate(
            &sym("ann_a"),
            Annotation::new("question", "ownership", "who frees this?"),
        )
        .expect("annotate question");
    let many = store
        .annotations(&sym("ann_a"))
        .expect("annotations after three writes");
    assert_eq!(
        many.len(),
        3,
        "three annotations must coexist on ann_a (bare INSERT, not upsert); got {}",
        many.len()
    );

    // (3) Default type: an untyped row (via Annotation::note) reads back as "note".
    let notes: Vec<&Annotation> = many.iter().filter(|a| a.r#type == "note").collect();
    assert_eq!(
        notes.len(),
        1,
        "exactly one note-typed annotation; got {}",
        notes.len()
    );
    assert_eq!(notes[0].value, "see PR #12");

    // (4) Custom / unknown type round-trips identically and classifies as Custom.
    store
        .annotate(
            &sym("ann_b"),
            Annotation::new("adr-ref", "decision", "ADR-002 stable identity").with_author("bob"),
        )
        .expect("annotate custom type");
    let custom = store
        .annotations(&sym("ann_b"))
        .expect("annotations for custom type");
    assert_eq!(custom.len(), 1, "one custom annotation on ann_b");
    assert_eq!(
        custom[0].r#type, "adr-ref",
        "custom type string must round-trip verbatim"
    );
    assert_eq!(custom[0].value, "ADR-002 stable identity");
    assert_eq!(
        classify(&custom[0].r#type),
        AnnotationClass::Custom,
        "unknown type must classify as Custom"
    );

    // (5) Type filter: annotations_by_type returns the right set across symbols.
    // Add one more assumption on ann_b so the "assumption" set spans two symbols.
    store
        .annotate(
            &sym("ann_b"),
            Annotation::new("assumption", "lifetime", "assumed 'static"),
        )
        .expect("annotate second assumption");
    let assumptions = store
        .annotations_by_type("assumption")
        .expect("annotations_by_type assumption");
    assert_eq!(
        assumptions.len(),
        2,
        "two assumptions across ann_a + ann_b; got {}",
        assumptions.len()
    );
    assert!(
        assumptions.iter().all(|(_, a)| a.r#type == "assumption"),
        "type filter must only return matching-type rows"
    );
    let assumption_syms: std::collections::HashSet<_> =
        assumptions.iter().map(|(s, _)| s.clone()).collect();
    assert!(
        assumption_syms.contains(&sym("ann_a")) && assumption_syms.contains(&sym("ann_b")),
        "assumption filter must span both annotated symbols"
    );

    // A type with no rows returns an empty vec, not an error.
    let none = store
        .annotations_by_type("no-such-type")
        .expect("annotations_by_type empty");
    assert!(none.is_empty(), "unknown type filter must return empty vec");

    // Custom-type filter also works through the same path.
    let custom_filter = store
        .annotations_by_type("adr-ref")
        .expect("annotations_by_type custom");
    assert_eq!(
        custom_filter.len(),
        1,
        "custom-type filter returns the one adr-ref row"
    );
    assert_eq!(custom_filter[0].0, sym("ann_b"));

    // (6) Scoped delete: delete only (type=note, key=thread-safety) on ann_a.
    // The assumption with the SAME key must survive (scoping by type protects it).
    let deleted = store
        .delete_annotations(&sym("ann_a"), Some("note"), "thread-safety")
        .expect("scoped delete by (type,key)");
    assert_eq!(deleted, 1, "exactly the one note row must be deleted");
    let after_scoped = store
        .annotations(&sym("ann_a"))
        .expect("annotations after scoped delete");
    assert_eq!(
        after_scoped.len(),
        2,
        "two annotations remain on ann_a after scoped delete; got {}",
        after_scoped.len()
    );
    assert!(
        after_scoped
            .iter()
            .any(|a| a.r#type == "assumption" && a.key == "thread-safety"),
        "the assumption sharing the key must survive a note-scoped delete"
    );
    assert!(
        !after_scoped.iter().any(|a| a.r#type == "note"),
        "no note-typed annotation may remain after deleting it"
    );

    // (7) Unscoped delete (ty=None): removes ALL rows for the key regardless of type.
    let deleted_all = store
        .delete_annotations(&sym("ann_a"), None, "thread-safety")
        .expect("unscoped delete by key");
    assert_eq!(
        deleted_all, 1,
        "the remaining thread-safety assumption must be deleted unscoped"
    );
    let after_unscoped = store
        .annotations(&sym("ann_a"))
        .expect("annotations after unscoped delete");
    assert!(
        after_unscoped.iter().all(|a| a.key != "thread-safety"),
        "no thread-safety annotation may remain after unscoped delete"
    );

    // (8) annotate on an absent symbol is a no-op (must not error, must store nothing).
    store
        .annotate(&sym("annotation_ghost"), Annotation::note("k", "v"))
        .expect("annotate on absent symbol must be a no-op");
    let ghost = store
        .annotations(&sym("annotation_ghost"))
        .expect("annotations for absent symbol");
    assert!(
        ghost.is_empty(),
        "absent symbol must carry no annotations after a no-op annotate"
    );

    // (9) Evidence envelope — every store must round-trip source_type / extraction_method /
    // last_verified, and must answer the freshness read `annotations_stale_since`. Uses a fresh
    // node (ann_c) so the staleness set is independent of the rows written above.
    let ann_c = Node::new(
        sym("ann_c"),
        NodeKind::Function,
        "ann_c",
        Language::new("rust"),
        Location::new("src/ann.rs", Span::ZERO),
    );
    store.upsert_nodes(&[ann_c]).expect("upsert ann_c");

    // A fully-specified, recently-verified annotation.
    store
        .annotate(
            &sym("ann_c"),
            Annotation::new("observation", "tls", "requires TLS 1.3")
                .with_source_type("static-analysis")
                .with_extraction_method("scip-rust@0.3")
                .with_last_verified(1_000),
        )
        .expect("annotate fresh evidence-enveloped row");
    // A stale (verified long ago) annotation on the same symbol.
    store
        .annotate(
            &sym("ann_c"),
            Annotation::new("observation", "old-fact", "verified ages ago")
                .with_source_type("code")
                .with_extraction_method("manual")
                .with_last_verified(100),
        )
        .expect("annotate stale evidence-enveloped row");

    let ann_c_rows = store.annotations(&sym("ann_c")).expect("ann_c annotations");
    assert_eq!(ann_c_rows.len(), 2, "two evidence-enveloped rows on ann_c");
    let tls = ann_c_rows
        .iter()
        .find(|a| a.key == "tls")
        .expect("tls row present");
    assert_eq!(
        tls.source_type, "static-analysis",
        "source_type must round-trip"
    );
    assert_eq!(
        tls.extraction_method, "scip-rust@0.3",
        "extraction_method must round-trip"
    );
    assert_eq!(tls.last_verified, 1_000, "last_verified must round-trip");

    // Defaulted envelope: an annotation written without the builders reads back with the safe
    // defaults (unspecified / manual / 0 — never verified) — the backward-compat guarantee at the
    // store layer, mirroring the serde defaults on the struct.
    let defaulted = ann_c_rows
        .iter()
        .find(|a| a.r#type == "observation" && a.key == "old-fact")
        .map(|_| Annotation::note("plain", "v"))
        .unwrap();
    store
        .annotate(&sym("ann_c"), defaulted)
        .expect("annotate defaulted-envelope row");
    let plain = store
        .annotations(&sym("ann_c"))
        .expect("re-read ann_c")
        .into_iter()
        .find(|a| a.key == "plain")
        .expect("plain row present");
    assert_eq!(plain.source_type, "unspecified", "default source_type");
    assert_eq!(
        plain.extraction_method, "manual",
        "default extraction_method"
    );
    assert_eq!(
        plain.last_verified, 0,
        "default last_verified (never verified)"
    );

    // Freshness read: cutoff=500 catches the stale (100) and never-verified (0) rows but NOT the
    // freshly-verified (1000) one. Strict `<`, so cutoff exactly == last_verified is not stale.
    let stale = store
        .annotations_stale_since(500)
        .expect("annotations_stale_since(500)");
    assert!(
        stale.iter().any(|(_, a)| a.key == "old-fact"),
        "stale (verified at 100) must be returned for cutoff 500"
    );
    assert!(
        stale.iter().any(|(_, a)| a.key == "plain"),
        "never-verified (last_verified 0) must be returned for cutoff 500"
    );
    assert!(
        !stale.iter().any(|(_, a)| a.key == "tls"),
        "freshly-verified (1000) must NOT be returned for cutoff 500"
    );
    // cutoff exactly at a row's last_verified must NOT include it (strict <).
    let stale_at_1000 = store
        .annotations_stale_since(1_000)
        .expect("annotations_stale_since(1000)");
    assert!(
        !stale_at_1000.iter().any(|(_, a)| a.key == "tls"),
        "verified exactly at the cutoff is NOT stale (strict <)"
    );

    // --- scope isolation (multi-tenant / partition; added last so prior counts are unaffected) ---
    let acme =
        func_node("billing_acme").with_scope(crate::scope::Scope::parse("org:acme/unit:pay"));
    let acme2 = func_node("billing_acme2").with_scope(crate::scope::Scope::parse("org:acme")); // ancestor
    let globex = func_node("billing_globex").with_scope(crate::scope::Scope::parse("org:globex"));
    store
        .upsert_nodes(&[acme, acme2, globex])
        .expect("upsert scoped nodes");

    // A scoped query returns the prefix subtree and NOTHING from another tenant (isolation).
    let scoped = store
        .find_symbols(&SymbolQuery {
            scope_prefix: Some("org:acme".to_string()),
            ..Default::default()
        })
        .expect("scoped find_symbols");
    assert!(
        scoped.iter().any(|n| n.name == "billing_acme")
            && scoped.iter().any(|n| n.name == "billing_acme2"),
        "scoped query must see the org:acme subtree"
    );
    assert!(
        !scoped.iter().any(|n| n.name == "billing_globex"),
        "SCOPE ISOLATION VIOLATED: org:acme query returned an org:globex node"
    );
    assert!(
        scoped
            .iter()
            .all(|n| n.scope.as_path().starts_with("org:acme")),
        "every scoped result must be within the org:acme subtree"
    );

    // Segment-aware: a non-existent sibling-ish prefix must not match by raw string prefix.
    let none = store
        .find_symbols(&SymbolQuery {
            scope_prefix: Some("org:acm".to_string()),
            ..Default::default()
        })
        .expect("scoped find_symbols (partial seg)");
    assert!(
        !none.iter().any(|n| n.name.starts_with("billing_acme")),
        "a partial-segment prefix (org:acm) must NOT leak org:acme nodes"
    );

    // Unscoped query still sees every scope (back-compat: default scope_prefix = None).
    let all_scoped = store
        .find_symbols(&SymbolQuery {
            exact_name: Some("billing_globex".to_string()),
            ..Default::default()
        })
        .expect("unscoped find_symbols");
    assert_eq!(
        all_scoped.len(),
        1,
        "unscoped query sees other-tenant nodes"
    );

    // Scope round-trips through the store (data model).
    let got_acme = store
        .get_node(&sym("billing_acme"))
        .expect("get scoped node")
        .expect("billing_acme exists");
    assert_eq!(
        got_acme.scope.as_path(),
        "org:acme/unit:pay",
        "scope persisted + round-trips"
    );

    // ── SYMBOL EPOCH (M8 / DoD-XA4) — symbol_epoch + the gen bump (the about-arm reuse seam) ──
    // This is the BUILD-GATE for every backend. It exercises ONLY GraphRead/GraphWrite trait
    // methods, so it is backend-generic; the skip-FTS-specific non-vacuous case (the store-crate
    // hot path) is asserted separately in the store crate's concrete test. Fresh symbol names so
    // none of the counts/state above are disturbed.
    let epoch_file = "src/epoch_gate.rs";
    let epoch_node = |name: &str| {
        Node::new(
            sym(name),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new(epoch_file, Span::ZERO),
        )
    };

    // (E0) No live node → no epoch.
    assert_eq!(
        store
            .symbol_epoch(&sym("epoch_absent"))
            .expect("epoch absent"),
        None,
        "symbol_epoch must be None for a symbol that has never been indexed"
    );

    // (E1) NON-SPURIOUS: a symbol that exists ONLY as an edge endpoint (interned, no node) and then
    // gets its FIRST node must be epoch 0 — NOT bumped. If the bump lived in `intern` this would be
    // wrongly >= 1. We create the edge-only state with an edge whose target was never a node, then
    // give that target its first node.
    store
        .upsert_edges(&[calls("epoch_edge_src", "epoch_edge_only_tgt")])
        .expect("edge introducing an interned-but-nodeless target");
    // The edge target has no node yet → no epoch.
    assert_eq!(
        store
            .symbol_epoch(&sym("epoch_edge_only_tgt"))
            .expect("epoch edge-only target"),
        None,
        "an edge-endpoint-only symbol (interned, no node) must have no epoch"
    );
    // Now its FIRST node arrives.
    store
        .upsert_nodes(&[epoch_node("epoch_edge_only_tgt")])
        .expect("first node for a previously edge-only symbol");
    assert_eq!(
        store
            .symbol_epoch(&sym("epoch_edge_only_tgt"))
            .expect("epoch after first node for edge-only symbol"),
        Some(0),
        "NON-SPURIOUS: a first-ever node for an edge-only symbol must be epoch 0 (the bump must NOT \
         live in intern — interning happens for edge endpoints too)"
    );

    // (E2) A plain first-ever node is also epoch 0.
    store
        .upsert_nodes(&[epoch_node("epoch_reused")])
        .expect("first-ever node for epoch_reused");
    assert_eq!(
        store
            .symbol_epoch(&sym("epoch_reused"))
            .expect("epoch first"),
        Some(0),
        "a first-ever node must be epoch 0"
    );
    // Re-upserting the SAME live node is an update, not a reuse — epoch must NOT advance.
    store
        .upsert_nodes(&[epoch_node("epoch_reused")])
        .expect("re-upsert live node");
    assert_eq!(
        store
            .symbol_epoch(&sym("epoch_reused"))
            .expect("epoch after live re-upsert"),
        Some(0),
        "re-upserting a LIVE node is an update, not a reuse — epoch must stay 0"
    );

    // (E3) NON-VACUOUS (trait path): delete the symbol's node (via remove_file), then re-add the
    // SAME name → epoch must be Some(g) with g >= 1. While deleted, the epoch is None.
    store.remove_file(epoch_file).expect("remove epoch file");
    assert_eq!(
        store
            .symbol_epoch(&sym("epoch_reused"))
            .expect("epoch while deleted"),
        None,
        "a removed symbol (no live node) must have no epoch"
    );
    store
        .upsert_nodes(&[epoch_node("epoch_reused")])
        .expect("re-add after delete");
    let reused = store
        .symbol_epoch(&sym("epoch_reused"))
        .expect("epoch after reuse")
        .expect("re-added symbol must have a live epoch");
    assert!(
        reused >= 1,
        "NON-VACUOUS: epoch after delete-then-re-add must be >= 1 (the gen bump fired); got {reused}"
    );

    // (E4) A second delete-then-re-add advances the epoch again (strictly monotonic per reuse).
    let before = reused;
    store
        .remove_file(epoch_file)
        .expect("remove epoch file (2)");
    store
        .upsert_nodes(&[epoch_node("epoch_reused")])
        .expect("re-add after delete (2)");
    let reused2 = store
        .symbol_epoch(&sym("epoch_reused"))
        .expect("epoch after second reuse")
        .expect("still live");
    assert!(
        reused2 > before,
        "epoch must strictly advance on each reuse: {before} -> {reused2}"
    );

    // (E5) The edge-only-then-first-node symbol (E1) was NOT touched by the reuse cycle above (it is
    // also in epoch_file, so remove_file deleted it too); re-adding it bumps it from 0 → >=1, proving
    // its initial 0 was a genuine first-ever, not a missed bump.
    let edge_then_node = store
        .symbol_epoch(&sym("epoch_edge_only_tgt"))
        .expect("epoch edge-only after the remove cycles");
    // After the two remove_file calls it has no live node.
    assert_eq!(
        edge_then_node, None,
        "the edge-only-then-node symbol was removed with the file; no live epoch"
    );
    store
        .upsert_nodes(&[epoch_node("epoch_edge_only_tgt")])
        .expect("re-add edge-only-then-node symbol");
    let edge_reused = store
        .symbol_epoch(&sym("epoch_edge_only_tgt"))
        .expect("epoch edge-only re-added")
        .expect("live");
    assert!(
        edge_reused >= 1,
        "a symbol whose FIRST node was epoch 0 must bump to >=1 once it is deleted and re-added; \
         got {edge_reused}"
    );

    // ── Shared-Import remove_file semantics (incr-integrity lane) ────────────────────────────
    // An Import node is keyed by module SPECIFIER and shared by every importer of the same
    // spec, so remove_file must KEEP it while survivor edges target it, RE-HOME it (both the
    // file column and the data-JSON location — the JSON path is asserted by the get_node
    // read-back, the column path by the last-importer delete matching the new home), and
    // delete it through the normal path once the last importer is gone. The suite runs with
    // history ON, so the owner-removal case also pins the archival boundary: the owner's own
    // File→Import edge is archived, surviving importers' edges stay live and unarchived.
    let import_node = |name: &str, home: &str| {
        Node::new(
            sym(name),
            NodeKind::Import,
            name,
            Language::new("rust"),
            Location::new(home, Span::ZERO),
        )
    };
    let file_node = |name: &str, path: &str| {
        Node::new(
            sym(name),
            NodeKind::File,
            name,
            Language::new("rust"),
            Location::new(path, Span::ZERO),
        )
    };
    let imports = |a: &str, b: &str, at: &str| {
        Edge::new(
            sym(a),
            sym(b),
            EdgeKind::Imports,
            ResolutionTier::Parsed,
            "conformance",
        )
        .with_location(Location::new(at, Span::ZERO))
    };
    let edges_to = |store: &S, name: &str| -> Vec<Edge> {
        store
            .all_edges()
            .expect("all_edges")
            .into_iter()
            .filter(|e| e.target == sym(name))
            .collect()
    };

    // (SI-1) Owner removal keeps + re-homes; last-importer removal deletes (falsifier 7).
    store.begin_batch().expect("begin si1");
    store
        .upsert_nodes(&[
            file_node("si_file_a", "imp/a.rs"),
            file_node("si_file_b", "imp/b.rs"),
            import_node("si_import", "imp/b.rs"), // homed at imp/b.rs (its one contribution)
        ])
        .expect("upsert si1 nodes");
    store
        .upsert_edges(&[
            imports("si_file_a", "si_import", "imp/a.rs"),
            imports("si_file_b", "si_import", "imp/b.rs"),
        ])
        .expect("upsert si1 edges");
    store.commit_batch().expect("commit si1");

    store
        .remove_file("imp/b.rs")
        .expect("remove owner imp/b.rs");
    let kept = store
        .get_node(&sym("si_import"))
        .expect("get_node si_import")
        .expect("shared Import node must SURVIVE the owner's removal while imp/a.rs imports it");
    assert_eq!(
        kept.location.file, "imp/a.rs",
        "kept Import node must be re-homed to the surviving importer (data-JSON location)"
    );
    let to_import = edges_to(store, "si_import");
    assert_eq!(
        to_import.len(),
        1,
        "exactly the surviving importer's edge remains: {to_import:?}"
    );
    assert_eq!(
        to_import[0].source,
        sym("si_file_a"),
        "the surviving edge is imp/a.rs's File→Import edge"
    );
    assert!(
        store
            .get_node(&sym("si_file_b"))
            .expect("get_node si_file_b")
            .is_none(),
        "the owner's File node itself is removed unconditionally"
    );
    // History boundary: the owner's edge was archived under the OWNER's file; the survivor's
    // edge is live and outside imp/a.rs's archive set.
    let hist_b = store.edge_history("imp/b.rs").expect("edge_history b");
    assert!(
        hist_b
            .iter()
            .any(|h| h.edge.source == sym("si_file_b") && h.edge.target == sym("si_import")),
        "the removed owner's File→Import edge must be archived under imp/b.rs"
    );
    let hist_a = store.edge_history("imp/a.rs").expect("edge_history a");
    assert!(
        !hist_a
            .iter()
            .any(|h| h.edge.source == sym("si_file_a") && h.edge.target == sym("si_import")),
        "the SURVIVING importer's edge must NOT be archived (it is live)"
    );

    // Last importer: the earlier re-home moved the FILE COLUMN too, or this delete would not
    // match the node and it would island forever.
    store
        .remove_file("imp/a.rs")
        .expect("remove last importer imp/a.rs");
    assert!(
        store
            .get_node(&sym("si_import"))
            .expect("get_node si_import after last importer")
            .is_none(),
        "removing the LAST importer must delete the Import node (self-terminating re-home; \
         a column-only or JSON-only re-home fails here)"
    );
    assert!(
        edges_to(store, "si_import").is_empty(),
        "no edges to the deleted Import node may remain"
    );
    assert!(
        !store
            .all_nodes()
            .expect("all_nodes after si1")
            .iter()
            .any(|n| n.symbol == sym("si_import")),
        "no island Import node may remain (falsifier 7)"
    );

    // (SI-2) One-run BATCH delete of owner + a non-owner with a third importer surviving: the
    // keep-check and re-home are evaluated PER remove_file CALL against current rows, never a
    // batch-start snapshot — a snapshot would re-home onto the doomed sibling (imp2/c1).
    store.begin_batch().expect("begin si2");
    store
        .upsert_nodes(&[
            file_node("si2_c1", "imp2/c1.rs"),
            file_node("si2_c2", "imp2/c2.rs"),
            file_node("si2_c3", "imp2/c3.rs"),
            import_node("si2_import", "imp2/c3.rs"), // owner: c3
        ])
        .expect("upsert si2 nodes");
    store
        .upsert_edges(&[
            imports("si2_c1", "si2_import", "imp2/c1.rs"),
            imports("si2_c2", "si2_import", "imp2/c2.rs"),
            imports("si2_c3", "si2_import", "imp2/c3.rs"),
        ])
        .expect("upsert si2 edges");
    store.commit_batch().expect("commit si2");

    store.begin_batch().expect("begin si2 removal");
    store.remove_file("imp2/c3.rs").expect("batch remove owner");
    store
        .remove_file("imp2/c1.rs")
        .expect("batch remove interim home");
    store.commit_batch().expect("commit si2 removal");
    let kept2 = store
        .get_node(&sym("si2_import"))
        .expect("get_node si2_import")
        .expect("Import node must survive a batch that leaves one importer alive");
    assert_eq!(
        kept2.location.file, "imp2/c2.rs",
        "per-call re-home: after removing c3 (home→c1) and then c1, the node lives at c2 — \
         a batch-start snapshot would have left it homed at the deleted c1"
    );
    let to2 = edges_to(store, "si2_import");
    assert_eq!(
        to2.len(),
        1,
        "only the live importer's edge remains: {to2:?}"
    );
    assert_eq!(to2[0].source, sym("si2_c2"), "c2's edge survives");
    store
        .remove_file("imp2/c2.rs")
        .expect("remove si2 last importer");
    assert!(
        store
            .get_node(&sym("si2_import"))
            .expect("get_node si2_import end")
            .is_none(),
        "si2 cleanup: last importer removal deletes the node"
    );

    // (SI-3) One-run batch removing ALL importers: node gone, no island (falsifier 7, batch shape).
    store.begin_batch().expect("begin si3");
    store
        .upsert_nodes(&[
            file_node("si3_d1", "imp3/d1.rs"),
            file_node("si3_d2", "imp3/d2.rs"),
            import_node("si3_import", "imp3/d1.rs"), // owner: d1 — removed FIRST below
        ])
        .expect("upsert si3 nodes");
    store
        .upsert_edges(&[
            imports("si3_d1", "si3_import", "imp3/d1.rs"),
            imports("si3_d2", "si3_import", "imp3/d2.rs"),
        ])
        .expect("upsert si3 edges");
    store.commit_batch().expect("commit si3");
    store.begin_batch().expect("begin si3 removal");
    store.remove_file("imp3/d1.rs").expect("batch remove d1");
    store.remove_file("imp3/d2.rs").expect("batch remove d2");
    store.commit_batch().expect("commit si3 removal");
    assert!(
        store
            .get_node(&sym("si3_import"))
            .expect("get_node si3_import")
            .is_none(),
        "a one-run batch removing ALL importers must leave no island Import node"
    );
    assert!(
        edges_to(store, "si3_import").is_empty(),
        "no edges to si3_import may remain after all importers are gone"
    );

    // (SI-4) A locationless ('') edge is NOT a survivor (falsifier 8): it must neither keep the
    // node alive nor win the MIN(file) re-home ('' sorts before every real path but is a path
    // no remove_file call ever matches).
    store.begin_batch().expect("begin si4");
    store
        .upsert_nodes(&[
            file_node("si4_owner", "imp4/o.rs"),
            file_node("si4_a", "imp4/a.rs"),
            func_node("si4_z"), // lives in src/lib.rs — outside the removed files
            import_node("si4_import", "imp4/o.rs"), // owner: o.rs
        ])
        .expect("upsert si4 nodes");
    store
        .upsert_edges(&[
            imports("si4_owner", "si4_import", "imp4/o.rs"),
            imports("si4_a", "si4_import", "imp4/a.rs"),
            // Locationless edge (schema-legal): no with_location → file '' in column stores.
            Edge::new(
                sym("si4_z"),
                sym("si4_import"),
                EdgeKind::Imports,
                ResolutionTier::Parsed,
                "conformance",
            ),
        ])
        .expect("upsert si4 edges");
    store.commit_batch().expect("commit si4");

    store.remove_file("imp4/o.rs").expect("remove si4 owner");
    let kept4 = store
        .get_node(&sym("si4_import"))
        .expect("get_node si4_import")
        .expect("si4 node survives via the real-file survivor");
    assert_eq!(
        kept4.location.file, "imp4/a.rs",
        "re-home must pick the real survivor file, never '' (falsifier 8)"
    );
    store
        .remove_file("imp4/a.rs")
        .expect("remove si4 last real importer");
    assert!(
        store
            .get_node(&sym("si4_import"))
            .expect("get_node si4_import after real importers gone")
            .is_none(),
        "a node referenced ONLY by a locationless ('') edge is DELETED, not kept (falsifier 8)"
    );
    // The '' edge's fate is store-specific (it dangles in column stores until pruned); prune to
    // leave the suite's graph clean either way.
    store
        .prune_dangling_edges()
        .expect("prune after si4 cleanup");

    // --- DEPTH-HORIZON HONESTY (wicked-estate#190) ---
    // Inline (not a separate entry point) so EVERY existing `graph_store_suite` caller — MemStore,
    // SqliteStore, SurrealStore, PostgresStore, the team-runtime store — is gated on it without
    // each backend's test file having to opt in. The `dh_*` fixture is symbol-disjoint from
    // everything above.
    traverse_reports_depth_horizon(store);

    // --- remove_file deletes BOTH predicate halves ---
    // The §4 contract: `remove_file(f)` removes an edge located in `f` AND a location-less edge
    // whose source node lives in `f`. Only the first half was exercised above, and SurrealStore's
    // predicate DELETE silently matched only that half (found during TS-S2A). `rf_*` is
    // symbol-disjoint from everything above.
    let rf_node = |name: &str, file: &str| {
        Node::new(
            sym(name),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new(file, Span::ZERO),
        )
    };
    store
        .upsert_nodes(&[
            rf_node("rf_src", "rf/removed.rs"),
            rf_node("rf_a", "rf/kept.rs"),
            rf_node("rf_b", "rf/kept.rs"),
        ])
        .expect("rf nodes");
    let sourced = calls("rf_src", "rf_a"); // no location: owned through its source node
    let located = calls("rf_a", "rf_b").with_location(Location::new("rf/removed.rs", Span::ZERO));
    let kept = calls("rf_b", "rf_a").with_location(Location::new("rf/kept.rs", Span::ZERO));
    store
        .upsert_edges(&[sourced, located, kept.clone()])
        .expect("rf edges");
    store.remove_file("rf/removed.rs").expect("remove rf file");
    let left: Vec<(String, String)> = store
        .all_edges()
        .expect("all edges")
        .into_iter()
        .filter(|e| e.source.0.contains("rf_") || e.target.0.contains("rf_"))
        .map(|e| (e.source.0, e.target.0))
        .collect();
    assert_eq!(
        left,
        vec![(kept.source.0, kept.target.0)],
        "remove_file must delete the source-owned edge AND the located edge, and nothing else"
    );
}

/// Multi-file symbol contributions (M4 / Option A — wicked-estate#152). Run on a FRESH store,
/// like [`traverse_multi_matches_union_of_traverse`]; a separate suite so backends outside the
/// shipped trio can adopt it independently of [`graph_store_suite`].
///
/// ONE logical symbol may be contributed by MORE THAN ONE file — the live case is C/C++: a `.h`
/// member prototype and its out-of-line `.cpp` definition mint one SymbolId across two files
/// (ADR-002 scheme 3; extract-level pin `cpp_member_proto_def_cross_file_single_id_hazard`, which
/// this suite retires per its M4 flip instruction). The store contract this suite pins:
///
/// 1. **Derived primary, never last-write-wins**: the node's `location`/`kind`/record equal the
///    PREFERRED contribution — definition before declaration (`Node::is_declaration` metadata),
///    lexicographic file tiebreak — regardless of extraction ORDER. The incremental file/kind
///    flap is dead.
/// 2. **`remove_file` deletes CONTRIBUTIONS**: a node with surviving contributions is re-homed
///    wholesale to the surviving preferred record; only a contribution-less node is deleted.
/// 3. **Zero id churn**: edges keyed by the SymbolId survive either file's removal untouched,
///    and the symbol's epoch never advances while any contribution survives (the node was never
///    deleted, so a later re-add is an update, not a reuse).
pub fn multi_file_contribution_suite<S: GraphStore>(store: &mut S) {
    const DECL_FILE: &str = "mf/a_header.h"; // lexicographically FIRST — a file-order tiebreak
    const DEF_FILE: &str = "mf/z_impl.cpp"; //  alone would wrongly pick the header.
    let span_at = |b: u32| Span {
        start_byte: b,
        end_byte: b + 4,
        start_line: b,
        start_col: 0,
        end_line: b,
        end_col: 4,
    };
    // The declaration contribution: header prototype — marked via the metadata flag, with its own
    // kind/doc/span so wholesale projection (never a field merge) is observable.
    let decl = {
        let mut n = Node::new(
            sym("mf_reset"),
            NodeKind::Method,
            "mf_reset",
            Language::new("cpp"),
            Location::new(DECL_FILE, span_at(1)),
        )
        .as_declaration();
        n.doc = Some("decl doc".to_string());
        n
    };
    // The definition contribution: out-of-line impl. A DIFFERENT kind pins the deterministic kind
    // reconciliation (the primary contribution's kind wins — no cross-run kind flap).
    let def = {
        let mut n = Node::new(
            sym("mf_reset"),
            NodeKind::Function,
            "mf_reset",
            Language::new("cpp"),
            Location::new(DEF_FILE, span_at(7)),
        );
        n.doc = Some("def doc".to_string());
        n
    };
    let caller = Node::new(
        sym("mf_caller"),
        NodeKind::Function,
        "mf_caller",
        Language::new("cpp"),
        Location::new("mf/caller.cpp", Span::ZERO),
    );

    // (MF-1) Index both files, WORST order for last-write-wins: definition first, declaration
    // last. One node; the primary must be the DEFINITION record even though (a) the declaration
    // wrote last and (b) the declaration's file sorts first.
    store.begin_batch().expect("begin mf1");
    store
        .upsert_nodes(&[def.clone(), caller.clone()])
        .expect("upsert def + caller");
    store
        .upsert_nodes(std::slice::from_ref(&decl))
        .expect("upsert decl");
    store
        .upsert_edges(&[calls("mf_caller", "mf_reset")])
        .expect("upsert caller edge");
    store.commit_batch().expect("commit mf1");

    let one = |store: &S| -> Vec<Node> {
        store
            .all_nodes()
            .expect("all_nodes")
            .into_iter()
            .filter(|n| n.symbol == sym("mf_reset"))
            .collect()
    };
    assert_eq!(
        one(store).len(),
        1,
        "proto + def are ONE logical symbol — exactly one node row"
    );
    let got = store
        .get_node(&sym("mf_reset"))
        .expect("get_node mf_reset")
        .expect("node exists");
    assert_eq!(
        got.location.file, DEF_FILE,
        "primary must be the DEFINITION contribution — not the last writer, not the \
         lexicographically-first file"
    );
    assert_eq!(
        got.kind,
        NodeKind::Function,
        "kind is the primary (definition) contribution's kind — deterministic reconciliation"
    );
    assert_eq!(
        got.doc.as_deref(),
        Some("def doc"),
        "the primary record projects WHOLESALE (definition's doc, no field merge)"
    );

    // The flap is dead: re-writing the declaration again (an incremental re-index of the header)
    // must NOT steal the primary.
    store
        .upsert_nodes(std::slice::from_ref(&decl))
        .expect("re-upsert decl");
    let got = store
        .get_node(&sym("mf_reset"))
        .expect("get_node after decl re-upsert")
        .expect("node exists");
    assert_eq!(
        got.location.file, DEF_FILE,
        "LAST-WRITE-WINS FLAP: re-indexing the header stole the primary from the definition"
    );

    // (MF-2) Remove the HEADER: the node survives (definition contribution remains), stays
    // definition-primary, and the edge keyed by the SymbolId is untouched (zero id churn).
    store.remove_file(DECL_FILE).expect("remove header");
    let got = store
        .get_node(&sym("mf_reset"))
        .expect("get_node after header removal")
        .expect("node must SURVIVE the header's removal — the .cpp still contributes it");
    assert_eq!(got.location.file, DEF_FILE, "still definition-primary");
    let dependents = store
        .neighbors(&sym("mf_reset"), Direction::Dependents)
        .expect("dependents after header removal");
    assert_eq!(
        dependents.len(),
        1,
        "the caller's edge survives the header removal untouched (zero id churn)"
    );
    assert_eq!(dependents[0].source, sym("mf_caller"));
    assert_eq!(
        store
            .symbol_epoch(&sym("mf_reset"))
            .expect("epoch after header removal"),
        Some(0),
        "a survivor was never deleted — its epoch must not advance"
    );

    // (MF-3) Idempotent re-index of the header (the incremental path: remove_file, then re-upsert
    // the same extraction), twice. Primary stays put; still exactly one node; epoch still 0.
    for round in 0..2 {
        store.remove_file(DECL_FILE).expect("re-index remove");
        store
            .upsert_nodes(std::slice::from_ref(&decl))
            .expect("re-index upsert");
        let got = store
            .get_node(&sym("mf_reset"))
            .expect("get_node during re-index")
            .expect("node exists");
        assert_eq!(
            got.location.file, DEF_FILE,
            "idempotent re-index round {round}: primary must be stable"
        );
        assert_eq!(one(store).len(), 1, "re-index round {round}: one node");
    }
    assert_eq!(
        store
            .symbol_epoch(&sym("mf_reset"))
            .expect("epoch after re-index rounds"),
        Some(0),
        "re-indexing a contributing file never deletes the node — epoch stays 0"
    );

    // (MF-4) Remove the IMPL: the node survives RE-HOMED to the declaration contribution —
    // declaration-primary, the declaration's kind/doc/flag, edges still intact.
    store.remove_file(DEF_FILE).expect("remove impl");
    let got = store
        .get_node(&sym("mf_reset"))
        .expect("get_node after impl removal")
        .expect("node must survive re-homed to the surviving declaration");
    assert_eq!(
        got.location.file, DECL_FILE,
        "re-homed to the surviving (declaration) contribution"
    );
    assert_eq!(
        got.kind,
        NodeKind::Method,
        "kind follows the new primary (declaration) contribution"
    );
    assert_eq!(
        got.doc.as_deref(),
        Some("decl doc"),
        "the surviving record projects wholesale"
    );
    assert!(
        got.is_declaration(),
        "the projected record IS the declaration contribution (metadata flag intact)"
    );
    assert_eq!(
        store
            .neighbors(&sym("mf_reset"), Direction::Dependents)
            .expect("dependents after impl removal")
            .len(),
        1,
        "the caller's edge survives the impl removal too (zero id churn)"
    );

    // (MF-5) Remove the header as well — the LAST contribution: now the node is deleted, no
    // island, and the epoch machinery sees a real delete (a later re-add would be a reuse).
    store.remove_file(DECL_FILE).expect("remove last file");
    assert!(
        store
            .get_node(&sym("mf_reset"))
            .expect("get_node after last removal")
            .is_none(),
        "removing the LAST contribution deletes the node"
    );
    assert!(one(store).is_empty(), "no island node may remain");
    assert_eq!(
        store
            .symbol_epoch(&sym("mf_reset"))
            .expect("epoch after true delete"),
        None,
        "a deleted symbol has no live epoch"
    );

    // (MF-6) Two DEFINITION contributions, no declaration markers (the pre-marking C/C++ reality):
    // the primary is the deterministic lexicographic MIN file, INDEPENDENT of upsert order. Two
    // symbols with mirrored orders pin order-independence.
    let two_def = |name: &str, file: &str| {
        Node::new(
            sym(name),
            NodeKind::Function,
            name,
            Language::new("cpp"),
            Location::new(file, Span::ZERO),
        )
    };
    store
        .upsert_nodes(&[two_def("mf_tie", "mf2/b.cpp")])
        .expect("tie: b first");
    store
        .upsert_nodes(&[two_def("mf_tie", "mf2/a.cpp")])
        .expect("tie: a second");
    store
        .upsert_nodes(&[two_def("mf_tie_rev", "mf2/a.cpp")])
        .expect("tie rev: a first");
    store
        .upsert_nodes(&[two_def("mf_tie_rev", "mf2/b.cpp")])
        .expect("tie rev: b second");
    for name in ["mf_tie", "mf_tie_rev"] {
        let got = store
            .get_node(&sym(name))
            .expect("get_node tie")
            .expect("tie node exists");
        assert_eq!(
            got.location.file, "mf2/a.cpp",
            "{name}: equal-role contributions must resolve to the deterministic MIN(file), \
             independent of write order"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TS-S2A — authoritative, replaceable edge support
// ─────────────────────────────────────────────────────────────────────────────

/// File the support suite's endpoint nodes live in — distinct from every fact/base file below, so
/// a `remove_file` of a fact's file never removes an endpoint node by accident.
const SUP_NODE_FILE: &str = "sup/nodes.ts";

fn sup_owner(producer: &str, snapshot: &str) -> crate::support::SupportOwner {
    crate::support::SupportOwner::new(producer, snapshot).expect("valid owner")
}

/// Replace an owner's support with plain edges as facts (content-derived ids), the shape most
/// cases below need. The opaque-id cases build [`crate::support::SupportFact`]s directly.
fn sup_replace<S: GraphStore>(
    store: &mut S,
    owner: &crate::support::SupportOwner,
    generation: u64,
    edges: &[Edge],
) -> crate::error::Result<crate::support::SupportReplacement> {
    let facts = edges
        .iter()
        .map(|e| crate::support::SupportFact::from_edge(e.clone()))
        .collect::<crate::error::Result<Vec<_>>>()?;
    store.replace_edge_supports(owner, generation, &facts)
}

/// One single-fact `flows_to` edge `source → target` at `file:byte`, with a construct name (which
/// leads the support-row order) and a resolution tier (which sets its confidence).
fn sup_flow(
    source: &str,
    target: &str,
    construct: &str,
    file: &str,
    byte: u32,
    tier: ResolutionTier,
    resolved_by: &str,
) -> Edge {
    use crate::flow::{FlowEvidence, FlowFact, FlowSemantics};
    let mut e = Edge::new(
        sym(source),
        sym(target),
        crate::edge_tags::other(crate::edge_tags::FLOWS_TO),
        tier,
        resolved_by,
    )
    .with_location(Location::new(
        file,
        Span {
            start_byte: byte,
            end_byte: byte + 1,
            start_line: byte,
            end_line: byte,
            ..Span::ZERO
        },
    ));
    FlowFact::new(
        FlowSemantics::ValuePreserving,
        FlowEvidence::Syntax,
        construct,
        "typescript",
    )
    .apply(&mut e);
    e
}

/// The public edge for `(source, target, kind)`, read through the ordinary graph read path.
fn sup_public<S: GraphStore>(
    store: &S,
    source: &str,
    target: &str,
    kind: &EdgeKind,
) -> Option<Edge> {
    store
        .neighbors(&sym(source), Direction::Dependencies)
        .expect("neighbors")
        .into_iter()
        .find(|e| e.target == sym(target) && &e.kind == kind)
}

fn sup_rows<S: GraphStore>(
    store: &S,
    source: &str,
    target: &str,
    kind: &EdgeKind,
) -> Vec<crate::support::EdgeSupport> {
    store
        .edge_supports(&sym(source), &sym(target), kind)
        .expect("edge_supports")
}

/// An edge with its endpoints renamed, so projections on two different keys can be compared.
fn sup_rebind(mut e: Edge) -> Edge {
    e.source = crate::symbol::SymbolId("S".into());
    e.target = crate::symbol::SymbolId("T".into());
    e
}

fn sup_sites(e: &Edge) -> Vec<u64> {
    e.metadata[crate::flow::FLOW_SUPPORT_KEY]
        .as_array()
        .expect("flow_support array")
        .iter()
        .map(|r| r["start_byte"].as_u64().expect("start_byte"))
        .collect()
}

fn sup_truncated(e: &Edge) -> u64 {
    e.metadata
        .get(crate::flow::FLOW_SUPPORT_TRUNCATED_KEY)
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

/// TS-S2A: the authoritative, replaceable support plane (`crate::support`,
/// `docs/ENGINE-CONTRACT.md` §3.4). Run on a FRESH store. Every shipped backend runs it; a new
/// backend must pass it alongside [`graph_store_suite`].
///
/// Pinned, each against the public read path (`neighbors`) AND the authoritative one
/// (`edge_supports`): identical replay is idempotent; `{a,b}→{b,c}` retracts only `a` and keeps
/// `b` once; an empty replacement retracts everything the owner held; two producers on one edge
/// are independent; a failed replacement (invalid fact, generation conflict, stale generation —
/// inside and outside a batch) leaves no half-old/half-new state; input order never matters;
/// staged and one-shot replacement agree (exactly for history, up to the cap for pre-merged
/// input, and the over-count past it is pinned as the boundary); repeated folds are stable;
/// eviction from the bounded sample cannot change identity or a later representative; and the
/// base plane coexists — `upsert_edges`, `remove_file` and `prune_dangling_edges` neither erase
/// support nor are erased by it, and retracting the last support restores the base edge exactly.
pub fn support_replacement_suite<S: GraphStore>(store: &mut S) {
    use crate::flow::{
        FLOW_CONFIDENCE_MIN_KEY, FLOW_SUPPORT_KEY, MAX_FLOW_SUPPORT, merge_flow_edges,
    };
    let flows = crate::edge_tags::other(crate::edge_tags::FLOWS_TO);
    let p = sup_owner("scip-typescript", "apps/web");
    let q = sup_owner("angular-compiler", "apps/web");

    let names = [
        "s_c", "s_a", "s_c2", "s_a2", "s_c3", "s_a3", "s_c4", "s_a4", "s_c5", "s_a5", "s_c6",
        "s_a6", "s_c7", "s_a7", "s_c8", "s_a8", "s_x", "s_y",
    ];
    let nodes: Vec<Node> = names
        .iter()
        .map(|n| {
            Node::new(
                sym(n),
                NodeKind::Variable,
                *n,
                Language::new("typescript"),
                Location::new(SUP_NODE_FILE, Span::ZERO),
            )
        })
        .collect();
    store.upsert_nodes(&nodes).expect("support endpoint nodes");

    let fact = |byte: u32| {
        sup_flow(
            "s_c",
            "s_a",
            "assignment",
            "sup/web.ts",
            byte,
            ResolutionTier::Scip,
            "scip-typescript",
        )
    };
    let (a, b, c) = (fact(10), fact(20), fact(30));

    // ── 1. Identical replay is idempotent ──────────────────────────────────────
    assert_eq!(
        store.support_generation(&p).expect("gen"),
        None,
        "fresh owner has no generation"
    );
    let r1 = sup_replace(store, &p, 1, &[a.clone(), b.clone()]).expect("gen 1");
    assert_eq!(
        (
            r1.replayed,
            r1.asserted,
            r1.retained,
            r1.retracted,
            r1.edges_touched
        ),
        (false, 2, 0, 0, 1),
        "{r1:?}"
    );
    let public_1 = sup_public(store, "s_c", "s_a", &flows).expect("projected edge exists");
    let rows_1 = sup_rows(store, "s_c", "s_a", &flows);
    assert_eq!(rows_1.len(), 2);
    assert_eq!(sup_sites(&public_1), vec![10, 20]);
    let replay = sup_replace(store, &p, 1, &[b.clone(), a.clone(), a.clone()])
        .expect("identical replay (any order, duplicates) is accepted");
    assert!(
        replay.replayed,
        "same generation + same set is a replay: {replay:?}"
    );
    assert_eq!(
        (replay.asserted, replay.retained, replay.retracted),
        (0, 2, 0)
    );
    assert_eq!(
        sup_public(store, "s_c", "s_a", &flows),
        Some(public_1.clone())
    );
    assert_eq!(sup_rows(store, "s_c", "s_a", &flows), rows_1);
    assert_eq!(store.support_generation(&p).expect("gen"), Some(1));

    // ── 2. {a,b} → {b,c}: only a retracted, b kept exactly once ────────────────
    let r2 = sup_replace(store, &p, 2, &[c.clone(), b.clone()]).expect("gen 2");
    assert_eq!(
        (r2.asserted, r2.retained, r2.retracted),
        (1, 1, 1),
        "{r2:?}"
    );
    let rows_2 = sup_rows(store, "s_c", "s_a", &flows);
    let facts_2: Vec<Edge> = rows_2.iter().map(|r| r.fact.clone()).collect();
    assert_eq!(
        facts_2.len(),
        2,
        "b must be stored exactly once: {rows_2:?}"
    );
    assert!(facts_2.contains(&b) && facts_2.contains(&c) && !facts_2.contains(&a));
    assert!(rows_2.iter().all(|r| r.owner == p && r.generation == 2));
    let public_2 = sup_public(store, "s_c", "s_a", &flows).expect("edge");
    assert_eq!(
        sup_sites(&public_2),
        vec![20, 30],
        "the retracted site leaves the sample"
    );

    // ── 5a. Failed replacements roll back (checked before the empty retraction) ─
    let before = (
        sup_public(store, "s_c", "s_a", &flows),
        sup_rows(store, "s_c", "s_a", &flows),
        store.support_generation(&p).expect("gen"),
    );
    let mut invalid = fact(40);
    invalid.resolved_by = String::new();
    // A valid new fact on ANOTHER key precedes the invalid one: a store that wrote fact by fact
    // would leave that key half-applied.
    let other_key = sup_flow(
        "s_x",
        "s_y",
        "assignment",
        "sup/web.ts",
        1,
        ResolutionTier::Scip,
        "scip-typescript",
    );
    for in_batch in [false, true] {
        if in_batch {
            store.begin_batch().expect("begin");
        }
        // The invalid fact reaches the STORE: it was valid when built and edited afterwards
        // (the fields are public), so the store's own re-validation must reject it.
        let mut edited =
            crate::support::SupportFact::from_edge(fact(40)).expect("valid when built");
        edited.edge = invalid.clone();
        let batch = [
            crate::support::SupportFact::from_edge(other_key.clone()).expect("valid"),
            crate::support::SupportFact::from_edge(fact(35)).expect("valid"),
            edited,
        ];
        let err = store
            .replace_edge_supports(&p, 3, &batch)
            .expect_err("a fact without resolved_by is rejected");
        assert!(err.to_string().contains("resolved_by"), "{err}");
        let err = sup_replace(store, &p, 2, std::slice::from_ref(&a))
            .expect_err("same generation, different set");
        assert!(err.to_string().contains("generation conflict"), "{err}");
        let err = sup_replace(store, &p, 1, &[a.clone(), b.clone()]).expect_err("older generation");
        assert!(err.to_string().contains("stale generation"), "{err}");
        if in_batch {
            store
                .commit_batch()
                .expect("commit after failed replacements");
        }
        let after = (
            sup_public(store, "s_c", "s_a", &flows),
            sup_rows(store, "s_c", "s_a", &flows),
            store.support_generation(&p).expect("gen"),
        );
        assert_eq!(
            after, before,
            "a failed replacement must change nothing (in_batch={in_batch})"
        );
        assert!(
            sup_public(store, "s_x", "s_y", &flows).is_none()
                && sup_rows(store, "s_x", "s_y", &flows).is_empty(),
            "no fact of a failed replacement may land (in_batch={in_batch})"
        );
    }

    // ── 3. Empty replacement retracts the owner's stale support ────────────────
    let r3 = sup_replace(store, &p, 3, &[]).expect("gen 3 empty");
    assert_eq!(
        (r3.asserted, r3.retained, r3.retracted, r3.edges_touched),
        (0, 0, 2, 1),
        "{r3:?}"
    );
    assert!(sup_rows(store, "s_c", "s_a", &flows).is_empty());
    assert_eq!(
        sup_public(store, "s_c", "s_a", &flows),
        None,
        "with no base and no support the public edge is gone"
    );
    assert_eq!(
        store.support_generation(&p).expect("gen"),
        Some(3),
        "the generation survives emptiness"
    );
    sup_replace(store, &p, 2, &[c.clone(), b.clone()])
        .expect_err("a stale replay cannot resurrect retracted support");
    assert!(sup_rows(store, "s_c", "s_a", &flows).is_empty());
    // A higher generation with the same (empty) set is not a replay but writes no fact.
    let r3b = sup_replace(store, &p, 4, &[]).expect("gen 4 empty");
    assert_eq!((r3b.replayed, r3b.edges_touched), (false, 0));
    assert_eq!(store.support_generation(&p).expect("gen"), Some(4));

    // ── 4. Two producers on one edge stay independent ─────────────────────────
    let qa = sup_flow(
        "s_c",
        "s_a",
        "angular_input",
        "sup/web.html",
        5,
        ResolutionTier::Heuristic,
        "angular-compiler",
    );
    sup_replace(store, &p, 5, &[a.clone(), b.clone()]).expect("p gen 5");
    sup_replace(store, &q, 1, std::slice::from_ref(&qa)).expect("q gen 1");
    let rows = sup_rows(store, "s_c", "s_a", &flows);
    assert_eq!(rows.len(), 3);
    let order: Vec<(String, String, String)> = rows
        .iter()
        .map(|r| {
            (
                r.owner.producer.clone(),
                r.owner.snapshot.clone(),
                r.fact_id.clone(),
            )
        })
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(
        order, sorted,
        "edge_supports is ordered by (producer, snapshot, fact_key)"
    );
    let both = sup_public(store, "s_c", "s_a", &flows).expect("edge");
    assert_eq!(sup_sites(&both).len(), 3);
    assert!((both.metadata[FLOW_CONFIDENCE_MIN_KEY].as_f64().unwrap() - 0.5).abs() < 1e-6);
    sup_replace(store, &p, 6, &[]).expect("p retracts");
    let only_q = sup_public(store, "s_c", "s_a", &flows).expect("q still supports the edge");
    assert_eq!(
        sup_rows(store, "s_c", "s_a", &flows)
            .iter()
            .map(|r| r.owner.clone())
            .collect::<Vec<_>>(),
        vec![q.clone()]
    );
    assert_eq!(
        only_q,
        merge_flow_edges(vec![qa.clone()]).remove(0),
        "Q alone projects to Q's fact"
    );
    assert_eq!(
        store.support_generation(&q).expect("gen"),
        Some(1),
        "P's write never touches Q's generation"
    );

    // ── 6. Input and owner order do not matter ────────────────────────────────
    sup_replace(store, &p, 7, &[c.clone(), a.clone(), b.clone()]).expect("p gen 7");
    let pq = sup_public(store, "s_c", "s_a", &flows).expect("edge");
    let pq_rows = sup_rows(store, "s_c", "s_a", &flows);
    sup_replace(store, &p, 8, &[]).expect("p out");
    sup_replace(store, &q, 2, &[]).expect("q out");
    sup_replace(store, &q, 3, std::slice::from_ref(&qa)).expect("q first this time");
    sup_replace(store, &p, 9, &[b.clone(), c.clone(), a.clone()]).expect("p, permuted");
    assert_eq!(sup_public(store, "s_c", "s_a", &flows), Some(pq));
    let strip =
        |rows: Vec<crate::support::EdgeSupport>| -> Vec<(crate::support::SupportOwner, String)> {
            rows.into_iter().map(|r| (r.owner, r.fact_id)).collect()
        };
    assert_eq!(strip(sup_rows(store, "s_c", "s_a", &flows)), strip(pq_rows));
    sup_replace(store, &p, 10, &[]).expect("p clean");
    sup_replace(store, &q, 4, &[]).expect("q clean");

    // ── 7. Staged vs one-shot ─────────────────────────────────────────────────
    // (a) History independence: a staged sequence ends where a one-shot replacement starts.
    let f2 = |byte: u32| {
        sup_flow(
            "s_c2",
            "s_a2",
            "assignment",
            "sup/web.ts",
            byte,
            ResolutionTier::Scip,
            "scip-typescript",
        )
    };
    let f3 = |byte: u32| {
        sup_flow(
            "s_c3",
            "s_a3",
            "assignment",
            "sup/web.ts",
            byte,
            ResolutionTier::Scip,
            "scip-typescript",
        )
    };
    let staged = sup_owner("scip-typescript", "staged");
    let oneshot = sup_owner("scip-typescript", "oneshot");
    sup_replace(store, &staged, 1, &[f2(1), f2(2), f2(3)]).expect("staged 1");
    sup_replace(store, &staged, 2, &[f2(2), f2(4)]).expect("staged 2");
    sup_replace(store, &oneshot, 1, &[f3(4), f3(2)]).expect("one-shot");
    assert_eq!(
        sup_public(store, "s_c2", "s_a2", &flows).map(sup_rebind),
        sup_public(store, "s_c3", "s_a3", &flows).map(sup_rebind),
        "a replacement's result must not depend on the owner's history"
    );
    // (b) A pre-merged fact projects like its raw facts, up to the cap. Support identity is per
    // SUBMITTED fact, so the authoritative rows deliberately differ (1 vs 3).
    let f4 = |byte: u32| {
        sup_flow(
            "s_c4",
            "s_a4",
            "assignment",
            "sup/web.ts",
            byte,
            ResolutionTier::Scip,
            "scip-typescript",
        )
    };
    let f5 = |byte: u32| {
        sup_flow(
            "s_c5",
            "s_a5",
            "assignment",
            "sup/web.ts",
            byte,
            ResolutionTier::Scip,
            "scip-typescript",
        )
    };
    let premerged = merge_flow_edges(vec![f4(1), f4(2), f4(3)]);
    sup_replace(store, &sup_owner("pre", "merged"), 1, &premerged).expect("pre-merged");
    sup_replace(store, &sup_owner("raw", "facts"), 1, &[f5(1), f5(2), f5(3)]).expect("raw");
    assert_eq!(
        sup_public(store, "s_c4", "s_a4", &flows).map(sup_rebind),
        sup_public(store, "s_c5", "s_a5", &flows).map(sup_rebind),
        "under the cap, folding in stages equals one fold"
    );
    assert_eq!(sup_rows(store, "s_c4", "s_a4", &flows).len(), 1);
    assert_eq!(sup_rows(store, "s_c5", "s_a5", &flows).len(), 3);
    // (c) The boundary: past the cap, a pre-merged fact plus a raw fact it had DROPPED counts
    // that fact twice — `flow_support_truncated` is "at least", exact only for a single fold.
    let n = MAX_FLOW_SUPPORT as u32 + 2; // 10 facts, cap 8 → the sample drops 2
    let raw: Vec<Edge> = (0..n).map(|i| f4(100 + i)).collect();
    let capped = merge_flow_edges(raw.clone());
    assert_eq!(sup_truncated(&capped[0]), 2);
    let dropped_site = raw
        .last()
        .cloned()
        .expect("the last-ordered fact is dropped");
    assert!(!sup_sites(&capped[0]).contains(&(100 + n as u64 - 1)));
    sup_replace(
        store,
        &sup_owner("pre", "merged"),
        2,
        &[capped[0].clone(), dropped_site],
    )
    .expect("overlapping pre-merged input");
    let over = sup_public(store, "s_c4", "s_a4", &flows).expect("edge");
    let shown = sup_sites(&over).len() as u64;
    assert_eq!(
        shown + sup_truncated(&over),
        n as u64 + 1,
        "documented boundary: overlapping pre-folded input over-counts by the overlap"
    );
    let raw: Vec<Edge> = (0..n).map(|i| f5(100 + i)).collect();
    sup_replace(store, &sup_owner("raw", "facts"), 2, &raw).expect("raw 10");
    let exact_raw = sup_public(store, "s_c5", "s_a5", &flows).expect("edge");
    assert_eq!(
        sup_sites(&exact_raw).len() as u64 + sup_truncated(&exact_raw),
        n as u64,
        "a single fold over raw facts counts exactly"
    );
    assert_eq!(sup_rows(store, "s_c5", "s_a5", &flows).len(), n as usize);

    // ── 8. Repeated folds are stable (counts, extrema, representative, cap, truncation) ─
    let again = sup_owner("raw", "facts");
    sup_replace(store, &again, 3, &raw).expect("same set, new generation");
    assert_eq!(
        sup_public(store, "s_c5", "s_a5", &flows),
        Some(exact_raw.clone())
    );
    let visitor = sup_flow(
        "s_c5",
        "s_a5",
        "a_visitor",
        "sup/other.ts",
        7,
        ResolutionTier::Tags,
        "visitor",
    );
    for round in 0..3 {
        sup_replace(
            store,
            &sup_owner("visitor", "v"),
            2 * round + 1,
            std::slice::from_ref(&visitor),
        )
        .expect("visitor joins");
        let with_visitor = sup_public(store, "s_c5", "s_a5", &flows).expect("edge");
        assert_eq!(
            sup_truncated(&with_visitor),
            3,
            "11 facts, cap 8 (round {round})"
        );
        sup_replace(store, &sup_owner("visitor", "v"), 2 * round + 2, &[]).expect("visitor leaves");
        assert_eq!(
            sup_public(store, "s_c5", "s_a5", &flows),
            Some(exact_raw.clone()),
            "a joined-then-retracted owner leaves no residue (round {round})"
        );
    }

    // ── 9. Eviction from the sample cannot change identity or a later representative ─
    let f6 = |construct: &str, byte: u32, tier: ResolutionTier| {
        sup_flow(
            "s_c6",
            "s_a6",
            construct,
            "sup/web.ts",
            byte,
            tier,
            "scip-typescript",
        )
    };
    let mut set: Vec<Edge> = (0..MAX_FLOW_SUPPORT as u32)
        .map(|i| f6("a_weak", 10 + i, ResolutionTier::Tags))
        .collect();
    let mid = f6("m_mid", 50, ResolutionTier::ImportMap);
    let strong = f6("z_strong", 60, ResolutionTier::Scip);
    set.push(mid.clone());
    set.push(strong.clone());
    let ev = sup_owner("scip-typescript", "evict");
    sup_replace(store, &ev, 1, &set).expect("10 facts");
    let sampled = sup_public(store, "s_c6", "s_a6", &flows).expect("edge");
    assert!(
        !sup_sites(&sampled).contains(&50),
        "m_mid is evicted from the sample: {:?}",
        sup_sites(&sampled)
    );
    assert_eq!(
        sampled.location.as_ref().map(|l| l.span.start_byte),
        Some(60)
    );
    assert_eq!(
        sup_rows(store, "s_c6", "s_a6", &flows).len(),
        10,
        "eviction never touches identity"
    );
    set.retain(|e| e != &strong);
    sup_replace(store, &ev, 2, &set).expect("the representative is retracted");
    let next = sup_public(store, "s_c6", "s_a6", &flows).expect("edge");
    assert_eq!(
        next.location.as_ref().map(|l| l.span.start_byte),
        Some(50),
        "the next representative is the evicted-from-sample m_mid, found from authoritative rows"
    );
    assert!((next.confidence.get() - mid.confidence.get()).abs() < f32::EPSILON);
    assert_eq!(
        next,
        merge_flow_edges(set.clone()).remove(0),
        "projection == one fold of the authoritative set"
    );

    // ── 10. The base plane coexists ───────────────────────────────────────────
    // Base flow edge from the indexer (syntax, file sup/base.ts) + producer support (sup/web.ts).
    let base = merge_flow_edges(vec![sup_flow(
        "s_c7",
        "s_a7",
        "expression",
        "sup/base.ts",
        3,
        ResolutionTier::Parsed,
        "tree-sitter",
    )])
    .remove(0);
    store
        .upsert_edges(std::slice::from_ref(&base))
        .expect("base write");
    let plain = sup_public(store, "s_c7", "s_a7", &flows).expect("base edge");
    assert_eq!(plain, base);
    let sp = sup_owner("scip-typescript", "base-coexist");
    let s1 = sup_flow(
        "s_c7",
        "s_a7",
        "assignment",
        "sup/web.ts",
        9,
        ResolutionTier::Scip,
        "scip-typescript",
    );
    sup_replace(store, &sp, 1, std::slice::from_ref(&s1)).expect("support over a base edge");
    assert_eq!(
        sup_public(store, "s_c7", "s_a7", &flows),
        Some(merge_flow_edges(vec![base.clone(), s1.clone()]).remove(0))
    );
    assert_eq!(
        sup_rows(store, "s_c7", "s_a7", &flows).len(),
        1,
        "the base plane is not support"
    );
    // A base re-write lands in the base contribution, not over the projection.
    let base2 = merge_flow_edges(vec![sup_flow(
        "s_c7",
        "s_a7",
        "expression",
        "sup/base.ts",
        4,
        ResolutionTier::Parsed,
        "tree-sitter",
    )])
    .remove(0);
    store
        .upsert_edges(std::slice::from_ref(&base2))
        .expect("base update");
    assert_eq!(
        sup_public(store, "s_c7", "s_a7", &flows),
        Some(merge_flow_edges(vec![base2.clone(), s1.clone()]).remove(0))
    );
    assert_eq!(
        sup_rows(store, "s_c7", "s_a7", &flows).len(),
        1,
        "a base write never erases support"
    );
    // remove_file of the SUPPORT's file: support is producer-owned, so the edge stays.
    store
        .remove_file("sup/web.ts")
        .expect("remove support file");
    assert_eq!(
        sup_public(store, "s_c7", "s_a7", &flows),
        Some(merge_flow_edges(vec![base2.clone(), s1.clone()]).remove(0))
    );
    // remove_file of the BASE's file retires the base contribution only.
    store.remove_file("sup/base.ts").expect("remove base file");
    assert_eq!(
        sup_public(store, "s_c7", "s_a7", &flows),
        Some(merge_flow_edges(vec![s1.clone()]).remove(0))
    );
    assert_eq!(sup_rows(store, "s_c7", "s_a7", &flows).len(), 1);
    // Retracting the last support with the base gone leaves nothing.
    sup_replace(store, &sp, 2, &[]).expect("retract");
    assert_eq!(sup_public(store, "s_c7", "s_a7", &flows), None);
    // Retracting the last support with the base present restores it byte-for-byte; same for a
    // non-flow kind, whose projection is the max-confidence representative.
    let calls_base = Edge::new(
        sym("s_c8"),
        sym("s_a8"),
        EdgeKind::Calls,
        ResolutionTier::Heuristic,
        "name-resolver",
    )
    .with_location(Location::new("sup/base8.ts", Span::ZERO));
    let calls_scip = Edge::new(
        sym("s_c8"),
        sym("s_a8"),
        EdgeKind::Calls,
        ResolutionTier::Scip,
        "scip-typescript",
    )
    .with_location(Location::new("sup/web8.ts", Span::ZERO));
    store
        .upsert_edges(std::slice::from_ref(&calls_base))
        .expect("calls base");
    sup_replace(store, &sp, 3, std::slice::from_ref(&calls_scip)).expect("calls support");
    assert_eq!(
        sup_public(store, "s_c8", "s_a8", &EdgeKind::Calls),
        Some(calls_scip.clone())
    );
    // A weaker base write is ignored by the base rule, exactly as without support.
    let weaker = Edge::new(
        sym("s_c8"),
        sym("s_a8"),
        EdgeKind::Calls,
        ResolutionTier::Tags,
        "tags",
    )
    .with_location(Location::new("sup/base8.ts", Span::ZERO));
    store.upsert_edges(&[weaker]).expect("weaker base write");
    sup_replace(store, &sp, 4, &[]).expect("retract calls support");
    assert_eq!(
        sup_public(store, "s_c8", "s_a8", &EdgeKind::Calls),
        Some(calls_base.clone()),
        "base restored exactly"
    );
    assert!(sup_rows(store, "s_c8", "s_a8", &EdgeKind::Calls).is_empty());

    // prune_dangling_edges never prunes support: a dangling supported edge stays (its owner
    // retracts it) and is not counted; a dangling base-only edge is pruned as ever.
    let ghost_support = sup_flow(
        "s_x",
        "ghost_target",
        "assignment",
        "sup/web.ts",
        1,
        ResolutionTier::Scip,
        "scip-typescript",
    );
    let ghost_base = Edge::new(
        sym("s_y"),
        sym("ghost_base"),
        EdgeKind::Calls,
        ResolutionTier::Parsed,
        "tree-sitter",
    );
    store
        .upsert_edges(&[ghost_base])
        .expect("dangling base edge");
    sup_replace(store, &sp, 5, std::slice::from_ref(&ghost_support)).expect("dangling support");
    let pruned = store.prune_dangling_edges().expect("prune");
    assert_eq!(pruned, 1, "only the base-only dangling edge is pruned");
    assert_eq!(
        sup_public(store, "s_x", "ghost_target", &flows),
        Some(merge_flow_edges(vec![ghost_support]).remove(0))
    );
    assert!(sup_public(store, "s_y", "ghost_base", &EdgeKind::Calls).is_none());
    sup_replace(store, &sp, 6, &[]).expect("owner retracts the dangling fact");
    assert!(sup_public(store, "s_x", "ghost_target", &flows).is_none());
    assert_eq!(store.prune_dangling_edges().expect("prune"), 0);

    // ── 11. Representative ties are decided by the fact SET, never by row order ─────────
    // Two facts identical in TS-S1's support order (same site, construct, class, resolver,
    // confidence) but different facts (evidence_count, provenance, extra metadata). Every backend
    // must project exactly `project_edge` of the set, whichever owner wrote first.
    let tie = |source: &str, target: &str, evidence: u32, tag: &str| {
        let mut e = sup_flow(
            source,
            target,
            "assignment",
            "sup/web.ts",
            70,
            ResolutionTier::Scip,
            "scip-typescript",
        );
        e.evidence_count = evidence;
        e.metadata.insert("tag".into(), serde_json::json!(tag));
        e
    };
    let (a1, a2) = (sup_owner("tie", "one-a"), sup_owner("tie", "two-a"));
    let (b1, b2) = (sup_owner("tie", "one-b"), sup_owner("tie", "two-b"));
    sup_replace(store, &a1, 1, &[tie("s_c", "s_a2", 3, "one")]).expect("one first");
    sup_replace(store, &a2, 1, &[tie("s_c", "s_a2", 1, "two")]).expect("two second");
    sup_replace(store, &b2, 1, &[tie("s_c2", "s_a", 1, "two")]).expect("two first");
    sup_replace(store, &b1, 1, &[tie("s_c2", "s_a", 3, "one")]).expect("one second");
    let first = sup_public(store, "s_c", "s_a2", &flows).expect("edge");
    let second = sup_public(store, "s_c2", "s_a", &flows).expect("edge");
    let expect = crate::support::project_edge(
        None,
        &[&tie("s_c", "s_a2", 1, "two"), &tie("s_c", "s_a2", 3, "one")],
    );
    assert_eq!(
        Some(first.clone()),
        expect,
        "projection == project_edge(set)"
    );
    assert_eq!(
        sup_rebind(first),
        sup_rebind(second),
        "write order must not pick the representative"
    );
    for o in [&a1, &a2, &b1, &b2] {
        sup_replace(store, o, 2, &[]).expect("tie clean");
    }

    // ── 12. remove_file's source-in-file predicate retires a base contribution ──────────
    // The base edge carries no location, so only "its source node lives in the removed file"
    // can retire it. Support keeps the edge alive (dangling source) until its owner retracts.
    store
        .upsert_nodes(&[Node::new(
            sym("s_src_in_file"),
            NodeKind::Function,
            "s_src_in_file",
            Language::new("typescript"),
            Location::new("sup/src_node.ts", Span::ZERO),
        )])
        .expect("source node");
    let loc_less = Edge::new(
        sym("s_src_in_file"),
        sym("s_a"),
        EdgeKind::Calls,
        ResolutionTier::Parsed,
        "tree-sitter",
    );
    let backed = Edge::new(
        sym("s_src_in_file"),
        sym("s_a"),
        EdgeKind::Calls,
        ResolutionTier::Heuristic,
        "scip-typescript",
    )
    .with_location(Location::new("sup/web.ts", Span::ZERO));
    store
        .upsert_edges(std::slice::from_ref(&loc_less))
        .expect("base");
    let sf = sup_owner("scip-typescript", "src-in-file");
    sup_replace(store, &sf, 1, std::slice::from_ref(&backed)).expect("support");
    assert_eq!(
        sup_public(store, "s_src_in_file", "s_a", &EdgeKind::Calls),
        Some(loc_less.clone()),
        "base wins at 1.0"
    );
    store
        .remove_file("sup/src_node.ts")
        .expect("remove the source's file");
    assert_eq!(
        sup_public(store, "s_src_in_file", "s_a", &EdgeKind::Calls),
        Some(backed.clone()),
        "base retired by the source predicate; support survives"
    );
    sup_replace(store, &sf, 2, &[]).expect("retract");
    assert_eq!(
        sup_public(store, "s_src_in_file", "s_a", &EdgeKind::Calls),
        None,
        "nothing resurrects the base"
    );

    // ── 13. prune retires a dangling supported key's base contribution ─────────────────
    let dangling_base = Edge::new(
        sym("s_y"),
        sym("ghost_both"),
        EdgeKind::Calls,
        ResolutionTier::Parsed,
        "tree-sitter",
    );
    let dangling_support = Edge::new(
        sym("s_y"),
        sym("ghost_both"),
        EdgeKind::Calls,
        ResolutionTier::Heuristic,
        "scip-typescript",
    );
    store
        .upsert_edges(std::slice::from_ref(&dangling_base))
        .expect("dangling base");
    let pr = sup_owner("scip-typescript", "prune-base");
    sup_replace(store, &pr, 1, std::slice::from_ref(&dangling_support)).expect("support");
    assert_eq!(
        store.prune_dangling_edges().expect("prune"),
        0,
        "a supported edge is not counted"
    );
    assert_eq!(
        sup_public(store, "s_y", "ghost_both", &EdgeKind::Calls),
        Some(dangling_support.clone()),
        "prune retired the base contribution, kept the support"
    );
    sup_replace(store, &pr, 2, &[]).expect("retract");
    assert_eq!(
        sup_public(store, "s_y", "ghost_both", &EdgeKind::Calls),
        None
    );

    // ── 14. Support never keeps a shared Import node alive ────────────────────────────
    // Only a BASE-plane importer in another file keeps an Import node on remove_file. A support
    // fact's site is not an importer: counting it would pin the node to a file that nothing will
    // ever remove again.
    store
        .upsert_nodes(&[Node::new(
            sym("s_import"),
            NodeKind::Import,
            "s_import",
            Language::new("typescript"),
            Location::new("sup/imp_b.ts", Span::ZERO),
        )])
        .expect("import node");
    let imp = Edge::new(
        sym("s_x"),
        sym("s_import"),
        EdgeKind::Imports,
        ResolutionTier::Scip,
        "scip-typescript",
    )
    .with_location(Location::new("sup/imp_a.ts", Span::ZERO));
    let io = sup_owner("scip-typescript", "import");
    sup_replace(store, &io, 1, std::slice::from_ref(&imp)).expect("support");
    store
        .remove_file("sup/imp_b.ts")
        .expect("remove the import's home");
    assert!(
        store.get_node(&sym("s_import")).expect("get").is_none(),
        "support at sup/imp_a.ts must not keep the Import node"
    );
    sup_replace(store, &io, 2, &[]).expect("retract");

    // ── 15. Fact identity is producer-owned and opaque ─────────────────────────────────
    use crate::support::SupportFact;
    let lsp = sup_owner("language-server", "polyglot");
    // Byte-identical edge content under two producer ids: two facts, not one.
    let same = sup_flow(
        "s_c",
        "s_a3",
        "assignment",
        "sup/web.ts",
        5,
        ResolutionTier::Scip,
        "lsp",
    );
    // Ids that differ only by whitespace, case or Unicode normalization form are distinct; the
    // store neither trims, case-folds nor normalizes, and returns each id exactly.
    let ids = [
        "java:User#save()",
        "ts:User.save",
        "ts:User.save ",
        "TS:User.save",
        "caf\u{e9}",
        "cafe\u{301}",
    ];
    let opaque: Vec<SupportFact> = ids
        .iter()
        .map(|id| SupportFact::new(*id, same.clone()).expect("fact"))
        .collect();
    let r = store
        .replace_edge_supports(&lsp, 1, &opaque)
        .expect("opaque ids");
    assert_eq!(r.asserted, ids.len(), "{r:?}");
    let rows = sup_rows(store, "s_c", "s_a3", &flows);
    let mut want: Vec<&str> = ids.to_vec();
    want.sort();
    assert_eq!(
        rows.iter().map(|r| r.fact_id.as_str()).collect::<Vec<_>>(),
        want,
        "every id stored exactly, ordered by byte order"
    );
    assert!(rows.iter().all(|r| r.fact == same));
    let mut shuffled = opaque.clone();
    shuffled.reverse();
    assert!(
        store
            .replace_edge_supports(&lsp, 1, &shuffled)
            .expect("replay")
            .replayed,
        "same ids + same content in any order is a replay"
    );
    // The same id re-asserted with different content is a change: retracted and asserted.
    let moved = sup_flow(
        "s_c",
        "s_a3",
        "assignment",
        "sup/web.ts",
        6,
        ResolutionTier::Scip,
        "lsp",
    );
    let mut changed = opaque.clone();
    changed[0] = SupportFact::new(ids[0], moved.clone()).expect("fact");
    let r = store
        .replace_edge_supports(&lsp, 2, &changed)
        .expect("changed content");
    assert_eq!(
        (r.asserted, r.retained, r.retracted),
        (1, ids.len() - 1, 1),
        "{r:?}"
    );
    let rows = sup_rows(store, "s_c", "s_a3", &flows);
    assert_eq!(rows.len(), ids.len());
    assert_eq!(
        rows.iter()
            .find(|r| r.fact_id == ids[0])
            .map(|r| r.fact.clone()),
        Some(moved)
    );
    // One id, two contents, one submission: rejected, nothing written.
    let clash = [
        SupportFact::new("dup", same.clone()).unwrap(),
        changed[0].clone().with_id("dup").unwrap(),
    ];
    store
        .replace_edge_supports(&lsp, 3, &clash)
        .expect_err("one id asserted twice with different content");
    assert_eq!(store.support_generation(&lsp).expect("gen"), Some(2));
    store
        .replace_edge_supports(&lsp, 3, &[])
        .expect("lsp clean");

    // ── 16. Identical display names across languages/toolchains never collide ──────────
    // Two toolchains each index a symbol displayed as `User`, with their own symbol schemes, and
    // both use the producer-local fact id "User". Storage keys on the full ids and never parses a
    // symbol scheme, so neither fact touches the other.
    let display = |id: &str, file: &str, lang: &str| {
        Node::new(
            crate::symbol::SymbolId(id.into()),
            NodeKind::Class,
            "User",
            Language::new(lang),
            Location::new(file, Span::ZERO),
        )
    };
    store
        .upsert_nodes(&[
            display(
                "scip-java maven acme 1.0 com/acme/User#",
                "sup/User.java",
                "java",
            ),
            display(
                "scip-typescript npm acme 1.0 src/`user.ts`/User#",
                "sup/user.ts",
                "typescript",
            ),
        ])
        .expect("same display name, two languages");
    let link = |src: &str, by: &str| {
        Edge::new(
            crate::symbol::SymbolId(src.into()),
            sym("s_a"),
            EdgeKind::Imports,
            ResolutionTier::Scip,
            by,
        )
    };
    let java = sup_owner("scip-java", "acme");
    let ts = sup_owner("scip-typescript", "acme");
    let java_fact = link("scip-java maven acme 1.0 com/acme/User#", "scip-java");
    let ts_fact = link(
        "scip-typescript npm acme 1.0 src/`user.ts`/User#",
        "scip-typescript",
    );
    store
        .replace_edge_supports(
            &java,
            1,
            &[SupportFact::new("User", java_fact.clone()).unwrap()],
        )
        .expect("java");
    store
        .replace_edge_supports(
            &ts,
            1,
            &[SupportFact::new("User", ts_fact.clone()).unwrap()],
        )
        .expect("ts");
    let read = |s: &S, e: &Edge| {
        s.edge_supports(&e.source, &e.target, &e.kind)
            .expect("rows")
    };
    assert_eq!(read(store, &java_fact).len(), 1);
    assert_eq!(read(store, &ts_fact).len(), 1);
    store
        .replace_edge_supports(&java, 2, &[])
        .expect("java retracts");
    assert!(read(store, &java_fact).is_empty());
    assert_eq!(
        read(store, &ts_fact).len(),
        1,
        "the other toolchain's `User` is untouched"
    );
    assert_eq!(read(store, &ts_fact)[0].fact, ts_fact);
    store.replace_edge_supports(&ts, 2, &[]).expect("ts clean");

    // ── 17. Owners are listed, ordered, including those whose set is now empty ─────────
    let owners = store.support_owners().expect("owners");
    let names: Vec<(String, String)> = owners
        .iter()
        .map(|o| (o.owner.producer.clone(), o.owner.snapshot.clone()))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted, "ordered by (producer, snapshot), each once");
    for o in &owners {
        assert_eq!(
            store.support_generation(&o.owner).expect("gen"),
            Some(o.generation)
        );
    }
    for (owner, generation) in [(&p, 10), (&java, 2), (&ts, 2), (&lsp, 3)] {
        assert!(
            owners
                .iter()
                .any(|o| &o.owner == owner && o.generation == generation),
            "{owner:?} at {generation} must be listed even with an empty set: {owners:?}"
        );
    }

    // The public envelope of a projected flow edge is exactly TS-S1's: every key a plain merge
    // carries, nothing more.
    let env = sup_public(store, "s_c6", "s_a6", &flows).expect("edge");
    let plain_merge = merge_flow_edges(set).remove(0);
    let keys = |e: &Edge| {
        e.metadata
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(keys(&env), keys(&plain_merge));
    assert!(env.metadata.contains_key(FLOW_SUPPORT_KEY));
}
