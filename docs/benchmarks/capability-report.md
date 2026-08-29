# wicked_estate capability benchmark

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
(`unresolved_refs_for_name`). A lower percentage signals incomplete resolution, not
fewer callers.

## Results

| Repo | Index (ms) | Files | Nodes | Edges | Unresolved | Footprint (bytes) | bytes/node | Search (µs) | Blast-radius (µs) | BR coverage% | Who-calls | Context chars | Est. tokens |
|------|-----------|-------|-------|-------|-----------|------------------|-----------|------------|------------------|-------------|----------|--------------|------------|
| axios | 587 | 204 | 1502 | 2621 | 6186 | 3928064 | 2615 | 14 | 1756 | 74.0 | 108 | 1511 | 377 |
| wicked-studio | 751 | 479 | 4699 | 10671 | 39182 | 17154048 | 3651 | 10 | 1677 | 25.1 | 171 | 1918 | 479 |
| wicked-crew | 544 | 213 | 2938 | 5845 | 19123 | 10915840 | 3715 | 10 | 2373 | 92.1 | 164 | 1895 | 473 |

## Per-repo receipts

### axios

**Path:** `/private/tmp/claude-501/-Users-michael-parcewski-Projects-wicked/f5d30481-90ff-4c55-9faa-abdb54e1619c/scratchpad/lanes/relative-imports/measure/axios`  
**Index time:** 587ms  
**Edge coverage:** 29.8%  
**Footprint:** 3928064 bytes  (2615 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `javascript` | 900 |
| `markdown` | 187 |
| `typescript` | 153 |
| `json` | 132 |
| `html` | 130 |

**Nodes by kind:**

- `"class"`: 14
- `"constant"`: 273
- `"enum"`: 1
- `"file"`: 204
- `"function"`: 260
- `"import"`: 187
- `"interface"`: 39
- `"method"`: 86
- `"module"`: 174
- `"struct"`: 115
- `"type_alias"`: 134
- `"variable"`: 15

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"contains"` | 1185 |
| `"calls"` | 827 |
| `"imports"` | 603 |
| `"extends"` | 6 |

**Capability receipts for top symbol `create`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 108 | Precise blast-radius: these nodes depend on `create` |
| blast-radius coverage | 74.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1511 | Agent receives 1511 chars of scoped context |
| context-pack est. tokens | ~377 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 14µs | Time to locate symbol by name |
| blast-radius latency | 1756µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 1557 | 1.000 | 1557 | 0 | 0 | 0 |
| `name-resolver` | 416 | 0.600 | 0 | 0 | 416 | 0 |
| `scoped-name-resolver` | 400 | 0.631 | 0 | 0 | 400 | 0 |
| `relative-import` | 231 | 0.900 | 0 | 231 | 0 | 0 |
| `import-map-resolver` | 17 | 0.630 | 0 | 0 | 17 | 0 |

### wicked-studio

**Path:** `/Users/michael.parcewski/Projects/wicked/wicked-studio`  
**Index time:** 751ms  
**Edge coverage:** 21.4%  
**Footprint:** 17154048 bytes  (3651 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `tsx` | 2251 |
| `typescript` | 1193 |
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
- `"import"`: 511
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
| `"imports"` | 3701 |
| `"calls"` | 3258 |
| `"extends"` | 3 |

**Capability receipts for top symbol `render`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 171 | Precise blast-radius: these nodes depend on `render` |
| blast-radius coverage | 25.1% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1918 | Agent receives 1918 chars of scoped context |
| context-pack est. tokens | ~479 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 10µs | Time to locate symbol by name |
| blast-radius latency | 1677µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 6048 | 1.000 | 6048 | 0 | 0 | 0 |
| `scoped-name-resolver` | 1838 | 0.638 | 0 | 0 | 1838 | 0 |
| `relative-import` | 1362 | 0.900 | 0 | 1362 | 0 | 0 |
| `name-resolver` | 1245 | 0.600 | 0 | 0 | 1245 | 0 |
| `import-map-resolver` | 178 | 0.630 | 0 | 0 | 178 | 0 |

### wicked-crew

**Path:** `/Users/michael.parcewski/Projects/wicked/wicked-crew`  
**Index time:** 544ms  
**Edge coverage:** 23.4%  
**Footprint:** 10915840 bytes  (3715 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `typescript` | 1840 |
| `css` | 467 |
| `javascript` | 307 |
| `json` | 93 |
| `bash` | 84 |
| `markdown` | 83 |
| `python` | 64 |

**Nodes by kind:**

- `"class"`: 28
- `"constant"`: 491
- `"file"`: 213
- `"function"`: 728
- `"import"`: 188
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
| `"imports"` | 1473 |
| `"extends"` | 9 |

**Capability receipts for top symbol `delete`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 164 | Precise blast-radius: these nodes depend on `delete` |
| blast-radius coverage | 92.1% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1895 | Agent receives 1895 chars of scoped context |
| context-pack est. tokens | ~473 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 10µs | Time to locate symbol by name |
| blast-radius latency | 2373µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 3580 | 1.000 | 3580 | 0 | 0 | 0 |
| `scoped-name-resolver` | 1100 | 0.643 | 0 | 0 | 1100 | 0 |
| `name-resolver` | 726 | 0.600 | 0 | 0 | 726 | 0 |
| `relative-import` | 430 | 0.900 | 0 | 430 | 0 | 0 |
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
