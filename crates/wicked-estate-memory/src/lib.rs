//! `wicked-memory` — the engine: capture + conversational recall over a `wicked-estate` store.
//!
//! L0b vertical slice (thesis-first, `02-STEPBACK.md`): capture writes a memory `Node` + its
//! embedding into a `SqliteStore`; recall fuses estate FTS (BM25) + vector ANN via RRF, reranks
//! with the L0a memory math (tier × recency × salience), filters by scope (inheritance), and
//! assembles a token-budgeted pack. Estate is a **library** here.
//!
//! Scope note (L3 done): `wicked-estate-core` now has a first-class `Scope` primitive +
//! `SymbolQuery.scope_prefix` (subtree predicate, conformance-tested isolation). Memory recall
//! defaults to its own **ancestor** filter here because inheritance recall (see broader/
//! root-scoped memories) is the opposite direction from estate's subtree predicate; both are
//! valid, different uses. An optional `scope_prefix` on recall flips to the subtree-inclusive
//! predicate (`path_in_prefix`, the one erase/coverage use) so descendant-scoped memories —
//! e.g. imported leaf scopes like `brain:wicked-garden/doc:<id>` — are recallable from above.

// (estate#333, wicked-core#957) A diagnostic line never kills the thread that writes it. std's
// `eprintln!` PANICS when the write fails, and a Node host (wicked-crew loading wicked-core through
// its napi addon, which links this crate) hands the process a stderr that is a non-blocking pipe:
// when the reader lags, the write fails with EAGAIN and the thread dies with no trace. This
// crate-wide shadow (textual scope: every module declared below) writes the same line and drops it
// when stderr refuses it. Unit tests keep std's macro, whose output the test harness captures.
// Its only diagnostics are the embedder fallbacks, so the shadow exists exactly when those calls do.
#[cfg(all(not(test), any(feature = "semantic", feature = "semantic-bge")))]
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stderr(), $($arg)*);
    }};
}

use wicked_estate_core::{
    Direction, Edge, EdgeKind, Node, NodeKind, ResolutionTier, SymbolId, SymbolQuery,
};
use wicked_estate_memory_core::{
    Candidate, Facets, MEMORY_NODE_KIND, Memory, RETIRED_MEMORY_NODE_KIND, Salience, Scope,
    budget_pack, facet_admits, memory_lock, meta_keys, salience as compute_salience,
};
use wicked_estate_overlay::XedgeStore;
use wicked_estate_retrieve::{Embedder, HashEmbedder, hybrid_search};
use wicked_estate_store::SqliteStore;

mod api;
mod consolidate;
/// Cross-store recall (the differentiator) — feature `cross`. See [`cross::OverlayMemStore`].
#[cfg(feature = "cross")]
pub mod cross;
mod memext;
mod packs;
/// Proposal-queue operations on the engine (DES-MEM-FACETED-001 §5.0).
mod proposal;
mod store;
pub use consolidate::ConsolidationReport;
use memext::MemExt;
pub use packs::{PackApplyReport, PackMemory, PackRef, apply_pack_memories, pack_memories};
pub use store::MemStore;

/// The `codebase-expedition` skill (SKILL.md), owned by this crate and embedded at compile time so
/// it travels with the binary. `CARGO_MANIFEST_DIR` resolves both in-workspace and from this
/// crate's own published tarball (the `skills/` dir ships — no `include`/`exclude` in Cargo.toml).
/// Re-exported so consumers (e.g. `wicked-estate-mcp`) reference it instead of copying the file.
pub const CODEBASE_EXPEDITION_SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/codebase-expedition/SKILL.md"
));

/// Recall result item surfaced to the caller.
#[derive(Debug, Clone)]
pub struct Recalled {
    pub id: SymbolId,
    pub content: String,
    pub tier: wicked_estate_memory_core::Tier,
    pub score: f64,
    /// The memory node's own hierarchical scope (S4 attribution). Recovered from the store at
    /// recall time. Empty string when the node could not be found or re-hydrated in the store,
    /// or when `Memory::from_node` returns `None` (node exists but is not a memory node).
    pub scope: String,
    /// What the memory is (`fact`, `skill`, `episode`, …), recovered with `scope`. `None` when the
    /// node could not be re-hydrated.
    pub kind: Option<wicked_estate_memory_core::MemKind>,
    /// When the memory was captured (unix seconds); 0 when the node could not be re-hydrated.
    pub created_at: i64,
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

/// Minimum cosine similarity for vector recall candidates, before RRF fusion. Tuned for
/// bge-small-en-v1.5, where unrelated text scores about 0.4–0.5. Lexical and graph-seed matches
/// are admitted independently of this semantic floor.
pub const SEMANTIC_MIN_SIMILARITY: f32 = 0.55;

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

/// The scope-visibility filter a recall applies — one of the two directions over the scope tree.
/// The variants are mutually exclusive BY CONSTRUCTION: a `scope_prefix` on the wire REPLACES the
/// inheritance rule rather than fusing with it. Replace (not fuse) for two reasons: (1) it keeps
/// `scope_prefix` meaning EXACTLY what it means on `memory.erase`/`memory.coverage` — a recall
/// with a prefix previews precisely the set an erase with that prefix would delete; (2) fusing
/// would make subtree-only recall inexpressible — root-scoped memories are ancestor-visible from
/// EVERY query scope, so a fused filter could never exclude them, while the fused view stays
/// expressible under replace (`Subtree("")` is a superset of any fusion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeFilter<'a> {
    /// Inheritance (the default): memories at the query scope or an ANCESTOR of it — a
    /// leaf-scoped query also sees broader/root-scoped memories — plus every OPERATOR memory
    /// (`operator:<id>`, how the operator works) and locked one-segment org/team memories.
    /// Facets still gate every admitted memory.
    Ancestors(&'a Scope),
    /// Subtree-inclusive (the `memory.erase`/`memory.coverage` direction, `path_in_prefix`):
    /// memories whose scope equals the prefix or DESCENDS from it (`""` = the root subtree =
    /// every memory). This is how descendant-scoped memories — e.g. imported leaf scopes like
    /// `brain:wicked-garden/doc:<id>` — are recalled from above.
    Subtree(&'a str),
}

impl ScopeFilter<'_> {
    /// Does a memory whose own scope is `mem_scope` pass this filter?
    ///
    /// NOTE: `mem_scope` is the memory-domain scope (decoded from the node's `metadata.scope`
    /// JSON via [`Memory::from_node`]), NOT the store's `nodes.scope` column — that column is the
    /// graph-domain predicate and stays `''` for memory nodes.
    fn admits(&self, mem_scope: &Scope, node: &Node) -> bool {
        match self {
            // An operator memory (`operator:<id>`) is about the person, not a project: every
            // scoped recall inherits it, as it inherits the root.
            ScopeFilter::Ancestors(query_scope) => {
                mem_scope.is_ancestor_of(query_scope)
                    || mem_scope.is_operator()
                    || (mem_scope.is_memory_layer() && memory_lock(node).is_some())
            }
            // Allocation-free segment walk (this runs per candidate in the rerank loop) —
            // exact `path_in_prefix` semantics for parse-normalized scopes, see Scope::path_in_prefix.
            ScopeFilter::Subtree(prefix) => mem_scope.path_in_prefix(prefix),
        }
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
        let store = SqliteStore::open(path)?;
        use wicked_estate_core::redact::{TEXT_REDACTION_EPOCH, TEXT_REDACTION_EPOCH_KEY};
        let epoch = store
            .meta_get(TEXT_REDACTION_EPOCH_KEY)?
            .and_then(|v| v.parse::<u32>().ok());
        if !epoch.is_some_and(|e| e >= TEXT_REDACTION_EPOCH)
            && wicked_estate_core::GraphRead::stats(&store)?.node_count > 0
        {
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr(),
                "REDACTION-PENDING: {path} holds text written before redaction; run \
                 `wicked-estate redact-store --db {path}`"
            );
        }
        let eng = Self::build(Box::new(store), MemExt::open(path)?);
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

    /// TRUNCATE-checkpoint the memory store's WAL — a one-line forwarder to the backend
    /// (`SqliteStore::checkpoint_truncate`: busy-tolerant, never blocking; a `busy` result just
    /// defers to a later call). Non-WAL backends (Postgres) return the `-1` no-WAL sentinel
    /// stats. The `.memext`
    /// sidecar needs no checkpoint: it is opened in SQLite's default rollback-journal mode.
    pub fn checkpoint_truncate(
        &mut self,
    ) -> wicked_estate_core::Result<wicked_estate_store::WalCheckpointStats> {
        self.store.checkpoint_truncate()
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
    /// ids and locked nodes are skipped: these candidates are for maintenance only.
    pub(crate) fn hydrate(&self, ids: &[String]) -> wicked_estate_core::Result<Vec<Memory>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(node) = self.store.get_node(&SymbolId(id.clone()))? {
                if memory_lock(&node).is_some() {
                    continue;
                }
                if let Some(m) = Memory::from_node(&node) {
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    /// All active memories (decoded). Full scan — fine at local-first scale;
    /// the indexed `memory_node_ext` projection (DESIGN §4) optimizes this when it lands.
    pub fn all_memories(&self) -> wicked_estate_core::Result<Vec<Memory>> {
        let nodes = self.memory_nodes(MEMORY_NODE_KIND)?;
        Ok(nodes.iter().filter_map(Memory::from_node).collect())
    }

    fn memory_nodes(&self, kind: &str) -> wicked_estate_core::Result<Vec<Node>> {
        self.store.find_symbols(&SymbolQuery {
            text: None,
            exact_name: None,
            kinds: vec![NodeKind::Other(kind.into())],
            language: None,
            limit: None,
            scope_prefix: None,
        })
    }

    // Only lifecycle management decodes retired nodes. Keep Memory::from_node active-only so
    // every retrieval and consolidation hydration shares the same exclusion gate. An undecodable
    // retired node is skipped (`None`), exactly as `all_memories` skips an undecodable active one,
    // so one bad row never blocks the retired list, a restore, or an erase.
    fn retired_memory(node: &Node) -> Option<(Memory, i64)> {
        if node.kind != NodeKind::Other(RETIRED_MEMORY_NODE_KIND.into()) {
            return None;
        }
        let retired_at = node
            .metadata
            .get(meta_keys::RETIRED_AT)
            .and_then(|v| v.as_i64())?;
        let mut active = node.clone();
        active.kind = NodeKind::Other(MEMORY_NODE_KIND.into());
        let memory = Memory::from_node(&active)?;
        Some((memory, retired_at))
    }

    /// Soft-retire one active memory by id or SymbolId string. Retains embeddings and edges.
    /// Returns None for an unknown or already-retired memory.
    pub fn retire(&mut self, id: &str, now: i64) -> wicked_estate_core::Result<Option<SymbolId>> {
        if id.is_empty() {
            return Err(wicked_estate_core::Error::Invalid(
                "retire: id must not be empty".into(),
            ));
        }
        for mut node in self.memory_nodes(MEMORY_NODE_KIND)? {
            let Some(memory) = Memory::from_node(&node) else {
                continue;
            };
            if memory.id != id && node.symbol.0 != id {
                continue;
            }
            Self::refuse_locked(&node)?;
            node.kind = NodeKind::Other(RETIRED_MEMORY_NODE_KIND.into());
            node.metadata
                .insert(meta_keys::RETIRED_AT.into(), now.into());
            let sym = node.symbol.clone();
            self.store.upsert_nodes(&[node])?;
            self.ext.remove(std::slice::from_ref(&sym.0))?;
            return Ok(Some(sym));
        }
        Ok(None)
    }

    /// Restore one retired memory by id or SymbolId string, preserving its original identity.
    pub fn unretire(&mut self, id: &str) -> wicked_estate_core::Result<Option<SymbolId>> {
        if id.is_empty() {
            return Err(wicked_estate_core::Error::Invalid(
                "unretire: id must not be empty".into(),
            ));
        }
        for mut node in self.memory_nodes(RETIRED_MEMORY_NODE_KIND)? {
            let Some((memory, _)) = Self::retired_memory(&node) else {
                continue;
            };
            if memory.id != id && node.symbol.0 != id {
                continue;
            }
            Self::refuse_locked(&node)?;
            node.kind = NodeKind::Other(MEMORY_NODE_KIND.into());
            node.metadata.remove(meta_keys::RETIRED_AT);
            let sym = node.symbol.clone();
            self.store.upsert_nodes(&[node])?;
            self.ext.upsert(&memory)?;
            return Ok(Some(sym));
        }
        Ok(None)
    }

    /// Management inventory of retired memories; never used as retrieval candidates.
    pub fn retired(
        &self,
        scope_prefix: Option<&str>,
    ) -> wicked_estate_core::Result<Vec<(Memory, i64)>> {
        let mut retired = Vec::new();
        for node in self.memory_nodes(RETIRED_MEMORY_NODE_KIND)? {
            let Some((memory, retired_at)) = Self::retired_memory(&node) else {
                continue;
            };
            if scope_prefix.is_none_or(|prefix| memory.scope.path_in_prefix(prefix)) {
                retired.push((memory, retired_at));
            }
        }
        Ok(retired)
    }

    fn erasure_memories(&self) -> wicked_estate_core::Result<Vec<Memory>> {
        let mut memories = self.all_memories()?;
        memories.extend(self.retired(None)?.into_iter().map(|(memory, _)| memory));
        Ok(memories)
    }

    /// Embed text with the engine's embedder (used by consolidation).
    pub(crate) fn embed_for(&self, text: &str) -> Vec<f32> {
        self.embedder
            .embed(&wicked_estate_core::redact::redact_free_text(text))
    }

    /// Fetch a node by id.
    pub fn node(
        &self,
        id: &SymbolId,
    ) -> wicked_estate_core::Result<Option<wicked_estate_core::Node>> {
        Ok(self
            .store
            .get_node(id)?
            .map(|node| wicked_estate_core::redact::redact_text_node(&node).unwrap_or(node)))
    }

    /// Right-to-erasure (FR-15/AC-10): hard-delete every memory whose scope is within `scope_prefix`,
    /// from the graph + FTS + vector + the sidecar index. Returns the count erased. **Memory-kind
    /// guarded:** only active and retired memory nodes are considered, so code/infra
    /// nodes sharing a scope are never touched. Idempotent.
    pub fn erase(&mut self, scope_prefix: &str) -> wicked_estate_core::Result<usize> {
        let mut locked = Vec::new();
        for kind in [MEMORY_NODE_KIND, RETIRED_MEMORY_NODE_KIND] {
            for node in self.memory_nodes(kind)? {
                let path = node.metadata.get(meta_keys::SCOPE).and_then(|v| v.as_str());
                if path.is_some_and(|path| {
                    wicked_estate_core::scope::path_in_prefix(path, scope_prefix)
                }) {
                    if let Err(error) = Self::refuse_locked(&node) {
                        locked.push(error.to_string());
                    }
                }
            }
        }
        if !locked.is_empty() {
            locked.sort();
            return Err(wicked_estate_core::Error::Invalid(locked.join("; ")));
        }
        let victims: Vec<Memory> = self
            .erasure_memories()?
            .into_iter()
            .filter(|m| m.scope.path_in_prefix(scope_prefix))
            .collect();
        let ids: Vec<SymbolId> = victims.iter().map(|m| m.symbol()).collect();
        let id_strs: Vec<String> = ids.iter().map(|s| s.0.clone()).collect();
        self.store.remove_nodes(&ids)?;
        self.ext.remove(&id_strs)?;
        Ok(ids.len())
    }

    /// Hard-delete the ONE memory whose id or `SymbolId` string equals `id`, from the graph, FTS,
    /// vector and sidecar index, leaving every other memory in its scope alone. Memory-kind
    /// guarded like [`Self::erase`]: a code/infra node id never matches. Returns the erased
    /// memory's `SymbolId`, or `None` when no memory has that id.
    pub fn erase_id(&mut self, id: &str) -> wicked_estate_core::Result<Option<SymbolId>> {
        for symbol in [
            SymbolId(id.into()),
            wicked_estate_core::Symbol::synthetic("mem", id).id(),
        ] {
            if let Some(node) = self.store.get_node(&symbol)? {
                if matches!(&node.kind, NodeKind::Other(k)
                    if k == MEMORY_NODE_KIND || k == RETIRED_MEMORY_NODE_KIND)
                {
                    Self::refuse_locked(&node)?;
                }
            }
        }
        let Some(victim) = self
            .erasure_memories()?
            .into_iter()
            .find(|m| m.id == id || m.symbol().0 == id)
        else {
            return Ok(None);
        };
        let sym = victim.symbol();
        self.store.remove_nodes(std::slice::from_ref(&sym))?;
        self.ext.remove(std::slice::from_ref(&sym.0))?;
        Ok(Some(sym))
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

    fn refuse_locked(node: &Node) -> wicked_estate_core::Result<()> {
        if let Some(lock) = memory_lock(node) {
            let id = node
                .symbol
                .as_str()
                .rsplit(' ')
                .next()
                .unwrap_or("")
                .trim_end_matches(':');
            return Err(wicked_estate_core::Error::Invalid(format!(
                "memory_locked: {id} {}@{}",
                lock.pack, lock.version
            )));
        }
        Ok(())
    }

    /// The shared ordinary-write gate: node ownership first, then reserved layer placement.
    fn check_lock(&self, mem: &Memory) -> wicked_estate_core::Result<()> {
        if let Some(node) = self.store.get_node(&mem.symbol())? {
            Self::refuse_locked(&node)?;
        }
        if mem.scope.is_memory_layer() {
            return Err(wicked_estate_core::Error::Invalid(format!(
                "memory_locked: {} is an org/team layer scope; only a verified pack writes there",
                mem.scope.as_path()
            )));
        }
        Ok(())
    }

    fn is_locked(&self, id: &SymbolId) -> wicked_estate_core::Result<bool> {
        Ok(self
            .store
            .get_node(id)?
            .is_some_and(|node| memory_lock(&node).is_some()))
    }

    /// Capture a memory: cheap, synchronous, immediately recallable (node + embedding).
    pub fn capture(&mut self, mem: &Memory) -> wicked_estate_core::Result<()> {
        self.check_lock(mem)?;
        let node = mem.to_node();
        let sym = mem.symbol();
        self.store.upsert_nodes(&[node])?;
        let text = wicked_estate_core::redact::redact_free_text(&mem.content);
        let vec = self.embedder.embed(&text);
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

    /// One BM25-ordered FTS candidate list over memory nodes. `text` is phrase-quoted by the
    /// store, so a multi-token string requires ADJACENT tokens.
    fn fts_list(&self, text: &str) -> wicked_estate_core::Result<Vec<SymbolId>> {
        Ok(self
            .store
            .find_symbols(&SymbolQuery {
                text: Some(text.to_string()),
                exact_name: None,
                kinds: vec![NodeKind::Other("memory".into())],
                language: None,
                limit: Some(self.k),
                scope_prefix: None,
            })?
            .iter()
            .map(|n| n.symbol.clone())
            .collect())
    }

    /// Keyword candidates: whole-query PHRASE list ∪ per-term OR lists, RRF-fused.
    ///
    /// The phrase list is the identifier-shaped-query (symbolish) fix: FTS5's default unicode61
    /// tokenizer splits code identifiers on `_`/`.`/`::` at index time, so the raw query
    /// phrase-matches those sub-tokens ADJACENTLY (`prompt_submit.py` → `[prompt, submit, py]` in
    /// order) and the memory containing the literal identifier outranks memories that merely
    /// scatter its unigrams (S3 parity bench, symbolish class). Skipped when the query is already
    /// a single bare term (identical list, no signal). Mirrors knowledge's `keyword_candidates`.
    fn keyword_candidates(&self, query: &str) -> wicked_estate_core::Result<Vec<SymbolId>> {
        const FUNCTION_WORDS: &[&str] = &[
            "about", "all", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can",
            "could", "did", "do", "does", "for", "from", "had", "has", "have", "how", "if", "in",
            "into", "is", "it", "its", "of", "on", "or", "our", "should", "that", "the", "their",
            "them", "there", "these", "they", "this", "to", "was", "we", "were", "what", "when",
            "which", "who", "will", "with", "would", "you", "your",
        ];
        // Significant terms (len ≥ 2 keeps "AI"/"DB"/"ML") exclude function words so prose
        // cannot accumulate relevance from them; fall back to the cleaned whole query.
        let mut terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 2)
            .map(|w| w.to_lowercase())
            .filter(|w| !FUNCTION_WORDS.contains(&w.as_str()))
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
        let phrase = query.trim();
        let single_bare_term = terms.len() == 1 && terms[0] == phrase.to_lowercase();
        if !phrase.is_empty() && !single_bare_term {
            let l = self.fts_list(phrase)?;
            if !l.is_empty() {
                lists.push(l);
            }
        }
        for term in terms {
            let l = self.fts_list(&term)?;
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
    ///
    /// `scope` picks the visibility DIRECTION — see [`ScopeFilter`]: `Ancestors` is the default
    /// inheritance predicate (memories at the query scope or an ancestor of it); `Subtree` is the
    /// subtree-inclusive `path_in_prefix` predicate `erase`/`coverage` already use (memories whose
    /// scope equals the prefix or descends from it; `Subtree("")` = everything).
    pub fn recall(
        &self,
        query: &str,
        scope: ScopeFilter<'_>,
        seeds: &[SymbolId],
        token_budget: usize,
        now: i64,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        let cands = self.ranked_candidates(
            query,
            scope,
            seeds,
            now,
            RecallMode::Hybrid,
            &Facets::default(),
        )?;
        Ok(self.pack_recalled(cands, token_budget))
    }

    /// Single-retriever recall for benchmarking the hybrid uplift (PR-14). Not the production path.
    pub fn recall_mode(
        &self,
        query: &str,
        scope: ScopeFilter<'_>,
        seeds: &[SymbolId],
        token_budget: usize,
        now: i64,
        mode: RecallMode,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        let cands = self.ranked_candidates(query, scope, seeds, now, mode, &Facets::default())?;
        Ok(self.pack_recalled(cands, token_budget))
    }

    /// **Un-budget-capped ranked recall (T-Y-RANKED).** Return the top-`k` recall candidates in
    /// recall order **WITHOUT** the [`budget_pack`] token-budget assembly — no token
    /// budget, no ≥1-Working-tier eviction. This is the fair `recall@k` comparand for the
    /// memory-≥-brain head-to-head (`design-track-baseline-v2.md` §3.4 Y-BLOCK-1, §7.1 "`recall_ranked`
    /// (no budget, no Working-eviction) frozen"): the production [`recall`]/[`recall_mode`] path runs
    /// `budget_pack`, which returns only 0–2 units at a small token budget and so caps `recall@10`
    /// near `recall@1` (the rigged-gate trap, `BUILD-SPEC.md` §7). `recall_ranked` shares the
    /// **identical** candidate-generation + rerank seam ([`Self::ranked_candidates`]) the production
    /// recall uses — it differs ONLY in the final assembly (top-`k` by recall order vs token-budgeted
    /// pack), so it cannot drift from production recall quality.
    ///
    /// `k` is the number of ranked units to return (e.g. 10 for `recall_any@10`). Candidates are
    /// sorted by [`Candidate::rank_cmp`]: Working follows every non-Working candidate, then
    /// `final_score(alpha)` descends within those groups. Returned scores retain their numeric
    /// meaning; consumers must preserve this order to retain the Working-tier priority.
    pub fn recall_ranked(
        &self,
        query: &str,
        scope: ScopeFilter<'_>,
        seeds: &[SymbolId],
        k: usize,
        now: i64,
        mode: RecallMode,
    ) -> wicked_estate_core::Result<Vec<Recalled>> {
        let mut cands =
            self.ranked_candidates(query, scope, seeds, now, mode, &Facets::default())?;
        // Shared recall order, with no token cap or Working eviction.
        cands.sort_by(|a, b| a.rank_cmp(b, self.alpha));
        Ok(cands
            .into_iter()
            .take(k)
            .map(|c| {
                let score = c.final_score(self.alpha);
                let mem = self
                    .store
                    .get_node(&c.id)
                    .ok()
                    .flatten()
                    .and_then(|n| Memory::from_node(&n));
                let scope = mem
                    .as_ref()
                    .map(|m| m.scope.as_path().to_string())
                    .unwrap_or_default();
                let (kind, created_at) = mem.map_or((None, 0), |m| (Some(m.kind), m.created_at));
                Recalled {
                    id: c.id,
                    content: c.content,
                    tier: c.tier,
                    score,
                    scope,
                    kind,
                    created_at,
                }
            })
            .collect())
    }

    /// The shared recall candidate-generation + rerank + scope/facet-filter seam (the pre-assembly
    /// slice). Both the production token-budgeted paths ([`Self::recall`]/[`Self::recall_mode`] and
    /// the `MemoryApi::recall` intent seam, which finish via [`Self::pack_recalled`]) and the
    /// un-budget-capped [`Self::recall_ranked`] call this, so every path shares EXACTLY the same
    /// retrieval + rerank logic and differs only in its final assembly step. Returns reranked
    /// [`Candidate`]s in fused (RRF) order; the caller applies either `budget_pack` (production) or
    /// a top-`k` cut (ranked). `intent` AND-composes the facet predicate at the scope gate.
    fn ranked_candidates(
        &self,
        query: &str,
        scope: ScopeFilter<'_>,
        seeds: &[SymbolId],
        now: i64,
        mode: RecallMode,
        intent: &Facets,
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
        //    Gated on the embedder carrying real semantic signal: the HashEmbedder fallback is a
        //    lexical hash, and fusing its neighbours as a peer retriever injects rank noise that
        //    degrades every query class (S3 parity bench, keyword-only vs hash-fused). An EXPLICIT
        //    `VectorOnly` request enables this leg even for a hash embedder; the cosine floor
        //    still applies, before RRF discards the raw similarity.
        let sem_ids: Vec<SymbolId> = if mode.uses_vector()
            && (self.embedder.is_semantic() || matches!(mode, RecallMode::VectorOnly))
        {
            let qvec = self.embedder.embed(query);
            let mut v = Vec::new();
            for (id, cosine) in self.store.nearest(&qvec, self.k)? {
                if cosine >= SEMANTIC_MIN_SIMILARITY && self.store.get_node(&id)?.is_some() {
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

        // 5. rerank + scope-filter (the caller's ScopeFilter direction) into Candidates.
        let mut cands: Vec<Candidate> = Vec::new();
        for (id, rrf) in fused {
            let Some(node) = self.store.get_node(&id)? else {
                continue;
            };
            let Some(mem) = Memory::from_node(&node) else {
                continue;
            };
            // Scope visibility — the direction is the caller's [`ScopeFilter`] choice:
            // `Ancestors` (inheritance, the default) or `Subtree` (the erase/coverage
            // `path_in_prefix` predicate). See `ScopeFilter::admits` for the domain note
            // (memory scope lives in `metadata.scope`, not the `nodes.scope` column).
            if !scope.admits(&mem.scope, &node) {
                continue;
            }
            // Facet visibility AND-composed with scope at the SAME gate (DES-MEM-FACETED-001 §4.3):
            // a candidate passes iff `facet_admits(mem.facets, intent)` is `Some`. `None` EXCLUDES a
            // memory whose facet constrains an axis the intent does not satisfy (no cross-user /
            // cross-repo leakage). The `Some(specificity)` becomes the score boost below. A memory
            // with EMPTY facets ⇒ `Some(0)` ⇒ admitted with no boost regardless of intent (so legacy
            // unfaceted nodes are unaffected); a FACETED memory under an empty intent is EXCLUDED.
            let Some(facet_specificity) = facet_admits(&mem.facets, intent) else {
                continue;
            };
            let age = (now - mem.created_at).max(0);
            if (mem.tier == wicked_estate_memory_core::Tier::Working
                || mem.kind == wicked_estate_memory_core::MemKind::Working)
                && age >= wicked_estate_memory_core::WORKING_TTL_SECS
            {
                continue; // expired session scratch (T0 TTL)
            }
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
                facet_specificity,
            });
        }
        Ok(cands)
    }

    /// Production assembly tail shared by every token-budgeted recall path ([`Self::recall`],
    /// [`Self::recall_mode`], and the `MemoryApi::recall` intent seam): token-budget the reranked
    /// candidates via [`budget_pack`] and map them to [`Recalled`], recovering each item's own
    /// scope for S4 attribution. (`recall_ranked` bypasses this — it does a top-`k` cut instead.)
    pub(crate) fn pack_recalled(
        &self,
        cands: Vec<Candidate>,
        token_budget: usize,
    ) -> Vec<Recalled> {
        budget_pack(cands, token_budget, self.alpha)
            .into_iter()
            .map(|c| {
                let score = c.final_score(self.alpha);
                // Recover the item's own scope for S4 attribution. The node was already
                // hydrated in ranked_candidates; a second get_node is a cheap pk lookup.
                let mem = self
                    .store
                    .get_node(&c.id)
                    .ok()
                    .flatten()
                    .and_then(|n| Memory::from_node(&n));
                let scope = mem
                    .as_ref()
                    .map(|m| m.scope.as_path().to_string())
                    .unwrap_or_default();
                let (kind, created_at) = mem.map_or((None, 0), |m| (Some(m.kind), m.created_at));
                Recalled {
                    id: c.id,
                    content: c.content,
                    tier: c.tier,
                    score,
                    scope,
                    kind,
                    created_at,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_memory_core::{MemKind, Tier};

    #[test]
    fn locked_nodes_survive_every_maintenance_rewrite_byte_identically() {
        use wicked_estate_memory_core::{MemoryApi, WORKING_TTL_SECS};
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:locks");
        let mut originals = Vec::new();
        for (kind, tier) in [
            (MemKind::Episode, Tier::Episodic),
            (MemKind::Working, Tier::Working),
            (MemKind::Fact, Tier::Semantic),
            (MemKind::Entity, Tier::Semantic),
            (MemKind::Archive, Tier::Archival),
        ] {
            let mut mem = Memory::new(kind, tier, scope.clone(), "Alice prefers quartz.", 1);
            mem.reinforce_pos = 10;
            mem.reinforce_total = 10;
            let mut node = mem.to_node();
            node.metadata.insert(
                "lock".into(),
                serde_json::json!({"pack": "fixture", "layer": "org", "version": "v1"}),
            );
            // A stale summary would normally be removed by the expiry repair pass.
            if kind == MemKind::Archive {
                node.metadata
                    .insert("source_ids".into(), serde_json::json!(["missing"]));
            }
            eng.store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
            eng.ext.upsert(&mem).unwrap();
            originals.push(node);
        }
        let now = 100 * WORKING_TTL_SECS;
        assert!(eng.recall_episodic_batch(&scope).unwrap().is_empty());
        let cluster = eng.all_memories().unwrap();
        for memory in &cluster {
            assert_eq!(eng.capture_facts(memory, None, now).unwrap(), 0);
        }
        assert_eq!(eng.archive_cluster(&scope, &cluster, None, now).unwrap(), 0);
        assert_eq!(eng.promote_skills(&scope, 1, 0.0, now).unwrap(), 0);
        assert_eq!(eng.archive(&scope, 0, 1.0, now).unwrap(), 0);
        MemoryApi::reflect(&mut eng, &scope.as_path(), now).unwrap();
        for node in &originals {
            eng.reinforce(&node.symbol, true, now).unwrap();
            assert!(
                eng.retire(&node.symbol.0, now)
                    .unwrap_err()
                    .to_string()
                    .contains("memory_locked")
            );
            assert_eq!(
                serde_json::to_value(eng.node(&node.symbol).unwrap()).unwrap(),
                serde_json::to_value(Some(node)).unwrap()
            );
        }
        assert_eq!(eng.count().unwrap(), originals.len());
    }

    #[test]
    fn reflect_skips_a_pre_lock_memory_in_a_layer_scope_instead_of_failing() {
        use wicked_estate_memory_core::MemoryApi;
        let mut eng = MemoryEngine::in_memory().unwrap();
        // Written before locks existed: an unlocked episode at a one-segment org scope.
        let legacy = Memory::new(
            MemKind::Episode,
            Tier::Episodic,
            Scope::parse("org:acme"),
            "Alice prefers quartz.",
            1,
        );
        let node = legacy.to_node();
        eng.store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
        eng.ext.upsert(&legacy).unwrap();
        assert_eq!(eng.capture_facts(&legacy, None, 2).unwrap(), 0);
        assert_eq!(
            eng.archive_cluster(&Scope::root(), std::slice::from_ref(&legacy), None, 2)
                .unwrap(),
            0
        );
        MemoryApi::reflect(&mut eng, "", 2).unwrap();
        assert_eq!(
            serde_json::to_value(eng.node(&legacy.symbol()).unwrap()).unwrap(),
            serde_json::to_value(Some(&node)).unwrap()
        );
    }

    #[test]
    fn malformed_locks_and_locked_retired_nodes_fail_closed() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        for value in [
            serde_json::Value::Null,
            serde_json::json!({"pack": "broken"}),
            serde_json::json!({"pack": "x", "layer": "user", "version": "v1"}),
        ] {
            let mem = Memory::new(MemKind::Fact, Tier::Semantic, Scope::root(), "quartz", 1);
            let mut node = mem.to_node();
            node.metadata.insert("lock".into(), value);
            assert_eq!(memory_lock(&node).unwrap().pack, "?");
            eng.store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
            assert!(
                eng.capture(&mem)
                    .unwrap_err()
                    .to_string()
                    .contains("memory_locked")
            );
            assert!(
                eng.erase_id(&mem.id)
                    .unwrap_err()
                    .to_string()
                    .contains("memory_locked")
            );
            assert!(
                eng.retire(&mem.id, 2)
                    .unwrap_err()
                    .to_string()
                    .contains("memory_locked")
            );
            node.kind = NodeKind::Other(RETIRED_MEMORY_NODE_KIND.into());
            node.metadata.insert(meta_keys::RETIRED_AT.into(), 2.into());
            eng.store.upsert_nodes(&[node]).unwrap();
            assert!(
                eng.unretire(&mem.id)
                    .unwrap_err()
                    .to_string()
                    .contains("memory_locked")
            );
        }
        assert!(
            eng.erase("")
                .unwrap_err()
                .to_string()
                .contains("memory_locked")
        );
    }

    #[test]
    fn memory_proposal_approval_obeys_capture_lock_checks() {
        use std::collections::BTreeMap;
        let mut eng = MemoryEngine::in_memory().unwrap();
        let id = eng
            .submit_proposal(
                "memory",
                serde_json::json!({"content": "quartz"}),
                Facets::default(),
                BTreeMap::new(),
                1,
            )
            .unwrap();
        let mem = Memory::new(MemKind::Fact, Tier::Semantic, Scope::root(), "original", 1)
            .with_id(format!("proposal:{id}"));
        let mut node = mem.to_node();
        node.metadata.insert(
            "lock".into(),
            serde_json::json!({"pack": "fixture", "layer": "org", "version": "v1"}),
        );
        eng.store.upsert_nodes(std::slice::from_ref(&node)).unwrap();
        assert!(
            eng.approve_proposal(&id, 2)
                .unwrap_err()
                .to_string()
                .contains("memory_locked")
        );
        assert_eq!(
            eng.list_proposals(None, None).unwrap()[0].state,
            wicked_estate_memory_core::ProposalState::Pending
        );
        assert_eq!(
            serde_json::to_value(eng.node(&node.symbol).unwrap()).unwrap(),
            serde_json::to_value(Some(&node)).unwrap()
        );
    }

    #[test]
    fn retire_hides_every_recall_lane_and_restores_identity_and_sidecar() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:retire");
        let mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "retire quartz",
            100,
        );
        let seed = wicked_estate_core::Symbol::synthetic("code", "seed").id();
        eng.capture_about(&mem, std::slice::from_ref(&seed))
            .unwrap();
        let original = eng.node(&mem.symbol()).unwrap().unwrap();
        let vector = eng.embed_for(&mem.content);
        assert_eq!(eng.ext.count().unwrap(), 1);
        assert_eq!(eng.retire(&mem.id, 101).unwrap(), Some(mem.symbol()));
        let retired = eng.node(&mem.symbol()).unwrap().unwrap();
        assert!(Memory::from_node(&retired).is_none());
        assert_eq!(retired.metadata["retired_at"], 101);
        assert_eq!(eng.count().unwrap(), 0);
        assert!(eng.all_memories().unwrap().is_empty());
        assert_eq!(eng.ext.count().unwrap(), 0);
        assert!(eng.ext.ids_in_tier(Tier::Semantic).unwrap().is_empty());
        assert!(eng.ext.fact_ids_reinforced(0).unwrap().is_empty());
        assert!(eng.ext.aged_ids(&[Tier::Semantic], 200).unwrap().is_empty());
        assert!(eng.hydrate(&[mem.symbol().0.clone()]).unwrap().is_empty());
        // The vector and about edge survive the kind change; hydration must reject both lanes.
        assert!(
            eng.store
                .nearest(&vector, 10)
                .unwrap()
                .iter()
                .any(|(id, _)| id == &mem.symbol())
        );
        assert_eq!(
            eng.about_seed_ids(std::slice::from_ref(&seed)).unwrap(),
            vec![mem.symbol()]
        );
        for mode in [
            RecallMode::KeywordOnly,
            RecallMode::VectorOnly,
            RecallMode::GraphOnly,
            RecallMode::Hybrid,
        ] {
            let hits = eng
                .recall_mode(
                    &mem.content,
                    ScopeFilter::Ancestors(&scope),
                    std::slice::from_ref(&seed),
                    500,
                    102,
                    mode,
                )
                .unwrap();
            assert!(hits.is_empty(), "retired memory leaked through {mode:?}");
        }
        assert_eq!(eng.retire(&mem.symbol().0, 103).unwrap(), None);
        assert_eq!(eng.retire("unknown", 103).unwrap(), None);
        let listed = eng.retired(Some("project:retire")).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].0.symbol(), listed[0].1), (mem.symbol(), 101));
        assert!(eng.retired(Some("project:other")).unwrap().is_empty());
        assert_eq!(eng.unretire(&mem.symbol().0).unwrap(), Some(mem.symbol()));
        assert_eq!(eng.unretire(&mem.id).unwrap(), None);
        assert_eq!(eng.ext.count().unwrap(), 1);
        assert_eq!(
            eng.node(&mem.symbol()).unwrap().unwrap().metadata,
            original.metadata
        );
        assert!(eng.retired(None).unwrap().is_empty());
        let hits = eng
            .recall_mode(
                &mem.content,
                ScopeFilter::Ancestors(&scope),
                &[seed],
                500,
                104,
                RecallMode::Hybrid,
            )
            .unwrap();
        assert_eq!(hits[0].id, mem.symbol());
    }

    #[test]
    fn retired_memories_survive_reflect_and_working_expiry_without_distillation() {
        use wicked_estate_memory_core::{MemoryApi, WORKING_TTL_SECS};
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:retire");
        for (kind, tier, content) in [
            (MemKind::Episode, Tier::Episodic, "Alice prefers quartz"),
            (MemKind::Working, Tier::Working, "temporary quartz"),
            (MemKind::Fact, Tier::Semantic, "retired quartz fact"),
        ] {
            let mem = Memory::new(kind, tier, scope.clone(), content, 100);
            eng.capture(&mem).unwrap();
            eng.retire(&mem.id, 101).unwrap();
        }
        assert!(eng.recall_episodic_batch(&scope).unwrap().is_empty());
        let reflected =
            MemoryApi::reflect(&mut eng, &scope.as_path(), 100 + WORKING_TTL_SECS).unwrap();
        assert_eq!(reflected.node_count, 0);
        assert!(reflected.distilled_facts.is_empty());
        assert_eq!(MemoryApi::coverage(&eng, None).unwrap().total, 0);
        assert!(MemoryApi::list(&eng, None).unwrap().is_empty());
        assert_eq!(
            MemoryApi::list_retired(&eng, None).unwrap().unwrap().len(),
            3
        );
    }

    #[test]
    fn an_undecodable_retired_node_never_blocks_list_restore_or_erase() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:retire");
        let good = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "good quartz",
            1,
        );
        let bad = Memory::new(MemKind::Fact, Tier::Semantic, scope, "bad quartz", 1);
        eng.capture(&good).unwrap();
        eng.capture(&bad).unwrap();
        eng.retire(&bad.id, 2).unwrap();
        eng.retire(&good.id, 2).unwrap();
        let mut node = eng.store.get_node(&bad.symbol()).unwrap().unwrap();
        node.metadata
            .insert(meta_keys::RETIRED_AT.into(), "not a time".into());
        eng.store.upsert_nodes(&[node]).unwrap();
        let listed = eng.retired(None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0.symbol(), good.symbol());
        assert_eq!(eng.unretire(&good.id).unwrap(), Some(good.symbol()));
        assert_eq!(eng.erase("project:retire").unwrap(), 1);
    }

    #[test]
    fn retire_survives_reopen_and_reconciles_only_active_sidecar_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");
        let path = path.to_str().unwrap();
        let mem = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            Scope::root(),
            "durable quartz",
            1,
        );
        {
            let mut eng = MemoryEngine::open(path).unwrap();
            eng.capture(&mem).unwrap();
            eng.retire(&mem.id, 2).unwrap();
            // Simulate a crash between the authoritative write and sidecar removal.
            eng.ext.upsert(&mem).unwrap();
        }
        let mut eng = MemoryEngine::open(path).unwrap();
        assert_eq!(eng.ext.count().unwrap(), 0);
        assert_eq!(eng.retired(None).unwrap().len(), 1);
        assert_eq!(eng.unretire(&mem.id).unwrap(), Some(mem.symbol()));
        assert_eq!(eng.ext.count().unwrap(), 1);
    }

    #[test]
    fn durable_capture_redacts_database_wal_and_recall() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");
        let scope = Scope::root();
        {
            let mut eng = MemoryEngine::open(path.to_str().unwrap()).unwrap();
            eng.capture(&Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                scope.clone(),
                "redaction fixture DB_PASSWORD=hunter2-example AKIAIOSFODNN7EXAMPLE",
                1,
            ))
            .unwrap();
            let hits = eng
                .recall(
                    "redaction fixture",
                    ScopeFilter::Ancestors(&scope),
                    &[],
                    500,
                    2,
                )
                .unwrap();
            assert!(!hits.is_empty());
            for hit in hits {
                assert!(!hit.content.contains("hunter2-example"));
                assert!(!hit.content.contains("AKIAIOSFODNN7EXAMPLE"));
            }
            eng.submit_proposal(
                "memory",
                serde_json::json!({"content": "password: \"hunter2-example\""}),
                Facets::default(),
                std::collections::BTreeMap::new(),
                1,
            )
            .unwrap();
            let proposals = eng.list_proposals(None, None).unwrap();
            assert_eq!(proposals.len(), 1);
            assert!(!proposals[0].payload.to_string().contains("hunter2-example"));
            assert_capture_files_clean(&path);
        }
        assert_capture_files_clean(&path);
    }

    fn assert_capture_files_clean(path: &std::path::Path) {
        for path in [
            path.to_path_buf(),
            std::path::PathBuf::from(format!("{}-wal", path.display())),
        ] {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => panic!("read fixture: {e}"),
            };
            for value in ["hunter2-example", "AKIAIOSFODNN7EXAMPLE"] {
                assert!(!bytes.windows(value.len()).any(|w| w == value.as_bytes()));
            }
        }
    }

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
        let out = eng
            .recall("billing", ScopeFilter::Ancestors(&scope), &[], 500, 2)
            .unwrap();
        assert!(out.iter().any(|r| r.content.contains("Stripe")));
    }

    /// WAL checkpointing (perf #5): the engine forwards `checkpoint_truncate` to its backend —
    /// on a durable SQLite store the `-wal` file must end up empty and recall must survive it.
    #[test]
    fn checkpoint_truncate_forwards_to_backend_and_recall_survives() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mem.db");
        let path_str = path.to_str().unwrap();
        let mut eng = MemoryEngine::open(path_str).unwrap();
        let scope = Scope::root();
        eng.capture(&Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "the WAL must not outgrow the database",
            1,
        ))
        .unwrap();

        let stats = eng.checkpoint_truncate().unwrap();
        assert!(!stats.busy, "no concurrent reader ⇒ the truncate completes");
        let wal = dir.path().join("mem.db-wal");
        assert_eq!(
            std::fs::metadata(&wal).expect("wal file kept").len(),
            0,
            "TRUNCATE must leave a zero-byte -wal"
        );
        let out = eng
            .recall("WAL database", ScopeFilter::Ancestors(&scope), &[], 500, 2)
            .unwrap();
        assert!(
            out.iter().any(|r| r.content.contains("outgrow")),
            "recall must survive a TRUNCATE checkpoint"
        );
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
            .recall(
                "what does the user drink",
                ScopeFilter::Ancestors(&scope),
                &[],
                1000,
                now,
            )
            .unwrap();
        assert!(!out.is_empty(), "recall returned nothing");
        // The oat-milk fact should surface (keyword + semantic both favor it).
        assert!(
            out.iter().any(|r| r.content.contains("oat milk")),
            "expected the oat-milk memory in: {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
    }

    // Symbolish parity (the S3 bench's one outright estate loss), memory-side mirror of the
    // knowledge test: an identifier-shaped query must rank the memory containing the LITERAL
    // identifier above memories that merely scatter its unigrams — the whole-query phrase list
    // in `keyword_candidates` preserves FTS5 token adjacency (`vault_gate.py` → [vault, gate, py]).
    #[test]
    fn identifier_query_ranks_adjacent_phrase_above_scattered_unigrams() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let now = 5;
        let scope = Scope::parse("org:acme/unit:eng");
        for i in 0..4 {
            eng.capture(&Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                scope.clone(),
                format!(
                    "note {i}: the vault held the gate open; py tooling gates the vault \
                     release, gate reviews vault py changes"
                ),
                now,
            ))
            .unwrap();
        }
        eng.capture(&Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "vault_gate.py enforces the evidence gate before phase transitions",
            now,
        ))
        .unwrap();

        let out = eng
            .recall(
                "vault_gate.py",
                ScopeFilter::Ancestors(&scope),
                &[],
                4000,
                now,
            )
            .unwrap();
        assert!(!out.is_empty(), "recall returned nothing");
        assert!(
            out[0].content.contains("vault_gate.py"),
            "the memory with the literal identifier must rank FIRST; got: {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
    }

    // The vector-list gate, memory-side: a NON-semantic embedder (the hash fallback) must not
    // surface zero-lexical-overlap memories through Hybrid vector fusion; a semantic embedder
    // must keep doing so. (An explicit RecallMode::VectorOnly ablation stays honored either way.)
    #[test]
    fn non_semantic_embedder_contributes_no_vector_candidates() {
        struct ConstEmbedder {
            semantic: bool,
        }
        impl Embedder for ConstEmbedder {
            fn id(&self) -> &str {
                "const:test"
            }
            fn embed(&self, _text: &str) -> Vec<f32> {
                vec![1.0, 0.0]
            }
            fn dim(&self) -> usize {
                2
            }
            fn is_semantic(&self) -> bool {
                self.semantic
            }
        }

        let run = |semantic: bool, mode: RecallMode| {
            let mut eng = MemoryEngine::in_memory()
                .unwrap()
                .with_embedder(Box::new(ConstEmbedder { semantic }));
            let scope = Scope::parse("org:acme/unit:eng");
            eng.capture(&Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                scope.clone(),
                "zebra quagga",
                5,
            ))
            .unwrap();
            // Query shares NO tokens with the memory — only the vector path can surface it.
            eng.recall_mode(
                "unrelated words",
                ScopeFilter::Ancestors(&scope),
                &[],
                2000,
                5,
                mode,
            )
            .unwrap()
        };

        assert!(
            !run(true, RecallMode::Hybrid).is_empty(),
            "semantic embedder: vector path must surface the zero-overlap memory"
        );
        assert!(
            run(false, RecallMode::Hybrid).is_empty(),
            "non-semantic embedder: vector noise must be gated out of Hybrid recall"
        );
        assert!(
            !run(false, RecallMode::VectorOnly).is_empty(),
            "explicit VectorOnly ablation must stay honored even for a non-semantic embedder"
        );
    }

    #[test]
    fn recall_semantic_embedder_no_match_returns_empty() {
        struct OrthogonalEmbedder;
        impl Embedder for OrthogonalEmbedder {
            fn id(&self) -> &str {
                "orthogonal:test"
            }
            fn embed(&self, text: &str) -> Vec<f32> {
                if text == "unrelated query" {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                }
            }
            fn dim(&self) -> usize {
                2
            }
            fn is_semantic(&self) -> bool {
                true
            }
        }

        let mut eng = MemoryEngine::in_memory()
            .unwrap()
            .with_embedder(Box::new(OrthogonalEmbedder));
        let scope = Scope::root();
        let now = 1_000;
        eng.capture(&Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "zebra quagga",
            now,
        ))
        .unwrap();

        let out = eng
            .recall(
                "unrelated query",
                ScopeFilter::Ancestors(&scope),
                &[],
                500,
                now,
            )
            .unwrap();
        assert!(out.is_empty(), "a cosine-zero hit is not a semantic match");
        let ranked = eng
            .recall_ranked(
                "unrelated query",
                ScopeFilter::Ancestors(&scope),
                &[],
                10,
                now,
                RecallMode::Hybrid,
            )
            .unwrap();
        assert!(
            ranked.is_empty(),
            "ranked recall must use the same relevance floor"
        );
    }

    #[test]
    fn recall_working_row_never_outranks_semantic_match() {
        let mut eng = MemoryEngine::in_memory()
            .unwrap()
            .with_embedder(Box::new(HashEmbedder::new(256)));
        let scope = Scope::root();
        let now = 1_000;
        let working = Memory::new(
            MemKind::Working,
            Tier::Working,
            scope.clone(),
            "Session goal (turn 3): investigate billing and database",
            now,
        );
        let semantic = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "billing uses Stripe",
            now - 60,
        );
        eng.capture(&working).unwrap();
        eng.capture(&semantic).unwrap();

        let out = eng
            .recall(
                "billing database",
                ScopeFilter::Ancestors(&scope),
                &[],
                500,
                now,
            )
            .unwrap();
        let ranked = eng
            .recall_ranked(
                "billing database",
                ScopeFilter::Ancestors(&scope),
                &[],
                10,
                now,
                RecallMode::Hybrid,
            )
            .unwrap();
        for hits in [&out, &ranked] {
            let ids: Vec<_> = hits.iter().map(|hit| hit.id.clone()).collect();
            assert_eq!(
                ids,
                [semantic.symbol(), working.symbol()],
                "both recall paths must put the semantic match first and retain matching Working rows"
            );
        }
        assert!(
            out[1].score > out[0].score,
            "tier ordering must override the stronger Working score"
        );
    }

    #[test]
    fn recall_ranks_topical_memory_above_legacy_session_goal() {
        let mut eng = MemoryEngine::in_memory()
            .unwrap()
            .with_embedder(Box::new(HashEmbedder::new(256)));
        let scope = Scope::parse("project:demo");
        let t0 = 1_000;
        let session_goal = Memory::new(
            MemKind::Working,
            Tier::Working,
            scope.clone(),
            "Session goal (turn 3): what does the build do to the files and the plan",
            t0,
        );
        let topical = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "billing database is postgres on port 5432",
            t0,
        );
        eng.capture(&session_goal).unwrap();
        eng.capture(&topical).unwrap();

        let out = eng
            .recall(
                "what does the billing database do",
                ScopeFilter::Ancestors(&scope),
                &[],
                500,
                t0 + 60,
            )
            .unwrap();
        assert_eq!(
            out.first().map(|r| &r.id),
            Some(&topical.symbol()),
            "the topical memory must rank first"
        );
        assert!(
            out.iter().all(|r| r.id != session_goal.symbol()),
            "function-word overlap must not surface the legacy session goal"
        );
    }

    #[test]
    fn recall_function_words_without_a_phrase_match_returns_empty() {
        let mut eng = MemoryEngine::in_memory()
            .unwrap()
            .with_embedder(Box::new(HashEmbedder::new(256)));
        let scope = Scope::parse("project:demo");
        let t0 = 1_000;
        for (kind, tier, content) in [
            (
                MemKind::Working,
                Tier::Working,
                "Session goal (turn 3): what does the build do to the files and the plan",
            ),
            (
                MemKind::Fact,
                Tier::Semantic,
                "billing database is postgres on port 5432",
            ),
        ] {
            eng.capture(&Memory::new(kind, tier, scope.clone(), content, t0))
                .unwrap();
        }

        let out = eng
            .recall(
                "what does the to be",
                ScopeFilter::Ancestors(&scope),
                &[],
                500,
                t0 + 60,
            )
            .unwrap();
        assert!(
            out.is_empty(),
            "unrelated function words must recall nothing"
        );
    }

    #[test]
    fn recall_skips_working_memory_past_its_ttl() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:demo");
        let t0 = 1_000;
        eng.capture(&Memory::new(
            MemKind::Working,
            Tier::Working,
            scope.clone(),
            "wip snapshot pre compaction turn count",
            t0,
        ))
        .unwrap();
        let recall = |eng: &MemoryEngine, now| {
            eng.recall(
                "wip snapshot",
                ScopeFilter::Ancestors(&scope),
                &[],
                500,
                now,
            )
            .unwrap()
        };
        assert_eq!(
            recall(&eng, t0 + 60).len(),
            1,
            "fresh working memory is recallable"
        );
        let expired = t0 + wicked_estate_memory_core::WORKING_TTL_SECS;
        assert!(
            recall(&eng, expired).is_empty(),
            "working memory expires after 24 h"
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

        let out = eng
            .recall(
                "secret roadmap",
                ScopeFilter::Ancestors(&acme),
                &[],
                1000,
                now,
            )
            .unwrap();
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
    fn facet_intent_gates_and_boosts_recall() {
        // DES-MEM-FACETED-001 §4.3: with intent {cli:codex}, an unfaceted memory and a {cli:codex}
        // memory both surface (the codex one boosted above the unfaceted one), while a {repo:other}
        // memory is EXCLUDED (it constrains an axis the intent does not carry — no leakage).
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let now = 1;
        // Same tier + near-identical bodies so base scores match and the facet boost is the only
        // differentiator between the global and the {cli:codex} memory.
        eng.capture(&Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            scope.clone(),
            "workspace write policy note one",
            now,
        ))
        .unwrap();
        eng.capture(
            &Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                scope.clone(),
                "workspace write policy note two",
                now,
            )
            .with_facets(Facets::try_from_map([("cli", "codex")]).unwrap()),
        )
        .unwrap();
        eng.capture(
            &Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                scope.clone(),
                "workspace write policy note three",
                now,
            )
            .with_facets(Facets::try_from_map([("repo", "other")]).unwrap()),
        )
        .unwrap();

        let intent = Facets::try_from_map([("cli", "codex")]).unwrap();
        let cands = eng
            .ranked_candidates(
                "workspace write policy note",
                ScopeFilter::Ancestors(&scope),
                &[],
                now,
                RecallMode::Hybrid,
                &intent,
            )
            .unwrap();
        let out = eng.pack_recalled(cands, 4000);

        let bodies: Vec<&String> = out.iter().map(|r| &r.content).collect();
        assert!(
            !out.iter().any(|r| r.content.contains("note three")),
            "a {{repo:other}} memory must NOT surface under a cli-only intent (no leakage): {bodies:?}"
        );
        let pos_global = out.iter().position(|r| r.content.contains("note one"));
        let pos_codex = out.iter().position(|r| r.content.contains("note two"));
        let (pos_global, pos_codex) = (
            pos_global.expect("the unfaceted memory must surface"),
            pos_codex.expect("the {cli:codex} memory must surface"),
        );
        assert!(
            pos_codex < pos_global,
            "the specificity-boosted {{cli:codex}} memory must rank above the unfaceted one; got: {:?}",
            out.iter()
                .map(|r| (&r.content, r.score))
                .collect::<Vec<_>>()
        );
    }

    /// Build an engine holding one memory per scope: root, `brain:test/doc:a` (a migrated-style
    /// leaf), and `brain:other/doc:b` (a sibling subtree). All three lexically match the query
    /// `"wicked garden fact"`, so any exclusion below is the SCOPE FILTER's doing, not retrieval's.
    fn subtree_fixture() -> (MemoryEngine, i64) {
        let now = 7;
        let mut eng = MemoryEngine::in_memory().unwrap();
        for (scope, text) in [
            (Scope::root(), "root wicked garden fact"),
            (
                Scope::parse("brain:test/doc:a"),
                "leaf wicked garden fact from the brain import",
            ),
            (
                Scope::parse("brain:other/doc:b"),
                "sibling wicked garden fact in another subtree",
            ),
        ] {
            eng.capture(&Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                scope,
                text,
                now,
            ))
            .unwrap();
        }
        (eng, now)
    }

    #[test]
    fn root_recall_without_prefix_does_not_see_leaf_scoped_memory() {
        // Pins the EXISTING inheritance semantics (no regression from adding Subtree): a recall at
        // root without a prefix sees only root-scoped memories — descendant (leaf) scopes stay
        // invisible. This is exactly the migration gap: memories imported at
        // `brain:wicked-garden/doc:<id>` are unreachable from an unscoped (root) recall.
        let (eng, now) = subtree_fixture();
        let root = Scope::root();
        let out = eng
            .recall(
                "wicked garden fact",
                ScopeFilter::Ancestors(&root),
                &[],
                4000,
                now,
            )
            .unwrap();
        assert!(
            out.iter().any(|r| r.content.contains("root")),
            "root recall must see the root-scoped memory"
        );
        assert!(
            !out.iter().any(|r| r.content.contains("leaf")),
            "ANCESTOR SEMANTICS REGRESSED: root recall without scope_prefix returned a \
             leaf-scoped memory: {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn subtree_empty_prefix_sees_root_and_leaf() {
        // `Subtree("")` = the root subtree = every memory: both the root-scoped and the
        // leaf-scoped memories surface (this is the recall garden's hooks need to reach the 205
        // migrated brain memories from an unscoped query).
        let (eng, now) = subtree_fixture();
        let out = eng
            .recall(
                "wicked garden fact",
                ScopeFilter::Subtree(""),
                &[],
                4000,
                now,
            )
            .unwrap();
        for expect in ["root", "leaf", "sibling"] {
            assert!(
                out.iter().any(|r| r.content.contains(expect)),
                "Subtree(\"\") must see the {expect}-scoped memory; got: {:?}",
                out.iter().map(|r| &r.content).collect::<Vec<_>>()
            );
        }
        // And the items carry their own scopes on the way out (S4 attribution intact).
        assert!(
            out.iter().any(|r| r.scope == "brain:test/doc:a"),
            "leaf item must carry its own scope; got: {:?}",
            out.iter().map(|r| &r.scope).collect::<Vec<_>>()
        );
    }

    #[test]
    fn subtree_prefix_sees_only_that_subtree() {
        // `Subtree("brain:test")` admits ONLY that subtree: not root (replace, not fuse — root is
        // an ancestor, not a descendant), not the `brain:other` sibling.
        let (eng, now) = subtree_fixture();
        let out = eng
            .recall(
                "wicked garden fact",
                ScopeFilter::Subtree("brain:test"),
                &[],
                4000,
                now,
            )
            .unwrap();
        assert!(
            out.iter().any(|r| r.content.contains("leaf")),
            "Subtree(\"brain:test\") must see its leaf memory; got: {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
        assert!(
            !out.iter().any(|r| r.content.contains("root")),
            "REPLACE VIOLATED: subtree recall returned the root-scoped (ancestor) memory"
        );
        assert!(
            !out.iter().any(|r| r.content.contains("sibling")),
            "SUBTREE ISOLATION VIOLATED: recall leaked the brain:other sibling subtree"
        );
        // Segment-aware: "brain:tes" is a string prefix of "brain:test" but NOT a scope ancestor —
        // it must match nothing (path_in_prefix isolation, same as erase/coverage).
        let none = eng
            .recall(
                "wicked garden fact",
                ScopeFilter::Subtree("brain:tes"),
                &[],
                4000,
                now,
            )
            .unwrap();
        assert!(
            none.is_empty(),
            "SEGMENT ISOLATION VIOLATED: partial-segment prefix matched: {:?}",
            none.iter().map(|r| &r.content).collect::<Vec<_>>()
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
            .recall(
                "system event",
                ScopeFilter::Ancestors(&scope),
                &[],
                budget,
                now,
            )
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
        let budgeted = eng
            .recall(query, ScopeFilter::Ancestors(&scope), &[], tiny_budget, now)
            .unwrap();

        // recall_ranked at k=10 with the SAME query: NO budget cap → up to 10 units.
        let ranked = eng
            .recall_ranked(
                query,
                ScopeFilter::Ancestors(&scope),
                &[],
                10,
                now,
                RecallMode::Hybrid,
            )
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
            .recall(
                "billing checkout",
                ScopeFilter::Ancestors(&scope),
                &[],
                big_budget,
                now,
            )
            .unwrap();
        let ranked = eng
            .recall_ranked(
                "billing checkout",
                ScopeFilter::Ancestors(&scope),
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
        let without = eng
            .recall(query, ScopeFilter::Ancestors(&scope), &[], 1000, now)
            .unwrap();
        // WITH the code seed: the `about` cross-edge surfaces it.
        let with = eng
            .recall(
                query,
                ScopeFilter::Ancestors(&scope),
                std::slice::from_ref(&checkout),
                1000,
                now,
            )
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
