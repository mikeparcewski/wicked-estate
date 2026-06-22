//! Connection pool for `SqliteStore` — implements `AsyncGraphStore` via deadpool.
//!
//! Enabled with the `pool` feature. The pool holds `SqliteStore` objects; each
//! `with_read` call checks out one store, runs the caller's closure in
//! `spawn_blocking`, and returns the result.

use std::path::PathBuf;

use deadpool::managed::{Manager, Metrics, Pool, RecycleResult};
use wicked_estate_core::{AsyncGraphStore, GraphRead, Result};

use crate::SqliteStore;

struct SqliteManager {
    path: PathBuf,
}

impl Manager for SqliteManager {
    type Type = SqliteStore;
    type Error = wicked_estate_core::Error;

    async fn create(&self) -> std::result::Result<SqliteStore, wicked_estate_core::Error> {
        SqliteStore::open_file(&self.path)
    }

    async fn recycle(
        &self,
        _obj: &mut SqliteStore,
        _metrics: &Metrics,
    ) -> RecycleResult<wicked_estate_core::Error> {
        Ok(())
    }
}

/// A pool of `SqliteStore` connections. Newtype over `Pool<SqliteManager>` so we can
/// implement `AsyncGraphStore` (an external trait) without violating the orphan rule.
#[derive(Clone)]
pub struct SqlitePool(Pool<SqliteManager>);

/// Open a `SqlitePool` pointing at `path` with up to `max_size` concurrent connections.
pub fn open_sqlite_pool(path: &str, max_size: usize) -> Result<SqlitePool> {
    let manager = SqliteManager {
        path: PathBuf::from(path),
    };
    Pool::builder(manager)
        .max_size(max_size)
        .build()
        .map(SqlitePool)
        .map_err(|e| wicked_estate_core::Error::Invalid(e.to_string()))
}

impl SqlitePool {
    /// Look up a cached response. Returns `None` if the key is absent or from an old graph version.
    pub async fn cache_get(&self, key: &str) -> wicked_estate_core::Result<Option<String>> {
        let key = key.to_string();
        let obj = self.0.get().await.map_err(|e| {
            wicked_estate_core::Error::Invalid(format!("pool checkout failed: {e}"))
        })?;
        tokio::task::block_in_place(move || obj.cache_get(&key))
    }

    /// Store a response for the current graph version.
    pub async fn cache_put(&self, key: &str, value: &str) -> wicked_estate_core::Result<()> {
        let key = key.to_string();
        let value = value.to_string();
        let mut obj = self.0.get().await.map_err(|e| {
            wicked_estate_core::Error::Invalid(format!("pool checkout failed: {e}"))
        })?;
        tokio::task::block_in_place(move || obj.cache_put(&key, &value))
    }
}

#[async_trait::async_trait]
impl AsyncGraphStore for SqlitePool {
    async fn with_read<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'a> FnOnce(&'a dyn GraphRead) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let obj = self.0.get().await.map_err(|e| {
            wicked_estate_core::Error::Invalid(format!("pool checkout failed: {e}"))
        })?;
        tokio::task::spawn_blocking(move || f(&*obj))
            .await
            .map_err(|e| {
                wicked_estate_core::Error::Invalid(format!("spawn_blocking panicked: {e}"))
            })?
    }

    async fn with_read_inline<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'a> FnOnce(&'a dyn GraphRead) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        // `get().await` is the only await; deadpool's RAII `Object` returns the connection on drop.
        let obj = self.0.get().await.map_err(|e| {
            wicked_estate_core::Error::Invalid(format!("pool checkout failed: {e}"))
        })?;
        // Run `f` on the CURRENT thread — NO nested `spawn_blocking`. The caller is already on a
        // blocking-pool thread (Lane X `OverlayReader`), so this holds exactly ONE blocking thread
        // per cross-recall instead of `1+k`; that bound is what the DoD-XA1b saturation gate asserts.
        f(&*obj)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Conformance tests — AsyncGraphStore contract
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{
        AsyncGraphStore, Edge, GraphWrite, Node,
        edge::{EdgeKind, ResolutionTier},
        node::{Language, Location, NodeKind, Span},
        symbol::{Descriptor, Symbol},
    };

    fn sym(name: &str) -> wicked_estate_core::symbol::SymbolId {
        Symbol::global("test", None, vec![Descriptor::method(name, None)]).id()
    }

    fn func_node(name: &str) -> Node {
        Node::new(
            sym(name),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new("src/lib.rs", Span::ZERO),
        )
    }

    fn calls_edge(a: &str, b: &str) -> Edge {
        Edge::new(
            sym(a),
            sym(b),
            EdgeKind::Calls,
            ResolutionTier::Scip,
            "pool-test",
        )
    }

    /// Write fixture data via a plain `SqliteStore`, then verify the pool reads it back.
    #[tokio::test]
    async fn pool_with_read_sees_written_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap();

        {
            let mut store = SqliteStore::open(path_str).unwrap();
            store.begin_batch().unwrap();
            store
                .upsert_nodes(&[func_node("a"), func_node("b")])
                .unwrap();
            store.upsert_edges(&[calls_edge("a", "b")]).unwrap();
            store.commit_batch().unwrap();
        }

        let pool = open_sqlite_pool(path_str, 4).unwrap();
        let stats = pool.with_read(|g| g.stats()).await.unwrap();
        assert_eq!(stats.node_count, 2, "expected 2 nodes");
        assert_eq!(stats.edge_count, 1, "expected 1 edge");
    }

    /// DoD-XA1b — `with_read_inline` must NOT deadlock under saturation. N = 2×cap concurrent
    /// cross-recalls each simulate Lane X's `OverlayReader`: a `spawn_blocking` thread that
    /// `block_on`s an inline read. With a SMALL `max_blocking_threads`, the correct inline impl
    /// (no nested `spawn_blocking`) holds exactly ONE blocking thread per recall and completes all
    /// N (~100ms). A regression to a `spawn_blocking`-nesting impl would need `1+k` blocking threads
    /// per recall, exhaust the pool, and deadlock — caught here as a timeout. The connection pool is
    /// sized generously so the property under test is BLOCKING-thread occupancy, not connections.
    #[test]
    fn with_read_inline_no_deadlock_under_saturation() {
        const CAP: usize = 4;
        let n = 2 * CAP; // 8 ≥ 2×cap

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(CAP)
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("deadlock.db");
            let path_str = path.to_str().unwrap().to_string();
            {
                let mut store = SqliteStore::open(&path_str).unwrap();
                store.begin_batch().unwrap();
                store
                    .upsert_nodes(&[func_node("a"), func_node("b")])
                    .unwrap();
                store.commit_batch().unwrap();
            }

            let pool = open_sqlite_pool(&path_str, n).unwrap();
            let handle = tokio::runtime::Handle::current();
            let mut joins = Vec::new();
            for _ in 0..n {
                let pool = pool.clone();
                let h = handle.clone();
                joins.push(tokio::task::spawn_blocking(move || {
                    // OverlayReader shape: already on a blocking thread, block_on an inline read.
                    h.block_on(async move {
                        pool.with_read_inline(|g: &dyn GraphRead| {
                            // Hold the inline path briefly so all N overlap → real saturation.
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            g.all_nodes().map(|v| v.len())
                        })
                        .await
                    })
                }));
            }

            // A deadlock manifests as a timeout; the correct impl finishes well under the bound.
            let counts = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                let mut out = Vec::new();
                for j in joins {
                    out.push(
                        j.await
                            .expect("spawn_blocking join")
                            .expect("with_read_inline"),
                    );
                }
                out
            })
            .await
            .expect("N concurrent inline cross-recalls must NOT deadlock (timeout == deadlock)");

            assert_eq!(counts.len(), n, "all {n} concurrent recalls completed");
            assert!(
                counts.iter().all(|&c| c == 2),
                "each recall sees the 2 written nodes; got {counts:?}"
            );
        });
    }

    /// `with_read` must propagate errors returned by the closure.
    #[tokio::test]
    async fn pool_closure_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("err.db");
        let _ = SqliteStore::open(path.to_str().unwrap()).unwrap();

        let pool = open_sqlite_pool(path.to_str().unwrap(), 2).unwrap();
        let result = pool
            .with_read(|_g| -> wicked_estate_core::Result<()> {
                Err(wicked_estate_core::Error::Invalid("intentional".into()))
            })
            .await;
        assert!(result.is_err(), "closure error must surface");
    }

    /// Clone is cheap (pool is internally Arc-backed); concurrent reads must all succeed.
    #[tokio::test]
    async fn pool_clone_and_concurrent_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conc.db");
        let _ = SqliteStore::open(path.to_str().unwrap()).unwrap();

        let pool = open_sqlite_pool(path.to_str().unwrap(), 4).unwrap();
        let mut handles = Vec::new();
        for _ in 0..8 {
            let p = pool.clone();
            handles.push(tokio::spawn(async move {
                p.with_read(|g| g.stats()).await.unwrap()
            }));
        }
        for h in handles {
            h.await.expect("task panicked");
        }
    }
}
