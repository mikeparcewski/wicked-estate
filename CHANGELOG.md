# Changelog

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
