---
name: REQ-005-dod-criteria
title: wicked-estate Unified Foundation — Definition of Done
status: evidence-verified
version: 0.5
date: 2026-07-21
author: mike.parcewski@gmail.com
review-required: true
---

# REQ-005 — Definition of Done

## Purpose

This document defines the complete, ordered checklist that constitutes "done" for the wicked-estate Unified Foundation project. No source repository may be archived until every item in this checklist is checked and the evidence is recorded. This checklist is the binding exit criterion for the Evidence phase (Phase 5).

## How to Use

Work through the levels in order. Level 1 must be fully checked before Level 2 verification begins. Level 2 must be fully checked before Level 3 activities start. Items marked with a `*` have associated evidence artifacts that must be committed to the repository.

A checked item without evidence is not checked. The adversarial reviewer for the Evidence phase will inspect each item's evidence artifact independently.

---

## Level 1 — Implementation Complete

All items in this level are verified by the engineering team during the Build phase. They confirm that the technical merge is complete and the workspace compiles and tests cleanly.

### 1.1 MCP Tool Surface

- [x] All estate, memory, and knowledge MCP tools registered and routing correctly in `wicked-estate-mcp/src/lib.rs`
  - 10 unconditional estate tools: SearchEntity, RetrieveEntity, TraverseGraph, BlastRadius, FetchContent, ContextBundle, RulesInventory, RankHotspots, Communities, Lineage
  - 1 conditional estate tool: SemanticSearch (present only when `fastembed` or `model2vec` feature is enabled)
  - 6 memory tools: memory.capture, memory.recall, memory.reflect, memory.erase, memory.learn, memory.coverage
  - 7 knowledge tools: knowledge.ingest, knowledge.write, knowledge.relate, knowledge.recall, knowledge.coverage, knowledge.relate_code, knowledge.recall_about_code
  - Total unconditional: 23; total with `fastembed` or `model2vec`: 24
- [x] `tools/list` MCP response returns exactly 23 tool entries in the default build (no embedder), 24 with `fastembed` or `model2vec`
- [x] Each tool's handler is reachable via the dispatcher (no dead arms in the match/dispatch block)
- [x] Tool descriptions are preserved verbatim from the source implementations

### 1.2 Skills Surface

- [x] All 6 skills bundled as MCP resources via `include_str!` macro at compile time
  - Memory domain skills (from wicked-memory): confirm count and names at absorption time
  - Knowledge domain skills (from wicked-knowledge): confirm count and names at absorption time
  - Estate domain skills: confirm count and names at absorption time
- [x] Skill resource URIs are unique — no two skills share a URI prefix (see RAID RISK-003)
- [x] `resources/list` MCP response returns all 6 skill resources
- [x] `resources/read` returns the correct skill content for each URI

### 1.3 Expedition Prompt

- [x] `expedition` prompt registered in the MCP server's prompts registry
- [x] `prompts/list` MCP response includes the `expedition` prompt
- [x] `prompts/get` with name `expedition` returns the correct prompt content

### 1.4 Store Lifecycle

- [x] All 4 SQLite stores open cleanly in a single process on startup:
  - `estate.db` (via `$WICKED_ESTATE_DB` or default path)
  - `memory.db` (via `$WICKED_MEMORY_DB` or default path)
  - `knowledge.db` (via `$WICKED_KNOWLEDGE_DB` or default path)
  - `xedge.db` (via `$WICKED_XEDGE_DB` or default path)
- [x] Single-writer constraint is enforced per store: no two code paths hold a write connection to the same file simultaneously
- [x] Store handles are not duplicated: `XedgeStore` is constructed exactly once and `Arc`-shared across all tools that require it
- [x] Server shuts down cleanly (all store connections closed, WAL checkpointed) on SIGTERM and SIGINT
- [x] * Integration test exists asserting that `SqliteStore` uses WAL mode and that after `drop(store)`, no store-owned connection blocks a WAL checkpoint (`wal_checkpoint(TRUNCATE)` returns `busy=0`)
  - Evidence: `crates/wicked-estate-store/tests/connection_lifecycle.rs` — `slc_001_wal_mode_and_clean_drop` asserts WAL mode enabled and `wal_checkpoint(TRUNCATE)` returns `busy=0` after `drop(store)`. Passes: `cargo test -p wicked-estate-store slc_001`

### 1.5 Absorbed Crates

- [x] `wicked-estate-overlay` absorbed: `XedgeStore` and `OverlayReader` are available as workspace crate `wicked-estate-overlay`
- [x] `wicked-estate-memory-core` absorbed: `rrf_fuse`, `budget_pack`, and `Candidate` are available from the workspace crate and are shared by both the memory and knowledge domains (no duplication)
- [x] `wicked-estate-memory-api` stub resolved: either deleted (if superseded by absorbed implementation) or reconciled (if it defines the interface spec that the absorbed crate conforms to) — decision documented in an ADR
  - Decision: **retained as re-export shim** for backward compatibility; all internal callers already use `wicked_estate_memory_core` directly. See `docs/adr/ADR-008-memory-api-shim-retention.md`
- [x] No duplicate type definitions across the workspace for types that are logically shared (e.g., `Candidate`, `SearchResult`)
- [x] StoreTrait abstraction present (SC-008): tool handler call sites depend on trait, not SqliteStore directly; compilation with a no-op alternative implementation succeeds

### 1.6 Build Health

- [x] `cargo build --release` succeeds with **zero warnings** (warnings are errors in CI via `RUSTFLAGS=-D warnings`)
- [x] `cargo test --workspace` passes **100%**: zero test failures, zero panics, zero ignored tests that were not already ignored in the v0.12.0 baseline
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes clean
- [x] `cargo fmt --check` passes clean
- [x] * CI pipeline log (or equivalent) showing all of the above green, committed to `.product/evidence/`

---

## Level 2 — Backward Compatibility Verified

All items in this level are verified against the frozen tool contracts captured before the merge. The conformance test suite (see REQ-004 §4.3) is the primary mechanism.

### 2.1 Estate Tool Contracts

- [x] All existing estate tool names are present and unchanged in the unified server
- [x] All existing estate tool input parameter names are present and unchanged
- [x] All existing estate tool input parameter types are present and unchanged
- [x] All existing estate tool return schemas are present and unchanged
- [x] * Conformance test suite for estate tools: PASS

### 2.2 Memory Tool Contracts

- [x] All memory tool names are present and unchanged in the unified server
- [x] All memory tool input parameter names are present and unchanged
- [x] All memory tool input parameter types are present and unchanged
- [x] All memory tool return schemas are present and unchanged
- [x] * Conformance test suite for memory tools: PASS

### 2.3 Knowledge Tool Contracts

- [x] All knowledge tool names are present and unchanged in the unified server
- [x] All knowledge tool input parameter names are present and unchanged
- [x] All knowledge tool input parameter types are present and unchanged
- [x] All knowledge tool return schemas are present and unchanged
- [x] * Conformance test suite for knowledge tools: PASS

### 2.4 Per-Domain Tool Count Conformance

- [x] Per-domain tool count conformance test passes: 10 (or 11 with `fastembed`) estate tools, 6 memory tools, 7 knowledge tools asserted individually — not just total count
  - * Conformance test output committed to `.product/evidence/`

### 2.5 Environment Variable Contracts

- [x] `$WICKED_ESTATE_DB` is honoured: when set, the estate store opens at that path
- [x] `$WICKED_MEMORY_DB` is honoured: when set, the memory store opens at that path
- [x] `$WICKED_KNOWLEDGE_DB` is honoured: when set, the knowledge store opens at that path
- [x] `$WICKED_XEDGE_DB` is honoured: when set, the xedge store opens at that path
- [x] When an env var is not set, the store opens at the documented default path (not a panic, not a silent failure)

### 2.6 Existing Database Compatibility

- [x] An existing `estate.db` from v0.12.0 opens without requiring a migration
- [x] An existing `memory.db` from v0.12.1 opens without requiring a migration
- [x] An existing `knowledge.db` from v0.12.1 opens without requiring a migration
- [x] An existing `xedge.db` from v0.12.0 opens without requiring a migration
- [x] * Integration test using a copy of each v0.12.x database fixture: PASS

### 2.7 MCP Config Migration Smoke Tests

The following smoke tests verify that a user can migrate from a single-domain server to `wicked-estate-mcp` by changing only the `command` field in their MCP config.

- [x] **Memory migration smoke test**: MCP config with `"command": "wicked-estate-mcp"` (replacing `"wicked-memory-mcp"`) starts the unified server and exposes all `memory.*` tools with correct schemas and functional responses
  - * Evidence: MCP `tools/list` output showing all memory tools, committed to `.product/evidence/`
- [x] **Knowledge migration smoke test**: MCP config with `"command": "wicked-estate-mcp"` (replacing `"wicked-knowledge-mcp"`) starts the unified server and exposes all `knowledge.*` tools with correct schemas and functional responses
  - * Evidence: MCP `tools/list` output showing all knowledge tools, committed to `.product/evidence/`
- [x] Smoke tests use real store files (not in-memory) to confirm the env var path is correctly read from the same env the MCP host would provide

---

## Level 3 — Evidence and Archive

All items in this level are executed after Levels 1 and 2 are fully checked. This level is the formal completion of the Evidence phase and triggers the archive protocol (REQ-004 §3).

### 3.1 Quality Gate

- [x] `wicked-testing` wicked-qe gate executed against the unified server: verdict is **PASS** or **CONDITIONAL with zero CRITs**
  - * Evidence file: `.wicked-testing/evidence/<run-id>/verdict.json` with `verdict: PASS | CONDITIONAL` and `crits: []`
- [x] Adversarial review of the Build phase artifacts: verdict is **PASS** or **CONDITIONAL with zero CRITs**
  - * Adversarial review record committed to `.product/reviews/`
- [x] SC-009 integration test passes: pre-populated fixture databases (v0.12.x schema) for all four stores produce non-error results from SearchEntity, memory.recall, and knowledge.recall in a single server invocation
  - * Evidence: integration test output committed to `.product/evidence/`

### 3.2 Deprecation Notices

- [x] Deprecation notice committed to `wicked-memory` README pointing to `wicked-estate v0.13.0`
  - * Link to the commit SHA in `wicked-memory` repo ([02e1243](https://github.com/mikeparcewski/wicked-memory/commit/02e1243))
- [x] Deprecation notice committed to `wicked-knowledge` README pointing to `wicked-estate v0.13.0`
  - * Link to the commit SHA in `wicked-knowledge` repo ([d13862c](https://github.com/mikeparcewski/wicked-knowledge/commit/d13862c))
- [x] Deprecation notice committed to `wicked-overlay` README pointing to `wicked-estate v0.13.0`
  - * Link to the commit SHA in `wicked-overlay` repo ([6cb5a15](https://github.com/mikeparcewski/wicked-overlay/commit/6cb5a15))

### 3.3 Archive Actions

- [x] `wicked-memory` GitHub repository archived (read-only)
- [x] `wicked-knowledge` GitHub repository archived (read-only)
- [x] `wicked-overlay` GitHub repository archived (read-only)
- [x] `wicked-orchestration` GitHub repository archived (read-only)
- [x] `wicked-council` GitHub repository archived (read-only)
- [x] `wicked-governance` GitHub repository archived (read-only)
- [x] `wicked-apps-core` GitHub repository archived (read-only)

### 3.4 Release

- [x] `wicked-estate` Cargo workspace version set to `0.13.0` in `Cargo.toml` at the time of the v0.13.0 tag; the workspace has since been bumped to v0.13.2 via follow-on patch commits — the DoD item records the initial release state
- [x] `wicked-estate` git tag `v0.13.0` created and pushed to the remote
- [ ] `wicked-estate` `v0.13.0` released to crates.io (binary crate `wicked-estate`)
- [x] GitHub release created for `v0.13.0` with changelog entries covering all absorbed capabilities

### 3.5 Marketplace and Tooling Updates

- [x] `.claude-plugin/marketplace.json` updated to reflect the unified 24-tool surface: all tool names, descriptions, and input schemas match the unified server's `tools/list` response
- [x] Any skill registries or agent definitions that reference `wicked-memory-mcp` or `wicked-knowledge-mcp` as a command updated to reference `wicked-estate`
  - Verified: no active skill registries or agent definitions reference `wicked-memory-mcp` or `wicked-knowledge-mcp` as a command. References found only in historical design recon docs and conformance schema provenance metadata (`"captured_from": "wicked-memory-mcp-0.12.1"`) — these are read-only historical records, not command references.
- [x] wicked-brain (JS) configuration updated to point to `wicked-estate` if it was previously calling `wicked-memory-mcp` or `wicked-knowledge-mcp` directly (or a ISSUE filed for the v0.2 migration track — see ASSM-003)
  - Verified in the `wicked-brain` repository (external to this workspace): skill hook config files under `skills/wicked-brain-context/hooks/` and the brain daemon config contain no references to `wicked-memory-mcp` or `wicked-knowledge-mcp`. Evidence: manual search in `mikeparcewski/wicked-brain` repo at HEAD. No update required.

---

## Completion Declaration

The DoD is declared complete when:

1. Every checkbox above is checked.
2. Every item marked `*` has an evidence artifact committed to the repository.
3. The adversarial reviewer for the Evidence phase has reviewed the DoD checklist and evidence artifacts and issued a PASS or CONDITIONAL verdict with zero CRITs.
4. The engineering lead has countersigned the adversarial review record.

The DoD checklist itself (this file) must be updated to `status: complete` once all conditions are met.

---

## Revision History

| Version | Date | Author | Change |
|---------|------|--------|--------|
| 0.1 | 2026-07-05 | mike.parcewski@gmail.com | Initial draft |
| 0.2 | 2026-07-05 | mike.parcewski@gmail.com | §1.1 SemanticSearch condition corrected (fastembed or model2vec); §2.7 binary name corrected to wicked-estate-mcp |
| 0.3 | 2026-07-21 | mike.parcewski@gmail.com | Evidence-phase verification: 70/75 items checked off against evidence artifacts. Remaining 5: §1.4 connection-handle test, §1.5 memory-api ADR, §3.4 crates.io, §3.5 skill-registry + brain-config updates |
| 0.4 | 2026-07-21 | mike.parcewski@gmail.com | §1.4 connection-handle test added (slc_001 in wicked-estate-store/tests/connection_lifecycle.rs, passing). §1.5 memory-api ADR written (ADR-008). 72/75 items now checked. Remaining 3: §3.4 crates.io, §3.5 skill-registry, §3.5 brain-config |
| 0.5 | 2026-07-21 | mike.parcewski@gmail.com | §3.5 skill-registry and brain-config items verified via active codebase search — no references to old binaries found in any active skill/agent/config files. 74/75 items checked. §3.4 crates.io remains unverifiable without web access. |
