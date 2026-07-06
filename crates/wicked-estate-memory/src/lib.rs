//! `wicked-memory` — the engine: capture + conversational recall over a `wicked-estate` store.
//!
//! L0b vertical slice (thesis-first, `02-STEPBACK.md`): capture writes a memory `Node` + its
//! embedding into a `SqliteStore`; recall fuses estate FTS (BM25) + vector ANN via RRF, reranks
//! with the L0a memory math (tier × recency × salience), filters by scope (inheritance), and
//! assembles a token-budgeted pack. Estate is a **library** here.
//!
//! Scope note (L3 done): `wicked-estate-core` now has a first-class `Scope` primitive +
//! `SymbolQuery.scope_prefix` (subtree predicate, conformance-tested isolation). Memory recall uses
//! its own **ancestor** filter here because inheritance recall (see broader/root-scoped memories)
//! is the opposite direction from estate's subtree predicate; both are valid, different uses.

use wicked_estate_core::{
    Direction, Edge, EdgeKind, NodeKind, ResolutionTier, SymbolId, SymbolQuery,
};
use wicked_estate_memory_core::{
    Candidate, Memory, Salience, Scope, budget_pack, salience as compute_salience,
};
use wicked_estate_overlay::XedgeStore;
use wicked_estate_retrieve::{Embedder, HashEmbedder, hybrid_search};
use wicked_estate_store::SqliteStore;

mod api;
mod background;
mod consolidate;
/// Cross-store recall (the differentiator) — feature `cross`. See [`cross::OverlayMemStore`].
#[cfg(feature = "cross")]
pub mod cross;
mod memext;
mod store;
pub use background::{ConsolidationHandle, spawn_consolidation};
pub use consolidate::ConsolidationReport;
use memext::MemExt;
pub use store::MemStore;

/// Recall result item surfaced to the caller.
#[derive(Debug, Clone)]
pub struct Recalled {
    pub id: SymbolId,
    pub content: String,
    pub tier: wicked_estate_memory_core::Tier,
    pub score: f64,
}

/// The memory engine (L0b: single-store, single-writer).
pub struct MemoryEngine {
    store: Box<dyn MemStore>,
    ext: MemExt,
    embedder: Box<dyn Embedder>,
    sal: Salience,
    /// RRF over-fetch per retriever.
    k: usize,
    /// salience boost weight in the recall rerank.
    alpha: f64,
    /// recall forgetting floor (FR-7): candidates with salience below this are excluded from recall.
    /// Default 0.0 (nothing forgotten); set higher to actively forget low-salience memory.
    recall_floor: f64,
    pub(crate) xedge: Option<std::sync::Arc<XedgeStore>>,
}

/// Current memory-store schema version (NFR-8 — forward migration tracked via the `meta` table).
pub const MEM_SCHEMA_VERSION: i64 = 1;

/// The embedder a fresh engine uses, selected at compile time by feature:
/// `semantic-bge` → estate `FastEmbedder` (ONNX/BGE-384, highest quality), else
/// `semantic` → estate `Model2VecEmbedder` (static, no ONNX), else the dependency-free `HashEmbedder`.
/// A real-model load failure (e.g. no network for the first download) degrades gracefully to
/// `HashEmbedder` rather than failing engine open.
fn default_embedder() -> Box<dyn Embedder> {
    #[cfg(feature = "semantic-bge")]
    {
        match wicked_estate_retrieve::FastEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(err) => {
                eprintln!("wicked-memory: FastEmbedder unavailable ({err}); using HashEmbedder")
            }
        }
    }
    #[cfg(all(feature = "semantic", not(feature = "semantic-bge")))]
    {
        match wicked_estate_retrieve::Model2VecEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(err) => {
                eprintln!("wicked-memory: model2vec unavailable ({err}); using HashEmbedder")
            }
        }
    }
    Box::new(HashEmbedder::new(256))
}

/// Which retrievers a recall uses. `Hybrid` is production; the single modes back the benchmark
/// (PR-14) that proves hybrid ≥ single-mode and that cross-edges add recall lift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallMode {
    /// keyword + vector + graph fused (the production path).
    Hybrid,
    /// keyword (FTS/BM25) only.
    KeywordOnly,
    /// vector (ANN) only.
    VectorOnly,
    /// graph (`about` cross-edges) only.
    GraphOnly,
}

impl RecallMode {
    fn uses_keyword(self) -> bool {
        matches!(self, RecallMode::Hybrid | RecallMode::KeywordOnly)
    }
    fn uses_vector(self) -> bool {
        matches!(self, RecallMode::Hybrid | RecallMode::VectorOnly)
    }
    fn uses_graph(self) -> bool {
        matches!(self, RecallMode::Hybrid | RecallMode::GraphOnly)
    }
}

impl MemoryEngine {
    /// Open an in-memory engine (tests / ephemeral).
    pub fn in_memory() -> wicked_estate_core::Result<Self> {
        Ok(Self::build(
            Box::new(SqliteStore::in_memory()?),
            MemExt::open(":memory:")?,
        ))
    }

    /// Open a durable engine at `path` (sidecar index at `<path>.memext`; reconciled at open).
    pub fn open(path: &str) -> wicked_estate_core::Result<Self> {
        let eng = Self::build(Box::new(SqliteStore::open(path)?), MemExt::open(path)?);
        // Crash-recovery: rebuild the sidecar index if it diverged from the authoritative nodes.
        let all = eng.all_memories()?;
        eng.ext.reconcile(&all)?;
        Ok(eng)
    }

    /// Open on any [`MemStore`] backend (the backend-agnostic constructor; a `PostgresStore` impl
    /// plugs in here for durable-enterprise memory). `ext` is memory's local sidecar index.
    pub fn with_backend(
        store: Box<dyn MemStore>,
        ext_path: &str,
    ) -> wicked_estate_core::Result<Self> {
        let eng = Self::build(store, MemExt::open(ext_path)?);
        let all = eng.all_memories()?;
        eng.ext.reconcile(&all)?;
        Ok(eng)
    }

    fn build(store: Box<dyn MemStore>, ext: MemExt) -> Self {
        Self {
            store,
            ext,
            embedder: default_embedder(),
            sal: Salience::default(),
            k: 32,
            alpha: 0.5,
            recall_floor: 0.0,
            xedge: None,
        }
    }

    /// Wire up the shared XedgeStore for cross-engine about-edge writes.
    pub fn with_xedge_store(mut self, xedge: std::sync::Arc<XedgeStore>) -> Self {
        self.xedge = Some(xedge);
        self
    }

    /// Set a custom embedder (e.g. estate's `Model2VecEmbedder`/`FastEmbedder` for real semantic
    /// recall, vs the default dependency-free `HashEmbedder`). MUST be set before any `capture`, since
    /// the vector dimension must match across stored memory embeddings and the query. Builder.
    pub fn with_embedder(mut self, e: Box<dyn Embedder>) -> Self {
        self.embedder = e;
        self
    }

    /// Set the recall forgetting floor (FR-7): memories whose salience falls below `floor` are
    /// excluded from recall (forgotten-from-recall, distinct from physical purge). Builder.
    pub fn with_recall_floor(mut self, floor: f64) -> Self {
        self.recall_floor = floor;
        self
    }

    /// Hydrate memories from node-ids (used by consolidation candidate queries). Missing/undecodable
    /// ids are skipped.
    pub(crate) fn hydrate(&self, ids: &[String]) -> wicked_estate_core::Result<Vec<Memory>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(node) = self.store.get_node(&SymbolId(id.clone()))? {
                if let Some(m) = Memory::from_node(&node) {
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    /// All stored memories (decoded) paired with their node. Full scan — fine at local-first scale;
    /// the indexed `memory_node_ext` projection (DESIGN §4) optimizes this when it lands.
    pub fn all_memories(&self) -> wicked_estate_core::Result<Vec<Memory>> {
        let nodes = self.store.find_symbols(&SymbolQuery {
            text: None,
            exact_name: None,
            kinds: vec![NodeKind::Other("memory".into())],
            language: None,
            limit: None,
            scope_prefix: None,
        })?;
        Ok(nodes.iter().filter_map(Memory::from_node).collect())
    }

    /// Embed text with the engine's embedder (used by consolidation).
    pub(crate) fn embed_for(&self, text: &str) -> Vec<f32> {
        self.embedder.embed(text)
    }

    /// Fetch a node by id.
    pub fn node(
        &self,
        id: &SymbolId,
    ) -> wicked_estate_core::Result<Option<wicked_estate_core::Node>> {
        self.store.get_node(id)
    }

    /// Right-to-erasure (FR-15/AC-10): hard-delete every memory whose scope is within `scope_prefix`,
    /// from the graph + FTS + vector + the sidecar index. Returns the count erased. **Memory-kind
    /// guarded:** only `Other("memory")` nodes are considered (via `all_memories`), so code/infra
    /// nodes sharing a scope are never touched. Idempotent.
    pub fn erase(&mut self, scope_prefix: &str) -> wicked_estate_core::Result<usize> {
        let victims: Vec<Memory> = self
            .all_memories()?
            .into_iter()
            .filter(|m| wicked_estate_core::scope::path_in_prefix(&m.scope.as_path(), scope_prefix))
            .collect();
        let ids: Vec<SymbolId> = victims.iter().map(|m| m.symbol()).collect();
        let id_strs: Vec<String> = ids.iter().map(|s| s.0.clone()).collect();
        self.store.remove_nodes(&ids)?;
        self.ext.remove(&id_strs)?;
        Ok(ids.len())
    }

    /// The memory store's sidecar schema version (NFR-8).
    pub fn schema_version(&self) -> wicked_estate_core::Result<i64> {
        self.ext.schema_version()
    }

    /// Count stored memory nodes.
    pub fn count(&self) -> wicked_estate_core::Result<usize> {
        Ok(self
            .store
            .find_symbols(&SymbolQuery {
                text: None,
                exact_name: None,
                kinds: vec![NodeKind::Other("memory".into())],
                language: None,
                limit: None,
                scope_prefix: None,
            })?
            .len())
    }

    /// Capture a memory: cheap, synchronous, immediately recallable (node + embedding).
    pub fn capture(&mut self, mem: &Memory) -> wicked_estate_core::Result<()> {
        let node = mem.to_node();
        let sym = mem.symbol();
        self.store.upsert_nodes(&[node])?;
        let vec = self.embedder.embed(&mem.content);
        self.store.set_embedding(&sym, &vec)?;
        self.ext.upsert(mem)?; // keep the sidecar index in sync (PR-1)
        Ok(())
    }

    /// Capture a memory AND link it to the code/infra symbol(s) it concerns (the unique bet).
    /// An `about` edge (memory → code/infra `SymbolId`) makes "what do we know about X" a graph
    /// traversal — recall can surface this memory from a code seed even when keyword/vector miss it.
    pub fn capture_about(
        &mut self,
        mem: &Memory,
        about: &[SymbolId],
    ) -> wicked_estate_core::Result<()> {
        self.capture(mem)?;
        let src = mem.symbol();
        let edges: Vec<Edge> = about
            .iter()
            .map(|tgt| {
                Edge::new(
                    src.clone(),
                    tgt.clone(),
                    EdgeKind::Other("about".into()),
                    ResolutionTier::Heuristic,
                    "wicked-memory",
                )
            })
            .collect();
        if !edges.is_empty() {
            self.store.upsert_edges(&edges)?;
        }
        Ok(())
    }

    /// Resolve code/infra symbol(s) by exact name (any kind). Use to link a memory to the code it
    /// concerns (`capture_about`) or to seed recall from a symbol the agent is currently looking at.
    pub fn resolve_code(&self, name: &str) -> wicked_estate_core::Result<Vec<SymbolId>> {
        Ok(self
            .store
            .find_symbols(&SymbolQuery {
                text: None,
                exact_name: Some(name.to_string()),
                kinds: Vec::new(),
                language: None,
                limit: Some(8),
                scope_prefix: None,
            })?
            .into_iter()
            .map(|n| n.symbol)
            .collect())
    }

    /// Keyword candidates with OR-term semantics: split the query into significant terms and union
    /// per-term FTS matches (estate FTS phrase-matches the whole `text`, so multi-word queries would
    /// otherwise return nothing). First-seen order ≈ term-priority; capped at `self.k`.
    fn keyword_candidates(&self, query: &str) -> wicked_estate_core::Result<Vec<SymbolId>> {
        // Significant terms (len ≥ 2 keeps "AI"/"DB"/"ML"); fall back to the cleaned whole query.
        let mut terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 2)
            .map(|w| w.to_lowercase())
            .collect();
        if terms.is_empty() {
            let q = query.trim().to_lowercase();
            if !q.is_empty() {
                terms.push(q);
            }
        }
        // Per-term BM25 lists, EACH with the full budget (no shared-cap starvation of selective
        // terms, order-independent). Fuse them as SEPARATE RRF inputs so a memory matching multiple
        // terms ACCUMULATES score and outranks a single-term match (multi-term relevance preserved).
        let mut lists: Vec<Vec<SymbolId>> = Vec::new();
        for term in terms {
            let l: Vec<SymbolId> = self
                .store
                .find_symbols(&SymbolQuery {
                    text: Some(term),
                    exact_name: None,
                    kinds: vec![NodeKind::Other("memory".into())],
                    language: None,
                    limit: Some(self.k),
                    scope_prefix: None,
                })?
                .iter()
                .map(|n| n.symbol.clone())
                .collect();
            if !l.is_empty() {
                lists.push(l);
            }
        }
        Ok(wicked_estate_memory_core::rrf_fuse(&lists, 60.0)
            .into_iter()
            .take(self.k)
            .map(|(id, _)| id)
            .collect())
    }

    /// Memories with an `about` edge to `code_symbol` (dependents: edges where target == code).
    fn about_seed_ids(&self, seeds: &[SymbolId]) -> wicked_estate_core::Result<Vec<SymbolId>> {
        let mut seen = std::collections::BTreeSet::new();
        let mut ids = Vec::new();
        for code in seeds {
            for edge in self.store.neighbors(code, Direction::Dependents)? {
                if matches!(&edge.kind, EdgeKind::Other(k) if k == "about")
                    && seen.insert(edge.source.clone())
                {
                    ids.push(edge.source.clone());
                }
            }
        }
        Ok(ids)
    }

    /// Conversational recall: return the most relevant slice for `query` within `token_budget`,
    /// scoped to `query_scope` and its ancestors (inheritance). `now` is unix-seconds (caller-owned
    /// clock → deterministic).
    pub fn recall(
        &self,
        query: &str,
        query_scope: &Scope,
        seeds: &[SymbolId],
        token_budget: usize,
        now: i64,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        self.recall_impl(
            query,
            query_scope,
            seeds,
            token_budget,
            now,
            RecallMode::Hybrid,
        )
    }

    /// Single-retriever recall for benchmarking the hybrid uplift (PR-14). Not the production path.
    pub fn recall_mode(
        &self,
        query: &str,
        query_scope: &Scope,
        seeds: &[SymbolId],
        token_budget: usize,
        now: i64,
        mode: RecallMode,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        self.recall_impl(query, query_scope, seeds, token_budget, now, mode)
    }

    /// **Un-budget-capped ranked recall (T-Y-RANKED).** Return the top-`k` recall candidates in
    /// final-rerank-score order **WITHOUT** the [`budget_pack`] token-budget assembly — no token
    /// budget, no ≥1-Working-tier eviction. This is the fair `recall@k` comparand for the
    /// memory-≥-brain head-to-head (`design-track-baseline-v2.md` §3.4 Y-BLOCK-1, §7.1 "`recall_ranked`
    /// (no budget, no Working-eviction) frozen"): the production [`recall`]/[`recall_mode`] path runs
    /// `budget_pack`, which returns only 0–2 units at a small token budget and so caps `recall@10`
    /// near `recall@1` (the rigged-gate trap, `BUILD-SPEC.md` §7). `recall_ranked` shares the
    /// **identical** candidate-generation + rerank seam ([`Self::ranked_candidates`]) the production
    /// recall uses — it differs ONLY in the final assembly (top-`k` by `final_score` vs token-budgeted
    /// pack), so it cannot drift from production recall quality.
    ///
    /// `k` is the number of ranked units to return (e.g. 10 for `recall_any@10`). Candidates are
    /// sorted by `final_score(alpha)` descending; the tie-break beyond that is the harness scorer's
    /// (`recall_scorer.py` re-sorts `(score desc, unit_id asc)`), so this returns scored
    /// `(id, content, score)` triples and the harness owns the deterministic cut.
    pub fn recall_ranked(
        &self,
        query: &str,
        query_scope: &Scope,
        seeds: &[SymbolId],
        k: usize,
        now: i64,
        mode: RecallMode,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        let mut cands = self.ranked_candidates(query, query_scope, seeds, now, mode)?;
        // Final-rerank-score order, descending — NO budget_pack (no token cap, no Working eviction).
        cands.sort_by(|a, b| {
            b.final_score(self.alpha)
                .partial_cmp(&a.final_score(self.alpha))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(cands
            .into_iter()
            .take(k)
            .map(|c| {
                let score = c.final_score(self.alpha);
                Recalled {
                    id: c.id,
                    content: c.content,
                    tier: c.tier,
                    score,
                }
            })
            .collect())
    }

    /// The shared recall candidate-generation + rerank + scope-filter seam (the pre-assembly slice).
    /// Both the production token-budgeted [`Self::recall_impl`] and the un-budget-capped
    /// [`Self::recall_ranked`] call this, so the two paths share EXACTLY the same retrieval + rerank
    /// logic and differ only in their final assembly step. Returns reranked [`Candidate`]s in fused
    /// (RRF) order; the caller applies either `budget_pack` (production) or a top-`k` cut (ranked).
    fn ranked_candidates(
        &self,
        query: &str,
        query_scope: &Scope,
        seeds: &[SymbolId],
        now: i64,
        mode: RecallMode,
    ) -> wicked_estate_core::Result<Vec<Candidate>> {
        // 1. keyword (BM25/FTS) candidates restricted to memory nodes. estate's FTS phrase-matches
        //    the whole `text`, so a multi-word semantic query matches nothing — tokenize into terms
        //    and UNION per-term matches (OR semantics) so keyword contributes on non-phrase queries.
        let kw_ids: Vec<SymbolId> = if mode.uses_keyword() {
            self.keyword_candidates(query)?
        } else {
            Vec::new()
        };

        // 2. semantic (vector ANN) candidates — via the MemStore trait, filtered to live nodes
        //    (embeddings may outlive their node). (Was estate's `semantic_search`, which required a
        //    concrete `VectorStore`; the trait keeps recall backend-agnostic.)
        let sem_ids: Vec<SymbolId> = if mode.uses_vector() {
            let qvec = self.embedder.embed(query);
            let mut v = Vec::new();
            for (id, _) in self.store.nearest(&qvec, self.k)? {
                if self.store.get_node(&id)?.is_some() {
                    v.push(id);
                }
            }
            v
        } else {
            Vec::new()
        };

        // 3. graph candidates: memories `about` the seed code/infra symbols (the unique bet).
        let graph_ids = if mode.uses_graph() {
            self.about_seed_ids(seeds)?
        } else {
            Vec::new()
        };

        // 4. RRF fusion of keyword ∪ graph ∪ semantic.
        let fused = hybrid_search(kw_ids, graph_ids, sem_ids, 60.0);

        // 5. rerank + scope-filter (ancestor inheritance) into Candidates.
        let mut cands: Vec<Candidate> = Vec::new();
        for (id, rrf) in fused {
            let Some(node) = self.store.get_node(&id)? else {
                continue;
            };
            let Some(mem) = Memory::from_node(&node) else {
                continue;
            };
            // scope inheritance: keep memories at the query scope OR an ancestor of it.
            if !mem.scope.is_ancestor_of(query_scope) {
                continue;
            }
            let age = (now - mem.created_at).max(0);
            let recency = wicked_estate_memory_core::decay(age, self.sal.lambda_per_day);
            let sal = compute_salience(&self.sal, mem.confidence(), age, mem.access_count);
            if sal < self.recall_floor {
                continue; // forgotten-from-recall (FR-7)
            }
            cands.push(Candidate {
                id,
                content: mem.content,
                tier: mem.tier,
                rrf,
                recency,
                salience: sal,
            });
        }
        Ok(cands)
    }

    fn recall_impl(
        &self,
        query: &str,
        query_scope: &Scope,
        seeds: &[SymbolId],
        token_budget: usize,
        now: i64,
        mode: RecallMode,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        let cands = self.ranked_candidates(query, query_scope, seeds, now, mode)?;

        // token-budgeted assembly (the production path; recall_ranked bypasses this).
        let pack = budget_pack(cands, token_budget, self.alpha);
        Ok(pack
            .into_iter()
            .map(|c| {
                let score = c.final_score(self.alpha);
                Recalled {
                    id: c.id,
                    content: c.content,
                    tier: c.tier,
                    score,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_memory_core::{MemKind, Tier};

    #[test]
    fn with_backend_constructor_is_backend_agnostic() {
        // The M-PG path: construct the engine over an explicit `Box<dyn MemStore>` (a PostgresStore
        // impl plugs in here identically). Verified on the SQLite backend.
        use wicked_estate_store::SqliteStore;
        let store: Box<dyn MemStore> = Box::new(SqliteStore::in_memory().unwrap());
        let mut eng = MemoryEngine::with_backend(store, ":memory:").unwrap();
        let scope = Scope::root();
        eng.capture(&Memory::new(
            MemKind::Episode,
            Tier::Episodic,
            scope.clone(),
            "billing uses Stripe",
            1,
        ))
        .unwrap();
        let out = eng.recall("billing", &scope, &[], 500, 2).unwrap();
        assert!(out.iter().any(|r| r.content.contains("Stripe")));
    }

    #[test]
    fn capture_then_recall_returns_relevant_slice() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("org:acme/agent:claude");
        let now = 10_000;
        for (kind, tier, text) in [
            (
                MemKind::Fact,
                Tier::Semantic,
                "The user prefers oat milk in coffee",
            ),
            (
                MemKind::Fact,
                Tier::Semantic,
                "Billing runs through Stripe in production",
            ),
            (
                MemKind::Episode,
                Tier::Episodic,
                "Deployed the checkout service on Friday",
            ),
        ] {
            let mem = Memory::new(kind, tier, scope.clone(), text, now);
            eng.capture(&mem).unwrap();
        }
        let out = eng
            .recall("what does the user drink", &scope, &[], 1000, now)
            .unwrap();
        assert!(!out.is_empty(), "recall returned nothing");
        // The oat-milk fact should surface (keyword + semantic both favor it).
        assert!(
            out.iter().any(|r| r.content.contains("oat milk")),
            "expected the oat-milk memory in: {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn recall_respects_scope_isolation() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let now = 5;
        let acme = Scope::parse("org:acme/agent:a");
        let other = Scope::parse("org:other/agent:b");
        eng.capture(&Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            acme.clone(),
            "secret acme roadmap",
            now,
        ))
        .unwrap();
        eng.capture(&Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            other.clone(),
            "secret other roadmap",
            now,
        ))
        .unwrap();

        let out = eng.recall("secret roadmap", &acme, &[], 1000, now).unwrap();
        assert!(
            out.iter().any(|r| r.content.contains("acme")),
            "should see acme's own memory"
        );
        assert!(
            !out.iter().any(|r| r.content.contains("other")),
            "SCOPE ISOLATION VIOLATED: acme recall returned other-org memory: {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn recall_budget_is_respected() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let now = 1;
        for i in 0..20 {
            let mem = Memory::new(
                MemKind::Episode,
                Tier::Episodic,
                scope.clone(),
                format!("event number {i} about the system"),
                now,
            );
            eng.capture(&mem).unwrap();
        }
        let budget = 10;
        let out = eng
            .recall("system event", &scope, &[], budget, now)
            .unwrap();
        let tokens: usize = out.iter().map(|r| (r.content.len() / 4).max(1)).sum();
        assert!(tokens <= budget, "budget {budget} exceeded: {tokens}");
    }

    #[test]
    fn recall_ranked_bypasses_budget_pack_truncation() {
        // T-Y-RANKED falsifier (design-track-baseline-v2 §3.4 Y-BLOCK-1): recall_ranked(k) must
        // return the top-k by final_score WITHOUT the budget_pack token cap. The trap it defeats:
        // the production budget-based recall returns only 0–2 units at a tight budget, so recall@10
        // is capped near recall@1. Here, with a TIGHT budget that starves budget_pack, recall_ranked
        // must still surface up to k units — else recall@k is rigged.
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let now = 1;
        // 12 memories, each long enough that budget_pack at a tiny budget fits only ~1.
        for i in 0..12 {
            let mem = Memory::new(
                MemKind::Episode,
                Tier::Episodic,
                scope.clone(),
                format!(
                    "system event number {i}: a fairly long description of the system event so its \
                     token_cost is non-trivial and the tiny token budget cannot fit many of them"
                ),
                now,
            );
            eng.capture(&mem).unwrap();
        }
        let query = "system event"; // matches all 12 (keyword OR-term + vector)

        // Production recall at a TIGHT budget: budget_pack caps the count hard.
        let tiny_budget = 20; // ~ one unit's token_cost
        let budgeted = eng.recall(query, &scope, &[], tiny_budget, now).unwrap();

        // recall_ranked at k=10 with the SAME query: NO budget cap → up to 10 units.
        let ranked = eng
            .recall_ranked(query, &scope, &[], 10, now, RecallMode::Hybrid)
            .unwrap();

        // The discriminating assertion: ranked returns strictly more than the budget-capped pack,
        // and reaches the requested k (10). A regression that routed recall_ranked through
        // budget_pack would return the same starved count as `budgeted`.
        assert!(
            ranked.len() > budgeted.len(),
            "recall_ranked must bypass the budget cap: ranked={} budgeted={}",
            ranked.len(),
            budgeted.len()
        );
        assert_eq!(
            ranked.len(),
            10,
            "recall_ranked(k=10) over 12 matching memories must return 10, got {}",
            ranked.len()
        );
        // Ranked output is in descending final_score order (no budget reshuffle).
        for w in ranked.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "recall_ranked must be sorted by final_score descending"
            );
        }
    }

    #[test]
    fn recall_ranked_shares_retrieval_with_production_recall() {
        // recall_ranked and production recall share the SAME candidate seam (ranked_candidates), so
        // at a budget large enough that budget_pack truncates nothing, the two return the same id
        // SET (only the assembly differs). This pins the no-drift property: recall_ranked is not a
        // second, divergent retrieval path.
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let now = 1;
        for (kind, tier, text) in [
            (MemKind::Fact, Tier::Semantic, "billing uses Stripe"),
            (
                MemKind::Fact,
                Tier::Semantic,
                "checkout persists to Postgres",
            ),
            (MemKind::Episode, Tier::Episodic, "deployed on Friday"),
        ] {
            eng.capture(&Memory::new(kind, tier, scope.clone(), text, now))
                .unwrap();
        }
        let big_budget = 100_000; // no truncation
        let prod = eng
            .recall("billing checkout", &scope, &[], big_budget, now)
            .unwrap();
        let ranked = eng
            .recall_ranked(
                "billing checkout",
                &scope,
                &[],
                100,
                now,
                RecallMode::Hybrid,
            )
            .unwrap();
        let prod_ids: std::collections::BTreeSet<String> =
            prod.iter().map(|r| r.id.0.clone()).collect();
        let ranked_ids: std::collections::BTreeSet<String> =
            ranked.iter().map(|r| r.id.0.clone()).collect();
        assert_eq!(
            prod_ids, ranked_ids,
            "at an untruncated budget, recall_ranked and production recall must surface the same id set"
        );
    }

    #[test]
    fn cross_edge_lifts_recall_the_unique_bet() {
        // The differentiator: a memory ABOUT a code symbol is recalled from the code seed even when
        // its text does NOT lexically/semantically match the query. With vs without the `about`
        // edge demonstrates the lift the L1 benchmark gate must prove at scale.
        use wicked_estate_core::Symbol;
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let now = 1;
        // A real code symbol (as estate would index it).
        let checkout = Symbol::synthetic("code", "checkout_service").id();

        // Memory whose CONTENT shares no words with the query, but is about `checkout`.
        let mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "always pass an idempotency token to avoid double charges",
            now,
        );
        eng.capture_about(&mem, std::slice::from_ref(&checkout))
            .unwrap();

        let query = "what should I know about this service"; // no lexical overlap with the memory

        // WITHOUT the seed: keyword+vector likely miss it.
        let without = eng.recall(query, &scope, &[], 1000, now).unwrap();
        // WITH the code seed: the `about` cross-edge surfaces it.
        let with = eng
            .recall(query, &scope, std::slice::from_ref(&checkout), 1000, now)
            .unwrap();

        let hit = |v: &[Recalled]| v.iter().any(|r| r.content.contains("idempotency"));
        assert!(
            hit(&with),
            "cross-edge recall must surface the about-memory"
        );
        assert!(
            !hit(&without) || with.len() >= without.len(),
            "the about-edge should add recall, not remove it (lift)"
        );
    }
}
