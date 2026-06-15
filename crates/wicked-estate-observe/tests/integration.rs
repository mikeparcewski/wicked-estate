//! Integration tests for `wicked-estate-observe`.
//!
//! These tests verify the in-process behaviour of the observe crate without
//! requiring a live OTLP collector — they exercise the `InMemorySink` from
//! `wicked-estate-core` and the env-driven factory.

use std::sync::Arc;
use wicked_estate_core::TelemetrySink;
use wicked_estate_core::observability::{
    AggregationTemporality, AttributeValue, InMemorySink, InstrumentationScope, KeyValue,
    LogRecord, Metric, MetricData, MetricValue, NumberDataPoint, Resource, SeverityNumber,
    SpanContext, SpanData, SpanId, SpanKind, SpanStatus, TraceId,
};

fn make_resource() -> Resource {
    Resource::service("test-service", "0.0.1")
}

fn make_scope() -> InstrumentationScope {
    InstrumentationScope::new("wicked_estate_observe_test")
}

fn make_span(name: &str) -> SpanData {
    SpanData {
        context: SpanContext {
            trace_id: TraceId::from_bytes([1u8; 16]),
            span_id: SpanId::from_bytes([2u8; 8]),
            trace_flags: SpanContext::FLAG_SAMPLED,
            is_remote: false,
        },
        parent_span_id: None,
        name: name.to_string(),
        kind: SpanKind::Internal,
        start_time_unix_nano: 1_000_000_000,
        end_time_unix_nano: 2_000_000_000,
        attributes: vec![KeyValue::str("test.key", "test.value")],
        events: vec![],
        links: vec![],
        status: SpanStatus::ok(),
    }
}

/// InMemorySink captures spans that are exported to it.
#[test]
fn in_memory_sink_captures_spans() {
    let sink = Arc::new(InMemorySink::default());
    let resource = make_resource();
    let scope = make_scope();
    let span = make_span("test-operation");

    sink.export_spans(&resource, &scope, std::slice::from_ref(&span))
        .expect("export_spans must succeed");

    let captured = sink.captured_spans();
    assert_eq!(captured.len(), 1, "one span must be captured");
    assert_eq!(captured[0].name, "test-operation");
}

/// InMemorySink captures metrics exported to it.
#[test]
fn in_memory_sink_captures_metrics() {
    let sink = Arc::new(InMemorySink::default());
    let resource = make_resource();
    let scope = make_scope();
    let metric = Metric {
        name: "test.counter".to_string(),
        description: "A test counter.".to_string(),
        unit: "1".to_string(),
        data: MetricData::Sum {
            data_points: vec![NumberDataPoint {
                attributes: vec![],
                start_time_unix_nano: 0,
                time_unix_nano: 1_000_000_000,
                value: MetricValue::I64(42),
            }],
            temporality: AggregationTemporality::Cumulative,
            is_monotonic: true,
        },
    };

    sink.export_metrics(&resource, &scope, &[metric])
        .expect("export_metrics must succeed");

    let captured = sink.captured_metrics();
    assert_eq!(captured.len(), 1, "one metric must be captured");
    assert_eq!(captured[0].name, "test.counter");
    assert_eq!(captured[0].unit, "1");
}

/// InMemorySink captures log records exported to it.
#[test]
fn in_memory_sink_captures_logs() {
    let sink = Arc::new(InMemorySink::default());
    let resource = make_resource();
    let scope = make_scope();
    let log = LogRecord {
        time_unix_nano: 1_000_000_000,
        observed_time_unix_nano: 1_000_000_100,
        severity_number: SeverityNumber::Info,
        severity_text: "INFO".to_string(),
        body: AttributeValue::Str("test log message".to_string()),
        attributes: vec![],
        trace_id: None,
        span_id: None,
    };

    sink.export_logs(&resource, &scope, &[log])
        .expect("export_logs must succeed");

    let captured = sink.captured_logs();
    assert_eq!(captured.len(), 1, "one log must be captured");
    assert_eq!(
        captured[0].body,
        AttributeValue::Str("test log message".to_string())
    );
}
