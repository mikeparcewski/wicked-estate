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
| wicked-studio | 1279 | 479 | 4707 | 10375 | 34097 | 16371712 | 3478 | 12 | 1699 | 100.0 | 171 | 1965 | 491 |
| wicked-crew | 1451 | 214 | 2982 | 5831 | 16786 | 10604544 | 3556 | 10 | 2098 | 100.0 | 155 | 1856 | 464 |

## Per-repo receipts

### wicked-studio

**Path:** `/Users/michael.parcewski/Projects/wicked/wicked-studio`  
**Index time:** 1279ms  
**Edge coverage:** 23.3%  
**Footprint:** 16371712 bytes  (3478 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `tsx` | 2254 |
| `typescript` | 1196 |
| `python` | 911 |
| `css` | 251 |
| `json` | 40 |
| `javascript` | 24 |
| `html` | 20 |
| `markdown` | 11 |

**Nodes by kind:**

- `"class"`: 20
- `"constant"`: 1408
- `"file"`: 479
- `"function"`: 1575
- `"import"`: 511
- `"interface"`: 266
- `"method"`: 26
- `"module"`: 10
- `"struct"`: 35
- `"type_alias"`: 328
- `"variable"`: 49

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"contains"` | 3717 |
| `"imports"` | 3701 |
| `"calls"` | 2954 |
| `"extends"` | 3 |

**Capability receipts for top symbol `render`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 171 | Precise blast-radius: these nodes depend on `render` |
| blast-radius coverage | 100.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1965 | Agent receives 1965 chars of scoped context |
| context-pack est. tokens | ~491 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 12µs | Time to locate symbol by name |
| blast-radius latency | 1699µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 6056 | 1.000 | 6056 | 0 | 0 | 0 |
| `scoped-name-resolver` | 1825 | 0.639 | 0 | 0 | 1825 | 0 |
| `relative-import` | 1362 | 0.900 | 0 | 1362 | 0 | 0 |
| `name-resolver` | 954 | 0.600 | 0 | 0 | 954 | 0 |
| `import-map-resolver` | 178 | 0.630 | 0 | 0 | 178 | 0 |

### wicked-crew

**Path:** `/Users/michael.parcewski/Projects/wicked/wicked-crew`  
**Index time:** 1451ms  
**Edge coverage:** 25.8%  
**Footprint:** 10604544 bytes  (3556 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `typescript` | 1884 |
| `css` | 467 |
| `javascript` | 307 |
| `json` | 93 |
| `bash` | 84 |
| `markdown` | 83 |
| `python` | 64 |

**Nodes by kind:**

- `"class"`: 28
- `"constant"`: 498
- `"file"`: 214
- `"function"`: 743
- `"import"`: 188
- `"interface"`: 238
- `"method"`: 197
- `"module"`: 74
- `"struct"`: 79
- `"type_alias"`: 500
- `"variable"`: 223

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"contains"` | 2580 |
| `"calls"` | 1758 |
| `"imports"` | 1484 |
| `"extends"` | 9 |

**Capability receipts for top symbol `call`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 155 | Precise blast-radius: these nodes depend on `call` |
| blast-radius coverage | 100.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1856 | Agent receives 1856 chars of scoped context |
| context-pack est. tokens | ~464 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 10µs | Time to locate symbol by name |
| blast-radius latency | 2098µs | Time for depth-3 dependent traversal |

**Resolution precision (by tier) — W3.5:**

> **Precision caveat:** confidence is a *proxy*, not ground-truth precision.
> True precision (fraction of edges that are correct) requires labeled data.
> Low-confidence-heavy resolvers (high `low` band) should be flagged for manual review.

| Resolver | Edges | Mean conf | exact (=1.0) | high [0.8,1.0) | medium [0.5,0.8) | low [0.0,0.5) |
|----------|-------|-----------|-------------|----------------|-----------------|--------------|
| `tree-sitter` | 3631 | 1.000 | 3631 | 0 | 0 | 0 |
| `scoped-name-resolver` | 1101 | 0.644 | 0 | 0 | 1101 | 0 |
| `name-resolver` | 657 | 0.600 | 0 | 0 | 657 | 0 |
| `relative-import` | 433 | 0.900 | 0 | 433 | 0 | 0 |
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
