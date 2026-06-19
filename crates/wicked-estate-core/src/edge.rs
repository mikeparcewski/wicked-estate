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
