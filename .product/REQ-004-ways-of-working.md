---
name: REQ-004-ways-of-working
title: wicked-estate Unified Foundation — Ways of Working
status: draft
version: 0.2
date: 2026-07-05
author: mike.parcewski@gmail.com
review-required: true
---

# REQ-004 — Ways of Working

## Purpose

This document defines the development conventions, process gates, archive protocol, testing approach, breaking-change policy, and versioning strategy for the wicked-estate Unified Foundation project. All contributors and adversarial reviewers must treat these rules as binding constraints, not guidelines.

---

## 1. Development Approach

### 1.1 Rust Workspace-First

The wicked-estate Cargo workspace is the single source of truth for all absorbed domains. The absorption sequence for each donor crate (wicked-memory, wicked-knowledge, wicked-overlay) follows three ordered steps:

1. **Path dependency** — add the donor crate's source directory to the workspace `members` list and replace any published crate reference in `Cargo.toml` with a `path = "../wicked-<domain>"` dependency. The donor repo is still intact at this point.
2. **Workspace member** — once the crate compiles cleanly as a workspace member with no API surface changes, promote all shared dependencies to `workspace.dependencies` and use `<dep>.workspace = true` in the crate's own `Cargo.toml`.
3. **Publish sub-crates** (future) — library crates that external consumers may embed (e.g., `wicked-estate-memory-core`, `wicked-estate-knowledge-core`) may be published to crates.io as separate packages under the `wicked-estate-*` namespace. This is a v0.14+ concern; v0.13.0 ships only the binary.

### 1.2 No Domain Feature Flags

All three domains (estate, memory, knowledge) are **always compiled in**. There are no feature flags that disable a domain at compile time. The binary surface is always the full 23-tool MCP server in the default build (24 when the `fastembed` or `model2vec` embedder feature is compiled in — adds `SemanticSearch`). This is an explicit design choice: conditional compilation of domains would create untested code paths and break the single-binary deployment contract. Only embedder backends are permitted as feature flags (see §1.3).

### 1.3 Optional Embedder Feature Flags

Feature flags are permitted — and expected — for embedder backends only:

| Flag | Meaning |
|------|---------|
| *(default)* | No embedding — knowledge tools degrade gracefully to BM25-only retrieval |
| `fastembed` | Enables the contextual ONNX/BGE embedder (highest quality); requires additional native deps. Install: `cargo install wicked-estate --features fastembed` |
| `model2vec` | Enables the static distilled embedder (lightweight, lower resource usage). Install: `cargo install wicked-estate --features model2vec` |

`fastembed` and `model2vec` feature flags gate the presence of `SemanticSearch` in `tools/list` (absent in the default build, present when either embedder is compiled in) and enable vector recall in the memory and knowledge domains. No other tool names, parameter schemas, or return schemas are affected by these flags.

### 1.4 Dependency Resolution

Before absorbing any crate, resolve the `workspace.dependencies` section first. Shared deps — `rusqlite`, `tokio`, `tree-sitter`, `serde`, `serde_json` — must be pinned to a single workspace version. Transitive version skew is the highest-probability failure mode (see RAID RISK-001).

---

## 2. Adversarial Review Gates

The wicked-estate Unified Foundation follows the same phased, adversarially-reviewed process used by all wicked-* products. Each phase must be completed, reviewed, and adversarially challenged before the next phase begins.

### 2.1 Phases

| # | Phase | Entry Condition | Exit Condition |
|---|-------|----------------|----------------|
| 1 | **Requirements** | Project kick-off | All REQ-* docs written; RAID populated; adversarial review PASS or CONDITIONAL |
| 2 | **Design** | REQ gate PASS | ADRs for absorption sequence, store isolation, skill resource URI scheme, crate publish strategy; adversarial review PASS or CONDITIONAL |
| 3 | **Test Strategy** | Design gate PASS | Test scenarios authored covering all 24 tools and backward-compat contracts; adversarial review PASS or CONDITIONAL |
| 4 | **Build** | Test Strategy gate PASS | All DoD Level 1 and Level 2 items checked; `cargo build --release` + `cargo test --workspace` green; adversarial review PASS or CONDITIONAL |
| 5 | **Evidence** | Build gate PASS | All DoD Level 3 items checked; wicked-qe gate PASS or CONDITIONAL (zero CRITs); archive protocol complete |

### 2.2 Adversarial Review Rules

- Reviewer must not be the author of the artifact under review.
- Any CRIT finding blocks advancement to the next phase; it must be resolved and the finding re-reviewed before proceeding.
- CONDITIONAL verdicts may advance the phase if and only if all open findings have a documented remediation plan and an agreed target phase.
- A review that produces no findings must include an explicit "No findings — reviewed thoroughly" statement to distinguish it from a skipped review.

### 2.3 Council Gate

Breaking changes (see §5) require an additional council gate independent of the adversarial review. Council gate requires at least two approvers who are not the author.

---

## 3. Archive Protocol

The archive protocol applies to seven repositories: `wicked-memory`, `wicked-knowledge`, `wicked-overlay`, `wicked-orchestration`, `wicked-council`, `wicked-governance`, `wicked-apps-core`.

**Critical constraint: no repository is archived until wicked-estate v0.13.0 has been released and all smoke tests pass.** Archiving before the unified server is operational leaves users with no working MCP server.

### 3.1 Per-Repository Steps

For each repository, execute in this exact order:

1. **Write deprecation notice** — update the repository's `README.md` with a prominent deprecation block at the top. The block must include:
   - The date of deprecation.
   - The replacement: `wicked-estate v0.13.0` with a link to the wicked-estate GitHub page.
   - A migration instruction (which `wicked-estate` binary to install, which env vars to set, which MCP config lines to change).
   - For `wicked-memory` and `wicked-knowledge`: a note that all tools are available unchanged in `wicked-estate` under the same names.
2. **Commit and push** — commit the deprecation notice as a single standalone commit with message `chore: deprecate in favour of wicked-estate v0.13.0`. Push to the default branch.
3. **Make private** (if applicable) — if the repository is public and owned by a team, change visibility to private via GitHub settings. Do not do this if external consumers depend on public access to the source (evaluate case by case).
4. **Archive** — use GitHub's repository archive feature. The repository becomes read-only. Issues and PRs are frozen.

### 3.2 Archive Sequencing

Archive wicked-memory, wicked-knowledge, and wicked-overlay first (these are the actively-used MCP servers being replaced). Archive wicked-orchestration, wicked-council, wicked-governance, and wicked-apps-core in a second pass; these are superseded by wicked-crew and wicked-garden and carry no active MCP tool surface.

### 3.3 crates.io Cleanup

After archiving `wicked-memory`, publish a final `0.12.2` patch release to crates.io with a `deprecated` marker in `Cargo.toml` and a `README.md` pointing to `wicked-estate-memory-core`. Do not yank `0.12.1` — yanking breaks consumers who have pinned to it. See RAID ISSUE-002.

---

## 4. Testing Approach

### 4.1 Unit Tests

Every crate in the workspace must have `#[test]`-annotated unit tests covering its internal logic. Tests must live in the same file as the code under test (`#[cfg(test)]` blocks) or in `src/tests/` within the crate. Coverage target is 80% line coverage per crate measured via `cargo llvm-cov`.

### 4.2 Integration Tests

Integration tests live in `tests/` at the workspace root (or per-crate `tests/` directories). Each integration test scenario that exercises the full MCP server must:

1. Spin up all 4 SQLite stores in a temporary directory (`tempfile::tempdir()`).
2. Start the MCP server with the 4 store paths set via env vars.
3. Send real MCP JSON-RPC requests over stdio.
4. Assert on the response schema and returned data.
5. Verify that exactly one connection handle is open per store file (see RAID RISK-002).

Integration tests must not share store state between test cases. Each test gets a fresh `tempdir`.

### 4.3 Conformance Tests — Tool Contract Backward Compatibility

A dedicated conformance test suite verifies that the tool surface has not regressed relative to the pre-unification contracts. For each of the 24 tools:

- The tool name is present in `tools/list` response.
- The `inputSchema` matches the frozen contract schema (stored in `tests/conformance/schemas/<tool-name>.json`).
- A representative call returns a valid response matching the frozen response schema.

These schemas are captured from the current production binaries (`wicked-memory-mcp`, `wicked-knowledge-mcp`, `wicked-estate-mcp`) before the merge begins and checked into the repo as golden files.

### 4.4 wicked-testing QE Gate

After the build phase passes, the test plan is executed via the `wicked-testing` acceptance pipeline. The QE gate verdict must be PASS or CONDITIONAL with zero CRITs before the archive protocol begins. Evidence files are stored in `.wicked-testing/evidence/` and referenced in the DoD Level 3 checklist.

---

## 5. Breaking Change Policy

### 5.1 Frozen Contracts

The following are frozen contracts — they must not change in v0.13.0 or any patch release in the v0.13.x series:

| Contract type | Examples |
|--------------|---------|
| MCP tool names | `SearchEntity`, `RetrieveEntity` (estate — bare names, no prefix); `memory.capture`, `memory.recall` (memory domain); `knowledge.ingest`, `knowledge.recall` (knowledge domain) |
| MCP tool parameter names | `content`, `query`, `limit`, `scope`, `max_nodes`, `token_budget` (drawn from REQ-003 §2 normative catalog) |
| MCP tool parameter types | string, number, boolean, array — shape must not change |
| MCP tool return schemas | top-level fields, field types |
| Environment variable names | `$WICKED_ESTATE_DB`, `$WICKED_MEMORY_DB`, `$WICKED_KNOWLEDGE_DB`, `$WICKED_XEDGE_DB` |
| Store file names (defaults) | `estate.db`, `memory.db`, `knowledge.db`, `xedge.db` |

### 5.2 CRIT Classification

Any proposed change to a frozen contract is automatically classified as a CRIT finding in adversarial review. CRITs block phase advancement without exception. Resolution requires either:

- (a) reverting the change, or
- (b) council gate approval with a documented migration path for existing users and a semver-major bump (v0.14.0).

### 5.3 Store Schema Changes

SQLite schema changes (new tables, new columns, altered column types) require:

1. A migration script in `migrations/<version>/<up.sql>` and `<down.sql>`.
2. An integration test that opens a v0.12.x database and verifies the migration applies cleanly.
3. Documentation in the relevant crate's `CHANGELOG.md`.

Migrations that are destructive (column drops, table drops) are forbidden in patch and minor releases.

---

## 6. Versioning

### 6.1 Workspace Version

The workspace version is unified to **0.13.0**. This absorbs:

- `wicked-estate` v0.12.0 (merge base)
- `wicked-memory` v0.12.1
- `wicked-knowledge` v0.12.1

The version bump from 0.12.x to 0.13.0 is a minor bump, reflecting new capability (unified binary) with no breaking changes to existing tool contracts.

### 6.2 Binary Crate

`wicked-estate-mcp` (the published binary) uses `version.workspace = true`. Its version is always the workspace version.

### 6.3 Library Crates

Library crates that external consumers may pin (`wicked-estate-memory-core`, `wicked-estate-knowledge-core`, `wicked-estate-overlay`) use `version.workspace = false` and maintain their own version numbers. This allows consumers to depend on a specific library version without being forced to update when the binary version bumps.

For v0.13.0, set all library crate versions to `0.13.0` to align with the initial unified release. Divergence is permitted from v0.14.0 onwards.

### 6.4 crates.io Publishing

The binary crate (`wicked-estate-mcp`) is published to crates.io as `wicked-estate`. Library crates are published if and only if there are known external consumers. For v0.13.0, only the binary is published. The `publish = false` field must be set explicitly in each library crate's `Cargo.toml` until the publish decision is made (see RAID ISSUE-003).

---

## Revision History

| Version | Date | Author | Change |
|---------|------|--------|--------|
| 0.1 | 2026-07-05 | mike.parcewski@gmail.com | Initial draft |
| 0.2 | 2026-07-05 | mike.parcewski@gmail.com | §1.3 embedder flag rule corrected (SemanticSearch gated, not ignored); §5.1 tool names and parameter examples corrected; §6.3 library crate policy aligned with REQ-002 |
