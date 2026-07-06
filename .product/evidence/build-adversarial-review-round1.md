# Build Adversarial Review — Round 1

**Date:** 2026-07-06  
**Reviewer scope:** Wave E diff vs DES-001 (contract conformance, HC-007 frozen fields, DEC-1 single-writer, ADR-ESTATE-008/009/010, §8.4 path guard)  
**Workspace SHA (HEAD):** 2b447be (pre-Wave-E base); Wave E changes uncommitted at review time  
**Review outcome:** PASS (after fixes)

---

## Review method

Parallel adversarial lenses applied to all Wave E files:

1. **HC-007 wire-format lens** — verify each tool returns the exact frozen JSON field names
2. **ADR compliance lens** — ADR-ESTATE-008/009/010, DEC-1, §8.5 all-or-nothing domain failure
3. **Integration seam lens** — `handle_request_unified` signature, SemanticSearch wiring, resources/prompts
4. **Test coverage lens** — new code in Wave E has matching test assertions
5. **Spec deviation lens** — DES-001 §8.4 path collision guard

Each finding was verified against source before being classified.

---

## Findings — Round 1

### CRIT-1 · SemanticSearch broken in unified path (fixed)

**File:** `crates/wicked-estate-mcp/src/lib.rs` (original `handle_request_unified`) and `src/main.rs`

**Defect:** `handle_request_unified` dispatched `SemanticSearch` with `semantic=None`, producing a `-32602` error for any SemanticSearch call. Meanwhile `tools_list_unified` correctly advertised SemanticSearch when the dim-guard passed — inconsistent: advertised but uncallable. The live `_semantic` Arc in `main.rs` was constructed but never threaded into the unified dispatch (the underscore prefix suppressed the "unused" warning, hiding the bug).

**Verification:** MCP protocol invariant — every tool in `tools/list` MUST be callable. This is a regression from the pre-Wave-E `handle_request_with_semantic` path.

**Fix applied:**  
- Added `semantic: Option<&dyn RetrievalTool>` parameter to `handle_request_unified`  
- SemanticSearch arm wired to `handle_tools_call_ctx(&id, &params, store, ctx, semantic)`  
- `main.rs`: renamed `_semantic` → `semantic`; threaded `semantic.as_ref().map(|s| s.as_ref() as &dyn RetrievalTool)` into `handle_request_unified`

---

### CRIT-2 · No unit tests for `handle_request_unified` (fixed)

**File:** `crates/wicked-estate-mcp/src/lib.rs` (test module)

**Defect:** Zero tests covered the Wave E dispatch surface. Critical paths with no coverage:
- ADR-ESTATE-008: `domains=None` must return JSON-RPC error `-32601` (not `isError:true`)
- `tools_list_unified` with `domains_available=true` must show 23 tools (10+6+7)
- Positive dispatch path via fake DomainHandles
- `resources/list`, `resources/read`, `prompts/get` round-trips

**Verification:** `cargo test -p wicked-estate-mcp` showed 35 tests, none covering `handle_request_unified`.

**Fix applied:** 12 new tests added:
- `unified_no_domains_memory_tool_returns_json_rpc_error` (ADR-ESTATE-008)
- `unified_no_domains_knowledge_tool_returns_json_rpc_error` (ADR-ESTATE-008)
- `unified_estate_tools_work_without_domains` (estate still works in degraded mode)
- `unified_tools_list_with_domains_returns_23_tools` (10+6+7 = 23)
- `unified_tools_list_without_domains_returns_10_tools` (estate-only = 10)
- `unified_memory_capture_responds_via_fake_domain` (HC-007: `memory_id` field present)
- `unified_resources_list_and_read` (bundled skills round-trip)
- `unified_prompts_get_expedition` (MCP prompt protocol)
- `FakeMemory` and `FakeKnowledge` trait stubs for isolation

---

### SIG-1 · Missing path collision guard (fixed)

**File:** `crates/wicked-estate-mcp/src/main.rs`

**Defect:** DES-001 §8.4 requires `assert_no_store_path_collision()` to be called after resolving all 4 store paths. The implementation resolved the paths but performed no collision check. If two env vars pointed to the same file (e.g. `WICKED_ESTATE_DB == WICKED_MEMORY_DB`), two engines would open the same SQLite file as their single-writer store, causing silent data corruption.

**Verification:** `grep assert_no_store_path_collision main.rs` returned empty before fix.

**Fix applied:** Inline path collision guard in `main.rs` after resolving `estate/memory/knowledge/xedge` paths. Skips `:memory:` paths (each gets its own in-memory DB; collision on `:memory:` string is false-positive). Uses `std::fs::canonicalize` with fallback to raw `PathBuf` for files that don't exist yet (fail-closed: they'll fail to open shortly after anyway).

---

## HC-007 wire-format audit — PASS

All 13 domain tool wire responses verified against frozen schemas:

| Tool | Expected | Actual | Status |
|---|---|---|---|
| `memory.capture` | `{"memory_id":"..."}` | ✅ line 78 tools/memory.rs | PASS |
| `memory.learn` | `{"memory_id":"..."}` | ✅ line 159 tools/memory.rs | PASS |
| `memory.erase` | `{"deleted_count":N}` | ✅ line 138 tools/memory.rs | PASS |
| `memory.recall` | `{items:[{memory_id, scope, content, tier, score}]}` | ✅ lines 101-108 tools/memory.rs | PASS |
| `memory.reflect` | `{scope, distilled_facts, node_count}` | ✅ serde of ReflectResult | PASS |
| `memory.coverage` | `{total, by_tier, by_kind}` | ✅ serde of MemoryCoverage | PASS |
| `knowledge.ingest` | `{"doc_id":"..."}` | ✅ line 66 tools/knowledge.rs | PASS |
| `knowledge.write` | `{"node_id":"..."}` | ✅ line 80 tools/knowledge.rs | PASS |
| `knowledge.relate` | `{"edge_id":"..."}` | ✅ line 97 tools/knowledge.rs | PASS |
| `knowledge.relate_code` | `{"xedge_count":N}` | ✅ line 146 tools/knowledge.rs | PASS |
| `knowledge.recall` | `{items:[{node_id,class,label,body_snippet,score}]}` | ✅ serde of KnowledgeItem | PASS |
| `knowledge.coverage` | `{total,by_class,recall_miss_count}` | ✅ serde of KnowledgeCoverage | PASS |
| `knowledge.recall_about_code` | `{items:[...]}` | ✅ line 158-161 tools/knowledge.rs | PASS |

**ADR-ESTATE-009** (id→memory_id at wire boundary): `item.id` mapped to `"memory_id"` key in recall dispatch ✅  
**ADR-ESTATE-009** (scope echo from request params): `"scope": scope` (from args, not per-item field) ✅  
**ADR-ESTATE-010** (fetch_epochs before capture/learn/relate_code): all three dispatchers call `fetch_epochs(store, ...)` before the engine method ✅  
**ADR-ESTATE-008** (domains=None → JSON-RPC error): `err_response(&id, -32601, "memory domain not available")` — NOT `isError:true` ✅

---

## DEC-1 single-writer audit — PASS

Each engine opens its own path:
- `estate.db` via `wicked_estate::open_async_store(&db_path)` + `SqliteStore::open(&db_path)` (separate handles, both open the SAME file — this is correct: one async read pool + one sync read for epoch lookups; neither is the memory/knowledge/xedge writer)
- `memory.db` via `MemoryEngine::open(&memory_path)` — owns its own rusqlite Connection
- `knowledge.db` via `KnowledgeEngine::open(&knowledge_path)` — owns its own rusqlite Connection
- `xedge.db` via `XedgeStore::open(&xedge_path)` — owns its own rusqlite Connection with Mutex

No two engines share a write connection. ✅

---

## §8.5 all-or-nothing domain failure — PASS

`domains_result` closures opens all three domain stores (xedge → memory → knowledge). If ANY fails, the whole closure returns `Err(...)`, `domain_engines = None`, and both the memory and knowledge domain tools return `-32601` from `handle_request_unified`. Estate tools continue serving. ✅

---

## OnceLock + URI uniqueness — PASS

`resources.rs` line 49-58: `BUNDLED_SKILLS.get_or_init` sorts URIs and calls `assert_ne!` on adjacent pairs. 6 distinct skill URIs, no duplicates. ✅

---

## Gate results (post-fix)

```
cargo build --workspace        → 0 errors, 0 warnings
cargo test --workspace         → 1011 passed, 0 failed, 1 ignored (LSP doctest)
cargo clippy --workspace --all-targets -- -D warnings → 0 errors, 0 warnings
```

---

## Verdict: PASS

All CRITs and SIGs addressed and verified. No open findings.

Wave E build adversarial review: **PASS**
