//! Proves [`MemStore`] satisfies the `GraphStore` contract — including the edge-direction
//! invariant and bounded reverse-reachability that blast-radius depends on.
//!
//! The conformance suite includes an edge_history archival test that requires history_enabled.
//! Both stores are opened with history explicitly ON via `new_with_history()` /
//! `set_history_enabled(true)` so the suite's assertions hold regardless of the default.

use wicked_estate_core::conformance;
use wicked_estate_core::{Descriptor, Language, Location, Node, NodeKind, Span, Symbol, SymbolId};
use wicked_estate_store::{GraphStoreMutExt, MemStore};

#[test]
fn memstore_satisfies_graph_store_contract() {
    // history must be ON for the edge_history archival assertion in the suite.
    let mut store = MemStore::new_with_history();
    conformance::graph_store_suite(&mut store);
}

#[test]
fn sqlite_store_satisfies_graph_store_contract() {
    let mut store = wicked_estate_store::SqliteStore::in_memory().expect("open in-memory sqlite");
    // history must be ON for the edge_history archival assertion in the suite.
    store
        .set_history_enabled(true)
        .expect("enable history for conformance");
    conformance::graph_store_suite(&mut store);
}

#[test]
fn sqlite_traverse_multi_matches_union() {
    let mut store = wicked_estate_store::SqliteStore::in_memory().expect("in-memory sqlite");
    conformance::traverse_multi_matches_union_of_traverse(&mut store);
}

#[test]
fn memstore_traverse_multi_matches_union() {
    let mut store = MemStore::new();
    conformance::traverse_multi_matches_union_of_traverse(&mut store);
}

/// Multi-file symbol contributions (M4 / Option A — wicked-estate#152): the h+cpp arc
/// (definition-preferred primary, remove-one-file survivor re-home, delete on last contribution),
/// idempotent re-index, deterministic tiebreak — the store-side retirement of the extract-level
/// `cpp_member_proto_def_cross_file_single_id_hazard` pin.
#[test]
fn sqlite_multi_file_contributions() {
    let mut store = wicked_estate_store::SqliteStore::in_memory().expect("in-memory sqlite");
    conformance::multi_file_contribution_suite(&mut store);
}

#[test]
fn memstore_multi_file_contributions() {
    let mut store = MemStore::new();
    conformance::multi_file_contribution_suite(&mut store);
}

fn epoch_sym(name: &str) -> SymbolId {
    Symbol::global("test", None, vec![Descriptor::method(name, None)]).id()
}

fn epoch_node(name: &str, file: &str) -> Node {
    Node::new(
        epoch_sym(name),
        NodeKind::Function,
        name,
        Language::new("rust"),
        Location::new(file, Span::ZERO),
    )
}

/// BUILD-GATE (M8 / DoD-XA4) — NON-VACUOUS epoch on the WATCH/REINDEX **skip-FTS** hot path.
///
/// This is the precise falsifier the antagonist hunts: if the gen bump lived only on the FTS
/// `upsert_nodes` path it would be INERT here, because `index_path`/the watcher write through
/// `upsert_nodes_skip_fts` (`wicked-estate/src/lib.rs:585`) → `upsert_nodes_no_fts` →
/// `upsert_nodes_inner(.., with_fts=false)`. The shared seam is what makes the bump fire on BOTH
/// paths. We delete a symbol, re-add the SAME name through the skip-FTS path, and require epoch ≥ 1.
///
/// `upsert_nodes_skip_fts` is on `GraphStoreMutExt`, which both SqliteStore and MemStore implement,
/// so this asserts the gate for both default-feature backends via the actual reindex entry point.
fn skip_fts_reuse_bumps_epoch<S: GraphStoreMutExt>(store: &mut S) {
    let file = "src/reindex_hot_path.rs";
    store.begin_batch().expect("begin");

    // First-ever node via the skip-FTS path → epoch 0.
    store
        .upsert_nodes_skip_fts(&[epoch_node("reindexed_sym", file)])
        .expect("first skip-fts upsert");
    assert_eq!(
        store
            .symbol_epoch(&epoch_sym("reindexed_sym"))
            .expect("epoch"),
        Some(0),
        "first-ever node via skip-FTS path must be epoch 0"
    );

    // Reindex cycle: remove the file, then re-add the SAME symbol via the skip-FTS path.
    store.remove_file(file).expect("remove_file (reindex)");
    assert_eq!(
        store
            .symbol_epoch(&epoch_sym("reindexed_sym"))
            .expect("epoch while removed"),
        None,
        "removed symbol has no live epoch"
    );
    store
        .upsert_nodes_skip_fts(&[epoch_node("reindexed_sym", file)])
        .expect("re-add via skip-fts");
    store.commit_batch().expect("commit");

    let epoch = store
        .symbol_epoch(&epoch_sym("reindexed_sym"))
        .expect("epoch after skip-fts reuse")
        .expect("re-added symbol is live");
    assert!(
        epoch >= 1,
        "NON-VACUOUS (skip-FTS): epoch after delete-then-re-add via the REINDEX hot path must be \
         >= 1 — proving the bump is NOT inert on the skip-FTS path; got {epoch}"
    );
}

#[test]
fn sqlite_skip_fts_reuse_bumps_epoch() {
    let mut store = wicked_estate_store::SqliteStore::in_memory().expect("in-memory sqlite");
    skip_fts_reuse_bumps_epoch(&mut store);
}

#[test]
fn memstore_skip_fts_reuse_bumps_epoch() {
    let mut store = MemStore::new();
    skip_fts_reuse_bumps_epoch(&mut store);
}

#[test]
fn factory_opens_sqlite_and_rejects_unbuilt_backends() {
    // The external-DB seam: SQLite works; designed-not-built backends fail loudly.
    let store =
        wicked_estate_store::open_store(":memory:").expect("open sqlite memory via factory");
    assert_eq!(store.stats().expect("stats").node_count, 0);
    // When the postgres feature is NOT enabled, the factory returns an error.
    // When it IS enabled, it attempts a real connection (which may fail for other reasons).
    #[cfg(not(feature = "postgres"))]
    assert!(
        wicked_estate_store::open_store("postgres://localhost/db").is_err(),
        "postgres is not built without the feature"
    );
    assert!(
        wicked_estate_store::open_store("surrealdb://localhost:8000/ns/db").is_err(),
        "surreal is not built yet"
    );
}
