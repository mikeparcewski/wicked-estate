//! W1.5 bake-off: SurrealDB conformance + micro-bench.
//!
//! Compiled ONLY with `--features surrealdb`.  The default test suite never
//! compiles this file.
//!
//! Run:
//!   cargo test -p wicked-estate-store --features surrealdb
//!   cargo test -p wicked-estate-store --features surrealdb surreal_ -- --nocapture

#![cfg(feature = "surrealdb")]

use wicked_estate_core::conformance::graph_store_suite;
use wicked_estate_store::SurrealStore;

/// The full conformance suite must pass for SurrealStore.
#[test]
fn surrealstore_satisfies_graph_store_contract() {
    let mut store = SurrealStore::in_memory().expect("SurrealStore::in_memory");
    graph_store_suite(&mut store);
}
