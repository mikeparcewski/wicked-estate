# Brain → estate consolidation: telemetry & relation-signal import contract

Stage S1 of the wicked-brain → wicked-estate consolidation. wicked-brain's tuned
ranking signals live only in its `.brain.db` (not in the markdown), so a naive
re-ingest silently drops them. This gives estate a **real, lossless destination**
for all four and defines the exact import surface the brain-side export tool targets.

Source of truth for the brain side: `wicked-brain/server/lib/sqlite-search.mjs`.

## The four signals

| # | Brain source (`.brain.db`) | Estate destination | Import surface |
|---|---|---|---|
| 1 | `links.confidence` (REAL, tuned ±0.1/±0.2 by `confirm_link`) | knowledge relation edge **`confidence`** (`Edge.confidence`, promoted `edges.confidence` column) — already existed | `knowledge.relate` `confidence` param (already present) |
| 2 | `links.evidence_count` (INTEGER, incremented by `confirm_link`) | knowledge relation edge **`evidence_count`** — NEW **first-class `Edge` field** (`u32`, `#[serde(default)]`). Round-trips in all four backends; `SqliteStore` additionally promotes it to the NEW queryable `edges.evidence_count` column | `knowledge.relate` **new** `evidence_count` param |
| 3 | `access_log(doc_id, session_id, accessed_at)` | NEW `access_log(item_id, session_id, accessed_at)` table | `wicked-estate import-telemetry` CLI |
| 4 | `search_misses(query, searched_at, session_id)` | NEW `search_misses(query, searched_at, session_id)` table | `wicked-estate import-telemetry` CLI |

## Per-signal column mapping

### 1. `links.confidence` → knowledge relation `confidence`
Knowledge relations are estate `Edge`s; every edge already carries a `Confidence`.
`knowledge.relate` already accepts an optional `confidence` (number). The migrator
passes the brain's stored value verbatim.

- brain `links.confidence` **→** `knowledge.relate` arg `confidence` **→** `Edge.confidence` / `edges.confidence`.
- Note: `knowledge.relate` defaults `confidence` to `0.8` when omitted; brain's table default is `0.5`. The migrator MUST send the brain's real per-link value so the default never applies.

### 2. `links.evidence_count` → knowledge relation `evidence_count`
No estate edge carried an evidence counter. Per human review of PR #95, it is a
**first-class field on the spine `Edge` struct** (`pub evidence_count: u32`,
`#[serde(default)]`) — not a metadata key. Because every backend stores the full
`Edge` as JSON (`data`) / holds the struct (MemStore), the field round-trips in all
four backends automatically, and the GraphStore conformance suite asserts that
round-trip for each. `SqliteStore` **additionally** promotes it to a queryable
`edges.evidence_count` column (written in lockstep with `data`); Postgres/Surreal/Mem
carry it via the serialized `Edge` (no promoted column — round-trip fidelity is
identical, the column is a SQLite-only audit affordance).

- brain `links.evidence_count` **→** `knowledge.relate` arg `evidence_count` (integer, default 0) **→** `Edge.evidence_count` (round-trips in all backends) **→** also mirrored to SQLite's `edges.evidence_count` column.

### 3. `access_log` → estate `access_log`
Per-item access telemetry (feeds an access-count + session-diversity ranking boost).

| brain column | estate column | notes |
|---|---|---|
| `doc_id` | `item_id` | the accessed item's stable id — a knowledge-node `SymbolId` string for the knowledge store |
| `session_id` | `session_id` | verbatim |
| `accessed_at` | `accessed_at` | epoch **millis** (unchanged) |

### 4. `search_misses` → estate `search_misses`
Failed/empty-query log (feeds synonym suggestion / gap-hunting). Columns map 1:1:
`query → query`, `searched_at → searched_at` (epoch millis), `session_id → session_id`
(nullable).

## Import surfaces

### `knowledge.relate` (MCP tool) — signals 1 & 2
Both the standalone `wicked-knowledge` server and the unified `wicked-estate`
MCP server accept:

```json
{ "name": "knowledge.relate",
  "arguments": {
    "src": "<knowledge node id>", "tgt": "<knowledge node id>", "rel": "governs",
    "confidence": 0.7, "evidence_count": 3, "provenance": "brain-migration" } }
```

`src`, `tgt`, `rel` required; `confidence` (default 0.8), `evidence_count`
(default 0), `provenance` (default `"knowledge.relate"`) optional. Both endpoints
must be live nodes (else `isError:true`). `evidence_count` must be a
non-negative integer that fits u32 (≤ 4294967295) — anything else is rejected
with JSON-RPC `-32602` rather than silently truncated.

Re-relating an existing `(src, tgt, rel)` updates the stored edge when the new
confidence is ≥ the stored one **or** the new `evidence_count` is greater —
evidence growth is strictly newer information, so a contradicted link (lower
confidence, higher evidence) still lands. A same-evidence lower-confidence
write is treated as stale and ignored.

### `wicked-estate import-telemetry <file.json>` (CLI) — signals 3 & 4
Bulk-imports the two telemetry tables from one JSON file. Point `--db` at the
target **SQLite store file** (the knowledge db for knowledge telemetry; both
signals are opaque id/query strings, so any SQLite store file works — graph db
or knowledge db). SQLite-only today: the telemetry tables live in the SQLite
schema and the import APIs are `SqliteStore` methods; a `postgres://` spec is
rejected with a clear error. Additive — never touches nodes/edges.

```
wicked-estate import-telemetry telemetry.json --db /path/to/knowledge.db
```

Accepted file shape (both arrays optional — `wicked_estate_store::TelemetryImport`):

```json
{
  "access_log": [
    { "item_id": "kconcept::<uuid>", "session_id": "s1", "accessed_at": 1700000000000 }
  ],
  "search_misses": [
    { "query": "how does rrf fusion work", "searched_at": 1700000300000, "session_id": "s1" },
    { "query": "vault evidence gate", "searched_at": 1700000400000 }
  ]
}
```

Prints `imported <N> access-log row(s), <M> search-miss(es) into <db>`.

## Migration mechanics (estate SQLite)

Estate versions its SQLite schema by **column-presence checks** (`PRAGMA table_info`),
not a `_schema_version` row (`migrate_schema` in
`crates/wicked-estate-store/src/sqlite.rs`).

- **New column** `edges.evidence_count` (SQLite only) — presence-guarded `ALTER TABLE …
  ADD COLUMN evidence_count INTEGER NOT NULL DEFAULT 0` in `migrate_schema`, plus the
  column in `schema.sql` for fresh DBs. `DEFAULT 0` backfills every pre-existing edge
  (code edges never confirm → stay 0). This column is a queryable audit **mirror** of the
  authoritative `Edge.evidence_count` field, which already round-trips inside `edges.data`;
  the two are written in lockstep by `upsert_edges`.
- **New tables** `access_log`, `search_misses` — added to `schema.sql` only. `SCHEMA`
  runs (`CREATE TABLE IF NOT EXISTS`) on every writable open, so existing DBs gain the
  tables on next open with no explicit ALTER.

All changes are additive with sensible defaults; existing rows/relations keep working.
