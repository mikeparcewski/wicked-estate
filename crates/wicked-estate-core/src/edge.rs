//! Graph edges and the **edge-direction invariant**.
//!
//! INVARIANT (see `docs/ENGINE-CONTRACT.md`): an edge points from **dependent → dependency**.
//! `source` is the dependent; `target` is the dependency. So "A calls B" is stored as
//! `source=A, target=B`. Blast-radius ("what breaks if I change B?") collects edges where
//! `target == B` and walks their `source`s. This matches the hard-won
//! `DEPENDENTS_BY = "target"` rule.

use crate::node::{Location, Metadata};
use crate::symbol::SymbolId;
use serde::{Deserialize, Serialize};

/// Metadata key carrying an edge's **evidence count** — how many times the relationship has been
/// independently confirmed/contradicted (an audit counter, distinct from `confidence`). Rides the
/// opaque `Edge::metadata` slot rather than a struct field so the spine type is unchanged (no fleet
/// ripple across the stores/conformance kit); `SqliteStore` promotes it to a queryable
/// `edges.evidence_count` column. Absent key ⇒ 0. This is the estate destination for
/// wicked-brain's `links.evidence_count` signal (the brain→estate consolidation).
pub const EVIDENCE_COUNT_META_KEY: &str = "evidence_count";

/// A confidence score in `[0.0, 1.0]`. Every edge carries one (ADR-001).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(v: f32) -> Self {
        Confidence(v.clamp(0.0, 1.0))
    }
    pub fn get(self) -> f32 {
        self.0
    }
}

/// Resolution tiers, cheap→precise, each with a default confidence
///.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionTier {
    /// Direct AST fact (e.g. `contains`, `defines`) — certain.
    Parsed,
    /// tree-sitter tags only — broad, low precision.
    Tags,
    /// import-map heuristic resolution.
    ImportMap,
    /// other heuristic / synthesizer.
    Heuristic,
    /// stack-graphs TSG name resolution.
    Tsg,
    /// SCIP indexer output — precise.
    Scip,
    /// on-demand LSP — precise.
    Lsp,
}

impl ResolutionTier {
    pub fn default_confidence(self) -> Confidence {
        Confidence::new(match self {
            ResolutionTier::Parsed | ResolutionTier::Scip | ResolutionTier::Lsp => 1.0,
            ResolutionTier::Tsg => 0.8,
            ResolutionTier::ImportMap => 0.6,
            ResolutionTier::Heuristic => 0.5,
            ResolutionTier::Tags => 0.3,
        })
    }

    fn provenance(self) -> Provenance {
        match self {
            ResolutionTier::Parsed => Provenance::Parsed,
            ResolutionTier::Tags => Provenance::Tags,
            ResolutionTier::ImportMap => Provenance::ImportMap,
            ResolutionTier::Heuristic => Provenance::Heuristic,
            ResolutionTier::Tsg => Provenance::Tsg,
            ResolutionTier::Scip => Provenance::Scip,
            ResolutionTier::Lsp => Provenance::Lsp,
        }
    }
}

/// How an edge was produced. Distinct from `resolved_by` (the *specific* resolver id) so we can
/// both classify (parsed vs heuristic) and trace (which resolver) — and monitor precision per
/// class (fixes the "no false-positive monitoring"; see the design notes).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Parsed,
    Tags,
    ImportMap,
    Heuristic,
    Tsg,
    Scip,
    Lsp,
    /// A dynamic-dispatch synthesizer, by name (e.g. "callback-edge-v2").
    Synthesizer(String),
    /// A drop-in extractor, by name (e.g. "event-bus", "django-orm").
    Extractor(String),
}

/// The kind of relationship. `Other(String)` carries non-code edges (event-bus, dispatch, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    Defines,
    Calls,
    Imports,
    References,
    Instantiates,
    Implements,
    Extends,
    Overrides,
    HasType,
    Returns,
    // ── Rules engine relationships (W15) ─────────────────────────────────────
    /// A Rule constrains or applies to a code symbol or Fact.
    Governs,
    /// A Rule matches on / reads from a Fact type (LHS binding).
    Evaluates,
    /// A Rule asserts or modifies a Fact type (RHS output).
    Produces,
    /// A code call site triggers a RuleSet (code → rules engine boundary).
    InvokedBy,
    Other(String),
}

/// A directed, attributed edge: `source` (dependent) → `target` (dependency).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// The DEPENDENT end (invariant).
    pub source: SymbolId,
    /// The DEPENDENCY end (invariant).
    pub target: SymbolId,
    pub kind: EdgeKind,
    pub confidence: Confidence,
    pub provenance: Provenance,
    /// The concrete resolver that produced this edge (e.g. "scip-typescript", "import-map-py").
    pub resolved_by: String,
    /// Where the relationship occurs (the call site, the import statement, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    #[serde(default)]
    pub metadata: Metadata,
}

impl Edge {
    /// Construct an edge, deriving confidence + provenance from the resolution tier.
    pub fn new(
        source: SymbolId,
        target: SymbolId,
        kind: EdgeKind,
        tier: ResolutionTier,
        resolved_by: impl Into<String>,
    ) -> Self {
        Self {
            source,
            target,
            kind,
            confidence: tier.default_confidence(),
            provenance: tier.provenance(),
            resolved_by: resolved_by.into(),
            location: None,
            metadata: Metadata::new(),
        }
    }

    pub fn with_location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    /// Set this edge's evidence count (audit counter — how many times the relationship has been
    /// confirmed/contradicted). Stored in `metadata` under [`EVIDENCE_COUNT_META_KEY`] so the spine
    /// struct is unchanged; `SqliteStore` promotes it to the `edges.evidence_count` column. Builder.
    pub fn with_evidence_count(mut self, n: u32) -> Self {
        self.metadata.insert(
            EVIDENCE_COUNT_META_KEY.to_string(),
            serde_json::Value::from(n),
        );
        self
    }

    /// This edge's evidence count (0 when the key is absent — the honest default for an edge that
    /// has never been confirmed). Reads the [`EVIDENCE_COUNT_META_KEY`] metadata slot.
    pub fn evidence_count(&self) -> u32 {
        self.metadata
            .get(EVIDENCE_COUNT_META_KEY)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32
    }

    /// Deduplication key: a relationship is identified by its endpoints + kind. When two
    /// resolvers produce the same edge, the store keeps the higher-confidence one (W3.4).
    pub fn dedup_key(&self) -> (String, String, String) {
        (
            self.source.0.clone(),
            self.target.0.clone(),
            // EdgeKind is small + serde-friendly; stringify for a stable key.
            serde_json::to_string(&self.kind).unwrap_or_default(),
        )
    }
}

/// Which side of an edge a query is anchored on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Edges where the node is the `target` — i.e. its **dependents** (blast-radius).
    Dependents,
    /// Edges where the node is the `source` — i.e. its **dependencies**.
    Dependencies,
    Both,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e() -> Edge {
        Edge::new(
            SymbolId("a".into()),
            SymbolId("b".into()),
            EdgeKind::Other("governs".into()),
            ResolutionTier::Heuristic,
            "test",
        )
    }

    #[test]
    fn evidence_count_defaults_to_zero() {
        // An edge that was never confirmed reads back 0 — the honest default. Legacy edges (whose
        // JSON has no evidence_count key) hydrate the same way.
        assert_eq!(e().evidence_count(), 0);
    }

    #[test]
    fn with_evidence_count_round_trips_through_metadata() {
        let edge = e().with_evidence_count(7);
        assert_eq!(edge.evidence_count(), 7);
        // It rides the opaque metadata slot — the spine struct gained no field.
        assert_eq!(
            edge.metadata
                .get(EVIDENCE_COUNT_META_KEY)
                .and_then(|v| v.as_u64()),
            Some(7)
        );
    }

    #[test]
    fn evidence_count_survives_serde_round_trip() {
        // This is the persistence path: SqliteStore stores the full Edge as JSON in edges.data, so
        // evidence_count MUST survive a serialize→deserialize cycle for the migration to be lossless.
        let edge = e().with_evidence_count(42);
        let json = serde_json::to_string(&edge).unwrap();
        let back: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.evidence_count(), 42);
    }
}
