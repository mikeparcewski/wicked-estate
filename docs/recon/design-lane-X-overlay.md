# Lane X — the cross-edge overlay (`xedge`) — DESIGN

> Foundational seam after **DEC-1 (REVERSED at the W gate): SEPARATE STORES + a first-class
> cross-edge overlay.** Each engine — estate (code), memory (experiential), knowledge (docs/
> ontology) — keeps its OWN SQLite file + OWN single writer (already how `wicked-estate` and
> `wicked-memory` ship: `MemoryEngine::open` opens its own `SqliteStore`,
> `wicked-memory/crates/wicked-memory/src/lib.rs:124-130`). There is NO shared store and NO
> single-writer actor (the obsoleted Lane W). Cross-domain edges (code↔knowledge↔memory) can no
> longer be native FK edges in one store; they live in a small dedicated single-writer **`xedge`
> overlay**. This is the first-class, reliable replacement for the fuzzy `correspond` name-Jaccard
> shim (`wicked-estate/crates/wicked-estate/src/main.rs:103-145`, `2089`) and for the honest
> non-resolution of cross-graph edges admitted today (`.../wicked-estate/src/lib.rs:1064-1069`).

DESIGN ONLY. Reviewer + antagonist attack it next. Every claim cites real code.

---

## TL;DR (lead)

- **Overlay home = its OWN tiny file `xedge.db` with its OWN single writer.** NOT a table in
  `estate.db`. Putting it in estate.db reintroduces exactly the multi-writer-on-one-file bug DEC-1
  escaped: estate's indexer is already estate.db's single writer, and memory/knowledge would have
  to open estate.db to write their `about`/`mentions`/`governs` edges → ≥3 writers, one file →
  `SQLITE_BUSY` day one (B-BLOCK-1, design-gate-verdicts.md:24). Own file = the multi-writer bug
  *structurally cannot* arise; symmetric with the per-engine topology DEC-1 just ratified.
- **Riskiest assumption = the read-time-join cost (C3).** `SqliteStore::traverse` is a single-DB
  `WITH RECURSIVE` CTE (`wicked-estate-store/src/sqlite.rs:949,1652`) — it **cannot** reach across
  files. So a cross-engine traverse is NOT one query; it is *home-CTE → xedge boundary-lookup →
  other-home-CTE*, re-paid every hop, every recall. The whole differentiator ("recall docs/
  decisions from a CODE seed") now rides a per-call N+1 join we accepted in exchange for killing
  the multi-writer bug. If that join is slow or unbounded it regresses the agent-eval benchmark
  (CLAUDE.md §9) — the one bar that may not move. Bounding it by relation-type + hop-cap is the
  core of §3.
- **Second-riskiest = cross-store dangling (§4).** A node deleted/renamed in its home store orphans
  an xedge row; the endpoint no longer exists in a store xedge cannot see. Must be *loud*, never a
  silent dangling edge (the B-ADV-2 failure, design-gate-verdicts.md:27).

---

## Background grounded in code (what exists today, what breaks)

- **`about` edge today is co-resident, not cross-store.** `capture_about` writes the `about` edge
  into memory's OWN store via `self.store.upsert_edges(&edges)` with
  `EdgeKind::Other("about")`, `source = mem-id`, `target = code SymbolId`
  (`wicked-memory/crates/wicked-memory/src/lib.rs:270-293`). Recall reads it back with
  `self.store.neighbors(code, Direction::Dependents)` filtered to `"about"`
  (`.../lib.rs:359-372`, `about_seed_ids`), fused into hybrid recall at `.../lib.rs:442-450`. This
  ONLY works because, in the shipped slice, estate is a *library* and the code node + memory node
  are co-resident in one SqliteStore (`.../lib.rs:1-6`, `MemStore: GraphStore`,
  `wicked-memory/crates/wicked-memory/src/store.rs:12`). **Under separate stores the code
  `SymbolId` is NOT a node in `memory.db`, so `target` points at nothing and
  `neighbors(code, …)` over memory.db returns nothing.** This is the exact seam xedge replaces.
- **The differentiator test that must keep passing:** `cross_edge_lifts_recall_the_unique_bet`
  (`wicked-memory/crates/wicked-memory/src/lib.rs:618-659`) — a memory whose CONTENT shares no
  words with the query is recalled from a code seed via the `about` edge. Post-pivot this lift must
  come from `xedge`, not a co-resident edge.
- **The read path is uniform and already abstracted.** Every retrieval tool takes
  `store: &dyn GraphRead` and calls `store.traverse(...)` / `store.neighbors(...)`, returning a
  `Subgraph` + `diagnostics` (`wicked-estate-retrieve/src/lib.rs`: `TraverseGraph` invoke 643-733,
  `BlastRadius` 781-804, `Lineage` 1032-1053, `ContextPack` 1426). The MCP supplies that
  `&dyn GraphRead` through ONE seam: `AsyncGraphStore::with_read(move |graph| handle_request_ctx(
  graph, …))` (`wicked-estate-mcp/src/main.rs:316`; trait at `core/src/traits.rs:213-218`). **That
  single `&dyn GraphRead` is the overlay injection point.**
- **`traverse` builds its edge set purely from `neighbors`.** `SqliteStore::traverse` computes
  reachable depths via the CTE, then induces edges by calling `self.neighbors(a, dir)` per anchor
  (`sqlite.rs:1675-1687`). Consequence: **an overlay that overrides only `neighbors` automatically
  folds cross-edges into traversal results too** for the boundary hop — no CTE change.
- **`Edge`/`EdgeKind`/`Direction`/`Confidence`/`Provenance` are the reusable vocabulary.** Edges
  are `source=dependent → target=dependency` (`core/src/edge.rs:117-134`); `EdgeKind::Other(String)`
  already carries non-code relations (`edge.rs:114`); `Edge::new(tier, resolved_by)` stamps
  confidence+provenance from the tier (`edge.rs:136-155`); blast-radius = dependents
  (`Direction::Dependents` = edges where `target==id`, `edge.rs:176-182`). xedge rows render into
  exactly these types so the read-union returns ordinary `Edge`s the tools already format.
- **`Governs` is native today but DEC-2 moves it to `Other("governs")`** (`edge.rs:107`,
  design-gate-verdicts.md:70). Knowledge `governs` edges are CROSS-engine (knowledge concept → code
  symbol) → they belong in xedge, not estate's native edge space (DEC-1 cascade,
  design-gate-verdicts.md:69).
- **Bus seam already exists** for reconciliation: cursor-poll + durable cursor + TTL self-heal + DLQ
  + dedup, in `wicked-brain/server/lib/memory-subscriber.mjs:1-55` (the catalog's
  `memory-subscriber.mjs` reuse target, event-catalog-contract.md:27-32). Event names are PINNED in
  `event-catalog-contract.md:14-26` (`wicked.estate.indexed`, `wicked.knowledge.ingested`, etc.).

---

## DECISIONS

### D-X1 — Overlay home: its OWN file `xedge.db`, its OWN single writer (`XedgeStore`)

`xedge` is a standalone SQLite file with a single dedicated writer process/handle — symmetric with
estate.db, memory.db, knowledge.db. **Rejected: a table in estate.db.** Rationale in RATIONALE-1;
the short form: estate's indexer is already estate.db's sole writer (`GraphWrite` "typically a
single writer", `core/src/traits.rs:138-140`), and memory + knowledge would each need to OPEN
estate.db to write their cross-edges → the precise ≥3-writers-one-file topology B-BLOCK-1 killed
(design-gate-verdicts.md:24). A separate file makes that impossible by construction.

Schema (one table + a deletion-reconcile journal):

```sql
-- xedge.db  (single writer: the xedge writer handle; many concurrent readers via WAL)
CREATE TABLE xedge (
  src_engine   TEXT NOT NULL,        -- 'estate' | 'memory' | 'knowledge'
  src_id       TEXT NOT NULL,        -- stable id IN ITS HOME store (see id contract below)
  rel          TEXT NOT NULL,        -- 'about' | 'mentions' | 'governs' | 'supersedes' | ...
  tgt_engine   TEXT NOT NULL,
  tgt_id       TEXT NOT NULL,
  confidence   REAL NOT NULL,        -- maps to core::Confidence [0,1] (edge.rs:13)
  provenance   TEXT NOT NULL,        -- resolver/source that asserted it (e.g. 'wicked-memory',
                                     --   'knowledge-resolver', 'scip-mention')
  resolved_by  TEXT NOT NULL,        -- mirrors Edge.resolved_by (edge.rs:127)
  ts           INTEGER NOT NULL,     -- unix seconds, writer-stamped
  PRIMARY KEY (src_engine, src_id, rel, tgt_engine, tgt_id)   -- = dedup_key, cross-engine form
);
-- Directional read indexes — the overlay's `neighbors` must be O(log n) from EITHER endpoint:
CREATE INDEX xedge_by_src ON xedge(src_engine, src_id, rel);
CREATE INDEX xedge_by_tgt ON xedge(tgt_engine, tgt_id, rel);   -- blast-radius / dependents direction
```

- **Key = stable ids only**, never line/content (CLAUDE.md "Stable IDs only", ADR-002). For estate:
  the `SymbolId` canonical string (`core/src/symbol.rs:107-109,161-169`). For memory: the mem-id =
  `Memory::symbol()` (`wicked-memory/.../lib.rs:259`, a `SymbolId` over a `Symbol::synthetic`/scope
  encoding). For knowledge: kconcept/kchunk id (knowledge engine not yet a crate — Lane B/knowledge
  owns minting these as a `SymbolId`-shaped stable string; xedge stores it opaquely).
- **`(engine, id)` is the composite identity.** Two engines may mint the same raw string for
  different things; the engine tag disambiguates. This is the "keyed on `(engine, stable-id)`"
  DEC-1 mandates (design-gate-verdicts.md:68).
- **`PRIMARY KEY` is the cross-engine `dedup_key`** — the same "endpoints + kind identify a
  relationship" rule as `Edge::dedup_key` (`edge.rs:162-171`); higher-confidence write wins on
  conflict (`GraphWrite::upsert_edges` semantics, `traits.rs:145`), enforced by
  `INSERT … ON CONFLICT … DO UPDATE SET … WHERE excluded.confidence > xedge.confidence`.
- **Confidence + provenance are MANDATORY columns**, `NOT NULL`, no defaults — directly honoring
  "Confidence + provenance on every edge … never present a heuristic edge as a fact" (CLAUDE.md;
  R7). The overlay client constructs them via the existing `Edge::new(tier, resolved_by)` so the
  values are derived from a `ResolutionTier`, never hand-set (`edge.rs:136-155`).

### D-X2 — Surface: a tiny `XedgeStore` + a thin `XedgeClient`; NOT a `GraphStore`

`xedge` is deliberately NOT a full `GraphStore` (no nodes, no FTS, no embeddings, no traverse — it
holds only boundary edges). Two narrow types:

- **`XedgeStore` (writer side, owns the single connection):**
  ```rust
  fn put(&mut self, edges: &[XEdge]) -> Result<usize>;     // upsert, higher-confidence wins
  fn prune_endpoint(&mut self, engine: &str, id: &str) -> Result<usize>; // delete rows touching id
  fn rename_endpoint(&mut self, engine: &str, old: &str, new: &str) -> Result<usize>;
  ```
- **`XedgeReader` (read side, WAL readers, cheap to clone):**
  ```rust
  fn out_edges(&self, engine: &str, id: &str, rels: &[String]) -> Result<Vec<XEdge>>; // src==(engine,id)
  fn in_edges (&self, engine: &str, id: &str, rels: &[String]) -> Result<Vec<XEdge>>; // tgt==(engine,id)
  ```
- **`XEdge` carries the same fields as the schema** and converts to/from `core::Edge` for the home
  engine the reader is currently anchored in (so the read-union yields ordinary `Edge`s).

WAL gives many concurrent readers + one writer on `xedge.db` safely — the same property estate.db
relies on; the single-writer discipline is the whole point.

### D-X3 — Read-union runs in the RETRIEVAL layer via an `OverlayReader` newtype

The union is **not** in any store's SQL and **not** in the engines. It is a thin `GraphRead`
adapter constructed at serve time:

```rust
struct OverlayReader<'a> {
    home: &'a dyn GraphRead,   // estate.db OR memory.db OR knowledge.db, unchanged
    home_engine: &'static str, // which engine `home` is
    xedge: XedgeReader,        // read-only handle on xedge.db
    // resolvers for the *other* engines' home stores, used to hydrate boundary endpoints:
    others: HashMap<&'static str, Arc<dyn AsyncGraphStore>>, // engine -> its serving pool
}
impl GraphRead for OverlayReader<'_> { /* see §3 for neighbors/traverse */ }
```

It is injected at the **one** existing seam: `with_read(move |graph| handle_request_ctx(
&OverlayReader{ home: graph, … }, …))` (today `graph` is passed raw,
`wicked-estate-mcp/src/main.rs:316`). Every tool keeps taking `&dyn GraphRead` (`traits.rs:228-233`)
— ZERO tool changes, ZERO engine changes. This is the §1-spine-respecting move: the overlay is a
new module wired through the existing trait, with the read-union as its consumer (CLAUDE.md §5).

### D-X4 — Who writes which cross-edge (each via the `XedgeClient`, into `xedge.db` only)

| cross-edge | written by | `src→tgt` | `rel` | when |
|---|---|---|---|---|
| mem → code | memory engine | `(memory,mem-id) → (estate,SymbolId)` | `about` | `capture_about` (replaces the in-store `upsert_edges`, `wicked-memory/.../lib.rs:289-291`), reached from MCP `memory.learn` (`wicked-memory-mcp/src/lib.rs:304-326`) |
| knowledge chunk → code | knowledge engine | `(knowledge,kchunk) → (estate,SymbolId)` | `mentions` | knowledge resolves a chunk's code mention (knowledge ingest path) |
| knowledge concept → code | knowledge engine | `(knowledge,kconcept) → (estate,SymbolId)` | `governs` | concept→code governance (the DEC-2 `Other("governs")` relation, now cross-engine) |

- **Each engine keeps its OWN writer for its OWN store** (unchanged) and additionally holds a thin
  `XedgeClient` to `xedge.db`. The client is the ONLY writer of `xedge.db`; engines never open each
  other's stores. (Whether the single `xedge.db` writer is a shared lock or an actor is a
  micro-decision — OQ-X1 — but it is one writer regardless.)
- **Retire-as-you-go (CLAUDE.md §8):** the change that lands xedge must DELETE the in-store `about`
  write in `capture_about` (`wicked-memory/.../lib.rs:289-291`) and redirect `about_seed_ids`
  (`.../lib.rs:359-372`) to the overlay read, in the SAME change — not leave both. The fuzzy
  `correspond` command (`wicked-estate/.../main.rs:2089`) and its helpers (`main.rs:103-241`) are
  superseded by typed xedge edges and should be retired or explicitly demoted to a discovery/
  suggest-only tool that PROPOSES xedge rows for human/LLM confirmation (it must never silently
  write fuzzy edges as facts — R7).

### D-X5 — Node-before-edge is now CROSS-store: existence checked at WRITE in the target's HOME store, recorded as confidence — without coupling

The trap: a silent dangling edge to an un-interned target (B-ADV-2, design-gate-verdicts.md:27;
already the rule in-store via `prune_dangling_edges`, `traits.rs:159-161`). Under separate stores
the target lives in a DIFFERENT engine's file.

Decision: **the writing engine verifies the target exists in the target engine's home store at
write time, via that engine's READ-ONLY MCP/CLI surface — never by opening its DB file.** Concretely
for the common mem→code case, memory ALREADY resolves the code symbol before linking:
`engine.resolve_code(name)` returns the estate `SymbolId`(s) that exist
(`wicked-memory/.../lib.rs:295-311`; MCP loop `wicked-memory-mcp/src/lib.rs:304-312` already records
`unresolved` names and captures UNLINKED when none resolve, `lib.rs:319-323`). So:

- **The id handed to `XedgeClient.put` is one that resolution already proved exists** — the write is
  node-before-edge by construction for the resolved path.
- **Coupling is avoided** because resolution goes through the target engine's read API (estate's
  retrieval/`find_symbols`), not its storage. xedge itself stores the edge opaquely and does NOT
  validate endpoints (it cannot see the home stores — that is the point).
- **Unresolved → not written**, surfaced loudly (the existing `(unresolved: …)` note,
  `wicked-memory-mcp/src/lib.rs:314-318`), never a fabricated edge.
- Residual staleness (target deleted AFTER a valid write) is the §4 problem, handled there.

---

## §3 — Read / union: the read-time join (the cost we accepted), bounded by relation type (C5)

**Where it runs:** the `OverlayReader` adapter in the retrieval layer (D-X3), behind `&dyn
GraphRead`. The engines and stores are untouched.

**`neighbors` (one hop):**
```
OverlayReader::neighbors(id, dir):
  1. home_edges = self.home.neighbors(id, dir)?          // native, in-store (sqlite.rs:1623)
  2. x = match dir {
        Dependencies => self.xedge.out_edges(home_engine, id.as_str(), rels)?  // src==id
        Dependents   => self.xedge.in_edges (home_engine, id.as_str(), rels)?  // tgt==id
        Both         => out ∪ in
     }
  3. cross_edges = x.map(XEdge::into_core_edge)           // ordinary core::Edge (edge.rs:117)
  4. return home_edges ++ cross_edges
```
`dir` maps cleanly to the column index: `Dependents` = "who points AT me" = `xedge_by_tgt`
(blast-radius direction, matching `Direction::Dependents` ⇒ `target==id`, `edge.rs:176-178`);
`Dependencies` = `xedge_by_src`. Because `SqliteStore::traverse` induces its edges by calling
`neighbors` per anchor (`sqlite.rs:1675-1687`), overriding `neighbors` ALSO injects the boundary
hop into `traverse` results — no CTE change for the first cross hop.

**`traverse` (multi-hop, cross-engine — the real cost):** a single-DB CTE cannot cross files
(`sqlite.rs:949,1652`). The overlay orchestrates a bounded alternation:
```
OverlayReader::traverse(start, spec):
  frontier = { (home_engine, start) }; out = Subgraph::default(); budget = spec.max_nodes
  for depth in 0..spec.max_depth:
     # (a) intra-engine: each engine expands its OWN nodes with its OWN bounded CTE
     for (engine, ids) in frontier.group_by_engine():
         sub = engine_store(engine).traverse_multi(ids, spec.with_depth(1))?   # native CTE, 1 ply
         merge(out, sub); decrement budget; if budget<=0 { out.truncated=true; break }
     # (b) cross-engine boundary: ONLY follow xedge rels allowed by spec (C5 bound)
     boundary = xedge.expand(frontier, dir=spec.direction, rels=allowed_rels(spec))?
     frontier = boundary.endpoints  # now possibly in OTHER engines
     if frontier.empty { break }
  return out  # nodes carry their engine; edges are core::Edge with provenance+confidence
```

**Bounding (this is what keeps it from regressing the bench, CLAUDE.md §9):**
- **Relation-type allow-list = C5.** `TraversalSpec.edge_kinds` (`core/src/query.rs:30-32`, parsed
  at `wicked-estate-retrieve/src/lib.rs:617-628`) already lets a caller restrict edge kinds. xedge
  rels are matched the SAME way: an empty list does NOT mean "all cross-edges" — for cross-engine
  expansion the **default allow-list is empty** (pure intra-engine traverse, today's behavior), and
  a caller OPTS IN to specific rels (`["about"]`, `["governs"]`). A blast-radius that wants code
  dependents only never touches xedge. This makes the join *pay-for-what-you-ask*.
- **Hop cap on cross-engine edges:** at most ONE engine-boundary crossing by default
  (`max_cross_hops = 1`), independent of `max_depth`. mem/knowledge are *leaves* off a code seed in
  the differentiator workload — you cross code→memory once, you don't then chase memory→code→memory.
  Configurable, defaulted low.
- **`max_nodes` budget is shared across engines** and decremented as each engine's CTE returns;
  `truncated=true` set the moment it's hit (mirrors `sqlite.rs:1666`). No unbounded whole-graph walk
  (CLAUDE.md "Bounded traversal only").
- **Confidence floor** on xedge rows reuses `spec.min_confidence` (`query.rs:35`).

**The differentiator read path — "recall grounding docs/decisions from a CODE seed":**
1. Agent is looking at code symbol `S` (estate `SymbolId`). Memory recall is called with `seeds=[S]`
   (`recall(query, scope, seeds, budget, now)`, `wicked-memory/.../lib.rs:377-393`).
2. `about_seed_ids` (today `self.store.neighbors(code, Dependents)` filtered to `"about"`,
   `.../lib.rs:359-372`) is **redirected to** `xedge.in_edges("estate", S, ["about"])` → the
   `(memory, mem-id)` sources. (Estate is no longer co-resident, so the native call would find
   nothing — this redirection IS the pivot.)
3. Those mem-ids are hydrated from **memory.db** (`self.store.get_node`, `.../lib.rs:454-458`),
   fused into the existing RRF (`hybrid_search(kw, graph, sem)`, `.../lib.rs:442-450`), reranked,
   budget-packed — the rest of recall is UNCHANGED.
4. Symmetric for "what docs/decisions govern `S`": `xedge.in_edges("estate", S, ["mentions",
   "governs"])` → `(knowledge, …)` ids → hydrate from knowledge.db. This delivers the
   differentiator via the overlay rather than a native FK, preserving
   `cross_edge_lifts_recall_the_unique_bet` (`.../lib.rs:618-659`) with the edge sourced from xedge.

**Staleness/coverage on the union (R5/R6):** the `OverlayReader` appends diagnostics to the
`RetrievalResult` (`core/src/query.rs:83-90`; tools already push `staleness_note()`,
`wicked-estate-retrieve/src/lib.rs:668,802`) — e.g. `XEDGE: 3 cross-edges unioned (about=2,
governs=1)`, and, on a §4 miss, `XEDGE-DANGLING: 1 edge whose target could not be hydrated from
knowledge.db (pruning queued)`. Loud, in-band, agent-visible (R6 marker discipline).

---

## §4 — Staleness / dangling: reconcile via the event catalog, with a read-time backstop — loud, never silent

A node deleted/renamed in its HOME store orphans the xedge row whose endpoint is in *another* file
xedge cannot see. Two layers, both loud:

**Primary — event-driven prune (matches the catalog, no new event names):**
- Home stores already log file-granular deltas and emit pinned coarse events. estate emits
  `wicked.estate.indexed {root, counts, commit, db_path, ts}` on (re)index
  (event-catalog-contract.md:17); the change-log seam is `GraphRead::changes_since(cursor)` +
  `GraphWrite::log_change` (`core/src/traits.rs:109-111,162-165`). Memory emits
  `wicked.memory.captured` (catalog:20); knowledge `wicked.knowledge.ingested` (catalog:22).
- **xedge runs a reconcile subscriber** reusing the existing cursor-poll primitive (durable cursor +
  TTL self-heal + DLQ + dedup, `wicked-brain/server/lib/memory-subscriber.mjs:1-55`; the catalog's
  designated reuse, event-catalog-contract.md:27-32). On `wicked.estate.indexed` it does
  **trigger→re-query** (events are COARSE — counts, not symbols, catalog:9-12): re-query estate for
  the set of estate ids xedge references, diff against live, and `prune_endpoint`/`rename_endpoint`
  the rows whose estate endpoint vanished or moved. Rename uses estate's stable-id property: a true
  rename yields a NEW `SymbolId` (`core/src/symbol.rs:217-231`), so reconcile maps old→new where the
  re-query can establish it, else prunes (loud).
- This keeps node-before-edge true over time: the edge is removed when its endpoint dies.

**Backstop — read-time existence check, bounded and loud (never silently dropped):**
- When the `OverlayReader` follows an xedge row to the other engine and **hydration returns `None`**
  (`get_node` miss in the target's home store — the same liveness check recall already does at
  `wicked-memory/.../lib.rs:432-434,455-457`), it: (a) **omits** the dead edge from the result
  (never returns an edge to a nonexistent node — no over-reporting, the `prune_dangling_edges`
  contract, `traits.rs:159-161`), and (b) emits a loud `XEDGE-DANGLING:` diagnostic AND enqueues a
  prune for the writer to apply (readers never write xedge). This is the read-time half of
  "loud, not silent" — a dangling row is reported the instant it's traversed, even before the event
  prune catches it.
- Contrast with the silent-failure the gate flagged: B-ADV-2's silent dangling edge that "returns
  Ok" (design-gate-verdicts.md:27). xedge does the opposite — drop + diagnostic + queued prune.

---

## §5 — Boundary: xedge ≠ garden's `injected:*` edges (Lane A/C, NOT Lane X)

**xedge holds ONLY knowledge/memory ↔ code boundary edges** (`about`, `mentions`, `governs`,
`supersedes`) — relations BETWEEN engines, keyed on `(engine, stable-id)`.

**Garden's `injected:dispatch` / `injected:bus` / `injected:capability` edges (C-aB1,
design-gate-verdicts.md:36) are a SEPARATE concern and are NOT xedge:**
- They are **intra-estate** synthetic edges — code↔code (a dispatch/bus/capability wiring within the
  code graph), the value garden's blast-radius depends on (C-aB1: estate `BlastRadius` is
  `Calls`-reachability and produces NONE of them today). They live in estate's OWN edge space as
  `EdgeKind::Other("injected:…")` on estate nodes, written by estate extractors (the `extra_edge` /
  synthetic-node machinery, `wicked-estate-extract/src/extra_edge.rs`; `Symbol::Synthetic`,
  `core/src/symbol.rs:101-103,124-129`). Resolving them is **Lane A absorbing injected edges into
  estate** (design-gate-verdicts.md:36), tracked under Lane A/C — NOT this overlay.
- **Litmus test:** does the edge connect two ids in the SAME engine? → that engine's native store
  (e.g. estate's `injected:*`). Does it connect ids in DIFFERENT engines (code↔memory, code↔
  knowledge)? → xedge. Garden's injected edges are code↔code → estate-native → out of scope for
  Lane X.
- Conflating them would (a) wrongly route intra-code edges through a cross-engine join they don't
  need (perf), and (b) leave garden's blast-radius regression (C-aB1) "fixed" in the wrong lane
  while it silently still under-reports. Stating the boundary keeps both fixes honest.

---

## RATIONALE

- **RATIONALE-1 (own file beats estate-table).** The DEC-1 reversal exists *because* combined/
  single-writer was an invasive, multi-writer-prone refactor (Lane W NO-GO, design-gate-
  verdicts.md:59-64). Estate.db already has exactly one writer — the indexer (`GraphWrite`
  "typically a single writer", `traits.rs:138`). A cross-edge table IN estate.db forces memory and
  knowledge to become writers of estate.db (they own `about`/`mentions`/`governs`), recreating
  "≥3 writers, one file → SQLITE_BUSY day one" (B-BLOCK-1, design-gate-verdicts.md:24) plus VACUUM/
  checkpoint exclusive-lock contention (B-W-3, :62). An own file with one writer is the *only* home
  that is symmetric with the topology DEC-1 ratified and that cannot reintroduce the trap. Cost of
  own file (an extra small DB, a read-time join) is exactly the cost DEC-1 *chose* over multi-writer
  (design-gate-verdicts.md:68).
- **RATIONALE-2 (union at retrieval, not in stores).** The MCP already funnels every tool through a
  single `&dyn GraphRead` (`wicked-estate-mcp/src/main.rs:316`), and tools never see a concrete
  store (`traits.rs:228-233`). Wrapping that one reference is the minimal, spine-respecting seam
  (CLAUDE.md §1/§5): no tool change, no engine change, no store change. Pushing the union into a
  store would require every store impl to know about xedge (violates storage-agnostic spine,
  `traits.rs:195-199`).
- **RATIONALE-3 (reuse `Edge`/`EdgeKind::Other`/`Confidence`/`Provenance`).** `Other(String)` was
  built for non-code edges (`edge.rs:90,114`) and `about` already uses it
  (`wicked-memory/.../lib.rs:283`). xedge rows convert to ordinary `core::Edge`, so every existing
  formatter (`edge_json`, denormalized endpoints, R7 confidence flagging,
  `wicked-estate-retrieve/src/lib.rs:683-722`) works unchanged.
- **RATIONALE-4 (relation-type opt-in = the bench guard).** The accepted cost is read-time join; the
  defense is that it's only paid when a caller names a cross rel. Empty `edge_kinds` stays
  intra-engine (today's behavior, today's bench numbers). The differentiator paths name their rels
  explicitly (`["about"]`, `["governs"]`). This is `TraversalSpec.edge_kinds` reused, not a new
  knob (`query.rs:30-32`).
- **RATIONALE-5 (events for prune, reusing pinned names + the subscriber).** The catalog already
  pins coarse `wicked.estate.indexed` / `wicked.knowledge.ingested` and mandates trigger→re-query
  (event-catalog-contract.md:9-12,17,22). xedge is just one more consumer reusing
  `memory-subscriber.mjs` mechanics (cursor + DLQ + TTL self-heal, catalog:27-32). No new event, no
  new transport.

---

## RISKS

- **R-X1 (riskiest — read-time-join cost regresses the bench).** Cross-engine recall is now
  home-CTE → xedge-lookup → other-home-hydrate, N+1 across processes, per call (`get_node`-per-id
  hydration, `wicked-memory/.../lib.rs:454-458`). If a hot symbol has many xedge rows, or
  `max_cross_hops` is set >1, latency balloons and the agent-eval benchmark regresses — the bar
  CLAUDE.md §9 forbids moving. Mitigations in §3 (rel allow-list default-empty, `max_cross_hops=1`,
  shared `max_nodes` budget, batch-hydrate ids in one `find_symbols`), but the *assumption that
  bounded cross-edges stay cheap at scale is unproven until benched.* This is THE thing the
  antagonist should attack and the bench must measure (parallel to PR-14's hybrid-uplift bench,
  `wicked-memory/.../lib.rs:88-100`).
- **R-X2 (cross-store dangling window).** Between a home-store delete and the event-prune, xedge
  rows are stale. The read-time backstop (§4) makes them loud + dropped, but a reader that bypasses
  the overlay (raw `xedge.db` SQL) sees stale rows — the same class as the file-level codegraph
  bypass that bit Lane C (C-aB3, design-gate-verdicts.md:38). Mitigation: xedge has no public raw-SQL
  consumer by design; all reads go through `XedgeReader`/`OverlayReader`. Must stay true.
- **R-X3 (id-space drift across engines).** xedge keys on `(engine, stable-id)`; if memory or
  knowledge mint ids that aren't stable under their own renames, prune/rename reconcile (§4)
  mis-fires. estate is safe (stable `SymbolId`, ADR-002, `symbol.rs:1-7`); memory mem-ids are
  derived from scope+content and must be equally stable. The cross-source dedup ADV
  (design-gate-verdicts.md:41 — `also_found_in` across 3 id-spaces with no equivalence fn) is
  RELATED but distinct: xedge does NOT attempt cross-engine *identity* merge, only typed *edges*;
  dedup of "same concept in 3 spaces" stays out of xedge (it'd need the equivalence fn nobody owns).
- **R-X4 (writer liveness for `xedge.db`).** One writer is the invariant, but the CLI/tools could
  accidentally open a second writer (the B-W-3 pattern, design-gate-verdicts.md:62). Mitigation:
  exactly one `XedgeStore` (writer); everything else gets `XedgeReader`. An advisory writer-lock +
  loud conflict on a 2nd writer open (as B-W-3 prescribes) is cheap insurance — OQ-X1.
- **R-X5 (recall redirection is a behavior change, must be atomic).** Redirecting `about_seed_ids`
  from in-store `neighbors` to `xedge.in_edges` (§3) and deleting the in-store `about` write
  (`wicked-memory/.../lib.rs:289-291`) MUST land together (CLAUDE.md §8) or recall silently loses
  the cross-edge arm (returns the `GraphOnly` mode as empty — the silent-regression class). The
  conformance test must assert the lift comes from xedge post-change
  (`cross_edge_lifts_recall_the_unique_bet` rewired to a separate-store fixture).

---

## OPEN QUESTIONS

- **OQ-X1 — single-writer mechanism for `xedge.db`:** advisory file-lock + loud conflict (B-W-3
  style) vs a tiny writer actor vs "whoever holds the one `XedgeStore`". All are one-writer; pick the
  cheapest that's enforceable. Leaning: advisory lock + `XedgeReader`-everywhere, no actor (the
  actor was Lane W's complexity we're escaping).
- **OQ-X2 — does `OverlayReader` need the OTHER engines' serving pools, or just their read MCP?**
  §3 hydration of boundary endpoints needs to read foreign home stores. In-process (estate + memory
  as libraries, today's shape, `wicked-memory/.../lib.rs:1-6`) it's a direct `&dyn GraphRead`; across
  separate MCP servers it's a read RPC. Which deployment is v1? Affects whether the join is in-proc
  cheap or network-bounded (feeds R-X1).
- **OQ-X3 — knowledge id minting.** kconcept/kchunk ids must be stable strings shaped like
  `SymbolId`. Lane B/knowledge owns this; xedge stores opaquely. Confirm the format + stability
  contract before wiring `mentions`/`governs`.
- **OQ-X4 — `max_cross_hops` default.** Set to 1 here (code→{memory,knowledge} leaves). Is there a
  real 2-hop workload (memory→code→knowledge in one traverse)? If not, hard-cap at 1 and make >1
  an explicit, bench-gated opt-in.
- **OQ-X5 — fate of `correspond` / `cross_graph_*`.** `correspond` (fuzzy Jaccard,
  `wicked-estate/.../main.rs:103-241,2089`) and `cross_graph_search/blast_radius`
  (`.../lib.rs:1077-1133`, name-union across repos) overlap xedge's intent. Retire `correspond`'s
  silent-edge use, OR repurpose it as a suggest-only producer of candidate xedge rows for
  confirmation (never auto-writing fuzzy edges as facts — R7). Decide in-lane (CLAUDE.md §8).
- **OQ-X6 — schema migration / versioning for `xedge.db`.** Mirror memory's `meta`-table forward-
  migration (`MEM_SCHEMA_VERSION`, `wicked-memory/.../lib.rs:58-59`)? Cheap to add now, painful
  later.
