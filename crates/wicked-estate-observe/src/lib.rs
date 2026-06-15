//! OTLP HTTP/JSON exporter for wicked-estate.
//!
//! Sends spans, metrics, and logs to an OTLP HTTP collector endpoint
//! using the JSON encoding format over blocking HTTP.

use std::sync::Arc;
use std::time::Duration;

use wicked_estate_core::observability::{
    AttributeValue, ExportError, ExportResult, ExporterConfig, InstrumentationScope, KeyValue,
    LogRecord, Metric, NoopSink, Protocol, Resource, SpanData, TelemetrySink,
};

/// An OTLP HTTP/JSON sink that posts telemetry to a collector endpoint.
pub struct OtlpSink {
    client: reqwest::blocking::Client,
    endpoint: String,
    headers: Vec<(String, String)>,
}

impl std::fmt::Debug for OtlpSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtlpSink")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl OtlpSink {
    /// Construct a new `OtlpSink`. Does **not** open a network connection.
    pub fn new(config: &ExporterConfig) -> Result<Self, ExportError> {
        let timeout = Duration::from_millis(config.timeout_ms);
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ExportError::Permanent(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            client,
            endpoint: config.endpoint.trim_end_matches('/').to_owned(),
            headers: config.headers.clone(),
        })
    }

    fn post(&self, path: &str, body: serde_json::Value) -> ExportResult {
        let url = format!("{}{}", self.endpoint, path);
        let mut req = self.client.post(&url);
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req
            .json(&body)
            .send()
            .map_err(|e| ExportError::Transient(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ExportError::Transient(format!(
                "HTTP {}",
                resp.status().as_u16()
            )))
        }
    }
}

impl TelemetrySink for OtlpSink {
    fn export_spans(
        &self,
        resource: &Resource,
        scope: &InstrumentationScope,
        spans: &[SpanData],
    ) -> ExportResult {
        if spans.is_empty() {
            return Ok(());
        }
        let body = serde_json::json!({
            "resourceSpans": [{
                "resource": { "attributes": resource.attributes.iter().map(kv_to_json).collect::<Vec<_>>() },
                "scopeSpans": [{
                    "scope": { "name": scope.name, "version": scope.version },
                    "spans": spans.iter().map(span_to_json).collect::<Vec<_>>()
                }]
            }]
        });
        self.post("/v1/traces", body)
    }

    fn export_metrics(
        &self,
        resource: &Resource,
        scope: &InstrumentationScope,
        metrics: &[Metric],
    ) -> ExportResult {
        if metrics.is_empty() {
            return Ok(());
        }
        let body = serde_json::json!({
            "resourceMetrics": [{
                "resource": { "attributes": resource.attributes.iter().map(kv_to_json).collect::<Vec<_>>() },
                "scopeMetrics": [{
                    "scope": { "name": scope.name, "version": scope.version },
                    "metrics": metrics.iter().map(metric_to_json).collect::<Vec<_>>()
                }]
            }]
        });
        self.post("/v1/metrics", body)
    }

    fn export_logs(
        &self,
        resource: &Resource,
        scope: &InstrumentationScope,
        logs: &[LogRecord],
    ) -> ExportResult {
        if logs.is_empty() {
            return Ok(());
        }
        let body = serde_json::json!({
            "resourceLogs": [{
                "resource": { "attributes": resource.attributes.iter().map(kv_to_json).collect::<Vec<_>>() },
                "scopeLogs": [{
                    "scope": { "name": scope.name, "version": scope.version },
                    "logRecords": logs.iter().map(log_to_json).collect::<Vec<_>>()
                }]
            }]
        });
        self.post("/v1/logs", body)
    }

    fn force_flush(&self) -> ExportResult {
        Ok(())
    }

    fn shutdown(&self) -> ExportResult {
        Ok(())
    }
}

fn span_kind_as_i32(kind: &wicked_estate_core::observability::SpanKind) -> i32 {
    match kind {
        wicked_estate_core::observability::SpanKind::Internal => 1,
        wicked_estate_core::observability::SpanKind::Server => 2,
        wicked_estate_core::observability::SpanKind::Client => 3,
        wicked_estate_core::observability::SpanKind::Producer => 4,
        wicked_estate_core::observability::SpanKind::Consumer => 5,
    }
}

fn span_to_json(span: &SpanData) -> serde_json::Value {
    serde_json::json!({
        "traceId": span.context.trace_id.to_hex(),
        "spanId": span.context.span_id.to_hex(),
        "name": span.name,
        "kind": span_kind_as_i32(&span.kind),
        "startTimeUnixNano": span.start_time_unix_nano.to_string(),
        "endTimeUnixNano": span.end_time_unix_nano.to_string(),
        "attributes": span.attributes.iter().map(kv_to_json).collect::<Vec<_>>(),
    })
}

fn metric_to_json(m: &Metric) -> serde_json::Value {
    serde_json::json!({
        "name": m.name,
        "description": m.description,
        "unit": m.unit,
    })
}

fn log_to_json(log: &LogRecord) -> serde_json::Value {
    serde_json::json!({
        "timeUnixNano": log.time_unix_nano.to_string(),
        "body": attribute_value_to_json(&log.body),
    })
}

fn attribute_value_to_json(v: &AttributeValue) -> serde_json::Value {
    match v {
        AttributeValue::Str(s) => serde_json::json!({"stringValue": s}),
        AttributeValue::Bool(b) => serde_json::json!({"boolValue": b}),
        AttributeValue::Int(i) => serde_json::json!({"intValue": i.to_string()}),
        AttributeValue::Double(d) => serde_json::json!({"doubleValue": d}),
        AttributeValue::StrArray(arr) => serde_json::json!({"arrayValue": arr}),
        AttributeValue::IntArray(arr) => serde_json::json!({"arrayValue": arr}),
    }
}

fn kv_to_json(kv: &KeyValue) -> serde_json::Value {
    serde_json::json!({
        "key": kv.key,
        "value": attribute_value_to_json(&kv.value),
    })
}

/// Build an `OtlpSink` wrapped in `Arc<dyn TelemetrySink>`.
pub fn open_otlp_sink(config: &ExporterConfig) -> Result<Arc<dyn TelemetrySink>, ExportError> {
    Ok(Arc::new(OtlpSink::new(config)?))
}

/// Build a sink from environment variables.
///
/// - `WICKED_OTEL_ENDPOINT` — if set, uses `OtlpSink`; otherwise returns `NoopSink`.
/// - `WICKED_OTEL_HEADERS` — comma-separated `key=value` pairs.
pub fn init_sink_from_env() -> Arc<dyn TelemetrySink> {
    let endpoint = match std::env::var("WICKED_OTEL_ENDPOINT") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            return wicked_estate_core::open_telemetry_sink(None)
                .unwrap_or_else(|_| Arc::new(NoopSink));
        }
    };

    let headers: Vec<(String, String)> = std::env::var("WICKED_OTEL_HEADERS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?.trim().to_owned();
            let v = parts.next()?.trim().to_owned();
            if k.is_empty() { None } else { Some((k, v)) }
        })
        .collect();

    let config = ExporterConfig {
        endpoint,
        headers,
        protocol: Protocol::OtlpHttpJson,
        timeout_ms: 5_000,
        compression: wicked_estate_core::observability::Compression::None,
        resource: Resource::default(),
        sampler: wicked_estate_core::observability::Sampler::AlwaysOn,
    };

    open_otlp_sink(&config).unwrap_or_else(|e| {
        eprintln!("wicked-estate: telemetry init failed ({e}); using noop sink");
        Arc::new(NoopSink)
    })
}
