# OTel Instrumentation Audit — wicked_estate

**Date:** 2026-06-15  
**Scope:** Pre-implementation gap analysis before wiring the OTLP exporter arm in `open_telemetry_sink`.  
**Auditor:** read-only, no code changes made.

---

## 1. What the Seam Provides

`crates/wicked-estate-core/src/observability.rs` (1 120 lines) is the complete OTel 1.x data model
and export seam — no runtime OTLP crate dependency, no background threads, no network. Key points:

### Three export channels on one trait

```rust
pub trait TelemetrySink: Send + Sync {
    fn export_spans(&self, resource, scope, spans: &[SpanData]) -> ExportResult;
    fn export_metrics(&self, resource, scope, metrics: &[Metric])  -> ExportResult;
    fn export_logs(&self, resource, scope, logs: &[LogRecord])     -> ExportResult;
    fn force_flush(&self) -> ExportResult;
    fn shutdown(&self)    -> ExportResult;
}
```

Methods take **batches** — the impl owns serialisation, retry, and transport.  
`Send + Sync` is required because the MCP server is multi-threaded.

### Data model completeness

All OTel 1.x primitive types are present:
- **Spans:** `SpanData`, `SpanKind`, `SpanStatus`, `SpanEvent`, `SpanLink`, `SpanContext`, `TraceId`, `SpanId` (W3C traceparent encode/decode).
- **Metrics:** `Metric`, `MetricData` (Sum / Gauge / Histogram), `NumberDataPoint`, `HistogramDataPoint`, `InstrumentKind`, `AggregationTemporality`.
- **Logs:** `LogRecord`, `SeverityNumber` (Trace/Debug/Info/Warn/Error/Fatal), body as `AttributeValue`.
- **Shared:** `Resource`, `InstrumentationScope`, `KeyValue`, `AttributeValue`.

### Semantic-convention constants already published

The module exports pub constants for every key the engine will use:
`SERVICE_NAME`, `SERVICE_VERSION`, `SERVICE_INSTANCE_ID`, `TELEMETRY_SDK_NAME`, `TELEMETRY_SDK_LANGUAGE`, `TELEMETRY_SDK_VERSION`, `CODE_FUNCTION`, `CODE_FILEPATH`, `CODE_LINENO`, `DB_SYSTEM`, `DB_STATEMENT`, `DB_OPERATION`.

Callers use the constants — no stringly-typed attribute keys scattered through call sites.

### Factory state

`open_telemetry_sink(None)` → `Box<NoopSink>` (built, zero overhead).  
`open_telemetry_sink(Some(cfg))` → `Err(ExportError::Permanent("…designed but not built — see ADR-006"))`.  
The OTLP network exporter arm is the only missing piece.

### ExporterConfig

`ExporterConfig::otlp_http(endpoint)` constructs a default config (OtlpHttpJson, 10 s timeout, AlwaysOn sampler, `Resource::service("wicked_estate", env!("CARGO_PKG_VERSION"))`). Three protocol variants: `OtlpGrpc`, `OtlpHttpProtobuf`, `OtlpHttpJson`. gzip compression available.

---

## 2. Current Instrumentation — What Exists Today

**Summary: zero emission sites.** The seam is built; nothing calls into it yet.

The full-codebase grep for every entry-point of the telemetry seam returned only:

| File | Finding |
|---|---|
| `wicked-estate-core/src/lib.rs` | Re-exports `TelemetrySink`, `NoopSink`, `open_telemetry_sink`, and all OTel types (public API surface). |
| `wicked-estate-core/src/observability.rs` | The seam itself — 16 unit tests on the data model and factory. |
| `wicked-estate-extract/src/cloud.rs` | Two doc comments citing `open_telemetry_sink` as a design pattern analogy — not a call site. |

No `tracing::` crate usage anywhere. No `log::` macros. No `metrics::` crate. No `#[instrument]` attributes. No existing span or metric emission anywhere in the pipeline.

**The codebase is observability-dark today.** From a monitoring perspective, every operation is a black box.

---

## 3. Gap Analysis

Operations are grouped by layer. Priority is HIGH for operations that dominate wall-clock time or represent user-visible latency, MEDIUM for supporting operations where measurement is useful but not critical, LOW for housekeeping.

### 3.1 CLI Commands (`crates/wicked-estate/src/main.rs` + `crates/wicked-estate/src/lib.rs`)

Each CLI invocation is a natural root span (SpanKind::Internal). The command is the operation name.

| Command | Priority | What to Instrument |
|---|---|---|
| `index` | **HIGH** | Root span `wicked_estate.index` wrapping the full `index_path` call. Attributes: `code.filepath` (root), `db.path`. End-of-span events: `node_count`, `edge_count`, `file_count` as span attributes. Metric: `wicked_estate.index.files` counter by language. Metric: `wicked_estate.index.duration` histogram (ms). Log (Info) on completion with counts. Log (Error) on any file-level extraction error (currently swallowed silently). |
| `blast-radius` | **HIGH** | Span `wicked_estate.blast_radius` with `symbol.name` attribute. Attributes on completion: `result.count` (resolved dependents), `unresolved.count`. Metric: `wicked_estate.blast_radius.duration` histogram. |
| `query` | **HIGH** | Span `wicked_estate.query` with `query.name` attribute and `result.count`. Metric: `wicked_estate.query.duration` histogram. |
| `rank` / `hotspots` | **MEDIUM** | Span `wicked_estate.rank`. Metric: `wicked_estate.rank.duration`. |
| `semantic` | **MEDIUM** | Span `wicked_estate.semantic_search` with `query.text` (truncated to 200 chars). Attributes: `result.count`, `embedding.available`. |
| `drift` | **MEDIUM** | Span `wicked_estate.drift`. Attributes: `managed.count`, `undeployed.count`, `unmanaged.count`. |
| `watch` (initial + each cycle) | **MEDIUM** | Span `wicked_estate.watch.initial_index`, then per-debounce-cycle span `wicked_estate.watch.reindex`. Metric: `wicked_estate.watch.reindex_count` counter. |
| `scip` | **MEDIUM** | Span `wicked_estate.scip_ingest` per SCIP file. Attribute: `edge.count`. |
| `tfstate` | **LOW** | Span `wicked_estate.tfstate_ingest`. Attribute: `resource.count`. |
| `compact` | **LOW** | Span `wicked_estate.compact`. Attributes: dangling edges pruned, stale cache rows, orphan embeddings. |
| `clusters` | **LOW** | Span `wicked_estate.clusters`. Attribute: `community.count`. |
| `cross-graph` | **LOW** | Span `wicked_estate.cross_graph`. Attributes: `repo.count`, `match.count`. |

**Note on `index`:** The `index_path` function in `lib.rs` already tracks `CI_TIMING` env var for stderr timing prints (lines 180–181). That logic should be replaced by — or supplemented with — the telemetry span, not duplicated.

### 3.2 MCP Tool Calls (`crates/wicked-estate-mcp/src/lib.rs` + `main.rs`)

The MCP server is the primary programmatic interface. Every `tools/call` invocation should produce a child span under a server-level root.

| Tool | Priority | What to Instrument |
|---|---|---|
| `SearchEntity` | **HIGH** | Span `wicked_estate.mcp.SearchEntity` (SpanKind::Server). Attributes: `query.name`, `result.count`, `fts.available`. Metric: `wicked_estate.mcp.tool.duration` histogram, dimension `tool_name=SearchEntity`. |
| `BlastRadius` | **HIGH** | Span `wicked_estate.mcp.BlastRadius`. Attributes: `symbol.id`, `result.count`, `depth`. Metric as above. |
| `TraverseGraph` | **HIGH** | Span `wicked_estate.mcp.TraverseGraph`. Attributes: `symbol.id`, `direction`, `depth`, `node.count`, `truncated` (bool). |
| `RetrieveEntity` | **MEDIUM** | Span `wicked_estate.mcp.RetrieveEntity`. Attributes: `symbol.id`, `found` (bool). |
| `FetchContent` | **MEDIUM** | Span `wicked_estate.mcp.FetchContent`. Attributes: `symbol.id`, `content.bytes` (size). |
| `SemanticSearch` | **MEDIUM** | Span `wicked_estate.mcp.SemanticSearch`. Attributes: `query.text` (truncated), `k`, `result.count`. |
| Cache hit/miss | **HIGH** | SpanEvent `cache.hit` or `cache.miss` on every `tools/call`, plus `wicked_estate.mcp.cache.hits` counter and `wicked_estate.mcp.cache.misses` counter. The L1/L2 cache in `main.rs` is invisible today — a cache hit ratio is critical for understanding MCP server efficiency. |
| `tools/call` errors (`isError: true`) | **HIGH** | Log (Error) with `tool_name`, error message, and the active span's `trace_id`/`span_id` for correlation. |

**MCP server root span:** The stdio `main.rs` loop should open a long-lived server span or, more practically, one span per request. SpanKind::Server is the right kind here (the MCP client is an LLM agent acting as the remote caller).

### 3.3 Storage Layer (`crates/wicked-estate-store/src/sqlite.rs`)

The store is the performance-critical bottleneck. Every expensive operation should be a child span of the calling operation (index, query, blast-radius, etc.).

| Operation | Priority | What to Instrument |
|---|---|---|
| `upsert_nodes_no_fts` (batch write) | **HIGH** | Child span `db.upsert_nodes`. Attributes: `db.system=sqlite`, `db.operation=upsert`, `batch.size` (node count). |
| `rebuild_fts_for_files` | **HIGH** | Child span `db.rebuild_fts`. Attributes: `db.system=sqlite`, `db.operation=fts_rebuild`, `file.count`. This is the operation that was O(2×nodes) before the bulk-rebuild fix (50s→0.085s); monitoring it prevents regression. |
| `find_symbols` (FTS5 BM25 query) | **HIGH** | Child span `db.find_symbols`. Attributes: `db.system=sqlite`, `db.operation=find_symbols`, `db.statement` (sanitised query text), `result.count`. Metric: `wicked_estate.db.query.duration` histogram, dimension `op=find_symbols`. |
| Graph traversal (recursive CTE blast-radius) | **HIGH** | Child span `db.traverse`. Attributes: `db.system=sqlite`, `db.operation=traverse`, `direction`, `depth`, `node.count`. Metric: `wicked_estate.db.query.duration`, `op=traverse`. |
| `upsert_edges` | **MEDIUM** | Child span `db.upsert_edges`. Attribute: `batch.size`. |
| `cache_get` / `cache_put` | **MEDIUM** | SpanEvent on the parent span: `cache.hit` or `cache.miss`. Counters `wicked_estate.db.cache.hits`, `wicked_estate.db.cache.misses`. |
| `compact` (vacuum + prune) | **LOW** | Child span `db.compact`. Attributes: counts for each category pruned. |
| `remove_file` | **LOW** | SpanEvent on the index span: `file.removed` with `file.path` attribute. |

### 3.4 Resolution (`crates/wicked-estate-resolve/src/lib.rs`)

Resolution is a post-extraction pass that runs per-file batch inside `index_path`. Instrumentation here reveals where references fail to resolve (a precision signal).

| Operation | Priority | What to Instrument |
|---|---|---|
| `resolve_all` dispatch | **HIGH** | Child span `resolve.all`. Attribute: `ref.count` (total unresolved refs input). |
| Per-resolver dispatch (`NameResolver`, `ScopedNameResolver`, `ImportMapResolver`, `InfraResolver`) | **MEDIUM** | SpanEvent per resolver on the `resolve.all` span: name, edges produced, duration. Or one child span per resolver if granularity is needed. |
| Unresolved refs remaining after all tiers | **HIGH** | Metric: `wicked_estate.resolve.unresolved` counter, dimension `language`. This is the coverage-gap signal from the agent-behavior rules (R3). It should be surfaced continuously, not only in the `blast-radius` honest-coverage line. |
| SCIP ingestion (`ingest_scip`) | **MEDIUM** | Child span `resolve.scip_ingest`. Attributes: `edge.count`, `scip.path`. |
| LSP on-demand call (`lsp::query`) | **MEDIUM** | Child span `resolve.lsp`. Attributes: `server.language`, `request.kind`, `resolved` (bool), `duration_ms`. |

### 3.5 Extraction (`crates/wicked-estate-extract/src/`)

Extraction is per-file and runs in parallel (rayon). Each file extraction should produce its own short-lived span, or the aggregate should be captured as a metric.

| Operation | Priority | What to Instrument |
|---|---|---|
| Per-file `Extractor::extract` call | **HIGH** | Metric: `wicked_estate.extract.files` counter, dimension `language`. Metric: `wicked_estate.extract.duration` histogram, dimension `language`. This lets you see which language extractors are slow without adding a span per file (10k files × 1 span each would overwhelm a backend). |
| Per-file node/edge counts | **MEDIUM** | Metric: `wicked_estate.extract.symbols` counter, dimension `language`. |
| File skipped (minified guard) | **MEDIUM** | Log (Warn) per skipped file with `code.filepath` attribute. Currently prints to stderr as `SKIPPED_MINIFIED`. |
| Extraction error (parse failure) | **HIGH** | Log (Error) per file with `code.filepath`, `language`, and error message. Currently silently swallowed in the rayon filter_map at `lib.rs:270`. |
| `TreeSitterExtractor` query compilation | **LOW** | SpanEvent on the index root span: `extractor.compile`, `language`, `duration_ms`. Grammar compilation is once per language per process; measuring it reveals cold-start cost. |

### 3.6 Error Paths

Every error path in the pipeline should emit a Log record at Error severity, correlated to the active span. The log body should include the `code.filepath` and `code.function` attributes where known.

Key error paths currently emitting only to stderr or returning `Err` without logging:
- `index_path` when `store.remove_file` fails (non-fatal, logged to stderr).
- `watch` re-index errors (non-fatal, `eprintln!` at line 895).
- MCP `tools/call` returning `isError: true` (no structured log, no trace correlation).
- Any `store.find_symbols`, `store.traverse`, or `store.blast_radius` returning `Err` inside a tool invocation.
- SCIP ingest failures (per-file, currently non-fatal with no structured record).

---

## 4. Recommended Span Names

Follow the OTel semantic conventions already started in ADR-006 §7. All spans are `SpanKind::Internal` unless noted.

| Span name | Kind | Caller |
|---|---|---|
| `wicked_estate.index` | Internal | `wicked_estate index` CLI command |
| `wicked_estate.query` | Internal | `wicked_estate query` CLI command |
| `wicked_estate.blast_radius` | Internal | `wicked_estate blast-radius` CLI command |
| `wicked_estate.rank` | Internal | `wicked_estate rank` CLI command |
| `wicked_estate.semantic_search` | Internal | `wicked_estate semantic` CLI command |
| `wicked_estate.drift` | Internal | `wicked_estate drift` CLI command |
| `wicked_estate.watch.initial_index` | Internal | `wicked_estate watch` on startup |
| `wicked_estate.watch.reindex` | Internal | `wicked_estate watch` per-debounce cycle |
| `wicked_estate.scip_ingest` | Internal | `wicked_estate scip` CLI command |
| `wicked_estate.compact` | Internal | `wicked_estate compact` CLI command |
| `wicked_estate.mcp.SearchEntity` | Server | MCP tools/call |
| `wicked_estate.mcp.RetrieveEntity` | Server | MCP tools/call |
| `wicked_estate.mcp.TraverseGraph` | Server | MCP tools/call |
| `wicked_estate.mcp.BlastRadius` | Server | MCP tools/call |
| `wicked_estate.mcp.FetchContent` | Server | MCP tools/call |
| `wicked_estate.mcp.SemanticSearch` | Server | MCP tools/call |
| `db.upsert_nodes` | Client | `SqliteStore::upsert_nodes_no_fts` |
| `db.rebuild_fts` | Client | `SqliteStore::rebuild_fts_for_files` |
| `db.find_symbols` | Client | `SqliteStore::find_symbols` |
| `db.traverse` | Client | `SqliteStore::traverse` (recursive CTE) |
| `db.upsert_edges` | Client | `SqliteStore::upsert_edges` |
| `db.compact` | Client | `SqliteStore::compact` |
| `resolve.all` | Internal | `resolve_all` in `wicked-estate-resolve` |
| `resolve.lsp` | Client | `lsp::query` on-demand call |
| `resolve.scip_ingest` | Internal | `ingest_scip` |

**Convention:** use dots, not underscores, between namespace segments (`wicked_estate.mcp.BlastRadius`, not `wicked_estate_mcp_BlastRadius`). The `db.*` spans follow the OTel database semantic conventions (`db.system`, `db.operation`, `db.statement` are already declared as constants in the module).

---

## 5. Recommended Metric Names

Follow OTel naming: dots, not underscores; lowercase; include units. All are cumulative counters or histograms unless stated.

| Metric name | Kind | Unit | Dimensions | Notes |
|---|---|---|---|---|
| `wicked_estate.index.duration` | Histogram | `ms` | `root_path` | Full `index_path` wall-clock time |
| `wicked_estate.index.files` | Counter | `1` | `language`, `status` (indexed/skipped/error) | Files processed per index run |
| `wicked_estate.index.nodes` | Counter | `1` | `language` | Symbols emitted per index run |
| `wicked_estate.index.edges` | Counter | `1` | `kind` | Edges emitted per index run |
| `wicked_estate.query.duration` | Histogram | `ms` | (none) | CLI `query` latency |
| `wicked_estate.blast_radius.duration` | Histogram | `ms` | (none) | CLI `blast-radius` latency |
| `wicked_estate.blast_radius.depth` | Histogram | `1` | (none) | Traversal depth per call (already in ADR-006 §7) |
| `wicked_estate.mcp.tool.duration` | Histogram | `ms` | `tool_name` | Per-tool invocation latency (MCP server) |
| `wicked_estate.mcp.cache.hits` | Counter | `1` | `tier` (l1/l2) | L1+L2 cache hits in MCP main loop |
| `wicked_estate.mcp.cache.misses` | Counter | `1` | `tier` (l1/l2) | Cache misses in MCP main loop |
| `wicked_estate.db.query.duration` | Histogram | `ms` | `op` (find_symbols/traverse/upsert_nodes/rebuild_fts) | SQLite operation latency |
| `wicked_estate.db.fts_rebuild.duration` | Histogram | `ms` | (none) | Specifically track the FTS rebuild (was 50s, now 0.085s — regression-sensitive) |
| `wicked_estate.resolve.unresolved` | Counter | `1` | `language`, `tier` (name/import-map/heuristic) | Refs unresolved per `docs/ENGINE-CONTRACT.md` §2.1 (per reference) — precision coverage gap |
| `wicked_estate.resolve.edges` | Counter | `1` | `tier`, `language` | Edges emitted per resolution tier |
| `wicked_estate.extract.duration` | Histogram | `ms` | `language` | Per-language extractor latency (aggregate, not per-file) |
| `wicked_estate.extract.errors` | Counter | `1` | `language` | Files that failed extraction |
| `wicked_estate.watch.reindex_count` | Counter | `1` | (none) | Number of watch-triggered re-index cycles |

**Already named in ADR-006 §7** (use these exact names, they are the authoritative starting point):
- `ci.files.indexed` (Counter, unit `"1"`) → use `wicked_estate.index.files` instead; rename `ci.*` to `wicked_estate.*` for consistent namespace.
- `ci.query.duration_ms` (Histogram, unit `"ms"`) → use `wicked_estate.query.duration`.
- `ci.blast_radius.depth` (Histogram, unit `"1"`) → keep as `wicked_estate.blast_radius.depth`.

The ADR used a `ci.*` prefix but the module itself uses `wicked_estate` as the service name. Align on `wicked_estate.*` throughout for a single namespace.

---

## 6. Implementation Notes

### 6.1 No runtime yet — the seam is batch-shaped, not streaming

`TelemetrySink::export_spans` takes `&[SpanData]`. The engine currently has **no span builder, no active-span stack, no context propagation**. The implementer must add:

1. A `Tracer` utility (or thin wrapper) that can open a span, record attributes/events, and close it — producing a `SpanData` to hand to the sink.
2. A thread-local or `Arc`-passed current span context for propagating `trace_id` / `parent_span_id` down the call chain.
3. A metric accumulator (counter map + histogram map) that flushes to `export_metrics` at the end of each top-level operation or on a timer.
4. Log emission inline at each error/warn site — `LogRecord` creation is cheap since there is no buffering required.

The simplest correct starting point: one `SpanData` per CLI invocation (no child spans initially), one `LogRecord` per error path, one metric flush per invocation end. Add child spans for the expensive operations (FTS rebuild, traversal) once the plumbing works.

### 6.2 The sink must be `Arc`-wrapped for the MCP server

The MCP server in `main.rs` is `#[tokio::main]` with a `store.with_read(move |graph| ...)` closure per request. The `TelemetrySink` must be `Arc<dyn TelemetrySink>` (or `Box`, but Arc is required for sharing across `move` closures). The trait already requires `Send + Sync`.

### 6.3 The `index_path` CI_TIMING env var is a shadow telemetry system

`lib.rs` line 180: `let timing = std::env::var("CI_TIMING").is_ok();` controls `eprintln!` timing prints at several checkpoints. This is a manual, stderr-only version of what spans should replace. When spans are implemented, this env-var path should be removed (§8 retire-as-you-go rule).

### 6.4 Parallelism in `index_path` complicates span parenting

The extraction loop at `lib.rs:270` uses `rayon::par_iter`. Rayon worker threads have no inherent span context. Options:
- Pass the parent `SpanContext` into the parallel closure and create independent child spans for each file (fan-out pattern) — this is correct but verbose.
- Instead, collect per-file metrics (file count, error count, duration) in a `Mutex<Vec<_>>` or `AtomicU64` counters in the rayon closure, then create a single parent span after the loop using the aggregated data. This is simpler and produces less span volume.

The second approach is recommended for the extraction layer. Reserve per-file spans for the MCP and CLI layers where the call volume is low.

### 6.5 MCP cache hit/miss is the highest-value metric

The L1 + L2 cache in `main.rs` (lines 152–183) is the most impactful MCP optimization. It is currently completely invisible: no counter, no log, no span event. A cache hit avoids an entire SQLite round-trip. Instrumenting the cache hit rate (per-tier) is the single highest-ROI telemetry addition for the MCP server.

### 6.6 Staleness signal already exists — wire it to a metric

`wicked_estate::commits_behind` is computed once at MCP startup and injected as a diagnostic string into every tool response (line 321–328, `lib.rs` MCP). Convert this to a gauge:

```
wicked_estate.index.commits_behind  Gauge  "1"  (none)
```

Emit it at startup and after each `watch` re-index cycle. Backends can then alert on `commits_behind > 0` without parsing diagnostic strings.

### 6.7 No `W3C traceparent` propagation path from MCP clients today

The MCP stdio transport is newline-delimited JSON-RPC 2.0 with no HTTP headers. There is no current mechanism for an MCP client (Claude Code, Cursor, etc.) to pass a `traceparent` header for distributed trace correlation. Each MCP request will start a new root span. This is acceptable for now; if trace propagation is needed later, the MCP `initialize` or `tools/call` params could carry it as an extension field.

### 6.8 `InstrumentationScope` naming

Use one scope per crate that produces telemetry:

| Crate | Scope name |
|---|---|
| `wicked-estate` (CLI pipeline) | `wicked_estate` |
| `wicked-estate-mcp` | `wicked_estate_mcp` |
| `wicked-estate-store` | `wicked_estate_store` |
| `wicked-estate-resolve` | `wicked_estate_resolve` |
| `wicked-estate-extract` | `wicked_estate_extract` |

Version = `env!("CARGO_PKG_VERSION")` in each crate.

### 6.9 The new OTLP crate belongs in `wicked-estate-observe`, not `wicked-estate-core`

ADR-006 §3 and §8 are explicit: `opentelemetry` and `opentelemetry-otlp` crates must not enter `wicked-estate-core`. Add a new crate `crates/wicked-estate-observe` (behind a `--features otlp` Cargo flag) that:
1. Implements `TelemetrySink` delegating to `opentelemetry-otlp`.
2. Adds the one match arm in `open_telemetry_sink` (or provides its own factory that wraps the core factory).
3. Links into `wicked-estate` and `wicked-estate-mcp` binaries only.

`wicked-estate-core` stays dependency-light (serde + thiserror only). All other crates receive a `Box<dyn TelemetrySink>` and never see the OTLP types.

---

## 7. Priority Implementation Order

If this is implemented incrementally, the highest-value sequence is:

1. **Arc<dyn TelemetrySink> plumbing** — thread the sink from `main.rs` through the call stack. No spans yet; just prove the wiring compiles.
2. **MCP cache hit/miss counters** — highest-ROI, zero span parenting complexity.
3. **MCP tool-call latency histogram** — one dimension (`tool_name`), one metric per request.
4. **CLI index span + metric flush** — root span for `index`, counter for files/nodes/edges.
5. **Storage layer child spans** — `db.find_symbols`, `db.traverse`, `db.rebuild_fts` as children of the root span.
6. **Error log records** — at every `isError: true` path in MCP, every extraction error in `index_path`.
7. **Resolution coverage metric** — `wicked_estate.resolve.unresolved` counter by language.
8. **Retire `CI_TIMING` env var** — replace the `eprintln!` timing prints with span events.
