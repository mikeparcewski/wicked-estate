//! W1.5 bake-off: SurrealDB conformance.
//!
//! Compiled ONLY with `--features surrealdb`.  The default test suite never
//! compiles this file — the `surrealdb-backend` CI lane is what runs it.
//!
//! Run:
//!   cargo test -p wicked-estate-store --features surrealdb

#![cfg(feature = "surrealdb")]

use wicked_estate_core::conformance;
use wicked_estate_store::SurrealStore;

fn fresh() -> SurrealStore {
    SurrealStore::in_memory().expect("SurrealStore::in_memory")
}

/// The full conformance suite must pass for SurrealStore (incl. the #190 depth-horizon case,
/// which `graph_store_suite` runs inline).
#[test]
fn surrealstore_satisfies_graph_store_contract() {
    let mut store = fresh();
    // history must be ON for the edge_history archival assertion in the suite (SqliteStore parity).
    store.set_history_enabled(true);
    conformance::graph_store_suite(&mut store);
}

/// History archival is opt-in: a default store archives nothing on `remove_file` (MemStore parity).
#[test]
fn surrealstore_history_is_off_by_default() {
    use wicked_estate_core::{
        Edge, EdgeKind, GraphRead, GraphWrite, Language, Location, Node, NodeKind, ResolutionTier,
        Span, SymbolId,
    };
    let mut store = fresh();
    let node = |n: &str| {
        Node::new(
            SymbolId(n.into()),
            NodeKind::Function,
            n,
            Language::new("rust"),
            Location::new("a.rs", Span::ZERO),
        )
    };
    store.upsert_nodes(&[node("h1"), node("h2")]).unwrap();
    store
        .upsert_edges(&[Edge::new(
            SymbolId("h1".into()),
            SymbolId("h2".into()),
            EdgeKind::Calls,
            ResolutionTier::Scip,
            "test",
        )])
        .unwrap();
    store.remove_file("a.rs").unwrap();
    assert!(
        store.edge_history("a.rs").unwrap().is_empty(),
        "history is opt-in; a default store must not archive"
    );
}

/// `traverse_multi` (the trait default here) must equal the union of per-seed `traverse`,
/// truncation causes included.
#[test]
fn surrealstore_traverse_multi_matches_union() {
    conformance::traverse_multi_matches_union_of_traverse(&mut fresh());
}

/// Multi-file symbol contributions (M4 / Option A — wicked-estate#152): the same suite every
/// other shipped backend runs.
#[test]
fn surrealstore_multi_file_contributions() {
    conformance::multi_file_contribution_suite(&mut fresh());
}
