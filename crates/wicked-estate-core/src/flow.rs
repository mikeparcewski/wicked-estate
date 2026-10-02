//! Semantic value-flow vocabulary: what a `flows_to` edge **claims**, and **how we know it**.
//!
//! `EdgeKind::Other("flows_to")` ([`edge_tags::FLOWS_TO`](crate::edge_tags::FLOWS_TO)) shipped in
//! wicked-estate#207 carrying a single scalar `metadata["construct"]`. That conflated two
//! independent questions, and a consumer could answer neither:
//!
//! 1. **Flow semantics** — does the producer's value become the consumer's value *whole*
//!    ([`FlowSemantics::ValuePreserving`]), or does it merely *contribute*
//!    ([`FlowSemantics::MayInfluence`])? `const c = a` and `const c = a + b` are not the same
//!    claim, and a caller reasoning about "where did this exact value come from" must not treat
//!    them alike.
//! 2. **Evidence origin** — was the fact read off the AST ([`FlowEvidence::Syntax`]), derived
//!    from a *resolved* call edge whose own confidence may be 0.5
//!    ([`FlowEvidence::CallDerived`]), or matched by a framework *naming convention* that the
//!    parser cannot prove ([`FlowEvidence::Convention`])?
//!
//! These are **orthogonal**: a convention match can be value-preserving, and a syntax-proven fact
//! can be may-influence. Evidence *strength* continues to live where the engine contract already
//! puts it — [`Edge::confidence`], [`Edge::provenance`], [`Edge::resolved_by`]. This module adds
//! the two missing *classifications*, plus the merge rule that keeps them from being silently
//! overwritten.
//!
//! # Why a set, not a scalar (the endpoint-dedup audit)
//!
//! An edge is identified by [`Edge::dedup_key`] = `(source, target, kind)`. Confidence,
//! provenance, location and metadata are **not** part of that key, and every store's
//! `upsert_edges` replaces the whole row when the incoming confidence is `>=` the stored one. Two
//! flow facts that share endpoints therefore collapse, last-writer-wins. This is reachable in
//! ordinary TypeScript — block-scoped shadowing inside one callable:
//!
//! ```text
//! function f(a: string, b: string) {
//!     const c = a + b;      // c -> a   may-influence,     at byte 62
//!     if (b) { const c = a; }  // c -> a   value-preserving,  at byte 116
//! }
//! ```
//!
//! Both locals resolve to one owner-scoped identity (`f:local:c`, `f:local:a`), so the two facts
//! share `(source, target, kind)`. Measured on the shipped code at `c4fa938`, exactly one survived
//! — `construct="assignment"` at byte 116 — and the may-influence contribution at byte 62 was
//! gone, with nothing recording that it had ever been asserted. A **scalar** classification cannot
//! represent the facts, so the vocabulary is set-valued and [`merge_flow_edges`] folds colliding
//! facts through a deterministic lattice *before* they reach a store.
//!
//! # The merge lattice
//!
//! For a group of edges sharing `(source, target, kind)`:
//!
//! | field | rule |
//! |---|---|
//! | [`FLOW_SEMANTICS_KEY`] | sorted set union — an edge supporting both claims says so |
//! | [`FLOW_EVIDENCE_KEY`] | sorted set union |
//! | [`FLOW_CONSTRUCTS_KEY`] | sorted set union |
//! | [`FLOW_RULES_KEY`] | sorted set union of stable rule ids |
//! | [`FLOW_SUPPORT_KEY`] | one deterministically-sorted entry per contributing fact, capped at [`MAX_FLOW_SUPPORT`] |
//! | `confidence` | **max** over contributions (agrees with every store's own `>=` upsert, so the merge is upsert-stable) |
//! | [`FLOW_CONFIDENCE_MIN_KEY`] | the **min**, written only when it differs — the weakest support is never hidden |
//! | `provenance` / `resolved_by` / `location` | taken from the representative fact: max confidence, tie-broken by the same total order the support list is sorted by |
//! | [`CONSTRUCT_KEY`] | the lexicographic minimum of [`FLOW_CONSTRUCTS_KEY`] — the pre-existing public scalar stays readable |
//!
//! Every rule is a commutative set operation or a total-order selection, so the result does not
//! depend on insertion order. There is no last-writer-wins step.
//!
//! # What this does NOT do
//!
//! [`merge_flow_edges`] folds the facts in **one emission batch**. It is not an occurrence table
//! and it has no retirement semantics: it cannot notice that a support disappeared in a later run,
//! and it cannot merge across two batches that reach the store separately. A cross-batch
//! equal-confidence `flows_to` collision would still be lossy. That is deliberately out of scope —
//! the authoritative, replaceable multi-support model is TS-S2A's seam. The reachable cross-batch
//! pairing (tree-sitter parsed facts vs. post-resolution call-derived facts) is audited as
//! endpoint-disjoint in `docs/ENGINE-CONTRACT.md` §3.2.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::edge::{Edge, EdgeKind};
use crate::edge_tags;
use crate::node::{Location, Metadata, Node};

/// `metadata["flow_semantics"]` — sorted array of [`FlowSemantics`] names.
pub const FLOW_SEMANTICS_KEY: &str = "flow_semantics";
/// `metadata["flow_evidence"]` — sorted array of [`FlowEvidence`] names.
pub const FLOW_EVIDENCE_KEY: &str = "flow_evidence";
/// `metadata["constructs"]` — sorted array of every construct supporting this edge.
pub const FLOW_CONSTRUCTS_KEY: &str = "constructs";
/// `metadata["flow_rules"]` — sorted array of stable rule ids (see [`flow_rule_id`]).
pub const FLOW_RULES_KEY: &str = "flow_rules";
/// `metadata["flow_support"]` — the per-fact evidence list, capped at [`MAX_FLOW_SUPPORT`].
pub const FLOW_SUPPORT_KEY: &str = "flow_support";
/// `metadata["flow_support_truncated"]` — how many support entries the cap dropped (R4).
pub const FLOW_SUPPORT_TRUNCATED_KEY: &str = "flow_support_truncated";
/// `metadata["flow_confidence_min"]` — the weakest contributing confidence, when it differs
/// from the edge's own (max) confidence.
pub const FLOW_CONFIDENCE_MIN_KEY: &str = "flow_confidence_min";
/// `metadata["construct"]` — the scalar key wicked-estate#207 shipped. Kept readable (it is the
/// lexicographic minimum of [`FLOW_CONSTRUCTS_KEY`]); [`FLOW_CONSTRUCTS_KEY`] is the complete set.
pub const CONSTRUCT_KEY: &str = "construct";

/// Cap on [`FLOW_SUPPORT_KEY`] entries per edge. Bounded output is agent-behaviour rule R4; an
/// unbounded support list on a hot endpoint is how a lineage answer blows a context budget.
pub const MAX_FLOW_SUPPORT: usize = 8;

/// What a flow edge claims about the value, independent of how sure we are that it happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FlowSemantics {
    /// The producer's value becomes the consumer's value, whole: `const c = a`, an argument
    /// binding a parameter, a `return`, a property read.
    ValuePreserving,
    /// The producer contributes to the consumer's value without determining it: an operand of
    /// `const c = a + b`. NOT a claim that the complete value is preserved.
    MayInfluence,
}

impl FlowSemantics {
    pub fn as_str(self) -> &'static str {
        match self {
            FlowSemantics::ValuePreserving => "value_preserving",
            FlowSemantics::MayInfluence => "may_influence",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "value_preserving" | "value" => Some(FlowSemantics::ValuePreserving),
            "may_influence" | "influence" => Some(FlowSemantics::MayInfluence),
            _ => None,
        }
    }
}

/// Where a flow fact came from. Distinct from [`Edge::provenance`], which classifies the
/// *resolution tier*: two facts can both be `Provenance::Parsed` while one is a direct syntax
/// fact and the other is a framework naming convention the parser cannot prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FlowEvidence {
    /// The AST proves it at this site: an assignment, a binary operand, a `return`.
    Syntax,
    /// Derived from a *resolved* `Calls` edge. Inherits that edge's confidence and provenance —
    /// uniqueness of the callee is not evidence about the call's resolution.
    CallDerived,
    /// A framework naming/shape convention matched in a query file. The syntax is real; the
    /// framework identity is **not** proven — an identifier named `Input` is not necessarily
    /// `@angular/core`'s `Input`, and a receiver named `route` is not necessarily an
    /// `ActivatedRoute`.
    Convention,
    /// RESERVED for TS-S2: a verified SCIP symbol/occurrence projection. Nothing emits this yet.
    Scip,
    /// RESERVED for TS-S3/TS-S4: a framework *compiler* fact (e.g. the Angular compiler's
    /// resolved template binding). Nothing emits this yet. Reserving the word is what keeps a
    /// convention match from later being relabelled as a compiler proof.
    Compiler,
}

impl FlowEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            FlowEvidence::Syntax => "syntax",
            FlowEvidence::CallDerived => "call_derived",
            FlowEvidence::Convention => "convention",
            FlowEvidence::Scip => "scip",
            FlowEvidence::Compiler => "compiler",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "syntax" => Some(FlowEvidence::Syntax),
            "call_derived" => Some(FlowEvidence::CallDerived),
            "convention" => Some(FlowEvidence::Convention),
            "scip" => Some(FlowEvidence::Scip),
            "compiler" => Some(FlowEvidence::Compiler),
            _ => None,
        }
    }

    /// Whether this class is actually emitted today. The reserved classes exist so the vocabulary
    /// is stable across TS-S2/TS-S3; emitting one before its wave lands would be a false claim.
    pub fn is_emitted(self) -> bool {
        matches!(
            self,
            FlowEvidence::Syntax | FlowEvidence::CallDerived | FlowEvidence::Convention
        )
    }
}

/// A stable identifier for the rule that asserted a flow fact: `<producer>/<evidence>/<construct>`,
/// e.g. `typescript/convention/angular_input`, `engine/call_derived/call_argument`. Derived from
/// data (the language name and the query-file capture), never from a hand-maintained registry, so
/// adding a language or a construct mints its rule id without a core change.
pub fn flow_rule_id(producer: &str, evidence: FlowEvidence, construct: &str) -> String {
    format!("{producer}/{}/{construct}", evidence.as_str())
}

/// One flow fact, before it is folded into an edge.
///
/// Forward-compatible (#231 review S2): the vocabulary grows with later waves, so outside this
/// crate a `match` needs a wildcard arm and a [`FlowFact`] is built with [`FlowFact::new`].
///
/// ```compile_fail
/// use wicked_estate_core::flow::FlowSemantics;
/// fn exhaustive(s: FlowSemantics) -> u8 {
///     match s {
///         FlowSemantics::ValuePreserving => 0,
///         FlowSemantics::MayInfluence => 1,
///     }
/// }
/// ```
///
/// ```compile_fail
/// use wicked_estate_core::flow::FlowEvidence;
/// fn exhaustive(e: FlowEvidence) -> u8 {
///     match e {
///         FlowEvidence::Syntax => 0,
///         FlowEvidence::CallDerived => 1,
///         FlowEvidence::Convention => 2,
///         FlowEvidence::Scip => 3,
///         FlowEvidence::Compiler => 4,
///     }
/// }
/// ```
///
/// ```compile_fail
/// use wicked_estate_core::flow::{FlowEvidence, FlowFact, FlowSemantics};
/// let _ = FlowFact {
///     semantics: FlowSemantics::ValuePreserving,
///     evidence: FlowEvidence::Syntax,
///     construct: "assignment".into(),
///     rule: "typescript/syntax/assignment".into(),
/// };
/// ```
///
/// The supported spellings compile:
///
/// ```
/// use wicked_estate_core::flow::{FlowEvidence, FlowFact, FlowSemantics};
/// let fact = FlowFact::new(FlowSemantics::ValuePreserving, FlowEvidence::Syntax, "assignment", "typescript");
/// let n = match fact.semantics { FlowSemantics::MayInfluence => 1, _ => 0 };
/// assert_eq!((n, fact.rule.as_str()), (0, "typescript/syntax/assignment"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowFact {
    pub semantics: FlowSemantics,
    pub evidence: FlowEvidence,
    pub construct: String,
    pub rule: String,
}

impl FlowFact {
    pub fn new(
        semantics: FlowSemantics,
        evidence: FlowEvidence,
        construct: impl Into<String>,
        producer: &str,
    ) -> Self {
        let construct = construct.into();
        let rule = flow_rule_id(producer, evidence, &construct);
        Self {
            semantics,
            evidence,
            construct,
            rule,
        }
    }

    /// Write this single fact's classification onto `edge`'s metadata. [`merge_flow_edges`]
    /// rewrites these keys when the edge turns out to collide with another fact.
    pub fn apply(&self, edge: &mut Edge) {
        write_sets(
            &mut edge.metadata,
            &BTreeSet::from([self.semantics]),
            &BTreeSet::from([self.evidence]),
            &BTreeSet::from([self.construct.clone()]),
            &BTreeSet::from([self.rule.clone()]),
        );
    }
}

fn write_sets(
    metadata: &mut Metadata,
    semantics: &BTreeSet<FlowSemantics>,
    evidence: &BTreeSet<FlowEvidence>,
    constructs: &BTreeSet<String>,
    rules: &BTreeSet<String>,
) {
    let strings = |items: Vec<String>| {
        serde_json::Value::Array(items.into_iter().map(serde_json::Value::String).collect())
    };
    metadata.insert(
        FLOW_SEMANTICS_KEY.to_string(),
        strings(semantics.iter().map(|s| s.as_str().to_string()).collect()),
    );
    metadata.insert(
        FLOW_EVIDENCE_KEY.to_string(),
        strings(evidence.iter().map(|e| e.as_str().to_string()).collect()),
    );
    metadata.insert(
        FLOW_CONSTRUCTS_KEY.to_string(),
        strings(constructs.iter().cloned().collect()),
    );
    metadata.insert(
        FLOW_RULES_KEY.to_string(),
        strings(rules.iter().cloned().collect()),
    );
    // The scalar wicked-estate#207 shipped stays readable and deterministic: the lexicographic
    // minimum of the set. It is a lossy summary BY CONSTRUCTION — `constructs` is the whole truth.
    if let Some(first) = constructs.iter().next() {
        metadata.insert(
            CONSTRUCT_KEY.to_string(),
            serde_json::Value::String(first.clone()),
        );
    }
}

/// The total order the support list — and therefore the representative-fact choice — is sorted by.
/// Every component is stable across runs; none of them is an insertion index.
/// One fact's identity and sort key: construct, semantics, evidence, rule, file, start byte, end
/// byte, resolved_by, and the confidence's f32 bits (monotonic for the non-negative range), so
/// two facts that differ only in confidence are two rows, not one row chosen by input order.
type SupportOrder = (
    String,
    String,
    String,
    String,
    String,
    u32,
    u32,
    String,
    u32,
);

fn support_order(edge: &Edge) -> SupportOrder {
    let get = |key: &str| {
        edge.metadata
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default()
    };
    let (file, start, end) = edge
        .location
        .as_ref()
        .map(|l: &Location| (l.file.clone(), l.span.start_byte, l.span.end_byte))
        .unwrap_or_default();
    (
        get(FLOW_CONSTRUCTS_KEY),
        get(FLOW_SEMANTICS_KEY),
        get(FLOW_EVIDENCE_KEY),
        get(FLOW_RULES_KEY),
        file,
        start,
        end,
        edge.resolved_by.clone(),
        edge.confidence.get().to_bits(),
    )
}

/// The support rows an edge contributes: its own already-recorded [`FLOW_SUPPORT_KEY`] entries if
/// it has been merged before, else one row synthesized from its single-fact metadata.
///
/// Reusing existing rows is what makes [`merge_flow_edges`] **idempotent and composable**: folding
/// an already-folded edge must not collapse its two-class `flow_semantics` array into whichever
/// value happens to sort first. A caller that merges per file and then again per run (TS-S2A's
/// likely shape) gets the same answer as one merge over everything.
///
/// **The cap is the boundary of that promise.** Up to [`MAX_FLOW_SUPPORT`] facts per edge, a
/// staged fold equals one fold exactly. Beyond it, the dropped facts' identities are gone: the
/// representative still composes (its row always survives the cap), but which non-representative
/// rows are kept can depend on batching, and [`FLOW_SUPPORT_TRUNCATED_KEY`] sums each fold's
/// drops, so merging overlapping pre-folded inputs can over-count. Read it as "at least this
/// many facts are not listed" only for a single fold.
fn support_rows(edge: &Edge) -> Vec<(SupportOrder, serde_json::Value)> {
    if let Some(existing) = edge
        .metadata
        .get(FLOW_SUPPORT_KEY)
        .and_then(|v| v.as_array())
    {
        if !existing.is_empty() {
            return existing
                .iter()
                .map(|entry| (support_entry_order(entry), entry.clone()))
                .collect();
        }
    }
    vec![(support_order(edge), support_entry(edge))]
}

/// The order an edge sorts by when choosing a group's representative: that of the fact it
/// represents. A fresh edge IS one fact ([`support_order`]). An already-merged edge carries
/// aggregate set metadata, so ordering it by that would let `merge(merge(X) ++ Y)` pick a
/// different representative than `merge(X ++ Y)`; its representative is the least-ordered of
/// its strongest support rows, exactly what the first fold chose. If a `MAX_FLOW_SUPPORT` cut
/// dropped every strongest row, fall back to the edge's own metadata.
fn representative_order(edge: &Edge) -> SupportOrder {
    let strongest = edge.confidence.get();
    edge.metadata
        .get(FLOW_SUPPORT_KEY)
        .and_then(|v| v.as_array())
        .and_then(|rows| {
            rows.iter()
                .filter(|row| {
                    row.get("confidence")
                        .and_then(|c| c.as_f64())
                        .is_some_and(|c| c as f32 == strongest)
                })
                .map(support_entry_order)
                .min()
        })
        .unwrap_or_else(|| support_order(edge))
}

/// The same total order as [`support_order`], read back off a stored support row.
fn support_entry_order(entry: &serde_json::Value) -> SupportOrder {
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let num = |key: &str| entry.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    (
        text("construct"),
        text("semantics"),
        text("evidence"),
        text("rule"),
        text("file"),
        num("start_byte"),
        num("end_byte"),
        text("resolved_by"),
        entry
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map_or(0, |c| (c as f32).to_bits()),
    )
}

fn support_entry(edge: &Edge) -> serde_json::Value {
    let mut entry = serde_json::Map::new();
    for key in [
        FLOW_CONSTRUCTS_KEY,
        FLOW_SEMANTICS_KEY,
        FLOW_EVIDENCE_KEY,
        FLOW_RULES_KEY,
    ] {
        if let Some(value) = edge.metadata.get(key) {
            // Singular names inside a support entry: one entry IS one fact.
            let singular = match key {
                FLOW_CONSTRUCTS_KEY => "construct",
                FLOW_SEMANTICS_KEY => "semantics",
                FLOW_EVIDENCE_KEY => "evidence",
                _ => "rule",
            };
            let flat = value
                .as_array()
                .and_then(|a| a.first().cloned())
                .unwrap_or_else(|| value.clone());
            entry.insert(singular.to_string(), flat);
        }
    }
    entry.insert(
        "confidence".to_string(),
        serde_json::json!(edge.confidence.get()),
    );
    entry.insert(
        "resolved_by".to_string(),
        serde_json::Value::String(edge.resolved_by.clone()),
    );
    if let Some(location) = &edge.location {
        entry.insert(
            "file".to_string(),
            serde_json::Value::String(location.file.clone()),
        );
        entry.insert(
            "line".to_string(),
            serde_json::json!(location.span.start_line),
        );
        entry.insert(
            "start_byte".to_string(),
            serde_json::json!(location.span.start_byte),
        );
        entry.insert(
            "end_byte".to_string(),
            serde_json::json!(location.span.end_byte),
        );
    }
    serde_json::Value::Object(entry)
}

/// Fold `flows_to` edges sharing `(source, target, kind)` into one non-lossy edge each, through the
/// lattice documented at the module level. Non-flow edges pass through untouched, in input order
/// relative to each other. Flow edges are emitted sorted by dedup key, so the output is a pure
/// function of the input *set*.
///
/// Call this on every batch of flow edges before handing them to a `GraphWrite` — the store's
/// `(source, target, kind)` upsert is `>=`, i.e. last-writer-wins at equal confidence, and that is
/// where the facts would otherwise be lost.
pub fn merge_flow_edges(edges: Vec<Edge>) -> Vec<Edge> {
    let flow_kind = edge_tags::other(edge_tags::FLOWS_TO);
    let mut passthrough = Vec::new();
    let mut groups: BTreeMap<(String, String, String), Vec<Edge>> = BTreeMap::new();
    for edge in edges {
        if edge.kind == flow_kind {
            groups.entry(edge.dedup_key()).or_default().push(edge);
        } else {
            passthrough.push(edge);
        }
    }

    let mut merged = Vec::with_capacity(groups.len());
    for (_, mut group) in groups {
        group.sort_by(|a, b| {
            // Max confidence first; then the stable total order. `total_cmp` keeps this a total
            // order even if a NaN ever reached a confidence field.
            b.confidence
                .get()
                .total_cmp(&a.confidence.get())
                .then_with(|| representative_order(a).cmp(&representative_order(b)))
        });

        let mut semantics = BTreeSet::new();
        let mut evidence = BTreeSet::new();
        let mut constructs = BTreeSet::new();
        let mut rules = BTreeSet::new();
        let mut supports: BTreeMap<SupportOrder, serde_json::Value> = BTreeMap::new();
        let mut min_confidence = f32::INFINITY;
        let mut already_dropped = 0usize;
        for edge in &group {
            let read = |key: &str| -> Vec<String> {
                edge.metadata
                    .get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            semantics.extend(
                read(FLOW_SEMANTICS_KEY)
                    .iter()
                    .filter_map(|s| FlowSemantics::parse(s)),
            );
            evidence.extend(
                read(FLOW_EVIDENCE_KEY)
                    .iter()
                    .filter_map(|s| FlowEvidence::parse(s)),
            );
            constructs.extend(read(FLOW_CONSTRUCTS_KEY));
            rules.extend(read(FLOW_RULES_KEY));
            // The minimum is over every contributing FACT, not over the folded edges: an
            // already-merged edge's own confidence is its strongest fact, so its weaker ones
            // live in its support rows and its prior `flow_confidence_min` (which also covers
            // rows a `MAX_FLOW_SUPPORT` cut dropped). Reading all three keeps a second fold
            // equal to one fold over everything.
            let rows = support_rows(edge);
            for (_, row) in &rows {
                if let Some(c) = row.get("confidence").and_then(|v| v.as_f64()) {
                    min_confidence = min_confidence.min(c as f32);
                }
            }
            if let Some(prior) = edge
                .metadata
                .get(FLOW_CONFIDENCE_MIN_KEY)
                .and_then(|v| v.as_f64())
            {
                min_confidence = min_confidence.min(prior as f32);
            }
            supports.extend(rows);
            min_confidence = min_confidence.min(edge.confidence.get());
            // A previously-truncated edge must not silently claim its dropped rows back.
            already_dropped += edge
                .metadata
                .get(FLOW_SUPPORT_TRUNCATED_KEY)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
        }

        // Representative: the max-confidence fact under the stable total order. Its provenance,
        // resolved_by and location ride the merged edge; every other fact's are preserved in the
        // support list, so nothing is discarded.
        let representative_key = representative_order(&group[0]);
        let mut edge = group.into_iter().next().expect("group is non-empty");
        write_sets(
            &mut edge.metadata,
            &semantics,
            &evidence,
            &constructs,
            &rules,
        );
        let total_support = supports.len();
        // Keep the first MAX_FLOW_SUPPORT rows in fact order, but never drop the representative's
        // own row: a later fold finds the representative through it, so losing it to the cap
        // would make the representative (location, provenance) depend on batching. It sorts
        // after every row it displaces, so the kept list stays in fact order.
        let mut kept: Vec<serde_json::Value> = Vec::with_capacity(MAX_FLOW_SUPPORT);
        let mut representative_row = None;
        for (i, (key, row)) in supports.into_iter().enumerate() {
            if i < MAX_FLOW_SUPPORT {
                kept.push(row);
            } else if key == representative_key {
                representative_row = Some(row);
            }
        }
        if let Some(row) = representative_row {
            kept.pop();
            kept.push(row);
        }
        let dropped = (total_support - kept.len()) + already_dropped;
        edge.metadata
            .insert(FLOW_SUPPORT_KEY.to_string(), serde_json::Value::Array(kept));
        if dropped > 0 {
            edge.metadata.insert(
                FLOW_SUPPORT_TRUNCATED_KEY.to_string(),
                serde_json::json!(dropped),
            );
        }
        // The representative is the max-confidence fact, so the edge's confidence IS the max.
        if min_confidence < edge.confidence.get() {
            edge.metadata.insert(
                FLOW_CONFIDENCE_MIN_KEY.to_string(),
                serde_json::json!(min_confidence),
            );
        } else {
            edge.metadata.remove(FLOW_CONFIDENCE_MIN_KEY);
        }
        merged.push(edge);
    }

    passthrough.extend(merged);
    passthrough
}

/// Read an edge's merged flow-semantics set.
pub fn flow_semantics_of(edge: &Edge) -> BTreeSet<FlowSemantics> {
    edge.metadata
        .get(FLOW_SEMANTICS_KEY)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().and_then(FlowSemantics::parse))
                .collect()
        })
        .unwrap_or_default()
}

/// Read an edge's merged evidence-class set.
pub fn flow_evidence_of(edge: &Edge) -> BTreeSet<FlowEvidence> {
    edge.metadata
        .get(FLOW_EVIDENCE_KEY)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().and_then(FlowEvidence::parse))
                .collect()
        })
        .unwrap_or_default()
}

/// True if `edge` is a semantic value-flow edge.
pub fn is_flow_edge(edge: &Edge) -> bool {
    edge.kind == EdgeKind::Other(edge_tags::FLOWS_TO.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// The eligibility predicate for structural / product surfaces
// ─────────────────────────────────────────────────────────────────────────────

/// **The** eligibility predicate: may `node` appear in an answer about *code structure*?
///
/// Synthetic value-flow slots ([`Node::is_value_flow_node`]) reuse ordinary [`NodeKind`]s and carry
/// the bare source identifier, so nothing except their `value_role` marker distinguishes a local
/// named `map` from the function `map`. Every surface that answers a structural question routes its
/// candidate set through here instead of re-deriving the rule; the published decision for every
/// consumer is the visibility matrix in `docs/ENGINE-CONTRACT.md` §3.3.
///
/// Surfaces that are deliberately **not** gated by this predicate — and why — are listed in that
/// matrix: raw graph export and `stats` (a faithful view of storage must stay faithful),
/// exact-`SymbolId` lookup (`RetrieveEntity`, `FetchContent`), explicit semantic traversal
/// (`Lineage relation=flows_to`, `SearchEntity include_values=true`), the PageRank *input* graph
/// (filtering it would renumber every real symbol's score, which is a measured contract change, not
/// a visibility correction), and `BlastRadius`/`TraverseGraph`, which follow all edge kinds by
/// locked contract.
///
/// [`NodeKind`]: crate::node::NodeKind
pub fn is_structural_symbol(node: &Node) -> bool {
    !node.is_value_flow_node()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::ResolutionTier;
    use crate::node::Span;
    use crate::symbol::SymbolId;

    fn flow_edge(
        construct: &str,
        semantics: FlowSemantics,
        byte: u32,
        tier: ResolutionTier,
    ) -> Edge {
        let mut edge = Edge::new(
            SymbolId("c".into()),
            SymbolId("a".into()),
            edge_tags::other(edge_tags::FLOWS_TO),
            tier,
            "tree-sitter",
        )
        .with_location(Location::new(
            "f.ts",
            Span {
                start_byte: byte,
                end_byte: byte + 1,
                ..Span::ZERO
            },
        ));
        FlowFact::new(semantics, FlowEvidence::Syntax, construct, "typescript").apply(&mut edge);
        edge
    }

    #[test]
    fn colliding_facts_merge_without_losing_either_class() {
        let influence = flow_edge(
            "expression",
            FlowSemantics::MayInfluence,
            62,
            ResolutionTier::Parsed,
        );
        let preserving = flow_edge(
            "assignment",
            FlowSemantics::ValuePreserving,
            116,
            ResolutionTier::Parsed,
        );

        let merged = merge_flow_edges(vec![influence.clone(), preserving.clone()]);
        assert_eq!(
            merged.len(),
            1,
            "same endpoints + kind must fold to one edge"
        );
        let semantics = flow_semantics_of(&merged[0]);
        assert!(
            semantics.contains(&FlowSemantics::MayInfluence)
                && semantics.contains(&FlowSemantics::ValuePreserving),
            "both classes must survive: {:?}",
            merged[0].metadata
        );
        assert_eq!(
            merged[0].metadata[FLOW_SUPPORT_KEY]
                .as_array()
                .unwrap()
                .len(),
            2,
            "both sites must stay explainable"
        );
    }

    #[test]
    fn merge_is_insertion_order_independent() {
        let a = flow_edge(
            "expression",
            FlowSemantics::MayInfluence,
            62,
            ResolutionTier::Parsed,
        );
        let b = flow_edge(
            "assignment",
            FlowSemantics::ValuePreserving,
            116,
            ResolutionTier::Parsed,
        );
        let forward = merge_flow_edges(vec![a.clone(), b.clone()]);
        let backward = merge_flow_edges(vec![b, a]);
        assert_eq!(forward, backward, "the lattice must be commutative");
    }

    #[test]
    fn weakest_support_is_recorded_not_hidden() {
        let strong = flow_edge(
            "assignment",
            FlowSemantics::ValuePreserving,
            10,
            ResolutionTier::Parsed,
        );
        let weak = flow_edge(
            "expression",
            FlowSemantics::MayInfluence,
            20,
            ResolutionTier::Heuristic,
        );
        let merged = merge_flow_edges(vec![weak, strong]);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].confidence.get() - 1.0).abs() < f32::EPSILON);
        assert!(
            (merged[0].metadata[FLOW_CONFIDENCE_MIN_KEY]
                .as_f64()
                .unwrap()
                - 0.5)
                .abs()
                < 1e-6,
            "the 0.5 support must stay visible: {:?}",
            merged[0].metadata
        );
    }

    #[test]
    fn legacy_construct_scalar_stays_readable_and_deterministic() {
        let merged = merge_flow_edges(vec![
            flow_edge(
                "expression",
                FlowSemantics::MayInfluence,
                62,
                ResolutionTier::Parsed,
            ),
            flow_edge(
                "assignment",
                FlowSemantics::ValuePreserving,
                116,
                ResolutionTier::Parsed,
            ),
        ]);
        assert_eq!(merged[0].metadata[CONSTRUCT_KEY], "assignment");
        assert_eq!(
            merged[0].metadata[FLOW_CONSTRUCTS_KEY],
            serde_json::json!(["assignment", "expression"])
        );
    }

    #[test]
    fn reserved_evidence_classes_are_not_emitted_yet() {
        for reserved in [FlowEvidence::Scip, FlowEvidence::Compiler] {
            assert!(
                !reserved.is_emitted(),
                "{} is reserved for a later wave",
                reserved.as_str()
            );
        }
        for live in [
            FlowEvidence::Syntax,
            FlowEvidence::CallDerived,
            FlowEvidence::Convention,
        ] {
            assert!(live.is_emitted());
        }
    }

    #[test]
    fn rule_ids_are_derived_from_data() {
        assert_eq!(
            flow_rule_id("typescript", FlowEvidence::Convention, "angular_input"),
            "typescript/convention/angular_input"
        );
        assert_eq!(
            flow_rule_id("engine", FlowEvidence::CallDerived, "call_argument"),
            "engine/call_derived/call_argument"
        );
    }

    /// The seam is public and composable: a caller that folds per file and again per run must
    /// get the same answer as one fold over everything. Without reusing the recorded support
    /// rows, a second pass would flatten a two-class `flow_semantics` array to whichever value
    /// sorts first — re-introducing exactly the loss this module exists to prevent.
    #[test]
    fn merging_an_already_merged_edge_is_idempotent() {
        let once = merge_flow_edges(vec![
            flow_edge(
                "expression",
                FlowSemantics::MayInfluence,
                62,
                ResolutionTier::Parsed,
            ),
            flow_edge(
                "assignment",
                FlowSemantics::ValuePreserving,
                116,
                ResolutionTier::Parsed,
            ),
        ]);
        let twice = merge_flow_edges(once.clone());
        assert_eq!(once, twice);
        assert_eq!(
            flow_semantics_of(&twice[0]),
            BTreeSet::from([FlowSemantics::MayInfluence, FlowSemantics::ValuePreserving])
        );
    }

    /// `flow_confidence_min` composes too (#231 review S1): it is the minimum over every
    /// contributing FACT, so a second fold must reproduce the one-pass value. Before the fix it
    /// was the minimum over the folded EDGES' confidences, so `merge(merge(strong, weak), mid)`
    /// reported 0.6 instead of 0.5, and `merge(merge(strong, weak), strong2)` erased the key.
    #[test]
    fn flow_confidence_min_composes_across_two_folds() {
        let strong = || {
            flow_edge(
                "assignment",
                FlowSemantics::ValuePreserving,
                10,
                ResolutionTier::Parsed,
            )
        };
        let weak = || {
            flow_edge(
                "expression",
                FlowSemantics::MayInfluence,
                20,
                ResolutionTier::Heuristic,
            )
        };
        let mid = || {
            flow_edge(
                "call_argument",
                FlowSemantics::ValuePreserving,
                30,
                ResolutionTier::ImportMap,
            )
        };
        // Sorts ahead of `strong` (byte 5 < 10), so it becomes the second fold's representative.
        let strong2 = || {
            flow_edge(
                "property",
                FlowSemantics::ValuePreserving,
                5,
                ResolutionTier::Parsed,
            )
        };
        let min_of = |e: &Edge| {
            e.metadata
                .get(FLOW_CONFIDENCE_MIN_KEY)
                .and_then(|v| v.as_f64())
        };

        for third in [mid as fn() -> Edge, strong2] {
            let one_pass = merge_flow_edges(vec![strong(), weak(), third()]);
            let first = merge_flow_edges(vec![strong(), weak()]);
            let two_pass = merge_flow_edges(first.into_iter().chain([third()]).collect());
            assert_eq!(one_pass.len(), 1);
            assert_eq!(two_pass.len(), 1);
            let (a, b) = (min_of(&one_pass[0]), min_of(&two_pass[0]));
            assert!(
                a.is_some_and(|m| (m - 0.5).abs() < 1e-6),
                "one pass must report the 0.5 fact: {:?}",
                one_pass[0].metadata
            );
            assert_eq!(a, b, "two folds must equal one: {:?}", two_pass[0].metadata);
        }
    }

    /// The representative composes too (codex review of #231): an already-merged edge must sort
    /// as its representative FACT, not as its aggregate set metadata. Otherwise
    /// `merge(merge(X) ++ Y)` can pick a different representative — and so a different
    /// location — than `merge(X ++ Y)`.
    #[test]
    fn representative_composes_across_two_folds() {
        let a = || {
            flow_edge(
                "assignment",
                FlowSemantics::ValuePreserving,
                10,
                ResolutionTier::Parsed,
            )
        };
        let b = || {
            flow_edge(
                "assignment",
                FlowSemantics::MayInfluence,
                20,
                ResolutionTier::Parsed,
            )
        };
        let c = || {
            flow_edge(
                "assignment",
                FlowSemantics::ValuePreserving,
                30,
                ResolutionTier::Parsed,
            )
        };
        let one_pass = merge_flow_edges(vec![a(), b(), c()]);
        let two_pass = merge_flow_edges(
            merge_flow_edges(vec![a(), b()])
                .into_iter()
                .chain([c()])
                .collect(),
        );
        assert_eq!(one_pass, two_pass, "two folds must equal one");
    }

    /// Copilot review of #231: two facts identical except for confidence shared one support
    /// key, so whichever was inserted last overwrote the other — an insertion-order-dependent
    /// `flow_support`.
    #[test]
    fn support_rows_that_differ_only_in_confidence_are_both_kept() {
        let strong = flow_edge(
            "assignment",
            FlowSemantics::ValuePreserving,
            10,
            ResolutionTier::Parsed,
        );
        let weak = flow_edge(
            "assignment",
            FlowSemantics::ValuePreserving,
            10,
            ResolutionTier::Heuristic,
        );
        let forward = merge_flow_edges(vec![strong.clone(), weak.clone()]);
        let backward = merge_flow_edges(vec![weak, strong]);
        assert_eq!(
            forward, backward,
            "the support list must not depend on input order"
        );
        assert_eq!(
            forward[0].metadata[FLOW_SUPPORT_KEY]
                .as_array()
                .unwrap()
                .len(),
            2,
            "{:?}",
            forward[0].metadata
        );
    }

    /// Copilot review of #231: when the support cap dropped every strongest row, a second fold
    /// fell back to the aggregate metadata and could pick a different representative. The
    /// representative's own row now always survives the cap.
    #[test]
    fn representative_row_survives_the_support_cap_across_folds() {
        let weak = |byte: u32| {
            flow_edge(
                "assignment",
                FlowSemantics::ValuePreserving,
                byte,
                ResolutionTier::Heuristic,
            )
        };
        let strong = flow_edge(
            "property_read",
            FlowSemantics::ValuePreserving,
            500,
            ResolutionTier::Parsed,
        );
        let stronger_later = flow_edge(
            "expression",
            FlowSemantics::MayInfluence,
            600,
            ResolutionTier::Parsed,
        );
        let x: Vec<Edge> = (0..MAX_FLOW_SUPPORT as u32)
            .map(|i| weak(10 + i))
            .chain([strong])
            .collect();
        let first = merge_flow_edges(x.clone());
        assert_eq!(
            merge_flow_edges(first.clone()),
            first,
            "re-folding a capped edge must be idempotent, truncation count included"
        );
        assert_eq!(
            first[0].metadata[FLOW_SUPPORT_TRUNCATED_KEY],
            serde_json::json!(1)
        );
        assert!(
            first[0].metadata[FLOW_SUPPORT_KEY]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["construct"] == "property_read"),
            "the representative's row must be kept: {:?}",
            first[0].metadata
        );
        let one_pass = merge_flow_edges(x.into_iter().chain([stronger_later.clone()]).collect());
        let two_pass = merge_flow_edges(first.into_iter().chain([stronger_later]).collect());
        assert_eq!(
            one_pass[0].location, two_pass[0].location,
            "the representative must not depend on batching"
        );
    }

    #[test]
    fn non_flow_edges_pass_through_untouched() {
        let calls = Edge::new(
            SymbolId("a".into()),
            SymbolId("b".into()),
            EdgeKind::Calls,
            ResolutionTier::ImportMap,
            "name-resolver",
        );
        let merged = merge_flow_edges(vec![calls.clone()]);
        assert_eq!(merged, vec![calls]);
    }
}
