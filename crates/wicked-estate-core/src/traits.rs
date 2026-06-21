//! The five traits every wicked_estate crate programs against. This is the load-bearing seam:
//! fixing these lets independent agents build extractors, resolvers, stores, rankers, and
//! retrieval tools in parallel without colliding (see `docs/plan/WAVE-PLAN.md`, Wave 0.4).
//!
//! Read methods take `&self`; mutation takes `&mut self`. Rankers and retrieval tools receive a
//! read-only `&dyn GraphStore`, so they cannot accidentally mutate the graph.

use crate::annotation::Annotation;
use crate::change::{Change, ChangeOp};
use crate::edge::{Direction, Edge, ResolutionTier};
use crate::error::Result;
use crate::history::HistoricalEdge;
use crate::node::{Language, Node, SourceFile};
use crate::query::{GraphStats, RetrievalResult, Subgraph, SymbolQuery, TraversalSpec};
use crate::refs::{Extraction, UnresolvedRef};
use crate::repo::RepoInfo;
use crate::semantics::NodeSemantics;
use crate::symbol::SymbolId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A read-only symbol lookup the resolver consults to bind names to [`SymbolId`]s.
pub trait SymbolIndex {
    /// All symbols whose simple name matches (the resolver disambiguates via ref hints).
    fn by_name(&self, name: &str) -> Vec<Node>;
    /// Exact lookup of a fully-qualified symbol id.
    fn get(&self, id: &SymbolId) -> Option<Node>;
    /// All nodes in the index — used by resolvers that need a global scan (e.g. `RulesBridgeResolver`).
    /// The default returns an empty vec; backends should override with their own bulk fetch.
    fn all_nodes(&self) -> Result<Vec<Node>> {
        Ok(vec![])
    }
}

/// EXTRACT phase: parse one file into nodes + intra-file edges + unresolved refs.
/// Stateless and parallelizable per file (no cross-file knowledge). Fan-out target (Wave 2.1).
pub trait Extractor: Send + Sync {
    /// Languages this extractor handles (tree-sitter grammar names).
    fn languages(&self) -> Vec<Language>;
    fn extract(&self, file: &SourceFile) -> Result<Extraction>;
}

/// RESOLVE phase: turn unresolved refs into edges using the whole-project symbol index.
/// Swappable behind this trait so resolution evolves without re-parsing (Wave 1.1).
pub trait Resolver: Send + Sync {
    /// Stable id recorded on every edge this resolver emits (e.g. "import-map-py").
    fn id(&self) -> &str;
    /// The resolution tier (sets default confidence / provenance).
    fn tier(&self) -> ResolutionTier;
    fn resolve(&self, refs: &[UnresolvedRef], index: &dyn SymbolIndex) -> Result<Vec<Edge>>;
}

/// What a backend can do natively, so retrieval can adapt (e.g. fall back to client-side fusion
/// when the store lacks vector search). An external DB can advertise different capabilities than
/// the embedded default — see `docs/adr/ADR-003-storage-backends.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreCapabilities {
    /// BM25 full-text search runs inside the store.
    pub full_text_search: bool,
    /// Vector / ANN search runs inside the store.
    pub vector_search: bool,
    /// `traverse` executes in the engine (one round-trip), not as client-side BFS.
    pub server_side_traversal: bool,
    /// `begin_batch`/`commit_batch` map to a real transaction.
    pub transactional_batch: bool,
    /// Safe for concurrent writers (an external/shared DB) vs single-writer (a local file).
    pub shared_writers: bool,
}

/// Read side of the graph. Deliberately separate from [`GraphWrite`] so a deployment can scale
/// readers independently of the single writer: an external DB backs this with a connection pool
/// or read replica, while the indexer holds the writer. Rankers and retrieval tools take
/// `&dyn GraphRead`, so they can never mutate the graph. (See `docs/adr/ADR-003-storage-backends.md`.)
pub trait GraphRead: Send {
    /// Native capabilities of this backend (drives retrieval fallbacks).
    fn capabilities(&self) -> StoreCapabilities;
    fn get_node(&self, id: &SymbolId) -> Result<Option<Node>>;
    fn find_symbols(&self, query: &SymbolQuery) -> Result<Vec<Node>>;
    /// Edges incident to `id` in the given direction. The edge-direction invariant is enforced
    /// here: `Direction::Dependents` returns edges where `target == id`.
    fn neighbors(&self, id: &SymbolId, dir: Direction) -> Result<Vec<Edge>>;
    /// Bounded traversal (e.g. reverse-reachability for blast-radius). Push this server-side on
    /// backends where `capabilities().server_side_traversal` is true (one round-trip, not BFS).
    fn traverse(&self, start: &SymbolId, spec: &TraversalSpec) -> Result<Subgraph>;
    /// All nodes — for global analytics (PageRank) and export. Local-first scale.
    fn all_nodes(&self) -> Result<Vec<Node>>;
    /// All edges — for global analytics (PageRank) and export.
    fn all_edges(&self) -> Result<Vec<Edge>>;
    /// Unresolved references whose written name matches `name` — i.e. potential MISSING callers of
    /// a symbol with that name. Powers honest blast-radius coverage: never silently claim
    /// "no dependents" when calls to that name went unresolved.
    fn unresolved_refs_for_name(&self, name: &str) -> Result<Vec<UnresolvedRef>>;
    /// The stored content digest for `file`, if indexed (incremental change detection). (Wave 2.6)
    fn file_digest(&self, file: &str) -> Result<Option<String>>;
    /// The git blob SHA recorded for `file` (`git hash-object`), if the repo was a git checkout.
    /// Content-addressed file-version id; correlates the graph to git history. (Wave 7 — live brain)
    fn file_git_sha(&self, file: &str) -> Result<Option<String>>;
    /// Repo-wide git provenance captured at index time (HEAD commit / branch / remote / dirty),
    /// if the indexed path was a git repo. (Wave 7 — live brain)
    fn repo_info(&self) -> Result<Option<RepoInfo>>;
    /// The read-only history of edges `file` produced at *prior* versions (newest first), each tagged
    /// with its git blob SHA. NEVER traversed — pure provenance lookup. Empty when history is off or
    /// the file has no superseded versions. (Wave 7 — the brain remembers old connections)
    fn edge_history(&self, file: &str) -> Result<Vec<HistoricalEdge>>;
    /// The stored source text for `file`, if any. (Wave 11.1 content store)
    fn file_content(&self, file: &str) -> Result<Option<String>>;
    /// The source slice for `node` (from its file's stored content + location span). (Wave 11.1)
    fn symbol_source(&self, node: &Node) -> Result<Option<String>>;
    /// Change-log deltas with `seq > cursor`, oldest first — for reactive subscription. A subscriber
    /// passes the last `seq` it saw and resumes from there. Capped per call by the impl. (Wave 7.1)
    fn changes_since(&self, cursor: u64) -> Result<Vec<Change>>;
    /// Semantic annotations for `symbol` (description / matched requirement / validated), if any.
    /// Powers requirement↔functionality linking alongside the structural graph. (Semantic linking)
    fn node_semantics(&self, symbol: &SymbolId) -> Result<Option<NodeSemantics>>;
    /// All symbols annotated with `requirement` — answers "which functionality satisfies R?".
    fn find_by_requirement(&self, requirement: &str) -> Result<Vec<Node>>;
    /// All typed key/value annotations on `symbol`, oldest first (by `ts`). An entity may carry
    /// many annotations of the same or different [`type`](Annotation::type); legacy/untyped rows
    /// read back with `type = "note"`. Empty vec (not an error) when the symbol has none / is
    /// absent. This is the seam that lets retrieval/MCP surface annotations from `&dyn GraphRead`.
    fn annotations(&self, symbol: &SymbolId) -> Result<Vec<Annotation>>;
    /// Every `(symbol, annotation)` pair whose annotation `type` equals `ty` — powers "all open
    /// questions" / "every assumption in the repo" without scanning all nodes. `ty` is matched as
    /// an opaque string (known convention OR custom type — identical treatment). Pairs are ordered
    /// by symbol then `ts` for deterministic output.
    fn annotations_by_type(&self, ty: &str) -> Result<Vec<(SymbolId, Annotation)>>;
    /// Every `(symbol, annotation)` pair whose evidence-envelope `last_verified` is **strictly
    /// before** `cutoff` (Unix-seconds) — i.e. the facts a re-verification window deems stale.
    /// Never-verified annotations (`last_verified == 0`) are stale for any positive `cutoff`. This
    /// is the freshness read the evidence envelope adds: "what needs re-verification?" — the store
    /// counterpart of [`Annotation::is_stale_since`]. Pairs are ordered by symbol then `ts` for
    /// deterministic output, parallel to [`Self::annotations_by_type`]. Surfaced to consumers via
    /// the `wicked-estate stale-annotations <cutoff>` CLI command.
    fn annotations_stale_since(&self, cutoff: i64) -> Result<Vec<(SymbolId, Annotation)>>;
    /// The live symbol's current EPOCH (`symbols.gen`) — a monotonic generation that increments each
    /// time a symbol with the same stable [`SymbolId`] is re-added after its node was deleted
    /// (reuse-after-delete). Returns `Some(gen)` when `id` currently has a live node, `None` when it
    /// has no live node (never indexed, edge-endpoint-only, or removed and not yet re-added). M8 /
    /// DoD-XA4. This is the cross-store seam the about-arm (DEC-X6-SEQ) stamps/validates xedge
    /// endpoints against, so a stale cross-store edge can fail-closed instead of resolving to a
    /// live-WRONG node that merely happens to reuse the same id. A first-ever node — including one for
    /// a symbol that previously existed ONLY as an edge endpoint / unresolved-ref — has epoch 0; the
    /// FIRST delete-then-re-add yields `Some(g)` with `g >= 1`. The bump fires in the store's shared
    /// node-upsert seam (covering both the FTS and skip-FTS reindex paths), NEVER in symbol interning.
    fn symbol_epoch(&self, id: &SymbolId) -> Result<Option<u64>>;
    fn stats(&self) -> Result<GraphStats>;
}

/// Write side of the graph — held by the indexer; typically a single writer. `begin_batch`/
/// `commit_batch` map to a transaction; remote backends should **bulk-batch** upserts to amortize
/// round-trips and stay idempotent under retry (see ADR-003).
pub trait GraphWrite {
    fn begin_batch(&mut self) -> Result<()>;
    fn commit_batch(&mut self) -> Result<()>;
    fn upsert_nodes(&mut self, nodes: &[Node]) -> Result<()>;
    /// Upsert edges; on a `dedup_key` collision the higher-confidence edge wins.
    fn upsert_edges(&mut self, edges: &[Edge]) -> Result<()>;
    /// Persist references the resolver could NOT bind. Keeping them is what lets blast-radius
    /// report its coverage instead of silently under-reporting (the soundness contract).
    fn upsert_unresolved_refs(&mut self, refs: &[UnresolvedRef]) -> Result<()>;
    /// Remove everything that originated from `file` (its nodes, edges, unresolved refs) — used by
    /// incremental re-indexing to replace a changed file's contributions atomically. (Wave 2.6)
    fn remove_file(&mut self, file: &str) -> Result<()>;
    /// Record a content digest for `file` (incremental change detection — fast xxh3). (Wave 2.6)
    fn set_file_digest(&mut self, file: &str, digest: &str) -> Result<()>;
    /// Record repo-wide git provenance for this graph (overwrites the single `meta` row). (Wave 7)
    fn set_repo_info(&mut self, info: &RepoInfo) -> Result<()>;
    /// Store a file's source text (the content store — lets retrieval return real code). (Wave 11.1)
    fn set_file_content(&mut self, file: &str, text: &str) -> Result<()>;
    /// Delete edges whose `source` or `target` is no longer a node (orphans left by incremental
    /// removal of a file's symbols). Returns the count pruned. Keeps blast-radius from over-reporting.
    fn prune_dangling_edges(&mut self) -> Result<usize>;
    /// Append a delta to the change log (file granularity — one entry per changed/removed file, not
    /// per node/edge, so the log never explodes during bulk indexing). Powers reactive subscription
    /// via [`GraphRead::changes_since`]. (Wave 7.1)
    fn log_change(&mut self, op: ChangeOp, target: &str) -> Result<()>;
    /// Set semantic annotations on node `symbol` (links code → requirements). PARTIAL update: each
    /// `Some(..)` writes that column, `None` leaves it unchanged. No-op if the symbol is absent.
    /// (Semantic linking — `description` / `requirement` / `requirement_validated`.)
    fn set_node_semantics(
        &mut self,
        symbol: &SymbolId,
        description: Option<&str>,
        requirement: Option<&str>,
        requirement_validated: Option<bool>,
    ) -> Result<()>;
    /// Attach a typed annotation to `symbol`. A bare INSERT, NOT an upsert — so the same symbol can
    /// carry MANY annotations (same or different `type`/`key`). No-op if the symbol is absent (not
    /// indexed). The store stamps `ts` when `annotation.ts == 0`. Stable `SymbolId` keying (ADR-002):
    /// annotations follow renames because they key on the symbol id, never line/content.
    fn annotate(&mut self, symbol: &SymbolId, annotation: Annotation) -> Result<()>;
    /// Delete annotations on `symbol` matching `key`, optionally scoped to a `type`. When `ty` is
    /// `Some(t)`, only rows with that exact `type` AND `key` are removed; when `None`, ALL rows for
    /// `key` are removed regardless of type. Returns the number of rows deleted. (Scoping by type is
    /// what makes the staleness rewrite of a derived `community` annotation safe — see the design.)
    fn delete_annotations(
        &mut self,
        symbol: &SymbolId,
        ty: Option<&str>,
        key: &str,
    ) -> Result<usize>;
}

/// Convenience supertrait for the common case where one object both reads and writes (the
/// embedded local store). Auto-implemented for any `GraphRead + GraphWrite + Send`. The default
/// impl is SQLite (the design notes); the trait keeps the engine storage-agnostic so SurrealDB
/// can be benched (W1.5) and an external DB (Postgres / server) added (ADR-003) **without touching
/// extractors, resolvers, rankers, or tools**.
pub trait GraphStore: GraphRead + GraphWrite + Send {}
impl<T: GraphRead + GraphWrite + Send> GraphStore for T {}

/// Async serving contract — implemented by connection pools and natively-async backends.
///
/// `with_read` hands the caller a `&dyn GraphRead` drawn from the pool (or constructed on the
/// fly for natively-async backends via `block_in_place`). The sync retrieval tools are unchanged;
/// only the connection-acquisition is async.
///
/// # Implementing for a new backend
/// - **Pool over a sync store** (e.g. SQLite): check out a connection, run `f` in
///   `spawn_blocking`, return the result.
/// - **Natively async store** (e.g. Postgres): implement a thin `GraphRead` adapter that wraps
///   the async client with `block_in_place`, pass it to `f`.
#[async_trait::async_trait]
pub trait AsyncGraphStore: Send + Sync {
    async fn with_read<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'a> FnOnce(&'a dyn GraphRead) -> Result<T> + Send + 'static,
        T: Send + 'static;
}

/// Assigns importance scores over the graph — personalized PageRank seeded by `seeds`
/// (100× weight on seeds; Aider repo-map pattern). Powers context ranking (Wave 4.1).
pub trait Ranker: Send + Sync {
    fn rank(&self, store: &dyn GraphRead, seeds: &[SymbolId]) -> Result<HashMap<SymbolId, f32>>;
}

/// The agent-facing query surface. The 3-tool API (`SearchEntity` / `TraverseGraph` /
/// `RetrieveEntity`) and `blast_radius` are each a `RetrievalTool` (Wave 4.3).
pub trait RetrievalTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn invoke(&self, store: &dyn GraphRead, request: &serde_json::Value)
    -> Result<RetrievalResult>;
}
