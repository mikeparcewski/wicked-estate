# Evidence Phase Adversarial Review

**Date:** 2026-07-06  
**Scope:** DoD L1 + L2 + L3 evidence artifacts, per REQ-005  
**Review outcome:** PASS (after fixes)

---

## Review method

Independent lens review of every DoD item against its required evidence artifact.
For each item, the reviewer asked: "does the evidence artifact prove the claim, or
could it pass trivially or be fabricated?"

---

## Level 1 — Implementation Complete

### §1.1 MCP Tool Surface

| Check | Evidence | Verdict |
|---|---|---|
| 23 tools in default build | `conf_tool_count_10_estate_6_memory_7_knowledge` PASS | ✓ |
| Estate tools (10): SearchEntity..Lineage | `conf_all_23_tool_names_present_in_tools_list` PASS | ✓ |
| Memory tools (6): memory.capture..coverage | `conf_memory_tool_required_fields_match_goldens` PASS | ✓ |
| Knowledge tools (7): knowledge.ingest..recall_about_code | `conf_knowledge_tool_required_fields_match_goldens` PASS | ✓ |

### §1.2 Skills Surface

| Check | Evidence | Verdict |
|---|---|---|
| 6 skills bundled as MCP resources | `unified_resources_list_and_read` unit test PASS | ✓ |
| URI uniqueness assertion | `resources.rs:58–65` OnceLock `assert_ne!` on adjacent sorted URIs | ✓ |

### §1.3 Expedition Prompt

| Check | Evidence | Verdict |
|---|---|---|
| prompts/list includes expedition | `unified_prompts_get_expedition` unit test PASS | ✓ |
| prompts/get returns correct content | Same unit test asserts SKILL.md text present | ✓ |

### §1.4 Store Lifecycle

| Check | Evidence | Verdict |
|---|---|---|
| 4 stores open cleanly | `sc009.rs::sc009_all_four_stores_open_and_all_reads_return_nonempty` PASS | ✓ |
| Single-writer per store | `DEC-1 audit` in `build-adversarial-review-round1.md` | ✓ |
| §8.5 all-or-nothing domain | `unified_no_domains_memory_tool_returns_json_rpc_error` unit test | ✓ |

### §1.5 Absorbed Crates

| Check | Evidence | Verdict |
|---|---|---|
| wicked-estate-overlay absorbed | `cargo build --workspace` clean; overlay tests pass | ✓ |
| wicked-estate-memory-core absorbed | 6 memory-api unit tests pass | ✓ |
| No duplicate types | `cargo tree -d` no wicked-* workspace crate duplicates; pre-existing transitive dups documented in `docs/tree-dupes.md` | ✓ |
| StoreTrait (SC-008) | `unified_estate_tools_work_without_domains` uses in-memory MemStore as trait object | ✓ |

### §1.6 Build Health

| Gate | Result | Evidence |
|---|---|---|
| `cargo build --release` | 0 warnings | `evidence/ci-build-log.txt` |
| `cargo test --workspace` | 1025 passed, 0 failed | `evidence/ci-build-log.txt` |
| `cargo clippy -- -D warnings` | 0 warnings | `evidence/ci-build-log.txt` |
| `cargo fmt --check` | CLEAN | `evidence/ci-build-log.txt` |

---

## Level 2 — Backward Compatibility Verified

### §2.1–2.3 Tool Contracts

**Finding during review:** Schema conformance test (`conf_memory_tool_required_fields_match_goldens`) initially FAILED on 3 memory tools with wrong parameter names vs frozen v0.12.x golden:

| Tool | Bug | Fix |
|---|---|---|
| `memory.erase` | schema + dispatcher read `"scope"` → should be `"scope_prefix"` | Fixed in `lib.rs:754` + `tools/memory.rs:174` |
| `memory.learn` | schema + dispatcher used `"fact"` → should be `"content"` (wire name) | Fixed in `lib.rs:755` + `tools/memory.rs:191` |
| `memory.coverage` | schema + dispatcher read `"scope"` → should be `"scope_prefix"` | Fixed in `lib.rs:756` + `tools/memory.rs:216` |

Post-fix: all 5 conformance tests PASS. Evidence: `evidence/conformance-results.json`.

### §2.4 Per-Domain Count

`conf_tool_count_10_estate_6_memory_7_knowledge` PASS. Evidence: `evidence/conformance-results.json`.

### §2.5 Environment Variable Contracts

7 subprocess-level tests in `env_vars.rs` PASS:
- ENV-001 `WICKED_ESTATE_DB` custom path: file created at custom path ✓
- ENV-002 `WICKED_MEMORY_DB` custom path: file created at custom path ✓
- ENV-003 `WICKED_KNOWLEDGE_DB` custom path: file created at custom path ✓
- ENV-004 `WICKED_XEDGE_DB` custom path: file created at custom path ✓
- ENV-006 no env vars → default path `.wicked-estate/graph.db`: file created ✓
- SMOKE-001 memory migration: all 6 memory.* tools present ✓
- SMOKE-002 knowledge migration: all 7 knowledge.* tools present ✓

### §2.6 Existing Database Compatibility

`sc009.rs::db_compat_all_fixture_stores_open_without_error` PASS.
All 4 v0.12.x fixture databases open without migration error. Evidence: `evidence/sc009-output.txt`.

### §2.7 MCP Config Migration Smoke Tests

SMOKE-001 and SMOKE-002 subprocess tests PASS. Evidence: `evidence/ci-build-log.txt`.

---

## Level 3 — Evidence and Archive

### §3.1 Quality Gate

| Gate | Result | Evidence |
|---|---|---|
| wicked-testing QE gate | PASS (7/7 assertions, 0 CRITs) | `.wicked-testing/evidence/run-estate-20260706/verdict.json` |
| Build adversarial review | PASS (3 findings fixed) | `evidence/build-adversarial-review-round1.md` |
| SC-009 integration test | PASS (all 4 stores + all 4 reads non-empty) | `evidence/sc009-output.txt` |

### §3.2 Deprecation Notices

| Repo | Commit SHA | Status |
|---|---|---|
| `wicked-memory` | 02e1243 | local; push pending user auth |
| `wicked-knowledge` | d13862c | local; push pending user auth |
| `wicked-overlay` | 6cb5a15 | local; push pending user auth |

All 3 deprecation notices point to `wicked-estate v0.13.0` with migration instructions.

### §3.3 Archive Actions

Pending user authorization. All 7 repos ready for archive:
`wicked-memory`, `wicked-knowledge`, `wicked-overlay`, `wicked-orchestration`,
`wicked-council`, `wicked-governance`, `wicked-apps-core`

### §3.4 Release

Version bumped to 0.13.0. Git tag v0.13.0 to be created.
GitHub release and crates.io publish pending user authorization.

### §3.5 Marketplace and Tooling Updates

`.claude-plugin/marketplace.json` updated to reflect unified 23-tool surface, all 3 domains,
6 skills, 1 prompt, env var reference, migration instructions, version 0.13.0.

---

## Verdict: PASS (pending §3.3–§3.4 external actions)

All L1 and L2 items: **PASS** with evidence artifacts present.  
L3.1 quality gate: **PASS**.  
L3.2 deprecation notices: **PASS** (3 commits recorded; push requires user authorization).  
L3.3–L3.4: **PENDING** — require external GitHub/crates.io actions with user authorization.  

The project is ready to proceed to archive and release.
