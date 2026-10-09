//! Authoritative, replaceable edge **support** (TS-S2A).
//!
//! A public edge is keyed `(source, target, kind)` ([`Edge::dedup_key`]). Until TS-S2A the only way
//! to put one in a store was [`GraphWrite::upsert_edges`](crate::traits::GraphWrite::upsert_edges):
//! `>=`-confidence, last-writer-wins on a tie, retired only by `remove_file`. That is the **base
//! plane**, and it has no notion of *who* asserted a fact, so a producer that re-runs and asserts
//! fewer facts cannot retract the ones it stopped asserting. [`merge_flow_edges`]'s `flow_support`
//! list cannot fill that gap either: it is a *bounded display sample* (capped at
//! [`MAX_FLOW_SUPPORT`](crate::flow::MAX_FLOW_SUPPORT)), so a fact the cap evicted is gone from it.
//!
//! This module defines the **support plane**: a producer-owned, replaceable set of facts that sits
//! beside the base plane and is folded into the public edge.
//!
//! # The contract
//!
//! * **Ownership.** Every support fact is owned by exactly one [`SupportOwner`] —
//!   `(producer, snapshot)`, two opaque non-empty strings. `producer` names the asserting system
//!   (`scip-typescript`, `angular-compiler`); `snapshot` names the unit that producer re-emits
//!   whole (a SCIP index for one project root, one compilation unit). Owners are independent: no
//!   operation on one owner modifies another owner's facts or generation (re-projection reads
//!   every owner's facts, by design).
//! * **Replacement.** [`GraphWrite::replace_edge_supports`](crate::traits::GraphWrite::replace_edge_supports)
//!   `(owner, generation, facts)` makes `facts` the owner's **complete** support set. Every fact
//!   the owner held before and does not assert now is retracted; an empty `facts` retracts all of
//!   them. There is no occurrence-by-occurrence delete.
//! * **Fact identity is producer-owned and opaque.** A fact is a [`SupportFact`]: an [`Edge`] plus
//!   a `fact_id` the producer chose (a SCIP occurrence, a compiler diagnostic id, a row key). Its
//!   identity within the owner is `(dedup_key, fact_id)`. The storage layer compares `fact_id`
//!   byte-for-byte and never parses, trims, case-folds or Unicode-normalizes it — nor any symbol id
//!   — so two languages or toolchains whose display names coincide never collide when their ids
//!   differ, and a producer's id scheme needs no core change. A producer with no ids of its own
//!   uses [`SupportFact::from_edge`], whose id is the fact's canonical content ([`fact_key`]).
//!   The same `fact_id` re-asserted with different content is a *changed* fact (retracted and
//!   asserted); two different contents under one id in one submission are rejected. The input is
//!   a *set*: its order and duplicates never affect what is stored or shown.
//! * **Generations.** Per owner, `generation` is a `u64` in `0..=i64::MAX` (every backend can store
//!   it exactly) that must not go backwards:
//!   * no stored generation, or `generation >` stored → the replacement applies;
//!   * `generation ==` stored and the same fact set (same ids, same content) → **idempotent
//!     replay**: nothing is written,
//!     [`SupportReplacement::replayed`] is `true`;
//!   * `generation ==` stored and a different set → rejected (`Error::Invalid`, "generation
//!     conflict"); nothing is written;
//!   * `generation <` stored → rejected (`Error::Invalid`, "stale generation"); nothing is written.
//!
//!   An empty replacement keeps the owner's generation, so a stale replay cannot resurrect what it
//!   retracted.
//! * **Atomicity.** A replacement is all-or-nothing on every backend: the owner's facts, its
//!   generation and every affected public edge move together, or none of them do. A failed
//!   replacement leaves no half-old/half-new generation, inside or outside an open batch.
//! * **Projection.** The public edge for a key with any support is a pure function of
//!   `(base, every owner's facts)` — [`project_edge`] — recomputed from those authoritative rows on
//!   every change, never from the previously projected edge. Its bounded `flow_support` sample is
//!   therefore *explanatory only*: evicting a row from it cannot change support identity, a later
//!   projection, or a later representative.
//! * **The base plane coexists.** When a key gains its first support, the edge the base plane had
//!   stored there is kept aside as the key's *base contribution*; `upsert_edges` on a supported
//!   key updates that base contribution (same `>=` / evidence rule as ever) and re-projects;
//!   `remove_file` and `prune_dangling_edges` retire the base contribution by exactly the rule they
//!   apply to edges, then re-project. A projected edge's owning file is its base contribution's
//!   (or none) — never a support fact's site. When the last support of a key is retracted, the base
//!   contribution becomes the plain public edge again, byte-for-byte. A store holding no support
//!   at all behaves exactly as before TS-S2A.
//! * **Support is producer-owned, not file-owned.** `remove_file` and `prune_dangling_edges` never
//!   delete a support fact. A supported edge whose endpoint node is absent stays visible until its
//!   owner retracts it — the producer asserted it, and only the producer may retract it. The one
//!   exception is erasure (`remove_nodes`), which deletes every support fact incident to an erased
//!   symbol; a producer replaying the same generation afterwards gets a generation conflict.
//!
//! # What is exact, and where exactness stops
//!
//! For a key whose base contribution is absent or a single fact, and whose facts are single
//! (un-merged) edges, `flow_support.len() + flow_support_truncated` equals the number of distinct
//! support ROWS exactly: the projection is one fold over the authoritative set. A row is keyed by
//! TS-S1's support order (construct, semantics, evidence, rule, file, bytes, resolved_by,
//! confidence), so two facts that differ only outside it — line/column, provenance,
//! `evidence_count`, extra metadata — are two facts but one row. Count facts with `edge_supports`. A *pre-merged*
//! input (an edge that already carries `flow_support`, e.g. the base plane's own
//! `merge_flow_edges` output) contributes its recorded rows and its recorded truncation, which
//! [`merge_flow_edges`] sums — so past the cap that count is "at least", exactly as documented in
//! `docs/ENGINE-CONTRACT.md` §3.2. Authoritative identity is never derived from the sample: read it
//! with [`GraphRead::edge_supports`](crate::traits::GraphRead::edge_supports).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::edge::Edge;
use crate::error::{Error, Result};
use crate::flow::{is_flow_edge, merge_flow_edges};

/// The public edge key: `(source, target, kind)` exactly as [`Edge::dedup_key`] spells it.
pub type EdgeKey = (String, String, String);

/// The largest generation every backend stores exactly (SQLite `INTEGER` and Postgres `BIGINT` are
/// signed 64-bit).
pub const MAX_SUPPORT_GENERATION: u64 = i64::MAX as u64;

/// An opaque identity string (owner part or `fact_id`) every backend stores exactly: non-empty and
/// free of NUL, which Postgres `TEXT` cannot hold. Nothing else is checked or rewritten.
pub(crate) fn check_opaque(what: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        return Err(Error::Invalid(format!(
            "{what} must be non-empty and contain no NUL byte, got {value:?}"
        )));
    }
    Ok(())
}

/// Who owns a set of support facts: the asserting `producer` and the `snapshot` it re-emits whole.
///
/// Built with [`SupportOwner::new`], which rejects an empty part — an anonymous owner could never
/// be replaced deliberately.
///
/// ```compile_fail
/// use wicked_estate_core::support::SupportOwner;
/// let _ = SupportOwner { producer: "scip".into(), snapshot: "web".into() };
/// ```
///
/// ```
/// use wicked_estate_core::support::SupportOwner;
/// let owner = SupportOwner::new("scip-typescript", "apps/web").unwrap();
/// assert_eq!((owner.producer.as_str(), owner.snapshot.as_str()), ("scip-typescript", "apps/web"));
/// assert!(SupportOwner::new("", "apps/web").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SupportOwner {
    pub producer: String,
    pub snapshot: String,
}

impl SupportOwner {
    pub fn new(producer: impl Into<String>, snapshot: impl Into<String>) -> Result<Self> {
        let owner = Self {
            producer: producer.into(),
            snapshot: snapshot.into(),
        };
        owner.validate()?;
        Ok(owner)
    }

    /// Owners reach a store through this type, but a store re-checks: a deserialized owner never
    /// passed through [`SupportOwner::new`].
    pub fn validate(&self) -> Result<()> {
        if self.producer.trim().is_empty() || self.snapshot.trim().is_empty() {
            return Err(Error::Invalid(format!(
                "support owner needs a non-empty producer and snapshot, got ({:?}, {:?})",
                self.producer, self.snapshot
            )));
        }
        check_opaque("support owner producer", &self.producer)?;
        check_opaque("support owner snapshot", &self.snapshot)
    }
}

/// One authoritative support row, as [`GraphRead::edge_supports`](crate::traits::GraphRead::edge_supports)
/// returns it, ordered by `(owner.producer, owner.snapshot, fact_id)`. `generation` is the owner's
/// current generation: a replacement rewrites the owner's whole set, so every fact it holds
/// belongs to that generation. `fact_id` is returned exactly as the producer submitted it.
///
/// The read types are `#[non_exhaustive]`: outside this crate they are built with `new`, never a
/// struct literal, so later fields are additive.
///
/// ```compile_fail
/// use wicked_estate_core::support::{EdgeSupport, SupportOwner};
/// fn forge(owner: SupportOwner, fact: wicked_estate_core::Edge) -> EdgeSupport {
///     EdgeSupport { owner, generation: 1, fact_id: "x".into(), fact }
/// }
/// ```
///
/// ```compile_fail
/// use wicked_estate_core::support::{SupportOwner, SupportOwnerState};
/// fn forge(owner: SupportOwner) -> SupportOwnerState { SupportOwnerState { owner, generation: 1 } }
/// ```
///
/// ```compile_fail
/// use wicked_estate_core::support::{SupportOwner, SupportReplacement};
/// fn forge(owner: SupportOwner) -> SupportReplacement {
///     SupportReplacement { owner, generation: 1, replayed: false, asserted: 0, retained: 0,
///                          retracted: 0, edges_touched: 0 }
/// }
/// ```
///
/// ```
/// use wicked_estate_core::support::{EdgeSupport, SupportOwner, SupportOwnerState};
/// use wicked_estate_core::{Edge, EdgeKind, ResolutionTier, SymbolId};
/// let owner = SupportOwner::new("scip-java", "acme").unwrap();
/// let e = Edge::new(SymbolId("a".into()), SymbolId("b".into()), EdgeKind::Calls,
///                   ResolutionTier::Scip, "scip-java");
/// let row = EdgeSupport::new(owner.clone(), 2, "occ:1", e);
/// assert_eq!((row.generation, row.fact_id.as_str()), (2, "occ:1"));
/// assert_eq!(SupportOwnerState::new(owner, 2).generation, 2);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EdgeSupport {
    pub owner: SupportOwner,
    pub generation: u64,
    pub fact_id: String,
    pub fact: Edge,
}

impl EdgeSupport {
    pub fn new(
        owner: SupportOwner,
        generation: u64,
        fact_id: impl Into<String>,
        fact: Edge,
    ) -> Self {
        Self {
            owner,
            generation,
            fact_id: fact_id.into(),
            fact,
        }
    }
}

/// One owner and its last applied generation, as
/// [`GraphRead::support_owners`](crate::traits::GraphRead::support_owners) returns it, ordered by
/// `(producer, snapshot)`. Kept after an empty replacement, so an owner with no facts is listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SupportOwnerState {
    pub owner: SupportOwner,
    pub generation: u64,
}

impl SupportOwnerState {
    pub fn new(owner: SupportOwner, generation: u64) -> Self {
        Self { owner, generation }
    }
}

/// What a [`replace_edge_supports`](crate::traits::GraphWrite::replace_edge_supports) call did.
/// The counts are over distinct facts (`asserted + retained` is the owner's new set size;
/// `retained + retracted` its old one); a fact whose `fact_id` survived with different content
/// counts once in `retracted` and once in `asserted`. `edges_touched` counts the public edge keys
/// re-projected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SupportReplacement {
    pub owner: SupportOwner,
    pub generation: u64,
    /// `true` for an idempotent replay of the stored generation: nothing was written.
    pub replayed: bool,
    pub asserted: usize,
    pub retained: usize,
    pub retracted: usize,
    pub edges_touched: usize,
}

/// One support fact as a producer submits it: an opaque, producer-owned `fact_id` and the [`Edge`]
/// it asserts. Validated on construction; built only through [`SupportFact::new`] or
/// [`SupportFact::from_edge`].
///
/// ```
/// use wicked_estate_core::support::SupportFact;
/// use wicked_estate_core::{Edge, EdgeKind, ResolutionTier, SymbolId};
/// let e = Edge::new(SymbolId("a".into()), SymbolId("b".into()), EdgeKind::Calls,
///                   ResolutionTier::Scip, "scip-java");
/// let f = SupportFact::new("scip-java occurrence 17", e.clone()).unwrap();
/// assert_eq!(f.fact_id, "scip-java occurrence 17"); // stored and returned exactly
/// assert!(SupportFact::new("", e).is_err());
/// ```
///
/// ```compile_fail
/// use wicked_estate_core::support::SupportFact;
/// fn forge(e: wicked_estate_core::Edge) -> SupportFact {
///     SupportFact { fact_id: "x".into(), key: e.dedup_key(), content: String::new(), edge: e }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SupportFact {
    /// Producer-owned, opaque, compared byte-for-byte.
    pub fact_id: String,
    pub key: EdgeKey,
    /// [`fact_key`] of `edge`: decides "retained" vs. "changed" for a re-asserted `fact_id`.
    pub content: String,
    pub edge: Edge,
}

impl SupportFact {
    /// Validate one fact under a producer-chosen id. Every support fact must carry what every edge
    /// must (`{confidence, provenance, resolved_by}`): a finite confidence in `[0, 1]`, a
    /// non-empty `resolved_by`, and non-empty endpoints. `fact_id` must be non-empty and NUL-free;
    /// it is not otherwise inspected.
    pub fn new(fact_id: impl Into<String>, edge: Edge) -> Result<Self> {
        let fact_id = fact_id.into();
        check_opaque("support fact_id", &fact_id)?;
        let c = edge.confidence.get();
        if !c.is_finite() || !(0.0..=1.0).contains(&c) {
            return Err(Error::Invalid(format!(
                "support fact {fact_id:?} has confidence {c}, outside [0, 1]"
            )));
        }
        if edge.resolved_by.trim().is_empty() {
            return Err(Error::Invalid(format!(
                "support fact {fact_id:?} has no resolved_by — every edge carries its provenance"
            )));
        }
        if edge.source.0.is_empty() || edge.target.0.is_empty() {
            return Err(Error::Invalid(format!(
                "support fact {fact_id:?} has an empty endpoint"
            )));
        }
        Ok(Self {
            key: edge.dedup_key(),
            content: fact_key(&edge),
            fact_id,
            edge,
        })
    }

    /// A fact whose id is its own canonical content — for a producer with no stable ids. Two
    /// such facts are the same fact exactly when their edges are identical.
    pub fn from_edge(edge: Edge) -> Result<Self> {
        let id = fact_key(&edge);
        Self::new(id, edge)
    }

    /// The same edge under another producer id (re-validated).
    pub fn with_id(self, fact_id: impl Into<String>) -> Result<Self> {
        Self::new(fact_id, self.edge)
    }

    /// The fact's identity within its owner.
    pub fn id(&self) -> (EdgeKey, String) {
        (self.key.clone(), self.fact_id.clone())
    }
}

/// The canonical CONTENT of a fact: its JSON with every object's keys sorted. Independent of
/// serde_json's map ordering features and of the order metadata was inserted in.
pub fn fact_key(edge: &Edge) -> String {
    let value = serde_json::to_value(edge).unwrap_or(serde_json::Value::Null);
    let mut out = String::new();
    write_canonical(&value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// De-duplicate a submitted fact list. The result is sorted by `(key, fact_id)`, so it is a pure
/// function of the input *set*. One `fact_id` on one key carrying two different contents is
/// rejected: the producer asserted two things under one name.
pub fn normalize_facts(facts: &[SupportFact]) -> Result<Vec<SupportFact>> {
    let mut by_id: BTreeMap<(EdgeKey, String), SupportFact> = BTreeMap::new();
    for submitted in facts {
        // The fields are public, so a fact may have been edited after construction: re-validate
        // and re-derive its key and content from `(fact_id, edge)` instead of trusting them.
        let fact = SupportFact::new(submitted.fact_id.clone(), submitted.edge.clone())?;
        match by_id.get(&fact.id()) {
            Some(seen) if seen.content != fact.content => {
                return Err(Error::Invalid(format!(
                    "support fact_id {:?} is asserted twice with different content on {:?}",
                    fact.fact_id, fact.key
                )));
            }
            Some(_) => {}
            None => {
                by_id.insert(fact.id(), fact);
            }
        }
    }
    Ok(by_id.into_values().collect())
}

/// What a backend must do to apply one replacement. Computed by [`plan_replacement`] from the
/// owner's stored state so every backend applies the same generation and idempotence rules.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ReplacementPlan {
    pub report: SupportReplacement,
    /// Facts to insert (new or changed in this generation), sorted by identity.
    pub insert: Vec<SupportFact>,
    /// Fact identities `(key, fact_id)` to delete (not re-asserted, or changed), sorted.
    pub delete: Vec<(EdgeKey, String)>,
    /// Public edge keys whose projection changes, sorted. Empty for a replay.
    pub touched: BTreeSet<EdgeKey>,
}

/// The owner's stored facts as a plan reads them: `(key, fact_id)` → content ([`fact_key`]).
pub type StoredFacts = BTreeMap<(EdgeKey, String), String>;

/// Decide a replacement. `stored_generation` / `stored` are the owner's current generation and
/// facts; `incoming` is [`normalize_facts`] output. Errors leave the caller with nothing to undo:
/// no backend may write before this returns `Ok`.
pub fn plan_replacement(
    owner: &SupportOwner,
    generation: u64,
    stored_generation: Option<u64>,
    stored: &StoredFacts,
    incoming: Vec<SupportFact>,
) -> Result<ReplacementPlan> {
    owner.validate()?;
    if generation > MAX_SUPPORT_GENERATION {
        return Err(Error::Invalid(format!(
            "support generation {generation} exceeds {MAX_SUPPORT_GENERATION}"
        )));
    }
    let incoming_map: StoredFacts = incoming
        .iter()
        .map(|f| (f.id(), f.content.clone()))
        .collect();
    if let Some(current) = stored_generation {
        if generation < current {
            return Err(Error::Invalid(format!(
                "stale generation: {owner:?} is at generation {current}, refusing {generation}"
            )));
        }
        if generation == current {
            if &incoming_map != stored {
                return Err(Error::Invalid(format!(
                    "generation conflict: {owner:?} generation {generation} was already applied \
                     with a different fact set; a new set needs a new generation"
                )));
            }
            return Ok(ReplacementPlan {
                report: SupportReplacement {
                    owner: owner.clone(),
                    generation,
                    replayed: true,
                    asserted: 0,
                    retained: stored.len(),
                    retracted: 0,
                    edges_touched: 0,
                },
                insert: Vec::new(),
                delete: Vec::new(),
                touched: BTreeSet::new(),
            });
        }
    }
    let delete: Vec<(EdgeKey, String)> = stored
        .iter()
        .filter(|(id, content)| incoming_map.get(*id) != Some(*content))
        .map(|(id, _)| id.clone())
        .collect();
    let insert: Vec<SupportFact> = incoming
        .into_iter()
        .filter(|f| stored.get(&f.id()) != Some(&f.content))
        .collect();
    let touched: BTreeSet<EdgeKey> = delete
        .iter()
        .map(|(k, _)| k.clone())
        .chain(insert.iter().map(|f| f.key.clone()))
        .collect();
    Ok(ReplacementPlan {
        report: SupportReplacement {
            owner: owner.clone(),
            generation,
            replayed: false,
            asserted: insert.len(),
            retained: incoming_map.len() - insert.len(),
            retracted: delete.len(),
            edges_touched: touched.len(),
        },
        insert,
        delete,
        touched,
    })
}

/// The base plane's collision rule, shared so a base contribution kept aside for a supported key
/// evolves exactly as the stored edge would have: the incoming edge replaces the existing one when
/// its confidence is `>=`, or when it carries more evidence (`evidence_count` is a monotonic audit
/// counter).
pub fn base_upsert_wins(existing: &Edge, incoming: &Edge) -> bool {
    incoming.confidence.get() >= existing.confidence.get()
        || incoming.evidence_count > existing.evidence_count
}

/// The public edge for one key: a pure function of the SET of base contribution and support facts
/// (input is ordered by [`fact_key`] before folding). `None` when there is neither.
///
/// * `flows_to` folds everything through [`merge_flow_edges`] — the TS-S1 lattice, so the public
///   envelope (`flow_semantics`, `flow_evidence`, `constructs`, `flow_rules`, `flow_support`,
///   `flow_support_truncated`, `flow_confidence_min`, `construct`) is exactly the one TS-S1 ships.
/// * Every other kind takes one representative: maximum confidence, then maximum
///   `evidence_count`, then the smallest [`fact_key`]. There is no metadata union for kinds that
///   never had a merge rule.
///
/// With no facts the base contribution is returned unchanged, byte-for-byte.
pub fn project_edge(base: Option<&Edge>, facts: &[&Edge]) -> Option<Edge> {
    if facts.is_empty() {
        return base.cloned();
    }
    // `merge_flow_edges` picks its representative by a total order that does not cover every
    // field (provenance, evidence_count, line/column, extra metadata), so facts tying on it would
    // otherwise be chosen by input order — and backends return rows in different orders. Sorting
    // by `fact_key` first makes the result a function of the SET.
    let mut all: Vec<(String, Edge)> = base
        .into_iter()
        .cloned()
        .chain(facts.iter().map(|e| (*e).clone()))
        .map(|e| (fact_key(&e), e))
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    let all: Vec<Edge> = all.into_iter().map(|(_, e)| e).collect();
    if is_flow_edge(&all[0]) {
        return merge_flow_edges(all).into_iter().next();
    }
    all.into_iter()
        .map(|e| (fact_key(&e), e))
        .min_by(|(ka, a), (kb, b)| {
            b.confidence
                .get()
                .total_cmp(&a.confidence.get())
                .then_with(|| b.evidence_count.cmp(&a.evidence_count))
                .then_with(|| ka.cmp(kb))
        })
        .map(|(_, e)| e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::{EdgeKind, ResolutionTier};
    use crate::edge_tags;
    use crate::flow::{FlowEvidence, FlowFact, FlowSemantics};
    use crate::node::{Location, Span};
    use crate::symbol::SymbolId;

    fn owner() -> SupportOwner {
        SupportOwner::new("scip-typescript", "apps/web").unwrap()
    }

    fn flow(byte: u32) -> Edge {
        let mut e = Edge::new(
            SymbolId("c".into()),
            SymbolId("a".into()),
            edge_tags::other(edge_tags::FLOWS_TO),
            ResolutionTier::Scip,
            "scip-typescript",
        )
        .with_location(Location::new(
            "f.ts",
            Span {
                start_byte: byte,
                end_byte: byte + 1,
                ..Span::ZERO
            },
        ));
        FlowFact::new(
            FlowSemantics::ValuePreserving,
            FlowEvidence::Syntax,
            "assignment",
            "typescript",
        )
        .apply(&mut e);
        e
    }

    #[test]
    fn fact_key_ignores_metadata_insertion_order() {
        let mut a = flow(1);
        let mut b = flow(1);
        a.metadata.insert("z".into(), serde_json::json!(1));
        a.metadata
            .insert("y".into(), serde_json::json!({"q": 1, "p": 2}));
        b.metadata
            .insert("y".into(), serde_json::json!({"p": 2, "q": 1}));
        b.metadata.insert("z".into(), serde_json::json!(1));
        assert_eq!(fact_key(&a), fact_key(&b));
        assert_ne!(fact_key(&a), fact_key(&flow(1)));
    }

    #[test]
    fn invalid_facts_are_rejected() {
        let mut no_provenance = flow(1);
        no_provenance.resolved_by = " ".into();
        assert!(SupportFact::from_edge(no_provenance).is_err());
        let mut empty_end = flow(1);
        empty_end.target = SymbolId(String::new());
        assert!(SupportFact::from_edge(empty_end).is_err());
        assert!(SupportOwner::new("p", "").is_err());
        assert!(SupportFact::new("", flow(1)).is_err());
        assert!(
            SupportFact::new("a\0b", flow(1)).is_err(),
            "Postgres TEXT cannot hold NUL"
        );
        assert!(SupportOwner::new("p\0", "s").is_err());
    }

    /// The storage layer never rewrites an id: whitespace, case and Unicode normalization forms
    /// are all distinct ids, returned exactly as given.
    #[test]
    fn fact_ids_are_opaque() {
        let ids = [" x", "x", "X", "x ", "\u{e9}", "e\u{301}"];
        let facts: Vec<SupportFact> = ids
            .iter()
            .map(|id| SupportFact::new(*id, flow(1)).unwrap())
            .collect();
        let norm = normalize_facts(&facts).unwrap();
        assert_eq!(norm.len(), ids.len());
        for id in ids {
            assert!(norm.iter().any(|f| f.fact_id == id), "{id:?} rewritten");
        }
        // Same id, different content in one submission: rejected.
        let clash = [
            SupportFact::new("k", flow(1)).unwrap(),
            SupportFact::new("k", flow(2)).unwrap(),
        ];
        assert!(
            normalize_facts(&clash)
                .unwrap_err()
                .to_string()
                .contains("twice")
        );
        // Same id, same content: one fact.
        let dup = [
            SupportFact::new("k", flow(1)).unwrap(),
            SupportFact::new("k", flow(1)).unwrap(),
        ];
        assert_eq!(normalize_facts(&dup).unwrap().len(), 1);
    }

    /// Re-asserting an id with different content is a change: retracted and asserted.
    #[test]
    fn changed_content_under_one_id_is_retract_plus_assert() {
        let v1 = normalize_facts(&[SupportFact::new("occ-1", flow(1)).unwrap()]).unwrap();
        let stored: StoredFacts = v1.iter().map(|f| (f.id(), f.content.clone())).collect();
        let v2 = normalize_facts(&[SupportFact::new("occ-1", flow(2)).unwrap()]).unwrap();
        let plan = plan_replacement(&owner(), 2, Some(1), &stored, v2.clone()).unwrap();
        assert_eq!(
            (
                plan.report.asserted,
                plan.report.retained,
                plan.report.retracted
            ),
            (1, 0, 1)
        );
        let err = plan_replacement(&owner(), 1, Some(1), &stored, v2).unwrap_err();
        assert!(err.to_string().contains("generation conflict"), "{err}");
    }

    #[test]
    fn generation_rules() {
        let facts = normalize_facts(&[
            SupportFact::from_edge(flow(1)).unwrap(),
            SupportFact::from_edge(flow(2)).unwrap(),
        ])
        .unwrap();
        let ids: StoredFacts = facts.iter().map(|f| (f.id(), f.content.clone())).collect();
        // Replay: same generation, same set.
        let replay = plan_replacement(&owner(), 3, Some(3), &ids, facts.clone()).unwrap();
        assert!(replay.report.replayed && replay.touched.is_empty());
        // Conflict: same generation, different set.
        let other = normalize_facts(&[SupportFact::from_edge(flow(1)).unwrap()]).unwrap();
        let err = plan_replacement(&owner(), 3, Some(3), &ids, other).unwrap_err();
        assert!(err.to_string().contains("generation conflict"), "{err}");
        // Stale.
        let err = plan_replacement(&owner(), 2, Some(3), &ids, facts.clone()).unwrap_err();
        assert!(err.to_string().contains("stale generation"), "{err}");
        // Out of range for a signed 64-bit column.
        assert!(plan_replacement(&owner(), u64::MAX, None, &ids, facts).is_err());
    }

    /// The public boundary is serialized by agents and later MCP surfaces: pin EVERY field of the
    /// new types, so a rename or a dropped field is a test failure, not a silent wire change.
    #[test]
    fn public_types_serialize_every_field() {
        let report = SupportReplacement {
            owner: owner(),
            generation: 3,
            replayed: false,
            asserted: 1,
            retained: 2,
            retracted: 4,
            edges_touched: 5,
        };
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({
                "owner": {"producer": "scip-typescript", "snapshot": "apps/web"},
                "generation": 3, "replayed": false, "asserted": 1, "retained": 2,
                "retracted": 4, "edges_touched": 5,
            })
        );
        let fact = flow(7);
        let row = EdgeSupport::new(owner(), 3, "occ-7", fact.clone());
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(
            json.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["fact", "fact_id", "generation", "owner"]
        );
        assert_eq!(
            serde_json::to_value(SupportOwnerState::new(owner(), 9)).unwrap(),
            serde_json::json!({"owner": {"producer": "scip-typescript", "snapshot": "apps/web"}, "generation": 9})
        );
        assert_eq!(
            json["fact"],
            serde_json::to_value(&fact).unwrap(),
            "the fact is the full edge"
        );
        assert_eq!(json["fact_id"], serde_json::json!("occ-7"));
        let back: EdgeSupport = serde_json::from_value(json).unwrap();
        assert_eq!(back, row);
    }

    #[test]
    fn non_flow_projection_is_order_independent() {
        let mk = |tier, by: &str| {
            Edge::new(
                SymbolId("x".into()),
                SymbolId("y".into()),
                EdgeKind::Calls,
                tier,
                by,
            )
        };
        let a = mk(ResolutionTier::Scip, "scip-a");
        let b = mk(ResolutionTier::Scip, "scip-b");
        let c = mk(ResolutionTier::Heuristic, "name");
        let one = project_edge(None, &[&a, &b, &c]).unwrap();
        let two = project_edge(Some(&c), &[&b, &a]).unwrap();
        assert_eq!(one, two);
        assert_eq!(one.resolved_by, "scip-a");
        assert_eq!(project_edge(Some(&c), &[]), Some(c));
        assert_eq!(project_edge(None, &[]), None);
    }
}
