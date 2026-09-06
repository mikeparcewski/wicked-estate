//! The proposal-queue primitive (DES-MEM-FACETED-001 §5.0) — a type-generic, inert write surface.
//!
//! Agents **submit** proposals (they land `Pending`, never recalled/applied); operators **approve**
//! (promote the payload to an active store, routed by `kind_type`) or **reject**. A proposal is
//! SAFE to write even from a `--readonly` worker precisely because it is inert until approved.
//!
//! Storage rides the same `wicked-estate` graph types a memory does: a proposal is a `Node` with
//! `kind = Other("proposal")` and its fields in `metadata`. A proposal node is a DIFFERENT kind
//! than `Other("memory")`, so `Memory::from_node` returns `None` on it and `memory.recall` never
//! surfaces a proposal — the inertness is structural, not a runtime check.
//!
//! `to_node`/`from_node` are hand-written (mirroring [`crate::Memory`], which has no serde): a
//! missing/invalid REQUIRED key makes `from_node` return `None` (fail-closed, exactly like the
//! Phase-1 facet backfill) rather than hydrate a half-formed proposal.

use crate::Facets;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use wicked_estate_core::{Language, Location, Node, NodeKind, Span, Symbol, SymbolId};

/// The node-kind label every proposal node carries (so estate stores them generically, and so they
/// are distinct from `Other("memory")` — a proposal is never a recall candidate).
pub const PROPOSAL_NODE_KIND: &str = "proposal";

/// Metadata keys for the proposal fields (the opaque extension bag; estate-core never reads these).
pub mod meta_keys {
    /// Validated lowercase kind_type token, e.g. `"memory"` or `"policy:security"`.
    pub const KIND_TYPE: &str = "kind_type";
    /// Type-specific payload, stored as a JSON **string** (round-tripped via serde_json).
    pub const PAYLOAD: &str = "payload";
    /// Agent-declared facets — a JSON object `{axis:value}`; written only when non-empty.
    pub const FACETS: &str = "facets";
    /// Authority-stamped provenance — a JSON object `{key:value}`; written only when non-empty.
    pub const PROVENANCE: &str = "provenance";
    /// Lifecycle state: `pending | approved | rejected`.
    pub const STATE: &str = "state";
    /// Unix-seconds capture time.
    pub const CREATED_AT: &str = "created_at";
}

/// The lifecycle state of a proposal. A proposal is submitted `Pending`, then an operator
/// transitions it to `Approved` (payload promoted to an active store) or `Rejected` (discarded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Pending,
    Approved,
    Rejected,
}

impl ProposalState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProposalState::Pending => "pending",
            ProposalState::Approved => "approved",
            ProposalState::Rejected => "rejected",
        }
    }

    /// Parse a wire/metadata token back to a state. `None` on anything else (fail-loud).
    pub fn parse(s: &str) -> Option<ProposalState> {
        match s {
            "pending" => Some(ProposalState::Pending),
            "approved" => Some(ProposalState::Approved),
            "rejected" => Some(ProposalState::Rejected),
            _ => None,
        }
    }
}

/// A `kind_type` is a validated lowercase token, optionally with ONE `:`-separated sub-type:
/// `^[a-z][a-z0-9_-]*(:[a-z][a-z0-9_-]*)?$` (e.g. `memory`, `policy:security`). Hand-rolled to
/// avoid a regex dependency, matching the [`crate::facets`] axis validator's spirit.
pub fn valid_kind_type(kind_type: &str) -> bool {
    /// One `^[a-z][a-z0-9_-]*$` token.
    fn valid_token(tok: &str) -> bool {
        let mut chars = tok.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    }
    match kind_type.split_once(':') {
        Some((head, tail)) => valid_token(head) && valid_token(tail),
        None => valid_token(kind_type),
    }
}

/// A queued proposal: an inert, type-generic write awaiting operator approval.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    /// Stable id (uuid-v7). The node's symbol is `Synthetic{scheme:"proposal", id}`.
    pub id: String,
    /// Validated kind_type token routing approval (`memory`, `policy:<steering_type>`, …).
    pub kind_type: String,
    /// Type-specific payload (memory: `{content, tier}`; policy: `{rule, severity, …}`).
    pub payload: Value,
    /// Orthogonal, intent-matching facets (Phase 1) carried onto the promoted artifact.
    pub facets: Facets,
    /// Authority-stamped provenance (run/unit/agent id) — set by the server, never the caller.
    pub provenance: BTreeMap<String, String>,
    /// Lifecycle state (submitted `Pending`).
    pub state: ProposalState,
    /// Unix-seconds capture time (caller-supplied clock → deterministic).
    pub created_at: i64,
}

impl Proposal {
    /// Build a fresh `Pending` proposal. Fails loud (`Err`) when `kind_type` is not a valid token —
    /// a malformed kind_type would mis-route approval, so it is rejected at construction. State is
    /// ALWAYS `Pending` here: the submit path never lets a caller pre-set `approved`/`rejected`.
    pub fn new(
        kind_type: impl Into<String>,
        payload: Value,
        facets: Facets,
        provenance: BTreeMap<String, String>,
        now: i64,
    ) -> Result<Proposal, String> {
        let kind_type = kind_type.into();
        if !valid_kind_type(&kind_type) {
            return Err(format!(
                "invalid kind_type {kind_type:?}: must match ^[a-z][a-z0-9_-]*(:[a-z][a-z0-9_-]*)?$"
            ));
        }
        Ok(Proposal {
            id: uuid::Uuid::now_v7().to_string(),
            kind_type,
            payload,
            facets,
            provenance,
            state: ProposalState::Pending,
            created_at: now,
        })
    }

    /// Stable estate symbol id for this proposal (`Synthetic{scheme:"proposal", id:<uuid-v7>}`).
    pub fn symbol(&self) -> SymbolId {
        Symbol::synthetic("proposal", self.id.clone()).id()
    }

    /// Materialize as an estate `Node` (kind `Other("proposal")`, fields in `metadata`).
    pub fn to_node(&self) -> Node {
        // The node `name` carries a human-legible label; the authoritative fields are in metadata.
        let mut node = Node::new(
            self.symbol(),
            NodeKind::Other(PROPOSAL_NODE_KIND.to_string()),
            format!("proposal:{}", self.kind_type),
            Language::new("proposal"),
            Location::new("proposal", Span::ZERO),
        );
        let m = &mut node.metadata;
        m.insert(meta_keys::KIND_TYPE.into(), self.kind_type.clone().into());
        // Payload as a JSON STRING (an opaque, type-specific blob — not a queryable graph field).
        m.insert(
            meta_keys::PAYLOAD.into(),
            Value::String(self.payload.to_string()),
        );
        if !self.facets.is_empty() {
            if let Ok(v) = serde_json::to_value(&self.facets) {
                m.insert(meta_keys::FACETS.into(), v);
            }
        }
        if !self.provenance.is_empty() {
            if let Ok(v) = serde_json::to_value(&self.provenance) {
                m.insert(meta_keys::PROVENANCE.into(), v);
            }
        }
        m.insert(meta_keys::STATE.into(), self.state.as_str().into());
        m.insert(meta_keys::CREATED_AT.into(), self.created_at.into());
        node
    }

    /// Reconstruct a `Proposal` from an estate `Node` written by [`Proposal::to_node`].
    ///
    /// Returns `None` when the node is NOT a proposal node, or a REQUIRED field
    /// (kind_type / payload / state / created_at) is missing or invalid — fail-closed, exactly like
    /// the memory facet backfill. `facets`/`provenance` are optional (absent ⇒ empty), but a
    /// PRESENT-but-invalid facets/provenance blob also fails closed (`None`) rather than silently
    /// dropping the constraint.
    pub fn from_node(node: &Node) -> Option<Proposal> {
        match &node.kind {
            NodeKind::Other(k) if k == PROPOSAL_NODE_KIND => {}
            _ => return None,
        }
        let m = &node.metadata;

        let kind_type = m.get(meta_keys::KIND_TYPE)?.as_str()?.to_string();
        if !valid_kind_type(&kind_type) {
            return None; // fail-closed on a corrupt kind_type
        }
        // Payload: a JSON string that must re-parse to a JSON value.
        let payload: Value = serde_json::from_str(m.get(meta_keys::PAYLOAD)?.as_str()?).ok()?;
        let state = ProposalState::parse(m.get(meta_keys::STATE)?.as_str()?)?;
        let created_at = m.get(meta_keys::CREATED_AT)?.as_i64()?;

        // Facets: absent ⇒ empty; present ⇒ validated serde path (invalid ⇒ None, fail-closed).
        let facets = match m.get(meta_keys::FACETS) {
            None => Facets::default(),
            Some(v) => serde_json::from_value::<Facets>(v.clone()).ok()?,
        };
        // Provenance: absent ⇒ empty; present ⇒ must be an object of string→string (else None).
        let provenance = match m.get(meta_keys::PROVENANCE) {
            None => BTreeMap::new(),
            Some(v) => serde_json::from_value::<BTreeMap<String, String>>(v.clone()).ok()?,
        };

        // id: strip the "proposal synthetic <id>:" rendering back to the uuid (mirrors Memory).
        let id = node
            .symbol
            .as_str()
            .rsplit(' ')
            .next()
            .unwrap_or("")
            .trim_end_matches(':')
            .to_string();

        Some(Proposal {
            id,
            kind_type,
            payload,
            facets,
            provenance,
            state,
            created_at,
        })
    }
}

/// The outcome of approving a proposal — routed by `kind_type` (DES-MEM-FACETED-001 §5.0).
#[derive(Debug, Clone, PartialEq)]
pub enum ApproveOutcome {
    /// `memory` kind_type: the payload was materialized into the ACTIVE memory store; `active_id`
    /// is the new memory's estate symbol id (now recallable).
    Promoted { active_id: String },
    /// `policy:*` kind_type: estate has no rules-write here, so the payload is handed back for the
    /// CALLER (crew) to route into steering. The proposal is marked `Approved`; nothing is written
    /// to an estate store.
    HandedOff { payload: Value },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facets(pairs: &[(&str, &str)]) -> Facets {
        Facets::try_from_map(pairs.iter().map(|(a, v)| (*a, *v))).expect("valid facets")
    }

    fn provenance(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn kind_type_validation() {
        assert!(valid_kind_type("memory"));
        assert!(valid_kind_type("policy"));
        assert!(valid_kind_type("policy:security"));
        assert!(valid_kind_type("a1_b-c:d2_e-f"));
        // Rejected shapes:
        assert!(!valid_kind_type("")); // empty
        assert!(!valid_kind_type("Memory")); // uppercase
        assert!(!valid_kind_type("1memory")); // leading digit
        assert!(!valid_kind_type("policy:")); // empty sub-type
        assert!(!valid_kind_type(":security")); // empty head
        assert!(!valid_kind_type("policy:a:b")); // two colons
        assert!(!valid_kind_type("policy security")); // space
    }

    #[test]
    fn new_rejects_invalid_kind_type() {
        let err = Proposal::new(
            "Policy:Security",
            serde_json::json!({}),
            Facets::default(),
            BTreeMap::new(),
            10,
        )
        .unwrap_err();
        assert!(err.contains("invalid kind_type"), "got: {err}");
    }

    #[test]
    fn new_hardcodes_pending_state() {
        let p = Proposal::new(
            "memory",
            serde_json::json!({"content": "x", "tier": "semantic"}),
            Facets::default(),
            BTreeMap::new(),
            10,
        )
        .unwrap();
        assert_eq!(p.state, ProposalState::Pending);
    }

    #[test]
    fn to_node_from_node_round_trip() {
        let mut p = Proposal::new(
            "policy:security",
            serde_json::json!({"rule": "no secrets in logs", "severity": "high"}),
            facets(&[("repo", "estate"), ("cli", "codex")]),
            provenance(&[("run_id", "R1"), ("run_agent", "claude")]),
            42,
        )
        .unwrap();
        p.state = ProposalState::Approved; // exercise a non-default state through the round-trip
        let node = p.to_node();
        assert!(matches!(node.kind, NodeKind::Other(ref k) if k == PROPOSAL_NODE_KIND));
        // payload persists as a JSON string, not an inline object.
        assert!(node.metadata[meta_keys::PAYLOAD].is_string());
        let back = Proposal::from_node(&node).expect("round-trip");
        assert_eq!(back, p);
    }

    #[test]
    fn empty_facets_and_provenance_are_omitted_and_hydrate_empty() {
        let p = Proposal::new(
            "memory",
            serde_json::json!({"content": "y", "tier": "semantic"}),
            Facets::default(),
            BTreeMap::new(),
            7,
        )
        .unwrap();
        let node = p.to_node();
        assert!(!node.metadata.contains_key(meta_keys::FACETS));
        assert!(!node.metadata.contains_key(meta_keys::PROVENANCE));
        let back = Proposal::from_node(&node).expect("round-trip");
        assert!(back.facets.is_empty());
        assert!(back.provenance.is_empty());
    }

    #[test]
    fn from_node_fails_closed_on_missing_required_keys() {
        let p = Proposal::new(
            "memory",
            serde_json::json!({"content": "z"}),
            Facets::default(),
            BTreeMap::new(),
            1,
        )
        .unwrap();
        for key in [
            meta_keys::KIND_TYPE,
            meta_keys::PAYLOAD,
            meta_keys::STATE,
            meta_keys::CREATED_AT,
        ] {
            let mut node = p.to_node();
            node.metadata.remove(key);
            assert!(
                Proposal::from_node(&node).is_none(),
                "missing required key {key} must fail closed (None)"
            );
        }
    }

    #[test]
    fn from_node_fails_closed_on_invalid_facets() {
        let p = Proposal::new(
            "memory",
            serde_json::json!({"content": "z"}),
            Facets::default(),
            BTreeMap::new(),
            1,
        )
        .unwrap();
        let mut node = p.to_node();
        node.metadata.insert(
            meta_keys::FACETS.to_string(),
            serde_json::json!({ "REPO": "x" }), // uppercase axis is invalid
        );
        assert!(
            Proposal::from_node(&node).is_none(),
            "present-but-invalid facets must fail closed, not default to empty"
        );
    }

    #[test]
    fn proposal_node_is_not_a_memory_node() {
        // The inertness invariant: a proposal node's kind is Other("proposal"), so Memory::from_node
        // returns None on it and memory.recall can never surface an unapproved proposal.
        let p = Proposal::new(
            "memory",
            serde_json::json!({"content": "should never be recalled", "tier": "semantic"}),
            Facets::default(),
            BTreeMap::new(),
            1,
        )
        .unwrap();
        let node = p.to_node();
        assert!(
            crate::Memory::from_node(&node).is_none(),
            "a proposal node MUST NOT hydrate as a memory (it is inert until approved)"
        );
    }
}
