# Lane X — the cross-edge overlay (`xedge`) — DESIGN v3 (FINAL)

> **Foundation HOLDS (do not relitigate): DEC-1 — SEPARATE STORES + a first-class single-writer
> cross-edge overlay (`xedge.db`).** Each engine (estate/memory/knowledge) keeps its OWN file +
> OWN single writer; cross-domain edges live in a tiny dedicated single-writer overlay keyed on
> `(engine, stable-id, epoch)`. The seam HOLDS: an **in-proc `OverlayReader: GraphRead`** wraps the
> home engine's `&dyn GraphRead` and drives foreign engines' async pools synchronously. The
> **panic worry is DISPROVEN** — traced in tokio 1.52.3 source, the seam *runs* (mechanism below).

**THE GOVERNING RULE OF THIS DOCUMENT.** v1 and v2 each failed their antagonist by **over-claiming
— treating designed-and-specified as already-solved.** v3 wins by the opposite discipline:
**every empirical claim is tagged either PROVEN-IN-DESIGN (a fact already true in committed code,
cited file:line) or BUILD-GATE (a thing the design does NOT claim to pass; it specifies the EXACT
acceptance test + ceiling and explicitly defers it to the build).** The DoD table tags every row.
Nothing below claims a runtime property (no-deadlock-under-load, no-bench-regression, reuse-safety)
is solved. Those are gates, written with their falsifier and ceiling, sequenced BEFORE the work
they protect. This is honest deferral-with-a-gate, not a re-derivation.

DESIGN ONLY. Every code claim cites real code, re-verified at the cited line for this round. v3
supersedes v2's DEC-X1 over-claim (the Postgres precedent), folds the v2-gate findings #1–#5 as a
spec, and is framed to **converge to a clean build gate, not loop**.

---

## 0 · What this round changes vs v2 (lead with the retraction)

| v2 said | v3 says | why |
|---|---|---|
| "DEC-X1 is **already solved in-tree** — `PostgresStore::rt_block` drives async from exactly such a blocking context (`postgres.rs:63-74`)." | **RETRACTED (false).** The ONLY `impl AsyncGraphStore` is `SqlitePool` (`pool.rs:74`); **no `PostgresStore` is ever placed behind `with_read`/`spawn_blocking`** (grep: zero hits). `rt_block` runs on tokio **worker threads or the global rt**, never on a `spawn_blocking` blocking-pool thread. It is NOT the OverlayReader's situation. | over-claim #1b |
| the seam is "already solved" | the seam's **mechanism is PROVEN-IN-DESIGN by source trace** (tokio 1.52.3, below); the seam **running under real concurrent load with bounded threads is a BUILD-GATE** (DoD-X1, DoD-X1b). The two are now separated. | governing rule |
| `recall()` is fine (graph arm gated by caller) | **FOLD #2 now:** `recall()` defaults `cross_edge_kinds = ["about"]` (opt-OUT for recall; opt-IN for code-graph tools). A naive `recall()` over the separate-store fixture MUST surface the `about` doc. | finding #2 |
| double-`spawn_blocking` is fine | **FIX #1c:** the inner foreign `with_read` runs **INLINE on the held blocking thread** (do NOT re-enter `spawn_blocking`); `max_blocking_threads` set explicitly with a documented floor; **a concurrency stress-test is a BUILD-GATE.** | finding #1c |
| bench gate "is a hard blocker" (prose) | **#4 hardened:** the bench fields, mixed corpus, and exact ceiling assertions are a **HARD LANDING GATE built BEFORE OverlayReader.** The design does NOT claim to pass it. | finding #4 |
| `symbols.gen` exists "default impl returns `Some(0)`" | **#5 hardened:** `symbols.gen`'s bump is specified CONCRETELY (0→1 node-count-transition counter); Lane A's `symbol_epoch`+`symbols.gen` is SEQUENCED BEFORE the about-arm is claimed reuse-safe (hard gate); put-time TOCTOU closed inside the single-writer txn; DoD-X4 asserts a **NON-ZERO** post-reuse gen. | finding #5 |
| `traverse_multi` default fold is acceptable | **#3 (ADV) folded:** the **SqliteStore `traverse_multi` specialization must land in the same change**, with a **sub-linear-in-CTE-count perf assertion** so equality-only conformance can't green-light the slow Mem/Postgres fold. | finding #3 |

**Everything in v2 NOT listed above still stands** and is not repeated: own-file `xedge.db` +
single writer (D-X1); `XedgeStore`/`XedgeReader` are not a `GraphStore` (D-X2); union runs behind
`&dyn GraphRead` via `OverlayReader` (D-X3); xedge rows convert to `core::Edge` so every formatter
works unchanged (`edge_json`, `wicked-estate-retrieve/src/lib.rs:108`); the 28-method delegation
table (DEC-X7); the boundary-vs-`injected:*` litmus; the event-driven prune over the pinned catalog.

---

## 1 · Grounding re-verification (every cite re-checked at the line for v3)

| claim | code (re-verified) | status |
|---|---|---|
| The MCP injects retrieval through ONE `with_read` seam, closure takes `&dyn GraphRead` | `store.with_read(move |graph| Ok(handle_request_ctx(graph, &req, &ctx_clone)))` (`wicked-estate-mcp/src/main.rs:316`) | TRUE |
| `with_read` runs the closure inside `spawn_blocking` | `tokio::task::spawn_blocking(move || f(&*obj)).await` (`pool.rs:83`); `F: FnOnce(&dyn GraphRead) -> Result<T> + Send + 'static` (`traits.rs:214-217`) | TRUE |
| `GraphRead` is **sync** | `pub trait GraphRead: Send` (`traits.rs:74`); 27 sync methods, single-start `traverse(&self, start, spec)` (`traits.rs:84`) | TRUE |
| The ONLY async serving impl is the SQLite pool | `impl AsyncGraphStore for SqlitePool` (`pool.rs:74`) is the sole impl in the tree | TRUE |
| **No Postgres is ever pooled / behind `with_read`** | grep for `PostgresStore` near `pool`/`with_read`/`spawn_blocking`/`AsyncGraphStore`: **zero hits**; `rt_block`'s fast arm requires `RuntimeFlavor::MultiThread` worker context (`postgres.rs:67-73`), the doc itself says `block_in_place` "panics on single-threaded runtimes" (`postgres.rs:59-61`) | **v2's "Postgres precedent" is FALSE → DELETED** |
| `block_in_place` from a `spawn_blocking` thread does NOT panic | tokio 1.52.3 `worker.rs:432-436`: context is `(EnterRuntime::NotEntered, is_some()==false)` → "We are outside of the tokio runtime, so blocking is fine … skip all of the thread pool blocking setup" → early `Ok(())`, runs `f` directly | **mechanism PROVEN-IN-DESIGN (source trace); runtime behavior under load = BUILD-GATE** |
| empty `edge_kinds` ⇒ ALL kinds (not opt-out) | `if spec.edge_kinds.is_empty() { String::new() } else { … AND e.kind IN (…) }` (`sqlite.rs:931-944`) | TRUE → separate field (DEC-X3) |
| `traverse` induces edges via `neighbors` per anchor | `for a in &anchors { for e in self.neighbors(a, spec.direction)? … }` (`sqlite.rs:1680-1686`); `cte_reach` is a single-DB integer-sid CTE (`sqlite.rs:904-982`) | TRUE → 1-hop folds, N-hop needs `traverse_multi` |
| estate intern is append-only (no delete, no generation) | `INSERT INTO symbols(sym) VALUES(?1) ON CONFLICT(sym) DO NOTHING` (`sqlite.rs:176-190`); interned per node on the write path (`sqlite.rs:1006-1009`); `SymbolId` is a pure logical-name-path string (`symbol.rs:1-7,107-109`); rename → NEW id (`symbol.rs:217-231`) | TRUE → epoch needed (DEC-X6) |
| mem-id is a uuid_v7 minted once, never reused | `id: uuid::Uuid::now_v7().to_string()` (`wicked-memory-core/src/lib.rs:140`); `Symbol::synthetic("mem", uuid)` (`:156-158`) | TRUE → memory epoch constant 0 |
| `recall()` does NOT pass cross-edges; graph arm reads memory.db | `recall()` hardcodes `RecallMode::Hybrid` (`wicked-memory/src/lib.rs:377-393`); graph arm = `about_seed_ids(seeds)` → `self.store.neighbors(code, Dependents)` filtered `"about"` over memory.db (`:359-372,443-447`); `GraphOnly` returns nothing post-pivot (`:98-110`) | TRUE → FOLD #2 |
| `capture_about` writes the `about` edge into memory's own store | `self.store.upsert_edges(&edges)` (`wicked-memory/src/lib.rs:289-291`); `resolve_code` → `self.store.find_symbols` over memory.db (`:297-311`) | TRUE → DEC-X9 deletes it |
| the differentiator test is co-resident + tautological | `MemoryEngine::in_memory()` single store (`wicked-memory/src/lib.rs:624`); falsifier `!hit(&without) || with.len() >= without.len()` — 2nd arm trivially true (`:655-658`) | TRUE → DoD-X6 rebuild |
| the bench has NO query-latency percentile + code-only corpus | `search_latency_us`/`blast_radius_latency_us` are single-shot, measured once via one `Instant::now()` (`capability.rs:112,114,305,309-318`); `baseline_corpus()` = `ts-axios`/`py-flask`/`poly-tree-sitter` (`bench/lib.rs:98-119`); ceiling test asserts only footprint+throughput (`tests/integration_bench.rs:256-295`) | TRUE → BUILD-GATE (#4) |

---

## DECISIONS

### DEC-X1 — [finding #1b — THE RETRACTION + the honest mechanism] The hydration seam: in-proc foreign-pool map, driven by `Handle::block_on` inside `spawn_blocking`

**The problem, unchanged and exact.** Every retrieval tool takes `&dyn GraphRead` and the MCP
injects it through ONE seam — `with_read(move |graph| …)` (`main.rs:316`) — which for SQLite runs
the closure in `spawn_blocking(move || f(&*obj))` (`pool.rs:83`) under `F: … + Send + 'static`
(`traits.rs:214-217`). An `OverlayReader: GraphRead` (sync, `traits.rs:74`) whose
`neighbors`/`traverse_multi` must read a *foreign* engine cannot `.await` that engine's async pool —
it is itself on a blocking-pool thread.

**THE RETRACTION (governing rule).** v2 DEC-X1 claimed this was "already solved in-tree" because
`PostgresStore::rt_block` "drives async work from exactly such a blocking context." **That is false
and is DELETED.** Re-verified: the only `impl AsyncGraphStore` is `SqlitePool` (`pool.rs:74`); **no
`PostgresStore` is ever pooled or placed behind `with_read`/`spawn_blocking`** anywhere in the
tree. `rt_block` runs on tokio worker threads (its fast arm requires `RuntimeFlavor::MultiThread`,
`postgres.rs:67-73`) or the process-global runtime — **never on a `spawn_blocking` blocking-pool
thread.** There is no Postgres precedent for the nesting the OverlayReader needs. The design does
not get to borrow that confidence.

**THE REAL MECHANISM (PROVEN-IN-DESIGN by source trace, NOT by precedent).** From a `spawn_blocking`
thread the tokio runtime context is **`NotEntered`** and the thread is not a runtime worker
(`maybe_cx.is_some() == false`). In tokio **1.52.3**, `block_in_place` handles exactly this case at
`worker.rs:432-436`:

```rust
// tokio-1.52.3/src/runtime/scheduler/multi_thread/worker.rs:432-436
(context::EnterRuntime::NotEntered, false) => {
    // We are outside of the tokio runtime, so blocking is fine.
    // We can also skip all of the thread pool blocking setup steps.
    return Ok(());            // ← early return: block_in_place is a no-op wrapper, NO panic
}
```

So inside `spawn_blocking`, `block_in_place(f)` is effectively `f()` with no setup and **no panic**;
a subsequent `Handle::block_on(future)` then **parks this blocking-pool thread** until the foreign
future completes. The seam therefore RUNS. **This is a mechanism claim grounded in source — it is
PROVEN-IN-DESIGN.** It is NOT a claim that the seam is correct under concurrent load (that is
DEC-X1b's BUILD-GATE) and it does NOT rest on the (false) Postgres precedent.

> **What "PROVEN-IN-DESIGN" buys and does not buy here.** It rules OUT the panic the v2 reviewer
> flagged as the load-bearing risk. It does NOT rule out blocking-pool starvation/deadlock under N
> concurrent cross-recalls — that is a thread-budget property, addressed by DEC-X1b and gated by
> DoD-X1b. The design explicitly does not claim to pass DoD-X1/DoD-X1b.

**The `'static` bound is satisfied** — the `OverlayReader` is constructed *inside* the closure,
capturing only owned/`Arc` data; `graph` (the home `&dyn GraphRead`) is borrowed for the closure's
own non-`'static` body:

```rust
// at the MCP seam (main.rs:316), home_engine = "estate":
let others = ctx.foreign_pools.clone();      // Arc<HashMap<&'static str, Arc<dyn AsyncGraphStore>>>
let xedge  = ctx.xedge_reader.clone();       // XedgeReader: cheap WAL clone, owned
store.with_read(move |graph| {               // graph: &dyn GraphRead (estate — HOME)
    let overlay = OverlayReader { home: graph, home_engine: "estate", xedge, others,
                                  cross_edge_kinds, budget };
    Ok(handle_request_ctx(&overlay, &req, &ctx_clone))
}).await?;
```

**Rejected (unchanged):** a read-RPC per engine for v1 — it adds a network hop + (de)serialization
to the *inner* loop of recall, strictly worse for the in-process / co-located deployment that ships
today (`wicked-memory/src/lib.rs:1-6`). RPC is the right seam when engines are on separate hosts;
that is DEC-X8-deferred, gated behind a `StoreCapabilities`-style negotiation.

### DEC-X1b — [finding #1c — the FIX + a BUILD-GATE] Collapse the double-`spawn_blocking`; the foreign read runs INLINE on the held thread; set `max_blocking_threads` with a documented floor

**The hazard, stated honestly.** If `OverlayReader`, already on estate's `spawn_blocking` thread,
calls a foreign pool's `with_read`, that pool *also* runs the inner closure in `spawn_blocking`
(`pool.rs:83`). So a single cross-recall consumes **estate's blocking thread (parked on `block_on`)
PLUS one blocking thread per foreign engine** — held simultaneously. Under N concurrent
cross-recalls the blocking pool can starve: tokio's default blocking pool is bounded (512), but the
*held-while-parked* pattern multiplies occupancy and is the classic pool-exhaustion deadlock shape.

**The FIX (design).**
1. **Do NOT re-enter `spawn_blocking` for the inner foreign read.** The OverlayReader does not call
   the foreign pool's `with_read` (which would nest `spawn_blocking`). It checks out a foreign
   connection and runs the foreign `GraphRead` closure **INLINE on the thread it already holds**.
   Concretely, add to `AsyncGraphStore` a sibling the overlay uses from a blocking context:
   ```rust
   // crates/wicked-estate-core/src/traits.rs — AsyncGraphStore (sibling of with_read)
   /// Acquire a connection and run `f` on the CURRENT thread (no spawn_blocking). Intended for
   /// callers already on a blocking-pool thread (the OverlayReader), so a cross-engine read does
   /// not consume a SECOND blocking-pool thread per engine. For `SqlitePool`: `get().await` then
   /// run `f` directly. The `get().await` is the only await; it is driven by the overlay's single
   /// `Handle::block_on`.
   async fn with_read_inline<F, T>(&self, f: F) -> Result<T>
   where F: for<'a> FnOnce(&'a dyn GraphRead) -> Result<T> + Send + 'static, T: Send + 'static;
   ```
   The OverlayReader does ONE `Handle::block_on(async { foreign.with_read_inline(|g| …).await })`
   per foreign engine per ply — the connection checkout is the only awaited step; the read itself
   is synchronous on the held thread. Net blocking-pool occupancy per cross-recall drops from
   `1 (estate) + k (foreign)` to **`1 (estate, parked on block_on)`**; foreign reads borrow a
   pooled *connection*, not a *blocking-pool thread*.
2. **Set `max_blocking_threads` explicitly with a documented floor.** The MCP runtime sets
   `max_blocking_threads` to a value `≥ expected_peak_concurrent_recalls + headroom`, documented as
   an operational bound (OQ-X10). Floor rationale: each in-flight cross-recall parks exactly one
   blocking thread on `block_on`; the floor must exceed peak in-flight recalls so the pool that
   serves estate's own `with_read` is never fully occupied by parked overlay threads.

**BUILD-GATE (the design does NOT claim to pass this).**
- **Test:** a concurrency stress-test — **N concurrent cross-recalls** (N swept past
  `max_blocking_threads`) through a real multi-thread MCP runtime, each crossing code→memory, each
  forced to actually hydrate a foreign endpoint.
- **Acceptance:** **(a) zero deadlocks / zero timeouts** across the sweep (every recall completes
  within a fixed wall-clock budget); **(b) bounded threads** — the process's tokio blocking-thread
  high-water mark stays `≤ max_blocking_threads` (assert via a metric / `tokio-metrics`), i.e. the
  inline fix actually prevented the `1+k` multiplication.
- **Ceiling:** N at least `2 × max_blocking_threads` with `max_blocking_threads` set to a small test
  value (e.g. 8) so the test genuinely exercises saturation, not headroom.
- **Tag:** **BUILD-GATE.** Until this is green, "no deadlock under load" is an argument, not a fact.

### DEC-X2 — [#3 ADV folded] `traverse_multi` on `GraphRead`; **SqliteStore specialization lands in the SAME change**; a sub-linear perf assertion guards the fold

`GraphRead` gains a multi-start primitive (the 28th method) so a cross-engine frontier is one CTE
per ply per engine, not O(frontier) single-start CTEs (each re-inducing edges via `neighbors` per
anchor, `sqlite.rs:1680-1686`):

```rust
// crates/wicked-estate-core/src/traits.rs — GraphRead (28th method)
/// Bounded traversal from MANY starts in one call. Default folds over `traverse` so existing
/// backends stay CORRECT until they specialize; SqliteStore MUST specialize (see below).
fn traverse_multi(&self, starts: &[SymbolId], spec: &TraversalSpec) -> Result<Subgraph> {
    let mut acc = Subgraph::default();
    for s in starts { merge_subgraph(&mut acc, self.traverse(s, spec)?); }
    Ok(acc)
}
```

**The #3 fold — equality-only conformance is not enough.** A pure equality conformance test
(`traverse_multi ≡ union of traverse`) is GREEN for the **slow default fold** on MemStore/Postgres,
which is the N+1 the gate flagged. So:

1. **SqliteStore specializes `traverse_multi` in the SAME change that adds the method** (§1/§8 — the
   spine change lands complete, not "specialize later"). The specialization seeds the recursive
   CTE's base case with all `starts`' sids (`SELECT sid FROM symbols WHERE sym IN (…)` then `UNION`
   the walk, mirroring `cte_reach`, `sqlite.rs:904-982`) — ONE `WITH RECURSIVE` for the whole
   frontier; edge induction batches to one `SELECT data FROM edges WHERE source IN (…)`.
2. **Conformance kit gets `traverse_multi_matches_union_of_traverse`** (equality) for MemStore,
   SqliteStore, PostgresStore, OverlayReader — correctness floor (DoD-X2).
3. **PLUS a perf assertion the equality test cannot give (DoD-X2b):** over a **wide frontier** (W
   starts), SqliteStore's `traverse_multi` issues a **CTE/recursive-query count that is sub-linear
   in W** (target: a small constant — one base CTE per ply, independent of W), asserted by counting
   prepared-statement executions (a SQLite trace hook / a counting connection wrapper). **Ceiling:**
   query-count(`traverse_multi(W starts, 1 ply)`) `≤ C` for a fixed small `C` (e.g. ≤ 3), **for all
   W in the sweep** — so it cannot scale with frontier width. This is the assertion that prevents
   equality-only conformance from green-lighting the slow Mem/Postgres fold as if it were the
   shipping cross path.

> **Honest scope:** MemStore/Postgres keep the **correct-but-slow default fold**; they are NOT on
> the differentiator's hot path (SQLite is the local default). The perf assertion is on SqliteStore,
> the engine that actually serves the cross-walk. Raising `max_cross_hops` (DEC-X4) above 1 is
> separately gated on the bench (DEC-X5) — so a slow fold can never silently ship as the cross path.

The `OverlayReader::traverse_multi` orchestrator (per-engine multi-seed CTE per ply, xedge boundary
between plies, shared `max_nodes` budget + `max_cross_nodes` cap, epoch-validated) is unchanged from
v2 DEC-X2 and not repeated.

### DEC-X3 — [finding #2 mechanics + R-X1] Default-OFF cross-edge gate for CODE tools; the field is a NEW inverted `TraversalSpec` field

`TraversalSpec` gains a separate, inverted field (empty = NONE), the inverse of `edge_kinds`
(empty = ALL, `sqlite.rs:931-944`), plus two budgets — unchanged from v2:

```rust
// crates/wicked-estate-core/src/query.rs — TraversalSpec (NEW Lane X fields)
pub cross_edge_kinds: Vec<String>,  // xedge rels to cross. EMPTY = NONE (inverse of edge_kinds).
pub max_cross_hops: u32,            // Default 1 (DEC-X4).
pub max_cross_nodes: usize,         // Default 64. Cap nodes pulled across the boundary per recall.
```

`Default` sets `cross_edge_kinds: vec![]`, `max_cross_hops: 1`, `max_cross_nodes: 64`. The **3 owned
code-graph tool changes** — `BlastRadius` (`lib.rs:800`, `TraversalSpec::blast_radius`),
`ContextPack` (`render_context`'s `neighbour_spec` `lib.rs:1297-1303`; `invoke` `lib.rs:1426`),
`ContextBundle` (raw `neighbors` calls `context_bundle.rs:182,190`) — are cross-OFF by default and
gain an explicit opt-IN `cross_edge_kinds` request field (parsed like `parse_edge_kinds`,
`lib.rs:617`). `TraverseGraph` (`lib.rs:643`) gains the same opt-in parse (a 4th, purely additive
touch). **This is the code-tool side: opt-IN.** (The honest caveat from the v2 gate: explicit
`TraversalSpec { … }` literals won't compile until the 3 new fields are set — this fails SAFE, it
does not silently fan; construction sites using `..Default::default()` are cross-OFF unchanged.)

### DEC-X3b — [finding #2 — FOLD NOW] `recall()` defaults `cross_edge_kinds = ["about"]` — opt-OUT for recall

**The bug, one layer down (grounded).** `recall()` hardcodes `RecallMode::Hybrid`
(`wicked-memory/src/lib.rs:377-393`) and never sets a cross gate; its graph arm is
`about_seed_ids(seeds)` (`:443-447`). If the cross gate defaulted OFF for recall the way it does for
code tools (DEC-X3), a **naive `recall()` from a code seed would surface NO `about` docs** — v1's
"differentiator OFF by default" recreated inside recall. The differentiator is recall's entire
unique bet; for recall, cross-`about` is the default behavior, not an opt-in.

**Decision (fold now, in design).** `recall()` and `recall_mode(Hybrid|GraphOnly)` **default
`cross_edge_kinds = ["about"]`**. Recall is **opt-OUT** (a caller may pass `[]` to suppress the
graph arm, e.g. for the cross-OFF bench baseline); code-graph tools (DEC-X3) stay **opt-IN**. This
is the asymmetry finding #2 demands: the overlay's value shows up the moment someone calls `recall`,
without a flag.

- **DoD-X3b (PROVEN-IN-DESIGN that it's a 1-line-class fold; the *behavior* is a BUILD-GATE test):**
  a **NAIVE `recall(query, scope, &[code_seed], budget, now)`** — no extra arguments — over the
  separate-store fixture (DEC-X9) surfaces the `about` doc. **Falsifier:** delete the `about` row
  from `xedge.db` → the same naive `recall()` does NOT surface it. (This is the recall-level twin of
  DoD-X6; it asserts the *default* path, not an opted-in one.)

### DEC-X4 — [R-X1] `max_cross_hops` default = 1, hard-gated above

Memory/knowledge are leaves off a code seed (`about_seed_ids` is one hop,
`wicked-memory/src/lib.rs:359-372`). **Default `max_cross_hops = 1`.** `>1` is gated on DEC-X5
showing the 2-hop latency first. Unchanged from v2.

### DEC-X5 — [finding #4 — a HARD LANDING GATE built BEFORE OverlayReader] Bench: query-latency p95 + cross-engine recall@k over a NEW mixed corpus

**This is the round's sharpest honesty point. The design does NOT claim to pass any bench. It
specifies the gate, its fields, its corpus, and its exact ceiling assertions, and orders it FIRST.**

Verified gaps (today): no percentile — `search_latency_us`/`blast_radius_latency_us` are single-shot
(`capability.rs:112,114`, one `Instant::now()` at `:309-318`); code-only corpus (`bench/lib.rs:98-119`);
the ceiling test asserts only footprint+throughput (`tests/integration_bench.rs:256-295`). A
cross-engine join is **unmeasurable** on what exists.

**Spec — three artifacts, all GREEN before `OverlayReader` is written:**

1. **`xedge_query_latency_p95_us`** — a NEW `RepoMetrics` field (alongside `search_latency_us`,
   `capability.rs:112`). **Driver:** run the differentiator query (naive `recall()` from a code
   seed, DEC-X3b) over **N ≥ 200 seeds** sampled from the mixed corpus; collect the per-call latency
   distribution; report **p50/p95/p99**.
   - **Ceiling assertion A (absolute):** `xedge_query_latency_p95_us ≤ CEIL_P95` on the fixture,
     where `CEIL_P95` is frozen in the ceiling test (the `tests/integration_bench.rs:256` pattern,
     extended). Initial value set loose (catch catastrophic regressions), tightened as the join
     optimizes — exactly the documented "tighten as optimisations land" discipline
     (`capability.rs:910`).
   - **Ceiling assertion B (no-regression):** with `cross_edge_kinds = []` (cross-OFF), the recall
     p95 must be **statistically indistinguishable from the pre-overlay baseline** (within a frozen
     delta `Δ`) — because DEC-X3/DEC-X3b's opt-OUT-for-recall path with `[]` must not move the
     intra-engine numbers.
2. **`cross_engine_recall_at_k`** — a NEW `RepoMetrics` field. Fraction of gold (memory/knowledge)
   items surfaced from a code seed within budget, over a labeled set (the corpus analogue of
   `ArmMetrics.answer_file_recall`, `bench/lib.rs:46`).
   - **Ceiling assertion C (lift, with margin M):** recall@k with cross-edges **ON** must **exceed**
     recall@k with cross-edges **OFF by a frozen margin `M > 0`** (`recall_on ≥ recall_off + M`) —
     else the overlay buys nothing. `M` is frozen in the test.
3. **A NEW mixed corpus** — extend `RepoSpec`/`baseline_corpus` (`bench/lib.rs:23-28,98-119`) with a
   fixture carrying REAL `about` rows: index `py-flask` into estate.db, a frozen checked-in seed of
   memories `about` Flask symbols (`corpus/xedge-seed.jsonl`) into memory.db, the `about` edges into
   xedge.db. The bench builds the three stores + `xedge.db`, wraps with `OverlayReader`, runs (1)+(2).
   - **Sequencing note:** (1)+(2) need a runnable `OverlayReader` to *exercise* the cross path, but
     the **corpus + the metric fields + the ceiling assertions (the gate harness)** are built FIRST,
     as the landing target the OverlayReader must satisfy. "Build the gate, then build to the gate"
     — the gate is not retrofitted after a green OverlayReader (which is exactly how v1's inert eval
     shipped a slow/wrong join green).

**Tag: BUILD-GATE — a HARD blocker on Lane X landing.** Until `xedge_query_latency_p95_us` (ceilings
A+B) and `cross_engine_recall_at_k` (ceiling C) are implemented over the mixed corpus and green, the
cost/lift model is an argument, not a measurement. Parallel to PR-14's hybrid-uplift bench
(`wicked-memory/src/lib.rs:88-100`).

### DEC-X6 — [finding #5 — concrete bump + TOCTOU close + de-vacuous DoD] Identity epoch fails CLOSED

**The trap, grounded (unchanged).** `SymbolId` is a pure name-path (`symbol.rs:1-7,107-109`); intern
is append-only with no delete and no generation (`sqlite.rs:176-190`, interned per node at
`:1006-1009`). Delete-a-symbol-then-re-add-the-same-name re-creates the SAME `SymbolId` string → an
old xedge row keyed on that string resolves to a LIVE-but-possibly-different node — confidently
wrong, silently (violates R7). v1's `None` backstop is blind (the node exists).

**Decision — carry a generation in the xedge key, validated at read** (schema unchanged from v2:
`src_epoch`/`tgt_epoch` columns in the PK). The v3 hardening over v2:

1. **`symbols.gen`'s bump mechanism, specified CONCRETELY (pick ONE, this design picks the
   counter).** estate adds a `gen INTEGER NOT NULL DEFAULT 0` column to `symbols` and a
   **node-count-transition counter**: `gen` for a `sid` is incremented **on the 0→1 live-node
   transition** — i.e. when a symbol name that currently has **no live node** (its node was removed
   by `remove_file`, `traits.rs:150-152`, leaving the interned `sym` row but no `nodes` row) gets a
   node again. Mechanism: `upsert_nodes` checks, per interned sid, whether a live `nodes` row exists;
   if not (this is a re-add after removal), `gen += 1` before insert. A first-ever intern leaves
   `gen = 0`; a reuse-after-removal yields `gen ≥ 1`. (The rejected alternative — a tombstone row on
   `remove_file` — is equivalent; the counter is chosen because it needs no new table and the
   transition is observable at `upsert_nodes` time, where intern already runs, `sqlite.rs:1006`.)
   **This is NOT "default impl returns `Some(0)`"** — the bump is a real state transition.
2. **Expose it:** `GraphRead::symbol_epoch(&SymbolId) -> Result<Option<u64>>` returns the current
   `gen` for a live symbol, `None` if no live node. **Lane A change** (callout).
3. **Read-time validation (fail-closed, loud) — unchanged from v2:** `xedge.expand` /
   `OverlayReader::neighbors` fetch each endpoint's CURRENT epoch (`symbol_epoch` for estate;
   constant 0 for memory, uuid_v7) and **drop the row if `row.epoch != current.epoch`**, emitting
   `XEDGE-STALE-EPOCH: edge to estate:<id> dropped (row gen=N, live gen=M — id reused; prune queued)`.
4. **Put-time TOCTOU closed (NEW in v3).** v2 stamped the endpoint epoch by reading `symbol_epoch`
   at write time — but between that read and the xedge INSERT, a concurrent estate reindex could
   bump `gen`, stamping a row already stale at birth. **Fix:** the epoch read and the xedge INSERT
   happen **inside the xedge single-writer transaction**, and the write **re-validates the endpoint
   epoch as the last step before commit** (read `symbol_epoch` again inside the txn; if it changed
   since the value being stamped, abort+retry the put). Because xedge has a single writer (D-X1), the
   re-read+insert is serialized on the xedge side; the residual race (estate bumps mid-txn) is closed
   by the re-validate-before-commit. A row is never committed with a known-stale epoch.
5. **DoD-X4 de-vacuoused (NEW in v3).** v2's DoD-X4 could pass vacuously at `epoch = 0` (if `gen`
   never bumps, `row.epoch == current.epoch == 0` always). v3's DoD-X4 **asserts a NON-ZERO
   post-reuse gen**: after delete+re-add, `symbol_epoch(id)` MUST return `Some(g)` with `g ≥ 1`, the
   old xedge row (stamped `epoch=0`) MUST be dropped with `XEDGE-STALE-EPOCH`, and MUST NOT resolve
   to the live node. A green DoD-X4 now *proves the bump fired*, not just that equal epochs match.

**Tag:** the schema + read-time fail-closed logic is **PROVEN-IN-DESIGN** (it's a deterministic
drop on inequality). The **bump actually firing + TOCTOU close + non-zero gen** is a **BUILD-GATE
(DoD-X4)** AND depends on the Lane A `symbols.gen`/`symbol_epoch` landing (sequencing below). Until
Lane A ships, estate epochs are constant 0 and reuse-detection is INERT for estate endpoints
(memory is safe, uuid_v7) — **the design does NOT claim the about-arm is reuse-safe before that
sequence completes.**

### DEC-X6-SEQ — [finding #5 — the SEQUENCE as a hard gate, not a callout] Lane A's `symbol_epoch` + `symbols.gen` land BEFORE the about-arm is claimed reuse-safe

This is promoted from v2's prose callout to a **hard sequencing gate**:

> **GATE:** the about-arm (DEC-X9 — `capture_about` redirect writing estate-endpoint xedge rows)
> MUST NOT be claimed reuse-safe, and SHOULD NOT land as "done," until **Lane A has shipped
> `symbols.gen` (with the 0→1 bump, DEC-X6.1) + `GraphRead::symbol_epoch` (DEC-X6.2), green in
> estate's conformance kit.** Order: **(1) Lane A epoch → (2) xedge put-time stamping + TOCTOU close
> → (3) about-arm + DoD-X4.** Landing the about-arm first ships the silent-wrong-node bug live on the
> first-shipping arm. The about-arm may land *functionally* before Lane A (it works; epochs are 0),
> but its DoD row is **BUILD-GATE: reuse-safety DEFERRED until DEC-X6-SEQ step (1) is green** — and
> the design says so explicitly rather than implying reuse is handled.

### DEC-X7 — [R-X1] The 28-method `OverlayReader` delegation table — UNCHANGED from v2

`OverlayReader: GraphRead` implements all 28 methods. FOLD cross-engine: `neighbors` (gated),
`traverse_multi` (gated), `traverse` (→`traverse_multi`), `get_node` (ROUTE by engine tag).
HOME-ONLY: `find_symbols`, `all_nodes`/`all_edges` (PageRank stays home — `ranked_symbols`,
`lib.rs:955,1315`, `context_bundle.rs:203`), `find_by_requirement`, `unresolved_refs_for_name`,
`node_semantics`/`annotations*`, file/content/provenance reads, `changes_since`, `stats`.
`capabilities()` reports home caps but forces `server_side_traversal=false` for the cross path. Full
table and rationale in v2 DEC-X7; not repeated. (DoD-X8.)

### DEC-X8 — [R-X5] Real, bounded reconcile: net-new `xedge-reconcile` subscriber + dirty-set prune — UNCHANGED from v2

A net-new `xedge-reconcile` subscriber **reuses** the cursor-poll MECHANICS (durable cursor + TTL
self-heal + DLQ + dedup, the `memory-subscriber.mjs` pattern) but is a distinct subscriber with its
OWN cursor on the pinned coarse events. Deletion-reconcile is **dirty-set incremental** via
`changes_since(cursor)` (`traits.rs:109-111`) → intersect the dirty file-set with xedge rows
referencing estate ids in those files → re-validate only those (live+same-epoch keep; vanished or
epoch-bumped → prune). Work is O(rows touching changed files), NOT O(xedge_rows). Read-time backstop
(DEC-X6.3) is the second loud layer. Full spec in v2 DEC-X8. (DoD-X7.)

### DEC-X9 — [R-X3 / C-X-3] Atomic about-redirect on a genuinely separate-store fixture — UNCHANGED mechanics, sequenced by DEC-X6-SEQ

ONE atomic change (§8): (1) `capture_about` (`wicked-memory/src/lib.rs:270-293`) **DELETEs** the
in-store `self.store.upsert_edges(&edges)` (`:289-291`) and writes
`(memory, mem.symbol(), 0) --about--> (estate, code_sid, current_epoch)` into `xedge.db`; (2)
`resolve_code` (`:297-311`) redirects from memory.db to **estate's read API** (the `others` map,
`home_engine="memory"`); (3) `about_seed_ids` (`:359-372`) redirects to
`xedge.in_edges("estate", code_id, ["about"])`, epoch-validated. `recall_impl`'s fuse/scope/budget
(`:408-495`) unchanged.

**Rebuilt test `cross_edge_lifts_recall_from_xedge`** (replaces the tautological
`cross_edge_lifts_recall_the_unique_bet`, `:618-659`, whose `with.len() >= without.len()` arm at
`:656` is trivially true): a **separate-store fixture** (estate `SqliteStore` + distinct memory
store + real `xedge.db`, via `OverlayReader`) with **non-tautological** assertions — (1) WITH seed +
default cross → the idempotency memory IS recalled; (2) WITHOUT seed → NOT; (3) **the lift is
sourced from xedge** — present with the xedge row, ABSENT with it deleted (the falsifier that fails
if the overlay is unwired); (4) memory.db `neighbors(code, Dependents)` returns empty (proves the
redirect, not a co-resident fallback). Landed in the SAME change as the `capture_about` delete (§8).
(DoD-X6.)

---

## RATIONALE

- **RATIONALE-1 (own file beats estate-table) — UNCHANGED.** A cross-edge table in estate.db forces
  memory+knowledge to write estate.db → the ≥3-writers-one-file bug DEC-1 escaped
  (`design-gate-verdicts.md:24`). Own file = structurally impossible.
- **RATIONALE-2 (union at retrieval) — UNCHANGED.** One `&dyn GraphRead` seam (`main.rs:316`);
  wrapping that reference (`OverlayReader`) is the spine-respecting move.
- **RATIONALE-3 (reuse `block_in_place`+`block_on`, don't async-ify the spine) — CORRECTED.**
  Async-ifying `GraphRead` would touch every store/tool/conformance test — the invasive refactor the
  DEC-1 reversal chose to AVOID (`design-gate-verdicts.md:64`). The seam reuses the runtime's escape
  hatch — **but the justification is the tokio 1.52.3 source trace (worker.rs:432-436), not the
  (deleted) Postgres precedent.** The mechanism is proven; the load behavior is gated (DEC-X1b).
- **RATIONALE-4 (separate default-OFF field is the bench guard, but recall is opt-OUT) —
  SHARPENED.** A separate `cross_edge_kinds` (empty=NONE) keeps code tools' intra-engine numbers
  byte-identical (DEC-X3); recall defaults `["about"]` because the differentiator is recall's whole
  point (DEC-X3b). The bench (DEC-X5) measures BOTH directions: cross-OFF must not regress (ceiling
  B), cross-ON must lift (ceiling C).
- **RATIONALE-5 (inline foreign read, not nested spawn_blocking).** The double-`spawn_blocking`
  multiplies blocking-pool occupancy (`1+k` threads held while parked); running the foreign read
  inline on the held thread (DEC-X1b) drops it to 1, the difference between a bounded pool and a
  starvation deadlock under load. Owning a `with_read_inline` sibling is cheaper than shipping the
  nesting and discovering the deadlock in production.
- **RATIONALE-6 (`traverse_multi` specialized + perf-asserted).** A multi-start CTE expands a whole
  frontier in one query; the per-start loop is O(frontier) CTEs (`sqlite.rs:1680-1686`). The
  specialization must land WITH the method (not later) and carry a sub-linear-in-W query-count
  assertion, because equality-only conformance is GREEN for the slow default fold — the exact #3 hole.
- **RATIONALE-7 (epoch carried, not hashed-in) — UNCHANGED.** Putting an epoch in the `SymbolId`
  string would churn estate's own identity on every reuse, breaking ADR-002. The epoch lives only in
  xedge's key.

---

## RISKS (each names its falsifier and whether it is a BUILD-GATE)

- **R-X1′ (cross-OFF for code tools must be provably inert) — mitigated, gated.** Rests on `Default`
  setting `cross_edge_kinds: vec![]` and code tools inheriting it. Falsifier = DoD-X3 (counting-fake-
  foreign-pool: zero foreign queries when cross-OFF). A clippy-style "no `cross_edge_kinds` set
  outside an opt-in parse" lint is a candidate CI guard. **Note the asymmetry:** recall is opt-OUT
  (DEC-X3b), so its falsifier (DoD-X3b) asserts the OPPOSITE — the `about` doc DOES surface by
  default, and vanishes when the xedge row is deleted.
- **R-X2′ (join cost unproven until the mixed-corpus bench exists) — BUILD-GATE, by design.** DEC-X5
  SPECS the gate with exact ceilings A/B/C; the design does NOT claim to pass it. **Lane X must not
  land before DEC-X5 is green.** This is the honest core of the round: the cost/lift question is
  empirical and deferred to a gate built first.
- **R-X3′ (`block_on`-inside-`spawn_blocking` under load) — panic DISPROVEN; starvation is a
  BUILD-GATE.** Panic ruled out by the source trace (worker.rs:432-436). Deadlock/starvation under N
  concurrent recalls is NOT ruled out by design — DEC-X1b's inline fix + `max_blocking_threads`
  floor is the mitigation, DoD-X1b (the stress-test: zero deadlocks + bounded threads) is the gate.
  Falsifier = the stress-test hangs or the blocking-thread high-water mark exceeds the cap.
- **R-X4′ (epoch is INERT until Lane A ships) — sequenced as a hard gate.** Until `symbol_epoch` +
  `symbols.gen` (with the real 0→1 bump) land, estate epochs are 0 and reuse-detection does nothing
  for estate endpoints (memory safe, uuid_v7). DEC-X6-SEQ makes this a **landing order**, not a
  hope; the about-arm's reuse-safety DoD row is explicitly BUILD-GATE-DEFERRED. Falsifier = DoD-X4
  (delete+re-add → non-zero gen → old row dropped).
- **R-X5′ (dirty-set reconcile needs `changes_since` to span the interval) — UNCHANGED.** The log is
  capped per call (`traits.rs:109-111`); the subscriber must page or miss dirty files. Mitigation:
  page the change-log; the read-time backstop (DEC-X6.3) is the late-but-loud safety net.
- **R-X6 (raw `xedge.db` bypass sees stale rows) — UNCHANGED.** No public raw-SQL consumer by
  design; all reads go through `XedgeReader`/`OverlayReader` (epoch + dangling validation). A future
  tool raw-SQLing `xedge.db` re-opens the silent-stale class (`design-gate-verdicts.md:38`).

---

## CROSS-LANE CALLOUT

**Exactly one condition forces a Lane A (estate) change; everything else folds inside Lane X.**

1. **Lane A — `GraphRead::symbol_epoch(&SymbolId) -> Result<Option<u64>>` + a `symbols.gen` column
   with the 0→1-transition bump** (DEC-X6.1/.2). estate's intern is append-only with no generation
   (`sqlite.rs:176-190`); reuse-detection needs estate to track + expose per-id generation. **This is
   now a SEQUENCING gate (DEC-X6-SEQ): it lands BEFORE the about-arm is reuse-safe.** Without it,
   estate-endpoint epochs are constant 0 and R-X4 stays open for those rows.
2. **Lane A — (optional fast-path) emit reused/removed sids** in `wicked.estate.indexed` / the
   change-log (DEC-X8 fast-path). Pure optimization; the dirty-file-set reconcile works without it.

**No Lane B (memory/knowledge) change is forced.** Memory's redirects (DEC-X9) live in
`wicked-memory`/`wicked-memory-mcp`, which Lane X owns for this seam; mem-id is reuse-safe (uuid_v7).
Knowledge's `mentions`/`governs` arms wait on OQ-X3 (knowledge id + reuse contract). **`traverse_multi`
(DEC-X2) is an estate SPINE change but it is Lane X's to make** — it lands before fan-out (§1), with
its SqliteStore specialization, in the same change.

---

## OPEN QUESTIONS (carried)

- **OQ-X3** — knowledge id + REUSE contract (feeds DEC-X6 knowledge epoch); only the `about` arm
  ships first (C-X-5 / DEC-X9).
- **OQ-X8** — single-writer mechanism for `xedge.db` (advisory file-lock + loud conflict, B-W-3
  style, vs a tiny writer actor); leaning advisory lock. Not load-bearing for the read-union.
- **OQ-X9** — `xedge.db` schema versioning (mirror memory's `meta`/`MEM_SCHEMA_VERSION`,
  `wicked-memory/src/lib.rs:58-59`); the epoch + TOCTOU columns make a `meta`/version row day-one.
- **OQ-X10** — `max_blocking_threads` default under concurrent cross-recall (DEC-X1b). DoD-X1b's
  stress-run picks the value + documents the floor.
- **DEC-X8-deferred** — read-RPC cross-engine when engines run on separate hosts (negotiated via a
  `StoreCapabilities`-style flag). Designed-not-built; out of scope for v1.

---

## DoD — every row tagged PROVEN-IN-DESIGN vs BUILD-GATE

> **PROVEN-IN-DESIGN** = a fact already true in committed code (cited), or a deterministic mechanism
> traced to source — the design stands on it now. **BUILD-GATE** = a runtime/empirical property the
> design does NOT claim to pass; the row states the exact test + ceiling + falsifier, deferred to the
> build and (where noted) sequenced before the work it protects.

| # | condition | acceptance test + ceiling / falsifier | TAG |
|---|---|---|---|
| DoD-X0 (mechanism) | seam does not panic | tokio 1.52.3 `worker.rs:432-436`: `(NotEntered, !is_some)` → early `Ok(())`; `block_in_place` from a `spawn_blocking` thread is a no-op, `block_on` parks. Source-traced. | **PROVEN-IN-DESIGN** |
| DoD-X1 | C-X-1 hydration seam runs | `OverlayReader` built inside a real multi-thread `SqlitePool::with_read`, reads one `about` cross-edge from a foreign memory pool via `with_read_inline`+`block_on`, returns the memory; under the MCP multi-thread runtime. Falsifier: returns nothing / errors. | **BUILD-GATE** |
| DoD-X1b | #1c no-deadlock + bounded threads | N concurrent cross-recalls (N ≥ 2×`max_blocking_threads`, cap set small e.g. 8): **(a) zero deadlocks/timeouts** (all complete within budget); **(b)** blocking-thread high-water mark `≤ max_blocking_threads`. Falsifier: hang, or high-water > cap (the inline fix didn't prevent the `1+k` multiplication). | **BUILD-GATE** |
| DoD-X2 | C-X-1 `traverse_multi` correctness | `traverse_multi_matches_union_of_traverse` GREEN in the conformance kit for MemStore + SqliteStore + PostgresStore + OverlayReader. | **BUILD-GATE** |
| DoD-X2b | #3 `traverse_multi` not-N+1 | SqliteStore `traverse_multi(W starts, 1 ply)` issues query-count `≤ C` (small fixed C, e.g. ≤ 3) **for all W in the sweep** (counted via a SQLite trace/counting wrapper). Falsifier: query-count scales with W (the slow fold leaked into SQLite). | **BUILD-GATE** |
| DoD-X3 | R-X1 code tools default-OFF + budgets | counting-fake-foreign-pool: `BlastRadius`/`ContextPack`/`ContextBundle` with NO `cross_edge_kinds` → ZERO foreign queries, home-only result; with `["about"]` → cross-edges present, foreign pool hit exactly `max_cross_hops` times. | **BUILD-GATE** (design of the gate + the inverted field = PROVEN-IN-DESIGN) |
| DoD-X3b | #2 recall default opt-OUT | a **NAIVE `recall(query, scope, &[code_seed], budget, now)`** over the separate-store fixture surfaces the `about` doc. **Falsifier: delete the `xedge` `about` row → naive `recall()` does NOT surface it.** | **BUILD-GATE** (the asymmetry decision = PROVEN-IN-DESIGN) |
| DoD-X4 | #5 epoch fail-closed, NON-VACUOUS | delete a symbol, re-add the same name → `symbol_epoch(id)` returns `Some(g)`, **g ≥ 1** (bump fired); the old xedge row (epoch 0) is DROPPED + `XEDGE-STALE-EPOCH`; NEVER resolves to the live node. Falsifier: `g == 0` (bump didn't fire) or the row resolves. **Depends on DEC-X6-SEQ step (1).** | **BUILD-GATE** (read-time fail-closed-on-inequality logic = PROVEN-IN-DESIGN; the bump firing + TOCTOU close = gate) |
| DoD-X5 | #4 bench LANDING gate | `xedge_query_latency_p95_us` (ceiling A: p95 ≤ CEIL_P95; ceiling B: cross-OFF recall p95 within Δ of pre-overlay baseline) AND `cross_engine_recall_at_k` (ceiling C: recall_on ≥ recall_off + M) GREEN over the mixed corpus (`corpus/xedge-seed.jsonl`). **Built BEFORE OverlayReader; hard blocker on landing.** | **BUILD-GATE** |
| DoD-X6 | R-X3 atomic redirect (opted-in) | `cross_edge_lifts_recall_from_xedge` on a separate-store fixture: lift PRESENT with the xedge row, ABSENT with it deleted (fails if overlay unwired); memory.db `neighbors` empty (proves redirect) — in the SAME change that deletes the in-store `about` write (§8). | **BUILD-GATE** |
| DoD-X7 | R-X5 bounded reconcile | net-new `xedge-reconcile` prunes a deleted estate endpoint's rows on `wicked.estate.indexed`, work O(rows-touching-changed-files) not O(xedge_rows) (assert query count). | **BUILD-GATE** |
| DoD-X8 | 28-method delegation | `OverlayReader` implements all 28 `GraphRead` methods; a test asserts PageRank (`all_nodes`/`all_edges`) and `find_symbols` are HOME-ONLY (no foreign nodes leak into ranking/search). | **BUILD-GATE** (the delegation table itself = PROVEN-IN-DESIGN) |
| DoD-X6-SEQ | #5 sequencing | Lane A `symbols.gen` (0→1 bump) + `symbol_epoch` GREEN in estate conformance **BEFORE** the about-arm is claimed reuse-safe. Order: Lane A epoch → xedge put-time stamping+TOCTOU → about-arm. | **BUILD-GATE (sequencing)** |

**Summary of tags:** 1 row PROVEN-IN-DESIGN (DoD-X0, the disproven panic); the design portions of
DoD-X3/X3b/X4/X8 (the inverted field, the opt-OUT asymmetry, the fail-closed-on-inequality rule, the
delegation table) are PROVEN-IN-DESIGN; **every runtime/empirical property — the seam running
(X1), no-deadlock-under-load (X1b), conformance + not-N+1 (X2/X2b), the gate behaviors (X3/X3b),
the bump firing + reuse-safety (X4/X6-SEQ), the bench ceilings (X5), the atomic lift (X6), bounded
reconcile (X7), no-leak delegation (X8) — is a BUILD-GATE the design explicitly does NOT claim to
pass.** Nothing here is over-claimed: the only thing asserted as already-true is what the cited code
already does and what the tokio source already guarantees.
