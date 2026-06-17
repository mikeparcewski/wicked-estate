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

    // Drop all tables so the conformance suite always starts with a fresh schema.
    {
        use sqlx::postgres::PgPoolOptions;
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        let pool = rt
            .block_on(PgPoolOptions::new().max_connections(1).connect(&url))
            .expect("connect for cleanup");
        rt.block_on(async {
            sqlx::query(
                "DROP TABLE IF EXISTS \
                 annotations, edge_history, changes, meta, cache, content, \
                 unresolved_refs, edges, nodes, files CASCADE",
            )
            .execute(&pool)
            .await
        })
        .expect("drop tables for fresh conformance run");
    }

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    store.set_history_enabled(true).expect("enable history");
    wicked_estate_core::conformance::graph_store_suite(&mut store);
}
