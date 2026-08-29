# Changelog

## [Unreleased]

### Changed
- **Type-nested definition identity (symbol-id scheme 2, ADR-002 amendment).** Definition SymbolIds now nest under the contiguous run of enclosing Type-suffixed definitions: `Repo.save` renders `…/Repo#save().` instead of `…/save().`. Previously every same-named definition in one module minted the SAME id and the store's `ON CONFLICT(symbol)` upsert collapsed them into one node — `this.save()` then "resolved" into the merged node at 0.65 (adversarial-review findings D03-1/D03-2/PER-7). Function-local and object-literal definitions deliberately stay module-flat (the chain truncates at any Method/Function/Term container; documented residuals are fixture-pinned). **Migration:** a previously-indexed DB is fully re-extracted on the next `index` of each repo (new per-repo `id_scheme` meta key; loud stderr line; `--force` equivalent). The key is written only after the re-extraction completes, so an interrupted migration re-fires the gate. On collision-heavy files, Calls edges drop and unresolved counts rise — the removed edges are the 0.65 false-precision merges, now honestly parked. NOT carried over: annotations on churned ids (orphaned), overlay/memory xedges (epoch-dropped), embeddings (re-run `--embeddings`), and SCIP edges — re-run `wicked-estate scip <root>` after the migration. Do not run pre-scheme binaries of the same version against a migrated DB (re-index `--force` if one did).

### Added
- **Many repos in ONE graph — `wicked-estate index <path> --db <f> --repo <name>`** (alias `--as`). A labelled run namespaces every path it stores as `<name>/…`, which is what makes `files.path`, `nodes.file` and the path-embedded SymbolIds unique per repo; it scopes the delete-sweep and the resolver's candidate set to that repo, and records provenance under `repo:<name>:commit|branch|remote|dirty` instead of the singular `repo_*` keys (which no longer clobber). `stats` reports every repo with its own file count and git state. No schema migration. Omit `--repo` and behaviour is unchanged — proved by `tests/multi_repo.rs::unlabelled_indexing_is_unchanged`. **Co-location, not linkage: edges do not resolve across repos.** `wicked-estate scip` takes the same `--repo` (SCIP documents are repo-relative; against a multi-repo graph an un-scoped ingest correlated nothing and reported `0 precise edges` — it now refuses).
- **`WICKED_RUNTIME` profile seam (foundation team profile):** one switch flips the foundation between `local` (zero-infra SQLite, the default) and `team` (self-hosted shared Postgres via `WICKED_STORE_URL`). New `resolve_store_spec`/`resolve_store_spec_from` in wicked-estate-store (priority: explicit `--db` > team profile > `WICKED_ESTATE_DB` > default; unknown profiles and team-without-postgres-URL fail loud). Wired into the `wicked-estate` CLI (new opt-in `postgres` feature builds the factory arm in) and `wicked-estate-mcp` (which fails loud under team — its async graph path and memory/knowledge stores are SQLite-only today; named follow-up). `deploy/docker-compose.team.yml` + `docs/team-runtime.md` document bring-up and the honest coverage matrix. Functional gate: `tests/team_runtime.rs` runs profile resolution → factory → full GraphStore conformance against a real Postgres in the CI postgres job.

### Fixed
- **Silent destruction of the first repo when two were indexed into one `--db`.** `files.path` is a relative path and SymbolIds embed it, so a second repo sharing `src/index.ts` overwrote the first's rows and the delete-sweep removed the rest — no error, no warning, `query alpha` → 0 matches. Indexing now REFUSES, before writing anything, any run that would overwrite another repo's content: an un-labelled second repo, an un-labelled index into a labelled graph, a `--repo` label already bound to a different repo, or the same repo under a second label. Every refusal names the conflict and the fix. Repo identity is the git `origin` remote **plus the indexed root's position inside the work tree** (so a moved or re-cloned checkout is one repo, while two packages of one monorepo — one `origin`, both with `src/index.ts` — are two and each needs its own label), and the canonical root path outside git.
- **PostgresStore torn read (locked decision #8):** `begin_batch`/`commit_batch` now map to ONE real transaction at `READ COMMITTED` — previously they were no-ops (`transactional_batch: false` under `shared_writers: true`), so a concurrent reader could observe a partially-written graph batch. All statements issued while a batch is open (reads included) ride the batch transaction, preserving read-your-own-writes for the resolver's mid-batch `SymbolIndex` lookups. `StoreCapabilities.transactional_batch` is now `true`. New deterministic two-connection concurrency test (`postgres_batch_commits_atomically_no_torn_reads`) proves a mid-batch reader sees old-or-new, never partial.
- Postgres conformance cleanup now also drops `symbol_gen`, so re-runs against the same database no longer inherit sticky `had_node` markers that skew epoch assertions.

## [0.14.2] — 2026-07-30

### Added
- `blast-radius --json` — machine-readable impact: target, dependents (id/name/kind/file/1-based line), and the honesty contract's unresolved-call count. JSON mode suppresses staleness/version notices so stdout is exactly one document.
- `graph-view --focus <symbol>` — seed the connected slice at a named symbol (exact-name match, up to the display limit); `--focus` without a value errors instead of silently no-oping.

## [0.14.1] — 2026-07-29

### Fixed
- `graph-view` returns a CONNECTED slice: seed with the top-ranked core, expand breadth-first along Calls/Imports edges (same filters, one 6-expansion budget per frontier node), backfill from the ranking — a plain top-N-by-PageRank slice rendered as scattered islands (observed: 51 nodes / 23 edges; now 50 / 63). `--limit 0` returns an empty slice instead of panicking.

## [0.14.0] — 2026-07-27

### Added
- `graph-view` CLI subcommand — symbol-level code graph rendered via the estate service
- SLC-001 store connection-lifecycle integration test (WAL mode + clean drop), hardened through three review rounds

### Changed
- DoD criteria checked off against existing evidence artifacts (docs); site astro 6 → 7

## [0.13.0] — 2026-07-06

### Added
- Unified MCP server: single binary exposes 23 tools across estate, memory, and knowledge domains
- Absorbs wicked-memory (6 tools: memory.capture/recall/reflect/erase/learn/coverage)
- Absorbs wicked-knowledge (7 tools: knowledge.ingest/write/relate/recall/coverage/relate_code/recall_about_code)
- Absorbs wicked-overlay (XedgeStore cross-engine search layer)
- wicked-estate-memory-core crate: MemoryApi trait, CaptureRequest, RecallQuery types
- wicked-estate-memory-api shim crate for clean re-exports
- SC-009 integration test: all 4 v0.12.x fixture DBs open in unified server
- Schema conformance tests (CONF-*) against frozen v0.12.x golden schemas
- ENV-001..006 subprocess environment variable tests
- 6 skill bundle resources accessible via MCP resources/list
- expedition prompt via MCP prompts/get

### Changed
- Workspace version: 0.12.0 → 0.13.0
- MCP server tool count: 10 estate tools + 6 memory + 7 knowledge = 23 total
- Default DB path: `.wicked-estate/graph.db` (relative to CWD)

### Deprecated
- wicked-memory: absorbed into wicked-estate; repository will be archived
- wicked-knowledge: absorbed into wicked-estate; repository will be archived
- wicked-overlay: absorbed into wicked-estate; repository will be archived

## [0.12.0] — 2025-12-01

Initial public release with 10 estate tools via MCP.
