# Recon plan — extraction-gaps lane (review doc 04, D04-1..D04-10 + engine defect #7)

Base: `d7d3b58` on `lane/extraction-gaps`. Planner synthesis of four recon lenses
(history / consumers / tests / risks), all facts re-verified against files opened in this
worktree. Governing decision for every query change: **rules as data** —
`docs/DESIGN-NOTES.md:30-33`, `CLAUDE.md:178-182` ("languages are data; the capability matrix is
generated"). No ADR pins query content (all in-scope `.scm` files have exactly one commit,
`797ce58`), so no ADR amendment is needed.

**Revision 2** — resolves the attack round: EG-ATK-1/2/3, A1, A2, FEAS-1/2/3 (majors) plus the
minors folded in where they touch the same sections (EG-ATK-4/5/6, A3/A4/A5/A6, FEAS-4/5/6/7/8).
Each resolution is marked `[ATK]` inline. One partial adjudication (not a rejection): EG-ATK-2 and
A4 assert the D7 helper is red for **rust** at unmodified base; verified false — the flat fixture
`tests/fixtures/sample.rs` contains **zero** `impl` blocks (grep count 0 in this worktree), so
`rust.scm:25-32` never fires on it and rust is trivially green. The *intent* is honored the
FEAS-4 way: S1 extends `sample.rs` with an `impl` block so the rust double-emit becomes observable
(red pre-fix within the same working tree), then the fix lands in the same green commit.

## 1. Findings acted on (citations = files opened in this worktree)

| Finding | Fact | Evidence |
|---|---|---|
| D04-1 (six gaps real) | fixture.go 10 declared / 4 emitted; fixture.rb 11/4; Fixture.java 7/4; Fixture.cs 7/4; Fixture.swift 11/3; fixture.hpp 12/4; fixture.h 12/3 | `review-artifacts/doc04-fixtures/out.ndjson` (d7d3b58 NDJSON); BEFORE store at `<scratchpad>/lanes/extraction-gaps/measure/before.db` |
| D04-2 (Go catch-all double-match) | `def_suffix` maps `struct`/`interface`/`type` all to `Suffix::Type` → same `Name#` id; store upsert is last-write-wins | `treesitter.rs:1341-1348`; `sqlite.rs:389-395` (`ON CONFLICT(symbol) DO UPDATE SET … kind=excluded.kind`) |
| D04-3 (Swift init emits nothing bare) | def needs both anchor + name capture; `init_declaration` has required `name:` = anonymous `"init"` | `treesitter.rs` def-emission gate (~1868-1871, method-identity lane's region — read-only here); tree-sitter-swift-0.7.3 node-types |
| D04-4 (Ruby alias_method colon; attr_* omitted) | `strip_literal_quotes` strips only paired `'…' "…" <…>` — leading `:` survives | `treesitter.rs:1382-1392`; `ruby.scm:22-26` gates `name:` to `(identifier)` |
| D04-5/D04-6 (C++ prototypes/fields; `.h` → C grammar) | c owns `["c","h"]`, cpp owns `["cpp","cc","cxx","hpp","hh"]`; `cpp.scm:24-35` captures only identifier/field_identifier declarators of `function_definition`; `cpp.scm:4-16` struct/class/enum have no `body:` constraint (c.scm:4-13 does) | `treesitter.rs:567-577`; `languages.toml` c row (ext line 55), cpp row (ext line 90); `cpp.scm`, `c.scm` opened |
| D04-7 (Java/C# patterns verified) | `@code_interface`/`@code_method`/`@code_property` roles map with zero Rust: `"field"\|"property" → Field`, `interface → Interface` | `treesitter.rs:1350-1370` (`def_nodekind`) |
| D04-8 (no failing-before evidence infra) | `assert_def` = name+kind membership; `assert_def_floor` = `count >= floor`; no duplicate-id guard; flat `tests/fixtures/sample.<ext>` are the files characterization tests load | `tests/languages.rs:20-79` (`load_fixture` path template `tests/fixtures/{filename}`) |
| D04-9 (Swift `extends` cap false) | `languages.toml:479` caps include `"extends"`; `swift.scm` (66 lines, opened) has no `code_extends`/`code_implements` pattern | `languages.toml:474-479`; `swift.scm` full read |
| D04-10 (TypeAlias approximation for Go) | `"type_alias" \| "type" => NodeKind::TypeAlias`; unknown roles → `Other` + `Suffix::Term` (`Name.` not `Name#`) | `treesitter.rs:1365, 1346, 1368` |
| Defect #7 (.h routing + Swift matrix + dead file + dead `@call.object`) | as D04-6/D04-9; `queries/c-sharp.scm` exists and nothing references it (`treesitter.rs:40` wires `csharp.scm`; repo-wide grep for `c-sharp.scm` in recon: 0 refs, history = initial commit only); `ruby.scm:69-72` captures `receiver: (_)? @call.object` and treesitter.rs has NO handler for the role (grep `call.object` in src: 0 hits) — dead capture, see D13 [ATK FEAS-3] | `treesitter.rs:567-577`; `queries/c-sharp.scm` present; `ruby.scm:69-72` opened |
| §11 siblings of the Go defect class (recon finding, not in doc 04) | `python.scm:15-23` (class-body Method pattern) + `:25-29` (unanchored Function pattern) emit every Python method twice on the SAME anchor node with different kinds; `rust.scm:4-7` + `:25-32` identical shape — but the flat `sample.rs` has no `impl` block, so the rust leg is unobservable until the fixture gains one [ATK FEAS-4/A4]. BEFORE store shows the re-kind is live: sample.py methods stored as `"function"` | `python.scm`, `rust.scm`, `tests/fixtures/sample.rs` opened; recon BEFORE DB `measure/before-db/py.db` |
| §11 sibling: C typedef-struct collision | `typedef struct Vector2 Vector2;` (flat sample.c:7) mints `sample/Vector2#` twice — TypeAlias + Struct, distinct anchors, kind conflict; `c_characterization` asserts BOTH (`languages.rs` ~404/412), passing only because the in-memory Extraction keeps both while the store keeps one. The anonymous form `typedef struct { … } Account;` is live at `tests/fixtures/c/sample.c:5-9` and MUST keep working [ATK FEAS-2] | recon probe DBs; both sample.c files opened |

## 2. Decisions (all explicit — no TBD)

**D1 — Go defined types emit as `@code_type` → `NodeKind::TypeAlias`.**
Rationale: zero core change (`treesitter.rs:1365` already maps it), correct id suffix
(`Suffix::Type` → `Name#`, `treesitter.rs:1344`); an unknown role like `newtype` would fall to
`Other` + `Suffix::Term` (`treesitter.rs:1346,1368`) giving types the wrong id shape, WARN noise in
wicked-governance's `unknown_other` bucket, and no bucket in garden's `emit_domain_model.py`.
The Go-semantics loss (`type X T` is a distinct type, not an alias) is recorded as a deliberate
approximation in a `go.scm` comment + DESIGN-NOTES line, per D04-10's recommendation.

**D2 — `.h` routes to the cpp grammar as DATA; no content sniff.**
Move `"h"` from the c row to the cpp row in BOTH `treesitter.rs:569/575` (LANG_TABLE) and
`languages.toml` (c ext line 55 → cpp ext line 90), then regenerate the matrix.
A sniff is unreachable from this lane's allowed surface: dispatch is extension-keyed before content
is read (`wicked-estate/src/lib.rs` builds `ext_map` once, ~608-623, and `base_extraction` does
`ext_map.get(ext)` at ~210 — a file this lane must not touch; the IaC-sniff precedent lives at
lib.rs ~175-208 in the same file), `extractor_for_extension(ext: &str)` has no content parameter
(`treesitter.rs:1260-1271`), and each extractor holds exactly one grammar (`:1214-1222`). A sniff
inside the C extractor would be a per-language Rust arm (forbidden).
Evidence the superset route is acceptable: recon recorded the cpp grammar parsing real C headers at
+2.5% error nodes and 1.8× faster than c. **[ATK EG-ATK-6]** The recon carried two corpus counts
(1,654 headers enumerated vs 1,438 used for the error-rate figure) without pinning their
relationship; the D2 decision does NOT hinge on the exact count (the sniff is unreachable in-lane
regardless), but the executor MUST re-derive the corpus in S11 with a recorded enumeration command
(e.g. the `find` over SDK/homebrew include dirs) and pin ONE count + error-rate pair used in both
this doc and the lane report. Measured costs are (a) C identifiers that are C++ keywords are
dropped (probe: `int template(...)` lost) and (b) forward declarations / elaborated uses become
phantom Struct nodes — cost (b) is eliminated by D6's `body:` constraints, which therefore MUST
land in the same commit as the routing change. Cost (a) is accepted and documented in the matrix
regen commit.

**D3 — Ruby symbol names: one generic strip of a leading `:` at the DEF-NAME seam only
(`treesitter.rs:1809`), NOT inside `strip_literal_quotes`. [ATK EG-ATK-1 / FEAS-7 — choice (a)]**
`strip_literal_quotes` is the shared seam for SIX capture channels: def names (:1809), route paths
(:1842), event topics (:1854), event emit topics (:1860), call refs (:1915, :1917), and imports
(:1934) — verified by grep in this worktree. Placing the colon strip inside it would rewrite every
ref/import/route/event name in all 73 languages: elisp keywords legitimately start with `:` in call
position (`elisp.scm` `(symbol) @call.function`) and racket symbol imports likewise — `:foo`→`foo`
there would silently change stored raw_names. Therefore: add a tiny generic helper next to
`strip_literal_quotes` (e.g. `strip_leading_symbol_colon`) and apply it at exactly ONE call site,
the `CaptureRole::DefName` arm (`treesitter.rs:1809`):
`def_name = Some((kind, strip_leading_symbol_colon(strip_literal_quotes(&text))))`.
Semantics: strip a single leading `:` only when not followed by another `:` and the remainder is
non-empty (`::X` and `:` unchanged). Still zero per-language Rust; the other five channels are
byte-for-byte untouched, so no ref/import audit is needed. Rationale for stripping at all: a query
predicate cannot rewrite text (tree-sitter has no transform predicates), and accepting
`:other_name` repeats the exact §11 scar this seam exists for ("unqueryable by real name" —
CLAUDE.md §11; precedent commit `3f0bdde` added the DefName strip for VB6). Def-name blast radius
audited: the only other `.scm` capturing an atom/symbol leaf as a def name is `prolog.scm:4`
`(atom)`, and Prolog atoms never begin with `:` (a *quoted* atom like `':-'` reaches DefName only
via strip_literal_quotes' quote removal — the non-empty + not-`::` guard leaves `:-` → `-` a
theoretical case; no prolog def-name capture feeds quoted atoms today, re-checked in this
worktree). Unit tests go next to the existing strip tests (treesitter.rs test module ~2765):
`":x"`→`"x"`, `"::X"` unchanged, `":"` unchanged, and one asserting `strip_literal_quotes` alone
still preserves `":keyword"` (pins that refs/imports are untouched).
This is the lane's second Rust touch beyond the routing table — flagged in merge notes.

**D4 — Swift: `init` via anonymous-token name capture; `deinit` included but compile-gated;
compile gate generalized to EVERY .scm edit. [ATK FEAS-8]**
`(init_declaration name: "init" @code_method.name) @code_method.def` (D04-3-verified, 2 matches on
the fixture). `(deinit_declaration "deinit" @code_method.name) @code_method.def` was probe-verified
by the risks lens (1 match on Fixture.swift under 0.7.3) even though node-types lists no `name`
field — because a non-compiling `.scm` silently drops its ENTIRE language (`for_language` returns
`None`), the executor runs `cargo test -p wicked-estate-extract every_wired_query_compiles`
immediately after **every `.scm` edit in S1-S9** (not just swift.scm). Known fallbacks if a pattern
fails to compile: swift deinit → delete pattern + record under not_done; ruby
`(alias name: (_method_name) …)` → fall back to `name: (_)` (the field constraint already restricts
to method-name position). Never commit a `.scm` that doesn't compile.

**D5 — Swift properties scoped to type bodies, not bare.**
The review's bare `(property_declaration name: (pattern (simple_identifier)))` also matches every
function-local `let/var` and top-level globals (risks-lens probe: 16 matches on swift/sample.swift,
2 with parent `statements`). Emit `@code_property` only under `(class_body …)` and
`(enum_class_body …)`. Protocol requirements (`protocol_property_declaration`) are left out —
smallest change; the doc-04 declared list doesn't include them.

**D6 — C++: `body:` constraints + pattern groups (b)(c)(e); free prototypes (d) DEFERRED.
[ATK A2]**
(a) Add `body:` to cpp.scm's struct/class/enum patterns (`cpp.scm:4-16`), matching c.scm's shape —
kills phantom Struct/Class/Enum at forward declarations and elaborated uses (`struct Foo *p;`),
which otherwise re-kind real definitions via the last-write-wins upsert and double under `.h`
routing (+105% phantom Structs measured on the header corpus).
(b) Member prototypes: `(field_declaration declarator: (function_declarator declarator:
(field_identifier) @code_method.name)) @code_method.def` — verified 4/4 on fixture.hpp (bar,
reset, pure, m). This also correctly classifies pure virtuals (`= 0` parses as field_declaration).
(c) Member fields: field_declaration with a declarator **alternation**
(`field_identifier` / `pointer_declarator` / `array_declarator` / `reference_declarator` paths) —
NOT a wildcard, which also captures `function_declarator` and would emit prototypes as Field
(kind conflict). Verified 5/5 vs 6-with-false-positive by the risks probe.
(d) **Free prototypes: DEFERRED, not implemented in this lane.** The attack proved a new
incremental-index data-loss path with no committed owner: with `.h`→cpp routing, `void bar();` in
`foo.h` mints the SAME SymbolId as the pre-existing `foo.cpp` definition node (module = path minus
extension), the store's `file` column flaps to whichever file indexed last, `remove_file`
deletes nodes WHERE file=?1 (`wicked-estate-store/src/sqlite.rs:1693-1756`), and the digest skip
(`wicked-estate/src/lib.rs:9-14`) won't re-extract `foo.cpp` — so deleting `foo.h` can permanently
delete a live definition node until a version bump or `--force`. No lane in RESOLUTION-PROGRAM.md
owns free-function node identity (method-identity lane = enclosing-type METHOD ids only; resolve
lane = edges, not node identity). Free prototypes are the ONLY group in D6 that collides with
PRE-EXISTING nodes; groups (b) and (e) collide only with each other (both new nodes for the
semantically-same member — additive, not a regression). Deferral terms: the per-parent anchored
pattern set (translation_unit / preproc_ifdef / preproc_if / declaration_list /
template_declaration — probe: review's translation_unit-only anchor captures 0 in include-guarded
headers vs 3/3 per-parent, 0 false positives on extra.cpp) is recorded HERE as the ready-to-land
design; it lands only after the program records an owner + decision for free-function header/impl
node identity (merge note 2). S11 quantifies collision prevalence on a real header-heavy corpus
(the bench-pinned tree-sitter/tree-sitter repo, `crates/wicked-estate-bench/src/lib.rs:113-118`)
to inform that decision. Free prototypes move to the DEFERRED list in §5's declared counts and to
not_done.
(e) Out-of-line member definitions: `(function_definition declarator: (function_declarator
declarator: (qualified_identifier name: (identifier) @code_method.name))) @code_method.def` —
`void Foo::reset() {}` is captured by NOTHING today (`cpp.scm:24-35` matches only
identifier/field_identifier). Probe-verified 1/1. Included because it is a pure cpp.scm data
addition and its collision with (b) is additive (see (d)).
Classification note (D04-5): `Foo() = default;` is already emitted as Function by `cpp.scm:24-28`
(body is optional in 0.23.4) — left as-is; `~Foo() = delete;` (destructor_name declarator) stays
uncaptured — recorded as not_done, out of the declared list's scope.

**D7 — Item-7 helper = per-file "no SymbolId emitted with more than one distinct NodeKind".**
The brief's literal form ("no two definition nodes share a SymbolId") fails at base for 13/32
fixture files for three distinct reasons; the brief's suggested fallback ("same anchor node emitted
twice") would NOT have caught the Go defect (the struct pattern anchors on `type_declaration`, the
catch-all on `type_spec` — different anchors). The kind-conflict form catches exactly the defect
class this lane owns (Go Struct-vs-TypeAlias, Python/Rust Method-vs-Function, C
TypeAlias-vs-Struct, C++ Class-vs-Struct phantoms) while passing the method-identity lane's
same-kind collisions (Java `run()` ×2 = Method+Method). It is not a weakening: it is the exact
predicate for "the store silently re-kinds a definition". The same-kind duplicate list is handed to
the method-identity lane as data (merge notes), not asserted on.
**[ATK EG-ATK-5] Contingency**: if the helper turns red at base for a language whose `.scm` this
lane must not touch (typescript/javascript/tsx — relative-imports lane; or anything surfaced by
method-identity churn), the test is NOT excluded and the helper is NOT weakened — the collision is
recorded under not_done with the language + colliding (id, kinds, lines) and handed to the owning
lane; the helper call for that test still lands, with the finding documented for program
adjudication.
**[ATK FEAS-1] Known blind spot, covered by an explicit assert**: the helper is kind-conflict-only,
so a spurious SAME-kind def (e.g. a Ruby alias_method pattern capturing the old name → second
Method node with the real method's SymbolId, whose `file`/location the upsert then flaps) passes
it. D9's alias_method pattern is shaped so this cannot happen, and S4 adds an explicit
count-assert (exactly one node named `original`) pinning it.

**D8 — Fix the §11 siblings that make D7's helper red at base: python.scm, rust.scm, c.scm + cpp.scm.**
- python.scm / rust.scm: delete the class-/impl-body Method pattern (`python.scm:15-23`,
  `rust.scm:24-32`), keeping the general Function pattern. This preserves **stored** behaviour
  exactly (BEFORE DB proves the store already keeps `"function"` for Python methods — last write
  wins; the blast-radius lens' empirical probe confirmed the same for rust impl methods), removes
  the duplicate node + duplicate Contains edge, and requires zero assertion changes
  (`python_characterization` already asserts `__init__`/`process` as Function). Kind-fidelity loss
  (methods emitted as Function) is pre-existing stored behaviour, recorded for the method-identity
  lane, which is the right place to restore Method kind together with enclosing-type identity.
  **[ATK FEAS-4]** The flat `tests/fixtures/sample.rs` has NO `impl` block, so the rust deletion
  would otherwise be an evidence-free by-analogy edit: S1 extends `sample.rs` with an `impl` block
  (e.g. `impl Point { fn translate(&mut self, …) {…} }`), adds `assert_def("translate", Function)`,
  and the helper — red on that extended fixture pre-fix — proves the rust double-emit before the
  pattern deletion lands in the same commit.
- c.scm AND cpp.scm typedef (**[ATK FEAS-2] exact replacement pattern set** — "the unconstrained
  non-struct typedef pattern" has no direct tree-sitter expression, and keeping the existing
  unconstrained pattern (`c.scm:34-37`, `cpp.scm:47-50`) would leave the Vector2 double-match
  intact since predicates gate only their own pattern). REPLACE the unconstrained pattern in both
  files with this set (predicates INSIDE the outermost parens — scar `7df786b`):
  (i) tag-named, self-name-suppressed — one pattern per tag kind:
  `(type_definition type: (struct_specifier name: (type_identifier) @_s) declarator:
  (type_identifier) @code_type.name (#not-eq? @_s @code_type.name)) @code_type.def`
  and the equivalents with `enum_specifier` / `union_specifier` (the self-name idiom
  `typedef enum Color Color;` exists for all three tags);
  (ii) anonymous-tag and non-tag types:
  `(type_definition type: [(struct_specifier !name) (enum_specifier !name) (union_specifier !name)
  (primitive_type) (sized_type_specifier) (type_identifier) (macro_type_specifier)] declarator:
  (type_identifier) @code_type.name) @code_type.def` — covers the most common C header idiom
  `typedef struct { … } Account;` (live at `tests/fixtures/c/sample.c:5-9`) and plain typedefs.
  Asserts: `Account` stays TypeAlias (anonymous form); a differently-named typedef
  (`typedef struct Vector2 Vec2;` added to the flat fixture) stays TypeAlias; self-named
  struct/enum typedefs emit NO second node (helper green + explicit assert_no_def-style check).
  `c_characterization`'s `assert_def("Vector2", TypeAlias)` is updated accordingly — a legitimate
  assertion change tracking a §11 fix, not a bar-lowering. cpp.scm gets the same set (the idiom is
  ubiquitous in C headers and required for the `.h`→cpp route).

**D9 — Ruby patterns. [ATK FEAS-1: alias_method split from attr_*]** Modify the existing
method/singleton_method patterns in place (`name: (identifier)` → `name: [(identifier) (setter)
(operator)]`, `ruby.scm:22-33`) — modifying avoids a second pattern double-matching plain
identifiers. Add `(alias name: (_method_name) @code_method.name) @code_method.def` (compile
fallback per D4: `name: (_)`). Then TWO separate call patterns — NOT one merged pattern, because a
merged every-symbol capture would emit a spurious Method def for alias_method's SECOND symbol (the
old name) with the SAME SymbolId and SAME kind as the real method, and the upsert
(`sqlite.rs:391`, `file=excluded.file`) would flap the real method's location — invisible to the
kind-conflict helper (Method+Method):
  (i) alias_method captures only the FIRST symbol via an anchored child:
  `(call method: (identifier) @_m arguments: (argument_list . (simple_symbol) @code_method.name)
  (#eq? @_m "alias_method")) @code_method.def`;
  (ii) attr_reader/attr_writer/attr_accessor keep the every-symbol capture in their own
  `#any-of?` pattern (`attr_accessor :a, :b` → one match per symbol).
Predicates inside the outer parens. Names rely on D3's def-name colon strip. Fixture adds
`alias_method :other_name, :original` where `original` is a real `def`; S4 asserts extraction
emits **exactly one** node named `original` (explicit count assert — the helper cannot see
same-kind duplicates, D7). `attr_reader "s"` (string arg) intentionally unmatched.

**D10 — Java/C#: merge as D04-7 verified.** `(annotation_type_declaration name: (identifier)
@code_interface.name body: (annotation_type_body) @code_interface.body) @code_interface.def` +
`(annotation_type_element_declaration name: (identifier) @code_method.name) @code_method.def`;
C# `(property_declaration name: (identifier) @code_property.name) @code_property.def` (covers
auto/expression-bodied/computed — name field is required in 0.21.3 for all three).
**Delete `queries/c-sharp.scm`** in the C# commit (§8): wired constant is
`include_str!("queries/csharp.scm")` (`treesitter.rs:40`), zero references, history = initial
commit only.

**D11 — Swift heritage: add patterns, keep the `extends` cap.** `(class_declaration name:
(type_identifier) @code_class.name (inheritance_specifier inherits_from: (user_type
(type_identifier) @code_extends.target))) @code_extends.def` (probe-verified: 5 matches over
class/struct/enum). Accepted, documented limitations: protocol conformance is syntactically
indistinguishable from superclass (all → Extends), `enum E: Int` emits Extends→Int (raw-value
type), `extension Foo: Equatable` unmatched. Update the stale `swift.scm:3` comment (0.7.1 →
0.7.3). Regenerate `docs/language-coverage-matrix.md` — after D2 + this, the diff is exactly the
c/cpp extension rows; the Swift `E` becomes true instead of being dropped.

**D12 — No version bump in this lane.** `CARGO_PKG_VERSION` lives in multiple files with a sync
script (release machinery, program-owned). Recorded under compatibility: without a bump or
`index --force`, existing DBs keep old nodes for unchanged files (digest skip has no query salt).

**D13 — `@call.object` (defect #7 third clause): KEEP the capture, recorded as declared input for
doc 03's planned self-receiver resolver. [ATK FEAS-3 / EG-ATK-4]**
`ruby.scm:69-72` captures `receiver: (_)? @call.object`; treesitter.rs has no handler for the role
(grep: 0 hits in src) → `CaptureRole::Other`, dead today. It is NOT deleted in S4: review doc 03
asks for a `@call.object`-driven self-receiver resolution improvement, making the capture the
declared consumer-referenced input (deleting it here would strand doc 03's ask). Listed explicitly
in §7 (with owner: doc-03/resolve work) and merge note 6 so the defect-#7 resolution accounting is
complete: routing = S9, Swift matrix = S7/S10, dead c-sharp.scm = S6, `@call.object` = kept-for-
doc-03 (this decision).

## 3. Step list (each step: files, tests proving it, deletions)

Setup (uncommitted): copy `dump_ndjson.rs` from the main checkout into
`crates/wicked-estate-extract/examples/` for local runs — **never `git add` it**; copy the doc04
fixtures into `<lane>/measure/`. Record baseline: `cargo test -p wicked-estate-extract` at
d7d3b58 = **446 passed / 0 failed / 1 ignored (pre-existing extra_edge doctest)** with
`CARGO_TARGET_DIR=<lane target>`; BEFORE NDJSON = `review-artifacts/doc04-fixtures/out.ndjson`;
BEFORE store = `measure/before.db`.

**[ATK EG-ATK-2 / FEAS-6] Green-tree rule for S1**: the helper and the fixes it turns red land in
ONE commit. The red-at-base demonstration is UNCOMMITTED evidence: with the helper + fixture
extension applied but the `.scm` fixes not yet, run the characterization tests, record the failing
output (expected red set: **python, c, and rust-on-the-extended-fixture** — rust is green on the
unmodified flat fixture, which has no `impl` block; see Revision note), paste it into the commit
message / lane report, then apply the fixes and commit green. Every commit on the lane branch is
green (§9).

| # | Change | Files | Test that proves it (fails pre-fix in-tree → passes at commit) | Deletes |
|---|---|---|---|---|
| S1 | ONE commit (subsumes former S2): shared helper `assert_no_conflicting_def_ids(&extraction, lang)` (per-file: no SymbolId with >1 distinct NodeKind among non-File nodes; failure message prints the colliding (id, kinds, lines)); call from every existing characterization test; §11 sibling fixes: delete python.scm:15-23 and rust.scm:24-32 duplicate Method patterns; replace c.scm + cpp.scm typedef pattern with D8's two-pattern set; extend flat sample.rs with an impl block | `tests/languages.rs`, `queries/python.scm`, `queries/rust.scm`, `queries/c.scm`, `queries/cpp.scm`, `tests/fixtures/sample.rs` (add impl block + translate), `tests/fixtures/sample.c` (add `typedef struct Vector2 Vec2;`), c asserts updated per D8 | Uncommitted red evidence: helper red for python + c at base, red for rust once the fixture has the impl block; green at commit. `assert_def("translate", Function)`; `Account` stays TypeAlias (`c/sample.c` anonymous form, integration corpus); Vec2 TypeAlias; no second Vector2 node; python_characterization unchanged; stored kinds unchanged via BEFORE/AFTER DB diff on sample.py/sample.rs | python.scm class-body pattern; rust.scm impl-body pattern; the unconstrained typedef pattern (both files); the phantom self-typedef node |
| S3 | Go: struct-field pattern + constrained type_spec alternation (D1); approximation comment | `queries/go.scm`, `tests/fixtures/sample.go` (add ID/Name/Tags-style fields, `type UserID string`, `type Handler func(int) error`, `type Matrix [][]float64`, an `a, b int` multi-name field), mirror into `tests/fixtures/go/sample.go`, `tests/languages.rs` (assert_def Field ×3+, TypeAlias ×3; helper proves no Struct/Interface re-kind) | go_characterization new asserts fail at base; helper stays green (the D04-2 catch-all would turn it red) | — |
| S4 | Ruby: setter/operator in-place edit, alias, SPLIT alias_method / attr_* patterns (D9); leading-`:` strip at the DefName seam only (D3); `@call.object` kept (D13) | `queries/ruby.scm`, `crates/wicked-estate-extract/src/treesitter.rs` (new `strip_leading_symbol_colon` helper beside :1382 + ONE call-site change at :1809 + unit tests beside ~2765 — nothing else), `tests/fixtures/sample.rb` + `tests/fixtures/ruby/sample.rb` (incl. `alias_method :other_name, :original` with `original` a real def), `tests/languages.rs` (assert_def `name=`, `[]`, `<=>`, `==`, `new_name`, `other_name`, `balance` as Method — bare names, no colon; count-assert exactly ONE node named `original`) | ruby_characterization new asserts fail at base; strip unit tests (`":x"`→`"x"`, `"::X"` unchanged, `":"` unchanged; `strip_literal_quotes(":keyword")` still `":keyword"`) fail at base; `original`-count assert would fail under the merged-pattern shape (FEAS-1 pinned) | — |
| S5 | Java `@interface` + elements (D10) | `queries/java.scm`, `tests/fixtures/sample.java` + `java/sample.java`, `tests/languages.rs` (Marker Interface; value/priority Method) | java_characterization new asserts fail at base | — |
| S6 | C# properties (D10); retire dead query file | `queries/csharp.scm`, **delete `queries/c-sharp.scm`**, `tests/fixtures/sample.cs` + `csharp/sample.cs`, `tests/languages.rs` (Id/Name/Total Field) | csharp_characterization new asserts fail at base; `cargo build -p wicked-estate-extract` proves nothing referenced the deleted file | `queries/c-sharp.scm` |
| S7 | Swift: type-body-scoped properties, init, deinit (compile-gated), heritage (D4/D5/D11); fix swift.scm:3 comment; NEW flat fixture + characterization test | `queries/swift.scm`, NEW `tests/fixtures/sample.swift`, `tests/fixtures/swift/sample.swift` (add init/deinit), `tests/languages.rs` (NEW swift_characterization: Point Struct, Box Class, x/y/sum/origin/item Field, init + deinit Method, Extends ref via assert helper; negative: no Field for a function-local `let`) | swift_characterization is entirely new (Swift had none); `every_wired_query_compiles` run after each .scm edit (D4) | stale 0.7.1 comment |
| S8 | C++: body: constraints, member prototypes, member fields (alternation), qualified out-of-line defs (D6 a/b/c/e). Free prototypes (d) DEFERRED per D6/A2 — the ready pattern set stays in this doc pending a program owner | `queries/cpp.scm`, `tests/fixtures/sample.cpp` + `cpp/sample.cpp`, NEW `tests/fixtures/cpp/guarded_header.hpp` (include guards + `#include <cstdint>` [ATK FEAS-5: .hpp carries the imports cap — a no-#include fixture hard-fails `fixture_files_produce_nodes`, `language_integration.rs:560-585`] + namespace + extern "C" + `struct Foo *p;` use + `class Widget;` fwd decl + out-of-line `Foo::bar`), `tests/languages.rs` (bar/reset/pure/m Method; count/shared/a Field; negative: no Struct for `Widget`/elaborated `Foo` use — via a new `assert_no_def(name)` helper) | cpp_characterization new asserts fail at base; helper green (phantom fwd-decls would turn it red) | phantom struct/class/enum emissions at non-definition sites |
| S9 | `.h` routing (D2): the lane's licensed Rust touch | `crates/wicked-estate-extract/src/treesitter.rs` (:569 remove `"h"`, :575 add `"h"`), `languages.toml` (c ext line 55, cpp ext line 90), NEW `tests/fixtures/sample.h` (= review fixture.h content + one `#include`), `tests/fixtures/cpp/sample_header.h` (with `#include` + a call — the integration test hard-fails `imports`-capped extensions with neither an Import node nor import-looking text), routing assertion in `tests/language_integration.rs` (or languages.rs): `extractor_for_extension("h").unwrap().languages() == ["cpp"]` — kept OUT of treesitter.rs's in-file test module to avoid cross-lane conflict. After this commit ALSO run `cargo test -p wicked-estate` (the consuming crate's `tests/all_languages.rs` exercises exactly this dispatch seam) [ATK EG-ATK-3] | New h_characterization: Foo Class, inlineDef Method, definedHere Function, Bar Struct from sample.h — fails at base (Foo absent under the C grammar); free prototypes in fixture.h counted under DEFERRED, not asserted (D6d); routing test fails at base | `"h"` from the c row (both files — single owner, no dual listing: `extractor_for_extension`, `by_extension`, and `ext_caps` are all first-match and must agree) |
| S10 | Regenerate coverage matrix; DESIGN-NOTES note for D1/D11 approximations | `docs/language-coverage-matrix.md` (via `python3 scripts/gen-coverage-matrix.py`), `docs/DESIGN-NOTES.md` (2 lines) | `python3 scripts/gen-coverage-matrix.py --check` exits 0 and prints ok; diff touches exactly the c/cpp rows (Swift row unchanged — cap now true) | stale matrix rows claiming c owns `.h` |
| S11 | Measurements + evidence + gates | none (scratch only) | See §5 protocol; all commands + outputs recorded in commit messages / lane report | — |

Commit convention: `fix(extract): …` / `test(extract): …` / `docs: …`, `--no-verify`, both
trailer lines. Per-crate builds only (`-p wicked-estate-extract`); `cargo fmt -p` not `--all`.
After any public-type-adjacent change: none expected (no core types touched).

## 4. Compatibility + migration (stored graphs, consumers)

- **`.h` identity break**: every symbol in every `.h` file re-schemes `ts-c …` → `ts-cpp …` and
  `language` flips c→cpp. **[ATK A5 — corrected dangling set]** `remove_file` DELETES `nodes_fts`
  and `embeddings` rows per symbol (`wicked-estate-store/src/sqlite.rs:1726-1730` inside
  :1693-1756), so those do NOT dangle — the real consequence there is lost embedding investment
  (re-embed cost on re-index). The rows that DO dangle — no pruning mechanism exists for any of
  them — are: `annotations.node_sym` (remove_file has no DELETE FROM annotations), overlay xedges,
  and memory `about` links keyed to dead `ts-c` ids. Named explicitly for the governance owner.
  This is the cost of fixing "class Foo gone"; there is no rename mechanism in scope.
- **Re-extraction trigger**: digest skip has no query/grammar salt; existing DBs pick up NOTHING
  for unchanged files until a `CARGO_PKG_VERSION` bump or `index --force`. The release carrying
  this lane must bump the version (repo sync script) — program-owned, recorded here (D12).
  **[ATK A5]** Even WITH the bump, a multi-repo labelled graph runs mixed `ts-c`/`ts-cpp` until
  each repo is individually re-indexed: version detection is per-repo
  (`repo_scope::meta_key(repo, "indexed_version")`, `wicked-estate/src/lib.rs:556-561`), so the
  mixed-scheme window is bounded by each repo's next index after the bump — not zero.
- **Governance/crew**: new Method/Constructor-kind nodes (Ruby setters/operators/attr_*, Swift
  init/deinit, C++ member prototypes, Java @interface elements) and every `.h` Class/Method are
  behavior-bearing in wicked-governance — repos at domain coverage 1.0 drop below 1.0 on re-index
  and the COVERAGE_SCRIPT gate denies until re-annotation. Field/TypeAlias additions are
  structural (safe). Behaviour change for governed runs — flagged in merge notes.
- **Resolution drift**: NameResolver is kind-blind and ScopedNameResolver skips ties. With D6(d)
  deferred, the FREE-function header/impl SymbolId collapse (the data-loss path) is NOT introduced
  by this lane; the residual collision is member prototype (.h, D6b) + out-of-line def (.cpp, D6e)
  sharing one SymbolId when stems match — both nodes are NEW and represent the semantically-same
  member, so the collapse is additive, though the node's `file` still flaps with index order and
  `remove_file` of one file drops the survivor until the other re-indexes. Handed to the
  method-identity lane (its enclosing-type work covers METHOD ids) in merge notes with the repro
  shape; free-function identity needs a program-level owner before D6(d) lands (merge note 2).
- **[EG-R1-1] Cross-KIND collision variant (blocking visibility)**: the bullet above is the
  same-kind flap; there is ALSO a Function-vs-Method kind conflict on one SymbolId — a free
  function and a same-named member method (proto/out-of-line def) now collide, and the store
  upsert re-kinds whichever lands last. Newly reachable via D6b/D6e (at base only the free
  Function was emitted). Repro, prevalence, owner requirement, and the executable pin are in §9;
  handoff in merge note 1(d). Sibling non-method Term-suffix collisions (Go const-vs-field, C++
  #define-vs-field) in §9 EG-COR-2 / merge note 1(e).
- **c/cpp self-typedef**: the `typedef struct X X;` idiom stops minting a TypeAlias node (kept for
  differently-named AND anonymous typedefs, D8). Stored graphs lose one phantom row per idiom
  occurrence — the row was already unreliable (kind flapped Struct/TypeAlias by match order).
- **Python/Rust**: stored kinds unchanged (Function was already the last write); in-memory dup
  node + dup Contains edge disappear.
- **[ATK A3] Benchmark/receipt drift (pre-declared for the integrator)**: the post-merge gate's
  bench receipts diff will move on the pinned tree-sitter/tree-sitter corpus
  (`wicked-estate-bench/src/lib.rs:113-118` — header-heavy C/C++): Files unchanged; Nodes UP
  (new Field/Method members, `.h` Classes/Methods appearing) and DOWN (phantom fwd-decl Structs,
  self-typedef phantoms, python/rust dup nodes removed); Edges similarly mixed (new Contains,
  removed dup Contains); Unresolved may shift as new named nodes join resolution. Direction+cause
  per column recorded in the lane report from the S11 tree-sitter-corpus run.
  `docs/benchmarks/capability-report.md` and `multi-repo-validation.md` embed per-language node
  counts that go stale after merge — regeneration is program-owned; listed in merge note 3.
- **[ATK A6] R4/R7**: retrieval budgets absorb the node growth (ContextBundle char budget +
  `truncated` flag, `wicked-estate-retrieve/src/context_bundle.rs:217-268`; `truncated:true` rate
  may rise for header-heavy classes); rank is edge-kind-gated (Calls|Imports —
  `wicked-estate-rank/src/lib.rs:156`), so Field/Contains growth doesn't move PageRank; new edges
  ride `Edge::new(tier)` so confidence/provenance are set generically; the Swift Extends
  superclass/protocol/raw-value conflation is documented in DESIGN-NOTES (S10).

## 5. Measurement protocol + gates (BEFORE recorded; AFTER required per step)

- BEFORE (done, at d7d3b58): NDJSON = `review-artifacts/doc04-fixtures/out.ndjson`
  (go 4 / rb 4 / java 4 [run ×2 one id] / cs 4 / swift 3 / hpp 4 / h 3); store kinds =
  `measure/before.db` (36 nodes / 30 edges / 8 files; kinds are snake_case JSON strings —
  `'"type_alias"'`, query with those spellings). Test baseline: 446 passed / 0 failed / 1 ignored.
- Declared-list fixups (pinned up front): `Foo() = default` counts as Function (D6);
  `~Foo` excluded/not_done; **free prototypes in fixture.h/fixture.hpp/extra.cpp move to a
  DEFERRED list (D6d) — not asserted, not counted as missing**.
- AFTER: (1) rebuild the local dump_ndjson example in the worktree (uncommitted), re-run all 7
  fixtures + extra.cpp + guarded_header — expect every declared (non-deferred) symbol emitted with
  the decided NodeKind and zero same-file SymbolId-with-conflicting-kinds;
  (2) **[ATK A1 — replaces the structurally-vacuous GROUP BY]** index the doc04 fixture dir with
  the AFTER binary (`<lane target>/debug/wicked-estate index measure/fixtures --db
  measure/after.db`) and run an extraction-vs-store RECONCILIATION (nodes.symbol is INTEGER
  PRIMARY KEY, so any `GROUP BY symbol HAVING COUNT(DISTINCT kind)>1` returns 0 rows on EVERY
  database and proves nothing): for each fixture file, (a) every (SymbolId, NodeKind) pair in the
  AFTER NDJSON appears in after.db with that EXACT kind — a kind mismatch is the silent-re-kind
  signal; (b) per-file distinct-SymbolId count in the NDJSON == store row count for that file —
  a shortfall reveals a same-kind collapse (FEAS-1's class). Scripted via sqlite3 + the NDJSON,
  commands recorded verbatim;
  (3) `cargo test -p wicked-estate-extract` count after (expect > 446 passed, 0 failed, 1 ignored
  — the pre-existing doctest); (4) matrix `--check` ok + row diff;
  (5) **[ATK A2/A3]** run the AFTER binary over the pinned tree-sitter/tree-sitter corpus (bench
  RepoSpec) — count SymbolIds emitted from >1 file (free-function header/impl collision
  prevalence, evidence for the D6(d) program decision) and record node-by-language before/after
  counts (the bench-receipt drift receipt);
  (6) **[ATK EG-ATK-6]** re-derive the header-corpus count with a recorded enumeration command and
  pin ONE count + error-rate pair for D2.
- **Gates [ATK EG-ATK-3]** (run at S11, plus per-step where named):
  `cargo clippy -p wicked-estate-extract --all-targets -- -D warnings` clean;
  `cargo build -p wicked-estate-extract` with 0 warnings;
  `cargo test -p wicked-estate-extract` green;
  `cargo test -p wicked-estate` green after S9 (per-crate, lane-legal — the crate depends on
  extract, `crates/wicked-estate/Cargo.toml:21`, and `tests/all_languages.rs` exercises the
  extension→extractor dispatch this lane edits);
  §9 agent-eval benchmark gate: vacuously green — `wicked-estate-bench/Cargo.toml` has no
  dependency on wicked-estate-extract (grep: 0 hits in this worktree);
  `every_wired_query_compiles` after every `.scm` edit (D4);
  the wicked-studio/wicked-crew corpora contain ZERO files in any in-lane language outside
  node_modules — recorded as a measurement note; the fixture dir + synthetic headers + the
  tree-sitter corpus are the measurement corpus.

## 6. Falsifier

The plan has failed if, at the final commit: the §5(2) extraction-vs-store reconciliation fails
for any doc04 fixture file — a (SymbolId, NodeKind) pair from the AFTER NDJSON absent or
wrong-kind in `after.db`, or a per-file distinct-SymbolId count ≠ that file's store row count —
OR any symbol from the seven fixtures' declared lists (as fixed in §5, deferred list excluded) is
missing or wrong-kind, OR extraction of the S4 fixture emits more or fewer than exactly one node
named `original`, OR `cargo test -p wicked-estate-extract` is not green (0 failed; only the
pre-existing ignored doctest), OR `cargo test -p wicked-estate` is not green after S9, OR
`cargo clippy -p wicked-estate-extract --all-targets -- -D warnings` is not clean, OR
`cargo build -p wicked-estate-extract` emits warnings, OR
`python3 scripts/gen-coverage-matrix.py --check` is not clean, OR `extractor_for_extension("h")`
does not resolve to cpp, OR the `assert_no_conflicting_def_ids` helper is not called from every
characterization test, OR any commit on the lane branch is not green (§9).

## 7. Not in scope (owned elsewhere / explicitly excluded)

- Definition-symbol construction (`treesitter.rs` ~1858-1905), method enclosing-type identity,
  same-kind SymbolId collisions (Java `run()` ×2; ts/cs/php/dart same-named methods; scala
  class+object; objc @interface+@implementation; bash/kotlin re-assigned variables) —
  method-identity lane.
- `typescript.scm` / `javascript.scm` / `tsx.scm` — relative-imports lane. If D7's helper is red
  at base for one of these, the collision is recorded under not_done and handed over — never
  weakened or excluded (D7 contingency).
- **C++ free-prototype capture (D6d) — DEFERRED**: ready pattern set recorded in D6; lands only
  after the program records an owner + decision for free-function header/impl node identity
  (merge note 2). Reported under not_done with the S11 prevalence measurement.
- `@call.object` in ruby.scm — KEPT deliberately (D13): dead today (no treesitter.rs handler),
  declared input for review doc 03's planned self-receiver resolver; owner = doc-03/resolve work.
- `crates/wicked-estate-resolve` (incl. the header-prototype/definition resolution tie) and
  `crates/wicked-estate/src/lib.rs` (incl. any content sniff).
- `~Foo() = delete;` destructor capture; Swift protocol-requirement properties; Swift methods
  inside type bodies stay Function (current swift.scm:42-45 behaviour — kind upgrade belongs with
  enclosing-type identity); Ruby `attr_reader "string"` form.
- CARGO_PKG_VERSION bump / release; CI wiring of `gen-coverage-matrix.py --check`;
  garden `emit_domain_model.py` type_alias bucket; stale swift comments at treesitter.rs:56/:522
  (outside the routing region — listed for the integrator); regeneration of
  `docs/benchmarks/capability-report.md` / `multi-repo-validation.md` (stale after merge — program).

## 8. Merge notes for other lanes / the program

1. **method-identity lane**: (a) this lane's helper is scoped to kind-conflicts precisely so your
   same-kind collisions stay yours — the distinct-anchor same-kind list is in §7; (b) MEMBER
   header-prototype/out-of-line-definition SymbolId collision (`foo.h` proto + `foo.cpp`
   `Foo::bar` def → one id; both nodes NEW in this lane, collapse additive, but `file` flaps and
   `remove_file` of one file drops the survivor) sits at the symbol-construction seam you own;
   (c) when you add enclosing types, Python/Rust methods should regain NodeKind::Method — this
   lane deliberately left them Function to preserve stored kinds; (d) **[EG-R1-1] your identity
   fix must cover the Function/Method suffix collision, not only method-vs-method** — a free
   function and a same-named member method share one SymbolId with CONFLICTING kinds (repro +
   executable pin in §9); (e) **[EG-COR-2] and the non-method `Suffix::Term` collisions** — Go
   `const X` vs struct field `X` (Constant vs Field, one id), C++ `#define` vs member field —
   repro + executable pin in §9. Both pins
   (`{go_const_vs_struct_field,cpp_free_function_vs_member_method}_..._known_defect`,
   tests/languages.rs) assert the defective shape and carry flip instructions for your landing.
2. **program (BLOCKING for D6d)**: free-function header/impl node identity has NO owner in
   RESOLUTION-PROGRAM.md (method-identity = method ids; resolve = edges). D6(d) free prototypes
   stay deferred until the program records an owner + decision — the S11 tree-sitter-corpus
   measurement (SymbolIds emitted from >1 file) is the evidence base. Data-loss repro: `foo.h`
   proto + `foo.cpp` def share a SymbolId; delete `foo.h` → `remove_file` drops the node; digest
   skip never re-extracts `foo.cpp` (until version bump / --force).
3. **program/release**: version bump required (D12) — and even with it, per-repo
   `indexed_version` means a mixed ts-c/ts-cpp window until each repo re-indexes (§4);
   crew/governance owners: re-indexing Ruby/Swift/C++/Java repos adds behavior-bearing nodes →
   domain coverage < 1.0 → COVERAGE_SCRIPT denies until re-annotation; `annotations.node_sym`,
   overlay xedges, and memory about-links keyed to dead ts-c ids have NO pruning mechanism (§4);
   bench receipts drift pre-declared in §4 (A3); capability-report/multi-repo-validation docs
   stale after merge.
4. **resolve lane**: new callable nodes (member prototypes, attr_* methods, init) change
   NameResolver uniqueness — previously-resolved Calls edges may drop to unresolved on tie
   (`resolve/src/lib.rs:46-66, 146-196` per recon). Repro shape: a.hpp member proto + a.cpp
   out-of-line def + b.cpp call. Needs a measured decision (prefer definition-with-body /
   non-header candidate).
5. **This lane's Rust touches** (for conflict awareness): `treesitter.rs:569/575` (routing table);
   NEW helper fn beside `strip_literal_quotes` (~:1382) + the DefName enum/classifier/arm changes
   for the `.name.symbol` opt-in suffix (SUPERSEDES the original D3 always-strip call-site — see
   §9 EG-COR-1; the other five strip channels untouched) + unit tests. Nothing in 1858-1905.
6. **doc-03/resolve work**: `ruby.scm:69-72` `@call.object` kept as your declared input (D13) —
   wire a handler or delete it when doc 03's self-receiver resolver lands.
7. `.h` moves to a SINGLE owner (cpp) in both LANG_TABLE and languages.toml — no dual listing;
   anyone adding an extension must edit both files (no test enforces sync; follow-up candidate).

## 9. Round-1 fixer corrections (EG-COR-1, EG-COR-2, EG-R1-1)

### EG-COR-1 — the generic def-name colon strip was WRONG (fixed)

The D3 decision routed every `CaptureRole::DefName` capture through a leading-`:` strip. That
audited the five ref/import/route/event channels but not DEF-name shapes across the 73 wired
languages: CSS pseudo-class selectors (`css.scm:13` captures `(selectors)` as `@code_type.name` —
`:root`, `:hover`, `:focus-visible`) and YAML symbol keys (`yaml.scm:9`, legacy Rails
`:adapter:`) legitimately start with `:` in DEF position. The strip silently renamed them and
re-schemed their SymbolIds fleet-wide — the exact failure D3 claimed to avoid.

**Fix (supersedes D3's mechanism and merge note 5's description):** the plain `.name` channel is
quote-strip-only again (`strip_def_name`, no colon handling). Colon-stripping is opt-in per query
via a new generic capture suffix `@code_<kind>.name.symbol` (`classify_capture` →
`DefName { symbol: true }`) — still zero per-language Rust; only `ruby.scm`'s two
`simple_symbol` def captures (alias_method, attr_*) use it. Regression pins:
`strip_def_name(":root") == ":root"` unit pin (treesitter.rs tests), new `css_characterization`
(`:root`/`:focus-visible` as TypeAlias from `fixtures/css/sample.css`) and a `:adapter` Struct pin
in `yaml_characterization` (`fixtures/sample.yaml`). Ruby behavior unchanged
(`other_name`/`balance` still emitted bare — `ruby_characterization` green).

### EG-COR-2 — Go const-vs-field cross-kind SymbolId collision (recorded, NOT fixed in-lane)

**not_done — owner: method-identity lane.** A package-level `const X` and a struct field `X`
emit the SAME SymbolId with CONFLICTING kinds; the store upsert re-kinds one. Both def suffixes
fall through to `Suffix::Term` (`def_suffix`, treesitter.rs — "constant" and "field" both hit the
catch-all) and the id carries no enclosing type. Concrete repro (lane build,
`dump_ndjson` on `package p; const X = 1; type S struct { X int }`):
`ts-go . . . probe_collide/X.` → sym_kind `constant` (line 2) AND `field` (line 3). The same
widened surface exists for C++ `#define` Constants vs the new member-field Fields (both
`Suffix::Term`). Newly reachable via this lane's D04-2 struct-field pattern (`go.scm` field
capture); the fleet guard can't see it because no fixture contains the construct — by design:
the executable handoff is the dedicated pin
`go_const_vs_struct_field_symbolid_collision_known_defect` (tests/languages.rs), which asserts
the CURRENT defective shape and fails loudly (with flip instructions) when the method-identity
lane's identity fix lands. Requirement on that lane: enclosing-type identity must cover
non-method `Suffix::Term` members (const/field/#define), not only methods.

### EG-R1-1 — C++ free-Function vs member-Method cross-kind collision (recorded, NOT fixed in-lane)

**Blocking-visibility for the method-identity lane.** `def_suffix` maps both "function" and
"method" to `Suffix::Method`, so a member method and a same-named free function in one module
share one SymbolId with CONFLICTING kinds. 3-line repro (lane build):
`class Foo { public: void reset(); }; void Foo::reset() {} void reset() {}` emits THREE nodes on
`ts-cpp . . . probe_collide/reset().` — kinds method (D6b proto), method (D6e out-of-line def),
function (free def). This pre-exists for in-class definitions vs free functions, but at base the
proto and out-of-line def were uncaptured, so a bare header prototype now suffices to trigger it —
the std::swap idiom (member `swap` proto + free `swap` in one header) is the textbook shape.
Prevalence on the 232-file tree-sitter corpus was 0 same-file kind conflicts (C-heavy corpus —
weak evidence, not absence). Requirement on the method-identity lane: the identity fix must cover
the Function/Method suffix collision, not only method-vs-method (merge note 1(d)). Executable pin:
`cpp_free_function_vs_member_method_symbolid_collision_known_defect` (tests/languages.rs), same
flip-on-fix contract as the Go pin.
