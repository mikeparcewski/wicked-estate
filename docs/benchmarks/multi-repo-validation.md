# Multi-codebase validation — must-have-value receipts

**Date:** 2026-06-13. **Binary:** `target/release/wicked-estate` (release). **Gate at run:** workspace
build 0 warnings · 410 tests · clippy clean.

This is the **pillar (c)** evidence: the engine delivers must-have value on *diverse, real* codebases —
not just the fixture. Receipts are reproducible via `cargo run -p wicked-estate-bench -- <repos>` (writes
`capability-report.md`) and the `wicked-estate` CLI directly.

## Repos exercised (diverse languages + scale)

| Repo | Primary lang | Files | Nodes | Edges | Index | Footprint | bytes/node | Blast-radius lat. |
|------|-------------|-------|-------|-------|-------|-----------|-----------|-------------------|
| wicked_estate (self) | **Rust** (+15 more) | 68 | 1,434 | 3,162 | 329 ms | 6.2 MB | 4,302 | 6.3 ms |
| prior art | **JavaScript** | 94 | 730 | 1,425 | 259 ms | 3.4 MB | 4,607 | 0.2 ms |
| prior art | **TypeScript** | 3,311 | 41,685 | 96,433 | 3.4 s | 210 MB | 5,045 | 49.6 ms |
| prior art | **TS+Py+bash+yaml** | 2,512 | 22,937 | 53,594 | 3.1 s | 160 MB | 6,995 | 33 ms |
| **eliza** (elizaOS) | **TS+Python** (3.1 GB) | 33,248 | **446,810** | **762,450** | ~90 s | — | — | ~1.0 s |

**Multi-language proof:** indexing wicked_estate itself surfaced **16 languages** in one repo —
rust, python, typescript, bash, javascript, csharp, cpp, go, java, ruby, tsx, c, json, yaml,
cloudformation, kubernetes — confirming languages are *data* (a grammar + `.scm` row), not core code.

## Must-have-value receipts (what an agent actually gets)

- **"Where do I start?"** — global PageRank top-N in ~0.1 s even on eliza's 446k-node graph
  (`wicked-estate rank`). Surfaces the highest-leverage symbols.
- **"Who breaks if I change X?"** — `blast-radius hasNativeBuffer` on eliza returned concrete
  dependents (`EVAL_SCENARIOS`, `cerebras`, `evalPass`, …) in ~1 s, **bounded** (max 5000 nodes) and
  tagged with an **honest coverage marker**: `… MAY be incomplete (precise tier pending)`
  (agent-behavior rules R3/R7 — never silently claim "safe").
- **Scoped context pack** — top-15 ranked stubs render to ~**500 tokens** (`chars/4`), the compact
  payload an LLM agent consumes per retrieval (R4 <25K).
- **Blast-radius coverage %** — e.g. prior art 100%, prior art 96%, prior art 65.8% — the engine
  reports *what fraction of callers it could bind*, so a low number signals incomplete resolution
  rather than fewer callers (soundness contract: no silent false negatives).

## A real bug this validation caught (why big-repo matters)

eliza (33,288 files) initially **failed to index**:
`variable number must be between ?1 and ?32766`. The FTS bulk-rebuild built one
`… file IN (?1, …, ?N)` clause with one bind param per file — over SQLite's hard limit of 32,766.
The 64-file regression gate and every repo under 32k files structurally could not hit it.
**Fix:** chunk the file list (`CHUNK = 16_000`), locked by test
`sqlite_rebuild_fts_chunks_beyond_sqlite_param_limit` (40k synthetic files). eliza now indexes to
446,810 nodes in ~90 s. This is the canonical argument for the DoD's "complex codebases" clause:
scale exposes what fixtures cannot.

## Honest gaps (not yet must-have-complete)

- **Ranking noise on monorepos:** eliza's PageRank surfaces generic names (`len`, `String`, `slice`)
  and test/benchmark/`.json` fixtures because they have the most references in the indexed set.
  Excluding `**/test/**`, `**/benchmarks/**` at index time (config, not built) would sharpen the
  "where do I start" view.
- **Footprint at scale:** bytes/node is 4.3–7.0 KB; symbol-string interning (WAVE-PLAN #64, deferred)
  targets the edges/nodes tables for a further cut.
- **Blast-radius precision tier:** static resolution only; SCIP/LSP precise tier raises coverage on
  dynamic-dispatch-heavy code (designed; W3.x).
