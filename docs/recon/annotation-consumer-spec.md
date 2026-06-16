# Spec: typed annotations — consumer contract (wicked-estate, landing 0.5)

Engine-side build is in progress (seam + storage + conformance, then CLI/MCP surface). The shapes
below are the **contract** — build against them. Additive to today's annotations (one new `type`
field); existing untyped `annotate` calls keep working.

## Model
An annotation is a name/value note on a graph entity (a symbol, keyed by stable `SymbolId`, ADR-002).
Multiple per entity (append, not upsert). Today's fields + one new `type`:

```
Annotation {
  type:       string    // NEW — semantic class; default "note"; fixed set OR custom
  key:        string    // name
  value:      string    // value
  confidence: number    // 0.0–1.0 — extraction/assertion certainty (meaning UNCHANGED)
  provenance: string    // where it came from
  author:     string    // who/what wrote it ("system" for engine-derived)
  ts:         integer    // unix seconds, engine-set
}
```
`type` is a **plain string, no enum** — fixed and custom types are stored and queried identically.
The only difference is that services give *fixed* types special handling; custom types get every
generic feature with no special semantics.

## Fixed types
| type | class | special handling |
|---|---|---|
| `note` | informational | default; pre-0.5 untyped rows read back as `note` |
| `observation` | informational | recorded fact-of-observation |
| `comment` | informational | lightweight remark |
| `assumption` | **advisory** | payload carries `advisory: true`; present as NOT a fact (agent rule R7). Confidence is unchanged — advisory is a separate trust signal. |
| `question` | **advisory** | open/unresolved; `advisory: true`; enumerable as "open questions" across the graph |
| `community` | system-derived | machine-written grouping label (`author:"system"`); excluded from human-note tallies |
| *(any other)* | custom | stored/queried like any; no special semantics |

## Write — CLI
```
wicked-estate annotate <name | --symbol <id>> --type <t> --key K --value V \
    [--confidence F] [--provenance S] [--author S] [--db <file>]
```
- `--type` defaults to `note` (back-compat). `--symbol <id>` is precise; `<name>` is fuzzy.
- Multiple annotations per symbol; no dedup.

## Read — CLI
```
wicked-estate annotations <name> [--type <t>] [--json] [--db <file>]
```
`--json`:
```json
{ "symbol": "<symbol_id>",
  "annotations": [
    { "type":"assumption", "key":"...", "value":"...", "confidence":0.7,
      "provenance":"...", "author":"...", "ts":1718500000, "advisory":true } ] }
```
- `--type <t>` filters (fixed or custom). `advisory:true` is emitted for `assumption`/`question`, else absent/false.

## Annotations in structured payloads
`nodes --json` and `source --json` (the bulk bundle) — each node object gains, when present:
```json
"annotations": [ { type,key,value,confidence,provenance,author,ts,advisory } ],
"annotation_summary": { "count": N, "by_type": {"note":2,"assumption":1}, "has_advisory": true }
```
`RetrieveEntity` (MCP) gains the same two fields.

## R4 payload cap
Annotations in payloads are capped at **20 per entity**. When an entity has more, **advisory-class
(`assumption`/`question`) are kept first**, then the rest by recency (`ts` desc). `annotation_summary.count`
always reflects the TRUE total, so a consumer knows it was capped (mirrors the source-bundle "summary
is always exact" rule). `annotations` queried directly via the CLI is **not** capped — only payloads.

## Contracts
- Stable `SymbolId` keying — annotations follow renames/moves (ADR-002).
- Backfill: pre-0.5 untyped annotations read as `type:"note"`. No consumer action.
- Read paths never mutate.

## Out of scope (this round)
- **Edge annotations** — deferred; nodes + communities only.
- The `community` write trigger is engine-internal (opt-in) — consumers just READ `type:"community"`.

## Build-ahead notes for the consumer
1. Gate "is this a fact?" off the computed **`advisory`** field, not the type string — so a custom
   advisory-like type you adopt later can opt in without your code changing.
2. Use **`annotation_summary`** for cheap triage ("does this symbol have open questions/assumptions?")
   without pulling every value.
3. Writes are additive and unordered; if you need idempotence, key on `(type,key)` yourself.
