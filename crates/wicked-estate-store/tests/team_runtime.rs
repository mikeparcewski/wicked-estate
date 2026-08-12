//! Functional test for the `WICKED_RUNTIME=team` profile (Phase 8, foundation
//! team-profile leg): the SAME estate operations that define the local
//! `GraphStore` contract must pass against the shared Postgres a team profile
//! resolves to.
//!
//! Skips gracefully when `TEST_POSTGRES_URL` is not set (same convention as
//! `postgres_conformance.rs`; CI's postgres-conformance job provides it).
//!
//! Profile resolution is exercised through the PURE seam
//! (`resolve_store_spec_from`) rather than by mutating process env — env vars
//! are process-global and the thin `resolve_store_spec` wrapper only reads
//! three of them (unit-tested separately in the lib).

#![cfg(feature = "postgres")]

/// Cross-BINARY serialization: this file and `postgres_conformance.rs` share one database
/// (same table names, fresh-schema drops). Cargo currently runs integration-test binaries
/// sequentially, but that is an implementation detail, not a contract — a session-level
/// Postgres advisory lock makes the isolation explicit. Held for the whole test; released
/// on drop (and by the server if the session dies). Same key in both files.
struct PgTestLease {
    rt: tokio::runtime::Runtime,
    pool: sqlx::PgPool,
}

impl PgTestLease {
    const KEY: i64 = 0x5749_434B; // "WICK"

    fn acquire(url: &str) -> Self {
        use sqlx::postgres::PgPoolOptions;
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        // max_connections(1): the advisory lock is session-scoped, so it must live on the
        // one connection this pool will ever hand out.
        let pool = rt
            .block_on(PgPoolOptions::new().max_connections(1).connect(url))
            .expect("connect for test lease");
        rt.block_on(
            sqlx::query("SELECT pg_advisory_lock($1)")
                .bind(Self::KEY)
                .execute(&pool),
        )
        .expect("acquire pg advisory test lock");
        Self { rt, pool }
    }
}

impl Drop for PgTestLease {
    fn drop(&mut self) {
        let _ = self.rt.block_on(
            sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(Self::KEY)
                .execute(&self.pool),
        );
    }
}

/// Drop every table the store creates so the run starts with a fresh schema
/// (mirrors postgres_conformance.rs).
fn drop_all_tables(url: &str) {
    use sqlx::postgres::PgPoolOptions;
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    let pool = rt
        .block_on(PgPoolOptions::new().max_connections(1).connect(url))
        .expect("connect for cleanup");
    rt.block_on(async {
        sqlx::query(
            "DROP TABLE IF EXISTS \
             annotations, edge_history, changes, meta, cache, content, \
             unresolved_refs, edges, nodes, files, symbol_gen CASCADE",
        )
        .execute(&pool)
        .await
    })
    .expect("drop tables for fresh team-runtime run");
}

#[test]
fn team_profile_resolution_drives_postgres_conformance() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("team_runtime: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _lease = PgTestLease::acquire(&url);

    // 1. The seam: a team profile resolves to the shared Postgres URL — overriding an
    //    ambient WICKED_ESTATE_DB and the local default (the coherent one-switch flip).
    let spec = wicked_estate_store::resolve_store_spec_from(
        None,
        Some("team"),
        Some(&url),
        Some("/tmp/ambient-engineer-local.db"),
        ".wicked-estate/graph.db",
    )
    .expect("team profile resolves");
    assert_eq!(spec, url, "team profile must resolve to WICKED_STORE_URL");

    // 2. The factory seam (ADR-003): the resolved spec opens through the same
    //    open_store entrypoint every binary uses — no caller-side special-casing.
    drop_all_tables(&spec);
    let store = wicked_estate_store::open_store(&spec).expect("factory opens resolved spec");
    let caps = wicked_estate_core::GraphRead::capabilities(&*store);
    assert!(
        caps.shared_writers,
        "team store must support shared writers"
    );
    assert!(
        caps.transactional_batch,
        "team store must have transactional batches (torn-read fix, decision #8)"
    );
    drop(store);

    // 3. The contract: the full GraphStore conformance suite — the exact operations the
    //    local SQLite store is held to — against the team-resolved Postgres.
    drop_all_tables(&spec);
    let mut store = wicked_estate_store::PostgresStore::open(&spec).expect("open postgres store");
    store.set_history_enabled(true).expect("enable history");
    wicked_estate_core::conformance::graph_store_suite(&mut store);
}
