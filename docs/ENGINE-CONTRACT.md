# Engine Contract

The hard invariants every crate must honor. Borrowed from the `prior art-contract.md`
pattern: pin the surprising things *once* so no one re-derives them wrong. Violations are caught
by `wicked_estate_core::conformance::graph_store_suite` and the edge-direction tests.

## 1. Edge-direction invariant (the one people get wrong)

```
            depends on
   source ───────────────▶ target
 (dependent)            (dependency)
```

- "A **calls** B"  → `Edge { source: A, target: B, kind: Calls }`
- "A **imports** B" → `Edge { source: A, target: B, kind: Imports }`

Therefore:
- **Dependencies of X** (what X needs) = edges where `source == X` → `Direction::Dependencies`.
- **Dependents of X** (who needs X) = edges where `target == X` → `Direction::Dependents`.
- **Blast radius of X** ("what breaks if I change X?") = transitive **dependents** =
  reverse-reachability following edges where `target == X`, then their sources, recursively.

This matches the hard-won `DEPENDENTS_BY = "target"` (a spike there caught a latent
direction bug in a reference impl — the design notes). `MemStore` and every
future store are verified against it in conformance.

## 2. Two-phase pipeline

```
 EXTRACT (per file, parallel)            RESOLVE (whole project, once)
 ───────────────────────────            ─────────────────────────────
 SourceFile ──Extractor──▶ Extraction   UnresolvedRef[] ──Resolver──▶ Edge[]
                           ├─ nodes              ▲                      │
                           ├─ local_edges        └── SymbolIndex ───────┘
                           └─ refs (UnresolvedRef)
```

- Extractors are **stateless and per-file** — no cross-file knowledge, so they parallelize.
- `local_edges` are intra-file facts known at parse time (`Contains`, `Defines`) at confidence 1.0.
- Cross-file references are emitted as `UnresolvedRef` (by name + hints) and bound later.
- Resolvers are **swappable**: changing resolution never requires re-parsing.

## 3. Confidence tiers (cheap → precise)

| Tier | Default confidence | Who emits it |
|---|---|---|
| `Parsed` | 1.0 | direct AST facts (contains/defines) |
| `Scip` / `Lsp` | 1.0 | precise indexers / on-demand LSP |
| `Tsg` | 0.8 | stack-graphs name resolution |
| `ImportMap` | 0.6 | import-map heuristics |
| `Heuristic` | 0.5 | synthesizers / other heuristics |
| `Tags` | 0.3 | tree-sitter tags only |

On a `(source, target, kind)` collision the **higher-confidence** edge wins (`Edge::dedup_key`).

## 4. GraphStore contract

Read methods (`get_node`, `find_symbols`, `neighbors`, `traverse`, `stats`) are `&self`; mutation
(`begin_batch`/`commit_batch`/`upsert_*`) is `&mut self`. `traverse` is **bounded only**
(`max_depth` + `max_nodes` required; unbounded whole-graph walks are out — see research/09).
Any new store MUST pass `wicked_estate_core::conformance::graph_store_suite`.

## 5. Rules engine node and edge kinds (W15)

Rules engine entities use first-class `NodeKind` and `EdgeKind` variants:

| NodeKind | Meaning |
|---|---|
| `Rule` | An individual rule (if/then, when/then, allow/deny, decision row) |
| `RuleSet` | A rule container: package, ruleset, policy, decision model |
| `Condition` | The LHS / when / if clause of a rule |
| `Action` | The RHS / then / effect clause of a rule |
| `Fact` | An entity or fact type the rule operates on (data model) |

| EdgeKind | Direction | Meaning |
|---|---|---|
| `Governs` | `Rule → code symbol` | Rule constrains or applies to a code symbol |
| `Evaluates` | `Rule → Fact` | Rule reads/matches on a Fact type (LHS binding) |
| `Produces` | `Rule → Fact` | Rule asserts or modifies a Fact type (RHS output) |
| `InvokedBy` | `code call site → RuleSet` | Code triggers the rules engine at this call site |

Edge direction follows the standard invariant: `source` = dependent, `target` = dependency.
- `InvokedBy`: the call site (source) depends on the RuleSet (target).
- `Governs`: the Rule (source) governs the code symbol (target) — blast-radius of the symbol surfaces the Rule as a dependent.

## 6. Wire contracts (stubs — filled in their waves)

- **MCP tools** (W4.3): the agent surface is `SearchEntity` / `TraverseGraph` / `RetrieveEntity`
  (+ `blast_radius`), each a `RetrievalTool`. Tools return `RetrievalResult { content, diagnostics }`
  where `diagnostics` carries staleness / coverage / `GRAPH-FALLBACK:` markers.
- **SCIP ingestion** (W1.4): SCIP indexer output is normalized into our `Symbol` scheme on ingest;
  merged edges get `provenance = Scip, confidence = 1.0`.
- **Extractor plugin ABI** (W6.1): drop-in extractors in `.wicked-estate-extractors/` emit nodes/edges
  with `provenance = Extractor(name)` and idempotent ids.
