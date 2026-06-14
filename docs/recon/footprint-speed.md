# Footprint + Speed — root cause & remediation plan

**Trigger:** "we need to work on speed and compression — we can't bloat people's disks."

## Baseline (measured 2026-06-13, release binary, indexing `~/Projects/prior art`)

- **Index time: ~52s** (clean internal timer). The earlier "119.56s real" was CONTAMINATED — that run overlapped a subagent's `cargo build/test` competing for CPU. Do not quote 119s. Prior "57s" was also stale/partial.
- **DB size: 357 MB** for one mid-size repo (2,511 files, 22,932 nodes, 53,564 edges). This is the bloat.

### Phase breakdown (CI_TIMING=1, 22,932 nodes / 211,264 refs)
```
extract+write (2511 files): 49.4s   ← 94% — THE speed bottleneck
resolve:                     0.37s   ← negligible
store edges+unresolved:      2.36s   ← writing 187,772 unresolved refs
total:                      52.3s
```
**Speed and disk are DIFFERENT problems.** The 228 MB of `unresolved_refs` costs only 2.36s to write — slimming it is a disk win, not the speed fix. The speed bottleneck is the fused `extract+write` phase (49.4s), not yet split into parse-vs-write.

| table | bytes | % of db | note |
|---|---|---|---|
| **unresolved_refs** | 227.9 MB | 64% | + its indexes ~20MB ⇒ **~70%** |
| edges | 41.4 MB | 12% | + autoindex/idx ~22MB |
| content | 30.5 MB (28MB raw text) | 9% | uncompressed source |
| nodes | 15.2 MB | 4% | + fts ~5.5MB |

## Root cause #1 — `unresolved_refs` is 70% of the DB

- **187,772 rows × ~1,213 bytes/row.**
- Schema today: `(id, raw_name TEXT, file TEXT, data TEXT)` where **`data` = the entire `UnresolvedRef` serialized as JSON** — which *re-stores* `raw_name`, `file`, `kind`, and `location` (already/also columns), plus a 6-field `span`, a `from` string, and `hints`. Triple-storage of name+file; ~1.2KB of mostly-redundant JSON per row.
- We persist refs that **can never resolve in-repo** (`'node:child_process'`, `'node:url'`, stdlib, dynamic property access) — pure noise for blast-radius (they are external deps, not missing in-repo callers).

**What blast-radius coverage actually needs** (`unresolved_refs_for_name(name)` → "N unresolved CALL refs to this name, so dependents may be incomplete"): `raw_name`, `kind`, `file`, and a line for reporting. It does NOT need the JSON blob, byte offsets, `from`, or `hints`.

### Fix #1 (the big one — do first)
Drop the `data` JSON blob. Store typed minimal columns: `(id, raw_name, kind, file, start_line)` (+ `start_col,end_line,end_col` ONLY if a resolver tier actually consumes them — verify against `wicked-estate-resolve`). Reconstruct `UnresolvedRef` from columns on read.
- Expected: ~1,213 → ~50 bytes/row ⇒ **228 MB → ~10–12 MB (~20×)**.
- Speed effect: SMALL. The unresolved write is only 2.36s of 52s (measured). This is a ~240 MB **disk** win, not a speed fix. Do not sell it as one.
- **CONFIRMED SAFE (read-back analysis):** persisted `unresolved_refs` are read through exactly one trait method — `unresolved_refs_for_name(name)` — whose only caller (`wicked-estate-retrieve/src/lib.rs:449`) uses `.len()` for coverage. Resolvers (`wicked-estate-resolve`) consume `r.location`/`r.hints["imports"]`/`r.kind` from the **in-memory** `Extraction.unresolved` during the index pass, NEVER from the DB. There is no incremental re-resolution path that re-reads persisted refs (re-resolution always re-extracts, getting fresh in-memory hints). ⇒ Dropping `data`/`hints`/`span`/`from` from persistence has **zero semantic impact**. Keep only `(id, raw_name, kind, file)` — `kind` retained so coverage can count call-refs specifically.

## Root cause #2 — uncompressed content (28 MB raw)
Content is content-addressed (storage agent in flight). Add **zstd** to the blob: `content(git_sha, blob BLOB)` storing `zstd(text, level=3)`; decompress on read. Source code → ~3–4× ⇒ **28 MB → ~7 MB**. zstd level 1–3 is ~500 MB/s — negligible index-time cost.

## Root cause #3 — symbol strings not interned (edges 41MB, nodes 15MB)
`edges.source`/`edges.target` and `nodes.symbol` store long SCIP symbol strings, repeated everywhere. Intern: `symbols(sid INTEGER PRIMARY KEY, sym TEXT UNIQUE)`; edges/nodes/unresolved reference `sid` (INT). Shrinks edges + nodes + their autoindexes (the per-row weight is the repeated string). **Phase 2** (bigger refactor; do after #1/#2 land + are benched).

## Incoming-table footprint (live-brain wave, in flight)
- **`edge_history` (edge_json blobs)**: given the no-bloat mandate, flip default to **OFF** (opt-in `--history`), compress `edge_json` with zstd, and lower retention (20 → ~5 per file). A read-only log that grows on every save during `watch` is exactly the bloat risk to avoid by default.
- **`embeddings`**: keep **opt-in** (only when semantic index built) and **int8-quantize** (384×f32=1.5KB/sym → 384B/sym, 4×). Not present in this baseline (0 rows) but guard before W5 semantic is enabled by default.

## Speed plan — target the 49.4s `extract+write` phase
The bottleneck is NOT footprint #1 (unresolved write = 2.36s) and NOT resolve (0.37s). It's the
fused `extract+write` phase. First step is to **split that timer** into its parts so we optimize the
real cost, not a guess:
- **(a) parse** — tree-sitter over 2511 files. Confirm extraction is actually rayon-parallel and not
  serialized behind a shared resource.
- **(b) write nodes** — 22,932 node upserts.
- **(c) write content** — 2511 source blobs (28 MB; will gain zstd in footprint #2).
- **(d) write FTS5** — `nodes_fts` population. PRIME SUSPECT: per-row FTS inserts are notoriously slow;
  batching, deferring to a single post-pass, or `content=''` external-content FTS can cut this hard.
- **Serial-write tail**: if parse is parallel but the single SQLite writer is the tail while cores idle,
  the write side is the ceiling — batch larger, write FTS/content off the critical path.

Hypothesis to confirm with the split: (d) FTS and/or the serial write tail dominate. ~20ms/file for
2511 files smells like per-row SQLite/FTS overhead, not parse.

**Targets:** full index of prior art **< 30s** and **< 100 MB** on disk (from ~52s / 357 MB).
Encode both as `wicked-estate-bench` gates (footprint #4) so they regress like correctness.

## RESULT (measured 2026-06-13, after fixes)
- **DISK: 357 MB → 154 MB (−57%).** unresolved_refs 228→34.8 MB (slim typed cols), content 30→10.6 MB (zstd). Remaining: edges+nodes ~56 MB (footprint #3 interning, not yet done) → target <100 MB still reachable.
- **SPEED: 114s → 55s (−52%).** The real bottleneck was NOT `extract+write` — it was an **untimed tail**: the global PageRank cache population called `petgraph::algo::page_rank`, which is **O(V·E)/iter with no epsilon early-stop = 59.8s** on 22.9k nodes / 53.6k edges. Replaced with the existing O(V+E) power iteration (uniform teleport == global PR) → **0.107s (560×)**. This was the single biggest speed win and it was pure waste on every index.
- **Next frontier:** `extract+write` is now 52s / 95% of the 55s. Split parse-vs-write (FTS/serial-write suspects) for the < 30s target. Banked wins verified; full gate green (build 0w / 378 tests / clippy clean).

## Make it a gate (CLAUDE.md §9: slow is a defect)
Add a **footprint + speed** check to `wicked-estate-bench`: assert `db_bytes / node_count` under a ceiling and `index_seconds` under a ceiling on the frozen corpus, so bloat/slowness regress like correctness does.

## Sequencing
All of #1/#2/#3 + edge_history/embeddings edits touch **`wicked-estate-store`** (and #1 touches `wicked-estate` persist + `wicked-estate-core/refs` read-back) — the SAME crate the live-brain storage agent is editing now. **Must serialize after** that agent completes; cannot parallelize (file collision).
