---
name: REQ-003-domain-model
title: wicked-estate Unified Foundation — Domain Model
status: draft
version: 0.2
date: 2026-07-05
author: mike.parcewski@gmail.com
review-required: true
---

# REQ-003 — wicked-estate Unified Foundation: Domain Model

> **Purpose.** This document is the single source of truth for the tool surface, store architecture, overlay federation model, memory tier model, crate topology, and migration contract of the unified `wicked-estate` server. All implementers, reviewers, and consumers MUST treat this document as normative. Ambiguity here is a defect; raise a PR against this file.

---

## Table of Contents

1. [Store Architecture (DEC-1)](#1-store-architecture-dec-1)
2. [Tool Catalog — complete and normative](#2-tool-catalog--complete-and-normative)
3. [MCP Resources (bundled skills)](#3-mcp-resources-bundled-skills)
4. [MCP Prompts](#4-mcp-prompts)
5. [Overlay / Federation Model](#5-overlay--federation-model)
6. [Memory 5-Tier Model](#6-memory-5-tier-model)
7. [Workspace Crate Topology (post-consolidation)](#7-workspace-crate-topology-post-consolidation)
8. [Event Bus Integration](#8-event-bus-integration)
9. [Migration Contract](#9-migration-contract)

---

## §1 — Store Architecture (DEC-1)

The unified server manages **four distinct SQLite databases**. Each database has exactly **one writer process** (single-writer constraint). Readers may open any database in WAL mode for concurrent reads. No cross-database foreign keys exist; referential integrity across stores is enforced at the application layer via epoch-stamped about-edges in `xedge.db`.

### Database Registry

| Database | Env Var | Owner (single writer) | Primary contents |
|---|---|---|---|
| `estate.db` | `$WICKED_ESTATE_DB` | `wicked-estate-mcp` process | Code graph nodes, edges, symbol table, FTS5 full-text index, vector store |
| `memory.db` | `$WICKED_MEMORY_DB` | wicked-memory domain within unified server | 5-tier memory nodes (T0–T4), recall FTS5 index, vector ANN table |
| `knowledge.db` | `$WICKED_KNOWLEDGE_DB` | wicked-knowledge domain within unified server | Knowledge nodes (doc/section/chunk/concept), typed relation edges, recall FTS5 index, vector ANN table |
| `xedge.db` | `$WICKED_XEDGE_DB` | `wicked-overlay` (XedgeStore) | Cross-store about-edges: (estate symbols ↔ memory nodes) and (estate symbols ↔ knowledge nodes). Epoch-stamped. |

### Single-Writer Invariant

- **estate.db**: All writes are routed through the `wicked-estate-mcp` binary (the ingestion pipeline: extract → resolve → store). No other process writes to `estate.db`.
- **memory.db** and **knowledge.db**: All writes originate from the wicked-memory and wicked-knowledge domains respectively, both hosted within the single unified server binary. They do not share a write path.
- **xedge.db**: Exclusively written by `XedgeStore` (the `wicked-estate-overlay` crate). The `knowledge.relate_code` tool and the overlay's epoch-maintenance routine are the only write paths.

### Isolation Guarantee

Tool calls in one domain (e.g., `memory.*`) MUST NOT write to another domain's store (e.g., `knowledge.db`). Cross-domain links are expressed only through `xedge.db`, written only by `wicked-estate-overlay`.

### Default Paths

When an env var is unset, the server falls back to a well-known default path relative to `$WICKED_HOME` (or `~/.wicked/`). Production deployments MUST set all four env vars explicitly.

---

## §2 — Tool Catalog — Complete and Normative

The unified server exposes **23 MCP tools** in the default build (no embedder) and **24** when `fastembed` or `model2vec` is enabled (adds `SemanticSearch`). Tool names are stable and MUST NOT change across the consolidation. Each tool is listed with: namespace, name, parameters (with defaults and caps), return type, and a one-line normative contract.

### 2.1 — `estate.*` Domain (11 tools)

All estate tools read from `estate.db`. None write to `estate.db` (writing is done by the extraction pipeline, not by MCP tools).

---

**`SearchEntity`**

| Field | Value |
|---|---|
| Parameters | `name: String`, `limit: u32 = 20` (max 100) |
| Returns | `Vec<SymbolStub>` — list of `{ symbol, kind, language, file }` |
| Contract | Full-text and prefix search over the symbol table; returns the top-`limit` matching symbol stubs ordered by relevance score. |

---

**`RetrieveEntity`**

| Field | Value |
|---|---|
| Parameters | `symbol: String` |
| Returns | `FullNode` — `{ symbol, kind, language, file, line, doc, metadata }` or `isError` if not found |
| Contract | Fetch the complete graph node for an exact symbol identifier; returns a structured error if the symbol is absent. |

---

**`TraverseGraph`**

| Field | Value |
|---|---|
| Parameters | `symbol: String`, `depth: u32 = 3` (max 16), `direction: Enum[deps, dependents, both] = both`, `edge_kinds: Vec<String>?`, `max_nodes: u32 = 200` (max 1000) |
| Returns | `GraphSlice` — `{ nodes: Vec<SymbolStub>, edges: Vec<Edge> }` |
| Contract | BFS traversal from `symbol` up to `depth` hops in the specified direction, optionally filtered to `edge_kinds`; halts early if `max_nodes` is reached (partial result, flagged). |

---

**`BlastRadius`**

| Field | Value |
|---|---|
| Parameters | `symbol: String`, `depth: u32 = 6` (max 24) |
| Returns | `Vec<SymbolStub>` — transitive dependents ordered by proximity |
| Contract | Compute all symbols transitively depending on `symbol` up to `depth` hops; answers "what breaks if I change this?". |

---

**`FetchContent`**

| Field | Value |
|---|---|
| Parameters | `symbol: String` |
| Returns | `SourceSlice` — `{ symbol, file, start_line, end_line, content: String }` or `isError` |
| Contract | Return the raw source text for the symbol's declaration/definition as stored during extraction; does not re-read the filesystem. |

---

**`ContextBundle`**

| Field | Value |
|---|---|
| Parameters | `target: String` (symbol or freetext query), `budget: u32 = 8000` (max 24000, in tokens) |
| Returns | `ContextPack` — `{ items: Vec<ContextItem>, total_tokens: u32, truncated: bool }` |
| Contract | Assemble a token-budgeted context pack for `target`: the target node's source, its direct dependencies' stubs, related doc/comment nodes, and recall snippets from memory and knowledge if cross-links exist; respects `budget` via priority truncation. |

---

**`RulesInventory`**

| Field | Value |
|---|---|
| Parameters | _(none)_ |
| Returns | `Vec<RuleSetNode>` — all RuleSet-kind nodes in the graph |
| Contract | Return all RuleSet nodes indexed in the estate graph; used by agents to discover project linting rules, architectural constraints, and coding standards. |

---

**`RankHotspots`**

| Field | Value |
|---|---|
| Parameters | `limit: u32 = 20` (max 200), `seeds: Vec<String>?` |
| Returns | `Vec<RankedSymbol>` — `{ symbol, score, kind, file }` |
| Contract | Return the top-`limit` symbols by personalised PageRank score (personalised to `seeds` if provided, global otherwise); higher score = more central in the dependency graph. |

---

**`Communities`**

| Field | Value |
|---|---|
| Parameters | `limit: u32 = 20` (max 200), `min_size: u32 = 3`, `resolution: f64 = 1.0` |
| Returns | `Vec<Community>` — `{ id, members: Vec<SymbolStub>, label? }` |
| Contract | Run Louvain community detection over the code graph; return up to `limit` communities of at least `min_size` nodes. `resolution` tunes granularity (higher = more, smaller communities). |

---

**`Lineage`**

| Field | Value |
|---|---|
| Parameters | `symbol: String`, `depth: u32 = 6` (max 24) |
| Returns | `Vec<SymbolStub>` — transitive dependencies ordered by proximity |
| Contract | Compute all symbols that `symbol` transitively depends on up to `depth` hops; answers "what does this depend on?". Complements `BlastRadius`. |

---

**`SemanticSearch`**

| Field | Value |
|---|---|
| Parameters | `query: String`, `k: u32 = 10` (max 100) |
| Returns | `Vec<SemanticResult>` — `{ symbol, score, snippet }` |
| Contract | **Conditional on embedder availability.** Cosine ANN search over the vector store; if no embedder is configured, returns `isError` with `code: EMBEDDER_NOT_CONFIGURED`. Results are sorted by descending cosine similarity. |

---

### 2.2 — `memory.*` Domain (6 tools)

All memory tools read from and write to `memory.db`. Cross-links to `xedge.db` are written via the XedgeStore API by two paths: `memory.learn` (always creates about-edges) and `memory.capture` (creates about-edges only when the `about` parameter is provided).

---

**`memory.capture`**

| Field | Value |
|---|---|
| Parameters | `content: String`, `kind: Enum[observation, decision, reflection, fact, procedure]`, `tier: Enum[T0, T1, T2, T3, T4]`, `scope: String`, `about: Vec<String>?` (estate symbol refs) |
| Returns | `memory_id: String` |
| Contract | Store a new memory node at the specified tier and scope. If `about` symbols are provided, delegates to XedgeStore to record cross-edges in `xedge.db`. |

---

**`memory.recall`**

| Field | Value |
|---|---|
| Parameters | `query: String`, `scope: String?`, `seeds: Vec<String>?` (memory IDs or symbols to bias toward), `token_budget: u32?` |
| Returns | `Vec<MemoryResult>` — `{ memory_id, tier, content, score, scope }` ranked by RRF-fused score |
| Contract | Hybrid recall: FTS5 BM25 + ANN cosine (if embedder present), fused by Reciprocal Rank Fusion; filtered to `scope` if provided; truncated to `token_budget` if specified; biased toward `seeds` neighbours. |

---

**`memory.reflect`**

| Field | Value |
|---|---|
| Parameters | `scope: String` |
| Returns | `ReflectionSummary` — `{ scope, distilled_facts: Vec<String>, node_count: u32 }` |
| Contract | Aggregate T0/T1 memories under `scope` and produce a distilled semantic summary; intended to be called by the consolidation agent to promote working memory into T2 facts. |

---

**`memory.erase`**

| Field | Value |
|---|---|
| Parameters | `scope_prefix: String` |
| Returns | `{ deleted_count: u32 }` |
| Contract | Hard-delete all memory nodes whose scope starts with `scope_prefix`; also removes associated xedge entries. Irreversible. Requires the scope prefix to be non-empty (refuses to erase all memory with an empty string). |

---

**`memory.learn`**

| Field | Value |
|---|---|
| Parameters | `fact: String`, `symbols: Vec<String>` (estate symbol identifiers) |
| Returns | `memory_id: String` |
| Contract | Store a T2-tier fact and immediately create about-edges in `xedge.db` linking the memory node to each symbol in `symbols`. Shorthand for `capture` + cross-edge creation in one atomic operation. |

---

**`memory.coverage`**

| Field | Value |
|---|---|
| Parameters | `scope_prefix: String?` |
| Returns | `{ total: u32, by_tier: Map<Tier, u32>, by_kind: Map<Kind, u32> }` |
| Contract | Return memory counts, optionally scoped to nodes whose scope starts with `scope_prefix`. Used by agents to assess coverage before triggering learning. |

---

### 2.3 — `knowledge.*` Domain (7 tools)

All knowledge tools read from and write to `knowledge.db`, except `knowledge.relate_code` and `knowledge.recall_about_code` which also touch `xedge.db` via the XedgeStore API.

---

**`knowledge.ingest`**

| Field | Value |
|---|---|
| Parameters | `doc: DocMeta` — `{ id, title, source_uri, content_type }`, `chunks: Vec<ChunkInput>` — `{ id, section, text, metadata? }` |
| Returns | `doc_id: String` |
| Contract | Ingest a document and its pre-chunked text into the knowledge graph; creates Doc, Section, and Chunk nodes; triggers FTS5 indexing and ANN embedding (if embedder present). Idempotent on `doc.id` — re-ingestion updates existing nodes. |

---

**`knowledge.write`**

| Field | Value |
|---|---|
| Parameters | `node: KnowledgeNodeInput` — `{ id?, class: Enum[doc, section, chunk, concept], label, body, metadata? }` |
| Returns | `node_id: String` |
| Contract | Upsert a single knowledge node; generates `node_id` if not provided; triggers FTS5 and ANN indexing. The primary escape hatch for creating nodes that don't originate from chunked document ingest. |

---

**`knowledge.relate`**

| Field | Value |
|---|---|
| Parameters | `src_id: String`, `tgt_id: String`, `rel: String` (typed relation label), `confidence: f64 = 1.0`, `provenance: String?` |
| Returns | `edge_id: String` or `isError` if either node is absent |
| Contract | Create a typed directed relation edge between two knowledge nodes. Fails closed (returns `isError`) if either `src_id` or `tgt_id` does not exist in `knowledge.db` — no dangling edges are permitted. |

---

**`knowledge.recall`**

| Field | Value |
|---|---|
| Parameters | `query: String`, `token_budget: u32?` |
| Returns | `Vec<KnowledgeResult>` — `{ node_id, class, label, body_snippet, score }` ranked by RRF-fused score |
| Contract | Hybrid recall over knowledge nodes: FTS5 BM25 + ANN cosine (if embedder present), fused by Reciprocal Rank Fusion; truncated to `token_budget` if specified. A recall that returns zero results on a non-trivial query is a coverage miss — tracked internally for gap hunting. |

---

**`knowledge.coverage`**

| Field | Value |
|---|---|
| Parameters | `class: Enum[doc, section, chunk, concept]?` |
| Returns | `{ total: u32, by_class: Map<Class, u32>, recall_miss_count: u32 }` |
| Contract | Return node counts optionally filtered to `class`, plus the running count of recall misses (zero-result recall calls) since last reset. Used by the gap-hunting skill to surface coverage gaps. |

---

**`knowledge.relate_code`**

| Field | Value |
|---|---|
| Parameters | `knowledge_id: String`, `symbol_ids: Vec<String>` |
| Returns | `{ xedge_count: u32 }` |
| Contract | Create about-edges in `xedge.db` linking a knowledge node to one or more estate symbols; verifies `knowledge_id` exists in `knowledge.db` before writing. Delegates all writes to XedgeStore. |

---

**`knowledge.recall_about_code`**

| Field | Value |
|---|---|
| Parameters | `symbol_ids: Vec<String>` |
| Returns | `Vec<KnowledgeResult>` — knowledge nodes linked to those symbols via about-edges |
| Contract | Reverse cross-store lookup: given estate symbol IDs, retrieve all knowledge nodes linked to them via `xedge.db`; useful for surfacing documentation and concepts relevant to a specific code location. |

---

### 2.4 — Tool Count Verification

| Domain | Unconditional | Conditional (fastembed or model2vec) |
|---|---|---|
| `estate.*` | 10 | +1 (SemanticSearch) |
| `memory.*` | 6 | — |
| `knowledge.*` | 7 | — |
| **Total** | **23** | **24** |

---

## §3 — MCP Resources (Bundled Skills)

The unified server exposes **6 skills** as MCP resources on the single server. Clients discover them via the standard MCP `resources/list` method. Resource URIs are stable across the consolidation.

| Resource URI | Origin | Description |
|---|---|---|
| `skill://codebase-expedition/SKILL.md` | wicked-memory | Hotspot-first codebase learning method: use `RankHotspots` to seed the walk, then `TraverseGraph` and `FetchContent` to build working memory systematically. |
| `skill://knowledge-ingest/SKILL.md` | wicked-knowledge | Structured document ingestion workflow: chunk, ingest, relate, verify coverage. |
| `skill://ontology-expedition/SKILL.md` | wicked-knowledge | Typed-relation pass: discover and record `rel` edges between knowledge nodes to build a coherent ontology. |
| `skill://knowledge-curation/SKILL.md` | wicked-knowledge | Dedup and collapse pass: surface near-duplicate nodes, merge with `knowledge.relate` supersedes edges, prune stale chunks. |
| `skill://cited-answer/SKILL.md` | wicked-knowledge | Grounded recall with citations: answer a question using `knowledge.recall` results; every claim must cite a `node_id`. |
| `skill://gap-hunting/SKILL.md` | wicked-knowledge | Turn recall misses into ingest tasks: poll `knowledge.coverage` for miss counts, surface the unanswered queries, produce a prioritised ingest work list. |

**Invariant:** Skill resource URIs MUST NOT change post-consolidation. Hosts that cached URIs from the pre-consolidation `wicked-knowledge-mcp` or `wicked-memory-mcp` servers will continue to resolve without reconfiguration.

> **Scheme note:** The `skill://` scheme is the canonical and frozen scheme used by the pre-consolidation binaries. URIs are preserved verbatim — no renaming in v0.13.0.

---

## §4 — MCP Prompts

The unified server exposes **1 MCP prompt**.

| Prompt name | Origin | Description |
|---|---|---|
| `expedition` | wicked-memory | Hotspot-first codebase learning method as a user-facing prompt: ranks hotspots, walks the graph, captures observations into memory. Equivalent in intent to `skill://codebase-expedition/SKILL.md` but surfaced as a prompt for direct invocation in MCP-prompt-aware clients. |

---

## §5 — Overlay / Federation Model

The `wicked-estate-overlay` crate provides `OverlayReader`, the cross-store federation layer. It holds a reference to the home `GraphStore` (estate) and a heterogeneous map of `ForeignEngine` trait objects keyed by engine ID.

### 5.1 — Method Classification

Methods on `OverlayReader` fall into three classes:

#### HOME-ONLY (17 methods)

These methods operate exclusively on the home estate graph. They MUST NOT expose or incorporate nodes from foreign engines (memory, knowledge). The home-only set covers:

- `search`, `search_semantic`, `search_by_kind`
- `rank` (PageRank), `rank_hotspots`
- `community` (Louvain)
- `lineage` (transitive deps)
- `blast_radius` (transitive dependents)
- `fetch_content`
- `rules_inventory`
- `context_bundle` (home graph portion only; cross-store enrichment is handled separately by `ContextBundle` tool logic)
- `retrieve_entity`, `list_entities`, `entity_count`
- `symbol_exists`, `kind_of`

The home-only invariant prevents agent confusion: a call to `lineage("MyStruct")` returns code symbols only, never memory nodes or knowledge concepts, even if cross-links exist.

#### FOLD methods (3 methods)

These methods union the home graph with foreign engine data via xedge about-edges:

| Method | Behaviour |
|---|---|
| `traverse` | BFS from a home symbol; at each node, if xedge about-edges exist, fetches related nodes from the corresponding foreign engine and includes them as annotated foreign entries in the result. |
| `traverse_multi` | Multi-source BFS; same fold logic as `traverse` but with multiple seed symbols. |
| `neighbors` | One-hop neighbor fetch from a home symbol; folds in foreign neighbours (memory nodes, knowledge nodes) linked via about-edges. |

Foreign entries in fold results are annotated with `{ engine_id, node_id, kind: "foreign" }` so consumers can distinguish home nodes from cross-store nodes without ambiguity.

#### ROUTE method (1 method)

| Method | Behaviour |
|---|---|
| `route` | Dispatches a symbol lookup to the home engine if the symbol prefix matches the estate namespace, or to the appropriate foreign engine if the symbol is a foreign-engine reference (e.g., `memory://...`, `knowledge://...`). Returns a unified `NodeRef` regardless of origin. |

### 5.2 — Epoch Validation

Cross-edges in `xedge.db` are stamped with the epoch of the estate symbol at the time the edge was created. The estate graph increments a symbol's epoch when the symbol is deleted and re-added (e.g., after re-extraction of a changed file).

When a FOLD method encounters an about-edge:

1. Read the edge's `symbol_epoch` from `xedge.db`.
2. Read the current epoch of the same symbol from `estate.db`.
3. If `current_epoch != edge_epoch`, the edge is **stale**. It is:
   - **Dropped** from the result (not surfaced to the caller).
   - **Logged** at WARN level with the symbol, stale epoch, and current epoch.
   - **Not deleted** by the reader (epoch cleanup is the responsibility of the `XedgeStore` maintenance routine).
4. If the symbol no longer exists in `estate.db`, the edge is also dropped and logged.

Fail-closed: stale edges never produce phantom cross-links.

### 5.3 — ForeignEngine Trait

```rust
/// Object-safe seam for heterogeneous engine map.
pub trait ForeignEngine: Send + Sync {
    fn engine_id(&self) -> &str;
    fn fetch_node(&self, node_id: &str) -> Result<Option<ForeignNode>>;
    fn fetch_nodes(&self, node_ids: &[&str]) -> Result<Vec<ForeignNode>>;
}
```

- `ForeignNode` carries `{ engine_id, node_id, label, body_snippet, kind }`.
- The trait is object-safe (no generics on methods, returns `Result<_>`).
- Implementations: `MemoryEngine` (reads `memory.db`) and `KnowledgeEngine` (reads `knowledge.db`).
- New foreign engines can be registered without touching `OverlayReader` logic.

### 5.4 — XedgeStore API (normative)

`XedgeStore` is the exclusive writer of `xedge.db`. Its public API (called by `memory.learn`, `memory.capture` with `about`, `knowledge.relate_code`, and the epoch-maintenance routine) is:

```rust
impl XedgeStore {
    /// Create about-edges. Returns the number of edges written.
    pub fn link(&self, symbol_ids: &[&str], foreign_engine: &str, node_id: &str, symbol_epoch: u64) -> Result<usize>;

    /// Remove all edges for a given foreign node.
    pub fn unlink_node(&self, foreign_engine: &str, node_id: &str) -> Result<usize>;

    /// Remove stale edges (epoch mismatch). Called by maintenance routine.
    pub fn sweep_stale(&self, current_epochs: &HashMap<String, u64>) -> Result<usize>;

    /// Reverse lookup: given symbol IDs, return all linked foreign nodes.
    pub fn about_symbol(&self, symbol_ids: &[&str]) -> Result<Vec<AboutEdge>>;

    /// Forward lookup: given a foreign node, return all linked symbols.
    pub fn symbols_for_node(&self, foreign_engine: &str, node_id: &str) -> Result<Vec<SymbolRef>>;
}
```

---

## §6 — Memory 5-Tier Model

The memory domain implements a five-tier model designed to mirror human cognitive memory: volatile working state, episodic events, distilled semantics, procedural patterns, and archival history.

### Tier Definitions

| Tier | Name | Scope | Durability | TTL | Intended content |
|---|---|---|---|---|---|
| T0 | Working | Session-scoped | Written to `memory.db` only when `WICKED_MEMORY_T0_PERSIST=1` is set; off by default. When unset, T0 is in-process only and is lost on restart. This is a deliberate tradeoff: T0 is designed for ephemeral session context, not durable recall. | Session lifetime | Active observations, draft hypotheses, scratch captures during a running session |
| T1 | Episodic | Per-project/scope | Persisted, medium | Days–weeks (tunable) | Specific events, decisions, and discoveries from past sessions; named by date and scope |
| T2 | Semantic | Per-project/scope | Persisted, permanent | No expiry | Distilled facts extracted by `memory.reflect`; the stable semantic layer of what the agent has learned |
| T3 | Procedural | Global / cross-project | Persisted, permanent | No expiry | How-to patterns, procedures, reusable recipes; extracted from repeated T1 patterns |
| T4 | Archival | Global | Persisted, compressed | No expiry; rarely recalled | Historical record; old T1/T2 demoted after long inactivity; restored on explicit recall only |

### Promotion Path

The consolidation agent (an autonomous agent, not a tool) drives tier promotion:

1. **T0 → T1**: After a session ends, surviving T0 captures with non-trivial recall weight are promoted to T1.
2. **T1 → T2**: T1 nodes that have been recalled repeatedly (above a configurable frequency threshold) and are older than a minimum age are promoted to T2 via `memory.reflect`. The distilled content replaces the raw episodic record.
3. **T1 → T3**: T1 nodes classified as `kind: procedure` that appear across multiple scopes are candidates for T3 promotion.
4. **T1/T2 → T4**: Nodes not recalled within the archival TTL window are demoted to T4 (compressed, flagged `archived: true`).

### Recall Behaviour by Tier

- `memory.recall` searches T0, T1, T2, T3 by default.
- T4 nodes are excluded from standard recall. Retrieval from T4 requires an explicit `tier: T4` filter.
- RRF fusion weights: T2 and T3 nodes receive a small score boost (configurable; default 1.1×) because they represent validated, stable knowledge.

---

## §7 — Workspace Crate Topology (Post-Consolidation)

The post-consolidation workspace adds four absorbed crates and reconciles one existing stub. All existing crates remain unchanged in their external API.

```
wicked-estate/                          # workspace root
  Cargo.toml
  crates/
    wicked-estate-core/                 # Symbol / Node / Edge / traits
                                        # Status: UNCHANGED
    wicked-estate-extract/              # tree-sitter extractors
                                        # Status: UNCHANGED
    wicked-estate-resolve/              # symbol resolvers
                                        # Status: UNCHANGED
    wicked-estate-store/                # GraphStore impls (SQLite WAL)
                                        # Status: UNCHANGED
    wicked-estate-rank/                 # PageRank + Louvain
                                        # Status: UNCHANGED
    wicked-estate-retrieve/             # all 11 estate.* tools
                                        # Status: UNCHANGED
    wicked-estate-mcp/                  # unified MCP server
                                        # Status: EXTENDED — routes all 24 tools
                                        #   (was: 11 estate tools only)
    wicked-estate/                      # binary entrypoint
                                        # Status: EXTENDED — default_embedder,
                                        #   open_async_store, unified server init
    wicked-estate-observe/              # OTLP telemetry
                                        # Status: UNCHANGED
    wicked-estate-memory-core/          # recall pipeline (rrf_fuse, budget_pack,
                                        #   RRF weight config)
                                        # Status: NEW — absorbed from wicked-memory
    wicked-estate-memory/               # MemoryEngine + 6 memory.* tools
                                        # Status: NEW — absorbed from wicked-memory
                                        # NOTE: reconciles with existing
                                        #   wicked-estate-memory-api stub (see below)
    wicked-estate-knowledge/            # KnowledgeEngine + 7 knowledge.* tools
                                        # Status: NEW — absorbed from wicked-knowledge
    wicked-estate-overlay/              # XedgeStore + OverlayReader + ForeignEngine trait
                                        # Status: NEW — absorbed from wicked-overlay
    wicked-estate-bench/                # benchmark harness
                                        # Status: UNCHANGED
```

### Reconciliation: `wicked-estate-memory-api` stub

The workspace currently contains `wicked-estate-memory-api` as a placeholder crate exposing trait definitions for the memory domain. After consolidation:

- The `MemoryEngine` trait and all shared types from `wicked-estate-memory-api` are migrated into `wicked-estate-memory-core` (the canonical home for shared memory types).
- `wicked-estate-memory-api` is either:
  - **Kept as a re-export shim** (`pub use wicked_estate_memory_core::*;`) for backward compatibility with any downstream crates that depend on it, **or**
  - **Removed** if no external dependents exist.
- The decision is deferred to the implementation phase but MUST be resolved before the first unified release. The default preference is the re-export shim to avoid a breaking change.

### Dependency Graph (simplified)

```
wicked-estate-core
  ↑ used by: wicked-estate-extract, wicked-estate-resolve,
             wicked-estate-store, wicked-estate-rank,
             wicked-estate-retrieve, wicked-estate-memory-core,
             wicked-estate-memory, wicked-estate-knowledge,
             wicked-estate-overlay

wicked-estate-store
  ↑ used by: wicked-estate-retrieve, wicked-estate-rank,
             wicked-estate-overlay

wicked-estate-memory-core
  ↑ used by: wicked-estate-memory

wicked-estate-retrieve, wicked-estate-memory, wicked-estate-knowledge, wicked-estate-overlay
  ↑ used by: wicked-estate-mcp

wicked-estate-mcp
  ↑ used by: wicked-estate (binary)
```

No circular crate dependencies are permitted.

---

## §8 — Event Bus Integration

### The Unified Server Does Not Emit Bus Events

The unified `wicked-estate` server (estate tools, memory tools, knowledge tools) is a **synchronous MCP server**. Tool calls are request/response; there is no internal event-emission path. The server MUST NOT depend on `wicked-bus` at the crate level.

### Bus Events Are Emitted by Consumers

External consumers of the unified server emit `wicked-bus` events for cross-product coordination:

| Consumer | Example bus event | Trigger |
|---|---|---|
| `wicked-crew` | `crew.context-bundle.completed` | After `ContextBundle` result is used to assemble agent context |
| `wicked-garden` | `garden.knowledge-ingested` | After a document ingest cycle driven by garden orchestration |
| `wicked-signals` | `signals.hotspot-ranked` | After a hotspot ranking run driven by signals |

This design preserves the clean separation: estate/memory/knowledge are **data stores**, not event producers. Event semantics live in the orchestration layer.

### Rationale

- Data stores emitting events would create tight coupling between store writes and bus topology.
- The MCP tool surface is the public API; bus events are a coordination concern of the orchestration layer.
- Single-writer constraint on each database is easier to reason about without an internal event pipeline.

---

## §9 — Migration Contract

This section is **normative** for all consumers migrating from `wicked-memory-mcp` or `wicked-knowledge-mcp` to the unified `wicked-estate` server.

### 9.1 — Tool Names: UNCHANGED

All 24 tool names are identical in the unified server. No tool was renamed. Clients that call tools by name require no code changes.

| Pre-consolidation server | Tool names |
|---|---|
| `wicked-estate-mcp` (v1) | `SearchEntity`, ..., `SemanticSearch` (11 tools — bare names, no prefix) |
| `wicked-memory-mcp` | `memory.capture`, ..., `memory.coverage` (6 tools) |
| `wicked-knowledge-mcp` | `knowledge.ingest`, ..., `knowledge.recall_about_code` (7 tools) |
| **Unified `wicked-estate`** | All 24 tools — names and signatures identical |

**Normative name policy:** Estate tools retain their existing bare names (no namespace prefix added). Memory and knowledge tools retain their `memory.` and `knowledge.` prefixes respectively. This asymmetry reflects the pre-consolidation tool registrations and is not changed by consolidation.

### 9.2 — Environment Variables: UNCHANGED

All four env vars are honoured by the unified server with the same semantics as their pre-consolidation counterparts.

| Env Var | Pre-consolidation owner | Post-consolidation owner |
|---|---|---|
| `$WICKED_ESTATE_DB` | `wicked-estate-mcp` | `wicked-estate` (unified) |
| `$WICKED_MEMORY_DB` | `wicked-memory-mcp` | `wicked-estate` (unified) |
| `$WICKED_KNOWLEDGE_DB` | `wicked-knowledge-mcp` | `wicked-estate` (unified) |
| `$WICKED_XEDGE_DB` | `wicked-overlay` | `wicked-estate` (unified) |

Operators who currently set these vars for separate processes need only point them at the same database files when starting the unified server. The database paths themselves do not change.

### 9.3 — Existing Database Files: COMPATIBLE

The unified server opens all four databases with the same schema versions as their pre-consolidation counterparts. **No schema migration is required** as a result of the consolidation itself.

Schema migrations driven by feature additions (not consolidation) follow the normal versioned migration process and are out of scope for this document.

### 9.4 — MCP Server Config: Minimal Change

The only required change to MCP host configuration is the **server command**:

| Before | After |
|---|---|
| `command: wicked-memory-mcp` | `command: wicked-estate-mcp` |
| `command: wicked-knowledge-mcp` | `command: wicked-estate-mcp` |
| `command: wicked-estate-mcp` (if separately configured) | `command: wicked-estate-mcp` (unchanged) |

A single `wicked-estate-mcp` server entry in the host config replaces the three separate server entries. All other config (env vars, args) is unchanged.

**If all three servers were already configured separately:** replace all three entries with one unified entry. Do not run the unified server three times.

### 9.5 — Three-Server Migration

Users who had all three servers configured (`wicked-estate-mcp`, `wicked-memory-mcp`, `wicked-knowledge-mcp`) replace three MCP server entries with one. All four env vars (`$WICKED_ESTATE_DB`, `$WICKED_MEMORY_DB`, `$WICKED_KNOWLEDGE_DB`, `$WICKED_XEDGE_DB`) are passed to the single `wicked-estate-mcp` server entry. No data migration required — all four database files are read as-is by the unified server.

### 9.6 — Skill Resource URIs: UNCHANGED

All 6 skill resource URIs are preserved verbatim. Hosts that cached `skill://codebase-expedition/SKILL.md` or any other skill URI will resolve them without change from the unified server.

### 9.7 — Migration Checklist

For operators migrating from pre-consolidation to unified:

- [ ] Build and install `wicked-estate` binary (replaces `wicked-memory-mcp`, `wicked-knowledge-mcp`, and the old `wicked-estate-mcp` binary).
- [ ] Update MCP host config: replace separate server entries with a single `wicked-estate` entry.
- [ ] Verify all four env vars (`$WICKED_ESTATE_DB`, `$WICKED_MEMORY_DB`, `$WICKED_KNOWLEDGE_DB`, `$WICKED_XEDGE_DB`) are set for the unified server process.
- [ ] Confirm existing database files are accessible at the configured paths.
- [ ] Restart the MCP host.
- [ ] Smoke-test: call one tool from each domain (`SearchEntity`, `memory.coverage`, `knowledge.coverage`) and confirm successful responses.

---

*End of REQ-003 — Domain Model*

*Document maintained under `.product/`. Changes require a reviewed PR; bump `version` on each revision.*
