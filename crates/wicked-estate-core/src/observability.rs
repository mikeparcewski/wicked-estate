//! OpenTelemetry-standard observability adapter seam.
//!
//! # What is built
//! * The **OTel 1.x data model** (`Resource`, `SpanData`, `Metric`, `LogRecord`, …) — plain Rust
//!   data types, no external `opentelemetry` crate required.
//! * The [`TelemetrySink`] trait — the vendor-pluggable export seam (mirrors OTel SDK
//!   `SpanExporter`/`MetricExporter`/`LogExporter` SPI).
//! * [`NoopSink`] — the zero-overhead default; every method returns `Ok(())`.
//! * [`open_telemetry_sink`] — the factory seam. `None` → `NoopSink` (built). `Some(cfg)` → the
//!   matching OTLP network arm returns a clear "designed, not built" error, exactly like
//!   `wicked_estate_store::open_store` for Postgres (see ADR-003).
//!
//! # What is designed, not built
//! * The OTLP gRPC / HTTP network exporters (delegating to `opentelemetry-otlp`).
//! * The engine's **tracer / meter emission pipeline**: instrumentation points (index/query/
//!   blast-radius spans, counters for parsed files, histogram for query latency).
//! * Batching, retry, and sampling at runtime.
//!
//! Adding a vendor backend = one `TelemetrySink` impl + one match arm in [`open_telemetry_sink`];
//! zero caller changes (same pattern as ADR-003 `open_store`).
//!
//! # Secret-handling rule
//! [`ExporterConfig::headers`] carries vendor auth tokens at **runtime** (e.g. `x-honeycomb-team`,
//! `dd-api-key`) and is **never persisted to the graph store** — consistent with ADR-004's
//! no-secret-storage rule.

use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────
// § 1  OTel semantic-convention key constants
// ────────────────────────────────────────────────────────────────────

/// `service.name` — OTel resource semantic convention.
pub const SERVICE_NAME: &str = "service.name";
/// `service.version` — OTel resource semantic convention.
pub const SERVICE_VERSION: &str = "service.version";
/// `service.instance.id` — OTel resource semantic convention.
pub const SERVICE_INSTANCE_ID: &str = "service.instance.id";
/// `telemetry.sdk.name` — OTel resource semantic convention.
pub const TELEMETRY_SDK_NAME: &str = "telemetry.sdk.name";
/// `telemetry.sdk.language` — OTel resource semantic convention (value: `"rust"`).
pub const TELEMETRY_SDK_LANGUAGE: &str = "telemetry.sdk.language";
/// `telemetry.sdk.version` — OTel resource semantic convention.
pub const TELEMETRY_SDK_VERSION: &str = "telemetry.sdk.version";
/// `code.function` — OTel code semantic convention.
pub const CODE_FUNCTION: &str = "code.function";
/// `code.filepath` — OTel code semantic convention.
pub const CODE_FILEPATH: &str = "code.filepath";
/// `code.lineno` — OTel code semantic convention.
pub const CODE_LINENO: &str = "code.lineno";
/// `db.system` — OTel database semantic convention.
pub const DB_SYSTEM: &str = "db.system";
/// `db.statement` — OTel database semantic convention.
pub const DB_STATEMENT: &str = "db.statement";
/// `db.operation` — OTel database semantic convention.
pub const DB_OPERATION: &str = "db.operation";

// ────────────────────────────────────────────────────────────────────
// § 2  Attributes — OTel AnyValue / KeyValue
// ────────────────────────────────────────────────────────────────────

/// OTel `AnyValue` — the union of scalar and array attribute values.
/// Corresponds to `opentelemetry_proto::common::v1::AnyValue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum AttributeValue {
    /// UTF-8 string.
    Str(String),
    /// Boolean.
    Bool(bool),
    /// Signed 64-bit integer.
    Int(i64),
    /// IEEE-754 64-bit float.
    Double(f64),
    /// Array of UTF-8 strings.
    StrArray(Vec<String>),
    /// Array of signed 64-bit integers.
    IntArray(Vec<i64>),
}

/// OTel `KeyValue` — a named attribute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyValue {
    /// Attribute key (e.g. `"service.name"`).
    pub key: String,
    /// Attribute value.
    pub value: AttributeValue,
}

impl KeyValue {
    /// Convenience constructor for a string attribute.
    pub fn str(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: AttributeValue::Str(value.into()),
        }
    }

    /// Convenience constructor for an integer attribute.
    pub fn int(key: impl Into<String>, value: i64) -> Self {
        Self {
            key: key.into(),
            value: AttributeValue::Int(value),
        }
    }

    /// Convenience constructor for a boolean attribute.
    pub fn bool(key: impl Into<String>, value: bool) -> Self {
        Self {
            key: key.into(),
            value: AttributeValue::Bool(value),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// § 3  Resource + InstrumentationScope
// ────────────────────────────────────────────────────────────────────

/// OTel `Resource` — describes the entity producing telemetry (process, service, container).
/// See <https://opentelemetry.io/docs/specs/otel/resource/semantic_conventions/>.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Attributes describing the resource (e.g. `service.name`, `service.version`).
    pub attributes: Vec<KeyValue>,
}

impl Resource {
    /// Build a `Resource` pre-populated with the OTel service semantic conventions.
    ///
    /// Sets:
    /// * `service.name` = `name`
    /// * `service.version` = `version`
    /// * `telemetry.sdk.name` = `"wicked_estate"`
    /// * `telemetry.sdk.language` = `"rust"`
    pub fn service(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            attributes: vec![
                KeyValue::str(SERVICE_NAME, name),
                KeyValue::str(SERVICE_VERSION, version),
                KeyValue::str(TELEMETRY_SDK_NAME, "wicked_estate"),
                KeyValue::str(TELEMETRY_SDK_LANGUAGE, "rust"),
            ],
        }
    }

    /// Look up the value of an attribute by key. Returns `None` if absent.
    pub fn get(&self, key: &str) -> Option<&AttributeValue> {
        self.attributes
            .iter()
            .find(|kv| kv.key == key)
            .map(|kv| &kv.value)
    }
}

/// OTel `InstrumentationScope` — identifies the library / module emitting telemetry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentationScope {
    /// Name of the instrumentation library (e.g. `"wicked_estate_retrieve"`).
    pub name: String,
    /// Optional version string.
    pub version: Option<String>,
}

impl InstrumentationScope {
    /// Create a scope with no version.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }

    /// Create a scope with an explicit version.
    pub fn versioned(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: Some(version.into()),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// § 4  Trace IDs / SpanContext  (W3C TraceContext)
// ────────────────────────────────────────────────────────────────────

/// 128-bit W3C trace identifier.
///
/// The W3C `traceparent` header format is:
/// ```text
/// 00-{trace_id_hex_32chars}-{span_id_hex_16chars}-{flags_hex_2chars}
/// ```
/// Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId([u8; 16]);

impl TraceId {
    /// The all-zeros invalid trace id (indicates no active trace).
    pub const INVALID: TraceId = TraceId([0u8; 16]);

    /// Create a `TraceId` from a raw byte array.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Return the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Encode as a lowercase 32-character hex string (W3C `traceparent` field 2).
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Decode from a 32-character lowercase hex string.
    ///
    /// Returns `None` if the string is not exactly 32 hex characters.
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = hex_nibble(chunk[0])?;
            let lo = hex_nibble(chunk[1])?;
            bytes[i] = (hi << 4) | lo;
        }
        Some(Self(bytes))
    }

    /// Returns `true` if this is the all-zeros invalid id.
    pub fn is_valid(&self) -> bool {
        self.0 != [0u8; 16]
    }
}

/// 64-bit W3C span identifier.
///
/// Encoded as a lowercase 16-character hex string in the W3C `traceparent` header (field 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// The all-zeros invalid span id (indicates no active span / no parent).
    pub const INVALID: SpanId = SpanId([0u8; 8]);

    /// Create a `SpanId` from a raw byte array.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    /// Return the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Encode as a lowercase 16-character hex string (W3C `traceparent` field 3).
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Decode from a 16-character lowercase hex string.
    ///
    /// Returns `None` if the string is not exactly 16 hex characters.
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 16 {
            return None;
        }
        let mut bytes = [0u8; 8];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = hex_nibble(chunk[0])?;
            let lo = hex_nibble(chunk[1])?;
            bytes[i] = (hi << 4) | lo;
        }
        Some(Self(bytes))
    }

    /// Returns `true` if this is not the all-zeros invalid id.
    pub fn is_valid(&self) -> bool {
        self.0 != [0u8; 8]
    }
}

/// Decode a single ASCII hex character to its nibble value. Returns `None` on invalid input.
fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// W3C TraceContext span context — the propagation carrier identifying a span in the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanContext {
    /// 128-bit trace identifier shared by all spans in a trace.
    pub trace_id: TraceId,
    /// 64-bit span identifier — unique within the trace.
    pub span_id: SpanId,
    /// W3C trace-flags byte. Bit 0 = sampled.
    pub trace_flags: u8,
    /// `true` if this context was propagated from a remote caller (vs. created locally).
    pub is_remote: bool,
}

impl SpanContext {
    /// The sampled flag bit in `trace_flags`.
    pub const FLAG_SAMPLED: u8 = 0x01;

    /// Returns `true` if the sampled flag is set.
    pub fn is_sampled(&self) -> bool {
        self.trace_flags & Self::FLAG_SAMPLED != 0
    }

    /// Render as a W3C `traceparent` header value:
    /// `00-{trace_id}-{span_id}-{flags}`.
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id.to_hex(),
            self.span_id.to_hex(),
            self.trace_flags
        )
    }
}

// ────────────────────────────────────────────────────────────────────
// § 5  Spans
// ────────────────────────────────────────────────────────────────────

/// OTel span kind — the role of the span in the distributed system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Default — internal operation within an application.
    #[default]
    Internal,
    /// Server-side handler of a synchronous request.
    Server,
    /// Client-side outbound synchronous request.
    Client,
    /// Message producer.
    Producer,
    /// Message consumer.
    Consumer,
}

/// OTel span status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusCode {
    /// Default — status not set.
    #[default]
    Unset,
    /// Operation succeeded.
    Ok,
    /// Operation failed.
    Error,
}

/// OTel span status (code + optional human-readable message).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpanStatus {
    /// Status classification.
    pub code: StatusCode,
    /// Human-readable description of the status (most useful for `Error`).
    pub message: Option<String>,
}

impl SpanStatus {
    /// Construct a status from an error message.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            code: StatusCode::Error,
            message: Some(message.into()),
        }
    }

    /// Construct a successful status.
    pub fn ok() -> Self {
        Self {
            code: StatusCode::Ok,
            message: None,
        }
    }
}

/// OTel span event — a timestamped annotation on a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanEvent {
    /// Event name (e.g. `"exception"`, `"cache.hit"`).
    pub name: String,
    /// Nanoseconds since Unix epoch.
    pub time_unix_nano: u64,
    /// Attributes attached to the event.
    pub attributes: Vec<KeyValue>,
}

/// OTel span link — a causality reference to another span (e.g. across async continuations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanLink {
    /// The span context being referenced.
    pub context: SpanContext,
    /// Attributes attached to the link.
    pub attributes: Vec<KeyValue>,
}

/// OTel `Span` — the complete record of a completed operation.
///
/// Fields follow the OTel proto `Span` message
/// (`opentelemetry_proto::trace::v1::Span`) without the proto dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanData {
    /// The span's own context (trace_id + span_id + flags).
    pub context: SpanContext,
    /// Parent span id, or `None` if this is the root span.
    pub parent_span_id: Option<SpanId>,
    /// Human-readable operation name (e.g. `"wicked_estate_store.query_symbols"`).
    pub name: String,
    /// Span kind (default: `Internal`).
    pub kind: SpanKind,
    /// Span start time as nanoseconds since Unix epoch.
    pub start_time_unix_nano: u64,
    /// Span end time as nanoseconds since Unix epoch.
    pub end_time_unix_nano: u64,
    /// Span-level attributes.
    pub attributes: Vec<KeyValue>,
    /// Time-ordered span events.
    pub events: Vec<SpanEvent>,
    /// Causality links to other spans.
    pub links: Vec<SpanLink>,
    /// Final status of the operation.
    pub status: SpanStatus,
}

// ────────────────────────────────────────────────────────────────────
// § 6  Metrics
// ────────────────────────────────────────────────────────────────────

/// OTel instrument kind — how measurements are aggregated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    /// Monotonically increasing sum (e.g. total bytes indexed).
    Counter,
    /// Non-monotonic sum (e.g. active connections).
    UpDownCounter,
    /// Distribution of values (e.g. query latency in ms).
    Histogram,
    /// Point-in-time measurement (e.g. queue depth).
    Gauge,
    /// Asynchronous monotonic counter.
    ObservableCounter,
    /// Asynchronous non-monotonic counter.
    ObservableUpDownCounter,
    /// Asynchronous gauge.
    ObservableGauge,
}

/// OTel aggregation temporality — how a metric accumulates over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregationTemporality {
    /// Each export covers only the interval since the last export.
    Delta,
    /// Each export covers the entire lifetime of the instrument.
    Cumulative,
}

/// A metric data point value — either signed integer or double-precision float.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "v")]
pub enum MetricValue {
    /// Signed 64-bit integer value.
    I64(i64),
    /// IEEE-754 64-bit float value.
    F64(f64),
}

/// OTel `NumberDataPoint` — a single scalar measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumberDataPoint {
    /// Dimension-slice attributes (e.g. `{"language": "rust"}`).
    pub attributes: Vec<KeyValue>,
    /// Start of the aggregation window (nanoseconds since Unix epoch).
    pub start_time_unix_nano: u64,
    /// End of the aggregation window (nanoseconds since Unix epoch).
    pub time_unix_nano: u64,
    /// The measured value.
    pub value: MetricValue,
}

/// OTel `HistogramDataPoint` — distribution statistics for a single aggregation window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramDataPoint {
    /// Dimension-slice attributes.
    pub attributes: Vec<KeyValue>,
    /// Start of the aggregation window (nanoseconds since Unix epoch).
    pub start_time_unix_nano: u64,
    /// End of the aggregation window (nanoseconds since Unix epoch).
    pub time_unix_nano: u64,
    /// Total number of measurements in the window.
    pub count: u64,
    /// Sum of all measurements (useful for computing means).
    pub sum: f64,
    /// Count per bucket. Length = `explicit_bounds.len() + 1`.
    pub bucket_counts: Vec<u64>,
    /// Upper-inclusive bucket boundaries (the last bucket is `(last_bound, +∞)`).
    pub explicit_bounds: Vec<f64>,
}

/// OTel `MetricData` — the aggregated payload of a metric instrument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MetricData {
    /// Scalar sum (Counter or UpDownCounter).
    Sum {
        /// Individual data points.
        data_points: Vec<NumberDataPoint>,
        /// How points accumulate over time.
        temporality: AggregationTemporality,
        /// `true` for Counter, `false` for UpDownCounter.
        is_monotonic: bool,
    },
    /// Point-in-time scalar measurement.
    Gauge {
        /// Individual data points.
        data_points: Vec<NumberDataPoint>,
    },
    /// Distribution of measurements.
    Histogram {
        /// Histogram data points.
        data_points: Vec<HistogramDataPoint>,
        /// How points accumulate over time.
        temporality: AggregationTemporality,
    },
}

/// OTel `Metric` — one instrument's complete export record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// Instrument name (e.g. `"ci.query.duration_ms"`).
    pub name: String,
    /// Human-readable description of what this metric measures.
    pub description: String,
    /// UCUM unit string (e.g. `"ms"`, `"By"`, `"1"`).
    pub unit: String,
    /// The aggregated data payload.
    pub data: MetricData,
}

// ────────────────────────────────────────────────────────────────────
// § 7  Logs
// ────────────────────────────────────────────────────────────────────

/// OTel severity number — simplified from the full 1–24 scale.
///
/// OTel mapping (abbreviated):
/// | Variant | OTel range | Meaning          |
/// |---------|-----------|------------------|
/// | Trace   | 1–4       | Fine-grained debug |
/// | Debug   | 5–8       | Developer debug  |
/// | Info    | 9–12      | Informational    |
/// | Warn    | 13–16     | Warning          |
/// | Error   | 17–20     | Error            |
/// | Fatal   | 21–24     | Fatal            |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SeverityNumber {
    /// OTel severity 1–4.
    Trace,
    /// OTel severity 5–8.
    Debug,
    /// OTel severity 9–12.
    #[default]
    Info,
    /// OTel severity 13–16.
    Warn,
    /// OTel severity 17–20.
    Error,
    /// OTel severity 21–24.
    Fatal,
}

/// OTel `LogRecord` — a single structured log entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// Time the event occurred (nanoseconds since Unix epoch).
    pub time_unix_nano: u64,
    /// Time the log was observed by the collector (nanoseconds since Unix epoch).
    pub observed_time_unix_nano: u64,
    /// Numeric severity level.
    pub severity_number: SeverityNumber,
    /// Short severity text (e.g. `"INFO"`, `"ERROR"`).
    pub severity_text: String,
    /// Log body — the human-readable or structured message.
    pub body: AttributeValue,
    /// Additional attributes attached to the log record.
    pub attributes: Vec<KeyValue>,
    /// Trace id, if the log was emitted within a trace.
    pub trace_id: Option<TraceId>,
    /// Span id, if the log was emitted within a span.
    pub span_id: Option<SpanId>,
}

// ────────────────────────────────────────────────────────────────────
// § 8  Exporter configuration
// ────────────────────────────────────────────────────────────────────

/// OTLP transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OTLP over gRPC (default port 4317).
    OtlpGrpc,
    /// OTLP over HTTP/1.1 with Protobuf encoding (default port 4318).
    OtlpHttpProtobuf,
    /// OTLP over HTTP/1.1 with JSON encoding (default port 4318).
    OtlpHttpJson,
}

/// Content-encoding compression for OTLP requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    /// No compression.
    #[default]
    None,
    /// gzip compression (`Content-Encoding: gzip`).
    Gzip,
}

/// OTel sampling strategy.
///
/// Matches the OTel SDK's built-in samplers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Sampler {
    /// Always sample (100% of traces exported).
    #[default]
    AlwaysOn,
    /// Never sample (0% — useful for disabling tracing with near-zero overhead).
    AlwaysOff,
    /// Sample the given fraction of traces (0.0 = never, 1.0 = always).
    TraceIdRatio(f64),
    /// Defer to the parent span's sampling decision; use the inner sampler for root spans.
    ParentBased(Box<Sampler>),
}

/// Configuration for a [`TelemetrySink`] backed by an OTLP exporter.
///
/// # Secret-handling
/// [`ExporterConfig::headers`] carries vendor authentication tokens at **runtime** —
/// for example `("x-honeycomb-team", "<key>")`, `("dd-api-key", "<key>")`,
/// `("api-key", "<key>")` for New Relic, etc. These tokens are **never written to the
/// graph store** — consistent with ADR-004's observe-only, no-secret-storage rule.
/// Supply them from environment variables or a secrets manager at startup; never hard-code
/// them or serialize them alongside persistent data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExporterConfig {
    /// OTLP transport protocol.
    pub protocol: Protocol,
    /// Collector endpoint URL (e.g. `"http://localhost:4317"` for local OTel Collector,
    /// `"https://api.honeycomb.io"` for Honeycomb Cloud).
    pub endpoint: String,
    /// HTTP / gRPC metadata headers — carries vendor auth tokens at runtime (see above).
    /// Never persisted.
    pub headers: Vec<(String, String)>,
    /// Export request timeout in milliseconds (default: 10 000 ms).
    pub timeout_ms: u64,
    /// Payload compression.
    pub compression: Compression,
    /// Resource describing the exporting service.
    pub resource: Resource,
    /// Sampling strategy for traces.
    pub sampler: Sampler,
}

impl ExporterConfig {
    /// Minimal constructor for OTLP-over-HTTP (JSON) — the easiest transport to bring up
    /// against a local OpenTelemetry Collector or a vendor HTTP ingestion endpoint.
    pub fn otlp_http(endpoint: impl Into<String>) -> Self {
        Self {
            protocol: Protocol::OtlpHttpJson,
            endpoint: endpoint.into(),
            headers: Vec::new(),
            timeout_ms: 10_000,
            compression: Compression::None,
            resource: Resource::service("wicked_estate", env!("CARGO_PKG_VERSION")),
            sampler: Sampler::AlwaysOn,
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// § 9  Export result types
// ────────────────────────────────────────────────────────────────────

/// Error from a [`TelemetrySink`] export call.
///
/// OTel exporters distinguish **transient** failures (retry is safe) from **permanent** ones
/// (retry is futile — bad config, auth failure, schema mismatch). Callers MUST NOT retry
/// [`ExportError::Permanent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum ExportError {
    /// Transient failure — safe to retry with backoff (network blip, back-pressure).
    #[error("transient export error: {0}")]
    Transient(String),
    /// Permanent failure — do not retry (bad config, auth failure, unrecognized protocol).
    #[error("permanent export error: {0}")]
    Permanent(String),
}

/// Return type of every [`TelemetrySink`] method.
pub type ExportResult = Result<(), ExportError>;

// ────────────────────────────────────────────────────────────────────
// § 10  The adapter trait — TelemetrySink
// ────────────────────────────────────────────────────────────────────

/// The observability export seam.
///
/// A backend (OTLP → Datadog / Honeycomb / Grafana Cloud / New Relic / AWS X-Ray / Azure Monitor /
/// GCP Cloud Trace) implements this trait; the engine hands it batched, OTel-shaped telemetry.
///
/// The interface mirrors the OTel SDK exporter SPI — `SpanExporter` / `MetricExporter` /
/// `LogExporter` — consolidated into a single trait so an impl can share connection state. Methods
/// receive a **batch** of pre-encoded OTel records; the impl is responsible for serialisation,
/// compression, retry, and transport (OTLP gRPC or HTTP).
///
/// # DESIGNED, NOT BUILT — see ADR-006
///
/// The default is [`NoopSink`] (observability OFF; zero overhead; local-first). Adding a vendor
/// backend = one `TelemetrySink` impl + one match arm in [`open_telemetry_sink`]; zero caller
/// changes (same pattern as `wicked_estate_store::open_store` — ADR-003).
///
/// # Threading
/// Implementations MUST be `Send + Sync` so the sink can be shared across MCP handler threads.
/// Methods take `&self` (shared ref) — the impl owns its internal connection pool / lock.
pub trait TelemetrySink: Send + Sync {
    /// Export a batch of completed spans.
    ///
    /// `resource` and `scope` apply uniformly to all spans in the batch.
    fn export_spans(
        &self,
        resource: &Resource,
        scope: &InstrumentationScope,
        spans: &[SpanData],
    ) -> ExportResult;

    /// Export a batch of metric instruments.
    ///
    /// `resource` and `scope` apply uniformly to all metrics in the batch.
    fn export_metrics(
        &self,
        resource: &Resource,
        scope: &InstrumentationScope,
        metrics: &[Metric],
    ) -> ExportResult;

    /// Export a batch of log records.
    ///
    /// `resource` and `scope` apply uniformly to all records in the batch.
    fn export_logs(
        &self,
        resource: &Resource,
        scope: &InstrumentationScope,
        logs: &[LogRecord],
    ) -> ExportResult;

    /// Flush any pending buffered data to the backend.
    ///
    /// Called before graceful shutdown; implementations SHOULD block until in-flight exports
    /// complete or `timeout_ms` elapses. [`NoopSink`] returns `Ok(())` immediately.
    fn force_flush(&self) -> ExportResult;

    /// Shut down the exporter — release connections, stop background threads.
    ///
    /// After `shutdown`, further export calls SHOULD return `Ok(())` silently (no panic).
    /// [`NoopSink`] is a no-op.
    fn shutdown(&self) -> ExportResult;
}

// ────────────────────────────────────────────────────────────────────
// § 11  NoopSink — the zero-overhead default
// ────────────────────────────────────────────────────────────────────

/// Zero-overhead observability sink — every method is a no-op.
///
/// This is the default returned by [`open_telemetry_sink(None)`][open_telemetry_sink] and keeps
/// the engine local-first: observability is OFF unless the operator explicitly provides an
/// [`ExporterConfig`] and wires the matching OTLP exporter crate.
#[derive(Debug, Clone, Default)]
pub struct NoopSink;

impl TelemetrySink for NoopSink {
    fn export_spans(
        &self,
        _resource: &Resource,
        _scope: &InstrumentationScope,
        _spans: &[SpanData],
    ) -> ExportResult {
        Ok(())
    }

    fn export_metrics(
        &self,
        _resource: &Resource,
        _scope: &InstrumentationScope,
        _metrics: &[Metric],
    ) -> ExportResult {
        Ok(())
    }

    fn export_logs(
        &self,
        _resource: &Resource,
        _scope: &InstrumentationScope,
        _logs: &[LogRecord],
    ) -> ExportResult {
        Ok(())
    }

    fn force_flush(&self) -> ExportResult {
        Ok(())
    }

    fn shutdown(&self) -> ExportResult {
        Ok(())
    }
}

// ────────────────────────────────────────────────────────────────────
// § 12  Factory — open_telemetry_sink
// ────────────────────────────────────────────────────────────────────

/// Open a [`TelemetrySink`] from an optional [`ExporterConfig`].
///
/// | `config` | Returns |
/// |----------|---------|
/// | `None`   | `Box<dyn TelemetrySink>` backed by [`NoopSink`] — zero overhead, observability OFF. (**Built**) |
/// | `Some(cfg)` with any [`Protocol`] | `Err(ExportError::Permanent("… designed but not built — see ADR-006"))`. (**Designed**) |
///
/// # Vendor extensibility
/// Adding Datadog / Honeycomb / Grafana Cloud support = one `TelemetrySink` impl (delegating
/// to `opentelemetry-otlp` when the exporter crate is added) + one match arm here on the
/// `cfg.protocol` (or a vendor-specific discriminator). **Zero caller changes** — identical to
/// the `wicked_estate_store::open_store` pattern from ADR-003.
pub fn open_telemetry_sink(
    config: Option<&ExporterConfig>,
) -> Result<Box<dyn TelemetrySink>, ExportError> {
    match config {
        None => Ok(Box::new(NoopSink)),
        Some(cfg) => Err(ExportError::Permanent(format!(
            "OTLP exporter for {:?} is designed but not built — see ADR-006. \
             To enable: add an `opentelemetry-otlp` impl for protocol {:?} \
             and one match arm in `open_telemetry_sink`; zero caller changes required.",
            cfg.protocol, cfg.protocol,
        ))),
    }
}

// ────────────────────────────────────────────────────────────────────
// § 13  Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── factory ──────────────────────────────────────────────────────

    #[test]
    fn noop_sink_factory_returns_ok() {
        let sink = open_telemetry_sink(None).expect("NoopSink must not fail");
        let resource = Resource::service("test", "0.0.0");
        let scope = InstrumentationScope::new("test");
        assert_eq!(sink.export_spans(&resource, &scope, &[]), Ok(()));
        assert_eq!(sink.export_metrics(&resource, &scope, &[]), Ok(()));
        assert_eq!(sink.export_logs(&resource, &scope, &[]), Ok(()));
        assert_eq!(sink.force_flush(), Ok(()));
        assert_eq!(sink.shutdown(), Ok(()));
    }

    #[test]
    fn configured_sink_returns_designed_not_built_error() {
        let cfg = ExporterConfig::otlp_http("http://localhost:4318");
        // Match directly rather than `unwrap_err()` — `Box<dyn TelemetrySink>` (the Ok type) is
        // not `Debug`, which `unwrap_err` would require.
        let Err(ExportError::Permanent(msg)) = open_telemetry_sink(Some(&cfg)) else {
            panic!("expected a Permanent 'designed but not built' error for a configured sink");
        };
        assert!(
            msg.contains("ADR-006"),
            "error message must cite ADR-006; got: {msg}"
        );
        assert!(
            msg.contains("designed but not built"),
            "error message must say 'designed but not built'; got: {msg}"
        );
    }

    // ── TraceId / SpanId hex round-trip ──────────────────────────────

    #[test]
    fn trace_id_hex_round_trip() {
        let bytes = [
            0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e,
            0x47, 0x36,
        ];
        let id = TraceId::from_bytes(bytes);
        let hex = id.to_hex();
        assert_eq!(hex.len(), 32);
        let round = TraceId::from_hex(&hex).expect("valid hex must round-trip");
        assert_eq!(id, round);
    }

    #[test]
    fn span_id_hex_round_trip() {
        let bytes = [0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7];
        let id = SpanId::from_bytes(bytes);
        let hex = id.to_hex();
        assert_eq!(hex.len(), 16);
        let round = SpanId::from_hex(&hex).expect("valid hex must round-trip");
        assert_eq!(id, round);
    }

    #[test]
    fn trace_id_invalid_hex_returns_none() {
        assert!(TraceId::from_hex("not-hex!!!").is_none());
        assert!(TraceId::from_hex("deadbeef").is_none()); // too short
    }

    #[test]
    fn span_id_invalid_hex_returns_none() {
        assert!(SpanId::from_hex("xyz").is_none());
        assert!(SpanId::from_hex("deadbeef").is_none()); // too short
    }

    #[test]
    fn trace_id_invalid_zero() {
        assert!(!TraceId::INVALID.is_valid());
    }

    #[test]
    fn span_id_invalid_zero() {
        assert!(!SpanId::INVALID.is_valid());
    }

    // ── traceparent header ────────────────────────────────────────────

    #[test]
    fn traceparent_format() {
        let ctx = SpanContext {
            trace_id: TraceId::from_bytes([
                0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e,
                0x47, 0x36,
            ]),
            span_id: SpanId::from_bytes([0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7]),
            trace_flags: SpanContext::FLAG_SAMPLED,
            is_remote: false,
        };
        let tp = ctx.to_traceparent();
        // format: 00-{32}-{16}-{02}
        assert!(tp.starts_with("00-"), "must start with '00-'; got {tp}");
        let parts: Vec<&str> = tp.split('-').collect();
        assert_eq!(
            parts.len(),
            4,
            "traceparent must have 4 dash-separated parts; got {tp}"
        );
        assert_eq!(parts[1].len(), 32);
        assert_eq!(parts[2].len(), 16);
        assert_eq!(parts[3], "01");
    }

    // ── Resource::service ────────────────────────────────────────────

    #[test]
    fn resource_service_contains_service_name() {
        let r = Resource::service("wicked_estate", "0.1");
        let val = r.get(SERVICE_NAME).expect("service.name must be present");
        assert_eq!(val, &AttributeValue::Str("wicked_estate".into()));
    }

    #[test]
    fn resource_service_contains_service_version() {
        let r = Resource::service("wicked_estate", "0.1");
        let val = r
            .get(SERVICE_VERSION)
            .expect("service.version must be present");
        assert_eq!(val, &AttributeValue::Str("0.1".into()));
    }

    #[test]
    fn resource_service_contains_sdk_language() {
        let r = Resource::service("wicked_estate", "0.1");
        let val = r
            .get(TELEMETRY_SDK_LANGUAGE)
            .expect("telemetry.sdk.language must be present");
        assert_eq!(val, &AttributeValue::Str("rust".into()));
    }

    // ── SpanData serde round-trip ─────────────────────────────────────

    #[test]
    fn span_data_serde_round_trip() {
        let span = SpanData {
            context: SpanContext {
                trace_id: TraceId::from_bytes([1u8; 16]),
                span_id: SpanId::from_bytes([2u8; 8]),
                trace_flags: SpanContext::FLAG_SAMPLED,
                is_remote: false,
            },
            parent_span_id: Some(SpanId::from_bytes([3u8; 8])),
            name: "wicked_estate_store.query_symbols".into(),
            kind: SpanKind::Internal,
            start_time_unix_nano: 1_000_000_000,
            end_time_unix_nano: 1_005_000_000,
            attributes: vec![KeyValue::str(CODE_FUNCTION, "query_symbols")],
            events: vec![SpanEvent {
                name: "cache.hit".into(),
                time_unix_nano: 1_001_000_000,
                attributes: vec![],
            }],
            links: vec![],
            status: SpanStatus::ok(),
        };
        let json = serde_json::to_string(&span).expect("SpanData must serialize");
        let back: SpanData = serde_json::from_str(&json).expect("SpanData must deserialize");
        assert_eq!(span, back);
    }

    // ── Metric serde round-trip ───────────────────────────────────────

    #[test]
    fn metric_sum_serde_round_trip() {
        let metric = Metric {
            name: "ci.files.indexed".into(),
            description: "Total source files indexed.".into(),
            unit: "1".into(),
            data: MetricData::Sum {
                data_points: vec![NumberDataPoint {
                    attributes: vec![KeyValue::str("language", "rust")],
                    start_time_unix_nano: 0,
                    time_unix_nano: 1_000_000_000,
                    value: MetricValue::I64(42),
                }],
                temporality: AggregationTemporality::Cumulative,
                is_monotonic: true,
            },
        };
        let json = serde_json::to_string(&metric).expect("Metric must serialize");
        let back: Metric = serde_json::from_str(&json).expect("Metric must deserialize");
        assert_eq!(metric, back);
    }

    #[test]
    fn metric_histogram_serde_round_trip() {
        let metric = Metric {
            name: "ci.query.duration_ms".into(),
            description: "Query latency in milliseconds.".into(),
            unit: "ms".into(),
            data: MetricData::Histogram {
                data_points: vec![HistogramDataPoint {
                    attributes: vec![],
                    start_time_unix_nano: 0,
                    time_unix_nano: 1_000_000_000,
                    count: 10,
                    sum: 55.0,
                    bucket_counts: vec![3, 4, 2, 1],
                    explicit_bounds: vec![1.0, 5.0, 20.0],
                }],
                temporality: AggregationTemporality::Delta,
            },
        };
        let json = serde_json::to_string(&metric).expect("Metric must serialize");
        let back: Metric = serde_json::from_str(&json).expect("Metric must deserialize");
        assert_eq!(metric, back);
    }

    // ── LogRecord serde round-trip ────────────────────────────────────

    #[test]
    fn log_record_serde_round_trip() {
        let log = LogRecord {
            time_unix_nano: 1_000_000_000,
            observed_time_unix_nano: 1_000_000_100,
            severity_number: SeverityNumber::Info,
            severity_text: "INFO".into(),
            body: AttributeValue::Str("index complete: 42 files".into()),
            attributes: vec![KeyValue::int("ci.file.count", 42)],
            trace_id: Some(TraceId::from_bytes([0xab; 16])),
            span_id: Some(SpanId::from_bytes([0xcd; 8])),
        };
        let json = serde_json::to_string(&log).expect("LogRecord must serialize");
        let back: LogRecord = serde_json::from_str(&json).expect("LogRecord must deserialize");
        assert_eq!(log, back);
    }

    #[test]
    fn log_record_without_trace_context_serde_round_trip() {
        let log = LogRecord {
            time_unix_nano: 500,
            observed_time_unix_nano: 501,
            severity_number: SeverityNumber::Warn,
            severity_text: "WARN".into(),
            body: AttributeValue::Str("cache miss".into()),
            attributes: vec![],
            trace_id: None,
            span_id: None,
        };
        let json = serde_json::to_string(&log).expect("LogRecord must serialize");
        let back: LogRecord = serde_json::from_str(&json).expect("LogRecord must deserialize");
        assert_eq!(log, back);
    }
}
