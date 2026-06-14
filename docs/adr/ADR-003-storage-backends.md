# ADR-003 — Storage Backends & External-Database Readiness

**Status:** Accepted (design); SQLite built, external backends designed-not-built · **Date:** 2026-06-12
**Implements:** `crates/wicked-estate-core/src/traits.rs` (`GraphRead`/`GraphWrite`/`GraphStore`/`StoreCapabilities`),
`crates/wicked-estate-store/src/lib.rs` (`StoreBackend`, `open_store`).

## Context

The default deployment is local-first SQLite (research/09). But the engine must also be able to
target an **external / shared database** — a team-wide Postgres, a SurrealDB server, a managed
graph DB — *without* a redesign that touches extractors, resolvers, rankers, or tools. This ADR
fixes the seam so that work is a backend module, not surgery. **No external backend is built here**;
this is the design + the trait shape that makes building one later a localized change.

## Decision

### 1. Storage lives behind three traits (built)
- **`GraphRead`** — `capabilities`, `get_node`, `find_symbols`, `neighbors`, `traverse`, `stats`.
- **`GraphWrite`** — `begin_batch`, `commit_batch`, `upsert_nodes`, `upsert_edges`.
- **`GraphStore: GraphRead + GraphWrite + Send`** — auto-implemented; the common local case.

**Why split read from write?** An external DB scales **many readers** (MCP handlers, agents,
rankers) independently of a **single writer** (the indexer). They want different connection types
(a pool / read replica vs. one writer) and different sharing guarantees. Rankers/tools take
`&dyn GraphRead`, so they structurally cannot mutate the graph. Trait upcasting (`&dyn GraphStore`
→ `&dyn GraphRead`, stable Rust ≥1.86) lets the local single-object case still feed read-only APIs.

### 2. One factory seam (built)
`wicked_estate_store::open_store(spec) -> Result<Box<dyn GraphStore>>` parses a connection spec
(`:memory:`, `sqlite://path`, bare path, `postgres://…`, `surrealdb://…`). Every entrypoint
(CLI, MCP, bench, indexer) opens its store here. Adding a backend = **one match arm + one module**;
zero caller changes. Postgres/SurrealDB currently return a clear `Error::Invalid("…designed…not built")`.

### 3. Capability negotiation (built)
`GraphRead::capabilities() -> StoreCapabilities { full_text_search, vector_search,
server_side_traversal, transactional_batch, shared_writers }`. Retrieval (W5) **adapts**: if a
backend lacks `vector_search`, RRF fusion falls back to client-side; if it lacks
`server_side_traversal`, traversal degrades to chunked `neighbors` calls. SQLite reports
`server_side_traversal=true, shared_writers=false`; a Postgres/server backend would report
`shared_writers=true`.

### 4. Sync core now; async is a documented escape hatch (decision)
The traits are **synchronous**. Justification: indexing is offline/batch, queries are **bounded
single-round-trip** (CTE / server-side `traverse`), and the MCP server's concurrency is modest.
A remote backend therefore uses a **connection pool + bulk batching + a blocking driver** (e.g.
sync `postgres`, or `block_on` against a shared runtime) behind the sync trait — the network cost
is round-trip *count*, which our design already minimizes, not async ceremony.
**If** a high-concurrency hosted server ever demands it, the migration is mechanical and contained:
make `GraphRead`/`GraphWrite` `#[async_trait]`, add a runtime to the CLI/MCP entrypoints; callers
already go through `open_store` + the trait, so the blast radius is the trait defs + impls, not the
domain logic. We do **not** pay that cost speculatively (a Universal Don't: no over-engineering).

### 5. Concurrency model (design)
`GraphStore: Send` (not `Sync`) — because a single-connection SQLite store is `!Sync`. Shared
deployments wrap appropriately: the embedded store behind `Arc<Mutex<_>>` for a multi-handler MCP
server; an external store as a **pool-backed handle that is itself `Sync`** (the pool is `Sync`),
handing each reader its own connection. `shared_writers` advertises whether concurrent writers are
safe. A `SharedGraphRead: GraphRead + Sync` marker can be added when the server backend lands.

### 6. Network robustness (design — partially pre-paid)
- **Idempotency:** upserts already key on `Edge::dedup_key` / `Symbol` PK with higher-confidence-wins
  merge, so a retried batch is safe. This carries directly to `ON CONFLICT` over Postgres.
- **Batching:** `begin_batch`/`commit_batch` map to a transaction; remote backends MUST bulk
  upsert (multi-row `INSERT … ON CONFLICT` / `COPY`), never per-row round-trips.
- **Retry/timeout:** the remote impl owns retry-with-backoff + timeouts internally; the trait
  surface is unchanged.
- **Schema versioning:** a shared DB needs a `schema_version` row + forward migrations; the SQLite
  store embeds `schema.sql` today, a Postgres store adds a migration runner (its concern).

## Backend matrix

| Backend | Spec | Status | Notes |
|---|---|---|---|
| SQLite (+FTS5/sqlite-vec) | `sqlite://path`, `:memory:` | **Built** (W1.1) | local-first default; `shared_writers=false` |
| SurrealDB (embedded/server) | `surrealdb://…` | Designed; bake-off | W1.5 challenger (research/09 caveats: BSL license, embedded perf, build time) |
| Postgres (+ pgvector, recursive CTE) | `postgres://…` | **Designed, not built** | team/shared external DB; `shared_writers=true`; pool-backed `Sync` reader |
| Generic graph DB (Neo4j/…) | `bolt://…` | Out of scope unless needed | native traversal; only if a use case demands it |

## Consequences

- Adding Postgres later: implement `GraphRead`+`GraphWrite` for `PgStore` (pool + bulk upsert +
  recursive CTE + pgvector), register one arm in `open_store`. No change to `wicked-estate-extract`,
  `wicked-estate-resolve`, `wicked-estate-rank`, `wicked-estate-retrieve`, `wicked-estate-mcp`, or the pipeline.
- The conformance suite (`wicked_estate_core::conformance::graph_store_suite`) is the acceptance gate for any
  new backend — it must pass identically, proving behavioral parity including the edge-direction
  invariant and bounded reverse-reachability.
- Retrieval must always consult `capabilities()` rather than assuming SQLite features.
