# scm-anchors — closing the residual identity collisions scheme 2 could not express

Lane: `fix/scm-identity-anchors` (base d7d3b58; scheme 2 + the def/pass-2 seam landed at 764622f, #130).
Plan status: **decided** — every decision below is final for this lane; deviations from the brief's
letter are recorded in §6 with the evidence that forced them.

Baseline (verified in this worktree, `CARGO_TARGET_DIR` = lane target):
`cargo test -p wicked-estate-extract` = 362 unit + 108 integration + 3 doctests, 0 failed,
1 pre-existing ignored doctest; `cargo test -p wicked-estate` = 153 passed, 0 failed.

---

## 1. Findings acted on (citations = files opened in this worktree)

| # | Finding | Evidence |
|---|---|---|
| F1 | Rust impl-block methods collide: rust.scm has NO impl_item pattern (deliberate — the #129 double-emit scar); impl methods match the general `function_item` pattern and stay module-flat, so two impls' same-named methods mint one SymbolId. | `src/queries/rust.scm:4-30` (patterns + NOTE); review "Engine defects" #1; risks-lens empirical: two impls' `save()` → 1 node at base |
| F2 | Go receiver methods collide: `method_declaration` captures name/params/result/body only — no `receiver:` capture; methods are top-level siblings of their type, so NO containment anchor can ever exist. | `src/queries/go.scm:12-18`; `treesitter.rs:1396-1399` (enclosing_chain requires strict containment, excludes range-equal) |
| F3 | Ruby: `class << self` matches nothing (`singleton_class` pattern requires `value: (constant)`), so its methods nest under the enclosing class and merge with instance methods; `def self.m` (singleton_method) also merges with instance `def m`. No pin exists for either shape today. | `src/queries/ruby.scm:9-13, 29-34`; `tests/fixtures/sample.rb` has `def self.checksum` but no `class << self` |
| F4 | TS object-literal fields: `public_field_definition` is captured only with `value: (arrow_function)`, so `x = { save(){} }` is invisible as a container and the literal's `save` merges with the class's real `save()` — pinned by `identity_field_object_literal_residual`. | `src/queries/typescript.scm:28-31`; `src/treesitter.rs:6550-6571` (pin + its own flip instruction) |
| F5 | Python ORM equal-range residual: both ORM patterns anchor `@code_field.def` at the WHOLE `class_definition`, so the field record is range-equal to the class and can never take it as owner — pinned by `identity_field_orm_equal_range_residual` (fix named in the pin itself: anchor at the assignment node). | `src/queries/python.scm:48-59, 71-85`; `src/treesitter.rs:6653-6683` |
| F6 | C++ out-of-line member vs free function: the D6e pattern captures only `name: (identifier)` inside `qualified_identifier` — the qualifier (`Foo::`) is discarded, so `void Foo::reset() {}` shares the free `void reset() {}`'s id. Pinned (residual half) with explicit flip instruction. | `src/queries/cpp.scm:43-52`; `tests/languages.rs:1412-1478` |
| F7 | C++ free prototypes (D6d) were deferred on a data-loss MECHANISM, not just a missing owner: `foo.h` proto + `foo.cpp` def mint one SymbolId (module strips one extension), `nodes.file` flaps last-write-wins, `remove_file` deletes by file, the digest skip never re-extracts the survivor. Deferral terms: "lands only after the program records an owner **+ decision**". Only the owner is recorded. | `src/queries/cpp.scm:55-63` (deferral NOTE); `docs/recon/extraction-gaps.md` §D6(d) + merge note 2; RESOLUTION-PROGRAM.md:41 (owner only) |
| F8 | Mechanism constraints: every PendingDef mints a Node + a Contains edge (no anchor-only record); `enclosing_chain` is strict-containment only; `classify_capture` accepts def suffixes `.def/.arrow/.decl/none` + `.name`/`.name.symbol`, everything else auxiliary-ignored; dedup keeps the LAST (start,end,name) record under uncontracted match order. | `src/treesitter.rs:2196-2216` (mint loop), :2258-2270 (Contains), :1396-1399, :1771-1800 (classify_capture), :2176-2194 (dedup) |
| F9 | Id migration is gated on one constant: `SYMBOL_ID_SCHEME = "2"` (`treesitter.rs:1354`); the gate compares the per-repo `id_scheme` meta key and forces full re-extraction on mismatch. No gate hashes the built-in `.scm` files — a query-only id change at an unchanged scheme silently mixes ids in existing DBs. | `src/treesitter.rs:1345-1355`; `wicked-estate/src/lib.rs` gates (version / extra-rules / id_scheme); `wicked-estate/tests/id_scheme.rs` references the const symbolically |
| F10 | Equal-range `@code_field.def` producer audit: of the 11 `.scm` files emitting `@code_field.def`, only **python.scm** anchors it at a node wider than the field itself (the `class_definition`). apex/cobol/cpp/csharp/d/go/java/pony/solidity/typescript all anchor at the field-level node. | `grep -B2 "@code_field.def" src/queries/*.scm` (run in this worktree, 2026-08-29) |
| F11 | The suffixes `.anchor` and `.owner` are unused across all query files — free namespace for new generic roles. | `grep -rn "\.anchor\b\|\.owner\b" src/queries/` = 0 hits |

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
    (scoped_type_identifier name: (type_identifier) @code_struct.name)
  ]
) @code_struct.anchor
```

- `impl Foo` and `impl Trait for Foo` both anchor under **Foo** (the `type:` field; the `trait:`
  field is deliberately not captured) — per the method-identity handover.
- The alternation normalizes `impl Foo<T>` and `impl Trait for crate::Foo` to the bare `Foo` in the
  QUERY (data, not Rust) — otherwise a path-qualification refactor would change method ids, the
  rename-breaks-identity class ADR-002 exists to prevent. Unanchorable impl targets (`&Foo`, tuples,
  `dyn Trait`) match nothing and their methods stay module-flat: accepted + listed in ADR-002
  residuals.
- **New residual, pinned:** two trait impls on ONE type (`Display for Foo` / `Debug for Foo`) still
  collide at `Foo#fmt().` — trait-qualified descriptors would need a disambiguator
  (`identity_disambiguator_is_none` pins None, per ADR-002) or a third descriptor with no recorded
  convention. Pinned as a known_defect-style test with a flip instruction; ADR residual entry added.
- **NodeKind stays Function** (no Method restoration): kind derivation from the chain is per-language-
  adjacent Rust beyond this lane's surface, and an impl-scoped duplicate Method pattern is the exact
  #129 nondeterministic-kind scar. `rust.scm:24-30`'s NOTE is rewritten (it names the method-identity
  lane as owner of what this lane now lands; the kind sentence stays, re-pointed to a program follow-up).

### D4 — Go: receiver owner capture on the existing method pattern

Add to the single `method_declaration` pattern (no duplicate pattern → no dedup interplay):

```scm
receiver: (parameter_list
  (parameter_declaration
    type: [
      (type_identifier) @code_method.owner
      (pointer_type (type_identifier) @code_method.owner)
      (generic_type type: (type_identifier) @code_method.owner)
    ]))
```

`func (r *T) M()` and `func (r T) M()` both mint `<module>/T#M().` — sharing the `T#` prefix with
`type T`'s own `<module>/T#` node, the scheme-2 shape.

### D5 — Ruby: both singleton forms converge on `C#self#m().`; pin first, then fix

1. **Pin first** (separate red-shape evidence, house style of the cpp pin): a new fixture with
   instance `def m`, `def self.m`, and `class << self; def n; end` in one class; a test asserting
   today's merges (`C#m().` ×1 for both m's; `C#n().` ×1 for both n's) with flip instructions.
2. **Fix:** new pattern `(singleton_class value: (self) @code_class.name body: (_) @code_class.body)
   @code_class.anchor` — the anchor's name is the `self` keyword's text, so `class << self` members
   nest as `C#self#m().` (chain: C → self, both Type). And `object: (self) @code_method.owner` added
   to the `singleton_method` pattern, so `def self.m` mints the SAME shape `C#self#m().` via the
   owner splice. The two syntactic spellings of a Ruby class-method converge on one id shape and are
   distinct from instance `C#m().` — the reason "self" wins over any invented name.
3. Unchanged + documented as residuals (ADR-002 list): `class << Foo` (already captured as
   `@code_class` named Foo — reopens `Foo#`'s namespace, merging with `class Foo`; its members land
   at `Foo#k().` not `Foo#self#k().`) and `def Foo.m` (receiver a constant — owner capture is
   restricted to `(self)`; splicing an arbitrary constant under the enclosing class would mint
   `C#Foo#m().`, an unrelated-type nesting). Smallest correct change; both shapes predate this lane.
4. `ruby.scm:100`'s `receiver: (_)? @call.object` is doc-03's declared input — untouched.

### D6 — TS/JS: object-valued class fields become Term defs; the split creates a NEW pinned residual

- typescript.scm + tsx.scm: `(public_field_definition name: (property_identifier) @code_field.name
  value: (object)) @code_field.def`; javascript.scm: the grammar-divergent
  `(field_definition property: (property_name/property_identifier) @code_field.name value: (object))
  @code_field.def` — per-grammar fixtures because a zero-match pattern fails silently (§11 sibling
  trap; javascript.scm:47 already documents the field_definition/public_field_definition divergence).
- Effect (zero Rust, per the pin's own instruction): the field is now a Term container, the
  truncation rule makes the literal's `save` **module-flat** — distinct from `A#save().`.
- Flip `identity_field_object_literal_residual` to assert the split AND pin the NEW residual in the
  same test: the literal `save` at `src/a/save().` now collides with any same-named module-level
  function (inexpressible without object-literal descriptors — a scheme change; ADR residual entry).
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

### D8 — C++ (D6d split): land the qualifier owner (flips the pin); free prototypes stay DEFERRED

- **Land:** `scope: (namespace_identifier) @code_method.owner` on the D6e out-of-line pattern
  (`cpp.scm:48-52`) — `void Foo::reset() {}` mints `<module>/Foo#reset().`, distinct from the free
  `reset()`. This — not a prototype capture — is what the pinned collision test actually pins: the
  out-of-line DEFINITION vs the free DEFINITION. Flip the residual half of
  `cpp_out_of_line_member_vs_free_function_collision_known_defect` (assert_eq → assert_ne) per its
  own instruction; the first (scheme-2) half stays intact. Single-level qualification only, matching
  the pattern's existing stated limit (`Ns::Foo::bar` at file scope remains unmatched — residual
  already documented in cpp.scm).
- **Do NOT land free-prototype capture.** The brief's premise ("the deferral condition is met") is
  incomplete against the recorded deferral terms: extraction-gaps.md merge note 2 requires "an owner
  **+ decision** for free-function header/impl node identity"; RESOLUTION-PROGRAM.md:41 records only
  the owner — no decision text exists anywhere in the program artifacts. Landing the capture re-opens
  the proven remove_file data-loss path (F7) whose mechanism lives in store paths this lane MUST NOT
  touch. The brief's own flip conditional ("if the fix resolves it") is moot regardless: prototypes
  change neither colliding id. The ready per-parent pattern set stays recorded in extraction-gaps.md
  §D6(d) verbatim; the cpp.scm deferral NOTE is updated to cite this plan and the still-missing
  decision. **Escalation (merge note M4):** the program must record the identity decision
  (one-logical-symbol proto/def merge with the store hazard filed as an estate issue, vs. distinct
  decl identity) before any lane lands D6d prototypes. Recorded as deviation DV-3 (§6).

### D9 — Measurements: fixture-level BEFORE = lane-base build; corpus stats per protocol

The protocol's BEFORE (main checkout release binary) and the lane base are both 764622f-era, but the
release binary's provenance is unverified from this lane; and the two prescribed corpora
(wicked-studio, wicked-crew) are TS/JS-only — they exercise ONLY the D6 object-literal change.
Therefore: per-language collision measurements (Rust/Go/Ruby/C++/Python) run the collision fixtures
through a **lane-base (764622f) debug build** as BEFORE vs the lane HEAD build as AFTER, via a copy
of the main checkout's untracked `examples/dump_ndjson.rs` (copied in, never committed). Corpus
before/after (index + `stats` nodes/edges/unresolved + one name-based `blast-radius --json` on a
previously-merged method) runs per the protocol's binaries into DBs under the lane `measure/` dir,
queried with `/usr/bin/sqlite3` (`.schema` first; kinds are JSON strings). Corpus "no change" for
Rust/Go/Ruby/C++ is expected and is NOT evidence the anchors are inert — the fixture numbers are.

### D10 — Coverage matrix: `--check` only

No `languages.toml` cap axis changes (anchors/owners add no capability string). Run
`python3 scripts/gen-coverage-matrix.py --check` after all `.scm` edits; expected exit 0. Regenerate
only if it exits 1.

---

## 3. Step list

Every step: work only in this worktree; `export CARGO_TARGET_DIR=<lane>/target`; per-crate cargo
only; commit `--no-verify` with the two mandated trailer lines; after each language run the full
`cargo test -p wicked-estate-extract` (fleet guard `assert_no_conflicting_def_ids` runs 22× inside
it) and keep 0 failed / no new ignored.

**S1 — seam + Rust (one commit: the seam lands with its first consumer, §5)**
- Files: `crates/wicked-estate-extract/src/treesitter.rs` (classify_capture `.anchor` + `.owner`
  arms; `DefAnchor{emit}` / new `DefOwner` role; `PendingDef{emit, owner}`; match-loop `def_owner`
  local with the kind-equality guard; pass-2 owner splice + `!emit` skip of Node/DefRec/Contains;
  `SYMBOL_ID_SCHEME` "2"→"3" + doc comment), `src/queries/rust.scm` (impl anchor pattern per D3;
  rewrite the :24-30 NOTE), `tests/fixtures/sample.rs` (add a second impl with a same-named method +
  `impl Trait for` forms), `tests/languages.rs` (new `rust_impl_methods_nest_under_type` distinctness
  test modeled on `go_const_vs_struct_field_symbolids_are_distinct`; new
  `rust_same_type_trait_impls_collision_known_defect` pin per D3), `treesitter.rs` unit tests (anchor
  emits no node / no Contains edge / no DefRec join for the impl range; owner splice id shape).
- Tests proving it: the new distinctness test (Point#translate ≠ Other#translate); the no-node
  assertions; `rust_characterization` unchanged (kinds stay Function); full crate suite green;
  `cargo test -p wicked-estate` green (id_scheme tests symbolic).
- Deletes: the stale ownership sentence in rust.scm's NOTE; scheme "2" doc text (superseded in place).

**S2 — Go**
- Files: `src/queries/go.scm` (receiver alternation per D4), `tests/fixtures/sample.go` (add
  `type B` + `func (b B) Area()`-style same-named receiver method), `tests/languages.rs`
  (`go_receiver_methods_nest_under_receiver_type`: A#M ≠ B#M; value + pointer receivers same shape).
- Tests: the new distinctness test; `go_characterization` (assert_def name+kind only — unchanged);
  fleet guard; full crate suite.
- Deletes: nothing (pure capture addition; ADR residual entry for Go is removed in S7).

**S3 — Ruby (two commits: pin, then fix+flip)**
- 3a Files: `tests/fixtures/` new `singleton.rb` (or extend sample.rb), `tests/languages.rs` new
  `ruby_singleton_vs_instance_collision_known_defect` pinning today's merges with flip instructions.
- 3b Files: `src/queries/ruby.scm` (singleton_class `(self)` anchor pattern + singleton_method
  `object: (self) @code_method.owner` per D5 — `:100 @call.object` untouched), flip the 3a pin to
  assert `C#m().` ≠ `C#self#m().` and `C#n().` ≠ `C#self#n().` and that both singleton forms mint the
  SAME id; document `class << Foo` / `def Foo.m` residuals in the test comment + ADR list.
- Tests: the flipped pin; `ruby_characterization`; fleet guard; full crate suite.
- Deletes: 3b re-points the 3a pin (never deletes it).

**S4 — TS/TSX/JS object-literal fields**
- Files: `src/queries/typescript.scm`, `src/queries/tsx.scm`, `src/queries/javascript.scm` (object-
  valued field patterns per D6 — import-capture sections untouched), `src/treesitter.rs` (flip
  `identity_field_object_literal_residual`: assert the split + pin the new module-flat residual),
  per-grammar fixture additions so each pattern provably matches (zero-match fails silently).
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

**S6 — C++ qualifier owner + pin flip**
- Files: `src/queries/cpp.scm` (add `scope: (namespace_identifier) @code_method.owner` to the D6e
  pattern; update the D6d deferral NOTE to cite this plan + the missing program decision),
  `tests/languages.rs` (flip the residual half of
  `cpp_out_of_line_member_vs_free_function_collision_known_defect` to assert_ne, keep the first half),
  fixture already exists in the test's probe string.
- Tests: the flipped pin (out-of-line `Foo#reset().` ≠ free `reset().`, and equals the in-class
  prototype's id — assert that too: proto + out-of-line def now correctly merge under `Foo#`);
  `cpp_characterization` + fleet guard; full crate suite.
- Deletes: the assert_eq residual half (re-pointed, not deleted). Free prototypes NOT landed (D8).

**S7 — ADR + measurements + fleet close-out**
- Files: `docs/adr/ADR-002-stable-symbol-identity.md` (Accepted-residuals list: remove Rust impl /
  Go receiver / Ruby singleton / object-literal / ORM entries; add the new pinned residuals from
  D3/D5/D6; note scheme "3" in the migration section), measurement artifacts under the lane
  `measure/` dir (never committed: dump_ndjson copy, DBs, command transcripts).
- Commands: fixture before/after per D9 (all five languages); corpus before/after per protocol
  (studio, crew); `python3 scripts/gen-coverage-matrix.py --check`;
  `cargo clippy -p wicked-estate-extract --all-targets -- -D warnings`;
  `cargo fmt -p wicked-estate-extract`; final `cargo test -p wicked-estate-extract` and
  `cargo test -p wicked-estate` with exact counts recorded.
- Deletes: the closed ADR residual entries (same change as their pins flipped — re-point contract).

---

## 4. Compatibility + migration

- **Ids churn** for: every Rust impl method, Go receiver method, Ruby singleton member, C++
  out-of-line member, TS/JS object-literal field member, Python ORM field. The scheme "3" bump (D2)
  forces a full re-extraction per repo via the existing id_scheme gate — no mixed graphs. Scheme 2 is
  unreleased, so released users (≤ v0.14.6, id_scheme absent) migrate ONCE for scheme 2 + 3 together.
- **After the forced re-extract per repo:** re-run `wicked-estate scip <root>` (remove_file deletes
  the confidence-1.0 SCIP edges by file); coverage/other annotations keyed to churned ids orphan
  (ADR-002 amendment's documented accepted loss) and need re-attachment; agent-held
  `resolve --json` symbol ids go stale.
- **Consumers:** ids are opaque strings everywhere (crew graph.ts queries by NAME; wire shapes
  unchanged). Numbers move: edges DOWN / unresolved UP where false 0.65 edges un-merge (the split
  makes >1 best-tier candidates park), node counts UP (new Field nodes for object-valued class
  fields), rank/hotspots ordering shifts. This is intended precision recovery, not regression — the
  S7 corpus measurements document it. Crew project graphs skip clean-HEAD repos entirely; operators
  must force a refresh (crew `RefreshOptions.force`) — release-notes item for the release lane.
- **No wire/schema/store change.** No languages.toml cap change → coverage matrix regen expected no-op.

## 5. Falsifier

Build the lane binary at plan completion and run the five collision fixtures through it
(dump_ndjson pattern): if ANY of — two Rust impls' same-named methods, two Go receiver types' `M()`,
Ruby `def m` vs `def self.m` (or `class << self` member vs instance), C++ `Foo::reset` vs free
`reset`, the TS object-literal `save` vs `A.save()`, the Python nested ORM field — still mints a
single shared SymbolId, the plan failed. Likewise if `cargo test -p wicked-estate-extract` or
`cargo test -p wicked-estate` is not green with every flipped pin asserting distinctness and zero
new ignored tests, or if a previously-green identity/characterization test was weakened rather than
re-pointed.

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

## 7. Not in scope

- Free-prototype node emission (D8; blocked on a program decision — see M4).
- `remove_file`/store paths, lsp.rs, plugin.rs, resolve crate, version files, TS/JS import captures
  (lane MUST-NOT-TOUCH).
- NodeKind::Method restoration for Rust/Go/Python methods (D3; program follow-up).
- `class << Foo` / `def Foo.m` Ruby shapes; multi-level C++ qualification (`Ns::Foo::bar` at file
  scope); Rust unanchorable impl targets (`&Foo`, tuples, `dyn`); trait-qualified Rust descriptors —
  all documented/pinned residuals, not silent gaps.
- Module-scope object-literal consts (D6).
- MI-R1-1 retirement (D7 — kept as a fleet guard with a direct unit test).
- Bench-corpus receipt regeneration (pre-declared acceptable drift, extraction-gaps merge note 3);
  the S7 fixture + corpus measurements are this lane's before/after evidence.

## 8. Merge notes for other lanes / the program

- **M1 (release lane):** ship this with #129/#130 in one release train; release notes must carry:
  forced re-extraction fires automatically per repo (scheme 3), re-run `wicked-estate scip`, crew
  graphs need a forced refresh, annotations on churned ids orphan.
- **M2 (method-identity lane, informational):** rust.scm's :24-30 NOTE ownership claim resolved
  here; `identity_field_object_literal_residual` and `identity_field_orm_equal_range_residual`
  flipped here per their embedded instructions; MI-R1-1 kept with a direct test, its python producer
  removed.
- **M3 (extraction-gaps lane / doc):** D6d free prototypes remain deferred; the deferral NOTE in
  cpp.scm now cites this plan; the per-parent pattern set in extraction-gaps.md §D6(d) remains the
  ready-to-land design.
- **M4 (program owner — ACTION REQUIRED):** record the free-function header/impl identity DECISION:
  either (a) proto+def are one logical symbol (SCIP-like), with the remove_file file-flap deletion
  hazard filed as a wicked-estate store issue and fixed store-side before the capture lands, or
  (b) declarations get distinct identity (accepting duplicate same-named candidates → resolver
  parking on C/C++ corpora). Until recorded, D6d prototype emission stays deferred per its own terms.
- **M5 (doc-03 owner):** `ruby.scm:100 @call.object` untouched and still unconsumed — unchanged input
  for the self-receiver resolver work.
