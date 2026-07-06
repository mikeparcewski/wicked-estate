---
name: RAID
title: wicked-estate Unified Foundation — RAID Log
status: draft
version: 0.2
date: 2026-07-05
author: mike.parcewski@gmail.com
review-required: true
---

# RAID — wicked-estate Unified Foundation

RAID log for the absorption of wicked-memory (v0.12.1), wicked-knowledge (v0.12.1), and wicked-overlay (v0.12.0) into the wicked-estate Cargo workspace, and the archival of seven superseded repositories.

Each item carries: an ID, a severity or status, a description, a mitigation or resolution, an owner, and the target phase by which it must be resolved.

---

## Risks

Risks are ordered by severity (HIGH → MEDIUM → LOW). Each risk has a mitigation that must be actioned before or during the target phase. Risks that are not mitigated by their target phase are escalated to Issues.

---

### RISK-001 — Cargo Dependency Graph Conflict

**Severity:** HIGH
**Owner:** Engineering
**Target phase:** Design (must be resolved before first absorption PR is opened)

**Description:**
`wicked-memory` and `wicked-knowledge` currently depend on the crates.io-published `wicked-estate-core v0.12.0`. When these crates are absorbed as workspace members via path dependencies, their transitive dependency trees must be unified under a single version of each shared crate. The most likely conflict points are:

- `rusqlite` — all four stores use it; a version mismatch causes compile errors or, worse, runtime ABI incompatibilities.
- `tokio` — runtime version mismatches cause executor incompatibilities.
- `tree-sitter` — used by knowledge indexing; its C grammar bindings are version-sensitive.
- `serde` / `serde_json` — derive macros are sensitive to minor version differences.

If two workspace members require incompatible versions of the same crate, Cargo will either fail to resolve (good — visible) or silently compile two copies (bad — hidden, causes type identity failures at runtime when passing structs across crate boundaries).

**Mitigation:**
1. Before opening any absorption PR, run `cargo tree --workspace -d` (duplicate detection) on a branch that adds the donor crate as a path member.
2. Resolve all duplicates in `[workspace.dependencies]` by pinning to the highest compatible version that satisfies all consumers.
3. If a version conflict cannot be resolved without a semver bump in a donor crate, that bump is treated as a pre-work item and done before absorption.
4. Add a CI check (`cargo deny check bans`) that enforces no duplicate crate versions in the workspace.

---

### RISK-002 — Single-Writer Constraint Breaks Under Unified Process

**Severity:** MEDIUM
**Owner:** Engineering
**Target phase:** Build (integration test must exist by the time the first multi-store PR merges)

**Description:**
SQLite's WAL mode serialises writes per file, but it does not prevent multiple connections from being opened to the same file within a single process. If the unified server accidentally opens two write handles to the same SQLite file — for example, if both the estate domain and the overlay domain independently try to open `xedge.db` — WAL lock contention will manifest as `SQLITE_BUSY` errors under load, and in the worst case as silent data corruption if both connections attempt schema migrations.

The risk is highest for `xedge.db` because `XedgeStore` (from wicked-overlay) and any estate code that reads overlay data must coordinate through a single handle.

**Mitigation:**
1. Each domain owns its store handle exclusively. `XedgeStore` is constructed exactly once at server startup and stored in an `Arc<XedgeStore>` that is cloned into each handler that needs it.
2. No domain may open a raw `rusqlite::Connection` to a file that another domain's store type owns. Enforce this via code review and, where possible, by making the `Connection` private to the store struct.
3. Add an integration test at the workspace level that asserts — using `/proc/self/fd` on Linux or `lsof` equivalent on macOS — that exactly one file descriptor is open per store file after server startup.
4. Add a startup assertion: if two store paths resolve to the same canonical path, the server panics with a clear error message rather than opening two connections.

---

### RISK-003 — Skill Resource URI Conflict

**Severity:** MEDIUM
**Owner:** Engineering
**Target phase:** Design (URI scheme must be decided before absorption)

**Description:**
The MCP `resources` namespace is flat: each resource has a URI, and URIs must be globally unique within the server. wicked-memory and wicked-knowledge each define skill resources. If either uses a URI prefix like `skill://` or `skills/` without a domain qualifier, the unified server will expose duplicate or shadowed resources. The MCP spec does not define behaviour for duplicate resource URIs — clients may silently take the first or last match.

**Mitigation:**
1. Before any absorption PR, audit all skill resource URIs defined in wicked-memory and wicked-knowledge.
2. Canonical URI scheme confirmed (source-verified in wicked-knowledge v0.12.1): `skill://{name}/SKILL.md` — name is the bare skill slug (e.g. `memory-capture`, `knowledge-ingest`). Domain-prefix qualifiers are embedded in the slug, not the URI scheme, so no two skills can share a URI if slugs are unique. **No rename required for absorbed crates** — both wicked-memory and wicked-knowledge already use this scheme.
3. Add a startup assertion that panics if two registered resource URIs are identical.
4. Document the final URI scheme in an ADR.

---

### RISK-004 — wicked-estate-memory-api Stub API Surface Conflict

**Severity:** LOW
**Owner:** Engineering
**Target phase:** Design (must be resolved during the design phase before the memory crate is absorbed)

**Description:**
The current wicked-estate workspace already contains a crate named `wicked-estate-memory-api`. This crate appears to be a stub or interface definition layer. Its exact API surface is unknown until read. If the stub defines types, trait bounds, or function signatures that conflict with the absorbed `wicked-estate-memory` crate's implementation, the workspace will not compile after absorption without a breaking change to one or the other.

Two failure modes:
- **Stub is more restrictive than implementation**: the absorbed crate does not satisfy the trait bounds; compilation fails.
- **Stub is broader than implementation**: the stub exposes a public API that the absorbed crate does not implement; callers that depend on the stub will break.

**Mitigation:**
1. Read and document the full public API surface of `wicked-estate-memory-api` at the start of the Design phase (see RAID ISSUE-001).
2. Decide which wins: if the stub is the interface spec, the absorbed crate must conform to it (and the stub is kept as the public facade). If the stub is vestigial, it is deleted and replaced by the absorbed crate's own public API.
3. Record the decision in an ADR before the absorption PR is opened.
4. If the stub is kept, its tests must pass against the absorbed implementation.

---

### RISK-005 — Archiving Before Unified Server Is Tested

**Severity:** LOW
**Owner:** Engineering / Project lead
**Target phase:** Evidence (mitigation is procedural — enforced by the DoD sequence)

**Description:**
If any source repository is archived before `wicked-estate v0.13.0` is released and smoke-tested, users who have pinned to `wicked-memory-mcp` or `wicked-knowledge-mcp` will be left with no working MCP server. The archived repos are read-only, so a quick patch release to the old repos is no longer possible.

This risk is low probability (it requires the archive protocol to be followed out of order) but high impact (user-visible outage).

**Mitigation:**
1. The DoD (REQ-005 §3) sequences Level 3 items so that archive actions cannot be checked before the wicked-qe gate passes and `v0.13.0` is tagged and released.
2. The archive protocol in REQ-004 §3.2 explicitly states: "Archive happens AFTER the unified server ships and passes tests — never before."
3. The adversarial reviewer for the Evidence phase must verify that the release tag exists and the crates.io publish is live before approving any archive action.

---

## Assumptions

Assumptions are beliefs held to be true that have not been formally verified. Each assumption must be validated during the target phase. If an assumption is found to be false, it is promoted to an Issue.

---

### ASSM-001 — Store Isolation Is a Hard Invariant

**Owner:** Architecture
**Target phase:** Requirements (validate now; any divergence opens a new requirement)
**Status:** Active

The design decision DEC-1 (store isolation: each domain owns its own SQLite file) is a permanent, non-negotiable invariant. No future version of wicked-estate will co-mingle estate, memory, knowledge, or xedge nodes in the same SQLite file. Cross-domain queries are served by the unified process joining in-memory, not by cross-file SQLite joins.

**Validation:** Confirm this is documented in an ADR and that no current or planned feature requires cross-file joins.

---

### ASSM-002 — Tool Names, Parameter Names, and Env Vars Are Frozen Contracts

**Owner:** Engineering
**Target phase:** Requirements (validate now)
**Status:** Active

Tool names, parameter names, parameter types, and environment variable names are frozen for all v0.13.x releases. No breaking changes are permitted. This assumption underlies the entire backward-compatibility verification in Level 2 of the DoD.

**Validation:** Confirm that no planned v0.13.0 feature requires renaming or restructuring any tool or env var. If a rename is needed, it must be deferred to v0.14.0 with an alias in v0.13.x.

---

### ASSM-003 — wicked-brain (JS) Operates Independently in v0.13.0

**Owner:** Engineering (wicked-brain team)
**Target phase:** Requirements
**Status:** VALIDATED — 2026-07-05

The wicked-brain JavaScript package (digital brain, indexed items, bus integration) continues to operate independently throughout v0.13.0. Its migration path to call wicked-estate MCP tools directly (instead of calling wicked-memory-mcp or wicked-knowledge-mcp as separate processes) is a separate work item tracked under a v0.2 milestone.

**Validation (2026-07-05):** Source-verified in the wicked-brain server implementation. wicked-brain does NOT call wicked-memory-mcp or wicked-knowledge-mcp. It operates as a standalone JS HTTP server with its own SQLite store and file-based memory system. No wicked-brain release is blocked on wicked-estate v0.13.0. Assumption confirmed — no action required in v0.13.0 scope.

---

### ASSM-004 — wicked-understanding (Skills Layer) Is Unaffected

**Owner:** Engineering (wicked-understanding team)
**Target phase:** Requirements
**Status:** Active

wicked-understanding (the agentskills.io skills layer) operates at the skill definition level, not the MCP tool level. Its skill definitions call MCP tools by name. As long as tool names are frozen (ASSM-002), wicked-understanding requires no changes when users migrate from the per-domain servers to wicked-estate.

**Validation:** Audit wicked-understanding skill definitions to confirm they reference tools by name only (no hardcoded `command` or server process assumptions).

---

### ASSM-005 — PostgreSQL Backend Remains ADR'd but Unimplemented in v0.13.0

**Owner:** Architecture
**Target phase:** Design
**Status:** Active

The architectural decision to support a PostgreSQL store backend has been recorded but is not scheduled for implementation in v0.13.0. The v0.13.0 scope is SQLite-only for all four stores.

**Validation:** Confirm the ADR is in place and that no v0.13.0 feature depends on PostgreSQL. If a consumer requires PostgreSQL in v0.13.0, this becomes a scope change requiring requirements revision.

---

### ASSM-006 — Python and TypeScript Bindings Are Excluded from the Workspace

**Owner:** Engineering
**Target phase:** Requirements
**Status:** Active

The Python bindings (`wicked-memory-py`) and TypeScript bindings (`wicked-memory-ts`) are not absorbed into the wicked-estate workspace. They are archived together with `wicked-memory`. Consumers of these bindings must be notified and either migrate to calling the MCP server directly or maintain their own forks.

**Validation:** Identify any known consumers of `wicked-memory-py` or `wicked-memory-ts`. If active consumers exist, a migration notice must be published before archiving.

---

## Issues

Issues are known problems that require active resolution before a specific phase can be completed. Each issue has an owner responsible for driving resolution and a target phase by which it must be closed.

---

### ISSUE-001 — wicked-estate-memory-api Crate Interface Decision Pending

**Status:** SCOPE-VERIFIED — ADR pending (Design phase)
**Owner:** Engineering
**Target phase:** Design (ADR must be recorded before absorption PR is opened)
**Raised:** 2026-07-05
**Updated:** 2026-07-05

**Description:**
The `wicked-estate-memory-api` crate exists in the current workspace. Its public API surface has been verified (2026-07-05): the crate defines a clean `MemoryApi` trait with `CaptureRequest`, `RecallQuery`, and `RecalledItem` types and `capture` / `recall` methods. This is the intended integration seam — the estate binary calls through this trait; the absorbed `wicked-estate-memory` crate will implement it.

**Scope is now known.** The remaining open item is recording the ADR confirming the decision: keep `wicked-estate-memory-api` as the interface spec, with `wicked-estate-memory` implementing the trait. No deletion or merge is required.

**Resolution required:**
1. ~~Read the crate's `src/lib.rs` and `Cargo.toml`.~~ ✓ Done 2026-07-05.
2. ~~Document the public API surface.~~ ✓ `MemoryApi` trait: `CaptureRequest`, `RecallQuery`, `RecalledItem`, `capture/recall`.
3. Make and record a decision in an ADR: keep as interface spec (confirmed direction), delete as vestigial, or merge into the absorbed crate.
4. Write a test that the absorbed implementation satisfies the stub's interface.
5. No deletion expected — no `wicked-estate-memory-api` cleanup needed.

**Acceptance criteria:** An ADR exists confirming `wicked-estate-memory-api` as the interface spec. The absorbed `wicked-estate-memory` crate compiles cleanly against the trait. The conformance test passes.

---

### ISSUE-002 — wicked-memory-core crates.io Deprecation

**Status:** OPEN
**Owner:** Engineering
**Target phase:** Evidence (must be closed before archive of wicked-memory)
**Raised:** 2026-07-05

**Description:**
`wicked-memory-core` is published to crates.io at v0.12.0 and may be depended on by external packages. After the wicked-memory repository is archived, no further patch releases can be published to the `wicked-memory-core` name. Consumers who `cargo update` after the archive will continue to use `0.12.0` (no change), but they will have no upgrade path.

Yanking `0.12.0` is destructive and breaks consumers who have pinned to it. Yanking is not the correct response.

**Resolution required:**
1. Identify known dependents of `wicked-memory-core` on crates.io (use `crates.io/api/v1/crates/wicked-memory-core/reverse_dependencies`).
2. Publish a final `0.12.2` patch release of `wicked-memory-core` to crates.io before archiving. This release must:
   - Set `deprecated = true` in `Cargo.toml` (crates.io honours this field).
   - Update `README.md` to point to `wicked-estate-memory-core` (or `wicked-estate` if no separate library crate is published).
   - Include a `CHANGELOG.md` entry dated at deprecation.
3. Do NOT yank `0.12.0` or `0.12.1`.

**Acceptance criteria:** A `wicked-memory-core v0.12.2` release exists on crates.io with `deprecated = true`. The `wicked-memory` repository is not archived until this release is live.

---

### ISSUE-003 — Version Strategy for Library Crates

**Status:** OPEN
**Owner:** Engineering + Architecture
**Target phase:** Design (decision required before workspace Cargo.toml is finalised)
**Raised:** 2026-07-05

**Description:**
The workspace must decide whether library crates (e.g., `wicked-estate-memory-core`, `wicked-estate-knowledge-core`, `wicked-estate-overlay`) are published to crates.io as separate packages, or whether they are workspace-internal only (embedding consumers must install the full `wicked-estate` binary).

Publishing library crates:
- Pro: external Rust programs can embed the memory or knowledge store without running the full MCP server.
- Con: increases release surface; each library crate must maintain its own version, changelog, and semver guarantees.
- Con: published library crates cannot be yanked from crates.io without impacting consumers; once published, the API is a frozen contract.

Not publishing library crates:
- Pro: simpler release process; a single binary version to track.
- Con: external consumers who want library access must vendor the crate or pin to a git dependency.
- Con: incompatible with the intended crates.io ecosystem positioning of wicked-*.

**Resolution required:**
1. Determine if any known external consumers require library-level access (not just MCP tool access) to the memory or knowledge domains.
2. If yes, define the library crate publish policy: which crates, under what names, with what API stability guarantee.
3. Record the decision in an ADR.
4. Set `publish = false` in the workspace `Cargo.toml` for any library crate that is not yet ready for crates.io publishing. Do not leave the `publish` field unset (default is `true`, which would publish on `cargo publish`).

**Interim decision (2026-07-05):** All absorbed library crates (`wicked-estate-memory-core`, `wicked-estate-knowledge-core`, `wicked-estate-overlay`) will carry `publish = false` in the workspace `Cargo.toml` until the ADR is recorded and the publish policy is finalised. This prevents accidental crates.io publishing during the absorption PR window.

**Acceptance criteria:** An ADR exists. Every library crate in the workspace has an explicit `publish` field in its `Cargo.toml`. No accidental publishing occurs on `cargo publish -p <crate>`.

---

## Dependencies

Dependencies are external pre-conditions that the project cannot control but must track. Blocked items are escalated to the project lead if a dependency slips.

---

### DEP-001 — wicked-estate v0.12.0 at HEAD, Tests Green

**Owner:** Engineering
**Target phase:** Build (must be satisfied before absorption begins)
**Status:** Pending verification

wicked-estate v0.12.0 is the merge base for the unified workspace. Before any absorption PR is opened, the v0.12.0 codebase must be at HEAD on the default branch, `cargo test --workspace` must be 100% green, and there must be no uncommitted changes in the workspace.

If any test in the baseline is failing at the time absorption begins, it is impossible to distinguish baseline failures from absorption-introduced failures. This dependency must be resolved first.

---

### DEP-002 — wicked-memory v0.12.1 and wicked-knowledge v0.12.1 at HEAD

**Owner:** Engineering (wicked-memory and wicked-knowledge maintainers)
**Target phase:** Build (must be satisfied before absorption of respective crates)
**Status:** Pending verification

Both donor repos must be at HEAD with all tests green before their crates are absorbed. Any in-flight work on these repos must be completed and merged before the absorption tag is cut. After the absorption tag, the donor repos enter a maintenance-only mode: no new features, emergency fixes only.

---

### DEP-003 — wicked-overlay v0.12.0 at HEAD

**Owner:** Engineering (wicked-overlay maintainer)
**Target phase:** Build (must be satisfied before overlay absorption)
**Status:** Pending verification

Same constraint as DEP-002 for the wicked-overlay repository.

---

### DEP-004 — Final Deprecation Commits on All 7 Archive Candidates

**Owner:** Engineering
**Target phase:** Evidence (before archive actions are executed)
**Status:** Not started

All 7 repositories (wicked-memory, wicked-knowledge, wicked-overlay, wicked-orchestration, wicked-council, wicked-governance, wicked-apps-core) must have a final deprecation commit on their default branch before the GitHub archive action is triggered. The archive action makes the repo read-only; a missing deprecation commit cannot be added after archive without unarchiving, which requires manual intervention.

The deprecation commit content requirements are defined in REQ-004 §3.1.

---

### DEP-005 — wicked-estate v0.13.0 Passes wicked-qe Gate Before Any Archive

**Owner:** Engineering / QE
**Target phase:** Evidence (gate condition for archive actions)
**Status:** Not started

No repository may be archived until:

1. The `v0.13.0` tag is pushed to the wicked-estate remote.
2. The `wicked-estate` binary is published to crates.io at `0.13.0`.
3. The wicked-testing wicked-qe gate verdict is PASS or CONDITIONAL with zero CRITs.
4. The adversarial review of the Build phase is PASS or CONDITIONAL with zero CRITs.

This dependency enforces the sequencing requirement in REQ-004 §3 and REQ-005 Level 3. It is the primary guard against RISK-005.

---

## Revision History

| Version | Date | Author | Change |
|---------|------|--------|--------|
| 0.1 | 2026-07-05 | mike.parcewski@gmail.com | Initial draft — all items populated from project context |
