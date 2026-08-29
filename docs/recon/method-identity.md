# Recon + plan: type-nested definition identity (engine defect #1)

Lane: `lane/method-identity` (base d7d3b58). Resolves review findings **D03-1, D03-2, PER-7**
(`estate-review/REVIEW-adversarial-2026-08-28.md`, "Doc 03 — receiver-type inference";
`review-artifacts/findings.json`). All file:line citations are against the lane worktree at
d7d3b58 unless another root is named. Every line cited below was opened in this session.

**Revision 2 (post-attack).** All six major attack issues are accepted and resolved in place —
none rejected: MI-ATK-1/MI-A1 (chain rule truncation, D1/S0/S1), MI-ATK-2/BR-1 (id_scheme write
moved post-extraction, D7/S2), BR-2 (gate predicate keys off `previously_indexed`, D7/S2),
BR-3 (S4 acceptance restated — dependents shrink on the fixture, by design), MI-A2 (pass-2
insertion pinned before the COBOL fixup, D2/S1). Minor issues MI-ATK-3/4, BR-4/5, MI-A3/4/5/6
are folded in where they land. §7 maps issue id → resolution.

## 1. Findings acted on

| Id | Claim → verdict | Engine evidence (opened) |
|---|---|---|
| D03-1 (audit, attack, periphery PER-7) | "`from` is `…Repo#update().`" → the id shape does not exist | `crates/wicked-estate-extract/src/treesitter.rs:1874-1885`: `Symbol::global(&scheme, None, vec![Descriptor::new(module, Namespace), Descriptor{name, suffix: def_suffix(anchor_kind), disambiguator: None}])`. Exactly two descriptors; the enclosing class is never pushed. `def_suffix` at `:1341-1348`. |
| D03-2 | "every resolver correctly parks on ambiguity" → false within a file: same-named defs collapse into ONE node and `this.save()` resolves to the merged node at 0.65 | `crates/wicked-estate-store/src/sqlite.rs:387-394` `INSERT INTO nodes … ON CONFLICT(symbol) DO UPDATE SET name, kind, language, file, data, scope` (last write wins). Reproduced: `scratchpad/doc03/g.db` — 7 nodes, one `ts-typescript . . . src/fixture/save().` for `Repo.save` + `Cache.save`; `flush().→save().` at 0.65 `scoped-name-resolver`. |
| PER-7 | Contains edges are File→def only; `from` = innermost enclosing def carrying no class | `treesitter.rs:2049-2062` (File→def Contains, "Wave 2.6 (Fix A)"); `enclosing()` at `:1521-1526` = smallest `DefRec` by byte range. |

Root cause in one sentence: the extractor mints a definition's id from `[module/, name<suffix>]`
only, so identity is *file-scoped by name*, not *path-scoped*, and the store's idempotent upsert
turns every same-named definition in one module into a merge.

### Scope of the collision (measured with the BEFORE binary, recon `risks` lens)
`scratchpad/measure/recon-fx/recon.db`: 5 TS `save` defs (2 classes, 1 interface, 1 object
literal, 1 decorated class) → ONE node; Rust `impl A { fn new }` + `impl B { fn new }` → ONE
node (kind `function`, because `rust.scm:3-7` and `:24-30` both match the same `function_item`);
Go `(a *A) M` + `(b *B) M` → ONE node; Python `Outer.run` / `Outer.Inner.run` / function-local
`run` → ONE node. The defect class is fleet-wide (§11); the fix lands at the generic seam and
covers every language whose query exposes a Type-suffixed owner anchor.

### Every place a definition id is minted (brief scope item; attack MI-ATK-4/MI-A6/BR-5c)
The "one constructor" claim below is scoped to the **tree-sitter seam**. Full enumeration:
- **In scope** — `treesitter.rs` def block (`:1874-1885`) + the four framework emitters
  (`:2150-2158`, `:2184-2192`, `:2209-2216`, `:2249-2257`) + the two Java test helpers. After S1,
  `def_symbol`/`def_symbol_at` are the only constructors **on this seam**.
- **Enumerated, deliberately unchanged** — `clips.rs:66` (`clips_sym`, used at `:360`, `:404`
  in tests): `Symbol::global` with the same 2-descriptor `[Namespace(module), name+suffix]` shape,
  scheme `"clips"`. CLIPS constructs (defrule/deffunction/deftemplate) are module-scoped with no
  type-membership concept, so the flat shape is correct there; they do NOT adopt
  `SYMBOL_ID_SCHEME` semantics, and the id_scheme gate's forced re-extract re-mints byte-identical
  ids for those files (a no-op-safe rewrite).
- **Out of scope, synthetic ids (not code-def path ids)** — `xml_rules.rs:221`, `extra_edge.rs:290`,
  and the grammarless rules/mainframe extractors (`blaze_brl`, `cics_sql`, `corticon`, `drl`,
  `excel_rules`, `hlasm`, `ims`, `jcl`, `json_rules`, `mq`, `odm`, `racf`, `rego_rules`) all mint
  `Symbol::synthetic` only.
- **Out of scope, IaC** — `tfstate.rs:252` (synthetic) and `:259` (`Symbol::global`, IaC ids —
  brief excludes them; the fn is `#[allow(dead_code)]`-adjacent collector code).
- **Grammarless File path** — `wicked-estate/src/lib.rs:147-171` mints File nodes + Contains only;
  no code-def ids (covered in D4).

### History (§4)
- The two-descriptor scheme is original, undiscussed code from 797ce58 (2026-06-14); no ADR or
  design note argued for it (`git log -S'fn def_suffix'` → 797ce58 only).
- ADR-002 (`docs/adr/ADR-002-stable-symbol-identity.md`, Decision) already defines identity as
  the "logical name path" and `descriptors` as "a path of (name, Suffix)" with Type `#` as a path
  component. Nesting is an amendment of shipped *practice*, not a reversal of the decision.
- The scheme was re-affirmed once, consciously, in `docs/recon/java-spring-framework-edges.md:37-41`
  ("3 descriptors … WRONG for this codebase … I match the real 2-descriptor scheme"). Consequence:
  four sibling minting sites and two test helpers restate the flat scheme (§3, step S1).
- Id reuse is already treated as a hazard: `symbol_epoch` (4e39c94) exists so cross-store
  consumers fail closed when "the same stable SymbolId" resolves to a different live node.
- `#117` (42a1040) is the precedent for an identity change: one choke point, "No schema
  migration", per-repo meta keys (`repo_scope::meta_key`, `crates/wicked-estate/src/repo_scope.rs:109-114`).

## 2. Decisions (brief decision points 1–6)

### D1 — Nest under the **contiguous run** of enclosing Type-suffixed definitions; ANY non-Type enclosing definition truncates the chain. (Revised per MI-ATK-1/MI-A1.)
New shape: `[module/] ++ [T1#, T2#, …] ++ [name<suffix>]`. The chain is computed from the def's
**containing definitions of every kind** — not a Type-only anchor list. Precise rule: take all
other pending defs whose byte range strictly contains the def's range (contains AND not
range-equal; self excluded; deduped by `(start, end, name)`), walk them **innermost → outermost**,
and collect a `T#` descriptor while `def_suffix(kind) == Suffix::Type`
(`class|struct|enum|trait|interface|module|namespace|type_alias|type`, `treesitter.rs:1341-1348`);
**STOP at the first container that is not Type-suffixed** — Method, Function, and Term containers
all truncate. Reverse the collected run to outer→inner. The chain therefore is exactly the Type
anchors *immediately* enclosing the def; anything declared inside a function/method/Term body
takes no Type descriptors. The chain applies to **every** definition kind (methods, constructors,
fields, enum members, interface method signatures, nested types).

The previous rule ("all strictly-containing Type anchors") was wrong on its own test 3: `class
Cache` strictly contains a method-local `const cb = () => …` (`typescript.scm:13-16` matches
`variable_declarator`+arrow anywhere), so it would have minted `Cache#cb().`; worse, a method-local
`const save = () => …` would have minted `Cache#save().` — the SAME id as the `Cache.save` method,
re-creating D03-2's merge (`sqlite.rs:387-394`) one level down. The truncation rule fixes both.

Worked cases (each fixture-pinned in S0):
- `Repo.save` — containers: `Repo` (Type) → `src/…/Repo#save().`
- `Outer.Inner.run` (Python) — containers innermost→out: `Inner` (Type), `Outer` (Type),
  contiguous → `Outer#Inner#run().`
- `cb` (`const cb = () => …` inside `Cache.flush`) — innermost container `flush()` (Method) →
  STOP → flat `…/cb().`
- method-local `const save = () => …` inside `Cache.flush` — flat `…/save().`, NOT `Cache#save().`
- `lit.save` (object-literal method in `export const lit = {…}`) — innermost container `lit.`
  (Term; the top-level const patterns, `typescript.scm:93-103`) → STOP → flat. **Term-suffixed
  containers truncate exactly like Method/Function ones** (MI-ATK-1's explicit question).
- `export const foo = () => {}` — fn def `foo().` strictly inside Term def `foo.` → STOP → flat
  (the `foo.foo().` false structure is still avoided).

**Known residual, pinned by S0 test 7:** `class A { save(){} x = { save(){} } }`. The field `x`
holding an object literal is NOT captured as a def (`typescript.scm:28-31` requires an
`arrow_function` value; the const patterns are top-level-anchored), so the walk cannot see it:
the inner `save`'s innermost containing def is `class A`, it mints `A#save().` and MERGES with the
real `A.save()` method. This is D03-2's failure mode surviving in one narrow shape, invisible to a
def-based walk without a query change. Recorded as a merge note to the extraction-gaps lane
(capture object-valued `public_field_definition` as a Term def; the truncation rule then fixes it
with zero Rust change) and pinned by `identity_field_object_literal_residual` so the later fix is
a conscious test update, not silence.

Why not "every enclosing def":
- ADR-002's rename-stability argument: a function rename must not churn ids of things declared
  inside it; nesting under functions multiplies churn beyond "rename = new identity" for the
  renamed symbol itself.
- SCIP's model makes function-local symbols `local N`, never path-nested. The spine has that
  variant (`Symbol::Local`, `crates/wicked-estate-core/src/symbol.rs:87-92`) and **zero uses** —
  adopting it is a bigger change than this lane and changes NodeKind counts/`resolve` semantics.
- Measured false structure with the BEFORE binary: TS `export const foo = () => {}` mints TWO
  overlapping DefRecs, `foo().` (`typescript.scm:12-16`, `variable_declarator`) inside `foo.`
  (`typescript.scm:99-103`, `lexical_declaration`). Nesting under Term defs would mint
  `foo.foo().`; nesting an object-literal method under its const would mint `lit.save().` (`lit`
  is kind `constant`, Term-suffixed — `recon.db`: `src/a/lit.|constant`).

Accepted residual (documented in the ADR amendment and pinned by the fixture tests): function-local
definitions (nested functions, arrows bound inside a method, object-literal methods) keep
`<module>/name<suffix>` and still collide with each other and with a same-named module-level
definition — in the S0 fixture, `lit.save` and the method-local `save` arrow merge into ONE flat
`…/save().` node (pinned by test 3). Plus the class-field object-literal shape above (test 7).
These are smaller, visible residuals — today they are hidden inside the bigger merge.

### D2 — Two-pass minting; no reliance on match order.
`tree-sitter 0.25.10` (`Cargo.lock:9000-9001`), `QueryCursor::matches` doc at
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tree-sitter-0.25.10/binding_rust/lib.rs:3052-3057`:
"one match may contain captures that appear *before* some of the captures from a previous match."
Document order is therefore not a contract. Today the def loop mints and pushes `DefRec` in the
same match iteration (`treesitter.rs:1866-1905`); the enclosing chain is unknowable there for a
one-pass design in general (e.g. Python methods come from a `class_definition`-rooted pattern,
`python.scm:15-22`, while the class comes from another pattern, `python.scm:3-7` — different
pattern indices, same start byte for the first method's class).

Design: pass 1 (inside the match loop) collects `PendingDef { kind, name, start, end, span,
signature }` — no id. Pass 2 mints every id from the **full** PendingDef list (all kinds — the
truncation walk in D1 needs the non-Type containers too, not a Type-only anchor list), **deduped
by `(start, end, name)`** (a decorated TS class matches two `@code_class.def` patterns,
`typescript.scm:43-45` and `:135-141`, producing two identical records; a Rust `function_item`
matches both `:3-7` and `:24-30`), via `def_symbol(scheme, module, chain, name, suffix)`, and
pushes `Node` + `DefRec`. Cost is O(defs²) per file worst case; worst observed defs/file is 208
(studio) — negligible against a 1.0 s full crew index.

**Insertion point (pinned; attack MI-A2):** pass 2 lands **between the match-loop close
(`treesitter.rs:2010`) and the COBOL paragraph span fixup (`:2012-2036`)** — the fixup iterates
`def_nodes`, which pass 2 now populates; placing pass 2 after it would silently no-op the COBOL
span extension (and `smoke_cobol` at `:4598-4612` asserts only a def count, so the regression
would ship green). `PendingDef`/`DefRec` keep **pre-fixup** byte ranges — identical to today's
`enclosing()` inputs (`:1521-1526`) — and the fixup keeps mutating only `node.location.span`
afterwards. S1 adds a one-line span assertion to `smoke_cobol` (a paragraph node's span end
exceeds its header line) so the ordering stays pinned by a test.

`enclosing()` (`:1521-1526`) is already only called after the loop (`:2131`, `:2272`); it keeps
working unchanged on the pass-2 `defs`.

### D3 — Generic seam only; languages whose owner is not a def anchor are a merge note.
The chain is computed purely from `DefRec` byte ranges + `def_suffix`; no `match lang`. Query
files decide what is an anchor:
- Fixed by this lane (owner IS a Type-suffixed def spanning its body): TS/JS classes and
  interfaces (`typescript.scm:43-45`, `:34-38`, interface body), Python classes and nested
  classes (`python.scm:3-7`, anchor = whole `class_definition`), Java classes/enums
  (`java.scm:3-13`), Ruby `class`/`singleton_class` with a constant value (`ruby.scm:3-13`),
  Ruby `module` (`ruby.scm:15-19`), Elixir `defmodule` (`elixir.scm:6-10`, the `call` node spans
  the do-block).
- NOT fixed (no owner anchor exists): Rust `impl_item` (`rust.scm:24-30` captures only the inner
  `function_item`; `struct A` at `:9-12` spans one line), Go receiver methods (`go.scm:11-17`, no
  receiver capture), Ruby `class << self` (`singleton_class value: (constant)` does not match
  `self`). These stay colliding after this lane. Recorded in §6 merge notes for the
  extraction-gaps lane; no per-language Rust arm here (Universal Don't "Rules as DATA").
- `type_alias`/`type` are in the Type set but never contain members in any wired grammar; harmless.

### D4 — Contains stays File→def. No Class→Method Contains edge.
`crates/wicked-estate-core/src/query.rs:41-53`: `TraversalSpec::blast_radius` follows **all** edge
kinds (`edge_kinds: vec![]`) in `Direction::Dependents`; the recon measured the File node already
listed as a dependent of `save` (`measure/before-doc03.db`, `blast-radius save --json`). A
Class→Method edge would make every method's blast radius include its class and, at depth ≥ 2,
everything that depends on the class — a user-visible semantic change for crew/studio/MCP with no
consumer that reads it (rank ignores Contains, `wicked-estate-rank/src/lib.rs:543-570`;
graph-view filters Calls|Imports). The grammarless path (`crates/wicked-estate/src/lib.rs:147-171`)
keeps the same File→node shape. The nested id itself now carries ownership; if a later lane needs
the owning type it reads the id's Type descriptors (structured `Symbol`, not string parsing).
`from` for refs stays `enclosing()`; heritage refs from a nested class now carry the nested id.

### D5 — SCIP is untouched; annotation/xedge/embedding links to churned ids are orphaned and documented, not migrated.
- `crates/wicked-estate-resolve/src/lib.rs:941-971`: SCIP occurrences correlate by document path +
  containing span (smallest node). ADR-002's "normalize SCIP symbols on ingest" was never built
  (`grep -n normaliz resolve/src/lib.rs` → 0). After nesting, `Repo.save` and `Cache.save` are two
  nodes with distinct spans, so SCIP's def map yields distinct targets — an improvement.
  `tests/scip_edges.rs:25-52` mints its own `ci-test` ids; unaffected.
- `by_name` is `Node.name` (`crates/wicked-estate/src/lib.rs:256-268`; `sqlite.rs:2055-2058`
  `WHERE name=?1`); `blast-radius <name>` is unaffected except for multiplicity (N hits instead
  of one merged hit).
- Annotations key on `symbols.sid` (`schema.sql:181-195`); `remove_file` deletes
  nodes/edges/unresolved_refs/files only (`sqlite.rs:1741-1755`) → annotations on old method ids
  survive as orphans. Overlay xedges are epoch-validated and drop silently (`xedge.rs:238-239`);
  embeddings for re-extracted files are deleted and only recomputed with `--embeddings`.
  Decision: **no re-key**. A (file,name,kind)→new-id re-key would guess for exactly the
  collided groups this lane splits (which of N `save`s owned the annotation?) — guessing is worse
  than an honest orphan. The migration note + the CLI warning text (which today promises "your
  annotations are preserved", `main.rs:121-127`) are corrected instead.

### D6 — Disambiguator stays `None`.
Unchanged (`treesitter.rs:1882`); overloads/declaration signatures inside one type still
collapse (last write wins). Pinned by an assertion so a later change is deliberate.

### D7 — Stored-graph versioning: a per-repo `id_scheme` meta key gates a forced full re-extract.
Both the BEFORE release binary and this worktree are **0.14.6** (`Cargo.toml:21`;
`target/release/wicked-estate --version`; `baseline/studio.db` meta `indexed_version=0.14.6`), so
the only existing full-re-extract gate (`crates/wicked-estate/src/lib.rs:556-568`, `force_full`
iff `indexed_version != CARGO_PKG_VERSION`) does NOT fire, and unchanged files are skipped
(`lib.rs:702-707`) → a silently mixed-id graph. Bumping the crate version is release-managed
(publish/tag jobs, plugin manifests) and would not protect the next scheme change. The
extra-rules digest gate (`lib.rs:574-583`) is the exact precedent: a per-repo key compared on
every `index_path_as` entry (also reached by `watch`, `main.rs:2043,2075`).

Rule: `SYMBOL_ID_SCHEME = "2"` (constant next to `def_suffix`, exported from the extract crate).

**Gate predicate (revised per BR-2):** the gate fires when `!previously_indexed.is_empty()` —
the label-scoped set already computed at `lib.rs:538-545`, exactly the per-repo semantics the key
needs — AND the stored `repo_scope::meta_key(repo, "id_scheme")` != `"2"` (absent counts as
`"1"`). NOT `prev_version.is_some()`: pre-version DBs (indexed before a81317d; the population
`main.rs:133-135` explicitly handles with `None => return`) have nodes + digests but no
`indexed_version` key — keying on the version key would skip them, leave their v1 ids alive
behind unchanged digests, and (under the old "always write" plan) stamp them scheme-2: silently
stale AND certified current. `prev_version` is not consulted at all; the eprintln reads the old
scheme value for its wording: `SYMBOL-ID SCHEME changed (v<old> → v2): forcing full
re-extraction of '<label|root>'`.

**Key write placement (revised per MI-ATK-2/BR-1):** the key is NOT written at the check site.
The precedents (`indexed_version` write at `lib.rs:561`, `extra_rules_digest` at `:583`) commit
their meta writes BEFORE extraction runs; copying that for `id_scheme` defeats the gate — a
crash/Ctrl-C between the write and the completed re-extract (collect + parallel read/digest of a
studio/crew-sized corpus is a seconds-to-minutes window, `lib.rs:595-707`) leaves a DB stamped
scheme-2 whose rows are still v1, and every later index skips them via unchanged digests
(`lib.rs:703-707`) — permanently mixed, the exact state the brief forbids. Instead the key is
written in exactly two places:
1. at the **end** of `index_path_as`, next to `prune_dangling_edges` (`lib.rs:973-982`), before
   `store.stats()` (`:990`) — after the forced re-extraction completed;
2. on the `changed.is_empty()` early return (`lib.rs:712-713`), **only when the gate did not fire
   this run** (a fresh DB or an already-scheme-2 DB with no file changes gets the key stamped; a
   gate-fired run that early-returns — a repo with zero source files — leaves the key old, so the
   gate re-fires next time, which is idempotent and correct).
A crash anywhere mid-run leaves the key at its old value → the next run re-fires the gate →
`force_full` again — idempotent. The pre-existing `indexed_version`/`extra_rules_digest` write
positions are NOT relocated: they carry the same latent ordering flaw, but their stale rows share
one id scheme (tolerable), and relocating them is out of this lane's acceptance criteria.

Mechanics unchanged: `force_full` routes every file through `remove_file` (`lib.rs:719-724`),
which deletes nodes by file regardless of id → no stale old-id nodes survive; the existing
deleted-file sweep (`lib.rs:683-696`, which runs before the changed/unchanged split) covers files
gone from disk. Multi-repo graphs re-extract each label the next time it is indexed;
`maybe_warn_version_mismatch` (`main.rs:107-135`) gains the same per-repo check so an operator is
told which labels still hold scheme-1 ids.

**Downgrade hazard (MI-A5/BR-5, documented not solved):** no gate written by the new binary can
stop a pre-scheme 0.14.6 binary from writing flat ids into a scheme-2 DB — the old binary does
not know the key, and the `indexed_version` gate cannot fire at equal versions (`lib.rs:558-560`).
The ADR amendment + CHANGELOG migration note say: do not run pre-scheme binaries against a
scheme-2 DB; if one did, re-index with `--force`. The next release's version bump closes this via
the `indexed_version` gate; only the NEW binary can carry the warning.

## 3. Step list

Order is fixed: S0 fixture/test first (red), S1 seam (green), S2 versioning (green), S3 docs,
S4 measurements. Per-crate cargo only; `CARGO_TARGET_DIR` = the lane target dir.

### S0 — Characterisation fixture + failing tests (extract crate)
Files: `crates/wicked-estate-extract/src/treesitter.rs` (unit tests module),
`crates/wicked-estate-extract/tests/fixtures/typescript/method_identity.ts` (new).
Fixture (the doc03 shape + the residual):
```ts
export class Repo { save(): void {} update(): void { this.save(); other.save(); } }
export class Cache {
  save(): void {}
  flush(): void { this.save(); const cb = () => this.save(); const save = () => {}; save(); }
}
export interface Store { save(): void; }
export const lit = { save() {}, run() { this.save(); } };
export function top() { const r = new Repo(); r.update(); }
```
(The method-local `const save = () => {}` is the MI-A1 collision input: under the unrevised rule
it would have minted `Cache#save().`, merging with the method.)
Type-under-Type is exercised inline in Python (`typescript.scm` has no `namespace` anchor —
`grep -n namespace queries/typescript.scm` → 0; `python.scm:3-7` anchors the whole
`class_definition`, so nested classes nest):
```python
class Outer:
    class Inner:
        def run(self): pass
    def run(self): pass
def top():
    def run(): pass
```
Tests (all in `treesitter.rs` `mod tests`, names final):
1. `identity_nests_methods_under_enclosing_type` — the three class/interface `save` defs have three
   DISTINCT symbols; `Repo.save` renders exactly
   `ts-typescript . . . src/method_identity/Repo#save().`; `Cache#save().`, `Store#save().`;
   class ids unchanged (`src/method_identity/Repo#`). Python half: `Outer.run` →
   `ts-python . . . <module>/Outer#run().`, `Outer.Inner.run` → `<module>/Outer#Inner#run().`,
   the inner class → `<module>/Outer#Inner#`, and the function-local `run` inside `top` stays
   `<module>/run().` (D1 residual).
2. `identity_refs_from_carry_the_type` — the Calls ref `this.save()` inside `Repo.update` has
   `from == …/Repo#update().`; inside `Cache.flush` `from == …/Cache#flush().`.
3. `identity_does_not_nest_under_functions_or_terms` — `cb` is `…/cb().` (flat, NOT `Cache#cb().`
   and NOT `Cache#flush().cb().` — the truncation rule, MI-ATK-1); the method-local `save` arrow
   does NOT mint `Cache#save().` (the MI-A1 collision input) — it is flat `…/save().` and MERGES
   with `lit.save`'s flat `…/save().` into one node (the documented D1 residual: assert exactly
   ONE flat `…/save().` node, and that `Cache#save().`'s span is the method's, not the arrow's);
   `lit.save` flat (Term container truncates); `top` is `…/top().`.
4. `identity_disambiguator_is_none` — every Method-suffixed def has `disambiguator == None`
   (pin D6 via the rendered string `().`).
5. `identity_contains_edges_stay_file_to_def` — every Contains edge has `source == file symbol`
   and the count equals the number of def nodes (D4 pinned; today only "any Contains" is asserted
   at `treesitter.rs:2794`).
6. `identity_dedupes_duplicate_anchors` — a `@Entity`-decorated class (two `@code_class.def`
   matches) yields `Ent#save().`, not `Ent#Ent#save().`.
7. `identity_field_object_literal_residual` (inline TS source, separate from the fixture file so
   falsifier 2's counts stay clean) — `class A { save(){} x = { save(){} } }` yields exactly ONE
   `…/A#save().` node: the field-object-literal method nests under the class and merges with the
   real method, because `x` is not captured as a def (`typescript.scm:28-31` requires an arrow
   value). Characterizes the MI-ATK-1 residual; when the extraction-gaps lane captures
   object-valued fields as Term defs this test MUST be consciously updated to assert the split.
Proof step: `cargo test -p wicked-estate-extract identity_` → tests 1, 2, 6, 7 FAIL at d7d3b58
(3, 4, 5 pass — they pin today's behaviour that must survive; test 3's flat assertions hold today
because EVERYTHING is flat today, and gain their force once S1 nests the members). Record the
exact output.
Deletes: nothing.

### S1 — The seam: two-pass mint + one `def_symbol` helper + sibling sites
Files: `crates/wicked-estate-extract/src/treesitter.rs` only (hunks confined to `:1341-1376`
helpers, `:1740-1760` collector declarations, `:1866-1905` def block, new pass-2 block inserted
**between the match-loop close (`:2010`) and the COBOL fixup (`:2012-2036`)** — pinned per MI-A2,
see D2 — and `:2146-2260` framework emitters). `crates/wicked-estate-extract/src/lib.rs:13`
re-export of the constant (one line).
Change:
- `pub const SYMBOL_ID_SCHEME: &str = "2";` with a doc comment pointing at the ADR-002 amendment.
- `DefRec` gains `name: String, suffix: Suffix` (needed for self-exclusion and lookup).
- `struct PendingDef { kind: &'static str, name: String, start: usize, end: usize, span: Span, signature: Option<String> }`.
- `fn enclosing_chain(pending: &[PendingDef], start, end) -> Vec<Descriptor>` — takes the **full**
  PendingDef list (all kinds; revised per MI-A1 — a Type-only anchor list cannot implement the
  truncation): containers = pending defs with `p.start <= start && end <= p.end &&
  !(p.start == start && p.end == end)`, deduped by `(start, end, name)`, sorted innermost-first
  (`start` desc, `end` asc); then `take_while(def_suffix(kind) == Suffix::Type)`, reversed to
  outer→inner, as `Descriptor::new(name, Suffix::Type)`. The first non-Type container ends the
  chain (D1).
- `fn def_symbol(scheme, module, chain: &[Descriptor], name, suffix) -> SymbolId` — the ONLY place
  a tree-sitter code-definition id is built (see §1's mint-site enumeration for the deliberately
  unchanged grammarless sites): `[Namespace(module)] ++ chain ++ [Descriptor{name, suffix, disambiguator: None}]`.
- The def block at `:1866-1905` becomes "push `PendingDef`" (no `Symbol::global`, no `Node`).
- Pass 2 (at the pinned insertion point, BEFORE the COBOL fixup so the fixup still sees populated
  `def_nodes`): mint via `enclosing_chain` + `def_symbol`, build `Node` (same fields as today:
  kind via `def_nodekind`, signature, location — **pre-fixup** byte ranges), push `DefRec`.
- `smoke_cobol` (`:4598-4612`) gains one span assertion (a paragraph node's span end exceeds its
  header line) so pass-2-before-fixup stays pinned by a test (MI-A2).
- Sibling sites: `di_pairs`, `route_triples`, `event_listen_type_triples`,
  `event_listen_topic_triples` carry the **byte position of the captured name node**
  (`c.node.start_byte()` is already computed as `pos` at `:1801`). Emission uses
  `fn def_symbol_at(defs: &[DefRec], pos, name) -> Option<SymbolId>` = smallest `DefRec`
  containing `pos` whose `name == name` (the method/class def's range contains its own name
  identifier); fallback when the language's def query did not capture that def:
  `def_symbol(scheme, module, enclosing_chain(pending, pos, pos) minus any leading descriptor with
  the same name, name, suffix)` — same dangling behaviour class as today, no silent drop.
- `event_emit_topic_sites` already uses `enclosing()` (`:2267`) — unchanged.
Tests: S0 tests 1–6 green; `cargo test -p wicked-estate-extract` (347 unit + 96 integration at
d7d3b58 — the 6 Java framework tests will FAIL until S1b); `cargo clippy -p wicked-estate-extract
--all-targets -- -D warnings` 0 warnings; `cargo fmt -p wicked-estate-extract`.
Deletes: the in-loop `Symbol::global(...)` at `:1874-1885`; the four hand-built
`Symbol::global` constructions at `:2150-2158` (DI), `:2184-2192` (route handler),
`:2209-2216` (event-listens type), `:2249-2257` (event-listens topic); the "same 2-descriptor
scheme" comments at `:1743-1744`, `:2147-2149`, `:2181-2183`, `:2206-2208`.

### S1b — Java framework tests prove the graph joins
Files: `crates/wicked-estate-extract/src/treesitter.rs` tests `:5488-5517` and the six users
(`:5534`, `:5577`, `:5612`, `:5658`, `:5743`, `:5772`, `:5811`, `:5838`, `:5899`, `:5927` per recon).
Change: replace `java_class_id`/`java_method_id` with `fn node_symbol(ex: &Extraction, name, kind) -> SymbolId`
that returns the symbol of the ACTUAL def node in `ex.nodes`; each test asserts
`edge.target == node_symbol(&ex, "listOrders", NodeKind::Method)` (route), `r.from == node_symbol(&ex, "OrderService", NodeKind::Class)` (DI),
listener/emitter likewise — the assertion now checks that the edge endpoint exists as a node,
which the old string-vs-string comparison could not (`:5597-5645` compare against a helper, never
against `ex.nodes`). One extra test pins the literal shape once:
`java_route_handler_target_is_type_nested` → `ts-java . . . OrderController/OrderController#listOrders().`.
Tests: `cargo test -p wicked-estate-extract java_` → all green; full crate green.
Deletes: `java_class_id` (`:5494-5505`), `java_method_id` (`:5507-5517`), the comment at `:5490-5493`.

### S2 — Stored-graph scheme gate + store-level tests
Files: `crates/wicked-estate/src/lib.rs` (`:553-583` block only — NOT the resolver slice
`:923-928` nor the unresolved accounting `:937-946`), `crates/wicked-estate/src/main.rs:107-135`
(`maybe_warn_version_mismatch`), `crates/wicked-estate/tests/id_scheme.rs` (new).
Change (lib.rs), revised per MI-ATK-2/BR-1/BR-2 (full rationale in D7):
- **Read/check** after the extra-rules block (`:574-583`): `let scheme_key =
  repo_scope::meta_key(repo, "id_scheme"); let prev_scheme = store.meta_get_key(&scheme_key);
  let scheme_gate = !previously_indexed.is_empty() && prev_scheme.as_deref() != Some(SYMBOL_ID_SCHEME);
  if scheme_gate { force_full = true; eprintln!("SYMBOL-ID SCHEME changed (v{} → v{}): forcing full re-extraction", prev_scheme.as_deref().unwrap_or("1"), SYMBOL_ID_SCHEME); }`.
  The predicate is `!previously_indexed.is_empty()` (`lib.rs:538-545`), NOT `prev_version.is_some()`
  — pre-version DBs must fire the gate too (BR-2).
- **Write** the key in exactly two places, neither at the check site: (a) end of `index_path_as`,
  next to `prune_dangling_edges` (`:973-982`), before `store.stats()` (`:990`); (b) the
  `changed.is_empty()` early return (`:712-713`), guarded by `!scheme_gate`. A crash mid-migration
  leaves the old key → the gate re-fires next run (MI-ATK-2/BR-1). The pre-existing
  `indexed_version`/`extra_rules_digest` writes are NOT relocated (out of lane).
Change (main.rs): the per-repo loop also compares `id_scheme`; message names the label and says
"symbol ids under types changed; annotations/xedges keyed on old ids are NOT carried over — run
`wicked-estate index <root> --repo <label>`". Delete the "(your annotations are preserved)" clause.
Tests (`tests/id_scheme.rs`, in-process, `SqliteStore::open(tempdir)`):
1. `same_version_old_scheme_db_is_fully_reextracted_without_force` — index the S0 fixture dir;
   then simulate the BEFORE state through the store API: `meta_set_key("id_scheme", "1")`,
   `upsert_nodes([Node with the flat symbol])`, leave digests as written; call `index_path` again
   with NO file change → assert the flat node is gone, `Repo#save().` and `Cache#save().` exist,
   `meta_get_key("id_scheme") == "2"`. (`file_digest` computation is private at `lib.rs:424`; the
   store-API set-up avoids recomputing it.) **MI-A4:** the seeded flat node's `Location.file` must
   be the exact rel path the indexer stores — read it back from an already-indexed node first;
   `remove_file` deletes `WHERE file=?1` (`sqlite.rs:1747-1749`), so a wrong path makes the node
   survive for a path reason, not a gate reason. This test is ALSO the interruption test
   MI-ATK-2 asks for: because the key write is last, the state a crash leaves behind (digests
   current, v1 rows present, key still old) is exactly the state this test constructs, and the
   assertion proves the gate re-fires on it.
2. `id_scheme_is_recorded_per_repo` — mirrors `tests/multi_repo.rs:500-535` for the new key
   (labelled runs write `repo:<label>:id_scheme`, never the bare key).
3. `fresh_db_writes_scheme_without_forcing` — first index of a fresh DB: gate must not fire
   (`previously_indexed` empty); after the run the key == "2" (written post-extraction / on the
   early return).
4. `store_keeps_same_named_methods_of_different_types_apart` (store-level collision test, the one
   that catches `sqlite.rs:387-394`): after indexing the fixture, `search(store, "save")`
   returns 4 nodes (`Repo#save().`, `Cache#save().`, `Store#save().`, and ONE flat `save().` —
   `lit.save` merged with the method-local arrow, the D1 residual) and `stats().node_count`
   equals the def count + 1 file node; no Calls edge from `Cache#flush().` targets `Repo#save().`.
5. `pre_version_db_still_fires_scheme_gate` (BR-2, decisive for the predicate) — build a
   pre-a81317d-shaped DB via the store API only, never calling `index_path` on it: index a
   throwaway DB once to read back the true rel path and digest (`store.file_digest`), then on a
   FRESH store `upsert_nodes([flat node at that rel path])` + `set_file_digest(rel, digest)` —
   nodes + current digests, NO `indexed_version` key, NO `id_scheme` key (`main.rs:133-135`'s
   population). `index_path` without force → flat node gone, nested ids present, key == "2".
   Under the rejected `prev_version.is_some()` predicate this test fails: the gate stays silent,
   the digest-matching file is skipped, and the flat node survives.
Tests: `cargo test -p wicked-estate` (47 lib + 20 main + 60 integration at d7d3b58, plus the 5 new);
`cargo test -p wicked-estate-resolve` must stay 61 unit + 1 lsp_live + 4 scip_edges
(+1 pre-existing ignored doctest, engine defect #5, out of scope — report unchanged, do not fix);
`cargo test -p wicked-estate-bench` (BR-4 — `footprint_and_speed_within_ceilings` at
`crates/wicked-estate-bench/tests/integration_bench.rs:256` gates bytes/node and nodes/sec, both
moved by more nodes + the second pass; per-crate, allowed).
Deletes: the "(your annotations are preserved)" text at `main.rs:121-127`.

### S3 — Documentation
Files: `docs/adr/ADR-002-stable-symbol-identity.md` (new section "Amendment 2026-08 — type-nested
member identity", status stays Accepted), `CHANGELOG.md` `[Unreleased]` → `### Changed`,
`docs/recon/java-spring-framework-edges.md:37-41` (append a dated note: scheme is now
type-nested; the edges are minted through `def_symbol`/`def_symbol_at`),
`docs/benchmarks/README.md` (dated note, BR-4).
ADR amendment content: the new descriptor rule (D1, including the truncation rule — the chain is
the contiguous run of Type anchors immediately enclosing the def; any Method/Function/Term
container ends it) with the worked strings; a 4th stability row "rename/move the enclosing type →
new identity for its members (correct: different logical path)"; a 5th row "move a member between
two types in one file → new identity"; "functions and Term bindings contribute no descriptor
(`Symbol::Local` remains unadopted)"; the residual (function-local defs, object-literal methods —
including the class-field object-literal collision shape from D1/test 7 — overloads with
`disambiguator: None`, Rust impl/Go receiver/Ruby `class << self` until their anchors exist); the
collision evidence (D03-1/D03-2/PER-7, `sqlite.rs:387-394`); `SYMBOL_ID_SCHEME` and the
`id_scheme` gate (write-after-extraction ordering + why); the migration note (§4 below) including
BR-5/MI-A5's two sentences: "re-run `wicked-estate scip <root>` after the forced re-extract —
`remove_file` deletes the confidence-1.0 SCIP edges by file (`sqlite.rs:1739-1749`)" and "do not
run pre-scheme binaries against a scheme-2 DB; if one did, re-index with `--force`".
Benchmarks note (`docs/benchmarks/README.md`): dated line stating the pinned node/edge/coverage
numbers in `capability-report.md`/`multi-repo-validation.md` predate the scheme-2 re-baseline;
`blast_radius_coverage_pct` is expected DOWN on collision-heavy repos (parked > falsely resolved)
— a precision correction, not a regression (MI-ATK-3's §9 position, stated where a bench reader
will look).
Tests: none (docs). `git diff --stat` shows only these four files.
Deletes: the sentence in `java-spring-framework-edges.md:40-41` is left as history with the
note above it (recon docs are records, not specs).

### S4 — Measurements (BEFORE/AFTER, verbatim commands)
Files: append a "Measurements" section to this document.
```
B=/Users/michael.parcewski/Projects/wicked/wicked-estate/target/release/wicked-estate
L=/private/tmp/claude-501/-Users-michael-parcewski-Projects-wicked/f5d30481-90ff-4c55-9faa-abdb54e1619c/scratchpad/lanes/method-identity
export CARGO_TARGET_DIR=$L/target
cd $L/wt && cargo build -p wicked-estate
A=$L/target/debug/wicked-estate
M=$L/measure; mkdir -p $M
git -C /Users/michael.parcewski/Projects/wicked/wicked-studio rev-parse HEAD   # pin
git -C /Users/michael.parcewski/Projects/wicked/wicked-crew  rev-parse HEAD    # pin
for c in studio crew; do
  $B index /Users/michael.parcewski/Projects/wicked/wicked-$c --db $M/$c-before.db
  $A index /Users/michael.parcewski/Projects/wicked/wicked-$c --db $M/$c-after.db
done
# fixture (S0 file copied to $M/doc03proj/src/fixture.ts)
$B index $M/doc03proj --db $M/doc03-before.db
$A index $M/doc03proj --db $M/doc03-after.db
Q() { /usr/bin/sqlite3 "$1" "$2"; }
for db in $M/*-before.db $M/*-after.db; do echo "== $db"
  Q $db ".schema nodes"   # once, to confirm kind is stored as a JSON string
  Q $db "SELECT kind, COUNT(*) FROM nodes GROUP BY kind ORDER BY 2 DESC"
  Q $db "SELECT COUNT(*) FROM edges WHERE kind='\"calls\"'"
  Q $db "SELECT COUNT(*) FROM unresolved_refs WHERE kind='\"calls\"'"
  Q $db "SELECT resolved_by, COUNT(*) FROM (SELECT json_extract(data,'$.resolved_by') resolved_by FROM edges WHERE kind='\"calls\"') GROUP BY 1"
  # collision groups: only observable AFTER (BEFORE already collapsed them)
  Q $db "SELECT COUNT(*) FROM (SELECT file,name,kind FROM nodes WHERE kind IN ('\"method\"','\"function\"','\"constructor\"','\"field\"') GROUP BY 1,2,3 HAVING COUNT(*)>1)"
  Q $db "SELECT COUNT(*) FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE n.kind='\"method\"' AND s.sym LIKE '%#%'"
done
# MI-A3: the nested fixture ids are observable only via '#' (Suffix::render makes Type 'Name#',
# so nested method ids end in '#save().', not '/save().') — both halves of falsifier 2:
Q $M/doc03-after.db "SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE s.sym LIKE '%#save().'"   # exactly 3: Repo#, Cache#, Store#
Q $M/doc03-after.db "SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE s.sym LIKE '%/save().'"   # exactly 1 flat (lit.save + method-local arrow, merged residual)
# multiplicity is proven here (search/nodes), NOT via blast-radius (BR-3):
$A search save --db $M/doc03-after.db
# blast-radius: dependents are EXPECTED to shrink on the fixture (see expectations below)
$B blast-radius save --db $M/doc03-before.db --json; $A blast-radius save --db $M/doc03-after.db --json
# the no-force migration acceptance test
cp $M/doc03-before.db $M/doc03-migrate.db
$A index $M/doc03proj --db $M/doc03-migrate.db          # expect the SYMBOL-ID SCHEME line, no --force
Q $M/doc03-migrate.db "SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE s.sym LIKE '%/update().' OR s.sym LIKE '%/flush().'"   # expect ONLY Repo#update()., Cache#flush().
Q $M/doc03-migrate.db "SELECT v FROM meta WHERE k='id_scheme'"   # expect 2
cargo test -p wicked-estate-resolve
```
Expectations and acceptance, restated per BR-3 (the old criterion "blast-radius save returns ≥
the BEFORE names" was FALSE on this plan's own fixture and is withdrawn):
- Nodes by kind `method`/`field`/`constructor` UP where collisions existed (crew
  `src/core/adapter.ts`: 4 classes, 69 stored methods, is the likely largest delta; studio has
  only 20 stored methods, so its delta will be small).
- **Multiplicity is proven via `search`/node queries** — exactly 4 distinct `save` nodes on
  `doc03-after.db` (3 nested + 1 flat residual) — NOT via blast-radius:
  `blast_radius_by_name` excludes the start symbols (`n.symbol != sym.symbol`,
  `wicked-estate/src/lib.rs:1108-1119`), so the split nodes never appear in their own output.
- **`blast-radius save` on the fixture is EXPECTED to LOSE the `update`/`flush`/`cb` dependents**,
  asserted, not glossed: BEFORE, those names appear only because 0.65 `scoped-name-resolver` edges
  point at the merged node (D03-2's false edges); AFTER, `by_name("save")` has ≥ 2 same-file
  candidates, `ScopedNameResolver` parks (`resolve/src/lib.rs:189-198`), no Calls edge enters any
  `save` node, and the dependents collapse toward the File node (Contains). The removed edges are
  the precision correction this lane exists to make.
- **The R7 honesty compensation is asserted explicitly**: the `unresolved` count in the
  blast-radius output RISES and is present in BOTH the `--json` document (`main.rs:1330-1343`,
  fed from `unresolved_refs_for_name` at `:1321`) and the text `coverage:` line
  (`main.rs:1357-1363`) — the contract studio/crew consume, so the lost edges are visible as
  parked, not silently absent.
- **Calls edges DOWN and unresolved rows UP** on collision files; report per `resolved_by` so the
  drop is attributable to removed false-precision 0.65 edges.
- On the studio/crew corpora, report the **per-name dependent-count delta** for the top affected
  names — no monotonicity claim in either direction.
- **§9 position (MI-ATK-3):** the agent-eval benchmark reader gates on resolved-edge completeness
  (`ResolverStats`, `crates/wicked-estate-bench/src/capability.rs:53`, breakdown at `:375-403`).
  The verdict rule for this change: edges removed AT the 0.65 `scoped-name-resolver` tier toward
  previously-merged nodes are corrections of false precision (D03-2), not recall regressions; the
  per-`resolved_by` table above is the gate evidence, and every other tier's counts must hold.
  `cargo test -p wicked-estate-bench` runs in S2's matrix (BR-4).
- After migrating any real DB that had SCIP edges: re-run `wicked-estate scip <root>` (BR-5 —
  the forced re-extract's `remove_file` deletes them by file).

## 4. Compatibility + migration

- **Which ids change:** every definition enclosed by a Type-suffixed definition — methods,
  constructors, fields/properties, enum members, interface method signatures, nested types.
  Top-level functions, classes, constants, imports, IaC/estate ids, synthetic ids, File ids are
  byte-identical (the tests pinning `seed_fixture/seed_fn().`, `alpha/src/a/x().`, `main().`
  remain valid; `tests/multi_repo.rs:219` rewrites the module prefix, still valid).
- **Store:** no schema change. A DB indexed by any earlier binary is fully re-extracted on the
  next `index` of each repo (S2 gate), with a loud stderr line; `--force` still works and is
  equivalent. `remove_file` purges old-id nodes/edges/unresolved rows by file; `prune_dangling_edges`
  drops any edge whose endpoint vanished. Postgres store: same upsert, same gate (the gate is in
  `index_path_as`, above the store).
- **Not carried over (documented, not migrated):** annotations attached to a churned id
  (`annotations --symbol <old>` still returns them; `stale-annotations` lists them under dead
  ids); overlay xedges / memory + knowledge about-edges to churned ids (epoch-dropped at read);
  embeddings (re-run `--embeddings`); any agent-held `--symbol` id from `resolve --json`;
  **SCIP edges** (BR-5: `remove_file` deletes all edges whose location file or source node is in
  the file, `sqlite.rs:1739-1749`, so the confidence-1.0 SCIP edges vanish in the forced pass —
  re-run `wicked-estate scip <root>` after the migration; stated in the ADR amendment + CHANGELOG).
- **Downgrade hazard (MI-A5/BR-5, documented):** a pre-scheme 0.14.6 binary run against a
  scheme-2 DB re-mints flat ids for any changed file — no gate the new binary writes can prevent
  it (the old binary ignores the key; the version gate cannot fire at equal versions). Migration
  note: do not run pre-scheme binaries against a scheme-2 DB; if one did, re-index with `--force`.
  The next release's version bump closes this via the `indexed_version` gate.
- **Consumers:** crew (`graph.ts`), studio (`RepoGraphModal.tsx`), garden (`_clients.py`,
  `estate_db.py`), wicked-core treat ids as opaque strings — number changes only. Visible string
  change: studio's `shortName()` (last `/` segment) renders `Repo#save().` instead of `save().`;
  garden's patch translator surfaces previously-merged methods as extra `#n` rows.
  `wicked-estate nodes/resolve/search/blast-radius <name>` are name-keyed: same results, more
  hits where collisions existed.
- **Resolver:** no code change (other lanes own it). Behaviour flips on collision files from
  wrong-edge (0.65 to a merged node) to parked; the receiver-aware recovery (`this.`) is the
  resolver-precision lane's follow-up and now has distinct targets to point at.
- **Version:** no crate version bump in this lane; the `id_scheme` key is the contract. A release
  bump remains release-managed.

## 5. Falsifier

The plan is wrong if any of these holds after S0–S2:
1. `cargo test -p wicked-estate-extract identity_nests_methods_under_enclosing_type` passes at
   d7d3b58 (then the fixture does not exercise the collision).
2. On `doc03-after.db`, `SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE s.sym
   LIKE '%/save().'` returns anything other than exactly ONE flat row (`…/save().` — `lit.save`
   merged with the method-local arrow, the D1 residual), **or** (MI-A3 — the `/` query is
   structurally blind to nested ids, `Suffix::render` makes Type `Name#`)
   `… WHERE s.sym LIKE '%#save().'` returns anything other than exactly THREE rows
   (`Repo#save().`, `Cache#save().`, `Store#save().`).
3. After S1, any Java `route-handler`/`event-listens`/`di-wired` edge in the framework tests has an
   endpoint that is not a `node.symbol` in the same `Extraction` (the graph does not join).
4. After S2, `doc03-migrate.db` (BEFORE-indexed, re-indexed by AFTER **without** `--force`) still
   contains `…/update().` or `…/flush().`, or `meta.id_scheme != 2`.
5. `cargo test -p wicked-estate-resolve` count moves from 61/1/4.
6. Any hunk lands in a `.scm` file, `resolve/src/lib.rs`, `wicked-estate/src/lib.rs:923-946`, or
   `treesitter.rs:540-600` (lane disjointness).
7. (MI-ATK-2/BR-1 ordering, grep-checkable) any `meta_set_key(&scheme_key, …)` call in
   `index_path_as` executes before the extraction/resolve phases — the only two permitted sites
   are the end-of-run write next to `prune_dangling_edges` and the gate-guarded
   `changed.is_empty()` early return.
8. (MI-A1) the method-local `const save = () => {}` in the fixture mints `Cache#save().`, or
   S0 test 3 and the S1 implementation cannot both be green as specified.

## 6. Not in scope + merge notes for other lanes

Not in scope (this lane):
- Resolver behaviour (parking vs receiver-aware `this.` resolution) — resolver-precision lane.
- The unresolved-row accounting (`lib.rs:937-946`) — off-limits region.
- `Symbol::Local` adoption for function-local definitions — separate ADR if wanted.
- Overload disambiguators (D6).
- Re-keying annotations/xedges (D5) — documented loss.
- Rust `function_item` double-match re-kinding methods to `function` (`rust.scm:3-7` + `:24-30`).
- The untracked `examples/dump_ndjson.rs` in the main checkout — not committed; measurements use
  `index` + sqlite3.

Merge notes → **extraction-gaps lane** (query files; this lane does not edit `.scm`):
- `rust.scm`: `impl_item` needs a Type-suffixed def anchor whose name is the `type:` field
  (`impl A` → anchor `A#` spanning the impl block) so `A::new`/`B::new` nest as `A#new().` /
  `B#new().`; and the `function_item` double-match (`:3-7` vs `:24-30`) should be resolved so
  methods stop being stored as `function`.
- `go.scm`: `method_declaration` needs the receiver type captured as an owner anchor (there is no
  enclosing node; the seam would need an explicit `@code_method.owner`-style capture — say so if
  the generic seam must grow a capture role; this lane leaves the role table untouched).
- `ruby.scm`: `class << self` (`singleton_class value: (self)`) is not matched by `:9-13`.
- `typescript.scm`: object-literal methods (`:24-31`) nest under nothing by D1; if a Type anchor
  for `const x = { … }` is wanted, it must be a query decision, not a Rust arm.
- `typescript.scm` (NEW, from MI-ATK-1/test 7): capture **object-valued** `public_field_definition`
  as a Term-suffixed def (today `:28-31` requires an `arrow_function` value). Without it,
  `class A { save(){} x = { save(){} } }` merges the field-literal `save` into `A#save().` — the
  truncation rule fixes it with zero Rust change the moment the capture exists; update
  `identity_field_object_literal_residual` when it lands.

Merge notes → **resolver-precision lane**: after this lane, `by_name("save")` returns N nodes for
N types in one file; `ScopedNameResolver` parks them. The `from` id now carries the enclosing
type descriptors (structured `Symbol`, parse via `Symbol`/`Descriptor`, not string ops), which is
the prerequisite D03-1 said was missing.

Merge notes → **program integrator**: `treesitter.rs` hunks are confined to `:1341-1376`,
`:1740-1760`, `:1866-1905`, a new pass-2 block between `:2010` and `:2012` (before the COBOL
fixup — pinned, MI-A2), `:2146-2260`, and the test module `:5488-5930` (+ one line in
`smoke_cobol` `:4598-4612`); the extension-routing table (`~:569-575`) is untouched.

## 7. Attack-issue resolutions (revision 2)

All majors accepted; nothing rejected. Minors folded in.

| Issue | Resolution |
|---|---|
| MI-ATK-1 (major) | D1 rewritten: chain = contiguous run of Type anchors immediately enclosing the def; ANY non-Type container (Method/Function/Term) truncates. Term explicitly truncates (`lit`); the not-captured class-field object-literal shape is pinned as a residual (S0 test 7) + a new `.scm` merge note. |
| MI-ATK-2 (major) | D7/S2: `id_scheme` written only post-extraction (end of `index_path_as`) + on the gate-guarded early return; crash re-fires the gate. Interruption state is exactly what S2 test 1 constructs; ordering pinned by falsifier 7. `indexed_version`/`extra_rules` writes not relocated (out of lane). |
| BR-1 (major) | Same fix as MI-ATK-2 — write placement moved to `prune_dangling_edges`/`stats` tail (`lib.rs:973-990`) and the `changed.is_empty()` return (`:712-713`). |
| BR-2 (major) | Gate predicate is `!previously_indexed.is_empty()` (`lib.rs:538-545`), not `prev_version.is_some()`; pre-version DBs fire the gate. Decisive new S2 test 5 (`pre_version_db_still_fires_scheme_gate`). |
| BR-3 (major) | S4 acceptance restated: multiplicity via `search`/nodes; the fixture's blast-radius dependents are EXPECTED to shrink (removed 0.65 false edges) and that is asserted; `unresolved` rise asserted in text + `--json` (`main.rs:1321-1363`); corpora report per-name deltas, no monotonicity claim. |
| MI-A1 (major) | Same rule change as MI-ATK-1; `enclosing_chain` consumes the FULL PendingDef list; the `const save = () =>` collision input added to the fixture + test 3; falsifier 8 added. |
| MI-A2 (major) | Pass-2 insertion pinned between the match-loop close (`:2010`) and the COBOL fixup (`:2012-2036`); DefRec keeps pre-fixup ranges; `smoke_cobol` gains a span assertion. |
| MI-ATK-3 (minor) | §9 verdict rule stated in S4 (per-`resolved_by` table is the gate evidence; 0.65 scoped-name removals = precision correction) + `docs/benchmarks/README.md` dated note in S3. |
| MI-ATK-4 / MI-A6 (minor) | §1 mint-site enumeration added: clips.rs:66/360/404 deliberately unchanged (module-scoped, no type nesting, force_full-safe); xml_rules/extra_edge/grammarless = synthetic; tfstate = IaC out of scope; "only constructor" claim scoped to the tree-sitter seam. |
| BR-4 (minor) | `cargo test -p wicked-estate-bench` added to S2's matrix; benchmarks docs dated note in S3. |
| BR-5 (minor) | SCIP re-ingest instruction + same-version downgrade note added to §4/S3 ADR amendment + CHANGELOG; clips enumeration per above. |
| MI-A3 (minor) | Falsifier 2 + S4 gain the `%#save().` companion query (exactly 3 rows). |
| MI-A4 (minor) | S2 test 1 requires the seeded flat node's `Location.file` to be the read-back rel path (`remove_file` deletes `WHERE file=?1`, `sqlite.rs:1747-1749`). |
| MI-A5 (minor) | Downgrade hazard documented (D7, §4, S3); only the new binary can warn; next release's version bump closes it. |
