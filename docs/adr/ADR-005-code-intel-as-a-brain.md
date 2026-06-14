# ADR-005 — wicked_estate as a Brain: Content Store, Query Cache, Cross-Graph

**Status:** Accepted (design); mostly not built — build path is Waves W5 (partial) + W11/W12 · **Date:** 2026-06-12
**Relates to:** ADR-001 (schema), ADR-003 (storage seam / external DB), ADR-004 (infra estate).

## Context

The graph already gives precise structural code intelligence. The next ambition: make wicked_estate a
**brain** — store the actual *content*, *cache* expensive results, and run *cross-graph* queries
(across repos / domains) so agents can do "crazy" multi-hop, multi-repo, semantic + structural
queries. The question: does this fit, or is it a rewrite? **It fits** — it's additive tables +
new tools behind the existing `GraphStore`/`GraphRead`/`RetrievalTool` seams. No core rewrite.

## Decision (the four pillars + what's already in place)

### 1. Content store (graph + text + vectors = a code brain)
Store the actual source *content* alongside graph nodes, plus arbitrary content (docs, decisions,
memories) as nodes — a graph + content hybrid.
- **Already in place:** nodes carry `signature` + `doc` + a JSON `metadata` blob; file nodes exist;
  `NodeKind::Synthetic`/`Other` + the drop-in extractor SDK (W6.1) admit non-code/content nodes.
- **Add:** a `content` table (source text per node/span, content-addressed), **FTS5** over it (W5.1),
  and an optional **embedding sidecar** (sqlite-vec, W5.2). Then **RRF hybrid retrieval is already
  built** (`wicked-estate-retrieve::reciprocal_rank_fusion`) → fuse graph + BM25 + vector. That *is* a code brain.

### 2. Versioned query cache (make crazy queries fast)
Expensive results (transitive closures, PageRank, blast-radius, cross-graph joins) get cached.
- **A versioned cache port:** a `query_cache` table keyed by
  `(query_hash, graph_version)`; entries auto-rejected on `producer_version` mismatch; **ancestor-chain
  invalidation** so aggregates bust when descendants change.
- **Materialized analytics** refreshed at index time: PageRank (built, `wicked-estate-rank`), hotspots,
  co-change, transitive call closures — stored, not recomputed per query.
- **Add:** the cache table + a `Cache` trait (sits beside `GraphStore`); analytics materialization step.

### 3. Cross-graph / multi-repo (the "brain spans repos")
One brain over many repos/graphs, queried together.
- **Already in place:** `SymbolId` carries **package coordinates** (manager/name/version) — so the
  *same exported symbol* is identifiable across repos. The **external-DB seam (ADR-003)** lets a
  shared team brain live in Postgres. Stable identity (ADR-002) makes cross-repo matching sound.
- **Add:** a `graphs` registry + a `graph_id` dimension on nodes/edges (or federated `ATTACH` of
  per-repo SQLite dbs); a **cross-graph query API** that joins by package identity. Enables:
  *cross-repo blast-radius* ("change service A's API → which other repos break?"), *"who implements
  this interface org-wide"*, *service-to-service call maps* (a cross-repo code-graph model + a
  graph-algebra of set operations over node/edge sets).

### 4. Brain query surface (new tools over MCP)
New `RetrievalTool`s on the existing MCP server: `SemanticSearch` (vector), `CrossGraphQuery`,
`FetchContent`, `Lineage`, and a **graph-algebra-lite** API (union/intersect/difference/reachable
over node & edge sets, scoped to what's useful). All ride the trait surface that's
already wired into `wicked-estate-mcp`.

### "Other fun stuff" this unlocks
- **Temporal graph:** version the graph over commits → "how did the call
  graph change", time-travel blast-radius. Stable IDs (ADR-002) make this tractable.
- **Join code ↔ infra estate** (ADR-004): code → deploys → infra resources in one graph →
  "this code change touches which live cloud resources?" lineage.
- **Memories:** decisions/learnings as first-class nodes linked to code.

## Why it fits (no rewrite)
Everything is additive behind existing seams: content/cache/graph_id are **more tables**;
cross-graph is **identity matching** on data we already store; new capabilities are **more
`RetrievalTool`s**; scale-out is the **external-DB backend (ADR-003)**. The `GraphStore` trait,
the RRF fusion, the MCP surface, the stable-ID + package-coordinate identity, and the extensible
node/edge model were all chosen with this in mind.

## Build path
- **W5** (already planned): FTS5 + embedding sidecar + RRF → the content+hybrid foundation.
- **W11 — Brain core:** content store, versioned query cache (`versioned cache-port` pattern),
  materialized analytics. 
- **W12 — Cross-graph:** `graphs` registry + `graph_id`/federation + cross-graph query API +
  the brain `RetrievalTool`s + graph-algebra-lite.
- Temporal + code↔infra join + memories: follow-ons once W11/W12 land.

## Consequences
- wicked_estate becomes a **local-first code brain** (graph + content + cache + semantic) that scales
  to a **shared team brain** (Postgres backend) and **org-wide cross-repo** intelligence — without
  changing the core engine. This is the through-line of the design: graph + content + cache +
  cross-graph + semantic, unified behind the existing seams.
