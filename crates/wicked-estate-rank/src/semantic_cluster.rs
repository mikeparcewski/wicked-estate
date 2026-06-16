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

/// L2-normalise a vector in place (no-op if zero norm).
fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Order clusters largest-first, tie-broken by the smallest member id, and sort members within each
/// cluster by id — fully deterministic output.
fn finalize(mut clusters: Vec<Vec<SymbolId>>) -> Vec<Vec<SymbolId>> {
    for c in clusters.iter_mut() {
        c.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    }
    clusters.retain(|c| !c.is_empty());
    clusters.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| a.first().map(|s| &s.0).cmp(&b.first().map(|s| &s.0)))
    });
    clusters
}

/// Lloyd's k-means in cosine space with deterministic farthest-point seeding.
fn kmeans(pts: &[(SymbolId, Vec<f32>)], k: usize, max_iter: usize) -> Vec<Vec<SymbolId>> {
    let n = pts.len();
    let k = k.min(n);
    if k == 0 {
        return Vec::new();
    }

    // Deterministic seeding: first centroid = first point (already id-sorted); each subsequent
    // centroid = the point maximising its minimum distance to the chosen centroids (farthest-point;
    // ties → lowest index).
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);
    centroids.push(pts[0].1.clone());
    while centroids.len() < k {
        let mut best_i = 0usize;
        let mut best_d = -1.0f32;
        for (i, (_, v)) in pts.iter().enumerate() {
            let min_d = centroids
                .iter()
                .map(|c| cosine_distance(v, c))
                .fold(f32::INFINITY, f32::min);
            if min_d > best_d {
                best_d = min_d;
                best_i = i;
            }
        }
        centroids.push(pts[best_i].1.clone());
    }

    let mut assign = vec![usize::MAX; n];
    for _ in 0..max_iter.max(1) {
        // Assignment: nearest centroid; ties → lowest centroid index.
        let mut changed = false;
        for (i, (_, v)) in pts.iter().enumerate() {
            let mut best_c = 0usize;
            let mut best_d = f32::INFINITY;
            for (ci, c) in centroids.iter().enumerate() {
                let d = cosine_distance(v, c);
                if d < best_d {
                    best_d = d;
                    best_c = ci;
                }
            }
            if assign[i] != best_c {
                assign[i] = best_c;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        // Update: centroid = L2-normalised mean of assigned vectors; empty clusters keep their
        // previous centroid (deterministic, avoids re-seeding churn).
        let dim = pts[0].1.len();
        let mut sums = vec![vec![0.0f32; dim]; centroids.len()];
        let mut counts = vec![0usize; centroids.len()];
        for (i, (_, v)) in pts.iter().enumerate() {
            if v.len() != dim {
                continue;
            }
            let c = assign[i];
            for (s, x) in sums[c].iter_mut().zip(v.iter()) {
                *s += x;
            }
            counts[c] += 1;
        }
        for (ci, cnt) in counts.iter().enumerate() {
            if *cnt > 0 {
                let mut mean: Vec<f32> = sums[ci].iter().map(|s| s / *cnt as f32).collect();
                normalize(&mut mean);
                centroids[ci] = mean;
            }
        }
    }

    let mut clusters: Vec<Vec<SymbolId>> = vec![Vec::new(); centroids.len()];
    for (i, (id, _)) in pts.iter().enumerate() {
        clusters[assign[i]].push(id.clone());
    }
    finalize(clusters)
}

/// DBSCAN over cosine distance. Points in no dense region are noise and are excluded from output.
fn dbscan(pts: &[(SymbolId, Vec<f32>)], eps: f32, min_pts: usize) -> Vec<Vec<SymbolId>> {
    let n = pts.len();
    let neighbors = |i: usize| -> Vec<usize> {
        (0..n)
            .filter(|&j| cosine_distance(&pts[i].1, &pts[j].1) <= eps)
            .collect()
    };

    const UNVISITED: i64 = -2;
    const NOISE: i64 = -1;
    let mut label = vec![UNVISITED; n];
    let mut cluster_id: i64 = 0;

    for i in 0..n {
        if label[i] != UNVISITED {
            continue;
        }
        let neigh = neighbors(i);
        if neigh.len() < min_pts {
            label[i] = NOISE;
            continue;
        }
        // Start a new cluster and expand via a deterministic queue.
        label[i] = cluster_id;
        let mut queue: std::collections::VecDeque<usize> = neigh.into_iter().collect();
        while let Some(j) = queue.pop_front() {
            if label[j] == NOISE {
                label[j] = cluster_id; // border point
            }
            if label[j] != UNVISITED {
                continue;
            }
            label[j] = cluster_id;
            let jn = neighbors(j);
            if jn.len() >= min_pts {
                for x in jn {
                    queue.push_back(x);
                }
            }
        }
        cluster_id += 1;
    }

    let mut clusters: Vec<Vec<SymbolId>> = vec![Vec::new(); cluster_id.max(0) as usize];
    for (i, (id, _)) in pts.iter().enumerate() {
        if label[i] >= 0 {
            clusters[label[i] as usize].push(id.clone());
        }
    }
    finalize(clusters)
}

/// Cluster symbols by embedding proximity, largest cluster first.
///
/// Runs [`ClusterAlgo::KMeans`] (exactly `k` clusters) or [`ClusterAlgo::Dbscan`] (density-based;
/// noise points excluded) per `params`. Deterministic: points are processed in `SymbolId` order
/// and all tie-breaks are by id/index. Empty input → empty output.
///
/// Quality depends on the embeddings: meaningful clusters need *semantic* vectors (`--embeddings`
/// + `fastembed`). Clustering `HashEmbedder` bag-of-words vectors yields noise.
pub fn semantic_clusters(
    embeddings: &[(SymbolId, Vec<f32>)],
    params: &SemanticClusterParams,
) -> Vec<Vec<SymbolId>> {
    if embeddings.is_empty() {
        return Vec::new();
    }
    let mut pts: Vec<(SymbolId, Vec<f32>)> = embeddings.to_vec();
    pts.sort_by(|a, b| a.0.0.cmp(&b.0.0));
    match params.algorithm {
        ClusterAlgo::KMeans => kmeans(&pts, params.k, params.max_iter),
        ClusterAlgo::Dbscan => dbscan(&pts, params.eps, params.min_pts),
    }
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

    fn sym(s: &str) -> SymbolId {
        SymbolId(s.to_string())
    }

    /// Two planted groups: A,B,C near e0=(1,0,0); D,E,F near e1=(0,1,0).
    fn planted() -> Vec<(SymbolId, Vec<f32>)> {
        vec![
            (sym("A"), vec![1.0, 0.0, 0.01]),
            (sym("B"), vec![0.99, 0.0, 0.0]),
            (sym("C"), vec![1.0, 0.01, 0.0]),
            (sym("D"), vec![0.0, 1.0, 0.0]),
            (sym("E"), vec![0.0, 0.99, 0.01]),
            (sym("F"), vec![0.01, 1.0, 0.0]),
        ]
    }

    fn group_of<'a>(clusters: &'a [Vec<SymbolId>], id: &str) -> &'a Vec<SymbolId> {
        clusters
            .iter()
            .find(|c| c.iter().any(|s| s.0 == id))
            .expect("symbol must be in some cluster")
    }

    fn same_cluster(clusters: &[Vec<SymbolId>], a: &str, b: &str) -> bool {
        group_of(clusters, a).iter().any(|s| s.0 == b)
    }

    #[test]
    fn kmeans_two_planted_clusters() {
        let p = SemanticClusterParams {
            algorithm: ClusterAlgo::KMeans,
            k: 2,
            ..Default::default()
        };
        let c = semantic_clusters(&planted(), &p);
        assert_eq!(c.len(), 2, "k=2 must yield 2 clusters");
        assert!(same_cluster(&c, "A", "B") && same_cluster(&c, "A", "C"));
        assert!(same_cluster(&c, "D", "E") && same_cluster(&c, "D", "F"));
        assert!(
            !same_cluster(&c, "A", "D"),
            "the two groups must be separate"
        );
    }

    #[test]
    fn dbscan_two_planted_clusters() {
        let p = SemanticClusterParams {
            algorithm: ClusterAlgo::Dbscan,
            eps: 0.1,
            min_pts: 2,
            ..Default::default()
        };
        let c = semantic_clusters(&planted(), &p);
        assert_eq!(c.len(), 2, "DBSCAN must find 2 dense groups");
        assert!(same_cluster(&c, "A", "C"));
        assert!(same_cluster(&c, "D", "F"));
        assert!(!same_cluster(&c, "A", "D"));
    }

    #[test]
    fn dbscan_marks_noise() {
        let mut data = planted();
        data.push((sym("Z"), vec![0.0, 0.0, 1.0])); // orthogonal to both groups → noise
        let p = SemanticClusterParams {
            algorithm: ClusterAlgo::Dbscan,
            eps: 0.1,
            min_pts: 2,
            ..Default::default()
        };
        let c = semantic_clusters(&data, &p);
        let z_clustered = c.iter().any(|cl| cl.iter().any(|s| s.0 == "Z"));
        assert!(
            !z_clustered,
            "the orthogonal outlier must be noise, not in a cluster"
        );
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn kmeans_returns_k_clusters() {
        // Three well-separated points → k=3 returns 3 non-empty clusters.
        let data = vec![
            (sym("X"), vec![1.0, 0.0, 0.0]),
            (sym("Y"), vec![0.0, 1.0, 0.0]),
            (sym("Z"), vec![0.0, 0.0, 1.0]),
        ];
        let p = SemanticClusterParams {
            algorithm: ClusterAlgo::KMeans,
            k: 3,
            ..Default::default()
        };
        let c = semantic_clusters(&data, &p);
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|cl| cl.len() == 1));
    }

    #[test]
    fn degenerate_inputs() {
        let empty: Vec<(SymbolId, Vec<f32>)> = Vec::new();
        assert!(semantic_clusters(&empty, &SemanticClusterParams::default()).is_empty());

        let single = vec![(sym("only"), vec![1.0, 0.0])];
        let p = SemanticClusterParams {
            algorithm: ClusterAlgo::KMeans,
            k: 1,
            ..Default::default()
        };
        let c = semantic_clusters(&single, &p);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].len(), 1);

        // All-identical vectors must not panic.
        let same = vec![
            (sym("a"), vec![1.0, 1.0]),
            (sym("b"), vec![1.0, 1.0]),
            (sym("c"), vec![1.0, 1.0]),
        ];
        let _ = semantic_clusters(&same, &SemanticClusterParams::default());
    }

    #[test]
    fn named_pair_oracle() {
        // The core promise: a related pair lands together, an unrelated pair lands apart.
        let c = semantic_clusters(
            &planted(),
            &SemanticClusterParams {
                algorithm: ClusterAlgo::KMeans,
                k: 2,
                ..Default::default()
            },
        );
        assert!(
            same_cluster(&c, "A", "B"),
            "related pair (A,B) must cluster together"
        );
        assert!(
            !same_cluster(&c, "A", "D"),
            "unrelated pair (A,D) must be apart"
        );
    }

    #[test]
    fn deterministic() {
        let p = SemanticClusterParams {
            algorithm: ClusterAlgo::KMeans,
            k: 2,
            ..Default::default()
        };
        let r1 = semantic_clusters(&planted(), &p);
        let r2 = semantic_clusters(&planted(), &p);
        assert_eq!(r1, r2, "clustering must be deterministic");

        let pd = SemanticClusterParams {
            algorithm: ClusterAlgo::Dbscan,
            eps: 0.1,
            min_pts: 2,
            ..Default::default()
        };
        assert_eq!(
            semantic_clusters(&planted(), &pd),
            semantic_clusters(&planted(), &pd)
        );
    }
}
