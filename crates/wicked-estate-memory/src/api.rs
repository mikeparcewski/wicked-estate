//! `MemoryApi` (7-method, DES-001 §4.2 + DES-MEM-FACETED-001) implemented for
//! [`crate::MemoryEngine`]. Uses `wicked_estate_memory_core` types directly.

use crate::{MemoryEngine, RecallMode, ScopeFilter};
use std::collections::{BTreeMap, HashMap};
use wicked_estate_core::SymbolId;
use wicked_estate_memory_core::{
    ApproveOutcome, CaptureRequest, Facets, MemKind, Memory, MemoryApi, MemoryCoverage,
    MemoryListItem, Proposal, ProposalState, RecallQuery, RecalledItem, ReflectResult, Scope, Tier,
};
use wicked_estate_overlay::XEdge;

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

impl MemoryApi for MemoryEngine {
    type Error = anyhow::Error;

    fn capture(&mut self, req: CaptureRequest) -> Result<String, anyhow::Error> {
        let mem = Memory::new(
            parse_kind(&req.kind)?,
            parse_tier(&req.tier)?,
            Scope::parse(&req.scope),
            req.content,
            req.now,
        )
        .with_facets(req.facets);
        let id = mem.symbol().0.clone();

        match &req.about {
            Some(about) if !about.is_empty() => {
                let seeds: Vec<SymbolId> = about.iter().cloned().map(SymbolId).collect();
                MemoryEngine::capture_about(self, &mem, &seeds)?;
                // write xedge entries if epochs are provided
                if let (Some(about_epochs), Some(xedge)) = (&req.about_epochs, &self.xedge) {
                    for (sym, &epoch) in about_epochs.iter() {
                        xedge.put_edge(&XEdge::about(
                            mem.symbol().0.clone(),
                            sym.as_str(),
                            epoch,
                        ))?;
                    }
                }
            }
            _ => {
                MemoryEngine::capture(self, &mem)?;
            }
        }

        Ok(id)
    }

    fn recall(&self, q: &RecallQuery) -> Result<Vec<RecalledItem>, anyhow::Error> {
        let scope = Scope::parse(&q.scope);
        // Wire → filter: `scope_prefix` present (even "") REPLACES the inheritance filter with
        // the subtree-inclusive erase/coverage predicate; absent keeps the existing behavior
        // exactly. See [`ScopeFilter`] for why replace, not fuse.
        let filter = match q.scope_prefix.as_deref() {
            Some(prefix) => ScopeFilter::Subtree(prefix),
            None => ScopeFilter::Ancestors(&scope),
        };
        let seeds: Vec<SymbolId> = q.seeds.iter().cloned().map(SymbolId).collect();
        // AND-compose the session intent at the recall gate (DES-MEM-FACETED-001 §4.3). The engine's
        // public `recall` carries no intent, so drive the shared `ranked_candidates` seam directly
        // with `q.intent` (empty intent ⇒ every UNFACETED memory admitted — faceted memories are
        // intent-scoped and require matching axes; legacy data is all unfaceted, so recall is
        // byte-identical), then the same production `pack_recalled` assembly tail.
        let cands = self.ranked_candidates(
            &q.query,
            filter,
            &seeds,
            q.now,
            RecallMode::Hybrid,
            &q.intent,
        )?;
        let out = self.pack_recalled(cands, q.token_budget);
        Ok(out
            .into_iter()
            .map(|r| RecalledItem {
                id: r.id.0,
                content: r.content,
                tier: r.tier.as_str().to_string(),
                score: r.score,
                scope: r.scope,
            })
            .collect())
    }

    fn reflect(&mut self, scope: &str, now: i64) -> Result<ReflectResult, anyhow::Error> {
        let s = Scope::parse(scope);
        MemoryEngine::reflect(self, &s, now)?;
        // collect semantic-tier facts in this scope (post-reflect)
        let all = self.all_memories()?;
        let distilled_facts: Vec<String> = all
            .iter()
            .filter(|m| m.tier == Tier::Semantic && s.is_ancestor_of(&m.scope))
            .map(|m| m.content.clone())
            .collect();
        let node_count = distilled_facts.len() as u32;
        Ok(ReflectResult {
            scope: scope.to_string(),
            distilled_facts,
            node_count,
        })
    }

    fn erase(&mut self, scope_prefix: &str, _now: i64) -> Result<u32, anyhow::Error> {
        if scope_prefix.is_empty() {
            anyhow::bail!("erase: scope_prefix must not be empty (refusing total wipe)");
        }
        // collect victim symbol IDs for xedge cleanup BEFORE erasing
        let victim_ids: Vec<String> = self
            .all_memories()?
            .into_iter()
            .filter(|m| m.scope.path_in_prefix(scope_prefix))
            .map(|m| m.symbol().0)
            .collect();
        let count = MemoryEngine::erase(self, scope_prefix)?;
        if let Some(xedge) = self.xedge.as_ref() {
            for sym_id in &victim_ids {
                xedge.delete_by_src_id("memory", sym_id)?;
            }
        }
        Ok(count as u32)
    }

    fn learn(
        &mut self,
        fact: &str,
        symbols: &[String],
        symbol_epochs: &HashMap<String, u64>,
        now: i64,
    ) -> Result<String, anyhow::Error> {
        let mem = Memory::new(MemKind::Fact, Tier::Semantic, Scope::root(), fact, now);
        let id = mem.symbol().0.clone();
        let seeds: Vec<SymbolId> = symbols.iter().cloned().map(SymbolId).collect();
        MemoryEngine::capture_about(self, &mem, &seeds)?;
        if let Some(xedge) = self.xedge.as_ref() {
            for (sym, &epoch) in symbol_epochs {
                xedge.put_edge(&XEdge::about(mem.symbol().0.clone(), sym.as_str(), epoch))?;
            }
        }
        Ok(id)
    }

    fn coverage(&self, scope_prefix: Option<&str>) -> Result<MemoryCoverage, anyhow::Error> {
        let mems = self.all_memories()?;
        let filtered: Vec<_> = if let Some(prefix) = scope_prefix {
            mems.into_iter()
                .filter(|m| m.scope.path_in_prefix(prefix))
                .collect()
        } else {
            mems
        };
        let total = filtered.len() as u32;
        let mut by_tier: HashMap<String, u32> = HashMap::new();
        let mut by_kind: HashMap<String, u32> = HashMap::new();
        for mem in &filtered {
            *by_tier.entry(mem.tier.as_str().to_string()).or_insert(0) += 1;
            *by_kind.entry(mem.kind.as_str().to_string()).or_insert(0) += 1;
        }
        Ok(MemoryCoverage {
            total,
            by_tier,
            by_kind,
        })
    }

    fn list(&self, scope_prefix: Option<&str>) -> Result<Vec<MemoryListItem>, anyhow::Error> {
        // Same node-kind enumeration + subtree filter as `coverage`/`erase` (NOT the query-driven
        // recall retrieval, which returns nothing for an empty query) so the operator surface sees
        // the COMPLETE in-scope set. Facets ride through verbatim — no intent exclusion.
        let mems = self.all_memories()?;
        let filtered: Vec<Memory> = match scope_prefix {
            Some(prefix) => mems
                .into_iter()
                .filter(|m| m.scope.path_in_prefix(prefix))
                .collect(),
            None => mems,
        };
        Ok(filtered
            .into_iter()
            .map(|m| MemoryListItem {
                id: m.id,
                content: m.content,
                tier: m.tier.as_str().to_string(),
                scope: m.scope.as_path().to_string(),
                facets: m.facets,
                created_at: m.created_at,
            })
            .collect())
    }

    // ── Proposal queue (DES-MEM-FACETED-001 §5.0) — forwards to the inherent engine methods ──

    fn submit_proposal(
        &mut self,
        kind_type: &str,
        payload: serde_json::Value,
        facets: Facets,
        provenance: BTreeMap<String, String>,
        now: i64,
    ) -> Result<String, anyhow::Error> {
        MemoryEngine::submit_proposal(self, kind_type, payload, facets, provenance, now)
    }

    fn list_proposals(
        &self,
        kind_type: Option<&str>,
        state: Option<ProposalState>,
    ) -> Result<Vec<Proposal>, anyhow::Error> {
        MemoryEngine::list_proposals(self, kind_type, state)
    }

    fn approve_proposal(&mut self, id: &str, now: i64) -> Result<ApproveOutcome, anyhow::Error> {
        MemoryEngine::approve_proposal(self, id, now)
    }

    fn reject_proposal(&mut self, id: &str, now: i64) -> Result<(), anyhow::Error> {
        MemoryEngine::reject_proposal(self, id, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_req(content: &str, kind: &str, tier: &str, scope: &str, now: i64) -> CaptureRequest {
        let mut r = CaptureRequest::default();
        r.content = content.into();
        r.kind = kind.into();
        r.tier = tier.into();
        r.scope = scope.into();
        r.now = now;
        r
    }

    #[test]
    fn api_capture_recall_roundtrip_via_seam() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let id = MemoryApi::capture(
            &mut eng,
            make_req(
                "the user prefers oat milk",
                "episode",
                "episodic",
                "org:acme/agent:claude",
                100,
            ),
        )
        .unwrap();
        assert!(!id.is_empty());
        let rq = RecallQuery::new(
            "what does the user drink",
            "org:acme/agent:claude",
            500,
            101,
        );
        let out = MemoryApi::recall(&eng, &rq).unwrap();
        assert!(out.iter().any(|r| r.content.contains("oat milk")));
        assert!(out.iter().all(|r| !r.tier.is_empty()));
    }

    #[test]
    fn api_recall_scope_prefix_flips_to_subtree_visibility() {
        // The MemoryApi seam: `scope_prefix` on RecallQuery must reach the engine as
        // ScopeFilter::Subtree. A leaf-scoped memory is invisible to a root-scoped recall
        // (inheritance) but visible with scope_prefix "" (root subtree) or its own subtree.
        let mut eng = MemoryEngine::in_memory().unwrap();
        MemoryApi::capture(
            &mut eng,
            make_req(
                "brain import landed at a leaf scope",
                "fact",
                "semantic",
                "brain:test/doc:a",
                100,
            ),
        )
        .unwrap();
        let q = |scope_prefix: Option<&str>| {
            let mut rq = RecallQuery::new("brain import leaf", "", 500, 101); // root query scope
            rq.scope_prefix = scope_prefix.map(str::to_string);
            rq
        };
        let inherit = MemoryApi::recall(&eng, &q(None)).unwrap();
        assert!(
            inherit.is_empty(),
            "no prefix ⇒ inheritance: root recall must NOT see the leaf memory: {inherit:?}"
        );
        for prefix in ["", "brain:test"] {
            let out = MemoryApi::recall(&eng, &q(Some(prefix))).unwrap();
            assert!(
                out.iter().any(|r| r.scope == "brain:test/doc:a"),
                "scope_prefix {prefix:?} must surface the leaf memory with its own scope: {out:?}"
            );
        }
        let other = MemoryApi::recall(&eng, &q(Some("brain:other"))).unwrap();
        assert!(
            other.is_empty(),
            "a disjoint prefix must match nothing: {other:?}"
        );
    }

    #[test]
    fn api_rejects_bad_kind_tier() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let r = MemoryApi::capture(&mut eng, make_req("x", "bogus", "episodic", "", 1));
        assert!(r.is_err());
    }

    #[test]
    fn api_list_returns_complete_set_with_facets_where_recall_excludes() {
        // The management browse (DES-MEM-FACETED-001): `list` returns the COMPLETE in-scope set,
        // faceted memories included, each carrying its facets — precisely the two things
        // `recall` cannot do for an operator (empty query retrieves nothing; empty intent
        // excludes faceted memories). This test is the falsifier for both.
        let mut eng = MemoryEngine::in_memory().unwrap();
        MemoryApi::capture(
            &mut eng,
            make_req(
                "plain note",
                "fact",
                "semantic",
                "org:acme/agent:claude",
                100,
            ),
        )
        .unwrap();
        let facets = {
            let mut m = BTreeMap::new();
            m.insert("cli".to_string(), "codex".to_string());
            Facets::try_from(m).unwrap()
        };
        let mut req = make_req(
            "codex quirk",
            "fact",
            "semantic",
            "org:acme/agent:codex",
            100,
        );
        req.facets = facets;
        MemoryApi::capture(&mut eng, req).unwrap();

        // list(None): the whole store, faceted + unfaceted, with per-item facets.
        let all = MemoryApi::list(&eng, None).unwrap();
        assert_eq!(all.len(), 2, "list returns the complete set: {all:?}");
        let codex = all
            .iter()
            .find(|i| i.content.contains("codex quirk"))
            .expect("faceted memory must appear in list");
        assert_eq!(
            codex.facets.get("cli"),
            Some("codex"),
            "list must carry per-item facets: {codex:?}"
        );
        let plain = all
            .iter()
            .find(|i| i.content.contains("plain note"))
            .unwrap();
        assert_eq!(plain.facets, Facets::default(), "unfaceted ⇒ empty facets");

        // scope_prefix restricts to the subtree; a disjoint prefix matches nothing.
        let only_codex = MemoryApi::list(&eng, Some("org:acme/agent:codex")).unwrap();
        assert_eq!(only_codex.len(), 1);
        assert!(only_codex[0].content.contains("codex quirk"));
        assert!(MemoryApi::list(&eng, Some("org:other")).unwrap().is_empty());

        // The gap `list` closes: empty-intent recall EXCLUDES the faceted memory even with a
        // matching query and whole-store visibility.
        let mut rq = RecallQuery::new("codex quirk", "", 500, 101);
        rq.scope_prefix = Some(String::new());
        let recalled = MemoryApi::recall(&eng, &rq).unwrap();
        assert!(
            !recalled.iter().any(|r| r.content.contains("codex quirk")),
            "empty-intent recall excludes faceted memories — the reason list exists: {recalled:?}"
        );
    }
}
