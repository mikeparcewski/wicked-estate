# Recon + plan: type-nested definition identity (engine defect #1)

Lane: `lane/method-identity` (base d7d3b58). Resolves review findings **D03-1, D03-2, PER-7**
(`estate-review/REVIEW-adversarial-2026-08-28.md`, "Doc 03 — receiver-type inference";
`review-artifacts/findings.json`). All file:line citations are against the lane worktree at
d7d3b58 unless another root is named. Every line cited below was opened in this session.

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

### D1 — Nest under the chain of enclosing **Type-suffixed** definitions only. Not under functions, not under Term-suffixed bindings.
New shape: `[module/] ++ [T1#, T2#, …] ++ [name<suffix>]`, where `T1..Tn` are the definitions
whose `def_suffix(kind) == Suffix::Type` (`class|struct|enum|trait|interface|module|namespace|type_alias|type`,
`treesitter.rs:1341-1348`) whose byte range *strictly contains* the definition's anchor range,
outermost first. The chain applies to **every** definition kind (methods, constructors, fields,
enum members, interface method signatures, nested types): `src/repo/Repo#save().`,
`src/repo/Repo#handler().`, `src/d/Outer#Inner#run().`, `src/a/Store#save().`, `src/a/Color#Red.`.

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

Accepted residual (documented in the ADR amendment and pinned by the fixture test): function-local
definitions (nested functions, arrows bound inside a method, object-literal methods) keep
`<module>/name().` and still collide with a same-named module-level definition. This is a smaller,
visible residual — today it is hidden inside the bigger merge.

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
signature }` — no id. Pass 2 (immediately after the loop, before nodes/edges assembly at
`treesitter.rs:2040`) builds the Type-anchor list from the pending defs, sorts by
`(start asc, end desc)`, **dedupes by `(start, end, name)`** (a decorated TS class matches two
`@code_class.def` patterns, `typescript.scm:43-45` and `:135-141`, producing two identical
records; a Rust `function_item` matches both `:3-7` and `:24-30`), then mints every id with
`def_symbol(scheme, module, chain, name, suffix)` and pushes `Node` + `DefRec`. Cost is
O(defs × type-anchors) per file; worst observed defs/file is 208 (studio) — negligible against a
1.0 s full crew index.

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
`index_path_as` reads `repo_scope::meta_key(repo, "id_scheme")`; if a prior index exists
(`prev_version.is_some()`) and the stored scheme != "2" (absent counts as "1"), set `force_full`
and print `SYMBOL-ID SCHEME changed (v1 → v2): forcing full re-extraction of '<label|root>'`.
Always write the key. `force_full` routes every file through `remove_file` (`lib.rs:719-724`),
which deletes nodes by file regardless of id → no stale old-id nodes survive; the existing
deleted-file sweep covers files gone from disk. Multi-repo graphs re-extract each label the next
time it is indexed (same semantics as `indexed_version`); `maybe_warn_version_mismatch`
(`main.rs:107-135`) gains the same per-repo check so an operator is told which labels still hold
scheme-1 ids.

## 3. Step list

Order is fixed: S0 fixture/test first (red), S1 seam (green), S2 versioning (green), S3 docs,
S4 measurements. Per-crate cargo only; `CARGO_TARGET_DIR` = the lane target dir.

### S0 — Characterisation fixture + failing tests (extract crate)
Files: `crates/wicked-estate-extract/src/treesitter.rs` (unit tests module),
`crates/wicked-estate-extract/tests/fixtures/typescript/method_identity.ts` (new).
Fixture (the doc03 shape + the residual):
```ts
export class Repo { save(): void {} update(): void { this.save(); other.save(); } }
export class Cache { save(): void {} flush(): void { this.save(); const cb = () => this.save(); } }
export interface Store { save(): void; }
export const lit = { save() {}, run() { this.save(); } };
export function top() { const r = new Repo(); r.update(); }
```
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
3. `identity_does_not_nest_under_functions_or_terms` — `lit.save` is `…/save().` (flat), `cb` is
   `…/cb().` (flat, NOT `Cache#flush().cb().`), `top` is `…/top().`; documents the residual.
4. `identity_disambiguator_is_none` — every Method-suffixed def has `disambiguator == None`
   (pin D6 via the rendered string `().`).
5. `identity_contains_edges_stay_file_to_def` — every Contains edge has `source == file symbol`
   and the count equals the number of def nodes (D4 pinned; today only "any Contains" is asserted
   at `treesitter.rs:2794`).
6. `identity_dedupes_duplicate_anchors` — a `@Entity`-decorated class (two `@code_class.def`
   matches) yields `Ent#save().`, not `Ent#Ent#save().`.
Proof step: `cargo test -p wicked-estate-extract identity_` → tests 1, 2, 6 FAIL at d7d3b58 (3, 4, 5
pass — they pin today's behaviour that must survive). Record the exact output.
Deletes: nothing.

### S1 — The seam: two-pass mint + one `def_symbol` helper + sibling sites
Files: `crates/wicked-estate-extract/src/treesitter.rs` only (hunks confined to `:1341-1376`
helpers, `:1740-1760` collector declarations, `:1866-1905` def block, new pass-2 block before
`:2040`, and `:2146-2260` framework emitters). `crates/wicked-estate-extract/src/lib.rs:13`
re-export of the constant (one line).
Change:
- `pub const SYMBOL_ID_SCHEME: &str = "2";` with a doc comment pointing at the ADR-002 amendment.
- `DefRec` gains `name: String, suffix: Suffix` (needed for self-exclusion and lookup).
- `struct PendingDef { kind: &'static str, name: String, start: usize, end: usize, span: Span, signature: Option<String> }`.
- `fn type_anchors(pending: &[PendingDef]) -> Vec<TypeAnchor>` — filter `def_suffix(kind) == Type`,
  sort `(start asc, end desc)`, dedupe `(start, end, name)`.
- `fn enclosing_types(anchors: &[TypeAnchor], start, end) -> Vec<Descriptor>` — anchors with
  `a.start <= start && end <= a.end && !(a.start == start && a.end == end)`, in sorted order
  (outer→inner), as `Descriptor::new(name, Suffix::Type)`.
- `fn def_symbol(scheme, module, chain: &[Descriptor], name, suffix) -> SymbolId` — the ONLY place
  a code-definition id is built: `[Namespace(module)] ++ chain ++ [Descriptor{name, suffix, disambiguator: None}]`.
- The def block at `:1866-1905` becomes "push `PendingDef`" (no `Symbol::global`, no `Node`).
- Pass 2 after the loop: mint, build `Node` (same fields as today: kind via `def_nodekind`,
  signature, location), push `DefRec`.
- Sibling sites: `di_pairs`, `route_triples`, `event_listen_type_triples`,
  `event_listen_topic_triples` carry the **byte position of the captured name node**
  (`c.node.start_byte()` is already computed as `pos` at `:1801`). Emission uses
  `fn def_symbol_at(defs: &[DefRec], pos, name) -> Option<SymbolId>` = smallest `DefRec`
  containing `pos` whose `name == name` (the method/class def's range contains its own name
  identifier); fallback when the language's def query did not capture that def:
  `def_symbol(scheme, module, enclosing_types(anchors, pos, pos) minus any anchor with the same
  name, name, suffix)` — same dangling behaviour class as today, no silent drop.
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
Change (lib.rs): after the `indexed_version` block, read
`repo_scope::meta_key(repo, "id_scheme")`; `if prev_version.is_some() && prev_scheme.as_deref() != Some(SYMBOL_ID_SCHEME) { force_full = true; eprintln!("SYMBOL-ID SCHEME changed (v{} → v{}): forcing full re-extraction", prev_scheme.unwrap_or("1"), SYMBOL_ID_SCHEME); }`;
`store.meta_set_key(&scheme_key, SYMBOL_ID_SCHEME)`.
Change (main.rs): the per-repo loop also compares `id_scheme`; message names the label and says
"symbol ids under types changed; annotations/xedges keyed on old ids are NOT carried over — run
`wicked-estate index <root> --repo <label>`". Delete the "(your annotations are preserved)" clause.
Tests (`tests/id_scheme.rs`, in-process, `SqliteStore::open(tempdir)`):
1. `same_version_old_scheme_db_is_fully_reextracted_without_force` — index the S0 fixture dir;
   then simulate the BEFORE state through the store API: `meta_set_key("id_scheme", "1")`,
   `upsert_nodes([Node with symbol ts-typescript . . . src/method_identity/save(). ])` (the flat
   id), leave digests as written; call `index_path` again with NO file change → assert the flat
   node is gone, `Repo#save().` and `Cache#save().` exist, `meta_get_key("id_scheme") == "2"`.
   (`file_digest` is private at `lib.rs:424`; the store API set-up avoids recomputing it.)
2. `id_scheme_is_recorded_per_repo` — mirrors `tests/multi_repo.rs:500-535` for the new key
   (labelled runs write `repo:<label>:id_scheme`, never the bare key).
3. `fresh_db_writes_scheme_without_forcing` — first index prints no scheme message (capture via
   the returned stats: `unchanged == 0` is trivially true; assert the key only).
4. `store_keeps_same_named_methods_of_different_types_apart` (store-level collision test, the one
   that catches `sqlite.rs:387-394`): after indexing the fixture, `search(store, "save")`
   returns 4 nodes (Repo, Cache, Store, lit) and `stats().node_count` equals the def count + 1
   file node; no Calls edge from `Cache#flush().` targets `Repo#save().`.
Tests: `cargo test -p wicked-estate` (47 lib + 20 main + 60 integration at d7d3b58, plus the 4 new);
`cargo test -p wicked-estate-resolve` must stay 61 unit + 1 lsp_live + 4 scip_edges
(+1 pre-existing ignored doctest, engine defect #5, out of scope — report unchanged, do not fix).
Deletes: the "(your annotations are preserved)" text at `main.rs:121-127`.

### S3 — Documentation
Files: `docs/adr/ADR-002-stable-symbol-identity.md` (new section "Amendment 2026-08 — type-nested
member identity", status stays Accepted), `CHANGELOG.md` `[Unreleased]` → `### Changed`,
`docs/recon/java-spring-framework-edges.md:37-41` (append a dated note: scheme is now
type-nested; the edges are minted through `def_symbol`/`def_symbol_at`).
ADR amendment content: the new descriptor rule (D1) with the worked strings; a 4th stability row
"rename/move the enclosing type → new identity for its members (correct: different logical
path)"; a 5th row "move a member between two types in one file → new identity"; "functions and
Term bindings contribute no descriptor (`Symbol::Local` remains unadopted)"; the residual
(function-local defs, object-literal methods, overloads with `disambiguator: None`, Rust
impl/Go receiver/Ruby `class << self` until their anchors exist); the collision evidence
(D03-1/D03-2/PER-7, `sqlite.rs:387-394`); `SYMBOL_ID_SCHEME` and the `id_scheme` gate;
the migration note (§4 below).
Tests: none (docs). `git diff --stat` shows only these three files.
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
# by-name lookups unaffected
$B blast-radius save --db $M/doc03-before.db --json; $A blast-radius save --db $M/doc03-after.db --json
# the no-force migration acceptance test
cp $M/doc03-before.db $M/doc03-migrate.db
$A index $M/doc03proj --db $M/doc03-migrate.db          # expect the SYMBOL-ID SCHEME line, no --force
Q $M/doc03-migrate.db "SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE s.sym LIKE '%/update().' OR s.sym LIKE '%/flush().'"   # expect ONLY Repo#update()., Cache#flush().
Q $M/doc03-migrate.db "SELECT v FROM meta WHERE k='id_scheme'"   # expect 2
cargo test -p wicked-estate-resolve
```
Expectations to state up front (so the reader does not misread the delta): nodes by kind `method`
/ `field` / `constructor` UP where collisions existed (crew `src/core/adapter.ts`: 4 classes,
69 stored methods, is the likely largest delta; studio has only 20 stored methods, so its delta
will be small); **Calls edges DOWN and unresolved rows UP** on collision files — `this.save()`
now sees ≥ 2 same-file candidates and `ScopedNameResolver` parks (`resolve/src/lib.rs:194-197`);
report per `resolved_by` so the drop is attributable to removed false-precision 0.65 edges;
`blast-radius save` returns more hits (multiplicity), never fewer names.

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
  embeddings (re-run `--embeddings`); any agent-held `--symbol` id from `resolve --json`.
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
2. After S1, `SELECT s.sym FROM nodes n JOIN symbols s ON s.sid=n.symbol WHERE s.sym LIKE '%/save().'`
   on `doc03-after.db` returns anything other than exactly one row (`…/save().` = `lit.save`,
   the D1 residual) plus `Repo#save().`, `Cache#save().`, `Store#save().`.
3. After S1, any Java `route-handler`/`event-listens`/`di-wired` edge in the framework tests has an
   endpoint that is not a `node.symbol` in the same `Extraction` (the graph does not join).
4. After S2, `doc03-migrate.db` (BEFORE-indexed, re-indexed by AFTER **without** `--force`) still
   contains `…/update().` or `…/flush().`, or `meta.id_scheme != 2`.
5. `cargo test -p wicked-estate-resolve` count moves from 61/1/4.
6. Any hunk lands in a `.scm` file, `resolve/src/lib.rs`, `wicked-estate/src/lib.rs:923-946`, or
   `treesitter.rs:540-600` (lane disjointness).

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

Merge notes → **resolver-precision lane**: after this lane, `by_name("save")` returns N nodes for
N types in one file; `ScopedNameResolver` parks them. The `from` id now carries the enclosing
type descriptors (structured `Symbol`, parse via `Symbol`/`Descriptor`, not string ops), which is
the prerequisite D03-1 said was missing.

Merge notes → **program integrator**: `treesitter.rs` hunks are confined to `:1341-1376`,
`:1740-1760`, `:1866-1905`, a new block before `:2040`, `:2146-2260`, and the test module
`:5488-5930`; the extension-routing table (`~:569-575`) is untouched.
