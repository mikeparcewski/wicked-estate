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

## Amendment 2026-08 — type-nested member identity (symbol-id scheme 2)

**Status:** Accepted (amends shipped practice, not the decision above) ·
**Resolves:** adversarial-review findings D03-1 / D03-2 / PER-7 (estate-review 2026-08-28)

### The defect

The Decision above defines `descriptors` as "a path of `(name, Suffix)`" with Type `#` as a path
component — but the shipped extractor built every definition id from exactly TWO descriptors,
`[module/, name<suffix>]`. The enclosing class was never pushed, so identity was *file-scoped by
name*: `Repo.save` and `Cache.save` in one module minted the SAME id, and the store's idempotent
upsert (`ON CONFLICT(symbol) DO UPDATE`, `wicked-estate-store/src/sqlite.rs`) collapsed them into
one node. `this.save()` then "resolved" into the merged node at 0.65 — a false edge presented as
a fact.

### The rule (scheme 2)

A definition's descriptors are `[module/] ++ [T1#, T2#, …] ++ [name<suffix>]`, where the chain
`T1#…Tn#` is the **contiguous run of Type-suffixed definitions immediately enclosing the
definition**, outer → inner. Computed by walking ALL containing definitions (strict containment,
duplicates by `(start, end, name)` deduped) innermost → outermost, collecting Type descriptors,
and **stopping at the first container that is not Type-suffixed** — Method, Function, and Term
containers all truncate the chain. Worked strings:

| Definition | Scheme 1 (flat) | Scheme 2 |
|---|---|---|
| `class Repo { save() {} }` | `src/m/save().` | `src/m/Repo#save().` |
| Python `class Outer: class Inner: def run` | `src/m/run().` | `src/m/Outer#Inner#run().` |
| `flush() { const cb = () => … }` | `src/m/cb().` | `src/m/cb().` (Method container truncates) |
| `const lit = { save() {} }` | `src/m/save().` | `src/m/save().` (Term container truncates) |
| `export function top()` | `src/m/top().` | `src/m/top().` (unchanged) |

Functions and Term bindings contribute NO descriptor: anything declared inside a
function/method/Term body stays module-flat (`Symbol::Local` remains unadopted). This preserves
the rename-stability argument above — renaming a function must not churn the ids of things
declared inside it.

### Stability semantics — two new rows

| Change | Identity | Why it is correct |
|---|---|---|
| Rename / move the enclosing type | **new identity for its members** | different logical path — same rule as a module move |
| Move a member between two types in one file | **new identity** | it is a different logical symbol |

### Accepted residuals (all fixture-pinned in `treesitter.rs` `identity_*` tests)

- Function-local definitions (nested functions, arrows bound inside a method, object-literal
  methods) keep `<module>/name<suffix>` and still collide with each other and with a same-named
  module-level definition — smaller, visible residuals that were previously hidden inside the
  bigger merge.
- `class A { save(){} x = { save(){} } }`: the object-valued field `x` is not captured as a
  definition (the TS query requires an arrow value), so the inner `save` nests under `A` and
  merges with the real `A.save()` (`identity_field_object_literal_residual`). Fix belongs in the
  query file (capture object-valued `public_field_definition` as a Term def) — extraction-gaps
  lane.
- Overloads within one type still collapse: `disambiguator` stays `None` (pinned by
  `identity_disambiguator_is_none`).
- Languages whose owner is not an enclosing definition anchor still collide: Rust `impl` blocks
  (only the inner `function_item` is captured), Go receiver methods, Ruby `class << self`. Query
  anchors are the fix (rules-as-data) — extraction-gaps lane.

### Migration — `SYMBOL_ID_SCHEME` and the `id_scheme` gate

`wicked_estate_extract::SYMBOL_ID_SCHEME = "2"`. `index_path_as` compares the per-repo
`id_scheme` meta key (absent = "1") and forces a full re-extraction on ANY previously-indexed
repo whose stored scheme differs — the binary version did not change, so the `indexed_version`
gate alone would skip every unchanged digest and leave a silently mixed graph. The key is written
only AFTER the re-extraction completes (unlike the version/rules gates, which write at the check
site): a crash or Ctrl-C mid-migration leaves the old key and the gate re-fires idempotently,
instead of stamping a scheme-2 DB whose rows are still v1.

Not carried over (documented loss, no guessed re-key): annotations on churned ids survive as
orphans under the old id; overlay/memory/knowledge xedges are epoch-dropped at read; embeddings
need `--embeddings` re-run; agent-held `--symbol` ids from `resolve --json` go stale. **Re-run
`wicked-estate scip <root>` after the forced re-extract** — `remove_file` deletes the
confidence-1.0 SCIP edges by file, so the forced pass removes them. SCIP *correlation* is
span-based and unaffected; nested ids give previously-merged members distinct spans — an
improvement.

**Downgrade hazard:** a pre-scheme binary of the SAME version writing into a scheme-2 DB re-mints
flat ids for any changed file — the old binary does not read the key, and the version gate cannot
fire at equal versions. Do not run pre-scheme binaries against a scheme-2 DB; if one did,
re-index with `--force`. The next release's version bump closes this via the `indexed_version`
gate.
