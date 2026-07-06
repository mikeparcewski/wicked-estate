//! Deterministic fuzzy name matching for entity-merge (FR-8 / PR-6), the tier between exact-match
//! and embedding/LLM adjudication.
//!
//! Uses **character trigram Jaccard similarity** — deterministic, zero-dependency, and correct for
//! the scope-bounded entity sets entity-merge compares within. (`gaoya` MinHash/LSH is the drop-in
//! swap when an entity set grows large enough that sublinear candidate retrieval matters; the merge
//! pipeline's contract — return likely-same candidates above a threshold — is unchanged by that swap.)
//!
//! Pipeline position (orchestrated in the engine, DEC-R): exact-CI → **fuzzy (here)** → the agent's
//! adjudication. `merge_candidates` returns the fuzzy hits as a HINT; the agent/skill decides the
//! residual. Cheap deterministic tiers cut the candidate set before the agent is consulted.

use std::collections::HashSet;

/// Normalize a name for matching: lowercase, collapse non-alphanumerics to single spaces, trim.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true; // trims leading
    for c in s.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Character trigrams of the normalized string (padded so short strings still produce shingles).
fn trigrams(s: &str) -> HashSet<[char; 3]> {
    let norm = normalize(s);
    let padded: Vec<char> = format!("  {norm} ").chars().collect();
    let mut set = HashSet::new();
    if padded.len() < 3 {
        return set;
    }
    for w in padded.windows(3) {
        set.insert([w[0], w[1], w[2]]);
    }
    set
}

/// Jaccard similarity of two strings' trigram sets, in [0,1]. Identical (post-normalize) → 1.0.
pub fn jaccard(a: &str, b: &str) -> f64 {
    let na = normalize(a);
    // Identical non-empty normalized forms → 1.0. Two strings that BOTH normalize to empty (e.g.
    // all-punctuation) must NOT be treated as the same entity (avoid false merges).
    if !na.is_empty() && na == normalize(b) {
        return 1.0;
    }
    let (ta, tb) = (trigrams(a), trigrams(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    inter / union
}

/// Indices of `candidates` whose trigram-Jaccard with `target` is `>= threshold`, best first.
/// The deterministic fuzzy tier of entity-merge.
pub fn fuzzy_candidates(target: &str, candidates: &[String], threshold: f64) -> Vec<usize> {
    let mut scored: Vec<(usize, f64)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, jaccard(target, c)))
        .filter(|(_, s)| *s >= threshold)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_canonicalizes() {
        assert_eq!(normalize("  Stripe, Inc. "), "stripe inc");
        assert_eq!(normalize("PayPal"), "paypal");
    }

    #[test]
    fn jaccard_identical_and_disjoint() {
        assert_eq!(jaccard("Stripe", "stripe"), 1.0);
        assert_eq!(jaccard("Stripe", " STRIPE "), 1.0);
        assert!(jaccard("Stripe", "Square") < 0.5);
    }

    #[test]
    fn jaccard_catches_near_dupes() {
        // typo / suffix variants score high; unrelated names score low.
        assert!(jaccard("Stripe Inc", "Stripe Incorporated") > 0.4);
        assert!(jaccard("PostgreSQL", "Postgres") > 0.3);
        assert!(jaccard("Alice", "Bob") < 0.2);
    }

    #[test]
    fn fuzzy_candidates_ranks_and_thresholds() {
        let cands = vec![
            "Square".to_string(),
            "stripe".to_string(),
            "Stripe Inc".to_string(),
        ];
        let hits = fuzzy_candidates("Stripe", &cands, 0.5);
        assert_eq!(hits[0], 1, "exact-normalized 'stripe' ranks first");
        assert!(!hits.contains(&0), "Square is below threshold");
    }
}
