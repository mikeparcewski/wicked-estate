//! Proposal-queue operations on the [`MemoryEngine`] (DES-MEM-FACETED-001 §5.0).
//!
//! These are the concrete write/read/route methods behind the `proposal.*` MCP tools. They run on
//! the engine's existing `GraphStore` (no new store seam): a proposal is stored as a
//! `NodeKind::Other("proposal")` node — a DIFFERENT kind than `Other("memory")`, so it is inert
//! (never a recall candidate) until approved. `submit` is a SAFE write (the queue is inert
//! regardless of the sandbox); `approve`/`reject` are operator writes to the active store.

use crate::MemoryEngine;
use wicked_estate_core::{NodeKind, Symbol, SymbolId, SymbolQuery};
use wicked_estate_memory_core::{
    ApproveOutcome, Facets, MemKind, Memory, Proposal, ProposalState, Scope, Tier,
    proposal::PROPOSAL_NODE_KIND,
};

impl MemoryEngine {
    /// Submit a NEW proposal — writes a `Pending` proposal node and returns its id.
    ///
    /// State is hard-coded `Pending` by [`Proposal::new`]; the node kind is set by
    /// [`Proposal::to_node`]. The caller supplies only `kind_type` / `payload` / `facets` /
    /// (server-stamped) `provenance` — it can set NEITHER the state NOR the node kind. This is a
    /// safe write even under `--readonly` (the proposal is never recalled or applied until approved).
    pub fn submit_proposal(
        &mut self,
        kind_type: &str,
        payload: serde_json::Value,
        facets: Facets,
        provenance: std::collections::BTreeMap<String, String>,
        now: i64,
    ) -> anyhow::Result<String> {
        let proposal = Proposal::new(kind_type, payload, facets, provenance, now)
            .map_err(|e| anyhow::anyhow!(e))?;
        let id = proposal.id.clone();
        self.store.upsert_nodes(&[proposal.to_node()])?;
        Ok(id)
    }

    /// List proposals, optionally filtered by `kind_type` and/or `state`. Full scan of the
    /// proposal-kind nodes (fine at local-first scale, like [`MemoryEngine::all_memories`]).
    pub fn list_proposals(
        &self,
        kind_type: Option<&str>,
        state: Option<ProposalState>,
    ) -> anyhow::Result<Vec<Proposal>> {
        let nodes = self.store.find_symbols(&SymbolQuery {
            text: None,
            exact_name: None,
            kinds: vec![NodeKind::Other(PROPOSAL_NODE_KIND.into())],
            language: None,
            limit: None,
            scope_prefix: None,
        })?;
        Ok(nodes
            .iter()
            .filter_map(Proposal::from_node)
            .filter(|p| kind_type.is_none_or(|kt| p.kind_type == kt))
            .filter(|p| state.is_none_or(|s| p.state == s))
            .collect())
    }

    /// Load a proposal by id, requiring it to be `Pending` (approve/reject only act on the queue).
    fn load_pending(&self, id: &str) -> anyhow::Result<Proposal> {
        let sym: SymbolId = Symbol::synthetic("proposal", id).id();
        let node = self
            .store
            .get_node(&sym)?
            .ok_or_else(|| anyhow::anyhow!("no proposal with id {id}"))?;
        let proposal = Proposal::from_node(&node)
            .ok_or_else(|| anyhow::anyhow!("node {id} is not a valid proposal"))?;
        if proposal.state != ProposalState::Pending {
            anyhow::bail!(
                "proposal {id} is {} — only a pending proposal can be approved/rejected",
                proposal.state.as_str()
            );
        }
        Ok(proposal)
    }

    /// Approve a `Pending` proposal, ROUTED by `kind_type`.
    ///
    /// - `memory`: materialize an ACTIVE memory (`Memory::new` from `payload.content` + `payload.tier`,
    ///   `.with_facets(proposal.facets)`, scope from `provenance["scope"]` or root) and capture it via
    ///   the existing memory write path (⇒ now recallable), then mark the proposal `Approved` ⇒
    ///   [`ApproveOutcome::Promoted`].
    /// - `policy:*`: estate has no rules-write here, so write NOTHING; mark the proposal `Approved`
    ///   and hand the payload back for the caller (crew) to route into steering ⇒
    ///   [`ApproveOutcome::HandedOff`].
    /// - anything else: fail loud (unknown kind_type shape).
    pub fn approve_proposal(&mut self, id: &str, now: i64) -> anyhow::Result<ApproveOutcome> {
        let mut proposal = self.load_pending(id)?;

        let outcome = if proposal.kind_type == "memory" {
            let payload = &proposal.payload;
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("memory proposal {id}: payload.content (string) is required")
                })?
                .to_string();
            let tier = parse_tier(
                payload
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("semantic"),
            )?;
            // MemKind is not part of the memory payload contract ({content, tier}); default to the
            // kind natural for the tier, with an optional explicit `payload.kind` override.
            let kind = match payload.get("kind").and_then(|v| v.as_str()) {
                Some(k) => parse_kind(k)?,
                None => kind_for_tier(tier),
            };
            // Scope rides provenance (authority-stamped) when present; else root.
            let scope = proposal
                .provenance
                .get("scope")
                .map(|s| Scope::parse(s))
                .unwrap_or_else(Scope::root);
            // Deterministic id from the proposal id ⇒ approval is IDEMPOTENT: if a crash or a
            // failed state-write leaves the proposal Pending after capture succeeded, a re-approve
            // upserts the SAME memory node (no duplicate) instead of minting a new random one.
            let mem = Memory::new(kind, tier, scope, content, now)
                .with_id(format!("proposal:{}", proposal.id))
                .with_facets(proposal.facets.clone());
            let active_id = mem.symbol().0.clone();
            self.capture(&mem)?; // active store write → immediately recallable
            ApproveOutcome::Promoted { active_id }
        } else if proposal.kind_type.starts_with("policy:") {
            // No rules-write in estate: hand the payload off for the caller to route to steering.
            ApproveOutcome::HandedOff {
                payload: proposal.payload.clone(),
            }
        } else {
            anyhow::bail!(
                "cannot approve proposal {id}: unknown kind_type {:?} (expected \"memory\" or \"policy:*\")",
                proposal.kind_type
            );
        };

        // Mark Approved only after the promotion succeeded (a failed capture leaves it Pending).
        proposal.state = ProposalState::Approved;
        self.store.upsert_nodes(&[proposal.to_node()])?;
        Ok(outcome)
    }

    /// Reject a `Pending` proposal — marks it `Rejected`; it is never promoted to an active store.
    pub fn reject_proposal(&mut self, id: &str, _now: i64) -> anyhow::Result<()> {
        let mut proposal = self.load_pending(id)?;
        proposal.state = ProposalState::Rejected;
        self.store.upsert_nodes(&[proposal.to_node()])?;
        Ok(())
    }
}

fn parse_tier(s: &str) -> anyhow::Result<Tier> {
    Ok(match s {
        "working" => Tier::Working,
        "episodic" => Tier::Episodic,
        "semantic" => Tier::Semantic,
        "procedural" => Tier::Procedural,
        "archival" => Tier::Archival,
        other => anyhow::bail!("unknown tier: {other}"),
    })
}

fn parse_kind(s: &str) -> anyhow::Result<MemKind> {
    Ok(match s {
        "working" => MemKind::Working,
        "episode" => MemKind::Episode,
        "entity" => MemKind::Entity,
        "fact" => MemKind::Fact,
        "skill" => MemKind::Skill,
        "archive" => MemKind::Archive,
        other => anyhow::bail!("unknown mem kind: {other}"),
    })
}

/// The `MemKind` natural for a tier when the payload does not name one explicitly.
fn kind_for_tier(tier: Tier) -> MemKind {
    match tier {
        Tier::Working => MemKind::Working,
        Tier::Episodic => MemKind::Episode,
        Tier::Semantic => MemKind::Fact,
        Tier::Procedural => MemKind::Skill,
        Tier::Archival => MemKind::Archive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use wicked_estate_memory_core::{MemoryApi, RecallQuery};

    fn facets(pairs: &[(&str, &str)]) -> Facets {
        Facets::try_from_map(pairs.iter().map(|(a, v)| (*a, *v))).expect("valid facets")
    }

    #[test]
    fn submit_list_approve_memory_promotes_and_marks_approved() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        // submit a memory proposal (inert)
        let id = eng
            .submit_proposal(
                "memory",
                serde_json::json!({"content": "codex needs workspace-write", "tier": "semantic"}),
                facets(&[("cli", "codex")]),
                BTreeMap::from([("run_id".to_string(), "R1".to_string())]),
                100,
            )
            .unwrap();

        // it is inert: memory.recall does NOT surface it yet
        let pre =
            MemoryApi::recall(&eng, &RecallQuery::new("codex workspace", "", 500, 101)).unwrap();
        assert!(
            !pre.iter().any(|r| r.content.contains("workspace-write")),
            "a pending proposal must NOT be recallable: {pre:?}"
        );

        // list(pending) shows it
        let pending = eng
            .list_proposals(Some("memory"), Some(ProposalState::Pending))
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);

        // approve(memory) → promoted, and now recallable with the proposal's facets as intent
        let outcome = eng.approve_proposal(&id, 102).unwrap();
        let active_id = match outcome {
            ApproveOutcome::Promoted { active_id } => active_id,
            other => panic!("expected Promoted, got {other:?}"),
        };
        assert!(!active_id.is_empty());

        let mut rq = RecallQuery::new("codex workspace", "", 500, 103);
        rq.intent = facets(&[("cli", "codex")]);
        let post = MemoryApi::recall(&eng, &rq).unwrap();
        assert!(
            post.iter().any(|r| r.content.contains("workspace-write")),
            "an approved memory proposal must be recallable: {post:?}"
        );

        // the proposal itself is now Approved (no longer pending)
        assert!(
            eng.list_proposals(None, Some(ProposalState::Pending))
                .unwrap()
                .is_empty(),
            "no pending proposals remain after approval"
        );
        let approved = eng
            .list_proposals(None, Some(ProposalState::Approved))
            .unwrap();
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].id, id);
    }

    #[test]
    fn approve_promotes_with_a_deterministic_id_for_idempotency() {
        // The promoted memory's id is DERIVED from the proposal id (not a fresh random uuid), so a
        // re-promote after a crash-before-mark (proposal still Pending, capture already done)
        // upserts the SAME memory node by symbol instead of duplicating. Verify the derivation.
        let mut eng = MemoryEngine::in_memory().unwrap();
        let id = eng
            .submit_proposal(
                "memory",
                serde_json::json!({"content": "deterministic promote", "tier": "semantic"}),
                Facets::default(),
                BTreeMap::new(),
                1,
            )
            .unwrap();
        let active_id = match eng.approve_proposal(&id, 2).unwrap() {
            ApproveOutcome::Promoted { active_id } => active_id,
            other => panic!("expected Promoted, got {other:?}"),
        };
        let expected = wicked_estate_core::Symbol::synthetic("mem", format!("proposal:{id}"))
            .id()
            .0;
        assert_eq!(
            active_id, expected,
            "promoted id must be derived from the proposal id (idempotent re-promote), not random"
        );
    }

    #[test]
    fn approve_policy_hands_off_without_writing_a_memory() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let id = eng
            .submit_proposal(
                "policy:security",
                serde_json::json!({"rule": "no secrets in logs", "severity": "high"}),
                Facets::default(),
                BTreeMap::new(),
                1,
            )
            .unwrap();
        let outcome = eng.approve_proposal(&id, 2).unwrap();
        match outcome {
            ApproveOutcome::HandedOff { payload } => {
                assert_eq!(payload["rule"], "no secrets in logs");
            }
            other => panic!("expected HandedOff, got {other:?}"),
        }
        // NOTHING was written to the active memory store.
        assert_eq!(
            eng.count().unwrap(),
            0,
            "a policy handoff must not write a memory"
        );
    }

    #[test]
    fn rejected_proposal_never_becomes_active() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let id = eng
            .submit_proposal(
                "memory",
                serde_json::json!({"content": "should be discarded", "tier": "semantic"}),
                Facets::default(),
                BTreeMap::new(),
                1,
            )
            .unwrap();
        eng.reject_proposal(&id, 2).unwrap();

        // no active memory materialized
        assert_eq!(eng.count().unwrap(), 0);
        let recall = MemoryApi::recall(&eng, &RecallQuery::new("discarded", "", 500, 3)).unwrap();
        assert!(
            recall.is_empty(),
            "a rejected proposal must never be recalled"
        );

        // it is Rejected, and can no longer be approved (not pending)
        let rejected = eng
            .list_proposals(None, Some(ProposalState::Rejected))
            .unwrap();
        assert_eq!(rejected.len(), 1);
        assert!(
            eng.approve_proposal(&id, 4).is_err(),
            "a rejected proposal cannot be approved"
        );
    }

    #[test]
    fn approve_unknown_id_fails_loud() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        assert!(eng.approve_proposal("does-not-exist", 1).is_err());
        assert!(eng.reject_proposal("does-not-exist", 1).is_err());
    }
}
