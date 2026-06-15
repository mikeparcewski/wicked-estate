//! `wicked-estate-core` — the spine of the wicked_estate engine.
//!
//! This crate is deliberately small and dependency-light. It defines:
//!   * the **graph data model** ([`Node`], [`Edge`], [`Symbol`]/[`SymbolId`]) with
//!     stable symbol identity and `{confidence, provenance, resolved_by}` on every edge,
//!   * the **two-phase pipeline** staging type ([`UnresolvedRef`] / [`Extraction`]), and
//!   * the **five traits** every other crate programs against:
//!     [`Extractor`], [`Resolver`], [`GraphStore`], [`Ranker`], [`RetrievalTool`].
//!
//! Fixing these here — before any fan-out — is what lets independent agents own
//! `wicked-estate-extract`, `wicked-estate-resolve`, `wicked-estate-store`, … without colliding. See `docs/plan/WAVE-PLAN.md`
//! (Wave 0) and `docs/ENGINE-CONTRACT.md` (the edge-direction invariant).

pub mod change;
pub mod conformance;
pub mod edge;
pub mod error;
pub mod history;
pub mod node;
pub mod observability;
pub mod query;
pub mod refs;
pub mod repo;
pub mod semantics;
pub mod symbol;
pub mod traits;

pub use change::{Change, ChangeOp};
pub use edge::{Confidence, Direction, Edge, EdgeKind, Provenance, ResolutionTier};
pub use error::{Error, Result};
pub use history::HistoricalEdge;
pub use node::{Language, Location, Metadata, Node, NodeKind, SourceFile, Span};
pub use observability::{
    AggregationTemporality, AttributeValue, ExportError, ExportResult, ExporterConfig,
    HistogramDataPoint, InMemorySink, InstrumentKind, InstrumentationScope, KeyValue, LogRecord,
    Metric, MetricData, MetricValue, NoopSink, NumberDataPoint, Protocol, Resource, Sampler,
    SeverityNumber, SpanContext, SpanData, SpanEvent, SpanId, SpanKind, SpanLink, SpanStatus,
    StatusCode, TelemetrySink, TraceId, open_telemetry_sink,
};
pub use query::{GraphStats, RetrievalResult, Subgraph, SymbolQuery, TraversalSpec};
pub use refs::{Extraction, UnresolvedRef};
pub use repo::RepoInfo;
pub use semantics::NodeSemantics;
pub use symbol::{Descriptor, Package, Suffix, Symbol, SymbolId};
pub use traits::{
    AsyncGraphStore, Extractor, GraphRead, GraphStore, GraphWrite, Ranker, Resolver, RetrievalTool,
    StoreCapabilities, SymbolIndex,
};
