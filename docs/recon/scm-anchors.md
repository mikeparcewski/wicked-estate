# scm-anchors — closing the residual identity collisions scheme 2 could not express

Lane: `fix/scm-identity-anchors` (base d7d3b58; scheme 2 + the def/pass-2 seam landed at 764622f, #130).
Plan status: **decided, revised** — every decision below is final for this lane; deviations from the
brief's letter are recorded in §6 with the evidence that forced them.
**Revision 2026-08-29:** the adversarial attack (invariants / blast-radius / feasibility lenses)
returned 8 blocking+major issues (ATK-1..3, BR-1..2, ISS-1..3) and 9 minors; ALL are resolved in
place below — none rejected. The defect class the attack found three times (a required field
constraint added to an EXISTING def pattern silently drops today's defs) is now a binding rule,
R-DEF-LOSS (D1), applied to D4/D5/D8. The C++ cross-file member proto/def hazard (ATK-2/BR-1) is
stanced explicitly in D8 and M4. Grammar field-type claims were re-verified against the vendored
node-types.json files (F12).

Baseline (verified in this worktree, `CARGO_TARGET_DIR` = lane target):
`cargo test -p wicked-estate-extract` = 362 unit + 108 integration + 3 doctests, 0 failed,
1 pre-existing ignored doctest; `cargo test -p wicked-estate` = 153 passed, 0 failed.

---

## 1. Findings acted on (citations = files opened in this worktree)

| # | Finding | Evidence |
|---|---|---|
| F1 | Rust impl-block methods collide: rust.scm has NO impl_item pattern (deliberate — the #129 double-emit scar); impl methods match the general `function_item` pattern and stay module-flat, so two impls' same-named methods mint one SymbolId. | `src/queries/rust.scm:4-30` (patterns + NOTE); review "Engine defects" #1; risks-lens empirical: two impls' `save()` → 1 node at base |
| F2 | Go receiver methods collide: `method_declaration` captures name/params/result/body only — no `receiver:` capture; methods are top-level siblings of their type, so NO containment anchor can ever exist. | `src/queries/go.scm:12-18`; `treesitter.rs:1396-1399` (enclosing_chain requires strict containment, excludes range-equal) |
| F3 | Ruby: `class << self` matches nothing (`singleton_class` pattern requires `value: (constant)`), so its methods nest under the enclosing class and merge with instance methods; `def self.m` (singleton_method) also merges with instance `def m`. No pin exists for either shape today. The singleton_method pattern has NO `object:` constraint, so `def Foo.m` / `def obj.m` ALSO extract today (nesting under the enclosing class by containment) — any edit that constrains `object:` must not drop them. | `src/queries/ruby.scm:9-13, 29-34`; `tests/fixtures/sample.rb` has `def self.checksum` but no `class << self` and no `def Foo.m` |
| F4 | TS object-literal fields: `public_field_definition` is captured only with `value: (arrow_function)`, so `x = { save(){} }` is invisible as a container and the literal's `save` merges with the class's real `save()` — pinned by `identity_field_object_literal_residual`. | `src/queries/typescript.scm:28-31`; `src/treesitter.rs:6550-6571` (pin + its own flip instruction) |
| F5 | Python ORM equal-range residual: both ORM patterns anchor `@code_field.def` at the WHOLE `class_definition`, so the field record is range-equal to the class and can never take it as owner — pinned by `identity_field_orm_equal_range_residual` (fix named in the pin itself: anchor at the assignment node). | `src/queries/python.scm:48-59, 71-85`; `src/treesitter.rs:6653-6683` |
| F6 | C++ out-of-line member vs free function: the D6e pattern captures only `name: (identifier)` inside `qualified_identifier` — the qualifier (`Foo::`) is discarded, so `void Foo::reset() {}` shares the free `void reset() {}`'s id. The pattern leaves `scope:` UNCONSTRAINED, so template-scoped out-of-line members (`void Foo<T>::reset() {}`, scope = template_type) extract today too — any edit that constrains `scope:` must not drop them. Pinned (residual half) with explicit flip instruction. | `src/queries/cpp.scm:43-52`; `tests/languages.rs:1412-1478` |
| F7 | C++ free prototypes (D6d) were deferred on a data-loss MECHANISM, not just a missing owner: `foo.h` proto + `foo.cpp` def mint one SymbolId (module strips one extension), `nodes.file` flaps last-write-wins, `remove_file` deletes by file, the digest skip never re-extracts the survivor. Deferral terms: "lands only after the program records an owner **+ decision**". Only the owner is recorded. | `src/queries/cpp.scm:55-63` (deferral NOTE); `docs/recon/extraction-gaps.md` §D6(d) + merge note 2; RESOLUTION-PROGRAM.md:41 (owner only) |
| F8 | Mechanism constraints: every PendingDef mints a Node + a Contains edge (no anchor-only record); `enclosing_chain` is strict-containment only; `classify_capture` accepts def suffixes `.def/.arrow/.decl/none` + `.name`/`.name.symbol`, everything else auxiliary-ignored; dedup keeps the LAST (start,end,name) record under uncontracted match order. | `src/treesitter.rs:2196-2216` (mint loop), :2258-2270 (Contains), :1396-1399, :1771-1800 (classify_capture), :2176-2194 (dedup) |
| F9 | Id migration is gated on one constant: `SYMBOL_ID_SCHEME = "2"` (`treesitter.rs:1354`); the gate compares the per-repo `id_scheme` meta key and forces full re-extraction on mismatch. No gate hashes the built-in `.scm` files — a query-only id change at an unchanged scheme silently mixes ids in existing DBs. The gate key is per repo LABEL, and the mismatch warning is CLI-only. | `src/treesitter.rs:1345-1355`; `wicked-estate/src/lib.rs` gates; `wicked-estate/src/main.rs:106-157` (per-label loop; annotations/**xedges** named as not carried over); `wicked-estate/tests/id_scheme.rs` |
| F10 | Equal-range `@code_field.def` producer audit: of the 11 `.scm` files emitting `@code_field.def`, only **python.scm** anchors it at a node wider than the field itself (the `class_definition`). apex/cobol/cpp/csharp/d/go/java/pony/solidity/typescript all anchor at the field-level node. | `grep -B2 "@code_field.def" src/queries/*.scm` (run in this worktree, 2026-08-29) |
| F11 | The suffixes `.anchor` and `.owner` are unused across all query files — free namespace for new generic roles. | `grep -rn "\.anchor\b\|\.owner\b" src/queries/` = 0 hits |
| F12 | Grammar field-type enumerations (vendored node-types.json, enumerated 2026-08-29, commands in the measure/ transcript): **go** (tree-sitter-go-0.23.4) `parameter_declaration.type` = `_type` ⊃ `_simple_type` ∪ `parenthesized_type`; `pointer_type`'s child is `_type` again, so `func (c *Cache[K,V]) Get()` parses as `(pointer_type (generic_type …))` and `func (r (T))` as `(parenthesized_type …)`. **ruby** (0.23.1) `singleton_method.object` = `_variable` {self, constant, identifier, super, nonlocal} ∪ `_arg` (an OPEN expression set). **cpp** (0.23.4) `qualified_identifier.scope` = {namespace_identifier, template_type, decltype, dependent_name}; `template_type` has `name: (type_identifier)`. **rust** (0.23.3) `generic_type.type` includes `scoped_type_identifier`. | `~/.cargo/registry/src/…/tree-sitter-{go-0.23.4,ruby-0.23.1,cpp-0.23.4,rust-0.23.3}/src/node-types.json` |
| F13 | Cross-file member twin of F7: `module_path` strips one extension (`treesitter.rs:1338-1343`) so `foo.h` and `foo.cpp` share module `foo`; the D6b in-class prototype (`cpp.scm:62-65`, landed) already mints `foo/Foo#reset().` in the header — a qualifier owner on the .cpp out-of-line def (D8) makes the header proto and the .cpp definition ONE SymbolId across TWO files. Same store mechanism as F7: `nodes.file` flaps last-write-wins, `remove_file` deletes by file, the digest skip never re-extracts the survivor. | `treesitter.rs:1338-1343`; `cpp.scm:58-65`; `docs/recon/extraction-gaps.md` §8 merge note 1(b) |
| F14 | The bench harness pins github.com/tree-sitter/tree-sitter (C/C++/Rust-heavy — exactly the id-churned languages) and indexes real source through the extractor; its tests are not in the plan's original gate list. | `crates/wicked-estate-bench/src/lib.rs:114-116`; `crates/wicked-estate-bench/tests/integration_bench.rs` |

---

## 2. Decisions (all final)

### D1 — Two generic capture roles, one seam: `.anchor` (non-emitting containment) + `.owner` (equal-range/no-containment splice)

The five fixes need exactly two expressive gaps closed, and neither can substitute for the other:

- **`@code_<kind>.anchor`** → `CaptureRole::DefAnchor { kind, emit: false }`. The record enters
  `pending` (participates in `enclosing_chain` as a Type container) but pass 2 mints **no Node, no
  DefRec, no Contains edge** for it. Used where a container node strictly contains the members but
  minting it would create phantom/duplicate nodes: Rust `impl_item` (a phantom `Foo#` Struct per impl
  block, span-clobbering the real struct via the last-write-wins upsert — the exact class #129
  eradicated) and Ruby `class << self` (no principled node exists). Skipping DefRec also keeps
  `def_symbol_at` framework-emitter joins from landing on an impl-block range, and keeps
  `identity_contains_edges_stay_file_to_def` (one Contains per definition record) true by construction.
- **`@code_<kind>.owner`** → new `CaptureRole::DefOwner { kind }`: the owner TYPE NAME captured
  *within the same match* as the def; `PendingDef` gains `owner: Option<String>`; pass 2 appends
  `Descriptor::new(owner, Suffix::Type)` as the **innermost** descriptor after `enclosing_chain`.
  Used where containment is structurally impossible: Go receivers (method is a sibling of its type),
  C++ qualified out-of-line members (the qualifier shares the def's own range — excluded by the
  strict-containment filter), Ruby `def self.m` (the `self` receiver is inside the def's range).

Why not owner-capture everywhere (skipping `.anchor`)? Rust would need an impl-scoped *duplicate*
method pattern next to the general `function_item` pattern; the (start,end,name) dedup keeps an
uncontracted-order winner → nondeterministic owner. That is the #129 double-emit scar in a new coat.
Why not containment everywhere? F2/F6: no containing node exists / the owner is range-equal.

Constraint documented at the seam: an `.anchor` pattern must never be range-equal + same-name with an
emitting def (the dedup would nondeterministically drop one). None of the planned patterns can be
(impl_item vs struct_item ranges differ; `singleton_class`/"self" collides with nothing). The dedup
itself is NOT changed.

**R-DEF-LOSS (binding rule for every `.scm` edit in this lane — added in revision; resolves ATK-1 /
ISS-1 / ISS-2 / ISS-3 as a class):** *an edit to an EXISTING def pattern must be zero-def-loss.*
Adding a required field constraint to a pattern that today matches unconstrained silently DROPS every
shape outside the constraint — the attack proved the original D4/D5/D8 drafts did exactly that (Go
pointer-generic receivers, Ruby non-self singleton receivers, C++ template-scoped out-of-line
members; F12 is the grammar evidence). Mechanics:
1. An owner/anchor capture added to an existing pattern is wrapped in an **optional quantifier
   (`?`)** — in-repo precedent `ruby.scm:31` (`parameters: (method_parameters)? @code_method.params`,
   fleet-proven, no duplicate-record scar) — so a shape outside the owner alternation degrades to an
   **ownerless def**, never a dropped def. Never add an overlapping sibling pattern instead: the
   constrained pattern's matches are a subset of the unconstrained one's, and the (start,end,name)
   dedup would pick a nondeterministic winner (the #129 scar).
2. Every owner-alternation branch is enumerated against the vendored grammar's node-types.json (F12),
   and every covered shape gets a fixture line + `assert_def` so a future narrowing turns a test red.
   Knowingly-ownerless shapes get a fixture line asserting the def is STILL extracted (flat).
3. NEW patterns (D3's impl anchor) may be non-exhaustive — an unmatched impl target loses only the
   anchor, never a def — but every knowingly-unmatched shape is listed as an ADR residual.
A shape may lose its OWNER (stay module-flat); it must never lose its DEF. The falsifier (§5) gains a
def-loss check.

This exceeds the brief's letter ("a new generic capture-role arm", singular) by: the second arm, two
`PendingDef` fields, ~15 lines of pass-2 plumbing, and one match-loop local. It is still **zero
per-language Rust** (Rules as DATA holds); the method-identity lane's merge note pre-authorized
exactly this growth for Go ("the seam would need an explicit @code_method.owner-style capture").
Recorded as deviation DV-1 (§6).

### D2 — Bump `SYMBOL_ID_SCHEME` "2" → "3" in the same commit as the first id-changing `.scm` edit

Every fix in this lane changes minted ids. Without the bump, any DB indexed by a post-#130 binary
keeps flat ids behind unchanged digests while changed files mint nested ids — the exact
silently-mixed-graph defect the gate was built to prevent (the constant's own doc says so,
`treesitter.rs:1345-1354`). Scheme 2 is **unreleased** (last tag v0.14.6 predates #129/#130), so
released users pay ONE forced re-extraction covering scheme 2 + this lane together.
`id_scheme.rs`'s 4 tests reference the constant symbolically and stay green. The constant is not a
version file (release lane owns Cargo.toml/manifests; this is extractor identity data). Recorded as
deviation DV-2 (§6).

### D3 — Rust: containment anchor at `impl_item`, named from the `type:` field; kind stays Function

```scm
(impl_item
  type: [
    (type_identifier) @code_struct.name
    (generic_type type: (type_identifier) @code_struct.name)
    (generic_type type: (scoped_type_identifier name: (type_identifier) @code_struct.name))
    (scoped_type_identifier name: (type_identifier) @code_struct.name)
  ]
) @code_struct.anchor
```

- `impl Foo` and `impl Trait for Foo` both anchor under **Foo** (the `type:` field; the `trait:`
  field is deliberately not captured) — per the method-identity handover.
- The alternation normalizes `impl Foo<T>`, `impl Trait for crate::Foo`, and (fourth branch, added
  in revision per ISS-5 / F12) `impl Trait for crate::Foo<T>` to the bare `Foo` in the QUERY (data,
  not Rust) — otherwise a path-qualification refactor would change method ids, the
  rename-breaks-identity class ADR-002 exists to prevent. Unanchorable impl targets (`&Foo`, tuples,
  `dyn Trait`) match nothing and their methods stay module-flat: accepted + listed EXPLICITLY in
  ADR-002 residuals and in the new pin's comment. Per R-DEF-LOSS rule 3 this non-exhaustiveness is
  safe: this is a NEW pattern — an unmatched impl loses only the anchor, no def.
- **New residual, pinned:** two trait impls on ONE type (`Display for Foo` / `Debug for Foo`) still
  collide at `Foo#fmt().` — trait-qualified descriptors would need a disambiguator
  (`identity_disambiguator_is_none` pins None, per ADR-002) or a third descriptor with no recorded
  convention. Pinned as a known_defect-style test with a flip instruction; ADR residual entry added.
- **NodeKind stays Function** (no Method restoration): kind derivation from the chain is per-language-
  adjacent Rust beyond this lane's surface, and an impl-scoped duplicate Method pattern is the exact
  #129 nondeterministic-kind scar. `rust.scm:24-30`'s NOTE is rewritten (it names the method-identity
  lane as owner of what this lane now lands; the kind sentence stays, re-pointed to a program follow-up).

### D4 — Go: OPTIONAL receiver owner capture on the existing method pattern (revised per ATK-1a / ISS-2)

Add to the single `method_declaration` pattern (no duplicate pattern → no dedup interplay), with the
**entire receiver sub-pattern optional** per R-DEF-LOSS — a receiver shape outside the alternation
keeps its def (ownerless, module-flat as today) instead of vanishing:

```scm
receiver: (parameter_list
  (parameter_declaration
    type: [
      (type_identifier) @code_method.owner
      (pointer_type (type_identifier) @code_method.owner)
      (generic_type type: (type_identifier) @code_method.owner)
      (pointer_type (generic_type type: (type_identifier) @code_method.owner))
      (parenthesized_type (type_identifier) @code_method.owner)
      (parenthesized_type (pointer_type (type_identifier) @code_method.owner))
    ]))?
```

The original three-branch draft missed `(pointer_type (generic_type …))` — the standard
pointer-receiver-on-generic-type idiom `func (c *Cache[K, V]) Get()` (F12: `pointer_type`'s child is
the `_type` supertype, which includes `generic_type`) — and parenthesized receiver types `func (r (T))`
/ `func (r (*T))`, both legal Go. All six branches mint `<module>/T#M().`. Shapes the alternation
still cannot own (e.g. a `qualified_type` receiver — illegal Go, but parseable) degrade ownerless via
the `?`. Fixture: one method per branch (value, pointer, generic, pointer-generic, parenthesized,
parenthesized-pointer), each with an `assert_def` line, so any future narrowing fails loudly.

### D5 — Ruby: both singleton forms converge on `C#self#m().`; pin first, then fix (revised per ATK-1c / ISS-1)

1. **Pin first** (separate red-shape evidence, house style of the cpp pin): a new fixture with
   instance `def m`, `def self.m`, `class << self; def n; end`, **and `def Foo.m` / `def obj.m`
   shapes** in one file; a test asserting today's merges (`C#m().` ×1 for both m's; `C#n().` ×1 for
   both n's) with flip instructions, plus `assert_def` lines pinning that the `def Foo.m` /
   `def obj.m` defs ARE extracted today.
2. **Fix:** new pattern `(singleton_class value: (self) @code_class.name body: (_) @code_class.body)
   @code_class.anchor` — the anchor's name is the `self` keyword's text, so `class << self` members
   nest as `C#self#m().` (chain: C → self, both Type). And **`object: (self)? @code_method.owner`**
   — OPTIONAL, per R-DEF-LOSS — added to the `singleton_method` pattern, so `def self.m` mints the
   SAME shape `C#self#m().` via the owner splice. The two syntactic spellings of a Ruby class-method
   converge on one id shape and are distinct from instance `C#m().` — the reason "self" wins over any
   invented name. *Why optional instead of the disjoint two-pattern split the attack offered as an
   alternative:* `singleton_method.object` admits `_variable` ∪ `_arg`, and `_arg` is an OPEN
   expression set (F12) — a non-self alternation can never be provably exhaustive, and any missed
   branch is a dropped def (the exact ATK-1 class). The optional form is exhaustive by construction
   and rides the fleet-proven `ruby.scm:31` precedent.
3. `def Foo.m` / `def obj.m`: **defs are KEPT** — the optional owner does not match, the def stays
   ownerless and nests under the enclosing class by containment exactly as today (`C#m().`), which
   means it STILL merges with instance `def m`. That residual merge is now **fixture-pinned** in the
   S3 known_defect test (residual half, with its own flip instruction: an owner splice for constant
   receivers would mint `C#Foo#m().` — an unrelated-type nesting — so it needs a program-recorded
   convention first) and listed in ADR-002. `class << Foo` likewise unchanged (already captured as
   `@code_class` named Foo — reopens `Foo#`'s namespace; members land at `Foo#k().`), documented
   residual. *Revision note:* the original draft required `object: (self)` while claiming `def Foo.m`
   was "unchanged" — falsified by its own edit (it would have dropped the def). The optional form
   makes the "unchanged" claim actually true, and the pin proves it.
4. `ruby.scm:100`'s `receiver: (_)? @call.object` is doc-03's declared input — untouched.

### D6 — TS/JS: object-valued class fields become Term defs; the split creates a NEW pinned residual

- typescript.scm + tsx.scm: `(public_field_definition name: [(property_identifier)
  (private_property_identifier)] @code_field.name value: (object)) @code_field.def` — the name
  alternation includes private fields (`#x = { save(){} }`) per ATK-5; javascript.scm: the
  grammar-divergent `field_definition` / `property:` shape with the same name-alternation intent,
  verified against the vendored JS node-types.json at implementation time — per-grammar fixtures
  because a zero-match pattern fails silently (§11 sibling trap; javascript.scm:47 already documents
  the field_definition/public_field_definition divergence).
- Effect (zero Rust, per the pin's own instruction): the field is now a Term container, the
  truncation rule makes the literal's `save` **module-flat** — distinct from `A#save().`.
- Flip `identity_field_object_literal_residual` to assert the split AND pin the NEW residual in the
  same test, with the residual text naming BOTH pools (per BR-4): the literal `save` at
  `src/a/save().` now collides with any same-named module-level function AND with same-named members
  of OTHER object-literal fields in the same module (the pooling moves from per-class to per-module —
  inexpressible without object-literal descriptors, a scheme change; ADR residual entry). S7 measures
  this pooling trade on the corpora (distinct source sites per module-flat literal-member id,
  before/after).
- **Computed-name fields** (`[key] = { … }`) stay uncaptured — their literal members keep nesting
  under the class and merging with real methods; added to the flipped pin's documented residuals and
  the ADR-002 list (ATK-5; the "keep the pin and document why" rule applies to this half too).
- **Scope: class fields only.** Module-scope `const lit = {...}` is NOT captured — it would churn
  thousands of nodes on TS corpora and shift `identity_does_not_nest_under_functions_or_terms`'s
  "exactly one flat save residual" semantics; out of scope, recorded.

### D7 — Python: move the ORM anchors to the statement node; keep MI-R1-1, tested directly

- Both patterns (SQLAlchemy + Django) re-anchor `@code_field.def` at the `(expression_statement)`
  wrapping the assignment instead of the whole `class_definition` (the pin's own named fix). Fields
  gain their real owners: `A#Model#t.`, `Article#title.` — flip
  `identity_field_orm_equal_range_residual` accordingly. The `identity_orm_*` trio must stay green
  (they pin non-collision/full-chain/determinism, which the move preserves or improves — any id
  assertion that legitimately improves is updated with the real owner, never weakened).
- **MI-R1-1 (equal-range anchor-artifact drop) is KEPT**, not retired: F10's audit shows python was
  its only `@code_field.def` producer, but the drop guards the generic seam against the recurrence
  class fleet-wide (73 languages, any future wide anchor). §8 compliance: the retirement candidate is
  recorded, and the doc comment's stale producer claim ("python.scm's ORM field patterns anchor at
  the WHOLE class_definition") is rewritten to "defensive; last real producer removed in this lane".
  Because the ORM tests no longer route through the drop path, add ONE direct unit test calling
  `enclosing_chain` with a handcrafted equal-range PendingDef list so the guard stays non-vacuously
  tested.

### D8 — C++ (D6d split): land the qualifier owner as an OPTIONAL alternation; stance the cross-file hazard; free prototypes stay DEFERRED (revised per ATK-1b / ISS-3 / ATK-2 / BR-1)

- **Land:** an optional scope alternation on the D6e out-of-line pattern (`cpp.scm:48-52`), per
  R-DEF-LOSS:

  ```scm
  declarator: (qualified_identifier
    scope: [
      (namespace_identifier) @code_method.owner
      (template_type name: (type_identifier) @code_method.owner)
    ]?
    name: (identifier) @code_method.name)
  ```

  `qualified_identifier.scope` admits exactly {namespace_identifier, template_type, decltype,
  dependent_name} (F12). `void Foo::reset() {}` (a class-name qualifier parses as
  namespace_identifier) mints `<module>/Foo#reset().`; `void Foo<T>::reset() {}` anchors under
  `Foo#` via the template_type branch — the original draft's required `(namespace_identifier)`-only
  constraint would have DROPPED template-scoped out-of-line members that extract today (ISS-3).
  decltype/dependent_name scopes degrade OWNERLESS (module-flat, exactly as today) — documented
  residuals — and the `?` keeps any future grammar-added scope kind from dropping defs. Fixture
  lines: `Foo<T>::reset` (assert nested under `Foo#`) and a decltype-scoped definition (assert the
  def is still extracted, flat), each with `assert_def`. This — not a prototype capture — is what
  the pinned collision test actually pins: the out-of-line DEFINITION vs the free DEFINITION. Flip
  the residual half of `cpp_out_of_line_member_vs_free_function_collision_known_defect`
  (assert_eq → assert_ne) per its own instruction; the first (scheme-2) half stays intact.
  Single-level qualification only, matching the pattern's existing stated limit (`Ns::Foo::bar` at
  file scope remains unmatched — residual already documented in cpp.scm).
- **Cross-file hazard — accepted and stanced, not silent (ATK-2 / BR-1; F13):** once the owner
  lands, `foo.h`'s in-class prototype (D6b, landed) and `foo.cpp`'s `void Foo::reset() {}` mint ONE
  SymbolId (`foo/Foo#reset().`) across TWO files, because `module_path` strips one extension. This
  is the member-level twin of the F7 mechanism D8 itself defers free prototypes over: `nodes.file`
  flaps last-write-wins per incremental run, `remove_file` deletes by file, and the digest skip
  never re-extracts the survivor — deleting/renaming `foo.h` can silently drop the live definition
  node and its edges until `foo.cpp` itself changes. Stance (all three, together):
  1. **M4 is EXTENDED** — the program's header/impl identity DECISION now covers member AND
     free-function proto/def identity (it is the same identity question), and the store-side hazard
     (file-flap + delete-by-file + digest-skip for any symbol spanning multiple files) is **filed as
     a wicked-estate store issue** by the program owner — store paths are this lane's MUST-NOT-TOUCH,
     so the fix cannot land here.
  2. **The cross-file case is PINNED:** new test `cpp_member_proto_def_cross_file_single_id_hazard`
     extracts a `foo.h`-shaped and a `foo.cpp`-shaped SourceFile separately (extraction is per-file
     and ids are deterministic from module + chain, so no store is needed) and asserts the two ids
     are EQUAL — the comment names the mechanism, the store issue, and a flip instruction gated on
     the program's M4 decision (flip to distinct ids if the decision is distinct-decl identity;
     retire the pin into a store-conformance test if the decision is one-logical-symbol with the
     store fixed).
  3. **§4 compat and the M1 release notes carry the hazard explicitly** (see §4, §8).
  The single-file pinned test's proto/def id equality is asserted **neutrally** — "single-id member
  semantics pending the M4 decision, see the cross-file hazard pin" — never as "correctly merge":
  if the program decides distinct-decl identity, that assertion flips with the decision. (The
  original draft asserted the merge as a correctness property; that framing was the internal
  inconsistency the attack caught, and it is withdrawn.)
- **Do NOT land free-prototype capture.** The brief's premise ("the deferral condition is met") is
  incomplete against the recorded deferral terms: extraction-gaps.md merge note 2 requires "an owner
  **+ decision** for free-function header/impl node identity"; RESOLUTION-PROGRAM.md:41 records only
  the owner — no decision text exists anywhere in the program artifacts. Landing the capture re-opens
  the proven remove_file data-loss path (F7) whose mechanism lives in store paths this lane MUST NOT
  touch. The brief's own flip conditional ("if the fix resolves it") is moot regardless: prototypes
  change neither colliding id. The ready per-parent pattern set stays recorded in extraction-gaps.md
  §D6(d) verbatim; the cpp.scm deferral NOTE is updated to cite this plan and the still-missing
  decision. Recorded as deviation DV-3 (§6).

### D9 — Measurements: fixture-level BEFORE = lane-base build; corpus stats per protocol (mechanism named per ISS-6; recorded as DV-4 per ATK-6)

The protocol's BEFORE (main checkout release binary) and the lane base are both 764622f-era, but the
release binary's provenance is unverified from this lane; and the two prescribed corpora
(wicked-studio, wicked-crew) are TS/JS-only — they exercise ONLY the D6 object-literal change.
Therefore: per-language collision measurements (Rust/Go/Ruby/C++/Python) run the collision fixtures
through a **lane-base (764622f) debug build** as BEFORE vs the lane HEAD build as AFTER.
**Mechanism (ISS-6):** `git worktree add <lane-scratch>/before-764622f 764622f` — a throwaway
READ-ONLY worktree under the lane scratchpad (never the main checkout), with its own
`CARGO_TARGET_DIR=<lane>/target-before`, removed after measurement; the main checkout's untracked
`examples/dump_ndjson.rs` is copied into BOTH trees uncommitted; every command lands verbatim in the
measure/ transcript. This BEFORE substitution deviates from the measurement protocol's letter and is
recorded as **DV-4** (§6) with the rationale: a pre-scheme-2 BEFORE would double-count the
method-identity lane's already-measured scheme-2 deltas as this lane's effect. Corpus before/after
(index + `stats` nodes/edges/unresolved + one name-based `blast-radius --json` on a
previously-merged method + the D6 literal-pooling query from BR-4) runs per the protocol's binaries
into DBs under the lane `measure/` dir, queried with `/usr/bin/sqlite3` (`.schema` first; kinds are
JSON strings). Corpus "no change" for Rust/Go/Ruby/C++ is expected and is NOT evidence the anchors
are inert — the fixture numbers are.

### D10 — Coverage matrix: `--check` only

No `languages.toml` cap axis changes (anchors/owners add no capability string). Run
`python3 scripts/gen-coverage-matrix.py --check` after all `.scm` edits; expected exit 0. Regenerate
only if it exits 1.

---

## 3. Step list

Every step: work only in this worktree; `export CARGO_TARGET_DIR=<lane>/target`; per-crate cargo
only; commit `--no-verify` with the two mandated trailer lines; after each language run the full
`cargo test -p wicked-estate-extract` (fleet guard `assert_no_conflicting_def_ids` runs 22× inside
it) and keep 0 failed / no new ignored. R-DEF-LOSS applies to every `.scm` edit.

**S1 — seam + Rust (one commit: the seam lands with its first consumer, §5)**
- Files: `crates/wicked-estate-extract/src/treesitter.rs` (classify_capture `.anchor` + `.owner`
  arms; `DefAnchor{emit}` / new `DefOwner` role; `PendingDef{emit, owner}`; match-loop `def_owner`
  local with the kind-equality guard; pass-2 owner splice + `!emit` skip of Node/DefRec/Contains;
  `SYMBOL_ID_SCHEME` "2"→"3" + doc comment), `src/queries/rust.scm` (impl anchor pattern per D3,
  four-branch alternation; rewrite the :24-30 NOTE), `tests/fixtures/sample.rs` (add a second impl
  with a same-named method + `impl Trait for` forms incl. the path-qualified-generic shape),
  `tests/languages.rs` (new `rust_impl_methods_nest_under_type` distinctness test modeled on
  `go_const_vs_struct_field_symbolids_are_distinct`; new
  `rust_same_type_trait_impls_collision_known_defect` pin per D3), `treesitter.rs` unit tests (anchor
  emits no node / no Contains edge / no DefRec join for the impl range; owner splice id shape).
- Tests proving it: the new distinctness test (Point#translate ≠ Other#translate); the no-node
  assertions; `rust_characterization` unchanged (kinds stay Function); full crate suite green;
  `cargo test -p wicked-estate` green (id_scheme tests symbolic).
- Deletes: the stale ownership sentence in rust.scm's NOTE; scheme "2" doc text (superseded in place).

**S2 — Go**
- Files: `src/queries/go.scm` (OPTIONAL six-branch receiver alternation per revised D4),
  `tests/fixtures/sample.go` (one method per receiver branch: value, pointer, generic,
  pointer-generic `func (c *Cache[K, V]) Get()`, parenthesized, parenthesized-pointer — plus
  `type B` + a same-named method for the distinctness test), `tests/languages.rs`
  (`go_receiver_methods_nest_under_receiver_type`: A#M ≠ B#M; value + pointer receivers same shape;
  `assert_def` line per receiver-branch fixture shape so a lossy narrowing turns red).
- Tests: the new distinctness test; the per-shape `assert_def` lines; `go_characterization`
  (updated def floor for the new fixture methods); fleet guard; full crate suite.
- Deletes: nothing (pure capture addition; ADR residual entry for Go is removed in S7).

**S3 — Ruby (two commits: pin, then fix+flip)**
- 3a Files: `tests/fixtures/` new `singleton.rb` (or extend sample.rb) including `def Foo.m` /
  `def obj.m` shapes, `tests/languages.rs` new
  `ruby_singleton_vs_instance_collision_known_defect` pinning today's merges with flip instructions
  + `assert_def` lines pinning that the non-self singleton defs ARE extracted.
- 3b Files: `src/queries/ruby.scm` (singleton_class `(self)` anchor pattern + singleton_method
  **`object: (self)? @code_method.owner`** — optional per revised D5 — `:100 @call.object`
  untouched), flip the 3a pin to assert `C#m().` ≠ `C#self#m().` and `C#n().` ≠ `C#self#n().` and
  that both singleton forms mint the SAME id; keep the residual half asserting `def Foo.m` still
  merges with instance `def m` (its own flip instruction per D5.3); document `class << Foo` /
  `def Foo.m` residuals in the test comment + ADR list.
- Tests: the flipped pin incl. the kept non-self `assert_def` lines; `ruby_characterization`; fleet
  guard; full crate suite.
- Deletes: 3b re-points the 3a pin (never deletes it).

**S4 — TS/TSX/JS object-literal fields**
- Files: `src/queries/typescript.scm`, `src/queries/tsx.scm`, `src/queries/javascript.scm` (object-
  valued field patterns per D6 incl. the private_property_identifier name branch — import-capture
  sections untouched), `src/treesitter.rs` (flip `identity_field_object_literal_residual`: assert
  the split + pin the new module-flat residual naming BOTH pools per D6/BR-4 + the computed-name
  residual per ATK-5), per-grammar fixture additions incl. a `#x = { save(){} }` line so each
  pattern provably matches (zero-match fails silently).
- Tests: the flipped pin; ts/tsx/js characterizations + fleet guard;
  `identity_does_not_nest_under_functions_or_terms` must stay green (module-scope consts uncaptured).
- Deletes: the pin's residual assertion is re-pointed; nothing else.

**S5 — Python ORM anchors**
- Files: `src/queries/python.scm` (move both `@code_field.def` anchors to the expression_statement),
  `src/treesitter.rs` (flip `identity_field_orm_equal_range_residual` to `A#Model#t.` /
  `Article#title.`; rewrite the MI-R1-1 doc comment; add the direct `enclosing_chain` equal-range
  unit test per D7).
- Tests: the flipped pin; `identity_orm_*` trio green (assertions updated only where an id
  legitimately gains its real owner); `python_characterization` + fleet guard; full crate suite.
- Deletes: the class_definition-anchored placement (replaced in the same change); the MI-R1-1 stale
  producer sentence.

**S6 — C++ qualifier owner + pin flip + cross-file hazard pin (revised per D8)**
- Files: `src/queries/cpp.scm` (OPTIONAL scope alternation per revised D8: namespace_identifier +
  template_type branches with `@code_method.owner`, `?`-wrapped; update the D6d deferral NOTE to
  cite this plan + the missing program decision), `tests/languages.rs` (flip + rewrite the pinned
  test per ISS-4, add the cross-file hazard pin), fixture: template + decltype shapes added to the
  test's probe strings.
- Test rewrite mechanics (ISS-4 — the original "flip assert_eq→assert_ne, keep the rest" was
  underspecified and would panic): after the owner lands, the probe's `Foo#`-prefixed Method count
  becomes 2 (in-class proto + out-of-line def), so `assert_eq!(nested.len(), 1)` becomes 2 and the
  old `!contains("Foo#")` finder no longer locates the out-of-line def — locate it by SPAN instead;
  assert `out_of_line.symbol == proto.symbol` **neutrally** (single-id member semantics pending M4;
  cross-ref the hazard pin — same-kind, so the fleet guard stays green) and assert BOTH ≠ the free
  `reset` id (the actual flip, per the pin's own instruction). New
  `cpp_member_proto_def_cross_file_single_id_hazard` test per D8 stance 2. Template out-of-line
  fixture line asserts nesting under `Foo#`; decltype-scope line asserts the def is kept (flat).
- Tests: the rewritten pin; the cross-file hazard pin; `cpp_characterization` + fleet guard; full
  crate suite.
- Deletes: the assert_eq residual half (re-pointed, not deleted). Free prototypes NOT landed (D8).

**S7 — ADR + measurements + bench gate + fleet audit + close-out**
- Files: `docs/adr/ADR-002-stable-symbol-identity.md` (Accepted-residuals list: remove Rust impl /
  Go receiver / Ruby singleton / object-literal / ORM entries; add the new pinned residuals from
  D3/D5/D6 — incl. literal-vs-literal module pooling, computed-name fields, `def Foo.m`, the C++
  cross-file member hazard, decltype/dependent_name flat scopes; note scheme "3" in the migration
  section), measurement artifacts under the lane `measure/` dir (never committed: dump_ndjson copy,
  before-worktree, DBs, command transcripts).
- Commands: fixture before/after per D9 (all five languages, via the before-764622f throwaway
  worktree); corpus before/after per protocol (studio, crew) incl. the BR-4 literal-pooling query
  (distinct source sites per module-flat literal-member id, before vs after);
  `python3 scripts/gen-coverage-matrix.py --check`;
  `cargo clippy -p wicked-estate-extract --all-targets -- -D warnings`;
  `cargo fmt -p wicked-estate-extract`; final `cargo test -p wicked-estate-extract`,
  `cargo test -p wicked-estate`, **and `cargo test -p wicked-estate-bench`** (ATK-3/BR-5: the bench
  crate indexes real source through the changed extractor and pins the C/C++/Rust-heavy
  tree-sitter/tree-sitter corpus; run under the lane CARGO_TARGET_DIR, record the exact result; if
  RED from id churn, record the failing tests VERBATIM in a not_done entry + M1 so the release lane
  regenerates receipts knowingly — a workspace-red crate must never be left for integration to
  discover) — exact counts recorded for all three.
- **Fleet audit (§11, per ATK-4):** sweep the 73 query files for the defect class this lane fixes —
  container constructs whose members are captured but whose container is not (method/function def
  patterns whose enclosing type node has no `@code_*.def`/`.anchor`); record the hit list in the
  merge notes. Known instance already in hand: Swift `extension Foo { func m() {} }` is uncaptured
  (swift.scm keyword-gates class_declaration on class/struct/enum; :81 confirms extensions
  unmatched) — extension methods stay module-flat and two extensions' same-named methods collide,
  the exact F1 shape, now expressible with `.anchor`. Minimum landing here: a Swift-extension
  known_defect pin OR an ADR residual entry with a flip instruction naming the `.anchor` role; the
  remainder of the hit list is handed to the program as merge note M6 (fixing N languages is beyond
  this lane's surface).
- Deletes: the closed ADR residual entries (same change as their pins flipped — re-point contract).

---

## 4. Compatibility + migration

- **Ids churn** for: every Rust impl method, Go receiver method, Ruby singleton member, C++
  out-of-line member, TS/JS object-literal field member, Python ORM field. The scheme "3" bump (D2)
  rides the landed id_scheme gate. **Precision (BR-3): the forced re-extraction fires per repo
  LABEL, on that label's next index** — a multi-repo DB holds scheme-2 ids for one label and
  scheme-3 ids for another until EVERY label is re-indexed, and cross-label surfaces (overlay
  xedges, multi-repo blast radius) span mixed schemes during that window. The mismatch warning is
  CLI-only; the MCP server (the primary agent surface) surfaces nothing — M1 carries the operator
  instruction. Scheme 2 is unreleased, so released users (≤ v0.14.6, id_scheme absent) migrate ONCE
  for scheme 2 + 3 together.
- **After the forced re-extract per repo:** re-run `wicked-estate scip <root>` (remove_file deletes
  the confidence-1.0 SCIP edges by file); **re-inject overlay xedges / re-run the injection
  pipelines (bus event→consumer, command→agent)** — the binary's own mismatch warning says
  annotations/xedges keyed on old ids are NOT carried over (`main.rs:136-143`), and injected edges
  typically land on handler METHODS, i.e. exactly the Rust-impl/Go-receiver classes this lane churns
  (BR-2); coverage/other annotations keyed to churned ids orphan (ADR-002 amendment's documented
  accepted loss) and need re-attachment; agent-held `resolve --json` symbol ids go stale.
- **C++ cross-file member hazard (D8/F13, explicit per ATK-2/BR-1):** after this lane, a member
  declared in `foo.h` and defined out-of-line in `foo.cpp` is ONE SymbolId across two files;
  `nodes.file` is last-write-wins, `remove_file` deletes by file, and the digest skip does not
  re-extract the survivor — deleting/renaming one of the pair can drop a live node until the other
  file changes, and node locations may flap decl↔def between incremental runs. Pinned
  (`cpp_member_proto_def_cross_file_single_id_hazard`), filed store-side via M4, named in M1's
  release notes. This is an accepted, recorded trade — NOT a silent one — pending the program's
  identity decision.
- **Consumers:** ids are opaque strings everywhere (crew graph.ts queries by NAME; wire shapes
  unchanged). Numbers move: edges DOWN / unresolved UP where false 0.65 edges un-merge (the split
  makes >1 best-tier candidates park), node counts UP (new Field nodes for object-valued class
  fields), rank/hotspots ordering shifts. This is intended precision recovery, not regression.
  **Expected direction, stated so S7 can check it (BR-6 / review rules R4 + R7):** blast-radius
  output size is non-increasing for previously-merged names (un-merged nodes shrink aggregated
  dependent sets — R4 output-size pressure DOWN; rank is edge-kind-gated on Calls|Imports and
  ContextBundle truncates with a visible `truncated` flag, per extraction-gaps [ATK A6]), and false
  0.65 edges become honestly-parked unresolved refs with visible confidence (R7 honesty UP). The S7
  name-based blast-radius before/after and the stats deltas are checked against this stated
  direction, not just recorded.
- Crew project graphs skip clean-HEAD repos entirely; operators must force a refresh
  (crew `RefreshOptions.force`) — release-notes item for the release lane.
- **No wire/schema/store change.** No languages.toml cap change → coverage matrix regen expected no-op.

## 5. Falsifier

Build the lane binary at plan completion and run the five collision fixtures through it
(dump_ndjson pattern): the plan is falsified if ANY of — two Rust impls' same-named methods, two Go
receiver types' `M()`, Ruby `def m` vs `def self.m` (or `class << self` member vs instance), C++
`Foo::reset` vs free `reset`, the TS object-literal `save` vs `A.save()`, the Python nested ORM
field — still mints a single shared SymbolId. **Zero-def-loss check (R-DEF-LOSS):** the plan is
ALSO falsified if any definition extracted from the per-language fixtures by the lane-base BEFORE
build is no longer extracted by the lane HEAD build (compare the dump_ndjson def sets per fixture —
losing an OWNER and staying flat is allowed; a def disappearing is not). Likewise if
`cargo test -p wicked-estate-extract` or `cargo test -p wicked-estate` is not green with every
flipped pin asserting distinctness and zero new ignored tests, if
`cargo test -p wicked-estate-bench` was not run and its exact result recorded, or if a
previously-green identity/characterization test was weakened rather than re-pointed.

## 6. Deviations from the brief (recorded, not silent)

- **DV-1 (D1):** two generic capture-role arms + PendingDef/pass-2 plumbing instead of "a" single
  arm. Forced by F2/F6 vs F1/F3: no single mechanism expresses both containment and equal-range
  ownership; still zero per-language Rust; pre-authorized shape in method-identity.md's merge notes.
- **DV-2 (D2):** `SYMBOL_ID_SCHEME` "2"→"3" — one const outside the permitted list. Landing id-shape
  changes without it defeats the landed migration gate (F9). Not a release-lane version file.
- **DV-3 (D8):** free-prototype capture NOT landed despite the brief's item 5 instruction. The
  recorded deferral terms (owner + decision) are unmet — no identity decision exists — and the
  data-loss mechanism is in this lane's MUST-NOT-TOUCH store paths. The pin flip the brief wants IS
  delivered, via the qualifier owner, which is what the pin actually measures.
- **DV-4 (D9):** fixture-measurement BEFORE binary is a lane-base (764622f) debug build via a
  throwaway worktree, not the protocol's main-checkout release binary. Rationale: the release
  binary's provenance vs scheme 2 is unverified from this lane, and a pre-scheme-2 BEFORE would
  double-count the method-identity lane's already-measured scheme-2 deltas as this lane's effect.
  The corpus stats runs still use the protocol's binaries as written.

## 7. Not in scope

- Free-prototype node emission (D6d; blocked on a program decision — see M4).
- The store-side fix for multi-file same-id symbols (file-flap / delete-by-file / digest-skip) —
  store paths are MUST-NOT-TOUCH; filed to the program via the extended M4, and the member-level
  instance this lane introduces is pinned (D8) and release-noted (M1).
- `remove_file`/store paths, lsp.rs, plugin.rs, resolve crate, version files, TS/JS import captures
  (lane MUST-NOT-TOUCH).
- NodeKind::Method restoration for Rust/Go/Python methods (D3; program follow-up).
- `class << Foo` / `def Foo.m` Ruby id-shape changes (defs KEPT ownerless per D5.3; the residual
  merge is fixture-pinned); multi-level C++ qualification (`Ns::Foo::bar` at file scope);
  decltype/dependent_name C++ scopes (defs kept, flat); Rust unanchorable impl targets (`&Foo`,
  tuples, `dyn`); trait-qualified Rust descriptors; TS/JS computed-name fields — all
  documented/pinned residuals, not silent gaps.
- Module-scope object-literal consts (D6).
- MI-R1-1 retirement (D7 — kept as a fleet guard with a direct unit test).
- Bench-corpus receipt REGENERATION (program-owned per extraction-gaps merge note 3) — but
  `cargo test -p wicked-estate-bench` IS run and its result recorded in S7 (ATK-3): not regenerating
  receipts never licenses shipping without knowing whether the crate went red.
- Fixing the Swift-extension sibling (and any further fleet-audit hits) in their languages — pinned
  or ADR-documented here, handed to the program as M6.

## 8. Merge notes for other lanes / the program

- **M1 (release lane):** ship this with #129/#130 in one release train; release notes must carry:
  forced re-extraction fires automatically **per repo label** (scheme 3) and the MCP server gives NO
  scheme-mismatch warning — multi-repo DBs must re-index every label (verify via the CLI or the
  per-label `id_scheme` meta keys); re-run `wicked-estate scip`; **re-inject overlay xedges /
  re-run the bus/command injection pipelines** (keyed on old ids, not carried over); crew graphs
  need a forced refresh; annotations on churned ids orphan; the C++ cross-file member proto/def
  single-id hazard (D8/F13) is live until the M4 store issue is resolved; if
  `cargo test -p wicked-estate-bench` went red from id churn (recorded verbatim in this lane's
  close-out), regenerate the bench receipts in the train.
  **Committed home (review round 1, impact-1-IMP-1):** the user-facing half of this note now
  lives in the repo itself — `CHANGELOG.md` `[Unreleased]` carries the scheme-3 entry (superseding
  the stale scheme-2 text in place, same discipline as the `treesitter.rs` scheme doc): the five
  newly-churned id classes, the per-label re-index gate + CLI-only warning, the SCIP/xedge/
  embeddings re-run instructions, and the live C++ cross-file proto/def single-id hazard. M1 was
  previously routed only through this lane's uncommitted report prose — a plan defect: release
  content must land in a committed artifact, not hand-off text. The release lane still owns the
  train composition and the bench-receipt regeneration.
- **M2 (method-identity lane, informational):** rust.scm's :24-30 NOTE ownership claim resolved
  here; `identity_field_object_literal_residual` and `identity_field_orm_equal_range_residual`
  flipped here per their embedded instructions; MI-R1-1 kept with a direct test, its python producer
  removed.
- **M3 (extraction-gaps lane / doc):** D6d free prototypes remain deferred; the deferral NOTE in
  cpp.scm now cites this plan; the per-parent pattern set in extraction-gaps.md §D6(d) remains the
  ready-to-land design.
- **M4 (program owner — ACTION REQUIRED, EXTENDED in revision):** record the header/impl identity
  DECISION covering **both free functions AND class members**: either (a) proto+def are one logical
  symbol (SCIP-like), with the store-side multi-file hazard (nodes.file flap + remove_file
  delete-by-file + digest skip) **filed as a wicked-estate store issue and fixed store-side**, or
  (b) declarations get distinct identity (accepting duplicate same-named candidates → resolver
  parking on C/C++ corpora). Until recorded: D6d prototype emission stays deferred per its own
  terms, and the member-level single-id shape this lane introduces (D8) stays pinned as
  `cpp_member_proto_def_cross_file_single_id_hazard` with its flip instruction gated on this
  decision.
- **M5 (doc-03 owner):** `ruby.scm:100 @call.object` untouched and still unconsumed — unchanged input
  for the self-receiver resolver work.
- **M6 (program owner — fleet audit hand-off, per ATK-4/§11):** the S7 fleet audit's hit list of
  "members captured, container not" constructs across the 73 query files. Known already: Swift
  `extension Foo` (pinned or ADR-documented in S7; the `.anchor` role now makes the fix expressible
  per-language as pure `.scm` data). The program schedules the per-language fixes; this lane does
  not fan out into N more languages.
