//! Postgres conformance test — skips gracefully when `TEST_POSTGRES_URL` is not set.
//!
//! Run against a real Postgres instance:
//!   TEST_POSTGRES_URL=postgres://user:pass@localhost/wicked_test \
//!   cargo test -p wicked-estate-store --features postgres

#[cfg(feature = "postgres")]
#[test]
fn postgres_store_satisfies_graph_store_contract() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let mut store =
        wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    store.set_history_enabled(true).expect("enable history");
    wicked_estate_core::conformance::graph_store_suite(&mut store);
}
