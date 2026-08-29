# Lane X — the cross-edge overlay (`xedge`) — DESIGN v2

> **Foundation HOLDS (do not relitigate): DEC-1 — SEPARATE STORES + a first-class single-writer
> cross-edge overlay (`xedge.db`).** Each engine (estate/memory/knowledge) keeps its OWN file +
> OWN single writer; cross-domain edges live in a tiny dedicated single-writer overlay keyed on
> `(engine, stable-id)`. v1 (`design-lane-X-overlay.md`) established the architecture; the Lane X
> gate (`wicked-memory/docs/recon/design-gate-verdicts.md:66-80`) returned **NO-GO/ITERATE** with
> the *read-union* unbuilt or mis-grounded. This revision folds the eight gate conditions
> (C-X-1..5, R-X1..5) as a spec, not a re-derivation. **The decision survives; the plumbing is
> rebuilt and the false "zero tool / zero engine changes" claim is retracted.**

DESIGN ONLY. Every claim cites real code. v2 supersedes v1's §3 (read-union), §4 (reconcile is now
real, not "reuse"), D-X3 (now owns 3 tool changes + a trait change), and R-X3/R-X4 grounding.

---

## TL;DR — what changed from v1, lead with the hydration seam (OQ-X2 / C-X-1)

1. **OQ-X2 RESOLVED — in-proc `&dyn GraphRead`-via-foreign-pool, driven by `Handle::block_on`
   inside `spawn_blocking`; NOT a read RPC for v1.** The blocker was "`GraphRead` is sync, runs in
   `spawn_blocking` (`pool.rs:83`), can't `.await` a foreign async pool, and `with_read`'s `F` is
   `+ 'static` (`traits.rs:216`)." It is **already solved in-tree**: `PostgresStore::rt_block`
   drives async work from exactly such a blocking context via
   `tokio::task::block_in_place(|| handle.block_on(f))` (`postgres.rs:63-74`). The `OverlayReader`
   uses the *same* trick to call foreign engines' `AsyncGraphStore::with_read` synchronously from
   inside estate's `spawn_blocking`. No `GraphRead` async-ification, no Lane W actor. v1's OQ-X2
   ("read-RPC vs in-proc map?") is answered: **in-proc handle map for v1**, RPC deferred (DEC-X8).
2. **`traverse_multi` is a DELIBERATE §1 spine change — owned, conformance-tested — NOT "zero
   engine changes."** v1 invented `traverse_multi` on `GraphRead` without adding it (the C-X-1
   mis-cite). v2 adds it as the 28th `GraphRead` method (`traits.rs:84`), multi-start, so the
   cross-engine frontier walk is one CTE per ply per engine, not O(frontier) CTEs (RATIONALE-6).
3. **Default-OFF cross-edge gate is a NEW field, `cross_edge_kinds: Vec<String>`, empty = NONE**
   (the inverse of `TraversalSpec.edge_kinds`, where empty = ALL — verified `sqlite.rs:931-944`).
   v1 claimed reusing `edge_kinds` made the join opt-in; **that was backwards and is retracted**
   (R-X1). `BlastRadius`/`ContextPack`/`ContextBundle` are the **3 owned tool changes** — they pass
   it OFF unless the caller opts in.
4. **Bench gets a query-latency p95 + a cross-engine recall@k gate over a NEW mixed corpus**
   (R-X2). Today's bench has single-shot µs counters (`capability.rs:111-114`), code-only corpus
   (`bench/lib.rs:98-119`), inert A/B — it cannot protect §9 against a slow join.
5. **Atomic about-redirect** (R-X3): `resolve_code`→estate-read-API + `about_seed_ids`→xedge +
   DELETE the in-store `about` write — ONE change; the differentiator test is rebuilt on a
   genuinely separate-store fixture with a falsifier that fails if the overlay is unwired.
6. **Identity epoch** (R-X4): a per-`sid` generation folded into the xedge key so SymbolId reuse
   (intern is append-only, no tombstone — `symbol.rs:1-7`) fails CLOSED (resolves to nothing,
   loud), never silently-wrong.
7. **Real bounded reconcile** (R-X5): a net-new `xedge` subscriber spec'd as real work, with a
   dirty-set incremental diff bounded by *changed* ids, not O(xedge_rows)/reindex.
8. **27→28-method `OverlayReader`** delegation table: exactly which fold cross-engine, which stay
   home-only and why (PageRank's `all_nodes`/`all_edges`/`find_by_requirement` stay home-only).

**Sibling-lane (A/B) impact found while folding:** ONE real cross-lane change surfaced. The
identity epoch (R-X4) needs estate to expose a symbol's current generation through a **read
method** (`symbol_epoch(&SymbolId) -> Option<u64>`), and the reconcile fast-path wants the indexer
to **emit which sids were reused** — both are **Lane A (estate) writer/reader changes**, not
pure-Lane-X. They are small and additive, but they are NOT in estate today and must be sequenced
into Lane A. (Details in DEC-X6 + the cross-lane callout.) Everything else folds inside Lane X.

---

## Grounding re-verification (v1 had three mis-cites the gate caught — corrected here)

| v1 claim | reality (cited) | verdict |
|---|---|---|
| empty `edge_kinds` ⇒ caller opted OUT of cross-edges (safe default) | `if spec.edge_kinds.is_empty() { String::new() }` → **no** `kind IN (...)` clause → ALL kinds (`sqlite.rs:931-944`) | **WRONG** → v2 adds a separate field (R-X1) |
| `traverse_multi` exists / "zero engine changes" | `GraphRead` has only `traverse(start, spec)`, single start (`traits.rs:84`); no `traverse_multi` | **WRONG** → v2 adds it deliberately (C-X-1) |
| mem-id is "scope+content-derived; must be stable" (R-X3) | mem-id = `uuid::Uuid::now_v7().to_string()` minted once in `Memory::new` (`wicked-memory-core/src/lib.rs:140`), wrapped `Symbol::synthetic("mem", uuid)` (`:156-158`) | mis-grounded; **conclusion (stability) survives** — uuid_v7 is stable by construction (C-X-4) |
| union folds into traverse "for free" via `neighbors` override | TRUE for the FIRST boundary hop (`traverse` induces edges via `self.neighbors(a, dir)`, `sqlite.rs:1680-1686`); FALSE for multi-hop (`cte_reach` is a single-DB integer-sid CTE, `sqlite.rs:904-982`) | half-true → v2 separates 1-hop (`neighbors`) from N-hop (`traverse_multi`) |

What v1 got RIGHT and v2 keeps: own-file `xedge.db` + single writer (D-X1); `XedgeStore`/
`XedgeReader` are not a `GraphStore` (D-X2); union runs in the retrieval layer behind `&dyn
GraphRead` (D-X3, now via `OverlayReader`); xedge rows convert to ordinary `core::Edge` so every
formatter works unchanged (`edge_json`, `wicked-estate-retrieve/src/lib.rs:108-121`); the boundary
vs garden `injected:*` litmus (v1 §5); event-driven prune over the pinned catalog
(`event-catalog-contract.md:14-26`). Those sections of v1 stand; this doc does not repeat them.

---

## DECISIONS

### DEC-X1 — [C-X-1 / OQ-X2, LOAD-BEARING] The hydration seam: in-proc foreign-pool map, driven synchronously via `Handle::block_on` inside `spawn_blocking`

**The problem, stated exactly.** Every retrieval tool takes `&dyn GraphRead` (`traits.rs:228-233`)
and the MCP injects it through ONE seam:
`store.with_read(move |graph| Ok(handle_request_ctx(graph, &req, &ctx_clone)))`
(`wicked-estate-mcp/src/main.rs:316`). For SQLite, `with_read` checks out a pooled connection and
runs the closure in `tokio::task::spawn_blocking(move || f(&*obj))` (`pool.rs:83`), where `F:
FnOnce(&dyn GraphRead) -> Result<T> + Send + 'static` (`traits.rs:214-217`). Consequences the gate
named (C-X-1):
- **(a) `GraphRead` is sync** (`pub trait GraphRead: Send`, `traits.rs:74`). An `OverlayReader:
  GraphRead` whose `neighbors`/`traverse_multi` need to read a *foreign* engine cannot `.await`
  that engine's async pool — it is itself running inside `spawn_blocking`.
- **(b) the `'static` bound on `F`** means the closure cannot borrow non-`'static` data across the
  seam.

**The resolution — already precedented in-tree, zero new async surface.**
`PostgresStore` is a natively-async backend whose *sync* `GraphRead` methods drive async `sqlx`
calls from a blocking context via:
```rust
// crates/wicked-estate-store/src/postgres.rs:63-74  (rt_block)
match tokio::runtime::Handle::try_current() {
    Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread =>
        tokio::task::block_in_place(|| handle.block_on(f)),   // ← the move
    _ => global_rt().block_on(f),
}
```
This is *exactly* the OverlayReader's situation: a sync `GraphRead` method, on a blocking thread,
needing to drive async work (a foreign engine's `with_read`). **The OverlayReader reuses this
pattern.** Its foreign reads call `foreign_pool.with_read(|fg| fg.neighbors(...))` and block on the
result via `Handle::block_on` (the multi-thread arm — the MCP runs a multi-thread runtime, the
`spawn_blocking` worker has a runtime handle). No `.await` in `GraphRead`, no async-ification of the
trait, **no Lane W actor.**

**The `'static` bound is satisfied** because the `OverlayReader` is constructed *inside* the
closure, capturing only owned/`Arc` data:
```rust
// at the MCP seam (main.rs:316), home_engine = "estate":
let others = ctx.foreign_pools.clone();              // Arc<HashMap<&'static str, Arc<dyn AsyncGraphStore>>>
let xedge   = ctx.xedge_reader.clone();              // XedgeReader: cheap WAL clone, owned
store.with_read(move |graph| {                       // graph: &dyn GraphRead (estate, the HOME engine)
    let overlay = OverlayReader { home: graph, home_engine: "estate", xedge, others, budget: ... };
    Ok(handle_request_ctx(&overlay, &req, &ctx_clone))
}).await?;
```
`graph` is borrowed for the closure's own (non-`'static`) body — the `'static` bound is on `F`'s
*captures* (`others`, `xedge`, `req`, `ctx_clone`), all owned/`Arc`. `OverlayReader<'a>` borrows
`graph` with lifetime `'a` local to the closure. This compiles under the existing signature with
**zero trait change to `AsyncGraphStore`**. (Conformance: a test that constructs an `OverlayReader`
inside a real `SqlitePool::with_read` and reads one cross-edge — DoD-X1.)

**Cost model of the join (stated, per the gate).** A cross-engine `neighbors`/`traverse_multi` is:
`home CTE/neighbors (in-proc, est. tens of µs — `search_latency_us` p50 today is single-digit-to-tens
µs, `capability.rs:111`) → xedge boundary lookup (one indexed SQLite query on `xedge.db`, O(log n)
via `xedge_by_src`/`xedge_by_tgt`, D-X1) → foreign hydration: ONE batched `find_symbols`/
`traverse_multi` per foreign engine per ply (NOT `get_node`-per-id — v1's N+1; v2 batches, see
DEC-X2)`. Each foreign call is a `block_on` of that engine's `spawn_blocking` → one pooled
connection checkout + one query. **Total foreign round-trips per recall = (engines touched) ×
(plies) — bounded by `max_cross_hops` (default 1, DEC-X3) → at most 1×(memory) + 1×(knowledge) = 2
foreign queries on the differentiator path.** This is the number the bench must hold (R-X2 / DEC-X5).

**Rejected:** read-RPC to each engine's read-MCP for v1. RPC adds a network hop + (de)serialization
to the *inner* loop of recall, turning a 2-query join into 2 RPCs — strictly worse for the
in-process deployment that ships today (estate + memory as libraries / co-located MCP servers,
`wicked-memory/src/lib.rs:1-6`). RPC is the *right* seam when engines are on separate hosts; that is
DEC-X8 (deferred), gated behind a `StoreCapabilities`-style negotiation, not v1.

### DEC-X2 — [C-X-1] The multi-hop API: add `traverse_multi(&[SymbolId], &TraversalSpec)` to `GraphRead` as a deliberate, conformance-tested §1 spine change

v1 hand-waved a "bounded single-start-per-frontier loop." That loop calls `traverse(start, spec)`
(`traits.rs:84`) once per frontier id → O(frontier) CTEs per ply, and each `traverse` re-inducts
edges by re-querying `neighbors` per anchor (`sqlite.rs:1680-1686`) → quadratic-ish on a wide
frontier. **Decision: add a multi-start method to the spine (§1) and own it as an engine change.**

```rust
// crates/wicked-estate-core/src/traits.rs — GraphRead (the 28th method)
/// Bounded traversal from MANY starts in one call — the frontier-expansion primitive the
/// cross-engine overlay needs (one CTE per ply, not one per start). `traverse` becomes
/// `traverse_multi(&[start], spec)`. Default impl folds over `traverse` so existing backends
/// stay correct until they specialize; SqliteStore overrides with a multi-seed CTE.
fn traverse_multi(&self, starts: &[SymbolId], spec: &TraversalSpec) -> Result<Subgraph> {
    let mut acc = Subgraph::default();
    for s in starts { merge_subgraph(&mut acc, self.traverse(s, spec)?); }
    Ok(acc)  // correct-but-slow default; the spine stays green for MemStore/Postgres immediately
}
```
- **SqliteStore specializes it** by seeding the recursive CTE's base case with all `starts`' sids
  (`SELECT sid FROM symbols WHERE sym IN (...)` then `UNION` the walk) — one `WITH RECURSIVE` for the
  whole frontier, mirroring `cte_reach` (`sqlite.rs:904-982`) but with a multi-row anchor. Edge
  induction batches: one `neighbors`-equivalent over the reached set (a single
  `SELECT data FROM edges WHERE source IN (...)`), not per-anchor.
- **§1 discipline (CLAUDE.md §1/§5/§9):** this is a trait-spine change, so it lands *before* fan-out
  with its conformance test green. The **GraphStore conformance kit**
  (`crates/wicked-estate-core/src/conformance.rs`) gets a `traverse_multi_matches_union_of_traverse`
  case: for any store, `traverse_multi(starts, spec)` ≡ the merged `traverse(s, spec)` over each `s`
  (same nodes/edges/min-depths). Every impl (MemStore, SqliteStore, PostgresStore, OverlayReader)
  must pass it. This is the "deliberate §1 spine change, conformance-tested — NOT zero engine
  changes" the gate demanded (C-X-1). **Owned:** estate's `GraphRead` grows one method; 3 store
  impls implement it (2 inherit the default until specialized; SqliteStore specializes for speed).
- **Why not keep the per-frontier loop?** Because the gate's whole R-X1/R-X2 worry is join cost; an
  O(frontier) CTE fan-out is the slow path that ships green and regresses §9 later. The multi-start
  CTE is the bounded primitive that makes the cost model in DEC-X1 honest.

The `OverlayReader::traverse_multi` (the cross-engine orchestrator) is then:
```
OverlayReader::traverse_multi(starts, spec):
  frontier: HashMap<engine, Vec<SymbolId>> = { home_engine: starts }
  out = Subgraph::default(); budget = spec.max_nodes; crossed = 0
  for _ply in 0..spec.max_depth:
     # (a) intra-engine expansion: each engine runs its OWN multi-seed CTE, 1 ply, in parallel-safe order
     for (engine, ids) in &frontier:
         sub = pool_for(engine).with_read(|g| g.traverse_multi(ids, &spec.one_ply()))?  # block_on, DEC-X1
         merge(&mut out, sub); if out.nodes.len() >= budget { out.truncated = true; return out }
     # (b) cross-engine boundary — ONLY if the caller opted into cross rels AND we have hops left
     if spec.cross_edge_kinds.is_empty() || crossed >= spec.max_cross_hops { break }   # DEC-X3 default-OFF
     boundary = xedge.expand(&frontier, dir=spec.direction, rels=&spec.cross_edge_kinds,
                             min_conf=spec.min_confidence)?       # indexed xedge query, both directions
     frontier = boundary.group_by_engine();  crossed += 1
     if frontier.is_empty() { break }
  out
```
`xedge.expand` validates the epoch (DEC-X6) and drops+diagnoses dangling rows (R-X5 backstop, v1 §4
kept). The cross-edge node budget is the SAME shared `spec.max_nodes` decremented across engines
(no separate unbounded walk) PLUS a cross-edge-specific cap `max_cross_nodes` (DEC-X3) so a hot
symbol with thousands of xedge rows cannot blow the frontier even at 1 hop.

### DEC-X3 — [R-X1] Default-OFF cross-edge gate: a NEW `TraversalSpec` field, empty = NONE; patch the 3 tools; real hop + node budgets

`TraversalSpec` (`core/src/query.rs`) today carries `edge_kinds` where **empty = ALL** (verified
`sqlite.rs:931-944`). Overloading it for cross-edges (v1) is backwards: an unfiltered intra-engine
traverse (empty `edge_kinds`, the common case) would fan into every engine. **Add a separate,
inverted field plus two budgets:**
```rust
// crates/wicked-estate-core/src/query.rs — TraversalSpec
pub struct TraversalSpec {
    pub direction: Direction,
    pub edge_kinds: Vec<EdgeKind>,        // UNCHANGED: empty = ALL intra-engine kinds
    pub max_depth: u32,
    pub max_nodes: usize,
    pub min_confidence: f32,
    // ── NEW (Lane X) — cross-engine is OFF unless explicitly requested ──
    /// xedge relations to follow across the engine boundary. **Empty = NONE** (the inverse of
    /// `edge_kinds`): a plain traverse never touches xedge. A caller OPTS IN with e.g.
    /// `["about"]` / `["governs","mentions"]`. Matched against `xedge.rel` (a TEXT column, D-X1),
    /// NOT against `EdgeKind` — cross rels are overlay-only strings.
    pub cross_edge_kinds: Vec<String>,
    /// Max engine-boundary crossings, independent of `max_depth`. Default 1 (DEC-X4).
    pub max_cross_hops: u32,
    /// Hard cap on nodes pulled across the boundary per recall (separate from `max_nodes` so a
    /// hot symbol with many xedge rows can't blow the frontier even at 1 hop). Default 64.
    pub max_cross_nodes: usize,
}
```
**`Default` sets `cross_edge_kinds: vec![]`, `max_cross_hops: 1`, `max_cross_nodes: 64`** — so every
existing construction site that uses `..Default::default()` (e.g. `SearchEntity`'s queries,
`render_context`'s `neighbour_spec` at `wicked-estate-retrieve/src/lib.rs:1297-1303`) is cross-OFF
with no code change and identical behavior to today. **The 3 tool changes we OWN** (v1's "zero tool
changes" was false — R-X1):

1. **`BlastRadius`** — `TraversalSpec::blast_radius(max_depth)` (`lib.rs:800`) must set
   `cross_edge_kinds: vec![]` explicitly (blast-radius is `Calls`-reachability; cross-edges are
   never wanted by default — a code-impact query must not drag in memories). Add an OPT-IN request
   field `cross_edge_kinds: ["about",...]` parsed like `edge_kinds` (`parse_edge_kinds`,
   `lib.rs:617-628`) for the rare "what do we *know* about everything this change touches" query.
2. **`ContextPack`** — `render_context`'s internal `neighbour_spec` (`lib.rs:1297-1303`) stays
   cross-OFF; `ContextPack::invoke` (`lib.rs:1426`) gains an opt-in `cross_edge_kinds` request field
   threaded into the spec. Off by default keeps today's bench numbers (RATIONALE-4).
3. **`ContextBundle`** — gathers neighbours via two `store.neighbors(seed, dir)` calls
   (`context_bundle.rs:182-197`). With the `OverlayReader` in place, `neighbors` ALREADY folds the
   first cross hop (DEC-X7) — so `ContextBundle` must pass the cross-gate too: it reads
   `cross_edge_kinds` from the request (default empty) and the `OverlayReader::neighbors` it calls
   honors that gate (DEC-X7). Without this, the moment `OverlayReader` wraps the store,
   `ContextBundle` would silently fan cross-edges on every call — the exact R-X1 regression.

`TraverseGraph` (`lib.rs:643`) already parses `edge_kinds`; it gains the same opt-in
`cross_edge_kinds` parse so an agent can deliberately walk `["about"]` across the boundary. That is
a 4th touched tool but it is purely additive (a new optional request field), not a default change.

**Conformance for the gate itself (DoD-X3):** a test asserting that `BlastRadius`/`ContextPack`/
`ContextBundle` invoked WITHOUT `cross_edge_kinds`, over an `OverlayReader` with live xedge rows,
return EXACTLY the home-only result (zero foreign queries — assert via a counting fake foreign
pool). And the inverse: with `cross_edge_kinds:["about"]`, the cross-edges appear and the foreign
pool is hit exactly `max_cross_hops` times. This makes "default-OFF" and the budgets *tested*, not
prose (the R-X1 fix).

### DEC-X4 — [R-X1/OQ-X4] `max_cross_hops` default = 1, hard-gated above

Memory/knowledge are **leaves** off a code seed in the differentiator workload (recall docs/
decisions FROM a code symbol — `about_seed_ids` is one hop, `wicked-memory/src/lib.rs:359-372`). You
cross code→memory once; you do not then chase memory→code→memory. **Default `max_cross_hops = 1`.**
`>1` is an explicit, bench-gated opt-in (DEC-X5 must show the 2-hop latency before any tool sets it
>1). No real 2-hop workload exists today (OQ-X4) → the cross-walk in DEC-X2 `break`s after one
boundary crossing by default.

### DEC-X5 — [R-X2 / C-X-2] Bench gate: query-latency p95 + cross-engine recall@k over a NEW mixed corpus

Today's bench cannot protect §9 here. Verified gaps:
- **No percentile.** `search_latency_us` / `blast_radius_latency_us` are SINGLE-SHOT measurements on
  the *top* symbol (`capability.rs:111-114`, measured once at `:305`). No distribution, no p95.
- **Code-only corpus.** `baseline_corpus()` = `ts-axios` / `py-flask` / `poly-tree-sitter`
  (`bench/lib.rs:98-119`) — zero memory/knowledge↔code edges. A cross-engine join is *unmeasurable*
  on it.
- **A/B is file-recall over code** (`ArmMetrics.answer_file_recall`, `bench/lib.rs:45-47`) — inert
  for cross-engine recall.

**Spec (the gate §9 needs):**
1. **`xedge_query_latency_p95_us`** — a new `RepoMetrics` field (alongside `search_latency_us`,
   `capability.rs:111`). Driver: run the differentiator query (recall from a code seed, opt-in
   `cross_edge_kinds:["about"]`) over **N≥200 seeds** sampled from the mixed corpus, collect the
   per-call latency distribution, report **p50/p95/p99**. The CI gate asserts a **ceiling on p95**
   (the `capability.rs:910` "asserts these ceilings on the fixture repo; tighten as optimisations
   land" pattern — extend it) AND a **no-regression delta vs the cross-OFF p95** (the join must not
   move the baseline intra-engine path, since DEC-X3 keeps it default-OFF: a cross-OFF blast-radius
   must be statistically indistinguishable from today).
2. **`cross_engine_recall_at_k`** — fraction of gold (memory/knowledge) items surfaced from a code
   seed within budget, over a labeled set. This is the differentiator's analogue of
   `answer_file_recall` and the thing `cross_edge_lifts_recall` (DEC-X7) proves at *unit* scale; the
   bench proves it at *corpus* scale. Gate: recall@k with cross-edges ON must **exceed** recall@k
   with cross-edges OFF by a frozen margin (else the overlay buys nothing — the inverse failure to
   R-X1's "too much").
3. **A NEW mixed bench corpus** — extend `RepoSpec`/`baseline_corpus` (`bench/lib.rs:23-28,98-119`)
   with a fixture that has REAL `about`/`mentions`/`governs` xedge rows: index `py-flask` into
   estate, capture a frozen set of memories `about` Flask symbols (deterministic, checked-in seed
   data — a `corpus/xedge-seed.jsonl`), and (when knowledge ships, C-X-5) `mentions`/`governs` rows.
   The bench builds the three stores + `xedge.db`, wraps with `OverlayReader`, and runs (1)+(2).
   Until knowledge is a crate (OQ-X3), the mixed corpus carries only the `about` arm (DEC-X9).

Without this, R-X1's bounding is "a DESIGN defense, unproven" (the gate's words, C-X-2) and a slow
join ships green. **This bench gate is a hard blocker on Lane X landing**, parallel to PR-14's
hybrid-uplift bench (`wicked-memory/src/lib.rs:88-100`).

### DEC-X6 — [R-X4] Identity epoch: fold a per-`sid` generation into the xedge key so SymbolId reuse fails CLOSED

**The trap, grounded.** A `SymbolId` is a pure logical-name-path string (`symbol.rs:1-7,107-109`).
The intern table maps `sym → sid` and is **append-only on the write path**: `intern` inserts on
miss (`sqlite.rs:1006-1009` interns every node's symbol; no delete-from-`symbols` exists). So
delete-a-symbol-then-re-add-the-same-name re-creates the SAME `SymbolId` string → an old xedge row
keyed on that string now resolves to a LIVE but possibly-DIFFERENT logical node. v1's `None`
backstop (v1 §4) is blind to this — the node *exists*, so hydration returns `Some`, and the edge is
served **confidently wrong, silently** (violates R7). The gate (R-X4) is correct and v1 missed it.

**Decision — carry a generation alongside the id in the xedge key, validated at read.**
```sql
-- xedge.db schema (v1 D-X1 + the epoch columns)
CREATE TABLE xedge (
  src_engine TEXT NOT NULL, src_id TEXT NOT NULL, src_epoch INTEGER NOT NULL,   -- ← NEW
  rel        TEXT NOT NULL,
  tgt_engine TEXT NOT NULL, tgt_id TEXT NOT NULL, tgt_epoch INTEGER NOT NULL,   -- ← NEW
  confidence REAL NOT NULL, provenance TEXT NOT NULL, resolved_by TEXT NOT NULL, ts INTEGER NOT NULL,
  PRIMARY KEY (src_engine, src_id, src_epoch, rel, tgt_engine, tgt_id, tgt_epoch)
);
```
- **`epoch` = the home engine's generation counter for that id at write time.** For estate, the
  cleanest source is a per-`sid` generation bumped whenever a symbol with that name is
  (re)interned/removed-then-readded. estate's intern is append-only today, so this is the **Lane A
  change** (cross-lane callout below): add a `gen` column to the `symbols` table, incremented when a
  removed name is re-interned, and expose it via a NEW read method
  `GraphRead::symbol_epoch(&SymbolId) -> Result<Option<u64>>`. For memory, the mem-id is a
  uuid_v7 minted once per memory (`memory-core/src/lib.rs:140`) — it is **never reused** (a new
  memory = a new uuid), so memory's epoch is constant `0` (uuid_v7 IS the generation). Knowledge:
  OQ-X3 — its id contract must state reuse semantics; default epoch `0` if ids are never reused.
- **Read-time validation (fail-closed, loud).** `xedge.expand` / `OverlayReader::neighbors`, for
  each candidate row, fetches the endpoint's CURRENT epoch via `symbol_epoch` (estate) / constant-0
  (memory) and **drops the row if `row.epoch != current.epoch`**, emitting
  `XEDGE-STALE-EPOCH: 1 edge to estate:<id> dropped (row gen=3, live gen=4 — symbol id was reused;
  prune queued)`. The edge resolves to NOTHING (fail-closed) — never to the wrong live node. This is
  the R7-honoring opposite of "silently wrong."
- **Write-time stamping.** `XedgeClient.put` stamps the endpoint epochs by asking the target engine
  for its CURRENT epoch at write time (the same `symbol_epoch` read used at validation) — so a
  freshly-written edge always matches until a reuse bumps the generation.
- **Why "carry alongside" beats "hash into the id string":** estate keys nodes/edges on the
  `SymbolId` string everywhere (`sqlite.rs` PKs, `edge.rs`); polluting that string with an epoch
  would churn estate's OWN identity and break ADR-002 stability. The epoch lives ONLY in xedge's
  key, where reuse-detection is the requirement. Estate's identity is untouched.

**Cross-lane:** `symbol_epoch` + the `symbols.gen` column are a **Lane A (estate) addition** — see
the callout. Memory needs nothing (uuid_v7). This is the one place a sibling lane must move.

### DEC-X7 — [R-X1] The 28-method `OverlayReader` delegation table

`OverlayReader: GraphRead` must implement all 28 methods (the 27 of `traits.rs:74-136` + the new
`traverse_multi`, DEC-X2). The gate is explicit: "cross-engine entities [are] invisible to the other
25" if you only override `neighbors`/`traverse`. Decision per method:

| method | OverlayReader behavior | why |
|---|---|---|
| `neighbors(id, dir)` | **FOLD** — home `neighbors` (`sqlite.rs:1623`) ++ xedge `out/in_edges` for `(home_engine,id)`, **gated by an OverlayReader-level `cross_edge_kinds`** carried from the request (default empty = home-only) | the 1-hop boundary; honors DEC-X3 default-OFF so `ContextBundle`'s raw `neighbors` calls don't leak (R-X1) |
| `traverse_multi(starts, spec)` | **FOLD** — the cross-engine orchestrator (DEC-X2); `spec.cross_edge_kinds`/`max_cross_hops` gate it | the N-hop spine; default-OFF |
| `traverse(start, spec)` | **FOLD** — `self.traverse_multi(&[start], spec)` | keeps `traverse` and `traverse_multi` consistent; the existing single-start tool path (`BlastRadius`/`Lineage` call `store.traverse`, `lib.rs:804,1062`) routes through the gated multi |
| `get_node(id)` | **ROUTE by engine tag** — if `id` belongs to home, home `get_node`; else `block_on(foreign_pool[engine].with_read(|g| g.get_node(id)))` (DEC-X1) | a cross-edge endpoint hydrated by `endpoint_json` (`lib.rs:77-98`) must resolve even when it lives in memory.db/knowledge.db, else every denormalized cross endpoint is a bare-id (under-served) |
| `find_symbols(query)` | **HOME-ONLY** | name/FTS search is per-engine; a code search must not return memories. Cross-engine search is a deliberate *future* tool, not an implicit union (would corrupt `SearchEntity`/`ContextBundle` seed resolution) |
| `all_nodes()` / `all_edges()` | **HOME-ONLY** | **PageRank** (`Ranker::rank`, `traits.rs:223`; `ranked_symbols` used by `BlastRadius`/`ContextPack`/`ContextBundle`, `lib.rs:955,1315,context_bundle.rs:203`) walks `all_nodes`/`all_edges`. Folding foreign nodes would (a) make PageRank cross-engine — a different, unbudgeted analytic — and (b) pull whole foreign graphs into estate's process. Cross-edges enrich *retrieval*, not *global ranking*. Stated explicitly per the gate's PageRank callout. |
| `find_by_requirement(req)` | **HOME-ONLY** | requirement→symbol is estate-semantic (`set_node_semantics`, `traits.rs:169`); memories/knowledge don't carry estate requirement annotations. Cross would return nothing useful + pay foreign cost |
| `unresolved_refs_for_name` | **HOME-ONLY** | unresolved-ref coverage is estate's static-resolution honesty (`BlastRadius`, `lib.rs:821`); no xedge analogue |
| `node_semantics` / `annotations` / `annotations_by_type` / `annotations_stale_since` | **HOME-ONLY** | annotations are per-store typed rows; an xedge has no annotations. A cross endpoint's annotations are fetched via the routed `get_node`→foreign `annotations` only if a tool explicitly retrieves that foreign node |
| `capabilities()` | **HOME ∧ overlay** — report home caps but force `server_side_traversal=false` for the cross path (the cross-walk is client-orchestrated, DEC-X2) so retrieval doesn't assume a one-round-trip cross traverse | retrieval negotiates on `StoreCapabilities` (`traits.rs:53-68`); lying here would mis-route |
| `file_digest` / `file_git_sha` / `repo_info` / `edge_history` / `file_content` / `symbol_source` | **HOME-ONLY** | all are estate file/content/provenance reads; cross endpoints' source (a memory's text) is not an estate file slice. `symbol_source` on a foreign node routes via `get_node`'s engine only if that tool path needs it |
| `changes_since(cursor)` | **HOME-ONLY** | the change-log is per-store; xedge has its OWN reconcile subscriber (DEC-X8), not this cursor |
| `stats()` | **HOME-ONLY** (optionally annotate `xedge_edge_count`) | graph stats are per-engine |

**Rule of thumb (stated for the reviewer):** a method FOLDS cross-engine iff it is *edge-following
retrieval* the differentiator needs (`neighbors`, `traverse`, `traverse_multi`) or *endpoint
hydration* for a cross-edge already surfaced (`get_node`, routed). Everything else — search, global
analytics, per-store provenance/annotations/stats — stays HOME-ONLY, because folding it either
changes a global computation's meaning (PageRank) or pays foreign cost for a result that is
semantically per-engine. This is the explicit 28-method delegation the gate required.

### DEC-X8 — [R-X5] Real, bounded reconcile: a net-new `xedge` subscriber + dirty-set incremental prune

v1 said "reuse `memory-subscriber.mjs`." The gate (R-X5): that's scaffolding, not the subscriber,
and a coarse-event-driven full re-query is O(xedge_rows)/reindex. **Spec it as real, bounded work:**

<!-- historical -->
- **Net-new component:** an `xedge-reconcile` subscriber (its own process or a task in the xedge
  writer). It *reuses the cursor-poll MECHANICS* — durable cursor + TTL self-heal + DLQ + dedup,
  the `wicked-brain/server/lib/memory-subscriber.mjs:1-55` pattern (retired 2026-08; frozen
  archive) (`event-catalog-contract.md:27-32`)
<!-- /historical -->
  — but is a distinct subscriber with its OWN cursor, subscribing to the pinned coarse events
  `wicked.estate.indexed` / `wicked.knowledge.ingested` / `wicked.memory.captured`
  (`event-catalog-contract.md:17,20,22`). "Reuse the library, build the subscriber" — owned as
  net-new (CLAUDE.md §5: it has a consumer — xedge.db — and a test, DoD-X5).
- **Bounded deletion-reconcile (the core of R-X5).** Events are COARSE (counts, not symbols —
  `event-catalog-contract.md:9-12`), so reconcile is trigger→re-query, but the re-query is
  **dirty-set incremental, NOT full**:
  - estate already logs file-granular deltas (`GraphRead::changes_since(cursor)` +
    `GraphWrite::log_change`, `traits.rs:109-111,162-165`; `remove_file` removes a file's
    symbols, `traits.rs:150-152`). On `wicked.estate.indexed`, the subscriber calls estate's
    `changes_since(its_cursor)` to get the **set of changed/removed files** since last reconcile —
    NOT all symbols.
  - It intersects that dirty file-set with the xedge rows that reference estate ids **in those
    files** (xedge stores `tgt_id`; the subscriber maps id→file via a single `find_symbols`/
    `get_node` per *referenced-and-dirty* id). Only those rows are re-validated: still-live + same
    epoch → keep; vanished → `prune_endpoint`; epoch bumped (id reused) → `prune_endpoint` (the row
    is stale by DEC-X6). **Work is O(xedge rows touching changed files), not O(xedge_rows).**
  - Rename: a true rename yields a NEW `SymbolId` (`symbol.rs:217-231`), so the old id vanishes from
    the dirty file's live set → pruned (loud). No silent dangling.
- **Fast-path (cross-lane, optional):** if estate's `wicked.estate.indexed` payload or its
  change-log carried the **set of removed/reused sids** (a Lane A enrichment — see callout), the
  subscriber skips the id→file mapping entirely and prunes exactly those. This is an optimization,
  not required for correctness; the dirty-file-set path above works with today's coarse events.
- **Read-time backstop unchanged (v1 §4):** the `OverlayReader` still drops + diagnoses
  (`XEDGE-DANGLING` / `XEDGE-STALE-EPOCH`) + enqueues a prune the instant a stale row is traversed,
  so a row is never *served* wrong even before the subscriber catches it. Two layers, both loud.

### DEC-X9 — [R-X3 / C-X-3] Atomic about-redirect, on a genuinely separate-store fixture

The differentiator must come from xedge post-pivot, and the change must be atomic (CLAUDE.md §8) or
recall's graph arm silently empties (`RecallMode::GraphOnly` returns nothing —
`wicked-memory/src/lib.rs:98-99,443-447`). **ONE change does all of:**

1. **`capture_about`** (`wicked-memory/src/lib.rs:270-293`): DELETE the in-store
   `self.store.upsert_edges(&edges)` (`:289-291`). Replace with `self.xedge.put(...)` writing
   `(memory, mem.symbol(), epoch=0) --about--> (estate, code_sid, epoch=current)` into `xedge.db`.
   The in-store `about` write is GONE, not early-returned (§8).
2. **`resolve_code`** (`wicked-memory/src/lib.rs:297-311`): today it calls
   `self.store.find_symbols(...)` against MEMORY's own store — which under separate stores has no
   code nodes → returns nothing → `memory.learn` silently captures UNLINKED
   (`wicked-memory-mcp/src/lib.rs:319-323`). **Redirect to estate's READ API** (the foreign
   `AsyncGraphStore` handle / read-MCP `find_symbols` — DEC-X1's `others` map, `home_engine="memory"`
   here). The id it returns is one estate proved exists → node-before-edge by construction (v1 D-X5
   survives, now actually wired). Unresolved → not written, surfaced loudly (`(unresolved: …)`,
   `wicked-memory-mcp/src/lib.rs:314-318`) — unchanged.
3. **`about_seed_ids`** (`wicked-memory/src/lib.rs:359-372`): today
   `self.store.neighbors(code, Dependents)` filtered to `"about"` over memory.db (finds nothing
   post-pivot). **Redirect to** `self.xedge.in_edges("estate", code_id, ["about"])` → the
   `(memory, mem-id)` sources, epoch-validated (DEC-X6). The rest of `recall_impl`
   (`:408-495`) — RRF fuse, scope filter, budget pack — is UNCHANGED; only the graph arm's source
   moves.

**The rebuilt test (C-X-3, R-X3 — the tautology fix).** `cross_edge_lifts_recall_the_unique_bet`
(`wicked-memory/src/lib.rs:618-659`) today (a) runs against ONE in-memory store (estate + memory
co-resident), so it passes even if the overlay is unwired, and (b) has a tautological falsifier:
`!hit(&without) || with.len() >= without.len()` (`:656`) — the `with.len() >= without.len()` arm is
trivially true, so the test passes even if the about-arm contributes NOTHING. **Rebuild as
`cross_edge_lifts_recall_from_xedge`:**
- **Separate-store fixture:** an estate `SqliteStore` (the code) + a *distinct* memory store + a
  real `xedge.db`, wired through an `OverlayReader` (`home_engine="memory"`, `others={estate:...}`).
  The code symbol exists ONLY in estate.db; the memory ONLY in memory.db; the `about` edge ONLY in
  xedge.db. This is the topology the gate demands.
- **Non-tautological assertions:** (1) WITH seed + `cross_edge_kinds:["about"]` → the idempotency
  memory IS recalled; (2) WITHOUT the seed → it is NOT (no lexical/semantic overlap, the v1 setup);
  (3) **the lift is sourced from xedge** — assert that with the xedge.db `about` row PRESENT the
  memory surfaces, and with the SAME fixture but the xedge row DELETED it does NOT (the falsifier
  that fails if the overlay is unwired); (4) assert the in-store path is dead: `memory.db`'s
  `neighbors(code, Dependents)` returns empty (proving the redirect, not a co-resident fallback).

This is "rebuild `cross_edge_lifts_recall` on a genuinely SEPARATE-store fixture asserting the lift
is sourced from xedge" (R-X3 / C-X-3), landed atomically with the `capture_about` delete (§8).

---

## RATIONALE

- **RATIONALE-1 (own file beats estate-table) — UNCHANGED from v1.** estate.db has one writer (the
  indexer, `GraphWrite` "typically a single writer", `traits.rs:138`); a cross-edge table in
  estate.db forces memory+knowledge to write estate.db → the ≥3-writers-one-file bug DEC-1 escaped
  (`design-gate-verdicts.md:24`). Own file = structurally impossible. (v1 RATIONALE-1.)
- **RATIONALE-2 (union at retrieval) — UNCHANGED.** One `&dyn GraphRead` seam (`main.rs:316`); tools
  never see a concrete store (`traits.rs:228-233`). Wrapping that reference is the spine-respecting
  move. v2 only makes the wrapper (`OverlayReader`) do real foreign reads via the existing
  `block_on` precedent (DEC-X1), instead of v1's invented `traverse_multi`.
- **RATIONALE-3 (reuse `block_in_place`+`block_on`, don't async-ify the spine).** Async-ifying
  `GraphRead` (the Lane-W-shaped temptation) would touch every store, every tool, every conformance
  test — an invasive refactor the DEC-1 reversal explicitly chose to AVOID
  (`design-gate-verdicts.md:64`). `PostgresStore::rt_block` (`postgres.rs:63-74`) proves a sync
  `GraphRead` can drive async work from `spawn_blocking` with no trait change. The overlay reuses
  the spine's own escape hatch. **This is the single most important reason the cost is bounded and
  the change is small.**
- **RATIONALE-4 (separate default-OFF field is the bench guard) — STRENGTHENED.** v1 leaned on
  `edge_kinds` empty=opt-out, which is FALSE (`sqlite.rs:931-944`). A *separate* `cross_edge_kinds`
  empty=NONE means a plain traverse/blast-radius is byte-identical to today (DEC-X3), so the bench's
  intra-engine numbers don't move; the cross path is paid only when a caller names a rel. The bench
  (DEC-X5) measures BOTH (cross-OFF must not regress; cross-ON must lift). This is the honest version
  of v1's RATIONALE-4.
- **RATIONALE-5 (events for prune) — UNCHANGED mechanics, REAL subscriber.** Pinned coarse names +
  cursor-poll library reuse (`event-catalog-contract.md:17-32`), but the subscriber is net-new and
  the reconcile is dirty-set-bounded (DEC-X8), not the O(xedge_rows) "reuse" the gate rejected.
- **RATIONALE-6 (`traverse_multi` over a per-frontier loop).** A multi-start CTE expands a whole
  frontier in one query (one `WITH RECURSIVE`, one batched edge-induction); the per-start loop is
  O(frontier) CTEs each re-inducing edges via `neighbors` per anchor (`sqlite.rs:1680-1686`). For a
  bounded cross-walk the difference is small at `max_cross_hops=1`, but the primitive must be the
  bounded one so raising the hop cap (DEC-X4) doesn't silently go quadratic and regress §9. Owning a
  spine method is cheaper than shipping a slow loop that the bench later red-flags.
- **RATIONALE-7 (epoch carried, not hashed-in).** estate keys everything on the `SymbolId` string
  (ADR-002). Putting an epoch in the string would churn estate's own identity on every reuse,
  breaking the rename-stability ADR-002 exists for. The epoch belongs only in xedge's key, the one
  place reuse-detection is the contract (DEC-X6).

---

## RISKS

- **R-X1′ (cross-OFF must be PROVABLY inert) — was the headline R-X1, now mitigated-but-watch.** The
  whole "bench numbers don't move" claim rests on `Default` setting `cross_edge_kinds: vec![]` and
  EVERY `..Default::default()` construction site inheriting it. If any tool constructs a
  `TraversalSpec` with a non-default cross field by accident, it fans cross-edges silently. Falsifier
  = DoD-X3's counting-fake-foreign-pool test (zero foreign queries when cross-OFF). Mitigation: the
  3 (+1) tool changes are explicit and tested; a clippy-style lint "no `cross_edge_kinds` set outside
  an opt-in parse" is a candidate CI guard (CLAUDE.md "candidates for dedicated CI lints").
- **R-X2′ (the join cost is still unproven until the mixed-corpus bench exists).** DEC-X5 SPECS the
  gate; until `xedge_query_latency_p95_us` + `cross_engine_recall_at_k` are implemented over the
  mixed corpus and green, the cost model in DEC-X1 is an argument, not a measurement. This is the
  same risk class the gate raised (C-X-2) — it is *reduced* to "build the bench" (a concrete task),
  not *resolved*. **Lane X must not land before DEC-X5 is green** (§9).
- **R-X3′ (`block_on`-inside-`spawn_blocking` requires the multi-thread runtime arm).**
  `rt_block`'s fast arm needs `RuntimeFlavor::MultiThread` (`postgres.rs:69`); on a single-thread
  runtime it falls back to a global runtime (`:72`) — fine for the MCP server (multi-thread) but a
  trap for any caller embedding the overlay under `#[tokio::test]` (current-thread). Mitigation: the
  OverlayReader reuses `rt_block` (or its logic) verbatim so the fallback is inherited and tested;
  DoD-X1 runs under the real multi-thread MCP runtime, not a current-thread test, to exercise the
  hot arm. **Deadlock check:** `block_on` of a foreign pool's `spawn_blocking` from within estate's
  `spawn_blocking` consumes a second blocking-pool thread; with N concurrent recalls each holding
  one estate blocking thread and blocking on a foreign one, the blocking pool must be sized >
  expected concurrency or it starves. Bench/load note: size `tokio` `max_blocking_threads`
  accordingly; this is a real operational bound DEC-X5's p95 run will surface.
- **R-X4′ (epoch source is a Lane A dependency).** DEC-X6 needs `symbol_epoch` + a `symbols.gen`
  column in estate. Until Lane A ships it, the epoch defaults to `0` everywhere → reuse-detection is
  INERT (the R-X4 hole stays open for estate endpoints). Memory is safe (uuid_v7). **This is a hard
  cross-lane sequencing dependency, not optional** — the about-arm (estate endpoints) is exactly
  where reuse matters. Flagged in the cross-lane callout; must be on Lane A's plan before the
  about-arm is claimed reuse-safe.
- **R-X5′ (dirty-set reconcile needs `changes_since` to span the reconcile interval).** DEC-X8's
  bound relies on `changes_since(cursor)` returning ALL files changed since the last reconcile; the
  log is "capped per call by the impl" (`traits.rs:109-111`). If the cap truncates a large reindex's
  delta, the subscriber must page (`while changes.len() == cap { advance cursor; repeat }`) or it
  silently misses dirty files → stale rows survive until the read-time backstop catches them
  (loud, but late). Mitigation: page the change-log; the read-time backstop (DEC-X8) is the
  safety net.
- **R-X6 (carried from v1 R-X2): raw `xedge.db` bypass sees stale rows.** No public raw-SQL consumer
  by design; all reads go through `XedgeReader`/`OverlayReader` (epoch + dangling validation). Must
  stay true — a future tool raw-SQLing `xedge.db` re-opens the silent-stale class (the C-aB3 bypass
  pattern, `design-gate-verdicts.md:38`).

---

## CROSS-LANE CALLOUT (the gate asked: did any condition need a sibling-lane A/B change?)

**YES — exactly one condition, R-X4 (identity epoch), forces a Lane A (estate) change; everything
else folds inside Lane X.** Specifically:

1. **Lane A — estate `GraphRead::symbol_epoch(&SymbolId) -> Result<Option<u64>>` + a `symbols.gen`
   column** (DEC-X6). estate's intern is append-only with no generation today
   (`sqlite.rs:1006-1009`, `symbol.rs:1-7`); reuse-detection needs estate to track and expose a
   per-id generation. Small, additive (one column, one read method, default impl returns `Some(0)`),
   but NOT in estate now → must be sequenced into Lane A before the about-arm is reuse-safe. Without
   it, estate-endpoint epochs are constant 0 and R-X4 stays open for those rows (R-X4′).
2. **Lane A — (optional fast-path) emit reused/removed sids** in `wicked.estate.indexed` or the
   change-log (DEC-X8 fast-path). Pure optimization; the dirty-file-set reconcile works without it.
   Tracked as a Lane A nice-to-have, not a blocker.

**No Lane B (memory/knowledge) change is forced by the read-union fixes.** Memory's side is all
in-lane (the `capture_about`/`resolve_code`/`about_seed_ids` redirects, DEC-X9, live in
`wicked-memory`/`wicked-memory-mcp`, which Lane X owns for this seam). Memory's mem-id is already
reuse-safe (uuid_v7). Knowledge's `mentions`/`governs` arms are gated on OQ-X3 (knowledge id +
reuse contract) and ship after the `about` arm (C-X-5 / DEC below) — that is a knowledge-crate
dependency, not a change to existing Lane B code.

**`traverse_multi` (DEC-X2) is an estate SPINE change but it is Lane X's to make** — it is the
`GraphRead`/conformance-kit change this lane owns and lands before fan-out (CLAUDE.md §1). It is
"engine code" but not a *sibling-lane* dependency: Lane X writes it.

---

## OPEN QUESTIONS (carried / updated)

- **OQ-X3 — knowledge id + REUSE contract** (was OQ-X3, now also feeds DEC-X6). kconcept/kchunk ids
  must be stable strings AND state whether they're ever reused (sets the knowledge epoch). Only the
  `about` arm ships first (C-X-5 / DEC-X9 scopes v1 to `about`); `mentions`/`governs` wait on this.
- **OQ-X8 (was OQ-X1) — single-writer mechanism for `xedge.db`.** Advisory file-lock + loud
  conflict (B-W-3 style, `design-gate-verdicts.md:62`) vs a tiny writer actor. Leaning advisory lock
  + `XedgeReader`-everywhere (no actor — the actor was Lane W's complexity we escaped). Unchanged
  from v1 OQ-X1; not load-bearing for the read-union.
- **OQ-X9 (was OQ-X6) — xedge.db schema versioning.** Mirror memory's `meta`-table forward migration
  (`MEM_SCHEMA_VERSION`, `wicked-memory/src/lib.rs:58-59`). The epoch columns (DEC-X6) make this
  more urgent — the schema is now v2-shaped from day one; add the `meta`/version row now.
- **OQ-X10 (NEW) — blocking-thread pool sizing under concurrent cross-recall** (from R-X3′). The
  `block_on`-inside-`spawn_blocking` join holds one estate blocking thread while blocking on a
  foreign one; size `max_blocking_threads` > peak concurrent cross-recalls or the pool starves.
  DEC-X5's p95-under-load run must report this bound; pick a default + document it.
- **DEC-X8-deferred — read-RPC cross-engine** (was implicit in v1 OQ-X2). When engines run on
  separate hosts, the in-proc handle map (DEC-X1) becomes a read RPC negotiated via a
  `StoreCapabilities`-style flag. Designed-not-built; out of scope for v1 (which is in-process /
  co-located, `wicked-memory/src/lib.rs:1-6`).

---

## DoD (falsifiable, per gate condition — what "Lane X v2 done" requires)

| # | condition | evidence path / falsifier |
|---|---|---|
| DoD-X1 | C-X-1 hydration seam | an `OverlayReader` built inside a real multi-thread `SqlitePool::with_read`, reading one `about` cross-edge from a foreign memory pool via `block_on`, returns the memory; runs under the MCP's multi-thread runtime (exercises `rt_block`'s hot arm) |
| DoD-X2 | C-X-1 `traverse_multi` | `traverse_multi_matches_union_of_traverse` GREEN in the GraphStore conformance kit for MemStore + SqliteStore + PostgresStore + OverlayReader |
| DoD-X3 | R-X1 default-OFF + budgets | counting-fake-foreign-pool test: `BlastRadius`/`ContextPack`/`ContextBundle` with NO `cross_edge_kinds` → ZERO foreign queries, home-only result; with `["about"]` → cross-edges present, foreign pool hit exactly `max_cross_hops` times |
| DoD-X4 | R-X4 epoch fail-closed | delete a symbol, re-add the same name (new gen), then traverse an old xedge row → row DROPPED + `XEDGE-STALE-EPOCH` diagnostic; NEVER resolves to the live node |
| DoD-X5 | R-X2 bench gate | `xedge_query_latency_p95_us` (p95 ceiling + no-regression-vs-cross-OFF) AND `cross_engine_recall_at_k` (cross-ON > cross-OFF by frozen margin) GREEN over the mixed corpus; **hard blocker on landing** (§9) |
| DoD-X6 | R-X3 atomic redirect | `cross_edge_lifts_recall_from_xedge` on a separate-store fixture: lift PRESENT with the xedge row, ABSENT with it deleted (fails if overlay unwired); memory.db `neighbors` empty (proves redirect) — landed in the SAME change that deletes the in-store `about` write (§8) |
| DoD-X7 | R-X5 bounded reconcile | a net-new `xedge-reconcile` subscriber prunes a deleted estate endpoint's rows on `wicked.estate.indexed`, doing work O(rows-touching-changed-files) not O(xedge_rows) (assert query count) |
| DoD-X8 | 28-method delegation | `OverlayReader` implements all 28 `GraphRead` methods; a test asserts PageRank (`all_nodes`/`all_edges`) and `find_symbols` are HOME-ONLY (no foreign nodes leak into ranking/search) |
