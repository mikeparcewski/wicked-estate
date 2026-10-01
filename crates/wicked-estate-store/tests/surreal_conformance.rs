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
    conformance::graph_store_suite(&mut fresh());
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
