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
    let mut truncated = false;
    for s in starts {
        let sub = store.traverse(s, spec).expect("traverse");
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
        truncated |= sub.truncated;
    }
    for s in starts {
        depths.remove(&s.0);
    }
    crate::query::Subgraph {
        nodes,
        edges,
        depths,
        truncated,
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

    // b's dependencies = what b depends on = edges where source==b → {c}.
    let deps = store
        .neighbors(&sym("b"), Direction::Dependencies)
        .expect("dependencies");
    assert_eq!(deps.len(), 1, "b has exactly one dependency");
    assert_eq!(deps[0].target, sym("c"), "c is the dependency of b");

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
    store
        .upsert_unresolved_refs(&[ghost_ref])
        .expect("upsert_unresolved_refs");

    let found = store
        .unresolved_refs_for_name("ghost")
        .expect("unresolved_refs_for_name");
    assert_eq!(found.len(), 1, "one unresolved ref for 'ghost'");
    assert_eq!(found[0].raw_name, "ghost");
    assert_eq!(found[0].from, sym("a"));

    let stats_after = store.stats().expect("stats after unresolved upsert");
    assert_eq!(
        stats_after.unresolved_ref_count, 1,
        "stats must reflect the stored unresolved ref"
    );

    // A name with no refs returns an empty vec, not an error.
    let none = store
        .unresolved_refs_for_name("no_such_name")
        .expect("empty lookup ok");
    assert!(none.is_empty(), "no unresolved refs for unknown name");

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
    // After remove_file("src/lib.rs") none of them should remain.
    store.remove_file("src/lib.rs").expect("remove_file");
    let remaining = store.all_nodes().expect("all_nodes after remove_file");
    assert!(
        remaining.iter().all(|n| n.location.file != "src/lib.rs"),
        "remove_file must remove all nodes whose location.file matches; remaining: {:?}",
        remaining
            .iter()
            .map(|n| &n.location.file)
            .collect::<Vec<_>>()
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
}
