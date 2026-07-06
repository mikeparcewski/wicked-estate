//! Deterministic distillation heuristics — the **model-free floor** (DEC-R).
//!
//! DEC-R recast (`BUILD-SPEC.md` §3): wicked-memory has **no `Reasoner` seam**. The engine is
//! model-free; the agent IS the reasoner. Judgment-consolidation (LLM extraction / dedup
//! adjudication / abstractive summarization) is the **agent's** job, supplied as `distilled[]` to
//! `capture_facts` / `archive_cluster`. What survives in-crate is the **deterministic extractive
//! floor** so the engine is *not inert without an agent* (option (b), the RESOLVED sub-decision):
//!
//! - [`heuristic_extract`]   — sentence-split facts + capitalized-token entities (the default
//!   distillation `capture_facts` applies when no agent `distilled[]` is given).
//! - [`heuristic_summary`]   — extractive truncation (the default `archive_cluster` applies).
//! - [`heuristic_same_entity`] — case-insensitive exact match (the `merge_candidates` confirm step;
//!   the agent/skill decides the residual fuzzy hits — `merge_candidates` only HINTS).
//!
//! None of these calls a model. The corruption-safety property holds by construction: absent an
//! agent, the floor writes deterministic extractive output; it never fabricates.

use serde::{Deserialize, Serialize};

/// A fact/entity distillation result. Produced by the deterministic floor ([`heuristic_extract`])
/// or supplied by the agent as `distilled[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extracted {
    pub facts: Vec<String>,
    pub entities: Vec<String>,
}

// ── shared deterministic heuristics (the model-free floor) ────────────────────────────────────────

fn truncate_on_boundary(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Deterministic extractive distillation (the model-free default for `capture_facts`). Splits the
/// text into sentence-ish facts and harvests capitalized tokens as entity mentions. No model.
pub fn heuristic_extract(text: &str) -> Extracted {
    let facts = text
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| s.len() > 3)
        .map(str::to_string)
        .collect();
    let entities = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            let mut ch = w.chars();
            matches!(ch.next(), Some(c) if c.is_uppercase()) && w.len() > 1
        })
        .map(str::to_string)
        .collect();
    Extracted { facts, entities }
}

/// Deterministic same-entity check (case-insensitive exact). The `merge_candidates` confirm step:
/// fuzzy candidates beyond an exact match are a HINT the agent/skill adjudicates (DEC-R).
pub fn heuristic_same_entity(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Deterministic extractive summary (the model-free default for `archive_cluster`): join + truncate
/// on a char boundary, never abstractive. Abstractive summarization is the agent's job.
pub fn heuristic_summary(items: &[&str], max_chars: usize) -> String {
    truncate_on_boundary(&items.join("; "), max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_extract_facts_and_entities() {
        let e = heuristic_extract("Alice prefers oat milk. She uses Stripe for billing.");
        assert!(e.facts.len() >= 2);
        assert!(e.entities.iter().any(|x| x == "Alice"));
        assert!(e.entities.iter().any(|x| x == "Stripe"));
    }

    #[test]
    fn heuristic_same_entity_is_case_insensitive_exact() {
        assert!(heuristic_same_entity("Stripe", " stripe "));
        assert!(!heuristic_same_entity("Stripe", "Square"));
    }

    #[test]
    fn heuristic_summary_truncates_on_boundary() {
        let s = heuristic_summary(&["abcdef", "ghijkl"], 5);
        assert!(s.chars().count() <= 6);
    }
}
