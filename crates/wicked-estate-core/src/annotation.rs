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

/// A single typed annotation attached to a symbol.
///
/// `r#type` is a plain string (no storage enum): a known convention or an arbitrary custom type.
/// It is the **last** field and `#[serde(default = …)]`, so an `Annotation` serialized by an older
/// build (without a `type`) still deserializes — defaulting to [`DEFAULT_ANNOTATION_TYPE`].
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
}

fn default_annotation_type() -> String {
    DEFAULT_ANNOTATION_TYPE.to_string()
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
        // An Annotation serialized by an older build had no `type` field.
        let legacy =
            r#"{"key":"k","value":"v","confidence":1.0,"provenance":"","author":"","ts":42}"#;
        let a: Annotation =
            serde_json::from_str(legacy).expect("legacy annotation must deserialize");
        assert_eq!(a.r#type, "note", "missing type must default to note");
        assert_eq!(a.key, "k");
        assert_eq!(a.ts, 42);
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
