---
name: REQ-002-technology-constraints
title: wicked-estate Unified Foundation — Technology Constraints
status: revised
version: 0.2
date: 2026-07-05
author: mike.parcewski@gmail.com
review-required: true
---

# REQ-002 — Technology Constraints

## Purpose

This document records the hard constraints, technology decisions by layer, and version pinning rules for the wicked-estate Unified Foundation consolidation. All technology choices must be traceable to a constraint or a decision record in `.RAID/decisions/`. No technology may be introduced to the stack without a corresponding DEC-NNNNN entry. Where a decision was made in a prior product (wicked-memory DEC-1, wicked-bus DEC-ECO-005, DEC-ECO-006), the reference is carried forward here with the specific impact on the consolidated binary stated explicitly.

---

## Hard Constraints

The following constraints are non-negotiable. Any technology choice that violates a hard constraint must be rejected regardless of merit. Exceptions require a RAID decision record and explicit human approval.

| ID | Constraint | Rationale |
|---|---|---|
| HC-001 | Single binary: all 24 tools must be served by a single `wicked-estate-mcp` process with no sidecar processes | The entire value proposition is one install, one server block; a design that requires a companion process is not a consolidation |
| HC-002 | No network calls in MCP tool handlers | Tool handlers must be synchronous-safe (no network I/O on the hot path); network latency would break the 200ms p95 NFR and introduce failure modes incompatible with local-first operation |
| HC-003 | Store isolation (DEC-1): separate SQLite files per domain (estate.db, memory.db, knowledge.db, xedge.db); no cross-domain row writes | Data safety invariant established when wicked-memory and wicked-knowledge were designed; consolidating the binary does not relax the store isolation contract |
| HC-004 | Single-writer per store: each SQLite file has exactly one write path enforced by `Arc<Mutex<Connection>>`; no concurrent writers to the same file | SQLite WAL supports concurrent readers but not concurrent writers without SQLITE_BUSY contention; the single-writer pattern is the established solution across all four stores |
| HC-005 | No unsafe code in new code added during consolidation | Existing crates may carry `unsafe` in tree-sitter grammar vendored code (which is excluded from the workspace); all new consolidation glue code must be safe Rust |
| HC-006 | No runtime embedding model dependency in the default feature set | The binary must work on a machine with no ONNX runtime, no model files, and no network access; semantic search is an optional feature compiled in at build time |
| HC-007 | All existing tool names, env var names, and JSON-RPC request/response schemas are preserved unchanged | Any renaming is a breaking change for every existing MCP host config in the ecosystem; backward compatibility is SC-001 and SC-006 |
| HC-008 | Cross-platform: macOS (arm64 + x86_64), Linux (x86_64 + arm64), Windows (Git Bash / WSL) | The MCP host ecosystem runs on all three OSes; a binary that fails on Linux or Windows is not a viable universal context server |

---

## Full Stack by Layer

### Layer 1 — Runtime Environment

**Technology:** Rust stable, edition 2024, rust-version 1.85, Tokio async runtime

**Decision:** Established by wicked-estate workspace; carried forward unchanged.

**Specification:**

- Rust edition 2024 (`edition = "2024"` in `[workspace.package]`). This is the edition already set in the wicked-estate workspace Cargo.toml. All absorbed crates (wicked-memory, wicked-knowledge, wicked-overlay) are also edition 2024; no edition migration is required.
- Minimum supported Rust version: 1.85 (`rust-version = "1.85"` in `[workspace.package]`). This is the floor across all existing crates and must not be lowered. CI enforces this with `rustup toolchain install 1.85` in the MSRV check job.
- Tokio async runtime: `tokio = { version = "1", features = ["rt-multi-thread", "macros", "io-util", "io-std"] }`. The MCP stdio transport reads from `stdin` and writes to `stdout` using Tokio's async I/O. The multi-thread runtime is required because the overlay's `OverlayReader` uses `Handle::block_on` to drive async pool reads from within a blocking-pool context (DEC-X1b in the wicked-overlay design).
- Release profile: `strip = true`, `lto = true`, `codegen-units = 1`. These settings are already in the wicked-estate workspace `[profile.release]` and apply to the consolidated binary. They are non-negotiable: they keep the binary small (strip), maximize inlining across crate boundaries (lto), and produce deterministic single-unit code (codegen-units = 1).
- The `rust-toolchain.toml` in the workspace root pins to `channel = "stable"` with `components = ["rustfmt", "clippy"]`. No nightly features are permitted.

**Version pinning:** Rust stable channel. rust-version minimum: 1.85. This is checked by `cargo +1.85 check` in CI.

---

### Layer 2 — Storage

**Technology:** SQLite WAL mode via bundled rusqlite, FTS5 for text search, sqlite-vec for vector/ANN, four isolated database files

**Decision:** DEC-1 (store isolation), DEC-ECO-006 (StoreTrait abstraction path)

**Specification:**

**SQLite WAL mode (primary):**
- `rusqlite = { version = "0.32", features = ["bundled"] }`. The `bundled` feature compiles SQLite directly into the binary; no system SQLite installation is required on any target platform. This is already the dependency in wicked-overlay and the approach used by wicked-estate-store.
- WAL journal mode is set at connection open time: `PRAGMA journal_mode=WAL`. WAL permits multiple concurrent readers while serializing writes. The single-writer pattern (HC-004) means only one `Arc<Mutex<Connection>>` write guard is ever held at a time per store file.
- `PRAGMA synchronous=NORMAL` in WAL mode: safe for local-first use (survives OS crash, not power loss; acceptable for developer tooling).
- Schema migrations run at connection open time using an embedded migration sequence (not a runtime migration framework dependency). Each store's migration sequence is owned by its respective crate.

**Four isolated store files:**
| File | Owner Crate | Default Path | Env Var |
|---|---|---|---|
| estate.db | wicked-estate-store | `$WICKED_HOME/estate.db` (default `~/.wicked/estate.db`) | `WICKED_ESTATE_DB` |
| memory.db | wicked-estate-memory-core (absorbed from wicked-memory-core) | `$WICKED_HOME/memory.db` (default `~/.wicked/memory.db`) | `WICKED_MEMORY_DB` |
| knowledge.db | wicked-estate-knowledge (absorbed from wicked-knowledge) | `$WICKED_HOME/knowledge.db` (default `~/.wicked/knowledge.db`) | `WICKED_KNOWLEDGE_DB` |
| xedge.db | wicked-estate-overlay (absorbed from wicked-overlay) | `$WICKED_HOME/xedge.db` (default `~/.wicked/xedge.db`) | `WICKED_XEDGE_DB` |

Each file is opened independently at startup. A failure to open one store (e.g., permission error on knowledge.db) logs a startup error and continues; the tools for the failed store return JSON-RPC error responses rather than crashing the process.

**FTS5 for text search:**
- FTS5 is compiled in as part of bundled SQLite (it is enabled by default in SQLite's bundled build). No additional feature flag is required.
- FTS5 tables are used in all four stores: estate.db indexes symbol names, docstrings, and file paths; memory.db indexes memory content; knowledge.db indexes knowledge node content; xedge.db does not use FTS5 (it stores only edge metadata).
- FTS5 rank function: BM25 (SQLite's built-in FTS5 rank). No external ranking library.

**sqlite-vec for vector/ANN:**
- Used in estate.db and memory.db for semantic recall when the `model2vec` or `fastembed` feature is compiled in. Not used in knowledge.db or xedge.db in v0.1.
- sqlite-vec is a SQLite extension loaded at runtime. It is included as a vendored Rust crate in the wicked-estate-retrieve dependency tree, not as a system extension. No external `.so`/`.dylib` loading.
- When neither `model2vec` nor `fastembed` feature is active, all code paths that would call sqlite-vec are gated behind `#[cfg(feature = "...")]` and sqlite-vec is not linked. The binary works fully without vector support.

**StoreTrait abstraction (DEC-ECO-006):**
- The SQLite implementations are wrapped behind a `StoreTrait` (or equivalent abstraction) at the crate boundary. In v0.1, only the SQLite implementation exists. The trait is defined and the SQLite implementation is the sole concrete type. This ensures the Postgres backend can be added in v0.2 by implementing the trait, without changing the tool handler signatures. The trait definition must be in place before v0.1 ships (SC-008 requires it: tool handler call sites depend on trait, not SqliteStore directly).
- The Postgres implementation is explicitly out of scope for v0.1 (see "Out of Scope" section).

**Version pinning:** `rusqlite = "0.32"` with `bundled` feature. SQLite version is determined by the bundled source in the rusqlite crate; no independent SQLite pin. sqlite-vec version follows the wicked-estate-retrieve dependency pin.

---

### Layer 3 — Protocol

**Technology:** MCP JSON-RPC 2.0 over stdio, hand-rolled (no rmcp dependency), JSON Schema for tool schemas, MCP resources for bundled skills, MCP prompts for expedition prompt

**Decision:** Established by wicked-estate-mcp, wicked-memory-mcp, and wicked-knowledge-mcp; unified pattern carried forward.

**Specification:**

**Transport — stdio:**
- The binary reads newline-delimited JSON-RPC 2.0 requests from stdin and writes newline-delimited JSON-RPC 2.0 responses to stdout. One request per line; one response per line. This is the MCP stdio transport contract.
- The main loop is synchronous (using `BufRead` on `stdin().lock()` and `writeln!` on `stdout().lock()`), consistent with the existing wicked-memory-mcp and wicked-knowledge-mcp implementations. Tokio's async runtime is available for internal store operations that require it (the overlay's `Handle::block_on` pattern), but the stdio loop itself is synchronous. This avoids async-on-sync deadlock hazards at the process boundary.
- Error on stdin read (EOF, broken pipe): the binary exits cleanly with code 0. It does not spin on EOF.

**Protocol — hand-rolled, no rmcp:**
- The JSON-RPC dispatch is hand-rolled: parse the incoming JSON with `serde_json`, match on `method`, dispatch to a handler, serialize the response. No MCP SDK crate (`rmcp` or equivalent) is used. This is the established pattern in all three existing servers and is carried forward. Using a framework would add a version dependency with a fast-moving upstream and potential breaking changes; the hand-rolled approach is stable and already proven.
- Supported methods: `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read`, `prompts/list`, `prompts/get`. Unknown methods return a JSON-RPC error with code -32601.
- The `initialize` response declares the server name (`"wicked-estate"`), version (from `workspace.package.version`), and capabilities: `tools`, `resources`, `prompts`.

**Tool schemas — JSON Schema:**
- Each tool's input schema is declared as a JSON Schema object embedded in the `tools/list` response. The schema must include `type: "object"`, `properties`, and `required` array.
- Schemas are defined as `serde_json::json!()` literals in the Rust source, not generated from Rust types. This is the pattern used in all three existing servers and is carried forward. A future refactor to generate schemas from Rust types is explicitly not a v0.1 commitment.
- All 24 tool schemas must be present in the `tools/list` response. `SemanticSearch` is included only when the binary is compiled with `model2vec` or `fastembed` feature.

**MCP resources — bundled skills:**
- The 6 bundled skills (codebase-expedition, knowledge-ingest, ontology-expedition, knowledge-curation, cited-answer, gap-hunting) are compiled into the binary at build time using `include_str!()` macros. The skill Markdown files live in their respective crate source trees (e.g., `crates/wicked-estate-memory/skills/codebase-expedition/` after absorption).
- `resources/list` returns a list of resource descriptors, one per bundled skill. `resources/read` returns the skill content by URI.
- The resource URI scheme is `skill://{name}/SKILL.md` (e.g., `skill://codebase-expedition/SKILL.md`). Confirmed source-verified in wicked-knowledge v0.12.1 (see RAID RISK-003). Domain-prefix qualifiers are embedded in the skill name slug, not the URI scheme. This is stable across versions; the scheme must not change without a DEC-NNNNN entry.

**MCP prompts — expedition prompt:**
- The codebase-expedition prompt is registered as an MCP prompt (in addition to being a resource). `prompts/list` returns it; `prompts/get` returns the prompt template with argument descriptors.

**Version pinning:** No MCP SDK version to pin (hand-rolled). serde_json: workspace dependency (`serde_json = "1"`). The MCP protocol version targeted is the version currently implemented by all three existing servers; any protocol version upgrade requires an explicit decision.

---

### Layer 4 — Cargo Workspace

**Technology:** wicked-estate workspace as the root; absorbed crates become workspace members

**Decision:** This is the primary structural decision of the consolidation. DEC-NNNNN must be filed before v0.1 ships.

**Specification:**

**Current workspace structure (pre-consolidation):**
```
wicked-estate/
  Cargo.toml            # [workspace] root
  crates/
    wicked-estate/      # main crate (graph APIs, index entry point)
    wicked-estate-bench/
    wicked-estate-core/ # published to crates.io; absorber dep
    wicked-estate-extract/
    wicked-estate-mcp/  # MCP server binary — CONSOLIDATED HERE
    wicked-estate-memory-api/
    wicked-estate-observe/
    wicked-estate-rank/
    wicked-estate-resolve/
    wicked-estate-retrieve/
    wicked-estate-store/ # published to crates.io; absorber dep
```

**Post-consolidation workspace structure (target):**
```
wicked-estate/
  Cargo.toml            # [workspace] root — members list extended
  crates/
    ... (existing crates unchanged) ...
    wicked-estate-memory-core/   # absorbed from wicked-memory/crates/wicked-memory-core
    wicked-estate-memory/        # absorbed from wicked-memory/crates/wicked-memory-mcp (lib only)
    wicked-estate-knowledge/     # absorbed from wicked-knowledge/src (the single-crate repo)
    wicked-estate-overlay/       # absorbed from wicked-overlay/src
    wicked-estate-mcp/           # EXTENDED: now dispatches to all four domains
```

**Absorption rules:**
- Each absorbed crate is renamed to the `wicked-estate-*` prefix to make its workspace membership unambiguous. The original crate name (e.g., `wicked-memory-core`) is preserved as the published crates.io name for backward compatibility until the old repos are archived and consumers have migrated.
- Crates.io dependencies on `wicked-estate-core` and `wicked-estate-store` in the absorbed crates switch to workspace path dependencies: `wicked-estate-core = { path = "../wicked-estate-core", version = "0.12.0" }`. This is required to prevent circular version pinning (absorbed crates cannot depend on their absorber via crates.io during development).
- `wicked-memory-py` and `wicked-memory-ts` are excluded from the workspace (they were already excluded in the wicked-memory workspace due to the maturin cdylib build constraint). They remain in the archived wicked-memory repository as historical artifacts and are explicitly out of scope for v0.1.

**Tool dispatch in the consolidated `wicked-estate-mcp`:**
- The consolidated MCP server's `tools/call` handler dispatches by tool name prefix: `memory.*` tools go to the `MemoryEngine` (from absorbed `wicked-estate-memory`); `knowledge.*` tools go to the knowledge handler (from absorbed `wicked-estate-knowledge`); all other tools go to the existing estate retrieval tools.
- The `MemoryEngine`, knowledge handler, and `XedgeStore` are instantiated once at startup and shared across requests as owned state in the main loop — consistent with the existing per-server pattern where each binary owns its engine instance.

**Version pinning:** All workspace members share the workspace version (`version.workspace = true`). The consolidated release is a single version bump to `0.13.0` (see OQ-003 in REQ-001). External consumers who pinned `wicked-memory-core = "0.12.x"` or `wicked-knowledge-mcp = "0.12.x"` must update their pins; migration guidance is part of the ship checklist.

---

### Layer 5 — Deployment

**Technology:** Single binary via `cargo install wicked-estate`; env vars for DB paths; `.claude-plugin/` for marketplace registration

**Decision:** Established by existing wicked-estate and wicked-memory release tooling; extended for consolidation.

**Specification:**

**Install:**
- `cargo install wicked-estate` installs `wicked-estate-mcp` to `$CARGO_HOME/bin/`. This is the primary install path for developer machines. No npm, no Python virtualenv, no system package manager dependency.
- Prebuilt binaries for macOS arm64, macOS x86_64, Linux x86_64, and Linux arm64 are published to the GitHub Release on each version tag via the existing `scripts/publish.sh` workflow. Windows prebuilt binaries via cross-compilation are a v0.2 target; Windows users use WSL or Git Bash with the Rust toolchain installed in v0.1.
- The installed binary name is `wicked-estate-mcp`. The prior binaries `wicked-memory-mcp` and `wicked-knowledge-mcp` are no longer installed by their respective packages after archival. Migration guide must note that users must remove old binaries from their PATH and update MCP config blocks.

**Environment variables (canonical list for v0.1):**

| Variable | Default | Purpose |
|---|---|---|
| `WICKED_ESTATE_DB` | `$WICKED_HOME/estate.db` (`~/.wicked/estate.db`) | Path to the estate code graph SQLite file |
| `WICKED_MEMORY_DB` | `$WICKED_HOME/memory.db` (`~/.wicked/memory.db`) | Path to the memory SQLite file |
| `WICKED_KNOWLEDGE_DB` | `$WICKED_HOME/knowledge.db` (`~/.wicked/knowledge.db`) | Path to the knowledge SQLite file |
| `WICKED_XEDGE_DB` | `$WICKED_HOME/xedge.db` (`~/.wicked/xedge.db`) | Path to the cross-store edge SQLite file |
| `WICKED_MEMORY_T0_PERSIST` | unset (off) | When set to `1`, T0 working-memory captures are persisted to `memory.db`; off by default (T0 is ephemeral in-process only). See REQ-003 §6 for the tier model. |

These five env vars are the complete set for v0.1. No other env vars configure store paths. The names are stable; renaming any of them is a breaking change requiring a DEC-NNNNN entry. `WICKED_ESTATE_DB` also accepts a `--db <path>` CLI flag; the flag takes precedence over the env var. The other three stores do not have CLI flag overrides in v0.1 (env var only).

**MCP host config block (canonical form):**
```json
{
  "mcpServers": {
    "wicked-estate": {
      "command": "wicked-estate-mcp",
      "env": {
        "WICKED_ESTATE_DB": "/absolute/path/.wicked/estate.db",
        "WICKED_MEMORY_DB": "/absolute/path/.wicked/memory.db",
        "WICKED_KNOWLEDGE_DB": "/absolute/path/.wicked/knowledge.db",
        "WICKED_XEDGE_DB": "/absolute/path/.wicked/xedge.db"
      }
    }
  }
}
```

Absolute paths are required for `WICKED_ESTATE_DB` in production configs; relative paths resolve from the MCP host's working directory, which differs between hosts and is unreliable.

**Marketplace registration:**
- The `.claude-plugin/` directory in the repository root contains the Claude Code plugin manifest. It is updated to reflect the new server name, the extended tool list (24 tools), and the bundled skills (6 resources). The manifest version must be bumped to match the consolidated release version.
- The `PLUGIN.md` at the workspace root is updated to reflect all 24 tools and the new single-binary install instructions.

**Version pinning:** Cargo package version follows workspace semver. Prebuilt binary artifacts are tagged by version and published via GitHub Releases. There is no npm package for `wicked-estate-mcp`; it is a Rust binary only.

---

### Layer 6 — Constraints (Cross-Cutting)

**Technology:** Rust borrow checker + Arc<Mutex<Connection>>, serde_json, JSON Schema, compile-time feature flags

**Specification:**

**No network calls in MCP tool handlers (HC-002):**
- MCP tool handler functions must be synchronous with respect to network I/O. All I/O is SQLite (local file). Any function that would make a network call (e.g., to an embedding API endpoint) must be rejected in code review. The only permitted I/O in a tool handler is: SQLite read via the store connection, SQLite write via the store connection, stderr logging.
- This constraint applies to the `model2vec` and `fastembed` feature paths: model inference must use a locally-loaded model file, not a remote inference API. Model files are either bundled in the binary (model2vec) or loaded from a local path on disk (fastembed). No network call at inference time.

**No unsafe in new code (HC-005):**
- All new Rust code written during the consolidation (the dispatch layer in `wicked-estate-mcp`, the workspace member crate wrappers, any glue code between absorbed crates) must be safe Rust. CI runs `cargo clippy -- -D warnings` and `cargo test` under `RUSTFLAGS="-D warnings"`. The `deny(unsafe_code)` attribute is applied to all new crate roots. Existing `unsafe` in vendored tree-sitter grammar crates is not subject to this constraint (they are excluded from the workspace).

**No runtime embedding model dependency in default features (HC-006):**
- The default feature set (`cargo install wicked-estate` with no `--features` flag) must produce a binary that runs without any ONNX runtime, without any model file download, and without any network access. The `SemanticSearch` tool is absent from `tools/list` in the default build. CI tests the default feature build explicitly.
- Optional features: `--features model2vec` enables the model2vec static distilled embedder (light, fast, model bundled or loaded from disk). `--features fastembed` enables the FastEmbed ONNX/BGE embedder (heaviest, highest quality, requires ONNX runtime). These features are mutually exclusive at the embedder selection level; if both are compiled, fastembed wins at runtime (consistent with the existing behavior in wicked-estate).

**Single-writer per store (HC-004):**
- `Arc<Mutex<Connection>>` is the enforced pattern for all four store connections. The connection is opened once at startup and held for the lifetime of the process. All write operations acquire the mutex, perform the write, and release. No write operation holds the mutex across an await point (in the main loop, which is synchronous, this is naturally enforced; in any async helper that wraps write operations, a `block_on` pattern is used rather than holding the mutex across an await).
- The `XedgeStore` in wicked-overlay already uses `rusqlite::Connection` directly (not a pool) with a mutex pattern. This is carried forward without change.
- `wicked-estate-store` uses a `SqlitePool` (connection pool) for the estate.db reads, but the write path is serialized through the pool's write-connection slot. This pattern is preserved in the consolidated binary.

**Tool name, env var, and schema immutability (HC-007):**
- A table of all 24 tool names and their JSON Schema field names is maintained in `.RAID/decisions/DEC-NNNNN-tool-name-registry.md`. Any change to a tool name, a required field name, or an env var name is a breaking change that requires: a DEC-NNNNN entry, a major version bump, and a migration guide.
- In v0.1, the tool registry is: estate tools (SearchEntity, RetrieveEntity, TraverseGraph, BlastRadius, FetchContent, ContextBundle, RulesInventory, RankHotspots, Communities, Lineage, SemanticSearch); memory tools (memory.capture, memory.recall, memory.reflect, memory.erase, memory.learn, memory.coverage); knowledge tools (knowledge.ingest, knowledge.write, knowledge.relate, knowledge.recall, knowledge.coverage, knowledge.relate_code, knowledge.recall_about_code).

---

### Layer 7 — Testing

**Technology:** Rust unit tests (cargo test), wicked-testing 3-agent acceptance pipeline for flow-level scenarios

**Specification:**

**Unit tests:**
- Each absorbed crate carries its existing unit tests (`#[cfg(test)]` modules). These tests are not modified during absorption; they run as part of `cargo test --workspace`.
- The consolidated `wicked-estate-mcp` crate adds integration-level unit tests covering: (a) `tools/list` returns the correct count of tools per feature set; (b) `tools/call` for each of the 24 tools with a minimal valid request returns a non-error JSON-RPC response; (c) concurrent `memory.capture` + `memory.recall` calls do not produce SQLITE_BUSY; (d) starting with no DB files present creates all four files with valid schemas.
- All tests use `tempfile::TempDir` for DB isolation; no test writes to a persistent path.

**Acceptance pipeline:**
- Scenarios for the 3-agent wicked-testing acceptance pipeline are authored in `.wicked-testing/scenarios/`. The consolidated binary is exercised against scenarios derived from REQ-001 Flows 1–5.
- At minimum, the following scenarios must exist before the Phase 0 gate: (1) single-binary tools-list (SC-002); (2) store isolation write-through check (SC-004); (3) backward compat — existing estate tool round-trip (SC-001); (4) migration fixture — memory.db + knowledge.db from prior versions (SC-006).

**Version pinning:** wicked-testing version follows the ecosystem pin (currently `>=0.6.0 <0.7.0`). The testing framework is not a runtime dependency of `wicked-estate-mcp`; it is a dev dependency for scenario execution only.

---

## Rejected Technologies

The following technologies were considered for the consolidation and rejected. They must not be re-introduced without a new DEC-NNNNN entry addressing the original rejection rationale.

| Technology | Considered For | Rejection Rationale | Status |
|---|---|---|---|
| rmcp (MCP SDK crate) | Protocol dispatch layer | Fast-moving upstream; adds a version dependency with potential breaking changes; hand-rolled approach is already proven across three servers and adds zero extra dependencies | Rejected pre-consolidation; inherited decision |
| Multiple processes with an internal IPC bus | Consolidation architecture | Defeats the purpose: one install but multiple processes is not "one binary"; adds IPC failure modes; the single-process consolidation is simpler and faster | Rejected in consolidation design |
| Postgres backend (v0.1) | Storage layer | Requires a running Postgres server; violates local-first operator model for developer machines; the StoreTrait abstraction stub is all that is required in v0.1 to preserve the v0.2 path | Deferred to v0.2 |
| Runtime migration framework (diesel_migrations, sqlx-migrate) | Schema migrations | Adds a dependency with its own version lifecycle; embedded migration sequences in Rust source are already proven in all three existing servers and require no additional framework | Rejected; established pattern carried forward |
| Network embedding APIs (OpenAI embeddings, Cohere embed) | Semantic search | Violates HC-002 (no network calls in tool handlers) and HC-006 (no runtime embedding model dependency in default); local model files only | Rejected; inherited from estate design |
| Shared SQLite file for all domains | Store consolidation | Violates HC-003 (store isolation, DEC-1); schema merging is complex and creates cross-domain write coupling; separate files are simpler and safer | Rejected; DEC-1 is a hard constraint |
| `wicked-memory-py` / `wicked-memory-ts` absorption | Workspace members | maturin cdylib build cannot be in a standard Cargo workspace (`-undefined dynamic_lookup` link flag conflict); already excluded in the wicked-memory workspace; out of scope for v0.1 | Deferred; remains in archived wicked-memory repo |

---

## Version Pinning Rules

1. **Workspace crates — binary and core crates:** The binary (`wicked-estate-mcp`) and core library crates (`wicked-estate-core`, `wicked-estate-store`) use `version.workspace = true`. The workspace version is bumped atomically for these crates in a release. External consumers pin to `>=X.Y.0, <X.(Y+1).0` (minor-locked). Major version bumps require a DEC-NNNNN entry listing all breaking changes. **Exception:** library crates designated for separate crates.io publishing (`wicked-estate-memory-core`, `wicked-estate-knowledge-core`, `wicked-estate-overlay`) follow the independent versioning policy in REQ-004 §6.3 — they use `version.workspace = false` and maintain their own version numbers.

2. **wicked-estate-core and wicked-estate-store (published to crates.io):** Absorbed crates reference these via workspace path (`{ path = "../wicked-estate-core" }`). External repos that pin to `wicked-estate-core = "0.12.0"` continue to work; the consolidation does not change the public API of these crates. The next crates.io publish of these crates is at the consolidated workspace version (0.13.0 or as decided per OQ-003 in REQ-001).

3. **rusqlite:** Pinned to `"0.32"` with `bundled` feature across all crates. Any upgrade must be coordinated across all workspace members simultaneously; a minor upgrade requires checking the SQLite version bundled in the new rusqlite version for FTS5 and WAL compatibility. Pin updates require a PR with an explicit confirmation that sqlite-vec compatibility is preserved.

4. **tokio:** `version = "1"` with explicit features listed per crate. No wildcard `features = ["full"]`. Each crate lists only the Tokio features it actually uses (e.g., `wicked-estate-mcp` uses `rt-multi-thread, macros, io-util, io-std`; not `net`, not `fs`, not `time`).

5. **serde / serde_json:** Workspace dependencies (`serde = "1"`, `serde_json = "1"`). All crates use `serde.workspace = true` and `serde_json.workspace = true`. No per-crate overrides.

6. **thiserror:** `thiserror = "2"` (workspace). thiserror 2 is already pinned in the wicked-estate workspace; wicked-memory was on the same version. No conflict.

7. **Cargo.lock:** Committed to the repository (application binary, not library). Reproducible CI builds require a committed lockfile. The lockfile is updated explicitly (`cargo update` with a PR) rather than automatically by CI.

8. **Feature flags:** `model2vec` and `fastembed` are not default features. They are opt-in at install time (`cargo install wicked-estate --features model2vec`). CI tests the default (no semantic) build and the model2vec build. The fastembed build is tested in a separate CI job on a runner with the ONNX runtime available.

---

## Out of Scope for v0.1

The following items are explicitly deferred. They must not be designed, implemented, or partially implemented in the v0.1 consolidation. Each deferred item is a candidate for a v0.2 decision record.

| Item | Rationale for Deferral |
|---|---|
| Postgres backend implementation | Requires a running Postgres server; violates local-first model for v0.1; StoreTrait abstraction stub is all that is required |
| wicked-brain MCP server deprecation | Requires coordinated deprecation with the wicked-brain skill layer; timeline is an open question (OQ-002, REQ-001) |
| `wicked-memory-py` Python bindings | maturin build constraint; remains in archived wicked-memory repo; no consumer has requested migration |
| `wicked-memory-ts` TypeScript bindings | Same as Python; no active consumer; out of scope |
| Unified `OverlaySearch` tool (server-side fan-out) | Useful but not required for v0.1 flows; agents can fan-out client-side; deferred to v0.2 design |
| Native Windows PowerShell support (without WSL/Git Bash) | Requires separate CI runner and extensive hook/binary testing; Git Bash + WSL covers the majority of Windows developer use cases for v0.1 |
| MCP streaming responses (partial results) | The current MCP protocol implementation uses complete responses; streaming is a protocol-level feature not in scope for v0.1 |
| Version unification across all wicked-* ecosystem crates | This is an ecosystem-level decision (OQ-003, REQ-001) that spans repos beyond wicked-estate; it is not a v0.1 deliverable |

---

## Open Questions

| ID | Question | RAID Reference |
|---|---|---|
| OQ-TC-001 | Should the `StoreTrait` abstraction be defined as a Rust trait with an associated error type, or as a simpler enum-dispatch type? The trait approach enables the Postgres v0.2 path cleanly but adds associated type complexity throughout the call tree. | Open — file DEC-NNNNN before v0.1 ships; this decision gates SC-008 (tool handler call sites must depend on trait, not SqliteStore directly) |
| OQ-TC-002 | Does the `OverlayReader`'s `Handle::block_on` pattern (DEC-X1b) remain safe when the wicked-estate-mcp main loop moves to a fully async Tokio runtime? The current approach uses a synchronous main loop to avoid this question; a future async migration must revisit it. | Open — not a v0.1 blocker; document in RISK register |
| OQ-TC-003 | Is the `uuid = { version = "1", features = ["v7"] }` dependency in wicked-memory-core compatible with the wicked-estate workspace's own UUID usage? If wicked-estate uses UUID v4 for event IDs and wicked-memory uses UUID v7 for memory node IDs, there is no conflict (both are valid UUID versions in the same crate), but it should be confirmed before absorption. | Open — quick audit required before absorption PR |
| OQ-TC-004 | The wicked-knowledge Cargo.toml is a single-crate repo (not a workspace); its `[lib]` and `[[bin]]` are both in the same `src/`. After absorption into the wicked-estate workspace, should `wicked-estate-knowledge` split into a `wicked-estate-knowledge-core` lib and a dispatch module, or remain as a single crate with both lib and no separate binary? | Open — recommendation is single crate (no separate binary after consolidation); DEC-NNNNN required |
