# Lane A — estate: DESIGN v2 (FINAL) — knowledge-capability program + xedge estate-side foundation

> **Status:** DESIGN ONLY. No code changed. Every code claim cites a real file:line, re-verified at
> the line for this round.
>
> **What changed vs v1.** v1's reviewer returned CONDITIONAL PASS and its silent-failure antagonist
> returned **6 BLOCKING + 4 ADV** — the theme was *"A's honesty mechanisms are asserted, not built,
> and several would ship silent-wrong."* v2 folds every blocking condition (C-A1..C-A4, A-aB1..A-aB6)
> and the **xedge estate-side work A now owns** from the **Lane X v3 PASS** (`symbols.gen` +
> `symbol_epoch`; `traverse_multi` as the 22nd `GraphRead` method + SqliteStore specialization;
> `with_read_inline` + a `max_blocking_threads` floor). Foundation is settled: DEC-1 SEPARATE STORES +
> the xedge overlay; DEC-2 `Governs → Other("governs")`.
>
> **THE GOVERNING RULE (the gate enforces it).** v1's antagonist won by finding *designed-and-specified
> treated as already-solved.* v2 adopts Lane X v3's discipline verbatim: **every item is tagged
> PROVEN-IN-DESIGN (a fact already true in committed code, cited file:line, or a deterministic mechanism)
> vs BUILD-GATE (a runtime/empirical property the design does NOT claim to pass — it states the exact
> test + ceiling + falsifier, deferred to the build, sequenced before the work it protects).** Nothing
> below over-claims. The DoD table (§9) tags every row.

---

## 0 · TL;DR — lead with PROVEN vs GATED, then the riskiest assumption

**PROVEN-IN-DESIGN (facts already true in committed code, this design stands on them now):**
- The `SemanticSearch` advertise/dispatch bug is real and exact: `tools/list` advertises it at
  `wicked-estate-mcp/src/lib.rs:308`, but `handle_tools_call_ctx` resolves the call against
  `let tools = all_tools();` (`lib.rs:337`) which **excludes** `SemanticSearch` by construction
  (`all_tools` doc, `lib.rs:46`). A list-then-call agent gets `-32602 unknown tool`. (C-A2 read side / D2)
- `Annotation::new` does **NOT** enforce provenance/author — it sets `provenance: String::new()` and
  `author: String::new()` (`annotation.rs:191-192`); `with_provenance`/`with_author` are *optional*
  builders (`annotation.rs:215,221`). **v1's A3 was mis-grounded** ("the constructor already requires
  them"); the *tool* must enforce. (C-A1)
- The MCP serving path is read-only: the seam is `store.with_read(move |graph| Ok(handle_request_ctx(
  graph, …)))` (`wicked-estate-mcp/src/main.rs:316`); `with_read` runs the closure in `spawn_blocking`
  and is read-only (`pool.rs:75,83`); `handle_request_ctx`/`handle_tools_call_ctx` take `&dyn GraphRead`
  (`wicked-estate-mcp/src/lib.rs:323,427`). There is **no write seam on the async pool**. (C-A2)
- **The long-lived MCP `store` is a `SqlitePool` (READ pool), NOT a writer.** `main.rs` opens it via
  `open_async_store(&db_path)` → `SqlitePool` (`wicked-estate-mcp/src/main.rs:143`; def
  `wicked-estate/src/lib.rs:1228`). The `open_store_ext(&db_path)` at `main.rs:160` is opened **inside
  the `commits_behind` let-block, consumed by one `.and_then(|s| s.meta_get_key("indexed_root"))`, and
  DROPPED at end of scope (`main.rs:172`)** — a throwaway for a single meta read, NOT a retained writer.
  The `_sem_store_for_future_ctx = SqliteStore::open(&db_path).ok()` (`main.rs:175`) is a dead unused `_`
  binding. **So the MCP process holds NO retained `GraphStoreMutExt` writer** — the only write it does to
  `graph.db` today is `cache_put` (below). (C-A2 — the v1-class mis-cite this corrects; see §2.2)
- **The MCP process is ALREADY a writer to `graph.db` via the L2 cache.** `store` is a `#[derive(Clone)]`
  Arc-backed `SqlitePool` (`pool.rs:37`); `store.cache_put(&key,&raw).await` (`main.rs:372`) writes
  through a pooled connection via `block_in_place` on `&mut obj` (`pool.rs:63,69`), and `cache_put` is a
  real `INSERT … ON CONFLICT … DO UPDATE` (`sqlite.rs:458-466`). estate uses the SQLite default
  **rollback journal, not WAL**, so two concurrent writers to `graph.db` (the external `index` writer ⊕
  this cache writer) is already `SQLITE_BUSY` territory — `Annotate` does not *introduce* the
  second-writer hazard, it *inherits an already-live one*. (C-A2 / A-RISK-1)
- `Embedder` has `embed()` + `dim()` but **NO `id()`** (`wicked-estate-retrieve/src/lib.rs:1634,1636,1639`)
  — the `id()` hits elsewhere are `Resolver::id`, a different trait. Dim-equality alone cannot distinguish
  two 384-d embedders. (A-aB1)
- The real failure on a dim-mismatched store is **silent-EMPTY, not wrong-cosine**: `nearest` doc,
  verbatim — *"Symbols whose stored dimensionality does not match `query.len()` are silently skipped so
  a mixed-dimension store does not panic"* (`wicked-estate-store/src/sqlite.rs:712`; a test asserts it at
  `:2446`). **v1's A2 prose was inverted.** (A-aB1)
- `default_embedder()` (binary `wicked-estate/src/lib.rs:1147`) is tiered FastEmbed→model2vec→hash, but
  its `EMBED-FALLBACK:` markers go to `eprintln!` = **stderr the agent never sees**
  (`wicked-estate/src/lib.rs:1155,1167`; a third such marker in `FastEmbedder::embed`,
  `wicked-estate-retrieve/src/lib.rs:1784`). (A-aB2)
- `MutatingTool` does **not exist** anywhere in the tree (grep: zero hits) — its result→envelope mapping
  is genuinely undesigned (specified §2.2, but the MCP write tool is DEFERRED, not shipped in v1). (A-aB5)
- `intern` (`wicked-estate-store/src/sqlite.rs:176`, `INSERT … ON CONFLICT(sym) DO NOTHING`) is called at
  **FIVE** sites — `upsert_nodes_no_fts` (`:201`), `upsert_nodes` (`:1008`), `upsert_edges` endpoints
  (`:1100,1101`), unresolved-refs (`:1142`) — and is append-only, no generation, no delete. The two
  NODE-upsert loops (`:197` skip-FTS, `:1002` FTS) are **physically duplicated**. **The gen-bump must NOT
  live in `intern`** (edge/unresolved paths legitimately intern symbols with no live node → would bump
  spuriously) — it goes in a shared node-upsert helper. (xedge ADV-1 / DEC-X6 — see §5.1; the first draft's
  "put it in `intern`" was corrected by the re-gate)
- `Lineage` is a real `RetrievalTool` (`wicked-estate-retrieve/src/lib.rs:1018`) **absent from
  `all_tools()`** — a promotion, not a new build. (C-A3)
- `watch` re-indexes **per debounced batch**: 500 ms debouncer (`wicked-estate/src/main.rs:1211`), "On
  each debounced batch, `index_path` is called again" (`wicked-estate/src/main.rs:1181`). One event per
  batch = a process storm. (A-aB6)
- estate shells out to `git` via `std::process::Command` in 3 verified sites
  (`wicked-estate/src/lib.rs:189,1010`, `wicked-estate/src/main.rs:1844`) — subprocess is a blessed
  precedent. The bus regex is `/^wicked\.[a-z0-9_]+(\.[a-z0-9_]+)*$/` (`wicked-bus/lib/validate.js:9`).
  (D4 / A-aB3)
- `index_path` already writes meta at index time (`meta_set_key("indexed_root")`
  `wicked-estate/src/lib.rs:230`, `("indexed_version")` `:237`) — the embedder id/dim write lands at the
  same pattern. `GraphStoreMutExt::meta_set_key`/`meta_get_key` is the seam
  (`wicked-estate-store/src/lib.rs:903,905`). (A-aB1)

**BUILD-GATE (this design does NOT claim to pass; each row in §9 has its test + ceiling + falsifier):**
the provenance/cap enforcement actually firing (DoD-A1/A5); the write-seam serving stale-free reads
under an in-process write (DoD-A2); the dim-guard converting silent-empty→honest-absence with a real
`Embedder::id()` + fail-closed-on-`meta==None` (DoD-A6a); SemanticSearch dispatch fixed + routed through
the real embedder + per-call MCP **diagnostics** (DoD-A6b); the dead-letter emit (DoD-A3); the watch
coalescing (DoD-A6); `symbols.gen`'s 0→1 bump firing at the SHARED seam + `symbol_epoch` returning a
**NON-ZERO** post-reuse gen (DoD-XA4 — Lane X's DoD-X4 dependency); `traverse_multi` correctness +
**sub-linear-in-W** query count (DoD-XA2/XA2b); `with_read_inline` + bounded blocking threads under
saturation (DoD-XA1b).

**The riskiest assumption (attack this first): A-RISK-1 — that an MCP-side `Annotate` write to `graph.db`
can serve coherent reads WITHOUT compounding the already-live second-writer hazard into `SQLITE_BUSY` /
corruption on a non-WAL file.** This is C-A2's core and it is genuinely hard, and HARDER than v1 framed it:
the MCP process holds NO retained writer (the `open_store_ext` handle is a throwaway, `main.rs:160→172`,
corrected above) — its long-lived `store` is a read `SqlitePool` (`main.rs:143`) that ALREADY writes
`graph.db` via `cache_put` (`main.rs:372`, `pool.rs:63,69`) concurrently with the external indexer, on the
default rollback journal. Adding an `Annotate` writer makes it worse, not new. **DECISION (corrected — see
§2.2): `Annotate` is CLI-only in v1** (it routes through the binary's existing `annotate` arm,
`wicked-estate/src/main.rs:1581`) **+ the MCP emits the `wicked.estate.annotated` doorbell**; the MCP
write tool is **NOT shipped in v1**. A real MCP write seam (a retained single-writer actor that ALSO owns
the cache writes, retiring `cache_put`-on-pool) is **designed-but-deferred** behind DoD-A2. This inverts
v1's "thread the handle we already hold" — there is no such handle, and pretending there was is exactly
the over-claim the gate exists to catch.

---

## 1 · Scope reconciliation — what A owns after the gate

Two work-streams, one lane:

**(I) Knowledge-capability program (v1 scope, hardened):** 3 thin tools (`RankHotspots`,
`Communities`, `Annotate`) + `Lineage` promotion + fix SemanticSearch + 3 `skill://` skills
(`power-moves`/`change-impact`/`rationale-archaeology`) + 3 coarse events
`wicked.estate.{indexed,drifted,annotated}` on the shared dead-letter emit seam.

**(II) xedge estate-side foundation (A now owns, from Lane X v3 PASS):** the three estate spine changes
Lane X depends on but does NOT make itself — `symbols.gen` + `GraphRead::symbol_epoch()`;
`traverse_multi` (the **22nd** `GraphRead` method — see §5.2; Lane X v3's "28th" is a miscount to
correct) + SqliteStore specialization; `with_read_inline` on `AsyncGraphStore` + an explicit
`max_blocking_threads` floor. These are **estate spine changes**
(`wicked-estate-core` traits + `wicked-estate-store`), sequenced BEFORE Lane X fans out (DEC-X6-SEQ,
DEC-X2). They land in estate's conformance kit; Lane X consumes them.

**Sequencing across the two streams (hard):** stream (II)'s `symbols.gen`/`symbol_epoch` is a
**DEC-X6-SEQ gate** — it must be green in estate conformance BEFORE Lane X's about-arm is claimed
reuse-safe. Stream (I) has no dependency on (II) and can land in parallel; the only ordering inside (I)
is that the embedder dim-guard (§3) precedes advertising a fixed SemanticSearch.

---

## 2 · The mutating path: `Annotate`, the write seam, and cache coherence (C-A1 + C-A2 + A-aB4 + A-aB5)

### 2.1 `Annotate` — provenance + forced author enforced **by a shared helper, not the constructor** (C-A1)

- **DECISION.** Define the `Annotate` operation with its honesty invariants in a shared
  `enforce_annotation(...)` helper (called by the CLI arm in v1, §2.2, and any future MCP write arm).
  **The helper — not the `Annotation` constructor — enforces honesty**, because `Annotation::new`
  defaults `provenance`/`author` to empty (`annotation.rs:191-192`, PROVEN; v1's A3 claimed the
  constructor enforces — it does not). The helper:
  1. **Rejects empty/whitespace provenance** (CLI: non-zero exit + stderr; MCP-deferred: `isError`
     `-32602`) — provenance is REQUIRED, no default-to-`"mcp-agent"` (v1 defaulted it; that laundered
     provenance). The caller must state where the annotation came from (a skill name, a tool chain, a
     human handle).
  2. **Forces `author = "mcp-agent"`** (or the CLI's resolved author) server-side, overwriting any client
     value — an agent cannot impersonate `system`-derived authorship (`is_system_derived`,
     `annotation.rs:84,267`).
  3. Builds via `Annotation::new(...).with_provenance(p).with_author(a)` so the persisted row always
     carries both.
- **Input schema** (the contract for both the CLI flags and the deferred MCP tool; `op:"requirement"`
  stamps a *plain requirement string only* — see A-aB4):
  ```json
  { "type": "object",
    "required": ["symbol", "key", "value", "provenance"],
    "properties": {
      "symbol":     { "type": "string", "description": "Stable SymbolId. INSERT-only; no-op if no live node (cannot create orphans)." },
      "type":       { "type": "string", "default": "note" },
      "key":        { "type": "string", "maxLength": 256 },
      "value":      { "type": "string", "maxLength": 8000 },
      "confidence": { "type": "number", "default": 1.0, "minimum": 0, "maximum": 1 },
      "provenance": { "type": "string", "minLength": 1, "description": "REQUIRED. Origin of the claim. Empty → isError." },
      "op":         { "type": "string", "enum": ["annotate","requirement"], "default": "annotate" } },
    "additionalProperties": false }
  ```
- **Semantics (grounded `traits.rs:180`).** `annotate` is a bare INSERT, not upsert; stable-SymbolId
  keyed (survives renames, ADR-002); no `delete` (an append-only caller cannot corrupt history;
  `delete_annotations` `traits.rs:185` stays a separate maintenance op). **PROVEN-IN-DESIGN:** the schema
  + the enforcement *design* + the empty-default constructor it compensates for (`annotation.rs:191-192`).
  **BUILD-GATE (DoD-A1):** the helper actually rejecting empty provenance + forcing author + no-op on
  absent symbol, tested at the CLI entry point in v1.

### 2.2 The write path: CLI-only in v1; the MCP write seam is designed-but-DEFERRED (C-A2 — the keystone, CORRECTED post-re-gate)

**The problem, exact and grounded — CORRECTED (the re-gate falsified v1's premise AND v2's first draft).**
The MCP handler is read-only by type (`&dyn GraphRead`, `wicked-estate-mcp/src/lib.rs:323,427`); the async
pool exposes `with_read` only (`pool.rs:75`), running closures in `spawn_blocking` (`pool.rs:83`).
`Annotate` needs a `&mut dyn GraphWrite` / the `GraphStoreMutExt` (`store/src/lib.rs:899`).

> **RETRACTION (the v1-class over-claim this fixes).** v1 — and this doc's first draft — said "the MCP
> `main.rs` already holds the writable `Box<dyn GraphStoreMutExt>` from `open_store_ext(&db_path)`
> (`main.rs:160`); thread that handle." **That handle does NOT exist as a retained writer.** Re-verified:
> `open_store_ext(&db_path)` at `wicked-estate-mcp/src/main.rs:160` is opened INSIDE the `commits_behind`
> let-block, consumed by one `s.meta_get_key("indexed_root")`, and DROPPED at `main.rs:172`. The
> long-lived `store` is `open_async_store(&db_path)` → `SqlitePool` (`main.rs:143`; def
> `wicked-estate/src/lib.rs:1228`) — a READ pool exposing only `with_read` + `cache_*`. **The MCP process
> holds NO retained `GraphStoreMutExt` writer.** Threading "the handle we already hold" was the exact
> "designed-and-specified treated as already-solved" bug the gate exists to catch.

**And the second-writer hazard is ALREADY LIVE.** `store` is a `#[derive(Clone)]` Arc-backed `SqlitePool`
(`pool.rs:37`); `store.cache_put(&key,&raw).await` (`main.rs:372`) writes `graph.db` through a pooled
connection via `block_in_place` on `&mut obj` (`pool.rs:63,69`), and `cache_put` is a real
`INSERT … ON CONFLICT … DO UPDATE` (`sqlite.rs:458-466`). estate uses the SQLite default **rollback
journal, not WAL**. So the MCP serving process is **already a concurrent writer** to `graph.db` (the L2
response cache) alongside any external `wicked-estate index` writer — `Annotate` does not *introduce* a
second writer, it would *compound an already-live one* on a journal mode that does not make concurrent
writers safe.

**DECISION (CORRECTED) — `Annotate` is CLI-only in v1; the MCP write seam is DESIGNED but DEFERRED.**

1. **v1 write path = the CLI, with enforcement in a SHARED helper.** The write executes through the
   binary's existing `annotate` arm (`wicked-estate/src/main.rs:1581`; `clusters --annotate` at `:1360`),
   which opens a real `GraphStoreMutExt` writer and runs single-writer with the indexer by operational
   discipline (one process at a time, not two concurrent writers). **The C-A1 honesty invariants
   (reject-empty-provenance, force `author="mcp-agent"`, force `requirement_validated=false`, truncate
   8000/256) live in a shared `enforce_annotation(...)` helper** called by the CLI arm now and by any
   future MCP arm — so enforcement is identical regardless of entry point and does not depend on shipping
   the MCP writer.
2. **The MCP surface stays read-only + emits the doorbell.** After a successful CLI `annotate`,
   `wicked.estate.annotated` fires (§4.2). `power-moves` (§4.1) teaches an agent to drive the CLI for
   writes in v1. This keeps the `GraphRead` trait pure (`traits.rs:74` "cannot accidentally mutate") and
   adds **zero** new writer to `graph.db`.
3. **MCP write seam = DESIGNED, DEFERRED behind DoD-A2.** Shipping an MCP write tool requires a **retained
   single-writer actor** in the MCP process that ALSO owns the cache writes — i.e. **retire
   `cache_put`-on-pool** and route BOTH the L2 cache AND `Annotate` through one serialized writer task (a
   `tokio::sync::mpsc` to a single owning `GraphStoreMutExt`). That is the only shape that yields one
   writer to a non-WAL `graph.db`. It is a `MutatingTool` + a write-actor + a cache re-plumb, gated on
   DoD-A2 (below). The design does NOT claim it for v1.
4. **Delete the dead handle.** `_sem_store_for_future_ctx` (`main.rs:175`, PROVEN dead) is wired to the
   semantic path (§3) or deleted in-change (CLAUDE.md §8). The scoped meta-read `open_store_ext`
   (`main.rs:160→172`) is harmless (opened+dropped) but must never be mistaken for a writer again.

- **`MutatingTool` result→envelope (A-aB5) — specified for the deferred seam, reused by the CLI now.**
  `MutatingTool` is net-new (PROVEN: zero hits tree-wide). Define
  `fn invoke(&self, store: &mut dyn GraphWrite, args: &Value) -> MutatingResult`; `MutatingResult` maps to
  the MCP `tools/call` envelope exactly like `RetrievalTool` — `{ content:[{type:"text",text}], isError,
  diagnostics:[…] }`: success → `isError:false` + coarse `{symbol,type,key,written:1}`; rejected
  provenance / cap breach / absent symbol → `isError:true` + loud diagnostic (R1 — `isError`→session
  abandonment, so the envelope must be honest). The same enforcement+result code backs the CLI arm's exit
  code/stderr in v1.
- **A-aB4 — `requirement_validated=false` server-side.** `op:"requirement"` routes `value` →
  `set_node_semantics(requirement=value)` (`traits.rs:169`, which carries a real
  `requirement_validated: Option<bool>` param) so the acceptance outcome ("annotate writes a typed
  annotation retrievable via `find_by_requirement`", `traits.rs:116`) is reachable. **The shared helper
  forces that flag `false`** — an agent stamping a requirement cannot assert it was *validated*
  (provenance-laundering). The client value is never honored.
- **A-aB5 caps in RUST, not JSON-schema.** The `maxLength` (256 key / 8000 value) is ADVISORY to the
  client — UNENFORCED on a non-conformant caller. The helper **truncates in Rust** at 8000/256 +
  `ANNOTATE-TRUNCATED: value clamped 8000←N`. A server invariant, not a hint.

- **Tags.** The RETRACTION + the already-live-cache-writer + the CLI-only-in-v1 decision + the
  deferred-actor spec + the `MutatingTool` envelope = **PROVEN-IN-DESIGN** (they rest on cited code: the
  dropped handle `main.rs:160→172`, the read pool `main.rs:143`, the live `cache_put` writer `main.rs:372`
  / `pool.rs:69` / `sqlite.rs:458-466`, the non-WAL journal, the empty-default constructor). **The MCP
  write seam serving stale-free reads with NO net-new writer (the actor + cache re-plumb) = BUILD-GATE
  (DoD-A2)** — explicitly NOT claimed for v1; the test is a concurrent indexer-write ⊕ actor-write ⊕
  serving-read → consistent read, ZERO `SQLITE_BUSY`, AND `cache_put`-on-pool retired (no second writer
  remains). v1 ships CLI-only and is not blocked on it.

### 2.3 Tool count reconciliation (C-A3) — read-only MCP surface in v1; write is CLI

- **The v1 MCP surface is READ-ONLY** (the corrected C-A2 ships no MCP mutating tool — write is the CLI,
  §2.2). v2 **folds the `Lineage` promotion** (PROVEN: exists at `retrieve/src/lib.rs:1018`, absent from
  `all_tools()`): the `change-impact` skill (§4.2) calls `Lineage` over MCP, so it must be in
  `all_tools()`. Net read surface: **7 → 10 unconditional read tools** (+`RankHotspots`, +`Communities`,
  +`Lineage` promoted) **+ a conditionally-present `SemanticSearch`** (fixed callable, §3). `Annotate` is
  **NOT a shipped MCP tool in v1** — its schema (§2.1) is the contract for the CLI flags now and the
  deferred MCP `MutatingTool` later. "+2 net-new read tools + 1 promotion + 1 fix" — thinner than v1's
  "+3 +1," because the mutating tool is deferred.
- **`RankHotspots`/`Communities` are net-new builds** (grep: zero hits) — the "10 unconditional" floor is
  contingent on building them (correctly BUILD-GATE in §9, not a current fact). `Lineage` is a real
  promotion (exists, just absent from `all_tools()`).
- **ADV (non-deterministic count, bounded).** `SemanticSearch` appears only with a real store +
  matching-dim+id embedder (§3); the other three are unconditional. So `tools/list` returns **10 or 11**
  read tools by semantic availability. The conformance tests (C-A4, §6) assert the **unconditional 10** as
  a floor + `SemanticSearch` present iff `has_semantic_search && meta_id/dim_ok`. (The v1-gate ADV "7–11"
  is bounded to "10 unconditional, +1 semantic"; no mutating tool inflates it.)

---

## 3 · The semantic path: dim-guard + dispatch fix + MCP diagnostics (A-aB1 + A-aB2)

### 3.1 A-aB1 — the dim-guard, rebuilt on a real `Embedder::id()` + fail-closed

v1's A2 mitigation read a meta key **nothing writes** and inverted the failure mode. Rebuilt:

1. **Add `Embedder::id() -> &str`** to the trait (`wicked-estate-retrieve/src/lib.rs:1634`, PROVEN: only
   `embed`@1636 + `dim`@1639 today — no `id`; the `id()` hits elsewhere are `Resolver::id`, a different
   trait). Each impl returns a stable identity (`"fastembed:bge-small-en-v1.5"`, `"model2vec:…"`,
   `"hash:v1"`). **Dim-equality is insufficient** — two distinct 384-d models produce incomparable
   vectors; identity, not dim, is the correctness key.
2. **Write embedder id + dim to `meta` at index time.** `compute_embeddings` (the `index --embeddings`
   path, `wicked-estate/src/lib.rs:1192`) writes `meta["embedder_id"] = embedder.id()` and
   `meta["embedder_dim"] = dim.to_string()` via `meta_set_key` (`wicked-estate-store/src/lib.rs:903`) — at
   the SAME pattern `index_path` already uses to write `indexed_root`/`indexed_version`
   (`wicked-estate/src/lib.rs:230,237`, PROVEN this exists).
3. **Fail closed on `meta == None`.** At MCP start, read `meta["embedder_id"]`/`["embedder_dim"]`. **If
   ABSENT (None) → do NOT advertise SemanticSearch** and emit a loud diagnostic (`EMBED-META-MISSING:
   store predates embedder tagging; semantic disabled, re-index with --embeddings`). The failure is
   silent-EMPTY (`nearest` silently skips mismatched rows, `wicked-estate-store/src/sqlite.rs:712`, PROVEN), so a missing-meta
   store would otherwise return quietly-degraded results — fail-closed converts that to honest absence.
4. **Mismatch guard.** If `meta["embedder_id"] != default_embedder().id()` (or dims differ) → do NOT
   advertise + `EMBED-MISMATCH: store=<id>/<dim>, runtime=<id>/<dim>; re-index`.

- **Tag.** The trait addition + meta-write site + the fail-closed-on-None *rule* = **PROVEN-IN-DESIGN**
  (rests on the existing meta seam + the verbatim `nearest` skip behavior). The guard actually firing
  (absent-meta → not advertised; mismatch → not advertised; matched → advertised) = **BUILD-GATE
  (DoD-A6a)**, with the falsifier: a hash-embedded store queried by a 384-d runtime returns *empty*
  semantic results AND the tool is *not advertised* (not "advertised but silently empty").

### 3.2 A-aB2 — fix dispatch + route through the real embedder + per-call MCP diagnostics

1. **Dispatch fix (PROVEN bug).** `handle_tools_call_ctx` resolves against `let tools = all_tools();`
   (`wicked-estate-mcp/src/lib.rs:337`) which excludes `SemanticSearch` (`all_tools` doc, `:46`). The
   registry that DOES include it, `all_tools_with_semantic` (`wicked-estate-mcp/src/lib.rs:62`), is **never
   called by the serving loop** — there is no live SemanticSearch dispatch path at all (that IS the bug).
   **FIX:** dispatch resolves against a registry that includes the **live** `SemanticSearch` instance
   when present (built with the real store + embedder), i.e. wire `all_tools_with_semantic` (rebuilt per
   #2) into `handle_tools_call_ctx`. A list-then-call agent must reach it.
2. **Route through the REAL tiered embedder, NOT a re-introduced hash fallback.** `all_tools_with_semantic`
   hardcodes `SemanticSearch::with_hash_embedder(vec_store)` (`wicked-estate-mcp/src/lib.rs:73`), and the
   advertise *description* does the same (`:309`) — PROVEN hardwired hash. v2 rebuilds it with
   `SemanticSearch::new(default_embedder(), vec_store)` (`default_embedder()`
   `wicked-estate/src/lib.rs:1147`, the tiered FastEmbed→model2vec→hash selector). The hardcoded
   `with_hash_embedder` (both `:73` and the `:309` description hack) is **deleted/rewired** (§8) — not a
   "default" in the sense of a live path (there is none), but the only place hash is wired. The
   `_sem_store_for_future_ctx` (`main.rs:175`) is the vector store wired in (or deleted, §2.2).
3. **Per-call MCP DIAGNOSTICS, not stderr (A-aB2 core).** `default_embedder()`'s `EMBED-FALLBACK:`
   markers go to `eprintln!` (`wicked-estate/src/lib.rs:1155,1167`, PROVEN = stderr the agent never sees;
   `FastEmbedder::embed` has a third such `eprintln!` at `wicked-estate-retrieve/src/lib.rs:1784`). **FIX:**
   when
   the active embedder is the hash fallback, the tool rides a **per-call diagnostic in the MCP `tools/call`
   response** (`LEXICAL-FALLBACK: no semantic model loaded; results are lexical`), surfaced in the
   `diagnostics` array the agent actually reads — R6 (loud marker) applied to the embedder tier. The
   self-label has a real impl surface (the response envelope), not a description string.
4. **Lexical floor untouched (C4).** `SearchEntity` FTS5/BM25 stays the always-on, model-free floor;
   semantic is additive and self-labeling when degraded.

- **Tag.** The dispatch fix + real-embedder routing + the diagnostic *channel* = **PROVEN-IN-DESIGN**
  (the bug and the stderr sink are cited). The fixed path actually dispatching + the diagnostic appearing
  per-call = **BUILD-GATE (DoD-A6b)**: list→call reaches SemanticSearch; a hash-fallback run carries
  `LEXICAL-FALLBACK` in the response `diagnostics` (falsifier: marker only on stderr → fail).

### 3.3 ADV — interrupted `compute_embeddings` → mixed-dim store

An interrupted `index --embeddings` can leave a store partially embedded (some rows old-dim, some new).
The meta write (§3.1) is the guard: **write `meta["embedder_id"]/["embedder_dim"] LAST**, after all
vectors are persisted, so a crash mid-embed leaves `meta` reflecting the PRIOR complete state (or None) →
fail-closed. Re-index overwrites. (DoD-A6a covers the None branch; the LAST-write ordering is the
PROVEN-IN-DESIGN mechanism, the crash-safety is BUILD-GATE-adjacent — a test that truncates an embed run
and asserts no mixed-dim serving.)

---

## 4 · Skills (3) + events (3) — folded with the dead-letter rule

### 4.1 The 3 `skill://` skills (PROVEN mechanism: memory-mcp bundling)

Replicate memory-mcp's `const SKILLS: &[(&str,&str,&str)]` with `include_str!` (PROVEN pattern,
`wicked-memory/crates/wicked-memory-mcp/src/lib.rs:42,151,160`); add `"resources":{}` to `initialize`
caps (`wicked-estate-mcp/src/lib.rs:284` currently `{"tools":{}}`); add `resources/list` + `resources/read`
arms to the `match method {` in `handle_request_ctx` (`wicked-estate-mcp/src/lib.rs:433`, which today has
no `resources/*` arm). Three disjoint-method skills:
- **`power-moves`** — breadth: the 10+conditional-1 read-tool map + the **CLI `annotate` for writes** +
  power combos + the C4 reading rules (read `STALENESS:`; treat low-confidence as heuristic;
  `LEXICAL-FALLBACK`/`EMBED-MISMATCH`/`EMBED-META-MISSING` → fall back to `SearchEntity`).
- **`change-impact`** — forward/backward dependency reasoning: resolve → `BlastRadius` + `Lineage`
  (now MCP-reachable, §2.3) → read `unresolved_callers` honestly (`retrieve/src/lib.rs:753,761,790`) →
  governing rules via `TraverseGraph edge_kinds=["governs"]` (DEC-2: `Governs` is native at `edge.rs:107`
  but the disjoint-C5 knowledge edges go to `Other("governs")` `edge.rs:114` per the overlay — so a
  `governs` traverse no longer collides knowledge edges into estate's edge space) → triage with
  `RankHotspots` seeded.
- **`rationale-archaeology`** — temporal/provenance reasoning: read annotations inline on `RetrieveEntity`
  (`retrieve/src/lib.rs:562`) → `edge_history` (CLI for v1, OQ-A2) → git provenance (`repo_info`
  `traits.rs:100`) → capture findings back via the **CLI `annotate`** (§2.2; confidence < 1.0, provenance
  = `"rationale-archaeology"`) — MCP write is deferred, so the loop closes through the CLI in v1.

Each skill referenced by a `resources` test (mirror `wicked-memory/.../lib.rs:437`) — no orphan ships
(CLAUDE.md §5). **Tag:** bundling = PROVEN-IN-DESIGN (verbatim reference impl); the resources arms +
test green = BUILD-GATE (DoD-A7).

### 4.2 Events — coarse, `wicked.<noun>.<past-verb>`, with the dead-letter rule (A-aB3 + A-aB6)

Three events mapped to the pinned event-catalog contract
(`wicked-memory/docs/recon/event-catalog-contract.md`):

| Logical | Bus `event_type` | Fired at (PROVEN site) | Payload (coarse — counts/ids, never per-symbol) |
|---|---|---|---|
| indexed | `wicked.estate.indexed` | end of `"index"` arm (`wicked-estate/src/main.rs:604`) + watch (coalesced, below) | `{root, counts{files,nodes,edges}, embeddings:bool, commit, db_path, ts}` |
| drifted | `wicked.estate.drifted` | end `"drift"` arm, non-empty only (`estate_drift` `lib.rs:917`, `DriftReport` `lib.rs:895`) | `{db_path, commits_behind, ts}` |
| annotated | `wicked.estate.annotated` | after a successful CLI `annotate` (`wicked-estate/src/main.rs:1581`) / `clusters --annotate` (`:1360`) — ONE event with `count` for the clusters case, never per-item | `{symbol, key, ts}` |

Names match the contract exactly (regex `/^wicked\.[a-z0-9_]+…/` PROVEN `validate.js:9`). Consumers
**trigger then re-query** (payload is counts, not symbols) — per the contract.

**A-aB3 — the dead-letter emit seam (replaces v1's silent fire-and-forget).** v1's detached
`std::process::Command::new("npx")` spawn drops events **silently** on any post-spawn failure (no
`.wait()`, WB-002/004 → unread stderr, no spool). v2 adopts the contract's seam:
- **50 ms budget, OFF-THREAD outcome read.** Spawn `wicked-bus emit` (NOT `npx` — resolve the bin
  directly, `WICKED_BUS_BIN` override else `wicked-bus`) on a detached thread that reads the child outcome
  with a bounded ~50 ms wait. The index hot path does NOT block on it (fire-and-forget *to the caller*),
  but the OUTCOME is observed off-thread.
- **NDJSON dead-letter on drop.** On spawn-Err or non-zero exit, append the event to a dead-letter spool
  (`~/.something-wicked/wicked-estate/emit-deadletter.ndjson`) and log loudly — **a dropped event is
  logged + spooled, NEVER silent** (the contract's rule, vs v1's `eprintln!`-and-forget). A bus-side
  drainer (or the next emit) can replay the spool.
- **Drop `npx`** (PROVEN precedent: the `git` subprocess sites use the bin directly, `lib.rs:189` etc.).

**A-aB6 — watch coalescing (replaces "one event per run" which is FALSE for watch).** `watch` re-indexes
per debounced batch (`main.rs:1181,1211`, PROVEN), so per-batch emit = process-storm + WAL contention.
**FIX:** the watch loop **coalesces** emits — a single trailing `wicked.estate.indexed` per *quiet
period* (debounce-the-emit, not just the index), with merged counts, so a burst of saves produces ONE
doorbell. Idempotency key `wicked-estate:<event_type>:<commit-or-db-mtime>:<coalesce-window>` so a
retried window doesn't double-fire.

- **Tag.** The seam shape + names + coalescing *design* = PROVEN-IN-DESIGN (rests on the contract, the
  bus regex, the debouncer site). The dead-letter actually catching a drop + the watch emitting once per
  burst = BUILD-GATE (DoD-A3 / DoD-A6).

---

## 5 · xedge estate-side foundation — the work A now owns (Lane X v3 PASS)

These are estate SPINE changes (`wicked-estate-core` + `wicked-estate-store`). They land in estate's
conformance kit; Lane X consumes them. Tagged against Lane X's own DoD ids (DoD-X2/X2b/X4/X1b) since A
builds them and X gates on them.

### 5.1 `symbols.gen` (0→1 bump in the NODE-upsert loops, covering both FTS variants) + `GraphRead::symbol_epoch()` (ADV-1 / DEC-X6)

**Grounded.** `SymbolId` is a pure name-path (`symbol.rs`), intern is append-only with no generation and
no delete (`sqlite.rs:176`, `INSERT … ON CONFLICT(sym) DO NOTHING`). Delete-then-re-add reuses the SAME
`SymbolId` → an old xedge row resolves to a LIVE-WRONG node (silent, violates R7). **`remove_file` deletes
the `nodes`/`edges`/`embeddings` rows but NOT the `symbols` (intern) row** (`sqlite.rs:1161+`) — so the
"sym survives, live node gone" premise the bump keys on is sound.

> **CORRECTION (the re-gate falsified v2's first-draft placement).** The first draft said "the bump lives
> in `intern` (`sqlite.rs:176`), the shared point both upsert paths funnel through." **That is UNSOUND and
> RETRACTED.** `self.intern(` is called at **five** sites, not two: `upsert_nodes_no_fts` (`sqlite.rs:201`),
> `upsert_nodes` (`:1008`), **`upsert_edges` endpoints (`:1100,:1101`)**, and **unresolved-refs (`:1142`)**.
> A "no live node → `gen += 1`" check inside `intern` would fire on **every edge insertion to a
> not-yet-defined target** — forward references / legitimately-dangling edges have an interned `sym` and
> no live `nodes` row — bumping `gen` spuriously and making the epoch meaningless (every cross-edge to a
> forward-declared symbol would look "reused"). The bump therefore CANNOT live in `intern`.

**DECISION (concrete bump, in the NODE-upsert path only — the duplication closed deliberately).**
1. Add `gen INTEGER NOT NULL DEFAULT 0` to `symbols`.
2. **The 0→1 live-node-transition bump lives in a shared NODE-upsert helper, NOT in `intern`.** The two
   node-upsert entry points — `upsert_nodes` (`sqlite.rs:1002`, FTS path) and `upsert_nodes_no_fts`
   (`sqlite.rs:197`, which `upsert_nodes_skip_fts` delegates to, `lib.rs:963→967`) — run the identical
   intern-then-insert preamble but are **physically duplicated** (the exact ADV-1 duplication hazard).
   **Fold the bump into ONE shared `fn upsert_nodes_inner(&mut self, nodes, with_fts: bool)` that BOTH
   public methods call**, so the bump logic is written once and covers BOTH the FTS and skip-FTS reindex
   paths. In that helper, per node sid: check whether a LIVE `nodes` row exists for the sid; if NOT (a
   re-add after `remove_file` left `sym` but no `nodes` row) → `gen += 1` before the node insert. A
   first-ever node leaves `gen=0`; a reuse-after-removal yields `gen ≥ 1`. **It is keyed on NODE insertion,
   so an edge to a forward-declared symbol (the `intern`-only path) never bumps.** **Putting it only on
   `upsert_nodes` (the FTS path) would ship reuse-detection INERT on the reindex hot path** — the
   watch/`index_path` path heavily uses the skip-FTS variant — ADV-1's exact warning; folding both into
   `upsert_nodes_inner` closes that.
3. **Expose `GraphRead::symbol_epoch(&SymbolId) -> Result<Option<u64>>`** (the read trait `traits.rs:74`) —
   current `gen` for a live symbol, `None` if no live node. This is THE Lane A cross-lane deliverable
   (Lane X's CROSS-LANE CALLOUT #1).
4. **TOCTOU note (X owns the close, A provides the read).** The put-time TOCTOU close lives inside xedge's
   single-writer txn (DEC-X6.4, Lane X) — A's job is only the accurate `gen` bump + `symbol_epoch` read.

- **Tag.** Schema + the bump *design* (in `upsert_nodes_inner`, NOT `intern`) + the 5-vs-2 intern-call
  grounding = PROVEN-IN-DESIGN (the five `intern` sites are cited; the node-vs-edge distinction is the
  load-bearing correctness point). **The bump FIRING on BOTH FTS variants + `symbol_epoch` returning
  NON-ZERO post-reuse = BUILD-GATE (DoD-XA4 = Lane X's DoD-X4 dependency)**, driven NON-vacuously through
  `index_path`/`remove_file`+reindex VIA the skip-FTS path (NOT `Some(0)`): delete a symbol, re-add same
  name → `symbol_epoch(id)` returns `Some(g)`, g ≥ 1; AND a forward-referenced (edge-only) symbol that
  later gets a node for the first time stays `gen=0` (no spurious bump). Falsifier: g == 0 after reuse
  (bump didn't fire on the skip-FTS path), OR g ≥ 1 for a first-time node that was previously only an edge
  target (spurious bump — the `intern`-placement bug). **DEC-X6-SEQ:** GREEN in estate conformance BEFORE
  Lane X claims the about-arm reuse-safe.

### 5.2 `traverse_multi` — the 22nd `GraphRead` method + SqliteStore specialization (SAME change) + DoD-X2b (DEC-X2)

**Grounded.** `GraphRead` is sync with **21 methods** (counted at `traits.rs:74-135`: capabilities,
get_node, find_symbols, neighbors, traverse, all_nodes, all_edges, unresolved_refs_for_name, file_digest,
file_git_sha, repo_info, edge_history, file_content, symbol_source, changes_since, node_semantics,
find_by_requirement, annotations, annotations_by_type, annotations_stale_since, stats), single-start
`traverse(&self, start, spec)` (`traits.rs:84`); `traverse` induces edges via `neighbors` per anchor
(`sqlite.rs:1680-1686` per Lane X v3 cite), so an N-wide frontier via single-start is O(frontier) CTEs.

> **CORRECTION (propagate upstream — CLAUDE.md §11).** Lane X v3 says "27 sync methods" / "28-method
> delegation" (`design-lane-X-overlay-v3.md:51`, DEC-X7/DoD-X8). **That count is wrong** — `GraphRead`
> has 21 methods, so `traverse_multi` is the **22nd**, and the `OverlayReader` delegation table is **22
> methods, not 28**. The decision is unchanged (add the method + specialize SQLite), but the number must
> be corrected here AND fed back to Lane X v3 (DoD-X8's "28-method delegation" → 22) — same fleet-scar
> the guardrails warn about: a count copied across lanes without re-counting.

**DECISION (A builds it; X consumes it).**
1. Add `traverse_multi(&self, starts: &[SymbolId], spec: &TraversalSpec) -> Result<Subgraph>` as the
   **22nd** `GraphRead` method, with a **default fold** over `traverse` (existing backends stay CORRECT
   until they specialize).
2. **SqliteStore specializes in the SAME change** (CLAUDE.md §1 spine-before-fan-out + §8): seed the
   recursive CTE base case with ALL `starts`' sids (`SELECT sid FROM symbols WHERE sym IN (…)` then UNION
   the walk, mirroring `cte_reach`) — ONE `WITH RECURSIVE` per ply for the whole frontier; edge induction
   batches to one `SELECT … WHERE source IN (…)`. Shipping the method with only the slow default fold is
   the #3 hole.
3. Conformance kit gains `traverse_multi_matches_union_of_traverse` (equality, all backends).

- **Tag.** Method + default + specialization *design* = PROVEN-IN-DESIGN (the per-anchor induction and
  `cte_reach` are cited). **Correctness (DoD-XA2 = X's DoD-X2)** and **NOT-N+1 (DoD-XA2b = X's DoD-X2b):
  SqliteStore `traverse_multi(W starts, 1 ply)` issues query-count ≤ C (small fixed C, e.g. ≤ 3) for ALL
  W in a sweep**, counted via a SQLite trace/counting-connection wrapper = BUILD-GATE. Falsifier:
  query-count scales with W (the slow fold leaked into SQLite). Equality-only conformance is GREEN for the
  slow fold, so the perf assertion is mandatory.

### 5.3 `with_read_inline` on `AsyncGraphStore` + explicit `max_blocking_threads` floor (ADV-3 / DEC-X1b)

**Grounded.** The only `impl AsyncGraphStore` is `SqlitePool` (`pool.rs:74`); `with_read` runs the
closure in `spawn_blocking` (`pool.rs:83`). Lane X's `OverlayReader`, already on a `spawn_blocking`
thread, calling a foreign pool's `with_read` would nest `spawn_blocking` (1 + k blocking threads held
while parked → starvation deadlock shape).

**DECISION (A provides the inline sibling; X uses it).**
1. Add `with_read_inline<F,T>(&self, f: F) -> Result<T>` to `AsyncGraphStore` (`traits.rs` /
   `pool.rs:74`) — a sibling of `with_read` that, for callers ALREADY on a blocking-pool thread, checks
   out a connection (`get().await`, the only await) and runs `f` on the CURRENT thread WITHOUT
   re-entering `spawn_blocking`. Net blocking-pool occupancy per cross-recall drops from `1 + k` to `1`.
   Precedent for RAII soundness: `cache_get`/`cache_put` already use `block_in_place` on the pool
   (`pool.rs:59,69`, PROVEN).
2. **Floor `max_blocking_threads` explicitly.** The MCP runtime sets `max_blocking_threads ≥
   expected_peak_concurrent_recalls + headroom`, documented as an operational bound (OQ-X10). Each
   in-flight cross-recall parks exactly one blocking thread on `block_on`; the floor must exceed peak
   in-flight recalls so estate's own `with_read` pool is never fully occupied by parked overlay threads.
   **ADV-2 (X owns):** floor the FOREIGN pools' `max_size` too — A's job is the estate-side
   `with_read_inline` + the `max_blocking_threads` floor on the estate MCP runtime.

- **Tag.** `with_read_inline` *design* + the RAII soundness argument = PROVEN-IN-DESIGN (deadpool RAII +
  the cited `block_in_place` precedent; the panic is disproven by Lane X v3's tokio 1.52.3
  `worker.rs:432-436` trace — A inherits that, does not re-derive it). **No-deadlock + bounded threads
  under saturation = BUILD-GATE (DoD-XA1b = X's DoD-X1b):** N concurrent cross-recalls (N ≥
  2×`max_blocking_threads`, cap small e.g. 8) → zero deadlocks/timeouts AND blocking-thread high-water
  mark ≤ `max_blocking_threads` DURING saturation (assert via `tokio-metrics`). ADV-3 verbatim: assert
  the high-water DURING saturation, not at rest.

---

## 6 · C-A4 — retire-as-you-go test updates (the named conformance tests)

Every contract change updates its conformance test IN THE SAME CHANGE (CLAUDE.md §8). The gate names two
specifically; all three are PROVEN to exist with hardcoded expectations:
- **`tools_list_returns_seven_tools`** (`mcp/src/lib.rs:543`, asserts `== 7`) → rewritten to the new
  floor: **10 unconditional read tools** + `SemanticSearch` present iff `has_semantic_search && dim_ok`.
  Renamed to drop the stale magic number.
- **`tools_list_contains_expected_names`** (`mcp/src/lib.rs:555`, PROVEN it lists the 7 names) → add
  `RankHotspots`, `Communities`, `Lineage` to the expected set (and `SemanticSearch` conditionally).
- **`input_schema_known_tools_return_some`** (`mcp/src/lib.rs:938`, PROVEN it loops the 6 names) → add
  the 3 new tools + `Annotate` so `input_schema(name).is_some()` covers every advertised tool. (The
  `input_schema_unknown_tool_returns_none` test at `:955` stays — `Annotate` and friends must be known.)
- The dead `with_hash_embedder` server default (`lib.rs:73,309`) is **DELETED**, not flag-guarded; the
  `_sem_store_for_future_ctx` unused binding (`main.rs:175`) is wired or deleted (§2.2/§3.2).

- **Tag.** PROVEN-IN-DESIGN that these exact tests exist + their current assertions (all cited). Their
  green-after-update = BUILD-GATE (DoD-A4).

---

## 7 · House-guardrail compliance (CLAUDE.md) — deltas from v1

| Guardrail | v2 honoring (deltas bold) |
|---|---|
| Confidence + provenance on every edge | **`Annotate` enforces provenance + forces author IN THE TOOL** (constructor defaults empty, `annotation.rs:191-192`); `RankHotspots`/`Communities` advisory scores + R7 diag. |
| Stable IDs only | Tools key on `SymbolId`; **`symbols.gen` adds a generation WITHOUT changing the `SymbolId` string** (epoch lives in xedge's key, not the id — ADR-002 intact). |
| Bounded traversal only | `traverse_multi` carries `max_nodes`/`max_depth` like `traverse`; **the SqliteStore specialization is ONE bounded CTE per ply, not N-per-node.** |
| Retire as you go (§8) | **Three named conformance tests rewritten** (§6); `with_hash_embedder` default + `_sem_store_for_future_ctx` deleted in-change. |
| New module needs a consumer (§5) | Each tool → behavior test; each skill → resources test; emit seam → emit test + index/drift/annotate callers; **`symbol_epoch`/`traverse_multi`/`with_read_inline` → estate conformance tests + Lane X is the consumer.** |
| Rules as DATA | No new `match lang {}`; events are config-shaped strings; the gen-bump is a deterministic state check, not per-language logic. |
| Agent-behavior R1/R3/R5/R6/R7 | **`MutatingTool` envelope honest `isError` (R1); `LEXICAL-FALLBACK`/`EMBED-MISMATCH`/`EMBED-META-MISSING` ride per-call diagnostics (R6, NOT stderr); dead-letter spool logs drops loudly (R6).** |
| C4 lexical floor | `SearchEntity` untouched; semantic additive + self-labeling + **fail-closed on missing/mismatched meta.** |

---

## 8 · DECISIONS · RATIONALE · RISKS (consolidated)

### DECISIONS
- **D1.** Read-surface changes: +`RankHotspots`, +`Communities`, **promote `Lineage`** (C-A3) — 7→**10
  unconditional read tools** + a conditional `SemanticSearch`. **No MCP mutating tool in v1** — write is
  CLI-only (D5); the `Annotate` schema is the contract for CLI flags + the deferred MCP tool.
- **D2.** Fix the SemanticSearch dispatch bug (`wicked-estate-mcp/src/lib.rs:337` resolves `all_tools()`
  vs advertise at `:308`; wire `all_tools_with_semantic` `:62`), route through `default_embedder()`
  (rewire the hardcoded `with_hash_embedder` `:73`/`:309`), per-call MCP **diagnostics** for
  `LEXICAL-FALLBACK` (A-aB2).
- **D3.** Dim-guard on a real **`Embedder::id()`** + meta-write-at-index-time (written LAST) + fail-closed
  on `meta==None`; failure is silent-EMPTY → honest-absence (A-aB1).
- **D4.** Shared `enforce_annotation` helper (CLI now, deferred MCP later) rejects empty provenance,
  forces `author`, forces `requirement_validated=false`, truncates 8000/256 in Rust; net-new
  `MutatingTool` with a specified result→envelope (C-A1/A-aB4/A-aB5).
- **D5 (CORRECTED).** `Annotate` is **CLI-only in v1** (write via the binary `annotate` arm,
  `wicked-estate/src/main.rs:1581`; MCP read-only + doorbell, ZERO net-new `graph.db` writer). The MCP
  write seam is **DESIGNED-but-DEFERRED**: it requires a retained single-writer actor that ALSO owns the
  L2 cache writes (retire `cache_put`-on-pool, `main.rs:372`/`pool.rs:69`), gated on DoD-A2. **v1's
  "thread the writable handle main.rs already holds" is RETRACTED — no such handle exists**
  (`open_store_ext` `main.rs:160` is dropped at `:172`; long-lived `store` is a read `SqlitePool`,
  `:143`). (C-A2)
- **D6.** Emit via the 50 ms-budget, off-thread-outcome-read, NDJSON-dead-letter seam (drop `npx`);
  3 coarse contract-named events; **watch coalesces** to one doorbell per burst (A-aB3/A-aB6).
- **D7.** 3 `skill://` skills via the memory-mcp bundling pattern.
- **D8 (xedge, CORRECTED).** `symbols.gen` 0→1 bump in a **shared NODE-upsert helper covering both FTS
  variants — NOT in `intern`** (which 5 sites incl. edge endpoints call, → spurious bumps) + `symbol_epoch`;
  `traverse_multi` (22nd method, not Lane X v3's "28th") + SqliteStore specialization same-change;
  `with_read_inline` + `max_blocking_threads` floor. Sequenced per DEC-X6-SEQ/DEC-X2.

### RATIONALE (load-bearing)
- **Enforce in the tool, not the type** — the constructor defaults provenance/author empty
  (`annotation.rs:191-192`); honesty is a tool invariant or it is nothing (C-A1 was mis-grounded the
  other way).
- **Diagnostics ride the response, not stderr** — an agent reads the MCP envelope, never the CLI's
  stderr (`lib.rs:1155` `eprintln!`); a marker the agent can't see is no marker (A-aB2).
- **Fail-closed on missing meta** — silent-EMPTY (`nearest` skip, `sqlite.rs:712`) is invisible;
  refusing to advertise is the only honest degraded state (A-aB1).
- **The gen-bump must live at the SHARED seam** — the reindex hot path uses the skip-FTS variant; a bump
  only on `upsert_nodes` ships reuse-detection inert (ADV-1).
- **`traverse_multi` specialized + perf-asserted same-change** — equality conformance is GREEN for the
  slow fold; the sub-linear-in-W assertion is the only thing that prevents the N+1 shipping green (#3).

### RISKS (each names its falsifier + tag)
- **A-RISK-1 (the keystone) — the DEFERRED MCP write seam serving coherent reads without a second writer.
  BUILD-GATE (DoD-A2).** v1 sidesteps it: `Annotate` is CLI-only + doorbell, ZERO net-new writer
  (the re-gate proved no retained MCP writer exists and the cache is already a concurrent writer).
  Falsifier (for the deferred seam): concurrent indexer-write ⊕ actor-write ⊕ serving-read produces a
  stale read or `SQLITE_BUSY`, OR `cache_put`-on-pool is not retired (a second writer remains).
- **A-RISK-2 — dim-guard bypass. BUILD-GATE (DoD-A6a).** Falsifier: a hash-embedded store is queried by a
  384-d runtime and SemanticSearch is still advertised (returns silently-empty).
- **A-RISK-3 — emit drop still silent. BUILD-GATE (DoD-A3).** Falsifier: kill the bus mid-emit → the
  event is neither logged nor in the dead-letter spool.
- **A-RISK-4 — gen-bump inert on the reindex path. BUILD-GATE (DoD-XA4).** Falsifier: reindex via the
  skip-FTS path after a delete+re-add → `symbol_epoch` returns `Some(0)` (bump didn't fire there).
- **A-RISK-5 — `traverse_multi` N+1 leaks into SQLite. BUILD-GATE (DoD-XA2b).** Falsifier: query-count
  scales with frontier width W.
- **A-RISK-6 — `with_read_inline` deadlocks under load. BUILD-GATE (DoD-XA1b).** Falsifier: N concurrent
  cross-recalls hang OR blocking-thread high-water > cap during saturation.
- **A-RISK-7 — coarse events starve a lane-C reaction. PROVEN-IN-DESIGN mitigation, falsifiable.**
  Falsifier: a lane-C reaction that cannot be expressed as "wake on `wicked.estate.indexed`, read
  `changes_since`" (`traits.rs:111`).

### OPEN QUESTIONS
- **OQ-A1** — the DEFERRED MCP write-seam shape (NOT v1; v1 is CLI-only): a single-slot write-actor task
  (`mpsc` → one owning `GraphStoreMutExt`) that ALSO serves the L2 cache writes, replacing `cache_put`-on-
  pool. Confirm the cache re-plumb is in the SAME change that lands the actor (so no second writer ever
  co-exists), and that read serving still goes through the `with_read` pool. Gated by DoD-A2.
- **OQ-A2** — does `rationale-archaeology` need a dedicated `EdgeHistory` MCP read tool, or is the CLI
  `changes`/history path enough for v1? (Leaning CLI; defer the tool to keep the surface thin.)
- **OQ-A3** — should the dead-letter spool be drained by estate (next-emit replay) or by a bus-side
  drainer (lane C)? (Leaning next-emit replay so estate stays self-contained.)
- **OQ-X10 (inherited)** — `max_blocking_threads` value; DoD-XA1b's stress run picks it + documents the
  floor.

---

## 9 · DoD — every row tagged PROVEN-IN-DESIGN vs BUILD-GATE

> PROVEN-IN-DESIGN = a fact already true in committed code (cited) or a deterministic mechanism. BUILD-GATE
> = a runtime/empirical property the design does NOT claim to pass; the row states the exact test +
> ceiling + falsifier.

| # | condition | acceptance test + ceiling / falsifier | TAG |
|---|---|---|---|
| DoD-A0a | SemanticSearch dispatch bug is real | `tools/list` advertises at `wicked-estate-mcp/src/lib.rs:308`; `handle_tools_call_ctx` resolves vs `all_tools()` (`:337`) which excludes it (`:46`); the including registry `all_tools_with_semantic` (`:62`) is never called by the serving loop. | **PROVEN-IN-DESIGN** |
| DoD-A0b | constructor does NOT enforce provenance | `Annotation::new` → `provenance/author = String::new()` (`annotation.rs:191-192`); builders optional. | **PROVEN-IN-DESIGN** |
| DoD-A0c | MCP holds NO retained writer; serving path read-only | long-lived `store` = `open_async_store` → `SqlitePool` (READ pool) (`wicked-estate-mcp/src/main.rs:143`); `open_store_ext` (`:160`) opened-in-scope + DROPPED (`:172`); `with_read` only (`pool.rs:75`), `spawn_blocking` (`:83`); handler `&dyn GraphRead` (`wicked-estate-mcp/src/lib.rs:323`); dead 2nd handle (`main.rs:175`). | **PROVEN-IN-DESIGN** |
| DoD-A0c2 | MCP is ALREADY a `graph.db` writer via the cache | `store.cache_put` (`main.rs:372`) → pooled `block_in_place` write (`pool.rs:63,69`), real `INSERT…ON CONFLICT…UPDATE` (`sqlite.rs:458-466`); journal = default rollback (not WAL). | **PROVEN-IN-DESIGN** |
| DoD-A0d | failure on dim-mismatch is silent-EMPTY | `nearest` doc: mismatched-dim rows "silently skipped" (`sqlite.rs:712`, test at `:2446`); `default_embedder` `EMBED-FALLBACK` → `eprintln!` (`wicked-estate/src/lib.rs:1155,1167`). | **PROVEN-IN-DESIGN** |
| DoD-A1 | C-A1 enforcement helper | shared `enforce_annotation`: empty/whitespace provenance → reject (CLI non-zero exit; MCP-deferred `isError`); `author` forced; absent symbol → no-op. Tested at the CLI entry in v1. Falsifier: an empty-provenance write persists, or a client `author=system` sticks. | **BUILD-GATE** |
| DoD-A2 | C-A2 — v1 CLI-only; MCP write seam DEFERRED | v1: write via CLI `annotate` (`wicked-estate/src/main.rs:1581`), MCP read-only + doorbell, ZERO net-new `graph.db` writer. DEFERRED MCP seam: a retained single-writer actor that ALSO owns the cache (retire `cache_put`-on-pool) passes — concurrent indexer ⊕ actor-write ⊕ serving-read → consistent read, ZERO `SQLITE_BUSY`. Falsifier (deferred): stale read / BUSY / a second writer remains. | **BUILD-GATE** (the RETRACTION + already-live-cache-writer + CLI-only decision + envelope spec = PROVEN-IN-DESIGN) |
| DoD-A3 | A-aB3 dead-letter emit | kill the bus mid-emit → event appended to `emit-deadletter.ndjson` + logged loudly; index path latency unaffected (50 ms off-thread); no `npx`. Falsifier: a drop is silent. | **BUILD-GATE** |
| DoD-A4 | C-A4 conformance updated | `tools_list_returns_seven_tools` (`mcp/src/lib.rs:543`, asserts ==7) rewritten to the 10 unconditional read-tool floor + conditional `SemanticSearch`; `tools_list_contains_expected_names` (`:555`) + `input_schema_known_tools_return_some` (`:938`) cover `RankHotspots`/`Communities`/`Lineage`; NO `Annotate` in `tools/list` (read-only surface, §2.3); all green. | **BUILD-GATE** (the 3 tests + their current asserts = PROVEN-IN-DESIGN) |
| DoD-A5 | A-aB4/A-aB5 server-side caps + flag | helper forces `requirement_validated=false` on `op:"requirement"` (`set_node_semantics` `requirement_validated: Option<bool>` `traits.rs:169`); value/key truncated 8000/256 in Rust + `ANNOTATE-TRUNCATED`. Falsifier: a 9000-char value persists, or a client-set validated flag sticks. | **BUILD-GATE** |
| DoD-A8 | R4 (<25K) ceiling on new/promoted read tools (v1-gate ADV) | `Communities`/`Lineage`/`RankHotspots` output stays < 25K chars (R4, CLAUDE.md runtime contract) on a wide graph — `Lineage` hardcodes `max_nodes=5000` (`retrieve/src/lib.rs:1110`); assert a char-budget cap + truncation diag on each. Falsifier: a >25K payload on a large repo. | **BUILD-GATE** |
| DoD-A6 | A-aB6 watch coalescing | a burst of N file saves under `watch` → exactly ONE `wicked.estate.indexed` (coalesced, merged counts), not N. Falsifier: per-debounce-batch emit (process storm). | **BUILD-GATE** |
| DoD-A6a | A-aB1 dim-guard (real `id()` + fail-closed) | `Embedder::id()` added; `meta[embedder_id/dim]` written LAST at index time; `meta==None` → SemanticSearch NOT advertised + `EMBED-META-MISSING`; mismatch → NOT advertised + `EMBED-MISMATCH`; match → advertised. Falsifier: hash-store queried 384-d is still advertised. | **BUILD-GATE** (trait add + meta seam + fail-closed RULE = PROVEN-IN-DESIGN) |
| DoD-A6b | A-aB2 dispatch fix + real embedder + diagnostics | list→call reaches `SemanticSearch` (resolves vs the live registry, not `all_tools()`); routed through `default_embedder()`; a hash-fallback run carries `LEXICAL-FALLBACK` in the response `diagnostics`. Falsifier: `-32602 unknown tool` on call, or marker only on stderr. | **BUILD-GATE** (the bug + stderr sink = PROVEN-IN-DESIGN) |
| DoD-A7 | D7 skills bundled | `resources/list` returns 3 skills, `resources/read` returns each body; a `resources` test references each (no orphan, §5). | **BUILD-GATE** (the bundling pattern = PROVEN-IN-DESIGN) |
| DoD-XA4 | xedge `symbols.gen` bump (in `upsert_nodes_inner`, NOT `intern`) + `symbol_epoch`, NON-VACUOUS + NON-SPURIOUS | delete a symbol, re-add same name VIA `index_path`/`remove_file`+reindex (incl. the skip-FTS path `upsert_nodes_no_fts`) → `symbol_epoch(id)` = `Some(g)`, **g ≥ 1**; AND a symbol that was previously only a forward-referenced edge target (`intern`-only, `sqlite.rs:1100-1101/1142`) getting its FIRST node stays `gen=0`. Falsifier: g==0 after reuse (inert on skip-FTS path), OR g≥1 for a first-time node (spurious bump — the `intern`-placement bug). **DEC-X6-SEQ: green BEFORE Lane X about-arm reuse-safe.** | **BUILD-GATE** (schema + node-upsert-helper placement, NOT `intern` = PROVEN-IN-DESIGN) |
| DoD-XA2 | xedge `traverse_multi` (22nd method) correctness | `traverse_multi_matches_union_of_traverse` GREEN for MemStore + SqliteStore + PostgresStore + OverlayReader. (Method count = 22, not Lane X v3's "28" — propagate correction.) | **BUILD-GATE** |
| DoD-XA2b | xedge `traverse_multi` not-N+1 | SqliteStore `traverse_multi(W starts, 1 ply)` query-count ≤ C (fixed small C, e.g. ≤ 3) for ALL W in the sweep (SQLite trace/counting wrapper). Falsifier: query-count scales with W. | **BUILD-GATE** |
| DoD-XA1b | xedge `with_read_inline` + bounded threads | N concurrent cross-recalls (N ≥ 2×`max_blocking_threads`, cap small e.g. 8): zero deadlocks/timeouts AND blocking-thread high-water ≤ cap DURING saturation. Falsifier: hang or high-water > cap. | **BUILD-GATE** (panic disproven by Lane X v3 tokio trace = PROVEN-IN-DESIGN) |

**Summary of tags.** 5 rows PROVEN-IN-DESIGN (DoD-A0a–d + A0c2 — the dispatch bug, the empty-default
constructor, the no-retained-writer read-only seam, the already-live cache writer, the silent-EMPTY
failure — all cited verbatim and re-verified against the re-gate); the design portions of
A1/A2/A4/A6a/A6b/A7 and the xedge rows (the enforcement-in-helper, the C-A2 RETRACTION + CLI-only
decision, the fail-closed rule, the node-upsert-helper bump placement, the disproven panic) are
PROVEN-IN-DESIGN. **Every runtime/empirical property — enforcement firing (A1/A5), the DEFERRED MCP write
seam (A2; v1 ships CLI-only and is NOT blocked on it), dead-letter catching a drop (A3), conformance green
(A4), R4 ceiling (A8), watch coalescing (A6), dim-guard firing (A6a), dispatch fixed + diagnostics (A6b),
skills bundled (A7), the gen-bump firing non-vacuously AND non-spuriously (XA4), traverse_multi correct +
not-N+1 (XA2/XA2b), no-deadlock-under-load (XA1b) — is a BUILD-GATE the design explicitly does NOT claim
to pass.** The only things asserted already-true are what the cited code already does and what Lane X v3's
tokio source-trace already guarantees. **The riskiest assumption is A-RISK-1 / DoD-A2 — and v2's
resolution is to NOT ship an MCP writer in v1 at all (CLI-only), because the re-gate proved the "handle we
already hold" does not exist and the cache is already a concurrent writer.** The two re-gate BLOCKERS this
revision closed: B1/B2 (the false retained-handle cite → CLI-only + deferred actor) and B3 (the gen-bump
moved out of `intern` — which 5 sites call incl. edge endpoints — into a shared node-upsert helper).
