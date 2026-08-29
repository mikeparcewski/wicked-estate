# wicked_estate capability benchmark

> **Note (admissibility residuals, 2026-08-29):** the pinned figures below were measured
> BEFORE the json/IaC family-guard fix (the Calls→data-target class — 402 edges on
> command_iq-class corpora — was still present) and before `unresolved_refs` gained byte-span
> columns. Post-fix before/after deltas for the command_iq/studio/crew corpora live in the
> fixing PR's evidence (`measure/bench-before.json` vs `measure/bench-after.json`); this file
> is deliberately annotated, not regenerated — it pins a different corpus set.

> **Waves W1.6 / W8.1** — engine capability receipt.  \
> The full agent A/B (baseline vs treatment with an LLM in the loop) is future work.
> This report measures what the engine itself delivers: index speed, graph completeness,
> query latency, context-pack compactness, on-disk footprint, and blast-radius coverage.

## Methodology

For each repo: index into a fresh in-memory `SqliteStore` via `wicked_estate::index_path`,
then run `search` and `blast_radius_by_name` on the top-ranked symbol from
`wicked_estate::important_symbols` (global PageRank over CALLS/IMPORTS edges).  Context-pack
size is measured by rendering the top-15 symbol stubs (signature + file:line + score).
Tokens are estimated as `chars / 4` (rough GPT tokenization proxy).

**Footprint:** a second index run writes to a temp on-disk `SqliteStore` (WAL mode).
The `.db` + `.db-wal` + `.db-shm` files are summed and the store is deleted on exit.

**Blast-radius coverage:** `resolved callers / (resolved + unresolved)` for the top symbol.
Unresolved = calls to that name that the resolver could not bind to a node
(`unresolved_refs_for_name`, defined in `docs/ENGINE-CONTRACT.md` §2.1). A lower
percentage signals incomplete resolution, not fewer callers.

## Results

| Repo | Index (ms) | Files | Nodes | Edges | Unresolved | Footprint (bytes) | bytes/node | Search (µs) | Blast-radius (µs) | BR coverage% | Who-calls | Context chars | Est. tokens |
|------|-----------|-------|-------|-------|-----------|------------------|-----------|------------|------------------|-------------|----------|--------------|------------|
| wicked-studio | 2398 | 479 | 4671 | 9229 | 34083 | 15708160 | 3363 | 27 | 7192 | 100.0 | 171 | 1866 | 466 |
| wicked-crew | 1500 | 213 | 2933 | 5392 | 16322 | 10153984 | 3462 | 27 | 4986 | 100.0 | 107 | 1740 | 435 |

> **Corpus change (2026-08-28):** three of the four previous rows were external "prior art" repos not present in this workspace; this report is regenerated on wicked-studio + wicked-crew under the per-reference unresolved definition (`docs/ENGINE-CONTRACT.md` §2.1). Rows are not comparable to the previous report.

## Per-repo receipts

### wicked-studio

**Path:** `/Users/michael.parcewski/Projects/wicked/wicked-studio`  
**Index time:** 2398ms  
**Edge coverage:** 21.3%  
**Footprint:** 15708160 bytes  (3363 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `tsx` | 2232 |
| `typescript` | 1184 |
| `python` | 909 |
| `css` | 251 |
| `json` | 40 |
| `javascript` | 24 |
| `html` | 20 |
| `markdown` | 11 |

**Nodes by kind:**

- `"class"`: 20
- `"constant"`: 1408
- `"file"`: 479
- `"function"`: 1573
- `"import"`: 483
- `"interface"`: 266
- `"method"`: 20
- `"module"`: 10
- `"struct"`: 35
- `"type_alias"`: 328
- `"variable"`: 49

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"contains"` | 3709 |
| `"calls"` | 3258 |
| `"imports"` | 2259 |
| `"extends"` | 3 |

**Capability receipts for top symbol `render`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 171 | Precise blast-radius: these nodes depend on `render` |
| blast-radius coverage | 100.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1866 | Agent receives 1866 chars of scoped context |
| context-pack est. tokens | ~466 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 27µs | Time to locate symbol by name |
| blast-radius latency | 7192µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 5968 | 1.000 | 5968 | 0 | 0 | 0 |
| `scoped-name-resolver` | 1838 | 0.638 | 0 | 0 | 1838 | 0 |
| `name-resolver` | 1245 | 0.600 | 0 | 0 | 1245 | 0 |
| `import-map-resolver` | 178 | 0.630 | 0 | 0 | 178 | 0 |

### wicked-crew

**Path:** `/Users/michael.parcewski/Projects/wicked/wicked-crew`  
**Index time:** 1500ms  
**Edge coverage:** 24.8%  
**Footprint:** 10153984 bytes  (3462 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `typescript` | 1838 |
| `css` | 467 |
| `javascript` | 304 |
| `json` | 93 |
| `bash` | 84 |
| `markdown` | 83 |
| `python` | 64 |

**Nodes by kind:**

- `"class"`: 28
- `"constant"`: 491
- `"file"`: 213
- `"function"`: 728
- `"import"`: 183
- `"interface"`: 233
- `"method"`: 183
- `"module"`: 74
- `"struct"`: 79
- `"type_alias"`: 500
- `"variable"`: 221

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"contains"` | 2537 |
| `"calls"` | 1826 |
| `"imports"` | 1020 |
| `"extends"` | 9 |

**Capability receipts for top symbol `json`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 107 | Precise blast-radius: these nodes depend on `json` |
| blast-radius coverage | 100.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1740 | Agent receives 1740 chars of scoped context |
| context-pack est. tokens | ~435 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 27µs | Time to locate symbol by name |
| blast-radius latency | 4986µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 3557 | 1.000 | 3557 | 0 | 0 | 0 |
| `scoped-name-resolver` | 1100 | 0.643 | 0 | 0 | 1100 | 0 |
| `name-resolver` | 726 | 0.600 | 0 | 0 | 726 | 0 |
| `import-map-resolver` | 9 | 0.630 | 0 | 0 | 9 | 0 |

## Regression ceilings

The `footprint_and_speed_within_ceilings` test in `wicked-estate-bench/tests/integration_bench.rs`
asserts these ceilings on the fixture repo.  Tighten them as optimisations land.

| Gate | Ceiling | Rationale |
|------|---------|-----------|
| `bytes_per_node` | `< 12_000.0` | ≈6.7 KB/node on prior art; 2× headroom |
| `nodes_per_second` | `> 20.0` | Very conservative; real runs see 1000+/s |

## How to run

```bash
# Default repos (workspace root + any that exist on disk):
cargo run -p wicked-estate-bench --bin wicked-estate-bench

# Explicit paths:
cargo run -p wicked-estate-bench --bin wicked-estate-bench -- /path/to/repo1 /path/to/repo2
```

*Last generated: see generated_at in JSON*
