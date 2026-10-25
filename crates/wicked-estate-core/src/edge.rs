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
    /// A compiler or toolchain catalog's semantic evidence (TS-S2C) — precise for what it
    /// declares. Distinct from `Scip` so a compiler fact is never attributed to an index.
    Compiler,
}

impl ResolutionTier {
    pub fn default_confidence(self) -> Confidence {
        Confidence::new(match self {
            ResolutionTier::Parsed
            | ResolutionTier::Scip
            | ResolutionTier::Lsp
            | ResolutionTier::Compiler => 1.0,
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
            ResolutionTier::Compiler => Provenance::Compiler,
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
    /// A compiler or toolchain catalog (TS-S2C semantic evidence, `ProducerClass::Compiler`).
    Compiler,
    /// A dynamic-dispatch synthesizer, by name (e.g. "callback-edge-v2").
    Synthesizer(String),
    /// A drop-in extractor, by name (e.g. "event-bus", "django-orm").
    Extractor(String),
}

/// The kind of relationship. `Other(String)` carries non-code edges (event-bus, dispatch, …).
///
/// `Ord` is derived so callers can break ties on edge kind without allocating — the path
/// query's adjacency sort does, once per comparison over every admissible edge. The order is
/// declaration order and carries no meaning beyond being total and stable; do not read
/// precedence into it, and do not reorder the variants to express one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    // ── Requirements / traceability ──────────────────────────────────────────
    /// A requirement is satisfied by a code symbol. Direction: `source` = requirement (the
    /// dependent), `target` = code (the dependency) — so `BlastRadius(code)` (dependents of the
    /// code) surfaces the requirements a change to it may break, exactly the traceability query.
    /// Mirrors the `Governs` rule→code precedent for the requirements overlay.
    SatisfiedBy,
    Other(String),
}

impl EdgeKind {
    /// Every built-in (unit) variant, in declaration order. Named in the refusal
    /// [`parse_user`](Self::parse_user) returns, so a caller who mistyped a kind sees what exists.
    /// `edge_kind_builtins_cover_every_unit_variant` fails to compile when a variant is added and
    /// not listed here.
    pub const BUILTINS: [EdgeKind; 16] = [
        EdgeKind::Contains,
        EdgeKind::Defines,
        EdgeKind::Calls,
        EdgeKind::Imports,
        EdgeKind::References,
        EdgeKind::Instantiates,
        EdgeKind::Implements,
        EdgeKind::Extends,
        EdgeKind::Overrides,
        EdgeKind::HasType,
        EdgeKind::Returns,
        EdgeKind::Governs,
        EdgeKind::Evaluates,
        EdgeKind::Produces,
        EdgeKind::InvokedBy,
        EdgeKind::SatisfiedBy,
    ];

    /// The one parser for an edge kind a user, a tool argument or a config file names (#361).
    ///
    /// * A built-in's stored spelling (`calls`, `has_type`, `satisfied_by`, …) is that variant —
    ///   decided by serde, so no variant list is hand-maintained here.
    /// * `other:<tag>` is the tag `<tag>`, explicitly.
    /// * A near-spelling of a built-in — any case, `_`/`-` dropped or swapped (`Calls`,
    ///   `HasType`, `invoked-by`) — is REFUSED naming the built-in, never taken as a tag: no store
    ///   writes that tag, so it would answer an honest-looking empty result for the kind the
    ///   caller meant (the accept-and-ignore class #197 closed for flags).
    /// * A tag that is a built-in's spelling or near-spelling (`other:calls`, `{"other":"Calls"}`)
    ///   and a nested prefix (`other:other:x`) are refused for the same reason.
    /// * The empty string, and an empty `other:` tag, are refused.
    /// * Anything else is the tag `Other(s)`. Tags are open data — drop-in rules mint their own
    ///   (`emits`, `consumes`) — so they are NOT checked against [`crate::edge_tags`].
    pub fn parse_user(s: &str) -> std::result::Result<EdgeKind, String> {
        if s.is_empty() {
            return Err(format!(
                "an edge kind must not be empty; built-in kinds: {}",
                Self::builtin_spellings()
            ));
        }
        if let Some(tag) = s.strip_prefix("other:") {
            return Self::parse_tag(tag, &format!("{s:?}"));
        }
        if let Some(k) = Self::builtin_exact(s) {
            return Ok(k);
        }
        if let Some(spelling) = Self::builtin_near(s) {
            return Err(format!(
                "{s:?} is not an edge kind; did you mean {spelling:?}? built-in kinds: {}",
                Self::builtin_spellings()
            ));
        }
        Ok(EdgeKind::Other(s.to_string()))
    }

    /// [`parse_user`](Self::parse_user) over a JSON item: a string, or the `{"other":"<tag>"}`
    /// object `export` and `path --json` print. Any other shape is refused, never dropped.
    pub fn parse_user_value(v: &serde_json::Value) -> std::result::Result<EdgeKind, String> {
        if let Some(s) = v.as_str() {
            return Self::parse_user(s);
        }
        let tag = v
            .as_object()
            .filter(|obj| obj.len() == 1)
            .and_then(|obj| obj.get("other"))
            .and_then(|t| t.as_str());
        if let Some(tag) = tag {
            return Self::parse_tag(tag, &v.to_string());
        }
        Err(format!(
            "{v} is not an edge kind: give a string (calls, flows_to, other:flows_to) or \
             {{\"other\":\"<tag>\"}}; built-in kinds: {}",
            Self::builtin_spellings()
        ))
    }

    /// An explicit tag (`other:<tag>` / `{"other":tag}`); `shown` is the caller's spelling.
    fn parse_tag(tag: &str, shown: &str) -> std::result::Result<EdgeKind, String> {
        if tag.is_empty() {
            return Err(format!(
                "{shown} names no tag; write other:<tag> (e.g. other:flows_to)"
            ));
        }
        if tag.starts_with("other:") {
            return Err(format!(
                "{shown} nests the other: prefix; write other:<tag> once"
            ));
        }
        if let Some(spelling) = Self::builtin_near(tag) {
            return Err(format!(
                "{shown} names the built-in kind {spelling:?} as a tag, which no store writes; \
                 write {spelling:?}"
            ));
        }
        Ok(EdgeKind::Other(tag.to_string()))
    }

    /// The built-in whose stored spelling is exactly `s`.
    fn builtin_exact(s: &str) -> Option<EdgeKind> {
        match serde_json::from_value(serde_json::Value::String(s.to_string())) {
            Ok(EdgeKind::Other(_)) | Err(_) => None,
            Ok(k) => Some(k),
        }
    }

    /// The stored spelling of the built-in that `s` spells, exactly or nearly: equal after ASCII
    /// case folding with `_` and `-` removed (`calls`, `Calls`, `HasType`, `has-type`,
    /// `INVOKED_BY`).
    fn builtin_near(s: &str) -> Option<String> {
        let norm = |x: &str| -> String {
            x.chars()
                .filter(|c| *c != '_' && *c != '-')
                .map(|c| c.to_ascii_lowercase())
                .collect()
        };
        let want = norm(s);
        Self::BUILTINS
            .iter()
            .filter_map(Self::spelling)
            .find(|spelling| norm(spelling) == want)
    }

    /// A built-in's stored (serde) spelling; `None` for `Other`.
    fn spelling(k: &EdgeKind) -> Option<String> {
        match serde_json::to_value(k) {
            Ok(serde_json::Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// The stored spelling of every built-in, comma-separated, for refusal messages.
    pub fn builtin_spellings() -> String {
        Self::BUILTINS
            .iter()
            .filter_map(Self::spelling)
            .collect::<Vec<_>>()
            .join(", ")
    }
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
    /// **Evidence count** — how many times this relationship has been independently confirmed /
    /// contradicted (an audit counter, distinct from `confidence`). The estate destination for
    /// wicked-brain's `links.evidence_count` signal. Structurally-derived code edges leave it at 0;
    /// knowledge relations set it via `knowledge.relate`. `#[serde(default)]` so an `Edge` persisted
    /// before this field existed (the JSON in `edges.data`) deserializes to 0 — the honest default
    /// for a relationship nobody has confirmed. `SqliteStore` also promotes it to a queryable
    /// `edges.evidence_count` column.
    #[serde(default)]
    pub evidence_count: u32,
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
            evidence_count: 0,
            location: None,
            metadata: Metadata::new(),
        }
    }

    pub fn with_location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    /// Set this edge's [`evidence_count`](Self::evidence_count) audit counter. Builder sugar over the
    /// public field; used by `knowledge.relate` to land wicked-brain's `links.evidence_count` value.
    pub fn with_evidence_count(mut self, n: u32) -> Self {
        self.evidence_count = n;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #361: tag kinds parse to `Other`, in every input form, and nothing is dropped.
    #[test]
    fn parse_user_takes_tags_in_every_form_and_refuses_the_rest() {
        let ok = |v: serde_json::Value| EdgeKind::parse_user_value(&v).unwrap();
        let other = |t: &str| EdgeKind::Other(t.to_string());
        assert_eq!(ok(serde_json::json!("flows_to")), other("flows_to"));
        assert_eq!(
            ok(serde_json::json!({"other": "di-wired"})),
            other("di-wired")
        );
        assert_eq!(ok(serde_json::json!("other:di-wired")), other("di-wired"));
        // A tag a drop-in rule minted, not in edge_tags.rs: still a tag.
        assert_eq!(ok(serde_json::json!("emits")), other("emits"));
        assert_eq!(ok(serde_json::json!("calls")), EdgeKind::Calls);
        for bad in [
            serde_json::json!("Calls"),
            // Multi-word built-ins: case variants and separator variants are near-spellings too.
            serde_json::json!("HasType"),
            serde_json::json!("InvokedBy"),
            serde_json::json!("satisfiedBy"),
            serde_json::json!("has-type"),
            serde_json::json!("INVOKED_BY"),
            // A built-in named as a tag, and a nested prefix: tags no store writes.
            serde_json::json!("other:calls"),
            serde_json::json!("other:Calls"),
            serde_json::json!({"other": "calls"}),
            serde_json::json!({"other": "HasType"}),
            serde_json::json!("other:other:flows_to"),
            serde_json::json!({"other": "other:flows_to"}),
            serde_json::json!(""),
            serde_json::json!(42),
            serde_json::json!("other:"),
            serde_json::json!({"other": ""}),
            serde_json::json!({"other": "a", "x": 1}),
            serde_json::json!(null),
        ] {
            let err = EdgeKind::parse_user_value(&bad)
                .expect_err(&format!("{bad} must be refused, not dropped or widened"));
            assert!(!err.is_empty(), "{bad}");
        }
        let err = EdgeKind::parse_user("Calls").unwrap_err();
        assert!(
            err.contains("\"Calls\"") && err.contains("\"calls\""),
            "{err}"
        );
        assert!(
            err.contains("satisfied_by"),
            "refusal lists the built-ins: {err}"
        );
        let err = EdgeKind::parse_user("HasType").unwrap_err();
        assert!(err.contains("did you mean \"has_type\""), "{err}");
        let err = EdgeKind::parse_user_value(&serde_json::json!({"other": "calls"})).unwrap_err();
        assert!(err.contains("write \"calls\""), "{err}");
        // Tags that merely CONTAIN a built-in spelling are still tags.
        assert_eq!(ok(serde_json::json!("calls_out")), other("calls_out"));
        assert_eq!(ok(serde_json::json!("event-emits")), other("event-emits"));
    }

    /// #361: every built-in round-trips through the one parser from its stored spelling — the
    /// extractor-config gap where `kind = "defines"` became the tag `defines`.
    #[test]
    fn every_builtin_round_trips_through_parse_user() {
        for k in &EdgeKind::BUILTINS {
            let spelling = match serde_json::to_value(k).unwrap() {
                serde_json::Value::String(s) => s,
                other => panic!("built-in {k:?} serialized as {other}"),
            };
            assert_eq!(&EdgeKind::parse_user(&spelling).unwrap(), k, "{spelling}");
        }
        assert_eq!(EdgeKind::parse_user("defines").unwrap(), EdgeKind::Defines);
        assert_eq!(EdgeKind::parse_user("has_type").unwrap(), EdgeKind::HasType);
    }

    /// Fails to compile when a unit variant is added without a `BUILTINS` row; the runtime
    /// assertion then checks the row is really there.
    #[test]
    fn edge_kind_builtins_cover_every_unit_variant() {
        let listed = |k: EdgeKind| EdgeKind::BUILTINS.contains(&k);
        for k in EdgeKind::BUILTINS {
            let covered = match k.clone() {
                EdgeKind::Contains
                | EdgeKind::Defines
                | EdgeKind::Calls
                | EdgeKind::Imports
                | EdgeKind::References
                | EdgeKind::Instantiates
                | EdgeKind::Implements
                | EdgeKind::Extends
                | EdgeKind::Overrides
                | EdgeKind::HasType
                | EdgeKind::Returns
                | EdgeKind::Governs
                | EdgeKind::Evaluates
                | EdgeKind::Produces
                | EdgeKind::InvokedBy
                | EdgeKind::SatisfiedBy => listed(k),
                EdgeKind::Other(_) => false,
            };
            assert!(covered);
        }
        let unique: std::collections::BTreeSet<_> = EdgeKind::BUILTINS.iter().collect();
        assert_eq!(unique.len(), EdgeKind::BUILTINS.len());
    }

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
        // A structurally-derived edge (Edge::new) was never confirmed → 0, the honest default.
        assert_eq!(e().evidence_count, 0);
    }

    #[test]
    fn with_evidence_count_sets_the_field() {
        let edge = e().with_evidence_count(7);
        assert_eq!(edge.evidence_count, 7, "builder sets the first-class field");
    }

    #[test]
    fn evidence_count_survives_serde_round_trip() {
        // The persistence path: every backend stores the full Edge as JSON in `data`, so
        // evidence_count MUST survive a serialize→deserialize cycle for the migration to be lossless.
        let edge = e().with_evidence_count(42);
        let json = serde_json::to_string(&edge).unwrap();
        let back: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.evidence_count, 42);
    }

    #[test]
    fn legacy_json_without_evidence_count_deserializes_to_zero() {
        // An Edge persisted before the field existed (no `evidence_count` key in the JSON) must
        // hydrate to 0 via `#[serde(default)]` — the backward-compat guarantee for old `edges.data`.
        let legacy = r#"{"source":"a","target":"b","kind":{"other":"governs"},
            "confidence":0.5,"provenance":"heuristic","resolved_by":"test"}"#;
        let back: Edge = serde_json::from_str(legacy).unwrap();
        assert_eq!(back.evidence_count, 0);
    }

    #[test]
    fn satisfied_by_kind_serdes_snake_case_and_round_trips() {
        // The requirements-graph traceability edge (requirement→code). The store persists EdgeKind
        // via serde_json, so it must round-trip as itself — a distinct native variant, NOT coerced
        // to `Other("satisfied_by")` — so `traverse`/blast-radius can filter on it by kind.
        let edge = Edge::new(
            SymbolId("req".into()),
            SymbolId("code".into()),
            EdgeKind::SatisfiedBy,
            ResolutionTier::Heuristic,
            "domain-graph",
        );
        let json = serde_json::to_string(&edge).unwrap();
        assert!(
            json.contains(r#""kind":"satisfied_by""#),
            "SatisfiedBy serializes snake_case, got {json}"
        );
        let back: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, EdgeKind::SatisfiedBy);
        assert_ne!(
            back.kind,
            EdgeKind::Other("satisfied_by".into()),
            "SatisfiedBy is a first-class variant, not the Other escape hatch"
        );
    }
}
