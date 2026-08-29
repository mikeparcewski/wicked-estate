# Benchmark suite — capability receipts

`wicked-estate-bench` measures what the engine delivers on real repos **without an LLM in the loop**.
These numbers are the inputs a future agent A/B will consume; they prove must-have value
independently of model choice or prompt design.

## How to run

```bash
# Against the workspace root (useful as a smoke test during development):
cargo run -p wicked-estate-bench --bin wicked-estate-bench

# Against explicit repos — pass as many as you like:
cargo run -p wicked-estate-bench --bin wicked-estate-bench -- /path/to/repo1 /path/to/repo2

# Write the Markdown report to docs/benchmarks/capability-report.md:
# (the binary always writes the report; pass --no-report to suppress — see src/main.rs)
cargo run -p wicked-estate-bench --bin wicked-estate-bench -- /path/to/repo1
```

The report is written to `docs/benchmarks/capability-report.md`.
JSON is printed to stdout for machine consumption.

## What each receipt proves

| Receipt field | How it is measured | Must-have value it proves |
|---|---|---|
| `index_ms` | Wall-clock of `wicked_estate::index_path` | The engine indexes in milliseconds, not minutes |
| `node_count` / `edge_count` | `GraphStats` from the store after indexing | The extractor actually parsed the repo |
| `db_bytes` | Sum of `.db` + `.db-wal` + `.db-shm` after on-disk index | Storage overhead is bounded (regression gate) |
| `bytes_per_node` | `db_bytes / node_count` | Per-symbol cost; gate: `< 12_000 bytes/node` |
| `who_calls_count` | Depth-3 blast-radius via `blast_radius_by_name` | The engine knows exactly who depends on a symbol |
| `blast_radius_coverage_pct` | `resolved / (resolved + unresolved_refs_for_name)` | Honest coverage: unresolved callers (ENGINE-CONTRACT §2.1) are counted, not hidden |
| `context_pack_est_tokens` | `top-15 symbol stubs (chars / 4)` | One retrieval costs ~N tokens, not whole-file reads |
| `languages` | Node count per `Language` tag | Polyglot repos are indexed; coverage is verifiable |
| `edges_by_kind_vec` | Edge count per `EdgeKind`, sorted by count | Call, import, and type edges are all present |
| `search_latency_us` | `wicked_estate::search` wall-clock (µs) | Symbol lookup is sub-millisecond |
| `blast_radius_latency_us` | `blast_radius_by_name` depth-3 wall-clock (µs) | Blast-radius traversal is sub-millisecond |

## Regression gates

The test `footprint_and_speed_within_ceilings` in
`crates/wicked-estate-bench/tests/integration_bench.rs` asserts two hard ceilings on the fixture repo
every `cargo test` run:

| Gate | Ceiling | Rationale |
|---|---|---|
| `bytes_per_node` | `< 12_000.0` | ≈6.7 KB/node measured on prior art; 2× headroom to avoid CI flakes |
| `nodes_per_second` | `> 20.0` | Very conservative floor; real repos see 1 000 + nodes/s |

**Tighten these as optimisations land.** The ceilings are intentionally loose regression-catchers
today — once sqlite-vec compression or page-size tuning ships, halve the bytes/node ceiling and
raise the throughput floor to match the new baseline.

## Interpreting blast-radius coverage

A `blast_radius_coverage_pct` below 100 % does **not** mean the engine is wrong — it means the
resolver could not bind some call-sites to a node.  Common causes:

- Cross-language calls (TypeScript calling a Python service — not in scope for tree-sitter)
- Dynamic dispatch / reflection
- Incremental resolution not yet at the SCIP/LSP tier for this language

The engine always surfaces unresolved refs rather than silently under-reporting (soundness
contract, `docs/agent-behavior-rules.md` R7; the definition — one row per unresolved reference —
is `docs/ENGINE-CONTRACT.md` §2.1).  A consuming agent sees the coverage percentage and
can weight its confidence accordingly.

## 2026-08 — symbol-id scheme 3 re-baseline note

The node/edge/coverage numbers pinned in `capability-report.md` and `multi-repo-validation.md`
predate the ADR-002 amendment (type-nested definition identity — shipped as symbol-id scheme 3; scheme 2 was its
unreleased first cut, superseded in place). After the
scheme change, previously-merged same-named members become distinct nodes (`method`/`function`
counts rise where collisions existed), and `blast_radius_coverage_pct` is expected DOWN on
collision-heavy repos: the 0.65 scoped-name edges into merged nodes were false precision
(review finding D03-2), and the resolver now parks those refs as unresolved instead. That is a
precision correction, not a regression — the verdict rule is the per-`resolved_by` breakdown
(edges removed at the 0.65 scoped-name tier toward previously-merged nodes are corrections;
every other tier's counts must hold). Re-run the bench binary to re-baseline.
