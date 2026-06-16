//! Semantic clustering over embedding vectors.
//!
//! Groups symbols by **what they do** (embedding proximity) rather than what they call. This
//! bridges vocabularies a call graph can't — a Kafka producer and a Pulsar producer embed near
//! each other and land in one cluster even with zero shared edges.
//!
//! # Status / seam
//!
//! [`semantic_clusters`] is the fixed entry point: it takes the full `(SymbolId, vector)` set
//! (from `VectorStore::all_embeddings`) and a [`SemanticClusterParams`], and returns clusters. The
//! algorithm body (k-means and DBSCAN) is owned by the semantic-clustering chunk; the **stub
//! returns an empty `Vec`** so nothing downstream silently receives a wrong partition before the
//! backend lands. [`cosine_distance`] is real and shared by both algorithms.
//!
//! Quality note: meaningful clusters require *semantic* embeddings (`--embeddings` with the
//! `fastembed` feature). The zero-dependency `HashEmbedder` is a bag-of-words hash — clustering its
//! vectors yields noise. Callers should warn when the active embedder is hash-only.

use wicked_estate_core::SymbolId;

/// Which clustering algorithm [`semantic_clusters`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterAlgo {
    /// Lloyd's k-means: partitions into exactly `k` clusters. Fast, but `k` must be chosen.
    KMeans,
    /// DBSCAN: density-based, discovers cluster count from `eps`/`min_pts`. No `k` needed;
    /// points in no dense region are left unclustered (noise).
    Dbscan,
}

/// Tuning knobs for [`semantic_clusters`].
#[derive(Debug, Clone)]
pub struct SemanticClusterParams {
    /// Algorithm to run.
    pub algorithm: ClusterAlgo,
    /// k-means: number of clusters. Ignored by DBSCAN.
    pub k: usize,
    /// DBSCAN: neighbourhood radius in cosine-distance space (`0.0..=2.0`). Ignored by k-means.
    pub eps: f32,
    /// DBSCAN: minimum points to form a dense region. Ignored by k-means.
    pub min_pts: usize,
    /// Maximum iterations (k-means convergence cap).
    pub max_iter: usize,
}

impl Default for SemanticClusterParams {
    fn default() -> Self {
        Self {
            algorithm: ClusterAlgo::Dbscan,
            k: 16,
            eps: 0.25,
            min_pts: 3,
            max_iter: 50,
        }
    }
}

/// Cosine distance `1 − cos(a, b)` in `[0, 2]`. Returns `1.0` (max ambiguity) if either vector has
/// zero norm or the dimensions differ. Real and shared by both clustering backends.
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 1.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 1.0;
    }
    1.0 - dot / (na.sqrt() * nb.sqrt())
}

/// Cluster symbols by embedding proximity, largest cluster first.
///
/// **STUB**: returns an empty `Vec` until the k-means/DBSCAN backend lands. [`cosine_distance`] and
/// the parameter types are stable; only the assignment loop is pending.
pub fn semantic_clusters(
    _embeddings: &[(SymbolId, Vec<f32>)],
    _params: &SemanticClusterParams,
) -> Vec<Vec<SymbolId>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_distance_basics() {
        // Identical direction → ~0 distance.
        assert!(cosine_distance(&[1.0, 0.0], &[2.0, 0.0]).abs() < 1e-6);
        // Orthogonal → 1.0.
        assert!((cosine_distance(&[1.0, 0.0], &[0.0, 1.0]) - 1.0).abs() < 1e-6);
        // Opposite → 2.0.
        assert!((cosine_distance(&[1.0, 0.0], &[-1.0, 0.0]) - 2.0).abs() < 1e-6);
        // Degenerate (zero norm / dim mismatch) → 1.0.
        assert!((cosine_distance(&[0.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((cosine_distance(&[1.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stub_returns_empty() {
        // Contract until the backend lands: no input → no clusters, and the stub never panics.
        let empty: Vec<(SymbolId, Vec<f32>)> = Vec::new();
        assert!(semantic_clusters(&empty, &SemanticClusterParams::default()).is_empty());
    }
}
