# Recon plan — extraction-gaps lane (review doc 04, D04-1..D04-10 + engine defect #7)

Base: `d7d3b58` on `lane/extraction-gaps`. Planner synthesis of four recon lenses
(history / consumers / tests / risks), all facts re-verified against files opened in this
worktree. Governing decision for every query change: **rules as data** —
`docs/DESIGN-NOTES.md:30-33`, `CLAUDE.md:178-182` ("languages are data; the capability matrix is
generated"). No ADR pins query content (all in-scope `.scm` files have exactly one commit,
`797ce58`), so no ADR amendment is needed.

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
| Defect #7 (.h routing + Swift matrix + dead file) | as D04-6/D04-9; `queries/c-sharp.scm` exists and nothing references it (`treesitter.rs:40` wires `csharp.scm`; repo-wide grep for `c-sharp.scm` in recon: 0 refs, history = initial commit only) | `treesitter.rs:567-577`; `queries/c-sharp.scm` present in worktree |
| §11 siblings of the Go defect class (recon finding, not in doc 04) | `python.scm:15-23` (class-body Method pattern) + `:25-29` (unanchored Function pattern) emit every Python method twice on the SAME anchor node with different kinds; `rust.scm:4-7` + `:25-32` identical shape. BEFORE store shows the re-kind is live: sample.py methods stored as `"function"` | `python.scm`, `rust.scm` opened; recon BEFORE DB `measure/before-db/py.db` |
| §11 sibling: C typedef-struct collision | `typedef struct Vector2 Vector2;` (sample.c) mints `sample/Vector2#` twice — TypeAlias + Struct, distinct anchors, kind conflict; `c_characterization` asserts BOTH (`languages.rs` ~404/412), passing only because the in-memory Extraction keeps both while the store keeps one | recon probe DBs; `tests/fixtures/sample.c` |

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
Evidence the superset route is acceptable: over 1,654 real SDK/homebrew headers the cpp grammar
parses C headers at +2.5% error nodes and 1.8× faster than c; measured costs are (a) C identifiers
that are C++ keywords are dropped (probe: `int template(...)` lost) and (b) forward declarations /
elaborated uses become phantom Struct nodes — cost (b) is eliminated by D6's `body:` constraints,
which therefore MUST land in the same commit as the routing change. Cost (a) is accepted and
documented in the matrix regen commit.

**D3 — Ruby symbol names: one generic strip of a leading `:` at the shared seam
(`strip_literal_quotes`, `treesitter.rs:1382-1392`).**
The brief's item 2 explicitly lists this option ("a single generic strip at the shared seam; no
Ruby-specific Rust"). A query predicate cannot rewrite text (tree-sitter has no transform
predicates), and accepting `:other_name` repeats the exact §11 scar the seam exists for
("unqueryable by real name" — CLAUDE.md §11; precedent commit `3f0bdde` added the DefName strip at
this same seam for VB6). Blast radius audited by recon: the only other `.scm` capturing an
atom/symbol leaf as a def name is `prolog.scm:4` `(atom)`, and Prolog atoms never begin with `:`;
the strip must not touch `::` (guard: strip a single leading `:` only when not followed by another
`:` and the remainder is non-empty). Unit test goes next to the existing strip tests
(`strip_literal_quotes_handles_call_and_import_forms`, treesitter.rs test module ~2765).
This is the lane's second Rust touch beyond the routing table — flagged in merge notes.

**D4 — Swift: `init` via anonymous-token name capture; `deinit` included but compile-gated.**
`(init_declaration name: "init" @code_method.name) @code_method.def` (D04-3-verified, 2 matches on
the fixture). `(deinit_declaration "deinit" @code_method.name) @code_method.def` was probe-verified
by the risks lens (1 match on Fixture.swift under 0.7.3) even though node-types lists no `name`
field for `deinit_declaration` — because a non-compiling swift.scm silently drops Swift
(`for_language` returns `None`), the executor runs
`cargo test -p wicked-estate-extract every_wired_query_compiles` immediately after editing
swift.scm and, if the deinit pattern fails to compile, deletes it and records deinit under
not_done. Never commit a swift.scm that doesn't compile.

**D5 — Swift properties scoped to type bodies, not bare.**
The review's bare `(property_declaration name: (pattern (simple_identifier)))` also matches every
function-local `let/var` and top-level globals (risks-lens probe: 16 matches on swift/sample.swift,
2 with parent `statements`). Emit `@code_property` only under `(class_body …)` and
`(enum_class_body …)`. Protocol requirements (`protocol_property_declaration`) are left out —
smallest change; the doc-04 declared list doesn't include them.

**D6 — C++: `body:` constraints + four new pattern groups.**
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
(d) Free prototypes anchored per parent — one pattern each under `translation_unit`,
`preproc_ifdef`, `preproc_if`, `declaration_list` (namespaces + `extern "C"`), and
`template_declaration`. The review's translation_unit-only anchor captures **0** declarations in
include-guarded headers (the dominant idiom; probe: 0 vs 3 on guarded.h) while still excluding
body-local prototypes and most-vexing-parse declarations (0 false positives on extra.cpp — MVP
`Foo f(Foo());` parses as init_declarator under 0.23.4, so only the body-local prototype was ever
a false-positive risk, handled by the anchoring).
(e) Out-of-line member definitions: `(function_definition declarator: (function_declarator
declarator: (qualified_identifier name: (identifier) @code_method.name))) @code_method.def` —
`void Foo::reset() {}` is captured by NOTHING today (`cpp.scm:24-35` matches only
identifier/field_identifier). Probe-verified 1/1. Included because without it the new header
prototype becomes the only node for such methods, and it is a pure cpp.scm data addition.
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

**D8 — Fix the §11 siblings that make D7's helper red at base: python.scm, rust.scm, c.scm (+cpp).**
- python.scm / rust.scm: delete the class-/impl-body Method pattern (`python.scm:15-23`,
  `rust.scm:24-32`), keeping the general Function pattern. This preserves **stored** behaviour
  exactly (BEFORE DB proves the store already keeps `"function"` for Python methods — last write
  wins), removes the duplicate node + duplicate Contains edge, and requires zero assertion changes
  (`python_characterization` already asserts `__init__`/`process` as Function). Kind-fidelity loss
  (methods emitted as Function) is pre-existing stored behaviour, recorded for the method-identity
  lane, which is the right place to restore Method kind together with enclosing-type identity.
  Executor must verify rust's stored kinds match before/after; if rust's last-write differs from
  python's, delete whichever pattern preserves stored kinds and record the choice.
- c.scm (and the equivalent typedef pattern in cpp.scm if present): constrain the typedef pattern
  with a predicate so `typedef struct X X;` (self-named) no longer mints a second `X#` node:
  `(type_definition type: (struct_specifier name: (type_identifier) @_s) declarator:
  (type_identifier) @code_type.name (#not-eq? @_s @code_type.name)) @code_type.def` alongside the
  unconstrained non-struct typedef pattern. `typedef struct foo foo_t;` (different names) keeps
  both nodes. Predicates INSIDE the outermost parens (scar `7df786b`: trailing predicates are
  silently ignored). This deletes the phantom duplicate; `c_characterization`'s
  `assert_def("Vector2", TypeAlias)` is updated to a differently-named typedef assert — a
  legitimate assertion change tracking a §11 fix, not a bar-lowering.

**D9 — Ruby patterns.** Modify the existing method/singleton_method patterns in place
(`name: (identifier)` → `name: [(identifier) (setter) (operator)]`, `ruby.scm:22-33`) — modifying
avoids a second pattern double-matching plain identifiers. Add `(alias name: (_method_name)
@code_method.name) @code_method.def`; add ONE call pattern for `alias_method` + `attr_reader` +
`attr_writer` + `attr_accessor` via `#any-of?` on the method identifier, capturing each
`simple_symbol` argument (attr_accessor :a, :b → one match per symbol), predicate inside the outer
parens. Names rely on D3's colon strip. `attr_reader "s"` (string arg) intentionally unmatched.

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

## 3. Step list (each step: files, tests proving it, deletions)

Setup (uncommitted): copy `dump_ndjson.rs` from the main checkout into
`crates/wicked-estate-extract/examples/` for local runs — **never `git add` it**; copy the doc04
fixtures into `<lane>/measure/`. Record baseline: `cargo test -p wicked-estate-extract` at
d7d3b58 = **446 passed / 0 failed / 1 ignored (pre-existing extra_edge doctest)** with
`CARGO_TARGET_DIR=<lane target>`; BEFORE NDJSON = `review-artifacts/doc04-fixtures/out.ndjson`;
BEFORE store = `measure/before.db`.

| # | Change | Files | Test that proves it (fails at base → passes after) | Deletes |
|---|---|---|---|---|
| S1 | Shared helper `assert_no_conflicting_def_ids(&extraction, lang)` (per-file: no SymbolId with >1 distinct NodeKind among non-File nodes; failure message prints the colliding (id, kinds, lines)); call from every existing characterization test | `tests/languages.rs` | Red at base for python + c characterization (proves the helper detects the class); green after S2 | — |
| S2 | §11 sibling fixes: delete python.scm:15-23 and rust.scm:24-32 duplicate Method patterns; add `#not-eq?` self-named-typedef constraint to c.scm (and cpp.scm typedef if present) | `queries/python.scm`, `queries/rust.scm`, `queries/c.scm`, (`queries/cpp.scm`), `tests/languages.rs` (c: replace `Vector2` TypeAlias assert with a differently-named typedef assert), `tests/fixtures/sample.c` (add `typedef struct Vector2 Vec2;` if needed to keep TypeAlias coverage) | S1 helper goes green on python/rust/c; python_characterization unchanged (Function asserts already match stored behaviour); verify stored kinds unchanged via BEFORE/AFTER DB diff on sample.py/sample.rs | python.scm class-body pattern; rust.scm impl-body pattern; the phantom self-typedef node |
| S3 | Go: struct-field pattern + constrained type_spec alternation (D1); approximation comment | `queries/go.scm`, `tests/fixtures/sample.go` (add ID/Name/Tags-style fields, `type UserID string`, `type Handler func(int) error`, `type Matrix [][]float64`, an `a, b int` multi-name field), mirror into `tests/fixtures/go/sample.go`, `tests/languages.rs` (assert_def Field ×3+, TypeAlias ×3; helper proves no Struct/Interface re-kind) | go_characterization new asserts fail at base; helper stays green (the D04-2 catch-all would turn it red) | — |
| S4 | Ruby: setter/operator in-place edit, alias, alias_method/attr_* patterns (D9); leading-`:` strip at shared seam (D3) | `queries/ruby.scm`, `crates/wicked-estate-extract/src/treesitter.rs` (strip_literal_quotes only, ~1382-1392, + unit test beside ~2765), `tests/fixtures/sample.rb` + `tests/fixtures/ruby/sample.rb`, `tests/languages.rs` (assert_def `name=`, `[]`, `<=>`, `==`, `new_name`, `other_name`, `balance` as Method — bare names, no colon) | ruby_characterization new asserts fail at base; strip unit test (`":x"`→`"x"`, `"::X"` unchanged, `":"` unchanged) fails at base | — |
| S5 | Java `@interface` + elements (D10) | `queries/java.scm`, `tests/fixtures/sample.java` + `java/sample.java`, `tests/languages.rs` (Marker Interface; value/priority Method) | java_characterization new asserts fail at base | — |
| S6 | C# properties (D10); retire dead query file | `queries/csharp.scm`, **delete `queries/c-sharp.scm`**, `tests/fixtures/sample.cs` + `csharp/sample.cs`, `tests/languages.rs` (Id/Name/Total Field) | csharp_characterization new asserts fail at base; `cargo build -p wicked-estate-extract` proves nothing referenced the deleted file | `queries/c-sharp.scm` |
| S7 | Swift: type-body-scoped properties, init, deinit (compile-gated), heritage (D4/D5/D11); fix swift.scm:3 comment; NEW flat fixture + characterization test | `queries/swift.scm`, NEW `tests/fixtures/sample.swift`, `tests/fixtures/swift/sample.swift` (add init/deinit), `tests/languages.rs` (NEW swift_characterization: Point Struct, Box Class, x/y/sum/origin/item Field, init + deinit Method, Extends ref via assert helper; negative: no Field for a function-local `let`) | swift_characterization is entirely new (Swift had none); `every_wired_query_compiles` run FIRST after each swift.scm edit | stale 0.7.1 comment |
| S8 | C++: body: constraints, member prototypes, member fields (alternation), per-parent free prototypes, qualified out-of-line defs (D6) | `queries/cpp.scm`, `tests/fixtures/sample.cpp` + `cpp/sample.cpp`, NEW `tests/fixtures/cpp/guarded_header.hpp` (include guards + namespace + extern "C" + template proto + `struct Foo *p;` use + `class Widget;` fwd decl + out-of-line `Foo::bar`), `tests/languages.rs` (bar/reset/pure/m Method; count/shared/a Field; freestanding Function; negative: no def named `localProto`, no Struct for `Widget`/elaborated `Foo` use — via a new `assert_no_def(name)` helper) | cpp_characterization new asserts fail at base; helper green (phantom fwd-decls would turn it red) | phantom struct/class/enum emissions at non-definition sites |
| S9 | `.h` routing (D2): the lane's licensed Rust touch | `crates/wicked-estate-extract/src/treesitter.rs` (:569 remove `"h"`, :575 add `"h"`), `languages.toml` (c ext line 55, cpp ext line 90), NEW `tests/fixtures/sample.h` (= review fixture.h content + one `#include`), `tests/fixtures/cpp/sample_header.h` (with `#include` + a call — the integration test hard-fails `imports`-capped extensions with neither an Import node nor import-looking text), routing assertion in `tests/language_integration.rs` (or languages.rs): `extractor_for_extension("h").unwrap().languages() == ["cpp"]` — kept OUT of treesitter.rs's in-file test module to avoid cross-lane conflict | New h_characterization: Foo Class, inlineDef Method, definedHere Function, Bar Struct from sample.h — fails at base (Foo absent under the C grammar); routing test fails at base | `"h"` from the c row (both files — single owner, no dual listing: `extractor_for_extension`, `by_extension`, and `ext_caps` are all first-match and must agree) |
| S10 | Regenerate coverage matrix; DESIGN-NOTES note for D1/D11 approximations | `docs/language-coverage-matrix.md` (via `python3 scripts/gen-coverage-matrix.py`), `docs/DESIGN-NOTES.md` (2 lines) | `python3 scripts/gen-coverage-matrix.py --check` exits 0 and prints ok; diff touches exactly the c/cpp rows (Swift row unchanged — cap now true) | stale matrix rows claiming c owns `.h` |
| S11 | Measurements + evidence | none (scratch only) | See §5 protocol; all commands + outputs recorded in commit messages / lane report | — |

Commit convention: `fix(extract): …` / `test(extract): …` / `docs: …`, `--no-verify`, both
trailer lines. Per-crate builds only (`-p wicked-estate-extract`); `cargo fmt -p` not `--all`.
After any public-type-adjacent change: none expected (no core types touched).

## 4. Compatibility + migration (stored graphs, consumers)

- **`.h` identity break**: every symbol in every `.h` file re-schemes `ts-c …` → `ts-cpp …` and
  `language` flips c→cpp. Symbol-keyed sidecars (annotations.node_sym, embeddings.symbol,
  nodes_fts, overlay xedges, memory `about` links) keyed by old ids dangle after re-index. This is
  the cost of fixing "class Foo gone"; there is no rename mechanism in scope.
- **Re-extraction trigger**: digest skip has no query/grammar salt; existing DBs pick up NOTHING
  for unchanged files until a `CARGO_PKG_VERSION` bump or `index --force`. The release carrying
  this lane must bump the version (repo sync script) — program-owned, recorded here (D12).
  Without it, mixed `ts-c`/`ts-cpp` graphs + pruned edges to re-extracted headers.
- **Governance/crew**: new Method/Constructor-kind nodes (Ruby setters/operators/attr_*, Swift
  init/deinit, C++ prototypes, Java @interface elements) and every `.h` Class/Method are
  behavior-bearing in wicked-governance — repos at domain coverage 1.0 drop below 1.0 on re-index
  and the COVERAGE_SCRIPT gate denies until re-annotation. Field/TypeAlias additions are
  structural (safe). Behaviour change for governed runs — flagged in merge notes.
- **Resolution drift**: NameResolver is kind-blind and ScopedNameResolver skips ties — a header
  prototype + sibling-`.cpp` definition of the same free function share one SymbolId (module =
  path minus extension), so `foo.h`+`foo.cpp` pairs collapse to one node whose `file` flaps with
  index order, and `remove_file(foo.h)` deletes the `.cpp`'s node. Cross-file collision is a
  symbol-construction issue (method-identity lane's seam) — handed over in merge notes with the
  repro shape; not fixable in query data here.
- **c self-typedef**: the `typedef struct X X;` idiom stops minting a TypeAlias node (kept for
  differently-named typedefs). Stored graphs lose one phantom row per idiom occurrence — the row
  was already unreliable (kind flapped Struct/TypeAlias by match order).
- **Python/Rust**: stored kinds unchanged (Function was already the last write); in-memory dup
  node + dup Contains edge disappear.

## 5. Measurement protocol (BEFORE recorded; AFTER required per step)

- BEFORE (done, at d7d3b58): NDJSON = `review-artifacts/doc04-fixtures/out.ndjson`
  (go 4 / rb 4 / java 4 [run ×2 one id] / cs 4 / swift 3 / hpp 4 / h 3); store kinds =
  `measure/before.db` (36 nodes / 30 edges / 8 files; kinds are snake_case JSON strings —
  `'"type_alias"'`, query with those spellings). Test baseline: 446 passed / 0 failed / 1 ignored.
- AFTER: (1) rebuild the local dump_ndjson example in the worktree (uncommitted), re-run all 7
  fixtures + extra.cpp + guarded_header — expect every declared symbol emitted with the decided
  NodeKind (declared counts fixed up front: `Foo() = default` counts as Function per D6, `~Foo`
  excluded/not_done) and zero same-file SymbolId-with-conflicting-kinds; (2) index the doc04
  fixture dir with the AFTER binary
  (`<lane target>/debug/wicked-estate index measure/fixtures --db measure/after.db`) and prove the
  ON CONFLICT path clean:
  `sqlite3 measure/after.db "SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol GROUP BY n.symbol HAVING COUNT(DISTINCT n.kind)>1;"`
  → 0 rows; kind spot-checks per fixture; (3) `cargo test -p wicked-estate-extract` count
  after (expect > 446 passed, 0 failed, 1 ignored — the pre-existing doctest); (4) matrix
  `--check` ok + row diff. NOTE: the brief's corpora (wicked-studio, wicked-crew) contain ZERO
  files in any in-lane language outside node_modules — recorded as a measurement note; the fixture
  dir + synthetic headers are the corpus.

## 6. Falsifier

The plan has failed if, at the final commit: indexing the doc04 fixtures with the AFTER binary
leaves `SELECT … GROUP BY symbol HAVING COUNT(DISTINCT kind)>1` non-empty, OR any symbol from the
seven fixtures' declared lists (as fixed in §5) is missing or wrong-kind in `after.db`, OR
`cargo test -p wicked-estate-extract` is not green (0 failed; only the pre-existing ignored
doctest), OR `python3 scripts/gen-coverage-matrix.py --check` is not clean, OR
`extractor_for_extension("h")` does not resolve to cpp, OR the `assert_no_conflicting_def_ids`
helper is not called from every characterization test.

## 7. Not in scope (owned elsewhere / explicitly excluded)

- Definition-symbol construction (`treesitter.rs` ~1858-1905), method enclosing-type identity,
  same-kind SymbolId collisions (Java `run()` ×2; ts/cs/php/dart same-named methods; scala
  class+object; objc @interface+@implementation; bash/kotlin re-assigned variables) —
  method-identity lane.
- `typescript.scm` / `javascript.scm` / `tsx.scm` — relative-imports lane.
- `crates/wicked-estate-resolve` (incl. the header-prototype/definition resolution tie) and
  `crates/wicked-estate/src/lib.rs` (incl. any content sniff).
- `~Foo() = delete;` destructor capture; Swift protocol-requirement properties; Swift methods
  inside type bodies stay Function (current swift.scm:42-45 behaviour — kind upgrade belongs with
  enclosing-type identity); Ruby `attr_reader "string"` form.
- CARGO_PKG_VERSION bump / release; CI wiring of `gen-coverage-matrix.py --check`;
  garden `emit_domain_model.py` type_alias bucket; stale swift comments at treesitter.rs:56/:522
  (outside the routing region — listed for the integrator).

## 8. Merge notes for other lanes / the program

1. **method-identity lane**: (a) this lane's helper is scoped to kind-conflicts precisely so your
   same-kind collisions stay yours — the distinct-anchor same-kind list is in §7; (b) cross-file
   header-prototype/definition SymbolId collision (`foo.h` proto + `foo.cpp` def → one id;
   `remove_file` deletes the survivor) is created/exposed by this lane's C++ prototype capture and
   can only be fixed at the symbol-construction seam you own; (c) when you add enclosing types,
   Python/Rust methods should regain NodeKind::Method — this lane deliberately left them Function
   to preserve stored kinds.
2. **resolve lane / program**: new callable nodes (prototypes, attr_* methods, init) change
   NameResolver uniqueness — previously-resolved Calls edges may drop to unresolved on tie
   (`resolve/src/lib.rs:46-66, 146-196` per recon). Repro shape: a.h proto + a.cpp def + b.cpp
   call. Needs a measured decision (prefer definition-with-body / non-header candidate).
3. **program/release**: version bump required (D12); crew/governance owners: re-indexing
   Ruby/Swift/C++/Java repos adds behavior-bearing nodes → domain coverage < 1.0 → COVERAGE_SCRIPT
   denies until re-annotation.
4. **This lane's Rust touches** (for conflict awareness): `treesitter.rs:569/575` (routing table)
   and `treesitter.rs:~1382-1392` + one unit test ~2765 (strip_literal_quotes, licensed by brief
   item 2). Nothing in 1858-1905.
5. `.h` moves to a SINGLE owner (cpp) in both LANG_TABLE and languages.toml — no dual listing;
   anyone adding an extension must edit both files (no test enforces sync; follow-up candidate).
