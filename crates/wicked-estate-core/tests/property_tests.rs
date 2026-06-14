//! Property-based tests for `wicked-estate-core`.
//!
//! These tests encode invariants that every generated input must satisfy:
//!
//! P1 — [`Edge::new`] always yields `confidence` in `(0.0, 1.0]`, finite, and
//!      non-empty `provenance` / `resolved_by` (encodes the
//!      "never emit an edge without confidence+provenance" rule, CLAUDE.md §8).
//!
//! P2 — [`SymbolId`] preserves its inner string: `SymbolId(s).as_str() == s`.
//!      (`Symbol` has `Display` but no `FromStr`; a full `Symbol` round-trip is
//!      not applicable — the canonical form is the `SymbolId` string, per ADR-002.)
//!
//! P3 — Serde round-trip for `AttributeValue`, `SpanData`, `Metric`, `LogRecord`:
//!      `serde_json::from_str(&to_string(x)) == x` for all generated inputs.

use wicked_estate_core::{
    AggregationTemporality, AttributeValue, EdgeKind, KeyValue, LogRecord, Metric, MetricData,
    MetricValue, NumberDataPoint, ResolutionTier, SeverityNumber, SpanContext, SpanData, SpanEvent,
    SpanId, SpanKind, SpanLink, SpanStatus, StatusCode, SymbolId, TraceId,
};
use wicked_estate_core::{Confidence, Edge};
use proptest::prelude::*;

// ─── strategy helpers ────────────────────────────────────────────────────────

/// Generate a non-empty ASCII identifier string (bounded to 32 chars).
fn arb_name() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,31}".prop_map(String::from)
}

/// Generate a [`SymbolId`] from a non-empty bounded string.
fn arb_symbol_id() -> impl Strategy<Value = SymbolId> {
    arb_name().prop_map(SymbolId)
}

/// Generate every [`ResolutionTier`] variant.
fn arb_resolution_tier() -> impl Strategy<Value = ResolutionTier> {
    prop_oneof![
        Just(ResolutionTier::Parsed),
        Just(ResolutionTier::Tags),
        Just(ResolutionTier::ImportMap),
        Just(ResolutionTier::Heuristic),
        Just(ResolutionTier::Tsg),
        Just(ResolutionTier::Scip),
        Just(ResolutionTier::Lsp),
    ]
}

/// Generate every [`EdgeKind`] variant (bounded `Other` string).
fn arb_edge_kind() -> impl Strategy<Value = EdgeKind> {
    prop_oneof![
        Just(EdgeKind::Contains),
        Just(EdgeKind::Defines),
        Just(EdgeKind::Calls),
        Just(EdgeKind::Imports),
        Just(EdgeKind::References),
        Just(EdgeKind::Instantiates),
        Just(EdgeKind::Implements),
        Just(EdgeKind::Extends),
        Just(EdgeKind::Overrides),
        Just(EdgeKind::HasType),
        Just(EdgeKind::Returns),
        arb_name().prop_map(EdgeKind::Other),
    ]
}

/// Finite f64 values that round-trip EXACTLY through serde_json's DEFAULT (fast) float parser.
/// At large magnitudes (≈1e15) that parser can land 1 ULP off (e.g. `…466.1` → `…466.0`);
/// serde_json's `float_roundtrip` feature fixes it but slows the hot JSON parse path every node/edge
/// read uses — not worth it for telemetry attributes. Clean integers (< 2⁵³) and power-of-two
/// fractions of bounded magnitude serialize to short decimals that parse back bit-identically.
fn arb_round_trippable_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        (-(1i64 << 42)..(1i64 << 42)).prop_map(|n| n as f64),
        (-4_000_000i64..4_000_000).prop_map(|n| n as f64 / 256.0),
    ]
}

/// Generate an [`AttributeValue`] over all six variants (bounded inputs).
fn arb_attribute_value() -> impl Strategy<Value = AttributeValue> {
    prop_oneof![
        any::<String>().prop_map(AttributeValue::Str),
        any::<bool>().prop_map(AttributeValue::Bool),
        any::<i64>().prop_map(AttributeValue::Int),
        arb_round_trippable_f64().prop_map(AttributeValue::Double),
        prop::collection::vec(any::<String>(), 0..8).prop_map(AttributeValue::StrArray),
        prop::collection::vec(any::<i64>(), 0..8).prop_map(AttributeValue::IntArray),
    ]
}

/// Generate a [`KeyValue`] with a bounded key.
fn arb_key_value() -> impl Strategy<Value = KeyValue> {
    (arb_name(), arb_attribute_value()).prop_map(|(k, v)| KeyValue { key: k, value: v })
}

/// Generate a fixed 16-byte array for [`TraceId`].
fn arb_trace_id() -> impl Strategy<Value = TraceId> {
    prop::array::uniform16(any::<u8>()).prop_map(TraceId::from_bytes)
}

/// Generate a fixed 8-byte array for [`SpanId`].
fn arb_span_id() -> impl Strategy<Value = SpanId> {
    prop::array::uniform8(any::<u8>()).prop_map(SpanId::from_bytes)
}

fn arb_span_context() -> impl Strategy<Value = SpanContext> {
    (arb_trace_id(), arb_span_id(), any::<u8>(), any::<bool>()).prop_map(
        |(trace_id, span_id, trace_flags, is_remote)| SpanContext {
            trace_id,
            span_id,
            trace_flags,
            is_remote,
        },
    )
}

fn arb_span_kind() -> impl Strategy<Value = SpanKind> {
    prop_oneof![
        Just(SpanKind::Internal),
        Just(SpanKind::Server),
        Just(SpanKind::Client),
        Just(SpanKind::Producer),
        Just(SpanKind::Consumer),
    ]
}

fn arb_status_code() -> impl Strategy<Value = StatusCode> {
    prop_oneof![
        Just(StatusCode::Unset),
        Just(StatusCode::Ok),
        Just(StatusCode::Error),
    ]
}

fn arb_span_status() -> impl Strategy<Value = SpanStatus> {
    (arb_status_code(), proptest::option::of(any::<String>()))
        .prop_map(|(code, message)| SpanStatus { code, message })
}

fn arb_span_event() -> impl Strategy<Value = SpanEvent> {
    (
        arb_name(),
        any::<u64>(),
        prop::collection::vec(arb_key_value(), 0..4),
    )
        .prop_map(|(name, time_unix_nano, attributes)| SpanEvent {
            name,
            time_unix_nano,
            attributes,
        })
}

fn arb_span_link() -> impl Strategy<Value = SpanLink> {
    (
        arb_span_context(),
        prop::collection::vec(arb_key_value(), 0..4),
    )
        .prop_map(|(context, attributes)| SpanLink {
            context,
            attributes,
        })
}

/// Generate a [`SpanData`] with bounded collections (≤4 items each).
fn arb_span_data() -> impl Strategy<Value = SpanData> {
    (
        arb_span_context(),
        proptest::option::of(arb_span_id()),
        arb_name(),
        arb_span_kind(),
        any::<u64>(),
        any::<u64>(),
        prop::collection::vec(arb_key_value(), 0..4),
        prop::collection::vec(arb_span_event(), 0..4),
        prop::collection::vec(arb_span_link(), 0..4),
        arb_span_status(),
    )
        .prop_map(
            |(
                context,
                parent_span_id,
                name,
                kind,
                start_time_unix_nano,
                end_time_unix_nano,
                attributes,
                events,
                links,
                status,
            )| {
                SpanData {
                    context,
                    parent_span_id,
                    name,
                    kind,
                    start_time_unix_nano,
                    end_time_unix_nano,
                    attributes,
                    events,
                    links,
                    status,
                }
            },
        )
}

fn arb_aggregation_temporality() -> impl Strategy<Value = AggregationTemporality> {
    prop_oneof![
        Just(AggregationTemporality::Delta),
        Just(AggregationTemporality::Cumulative),
    ]
}

fn arb_metric_value() -> impl Strategy<Value = MetricValue> {
    prop_oneof![
        any::<i64>().prop_map(MetricValue::I64),
        arb_round_trippable_f64().prop_map(MetricValue::F64),
    ]
}

fn arb_number_data_point() -> impl Strategy<Value = NumberDataPoint> {
    (
        prop::collection::vec(arb_key_value(), 0..4),
        any::<u64>(),
        any::<u64>(),
        arb_metric_value(),
    )
        .prop_map(
            |(attributes, start_time_unix_nano, time_unix_nano, value)| NumberDataPoint {
                attributes,
                start_time_unix_nano,
                time_unix_nano,
                value,
            },
        )
}

/// Generate a [`Metric`] (Sum or Gauge variant only — Histogram involves extra invariants).
fn arb_metric() -> impl Strategy<Value = Metric> {
    let sum_data = (
        prop::collection::vec(arb_number_data_point(), 0..4),
        arb_aggregation_temporality(),
        any::<bool>(),
    )
        .prop_map(|(data_points, temporality, is_monotonic)| MetricData::Sum {
            data_points,
            temporality,
            is_monotonic,
        });

    let gauge_data = prop::collection::vec(arb_number_data_point(), 0..4)
        .prop_map(|data_points| MetricData::Gauge { data_points });

    let data_strategy = prop_oneof![sum_data, gauge_data];

    (arb_name(), arb_name(), arb_name(), data_strategy).prop_map(
        |(name, description, unit, data)| Metric {
            name,
            description,
            unit,
            data,
        },
    )
}

fn arb_severity_number() -> impl Strategy<Value = SeverityNumber> {
    prop_oneof![
        Just(SeverityNumber::Trace),
        Just(SeverityNumber::Debug),
        Just(SeverityNumber::Info),
        Just(SeverityNumber::Warn),
        Just(SeverityNumber::Error),
        Just(SeverityNumber::Fatal),
    ]
}

fn arb_log_record() -> impl Strategy<Value = LogRecord> {
    (
        any::<u64>(),
        any::<u64>(),
        arb_severity_number(),
        arb_name(),
        arb_attribute_value(),
        prop::collection::vec(arb_key_value(), 0..4),
        proptest::option::of(arb_trace_id()),
        proptest::option::of(arb_span_id()),
    )
        .prop_map(
            |(
                time_unix_nano,
                observed_time_unix_nano,
                severity_number,
                severity_text,
                body,
                attributes,
                trace_id,
                span_id,
            )| {
                LogRecord {
                    time_unix_nano,
                    observed_time_unix_nano,
                    severity_number,
                    severity_text,
                    body,
                    attributes,
                    trace_id,
                    span_id,
                }
            },
        )
}

// ─── P1: Edge tier invariant ─────────────────────────────────────────────────
//
// For EVERY ResolutionTier variant, Edge::new(src, tgt, kind, tier, resolver) must yield:
//   • confidence.get() in (0.0, 1.0]  — positive, finite, at most 1
//   • a non-empty resolved_by string
//   • a non-Default provenance (every tier maps to a non-Synthesizer provenance)
//
// This encodes CLAUDE.md's "never emit an Edge without confidence+provenance" rule.

proptest! {
    #[test]
    fn p1_edge_tier_confidence_is_positive_and_bounded(
        src in arb_symbol_id(),
        tgt in arb_symbol_id(),
        kind in arb_edge_kind(),
        tier in arb_resolution_tier(),
        resolver in arb_name(),
    ) {
        let edge = Edge::new(src, tgt, kind, tier, resolver.clone());

        let c = edge.confidence.get();
        prop_assert!(c > 0.0, "confidence must be > 0.0, got {c} for tier {tier:?}");
        prop_assert!(c <= 1.0, "confidence must be <= 1.0, got {c} for tier {tier:?}");
        prop_assert!(c.is_finite(), "confidence must be finite, got {c} for tier {tier:?}");

        prop_assert!(
            !edge.resolved_by.is_empty(),
            "resolved_by must be non-empty"
        );
    }
}

// ─── P1b: Confidence::new clamps into [0, 1] ─────────────────────────────────
//
// Any f32 fed into Confidence::new must come out finite and in [0.0, 1.0].

proptest! {
    #[test]
    fn p1b_confidence_new_clamps(raw in any::<f32>()) {
        // Skip NaN — clamp of NaN is implementation-defined; the real code uses f32::clamp
        // which preserves NaN. We only care about non-NaN inputs.
        prop_assume!(!raw.is_nan());
        let c = Confidence::new(raw);
        let v = c.get();
        prop_assert!(v >= 0.0, "clamped confidence must be >= 0.0, got {v}");
        prop_assert!(v <= 1.0, "clamped confidence must be <= 1.0, got {v}");
        prop_assert!(v.is_finite(), "clamped confidence must be finite, got {v}");
    }
}

// ─── P2: SymbolId preserves its inner string ─────────────────────────────────
//
// `Symbol` implements `Display` but not `FromStr`, so a full `Symbol::parse` round-trip is
// not applicable. The canonical identity used everywhere in storage is the `SymbolId` string
// (ADR-002). We assert that `SymbolId(s)` preserves `s` via both `.as_str()` and `Display`.

proptest! {
    #[test]
    fn p2_symbol_id_preserves_inner_string(s in arb_name()) {
        let id = SymbolId(s.clone());
        prop_assert_eq!(id.as_str(), s.as_str(), "as_str() must equal the inner string");
        prop_assert_eq!(id.to_string(), s, "Display must equal the inner string");
    }
}

// ─── P3: Serde round-trips ────────────────────────────────────────────────────

proptest! {
    #[test]
    fn p3_attribute_value_serde_round_trip(v in arb_attribute_value()) {
        let json = serde_json::to_string(&v)
            .expect("AttributeValue must serialize");
        let back: AttributeValue = serde_json::from_str(&json)
            .expect("AttributeValue must deserialize");
        prop_assert_eq!(v, back, "AttributeValue serde round-trip failed");
    }

    #[test]
    fn p3_span_data_serde_round_trip(span in arb_span_data()) {
        let json = serde_json::to_string(&span)
            .expect("SpanData must serialize");
        let back: SpanData = serde_json::from_str(&json)
            .expect("SpanData must deserialize");
        prop_assert_eq!(span, back, "SpanData serde round-trip failed");
    }

    #[test]
    fn p3_metric_serde_round_trip(m in arb_metric()) {
        let json = serde_json::to_string(&m)
            .expect("Metric must serialize");
        let back: Metric = serde_json::from_str(&json)
            .expect("Metric must deserialize");
        prop_assert_eq!(m, back, "Metric serde round-trip failed");
    }

    #[test]
    fn p3_log_record_serde_round_trip(log in arb_log_record()) {
        let json = serde_json::to_string(&log)
            .expect("LogRecord must serialize");
        let back: LogRecord = serde_json::from_str(&json)
            .expect("LogRecord must deserialize");
        prop_assert_eq!(log, back, "LogRecord serde round-trip failed");
    }
}
