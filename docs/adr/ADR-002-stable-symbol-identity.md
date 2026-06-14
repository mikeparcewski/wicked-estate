# ADR-002 — Stable Symbol Identity

**Status:** Accepted · **Date:** 2026-06-12 · **Wave:** W0.3
**Implements:** `crates/wicked-estate-core/src/symbol.rs` (tests pin the semantics)

## Context

the graph uses **content-hash node IDs**, so renaming or moving a function breaks every
edge that referenced it (the design notes). None of the reviewed systems have
SCIP-style stable monikers. Without stable identity, incremental indexing churns,
edges rot, and a temporal graph is impossible.

## Decision

Adopt a **SCIP-inspired structured `Symbol`** that renders to a canonical `SymbolId` string used
as the primary key everywhere. Identity is derived from the symbol's **logical name path**, never
from source bytes or line numbers.

`Symbol` variants:
- `Global { scheme, package, descriptors }` — exported/qualified symbols. `descriptors` is a path
  of `(name, Suffix)` where `Suffix ∈ {Namespace `/`, Type `#`, Term `.`, Method `().`,
  TypeParameter `[]`, Parameter `()`, Meta `:`, Macro `!`}`. Overloads disambiguate via an
  optional method disambiguator.
- `Local { scheme, file, id }` — file-scoped locals, not addressable cross-file.
- `File { path }` — source-file nodes.
- `Synthetic { scheme, id }` — extractor-injected non-code nodes.

Example: function `parse` in module `src/util` →
`scip-ts . . . src/util/parse().` (verified in `symbol.rs::renders_scip_like_string`).

## Stability semantics (the worked examples — all tested)

| Change | Identity | Test |
|---|---|---|
| Line shift / reformat / unrelated edit in same file | **unchanged** ✅ | `id_is_stable_across_location_changes` |
| Rename the symbol | **new identity** (correct — different symbol) | `rename_changes_identity` |
| Move to a different module path | **new identity** (correct — different logical location) | `module_move_changes_identity` |

The win over content-hash: identity tracks the *logical name path*, so line shifts and formatting
never churn IDs and edges survive. A rename or module move is a genuine logical change, so a new
identity is the correct behavior — not a bug.

## Consequences

- Enables correct incremental re-indexing (only genuinely-changed symbols get new IDs) and a
  future temporal graph (W7.1).
- For languages where the "logical path" includes the file/module (most of them), a file move
  *does* change the ID. That is intended. Cross-move *history* (linking old→new identity across a
  rename) is a separate, optional concern handled at the temporal layer, not here.
- Precise tiers (SCIP) emit their own symbol strings; we normalize them into this scheme on
  ingest so tree-sitter-derived and SCIP-derived identities reconcile (W1.4 / W3.4).
