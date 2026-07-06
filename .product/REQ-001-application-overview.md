---
name: REQ-001-application-overview
title: wicked-estate Unified Foundation — Application Overview
status: revised
version: 0.2
date: 2026-07-05
author: mike.parcewski@gmail.com
review-required: true
---

# REQ-001 — Application Overview

## Purpose

wicked-estate Unified Foundation is the consolidation of three standalone Rust MCP stdio servers — wicked-memory, wicked-knowledge, and wicked-overlay — into the existing wicked-estate Cargo workspace, producing a single MCP stdio binary (`wicked-estate-mcp`) that exposes 23 MCP tools in the default build (24 when the `fastembed` or `model2vec` embedder feature is enabled) from all three products under their existing namespaces, with all existing tool names, env vars, and behavioral contracts preserved.

The motivation is the building-blocks argument: MCP stdio is the universal agent context interface. Every major agentic CLI — Claude Code, Claude Desktop, Gemini CLI, Cursor, GitHub Copilot, and custom Python or TypeScript agent loops — speaks JSON-RPC 2.0 over stdio. The current ecosystem forces any agent that needs code intelligence, persistent memory, and curated knowledge to register three separate MCP server entries, manage three separate installs, and coordinate across three separate processes. This is operational friction with no architectural benefit: the three products are already tightly coupled (wicked-memory and wicked-knowledge both depend on wicked-estate-core and wicked-estate-store; wicked-overlay bridges all three), so the notion that they are independent deployable units is fiction enforced by packaging, not by design.

Unifying into a single binary eliminates that friction: one `cargo install wicked-estate`, one MCP server block in every host config, one process with one startup cost, and a single semantic result surface where the overlay's OverlayReader can fold code graph, memory, and knowledge results together at query time. The unified binary still enforces store isolation (DEC-1): each domain writes to its own SQLite file (estate.db for code, memory.db for memories, knowledge.db for curated nodes, xedge.db for cross-store edges), so no domain's write path can corrupt another's and single-writer enforcement remains per-file. Isolation is a data safety property, not a deployment property; unification does not require violating it.

After consolidation, wicked-memory, wicked-knowledge, and wicked-overlay are archived on GitHub. wicked-estate becomes the single Rust foundation for the entire wicked-* ecosystem: the substrate that every skill layer, every agent framework integration, and every agentic CLI reaches for when it needs code intelligence, persistent memory, or structured knowledge.

---

## Core User Flows

### Flow 1 — Developer adds wicked-estate to an MCP host and gets all three domains from one server

1. Developer runs `cargo install wicked-estate` (or downloads a prebuilt binary). This installs a single binary: `wicked-estate-mcp`.
2. Developer adds one MCP server block to their host config. For Claude Code this is `.claude/settings.json`; for Claude Desktop it is `claude_desktop_config.json`; for Gemini CLI it is the `mcpServers` block in the agent config; for Cursor it is the MCP extension config:
   ```json
   {
     "mcpServers": {
       "wicked-estate": {
         "command": "wicked-estate-mcp",
         "env": {
           "WICKED_ESTATE_DB": "/path/to/repo/.wicked/estate.db",
           "WICKED_MEMORY_DB": "/path/to/repo/.wicked/memory.db",
           "WICKED_KNOWLEDGE_DB": "/path/to/repo/.wicked/knowledge.db",
           "WICKED_XEDGE_DB": "/path/to/repo/.wicked/xedge.db"
         }
       }
     }
   }
   ```
3. The MCP host starts `wicked-estate-mcp`. The binary opens all four stores at startup (creating any that do not yet exist), registers all 23 tools (24 when the `fastembed` or `model2vec` feature is compiled in) and all bundled skills as MCP resources, and begins reading JSON-RPC 2.0 requests from stdin.
4. The host's `tools/list` response contains 23 tools in the default build (24 when the `fastembed` or `model2vec` feature is compiled in): the 10 unconditional estate tools (`SearchEntity`, `RetrieveEntity`, `TraverseGraph`, `BlastRadius`, `FetchContent`, `ContextBundle`, `RulesInventory`, `RankHotspots`, `Communities`, `Lineage`) plus `SemanticSearch` when the embedder feature is compiled in, 6 memory tools (`memory.capture`, `memory.recall`, `memory.reflect`, `memory.erase`, `memory.learn`, `memory.coverage`), and 7 knowledge tools (`knowledge.ingest`, `knowledge.write`, `knowledge.relate`, `knowledge.recall`, `knowledge.coverage`, `knowledge.relate_code`, `knowledge.recall_about_code`).
5. Developer indexes their repo: `wicked-estate index --db /path/to/repo/.wicked/estate.db /path/to/repo`. The code graph is ready. Memory and knowledge stores are empty and ready for agent writes.
6. Developer queries from within the host: `SearchEntity` for symbol lookup, `memory.recall` for cross-session context, `knowledge.recall` for curated documentation. All three return in a single server round-trip.

**No migration required for existing wicked-estate users:** the `wicked-estate-mcp` binary continues to respond to all 10+1 existing estate tools exactly as before. The 13 new tools are additive. If `WICKED_MEMORY_DB` is not set, the memory tools still open `$WICKED_HOME/memory.db` (defaulting to `~/.wicked/memory.db`) by default and work normally.

---

### Flow 2 — Agent captures memory during a session and later recalls it, including knowledge linked to estate symbols

1. During an active crew or agentic session, the agent calls `memory.capture` with a content string and a scope (e.g., `"scope": "project:wicked-estate"`). The call is dispatched to the `MemoryEngine` inside `wicked-estate-mcp`. The engine writes the memory node to `memory.db` using the same `wicked-estate-store` SQLite pool infrastructure as the code graph store. The write is serialized through `Arc<Mutex<Connection>>` — no concurrent writes to `memory.db`.
2. In a subsequent session (a new process invocation, a different day), the agent calls `memory.recall` with a query string and the same scope. The `MemoryEngine` runs RRF-fused retrieval: FTS5 keyword recall plus optional vector ANN recall (if the `model2vec` or `fastembed` feature was compiled in). Results are ranked and returned as a JSON array of memory candidates with their content, score, and scope.
3. Cross-tool recall: the agent follows up with `knowledge.recall_about_code`, providing a code symbol as the seed (e.g., `"symbol": "MemoryEngine"`). The `wicked-knowledge` handler uses `OverlayReader` to traverse the xedge.db cross-store edges: it finds knowledge nodes that have been related to that symbol via a prior `knowledge.relate_code` call, and returns those knowledge nodes ranked by edge weight and recency. The estate code graph, the memory store, and the knowledge store are all queried within a single `wicked-estate-mcp` process — no inter-process RPC, no socket overhead.
4. The agent synthesizes the recall output: memory provides the session context ("last week we decided to use the RRF-fuse pipeline"), knowledge provides the design rationale linked to the symbol ("MemoryEngine uses Arc<Mutex<Connection>> for single-writer enforcement per DEC-1"), and the code graph provides the current implementation signature via `RetrieveEntity`. All three came from one MCP server.

---

### Flow 3 — Agent runs the codebase-expedition skill to learn an unfamiliar repository

1. The MCP host loads the bundled `codebase-expedition` skill from `wicked-estate-mcp`'s `resources/list` response. The skill is compiled into the binary at build time via `include_str!()` and served as an MCP resource — no separate file deployment required.
2. The agent reads the skill prompt from the `resources/read` response. The skill instructs the agent to: (a) call `SearchEntity` to locate the key symbols and modules; (b) call `TraverseGraph` to trace call chains from entry points; (c) call `BlastRadius` on the core modules to understand ripple risk; (d) call `ContextBundle` to build a focused context window around the most important symbols; (e) call `knowledge.ingest` to store curated findings as knowledge nodes in `knowledge.db`; and (f) call `memory.learn` to store the expedition summary as a persistent memory in `memory.db`.
3. The agent executes the skill against the new repository. All tool calls are dispatched to the same `wicked-estate-mcp` process: estate tools read from `estate.db`, knowledge writes go to `knowledge.db`, memory writes go to `memory.db`. Cross-store isolation is preserved: the expedition's write operations to knowledge and memory cannot interfere with the code graph.
4. After the expedition, any future agent session on the same repository can call `memory.recall` to retrieve the expedition summary and `knowledge.recall` to retrieve specific design findings, without re-running the full expedition. The knowledge nodes are also cross-linked to estate symbols via `knowledge.relate_code`, enabling `knowledge.recall_about_code` to surface design findings when the agent encounters a relevant symbol.

---

### Flow 4 — Knowledge ingestion workflow: ingest documentation, relate to code, recall during agent work

1. A developer or agent calls `knowledge.ingest` with a document body (architecture decision, API contract, design specification, or any text). The `wicked-knowledge` handler writes the document as a knowledge node in `knowledge.db` using the `Other("k*")` symbol kind on the estate graph spine. The node is FTS5-indexed for keyword recall and optionally vector-indexed for semantic recall.
2. The agent (or developer) calls `knowledge.relate_code` with the knowledge node's ID and a list of code symbol names (e.g., `["XedgeStore", "OverlayReader", "wicked_overlay::GraphRead"]`). The handler resolves the symbol names against `estate.db` using `wicked-estate-retrieve`, then writes directed `about` edges into `xedge.db` via `XedgeStore`. The single writer for `xedge.db` is enforced by `Arc<Mutex<Connection>>` inside the wicked-overlay integration layer.
3. During subsequent agent work, when the agent encounters the symbol `XedgeStore` in the code (e.g., via `RetrieveEntity`), it calls `knowledge.recall_about_code` with `"symbol": "XedgeStore"`. The `OverlayReader` traverses the xedge.db edges from the symbol node and returns the linked knowledge nodes: the architecture decision that motivated the XedgeStore design, the API contract it implements, any curation notes added by the `knowledge-curation` skill.
4. The agent can also call `knowledge.relate` to draw edges between two knowledge nodes (e.g., link an API contract to the architecture decision that produced it). These knowledge-to-knowledge edges are stored in `knowledge.db` directly and do not require xedge.db.
5. The `knowledge.coverage` tool returns a summary of what the knowledge store contains: node counts by class, FTS coverage, and xedge edge counts. This allows the agent or developer to audit the knowledge store's completeness before relying on it for a retrieval-augmented task.

**Bundled skills for this workflow:** The `knowledge-ingest` skill (prompt for structuring and ingesting a document), `knowledge-curation` skill (systematic review and enrichment of existing nodes), and `ontology-expedition` skill (graph-walking to discover gaps in the knowledge ontology) are all compiled into `wicked-estate-mcp` as MCP resources. No separate skill file deployment is required.

---

### Flow 5 — Cross-store overlay query: single semantic result surface across estate, memory, and knowledge

1. The agent calls `SearchEntity` with a query string (e.g., `"query": "single-writer enforcement"`). The estate handler returns code symbols matching the query from `estate.db` via FTS5 and/or vector ANN.
2. The agent calls `memory.recall` with the same query string. The memory handler returns memory nodes matching the query from `memory.db` via the RRF-fuse pipeline (FTS5 + optional vector ANN). Results include session context and learned patterns related to the query.
3. The agent calls `knowledge.recall` with the same query string. The knowledge handler returns knowledge nodes from `knowledge.db`, including curated design decisions, documentation excerpts, and ontology nodes.
4. Inside the `wicked-estate-mcp` process, the `OverlayReader` can be invoked directly (not yet exposed as a standalone tool in v0.1, but used internally by `knowledge.recall_about_code`) to union the home graph (estate) with foreign graphs (memory, knowledge) at read time via the `with_read_inline` seam — a no-deadlock pattern enforced by the estate store's pool design. This means `knowledge.recall_about_code` and `memory.recall` can optionally fold in estate graph neighbors of their seed nodes without the agent having to make separate calls.
5. The agent's context window now contains a unified result surface: symbols from the code graph, session memories, and curated knowledge nodes, all sourced from a single MCP server process. No client-side fan-out to multiple MCP servers is required. The agent can synthesize across all three result sets to produce a grounded response.

**v0.1 boundary:** In v0.1, the agent performs the fan-out (calling each tool separately). A future unified `OverlaySearch` tool that performs the fan-out server-side and returns a single merged result set is out of scope for v0.1 (see OQ-002).

---

### Flow 6 — Team deployment: multiple agents share a PostgreSQL-backed store (v0.2 path)

> This flow describes the target state for v0.2. It is included here to establish the architectural boundary so that v0.1 design decisions do not inadvertently foreclose the v0.2 path.

1. A team runs a shared `wicked-estate-mcp` process backed by a PostgreSQL database. The `WICKED_ESTATE_DB` env var points to a PostgreSQL connection string rather than a SQLite file path. The `StoreTrait` abstraction (already ADR'd as DEC-ECO-006 in the ecosystem) routes all reads and writes through the Postgres backend.
2. Multiple agents running concurrently on different machines connect to the same MCP server (or to separate server instances all pointing at the same Postgres database). Memory captured by agent A (e.g., a crew session that decided on the authentication strategy) is immediately available to agent B via `memory.recall`.
3. Knowledge nodes ingested by one agent (e.g., an automated docs-ingestion workflow) are queryable by all agents via `knowledge.recall` and `knowledge.recall_about_code`. The code graph indexed by the CI pipeline is shared across all agent instances.
4. Single-writer enforcement for Postgres uses `SELECT ... FOR UPDATE SKIP LOCKED` queue patterns (consistent with the wicked-bus Postgres backend design in DEC-ECO-005) rather than `Arc<Mutex<Connection>>`. The external interface (tool names, env vars, request/response schemas) is identical to the SQLite deployment.
5. xedge.db cross-store edges are also Postgres-backed in this mode. The `XedgeStore` abstraction routes through the `StoreTrait` Postgres implementation.

**v0.1 constraint:** The Postgres backend implementation is out of scope for v0.1. The `StoreTrait` abstraction must be designed into the v0.1 consolidation so that the Postgres path can be added in v0.2 without interface breakage. See OQ-001.

---

## What This Is NOT

- **Not a replacement for the wicked-brain skills layer.** wicked-brain is a Claude Code skill adapter that provides high-level agent orchestration patterns (context surfacing, session teardown, memory consolidation). After consolidation, wicked-brain skills may call `wicked-estate-mcp` tools directly, making wicked-brain a thin adapter over the unified foundation rather than a standalone memory system. wicked-brain's skill orchestration logic and its agent graph remain in the wicked-brain repository. The deprecation timeline for wicked-brain's own MCP server (currently separate) is an open question (OQ-002), not a v0.1 deliverable.
- **Not a wicked-brain replacement in v0.13.0.** wicked-brain (JS) operates independently and does not call wicked-memory-mcp or wicked-knowledge-mcp directly (confirmed by source audit). Archiving the standalone Rust server repos does not break wicked-brain.
- **Not a SaaS product.** wicked-estate has no cloud backend, no user accounts, no API keys for its own infrastructure. All stores are local SQLite files by default. The Postgres path (v0.2) is a self-hosted option, not a managed service.
- **Not a vector-only store.** The primary retrieval mechanism is FTS5 keyword search. Vector/ANN search via `SemanticSearch` (estate) and optional vector recall in `memory.recall` and `knowledge.recall` is an additive feature behind a compile-time feature flag (`model2vec` or `fastembed`). The binary works fully without any embedding model installed.
- **Not a replacement for wicked-understanding.** wicked-understanding is the skills-and-analysis layer (comprehension workflows, documentation generation, architectural analysis). It consumes wicked-estate tools but is not absorbed by this consolidation. The skills in wicked-understanding remain in their own repository and continue to call `wicked-estate-mcp` as an MCP server.
- **Not a graph database.** wicked-estate uses SQLite as its backing store with a graph schema (nodes, edges, FTS5 index, optional vector index). It is not a general-purpose graph database (Neo4j, DGraph, etc.) and is not a replacement for one. The graph model is specific to the code intelligence, memory, and knowledge domains.
- **Not an agent framework.** wicked-estate provides retrieval and persistence tools over MCP. It does not orchestrate agent loops, manage phase gates, or dispatch tasks. Agent orchestration is the responsibility of the skills layer (wicked-crew, wicked-brain) and the host CLI.
- **Not multi-writer.** The single-writer-per-store constraint is a hard design invariant, not a v0.1 limitation. Each SQLite file has exactly one write path in the process, enforced by `Arc<Mutex<Connection>>`. Concurrent multi-writer access to a single SQLite file is explicitly out of scope at all versions. The Postgres backend (v0.2) provides multi-process concurrency through `SKIP LOCKED` queue semantics, not by relaxing the single-writer invariant.

---

## Success Criteria

- [ ] **SC-001 — Backward compatibility for existing wicked-estate users**: All 10+1 existing estate tools (`SearchEntity`, `RetrieveEntity`, `TraverseGraph`, `BlastRadius`, `FetchContent`, `ContextBundle`, `RulesInventory`, `RankHotspots`, `Communities`, `Lineage`, `SemanticSearch` conditional) respond to the same JSON-RPC 2.0 request shapes they did before consolidation, measurable as: the wicked-estate existing integration test suite passes without modification against the consolidated binary.
- [ ] **SC-002a — Exactly 23 tools in the default feature build (no embedder)**: `tools/list` from a single `wicked-estate-mcp` process compiled without any embedder feature returns exactly 23 tool definitions (10 unconditional estate tools + 6 memory tools + 7 knowledge tools; `SemanticSearch` absent), measurable as: `echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | wicked-estate-mcp | jq '.result.tools | length'` returns `23` in CI against the default feature set.
- [ ] **SC-002b — Exactly 24 tools with `--features fastembed` or `--features model2vec`**: `tools/list` from a `wicked-estate-mcp` process compiled with either embedder feature returns exactly 24 tool definitions (all 23 above plus `SemanticSearch`), measurable as: the same command on a fastembed or model2vec build returns `24`.
- [ ] **SC-003 — Single binary install**: `cargo install wicked-estate` produces a single binary (`wicked-estate-mcp`) that passes `--help` and handles a `tools/list` request without any additional runtime dependencies beyond the OS standard library, measurable as: install + `echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | wicked-estate-mcp` returns a valid JSON-RPC response on macOS, Linux x86_64, and Linux arm64.
- [ ] **SC-004 — Store isolation preserved (DEC-1)**: Writing a memory node via `memory.capture` does not create any row in estate.db or knowledge.db, and writing a knowledge node via `knowledge.ingest` does not create any row in estate.db or memory.db, measurable as: after each write operation, a direct SQLite query on the non-target stores shows zero new rows (automated test).
- [ ] **SC-005 — Bundled skills available as MCP resources**: `resources/list` from `wicked-estate-mcp` returns at least 6 resource entries corresponding to the bundled skills (codebase-expedition from wicked-memory; knowledge-ingest, ontology-expedition, knowledge-curation, cited-answer, gap-hunting from wicked-knowledge), measurable as: `resources/list` response contains a JSON array with `length >= 6` and each entry has a `uri` and `name` field.
- [ ] **SC-006 — Migration path for existing wicked-memory and wicked-knowledge users**: A user who previously ran separate `wicked-memory-mcp` and `wicked-knowledge-mcp` binaries can switch to `wicked-estate-mcp` by: (a) pointing the same `WICKED_MEMORY_DB` env var at their existing `memory.db` file and the same `WICKED_KNOWLEDGE_DB` env var at their existing `knowledge.db` file, and (b) calling `memory.recall` and `knowledge.recall` against the existing data, receiving the same results as before, measurable as: a test fixture with pre-populated memory.db and knowledge.db files from wicked-memory v0.12.1 and wicked-knowledge v0.12.1 produces identical recall results when queried via the consolidated binary.
- [ ] **SC-007 — No regression to existing wicked-estate users' MCP configs**: An MCP host config block that previously pointed at `wicked-estate-mcp` from the pre-consolidation binary works unchanged with the post-consolidation binary, measurable as: CI starts the consolidated binary with an MCP config block containing only `WICKED_ESTATE_DB` (no `WICKED_MEMORY_DB` or `WICKED_KNOWLEDGE_DB`) and all 10+1 estate tools respond correctly. Memory and knowledge tools default to `$WICKED_HOME/memory.db` and `$WICKED_HOME/knowledge.db` (i.e., `~/.wicked/memory.db` and `~/.wicked/knowledge.db`) and do not error on startup.
- [ ] **SC-008 — StoreTrait / GraphStore abstraction in place for v0.2 Postgres path**: The `StoreTrait` / `GraphStore` abstraction is in place in v0.13.0 with a SQLite implementation such that adding a Postgres implementation in v0.2 requires no changes to tool handler call sites, measurable as: a design review or compilation test with a no-op Postgres stub demonstrates tool handlers depend only on the trait interface, not on `SqliteStore` directly.
- [ ] **SC-009 — Three-server migration: pre-consolidation database files work unchanged**: A user who had all three pre-consolidation servers (wicked-estate-mcp, wicked-memory-mcp, wicked-knowledge-mcp) configured with separate existing database files can replace all three MCP server entries with a single wicked-estate entry pointing all four env vars at those same files and receive correct non-error results from at least one tool in each domain, measurable as: an integration test fixture with pre-populated estate.db (v0.12.0 schema), memory.db (v0.12.1 schema), knowledge.db (v0.12.1 schema), and xedge.db (v0.12.0 schema) produces non-error responses from `SearchEntity`, `memory.recall`, and `knowledge.recall` respectively.

---

## Non-Functional Requirements

### Performance

- **Startup time**: The consolidated binary must initialize all four store connections and register all tools within 500ms on a cold start (no warm JVM or runtime), measured from process start to the first valid `tools/list` response on a modern developer laptop (M-series Mac or equivalent x86_64). This constraint is tighter than a typical server because MCP hosts restart the server on each session in some configurations.
- **Tool response p95**: All 24 tools must return a response within 200ms p95 for stores with fewer than 1 million nodes (code symbols + memory nodes + knowledge nodes combined), measured under single-threaded sequential request load. This is a retrieval constraint, not a write constraint; `knowledge.ingest` with large documents may take longer and is not subject to the 200ms p95 target.
- **Incremental estate indexing**: `wicked-estate index --incremental` on a repo delta of 100,000 lines must complete within 30 seconds. This is unchanged from the pre-consolidation wicked-estate NFR.

### Reliability

- **Single-writer enforcement**: Each SQLite file (`estate.db`, `memory.db`, `knowledge.db`, `xedge.db`) must have exactly one write path in the process. Concurrent write attempts from multiple Tokio tasks must be serialized by the `Arc<Mutex<Connection>>` pattern. A concurrent write test (two Tokio tasks both calling `memory.capture` simultaneously) must not produce a SQLITE_BUSY error or data corruption.
- **Graceful degradation on missing stores**: If `WICKED_MEMORY_DB` or `WICKED_KNOWLEDGE_DB` is not set and the default path file does not exist, the binary creates the file (with schema migrations applied) rather than failing to start. This preserves the behavior of the pre-consolidation binaries.
- **Clean shutdown**: On SIGTERM or stdin EOF, the binary flushes all pending writes, closes all store connections, and exits with code 0. No in-flight write is lost on clean shutdown.

### Platform Support

- The binary must compile and run on: macOS arm64 (M-series), macOS x86_64, Linux x86_64, Linux arm64, and Windows via Git Bash or WSL. Native PowerShell on Windows is a best-effort target for v0.1; full PowerShell support is a v0.2 requirement.
- `rusqlite` is bundled (the `bundled` feature), so there is no system SQLite dependency. No other system library is required for the default feature set.

### Protocol Compliance

- The binary must pass the MCP JSON-RPC 2.0 conformance checks for: `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read`, and `prompts/list` / `prompts/get`. Error responses must use the standard JSON-RPC error object shape with numeric code, string message, and optional data field.
- Tool schemas must be valid JSON Schema (draft 7 or later) with `type`, `properties`, and `required` fields. All required parameters must be present in the schema; no undocumented required parameters.

### Observability

- All tool dispatch errors (store errors, schema validation failures, unknown tool names) must be logged to stderr in a structured format (key=value pairs) with a timestamp and the tool name. They must not be silently swallowed.
- The binary must respond to `tools/call` for an unknown tool name with a JSON-RPC error response (code -32601, method not found) rather than exiting or blocking.

---

## Phase Placement Table

| Phase | Relevance |
|---|---|
| Clarify | Primary reference: defines what the Unified Foundation is, what user flows it must support, and what the success criteria are |
| Design | Scope boundary: the six flows and the "What This Is NOT" section drive architecture decisions — store isolation, tool namespace layout, binary entry point, and resource embedding |
| Build | SC-001 through SC-009 are the acceptance gates for the consolidation build; NFRs are the SLA targets for performance assertions |
| Test | The 3-agent acceptance pipeline runs scenarios derived from Flows 1–5 against SC-001 through SC-009; NFRs produce concrete timing assertions |
| Review | Adversarial reviewer checks this document against the implementation: are all 23/24 tools present (SC-002a/SC-002b), do the existing user flows still work, is store isolation preserved? |
| Ship | All SC-001 through SC-009 must be checked before the release gate is approved; wicked-memory and wicked-knowledge GitHub repos are archived after ship |

---

## Open Questions

| ID | Question | RAID Reference |
|---|---|---|
| OQ-001 | What is the timeline for the Postgres backend (v0.2)? The `StoreTrait` abstraction must be at least stubbed in v0.1 to avoid interface breakage in v0.2. Does v0.1 ship with the trait defined but only the SQLite implementation? | Open — file ASSM-00001 if deferring trait definition to v0.2 |
| OQ-002 | What is the deprecation path for wicked-brain's own MCP server after consolidation? wicked-brain currently runs its own memory/knowledge access layer. Does it migrate to calling `wicked-estate-mcp` tools directly, or does it continue as an independent server? | Open — align with wicked-brain maintainer; no RAID item yet |
| OQ-003 | Version unification strategy: does the consolidated workspace publish all crates at a single workspace version (e.g., v0.13.0 for all), or does each absorbed crate increment independently? A single workspace version simplifies release tooling but forces a major bump on wicked-memory and wicked-knowledge consumers. | Open — recommendation is single workspace version; file DEC-NNNNN before ship |
