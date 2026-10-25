# Per-query freshness overhead (wicked-estate#351)

What `wicked_estate::freshness::FreshnessProbe::annotate` adds to one retrieval response, warm.
It is the per-call cost of agent rule R5 as #351 implements it: commits behind per root,
measured now; drift of every file the answer touches; and new, never-indexed source files.

## How it is measured

`cargo test -p wicked-estate-bench --lib freshness -- --nocapture` runs
`wicked_estate_bench::freshness_overhead`:

- **Corpus:** a generated, committed git repo of 300 TypeScript modules, indexed by the real
  `index_path` into an on-disk SQLite graph. It is offline and deterministic. The frozen baseline
  corpus is cloned from the network, which the default `gate` must not need.
- **Probe:** one long-lived probe, opened on its own store handle as the MCP server's is.
- **Answer:** each measured call annotates an answer touching 20 distinct files, the shape of a
  `BlastRadius` page. One of those files is edited on disk, so the drift path (stat, digest cache,
  rows marked `stale`) does real work.
- **Calls:** 1 warm-up, then 100 measured calls.

## Result

Five runs of the same unoptimized test build on one macOS arm64 host, shared with other
builds (1-minute load average in the first column):

| Host load | p50 | p95 | max |
|---|---|---|---|
| ~25 | 0.62 ms | 0.67 ms | 0.72 ms |
| ~35 | 0.85 ms | 10.51 ms | 35.68 ms |
| ~32 | 2.48 ms | 7.28 ms | 45.92 ms |
| ~31 | 2.08 ms | 8.91 ms | 16.40 ms |
| ~31 | 2.09 ms | 8.39 ms | 66.96 ms |

The target in #351 is p95 ≤ 10 ms warm. Four runs meet it, and one is 0.5 ms over at the host's
busiest. The spread follows host contention, not the code. A warm call spawns no process (one
spawn costs >20 ms on this host), and the outliers are scheduling stalls. The test keeps a 5×
tripwire (50 ms) for that reason. A release build, or an idle host, sits at the low end.

## Why the design is spawn-free in the steady state

The first cut followed the issue's sketch: `git rev-parse HEAD` per call, plus a `git status` for
new files, plus `git diff --cached`. On the same host it measured **p50 43.8 ms / p95 87.6 ms**.
A bare `git --version` spawn alone costs ~21 ms p50 there, so the per-call process was the cost,
not the work. The issue's falsifier 3 fired.

The probe now spawns nothing on a warm call:

- **HEAD** is read from the git files every call: `.git/HEAD`, the ref, the common dir of a linked
  worktree, then `packed-refs`. It is exact; a commit is seen on the next call. `git rev-parse`
  is the fallback only for a ref format the reader does not handle.
- **The `rev-list` count** behind HEAD is cached on `(root, HEAD, graph_version)`.
- **The new-file list** (`git status --porcelain=v2 -z --untracked-files=normal`, with
  `GIT_OPTIONAL_LOCKS=0` so it never takes `index.lock`) is re-run when any of these changes:
  HEAD; the `.git/index` stamp (stage/commit); the mtime of any directory that holds an indexed
  file, or of the root (adding a file changes its directory's mtime). It is also re-run at least
  every 10 s. That last bound covers the key's one blind spot: a new source file in a directory
  tree that holds no indexed file yet.
- **File drift** is a `stat` per touched file, and a re-hash only when `(mtime, size)` changed.

Re-run the bench after any change to `freshness.rs` and update this table.
