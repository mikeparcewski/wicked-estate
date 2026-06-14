//! Proves [`MemStore`] satisfies the `GraphStore` contract — including the edge-direction
//! invariant and bounded reverse-reachability that blast-radius depends on.
//!
//! The conformance suite includes an edge_history archival test that requires history_enabled.
//! Both stores are opened with history explicitly ON via `new_with_history()` /
//! `set_history_enabled(true)` so the suite's assertions hold regardless of the default.

use wicked_estate_core::conformance;
use wicked_estate_store::MemStore;

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
fn factory_opens_sqlite_and_rejects_unbuilt_backends() {
    // The external-DB seam: SQLite works; designed-not-built backends fail loudly.
    let store =
        wicked_estate_store::open_store(":memory:").expect("open sqlite memory via factory");
    assert_eq!(store.stats().expect("stats").node_count, 0);
    assert!(
        wicked_estate_store::open_store("postgres://localhost/db").is_err(),
        "postgres is not built yet"
    );
    assert!(
        wicked_estate_store::open_store("surrealdb://localhost:8000/ns/db").is_err(),
        "surreal is not built yet"
    );
}
