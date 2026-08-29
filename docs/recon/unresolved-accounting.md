# Unresolved accounting — one definition, applied everywhere

**Date:** 2026-08-28
**Lane:** `lane/unresolved-accounting` (base `d7d3b58`)
**Findings acted on:** D03-3@audit:doc03, D03-3@attack:receiver-inference, D03-1@repro:baseline-stats,
D01-9@repro:implement-and-measure, P-1 (`estate-review/review-artifacts/findings.json`); engine defect #3 in
`estate-review/REVIEW-adversarial-2026-08-28.md:158`.
**Status:** plan. No engine code changed by this document.

Every `file:line` below was opened in the lane worktree at `d7d3b58` unless a path says otherwise.

---

## 1. The defect, restated with evidence

Three code sites each answer "is this reference unresolved?" and they disagree.

| # | Site | Key it uses | Effect |
|---|---|---|---|
| (a) persistence | `crates/wicked-estate/src/lib.rs:937-944` — `resolved_locations: HashSet<Location>` from `resolved.iter().filter_map(\|e\| e.location.clone())`; a ref is persisted iff `!resolved_locations.contains(&r.location)` | exact `Location` of a **surviving** edge | **over-counts**: `resolve_all` keeps one edge (one location) per `(source,target,kind)` (`crates/wicked-estate-resolve/src/lib.rs:829-841`, key at `crates/wicked-estate-core/src/edge.rs:181-188`), so the 2nd..Nth site of a bound relationship has no surviving location and is written to `unresolved_refs` (`crates/wicked-estate-store/src/sqlite.rs:1620-1636`). |
| (b) telemetry | `crates/wicked-estate-resolve/src/lib.rs:891-912` — `resolved_ref_keys: HashSet<(source id, kind json)>` | `(source, kind)` | **under-counts**: one bound Calls edge from `f` cancels every Calls ref of `f`, including a call to an undefined `h()`. Introduced by `7c9caf0` ("Finding 7": None-location edges never cancelled their ref). The premise is gone: every resolver in the index slice attaches the ref location (`resolve/src/lib.rs:60, :211, :382, :530`). |
| (c) consumers | `sqlite.rs:2302-2312` `unresolved_refs_for_name` (`WHERE u.raw_name=?1`), `sqlite.rs:2750-2752` `COUNT(*)` → `GraphStats.unresolved_ref_count` | whatever (a) persisted | inherits (a): CLI `blast-radius` (`crates/wicked-estate/src/main.rs:1321, :1342, :1356-1361`), MCP `BlastRadius.unresolved_callers` + R3 coverage line (`crates/wicked-estate-retrieve/src/lib.rs:822-823, :884-901`), bench `blast_radius_coverage_pct` / `coverage_pct` (`crates/wicked-estate-bench/src/capability.rs:322-336, :541-550`). |

Measured on the review baselines (`scratchpad/baseline/{studio,crew}.db`, command in §7):

| Corpus | unresolved rows by kind | rows whose `(from_sym, raw_name→node.name, kind)` already has an edge | `raw_name='expect'` rows |
|---|---|---|---|
| studio | calls 38,536 · imports 1,857 · extends 7 | **6,317** (all Calls) | 4,932 |
| crew | calls 15,945 · imports 939 · extends 12 | **2,565** | 2,607 |

Synthetic fixture with the HEAD release binary (tests-lens recon, `measure/synth-before.db`): `f` calls `g()` three
times → 1 Calls edge (line 3) + 2 unresolved rows for `g` (lines 4, 5); `blast-radius g` prints
`2 unresolved call(s)`; the undefined `h()` gives exactly 1 row.

Two things the review did not say, both confirmed in recon and both shaping the design:

1. **The Location key also under-counts.** `class C extends A implements B` with `B` undefined persists **0**
   unresolved rows: both heritage refs use `ts_span(anchor)` (`crates/wicked-estate-extract/src/treesitter.rs:1964-1973`),
   so the Extends edge's location "covers" the Implements ref. Same pattern for event-emit refs (`:2005`).
   Any fix that stays location-keyed keeps this false negative — the R3 failure mode ("partial coverage is worse
   than none", `docs/agent-behavior-rules.md:19-22`).
2. **`Edge` does not carry `raw_name`.** `measure_synth_precision` already works around that with the same
   `(source, kind)` first-match heuristic as the telemetry counter (`resolve/src/lib.rs:742-747, :781-790`).
   A target-name join is not a safe general key either: Imports refs keep the quoted specifier (`'./mod'`,
   `treesitter.rs:1936-1937`) while the Import node is the de-quoted canonical (`:2078`), and the
   relative-imports lane binds specifiers to File nodes named by path.

The user-facing definition already exists in prose and contradicts the implementation: "captures that matched the
symbol's name in source but whose callers could not be resolved" (`docs/getting-started.md:118-121`); "14 confirmed
+ 3 unresolved" (`docs/graph-first-retrieval.md:161-163`). `docs/ENGINE-CONTRACT.md` has no definition at all
(§2 at `:28-42` only says refs are "bound later").

---

## 2. Decisions

### D1 — Definition and key: per-reference attribution, relationship-keyed coverage

> A reference is **unresolved** iff the resolve pass bound **no edge for its relationship**
> `(from, raw_name, kind)`. Every site of an unbound relationship is persisted (one `unresolved_refs`
> row per site). Sites of a bound relationship are never unresolved, whether or not their own location
> survived edge dedup.

How "bound" is determined — by **reference identity**, not by target name and not by location alone:

- Inside `resolve_all` (before dedup), each resolver's output edge is attributed to the ref(s) at
  `(edge.location, edge.kind)`. Refs are bucketed by that pair once (`HashMap<(&file, Span, kind), Vec<usize>>`
  over `refs`, O(R)).
- Bucket of size 1 (Calls, Imports — every case measured on studio/crew): the ref is bound; its key goes into
  `bound: HashSet<(SymbolId, String, kind_json)>`.
- Bucket of size >1 (heritage lists, event emits — refs that share an anchor span): the resolver is re-run on each
  ref of the bucket individually (`resolver.resolve(std::slice::from_ref(r), index)`); a ref is bound iff that call
  returns an edge. Exact, no name matching, and cheap: it costs one extra call per shared-span ref. All four
  shipped resolvers are per-ref loops with no cross-ref state (`resolve/src/lib.rs:46-49, :144-148, :330-335,
  :473-476`), so a single-ref call gives the same answer as the batch call. `RulesBridgeResolver` scans
  `all_nodes()` per call (`:578-586`) but is not in the index slice and its refs (`rules-engine:` prefix) never
  share a span.
- Edges with `location: None` attribute to nothing. Every slice resolver attaches the ref location; the contract
  states that a resolver whose edge should count as binding a ref must carry the ref's location.
- `unresolved = refs.iter().filter(|r| !bound.contains(&key(r)))` — the key uses `from`, so it is repo-scoped by
  construction (D4).

Why relationship-keyed and not per-ref `bound[i]`: for the shipped slice they are identical (binding is a function
of `(from, raw_name, kind)`, the per-file hints, and `location.file`, which is the same for every site of one
relationship: Calls attribute to the enclosing def in that file, Imports to the file symbol —
`treesitter.rs:2128-2132`). The brief's wording is the relationship, the edge dedup key is the relationship, and a
future site-aware resolver that binds site 1 still proves the relationship is resolved — the edge exists.

Why not the review's "count distinct `(from, raw_name)` pairs": it collapses **unbound** relationships too —
`expect` would go from 4,932 rows to 212 distinct `from_sym` (studio; 2,607 → 97 on crew). The brief requires
zero-candidate names not to drop; rows stay per-site.

Why not a target-name join (`edge.target.name == raw_name`): breaks on Imports (quoted specifier vs canonical
node), on the relative-imports lane's File targets, and would need per-language normalisation in Rust — the
"Rules as data" Don't. Ref identity needs none of it.

Why not location-only (today's key with dedup fixed): keeps the heritage/event false negative (§1 item 1).

### D2 — Repeat-site visibility: NO in this lane; `evidence_count` is off the table permanently

`Edge.evidence_count` is wicked-brain's confirm/contradict audit counter (`edge.rs:126-133`: "Structurally-derived
code edges leave it at 0; knowledge relations set it via `knowledge.relate`"), landed in `f79ef57` (PR #95). It
participates in the store merge rule in all three backends — `ON CONFLICT ... WHERE excluded.confidence >=
edges.confidence OR excluded.evidence_count > edges.evidence_count` (`sqlite.rs:1591-1597`, `postgres.rs:731-739`,
MemStore `store/src/lib.rs:357-360`) — the knowledge crate depends on that branch (`knowledge/src/engine.rs:806`,
"evidence growth must win the upsert"), and the conformance kit pins the honest 0 on a code edge
(`crates/wicked-estate-core/src/conformance.rs:196-202, :250-253`). Using it for site multiplicity would (i) let a
re-indexed 0.6 NameResolver edge carrying `sites=3` overwrite a SCIP 1.0 edge with `evidence_count=0`, (ii) be reset
to 0 by `wicked-estate scip` (`Edge::new` → 0, `lib.rs:1179-1194` upserts without `remove_file`), and (iii) rewrite
the brain-consolidation contract. Rejected.

A locations list on `Edge` reverses the W11 slim decision (`schema.sql:88-95`, "~8× disk reduction") and inflates
every edge-carrying surface (traverse `edge_json`, `export`, `edge_history`) by up to 148 (studio) / 169 (crew)
sites per key — an R4 regression. Rejected.

Nothing outside the repo asks for a site count: crew forwards `unresolved` as a number
(`wicked-crew/packages/crew/src/api/routes.ts:1781-1787`, `projects/graph.ts:921-936`), studio renders
"+N unresolved call(s)" (`wicked-studio/src/components/RepoGraphModal.tsx:152-155`), garden reads
`unresolved_callers` as prose (`wicked-garden/skills/search/SKILL.md:111-118`). A `metadata.sites` integer without
a consumer is an orphan (§5). If a consumer appears, the right carrier is a bounded integer in `Edge.metadata`
(ADR-001 `:46-48` names metadata as the extension point; it rides in `edges.data`, never enters the merge rule) —
recorded under §6 not-in-scope so it is not relitigated from scratch.

### D3 — Rows for refs that later resolve: keep the per-file delete path; no schema change; document the two exceptions

Trace: deleted files → `store.remove_file(path)` (`crates/wicked-estate/src/lib.rs:688-693`); **changed** files →
`store.remove_file(&fw.rel)` before re-extraction (`:718-724`); `remove_file` runs `DELETE FROM unresolved_refs
WHERE file=?1` (`sqlite.rs:1750-1752`; postgres `:847`; MemStore `store/src/lib.rs:419` `retain(|r|
r.location.file != file)`; surreal `:242-262`). The persisted `file` equals the from-symbol's file for both Calls
(enclosing def, same file) and Imports (file symbol), so a changed file's rows are rebuilt from scratch and cannot
accumulate. `unresolved_refs` has no uniqueness constraint (`schema.sql:96-105`) and needs none under this path.

Two documented exceptions, both pre-existing and outside this lane:
1. The unchanged-importer limitation (`crates/wicked-estate/src/lib.rs:17-24`): a ref in an unchanged file stays
   unresolved until that file changes or a full re-index runs.
2. `ingest_scip_as` upserts SCIP edges and never prunes `unresolved_refs` (`lib.rs:1179-1194`); the table lacks
   the `raw_name→target` mapping needed to prune by key. `docs/getting-started.md:208-209` promises the count
   "should drop significantly" after `scip` — that sentence is corrected to state the actual behaviour.

The definition is therefore stated as **per resolve pass**: rows reflect the last pass over each file.

The conformance kit gains one assertion the definition relies on and the kit currently lacks: `remove_file(f)` drops
`f`'s unresolved rows (`conformance.rs:493-516` asserts nodes only). No Postgres DDL change (`postgres.rs:195-204`),
no migration.

### D4 — Multi-repo: the key contains the namespaced `from`; no store-side scope change

Labelled runs namespace every path embedded in SymbolIds and in `location.file` (`crates/wicked-estate/src/repo_scope.rs:5-14`;
`lib.rs:664-674`); `InMemoryIndex::build(reader, scope)` filters candidates by prefix (`lib.rs:253-266`);
`all_refs` holds only this run's changed files (`lib.rs:877-883`). A key containing `from` cannot be cancelled by
another repo's edge. `unresolved_refs_for_name` stays label-blind (`sqlite.rs:2302-2312`) — crew's project route
already reports the union across member repos; that is pre-existing, documented in ENGINE-CONTRACT, and not
worsened. Proven by a unit test (step S2-T6) rather than another 157 s `multi_repo.rs` case
(`multi_repo.rs:253 resolution_is_scoped_to_the_indexed_repo` already covers resolution scoping).

### D5 — Where the set is computed: once, in `wicked-estate-resolve`, returned to the caller

`resolve_all` returns `Result<Vec<Edge>>` (`resolve/src/lib.rs:823-827`) and has 9 callers: `crates/wicked-estate/src/lib.rs:929`
(production), `:1879` (a lib test), and 7 in the resolve crate's test module (`:1428, :1651, :1699, :1754, :1763,
:2038, :2435`). The resolver-precision and relative-imports lanes are editing that test module. To keep the seam
stable for them:

- New `pub struct Resolution { pub edges: Vec<Edge>, pub unresolved: Vec<UnresolvedRef> }` and
  `pub fn resolve_all_with_coverage(resolvers, refs, index) -> Result<Resolution>` carrying the whole body (dedup,
  attribution, telemetry).
- `resolve_all` becomes `resolve_all_with_coverage(..).map(|r| r.edges)` — the edges-only view, kept for the
  test callers; its doc says production must use the coverage form. Nothing else is left behind: the
  `HashSet<Location>` block and the `(source, kind)` telemetry key are deleted in the same commit (§8).
- The telemetry counter `wicked_estate.resolve.unresolved` becomes `resolution.unresolved.len()`. It will go **up**
  (it under-counts today); the commit body cites `7c9caf0` and states why Finding 7's premise no longer holds.

### D6 — Consumer surfaces: numbers change, shapes do not

- `blast-radius --json` keeps `{target, dependents[], unresolved: number}` (`main.rs:1331-1345`); the text line keeps
  the `coverage: N resolved dependent(s); M unresolved call(s) reference '…'` prefix (`main.rs:1356-1361`) —
  `scripts/blast-check-pre-commit.sh:181, :189` greps it, crew types it (`crew-api-types/index.d.ts:1239-1244`).
- `stats` gains `unresolved=N` on its first line (`main.rs:1385-1388` prints `nodes= edges= files= db=`; the
  count is computed at `sqlite.rs:2750-2752` and printed nowhere today). crew's probe regex reads only the
  per-repo `files=` block (`wicked-crew/packages/crew/src/projects/graph.ts:225-229`); garden splits on whitespace
  and keeps `nodes/edges/files` (`wicked-garden/scripts/_estate_client.py:696-735`). Both tolerate the token.
- `unresolved_refs_for_name` unchanged (kind/scope filters are §6).
- Bench `coverage_pct` and `blast_radius_coverage_pct` rise; the only gate is a `[0,100]` range check
  (`crates/wicked-estate-bench/tests/integration_bench.rs:302-305`). `docs/benchmarks/capability-report.md:26-29`
  is regenerated.

---

## 3. Steps

Each step: files → change → tests that prove it → what it deletes. Per-crate cargo only, with
`CARGO_TARGET_DIR=<lane>/target`.

### S1 — `Resolution` + `resolve_all_with_coverage` (resolve crate)

**Files:** `crates/wicked-estate-resolve/src/lib.rs` (the `resolve_all` block `:806-935` and the end of the tests
module — append only), `crates/wicked-estate-resolve/README.md:12, :25, :46`.

**Change:**
1. Add `pub struct Resolution { pub edges: Vec<Edge>, pub unresolved: Vec<UnresolvedRef> }` (derive `Debug, Clone,
   Default`).
2. Add `pub fn resolve_all_with_coverage(resolvers, refs, index) -> Result<Resolution>`:
   - bucket refs by `(location.file, location.span, kind_json)` → `Vec<usize>`;
   - per resolver: `edges = resolver.resolve(refs, index)?`; for each edge with a location, look up the bucket:
     size 1 → insert `key(refs[i])` into `bound`; size >1 → remember `(resolver index, bucket)` for the collision
     pass; then dedup into `best` exactly as today (`:829-841`, strict `>`);
   - collision pass: for each remembered `(resolver, bucket)` and each ref `i` in the bucket not already bound,
     `resolver.resolve(std::slice::from_ref(&refs[i]), index)?` non-empty → bound;
   - `unresolved = refs.iter().filter(|r| !bound.contains(&key(r))).cloned().collect()`;
   - telemetry block moved here verbatim except the counter value = `unresolved.len()`; delete the
     `resolved_ref_keys` set and the Finding 7 comment (`:891-912`).
   - `key(r) = (r.from.0.clone(), r.raw_name.clone(), serde_json::to_string(&r.kind))` — a small private fn shared by
     the bucket and the filter so there is exactly one key expression.
3. `resolve_all` → `Ok(resolve_all_with_coverage(resolvers, refs, index)?.edges)`; doc comment updated (the
   existing "recommended order" text stays).
4. README: document `Resolution`/`resolve_all_with_coverage`; the example at `:46` uses the coverage form.

**Tests (resolve crate, appended at the end of `mod tests`; all use the existing `VecIndex` at `:1181` and the real
`NameResolver`/`ScopedNameResolver` unless stated):**
- T1 `coverage_repeat_call_sites_are_not_unresolved`: 3 Calls refs `f→g` at lines 3/4/5, `g` defined →
  `edges.len()==1`, `unresolved.is_empty()`.
- T2 `coverage_keeps_every_site_of_an_unbound_relationship`: 2 Calls refs `f→h`, `h` undefined → `edges.is_empty()`,
  `unresolved.len()==2` (honest coverage: rows are per site).
- T3 `coverage_key_includes_kind`: Calls ref `f→x` bound + Imports ref `file→x` (same raw_name) unbound → unresolved
  is exactly the Imports ref (the 58 studio rows with a same-name edge of another kind).
- T4 `coverage_attributes_shared_span_refs_individually`: two Implements refs at one `Location`, raw `A` (defined)
  and `B` (undefined) → 1 edge, `unresolved == [B ref]`. This is the fx2 false negative; fails at HEAD.
- T5 `coverage_repeat_import_statements_are_not_unresolved`: mock resolver that binds Imports refs by raw_name; two
  Imports refs, same `from`/raw_name, lines 0 and 1 → 1 edge, 0 unresolved. (The index_path form of this
  assertion needs the relative-imports lane — merge note M3.)
- T6 `coverage_key_is_scoped_by_from`: refs from `sa/src/x.ts/f().` and `sb/src/x.ts/f().`, both raw `g`; mock
  resolver binds only refs whose `location.file` starts with `sa/` → unresolved is exactly the `sb` ref (D4).
- T7 `coverage_ignores_edges_without_location`: mock resolver emitting a location-less edge → the ref stays
  unresolved and the edge is still returned (contract: attribution needs the location).
- Existing `resolve_all_*` tests at `:1403, :1638, :1666, :1707, :2021, :2408` must pass unchanged (they exercise
  the wrapper).

**Deletes:** the `(source, kind)` key and the Finding 7 comment (`resolve/src/lib.rs:891-912`).

**Gate:** `cargo build -p wicked-estate-resolve` 0 warnings; `cargo clippy -p wicked-estate-resolve -- -D warnings`;
`cargo test -p wicked-estate-resolve` → 61 + 7 new unit, 1 lsp_live, 4 scip_edges, doctests 1 passed / 1 ignored
(the `lsp.rs:532` orphan belongs to another lane — count unchanged, not this lane's to fix).

### S2 — Persistence consumes `Resolution` (wicked-estate crate)

**Files:** `crates/wicked-estate/src/lib.rs:929-946` only (line 929 is the call adjacent to the slice literal
`:923-928`, which is not touched — merge note M1).

**Change:** `let resolution = resolve_all_with_coverage(resolvers, &all_refs, &index)?;` returned from the block as
`(resolution, estate)`; `store.upsert_edges(&resolution.edges)?; store.upsert_unresolved_refs(&resolution.unresolved)?;`.
Replace the "Compute unresolved refs (same logic as full index)" comment with one sentence citing
`docs/ENGINE-CONTRACT.md §2.1`.

**Tests:** existing `cargo test -p wicked-estate` lib (47), main (20), integration (59) unchanged; plus S4's e2e.

**Deletes:** the `resolved_locations: HashSet<Location>` block and the filter (`lib.rs:937-944`), the stale
"(same logic as full index)" comment (there is no separate full-index path: `grep resolved_locations` → this site
only), and the now-unused `HashSet`/`Location` imports if nothing else in the file uses them (check with the build).

### S3 — CLI `stats` prints the count; docstrings cite the definition

**Files:** `crates/wicked-estate/src/main.rs:1385-1388` (stats line), `:1353-1361` (comment only),
`crates/wicked-estate-retrieve/src/lib.rs:763-765` (doc comment), `crates/wicked-estate-core/src/traits.rs:139-142,
:235-237` (doc comments), `crates/wicked-estate-bench/src/capability.rs:145-149, :700-708` (doc + generated report text).

**Change:** stats first line becomes `nodes={} edges={} files={} unresolved={} db={:.1}MB`. Doc comments say
"unresolved per `docs/ENGINE-CONTRACT.md §2.1`" instead of restating the definition. No format change to
`blast-radius` text or JSON.

**Tests:** S4's CLI test asserts the `unresolved=` token; `cargo test -p wicked-estate-retrieve` (100) and
`-p wicked-estate-bench` (10 + 5) unchanged.

**Deletes:** the three restated prose definitions in code comments (replaced by the citation).

### S4 — End-to-end regression tests (wicked-estate crate)

**Files:** `crates/wicked-estate/tests/e2e.rs` (new test fn), new `crates/wicked-estate/tests/unresolved_accounting_cli.rs`
(pattern: `tests/repo_flag_cli.rs`).

**Fixture** (written to a temp dir): `src/mod.ts` exports `g`, `k`; `src/main.ts`:
`import {g, k} from './mod'; import type {G} from './mod'; export function f(){ g(); g(); g(); h(); k(); }`.

**e2e assertions (`SqliteStore::in_memory` + `index_path`):**
- exactly one Calls edge with target name `g` from `f` (`all_edges()` filtered); `unresolved_refs_for_name("g")`
  is empty; `unresolved_refs_for_name("h").len() == 1`;
- `stats().unresolved_ref_count == unresolved_refs_for_name("h").len() + unresolved_refs_for_name("'./mod'").len()`
  — persistence and stats agree (the Imports rows are whatever the shipped resolvers leave; at HEAD that is 2,
  the relative-imports lane takes it to 0 — merge note M3; this assertion is written so it holds either way);
- incremental: append a 4th `g()` and a call to `h()` to `main.ts`, `index_path` again → `g` still 0 rows, `h` now 2
  rows (rebuilt, not accumulated: proves D3's `remove_file` on the changed file); delete `main.ts`, `index_path`
  → 0 rows for both;
- heritage: `src/c.ts`: `class A {}  class C extends A implements B {}` → `unresolved_refs_for_name("B").len() == 1`
  (fx2 false negative; 0 at HEAD).

**CLI test (`cargo run --bin wicked-estate` via `env!("CARGO_BIN_EXE_wicked-estate")`):** `index` the fixture;
`stats` stdout contains `unresolved=`; `blast-radius g` contains `0 unresolved call(s) reference 'g'`;
`blast-radius h` contains `1 unresolved call(s) reference 'h'`; `blast-radius g --json` parses and `unresolved == 0`.

**Deletes:** nothing (new tests). Baselines to record in the test names' doc comments: HEAD gives g=2, h=1, B=0.

### S5 — Conformance kit: `remove_file` clears the file's unresolved rows

**Files:** `crates/wicked-estate-core/src/conformance.rs` (after `:299-329`).

**Change:** upsert a second ghost ref located in `src/other.rs`; `remove_file("src/lib.rs")`; assert
`unresolved_refs_for_name("ghost").len() == 1` (the other file's row survives), `stats().unresolved_ref_count == 1`.

**Tests:** `cargo test -p wicked-estate-store` (SQLite + MemStore conformance: 7 tests; postgres/surreal
conformance are feature+env gated and run 0 tests locally — stated as unverified in the PR, not claimed).

**Deletes:** nothing. No schema, no migration, no Postgres DDL change.

### S6 — Docs: one normative definition, cited everywhere else

**Files and edits:**
- `docs/ENGINE-CONTRACT.md` — new `### 2.1 Unresolved references` under §2: the D1 definition verbatim; the key;
  "one row per site of an unbound relationship"; attribution by reference identity (a binding edge carries the
  ref's location); "computed per resolve pass" with the two D3 exceptions (unchanged importers; `scip` does not
  prune) and the tfstate path (`lib.rs:1236-1243` persists collector refs without a resolve pass — by design, no
  resolvers exist for them); consumers listed (persistence, telemetry counter, `unresolved_refs_for_name`,
  `GraphStats.unresolved_ref_count`). This is the only place the definition is written out.
- `docs/agent-behavior-rules.md` R3 (`:19-22`): one sentence — "unresolved references are defined in
  ENGINE-CONTRACT §2.1; the coverage line counts them per site".
- `docs/getting-started.md:118-121`: replace the "captures that matched the symbol's name … whose callers could not
  be resolved" bullet with the §2.1 wording + link; `:208-209`: replace "the unresolved-call count should drop
  significantly" with the truthful statement (SCIP edges land; rows are pruned on the next re-index of the file).
- `docs/graph-first-retrieval.md:161-163`: keep the example, add the citation.
- `docs/benchmarks/README.md:33, :64`: cite §2.1.
- `docs/recon/otel-instrumentation-audit.md:222`: counter description → "refs unresolved per ENGINE-CONTRACT §2.1".
- `docs/benchmarks/capability-report.md`: regenerate via `cargo run -p wicked-estate-bench --bin wicked-estate-bench --
  <repo>` per `docs/benchmarks/README.md:10-18` after S1-S3 (numbers move; the report is generated, not hand-edited).
- `crates/wicked-estate/src/lib.rs` module doc `:17-24`: add one line that the Known Limitation is exception 1 of
  §2.1.

**Deletes:** the four independent prose definitions (getting-started, graph-first, benchmarks README, capability.rs
generator text) as definitions — they become citations.

### S7 — Measurements (before/after), recorded in the PR body

Protocol per the brief. BEFORE binary = `/Users/michael.parcewski/Projects/wicked/wicked-estate/target/release/wicked-estate`
(read-only); AFTER = `<lane>/target/debug/wicked-estate` after `CARGO_TARGET_DIR=<lane>/target cargo build -p wicked-estate`.
DBs under `<lane>/measure/` (`mkdir -p`). Commands, verbatim (`$BIN`, `$DB`, `$REPO` substituted):

```
$BIN index $REPO --db $DB
$BIN stats --db $DB
$BIN blast-radius apiFetch --db $DB
$BIN blast-radius expect --db $DB
/usr/bin/sqlite3 $DB "select kind,count(*) from unresolved_refs group by kind;"
/usr/bin/sqlite3 $DB "select count(*) from unresolved_refs u where exists(select 1 from edges e join nodes n on n.symbol=e.target where e.source=u.from_sym and e.kind=u.kind and n.name=u.raw_name);"
/usr/bin/sqlite3 $DB "select count(*) from unresolved_refs where raw_name='expect';"
/usr/bin/sqlite3 $DB "select count(*) from unresolved_refs u where u.kind='\"imports\"';"
```
(`nodes.symbol` and `edges.target` are both sids; joining `nodes` on `symbols.sym` returns 0 — the wrong join in one
recon query. Kinds are JSON strings.)

Corpora: `/Users/michael.parcewski/Projects/wicked/wicked-studio`, `/Users/michael.parcewski/Projects/wicked/wicked-crew`,
plus the S4 fixture and `class C extends A implements B` (fx2) indexed through both binaries.

Expected AFTER (fresh full index): artifact query = **0** on studio and crew; Calls rows ≈ 32,219 (studio) /
13,380 (crew) (= before − artifact rows); `expect` = **4,932** / **2,607** unchanged; `blast-radius apiFetch`
unresolved **49 → 0** (`apiFetch` has 2 Calls edges in studio; all 49 rows are sites of those two relationships —
if any remain, they must be from a `from_sym` with no `apiFetch` edge and are listed by the query); `blast-radius
expect` count unchanged; `stats` prints `unresolved=`; fx2 → 1 Implements row (0 before); synthetic fixture → g 0
rows (2 before), h 1 row (1 before), Imports rows 2 before and 2 after in this lane (M3). Telemetry counter: not
observable in-process (`crates/wicked-estate-observe/src/lib.rs:322-331` OnceLock NoopSink) — proven by T1-T7
through the shared function instead; optionally `WICKED_OTEL_ENDPOINT` file sink on the fixture run if the sink
supports it.

Perf: BEFORE full studio index is 3.67 s wall (risks-lens `time -p`); the accounting is one O(R) bucket map plus
O(E) lookups, no SQL. Record `time -p $BIN index` before/after; a >5% regression is a defect (§9).

### Commit plan (lane branch, `--no-verify`, trailers)

1. `docs(recon): unresolved-accounting plan` — this file.
2. `fix(resolve): resolve_all_with_coverage — one unresolved definition, attributed per ref` — S1 (+ README).
3. `fix(index): persist unresolved from Resolution; drop the Location-keyed set` — S2 + S3 + S4.
4. `test(core): conformance — remove_file clears a file's unresolved rows` — S5.
5. `docs: define unresolved once (ENGINE-CONTRACT §2.1) and cite it` — S6 + regenerated capability report.

Commits 2 and 3 could be one; kept separate so the seam (S1) lands green before its consumer (§1).

---

## 4. Compatibility and migration

- **Schema:** none. `unresolved_refs` columns, indexes, Postgres DDL (`postgres.rs:195-204`), Surreal blob
  (`surreal.rs:188-194`) unchanged. `Edge` unchanged. `GraphStore` trait unchanged; the kit gains one assertion.
- **Stored graphs:** rows are recomputed only for files a run re-extracts (D3). An existing DB keeps the over-counted
  rows for unchanged files until they change or `wicked-estate index <path> --force` (`main.rs:673, :765`)
  re-extracts everything. Stated in the changelog and in ENGINE-CONTRACT §2.1; no migration step is possible
  without the in-memory refs.
- **Wire shapes:** `blast-radius --json` keys/types unchanged; text `coverage:` line unchanged; MCP `BlastRadius`
  content keys unchanged (schema goldens compare `inputSchema` only, `crates/wicked-estate-mcp/tests/conformance_schemas.rs:221-228`);
  `stats` line gains `unresolved=N` (consumers tolerant, D6).
- **Numbers that move:** every `unresolved` count and coverage percentage drops toward the truth; the OTel counter
  rises. Bench receipts (`docs/benchmarks/capability-report.md`) are regenerated in the same PR. Any doc example
  quoting a specific count is a stale example, not a contract.
- **Behavioural risk:** a resolver that emits a location-less edge no longer counts as binding anything (today the
  telemetry key counted it, persistence did not). All four slice resolvers attach locations; the contract now says so.
- **Determinism:** the surviving edge/location on a dedup tie is still first-seen (strict `>`); the unresolved set is
  order-independent (a set of keys). Ids untouched.

---

## 5. Falsifier

The plan is wrong if, after S1-S4 on a **fresh full index** of wicked-studio with the lane binary, any of:
1. the artifact query in §3/S7 returns > 0;
2. `select count(*) from unresolved_refs where raw_name='expect'` ≠ 4,932 (or crew ≠ 2,607);
3. `blast-radius apiFetch` reports > 0 unresolved while every `unresolved_refs` row for `apiFetch` has a same-source
   `apiFetch` edge;
4. the fx2 fixture (`class C extends A implements B`, `B` undefined) persists 0 unresolved rows;
5. T4 passes at HEAD without the collision pass (would mean the shared-span premise is wrong and the pass is dead code);
6. `time -p` full studio index regresses by > 5%.

---

## 6. Not in scope (owned elsewhere or deliberately deferred)

- **Repeat-site visibility** (`metadata.sites`, "called from 50 sites") — no consumer; D2 records the carrier if one
  appears.
- **`unresolved_refs_for_name` kind/scope filters** — it counts Imports rows as "call(s)" and sums across repos in
  a co-located graph (`sqlite.rs:2302-2312`); pre-existing consumer-side behaviour, documented in §2.1, not changed.
- **`ingest_scip_as` pruning `unresolved_refs`** (`lib.rs:1179-1194`) — needs a key the table does not store;
  documented as exception 2.
- **Unchanged-importer re-resolution** (`lib.rs:17-24`) — exception 1.
- **`ingest_tfstate`** (`lib.rs:1236-1243`) persists collector refs unresolved without a resolve pass — by design.
- **Telemetry `language` attribute** (`resolve/src/lib.rs:918` emits `"unknown"`) — unchanged.
- **Extends/Implements/event refs sharing an anchor span** (`treesitter.rs:1964-1973, :2005`) — handled exactly by
  the collision pass; giving each ref its own span is the extraction-gaps lane's call (M4).
- **NameResolver kind-blindness / wrong bindings** (defect #2): a ref bound to a wrong node is "resolved" under this
  definition — the accounting is faithful to the resolvers; precision is the resolver-precision lane.
- **`measure_synth_precision`'s `(source, kind)` first-match** (`resolve/src/lib.rs:742-747, :781-790`) — same defect
  class; flagged (M2), not edited here.
- Postgres/Surreal conformance execution (feature + env gated).

---

## 7. Merge notes for the other lanes

- **M1 (all lanes touching `crates/wicked-estate/src/lib.rs`):** this lane edits `:929` (the `resolve_all` call → `resolve_all_with_coverage`) and deletes `:937-944`. The slice literal `:923-928` is untouched. If a lane changes the slice, the call on `:929` must read `let resolution = resolve_all_with_coverage(resolvers, &all_refs, &index)?;`.
- **M2 (resolver-precision):** `measure_synth_precision` correlates edges to refs by `(source, kind)` first-match (`resolve/src/lib.rs:781-790`) — the same key the telemetry counter had. After S1, `Resolution` gives an exact per-ref answer; the synth-precision harness should use it. Also: `resolve_all` is now a wrapper; new tests in the resolve crate that need the unresolved set call `resolve_all_with_coverage`. This lane appends its tests at the **end** of `mod tests` to minimise conflicts.
- **M3 (relative-imports):** the brief's fixture expectation "1 Imports edge, 0 unresolved rows for the second `import './mod'`" is unreachable at HEAD — no shipped resolver binds quoted relative specifiers; the only Imports edge is the extractor-local File→Import-node edge (`resolved_by=tree-sitter`), not derived from the ref. This lane proves the mechanism with a mock resolver (T5) and writes the e2e stats assertion so it holds for any Imports count. After the relative-import resolver lands, add `assert!(store.unresolved_refs_for_name("'./mod'").is_empty())` to the S4 e2e test. Your resolver must call `.with_location(r.location.clone())` on every edge (the review patch does, 1 hit) or its refs will stay unresolved under §2.1.
- **M4 (method-identity / extraction-gaps):** heritage and event-emit refs reuse the declaring node's span (`treesitter.rs:1964-1973, :2005`). The collision pass makes accounting exact regardless; if you give each heritage target its own span, the collision pass simply stops firing (T4 must still pass — it constructs shared-span refs directly).
- **All lanes:** any doc or test that quotes a specific unresolved count (e.g. "49") is stale after this lane; cite `docs/ENGINE-CONTRACT.md §2.1` rather than restating the definition.
