---
id: wicked-estate-adr-002
title: "Stable Symbol Identity"
status: active
date: 2026-06-12
---
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
containers all truncate the chain.

Anchor-artifact exception: a non-Type pending definition whose byte range EQUALS a Type-suffixed
pending definition's range is the same syntactic node re-captured by a second query pattern (e.g.
python.scm's ORM field patterns anchor `@code_field.def` at the WHOLE `class_definition`), not a
real inner scope — such records are dropped from the container walk before truncation applies.
Without the drop, the equal-range Term truncated the chain of every member of a nested ORM class
(re-minting the collision this amendment removes) and left the flat two-model case correct only
by uncontracted `QueryCursor::matches` order. Pinned by
`identity_orm_equal_range_anchor_nested_models_do_not_collide`,
`identity_orm_equal_range_anchor_keeps_full_type_chain`,
`identity_orm_two_flat_models_save_deterministic`.

Worked strings:

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

### Scheme 3 — query-level identity roles (`.anchor` / `.owner`)

Scheme 2 could only nest under an ENCLOSING, EMITTED definition. Scheme 3 (scm-anchors lane,
`docs/recon/scm-anchors.md`) adds two generic capture roles so query files can supply the owner
where scheme 2 structurally could not — zero per-language Rust (rules-as-data):

- **`@code_<kind>.anchor`** — a NON-EMITTING containment anchor: the captured container enters
  the chain walk as a Type container but mints no Node/DefRec/Contains edge. Consumers: Rust
  `impl_item` (methods nest under the impl's `type:` name — `impl Foo` and `impl Trait for Foo`
  both under `Foo#`), Ruby `class << self` (members nest as `C#self#m().`).
- **`@code_<kind>.owner`** — an owner TYPE NAME captured in the def's own match, spliced as the
  INNERMOST Type descriptor where no containing node exists. Consumers: Go receivers
  (`<module>/T#M().` — value/pointer/generic/parenthesized receivers share one shape), C++
  `Foo::`/`Foo<T>::` out-of-line qualifiers (`<module>/Foo#reset().`), Ruby `def self.m`
  (converges with `class << self` on `C#self#m().`).

Binding rule for query edits (R-DEF-LOSS): an owner/anchor capture added to an EXISTING def
pattern must be optional (`?`) so a shape outside the enumerated alternation degrades to an
OWNERLESS def, never a dropped def — a def may lose its owner, never its extraction.

Object-valued TS/JS class fields (`x = { save(){} }`, `#x = { … }`) are additionally captured as
Term-suffixed Field defs, so their literal members truncate at the field (module-flat) instead of
merging with the class's real methods.

### Stability semantics — two new rows

| Change | Identity | Why it is correct |
|---|---|---|
| Rename / move the enclosing type | **new identity for its members** | different logical path — same rule as a module move |
| Move a member between two types in one file | **new identity** | it is a different logical symbol |

### Accepted residuals (all fixture-pinned in `treesitter.rs` `identity_*` tests or
`tests/languages.rs` known_defect pins)

Closed by scheme 3 (their pins flipped in the scm-anchors lane): the object-literal-field merge
(`identity_field_object_literal_residual`, now asserting the split), the ORM equal-range
wrong-owner ids (`identity_field_orm_equal_range_residual`, now asserting real owners —
python.scm anchors `@code_field.def` at the field's own statement), and the Rust-impl /
Go-receiver / Ruby-singleton owner gaps (distinctness tests + flipped pins per language).

Still open:

- Function-local definitions (nested functions, arrows bound inside a method, object-literal
  methods) keep `<module>/name<suffix>` and still collide with each other and with a same-named
  module-level definition — smaller, visible residuals that were previously hidden inside the
  bigger merge.
- Object-literal field members pool PER-MODULE: the split moves `x = { save(){} }`'s member from
  the class merge to module-flat `src/m/save().`, shared with any same-named module-level
  function and with other literals' same-named members. Distinct ids need object-literal
  descriptors — a scheme change (pinned inside `identity_field_object_literal_residual`).
- TS/JS computed-name fields (`[k] = { … }`) stay uncaptured; their literal members still nest
  under the class and merge with same-named real methods (pinned, same test).
- Overloads within one type still collapse: `disambiguator` stays `None` (pinned by
  `identity_disambiguator_is_none`).
- Rust: two trait impls on ONE type merge (`Ta for Foo` / `Tb for Foo` both anchor under `Foo#`
  — `rust_same_type_trait_impls_collision_known_defect`); unanchorable impl targets (`&Foo`,
  tuples, `dyn Trait`) match no anchor branch and their methods stay module-flat.
- Ruby: `def Foo.m` / `def obj.m` keep OWNERLESS defs (nested under the enclosing class by
  containment), so `def Foo.m` still merges with instance `def m` — an owner splice for constant
  receivers needs a program-recorded convention (pinned inside
  `ruby_singleton_vs_instance_collision_known_defect`). `class << Foo` reopens `Foo#`'s
  namespace (unchanged).
- C++: decltype/dependent_name-scoped out-of-line members keep OWNERLESS module-flat defs
  (pinned); multi-level qualification (`Ns::Foo::bar` at file scope) is unmatched.
  ~~The member proto/def CROSS-FILE single-id hazard~~ — **RESOLVED by the M4 decision
  (Option A, third amendment below)**: one id across two files is now the recorded
  convention, made safe by the store's multi-file contribution table (wicked-estate#152);
  the former hazard pin (`cpp_member_proto_def_cross_file_single_id_hazard`) is retired
  into a convention regression guard + the store conformance suite
  (`multi_file_contribution_suite`).
- C++, namespace direction of the qualifier ambiguity (review round 2, R2-COR-1): the
  GRAMMAR ambiguity is unchanged — `qualified_identifier.scope` parses class and namespace
  qualifiers identically (`namespace_identifier`), so `void ns::helper(int) {}` at file
  scope still mints kind **Method** on the id an in-namespace `void helper() {}` definition
  mints with kind **Function**. Under the M4 convention (third amendment below) the shared
  id is CORRECT (one logical symbol) and the store derives one deterministic primary kind
  from the preferred contribution, so the re-kind FLAP is dead; the raw extraction stream
  keeps both kinds (pinned, flipped:
  `cpp_namespace_qualified_free_fn_cross_kind_collision_known_defect`). The remaining wrong
  bit is the OVERLOAD collapse feeding it — `helper()` vs `helper(int)` are different
  functions sharing one id because `disambiguator` stays `None` (see the overload residual
  above; a scheme change).
- C/C++ cross-grammar seam (named by the M4 record, D6d): `.h` routes to the C++ grammar
  (`ts-cpp` ids) while `.c` routes to the C grammar (`ts-c` ids), so a C header prototype
  and its `.c` definition mint TWO nodes — the declared C API surface is visible (D6d), but
  it does not unify with the `ts-c` definition. Unification is an id-shape change (churns
  every C symbol id) and is explicitly OUT of Option A's zero-churn bounds; pinned
  (`header_plus_c_stays_two_nodes_cross_grammar_residual`,
  `crates/wicked-estate/tests/free_proto_emission.rs`).
- C++ free-prototype emission gaps (D6d, all recorded in `cpp.scm`'s pattern comment):
  a most-vexing-parse declaration at TU/namespace scope matches (it IS a function
  declaration per [dcl.ambig.res]; body-local MVP is guarded by the per-parent anchoring);
  a body-local prototype inside a preproc block inside a function body leaks through the
  preproc parents; un-braced `extern "C" int f(int);` (parent `linkage_specification`) and
  pointer-returning prototypes (`int* getPtr();`) are not captured — the latter consistent
  with the function_definition patterns' identical shape gap.
- Fleet-audit hand-off (merge note M6): Swift `extension Foo` members are module-flat and two
  extensions' same-named methods collide (pinned,
  `swift_extension_methods_collision_known_defect` — fixable per-language with `.anchor`); Lua
  colon-methods (`function Obj:method()`) do not capture the receiver `Obj`; Kotlin anonymous
  `companion object` is not a container (its members nest under the class).

### Migration — `SYMBOL_ID_SCHEME` and the `id_scheme` gate

`wicked_estate_extract::SYMBOL_ID_SCHEME = "3"` (bumped from "2" by the scm-anchors lane in the
same commit as its first id-changing query edit; scheme 2 never shipped in a release, so released
users migrate once for both). `index_path_as` compares the per-repo
`id_scheme` meta key (absent = "1") and forces a full re-extraction on ANY previously-indexed
repo whose stored scheme differs. The gate fires PER REPO LABEL on that label's next index — a
multi-repo DB holds mixed schemes until every label re-indexes, and the mismatch warning is
CLI-only (the MCP server surfaces nothing) — the binary version did not change, so the `indexed_version`
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
improvement. **Re-inject overlay xedges / re-run the bus/command injection pipelines** after the
forced re-extract — injected edges keyed on old ids are not carried over, and they typically land
on handler METHODS, exactly the Rust-impl/Go-receiver id classes scheme 3 churns.

**Downgrade hazard:** a pre-scheme binary of the SAME version writing into a scheme-2 DB re-mints
flat ids for any changed file — the old binary does not read the key, and the version gate cannot
fire at equal versions. Do not run pre-scheme binaries against a scheme-2 DB; if one did,
re-index with `--force`. The next release's version bump closes this via the `indexed_version`
gate.

## Amendment 2026-09 — M4: header/impl proto+def identity = Option A (one logical symbol; NO scheme bump)

The M4 decision the 0.15.0 release deferred (scm-anchors D8; deferral terms in
`docs/recon/extraction-gaps.md` §D6(d)) is recorded: **a C/C++ prototype and its definition are
ONE logical symbol** — Option A of `scratch/proposals/ESTATE-M4-DECISION-BRIEF.md` (workspace
repo). The fix is store-side, not identity-side: identity from the logical name path (this ADR's
founding rule) was never the broken layer.

### The decision

- **Identity (unchanged — zero id churn, `SYMBOL_ID_SCHEME` stays "3"):** a header prototype and
  its impl-file definition mint ONE SymbolId (`module_path` strips one extension; `foo.h` and
  `foo.cpp` share module `foo`). D6d free prototypes (wicked-estate#140) JOIN the id their
  definitions already mint — no existing id changes shape. A header-only prototype with no
  definition mints the id alone, as a declaration-primary node.
- **Store (wicked-estate#152):** nodes gain per-`(symbol, file)` CONTRIBUTION rows
  (`node_files`); the `nodes` row is a DERIVED projection of the preferred contribution —
  **definition before declaration** (`metadata.is_declaration`, set by the extractor's
  `@code_<kind>.decl` capture role), lexicographic file tiebreak — never last-write-wins.
  `remove_file` retires contributions and re-homes survivors; only a node losing its LAST
  contribution is deleted. This kills all three F7 mechanisms (file/kind flap, cross-file
  delete, digest-skip data loss). Conformance: `multi_file_contribution_suite`, every backend.
- **Kind convention (the R2-COR-1 fold):** the node's primary kind is the preferred
  contribution's kind. The raw extraction stream may legitimately carry conflicting kinds for
  one id (the class-vs-namespace qualifier grammar ambiguity has no query-level fix); the store
  reconciles deterministically, so the #129 re-kind-flap class stays dead.

### Why A over distinct-decl identity (B)

Recorded in full in the decision brief; the deciding asymmetries: B is a scheme-4 bump (full
ADR-002 migration fallout for a C++-only shape change) that puts the tree-sitter tier in
disagreement with the SCIP tier's identity for the same function, and both shipped resolvers
skip ties — distinct decl ids would park most C/C++ call resolution as unresolved.

### Evidence (S11 prevalence, recorded on wicked-estate#140)

Bench-pinned `tree-sitter/tree-sitter` corpus, pre-store extraction stream: **today** 6,648
distinct SymbolIds, 103 (1.55%) multi-file — every one `Import`-kind (the class the store
already re-homed). **Forward** (D6d landed, no store fix): a name-level proxy finds ~230
colliding ids (~3.5%), 205 of them the `ts_*` public C API — the entire public surface of a
typical C library, which is why the store fix landed FIRST (#152) and emission second (#140).

### Explicitly still open (do NOT read this amendment as fixing them)

- **The overload disambiguator.** Overloads within one type/scope still collapse into one id —
  `helper()` and `helper(int)` share `…/ns#helper().` — because `disambiguator` stays `None`
  (pinned: `identity_disambiguator_is_none`). Fixing it (parameter-type hash) is a SCHEME
  CHANGE with full migration fallout, and it is the true fix under the R2-COR-1 cross-kind
  pin's fixture. M4/Option A did not touch it.
- **The C/C++ cross-grammar seam** (`.h`→ts-cpp vs `.c`→ts-c: no unification; see the residual
  bullet above).
- The D6d emission-shape residuals recorded in `cpp.scm` (TU-scope most-vexing-parse,
  preproc-inside-body leak, un-braced `extern "C"`, pointer-returning prototypes).
