# Lane W — the single-writer seam (DESIGN)

> **Framing (DECIDED — DEC-1, not relitigated here).** Estate owns the SOLE writer to the shared
> store. Memory + knowledge MCPs + brain's reactive daemon are READ clients that route WRITES
> through estate's write API. This is the shape wicked-memory ALREADY has — it rides estate as a
> library (`MemoryEngine` holds `Box<dyn MemStore>`, `MemoryEngine::open` →
> `SqliteStore::open`, `wicked-memory/crates/wicked-memory/src/lib.rs:124-125`). NOT "3 peer
> servers funneling writes." Reads stay multi-client over WAL. Knowledge chunks get their OWN FTS
> table (fixes B-BLOCK-2). Standalone = each engine its own writer; combined = estate is the writer.
>
> DESIGN ONLY. Reviewer + antagonist attack this next. Every claim cites real code.

---

## 0. The seam in one sentence

`AsyncGraphStore` today is `with_read`-ONLY (`crates/wicked-estate-core/src/traits.rs:213-218`) — there
is **no write seam at all** at the async layer. Lane W adds a **single-writer actor** that owns the
one `&mut dyn GraphWrite` connection behind a `WriteApi` (a `mpsc` command queue), exposes it via a
new `with_write`/`submit` method on a `WriteHandle`, and routes EVERY mutation — memory `capture`,
knowledge `ingest`/`write`/`relate`, the `Annotate` tool, the daemon's deterministic re-links —
through that one actor. Reads keep going through the existing `SqlitePool` (8 read connections,
`pool.rs:41`) over WAL. One writer ⇒ no `SQLITE_BUSY` ⇒ B-BLOCK-1 dissolved by construction.

---

## 1. Grounded problem statement (why this lane exists)

| # | Evidence (file:line) | What it proves |
|---|---|---|
| W-E1 | `traits.rs:213-218` — `AsyncGraphStore { async fn with_read<F,T>(…) }` | The async serving trait has **only** a read method. No write seam. C-A2 / the async-pool gap is real and structural, not a wiring oversight. |
| W-E2 | `mcp/src/main.rs:143` opens `open_async_store` (pool, 8 conns) **and** `main.rs:175-179` opens a 2nd `SqliteStore::open(&db)` for SemanticSearch | The estate MCP process already holds **two** independent SQLite handles to one file. Both are read-only today, but the 2nd is a full `SqliteStore` (`GraphStore` = read+write). |
| W-E3 | `wicked-memory-mcp/src/main.rs:21` `MemoryEngine::open(&db)` → `lib.rs:125` `SqliteStore::open(path)` | The memory MCP opens its OWN writer `Connection`. Its doc comment literally says "Point this at the SAME db as `wicked-estate-mcp` … for a unified code+memory graph." That is the **multi-writer, one-file** configuration B-BLOCK-1 condemns. |
| W-E4 | `sqlite.rs:337-343` `open()` sets `journal_mode=WAL; synchronous=NORMAL; auto_vacuum=INCREMENTAL` — **no `busy_timeout`** | A 2nd writer hitting a held write-lock fails IMMEDIATELY with `SQLITE_BUSY`, not after a wait. First `ingest`⊕`capture` overlap → error on day one. |
| W-E5 | `sqlite.rs:1503` `shared_writers: false` | The store SELF-DECLARES it is not safe for concurrent writers. Retrieval negotiates this via `StoreCapabilities` (`traits.rs:57-68`); nothing today enforces single-writer at the process boundary. |
| W-E6 | `sqlite.rs:1095-1133` `upsert_edges` interns BOTH `e.source.0` and `e.target.0` (`self.intern(...)`) | The raw `GraphWrite` will **silently create a symbol-intern row for a non-existent target** and write a dangling edge (no node). This is B-ADV-2 at the trait level — the write API must verify endpoints, not just `relate`. |
| W-E7 | `schema.sql:46-51` `nodes_fts USING fts5(symbol UNINDEXED, name, signature, doc)` — ONE unweighted table | Paragraph chunks in this one BM25 space shift corpus avg-doc-length and perturb every code-symbol score (B-BLOCK-2). Knowledge needs its OWN FTS table. |
| W-E8 | `main.rs:195-244` request cache invalidates ONLY on `db_path` **mtime change** | An in-process write that does NOT change the watched file's mtime in time (WAL writes touch `-wal`, not the main file's mtime until checkpoint) ⇒ the loop serves a STALE cached read after its own write (C-A2). |
| W-E9 | `wicked-memory/src/lib.rs:114-142` — `MemoryEngine` is built over `Box<dyn MemStore>`; `MemExt::open` opens `<path>.memext` (`memext.rs:142-146`), a SEPARATE file | **Precedent**: memory already runs a two-file topology — shared estate graph + its own `.memext` FTS/salience sidecar. The "knowledge gets its own FTS table" decision is the same move, one table instead of one file. |
| W-E10 | `pool.rs:26-33` `recycle` is a no-op; `pool.rs:73-89` `with_read` does `spawn_blocking(f(&*obj))` | The pool gives N read connections but has no concept of a designated writer. A writer added naively as "just another pool checkout" would reintroduce multi-writer. The writer must be a SINGLE long-lived connection, not pool-managed. |

**The contradiction DEC-1 resolves:** you cannot have BOTH "3 writers, one file" AND "WAL makes it
safe." WAL gives unlimited concurrent *readers* + ONE *writer*. The fix is not `busy_timeout`
(necessary-but-insufficient: it converts hard errors into latency + checkpoint-starvation under a
write-heavy daemon). The fix is to make the "one writer" literally true: one connection, one owner,
everything else routes to it.

---

## 2. DECISIONS

### DEC-W1 — The write API estate exposes: `WriteApi` (a typed command surface over the single writer)

A new trait in `wicked-estate-core`, sibling to `GraphWrite`, expressing the **operations callers
need** as coarse, atomic, idempotent units — NOT a thin passthrough of `GraphWrite`'s 14 methods.
`GraphWrite` stays the storage-impl contract (held by the writer connection); `WriteApi` is the
**caller-facing** contract the actor speaks.

```rust
/// Caller-facing write surface. Every method is ONE atomic unit (its own transaction unless
/// inside an explicit `batch`). Implemented by the single-writer actor's handle. Async because
/// the call crosses the actor's mpsc queue; the actor runs the sync `GraphWrite` inside.
#[async_trait]
pub trait WriteApi: Send + Sync + Clone {
    /// Batch upsert: nodes are written BEFORE edges (node-before-edge ordering enforced HERE,
    /// not left to the caller). Edges whose endpoints are not present as nodes — either in this
    /// batch or already in the store — are REJECTED (fixes W-E6 / B-ADV-2), not silently interned.
    /// Returns per-edge resolution so the caller learns which edges were dropped and why.
    async fn write(&self, batch: WriteBatch) -> Result<WriteReceipt>;

    /// Typed-edge relate with endpoint verification. `src` and `tgt` MUST resolve to existing
    /// nodes; an un-interned endpoint is an error (isError), never a dangling no-op. `kind` is an
    /// EdgeKind (knowledge passes `Other("governs")` etc. per DEC-2). Idempotent on dedup_key.
    async fn relate(&self, src: SymbolId, tgt: SymbolId, kind: EdgeKind, prov: Provenance) -> Result<()>;

    /// Attach a typed annotation. Server FORCES author + non-empty provenance (C-A1), and FORCES
    /// requirement_validated=false on requirement annotations (A-aB4 provenance-laundering guard).
    /// No-op-on-absent becomes an ERROR here (the symbol must exist).
    async fn annotate(&self, symbol: SymbolId, ann: Annotation) -> Result<()>;

    /// Remove everything a source contributed (incremental re-ingest): remove_file + prune
    /// dangling + log_change, as ONE transaction. Used by knowledge remove-by-source (T3) and
    /// estate incremental re-index alike.
    async fn remove_source(&self, source: &str) -> Result<RemoveReceipt>;

    /// Embedding write (memory/knowledge vector arm). Co-located with the node write so the
    /// vector and the node it belongs to commit together (no half-indexed symbol on crash).
    async fn set_embedding(&self, symbol: SymbolId, vec: Vec<f32>, embedder_id: String) -> Result<()>;

    /// Explicit multi-call transaction for the rare compound op (e.g. an expedition writing
    /// concepts + typed relations + chunk→concept edges atomically). The closure receives a
    /// `&mut dyn GraphWrite` — the ONE writer connection, inside `begin_batch`/`commit_batch`.
    async fn batch<F>(&self, f: F) -> Result<()>
    where F: FnOnce(&mut dyn GraphWrite) -> Result<()> + Send + 'static;
}
```

`WriteBatch` carries `{ nodes: Vec<Node>, edges: Vec<Edge>, unresolved: Vec<UnresolvedRef>,
content: Vec<(file, text)>, embeddings: Vec<(SymbolId, Vec<f32>)> }`. The actor applies them in a
single SQLite transaction in dependency order (symbols → nodes → embeddings/content → edges →
unresolved → change-log), so a partially-applied batch can never be observed.

**Surface rationale (maps to the program's write needs):**
- memory `capture` / `capture_about` (`lib.rs:257-296`) → `write(batch{nodes, embeddings})` + `relate` for `about` edges.
- knowledge `ingest` (S1, T3) → `write(batch{document/section/chunk nodes, contains/about/mentions edges, embeddings, content})` — one atomic doc.
- knowledge `write`/`relate` typed C5 edges (T3) → `relate(src, tgt, Other("governs"|"contradicts"|…), prov)`.
- `Annotate` MCP tool (Lane A) → `annotate(symbol, ann)` with the C-A1/A-aB4 server-side forcing.
- daemon deterministic re-links (C5 `about`/`mentions`) → `relate(...)`.

**Atomicity guarantees (the contract the reviewer will hold us to):**
1. Each `WriteApi` call is atomic: it either fully commits or the store is unchanged (one SQLite txn).
2. **Node-before-edge** is enforced inside `write`; callers cannot emit an edge to a missing node.
3. **Endpoint verification** on `relate`/edges: both endpoints must already be nodes OR be in the same `write` batch — else the edge is rejected with a per-edge reason (no silent dangling, W-E6).
4. **Idempotent under retry**: `write`/`relate` are upserts keyed on `(source,target,kind)` (`sqlite.rs:1110`) / `dedup_key`; a re-submitted command after an actor restart yields the same end-state.
5. **Writer-down is loud**: if the actor task is gone or the channel is closed, every `WriteApi` call returns `Err(Error::WriterUnavailable)` → the MCP tool returns `isError:true` (R1/R6). Never a silent success.

### DEC-W2 — How it's reached: in-process single-writer ACTOR (recommended), cross-process write-RPC as the fallback shape

**Reach decision (the headline): IN-PROCESS SINGLE-WRITER ACTOR.** When memory/knowledge run AS
estate library layers (the DEC-1 facade shape), they hold a `WriteHandle` (a cloneable `mpsc::Sender`
to the actor) — an in-process channel send, not IPC. This is the recommended deployment and the one
the combined brain uses.

```
                          ┌─────────────────────────────────────────────┐
   memory facade ─┐       │  estate process (combined mode)               │
   knowledge facade├─ WriteApi(Handle: mpsc::Sender) ─▶ ┌──────────────┐  │
   Annotate tool ─┤       │                              │ WRITER ACTOR │  │
   daemon re-link ┘       │                              │ owns the ONE │  │
                          │                              │ &mut Sqlite  │  │
   all reads ────────────▶│  SqlitePool (8 read conns) ◀─┤  (GraphWrite)│  │
   (memory/knowledge/     │  with_read (pool.rs:73)      └──────┬───────┘  │
    estate retrieval)     │         ▲                           │ commit   │
                          │         └── cache invalidation ─────┘ (epoch)  │
                          └─────────────────────────────────────────────┘
                                              one graph.db (WAL)
```

The actor:
- Owns a single `SqliteStore` opened ONCE (the writer connection). It is **not** pool-managed (W-E10: pool checkouts would reintroduce multi-writer).
- Receives `WriteCommand`s on a bounded `mpsc` channel. Processes them serially. Each command = one transaction. Serialization is in Rust (channel order), so the writer never contends with itself and `SQLITE_BUSY` cannot arise from within the process.
- Holds the writer connection in a dedicated OS thread (sync `rusqlite` is `!Send` across `.await`); the async `WriteApi` methods send a command + a `oneshot` reply channel and `.await` the reply. This mirrors the existing `spawn_blocking` discipline already used for reads (`pool.rs:83`).
- Sets `busy_timeout` anyway (defense-in-depth for any stray external `wicked-estate index` process — see DEC-W4), but in normal operation the timeout is never hit because there is exactly one writer.

**`AsyncGraphStore` gap resolution.** Extend the async seam so it has a write side, symmetric to
`with_read`:

```rust
#[async_trait]
pub trait AsyncGraphStore: Send + Sync {
    async fn with_read<F, T>(&self, f: F) -> Result<T> where …;          // unchanged (traits.rs:214)
    /// Hand the caller the single writer. The impl guarantees serialization: only ONE `with_write`
    /// body runs at a time, process-wide. For the pool+actor impl this enqueues onto the actor.
    async fn with_write<F, T>(&self, f: F) -> Result<T>
    where F: FnOnce(&mut dyn GraphWrite) -> Result<T> + Send + 'static, T: Send + 'static;
}
```

`SqlitePool` (the `AsyncGraphStore` impl, `pool.rs:74`) gains a writer-actor field. `with_read` keeps
checking out a read connection; `with_write` routes to the actor. The high-level `WriteApi` methods
are thin wrappers over `with_write` that add the node-before-edge ordering + endpoint verification +
C-A1 forcing. **This is the seam that does not exist today** (W-E1) and is Lane W's deliverable.

**Cross-process shape (fallback / standalone-coexistence).** If memory/knowledge run as SEPARATE MCP
server processes pointed at the same file (the W-E3 configuration that exists in the binaries today),
the in-process actor is not shared across processes — so DEC-W2's guarantee would break. Resolution:
in a co-deployed combined brain we **do not** run them as separate writer processes. The cross-process
write path is a write-RPC: the non-estate process sends its mutation to the estate process's writer
(over the same MCP/stdio or a unix socket) rather than opening its own `SqliteStore`. The estate
process remains the sole holder of a writer connection. The memory/knowledge MCP binaries change from
"open my own `SqliteStore::open`" to "open a read pool + a `WriteHandle` that targets the estate
writer." (See DEC-W6 for how this stays one code path with standalone.)

**Request-cache coherence (C-A2 — must invalidate on in-process write).** Today the cache clears only
on db-file mtime change (W-E8), which an in-process WAL write does not reliably trigger. Fix: the
writer actor publishes a monotonically increasing **write epoch** (an `AtomicU64`) on every committed
command. The MCP request loop (`main.rs:236-244`) replaces its mtime check with an epoch check:

```
if current_epoch != cached_epoch { request_cache.clear(); cached_epoch = current_epoch; }
```

Keep the mtime check too, for external `wicked-estate index` writers (which bump mtime but not the
in-process epoch). Cache key (`main.rs:247-253`) is unchanged; only the invalidation trigger gains the
epoch. This closes the "in-process write serves stale read" hole the reviewer flagged.

### DEC-W3 — Reads stay concurrent; writes serialize through the one writer

- Reads: unchanged. memory recall, knowledge recall, estate retrieval all go through `with_read` →
  `SqlitePool` (8 connections, `pool.rs:41`) over WAL. WAL permits unlimited concurrent readers
  alongside the single writer. No reader ever blocks on the writer except for the microsecond WAL
  page-visibility boundary.
- Writes: every mutation is one `WriteCommand` on the actor's mpsc queue, applied serially. Because
  there is exactly ONE writer connection (DEC-W2), the SQLite-level writer lock is never contended
  from within the process ⇒ **`SQLITE_BUSY` is structurally impossible** in combined mode.
- Backpressure: the mpsc channel is BOUNDED (e.g. 1024). Under a write storm (e.g. the `watch`-path
  per-debounce-batch issue, A-aB6), `WriteApi::write` `.await`s on a full channel rather than dropping
  — visible latency, never silent loss. Coalescing of watch-path batches happens BEFORE the queue
  (Lane A's concern), so the queue carries coalesced units.

### DEC-W4 — Failure modes loud; FK cross-edges resolvable; knowledge's own FTS table

**Loud failure (R1/R6):**
- Writer actor task panics or its thread dies → the `WriteHandle`'s channel send fails → `WriteApi`
  returns `Err(WriterUnavailable)` → MCP tool result `isError:true` with a `WRITER-DOWN:` marker.
  The agent SEES it (contrast A-aB2's CLI-stderr-the-agent-never-sees anti-pattern).
- Endpoint verification failure (relate to a missing node) → `isError:true` naming the missing
  endpoint (B-ADV-2). Never the silent dangling edge `upsert_edges` would write (W-E6).
- `busy_timeout` (DEC-W2, defense-in-depth) is set so a stray external writer produces a *delayed*
  error, not an instant `SQLITE_BUSY` crash — but in-process this path is never taken.

**Native FK cross-edges stay resolvable (the whole point of combined).** Because all writers share
ONE store and ONE symbol-intern table (`schema.sql:20-23`), a knowledge `mentions` edge or a memory
`about` edge to a code symbol resolves natively: the code symbol's `SymbolId` is already interned by
estate's indexer, and `relate` verifies the endpoint exists before writing. `find_by_requirement`
(`traits.rs:116`), `neighbors`, and `traverse` (`traits.rs:81-84`) then see code↔knowledge↔memory
edges in one graph — the "recall from code for free" differentiator (D1.2). Splitting into separate
files would degrade this to the `correspond` shim; the single store keeps it native.

**Knowledge's own FTS table (fixes B-BLOCK-2 without a separate DB).** Add a second FTS5 virtual
table to the schema, parallel to `nodes_fts` (`schema.sql:46-51`):

```sql
-- Knowledge chunk full-text. SEPARATE BM25 space so paragraph-length chunks never perturb the
-- code-symbol corpus statistics in nodes_fts (B-BLOCK-2). Same join-back pattern: symbol UNINDEXED.
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  symbol UNINDEXED,   -- string SymbolId of the chunk node; join via symbols.sym → sid → nodes
  title,              -- section/heading context
  body                -- the chunk text (the searchable content, C1 "chunk content full-text-searchable")
);
```

`write` routes chunk nodes (NodeKind `Other("chunk")`) to `chunks_fts` and code nodes to `nodes_fts`
— same single writer, same transaction, two disjoint BM25 spaces. Code-symbol scores are untouched;
estate's agent-eval benchmark (CLAUDE.md §9) does not regress. This is the "own FTS table, not a
separate file" form of W-E9's precedent.

### DEC-W5 — `WriteApi` lives in core; `SqlitePool` implements it; the actor lives in the store crate

- `WriteApi` + `WriteBatch`/`WriteReceipt` + the `with_write` addition to `AsyncGraphStore` go in
  `wicked-estate-core/src/traits.rs` (the spine — §1 of CLAUDE.md: spine before fan-out). This is a
  trait-spine change and MUST land + get a conformance test green BEFORE memory/knowledge wire to it.
- The single-writer actor (`WriterActor`, the dedicated thread + mpsc + epoch) lives in
  `wicked-estate-store/src/writer.rs`, behind the `serve` feature (alongside `pool.rs`). `SqlitePool`
  gains the actor and the `with_write`/`WriteApi` impl. Zero change to `Extractor`/`Resolver`/`Ranker`/
  `RetrievalTool`.
- A conformance test (extend `conformance.rs`) asserts: (a) `with_write` serializes (two concurrent
  writers see no `SQLITE_BUSY`, final state consistent); (b) endpoint verification rejects dangling
  edges; (c) write epoch increments per commit; (d) `WriterUnavailable` surfaces when the actor is
  dropped.

### DEC-W6 — Standalone vs combined: ONE code path, two factory arms

The duality is resolved at the FACTORY, not in two divergent engines. Both memory and knowledge are
already built over a `Box<dyn MemStore>`/`Box<dyn GraphStore>` (`wicked-memory/src/lib.rs:44,134-142`)
— the backend is injected. Lane W adds a write-target injection alongside it.

- **Standalone (engine is its own writer):** the factory hands the engine a `WriteHandle` whose actor
  owns a LOCAL `SqliteStore` for that engine's own file (memory's `memory.db`, knowledge's own KB).
  Each engine is its own single writer over its own file. No estate present. The lexical floor (C4)
  works offline.
- **Combined (estate is the writer):** the factory hands the engine a `WriteHandle` that targets
  ESTATE's writer actor (in-process channel in the combined brain, or write-RPC if separate
  processes). The engine writes the SAME way — `WriteApi::write(...)` — it just lands in estate's
  store with native code cross-edges.

The engine code is identical in both modes: it calls `WriteApi`. Only which `WriteHandle` it was
constructed with differs — exactly how `MemoryEngine::with_backend` (`lib.rs:134`) already swaps the
store. No `if combined { … } else { … }` branches in the write path; the divergence is one
constructor argument resolved at process start. This is the "without two code paths diverging"
requirement satisfied by dependency injection, the pattern the codebase already uses.

---

## 3. RATIONALE (why this shape over the alternatives)

- **Actor over `busy_timeout`+serialize.** `busy_timeout` alone (the B-BLOCK-1 "necessary but
  insufficient" option) leaves N writer connections fighting; under the reactive daemon's write rate
  it converts crashes into latency spikes and **checkpoint starvation** (WAL checkpoint needs a brief
  exclusive lock that a steady stream of writers can indefinitely defer). The actor makes "one writer"
  literally true, so the failure mode is designed out, not merely softened. `busy_timeout` is retained
  only as defense against a stray external `index` process (DEC-W4).
- **Actor over separate-files + read-time union.** Separate files would sidestep multi-writer but
  destroy the native code↔knowledge cross-edge (D1.2) — the differentiator — degrading it to a
  read-time `correspond` shim. DEC-1 already rejected this for one local brain. The own-FTS-table move
  (DEC-W4) gets B-BLOCK-2's benefit (disjoint BM25 spaces) WITHOUT giving up the shared graph.
- **Reuse the existing pool for reads.** `SqlitePool` + `with_read` already work and are conformance-
  tested (`pool.rs:130-186`). Lane W is purely additive on the write side; reads are untouched, so the
  agent-eval benchmark surface does not move.
- **Symmetric `with_read`/`with_write` on `AsyncGraphStore`.** Keeps the spine coherent: one async
  serving trait, two sides, mirroring the sync `GraphRead`/`GraphWrite` split (`traits.rs:74,141`).
  An external DB (ADR-003) implements `with_write` with its own concurrency (it can set
  `shared_writers:true`); the local file implements it with the actor. Retrieval/tools never know.
- **Epoch over mtime for cache coherence.** The mtime check (W-E8) is correct for external writers but
  blind to in-process WAL writes. An in-process epoch is the cheapest correct signal and composes with
  the existing mtime check rather than replacing it.
- **Endpoint verification in `WriteApi`, not in `upsert_edges`.** `upsert_edges` is the low-level
  storage primitive used by the bulk indexer, where interning unknown targets is sometimes legitimate
  (forward references resolved later). The VERIFICATION belongs at the caller-facing `WriteApi.relate`/
  `write` layer (B-ADV-2), where "this edge must point at a real node NOW" is the contract. This keeps
  the indexer fast and the agent-facing writes honest.

---

## 4. RISKS

- **R-W1 (riskiest assumption): the cross-process configuration that ships TODAY violates the
  single-writer invariant, and the combined brain may still launch the MCPs as separate processes.**
  The memory MCP binary opens its own writer (`wicked-memory-mcp/src/main.rs:21`) and its doc comment
  *instructs* pointing it at estate's db. If the brain's compose step keeps launching three separate
  MCP server processes against one file, DEC-W2's in-process actor is NOT shared — three writer
  connections reappear and B-BLOCK-1 returns. The design's guarantee holds ONLY if combined mode runs
  memory/knowledge AS LIBRARY LAYERS in the estate process (or routes their writes via write-RPC).
  This is a deployment/topology commitment Lane C must honor; it is not enforced by the type system.
  **Falsifier:** two separate MCP processes pointed at one file, each issuing a write within the same
  WAL-write window, still produce `SQLITE_BUSY` (or silent divergence) — the actor cannot prevent what
  it does not own.
- **R-W2: `rusqlite::Connection` is `!Send` across `.await`; the actor thread + oneshot reply adds a
  thread-hop per write.** Latency per write grows by a channel round-trip + thread wakeup. For the
  bulk indexer (thousands of `upsert_nodes`) this could regress index time if every node batch hops
  the channel. Mitigation: the bulk indexer keeps using a directly-owned `SqliteStore` (it is the sole
  process at index time); the ACTOR path is for the *serving* runtime (MCP/daemon writes), which are
  comparatively low-rate. Two write entry points, but ONE invariant (only one writer connection open
  at a time per file) — the indexer and the serving actor never coexist on the same file.
- **R-W3: write epoch vs WAL visibility race.** The epoch increments at commit, but a reader on
  another pool connection might observe the committed page a few microseconds later. A read that
  raced an epoch bump could serve data one transaction stale for that window. For a local brain this
  is benign (next call is correct), but a test asserting strict read-your-writes ACROSS connections
  must account for it. In-process read-your-writes (same actor, sequential) is exact.
- **R-W4: `WriteApi::batch` hands out `&mut dyn GraphWrite`.** A misbehaving caller could hold it and
  do unbounded work inside the actor, stalling the single writer for everyone (head-of-line blocking).
  Mitigation: document `batch` as "small compound ops only"; consider a soft time budget + a metric on
  actor-occupancy. The common path (`write`/`relate`/`annotate`) does not expose the raw connection.
- **R-W5: scope of the spine change.** Adding `with_write` to `AsyncGraphStore` (`traits.rs:213`) is a
  breaking trait change; every `AsyncGraphStore` impl (today only `SqlitePool`) must implement it, and
  the conformance kit must cover it BEFORE fan-out (CLAUDE.md §1). Low blast radius now (one impl), but
  it gates memory/knowledge wiring — it must land first.
- **R-W6: knowledge `chunks_fts` doubles FTS write cost on ingest.** Every chunk write touches a second
  virtual table. For large-document ingest this is real I/O. Acceptable (ingest is not latency-
  critical), but the write-batch-size metric (`sqlite.rs:1057-1091`) should be extended to cover chunk
  FTS so regressions are visible.

---

## 5. OPEN QUESTIONS

- **OQ-W1:** Combined-mode deployment — does Lane C run memory/knowledge as in-process library layers
  in the estate process (clean: shared actor) OR as separate MCP processes with write-RPC to estate
  (more moving parts, a new RPC surface to design)? DEC-W2 recommends in-process; R-W1 says the
  decision must be explicit and is currently contradicted by the shipping binaries. **Needs Lane C +
  Lane B sign-off.**
- **OQ-W2:** Does the bulk indexer route through the actor too (one write path, simpler invariant, R-W2
  latency cost) or stay direct (fast, but two write entry points to reason about)? Leaning: indexer
  stays direct because it is the sole writer at index time; serving runtime uses the actor. Confirm no
  scenario runs `index` and the serving daemon against one file simultaneously.
- **OQ-W3:** `with_write` signature — hand out `&mut dyn GraphWrite` (flexible, but R-W4 head-of-line
  risk) vs only the coarse `WriteApi` verbs (safer, but the rare compound op needs `batch`). Current
  design offers both; is `batch` worth the footgun, or should compound ops be modeled as explicit
  multi-op `WriteBatch` variants instead of a closure?
- **OQ-W4:** Bounded mpsc capacity + backpressure policy under the watch-path storm (A-aB6). 1024 is a
  guess. Should `WriteApi::write` block (backpressure) or return a `Busy` error the caller retries?
  Block is simpler and loss-free; error gives the caller agency. Leaning block + a queue-depth metric.
- **OQ-W5:** Standalone knowledge's own FTS — does standalone knowledge reuse `chunks_fts` in its OWN
  file (consistent), or does it just use `nodes_fts` since there are no code symbols to dilute? Leaning
  reuse `chunks_fts` everywhere so combined and standalone share one schema (DEC-W6 one-code-path).
- **OQ-W6:** Endpoint verification cost — `relate`/`write` must check both endpoints exist. For a large
  ingest batch (many `mentions` edges) that is many `SELECT sid FROM symbols`. Batch the verification
  (one `IN (...)` query per batch) — confirm the receipt-per-edge contract survives batching.
- **OQ-W7:** Interaction with the L2 SQLite request cache (`main.rs:277-299`, `cache_put` via the pool,
  `pool.rs:63`). `cache_put` is a WRITE through a pool connection — that is a SECOND writer today. Must
  the cache write ALSO route through the actor (consistent single-writer) or move to its own file?
  This is a latent multi-writer the design must absorb. **Flag for the reviewer — `cache_put` is a
  writer the framing did not enumerate.**

---

## 6. Summary for the gate

- **Writer-reach decision:** IN-PROCESS SINGLE-WRITER ACTOR. Memory + knowledge + Annotate + daemon
  hold a cloneable `WriteHandle` (mpsc to the actor) and call a typed `WriteApi`; the actor owns the
  ONE `&mut dyn GraphWrite` connection on a dedicated thread. Reads stay on the existing `SqlitePool`/
  `with_read` over WAL. Cross-process is the fallback shape (write-RPC to estate's writer), explicitly
  NOT three separate writer processes on one file.
- **Riskiest assumption (R-W1):** the guarantee holds only if combined mode runs memory/knowledge as
  library layers in the estate process (or routes their writes to estate's actor). The shipping MCP
  binaries today each open their OWN `SqliteStore` writer (`wicked-memory-mcp/src/main.rs:21`) against
  the same file — the exact multi-writer configuration this lane abolishes. The actor cannot enforce
  what it does not own; Lane C's compose topology must honor single-writer, and OQ-W7's `cache_put` is
  a writer nobody counted.
