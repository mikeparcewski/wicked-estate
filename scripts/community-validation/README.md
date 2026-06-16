# Community-detection validation

Three test layers back the community/clustering features (semantic clustering, richer edges,
package-aware + tunable Louvain). Each catches a different failure class.

## Layer 1 — unit (CI, every change)
Pure-algorithm tests, run by `cargo test`:
- `wicked-estate-rank` `community.rs`: known-answer graphs (two cliques + bridge → 2 communities,
  not 1), `modularity ≥ union-find`, `max_community_fraction < 0.30` on a clustered graph,
  resolution granularity, hierarchical shatter-guard, package-bias non-merge, determinism.
- `wicked-estate-rank` `semantic_cluster.rs`: planted clusters (k-means + DBSCAN), DBSCAN noise,
  named-pair oracle, degenerate inputs, determinism.
- `wicked-estate-extract`: Java/Spring DI + route edges on fixtures, with negative cases.

## Layer 2 — bench metrics (CI, every change)
`wicked-estate-bench` `community_metrics.rs`: `community_metrics()` over synthetic stores asserts
modularity, count, **max-community fraction**, and singleton rate. The mega-community gate
(`max_community_fraction < 0.30` on a clustered graph) is the single assertion that would have
caught the union-find backend before it shipped.

## Layer 3 — heavy repos (operator-run, not CI)
`validate-clusters.sh <repo> [resolution] [db]` indexes a real repo and reports the metrics +
sampled cluster members for human module-alignment review. Targets:

| Repo | What it proves |
|---|---|
| wicked-estate (self) | clusters align to crate boundaries; modularity > 0.6; max-fraction < 0.30 |
| Apache Kafka (~200k symbols) | stress: no community > ~5k; the old union-find mega-community does not return |
| React (TS, `--embeddings`+`fastembed`) | semantic named-pair oracle: `useState`/`useReducer` together, `ReactDOM.render` apart |

Self-index result (3851 nodes, this repo): **127 communities, modularity 0.805, max-fraction
0.092**, largest clusters dominated by single crates (extract / store / core). Run:

```sh
cargo build -p wicked-estate
scripts/community-validation/validate-clusters.sh crates
```
