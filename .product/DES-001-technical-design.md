---
name: DES-001-technical-design
title: wicked-estate Unified Foundation — Technical Design
status: reviewed
version: 1.1
date: 2026-07-05
author: mike.parcewski@gmail.com
review-required: true
---

# DES-001 — Technical Design

## Purpose

Normative technical design for the wicked-estate Unified Foundation (v0.13.0). Specifies how wicked-memory, wicked-knowledge, and wicked-overlay are absorbed into the wicked-estate Cargo workspace, how the unified MCP stdio server is constructed, and how all backward-compatibility guarantees from REQ-003 §9 are mechanically enforced. Deviation from this design requires a document update and adversarial re-review before the change is implemented.

---

## §1 — Guiding Constraints

All design choices are subordinate to these binding requirements:

- **DEC-1**: Four isolated SQLite files; no cross-file joins; one writer per file.
- **HC-001**: Single process, single binary (`wicked-estate-mcp`).
- **HC-007**: Tool names, parameter names, and parameter types are frozen contracts.
- **REQ-004 §1.3**: Only embedder backends (`fastembed`, `model2vec`) are permitted as Cargo feature flags.
- **REQ-004 §6**: All absorbed library crates carry `publish = false` in v0.13.0.
- **ASSM-002**: No rename of any tool, env var, or parameter.

---

## §2 — Crate Topology: Before → After

### 2.1 — Before (v0.12.0, current workspace — 11 crates)

```
crates/
  wicked-estate-core/        spine — types, traits, conformance kit
  wicked-estate-extract/     tree-sitter Extractor impls
  wicked-estate-resolve/     Resolver impls (import-map, SCIP, TSG, LSP)
  wicked-estate-store/       SqliteStore + MemStore
  wicked-estate-rank/        PageRank Ranker
  wicked-estate-retrieve/    10+1 estate RetrievalTools + reciprocal_rank_fusion + ContextBundle
  wicked-estate-mcp/         MCP server binary (wicked-estate-mcp) — estate tools only
  wicked-estate-observe/     Observability (staleness, metrics)
  wicked-estate-bench/       agent-eval benchmark harness
  wicked-estate-memory-api/  MemoryApi trait seam (CaptureRequest, RecallQuery, MemoryApi)
  wicked-estate/             wicked-estate CLI binary (index, scan, check)
```

### 2.2 — After (v0.13.0 target workspace — 15 crates)

Four crates absorbed, one crate reconciled:

```
crates/
  wicked-estate-core/            UNCHANGED
  wicked-estate-extract/         UNCHANGED
  wicked-estate-resolve/         UNCHANGED
  wicked-estate-store/           UNCHANGED
  wicked-estate-rank/            UNCHANGED
  wicked-estate-retrieve/        UNCHANGED (its `reciprocal_rank_fusion` is estate-side
                                   RRF; knowledge does NOT use it — see dep graph note)
  wicked-estate-observe/         UNCHANGED
  wicked-estate-bench/           UNCHANGED

  wicked-estate/                 EXTENDED — open_async_store, unified server init
  wicked-estate-mcp/             EXTENDED — routes all 24 tools; adds resources +
                                             prompts; adds 4-store startup
  wicked-estate-memory-api/      RECONCILED — becomes a re-export shim:
                                   pub use wicked_estate_memory_core::*;

  wicked-estate-overlay/         NEW — XedgeStore + OverlayReader + ForeignEngine
                                   publish = false
  wicked-estate-memory-core/     NEW — recall pipeline (rrf_fuse, budget_pack,
                                   Candidate, Tier), MemoryApi trait + all shared
                                   types. Canonical home of the fusion/budget math
                                   (wicked-memory-core/src/recall.rs migrates here)
                                   publish = false (see RAID ISSUE-003)
  wicked-estate-memory/          NEW — MemoryEngine implementing MemoryApi
                                   (6 memory.* tool implementations)
                                   publish = false
  wicked-estate-knowledge/       NEW — KnowledgeEngine + 7 knowledge.* tools
                                   (defines KnowledgeApi trait and implementation)
                                   publish = false
```

**Key topology note:** `wicked-estate-memory` and `wicked-estate-memory-core` are distinct crates. `memory-core` holds the shared types (MemoryApi trait, RRF pipeline, CaptureRequest, etc.). `memory` holds the MemoryEngine implementation that uses `memory-core` as a dependency. The existing `wicked-estate-memory-api` becomes a re-export shim pointing to `wicked-estate-memory-core`, preserving any external consumers without a compile break.

**Dependency graph (simplified):**
```
wicked-estate-core
  ↑ used by: extract, resolve, store, rank, retrieve, memory-core,
             memory, knowledge, overlay

wicked-estate-store
  ↑ used by: retrieve, rank, overlay

wicked-estate-overlay
  ↑ used by: memory, knowledge

wicked-estate-memory-core
  ↑ used by: memory, knowledge
  (knowledge DEPENDS on memory-core: wicked-knowledge/src/engine.rs:19 imports
   {Candidate, Tier, budget_pack, rrf_fuse} from wicked_memory_core, per its
   R3 no-second-recall-impl rule. wicked-estate-retrieve's
   reciprocal_rank_fusion is a separate estate-side function knowledge never
   uses — the Round 2 CRIT-3 claim that "rrf_fuse is in retrieve" was wrong.)

wicked-estate-retrieve, wicked-estate-memory,
wicked-estate-knowledge, wicked-estate-overlay
  ↑ used by: wicked-estate-mcp

wicked-estate-memory-api  (re-export shim)
  ↑ used by: wicked-estate-mcp
  (tools/memory.rs imports RecallQuery, RecalledItem, ReflectResult via this shim
   which re-exports from memory-core — no direct memory-core dep needed in mcp)

wicked-estate-mcp
  ↑ used by: wicked-estate (binary)
```

---

## §3 — Absorption Sequence

Five waves. Each wave is a separate PR. No wave may merge until the previous wave's tests are green.

### Wave A — wicked-estate-overlay

**Dependencies satisfied:** wicked-estate-core, wicked-estate-store (both in workspace).

**PR scope:**
1. Create `crates/wicked-estate-overlay/` with absorbed source from wicked-overlay.
2. `publish = false` in Cargo.toml. All shared deps → workspace path deps.
3. `cargo tree --workspace -d` — zero duplicates required before PR merges.
4. `cargo build -p wicked-estate-overlay && cargo test -p wicked-estate-overlay` green.
5. Do NOT wire into wicked-estate-mcp in this PR.

### Wave B — wicked-estate-memory-core

**Dependencies satisfied:** core, store, overlay (Wave A).

**PR scope:**
1. Create `crates/wicked-estate-memory-core/` with the recall pipeline from wicked-memory-core.
2. **Migrate all MemoryApi types here:** `CaptureRequest`, `RecallQuery`, `RecalledItem`, `MemoryCoverage`, and the full extended `MemoryApi` trait (see §4). This is the canonical home.
3. `publish = false`. Workspace path deps.
4. `cargo build -p wicked-estate-memory-core && cargo test -p wicked-estate-memory-core` green.

### Wave C — wicked-estate-memory

**Dependencies satisfied:** core, store, overlay (A), memory-core (B).

**PR scope:**
1. Create `crates/wicked-estate-memory/` with MemoryEngine from wicked-memory.
2. MemoryEngine implements `MemoryApi` from `wicked-estate-memory-core`.
3. MemoryEngine is constructed with `Arc<XedgeStore>` for erase cross-edge cleanup (see §4.3).
4. `cargo build -p wicked-estate-memory && cargo test -p wicked-estate-memory` green.

### Wave D — wicked-estate-knowledge

**Dependencies satisfied:** core, store, retrieve (Embedder/HashEmbedder/VectorStore), overlay (A), and **memory-core (B)** — `wicked-knowledge/src/engine.rs:19` imports `{Candidate, Tier, budget_pack, rrf_fuse}` from `wicked_memory_core` under its R3 no-second-recall-impl rule; those symbols migrate to `wicked-estate-memory-core` in Wave B. Wave D therefore starts only after Wave B. Waves **C and D** are the parallel pair (both depend on B, neither on the other).

**Pre-condition check (RESOLVED 2026-07-05):** The original check ("verify `rrf_fuse` in `wicked-estate-retrieve` is `pub`") was based on a false premise — retrieve has no `rrf_fuse`; its fusion function is `pub fn reciprocal_rank_fusion` (`wicked-estate-retrieve/src/lib.rs:1469`), which wicked-knowledge does not use. The real shared symbols are `pub` in `wicked-memory-core/src/recall.rs` (`rrf_fuse` L12, `Candidate` L27, `budget_pack` L51) and `lib.rs` (`Tier` L31). `wicked-estate-retrieve` remains genuinely UNCHANGED; no visibility change is needed anywhere.

**PR scope:**
1. Create `crates/wicked-estate-knowledge/` with KnowledgeEngine from wicked-knowledge.
2. Defines `KnowledgeApi` trait and `KnowledgeItem`/`KnowledgeCoverage` types (see §5).
3. Redirect imports: `use wicked_memory_core::{...}` → `use wicked_estate_memory_core::{...}`; retrieve imports (`Embedder`, `HashEmbedder`, `VectorStore`) unchanged.
4. `cargo build -p wicked-estate-knowledge && cargo test -p wicked-estate-knowledge` green.

### Wave E — wicked-estate-memory-api reconciliation + wicked-estate-mcp extension

This is the unification PR. It is the largest PR and requires adversarial review of its diff before merge.

**PR scope:**
1. Update `wicked-estate-memory-api/src/lib.rs` to the re-export shim (see §4.1); update its `Cargo.toml` to add `wicked-estate-memory-core` as a path dependency.
2. Retain (do NOT remove) `wicked-estate-memory-api` from `wicked-estate-mcp/Cargo.toml` — `tools/memory.rs` imports `RecallQuery`, `RecalledItem`, and `ReflectResult` through this shim (see §2.2 dep graph). Removing it would produce a compile error.
3. Wire all four new crates into `wicked-estate-mcp` (see §6 — §9).
4. Update `wicked-estate/src/main.rs` for unified server init (see §8).
5. All 24 tools routing and responding correctly.
6. Skills bundled (see §7).
7. `cargo build --workspace && cargo test --workspace` green.

---

## §4 — MemoryApi Trait Extension

### 4.1 — wicked-estate-memory-api becomes a re-export shim

After Wave B, `crates/wicked-estate-memory-api/src/lib.rs` becomes:

```rust
//! Re-export shim — preserved for backward compatibility.
//! All types have moved to `wicked-estate-memory-core`.
pub use wicked_estate_memory_core::{
    CaptureRequest, RecallQuery, RecalledItem, MemoryCoverage, ReflectResult, MemoryApi,
};
```

`ReflectResult` is included in the re-export because `MemoryApi::reflect` now returns it. Any dependent that calls `reflect()` must import or pattern-match the result type.

`wicked-estate-memory-api/Cargo.toml` must also add a path dependency on `wicked-estate-memory-core`:
```toml
[dependencies]
wicked-estate-memory-core = { path = "../wicked-estate-memory-core" }
```

**Breaking change acknowledgement:** The `reflect()` method return type changes from `Result<usize, _>` to `Result<ReflectResult, _>` to satisfy the REQ-003 §2.2 wire contract (`distilled_facts: Vec<String>` cannot be produced from a bare count). This change requires the council gate per REQ-004 §2.3. Existing callers that pattern-match or destructure the usize return must be updated. Since `wicked-estate-memory-api` carries `publish = false`, no crates.io consumers are affected — the blast radius is internal to this workspace.

The crate is kept in the workspace with `publish = false`.

### 4.2 — Extended MemoryApi Trait (in wicked-estate-memory-core)

The current trait (3 methods: capture, recall, reflect) is extended to 6 to cover all `memory.*` MCP tools. The existing method signatures are preserved unchanged — only additions:

```rust
// ── existing (return types corrected to satisfy wire format) ────────────────
pub trait MemoryApi {
    type Error;
    fn capture(&mut self, req: CaptureRequest) -> Result<String, Self::Error>;
    fn recall(&self, q: &RecallQuery)           -> Result<Vec<RecalledItem>, Self::Error>;
    /// Distil a scope into semantic facts and write them as T2-tier nodes.
    /// Returns a ReflectResult carrying the distilled text — `usize` is insufficient
    /// because the wire format (REQ-003 §2.2) requires `distilled_facts: Vec<String>`.
    fn reflect(&mut self, scope: &str, now: i64) -> Result<ReflectResult, Self::Error>;

    // ── new in v0.13.0 ────────────────────────────────────────────────────────
    /// Hard-delete all memory nodes whose scope starts with `scope_prefix`.
    /// Returns `Err` if `scope_prefix` is empty (refuses total wipe).
    /// Implementations MUST also remove associated xedge entries (see §4.3).
    fn erase(&mut self, scope_prefix: &str, now: i64) -> Result<u32, Self::Error>;

    /// Store a T2-tier fact and create about-edges to `symbols` atomically.
    /// Equivalent to capture(kind=fact, tier=T2, scope="", about=symbols) but atomic.
    /// `symbol_epochs` is pre-fetched by the dispatch layer (see §4.5 ADR-ESTATE-010).
    fn learn(&mut self, fact: &str, symbols: &[String],
             symbol_epochs: &std::collections::HashMap<String, u64>, now: i64)
        -> Result<String, Self::Error>;

    /// Return memory counts, optionally scoped. `scope_prefix = None` returns global totals.
    fn coverage(&self, scope_prefix: Option<&str>) -> Result<MemoryCoverage, Self::Error>;
}

// `now: i64` in reflect / erase / learn is Unix timestamp seconds — generated by the
// dispatch layer via `chrono::Utc::now().timestamp()` (or equivalent), NOT extracted from
// request params. The trait accepts it as a parameter to remain testable with fixed clocks.
```

### 4.3 — Shared Return Types (wicked-estate-memory-core)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCoverage {
    pub total:    u32,
    pub by_tier:  std::collections::HashMap<String, u32>,  // e.g. "T0"→3, "T2"→12
    pub by_kind:  std::collections::HashMap<String, u32>,  // e.g. "fact"→8, "decision"→4
}

/// Return type of MemoryApi::reflect — carries distilled facts as required by REQ-003 §2.2.
/// The wire format exposes `{ scope, distilled_facts: Vec<String>, node_count: u32 }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectResult {
    pub scope:           String,
    pub distilled_facts: Vec<String>,   // frozen contract field name — HC-007
    pub node_count:      u32,
}
```

`by_kind` on `MemoryCoverage` is required per REQ-003 §2.2 return schema. Omitting it violates HC-007. `distilled_facts` on `ReflectResult` is similarly frozen — returning `usize` from `reflect` cannot satisfy the wire contract.

### 4.4 — XedgeStore Lifetime in MemoryEngine

`MemoryEngine` (in `wicked-estate-memory`) is constructed with an `Arc<XedgeStore>`. This allows `erase` to clean up cross-edges internally, satisfying both the MemoryApi contract and the DEC-1 single-writer invariant:

```rust
pub struct MemoryEngine {
    store:  MemoryStore,         // write path to memory.db
    xedge:  Arc<XedgeStore>,     // write path to xedge.db (owned by overlay crate)
}
```

The `Arc<XedgeStore>` is constructed once at startup in `main.rs` and shared between MemoryEngine and KnowledgeEngine. This is the single XedgeStore instance for the process — DEC-1 enforced.

### 4.5 — Symbol Epoch Lookup for about-edge Writes

Three operations must write about-edges to `xedge.db` with a `symbol_epoch: u64` that matches the estate symbol's current epoch in `estate.db` — otherwise `OverlayReader` drops the edge as stale (REQ-003 §5.2):

- `memory.capture` when the `about` parameter is provided
- `memory.learn` (always writes about-edges to the `symbols` param)
- `knowledge.relate_code`

**Decision (ADR-ESTATE-010):** The dispatch layer (tools/memory.rs, tools/knowledge.rs) is responsible for fetching current symbol epochs from `estate.db` before calling the trait method. Neither `MemoryEngine` nor `KnowledgeEngine` holds an estate store reference — this keeps the DEC-1 isolation clean. The dispatch functions receive `store: &dyn GraphRead` (already present in `handle_request_unified`) and look up epochs inline.

`CaptureRequest` gains a new field `about_epochs: Option<HashMap<String, u64>>` (optional — `None` when `about` is absent). The complete updated struct fragment:

```rust
pub struct CaptureRequest {
    // existing fields preserved verbatim ...
    pub about:        Option<Vec<String>>,
    pub about_epochs: Option<HashMap<String, u64>>,  // NEW — None when about is None
}
```

**Breaking-change note:** Adding any field to `CaptureRequest` breaks existing struct-literal construction (`CaptureRequest { field_a: .., field_b: .. }` will not compile when a new field has no default). Since `wicked-estate-memory-api` has `publish = false`, the blast radius is internal only. Mitigation: apply `#[non_exhaustive]` to `CaptureRequest` so callers use functional update syntax (`..Default::default()`), or convert all existing construction sites in the same Wave B PR. The council gate per REQ-004 §2.3 covers this alongside the `reflect()` return-type change.

`memory.learn` and `knowledge.relate_code` receive epochs via a new parallel parameter. The `MemoryApi::learn` and `KnowledgeApi::relate_code` signatures are extended:

```rust
// tools/memory.rs — dispatch for memory.capture (about= present)
fn dispatch_capture(..., store: &dyn GraphRead, memory: &mut dyn MemoryApi<Error=anyhow::Error>) -> Value {
    let about: Vec<String> = /* extract params["about"] */;
    let about_epochs = fetch_epochs(store, &about); // HashMap<String, u64>
    let req = CaptureRequest {
        // ... existing fields ...
        about:        Some(about),
        about_epochs: Some(about_epochs),
    };
    match memory.capture(req) {
        Ok(memory_id) => ok_response(id, json!({"memory_id": memory_id})),
        Err(e) => json_rpc_error(id, -32603, e.to_string()),
    }
}

// Updated trait extension for learn:
fn learn(&mut self, fact: &str, symbols: &[String], symbol_epochs: &HashMap<String, u64>,
         now: i64) -> Result<String, Self::Error>;

// Updated KnowledgeApi for relate_code:
fn relate_code(&mut self, knowledge_id: &str, symbol_ids: &[String],
               symbol_epochs: &HashMap<String, u64>) -> anyhow::Result<u32>;
```

`fetch_epochs` performs a `GraphRead::node_by_id` lookup (or equivalent read-only query) for each symbol ID and extracts the `epoch` field. Symbols not found in `estate.db` are omitted from `about_epochs` — the engine skips the about-edge for unknown symbols rather than failing the whole call.

Engines consume `symbol_epochs` by iterating the map and calling `XedgeStore::link` **once per entry** with that entry's individual epoch — never a single `link` call for all symbols at once. A single call would stamp every edge with the same epoch, causing silent data corruption for heterogeneous symbol sets that have different extraction epochs.

The dispatch functions for `memory.capture`, `memory.learn`, and `knowledge.relate_code` receive `store: &dyn GraphRead` alongside the domain handle. The unified dispatch must thread `store` into these calls — the existing `handle_request_unified` signature already carries `store: &dyn GraphRead` (§6.2), so no new argument is needed at the top level.

---

## §5 — KnowledgeApi Trait

`KnowledgeApi` is defined inside `wicked-estate-knowledge` (not a separate crate). It is the write interface for the knowledge domain — the same isolation guarantee as MemoryApi: no MCP handler touches `knowledge.db` directly.

### 5.1 — Types

```rust
/// Metadata for a document to ingest (frozen contract per REQ-003 §2.3).
#[derive(Debug, Serialize, Deserialize)]
pub struct DocMeta {
    pub id:           String,
    pub title:        String,
    pub source_uri:   String,
    pub content_type: String,
}

/// One pre-chunked text chunk.
#[derive(Debug, Serialize, Deserialize)]
pub struct ChunkInput {
    pub id:        String,
    pub section:   String,
    pub text:      String,
    pub metadata:  Option<serde_json::Value>,
}

/// Input for writing a single knowledge node.
#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeNodeInput {
    pub id:       Option<String>,                      // generated if absent
    pub class:    String,                              // doc | section | chunk | concept
    pub label:    String,
    pub body:     String,
    pub metadata: Option<serde_json::Value>,
}

/// One recalled knowledge node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItem {
    pub node_id:      String,
    pub class:        String,
    pub label:        String,
    pub body_snippet: String,
    pub score:        f64,
}

/// Coverage stats for knowledge.coverage (frozen schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCoverage {
    pub total:            u32,
    pub by_class:         std::collections::HashMap<String, u32>,
    pub recall_miss_count: u32,
}
```

### 5.2 — Trait

```rust
pub trait KnowledgeApi {
    /// Ingest a document and its chunks. Idempotent on doc.id.
    fn ingest(&mut self, doc: DocMeta, chunks: Vec<ChunkInput>) -> anyhow::Result<String>;

    /// Upsert a single knowledge node; generates id if None.
    fn write(&mut self, node: KnowledgeNodeInput) -> anyhow::Result<String>;

    /// Create a typed directed relation between two knowledge nodes. Fails if either absent.
    fn relate(&mut self, src_id: &str, tgt_id: &str, rel: &str,
              confidence: f64, provenance: Option<&str>) -> anyhow::Result<String>;

    /// Hybrid recall (FTS5 + ANN via RRF). Truncated to token_budget if Some.
    fn recall(&self, query: &str, token_budget: Option<u32>) -> anyhow::Result<Vec<KnowledgeItem>>;

    /// Coverage stats, optionally filtered to a class.
    fn coverage(&self, class: Option<&str>) -> anyhow::Result<KnowledgeCoverage>;

    /// Create about-edges in xedge.db from a knowledge node to estate symbols.
    /// `symbol_epochs` is pre-fetched by the dispatch layer from estate.db (see §4.5).
    fn relate_code(&mut self, knowledge_id: &str, symbol_ids: &[String],
                   symbol_epochs: &HashMap<String, u64>) -> anyhow::Result<u32>;

    /// Reverse lookup: knowledge nodes about given estate symbol IDs.
    /// No budget parameter — REQ-003 §2.3 frozen contract has none; all matching nodes returned.
    /// `KnowledgeItem.score` for these results is set to the xedge confidence value for the
    /// about-edge, or `1.0` if the edge carries no explicit confidence (deterministic link).
    fn recall_about_code(&self, symbol_ids: &[String]) -> anyhow::Result<Vec<KnowledgeItem>>;
}
```

`KnowledgeEngine` (in `wicked-estate-knowledge`) implements `KnowledgeApi`. Like `MemoryEngine`, it holds an `Arc<XedgeStore>` for the two xedge-writing methods.

---

## §6 — wicked-estate-mcp Dispatch Architecture

### 6.1 — Preserve Existing Interface

The existing public functions in `lib.rs` are preserved without signature changes:

- `handle_request(store, req)` — unchanged (used by existing tests)
- `handle_request_ctx(store, req, ctx)` — unchanged
- `handle_request_with_semantic(store, req, ctx)` — unchanged
- `McpContext` struct — unchanged (5 existing fields retained, no additions)
- `semantic_advert(ctx)` — unchanged

### 6.2 — New Unified Dispatch Function

A new function is added to `lib.rs` that handles all 24 tools:

```rust
pub struct DomainHandles<'a> {
    pub memory:    &'a mut dyn MemoryApi<Error = anyhow::Error>,
    pub knowledge: &'a mut dyn KnowledgeApi,
}

pub fn handle_request_unified(
    store:   &dyn GraphRead,
    req:     &Value,
    ctx:     &McpContext,
    domains: Option<&mut DomainHandles<'_>>,
) -> Value {
    // Parse method and id as before
    let method = /* ... */;

    match method.as_str() {
        "initialize"               => handle_initialize_unified(&id),
        "notifications/initialized" => return Value::Null,
        "tools/list"               => tools_list_unified(&id, ctx, domains.is_some()),
        "resources/list"           => resources_list(&id),
        "resources/read"           => resources_read(&id, /* uri from params */),
        "prompts/list"             => prompts_list(&id),
        "prompts/get"              => prompts_get(&id, /* name from params */),
        "tools/call"               => {
            let tool = /* extract from params */;
            match tool.as_str() {
                // ── estate (10 unconditional + 1 conditional) ───────────────
                "SearchEntity" | "RetrieveEntity" | "TraverseGraph" | "BlastRadius"
                | "FetchContent" | "ContextBundle" | "RulesInventory" | "RankHotspots"
                | "Communities" | "Lineage"
                    => tools::estate::dispatch(tool, params, store, ctx),

                #[cfg(any(feature = "fastembed", feature = "model2vec"))]
                "SemanticSearch"
                    => tools::estate::semantic_search(params, store, ctx),

                // ── memory (6 tools) ──────────────────────────────────────
                "memory.capture" | "memory.recall" | "memory.reflect"
                | "memory.erase" | "memory.learn" | "memory.coverage"
                    => match domains {
                        Some(d) => tools::memory::dispatch(tool, &id, params, store, d.memory),
                        None    => json_rpc_error(&id, -32601, "memory domain not available"),
                    },

                // ── knowledge (7 tools) ───────────────────────────────────
                "knowledge.ingest" | "knowledge.write" | "knowledge.relate"
                | "knowledge.recall" | "knowledge.coverage"
                | "knowledge.relate_code" | "knowledge.recall_about_code"
                    => match domains {
                        Some(d) => tools::knowledge::dispatch(tool, &id, params, store, d.knowledge),
                        None    => json_rpc_error(&id, -32601, "knowledge domain not available"),
                    },

                _ => json_rpc_error(&id, -32602, "method not found"), // INVALID_PARAMS: method is tools/call, the invalid part is the `name` parameter
            }
        },
        _ => json_rpc_error(&id, -32601, "method not found"),
    }
}
```

`domains: None` is the graceful-degradation path when a store fails to open at startup. Estate tools remain available.

### 6.3 — Module Layout and Wire Format Conventions

```
crates/wicked-estate-mcp/src/
  lib.rs              public API (both unified and legacy dispatch)
  main.rs             startup + stdio loop
  tools/
    estate.rs         existing 10+1 estate dispatch (UNCHANGED except signature alignment)
    memory.rs         6 memory.* dispatch — NEW
    knowledge.rs      7 knowledge.* dispatch — NEW
  resources.rs        skills bundled via include_str!() — NEW
  prompts.rs          expedition prompt — NEW
```

**Wire format for primitive-returning tools (frozen per REQ-003 §2.2/§2.3):**

| Tool | Trait return | Wire `result` |
|---|---|---|
| `memory.erase` | `u32` (count deleted) | `{"deleted_count": N}` |
| `memory.learn` | `String` (memory node ID) | `{"memory_id": "..."}` |
| `knowledge.relate_code` | `u32` (edges written) | `{"xedge_count": N}` |
| `knowledge.relate` | `String` (edge ID) | `{"edge_id": "..."}` |
| `memory.capture` | `String` (memory node ID) | `{"memory_id": "..."}` |
| `knowledge.ingest` | `String` (doc ID) | `{"doc_id": "..."}` |
| `knowledge.write` | `String` (node ID) | `{"node_id": "..."}` |

The exact JSON key names are frozen contracts per HC-007 — verify against REQ-003 §2.2/§2.3 before implementation.

### 6.4 — Updated tools/list for Unified Response

```rust
fn tools_list_unified(id: &Value, ctx: &McpContext, domains_available: bool) -> Value {
    let mut tools = estate_tool_schemas();  // 10 unconditional
    #[cfg(any(feature = "fastembed", feature = "model2vec"))]
    if semantic_advert(ctx).is_ok() {       // dim-guard check (existing logic)
        tools.push(semantic_search_schema());
    }
    if domains_available {
        tools.extend(memory_tool_schemas());    // 6
        tools.extend(knowledge_tool_schemas()); // 7
    }
    ok_response(id, json!({"tools": tools}))
}
```

### 6.5 — memory.recall Wire Format Mapping

`RecalledItem` (defined in `wicked-estate-memory-core`) uses `id: String` internally. The MCP wire format for `memory.recall` (per REQ-003 §2.2) exposes this field as `memory_id`. `RecalledItem` also does not carry a `scope` field — scope is a dispatch-level concern.

**Decision (ADR-ESTATE-009):** `tools/memory.rs` builds the wire JSON from `RecalledItem` fields at dispatch time, without modifying `RecalledItem` in `memory-core`. This keeps the re-export shim boundary intact and avoids a breaking change to `wicked-estate-memory-api`.

```rust
// tools/memory.rs — memory.recall handler
fn dispatch_recall(id: &Value, params: &Value, memory: &dyn MemoryApi<Error=anyhow::Error>) -> Value {
    let query = /* extract params["query"] as &str */;
    let scope = params.get("scope").and_then(Value::as_str).unwrap_or("");
    let seeds: Vec<String> = params.get("seeds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let token_budget = params.get("token_budget").and_then(Value::as_u64).map(|n| n as u32);
    let rq = RecallQuery {
        query:        query.to_string(),
        scope:        scope.to_string(),
        seeds,
        token_budget,
    };
    match memory.recall(&rq) {
        Ok(items) => {
            let wire: Vec<Value> = items.into_iter().map(|item| json!({
                "memory_id": item.id,      // id → memory_id rename at wire boundary
                "scope":     scope,         // echoed from request params (not stored on item)
                "content":   item.content,
                "tier":      item.tier,
                "score":     item.score,   // NOT "confidence" — frozen field name per REQ-003 §2.2
            })).collect();
            ok_response(id, json!({"items": wire}))
        },
        Err(e) => json_rpc_error(id, -32603, e.to_string()),
    }
}
```

**Scope semantics (echo, not per-item property):** `scope` is re-echoed from the request params — it is the filter value applied to the query, not the item's individual stored scope. When `scope` is absent from the request (it is `String?` per REQ-003 §2.2), the wire item gets `"scope": ""`, meaning "no scope filter was applied". This is a deliberate design choice: `RecalledItem` does not carry a `scope` field (to avoid a breaking change to the re-export shim), and unscoped queries by definition do not filter by scope. Clients that need per-item scope values must use the `scope` filter on request; items returned for a scoped query all satisfy that scope by construction.

The `id` → `memory_id` rename and `score` (not `confidence`) field name apply to `memory.recall` item mappings at the wire boundary. `memory.reflect` returns a `ReflectResult` with `distilled_facts: Vec<String>` — no `id` or `score` field is present in that response.

---

## §7 — Skills and Prompts

### 7.1 — Skills Bundling via include_str!()

Six skills compiled in at build time. Source files live in the absorbed crates:

| Skill URI | Source path (post-absorption) |
|---|---|
| `skill://codebase-expedition/SKILL.md` | `crates/wicked-estate-memory/skills/codebase-expedition/SKILL.md` |
| `skill://knowledge-ingest/SKILL.md` | `crates/wicked-estate-knowledge/skills/knowledge-ingest/SKILL.md` |
| `skill://ontology-expedition/SKILL.md` | `crates/wicked-estate-knowledge/skills/ontology-expedition/SKILL.md` |
| `skill://knowledge-curation/SKILL.md` | `crates/wicked-estate-knowledge/skills/knowledge-curation/SKILL.md` |
| `skill://cited-answer/SKILL.md` | `crates/wicked-estate-knowledge/skills/cited-answer/SKILL.md` |
| `skill://gap-hunting/SKILL.md` | `crates/wicked-estate-knowledge/skills/gap-hunting/SKILL.md` |

`resources.rs` uses `include_str!("../../wicked-estate-memory/skills/codebase-expedition/SKILL.md")` for the memory skill and `include_str!("../../wicked-estate-knowledge/skills/.../SKILL.md")` for the knowledge skills — paths relative to `wicked-estate-mcp/src/resources.rs`. Skill files live in the implementation crate (`wicked-estate-memory`), not in the shared-types crate (`wicked-estate-memory-core`). Verified correct after absorption.

Skills are initialized into an `OnceLock<Vec<McpResource>>` at first call, not per-request, to guarantee the uniqueness assertion fires exactly once at startup.

### 7.2 — Startup URI Uniqueness Assertion

```rust
static BUNDLED_SKILLS: OnceLock<Vec<McpResource>> = OnceLock::new();

fn bundled_skills() -> &'static Vec<McpResource> {
    BUNDLED_SKILLS.get_or_init(|| {
        let skills = build_skill_list();
        let mut uris: Vec<&str> = skills.iter().map(|s| s.uri.as_str()).collect();
        uris.sort();
        for i in 1..uris.len() {
            assert_ne!(uris[i-1], uris[i],
                "BUG: duplicate skill URI in bundled_skills: {}", uris[i]);
        }
        skills
    })
}
```

### 7.3 — resources/list and resources/read

```rust
fn resources_list(id: &Value) -> Value {
    let skills = bundled_skills();
    let items: Vec<Value> = skills.iter().map(|s|
        json!({"uri": s.uri, "name": s.name(), "mimeType": "text/markdown"})
    ).collect();
    ok_response(id, json!({"resources": items}))
}

fn resources_read(id: &Value, uri: &str) -> Value {
    match bundled_skills().iter().find(|s| s.uri == uri) {
        Some(s) => ok_response(id, json!({
            "contents": [{"uri": uri, "text": s.content}]
        })),
        None => json_rpc_error(id, -32602, format!("resource not found: {uri}")),
    }
}
```

### 7.4 — Expedition Prompt (prompts/list and prompts/get)

The codebase-expedition skill is also registered as an MCP prompt (per REQ-003 §4):

```rust
fn prompts_list(id: &Value) -> Value {
    ok_response(id, json!({"prompts":[{
        "name": "expedition",
        "description": "Hotspot-first codebase exploration: RankHotspots → TraverseGraph → FetchContent",
        "arguments": [
            {"name": "repo_path", "description": "Path to the indexed repo", "required": true}
        ]
    }]}))
}

fn prompts_get(id: &Value, name: &str) -> Value {
    if name != "expedition" {
        return json_rpc_error(id, -32602, format!("prompt not found: {name}"));
    }
    // MCP prompts/get response must wrap content in a `messages` array with role+content shape.
    let skill_text = include_str!("../../wicked-estate-memory/skills/codebase-expedition/SKILL.md");
    ok_response(id, json!({
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": skill_text
            }
        }]
    }))
}
```

`prompts/get` for any name other than `"expedition"` returns a JSON-RPC `-32602` (Invalid Params) error. The `skill_text` is the same compile-time-embedded content used in `resources/read` — no runtime file I/O.

---

## §8 — Store Initialization (main.rs)

### 8.1 — Updated handle_initialize

`handle_initialize_unified` extends the capabilities object to declare `resources` and `prompts`:

```rust
fn handle_initialize_unified(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {},
                "resources": {},
                "prompts": {}
            },
            "serverInfo": {
                "name": "wicked-estate",
                "version": SERVER_VERSION
            }
        }
    })
}
```

The existing `handle_initialize` (capabilities: {tools: {}}) is preserved for backward compatibility with the existing test suite.

### 8.2 — Startup Sequence

```
1.  Parse CLI args / resolve env vars
2.  Resolve all five env vars → four store paths
3.  Assert no two canonical paths are identical (§8.4)
4.  Open estate.db   → SqliteStore (existing)
5.  Open xedge.db    → XedgeStore::open() — fail-soft if unable
6.  Open memory.db   → MemoryStore::open() — fail-soft if unable
7.  Open knowledge.db → KnowledgeStore::open() — fail-soft if unable
8.  Construct MemoryEngine with (MemoryStore, Arc<XedgeStore>) — if both open
9.  Construct KnowledgeEngine with (KnowledgeStore, Arc<XedgeStore>) — if both open
10. Call bundled_skills() to init OnceLock and assert URI uniqueness
11. Enter JSON-RPC stdio loop using handle_request_unified
```

**Notification handling in the stdio loop:** `handle_request_unified` returns `Value::Null` for `notifications/initialized` (and any future notifications). The stdio loop MUST check for `Value::Null` and skip writing it to stdout — writing `null\n` would emit a spurious response to a notification, which is a protocol violation.

```rust
// main.rs — simplified stdio loop
loop {
    let req = read_line_as_json(&mut stdin)?;
    let resp = handle_request_unified(&store, &req, &ctx, domains.as_mut());
    if resp != Value::Null {          // skip writing for notifications
        write_json_line(&mut stdout, &resp)?;
    }
}
```

Fail-soft: if any new store fails to open, log a warning to stderr and set `domains = None`. Estate tools remain available; memory/knowledge tools return a JSON-RPC protocol error (code -32601, see §8.5).

### 8.3 — Path Resolution

```
WICKED_HOME    = $WICKED_HOME or ~/.wicked/
estate.db      = $WICKED_ESTATE_DB    or $WICKED_HOME/estate.db
memory.db      = $WICKED_MEMORY_DB    or $WICKED_HOME/memory.db
knowledge.db   = $WICKED_KNOWLEDGE_DB or $WICKED_HOME/knowledge.db
xedge.db       = $WICKED_XEDGE_DB     or $WICKED_HOME/xedge.db
T0 persist     = $WICKED_MEMORY_T0_PERSIST == "1" (default off)
```

`$WICKED_ESTATE_DB` also accepts a `--db <path>` CLI flag (preserved from v0.12.0).

**One-time breaking change (default path):** v0.12.0 default was `.wicked-estate/graph.db` (CWD-relative). v0.13.0 default is `$WICKED_HOME/estate.db`. Users who relied on CWD-relative auto-creation must either set `WICKED_ESTATE_DB` or move the file. The migration guide (DoD Level 3.4) must include this step, and `wicked-estate`'s CHANGELOG must document it. This breaking change requires the council gate per REQ-004 §2.3.

### 8.4 — Path Collision Guard

```rust
fn assert_no_store_path_collision(paths: &[(&str, &str)]) {
    let canonical: Vec<_> = paths.iter()
        .map(|(name, p)| (*name, std::fs::canonicalize(p).unwrap_or_else(|_| p.into())))
        .collect();
    for i in 0..canonical.len() {
        for j in (i+1)..canonical.len() {
            if canonical[i].1 == canonical[j].1 {
                panic!("Store path collision: {} and {} both resolve to {:?}",
                    canonical[i].0, canonical[j].0, canonical[i].1);
            }
        }
    }
}
// called with:
// [("estate.db", &estate_path), ("memory.db", &memory_path),
//  ("knowledge.db", &knowledge_path), ("xedge.db", &xedge_path)]
```

### 8.5 — Domain Failure Policy (All-or-Nothing)

**Rule:** If any of the three domain stores fails to open — `xedge.db`, `memory.db`, or `knowledge.db` — then `domains = None` for the entire server session. There is no partial-domain mode.

**Rationale:** `XedgeStore` is constructed once and shared (`Arc<XedgeStore>`) between both `MemoryEngine` and `KnowledgeEngine`. If `xedge.db` cannot be opened, neither engine can be safely constructed — there is no design for a memory-without-overlay or knowledge-without-overlay path. If `xedge.db` opens but `memory.db` or `knowledge.db` fails, constructing one engine but not the other produces an inconsistent `DomainHandles` that the type system cannot express (both fields are required, not `Option`). The all-or-nothing policy is therefore structural, not an arbitrary restriction.

**Startup sequence deviation:** If any domain store fails, steps 8–9 are skipped and `domains = None` is passed to the stdio loop. Steps 1–4 (estate store) are unaffected — estate tools continue serving normally.

**Stderr diagnostic format:** Each failed store emits a single line to stderr before the server enters the stdio loop. Format:

```
[wicked-estate] WARN: {store_name} store unavailable ({path}): {error_message}
[wicked-estate] WARN: memory and knowledge tools disabled (domains=None)
```

Example:
```
[wicked-estate] WARN: xedge store unavailable (/home/user/.wicked/xedge.db): unable to open database file
[wicked-estate] WARN: memory and knowledge tools disabled (domains=None)
```

The `[wicked-estate] WARN:` prefix ensures the diagnostic is greppable in MCP host logs without noise.

**Effect on tools/list:** When `domains = None`, `tools_list_unified` omits all 13 memory and knowledge tool schemas. Clients receive only 10 (or 11 with embedder) estate tools. Omitting is preferable to listing tools that always return `isError` — the latter misleads agents into retrying permanently unavailable tools.

**Effect on tools/call with a stale client:** A `tools/call` for any memory or knowledge tool name when `domains = None` returns a JSON-RPC error (not `isError` in `result`). The message is domain-specific:

```json
// for memory.* tools:
{"jsonrpc":"2.0","id":<id>,"error":{"code":-32601,"message":"memory domain not available"}}

// for knowledge.* tools:
{"jsonrpc":"2.0","id":<id>,"error":{"code":-32601,"message":"knowledge domain not available"}}
```

This handles the case where a client cached `tools/list` before the domain failure was observed.

---

## §9 — Feature Flag Propagation

### 9.1 — wicked-estate-mcp/Cargo.toml

```toml
[features]
fastembed = ["wicked-estate-retrieve/fastembed"]
model2vec = ["wicked-estate-retrieve/model2vec"]
```

No new feature flag deps in wicked-estate-mcp itself. The MCP crate forwards the flag to wicked-estate-retrieve where the embedder implementation lives.

### 9.2 — SemanticSearch Conditional Compilation

```rust
// tools/list — add conditionally (canonical form: #[cfg] on the outer if, not the inner push):
#[cfg(any(feature = "fastembed", feature = "model2vec"))]
if semantic_advert(ctx).is_ok() {
    tools.push(semantic_search_schema());
}

// tools/call — dispatch arm:
#[cfg(any(feature = "fastembed", feature = "model2vec"))]
"SemanticSearch" => tools::estate::semantic_search(params, store, ctx),
```

In the default build, both arms are removed at compile time. A `tools/call` for `SemanticSearch` returns `-32601 Method Not Found` — correct per MCP spec.

---

## §10 — Archival Protocol

After v0.13.0 is released to crates.io and all DoD Level 3 items are checked:

| Repository | Action | Prerequisite |
|---|---|---|
| wicked-memory | GitHub archive (private, then archived) | v0.13.0 live; deprecation README committed; wicked-memory-core v0.12.2 published to crates.io with `deprecated=true` (RAID ISSUE-002) |
| wicked-knowledge | GitHub archive | v0.13.0 live; deprecation README |
| wicked-overlay | GitHub archive | v0.13.0 live; deprecation README |
| wicked-orchestration | GitHub archive | Superseded; no active users |
| wicked-council | GitHub archive | Superseded; no active users |
| wicked-governance | GitHub archive | Superseded; no active users |
| wicked-apps-core | GitHub archive | Superseded; no active users |

Archive procedure (identical to wicked-agent precedent): if public → unarchive → set private → re-archive. If already private → archive directly.

---

## §11 — Key Design Decisions

| ID | Decision | Rationale |
|---|---|---|
| ADR-ESTATE-001 | McpContext is not changed; DomainHandles is a separate struct | Preserves existing estate tool path, 10+ tests, and dim-guard logic unchanged |
| ADR-ESTATE-002 | MemoryApi extended with erase/learn/coverage (type-safe isolation) | MCP handlers must never touch memory.db directly — type system enforces the invariant |
| ADR-ESTATE-003 | KnowledgeApi defined inside wicked-estate-knowledge (not a new crate) | knowledge is publish=false and internal; a separate crate would be premature |
| ADR-ESTATE-004 | wicked-estate-memory-api becomes a re-export shim | Backward-compatibility for type imports (CaptureRequest, RecallQuery, RecalledItem) without code changes. `reflect()` return type is a documented breaking change (usize→ReflectResult); council gate required per REQ-004 §2.3; blast radius is internal only (publish=false). |
| ADR-ESTATE-005 | MemoryEngine / KnowledgeEngine hold Arc<XedgeStore> at construction | erase cleanup needs xedge write access; holding Arc satisfies single-writer without trait leaking |
| ADR-ESTATE-006 | Skills bundled via include_str!() in OnceLock | Self-contained binary; uniqueness assertion fires exactly once at startup |
| ADR-ESTATE-007 | Default store path changed to $WICKED_HOME/estate.db | CWD-relative paths are unsafe for MCP stdio; host working directory is not user-controlled |
| ADR-ESTATE-008 | Domain tools unavailable (domains=None) returns a JSON-RPC protocol error (`{"error":{"code":-32601,...}}`), not an MCP `isError` result | JSON-RPC error is the correct response when the method is structurally unavailable; `isError` is reserved for tool-call failures on available tools. Estate tools continue serving. |
| ADR-ESTATE-009 | tools/memory.rs maps RecalledItem.id → memory_id and echoes scope from request params at wire boundary | RecalledItem is the re-export shim boundary; extending it would break any external consumer pinned to wicked-estate-memory-api |
| ADR-ESTATE-010 | Dispatch layer fetches symbol_epoch from estate.db before calling capture/learn/relate_code | Keeps DEC-1 clean — engines hold no estate store reference; dispatch already has store: &dyn GraphRead |

---

## §12 — Open Questions

| ID | Question | Phase | Notes |
|---|---|---|---|
| OQ-DES-001 | Should Wave E be split into a skeleton PR (compiles, all dispatch arms present, all return NOT_IMPLEMENTED) followed by a full-wiring PR? | Design | Preferred: yes. Reduces diff size per review. |
| OQ-DES-003 | Does wicked-estate-knowledge currently expose a single KnowledgeStore::open() entry point, or does it require the caller to pass a raw connection? | Build | Verify during Wave D before the KnowledgeEngine wrapper is written. |

---

## Revision History

| Version | Date | Author | Change |
|---|---|---|---|
| 0.1 | 2026-07-05 | mike.parcewski@gmail.com | Initial design draft |
| 0.2 | 2026-07-05 | mike.parcewski@gmail.com | Round 1 adversarial review fixes: McpContext preserved; correct crate topology (5 waves, wicked-estate-memory separate from core); corrected KnowledgeApi signatures; MemoryCoverage by_kind added; memory-api re-export shim; skills OnceLock; capabilities update; prompts design added |
| 0.3 | 2026-07-05 | mike.parcewski@gmail.com | Round 2 adversarial review fixes: prompt name expedition (C1); id propagation to all 6 new MCP helpers (C2); Wave D knowledge does not depend on memory-core, rrf_fuse is in retrieve (C3); removed budget param from recall_about_code (S1); §6.5 memory.recall wire format mapping id→memory_id + scope echo from params (S2); §8.5 all-or-nothing domain failure policy, stderr diagnostic format, tools/list omission, ADR-ESTATE-009 (S3) |
| 0.4 | 2026-07-05 | mike.parcewski@gmail.com | Round 3 adversarial review fixes: fixed recall call to RecallQuery struct (CRIT-1); id param added to all json_rpc_error calls in §6.2 (CRIT-2); wire field score not confidence (CRIT-3); added ReflectResult struct + corrected reflect return type in §4.2/§4.3 (CRIT-4); codebase-expedition skill path moved to wicked-estate-memory not memory-core (CRIT-5); params token_budget/seeds replace limit in §6.5 (CRIT-6); rrf_fuse location clarified in §2.2 crate description (SIG-1); #[cfg(fastembed/model2vec)] guard added to semantic_search_schema in §6.4 (SIG-2); ADR-ESTATE-008 corrected to JSON-RPC error not isError (SIG-3); notification null-skip specified in §8.2 stdio loop (SIG-4) |
| 0.5 | 2026-07-05 | mike.parcewski@gmail.com | Round 4 adversarial review fixes: prompts_get response shape specified with messages array (SIG-1); §4.1 re-export shim adds ReflectResult + Cargo.toml dep note + breaking change acknowledgement with council gate (SIG-2 + MINOR-1 + MINOR-3); ADR-ESTATE-004 updated to qualify backward-compat claim; now:i64 dispatcher-generation documented in §4.2 (MINOR-2) |
| 0.6 | 2026-07-05 | mike.parcewski@gmail.com | Round 5 adversarial review fixes: §6.5 scope echo semantics documented (unscoped→"", intentional) (SIG-1); §2.2 dep graph adds memory-api shim as mcp dep; Wave E note retains memory-api dep (SIG-2); Wave D rrf_fuse visibility pre-condition check added (SIG-3); recall_about_code score semantics specified in §5.2 (MINOR-3); §9.2 #[cfg] canonical form aligned with §6.4 (MINOR-1) |
| 0.7 | 2026-07-05 | mike.parcewski@gmail.com | Round 6 adversarial review fixes: §4.5 symbol_epoch lookup design specified (dispatch layer fetches epochs from estate.db via GraphRead, threads through to capture/learn/relate_code), CaptureRequest.about_epochs added, learn/relate_code signatures extended, ADR-ESTATE-010; reflect MINOR §6.5 prose corrected to not claim id→memory_id applies to reflect |
| 0.8 | 2026-07-05 | mike.parcewski@gmail.com | Round 7 adversarial review fixes: §4.2 learn signature updated to match §4.5 extended form with symbol_epochs (CRIT-1); §6.2 dispatch calls to memory/knowledge modules now pass store for epoch lookup (CRIT-2); §4.5 CaptureRequest.about_epochs typed as Option<HashMap> with full struct fragment, #[non_exhaustive] mitigation noted, council gate reference added (SIG-1) |
| 0.9 | 2026-07-05 | mike.parcewski@gmail.com | Round 8 adversarial review fixes: §4.5 per-symbol link iteration specified (call link once per symbol_epochs entry, not once for all) (SIG-1); §7.2 uri.as_str() fixed (MINOR-1); §6.3 wire format table added for u32-returning tools (MINOR-3) |
| 1.0 | 2026-07-05 | mike.parcewski@gmail.com | Round 9 adversarial review fix: §6.3 wire format table — memory.capture and memory.learn corrected from node_id to memory_id (HC-007 CRIT). Round 10 PASS — minor cleanup: Wave E step numbering, §6.3 table header, §8.5 example message clarified, §4.5 pseudocode Some() wrappers added. Status: reviewed. |
| 1.1 | 2026-07-05 | mike.parcewski@gmail.com | Pre-build gate correction (new source evidence, reverses design-review Round 2 CRIT-3): wicked-knowledge imports {Candidate, Tier, budget_pack, rrf_fuse} from wicked_memory_core (wicked-knowledge/src/engine.rs:19, Cargo.toml R3 rule) — NOT from wicked-estate-retrieve, whose fusion fn is the separate `reciprocal_rank_fusion` (lib.rs:1469). §2.1/§2.2 crate listings, §2.2 dep graph, and §3 Wave D corrected: wicked-estate-knowledge depends on wicked-estate-memory-core; wave order A → B → (C ∥ D) → E; rrf_fuse visibility pre-condition replaced with resolved ground truth; retrieve genuinely UNCHANGED. Tool contracts, wire formats, and all other sections unaffected. |
