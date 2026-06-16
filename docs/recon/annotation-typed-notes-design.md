# Design: Typed name/value annotations (notes / assumptions / questions / observations)

**Status:** proposal (planning only — no code written) · **HEAD studied:** `5408ace`
**Author:** recon · **Scope:** extension of the existing annotation mechanism, not a replacement.

> Ask (project owner, paraphrased): *"Store one or more assumptions, questions, notes,
> observations, comments, etc. on graph entities. Fixed types the services can do special
> semantic things with (note, assumption, observation, comment, question), plus custom types —
> and if custom types still get the special features without being differentiated, even better."*

This document designs that feature as a **`type` field on the annotation record we already
have**. It also folds in open review item **#7** (community/`cluster_id`) by modelling a node's
community as a *system-derived* annotation of a known type rather than a bespoke column.

---

## 0. What already exists (study, with citations)

There are **two distinct annotation systems** in the tree today. Do not conflate them.

### A. The generic key/value annotation store — *this is what we extend*

- **Schema:** `annotations` table — `crates/wicked-estate-store/src/schema.sql:150-161`:
  ```sql
  CREATE TABLE IF NOT EXISTS annotations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_sym INTEGER NOT NULL,   -- sid FK → symbols.sid (same int PK as nodes.symbol)
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 1.0,
    provenance TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL DEFAULT '',
    ts INTEGER NOT NULL DEFAULT (strftime('%s','now'))
  );
  CREATE INDEX idx_annotations_node ON annotations(node_sym);
  CREATE INDEX idx_annotations_key  ON annotations(key);
  ```
- **Record struct:** `Annotation` — `crates/wicked-estate-store/src/sqlite.rs:70-79`
  (`key, value, confidence, provenance, author, ts`). Re-exported at
  `crates/wicked-estate-store/src/lib.rs:9`.
- **Methods (inherent on `SqliteStore`, NOT on a trait):**
  - `annotate_node(symbol, key, value, confidence, provenance, author)` — `sqlite.rs:482-512`.
    Note: this is a bare `INSERT`, **not** an upsert, so it already supports **multiple
    annotations per entity** (including duplicate keys). No-op if the symbol is absent (`:500-503`).
  - `get_annotations(symbol) -> Vec<Annotation>` — `sqlite.rs:514-553`, ordered `ts ASC`.
  - `find_by_annotation(key, value: Option) -> Vec<Node>` — `sqlite.rs:558-585`.
  - `delete_annotation(symbol, key) -> usize` — `sqlite.rs:587-613`. **Deletes ALL rows for
    that key** (gotcha — there is no per-row delete today; see Open Questions).
- **CLI surfaces** (`crates/wicked-estate/src/main.rs`):
  - `annotate <name>|--symbol <id> --key K --value V [--confidence][--provenance][--author]` —
    arm at `:1510-1554`. Fuzzy `<name>` annotates **every** match (`:1538-1551`).
  - `annotations <name>|--symbol <id>` — arm at `:1560-1600`.
  - `nodes [--kind K] [--annotated-with K[=V]] [--json]` — arm at `:1820-1901`; the
    `--annotated-with` filter calls `find_by_annotation` (`:1834-1868`).
- **Persistence guarantee:** annotations live in their own table and **survive re-indexing**
  (`main.rs:76`, `:90`) because re-index replaces `nodes`/`edges` per file, never `annotations`.

### B. The `NodeSemantics` system — *separate; do not reuse for this*

Three fixed columns hung off `nodes` (`description`, `requirement`, `requirement_validated`):
`crates/wicked-estate-core/src/semantics.rs:15-24`, schema `schema.sql:32-34`, trait methods
`GraphRead::node_semantics` / `GraphWrite::set_node_semantics` / `find_by_requirement`
(`crates/wicked-estate-core/src/traits.rs:106-110,142-151`). This is a *fixed-shape* requirement-
linking feature. It is **not** the generic store and is out of scope to change — but it is the
precedent that annotation-read methods *can* live on the `GraphRead` trait (see §6 seam).

### C. How annotations surface in retrieval/MCP today: **they do not**

`grep -ri annotat crates/wicked-estate-retrieve/src` → **zero hits**. `RetrieveEntity`
(`crates/wicked-estate-retrieve/src/lib.rs:390-469`), `ContextBundle`
(`context_bundle.rs`), and the MCP server (`crates/wicked-estate-mcp/src/lib.rs:48-76`,
`handle_request(store: &dyn GraphRead, …)` at `:411`) emit **no annotation data at all**.
The `source --json` bundle (`main.rs:892-983` → `source_bundle::build_bundle`,
`crates/wicked-estate/src/source_bundle.rs:155-172`) and `nodes --json` (`main.rs:1842-1885`)
also omit annotations. So "typed annotations in payloads" is **net-new surface area**, gated by
one structural fact (§6): the read methods are inherent on `SqliteStore`, but every retrieval/MCP
path holds only `&dyn GraphRead`.

### D. Community / `cluster_id` today (review item #7)

Communities are computed **on demand** and **never persisted per node**: `detect_communities` /
`summarize_communities` in `crates/wicked-estate-rank` (used at `main.rs:940-952`,
`crates/wicked-estate/src/lib.rs:1877-1944`), surfaced only via `clusters [--summary] --json` and
`source --cluster <id>`. There is **no `cluster_id` column** anywhere
(`grep -i cluster_id` → none). Item #7 proposes adding one; §7 below recommends *not* adding a
column and instead writing the community as a derived annotation.

---

## 1. Data model — recommendation

**Add exactly one nullable column, `type`, to the existing `annotations` table. Do not add a new
table. Do not touch `NodeSemantics`.**

```sql
-- schema.sql, appended to the existing CREATE TABLE annotations (...):
  type TEXT NOT NULL DEFAULT 'note'
-- plus:
CREATE INDEX IF NOT EXISTS idx_annotations_type ON annotations(type);
```

Rationale (opinionated, not a menu):

1. **One table.** A note, an assumption, a question, an observation, a comment, and an arbitrary
   custom type are the *same record shape* — `(entity, type, key, value, confidence, provenance,
   author, ts)`. They differ only by the value of one discriminator column. A second table would
   duplicate the FK, the indexes, the read/write/delete methods, and every payload-shaping site
   for zero modelling benefit. Rule §8 (retire-as-you-go) and the "rules as DATA" Don't both push
   toward one generic store keyed by a data value, not a structural fork.
2. **`type` is just a string.** Fixed types and custom types are stored **identically**. There is
   **no enum in the schema** and no `match type { … }` in storage — the known set is a *convention
   recognised by services*, exactly per the owner's "if custom types still get the special
   features, even better." Storage stays language/type-agnostic (the project's core Don't:
   logic-as-data, never compiled per-variant arms).
3. **Back-compat with existing untyped rows.** `NOT NULL DEFAULT 'note'` means every pre-existing
   row reads back as `type = "note"` (informational — the safest default; an old untyped tag was
   never an assumption or a question). `CREATE TABLE IF NOT EXISTS` will **not** alter an existing
   DB, so the open path must run an idempotent migration: `ALTER TABLE annotations ADD COLUMN type
   TEXT NOT NULL DEFAULT 'note'` guarded by a `PRAGMA table_info(annotations)` check (mirror the
   existing `execute_batch(SCHEMA)` flow at `sqlite.rs:269-306`). New DBs get the column from
   `SCHEMA` directly. **No data rewrite, no version bump required** — the default backfills on read.
4. **Multiple annotations per entity** is already true (bare INSERT, §0.A). `type` does not change
   that; an entity can carry many annotations of the same or different types.
5. **Identity stays the stable `SymbolId`.** `annotations.node_sym` is the interned `sid` for the
   `SymbolId` string (`schema.sql:148`, `sqlite.rs:491-499`). ADR-002 stable identity is preserved;
   annotations follow renames because they key on the symbol id, never line/content.

### Updated record struct

`Annotation` (`sqlite.rs:70-79`) gains one field. Add it **last** and default it on
deserialization so any serialized `Annotation` from an older build still loads:

```rust
pub struct Annotation {
    pub r#type: String,   // NEW — "note" | "assumption" | "observation" | "comment" | "question" | <custom>
    pub key: String,
    pub value: String,
    pub confidence: f64,
    pub provenance: String,
    pub author: String,
    pub ts: i64,
}
```

### A small typed helper in `core` (the only new type)

Put a **known-type registry** in `wicked-estate-core` so storage, CLI, and retrieval all branch
off one source of truth (rule §1 spine-before-fan-out; §11 fix-at-the-shared-seam). It is *data +
classification*, not a closed enum the storage layer switches on:

```rust
// crates/wicked-estate-core/src/annotation.rs   (new, ~40 lines)
pub const KNOWN_ANNOTATION_TYPES: &[&str] =
    &["note", "assumption", "observation", "comment", "question", "community"];

/// Semantic class a service uses to branch. `Custom(_)` deliberately maps onto the same
/// generic features as a known type — that is the owner's "even better" property.
pub enum AnnotationClass { Note, Assumption, Observation, Comment, Question, Community, Custom }

pub fn classify(ty: &str) -> AnnotationClass { /* str match → class; unknown ⇒ Custom */ }
/// Does this annotation reduce trust in the entity / need human review?
pub fn is_advisory(ty: &str) -> bool { matches!(classify(ty), Assumption | Question) }
/// Is this machine-derived rather than human-asserted? (community, future derived types)
pub fn is_system_derived(ty: &str) -> bool { matches!(classify(ty), Community) }
```

`classify` returning `Custom` for unknown strings is the whole "custom types get the same generic
features for free" guarantee: every generic path (store/query/filter/payload) treats `Custom`
identically to a known type; only the *special* semantic hooks key off the known classes.

---

## 2. Fixed types vs custom types — the semantics

**Generic features (identical for every type, known or custom):** stored in the one table,
returned by `get_annotations`, filterable by `find_by_annotation` and by type, emitted in every
payload with `{type, key, value, confidence, provenance, author, ts}`. A custom type like
`"adr-ref"` or `"security-concern"` is a first-class citizen with zero special-casing required.

**Special semantic handling (the known set only).** Concretely, what each known type *enables* —
each is one branch on `AnnotationClass`, never a per-type code fork in storage:

| Type | Class | Special semantic behaviour a service applies |
|---|---|---|
| `assumption` | advisory | **Lowers trust.** A retrieval tool surfacing an entity with an `assumption` annotation SHOULD attach an advisory flag and MAY discount derived claims about that entity. Counts toward an "entity rests on N unverified assumptions" signal. Agent-behavior R7 (confidence visible): an assumption is *not a fact* and must be presented as such. |
| `question` | advisory | **Open question.** Surfaced as an unresolved item ("3 open questions on this symbol"); a future `open-questions` query/tool can enumerate them across the graph (reusing `find_by_annotation("__type__", "question")` semantics, see §3). Signals incomplete understanding. |
| `note` | informational | Free-form human/agent note. Returned as-is; no trust effect. The **default** type for untyped/legacy rows. |
| `observation` | informational | A recorded fact-of-observation ("this is only called in tests"). Informational; higher implicit trust than `assumption` but still provenance-tagged. |
| `comment` | informational | Lightweight remark / discussion. Informational; lowest semantic weight. |
| `community` | system-derived | **Reserved, machine-written** (see §7). `is_system_derived` ⇒ tools MAY render it as a grouping label and SHOULD NOT count it as a human annotation in "N notes" tallies. Authored only by the pipeline, `author="system"`, `provenance="louvain:<params>"`. |

**Where the pipeline / a tool branches on type (illustrative):**

```rust
for a in get_annotations(&sym)? {
    match wicked_estate_core::annotation::classify(&a.r#type) {
        Assumption | Question => advisory.push(&a),     // → R7 advisory flag in payload
        Community               => grouping = Some(&a),  // → grouping label, not a "note"
        Note | Observation | Comment => notes.push(&a),  // → informational
        Custom                  => notes.push(&a),       // custom ⇒ same generic path
    }
}
```

The verdict-style rule: **only the known classes get hooks; `Custom` always falls through to the
generic informational path.** Adding a new known type later = one new arm in `classify` +
(optionally) one new branch where a service wants special handling — zero schema change.

---

## 3. Surfaces

### CLI (`crates/wicked-estate/src/main.rs`)

- `annotate … --type <t>` — add `--type` to the `annotate` arm (`:1510-1554`); default `"note"`
  when omitted (back-compat: existing scripts that omit it behave exactly as before). Thread it
  into `annotate_node(…, r#type, key, value, …)`.
- `annotations <name> [--type <t>]` — add an optional `--type` filter to the `annotations` arm
  (`:1560-1600`); when present, show only rows whose `type` matches. Print the type in the output
  line (currently `key=value [confidence=… provenance=… author=…]`, `:1569-1572,1590-1594`).
- `nodes --annotated-with K[=V] [--type <t>]` — extend `find_by_annotation` with an optional type
  filter (`:1834-1868`). Useful operationally: `nodes --type assumption` ⇒ "every entity carrying
  an assumption".
- `delete_annotation` gains an optional `--type` qualifier so a caller can scope the delete (the
  current all-rows-for-key behaviour is widened too coarsely — see Open Questions).

### Retrieval / MCP payloads

Once annotation reads are on `GraphRead` (§6), add a **bounded** `annotations` array to:

- **`RetrieveEntity`** (`retrieve/src/lib.rs:426-466`) — add, after `doc`:
  ```json
  "annotations": [
    { "type": "assumption", "key": "thread-safety",
      "value": "assumed Send+Sync; unverified", "confidence": 0.6,
      "provenance": "manual", "author": "alice", "ts": 1718500000 }
  ],
  "annotation_summary": { "total": 4, "assumption": 1, "question": 1, "note": 2 },
  "advisory": true          // present iff any advisory-class annotation exists
  ```
  `advisory: true` is the R7 hook (heuristic/assumption visibly flagged). Default-include the array
  but **cap it** (see R4 below); `annotation_summary` always fits even when the array is capped.
- **`source --json` bundle** (`source_bundle.rs:155-172`) — add a per-node `annotations` array.
  `build_bundle` already takes closures for `symbol_source`/`file_git_sha` (`main.rs:974-980`); add
  an `annotations_of: |&SymbolId| -> Vec<Annotation>` closure the same way (the CLI passes
  `store.get_annotations`). Keeps the bundle builder store-agnostic.
- **`nodes --json`** (`main.rs:1842-1885`) — add `annotations` per node object (this arm already
  holds a concrete `SqliteStore` at `:1832`, so it can call `get_annotations` directly without the
  trait change).
- **MCP** inherits `RetrieveEntity` automatically — the MCP server just lists/dispatches the
  retrieval tools (`mcp/src/lib.rs:48-76,309-371`); no MCP-specific change beyond the schema doc
  string for `RetrieveEntity` (`retrieve_entity_schema()` referenced at `mcp/src/lib.rs:236`).

**R4 (output < 25K chars) is the hard constraint here.** Typed annotations are unbounded
user/agent content injected into already-large payloads. Mandate:
- A per-tool cap (recommend **default 20 annotations/entity**, **value truncated to ~500 chars**),
  with `annotation_summary.total` always reporting the true count so truncation is never silent
  (mirrors the bundle's `source_truncated` honesty at `source_bundle.rs:17`).
- Advisory-class annotations (`assumption`/`question`) are **prioritised** when the cap bites —
  they change agent behaviour, so they must not be the ones dropped.
- `system-derived` (`community`) rows are excluded from the human-annotation array by default and
  represented once in `annotation_summary` / as a single grouping field, not repeated per payload.

### Filtering primitive (one new read)

Add `annotations_by_type(type) -> Vec<(SymbolId, Annotation)>` (cheap: `idx_annotations_type` +
join to `symbols`) to power "all open questions" / "every assumption in the repo" without scanning
all nodes. This is the read that a future `open-questions`/`assumptions` CLI or tool stands on.

---

## 4. Entities

- **Nodes: yes, now.** `annotations.node_sym` already keys nodes by interned `SymbolId` — no change.
- **Edges: deferred, but the model already fits.** Edges are keyed by `(source, target, kind)`
  (`schema.sql:51-59`), not a single id, so edge annotations need either an edge-id surrogate or a
  composite-key table. Recommendation: **do not build edge annotations in v1**; note it as a clean
  extension (a sibling `edge_annotations(source,target,kind,type,key,value,…)` table reusing the
  same `type` vocabulary). Most assumptions/questions attach to *symbols*, not specific edges.
- **Communities: covered via §7** — a community is addressed as an annotation on its *member
  nodes* (or a representative node), not as a first-class annotatable entity. No new entity type.
- **Files: out of scope.** Files are not `SymbolId`-keyed nodes; skip for v1.

Keep the surface honest: v1 = **node annotations only**, typed. Edges/files are explicitly
not-yet-done (rule §7).

---

## 5. (covered in §7)

## 6. Build plan — seam first, then parallel-safe chunks

The **one true dependency** (rule §1, §2 — serialize only real cross-cutting deps) is: *retrieval
and MCP hold `&dyn GraphRead`, but annotation reads are inherent on `SqliteStore`.* Until a read
method exists on the trait, **no retrieval/MCP surface can show annotations**. So:

**Chunk 0 — the seam (serial, must land first).** `crates/wicked-estate-core`.
- Add `crates/wicked-estate-core/src/annotation.rs`: a `core::Annotation` struct (move/lift the
  shape so it is not store-private), `KNOWN_ANNOTATION_TYPES`, `AnnotationClass`, `classify`,
  `is_advisory`, `is_system_derived`.
- Add to `GraphRead`: `fn annotations(&self, symbol: &SymbolId) -> Result<Vec<Annotation>>` and
  `fn find_by_annotation_type(&self, ty: &str) -> Result<Vec<(SymbolId, Annotation)>>` (parallels
  the existing `node_semantics` precedent at `traits.rs:106-110`).
- Add to `GraphWrite`: `fn annotate(&mut self, symbol, ty, key, value, confidence, provenance,
  author) -> Result<()>` and a scoped `fn delete_annotation(&mut self, symbol, ty: Option<&str>,
  key: &str)`.
- **Extend the GraphStore conformance kit** (`crates/wicked-estate-core/src/conformance.rs`):
  typed round-trip, default-type back-compat, multi-annotation-per-entity, type filter,
  system-derived exclusion. **Conformance is the gate (§9)** — both `SqliteStore` and `MemStore`
  must pass before any fan-out.

**Chunk 1 — storage (parallel after Chunk 0).** `crates/wicked-estate-store`.
- `schema.sql`: add `type` column + `idx_annotations_type`; add the idempotent
  `ALTER TABLE … ADD COLUMN type` migration in the open path (`sqlite.rs:269-306`).
- Implement the new trait methods on `SqliteStore` (rewire existing `annotate_node`/
  `get_annotations`/`find_by_annotation`/`delete_annotation`, `sqlite.rs:482-613`, to carry `type`)
  **and** on `MemStore` (`crates/wicked-estate-store/src/lib.rs`).
- Retire (§8): the bare inherent `annotate_node`/`get_annotations` either become trait impls or are
  deleted in favour of the trait methods in the **same change** — no parallel old/new API.

**Chunk 2 — CLI surface (parallel after Chunk 0; file-disjoint from retrieve).**
`crates/wicked-estate/src/main.rs` + `source_bundle.rs`.
- `--type` on `annotate`; `--type` filter on `annotations` and `nodes`; print `type`.
- Thread an `annotations_of` closure into `source_bundle::build_bundle`; add `annotations` to
  `nodes --json`.

**Chunk 3 — retrieval/MCP surface (parallel after Chunk 0; file-disjoint from CLI).**
`crates/wicked-estate-retrieve` (+ schema doc-string in `crates/wicked-estate-mcp`).
- `annotations` array + `annotation_summary` + `advisory` flag on `RetrieveEntity`
  (`retrieve/src/lib.rs:426-466`), behind the R4 cap + advisory-priority logic.
- Optionally surface `annotation_summary` on `ContextBundle` neighbours (count only, never the
  full array — R4).
- Update `retrieve_entity_schema()` doc text so MCP `tools/list` advertises the new fields.

**Chunk 4 — community-as-annotation (depends on Chunk 1).** `crates/wicked-estate` (+ maybe
`wicked-estate-rank`). See §7. This is the item-#7 resolution; it's a *writer* on top of the seam.

> The two consumer chunks (2 and 3) are **file-disjoint** and run in parallel once Chunk 0 + the
> conformance test are green — exactly the spine-then-fan-out pattern (§1). Coordinate with the
> other agent currently editing `retrieve`/`main.rs`: Chunk 2 and Chunk 3 touch those files, so
> sequence against their work or carve the specific functions.

**Gates that stay green (§9):** workspace build (0 warnings) · all tests · **GraphStore
conformance** (now incl. typed annotations) · agent-eval benchmark must not regress.

---

## 7. Fold-in: `cluster_id` / community as a system-derived annotation (review item #7)

**Recommendation: do NOT add a `cluster_id` column to `nodes`. Model a node's community as a
derived annotation of the reserved known type `community`.**

- A pipeline step (post-`detect_communities`, `main.rs:940-952`) writes, per member node:
  `annotate(sym, type="community", key="community", value="<community-id-or-label>",
  confidence=<modularity-or-1.0>, provenance="louvain:res=<γ>,bias=<b>", author="system")`.
- **Why this over a column:**
  1. *No schema fork for a derived attribute.* The annotation store already exists, is indexed by
     node and by key/type, and survives re-index. A bespoke `cluster_id` column couples the node
     row to one partitioning algorithm and one resolution; communities are parameterised
     (`--resolution`, `--hierarchical`, `--weight semantic`, `main.rs:361-369`) and **recomputed**,
     so a single column lies the moment you re-run with different γ.
  2. *Provenance + confidence come for free* — the annotation row carries which algorithm/params
     produced the label (the project Don't: "confidence + provenance on every edge"; same spirit
     for derived node facts). A raw `cluster_id` integer carries none of that.
  3. *Multiple partitions coexist.* Graph-Louvain vs semantic clustering (`clusters --weight
     semantic`) can both write, distinguished by `key`/`provenance` (e.g. `key="community"` vs
     `key="community:semantic"`) without competing for one column.
  4. *`is_system_derived` keeps it honest* — tools render it as a grouping label, exclude it from
     human-annotation tallies, and never present it as a user note (§2).
- **Staleness:** because it's derived, the writer SHOULD clear prior `community` rows
  (`delete_annotation(sym, Some("community"), "community")`) before rewriting, and the value MAY go
  stale after incremental re-index — acceptable for a *derived* annotation, and honest because the
  provenance records the params it was computed under. (This is a deliberate trade vs a column that
  would also be stale but *silently*.)

This **resolves item #7** without a node-schema change: community membership becomes one more
typed annotation, queryable by the same `--annotated-with community` / `find_by_annotation_type`
machinery as everything else.

---

## Open questions for the owner

1. **Default type for legacy/untyped rows:** confirm `"note"` (informational, no trust effect) is
   the right backfill rather than a neutral `"untyped"` sentinel.
2. **`delete_annotation` semantics:** today it deletes **all** rows for a key
   (`sqlite.rs:587-613`). Scope new deletes by `(type, key)`? Add a precise per-row delete (by
   `id`/`ts`)? The current coarse behaviour is a latent foot-gun once multiple typed rows share a
   key.
3. **R4 caps:** OK to default to **20 annotations/entity, ~500-char values**, advisory-class
   prioritised when truncating? Or a char-budget like the source bundle's `max_total_chars`?
4. **`advisory` trust effect:** should an `assumption`/`question` annotation actually *discount* an
   entity in ranking/RRF, or only *flag* it in payloads (R7)? Recommend flag-only in v1; discount
   is a benchmarked follow-up so we don't regress the agent-eval.
5. **Community write trigger:** auto-write `community` annotations on every `index`, or only on an
   explicit `clusters --persist` opt-in? Auto-write risks stale labels after partial re-index;
   recommend opt-in for v1.
6. **Edge annotations:** confirm deferring to a later wave (v1 = nodes only) is acceptable.
7. **Type validation:** strictly none — any non-empty string is a valid custom type (matches the
   "custom types for free" ask)? Or reject reserved-but-misused values (e.g. a human writing
   `community` by hand)? Recommend: warn-but-allow; reserve `author="system"` semantics, not the
   string.
