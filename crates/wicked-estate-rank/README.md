# wicked-estate-rank

PageRank-based symbol importance ranking over the code graph, plus community detection and semantic clustering.

## What it does

- Runs biased-teleport power iteration (O(V+E) per step, L1 epsilon early-stop) on a `petgraph::DiGraph` built from `Calls` and `Imports` edges only; `Contains`/`Defines` edges are excluded to keep structural containment out of the relevance signal.
- Supports global PageRank (empty seeds, uniform teleport) and personalized PageRank (non-empty seeds receive `SEED_WEIGHT = 100×` teleport, matching the Aider repo-map pattern).
- Handles dangling nodes (zero out-degree) by redistributing mass uniformly, preserving the row-stochastic invariant.
- Does NOT call `petgraph::algo::page_rank` — measured at ~60s on 22.9k nodes / 53.6k edges; the custom iteration ranks the same graph in ~0.1s.
- Provides community detection (Louvain-style modularity) and semantic clustering as companion modules.

## Key types / traits

| Item | Description |
|---|---|
| `PageRank` | Implements `wicked_estate_core::Ranker`; configurable `damping`, `max_iter`, `epsilon`. |
| `ranked_symbols(store, seeds, top_n)` | Run PageRank, return top-N `(SymbolId, f32)` sorted descending by score. |
| `DEFAULT_DAMPING` | `0.85` (Brin & Page 1998). |
| `SEED_WEIGHT` | `100.0` — teleport multiplier for personalized seeds. |
| `detect_communities(store, params)` | Louvain community detection over the `Calls`/`Imports` sub-graph; returns cluster assignments. |
| `semantic_clusters(nodes, params)` | K-means or agglomerative clustering over node embeddings. |

## Usage

```rust
use wicked_estate_rank::{PageRank, ranked_symbols};
use wicked_estate_core::{Ranker, SymbolId};

// Global PageRank — top 20 symbols by importance
let top = ranked_symbols(&store, &[], 20)?;

// Personalized — bias toward the symbol the agent is working on
let seeds = vec![SymbolId("my_module::MyStruct".into())];
let scores = PageRank::new().rank(&store, &seeds)?;
```

## Crate features

No optional feature flags.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
