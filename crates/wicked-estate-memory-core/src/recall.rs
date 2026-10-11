//! Recall: RRF fusion + reranking + token-budget assembly (the explicit formula, DESIGN §9).
//!
//! Pure logic over already-retrieved candidates so it is deterministic + unit-testable. The store
//! wiring (FTS ∪ vector ∪ graph-traverse candidate generation) is the L0-integration crate's job;
//! this module owns the *fusion + rerank + budget* that decide the relevant slice.

use crate::Tier;
use wicked_estate_core::SymbolId;

/// Reciprocal-rank fusion (Cormack/Clarke; k=60). `lists` are ranked id lists from each retriever
/// (keyword / vector / graph). Returns ids with fused score, descending.
pub fn rrf_fuse(lists: &[Vec<SymbolId>], k: f64) -> Vec<(SymbolId, f64)> {
    use std::collections::HashMap;
    let mut acc: HashMap<SymbolId, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *acc.entry(id.clone()).or_insert(0.0) += 1.0 / (k + (rank as f64) + 1.0);
        }
    }
    let mut out: Vec<_> = acc.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Facet-specificity boost weight (DES-MEM-FACETED-001 §4.3): a memory that matches `n` intent
/// facets is boosted by `(1 + β·n)`. A conservative BOOST, not a hard primary sort — a highly
/// relevant global (0-facet) memory must not be buried under a marginally-relevant faceted one.
pub const FACET_SPECIFICITY_BETA: f64 = 0.25;

/// A recall candidate with the signals needed to rerank + budget it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: SymbolId,
    pub content: String,
    pub tier: Tier,
    pub rrf: f64,
    pub recency: f64,  // [0,1] from decay()
    pub salience: f64, // [0,1]
    /// Count of intent facets this memory matched (its specificity; 0 for unfaceted memories or an
    /// empty intent). Folded into `final_score` as the `(1 + β·specificity)` boost.
    pub facet_specificity: usize,
}

impl Candidate {
    /// Final rerank score (DESIGN §9 step 4): rrf × tier_weight × recency × (1 + α·salience),
    /// with the DES-MEM-FACETED-001 §4.3 specificity boost `(1 + β·facet_specificity)` folded in.
    pub fn final_score(&self, alpha: f64) -> f64 {
        self.rrf
            * self.tier.weight()
            * self.recency
            * (1.0 + alpha * self.salience)
            * (1.0 + FACET_SPECIFICITY_BETA * self.facet_specificity as f64)
    }
    /// Rough token cost (≈ chars/4) for budgeting.
    pub fn token_cost(&self) -> usize {
        (self.content.len() / 4).max(1)
    }
}

/// Greedy token-budgeted assembly (DESIGN §9 step 5). Adds by descending `final_score` until
/// `token_budget` is exhausted; over-budget prune order = lowest-first (a natural consequence of
/// greedy-by-score). No tier gets a reserved slot: a Working item is packed only on its score.
/// Returns the selected candidates in final-score order.
pub fn budget_pack(mut cands: Vec<Candidate>, token_budget: usize, alpha: f64) -> Vec<Candidate> {
    cands.sort_by(|a, b| {
        b.final_score(alpha)
            .partial_cmp(&a.final_score(alpha))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out: Vec<Candidate> = Vec::new();
    let mut used = 0usize;
    for c in cands {
        let cost = c.token_cost();
        if used + cost <= token_budget {
            used += cost;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, tier: Tier, rrf: f64, chars: usize) -> Candidate {
        Candidate {
            id: SymbolId(id.into()),
            content: "x".repeat(chars),
            tier,
            rrf,
            recency: 1.0,
            salience: 0.5,
            facet_specificity: 0,
        }
    }

    #[test]
    fn budget_pack_reserves_no_slot_for_working() {
        // Two strong semantic hits fill the budget; a weak Working item must not evict one.
        let cands = vec![
            cand("s1", Tier::Semantic, 0.9, 40),
            cand("s2", Tier::Semantic, 0.8, 40),
            cand("w", Tier::Working, 0.1, 40),
        ];
        let out = budget_pack(cands, 20, 0.5);
        let ids: Vec<&str> = out.iter().map(|c| c.id.0.as_str()).collect();
        assert_eq!(ids, ["s1", "s2"]);
    }
}
