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
| wicked_estate | 329 | 68 | 1434 | 3162 | 6125 | 6168576 | 4302 | 31 | 6348 | 59.3 | 421 | 1941 | 485 |
| prior art | 259 | 94 | 730 | 1425 | 5868 | 3362816 | 4607 | 12 | 195 | 100.0 | 8 | 1676 | 419 |
| prior art | 3406 | 3311 | 41685 | 96433 | 164319 | 210321408 | 5045 | 14 | 49590 | 96.0 | 5000 | 2010 | 502 |
| prior art | 3066 | 2512 | 22937 | 53594 | 187872 | 160448512 | 6995 | 14 | 33048 | 65.8 | 2754 | 2147 | 536 |

## Per-repo receipts

### wicked_estate

**Path:** `/Users/michael.parcewski/Projects/wicked_estate`  
**Index time:** 329ms  
**Edge coverage:** 34.0%  
**Footprint:** 6168576 bytes  (4302 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `rust` | 1163 |
| `python` | 74 |
| `typescript` | 34 |
| `bash` | 28 |
| `javascript` | 19 |
| `csharp` | 17 |
| `cpp` | 15 |
| `go` | 14 |
| `java` | 14 |
| `ruby` | 14 |
| `tsx` | 13 |
| `c` | 11 |
| `json` | 6 |
| `yaml` | 6 |
| `cloudformation` | 3 |
| `kubernetes` | 3 |

**Nodes by kind:**

- `"class"`: 18
- `"constant"`: 87
- `"enum"`: 32
- `"field"`: 39
- `"file"`: 68
- `"function"`: 907
- `"import"`: 84
- `"interface"`: 4
- `"method"`: 28
- `"module"`: 1
- `"struct"`: 113
- `"trait"`: 13
- `"type_alias"`: 12
- `"variable"`: 24
- `{"other":"resource"}`: 4

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"calls"` | 1756 |
| `"contains"` | 1282 |
| `"imports"` | 120 |
| `"extends"` | 3 |
| `"implements"` | 1 |

**Capability receipts for top symbol `new`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 421 | Precise blast-radius: these nodes depend on `new` |
| blast-radius coverage | 59.3% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1941 | Agent receives 1941 chars of scoped context |
| context-pack est. tokens | ~485 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 31µs | Time to locate symbol by name |
| blast-radius latency | 6348µs | Time for depth-3 dependent traversal |

### prior art

**Path:** `/Users/michael.parcewski/Projects/wicked/prior art`  
**Index time:** 259ms  
**Edge coverage:** 19.5%  
**Footprint:** 3362816 bytes  (4607 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `javascript` | 623 |
| `json` | 107 |

**Nodes by kind:**

- `"class"`: 9
- `"constant"`: 150
- `"file"`: 94
- `"function"`: 234
- `"import"`: 66
- `"method"`: 68
- `"struct"`: 91
- `"variable"`: 18

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"contains"` | 570 |
| `"calls"` | 527 |
| `"imports"` | 328 |

**Capability receipts for top symbol `walk`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 8 | Precise blast-radius: these nodes depend on `walk` |
| blast-radius coverage | 100.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 1676 | Agent receives 1676 chars of scoped context |
| context-pack est. tokens | ~419 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 12µs | Time to locate symbol by name |
| blast-radius latency | 195µs | Time for depth-3 dependent traversal |

### prior art

**Path:** `/Users/michael.parcewski/Projects/prior art`  
**Index time:** 3406ms  
**Edge coverage:** 37.0%  
**Footprint:** 210321408 bytes  (5045 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `go` | 32675 |
| `bash` | 3099 |
| `rust` | 2636 |
| `tsx` | 1262 |
| `yaml` | 843 |
| `typescript` | 842 |
| `json` | 311 |
| `python` | 10 |
| `javascript` | 7 |

**Nodes by kind:**

- `"class"`: 7
- `"constant"`: 4743
- `"enum"`: 67
- `"file"`: 3311
- `"function"`: 21018
- `"import"`: 588
- `"interface"`: 359
- `"method"`: 1567
- `"struct"`: 3085
- `"trait"`: 11
- `"type_alias"`: 111
- `"variable"`: 6818

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"calls"` | 44475 |
| `"contains"` | 37786 |
| `"imports"` | 14166 |
| `"extends"` | 3 |
| `"implements"` | 3 |

**Capability receipts for top symbol `Parallel`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 5000 | Precise blast-radius: these nodes depend on `Parallel` |
| blast-radius coverage | 96.0% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 2010 | Agent receives 2010 chars of scoped context |
| context-pack est. tokens | ~502 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 14µs | Time to locate symbol by name |
| blast-radius latency | 49590µs | Time for depth-3 dependent traversal |

### prior art

**Path:** `/Users/michael.parcewski/Projects/prior art`  
**Index time:** 3066ms  
**Edge coverage:** 22.2%  
**Footprint:** 160448512 bytes  (6995 bytes/node)  

**Nodes by language:**

| Language | Nodes |
|----------|-------|
| `typescript` | 16575 |
| `tsx` | 2285 |
| `json` | 1333 |
| `javascript` | 1251 |
| `python` | 1098 |
| `bash` | 385 |
| `yaml` | 10 |

**Nodes by kind:**

- `"class"`: 320
- `"constant"`: 3993
- `"enum"`: 1
- `"file"`: 2512
- `"function"`: 6871
- `"import"`: 2074
- `"interface"`: 1755
- `"method"`: 2785
- `"struct"`: 1142
- `"type_alias"`: 1006
- `"variable"`: 478

**Edges by kind:**

| Edge kind | Count |
|-----------|-------|
| `"calls"` | 23059 |
| `"contains"` | 18351 |
| `"imports"` | 12128 |
| `"implements"` | 30 |
| `"extends"` | 26 |

**Capability receipts for top symbol `map`:**

| Metric | Value | What it proves |
|--------|-------|----------------|
| who-calls count | 2754 | Precise blast-radius: these nodes depend on `map` |
| blast-radius coverage | 65.8% | Fraction of callers the resolver bound (lower → incomplete resolution) |
| context-pack chars | 2147 | Agent receives 2147 chars of scoped context |
| context-pack est. tokens | ~536 | Estimated LLM token cost for one context retrieval |
| context-pack symbols | 15 | Symbols ranked into the pack |
| search latency | 14µs | Time to locate symbol by name |
| blast-radius latency | 33048µs | Time for depth-3 dependent traversal |

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
