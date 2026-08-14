//! Deterministic fuzzy name matching for entity-merge (FR-8 / PR-6), the tier between exact-match
//! and embedding/LLM adjudication.
//!
//! Two retrieval paths behind the same public API:
//! - **O(n) trigram-Jaccard** — for candidate sets below `LSH_THRESHOLD` (200). No extra state,
//!   exact Jaccard similarity.
//! - **gaoya MinHash/LSH** — for candidate sets ≥ `LSH_THRESHOLD`. Sublinear retrieval at the cost
//!   of approximate similarity; the same threshold semantics, different algorithm.
//!
//! Pipeline position (orchestrated in the engine, DEC-R): exact-CI → **fuzzy (here)** → the agent's
//! adjudication. `merge_candidates` returns the fuzzy hits as a HINT; the agent/skill decides the
//! residual. Cheap deterministic tiers cut the candidate set before the agent is consulted.

use gaoya::minhash::{MinHashIndex, MinHasher, MinHasher32, calculate_minhash_params};
use std::collections::HashSet;

const LSH_THRESHOLD: usize = 200;
const LSH_NUM_HASHES: usize = 200;

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
/// Returns empty when `normalize(s)` is empty to avoid false merges on all-punctuation inputs.
fn trigrams(s: &str) -> HashSet<[char; 3]> {
    let norm = normalize(s);
    if norm.is_empty() {
        return HashSet::new();
    }
    let padded: Vec<char> = format!("  {norm} ").chars().collect();
    let mut set = HashSet::new();
    for w in padded.windows(3) {
        set.insert([w[0], w[1], w[2]]);
    }
    set
}

/// Sorted, deduplicated character trigrams as a `Vec` for MinHash iteration.
/// Derived from `trigrams()` so both paths share the same normalization and empty-guard.
fn trigrams_vec(s: &str) -> Vec<[char; 3]> {
    let mut out: Vec<[char; 3]> = trigrams(s).into_iter().collect();
    out.sort_unstable();
    out
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

/// MinHash/LSH candidate retrieval for large sets. Uses gaoya for sublinear lookup.
/// LSH is candidate-generation only: every hit is re-scored with exact `jaccard()` to enforce
/// the documented threshold semantics (MinHash similarity estimates can exceed the true Jaccard).
fn lsh_candidates(target: &str, candidates: &[String], threshold: f64) -> Vec<usize> {
    let (num_bands, band_width) = calculate_minhash_params(threshold, LSH_NUM_HASHES);
    let hasher = MinHasher32::new(num_bands * band_width);
    let mut index: MinHashIndex<u32, usize> = MinHashIndex::new(num_bands, band_width, threshold);
    for (i, c) in candidates.iter().enumerate() {
        index.insert(i, hasher.create_signature(trigrams_vec(c).into_iter()));
    }
    let query_sig = hasher.create_signature(trigrams_vec(target).into_iter());
    let mut results: Vec<(usize, f64)> = index
        .query_owned_return_similarity(&query_sig)
        .into_iter()
        .filter_map(|(i, _)| {
            let exact = jaccard(target, &candidates[i]);
            if exact >= threshold {
                Some((i, exact))
            } else {
                None
            }
        })
        .collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.into_iter().map(|(i, _)| i).collect()
}

/// Indices of `candidates` whose trigram similarity with `target` is `>= threshold`, best first.
/// Dispatches to O(n) trigram-Jaccard below `LSH_THRESHOLD` candidates, LSH above it.
pub fn fuzzy_candidates(target: &str, candidates: &[String], threshold: f64) -> Vec<usize> {
    if candidates.len() < LSH_THRESHOLD {
        let mut scored: Vec<(usize, f64)> = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (i, jaccard(target, c)))
            .filter(|(_, s)| *s >= threshold)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(i, _)| i).collect()
    } else {
        lsh_candidates(target, candidates, threshold)
    }
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

    #[test]
    fn lsh_path_finds_exact_match_in_large_set() {
        // 250 candidates crosses LSH_THRESHOLD (200), exercising the gaoya path.
        let mut cands: Vec<String> = (0..249).map(|i| format!("unrelated_entity_{i}")).collect();
        cands.push("stripe".to_string()); // index 249
        assert_eq!(cands.len(), 250);
        let hits = fuzzy_candidates("Stripe", &cands, 0.5);
        assert!(
            hits.contains(&249),
            "LSH path must find normalized-identical 'stripe'"
        );
    }
}
