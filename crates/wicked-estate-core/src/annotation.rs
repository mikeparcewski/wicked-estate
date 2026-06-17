//! Typed key/value annotations on graph entities.
//!
//! An annotation is a `(type, key, value, confidence, provenance, author, ts)` record attached to
//! a symbol. The `type` discriminator distinguishes notes, assumptions, observations, comments,
//! questions, a system-derived `community` label, and **arbitrary custom types** — all stored and
//! queried **identically**. The known set is a *convention services branch on* via [`classify`];
//! storage and the [`GraphStore`](crate::GraphStore) trait never `match` on it (the "rules as
//! DATA" Don't). A custom type is a first-class citizen: it round-trips and filters exactly like a
//! known one, and falls through [`classify`] to [`AnnotationClass::Custom`].
//!
//! This is the seam that lets retrieval/MCP surface annotations: read methods live on
//! [`GraphRead`](crate::GraphRead), so every consumer that holds `&dyn GraphRead` can reach them.
//! See `docs/recon/annotation-typed-notes-design.md`.

use serde::{Deserialize, Serialize};

/// The default annotation type. Untyped/legacy rows read back as this (the safest default — an old
/// untyped tag was never an assumption or a question).
pub const DEFAULT_ANNOTATION_TYPE: &str = "note";

/// The known annotation types. These are **conventions** a service may branch on (via
/// [`classify`]); they are NOT an exhaustive set. Custom types are stored/queried identically and
/// classify to [`AnnotationClass::Custom`]. Adding a known type is one new arm in [`classify`] —
/// no schema change, no storage change.
pub const KNOWN_ANNOTATION_TYPES: &[&str] = &[
    "note",
    "assumption",
    "observation",
    "comment",
    "question",
    "community",
];

/// The semantic class a service uses to branch on an annotation's `type`. [`AnnotationClass::Custom`]
/// deliberately maps onto the same generic features as a known type — that is the "custom types get
/// the special features for free" guarantee. Only the *special* semantic hooks key off the known
/// classes; every generic path (store / query / filter / payload) treats `Custom` identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationClass {
    /// Free-form human/agent note (the default). No trust effect.
    Note,
    /// An unverified assumption — advisory; lowers trust, must be presented as not-a-fact (R7).
    Assumption,
    /// A recorded fact-of-observation. Informational; higher implicit trust than an assumption.
    Observation,
    /// Lightweight remark / discussion. Informational; lowest semantic weight.
    Comment,
    /// An open question — advisory; signals incomplete understanding.
    Question,
    /// A machine-derived community / grouping label. System-derived; not a human annotation.
    Community,
    /// Any type outside the known set. Handled by the same generic machinery as a known type.
    Custom,
}

/// Classify an annotation `type` string into its [`AnnotationClass`]. Unknown strings (custom
/// types) map to [`AnnotationClass::Custom`]. Matching is case-sensitive: the known types are exact
/// lowercase conventions, and `"Note"` is a distinct custom type from `"note"`.
pub fn classify(ty: &str) -> AnnotationClass {
    match ty {
        "note" => AnnotationClass::Note,
        "assumption" => AnnotationClass::Assumption,
        "observation" => AnnotationClass::Observation,
        "comment" => AnnotationClass::Comment,
        "question" => AnnotationClass::Question,
        "community" => AnnotationClass::Community,
        _ => AnnotationClass::Custom,
    }
}

/// Does this annotation type reduce trust in the entity / need human review? Assumptions and open
/// questions are advisory (agent-behavior R7: present as not-a-fact). Custom types are never
/// advisory — they fall through to the generic informational path.
pub fn is_advisory(ty: &str) -> bool {
    matches!(
        classify(ty),
        AnnotationClass::Assumption | AnnotationClass::Question
    )
}

/// Is this annotation type machine-derived rather than human-asserted? (`community`, and future
/// derived types.) System-derived rows are excluded from "N notes" tallies and rendered as a
/// grouping label, not a user note.
pub fn is_system_derived(ty: &str) -> bool {
    matches!(classify(ty), AnnotationClass::Community)
}

/// The default `source_type`. Untyped/legacy rows read back as this — the safest default, since an
/// old annotation predating the evidence envelope made no claim about *what kind of source* backed
/// it. Distinct from `provenance` (free-form) and `extraction_method` (the *how*): this is the *kind
/// of thing* the fact came from.
pub const DEFAULT_SOURCE_TYPE: &str = "unspecified";

/// The known `source_type` values. Like [`KNOWN_ANNOTATION_TYPES`], these are **conventions** —
/// not an exhaustive enum. Any other string is accepted and stored/queried identically (the "rules
/// as DATA" Don't); the store never `match`es on this. Mirrors the factory's evidence-model source
/// taxonomy (code / config / sme-answer / static-analysis / runtime-trace / …).
pub const KNOWN_SOURCE_TYPES: &[&str] = &[
    "unspecified",
    "code",
    "config",
    "sme-answer",
    "static-analysis",
    "runtime-trace",
    "documentation",
];

/// The default `extraction_method`. Legacy rows — and human-asserted notes — read back as this.
/// Distinct from `source_type` (what kind of source) and `provenance` (the free-form origin string,
/// e.g. `"louvain:res=1.0"`): this records *by what method* the fact was extracted, e.g. a tool name
/// + version (`"scip-rust@0.3"`) or `"manual"` for a hand-written note.
pub const DEFAULT_EXTRACTION_METHOD: &str = "manual";

/// A single typed annotation attached to a symbol.
///
/// `r#type` is a plain string (no storage enum): a known convention or an arbitrary custom type.
///
/// # Evidence envelope (additive, backward-compatible)
///
/// `source_type`, `extraction_method`, and `last_verified` form an *evidence envelope* that records
/// **what kind of source** backed the fact, **by what method** it was extracted, and **when it was
/// last re-verified** (a freshness clock distinct from `ts`, which is write-time). They split the
/// overloaded free-form `provenance` (which conflated *source* and *method*) into named axes and add
/// the freshness signal the layer previously had nowhere on a fact.
///
/// All three — like `r#type` — are `#[serde(default = …)]` **trailing** fields, so an `Annotation`
/// serialized by an older build (without any of them) still deserializes: `source_type` →
/// [`DEFAULT_SOURCE_TYPE`], `extraction_method` → [`DEFAULT_EXTRACTION_METHOD`], `last_verified` →
/// `0` (never verified). No data rewrite; old rows backfill on read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// The annotation key (arbitrary string).
    pub key: String,
    /// The annotation value (arbitrary string).
    pub value: String,
    /// Confidence in this annotation, 0.0–1.0. Defaults to 1.0 for human-asserted notes.
    pub confidence: f64,
    /// Where this annotation came from (free-form, e.g. `"manual"`, `"louvain:res=1.0"`).
    pub provenance: String,
    /// Who/what authored it (free-form, e.g. a username or `"system"`).
    pub author: String,
    /// Unix-seconds timestamp the annotation was written.
    pub ts: i64,
    /// The annotation type discriminator. Known set in [`KNOWN_ANNOTATION_TYPES`]; any other string
    /// is a custom type. Defaults to [`DEFAULT_ANNOTATION_TYPE`] for legacy/untyped rows.
    #[serde(default = "default_annotation_type", rename = "type")]
    pub r#type: String,
    /// Evidence envelope — **what kind of source** backed this fact (known set in
    /// [`KNOWN_SOURCE_TYPES`]; any other string is accepted identically). Defaults to
    /// [`DEFAULT_SOURCE_TYPE`] for legacy rows. Distinct from `provenance`: this is the *kind* of
    /// source, not the free-form origin string.
    #[serde(default = "default_source_type")]
    pub source_type: String,
    /// Evidence envelope — **by what method** this fact was extracted: a tool name + version
    /// (e.g. `"scip-rust@0.3"`) or [`DEFAULT_EXTRACTION_METHOD`] (`"manual"`) for hand-written notes.
    /// Defaults to [`DEFAULT_EXTRACTION_METHOD`] for legacy rows.
    #[serde(default = "default_extraction_method")]
    pub extraction_method: String,
    /// Evidence envelope — **when this fact was last re-verified**, as a Unix-seconds timestamp.
    /// Distinct from `ts` (write-time): this is the freshness clock a re-verification window reads to
    /// flag stale facts (see [`Annotation::is_stale_since`]). `0` means *never verified* — the
    /// default for legacy rows and for facts that have not yet been checked.
    #[serde(default)]
    pub last_verified: i64,
}

fn default_annotation_type() -> String {
    DEFAULT_ANNOTATION_TYPE.to_string()
}

fn default_source_type() -> String {
    DEFAULT_SOURCE_TYPE.to_string()
}

fn default_extraction_method() -> String {
    DEFAULT_EXTRACTION_METHOD.to_string()
}

impl Annotation {
    /// Construct a typed annotation with `confidence = 1.0` and the given fields. `ts` is left 0 so
    /// the store can stamp it (`strftime('%s','now')` in SQLite) on insert.
    pub fn new(
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Annotation {
            key: key.into(),
            value: value.into(),
            confidence: 1.0,
            provenance: String::new(),
            author: String::new(),
            ts: 0,
            r#type: r#type.into(),
            // Evidence-envelope defaults: a freshly-constructed annotation makes no source/method
            // claim and has never been verified. Callers opt in via the `with_*` builders below.
            source_type: DEFAULT_SOURCE_TYPE.to_string(),
            extraction_method: DEFAULT_EXTRACTION_METHOD.to_string(),
            last_verified: 0,
        }
    }

    /// Construct a `note`-typed annotation (the default type).
    pub fn note(key: impl Into<String>, value: impl Into<String>) -> Self {
        Annotation::new(DEFAULT_ANNOTATION_TYPE, key, value)
    }

    /// Set the confidence (builder-style).
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence;
        self
    }

    /// Set the provenance (builder-style).
    pub fn with_provenance(mut self, provenance: impl Into<String>) -> Self {
        self.provenance = provenance.into();
        self
    }

    /// Set the author (builder-style).
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    /// Set the evidence-envelope `source_type` (builder-style) — the *kind* of source backing this
    /// fact. See [`KNOWN_SOURCE_TYPES`] for the conventional values (any string is accepted).
    pub fn with_source_type(mut self, source_type: impl Into<String>) -> Self {
        self.source_type = source_type.into();
        self
    }

    /// Set the evidence-envelope `extraction_method` (builder-style) — the tool+version (or
    /// `"manual"`) that produced this fact.
    pub fn with_extraction_method(mut self, extraction_method: impl Into<String>) -> Self {
        self.extraction_method = extraction_method.into();
        self
    }

    /// Set the evidence-envelope `last_verified` clock (builder-style) — the Unix-seconds time this
    /// fact was last re-verified. `0` means never verified.
    pub fn with_last_verified(mut self, last_verified: i64) -> Self {
        self.last_verified = last_verified;
        self
    }

    /// Whether this annotation is **stale** relative to `cutoff` (a Unix-seconds threshold): true
    /// when it was last verified *strictly before* `cutoff`. A never-verified annotation
    /// (`last_verified == 0`) is stale for any positive `cutoff`. This is the per-fact freshness
    /// predicate a re-verification window applies — the in-memory counterpart of the store's
    /// `annotations_stale_since(cutoff)` read, kept here so the rule lives with the data.
    pub fn is_stale_since(&self, cutoff: i64) -> bool {
        self.last_verified < cutoff
    }

    /// The semantic class of this annotation's type.
    pub fn class(&self) -> AnnotationClass {
        classify(&self.r#type)
    }

    /// Whether this annotation is advisory (assumption / question).
    pub fn is_advisory(&self) -> bool {
        is_advisory(&self.r#type)
    }

    /// Whether this annotation is machine-derived (community / future derived types).
    pub fn is_system_derived(&self) -> bool {
        is_system_derived(&self.r#type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_known_and_custom() {
        assert_eq!(classify("note"), AnnotationClass::Note);
        assert_eq!(classify("assumption"), AnnotationClass::Assumption);
        assert_eq!(classify("observation"), AnnotationClass::Observation);
        assert_eq!(classify("comment"), AnnotationClass::Comment);
        assert_eq!(classify("question"), AnnotationClass::Question);
        assert_eq!(classify("community"), AnnotationClass::Community);
        assert_eq!(classify("adr-ref"), AnnotationClass::Custom);
        assert_eq!(classify(""), AnnotationClass::Custom);
        // Case-sensitive: "Note" is a distinct custom type from "note".
        assert_eq!(classify("Note"), AnnotationClass::Custom);
    }

    #[test]
    fn advisory_and_system_derived_predicates() {
        assert!(is_advisory("assumption"));
        assert!(is_advisory("question"));
        assert!(!is_advisory("note"));
        assert!(!is_advisory("custom-thing"));
        assert!(is_system_derived("community"));
        assert!(!is_system_derived("note"));
        assert!(!is_system_derived("assumption"));
    }

    #[test]
    fn known_set_all_classify_non_custom() {
        for &ty in KNOWN_ANNOTATION_TYPES {
            assert_ne!(
                classify(ty),
                AnnotationClass::Custom,
                "known type {ty:?} must not classify as Custom"
            );
        }
    }

    #[test]
    fn legacy_json_without_type_defaults_to_note() {
        // An Annotation serialized by an older build had no `type` field — NOR any of the
        // evidence-envelope fields (source_type / extraction_method / last_verified). All must
        // backfill to their safe defaults so old serialized data still deserializes.
        let legacy =
            r#"{"key":"k","value":"v","confidence":1.0,"provenance":"","author":"","ts":42}"#;
        let a: Annotation =
            serde_json::from_str(legacy).expect("legacy annotation must deserialize");
        assert_eq!(a.r#type, "note", "missing type must default to note");
        assert_eq!(a.key, "k");
        assert_eq!(a.ts, 42);
        // Evidence envelope backfills on OLD data (the backward-compat guarantee).
        assert_eq!(
            a.source_type, DEFAULT_SOURCE_TYPE,
            "missing source_type must default"
        );
        assert_eq!(
            a.extraction_method, DEFAULT_EXTRACTION_METHOD,
            "missing extraction_method must default"
        );
        assert_eq!(
            a.last_verified, 0,
            "missing last_verified must default to 0 (never verified)"
        );
    }

    #[test]
    fn legacy_json_with_type_but_no_evidence_envelope_defaults() {
        // A row from the *intermediate* build that had `type` but predates the evidence envelope.
        let legacy = r#"{"key":"k","value":"v","confidence":0.5,"provenance":"louvain:res=1.0","author":"system","ts":7,"type":"community"}"#;
        let a: Annotation = serde_json::from_str(legacy).expect("must deserialize");
        assert_eq!(a.r#type, "community");
        assert_eq!(a.provenance, "louvain:res=1.0");
        assert_eq!(a.source_type, DEFAULT_SOURCE_TYPE);
        assert_eq!(a.extraction_method, DEFAULT_EXTRACTION_METHOD);
        assert_eq!(a.last_verified, 0);
    }

    #[test]
    fn evidence_envelope_round_trips() {
        let a = Annotation::new("observation", "uses-tls", "endpoint requires TLS 1.3")
            .with_source_type("static-analysis")
            .with_extraction_method("scip-rust@0.3")
            .with_last_verified(1_700_000_000);
        let json = serde_json::to_string(&a).unwrap();
        // New fields serialize under their plain names (no rename).
        assert!(
            json.contains("\"source_type\":\"static-analysis\""),
            "got {json}"
        );
        assert!(
            json.contains("\"extraction_method\":\"scip-rust@0.3\""),
            "got {json}"
        );
        assert!(json.contains("\"last_verified\":1700000000"), "got {json}");
        let back: Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back, a,
            "evidence-envelope round-trip must preserve all fields"
        );
    }

    #[test]
    fn is_stale_since_freshness_predicate() {
        // Never verified (last_verified == 0) is stale for any positive cutoff.
        let never = Annotation::new("note", "k", "v");
        assert!(never.is_stale_since(1), "never-verified is stale");
        assert!(
            !never.is_stale_since(0),
            "cutoff 0 means 'verified before epoch' — nothing is stale at 0"
        );

        let verified = Annotation::new("note", "k", "v").with_last_verified(100);
        assert!(
            verified.is_stale_since(101),
            "verified before cutoff is stale"
        );
        assert!(
            !verified.is_stale_since(100),
            "verified exactly at cutoff is NOT stale (strict <)"
        );
        assert!(
            !verified.is_stale_since(99),
            "verified after cutoff is fresh"
        );
    }

    #[test]
    fn type_field_serializes_as_type() {
        let a = Annotation::new("assumption", "thread-safety", "assumed Send+Sync");
        let json = serde_json::to_string(&a).unwrap();
        assert!(
            json.contains("\"type\":\"assumption\""),
            "field must serialize under the JSON key `type`; got {json}"
        );
        let back: Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a, "round-trip must preserve all fields");
    }

    #[test]
    fn builders_and_helpers() {
        let a = Annotation::new("question", "k", "v")
            .with_confidence(0.5)
            .with_provenance("manual")
            .with_author("alice");
        assert_eq!(a.confidence, 0.5);
        assert_eq!(a.provenance, "manual");
        assert_eq!(a.author, "alice");
        assert!(a.is_advisory());
        assert!(!a.is_system_derived());
        assert_eq!(a.class(), AnnotationClass::Question);

        let n = Annotation::note("k", "v");
        assert_eq!(n.r#type, "note");
        assert_eq!(n.confidence, 1.0);
    }
}
