//! Cross-store recall — the differentiator, as a production capability (feature `cross`).
//!
//! [`OverlayMemStore`] is a [`MemStore`] whose graph reads route through a `wicked-overlay`
//! [`OverlayReader`]: **HOME** = a code-graph engine (a Sync estate `MemStore`), **FOREIGN** = one or
//! more knowledge-engine pools, bridged by `about` rows in an [`XedgeStore`]. A [`MemoryEngine`] runs
//! over it unchanged (`MemoryEngine::with_backend`), so recall from a code seed folds the `about`
//! cross-edge and hydrates the grounding doc out of the *foreign* store — even with **zero lexical
//! overlap** between query and doc (which FTS/BM25 cannot bridge). This is the same seam the
//! `xedge_bench` landing gate exercises, lifted from the bench into the engine.
//!
//! ```ignore
//! use std::{collections::HashMap, sync::Arc};
//! use wicked_memory::{MemoryEngine, RecallMode};
//! use wicked_memory::cross::{ForeignEngine, ForeignPools, OverlayMemStore, XEdge, XedgeStore, open_sqlite_pool};
//!
//! let mut others: ForeignPools = HashMap::new();
//! others.insert("memory", Arc::new(open_sqlite_pool("knowledge.db", 4)?) as Arc<dyn ForeignEngine>);
//! let store = OverlayMemStore::new(home_code_store, "estate", Arc::new(others), xedge, vec!["about".into()]);
//! let eng = MemoryEngine::with_backend(Box::new(store), ":memory:")?;
//! let hits = eng.recall_mode("what should I know here", &scope, &[code_seed], 2000, now, RecallMode::Hybrid)?;
//! ```

use std::sync::Arc;

use wicked_estate_core::{
    Direction, Edge, GraphRead, GraphWrite, Node, Result, StoreCapabilities, Subgraph, SymbolId,
    SymbolQuery, TraversalSpec, annotation::Annotation, change::Change, change::ChangeOp,
    history::HistoricalEdge, query::GraphStats, refs::UnresolvedRef, repo::RepoInfo,
    semantics::NodeSemantics,
};
use wicked_estate_store::MemStore as EstateMemStore;

// Re-exported so a consumer wires cross-store recall without depending on wicked-overlay /
// wicked-estate-store directly.
pub use wicked_estate_overlay::{
    CrossBudget, ForeignEngine, ForeignPools, OverlayReader, XEdge, XedgeStore,
};
pub use wicked_estate_store::{SqlitePool, open_sqlite_pool};

use crate::MemStore;

/// A [`MemStore`] backend that unions a code-graph HOME with foreign knowledge pool(s) at read time
/// via `about` cross-edges. See the module docs for the wiring.
pub struct OverlayMemStore {
    home: EstateMemStore,
    home_engine: &'static str,
    others: Arc<ForeignPools>,
    xedge: XedgeStore,
    cross_edge_kinds: Vec<String>,
}

impl OverlayMemStore {
    /// Build the cross-store backend. `home` holds the code seeds; `home_engine` is its xedge tag
    /// (e.g. `"estate"`); `others` maps foreign engine tags → pools; `xedge` holds the `about` rows;
    /// `cross_edge_kinds` = `["about"]` folds the cross edges (`[]` = home-only baseline).
    pub fn new(
        home: EstateMemStore,
        home_engine: &'static str,
        others: Arc<ForeignPools>,
        xedge: XedgeStore,
        cross_edge_kinds: Vec<String>,
    ) -> Self {
        Self {
            home,
            home_engine,
            others,
            xedge,
            cross_edge_kinds,
        }
    }

    /// Build an `OverlayReader` borrowing the home, for one graph read.
    fn overlay(&self) -> OverlayReader<'_, EstateMemStore> {
        OverlayReader::new(
            &self.home,
            self.home_engine,
            self.xedge.reader(),
            Arc::clone(&self.others),
            self.cross_edge_kinds.clone(),
            CrossBudget::default(),
        )
    }
}

// Graph READS route through the overlay; everything else is HOME-ONLY (delegated to the home store),
// so no foreign node leaks into keyword/vector search — the gold is reachable only via the `about` fold.
impl GraphRead for OverlayMemStore {
    fn capabilities(&self) -> StoreCapabilities {
        self.overlay().capabilities()
    }
    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>> {
        self.overlay().get_node(id)
    }
    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>> {
        self.overlay().find_symbols(query)
    }
    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>> {
        self.overlay().neighbors(id, dir)
    }
    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph> {
        self.overlay().traverse(start, spec)
    }
    fn traverse_multi(&self, starts: &[SymbolId], spec: &TraversalSpec) -> Result<Subgraph> {
        self.overlay().traverse_multi(starts, spec)
    }
    fn all_nodes(&self) -> Result<Vec<Node>> {
        self.overlay().all_nodes()
    }
    fn all_edges(&self) -> Result<Vec<Edge>> {
        self.overlay().all_edges()
    }
    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>> {
        self.overlay().unresolved_refs_for_name(name)
    }
    fn file_digest(&self, file: &str) -> Result<Option<String>> {
        self.overlay().file_digest(file)
    }
    fn file_git_sha(&self, file: &str) -> Result<Option<String>> {
        self.overlay().file_git_sha(file)
    }
    fn repo_info(&self) -> Result<Option<RepoInfo>> {
        self.overlay().repo_info()
    }
    fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>> {
        self.overlay().edge_history(file)
    }
    fn file_content(&self, file: &str) -> Result<Option<String>> {
        self.overlay().file_content(file)
    }
    fn symbol_source(&self, node: &Node) -> Result<Option<String>> {
        self.overlay().symbol_source(node)
    }
    fn changes_since(&self, cursor: u64) -> Result<Vec<Change>> {
        self.overlay().changes_since(cursor)
    }
    fn node_semantics(&self, symbol: &SymbolId) -> Result<Option<NodeSemantics>> {
        self.overlay().node_semantics(symbol)
    }
    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>> {
        self.overlay().find_by_requirement(requirement)
    }
    fn annotations(&self, symbol: &SymbolId) -> Result<Vec<Annotation>> {
        self.overlay().annotations(symbol)
    }
    fn annotations_by_type(&self, ty: &str) -> Result<Vec<(SymbolId, Annotation)>> {
        self.overlay().annotations_by_type(ty)
    }
    fn annotations_stale_since(&self, cutoff: i64) -> Result<Vec<(SymbolId, Annotation)>> {
        self.overlay().annotations_stale_since(cutoff)
    }
    fn symbol_epoch(&self, id: &SymbolId) -> Result<Option<u64>> {
        self.overlay().symbol_epoch(id)
    }
    fn stats(&self) -> Result<GraphStats> {
        self.overlay().stats()
    }
}

// GraphWrite — HOME-ONLY (capture happens on the home / knowledge engines before wrapping).
impl GraphWrite for OverlayMemStore {
    fn begin_batch(&mut self) -> Result<()> {
        self.home.begin_batch()
    }
    fn commit_batch(&mut self) -> Result<()> {
        self.home.commit_batch()
    }
    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()> {
        self.home.upsert_nodes(nodes)
    }
    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()> {
        self.home.upsert_edges(edges)
    }
    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()> {
        self.home.upsert_unresolved_refs(refs)
    }
    fn remove_file(&mut self, file: &str) -> Result<()> {
        self.home.remove_file(file)
    }
    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()> {
        self.home.set_file_digest(file, digest)
    }
    fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
        self.home.set_repo_info(info)
    }
    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()> {
        self.home.set_file_content(file, text)
    }
    fn prune_dangling_edges(&mut self) -> Result<usize> {
        self.home.prune_dangling_edges()
    }
    fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()> {
        self.home.log_change(op, target)
    }
    fn set_node_semantics(
        &mut self,
        symbol: &SymbolId,
        description: Option<&str>,
        requirement: Option<&str>,
        requirement_validated: Option<bool>,
    ) -> Result<()> {
        self.home
            .set_node_semantics(symbol, description, requirement, requirement_validated)
    }
    fn annotate(&mut self, symbol: &SymbolId, annotation: Annotation) -> Result<()> {
        self.home.annotate(symbol, annotation)
    }
    fn delete_annotations(
        &mut self,
        symbol: &SymbolId,
        ty: Option<&str>,
        key: &str,
    ) -> Result<usize> {
        self.home.delete_annotations(symbol, ty, key)
    }
}

// MemStore — the vector arm stays HOME-ONLY (the gold lives in the foreign store, reachable only via
// the `about` fold — never the vector index, so a vector hit can't masquerade as the cross lift).
impl MemStore for OverlayMemStore {
    fn set_embedding(&mut self, symbol: &SymbolId, vec: &[f32]) -> Result<()> {
        self.home.set_embedding(symbol, vec)
    }
    fn nearest(&self, query_vec: &[f32], k: usize) -> Result<Vec<(SymbolId, f32)>> {
        self.home.nearest(query_vec, k)
    }
    fn remove_nodes(&mut self, _ids: &[SymbolId]) -> Result<usize> {
        // Erase happens on the home / knowledge engines before wrapping; fail LOUD rather than a
        // silent (0-rows) success so a caller can't mistake an unsupported delete for a no-op.
        Err(wicked_estate_core::Error::Invalid(
            "OverlayMemStore::remove_nodes is unsupported — erase on the home/foreign engine"
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wicked_estate_core::Symbol;
    use wicked_estate_memory_core::{MemKind, Memory, Scope, Tier};

    use crate::{MemoryEngine, RecallMode};

    fn code_seed_node(name: &str) -> Node {
        use wicked_estate_core::node::{Language, Location, NodeKind, Span};
        Node::new(
            Symbol::synthetic("code", name).id(),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new("src/code.rs", Span::ZERO),
        )
    }

    // Zero-lexical-overlap corpus: each query shares NO content words with its gold doc, so recall can
    // only come via the `about` cross-edge — not keyword match.
    const DOCS: &[&str] = &[
        "always pass an idempotency token so a retried submission never double-charges the buyer",
        "refresh tokens are single-use; reuse of a consumed one revokes the whole family as a theft signal",
        "oversell is prevented by a conditional decrement that aborts when the counter would drop below zero",
    ];
    const QUERIES: &[&str] = &[
        "what should I keep in mind when working here",
        "is there anything subtle to remember for this part",
        "what do we know that matters around this module",
    ];
    const SEEDS: &[&str] = &["checkout_service", "auth_session", "inventory_ledger"];

    /// Recall each query FROM its code seed over a `MemoryEngine` backed by an `OverlayMemStore`.
    /// `cross_edge_kinds = ["about"]` ⇒ the fold fires (cross-ON); `[]` ⇒ home-only (the gold is in
    /// the foreign store, unreachable). Returns how many gold docs were recalled.
    fn cross_recall_hits(cross_edge_kinds: Vec<String>) -> usize {
        let scope = Scope::root();
        // Gold docs built ONCE so the uuid-minted id is shared between the foreign store + the xedge.
        let gold: Vec<Memory> = DOCS
            .iter()
            .enumerate()
            .map(|(i, d)| {
                Memory::new(
                    MemKind::Fact,
                    Tier::Semantic,
                    scope.clone(),
                    *d,
                    5000 + i as i64,
                )
            })
            .collect();

        // FOREIGN knowledge store (file-backed) holding ONLY the gold docs; open a read pool over it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        let path_str = path.to_str().unwrap().to_string();
        {
            let mut knowledge = MemoryEngine::open(&path_str).unwrap();
            for mem in &gold {
                knowledge.capture(mem).unwrap();
            }
        }
        let pool = open_sqlite_pool(&path_str, 4).unwrap();

        // HOME code-graph engine (the code seeds only).
        let mut home = EstateMemStore::new();
        home.begin_batch().unwrap();
        let code_nodes: Vec<Node> = SEEDS.iter().map(|n| code_seed_node(n)).collect();
        home.upsert_nodes(&code_nodes).unwrap();
        home.commit_batch().unwrap();

        // FOREIGN pools + the `about` xedge (each gold doc --about--> its code seed).
        let mut others: ForeignPools = HashMap::new();
        others.insert("memory", Arc::new(pool) as Arc<dyn ForeignEngine>);
        let xedge = XedgeStore::in_memory().unwrap();
        for (i, mem) in gold.iter().enumerate() {
            let code_id = Symbol::synthetic("code", SEEDS[i]).id();
            xedge
                .put_edge(&XEdge::about(mem.symbol().0, code_id.0, 0))
                .unwrap();
        }

        let store = OverlayMemStore::new(home, "estate", Arc::new(others), xedge, cross_edge_kinds);

        // Drive the OverlayReader's foreign fold on a blocking thread under a multi-thread runtime
        // (`Handle::block_on` from `spawn_blocking` — the no-deadlock seam, DEC-X1b).
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            tokio::task::spawn_blocking(move || {
                let eng = MemoryEngine::with_backend(Box::new(store), ":memory:").unwrap();
                let mut hits = 0usize;
                for i in 0..QUERIES.len() {
                    let seed = Symbol::synthetic("code", SEEDS[i]).id();
                    let found = eng
                        .recall_mode(
                            QUERIES[i],
                            &scope,
                            std::slice::from_ref(&seed),
                            2000,
                            6000,
                            RecallMode::Hybrid,
                        )
                        .unwrap()
                        .iter()
                        .any(|r| r.content == DOCS[i]);
                    if found {
                        hits += 1;
                    }
                }
                hits
            })
            .await
            .unwrap()
        })
    }

    #[test]
    fn cross_store_recall_lifts_a_doc_from_a_code_seed() {
        let cross_off = cross_recall_hits(vec![]);
        let cross_on = cross_recall_hits(vec!["about".to_string()]);
        assert_eq!(
            cross_off, 0,
            "home-only baseline finds no gold (it lives only in the foreign store)"
        );
        assert!(
            cross_on > cross_off,
            "the `about` fold must LIFT recall: on={cross_on} off={cross_off}"
        );
        assert_eq!(
            cross_on,
            DOCS.len(),
            "cross-ON recalls every gold doc from its code seed"
        );
    }
}
