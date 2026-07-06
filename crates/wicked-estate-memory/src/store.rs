//! `MemStore` — the storage abstraction the engine runs on, so memory is **backend-agnostic** (the
//! M-PG foundation). It is `GraphStore` (all the graph read/write the engine needs) plus the three
//! capabilities estate keeps inherent on concrete stores: embeddings, vector-nearest, and node
//! deletion. The engine holds a `Box<dyn MemStore>`; SQLite is the only impl built+verified here, and
//! a `PostgresStore` impl drops in behind the same trait once developed against a live PG (see
//! `08-FINISH-PLAN.md` M-PG — pgvector for `nearest`, a `remove_nodes` on the PG store).

use wicked_estate_core::{GraphStore, Result, SymbolId};
use wicked_estate_store::SqliteStore;

/// What the memory engine requires of a storage backend.
pub trait MemStore: GraphStore {
    /// Store/replace the embedding vector for a symbol.
    fn set_embedding(&mut self, symbol: &SymbolId, vec: &[f32]) -> Result<()>;
    /// `k` nearest symbols to `query_vec` by cosine similarity.
    fn nearest(&self, query_vec: &[f32], k: usize) -> Result<Vec<(SymbolId, f32)>>;
    /// Hard-delete nodes (+ their FTS/vector rows and incident edges).
    fn remove_nodes(&mut self, ids: &[SymbolId]) -> Result<usize>;
}

impl MemStore for SqliteStore {
    fn set_embedding(&mut self, symbol: &SymbolId, vec: &[f32]) -> Result<()> {
        SqliteStore::set_embedding(self, symbol, vec)
    }
    fn nearest(&self, query_vec: &[f32], k: usize) -> Result<Vec<(SymbolId, f32)>> {
        // `nearest` lives on the `VectorStore` trait (estate retrieve crate) for SqliteStore.
        <SqliteStore as wicked_estate_retrieve::VectorStore>::nearest(self, query_vec, k)
    }
    fn remove_nodes(&mut self, ids: &[SymbolId]) -> Result<usize> {
        SqliteStore::remove_nodes(self, ids)
    }
}

/// Postgres backend (durable-enterprise memory). Capability-honest to estate's design (ADR-003): the
/// PG arm reports `vector_search: false`, so embeddings are not stored and `nearest` yields nothing —
/// recall falls back to graph + ILIKE keyword retrieval (the designed RRF fallback). `remove_nodes`
/// is a real atomic delete. Compile-verified under `--features postgres`; e2e needs a live PG.
#[cfg(feature = "postgres")]
impl MemStore for wicked_estate_store::PostgresStore {
    fn set_embedding(&mut self, _symbol: &SymbolId, _vec: &[f32]) -> Result<()> {
        // No vector backend on PG (StoreCapabilities.vector_search == false) — nothing to store.
        Ok(())
    }
    fn nearest(&self, _query_vec: &[f32], _k: usize) -> Result<Vec<(SymbolId, f32)>> {
        // No vector candidates → recall fuses graph + keyword only (RRF fallback per ADR-003).
        Ok(Vec::new())
    }
    fn remove_nodes(&mut self, ids: &[SymbolId]) -> Result<usize> {
        wicked_estate_store::PostgresStore::remove_nodes(self, ids)
    }
}
