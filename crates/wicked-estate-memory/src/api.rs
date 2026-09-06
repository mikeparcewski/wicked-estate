//! `MemoryApi` (6-method, DES-001 §4.2) implemented for [`crate::MemoryEngine`].
//! Uses `wicked_estate_memory_core` types directly.

use crate::{MemoryEngine, RecallMode, ScopeFilter};
use std::collections::HashMap;
use wicked_estate_core::SymbolId;
use wicked_estate_memory_core::{
    CaptureRequest, MemKind, Memory, MemoryApi, MemoryCoverage, RecallQuery, RecalledItem,
    ReflectResult, Scope, Tier,
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
}
