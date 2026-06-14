# ADR-006 — OpenTelemetry Observability Adapter Seam

**Status:** Accepted (design); data model + trait + NoopSink + factory built — OTLP network
exporters designed, not built · **Date:** 2026-06-13  
**Implements:** `crates/wicked-estate-core/src/observability.rs`  
**Relates to:** ADR-003 (storage backend seam / vendor-pluggability pattern), ADR-004 (no secret storage / observe-only).

---

## Context

The engine is local-first (single binary, embedded SQLite). As teams adopt it, operators will
want telemetry shipped to their existing observability platform — Datadog, Honeycomb, Grafana
Cloud, New Relic, AWS X-Ray / CloudWatch, Azure Monitor, GCP Cloud Trace. Rather than hard-coding
a vendor SDK or building a full runtime pipeline speculatively, this ADR fixes the **minimal seam**
that makes any of these backends pluggable with a single impl + a single factory arm, zero caller
changes — identical to the storage-backend pattern in ADR-003.

The second constraint is consistency with ADR-004: the engine must never persist secrets. Vendor
auth tokens are runtime configuration, supplied by the operator from environment variables or a
secrets manager, and are never written to the graph store.

---

## Decision

### 1. Data model fidelity (OTel 1.x spec)

The data model in `wicked_estate_core::observability` mirrors the OpenTelemetry proto types faithfully but
without a proto or SDK crate dependency — plain Rust structs with `serde`. This lets the seam and
its tests compile and run in every environment without pulling in gRPC or async runtimes at the
spine layer.

**Attribute layer:**
- `AttributeValue` — the OTel `AnyValue` union: `Str`, `Bool`, `Int`, `Double`, `StrArray`, `IntArray`.
- `KeyValue` — named attribute carrier.
- `Resource` — the logical entity producing telemetry; `Resource::service(name, version)` populates
  `service.name`, `service.version`, `telemetry.sdk.name`, `telemetry.sdk.language = "rust"`.
- `InstrumentationScope` — identifies the library emitting telemetry (e.g. `"wicked_estate_retrieve"`).
- Public `const` keys for every OTel semantic convention the engine will emit: `SERVICE_NAME`,
  `SERVICE_VERSION`, `SERVICE_INSTANCE_ID`, `TELEMETRY_SDK_NAME`, `TELEMETRY_SDK_LANGUAGE`,
  `TELEMETRY_SDK_VERSION`, `CODE_FUNCTION`, `CODE_FILEPATH`, `CODE_LINENO`, `DB_SYSTEM`,
  `DB_STATEMENT`, `DB_OPERATION`.

**W3C TraceContext identity:**
- `TraceId([u8; 16])` and `SpanId([u8; 8])` with `to_hex()` / `from_hex()` for W3C `traceparent`
  header encoding (`00-{trace_id_32chars}-{span_id_16chars}-{flags_2chars}`).
- `SpanContext { trace_id, span_id, trace_flags, is_remote }` with `to_traceparent()`.

**Spans (OTel proto `Span`):**
- `SpanKind` — `Internal | Server | Client | Producer | Consumer` (default: `Internal`).
- `StatusCode` — `Unset | Ok | Error`; `SpanStatus { code, message }`.
- `SpanEvent { name, time_unix_nano, attributes }` — timestamped annotation.
- `SpanLink { context, attributes }` — causality reference to another span.
- `SpanData` — the complete completed-span record.

**Metrics (OTel metrics model):**
- `InstrumentKind` — `Counter | UpDownCounter | Histogram | Gauge | Observable*`.
- `AggregationTemporality` — `Delta | Cumulative`.
- `MetricValue` — `I64 | F64`.
- `NumberDataPoint` and `HistogramDataPoint` — the two point types.
- `MetricData` — `Sum { data_points, temporality, is_monotonic }`, `Gauge { data_points }`,
  `Histogram { data_points, temporality }`.
- `Metric { name, description, unit, data }`.

**Logs (OTel logs model):**
- `SeverityNumber` — six-value simplified enum mapping OTel's 1–24 scale:

  | Variant | OTel range |
  |---------|-----------|
  | Trace   | 1–4       |
  | Debug   | 5–8       |
  | Info    | 9–12      |
  | Warn    | 13–16     |
  | Error   | 17–20     |
  | Fatal   | 21–24     |

- `LogRecord { time_unix_nano, observed_time_unix_nano, severity_number, severity_text, body,
  attributes, trace_id, span_id }`.

**Export config and result types:**
- `Protocol` — `OtlpGrpc | OtlpHttpProtobuf | OtlpHttpJson`.
- `Compression` — `None | Gzip`.
- `Sampler` — `AlwaysOn | AlwaysOff | TraceIdRatio(f64) | ParentBased(Box<Sampler>)`.
- `ExporterConfig { protocol, endpoint, headers, timeout_ms, compression, resource, sampler }`.
  `ExporterConfig::otlp_http(endpoint)` is a convenience constructor for OTLP/HTTP JSON.
- `ExportError::Transient(String)` (retry safe) / `ExportError::Permanent(String)` (do not retry).
- `ExportResult = Result<(), ExportError>`.

### 2. The adapter SPI — `TelemetrySink`

```rust
pub trait TelemetrySink: Send + Sync {
    fn export_spans(&self, resource: &Resource, scope: &InstrumentationScope, spans: &[SpanData]) -> ExportResult;
    fn export_metrics(&self, resource: &Resource, scope: &InstrumentationScope, metrics: &[Metric]) -> ExportResult;
    fn export_logs(&self, resource: &Resource, scope: &InstrumentationScope, logs: &[LogRecord]) -> ExportResult;
    fn force_flush(&self) -> ExportResult;
    fn shutdown(&self) -> ExportResult;
}
```

This mirrors the OTel SDK's `SpanExporter` / `MetricExporter` / `LogExporter` SPIs consolidated
into one trait so a vendor impl can share a single connection pool. Methods take batches; the impl
owns serialisation, compression, retry, and transport.

`Send + Sync` is required: the sink is shared across MCP handler threads (`Arc<dyn TelemetrySink>`).

### 3. Vendor-pluggability — one impl + one factory arm, zero caller changes

The pattern is identical to ADR-003 `open_store`:

```
operator config  ──→  open_telemetry_sink(Some(&cfg))
                               │
             ┌─────────────────┴──────────────────────┐
             │                                        │
         cfg.protocol                              None
         (future arms)                          NoopSink  ← built
             │
         match arm per backend  ← designed, not built
         (OtlpGrpc → opentelemetry-otlp, etc.)
```

Adding Datadog/Honeycomb/Grafana Cloud/X-Ray support:
1. Add a `TelemetrySink` impl (delegating to `opentelemetry-otlp` or a vendor SDK) in a new
   `ci-observe` crate (keeps the heavy OTLP crates out of `wicked-estate-core`).
2. Add one `match` arm in `open_telemetry_sink` on `cfg.protocol` or a vendor discriminator.
3. Zero changes to `wicked-estate-extract`, `wicked-estate-resolve`, `wicked-estate-store`, `wicked-estate-rank`, `wicked-estate-retrieve`, `wicked-estate-mcp`,
   `wicked-estate`, or any entrypoint — they all receive `Box<dyn TelemetrySink>`.

### 4. What is BUILT

| Artefact | Location |
|---|---|
| OTel 1.x data model (all types above) | `crates/wicked-estate-core/src/observability.rs` |
| `TelemetrySink` trait | same |
| `NoopSink` + all five method impls | same |
| `open_telemetry_sink(None) → NoopSink` | same |
| `open_telemetry_sink(Some(cfg)) → Err(Permanent("…designed…"))` | same |
| `ExporterConfig::otlp_http(endpoint)` convenience ctor | same |
| `Resource::service(name, version)` ctor | same |
| `TraceId` / `SpanId` hex encode/decode + round-trip tests | same |
| Full test suite (16 tests) | `observability::tests` |
| `pub mod observability` + re-exports in `wicked_estate_core::lib` | `crates/wicked-estate-core/src/lib.rs` |
| This ADR | `docs/adr/ADR-006-observability-otel-adapter.md` |

### 5. What is DESIGNED, NOT BUILT

These are the next-wave tasks after the instrumentation points are known:

| Item | Notes |
|---|---|
| OTLP gRPC / HTTP network exporters | Requires `opentelemetry-otlp` crate; lives in `ci-observe` (new crate) |
| Engine instrumentation points | index span, query span, blast-radius span, `ci.files.indexed` counter, `ci.query.duration_ms` histogram |
| Batching, retry, back-pressure | Inside the OTLP impl; the trait is already batch-shaped |
| Sampling runtime | `Sampler` config is defined; wiring it to actual sampling decisions is an impl concern |
| OTel Collector config examples | Datadog/Honeycomb/Grafana/X-Ray routing via a sidecar collector |

### 6. Secret-handling rule (ADR-004 consistency)

`ExporterConfig::headers` carries vendor auth tokens at **runtime only**:

```
# Honeycomb
headers = [("x-honeycomb-team", "<key>")]
# Datadog
headers = [("dd-api-key", "<key>")]
# New Relic
headers = [("api-key", "<key>")]
```

These tokens MUST be sourced from environment variables or a secrets manager at startup. They MUST
NOT be written to the graph store, a config file checked into the repo, or any persistent engine
artefact. This is the same rule as ADR-004 §5 ("No secret storage"): the engine observes and
reports; it never persists credentials.

### 7. OTel semantic conventions the engine will emit

When the instrumentation pipeline is built (designed, not built here), these conventions apply:

| Signal | Key | Value |
|--------|-----|-------|
| All | `service.name` | `"wicked_estate"` |
| All | `service.version` | `env!("CARGO_PKG_VERSION")` |
| All | `telemetry.sdk.language` | `"rust"` |
| Span (index) | `code.filepath` | absolute path of indexed file |
| Span (query) | `db.system` | `"sqlite"` (or backend id) |
| Span (query) | `db.statement` | sanitised query string |
| Span (query) | `db.operation` | `"find_symbols"` / `"traverse"` / … |
| Metric | `ci.files.indexed` (counter, unit `"1"`) | files successfully indexed |
| Metric | `ci.query.duration_ms` (histogram, unit `"ms"`) | end-to-end query latency |
| Metric | `ci.blast_radius.depth` (histogram, unit `"1"`) | traversal depth per call |

### 8. Mapping to the Rust `opentelemetry` crate ecosystem

When the OTLP exporter arms are built the types in `wicked_estate_core::observability` map directly:

| This module | `opentelemetry` / proto equivalent |
|---|---|
| `Resource` | `opentelemetry_sdk::Resource` |
| `InstrumentationScope` | `opentelemetry::InstrumentationScope` |
| `TraceId` / `SpanId` | `opentelemetry::trace::{TraceId, SpanId}` |
| `SpanData` | `opentelemetry_sdk::trace::SpanData` |
| `Metric` / `MetricData` | `opentelemetry_sdk::metrics::data::*` |
| `LogRecord` | `opentelemetry_sdk::logs::LogRecord` |
| `TelemetrySink` | `opentelemetry_sdk::export::trace::SpanExporter` + metric/log analogues |

The OTLP impl crate will convert from these local types into the SDK types (or build the proto
messages directly if using `opentelemetry-otlp` in raw mode) inside the match arm. The spine
never touches those crates.

---

## Consequences

- **Local-first stays zero-overhead.** `NoopSink` is the default; no background threads, no
  allocations, no network — the engine's character is unchanged for users who don't configure a
  sink.
- **Drop-in vendor backend.** Any OTLP-compatible backend (the long list in the Context section)
  can be wired without touching the engine's domain logic.
- **Spine stays dependency-light.** `opentelemetry` and `opentelemetry-otlp` crates never enter
  `wicked-estate-core`. They belong in the future `ci-observe` crate behind a Cargo feature flag.
- **Type safety now.** Callers that instrument spans/metrics/logs compile-check their attribute
  keys against the `pub const` list — no stringly-typed attribute names scattered through the
  codebase.
- **No new core seams disturbed.** The five traits in `traits.rs` are untouched. This is a new
  module on the spine, not a modification to any existing one.
