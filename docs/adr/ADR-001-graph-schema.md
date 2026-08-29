---
id: wicked-estate-adr-001
title: "Graph Schema v2"
status: active
date: 2026-06-12
---
# ADR-001 — Graph Schema v2

**Status:** Accepted · **Date:** 2026-06-12 · **Wave:** W0.2
**Implements:** `crates/wicked-estate-core/src/{node,edge,refs}.rs`

## Context

A small node/edge graph in SQLite is the proven substrate, but three recurring gaps make such a
graph untrustworthy: provenance with no confidence score (you can't tell a guess from a fact);
confidence + `resolvedBy` but content-hash node IDs that break on every rename; and rich edge
metadata that is never ranked. We want one schema that fixes all three.

## Decision

**Nodes** (`node.rs::Node`) are keyed by a stable [`SymbolId`] (see ADR-002), never by content
hash. A node carries `kind` ([`NodeKind`]), `name`, `language` (a *newtype string*, so adding a
language is zero core change), a mutable `location`, optional `signature`/`doc`, and a JSON
`metadata` bag.

`NodeKind` is an enum of the common kinds (File, Module, Class, Struct, Interface, Trait,
Function, Method, Field, Constant, TypeAlias, Macro, Import, …) plus `Synthetic` (for non-code
nodes injected by extractors) and `Other(String)` (open extension).

**Edges** (`edge.rs::Edge`) are directed `source → target` and **every edge carries four
mandatory fields**:

| Field | Type | Purpose |
|---|---|---|
| `confidence` | `Confidence` (f32 ∈ [0,1]) | how sure we are of this edge |
| `provenance` | `Provenance` | *class* of producer (Parsed / Tags / ImportMap / Heuristic / Tsg / Scip / Lsp / Synthesizer(name) / Extractor(name)) |
| `resolved_by` | `String` | the *specific* resolver id (e.g. `scip-typescript`) |
| `kind` | `EdgeKind` | Contains, Defines, Calls, Imports, References, Instantiates, Implements, Extends, Overrides, HasType, Returns, `Other(String)` |

Confidence/provenance are derived from a `ResolutionTier` (`Parsed|Scip|Lsp = 1.0`, `Tsg = 0.8`,
`ImportMap = 0.6`, `Heuristic = 0.5`, `Tags = 0.3`) so resolvers stay consistent. Splitting
`provenance` (class) from `resolved_by` (instance) is what enables **per-class precision
monitoring** (W3.5) — impossible with a single combined provenance field.

**Edge direction is an invariant**: `source = dependent`, `target = dependency`. Documented and
tested in `docs/ENGINE-CONTRACT.md` and `conformance.rs`.

**Two-phase staging**: extraction emits `UnresolvedRef`s (`refs.rs`) — references by *name*, not
yet bound — which the resolver pass turns into edges once all symbols are known. This decouples
parse from resolve (resolution swappable without re-parsing) and is the `unresolved_refs`
pattern made first-class.

**Metadata** is `serde_json::Map` on both nodes and edges — the open extension point for
extractor-specific data (ORM table names, call-site receiver types, event-bus topics).

## Consequences

- A single edge may be produced by multiple resolvers; the store keeps the **highest-confidence**
  one on a `(source, target, kind)` collision (`Edge::dedup_key`, W3.4).
- The schema is storage-agnostic — it maps cleanly to SQLite tables *or* SurrealDB records
  (ADR-003 / W1.5), because identity and attributes live in the types, not the engine.
- `Confidence` deserialization does not re-clamp; inputs are trusted within the pipeline.
