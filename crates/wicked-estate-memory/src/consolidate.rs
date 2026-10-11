//! Consolidation + learning: the pipeline that moves/upgrades memory across tiers and lets it learn.
//!
//! **DEC-R model-free recast** (`BUILD-SPEC.md` §3): no `Reasoner` seam. Consolidation is built from
//! deterministic verbs; judgment-consolidation (LLM distillation) is the **agent's** job, supplied
//! as `distilled[]`. Absent an agent, the deterministic floor (`heuristic_extract`/`heuristic_summary`)
//! keeps the engine useful (option (b)) — never inert, never fabricating.
//!
//! - `recall_episodic_batch`: return the un-reflected episodic batch for a scope (the read half).
//! - `capture_facts`        : write agent-supplied facts (`distilled[]`); when none given, distill
//!   with the deterministic floor (`heuristic_extract`). The write half of `reflect`.
//! - `merge_candidates`     : a deterministic dedup HINT (exact-CI ∪ fuzzy); the agent/skill decides.
//! - `archive_cluster`      : fold aged low-salience memory into a T4 node; agent summary or the
//!   extractive `heuristic_summary` default.
//! - `reflect`  : the convenience pass wiring read→distill→write deterministically (model-free).
//! - `reinforce`: learning — bump reinforcement (→ Wilson confidence → salience → recall rank).
//! - `promote_skills`: T2 → T3 procedural (well-reinforced facts become skills).
//! - `archive`  : aged low-salience memory → T4 archival summary nodes, one per exact source scope
//!   + facet set (sources move to the archival tier); Working memory past its 24 h TTL is dropped.
//!
//! Implemented on `MemoryEngine` from a child module so it can use the engine internals while
//! keeping `lib.rs` focused. Fully deterministic and offline/testable (no model in the loop).

use crate::MemoryEngine;
use wicked_estate_core::{Edge, EdgeKind, ResolutionTier, Result, SymbolId};
use wicked_estate_memory_core::{
    Extracted, Facets, MemKind, Memory, Scope, Tier, WORKING_TTL_SECS, fuzzy_candidates,
    heuristic_extract, heuristic_same_entity, heuristic_summary, normalize,
    salience as compute_salience,
};

/// What one consolidation pass did (returned by [`MemoryEngine::consolidate`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsolidationReport {
    pub new_facts: usize,
    pub new_skills: usize,
    pub archived: usize,
}

impl MemoryEngine {
    /// Run one full consolidation pass over `scope`: reflect (T1→T2) → promote (T2→T3) → archive
    /// (→T4). The single "run maintenance" entry point a scheduler/host calls periodically. Sync +
    /// idempotent; uses sensible default thresholds (tuned post-benchmark, L5). A background loop is
    /// a thin wrapper a host adds (requires a concurrency model — see BACKLOG).
    pub fn consolidate(&mut self, scope: &Scope, now: i64) -> Result<ConsolidationReport> {
        // Expired session scratch goes first, so nothing below can distil or summarize it.
        self.drop_expired_working(scope, now)?;
        let new_facts = self.reflect(scope, now)?;

        // Adaptive per-scope calibration (PR-8): with enough samples, calibrate promote/archive
        // cutoffs to the scope's OWN median (p50) instead of fixed constants; otherwise fall back to
        // defaults. (rust-self-learning's per-domain calibration idea — no streaming-quantile dep.)
        const MIN_SAMPLES: usize = 8;
        let mems = self.all_memories()?;
        let in_scope: Vec<&Memory> = mems
            .iter()
            .filter(|m| scope.is_ancestor_of(&m.scope))
            .collect();

        let fact_conf: Vec<f64> = in_scope
            .iter()
            .filter(|m| m.kind == MemKind::Fact)
            .map(|m| m.confidence())
            .collect();
        let promote_conf = if fact_conf.len() >= MIN_SAMPLES {
            wicked_estate_memory_core::p50(&fact_conf)
                .unwrap_or(0.3)
                .max(0.3)
        } else {
            0.3
        };
        let new_skills = self.promote_skills(scope, 5, promote_conf, now)?;

        let sals: Vec<f64> = in_scope
            .iter()
            .filter(|m| matches!(m.tier, Tier::Episodic | Tier::Semantic))
            .map(|m| {
                compute_salience(
                    &self.sal,
                    m.confidence(),
                    now - m.created_at,
                    m.access_count,
                )
            })
            .collect();
        let archive_floor = if sals.len() >= MIN_SAMPLES {
            wicked_estate_memory_core::p50(&sals)
                .unwrap_or(0.3)
                .min(0.3)
        } else {
            0.3
        };
        let archived = self.archive(scope, 90 * 86_400, archive_floor, now)?;
        Ok(ConsolidationReport {
            new_facts,
            new_skills,
            archived,
        })
    }

    fn edge(src: &SymbolId, tgt: &SymbolId, rel: &str) -> Edge {
        Edge::new(
            src.clone(),
            tgt.clone(),
            EdgeKind::Other(rel.into()),
            ResolutionTier::Heuristic,
            "wicked-memory",
        )
    }

    /// Does a memory of `kind` with this exact content already exist in `scope`? (cheap dedup)
    fn exists(
        &self,
        kind: MemKind,
        content: &str,
        scope: &Scope,
        facets: &Facets,
    ) -> Result<Option<SymbolId>> {
        for m in self.all_memories()? {
            if m.kind == kind
                && m.content == content
                && m.scope.as_path() == scope.as_path()
                && &m.facets == facets
            {
                return Ok(Some(m.symbol()));
            }
        }
        Ok(None)
    }

    /// Entities VISIBLE to an episode at `mem_scope` (its own scope or an ANCESTOR — INHERITANCE
    /// direction, the FATAL-fix). Used by `merge_candidates`.
    fn visible_entities(&self, mem_scope: &Scope) -> Result<Vec<Memory>> {
        Ok(self
            .all_memories()?
            .into_iter()
            .filter(|m| m.kind == MemKind::Entity && m.scope.is_ancestor_of(mem_scope))
            .collect())
    }

    /// Deterministic dedup HINT (DEC-R): for `name`, return likely-same existing entities visible to
    /// `mem_scope`, ordered exact-first then fuzzy. **Only a HINT** — the *engine* auto-merges only the
    /// deterministic-confident matches (exact-CI ∪ the fuzzy hit confirmed by `heuristic_same_entity`);
    /// any residual fuzzy ambiguity is left for the agent/skill to adjudicate (it never calls a model).
    /// Returns the canonical symbols of the confident matches (typically 0 or 1).
    fn merge_candidates(&self, name: &str, mem_scope: &Scope) -> Result<Vec<SymbolId>> {
        let entities = self.visible_entities(mem_scope)?;
        // tier 1: exact normalized match.
        if let Some(m) = entities
            .iter()
            .find(|m| normalize(&m.content) == normalize(name))
        {
            return Ok(vec![m.symbol()]);
        }
        // tier 2: fuzzy candidates, confirmed by the deterministic floor (no model).
        let names: Vec<String> = entities.iter().map(|m| m.content.clone()).collect();
        let mut hints = Vec::new();
        for idx in fuzzy_candidates(name, &names, 0.6) {
            if heuristic_same_entity(&names[idx], name) {
                hints.push(entities[idx].symbol());
            }
        }
        Ok(hints)
    }

    /// Entity-merge (FR-8 / PR-6), deterministic-first: reuse the canonical entity surfaced by
    /// `merge_candidates`, else create a new one. Returns the canonical entity's symbol.
    fn merge_or_create_entity(
        &mut self,
        name: &str,
        mem_scope: &Scope,
        now: i64,
    ) -> Result<SymbolId> {
        if let Some(canonical) = self.merge_candidates(name, mem_scope)?.into_iter().next() {
            return Ok(canonical);
        }
        // new entity.
        let emem = Memory::new(
            MemKind::Entity,
            Tier::Semantic,
            mem_scope.clone(),
            name,
            now,
        );
        let s = emem.symbol();
        self.capture(&emem)?;
        Ok(s)
    }

    /// The READ half (DEC-R verb): the un-reflected episodic batch visible to `scope` (its own scope
    /// or descendants). Pure read — the agent (or `reflect`'s deterministic floor) distills it.
    pub fn recall_episodic_batch(&self, scope: &Scope) -> Result<Vec<Memory>> {
        // Candidates from the sidecar index (indexed by tier) — not a full node scan (PR-1).
        let ids = self.ext.ids_in_tier(Tier::Episodic)?;
        Ok(self
            .hydrate(&ids)?
            .into_iter()
            .filter(|m| !is_scratch(m)) // scratch is never distilled
            .filter(|m| scope.is_ancestor_of(&m.scope))
            .collect())
    }

    /// The WRITE half (DEC-R verb): persist a distillation of one episode into the semantic tier,
    /// deduping. `distilled` is the **agent's** judgment-extraction; when `None`, fall back to the
    /// deterministic extractive floor (`heuristic_extract`) so the engine is useful model-free
    /// (option (b)). Facts become `Fact` nodes (`derived_from` the episode); entities merge via the
    /// deterministic `merge_candidates` HINT (`mentions` the episode). Returns new facts created.
    pub fn capture_facts(
        &mut self,
        episode: &Memory,
        distilled: Option<Extracted>,
        now: i64,
    ) -> Result<usize> {
        let ex = distilled.unwrap_or_else(|| heuristic_extract(&episode.content));
        let ep_sym = episode.symbol();

        let mut new_facts = 0usize;
        for fact in ex.facts {
            if self
                .exists(MemKind::Fact, &fact, &episode.scope, &episode.facets)?
                .is_some()
            {
                continue;
            }
            // A derived fact keeps its episode's restrictions (scope AND facets).
            let fmem = Memory::new(
                MemKind::Fact,
                Tier::Semantic,
                episode.scope.clone(),
                fact,
                now,
            )
            .with_facets(episode.facets.clone());
            let fsym = fmem.symbol();
            self.capture(&fmem)?;
            self.store
                .upsert_edges(&[Self::edge(&fsym, &ep_sym, "derived_from")])?;
            new_facts += 1;
        }

        for entity in ex.entities {
            let esym = self.merge_or_create_entity(&entity, &episode.scope, now)?;
            self.store
                .upsert_edges(&[Self::edge(&ep_sym, &esym, "mentions")])?;
        }
        Ok(new_facts)
    }

    /// Reflect over episodic memories in `scope` (+ descendants): distill facts/entities into the
    /// semantic tier, deduping. **Model-free** (DEC-R): wires `recall_episodic_batch` → the
    /// deterministic extractive floor → `capture_facts`. An agent that wants judgment-consolidation
    /// supplies its own `distilled[]` per episode via `capture_facts` directly. Returns new facts.
    pub fn reflect(&mut self, scope: &Scope, now: i64) -> Result<usize> {
        let episodes = self.recall_episodic_batch(scope)?;
        let mut new_facts = 0usize;
        for ep in episodes {
            // No agent distillation in the convenience pass → the deterministic floor (option (b)).
            new_facts += self.capture_facts(&ep, None, now)?;
        }
        Ok(new_facts)
    }

    /// Learning: record a reinforcement (or contradiction) on a memory. Raises/lowers its Wilson
    /// confidence (→ salience → recall rank). Idempotent re-upsert of the node.
    pub fn reinforce(&mut self, id: &SymbolId, positive: bool, now: i64) -> Result<()> {
        let Some(node) = self.store.get_node(id)? else {
            return Ok(());
        };
        let Some(mut m) = Memory::from_node(&node) else {
            return Ok(());
        };
        m.reinforce_total += 1;
        if positive {
            m.reinforce_pos += 1;
        }
        m.access_count += 1;
        m.last_access = now;
        self.store.upsert_nodes(&[m.to_node()])?;
        self.ext.upsert(&m)?; // keep the sidecar index in sync (PR-1)
        Ok(())
    }

    /// Promote well-reinforced semantic facts to the procedural tier (T3) as skills.
    /// `min_total` = minimum reinforcement observations; `min_conf` = Wilson-confidence floor.
    pub fn promote_skills(
        &mut self,
        scope: &Scope,
        min_total: u64,
        min_conf: f64,
        now: i64,
    ) -> Result<usize> {
        // Indexed candidates: facts with enough reinforcement (idx_mem_ext_kind), then refine in Rust.
        let ids = self.ext.fact_ids_reinforced(min_total)?;
        let facts: Vec<Memory> = self
            .hydrate(&ids)?
            .into_iter()
            .filter(|m| {
                m.kind == MemKind::Fact
                    && scope.is_ancestor_of(&m.scope)
                    && m.reinforce_total >= min_total
                    && m.confidence() >= min_conf
            })
            .collect();
        let mut n = 0;
        for f in facts {
            if self
                .exists(MemKind::Skill, &f.content, &f.scope, &f.facets)?
                .is_some()
            {
                continue;
            }
            // A promoted skill keeps its fact's restrictions (scope AND facets).
            let skill = Memory::new(
                MemKind::Skill,
                Tier::Procedural,
                f.scope.clone(),
                f.content.clone(),
                now,
            )
            .with_facets(f.facets.clone());
            let ssym = skill.symbol();
            self.capture(&skill)?;
            self.store
                .upsert_edges(&[Self::edge(&ssym, &f.symbol(), "derived_from")])?;
            n += 1;
        }
        Ok(n)
    }

    /// Fold an explicit `cluster` of source memories into self-contained T4 archival nodes (DEC-R
    /// verb). `summary` is the **agent's** abstractive summary; when `None`, fall back to the
    /// deterministic extractive `heuristic_summary` (option (b)). Records the subsumed source ids so
    /// the archive survives a later physical purge of the sources. Returns sources subsumed (0 if
    /// the cluster is empty). No model in the loop.
    ///
    /// **Never widens visibility.** Sources outside `scope` are ignored, and the rest are folded
    /// per (exact scope, facet set): each T4 node lives at its sources' own scope with their facets,
    /// so a root-scope pass cannot expose project B's or a user-restricted memory to anyone the
    /// sources were hidden from, and erasing a scope erases its summaries too. An agent `summary`
    /// is applied only when the WHOLE cluster is one such group with nothing filtered out. Scratch
    /// (Working kind or tier) is never archived.
    pub fn archive_cluster(
        &mut self,
        scope: &Scope,
        cluster: &[Memory],
        summary: Option<String>,
        now: i64,
    ) -> Result<usize> {
        let mut groups: Vec<(Scope, Facets, Vec<Memory>)> = Vec::new();
        let mut kept = 0usize;
        // Scratch (Working kind or tier) is never archived, whatever its scope or age.
        for m in cluster
            .iter()
            .filter(|m| !is_scratch(m) && scope.is_ancestor_of(&m.scope))
        {
            kept += 1;
            match groups
                .iter_mut()
                .find(|(s, f, _)| s.as_path() == m.scope.as_path() && f == &m.facets)
            {
                Some((_, _, members)) => members.push(m.clone()),
                None => groups.push((m.scope.clone(), m.facets.clone(), vec![m.clone()])),
            }
        }
        // A supplied summary describes the WHOLE cluster: it is used only when that whole cluster
        // is one exact (scope, facets) group with nothing filtered out; else regenerate per group.
        let summary = if groups.len() == 1 && kept == cluster.len() {
            summary
        } else {
            None
        };
        let mut n = 0;
        for (group_scope, facets, members) in groups {
            n += self.archive_group(&group_scope, facets, &members, summary.clone(), now)?;
        }
        Ok(n)
    }

    /// One T4 node over `members`, which share `scope` and `facets` exactly.
    fn archive_group(
        &mut self,
        scope: &Scope,
        facets: Facets,
        cluster: &[Memory],
        summary: Option<String>,
        now: i64,
    ) -> Result<usize> {
        let summary = summary.unwrap_or_else(|| {
            let texts: Vec<&str> = cluster.iter().map(|m| m.content.as_str()).collect();
            heuristic_summary(&texts, 2000)
        });
        let arc = Memory::new(
            MemKind::Archive,
            Tier::Archival,
            scope.clone(),
            summary,
            now,
        )
        .with_facets(facets);
        // self-contained provenance: record subsumed source ids in metadata.
        let arc_node = {
            let mut n = arc.to_node();
            let ids: Vec<serde_json::Value> = cluster
                .iter()
                .map(|m| serde_json::Value::from(m.id.clone()))
                .collect();
            n.metadata
                .insert("source_ids".into(), serde_json::Value::Array(ids));
            n.metadata.insert(
                "proof_count".into(),
                serde_json::Value::from(cluster.len() as u64),
            );
            n
        };
        let arc_sym = arc.symbol();
        // The sources move to the archival tier with the summary (content kept, nothing deleted):
        // archiving is idempotent, so the next pass does not fold the same memories again.
        let moved: Vec<Memory> = cluster
            .iter()
            .filter(|m| m.tier != Tier::Archival)
            .map(|m| {
                let mut m = m.clone();
                m.tier = Tier::Archival;
                m
            })
            .collect();
        let mut nodes = vec![arc_node];
        nodes.extend(moved.iter().map(Memory::to_node));
        self.store.upsert_nodes(&nodes)?;
        for m in &moved {
            self.ext.upsert(m)?; // keep the sidecar tier index in sync
        }
        let vec = self.embed_for(&arc.content);
        self.store.set_embedding(&arc_sym, &vec)?;
        self.ext.upsert(&arc)?; // keep the sidecar in sync — archive bypasses capture() (drift fix)
        let edges: Vec<Edge> = cluster
            .iter()
            .map(|m| Self::edge(&arc_sym, &m.symbol(), "derived_from"))
            .collect();
        self.store.upsert_edges(&edges)?;
        Ok(cluster.len())
    }

    /// Working memories in `scope` (+ descendants) past [`WORKING_TTL_SECS`] at `now`, by KIND in
    /// every tier (a `kind: working` capture without a tier lands Episodic) or by the Working tier.
    pub(crate) fn expired_working(&self, scope: &Scope, now: i64) -> Result<Vec<Memory>> {
        let ids = self.ext.aged_ids(
            &[
                Tier::Working,
                Tier::Episodic,
                Tier::Semantic,
                Tier::Procedural,
                Tier::Archival,
            ],
            now - WORKING_TTL_SECS,
        )?;
        Ok(self
            .hydrate(&ids)?
            .into_iter()
            .filter(|m| m.kind == MemKind::Working || m.tier == Tier::Working)
            .filter(|m| scope.is_ancestor_of(&m.scope))
            .collect())
    }

    /// Archive aged, low-salience memories in `scope` into self-contained T4 summary nodes (one per
    /// exact source scope + facet set), and drop Working memories older than [`WORKING_TTL_SECS`]
    /// (session scratch, whatever their salience; never summarized). **Model-free** (DEC-R).
    /// Returns sources archived plus working memories dropped.
    pub fn archive(
        &mut self,
        scope: &Scope,
        max_age_secs: i64,
        salience_floor: f64,
        now: i64,
    ) -> Result<usize> {
        // Indexed candidates: aged episodic/semantic rows (idx_mem_ext on tier+created_at), then
        // compute time-dependent salience in Rust on the bounded set (no stale snapshot).
        let ids = self
            .ext
            .aged_ids(&[Tier::Episodic, Tier::Semantic], now - max_age_secs)?;
        let cand: Vec<Memory> = self
            .hydrate(&ids)?
            .into_iter()
            .filter(|m| {
                !is_scratch(m) // scratch is dropped by its TTL, never archived
                    && scope.is_ancestor_of(&m.scope)
                    && (now - m.created_at) >= max_age_secs
                    && compute_salience(
                        &self.sal,
                        m.confidence(),
                        now - m.created_at,
                        m.access_count,
                    ) < salience_floor
            })
            .collect();
        let dropped = self.drop_expired_working(scope, now)?.len();
        // No agent summary in the convenience pass → the deterministic floor (option (b)).
        Ok(self.archive_cluster(scope, &cand, None, now)? + dropped)
    }

    /// Drop expired Working memories (see [`Self::expired_working`]), then repair archive summaries.
    ///
    /// Two invariants, enforced on every maintenance pass (idempotent):
    /// - no summary covers scratch: Working kind or tier is never archived;
    /// - no summary outlives a source: an Archive whose `source_ids` name a memory that no longer
    ///   exists (0.29.1 deleted expired scratch but kept 0.29.0's root summaries of it) or a Working
    ///   memory is removed, and rebuilt from its surviving durable sources at their exact scope and
    ///   facets; with no survivor it is only removed.
    ///
    /// Returns the removed node ids (sources + summaries) so the caller can clear cross-store edges.
    pub(crate) fn drop_expired_working(
        &mut self,
        scope: &Scope,
        now: i64,
    ) -> Result<Vec<SymbolId>> {
        let mut removed: Vec<SymbolId> = self
            .expired_working(scope, now)?
            .iter()
            .map(Memory::symbol)
            .collect();
        self.remove_memories(&removed)?;

        let all = self.all_memories()?;
        let by_id: std::collections::HashMap<&str, &Memory> =
            all.iter().map(|m| (m.id.as_str(), m)).collect();
        let mut stale: Vec<SymbolId> = Vec::new();
        let mut survivors: Vec<Memory> = Vec::new();
        for arc in all.iter().filter(|m| m.kind == MemKind::Archive) {
            let Some(node) = self.store.get_node(&arc.symbol())? else {
                continue;
            };
            let sources: Vec<String> = node
                .metadata
                .get("source_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let present: Vec<&Memory> = sources
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect();
            let durable: Vec<&Memory> =
                present.iter().copied().filter(|m| !is_scratch(m)).collect();
            if durable.len() == sources.len() {
                continue; // sound: every source exists and none is scratch
            }
            stale.push(arc.symbol());
            for m in durable {
                if !survivors.iter().any(|s| s.id == m.id) {
                    survivors.push(m.clone());
                }
            }
        }
        self.remove_memories(&stale)?;
        removed.extend(stale);
        // Root as the filter scope: every survivor is kept and lands at its own scope + facets.
        self.archive_cluster(&Scope::root(), &survivors, None, now)?;
        Ok(removed)
    }

    /// Hard-delete memory nodes from the graph + the sidecar index.
    fn remove_memories(&mut self, ids: &[SymbolId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let id_strs: Vec<String> = ids.iter().map(|s| s.0.clone()).collect();
        self.store.remove_nodes(ids)?;
        self.ext.remove(&id_strs)?;
        Ok(())
    }
}

/// Session scratch: Working kind or Working tier. Never distilled, never archived.
fn is_scratch(m: &Memory) -> bool {
    m.kind == MemKind::Working || m.tier == Tier::Working
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryEngine, ScopeFilter};
    use wicked_estate_memory_core::{MemKind, Memory, Scope, Tier};

    fn cap(
        eng: &mut MemoryEngine,
        kind: MemKind,
        tier: Tier,
        scope: &Scope,
        text: &str,
        now: i64,
    ) -> SymbolId {
        let m = Memory::new(kind, tier, scope.clone(), text, now);
        let s = m.symbol();
        eng.capture(&m).unwrap();
        s
    }

    #[test]
    fn reflect_creates_facts_and_entities() {
        // DEC-R re-point (T-B-DECR): `reflect` now distills via the deterministic floor
        // (`heuristic_extract` through `capture_facts(None)`), NOT a Reasoner auto-distill. This is
        // the option-(b) floor — ≥1 fact + entities from the heuristic ALONE, no model in the loop.
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("org:acme");
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "Alice prefers oat milk. She uses Stripe.",
            1,
        );
        let new = eng.reflect(&scope, 2).unwrap();
        assert!(
            new >= 1,
            "deterministic floor must distill ≥1 fact, got {new}"
        );
        let mems = eng.all_memories().unwrap();
        assert!(mems.iter().any(|m| m.kind == MemKind::Fact));
        assert!(
            mems.iter()
                .any(|m| m.kind == MemKind::Entity && m.content == "Alice")
        );
        // dedup: reflecting again creates no new facts.
        assert_eq!(eng.reflect(&scope, 3).unwrap(), 0);
    }

    #[test]
    fn capture_facts_uses_agent_distillation_when_supplied() {
        // DEC-R: the agent IS the reasoner. When `distilled[]` is supplied, `capture_facts` writes
        // exactly the agent's facts/entities — the deterministic floor is the fallback, not an
        // override. Proves the agent path is honoured (option-(b) is only the no-agent default).
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("org:acme");
        let ep = Memory::new(
            MemKind::Episode,
            Tier::Episodic,
            scope.clone(),
            "raw turn",
            1,
        );
        eng.capture(&ep).unwrap();
        let distilled = Extracted {
            facts: vec!["billing is handled by Stripe".into()],
            entities: vec!["Stripe".into()],
        };
        let n = eng.capture_facts(&ep, Some(distilled), 2).unwrap();
        assert_eq!(n, 1, "exactly the one agent-supplied fact is written");
        let mems = eng.all_memories().unwrap();
        assert!(
            mems.iter()
                .any(|m| m.kind == MemKind::Fact && m.content == "billing is handled by Stripe")
        );
        assert!(
            mems.iter()
                .any(|m| m.kind == MemKind::Entity && m.content == "Stripe")
        );
    }

    #[test]
    fn reinforce_raises_confidence() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let id = cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &scope,
            "billing uses Stripe",
            1,
        );
        let before = Memory::from_node(&eng_get(&eng, &id)).unwrap().confidence();
        for _ in 0..8 {
            eng.reinforce(&id, true, 2).unwrap();
        }
        let after = Memory::from_node(&eng_get(&eng, &id)).unwrap().confidence();
        assert!(
            after > before,
            "confidence should rise: {before} -> {after}"
        );
    }

    #[test]
    fn promote_creates_skill_when_reinforced() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let id = cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &scope,
            "retry with idempotency keys",
            1,
        );
        for _ in 0..10 {
            eng.reinforce(&id, true, 2).unwrap();
        }
        let n = eng.promote_skills(&scope, 5, 0.3, 3).unwrap();
        assert_eq!(n, 1);
        assert!(
            eng.all_memories()
                .unwrap()
                .iter()
                .any(|m| m.tier == Tier::Procedural)
        );
    }

    #[test]
    fn consolidate_runs_full_pass() {
        // DEC-R re-point (T-B-DECR): a full pass distills via the deterministic floor (no model).
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "Alice uses Stripe for billing",
            1,
        );
        let rep = eng.consolidate(&scope, 2).unwrap();
        assert!(rep.new_facts >= 1, "consolidate should distill facts");
        assert!(
            eng.all_memories()
                .unwrap()
                .iter()
                .any(|m| m.kind == MemKind::Fact)
        );
    }

    #[test]
    fn consolidate_loop_invokes_no_model() {
        // T-B-DAEMON-NOMODEL (B-6/B12): a COMMITTED test (not a grep) that a full `consolidate()`
        // pass writes the semantic tier from the DETERMINISTIC FLOOR only — no LLM/judgment model.
        //
        // The strongest possible proof is structural: the Reasoner seam is DELETED, so there is no
        // model to inject. We assert it BEHAVIOURALLY by content-provenance — every fact written by
        // the loop is byte-identical to `heuristic_extract` output for some source episode. A model
        // (abstractive/normalizing) would necessarily produce text the deterministic floor did not,
        // so a non-empty fact set that is a SUBSET of the floor's output proves the floor is the only
        // writer. (This is the "poison seam that panics if called" intent, realized by deletion +
        // exact-provenance rather than a stub that can drift.)
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        let episodes = [
            "Alice prefers oat milk. She uses Stripe for billing.",
            "Bob deploys on Fridays. The service runs on Kubernetes.",
        ];
        for (i, text) in episodes.iter().enumerate() {
            cap(
                &mut eng,
                MemKind::Episode,
                Tier::Episodic,
                &scope,
                text,
                1 + i as i64,
            );
        }
        let rep = eng.consolidate(&scope, 10).unwrap();
        assert!(
            rep.new_facts >= 1,
            "the loop must distill at least one fact"
        );

        // The exact set of fact strings the model-free floor would emit for these episodes.
        let mut floor_facts: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for text in episodes {
            for f in heuristic_extract(text).facts {
                floor_facts.insert(f);
            }
        }
        let written_facts: Vec<String> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == MemKind::Fact)
            .map(|m| m.content)
            .collect();
        assert!(!written_facts.is_empty(), "facts must have been written");
        for f in &written_facts {
            assert!(
                floor_facts.contains(f),
                "fact {f:?} was NOT produced by the deterministic floor — a model is in the loop"
            );
        }
    }

    #[test]
    fn entity_merge_dedupes_near_duplicates() {
        // PR-6: "Stripe" and "stripe" (and a fuzzy variant) collapse to one entity node.
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "We use Stripe.",
            1,
        );
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "Billing via stripe.",
            2,
        );
        eng.reflect(&scope, 3).unwrap();
        let stripe_entities = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == MemKind::Entity && m.content.to_lowercase().contains("stripe"))
            .count();
        assert_eq!(stripe_entities, 1, "Stripe/stripe must merge to ONE entity");
    }

    #[test]
    fn entity_merge_respects_tenant_isolation() {
        // FATAL-fix regression: entity reuse must follow scope INHERITANCE, never leak across tenants.
        // (1) cross-tenant non-merge even when reflecting at root.
        let mut eng = MemoryEngine::in_memory().unwrap();
        let acme = Scope::parse("org:acme");
        let comp = Scope::parse("org:competitor");
        cap(
            &mut eng,
            MemKind::Entity,
            Tier::Semantic,
            &comp,
            "Stripe",
            0,
        ); // competitor already knows Stripe
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &acme,
            "We use Stripe heavily.",
            1,
        );
        eng.reflect(&Scope::root(), 2).unwrap(); // maintenance at root must NOT merge across tenants
        let stripe: Vec<_> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == MemKind::Entity && m.content == "Stripe")
            .collect();
        assert_eq!(
            stripe.len(),
            2,
            "acme + competitor keep SEPARATE Stripe entities (no cross-tenant merge)"
        );
        assert!(stripe.iter().any(|m| m.scope.as_path() == "org:acme"));
        assert!(stripe.iter().any(|m| m.scope.as_path() == "org:competitor"));
    }

    #[test]
    fn entity_merge_reuses_ancestor_entity() {
        // (3) an entity at an ANCESTOR scope IS reused by a descendant episode (inheritance).
        let mut eng = MemoryEngine::in_memory().unwrap();
        let org = Scope::parse("org:acme");
        let team = Scope::parse("org:acme/team:a");
        cap(&mut eng, MemKind::Entity, Tier::Semantic, &org, "Stripe", 0); // org-level entity
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &team,
            "team uses Stripe.",
            1,
        );
        eng.reflect(&team, 2).unwrap();
        let stripe = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == MemKind::Entity && m.content == "Stripe")
            .count();
        assert_eq!(
            stripe, 1,
            "descendant episode reuses the ancestor's Stripe entity (no duplicate)"
        );
    }

    #[test]
    fn recall_floor_forgets_low_salience() {
        // FR-7: with a recall floor, an old, unreinforced, never-accessed memory is excluded.
        let mut eng = MemoryEngine::in_memory().unwrap().with_recall_floor(0.4);
        let scope = Scope::root();
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "ancient trivial note about the system",
            0,
        );
        let far = 1000 * 86_400; // ~3 years later → decayed salience well below 0.4
        let out = eng
            .recall("system note", ScopeFilter::Ancestors(&scope), &[], 500, far)
            .unwrap();
        assert!(
            out.is_empty(),
            "decayed memory below the floor must be forgotten from recall"
        );
        // without a floor it would still surface:
        let mut eng2 = MemoryEngine::in_memory().unwrap();
        cap(
            &mut eng2,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "ancient trivial note about the system",
            0,
        );
        assert!(
            !eng2
                .recall("system note", ScopeFilter::Ancestors(&scope), &[], 500, far)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn archive_creates_t4_node() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        // old, never-accessed, never-reinforced → low salience.
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "old log line one",
            0,
        );
        cap(
            &mut eng,
            MemKind::Episode,
            Tier::Episodic,
            &scope,
            "old log line two",
            0,
        );
        let now = 400 * 86_400; // ~400 days later
        let n = eng.archive(&scope, 30 * 86_400, 0.5, now).unwrap();
        assert_eq!(n, 2, "both old low-salience episodes archived");
        assert!(
            eng.all_memories()
                .unwrap()
                .iter()
                .any(|m| m.tier == Tier::Archival)
        );
    }

    #[test]
    fn archive_drops_expired_working_and_is_idempotent() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:demo");
        let t0 = 10_000;
        cap(
            &mut eng,
            MemKind::Working,
            Tier::Working,
            &scope,
            "WIP: turn count 4",
            t0,
        );
        let now = t0 + WORKING_TTL_SECS;
        cap(
            &mut eng,
            MemKind::Working,
            Tier::Working,
            &scope,
            "WIP: fresh task",
            now,
        );

        // Salience floor 0 and a far max age: only the TTL can select the Working memory.
        assert_eq!(eng.archive(&scope, 10 * 365 * 86_400, 0.0, now).unwrap(), 1);
        let tiers = |eng: &MemoryEngine, text: &str| -> Vec<Tier> {
            eng.all_memories()
                .unwrap()
                .into_iter()
                .filter(|m| m.content == text && m.kind == MemKind::Working)
                .map(|m| m.tier)
                .collect()
        };
        assert!(
            tiers(&eng, "WIP: turn count 4").is_empty(),
            "expired working is dropped"
        );
        assert_eq!(
            tiers(&eng, "WIP: fresh task"),
            [Tier::Working],
            "fresh working untouched"
        );
        // A second pass finds nothing new, and no summary of the scratch was written.
        assert_eq!(eng.archive(&scope, 10 * 365 * 86_400, 0.0, now).unwrap(), 0);
        assert_eq!(
            eng.count().unwrap(),
            1,
            "only the fresh working memory remains"
        );
    }

    #[test]
    fn consolidate_archive_pass_is_idempotent() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::root();
        cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &scope,
            "old fact one",
            0,
        );
        cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &scope,
            "old fact two",
            0,
        );
        let now = 400 * 86_400;
        let first = eng.consolidate(&scope, now).unwrap();
        assert_eq!(first.archived, 2);
        let second = eng.consolidate(&scope, now + 1).unwrap();
        assert_eq!(
            second.archived, 0,
            "already-archived sources are not folded again"
        );
    }

    // ── w9-memfix2 (codex post-merge review): consolidation never widens visibility ──

    fn alice() -> wicked_estate_memory_core::Facets {
        wicked_estate_memory_core::Facets::try_from_map([("user", "alice")]).unwrap()
    }

    fn contents_visible_from(
        eng: &MemoryEngine,
        scope: &Scope,
        query: &str,
        now: i64,
    ) -> Vec<String> {
        eng.recall(query, ScopeFilter::Ancestors(scope), &[], 100_000, now)
            .unwrap()
            .into_iter()
            .map(|r| r.content)
            .collect()
    }

    #[test]
    fn root_archive_keeps_each_project_in_its_own_scope() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let (a, b) = (Scope::parse("project:a"), Scope::parse("project:b"));
        cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &a,
            "zebra ledger secret of project a",
            0,
        );
        cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &b,
            "yak ledger note of project b",
            0,
        );
        let now = 400 * 86_400;
        eng.archive(&Scope::root(), 30 * 86_400, 0.5, now).unwrap();
        let leaked = contents_visible_from(&eng, &b, "zebra ledger secret", now);
        assert!(
            leaked.iter().all(|c| !c.contains("zebra")),
            "project a content recallable from project b after a root archive: {leaked:?}"
        );
        // Erasing project a leaves no copy of its content behind.
        eng.erase("project:a").unwrap();
        let left: Vec<String> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .filter(|c| c.contains("zebra"))
            .collect();
        assert!(left.is_empty(), "erase(project:a) left a copy: {left:?}");
    }

    #[test]
    fn archive_summary_keeps_its_sources_facets() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let root = Scope::root();
        let m = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            root.clone(),
            "alice private quokka plan",
            0,
        )
        .with_facets(alice());
        eng.capture(&m).unwrap();
        let now = 400 * 86_400;
        eng.archive(&root, 30 * 86_400, 0.5, now).unwrap();
        let seen = contents_visible_from(&eng, &root, "alice private quokka plan", now);
        assert!(
            seen.iter().all(|c| !c.contains("quokka")),
            "a {{user:alice}} memory became recallable without the facet: {seen:?}"
        );
    }

    #[test]
    fn promoted_skill_and_reflected_fact_keep_facets() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let root = Scope::root();
        let fact = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            root.clone(),
            "alice deploys with the blue lever",
            1,
        )
        .with_facets(alice());
        let fid = fact.symbol();
        eng.capture(&fact).unwrap();
        for _ in 0..5 {
            eng.reinforce(&fid, true, 2).unwrap();
        }
        eng.promote_skills(&root, 5, 0.0, 3).unwrap();
        let ep = Memory::new(
            MemKind::Episode,
            Tier::Episodic,
            root.clone(),
            "Alice uses Postgres for billing.",
            4,
        )
        .with_facets(alice());
        eng.capture(&ep).unwrap();
        eng.reflect(&root, 5).unwrap();
        let derived: Vec<Memory> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| {
                matches!(m.kind, MemKind::Skill) || (m.kind == MemKind::Fact && m.id != fact.id)
            })
            .collect();
        assert!(
            !derived.is_empty(),
            "expected a promoted skill and a reflected fact"
        );
        for m in &derived {
            assert_eq!(
                m.facets,
                alice(),
                "derived {:?} dropped its source's facets",
                m.kind
            );
        }
    }

    #[test]
    fn expired_working_memory_is_dropped_not_archived() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let scope = Scope::parse("project:demo");
        let t0 = 10_000;
        cap(
            &mut eng,
            MemKind::Working,
            Tier::Working,
            &scope,
            "WIP walrus snapshot",
            t0,
        );
        let now = t0 + WORKING_TTL_SECS;
        eng.archive(&scope, 10 * 365 * 86_400, 0.0, now).unwrap();
        let left: Vec<String> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .filter(|c| c.contains("walrus"))
            .collect();
        assert!(
            left.is_empty(),
            "expired working memory survived maintenance: {left:?}"
        );
        assert!(contents_visible_from(&eng, &scope, "WIP walrus snapshot", now + 1).is_empty());
    }

    // ── w9-memfix3 (codex re-review of #329) ──────────────────────────────────────

    fn holds(eng: &MemoryEngine, needle: &str) -> Vec<String> {
        eng.all_memories()
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .filter(|c| c.contains(needle))
            .collect()
    }

    /// A 0.29.0-style legacy archive: a root-scoped, unfaceted summary over `sources`.
    fn legacy_summary(eng: &mut MemoryEngine, sources: &[&Memory], text: &str, now: i64) {
        let arc = Memory::new(MemKind::Archive, Tier::Archival, Scope::root(), text, now);
        let mut node = arc.to_node();
        let ids: Vec<serde_json::Value> = sources
            .iter()
            .map(|m| serde_json::Value::from(m.id.clone()))
            .collect();
        node.metadata
            .insert("source_ids".into(), serde_json::Value::Array(ids));
        eng.store.upsert_nodes(&[node]).unwrap();
        eng.ext.upsert(&arc).unwrap();
    }

    #[test]
    fn legacy_summary_of_expired_working_is_removed() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let a = Scope::parse("project:a");
        let t0 = 10_000;
        let wip = Memory::new(
            MemKind::Working,
            Tier::Archival,
            a.clone(),
            "WIP otter secret",
            t0,
        );
        eng.capture(&wip).unwrap();
        legacy_summary(&mut eng, &[&wip], "WIP otter secret", t0 + 1);
        let now = t0 + WORKING_TTL_SECS;
        eng.consolidate(&a, now).unwrap();
        assert!(
            holds(&eng, "otter").is_empty(),
            "legacy summary of expired WIP survived"
        );
    }

    #[test]
    fn mixed_legacy_summary_is_rebuilt_from_survivors_at_their_scope() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let (a, b) = (Scope::parse("project:a"), Scope::parse("project:b"));
        let t0 = 10_000;
        let wip = Memory::new(
            MemKind::Working,
            Tier::Archival,
            a.clone(),
            "WIP heron scratch",
            t0,
        );
        let keep = Memory::new(
            MemKind::Fact,
            Tier::Archival,
            b.clone(),
            "bison ledger rule",
            t0,
        );
        eng.capture(&wip).unwrap();
        eng.capture(&keep).unwrap();
        legacy_summary(
            &mut eng,
            &[&wip, &keep],
            "WIP heron scratch bison ledger rule",
            t0 + 1,
        );
        eng.consolidate(&Scope::root(), t0 + WORKING_TTL_SECS)
            .unwrap();
        assert!(
            holds(&eng, "heron").is_empty(),
            "expired WIP text survived in a summary"
        );
        let roots: Vec<String> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == MemKind::Archive && m.scope.as_path() == Scope::root().as_path())
            .map(|m| m.content)
            .collect();
        assert!(roots.is_empty(), "a root summary remains: {roots:?}");
        assert!(
            eng.all_memories()
                .unwrap()
                .iter()
                .any(|m| m.kind == MemKind::Archive && m.scope.as_path() == b.as_path()),
            "the surviving source is re-summarized at its own scope"
        );
    }

    #[test]
    fn working_kind_in_any_tier_expires_and_is_never_distilled() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let s = Scope::parse("project:demo");
        let t0 = 10_000;
        cap(
            &mut eng,
            MemKind::Working,
            Tier::Episodic,
            &s,
            "Badger notes: Postgres runs billing.",
            t0,
        );
        let fresh = t0 + WORKING_TTL_SECS;
        cap(
            &mut eng,
            MemKind::Working,
            Tier::Episodic,
            &s,
            "Mole notes: Redis runs sessions.",
            fresh,
        );
        eng.consolidate(&s, fresh).unwrap();
        assert!(
            holds(&eng, "Badger").is_empty() && holds(&eng, "Postgres").is_empty(),
            "expired working-kind memory (episodic tier) or a derivative survived"
        );
        assert_eq!(
            holds(&eng, "Redis").len(),
            1,
            "fresh scratch is kept but never distilled"
        );
    }

    #[test]
    fn supplied_summary_is_ignored_when_the_cluster_was_filtered() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let (a, b) = (Scope::parse("project:a"), Scope::parse("project:b"));
        let am = Memory::new(MemKind::Fact, Tier::Semantic, a.clone(), "aardvark rule", 0);
        let bm = Memory::new(MemKind::Fact, Tier::Semantic, b.clone(), "ferret rule", 0);
        eng.capture(&am).unwrap();
        eng.capture(&bm).unwrap();
        eng.archive_cluster(&a, &[am, bm], Some("aardvark rule; ferret rule".into()), 5)
            .unwrap();
        let leaked: Vec<String> = eng
            .all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.scope.as_path() == a.as_path() && m.content.contains("ferret"))
            .map(|m| m.content)
            .collect();
        assert!(
            leaked.is_empty(),
            "B's content landed in A's archive: {leaked:?}"
        );
    }

    // ── w9-memfix3 r2 (codex review of #331) ──────────────────────────────────────

    fn archives_holding(eng: &MemoryEngine, needle: &str) -> Vec<String> {
        eng.all_memories()
            .unwrap()
            .into_iter()
            .filter(|m| m.kind == MemKind::Archive && m.content.contains(needle))
            .map(|m| m.content)
            .collect()
    }

    #[test]
    fn orphaned_legacy_summary_is_removed_by_maintenance() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let gone = Memory::new(
            MemKind::Working,
            Tier::Working,
            Scope::parse("project:a"),
            "x",
            1,
        );
        // 0.29.1 already deleted `gone`; only its root summary is left.
        legacy_summary(&mut eng, &[&gone], "WIP lynx scratch notes", 2);
        eng.consolidate(&Scope::root(), 4).unwrap();
        assert!(
            holds(&eng, "lynx").is_empty(),
            "maintenance must remove the orphaned summary"
        );
    }

    #[test]
    fn a_sound_archive_stays_recallable() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let root = Scope::root();
        cap(
            &mut eng,
            MemKind::Fact,
            Tier::Semantic,
            &root,
            "old gecko fact one",
            0,
        );
        let now = 400 * 86_400;
        eng.archive(&root, 30 * 86_400, 0.5, now).unwrap();
        eng.consolidate(&root, now + 1).unwrap();
        assert!(
            !archives_holding(&eng, "gecko").is_empty(),
            "sound archive kept by repair"
        );
        assert!(!contents_visible_from(&eng, &root, "old gecko fact one", now + 2).is_empty());
    }

    #[test]
    fn scoped_cleanup_never_rearchives_sibling_scratch() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let (a, b) = (Scope::parse("project:a"), Scope::parse("project:b"));
        let t0 = 10_000;
        let wa = Memory::new(
            MemKind::Working,
            Tier::Working,
            a.clone(),
            "WIP-A viper",
            t0,
        );
        let wb = Memory::new(
            MemKind::Working,
            Tier::Working,
            b.clone(),
            "WIP-B cobra",
            t0,
        );
        eng.capture(&wa).unwrap();
        eng.capture(&wb).unwrap();
        legacy_summary(&mut eng, &[&wa, &wb], "WIP-A viper WIP-B cobra", t0 + 1);
        eng.consolidate(&a, t0 + WORKING_TTL_SECS).unwrap();
        assert!(
            archives_holding(&eng, "cobra").is_empty(),
            "sibling scratch re-archived"
        );
    }

    #[test]
    fn archive_cluster_never_summarizes_working() {
        let mut eng = MemoryEngine::in_memory().unwrap();
        let root = Scope::root();
        let f = Memory::new(
            MemKind::Fact,
            Tier::Semantic,
            root.clone(),
            "durable mongoose rule",
            0,
        );
        let w = Memory::new(
            MemKind::Working,
            Tier::Working,
            root.clone(),
            "scratch tapir note",
            0,
        );
        eng.capture(&f).unwrap();
        eng.capture(&w).unwrap();
        eng.archive_cluster(&root, &[f, w], Some("mongoose and tapir".into()), 5)
            .unwrap();
        assert!(
            archives_holding(&eng, "tapir").is_empty(),
            "working content archived"
        );
    }

    fn eng_get(eng: &MemoryEngine, id: &SymbolId) -> wicked_estate_core::Node {
        eng.node(id).unwrap().unwrap()
    }
}
