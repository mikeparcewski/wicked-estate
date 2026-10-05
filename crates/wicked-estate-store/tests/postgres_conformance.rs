//! Postgres conformance tests — skip gracefully when `TEST_POSTGRES_URL` is not set.
//!
//! Run against a real Postgres instance:
//!   TEST_POSTGRES_URL=postgres://user:pass@localhost/wicked_test \
//!   cargo test -p wicked-estate-store --features postgres

#![cfg(feature = "postgres")]

use wicked_estate_core::{
    Edge, EdgeKind, GraphRead, GraphWrite, Language, Location, Node, NodeKind, ResolutionTier,
    Span, SymbolId,
};

/// Both tests hit the same database (same table names). The Rust test harness runs tests in one
/// binary concurrently, so serialize them — a fresh-schema drop racing another test's writes
/// would produce phantom failures unrelated to the contract.
static PG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Cross-BINARY serialization: this file and `team_runtime.rs` share one database. Cargo
/// currently runs integration-test binaries sequentially, but that is an implementation
/// detail, not a contract — a session-level Postgres advisory lock makes the isolation
/// explicit. Held for the whole test; released on drop (and by the server if the session
/// dies). Same key in both files.
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

/// Drop every table the store creates so each test starts with a fresh schema.
/// `symbol_gen` matters: its `had_node` marker is sticky by design, so a leftover row from a
/// prior run would flip first-insert epochs from 0 to 1 and fail the epoch conformance block.
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
             unresolved_refs, edges, nodes, node_files, files, symbol_gen, \
             support_owners, edge_supports, edge_base CASCADE",
        )
        .execute(&pool)
        .await
    })
    .expect("drop tables for fresh conformance run");
}

#[test]
fn postgres_store_satisfies_graph_store_contract() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);

    drop_all_tables(&url);

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    store.set_history_enabled(true).expect("enable history");
    wicked_estate_core::conformance::graph_store_suite(&mut store);
}

/// Multi-file symbol contributions (M4 / Option A — wicked-estate#152) on a live Postgres — the
/// SAME shared suite the Mem/Sqlite tests pin (tests/conformance.rs), so the PG `node_files`
/// contribution table, definition-preferred derived primary, and remove_file survivor re-home
/// cannot drift without a test signal.
#[test]
fn postgres_store_satisfies_multi_file_contribution_contract() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);

    drop_all_tables(&url);

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    wicked_estate_core::conformance::multi_file_contribution_suite(&mut store);
}

/// TS-S2A authoritative, replaceable edge support on a live Postgres — the SAME shared suite the
/// Mem/Sqlite/Surreal tests run, so the PG support tables, the per-replacement transaction (a
/// SAVEPOINT inside an open batch) and the remove_file/prune heal cannot drift silently.
#[test]
fn postgres_store_satisfies_support_replacement_contract() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);

    drop_all_tables(&url);

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    wicked_estate_core::conformance::support_replacement_suite(&mut store);
}

/// The back-fill support surface (#141) on a live Postgres — the SAME shared body the
/// Mem/Sqlite unit tests pin, so the PG implementations of `parked_relative_import_refs`
/// and `delete_unresolved_refs` cannot drift without a test signal.
#[test]
fn postgres_store_satisfies_backfill_support_contract() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);

    drop_all_tables(&url);

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    wicked_estate_store::backfill_conformance::backfill_support_suite(&mut store);
}

fn node(name: &str, file: &str) -> Node {
    Node::new(
        SymbolId(format!("torn:{name}")),
        NodeKind::Function,
        name,
        Language::new("rust"),
        Location::new(file, Span::ZERO),
    )
}

fn edge(a: &Node, b: &Node) -> Edge {
    Edge::new(
        a.symbol.clone(),
        b.symbol.clone(),
        EdgeKind::Calls,
        ResolutionTier::Scip,
        "torn-read-test",
    )
}

/// Locked decision #8 regression test: the graph batch must be ONE transaction — a concurrent
/// reader on its own connection sees the pre-batch state or the full committed batch, NEVER a
/// partial batch (the torn read).
///
/// Choreography (channel-synchronized, deterministic — not a timing lottery):
///   writer:  begin_batch → upsert first half → [A] → wait [B] → upsert second half + edges
///            → commit_batch → [C]
///   reader:  wait [A] → sample counts (must be 0 or FULL; the old per-statement auto-commit
///            impl deterministically shows the half-written batch here) → [B] → wait [C]
///            → sample counts (must be FULL)
///
/// Also pins read-your-own-writes INSIDE the batch on the writer store: the resolver's
/// `SymbolIndex` lookups during `index_path` must see nodes written earlier in the same batch.
#[test]
fn postgres_batch_commits_atomically_no_torn_reads() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_torn_read: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);

    drop_all_tables(&url);

    const HALF: usize = 50;
    const FULL: u64 = (HALF as u64) * 2;

    let nodes: Vec<Node> = (0..HALF * 2)
        .map(|i| node(&format!("fn_{i:03}"), "src/torn.rs"))
        .collect();
    let edges: Vec<Edge> = nodes.windows(2).map(|w| edge(&w[0], &w[1])).collect();

    // Open BOTH stores before the batch starts: `open` runs `ALTER TABLE … IF NOT EXISTS`
    // migrations that need locks an open batch transaction would block on.
    let reader = wicked_estate_store::PostgresStore::open(&url).expect("open reader store");
    let mut writer = wicked_estate_store::PostgresStore::open(&url).expect("open writer store");

    let (tx_a, rx_a) = std::sync::mpsc::channel::<()>();
    let (tx_b, rx_b) = std::sync::mpsc::channel::<()>();
    let (tx_c, rx_c) = std::sync::mpsc::channel::<()>();

    let writer_nodes = nodes.clone();
    let writer_edges = edges.clone();
    let writer_thread = std::thread::spawn(move || {
        writer.begin_batch().expect("begin_batch");
        writer
            .upsert_nodes(&writer_nodes[..HALF])
            .expect("upsert first half");

        // Read-your-own-writes inside the open batch (same store, same transaction).
        let own = writer.stats().expect("writer stats").node_count;
        assert_eq!(
            own, HALF as u64,
            "writer must see its own uncommitted batch writes (resolver depends on this)"
        );

        tx_a.send(()).expect("signal A");
        rx_b.recv().expect("wait B");

        writer
            .upsert_nodes(&writer_nodes[HALF..])
            .expect("upsert second half");
        writer.upsert_edges(&writer_edges).expect("upsert edges");
        writer.commit_batch().expect("commit_batch");
        tx_c.send(()).expect("signal C");
    });

    // [A] — the writer is mid-batch with exactly HALF nodes written and uncommitted.
    rx_a.recv().expect("wait A");
    let mid = reader.stats().expect("reader stats mid-batch");
    assert!(
        mid.node_count == 0 || mid.node_count == FULL,
        "TORN READ: concurrent reader saw {} of {FULL} nodes mid-batch — \
         the graph batch is not one transaction (locked decision #8)",
        mid.node_count
    );
    assert!(
        mid.edge_count == 0 || mid.edge_count == FULL - 1,
        "TORN READ: concurrent reader saw {} of {} edges mid-batch",
        mid.edge_count,
        FULL - 1
    );

    tx_b.send(()).expect("signal B");
    rx_c.recv().expect("wait C");
    writer_thread.join().expect("writer thread");

    // After commit the full batch is visible — all nodes AND all edges.
    let after = reader.stats().expect("reader stats post-commit");
    assert_eq!(after.node_count, FULL, "full batch visible after commit");
    assert_eq!(
        after.edge_count,
        FULL - 1,
        "all batch edges visible after commit"
    );
}

/// TS-S2A on concurrent Postgres writers (`shared_writers: true`): replacements are serialized by
/// the support advisory lock, so racing generations can never move an owner backwards or leave it
/// holding a mix of two generations' facts. Generation 3 always wins; generation 2 either applied
/// first (and was replaced) or was refused as stale. Repeated to give the race room to happen.
#[test]
fn postgres_concurrent_support_replacements_serialize() {
    use wicked_estate_core::SupportOwner;
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);
    drop_all_tables(&url);

    let fact = |target: &str| {
        Edge::new(
            SymbolId("race:src".into()),
            SymbolId(format!("race:{target}")),
            EdgeKind::Calls,
            ResolutionTier::Scip,
            "scip-typescript",
        )
    };
    let gen2: Vec<Edge> = (0..20).map(|i| fact(&format!("two{i}"))).collect();
    let gen3: Vec<Edge> = (0..20).map(|i| fact(&format!("three{i}"))).collect();
    for round in 0..10 {
        let owner = SupportOwner::new("race", format!("round{round}")).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        // Both stores are opened up front: concurrent `CREATE TABLE IF NOT EXISTS` is itself a
        // Postgres race, and a panic before the barrier would strand the other thread.
        let spawn = |generation: u64, facts: Vec<Edge>| {
            let (owner, barrier) = (owner.clone(), barrier.clone());
            let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open");
            let facts: Vec<wicked_estate_core::SupportFact> = facts
                .into_iter()
                .map(|e| wicked_estate_core::SupportFact::from_edge(e).expect("valid fact"))
                .collect();
            std::thread::spawn(move || {
                barrier.wait();
                store.replace_edge_supports(&owner, generation, &facts)
            })
        };
        let a = spawn(2, gen2.clone());
        let b = spawn(3, gen3.clone());
        let ra = a.join().expect("thread a");
        b.join()
            .expect("thread b")
            .expect("generation 3 always applies");
        if let Err(e) = ra {
            assert!(e.to_string().contains("stale generation"), "{e}");
        }
        let store = wicked_estate_store::PostgresStore::open(&url).expect("open");
        assert_eq!(
            store.support_generation(&owner).unwrap(),
            Some(3),
            "round {round}"
        );
        for f in &gen2 {
            assert!(
                store
                    .edge_supports(&f.source, &f.target, &f.kind)
                    .unwrap()
                    .iter()
                    .all(|r| r.owner != owner),
                "round {round}: a generation-2 fact survived generation 3"
            );
        }
        for f in &gen3 {
            assert_eq!(
                store
                    .edge_supports(&f.source, &f.target, &f.kind)
                    .unwrap()
                    .iter()
                    .filter(|r| r.owner == owner)
                    .count(),
                1,
                "round {round}: every generation-3 fact is held once"
            );
        }
    }
}

/// TS-S2A atomicity on a live Postgres, with the failure INSIDE the write: a trigger raises on a
/// poison row after the replacement already deleted a retracted fact and inserted a new one.
/// The store must be unchanged afterwards — outside a batch (the replacement's own transaction)
/// and inside one (a SAVEPOINT), where the caller's earlier batch writes must survive the commit.
#[test]
fn postgres_replacement_rolls_back_a_failure_inside_the_transaction() {
    use wicked_estate_core::{SupportFact, SupportOwner};
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("postgres_conformance: TEST_POSTGRES_URL not set — skipping");
            return;
        }
    };
    let _guard = PG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lease = PgTestLease::acquire(&url);
    drop_all_tables(&url);

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    let raw = |sql: &str| {
        use sqlx::postgres::PgPoolOptions;
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        let pool = rt
            .block_on(PgPoolOptions::new().max_connections(1).connect(&url))
            .expect("connect");
        rt.block_on(sqlx::raw_sql(sql).execute(&pool))
            .expect("raw sql");
    };
    let fact = |t: &str, by: &str| {
        SupportFact::new(
            format!("occ:{t}"),
            Edge::new(
                SymbolId("pg:src".into()),
                SymbolId(format!("pg:{t}")),
                EdgeKind::Calls,
                ResolutionTier::Scip,
                by,
            ),
        )
        .expect("fact")
    };
    let snapshot = |store: &wicked_estate_store::PostgresStore, owner: &SupportOwner| {
        let mut edges = store.all_edges().unwrap();
        edges.sort_by_key(|e| e.dedup_key());
        let mut rows = Vec::new();
        for t in ["a", "b", "c", "d"] {
            let f = fact(t, "x");
            rows.extend(
                store
                    .edge_supports(&f.edge.source, &f.edge.target, &f.edge.kind)
                    .unwrap(),
            );
        }
        (edges, rows, store.support_generation(owner).unwrap())
    };
    for in_batch in [false, true] {
        let owner = SupportOwner::new("scip-typescript", format!("web-{in_batch}")).unwrap();
        raw("DROP TRIGGER IF EXISTS ts_s2a_poison ON edge_supports;");
        store
            .replace_edge_supports(&owner, 1, &[fact("a", "scip"), fact("b", "scip")])
            .unwrap();
        raw(
            "CREATE OR REPLACE FUNCTION ts_s2a_poison() RETURNS trigger AS $$ \
             BEGIN IF NEW.data LIKE '%poison%' THEN RAISE EXCEPTION 'injected storage failure'; \
             END IF; RETURN NEW; END $$ LANGUAGE plpgsql; \
             CREATE TRIGGER ts_s2a_poison BEFORE INSERT ON edge_supports \
             FOR EACH ROW EXECUTE FUNCTION ts_s2a_poison();",
        );
        let before = snapshot(&store, &owner);
        if in_batch {
            store.begin_batch().unwrap();
            store
                .upsert_nodes(&[Node::new(
                    SymbolId(format!("pg:batch_node_{in_batch}")),
                    NodeKind::Function,
                    "batch_node",
                    Language::new("rust"),
                    Location::new("b.rs", Span::ZERO),
                )])
                .unwrap();
        }
        // `{a,b}` → `{b,c,d}`: a's delete and c's insert run before the poison row (d).
        let err = store
            .replace_edge_supports(
                &owner,
                2,
                &[fact("b", "scip"), fact("c", "scip"), fact("d", "poison")],
            )
            .expect_err("the trigger aborts the replacement");
        assert!(
            err.to_string().contains("injected storage failure"),
            "{err}"
        );
        if in_batch {
            store.commit_batch().unwrap();
            assert!(
                store
                    .get_node(&SymbolId(format!("pg:batch_node_{in_batch}")))
                    .unwrap()
                    .is_some(),
                "a failed replacement must not roll back the caller's batch"
            );
        }
        assert_eq!(
            snapshot(&store, &owner),
            before,
            "no half-old/half-new generation (in_batch={in_batch})"
        );
        raw("DROP TRIGGER IF EXISTS ts_s2a_poison ON edge_supports;");
        let ok = store
            .replace_edge_supports(&owner, 2, &[fact("b", "scip"), fact("c", "scip")])
            .expect("generation 2 is still free after the rollback");
        assert_eq!((ok.asserted, ok.retained, ok.retracted), (1, 1, 1));
        store.replace_edge_supports(&owner, 3, &[]).unwrap();
    }
}
