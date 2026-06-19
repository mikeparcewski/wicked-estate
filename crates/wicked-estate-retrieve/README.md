# wicked-estate-retrieve

Agent-facing retrieval tools: the `RetrievalTool` impls that LLM agents call to query the code graph, plus RRF hybrid fusion, token-budgeted context rendering, and optional semantic search.

## What it does

- Implements seven named tools consumed by `wicked-estate-mcp` and directly callable via `RetrievalTool::invoke`.
- Enforces agent-behavior rules on every response: R1 (never `isError: true`), R4 (output capped by `limit`/`max_nodes`/`token_budget`), R5 (staleness notes), R7 (low-confidence edges flagged as `R7-CONFIDENCE:`).
- `BlastRadius` includes a compact summary (by-kind counts, top files, PageRank-ranked dependents) and reports unresolved caller count for coverage transparency.
- `render_context` / `ContextPack`: gathers seed neighbourhood, ranks by personalised PageRank, renders elided stubs (`kind name sig + first doc line + file:line`), packs within a token budget.
- `reciprocal_rank_fusion`: fuses multiple ranked `SymbolId` lists (graph traversal + name search) with `k=60` into one combined ranking.
- `Embedder` trait with `HashEmbedder` (deterministic, zero deps) default; real semantic quality via feature-gated `FastEmbedder` (ONNX/BGE) or `Model2VecEmbedder` (static distilled).

## Key types / traits

| Item | Description |
|---|---|
| `SearchEntity` | Find symbols by exact or substring name; two-pass exact-then-FTS merge. |
| `RetrieveEntity` | Full detail for one symbol id; optional source inlining, typed annotations. |
| `TraverseGraph` | Bounded multi-hop walk; edge endpoints denormalized with `{confidence, provenance, resolved_by}`. |
| `BlastRadius` | Transitive dependents (reverse-reachability on Calls); summary + unresolved-caller count. |
| `Lineage` | Transitive dependencies (forward-reachability on Calls+Imports); directional complement of `BlastRadius`. |
| `FetchContent` | Return the exact source slice for a symbol by byte span. |
| `ContextBundle` | One-shot seed + ranked neighbours + budgeted stubs (W12). |
| `ContextPack` | Token-budgeted ranked elided-stub context; accepts seeds array or a name query. |
| `RulesInventory` | Lists all rules-engine nodes (RuleSet, Rule) in the graph + the code files that invoke them via `InvokedBy` edges. Rules engine discovery for LLM agents. |
| `SemanticSearch` | ANN search via cosine similarity over stored embeddings; requires a `VectorStore`. |
| `Embedder` | Trait: `embed(text) -> Vec<f32>` + `dim() -> usize`. |
| `HashEmbedder` | Deterministic bag-of-words FNV-1a embedder; zero deps, proves wiring, no semantic quality. |
| `reciprocal_rank_fusion(lists, k)` | RRF over multiple ranked `SymbolId` lists; `k=60.0` default. |
| `render_context(store, seeds, budget)` | Token-budgeted elided-stub string for LLM prompts. |

For rules queries: use `edge_kinds=["invoked_by"]` with `TraverseGraph` to trace code→rules connections, `["governs"]` for ruleset→rule structure, `["evaluates"]` / `["produces"]` for rule internals.

## Usage

```rust
use wicked_estate_retrieve::SearchEntity;
use wicked_estate_core::RetrievalTool;
use serde_json::json;

let result = SearchEntity.invoke(&store, &json!({ "name": "parse_request", "limit": 10 }))?;
// result.content: { "matches": [...], "total": N }
// result.diagnostics: staleness + coverage notes
```

## Crate features

| Feature | Effect |
|---|---|
| `fastembed` | Enables `FastEmbedder` (ONNX/BGE-small-en-v1.5, 384-dim). Downloads model from HuggingFace on first use. |
| `model2vec` | Enables `Model2VecEmbedder` (static distilled, no ONNX runtime, ~30MB model). |

Both are off by default. The default build uses `HashEmbedder` (deterministic, dependency-free, not semantically meaningful).

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
