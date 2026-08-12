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
             unresolved_refs, edges, nodes, files, symbol_gen CASCADE",
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

    drop_all_tables(&url);

    let mut store = wicked_estate_store::PostgresStore::open(&url).expect("open postgres store");
    store.set_history_enabled(true).expect("enable history");
    wicked_estate_core::conformance::graph_store_suite(&mut store);
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
