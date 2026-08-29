# Unresolved accounting — one definition, applied everywhere

**Date:** 2026-08-28 (rev 2 — resolves attack issues A1-A3, UA-1, F1-F3 and folds in the minors)
**Lane:** `lane/unresolved-accounting` (base `d7d3b58`)
**Findings acted on:** D03-3@audit:doc03, D03-3@attack:receiver-inference, D03-1@repro:baseline-stats,
D01-9@repro:implement-and-measure, P-1 (`estate-review/review-artifacts/findings.json`); engine defect #3 in
`estate-review/REVIEW-adversarial-2026-08-28.md:158`.
**Status:** plan. No engine code changed by this document.

Every `file:line` below was opened in the lane worktree at `d7d3b58`/`cf41f52` unless a path says otherwise.

Revision 2 changes, by attack issue:
- **A1 (major):** the definition is now **per reference**, not per relationship `(from, raw_name, kind)`.
  The relationship key carried a latent false negative (one bound site cancels every same-simple-name site from
  the same enclosing symbol — `this.save()` vs `cache.save()`, raw_name is the method name without the receiver,
  `treesitter.rs:1911`) that becomes real the moment the resolver-precision lane's receiver inference lands.
  `key()`, the `(from, raw_name, kind)` HashSet, and D4's scope argument are dropped; attribution is `bound:
  Vec<bool>` by ref index. New T8 covers the receiver-hint case. Measured numbers are unchanged for the shipped
  slice (every slice resolver is a per-ref loop emitting one located edge per bound ref).
- **A2 + F2 (major):** the S4 e2e heritage fixture is now `class C implements A, B` (shared **(span, kind)** —
  the case that needs the collision pass end-to-end); `extends A implements B` is kept as a second case that the
  kind-in-bucket key alone fixes. §1 item 1 rewritten: at HEAD the persistence key is Location-only and
  **kind-less**, which is what breaks fx2.
- **A3 + F8 (major/minor):** the resolver contract (edge carries the ref's location AND kind; per-ref
  deterministic) is written on the `Resolver` trait itself (S3), not only in ENGINE-CONTRACT prose. The false
  "RulesBridge refs never share a span" claim is corrected (every rules-engine ref in a file sits at
  `Span::ZERO`/`InvokedBy`, `extra_edge.rs:360-366`) with a merge note for the defect-#4 lane. New T9 covers the
  kind half of the contract.
- **UA-1 + A6 (major/minor):** §4 rewritten: a version bump already **forces full re-extraction** per repo
  (`lib.rs:556-568`), so released consumers self-heal; the real hazard is a same-version re-index (mixed
  definitions, no warning). New S7 scenario measures exactly that; the change must ship under a version bump;
  CHANGELOG entry added to S6.
- **F1 (major):** T2-T9 use mock resolvers with explicit binding tables; only T1 touches the real slice.
  Accounting tests must not encode NameResolver kind semantics (owned by resolver-precision).
- **F3 (major):** the perf row compares release vs release (base `d7d3b58` built in a separate target dir).
- Minors folded in: A4/F5 (bucket-size claim dropped; histogram measured in-process), A5 (wrapper removal task),
  A7 (no JSON strings in the hot-path key), A8 (governance.md + absent ./wiki), UA-2 (collision pass is one call
  per (resolver, ref)), UA-3/F7 (stats identity fixed; direct Finding-7 regression test), UA-4 (scip prune
  wording), UA-5 (site-multiplicity contract sentence), UA-6 (capability-report corpora note), UA-7 (CHANGELOG in
  file lists; `BlastRadius::description()` is a golden — untouched), F4 (conformance placement), F6 (falsifier 5
  as a negative control).

---

## 1. The defect, restated with evidence

Three code sites each answer "is this reference unresolved?" and they disagree.

| # | Site | Key it uses | Effect |
|---|---|---|---|
| (a) persistence | `crates/wicked-estate/src/lib.rs:937-944` — `resolved_locations: HashSet<Location>` from `resolved.iter().filter_map(\|e\| e.location.clone())`; a ref is persisted iff `!resolved_locations.contains(&r.location)` | exact `Location` of a **surviving** edge — kind-less | **over-counts**: `resolve_all` keeps one edge (one location) per `(source,target,kind)` (`crates/wicked-estate-resolve/src/lib.rs:829-841`, key at `crates/wicked-estate-core/src/edge.rs:181-188`), so the 2nd..Nth site of a bound relationship has no surviving location and is written to `unresolved_refs` (`crates/wicked-estate-store/src/sqlite.rs:1620-1636`). |
| (b) telemetry | `crates/wicked-estate-resolve/src/lib.rs:891-912` — `resolved_ref_keys: HashSet<(source id, kind json)>` | `(source, kind)` | **under-counts**: one bound Calls edge from `f` cancels every Calls ref of `f`, including a call to an undefined `h()`. Introduced by `7c9caf0` ("Finding 7": None-location edges never cancelled their ref). The premise is gone: every resolver in the index slice attaches the ref location (`resolve/src/lib.rs:60, :211, :382, :530`). |
| (c) consumers | `sqlite.rs:2302-2312` `unresolved_refs_for_name` (`WHERE u.raw_name=?1`), `sqlite.rs:2750-2752` `COUNT(*)` → `GraphStats.unresolved_ref_count` | whatever (a) persisted | inherits (a): CLI `blast-radius` (`crates/wicked-estate/src/main.rs:1321, :1342, :1356-1361`), MCP `BlastRadius.unresolved_callers` + R3 coverage line (`crates/wicked-estate-retrieve/src/lib.rs:822-823, :884-901`), bench `blast_radius_coverage_pct` / `coverage_pct` (`crates/wicked-estate-bench/src/capability.rs:322-336, :541-550`). |

Measured on the review baselines (`scratchpad/baseline/{studio,crew}.db`, commands in §3/S7):

| Corpus | unresolved rows by kind | rows whose `(from_sym, raw_name→node.name, kind)` already has an edge | `raw_name='expect'` rows |
|---|---|---|---|
| studio | calls 38,536 · imports 1,857 · extends 7 | **6,317** (all Calls) | 4,932 |
| crew | calls 15,945 · imports 939 · extends 12 | **2,565** | 2,607 |

Synthetic fixture with the HEAD release binary (tests-lens recon, `measure/synth-before.db`): `f` calls `g()` three
times → 1 Calls edge (line 3) + 2 unresolved rows for `g` (lines 4, 5); `blast-radius g` prints
`2 unresolved call(s)`; the undefined `h()` gives exactly 1 row.

Two things the review did not say, both confirmed in recon and both shaping the design:

1. **The Location key also under-counts, via two distinct mechanisms.** (i) The HEAD persistence key is
   Location-only and **kind-less** (`lib.rs:937-944` is a `HashSet<Location>`): `class C extends A implements B`
   with `B` undefined persists **0** rows, because the Extends edge's location "covers" the Implements ref — both
   heritage refs use `ts_span(anchor)` with **different kinds** (`crates/wicked-estate-extract/src/treesitter.rs:1964-1973`).
   Putting kind into the key fixes this case by itself. (ii) Multi-target clauses share **(span, kind)**:
   `class C implements A, B` produces one query match per `type_identifier` with the same class anchor
   (`crates/wicked-estate-extract/src/queries/typescript.scm:54-59`; interfaces: `extends_type_clause`, `:67-71`),
   so two Implements refs sit at one `(location, kind)`. No location-shaped key can attribute the single edge for
   `A` to the right ref — that case needs per-ref re-resolution (the collision pass, D1). Same pattern for
   event-emit refs (`treesitter.rs:2005`) and for rules-engine refs, which all sit at `Span::ZERO`/`InvokedBy`
   in one file (`crates/wicked-estate-extract/src/extra_edge.rs:360-366`). Any fix that cannot separate these is
   the R3 failure mode ("partial coverage is worse than none", `docs/agent-behavior-rules.md:19-22`).
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

### D1 — Definition: per-reference attribution (A1)

> A reference is **unresolved** iff no resolver emitted an edge **attributed to it** — an edge carrying the
> reference's exact `(location, kind)` — after per-ref re-resolution of references that share `(location, kind)`.
> Each unresolved reference is one `unresolved_refs` row (rows stay per site; zero-candidate names like `expect`
> keep every row).

How attribution works, inside the new `resolve_all_with_coverage` (before edge dedup):

- `bound: Vec<bool>` indexed by ref position. No key function, no `raw_name` in the accounting, no scope
  argument — a ref is its own identity, scoped by construction (D4).
- Refs are bucketed once by `(&location.file, location.span, &kind)` →
  `HashMap<(&str, Span, &EdgeKind), Vec<usize>>` (O(R); `EdgeKind` derives `Hash + Eq`, `edge.rs:91-93`;
  `Span` derives `Hash + Eq`, `node.rs:21`; **no JSON strings on this path** — A7).
- Per resolver: for each output edge with a location, look up the bucket at `(edge.location, edge.kind)`.
  Bucket size 1 → `bound[i] = true`. Bucket size >1 → the edge is ambiguous between the bucket's refs; record
  `(resolver_idx, bucket_id)` in a `HashSet` (each pair processed **once** — UA-2).
- Collision pass: for each recorded `(resolver, bucket)`, for each ref `i` in the bucket with `!bound[i]`,
  `resolver.resolve(std::slice::from_ref(&refs[i]), index)?` returns an edge at the ref's `(location, kind)` →
  `bound[i] = true`. Cost bound: **one resolve call per (resolver, unbound shared-key ref)** — stated in the
  code comment. Exact because every slice resolver is a stateless per-ref loop
  (`resolve/src/lib.rs:46-49, :144-148, :330-335, :473-476`), which S3 promotes from observation to contract on
  the `Resolver` trait (traits.rs:44-51 has no contract text today).
- Edges with `location: None` attribute to nothing (T7); edges whose kind differs from every ref at their
  location attribute to nothing (T9). Dedup into `best` runs exactly as today (`:829-841`, strict `>`) **after**
  attribution.
- `unresolved = refs where !bound[i]`.

**Why per-ref and not the relationship key `(from, raw_name, kind)` (rev-1 design, rejected by A1):**
`raw_name` is the method name without the receiver (`treesitter.rs:1911-1917`), so `this.save()` and
`cache.save()` inside one function share the relationship key. The resolver-precision lane's brief is receiver
inference (D03-1/D03-3; ADR-001:46-48 names call-site receiver types as edge metadata): the moment a resolver
binds one receiver's call using per-ref hints, a relationship key would mark the *other* receiver's call
"resolved" with **no edge** — a silent false negative. Per-ref attribution is also strictly less machinery: the
rev-1 plan computed per-ref attribution and then collapsed it into a key; the collapse is deleted, nothing is
added. For the shipped slice the two give identical numbers (the only per-ref hint today is the per-file
`imports` map, `treesitter.rs:2133-2139`); T8 pins the case where they diverge.

**Why not the review's "count distinct `(from, raw_name)` pairs":** it collapses **unbound** relationships too —
`expect` would go from 4,932 rows to 212 distinct `from_sym` (studio; 2,607 → 97 on crew). The brief requires
zero-candidate names not to drop; rows stay per-site.

**Why not a target-name join (`edge.target.name == raw_name`):** breaks on Imports (quoted specifier vs
canonical node), on the relative-imports lane's File targets, and would need per-language normalisation in Rust —
the "Rules as data" Don't. Ref identity needs none of it.

**Why not location-only (today's key with dedup fixed):** kind-less, and cannot separate multi-target clauses
(§1 item 1).

**Cost note (A4/F5):** rev 1 claimed shared-key buckets were "measured" absent for Calls/Imports; the persisted
table stores only `line` (no span, `schema.sql:96-105`, `sqlite.rs:1620-1636`), so that is not measurable from
the DBs and the claim is withdrawn. Correctness does not depend on bucket sizes (the collision pass is exact for
any size); the cost is bounded by falsifier (6) on like-for-like builds, and S7 records the in-process bucket
histogram + collision-call count on the fixture and studio runs.

### D2 — Repeat-site visibility: NO in this lane; `evidence_count` is off the table permanently

<!-- historical -->
`Edge.evidence_count` is retired wicked-brain's confirm/contradict audit counter, inherited in the
consolidation (`edge.rs:126-133`: "Structurally-derived
code edges leave it at 0; knowledge relations set it via `knowledge.relate`"), landed in `f79ef57` (PR #95). It
participates in the store merge rule in all three backends — `ON CONFLICT ... WHERE excluded.confidence >=
edges.confidence OR excluded.evidence_count > edges.evidence_count` (`sqlite.rs:1591-1597`, `postgres.rs:731-739`,
MemStore `store/src/lib.rs:357-360`) — the knowledge crate depends on that branch (`knowledge/src/engine.rs:806`,
"evidence growth must win the upsert"), and the conformance kit pins the honest 0 on a code edge
(`crates/wicked-estate-core/src/conformance.rs:196-202, :250-253`). Using it for site multiplicity would (i) let a
re-indexed 0.6 NameResolver edge carrying `sites=3` overwrite a SCIP 1.0 edge with `evidence_count=0`, (ii) be reset
to 0 by `wicked-estate scip` (`Edge::new` → 0, `lib.rs:1179-1194` upserts without `remove_file`), and (iii) rewrite
the brain-consolidation contract. Rejected.
<!-- /historical -->

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

**Contract consequence (UA-5), stated in §2.1 and in the blast-radius doc comment (`retrieve/src/lib.rs:763-765`
— the doc comment only; `BlastRadius::description()` at `:775-780` is embedded in MCP goldens and is not edited,
UA-7):** after this fix, edges and unresolved counts are per relationship / per unbound site respectively; the
number of call sites of a bound relationship is **persisted nowhere** (it previously leaked to consumers through
the over-counted rows — studio's "+49 unresolved call(s)" banner for `apiFetch` disappears rather than becoming
"called from 49 sites"). D2 records the carrier if a consumer ever asks.

### D3 — Rows for refs that later resolve: keep the per-file delete path; no schema change; document the exceptions

Trace: deleted files → `store.remove_file(path)` (`crates/wicked-estate/src/lib.rs:688-693`); **changed** files →
`store.remove_file(&fw.rel)` before re-extraction (`:718-724`); `remove_file` runs `DELETE FROM unresolved_refs
WHERE file=?1` (`sqlite.rs:1750-1752`; postgres `:847`; MemStore `store/src/lib.rs:419` `retain(|r|
r.location.file != file)`; surreal `:242-262`). The persisted `file` equals the from-symbol's file for both Calls
(enclosing def, same file) and Imports (file symbol), so a changed file's rows are rebuilt from scratch and cannot
accumulate. `unresolved_refs` has no uniqueness constraint (`schema.sql:96-105`) and needs none under this path.

Two documented exceptions, both pre-existing and outside this lane:
1. The unchanged-importer limitation (`crates/wicked-estate/src/lib.rs:17-24`): a ref in an unchanged file stays
   unresolved until that file changes or a full re-index runs.
2. `ingest_scip_as` upserts SCIP edges and does not prune `unresolved_refs` (`lib.rs:1179-1194`). **Deferred, not
   impossible** (UA-4): a Calls-only prune by `(from_sym, kind, target.name == raw_name)` is feasible — every
   slice resolver binds by `index.by_name(&r.raw_name)` and SCIP Calls edges target the definition node — and is
   the recorded follow-up; a *general* prune fails on Imports (quoted specifier vs canonical, §1 item 2), which
   is why it is not the definition. `docs/getting-started.md:208-209` promises the count "should drop
   significantly" after `scip` — that sentence is corrected to state the actual behaviour.

The definition is therefore stated as **per resolve pass**: rows reflect the last pass over each file.

The conformance kit gains one assertion the definition relies on and the kit currently lacks: `remove_file(f)` drops
`f`'s unresolved rows (`conformance.rs:493-505` asserts nodes only). No Postgres DDL change (`postgres.rs:195-204`),
no migration.

### D4 — Multi-repo: per-ref is scoped by construction; no store-side scope change

There is no key to scope (A1): each ref is attributed individually, and `all_refs` holds only this run's changed
files (`lib.rs:877-883`), whose paths — and the SymbolIds/locations inside them — are namespaced by the label
(`crates/wicked-estate/src/repo_scope.rs:5-14`; `lib.rs:664-674`). `InMemoryIndex::build(reader, scope)` filters
candidates by prefix (`lib.rs:253-266`). A ref from repo `sa` cannot be marked bound by anything that happens to
repo `sb`'s refs, because nothing aggregates across refs any more. `unresolved_refs_for_name` stays label-blind
(`sqlite.rs:2302-2312`) — crew's project route already reports the union across member repos; that is
pre-existing, documented in ENGINE-CONTRACT, and not worsened. Proven by unit test T6 rather than another 157 s
`multi_repo.rs` case (`multi_repo.rs:253 resolution_is_scoped_to_the_indexed_repo` already covers resolution
scoping).

### D5 — Where the set is computed: once, in `wicked-estate-resolve`, returned to the caller

`resolve_all` returns `Result<Vec<Edge>>` (`resolve/src/lib.rs:823-827`) and has 9 callers: `crates/wicked-estate/src/lib.rs:929`
(production), `:1879` (a lib test), and 7 in the resolve crate's test module (`:1428, :1651, :1699, :1754, :1763,
:2038, :2435`). The resolver-precision and relative-imports lanes are editing that test module. To keep the seam
stable for them:

- New `pub struct Resolution { pub edges: Vec<Edge>, pub unresolved: Vec<UnresolvedRef> }` and
  `pub fn resolve_all_with_coverage(resolvers, refs, index) -> Result<Resolution>` carrying the whole body
  (attribution, collision pass, dedup, telemetry).
- `resolve_all` becomes `resolve_all_with_coverage(..).map(|r| r.edges)` — the edges-only view, kept **only** so
  the 7 test call sites other lanes are editing don't conflict; its doc says production must use the coverage
  form. **Removal is a tracked migration remainder, not a permanent second entry point (A5):** the wrapper and
  its 9 call sites are renamed in the post-lane integration merge of this review program (recorded as merge note
  M2's closing task; deadline = the integration merge that lands after resolver-precision and relative-imports,
  i.e. before the release that ships this fix). Nothing else is left behind: the `HashSet<Location>` block and
  the `(source, kind)` telemetry key are deleted in the same commits as their replacement (§8 retire-as-you-go).
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
  (`crates/wicked-estate-bench/tests/integration_bench.rs:302-305`). `docs/benchmarks/capability-report.md` is
  regenerated — **corpora note (UA-6):** three of its four current rows are external "prior art" repos
  (`capability-report.md:26-31`) not present in this workspace; if those paths exist locally the report is
  regenerated against the same corpora (paths named in the PR body), otherwise it is regenerated on studio + crew
  with a one-line note under the table that the corpus set changed with this PR and rows are not comparable to
  the previous report.
- `CHANGELOG.md` `[Unreleased]` gains a `Fixed` entry (UA-7): the definition citation, the 49→0 `apiFetch`
  example, and the UA-1 statement (version bump ⇒ full re-extract on next `index`; same-version binaries need
  `index --force`).

### D7 — Resolver contract, written on the trait (A3)

The accounting imposes a real contract on `Resolver::resolve` that today exists nowhere
(`crates/wicked-estate-core/src/traits.rs:44-51` is bare). S3 writes it as the trait's doc comment, verbatim
target:

> A resolver binds a reference by returning an edge that carries the reference's exact location **and** kind —
> attribution is by `(edge.location, edge.kind)`; an edge with a different kind, or `location: None`, binds
> nothing (it is still returned and may survive dedup). `resolve()` must be deterministic per ref — calling it
> with a single-ref slice must give that ref's portion of the batch answer — because the accounting re-runs it
> per ref for references that share `(location, kind)`.

All six current implementations already satisfy it (slice resolvers `resolve/src/lib.rs:52-60, :202-210,
:372-382, :522-530`; RulesBridge `:613-621`; MethodResolutionSynthesizer `:684-692` — the latter two are not in
the index slice, `wicked-estate/src/lib.rs:923-928`). T7 (location half) and T9 (kind half) pin it.

**RulesBridge correction (A3):** rev 1 claimed its refs "never share a span". False — every rules-engine ref in a
file is emitted at `(file, Span::ZERO, InvokedBy)` from the file symbol (`extra_edge.rs:360-366`), so two schemes
in one file share a bucket. If the defect-#4 lane wires `RulesBridgeResolver` into the slice, each collided ref
re-runs a `resolve()` that scans `all_nodes()` (`resolve/src/lib.rs:578-586`) — k single-ref re-runs per file,
k = schemes per file. Acceptable only at that k, and it must be measured when wired; the durable fix is distinct
spans per rules-engine ref. Recorded as merge note M5.

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
2. Add `pub fn resolve_all_with_coverage(resolvers, refs, index) -> Result<Resolution>` implementing D1 exactly:
   - `bound: Vec<bool>` sized to `refs.len()`;
   - bucket refs once by `(&r.location.file as &str, r.location.span, &r.kind)` → `Vec<usize>` (no
     `serde_json::to_string` on this path — A7);
   - per resolver: `edges = resolver.resolve(refs, index)?`; for each edge with a location, look up the bucket at
     `(edge.location.file, edge.location.span, &edge.kind)`: size 1 → `bound[i] = true`; size >1 → insert
     `(resolver_idx, bucket_id)` into a `HashSet` (once per pair — UA-2); then dedup into `best` exactly as today
     (`:829-841`, strict `>`);
   - collision pass: for each recorded `(resolver, bucket)`, for each `i` in the bucket with `!bound[i]`,
     `resolver.resolve(std::slice::from_ref(&refs[i]), index)?` yields an edge at the ref's `(location, kind)` →
     `bound[i] = true`. Doc comment states the cost bound: one extra resolve call per (resolver, unbound
     shared-key ref);
   - `unresolved = refs.iter().enumerate().filter(|(i, _)| !bound[*i]).map(|(_, r)| r.clone()).collect()`;
   - telemetry block moved here verbatim except the counter value = `unresolved.len()`; delete the
     `resolved_ref_keys` set and the Finding 7 comment (`:891-912`).
3. `resolve_all` → `Ok(resolve_all_with_coverage(resolvers, refs, index)?.edges)`; doc comment updated (the
   existing "recommended order" text stays) + the A5 removal note.
4. README: document `Resolution`/`resolve_all_with_coverage`; the example at `:46` uses the coverage form.

**Tests (resolve crate, appended at the end of `mod tests`).** Per F1, **T2-T9 use mock resolvers** implementing
the 3-method `Resolver` trait (`traits.rs:44-51`) with explicit binding tables (a set of ref indices or
`(from, raw_name, kind)` rows the mock binds, emitting one edge per bound ref at that ref's `(location, kind)`)
— accounting tests must not encode NameResolver kind semantics, which the resolver-precision lane owns and will
change. T1 is the single real-slice smoke test.

- T1 `coverage_repeat_call_sites_are_not_unresolved` (real `NameResolver`, `VecIndex` at `:1181`, one Function
  node `g`): 3 Calls refs `f→g` at lines 3/4/5 → `edges.len()==1` post-dedup, `unresolved.is_empty()`.
- T2 `coverage_keeps_every_site_of_an_unbound_relationship` (mock binds nothing): 2 Calls refs `f→h` →
  `edges.is_empty()`, `unresolved.len()==2` (honest coverage: rows are per site).
- T3 `attribution_key_includes_kind` (mock emits one Calls edge at location L): a Calls ref and an Imports ref
  both at L → the Calls ref is bound, the Imports ref is unresolved (buckets are per kind).
- T4 `collision_pass_attributes_shared_key_refs_individually` (mock binds raw_name `A` only): two Implements refs
  at one `(location, kind)`, raw `A` and `B` → 1 edge, `unresolved == [B ref]`. **Negative control (F6):** run
  once during S1 with the collision pass disabled (a >1 bucket treated as binding all its refs) and record the
  failing assertion in the commit body — `resolve_all_with_coverage` does not exist at HEAD, so "fails at HEAD"
  is not executable; this is the executable equivalent.
- T5 `repeat_import_statements_are_not_unresolved` (mock binds Imports by raw_name): two Imports refs, same
  `from`/raw_name, lines 0 and 1 → 1 edge post-dedup, 0 unresolved. (The index_path form of this assertion needs
  the relative-imports lane — merge note M3.)
- T6 `accounting_is_scoped_per_ref` (mock binds only refs whose `location.file` starts with `sa/`): refs from
  `sa/src/x.ts/f().` and `sb/src/x.ts/f().`, both raw `g` → unresolved is exactly the `sb` ref (D4).
- T7 `edges_without_location_attribute_nothing` (mock emits a location-less edge): the ref stays unresolved and
  the edge is still returned (D7, location half).
- T8 `a_bound_site_does_not_cancel_a_sibling_site` (A1's differentiator; mock with a per-ref binding table): two
  Calls refs, same `from`, same raw_name `save`, same kind, different locations (the `this.save()` /
  `cache.save()` shape, `treesitter.rs:1911-1917`); the mock binds only the first → `unresolved == [second ref]`.
  Under the rejected relationship key this test fails; it pins the receiver-inference safety.
- T9 `edge_kind_must_match_ref_kind` (D7, kind half; mock emits an edge at the ref's location with a different
  kind): the ref stays unresolved.
- Existing `resolve_all_*` tests at `:1403, :1638, :1666, :1707, :2021, :2408` must pass unchanged (they exercise
  the wrapper).

**Deletes:** the `(source, kind)` key and the Finding 7 comment (`resolve/src/lib.rs:891-912`).

**Gate:** `cargo build -p wicked-estate-resolve` 0 warnings; `cargo clippy -p wicked-estate-resolve -- -D warnings`;
`cargo test -p wicked-estate-resolve` → 61 + 9 new unit, 1 lsp_live, 4 scip_edges, doctests 1 passed / 1 ignored
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

### S3 — CLI `stats` prints the count; the contract lands on the trait; docstrings cite the definition

**Files:** `crates/wicked-estate/src/main.rs:1385-1388` (stats line), `:1353-1361` (comment only),
`crates/wicked-estate-retrieve/src/lib.rs:763-765` (doc comment **only** — `BlastRadius::description()` at
`:775-780` is embedded in the MCP goldens `crates/wicked-estate-mcp/tests/conformance/raw/wicked-estate-mcp-0.12.0-tools-list.json:90`
and `conformance/schemas/BlastRadius.json:3`; do not edit it — UA-7),
`crates/wicked-estate-core/src/traits.rs:44-51` (the `Resolver` trait doc — D7 contract text) and `:139-142,
:235-237` (doc comments), `crates/wicked-estate-bench/src/capability.rs:145-149, :700-708` (doc + generated report text).

**Change:** stats first line becomes `nodes={} edges={} files={} unresolved={} db={:.1}MB`. The `Resolver` trait
doc gets the D7 contract verbatim. Other doc comments say "unresolved per `docs/ENGINE-CONTRACT.md §2.1`" instead
of restating the definition; the blast-radius doc comment gains the UA-5 per-relationship sentence. No format
change to `blast-radius` text or JSON.

**Tests:** S4's CLI test asserts the `unresolved=` token; `cargo test -p wicked-estate-retrieve` (100) and
`-p wicked-estate-bench` (10 + 5) unchanged; MCP goldens unchanged (`description()` untouched).

**Deletes:** the three restated prose definitions in code comments (replaced by the citation).

### S4 — End-to-end regression tests (wicked-estate crate)

**Files:** `crates/wicked-estate/tests/e2e.rs` (new test fns), new `crates/wicked-estate/tests/unresolved_accounting_cli.rs`
(pattern: `tests/repo_flag_cli.rs`).

**Fixture A** (temp dir 1): `src/mod.ts` exports `g`, `k`; `src/main.ts`:
`import {g, k} from './mod'; import type {G} from './mod'; export function f(){ g(); g(); g(); h(); k(); }`.

**Fixture B — heritage** (temp dir 2, its own store, so Fixture A's stats identity stays exact — UA-3/F7):
- **B1 (collision pass, end-to-end — A2/F2):** `src/c1.ts`: `interface A {}  class C implements A, B {}` —
  two Implements refs share `(span, kind)` (`typescript.scm:54-59`, one match per `type_identifier`;
  `treesitter.rs:1964-1973`); expect `unresolved_refs_for_name("B").len() == 1` and 0 rows for `A`. HEAD
  baseline: 0 rows for `B` (kind-less Location set — record in the test doc comment).
- **B2 (kind-in-bucket):** `src/c2.ts`: `class A {}  class C extends A implements B {}` — different kinds at one
  span; expect `unresolved_refs_for_name("B").len() == 1`. HEAD baseline: 0 rows (fixed by the bucket key alone;
  labelled as such in the doc comment so nobody re-attributes it to the collision pass).

**e2e assertions (`SqliteStore::in_memory` + `index_path`, Fixture A):**
- exactly one Calls edge with target name `g` from `f` (`all_edges()` filtered); `unresolved_refs_for_name("g")`
  is empty; `unresolved_refs_for_name("h").len() == 1` — the direct Finding-7 regression: same `from`, same kind,
  one bound raw_name (`g`) and one unbound (`h`), the exact under-count input of `7c9caf0`'s `(source, kind)`
  key (F7);
- `stats().unresolved_ref_count == unresolved_refs_for_name("h").len() + unresolved_refs_for_name("'./mod'").len()`
  — persistence and stats agree (the Imports rows are whatever the shipped resolvers leave; at HEAD that is 2,
  the relative-imports lane takes it to 0 — merge note M3; the assertion holds either way, and Fixture B lives in
  its own store so no heritage row leaks into this sum — UA-3);
- incremental: append a 4th `g()` and a call to `h()` to `main.ts`, `index_path` again → `g` still 0 rows, `h` now 2
  rows (rebuilt, not accumulated: proves D3's `remove_file` on the changed file); delete `main.ts`, `index_path`
  → 0 rows for both.

**CLI test (`env!("CARGO_BIN_EXE_wicked-estate")`, Fixture A):** `index` the fixture;
`stats` stdout contains `unresolved=`; `blast-radius g` contains `0 unresolved call(s) reference 'g'`;
`blast-radius h` contains `1 unresolved call(s) reference 'h'`; `blast-radius g --json` parses and `unresolved == 0`.

**Deletes:** nothing (new tests). HEAD baselines recorded in the tests' doc comments: g=2, h=1, B1=0, B2=0.

### S5 — Conformance kit: `remove_file` clears the file's unresolved rows

**Files:** `crates/wicked-estate-core/src/conformance.rs`.

**Change (placement per F4 — no new `remove_file` call, no assertions ahead of sections that still read
`src/lib.rs` state):** upsert a second ghost ref located in `src/other.rs` **next to the existing ghost upsert at
`:299-312`**; the two assertions — `unresolved_refs_for_name("ghost").len() == 1` (the other file's row survives)
and `stats().unresolved_ref_count == 1` — go **immediately after the kit's existing
`store.remove_file("src/lib.rs")` at `:496`**, before the prune_dangling_edges section.

**Tests:** `cargo test -p wicked-estate-store` (SQLite + MemStore conformance: 7 tests; postgres/surreal
conformance are feature+env gated and run 0 tests locally — stated as unverified in the PR, not claimed).

**Deletes:** nothing. No schema, no migration, no Postgres DDL change.

### S6 — Docs: one normative definition, cited everywhere else

**Files and edits:**
- `docs/ENGINE-CONTRACT.md` — new `### 2.1 Unresolved references` under §2: the D1 definition verbatim (per-ref,
  attribution by `(edge.location, edge.kind)`, collision re-run for shared keys); "one row per unresolved
  reference (per site)"; the D7 resolver contract (location AND kind, per-ref deterministic); the UA-5 sentence
  (site multiplicity of a bound relationship is not persisted); "computed per resolve pass" with the D3
  exceptions (unchanged importers; `scip` does not prune — deferred Calls-only prune named) and the tfstate path
  (`lib.rs:1236-1243` persists collector refs without a resolve pass — by design, no resolvers exist for them);
  the UA-1 re-index statement (version bump ⇒ full re-extraction on next `index`, `lib.rs:556-568`; same-version
  binaries need `index --force`); consumers listed (persistence, telemetry counter, `unresolved_refs_for_name`,
  `GraphStats.unresolved_ref_count`). This is the only place the definition is written out.
- `docs/agent-behavior-rules.md` R3 (`:19-22`): one sentence — "unresolved references are defined in
  ENGINE-CONTRACT §2.1; the coverage line counts them per site".
- `docs/getting-started.md:118-121`: replace the "captures that matched the symbol's name … whose callers could not
  be resolved" bullet with the §2.1 wording + link; `:208-209`: replace "the unresolved-call count should drop
  significantly" with the truthful statement (SCIP edges land; rows are pruned on the next re-index of the file).
- `docs/graph-first-retrieval.md:161-163`: keep the example, add the citation.
- `docs/benchmarks/README.md:33, :64` and `:60-66` ("The engine always surfaces unresolved refs…" — A8): cite §2.1.
- `docs/governance.md:38` ("unresolved references reported separately" — A8): cite §2.1.
- `docs/recon/otel-instrumentation-audit.md:222`: counter description → "refs unresolved per ENGINE-CONTRACT §2.1".
- `docs/benchmarks/capability-report.md`: regenerate via the bench harness after S1-S3 with the UA-6 corpora rule
  from D6 (same corpora if present locally, else studio + crew + a note under the table).
- `CHANGELOG.md` `[Unreleased]` → `Fixed`: the D6/UA-7 entry (definition citation, 49→0 example, UA-1
  version-bump/--force statement).
- `crates/wicked-estate/src/lib.rs` module doc `:17-24`: add one line that the Known Limitation is exception 1 of
  §2.1.
- Note (A8): `./wiki` named in CLAUDE.md does not exist in this checkout — no doc site to update there. The
  falsifier "grep for restated definitions returns only §2.1" is run verbatim and pasted in the PR body.

**Deletes:** the independent prose definitions (getting-started, graph-first, benchmarks README, governance,
capability.rs generator text) as definitions — they become citations.

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
plus Fixture A, B1 and B2 indexed through both binaries.

Expected AFTER (fresh full index): artifact query = **0** on studio and crew; Calls rows ≈ 32,219 (studio) /
13,380 (crew) (= before − artifact rows); `expect` = **4,932** / **2,607** unchanged; `blast-radius apiFetch`
unresolved **49 → 0** (`apiFetch` has 2 Calls edges in studio; all 49 rows are sites of those two relationships —
verified on the baseline: 0 apiFetch rows lack a same-source apiFetch edge); `blast-radius expect` count
unchanged; `stats` prints `unresolved=`; B1 → 1 Implements row for `B` (0 before); B2 → 1 row (0 before);
Fixture A → g 0 rows (2 before), h 1 row (1 before), Imports rows 2 before and 2 after in this lane (M3).
Telemetry counter: not observable in-process (`crates/wicked-estate-observe/src/lib.rs:322-331` OnceLock
NoopSink) — proven by T1-T9 through the shared function instead.

**Same-version re-index scenario (UA-1):** copy the HEAD-built studio DB; run the lane binary
`index $REPO --db $COPY` **without** `--force` (both binaries report the same `CARGO_PKG_VERSION`, so
`force_full` stays false, `lib.rs:556-568`, and `maybe_warn_version_mismatch` is silent, `main.rs:107-140`) —
record the artifact query (expected **unchanged, > 0**: 0 files re-extracted, old rows kept) and
`stats` `unresolved=`; then `index --force` → artifact query expected **0**. This is the measured proof of the
§4 statement and the CHANGELOG sentence.

**Collision-pass instrumentation (A4/F5):** the bucket-size histogram and the number of collision-pass resolver
calls are recorded in-process (a `#[cfg(any(test, debug_assertions))]` counter or debug log inside
`resolve_all_with_coverage`) on the Fixture B and studio runs — the persisted table stores only `line`, so DB
queries cannot establish span-level bucket sizes.

**Perf (F3 — like-for-like builds only):** the shipped debug/release BEFORE-vs-AFTER pairing is invalid for
timing. For the timing row: `git worktree add <lane>/measure/base-wt d7d3b58` (a nested worktree of the lane
repo), build **both** at release profile in **separate** target dirs (shared target dirs contaminate builds):
`CARGO_TARGET_DIR=<lane>/measure/target-base cargo build --release -p wicked-estate` in `base-wt` and
`CARGO_TARGET_DIR=<lane>/target cargo build --release -p wicked-estate` in the lane worktree; `time -p` a full
studio index with each; state the profile in the PR body. A >5% regression of the lane release binary over the
base release binary is a defect (§5 (6)). All non-timing measurements keep the brief's BEFORE/AFTER binaries.

### Commit plan (lane branch, `--no-verify`, trailers)

1. `docs(recon): unresolved-accounting plan` — this file (rev 2 amends it in place).
2. `fix(resolve): resolve_all_with_coverage — one unresolved definition, attributed per ref` — S1 (+ README).
3. `fix(index): persist unresolved from Resolution; drop the Location-keyed set` — S2 + S3 + S4.
4. `test(core): conformance — remove_file clears a file's unresolved rows` — S5.
5. `docs: define unresolved once (ENGINE-CONTRACT §2.1) and cite it` — S6 (incl. CHANGELOG) + regenerated
   capability report.

Commits 2 and 3 could be one; kept separate so the seam (S1) lands green before its consumer (§1).

---

## 4. Compatibility and migration

- **Schema:** none. `unresolved_refs` columns, indexes, Postgres DDL (`postgres.rs:195-204`), Surreal blob
  (`surreal.rs:188-194`) unchanged. `Edge` unchanged. `GraphStore` trait unchanged; the kit gains one assertion.
- **Stored graphs (UA-1, corrected in both directions):** the definition applies **per resolve pass** (D3).
  The engine already self-heals across releases: `index` compares the per-repo `indexed_version` meta key to
  `CARGO_PKG_VERSION` and **forces full re-extraction on any mismatch** (`crates/wicked-estate/src/lib.rs:556-568`,
  "VERSION CHANGE detected … forcing full re-extraction"; honoured at `:703`). Released consumers install a
  versioned binary and index without `--force` (crew `projects/graph.ts:592`), so their first `index` under the
  new version rewrites every row under the new definition. The **real hazard is a same-version re-index**: a dev
  or lane binary with an unchanged `CARGO_PKG_VERSION` re-extracts only changed files, leaving a DB that mixes
  rows written under the old location key (unchanged files) and the new definition (changed files) — silently,
  because `maybe_warn_version_mismatch` (`main.rs:107-140`) only fires on a version difference. Therefore
  (A6): **this change ships under a version bump** (repo version-sync script per release protocol), the
  CHANGELOG entry says "existing graphs are fully rebuilt on the next `index` under the new version;
  same-version binaries must `wicked-estate index <path> --force` (`main.rs:673, :765`)", ENGINE-CONTRACT §2.1
  states the same, and S7 measures the mixed-definition scenario explicitly.
- **Wire shapes:** `blast-radius --json` keys/types unchanged; text `coverage:` line unchanged; MCP `BlastRadius`
  content keys unchanged and `description()` untouched (it is embedded in the MCP goldens — UA-7;
  schema goldens compare `inputSchema` only, `crates/wicked-estate-mcp/tests/conformance_schemas.rs:221-228`);
  `stats` line gains `unresolved=N` (consumers tolerant, D6).
- **Numbers that move:** every `unresolved` count and coverage percentage drops toward the truth; the OTel counter
  rises. Bench receipts (`docs/benchmarks/capability-report.md`) are regenerated in the same PR (UA-6 corpora
  rule). Any doc example quoting a specific count is a stale example, not a contract.
- **Behavioural risk:** a resolver that emits a location-less edge, or an edge whose kind differs from the ref's,
  no longer counts as binding anything (today the telemetry key counted the former). All six implementations
  attach the ref's location and kind; the contract now lives on the trait (D7) and is pinned by T7/T9.
- **Site multiplicity (UA-5):** not persisted anywhere after this fix — consumers that saw "+49 unresolved
  call(s)" were reading an artifact, and no per-site signal replaces it (D2 records the carrier if one is ever
  needed). Stated in §2.1 and the blast-radius doc comment.
- **Determinism:** the surviving edge/location on a dedup tie is still first-seen (strict `>`); `bound` depends
  only on resolver output per ref, not on iteration order. Ids untouched.

---

## 5. Falsifier

The plan is wrong if, after S1-S4 on a **fresh full index** of wicked-studio with the lane binary, any of:
1. the artifact query in §3/S7 returns > 0;
2. `select count(*) from unresolved_refs where raw_name='expect'` ≠ 4,932 (or crew ≠ 2,607);
3. `blast-radius apiFetch` reports > 0 unresolved while every `unresolved_refs` row for `apiFetch` has a same-source
   `apiFetch` edge;
4. fixture B1 (`class C implements A, B`, `B` undefined) persists 0 rows for `B` — the collision pass failed
   end-to-end; or fixture B2 (`extends A implements B`) persists 0 rows for `B` — the kind-in-bucket key failed;
5. T4 still passes with the collision pass disabled (>1 buckets treated as binding all their refs) — the negative
   control of F6; would mean the pass is dead code and the shared-key premise wrong;
6. `time -p` full studio index with the lane **release** binary regresses by > 5% over the base-`d7d3b58`
   **release** binary built per S7 (like-for-like profile — F3);
7. the same-version re-index scenario (S7/UA-1) shows the no-`--force` run *changing* unchanged files' rows (the
   incremental seam leaked) or the `--force` run leaving the artifact query > 0.

---

## 6. Not in scope (owned elsewhere or deliberately deferred)

- **Repeat-site visibility** (`metadata.sites`, "called from 50 sites") — no consumer; D2 records the carrier if one
  appears.
- **`unresolved_refs_for_name` kind/scope filters** — it counts Imports rows as "call(s)" and sums across repos in
  a co-located graph (`sqlite.rs:2302-2312`); pre-existing consumer-side behaviour, documented in §2.1, not changed.
- **`ingest_scip_as` pruning `unresolved_refs`** (`lib.rs:1179-1194`) — deferred; the feasible Calls-only prune is
  named in D3 exception 2 as the follow-up (UA-4).
- **Unchanged-importer re-resolution** (`lib.rs:17-24`) — exception 1.
- **`ingest_tfstate`** (`lib.rs:1236-1243`) persists collector refs unresolved without a resolve pass — by design.
- **Telemetry `language` attribute** (`resolve/src/lib.rs:918` emits `"unknown"`) — unchanged.
- **Extends/Implements/event/rules-engine refs sharing an anchor span or `Span::ZERO`**
  (`treesitter.rs:1964-1973, :2005`; `extra_edge.rs:360-366`) — handled exactly by the collision pass; giving
  each ref its own span is the extraction-gaps / defect-#4 lanes' call (M4, M5).
- **NameResolver kind-blindness / wrong bindings** (defect #2): a ref bound to a wrong node is "resolved" under this
  definition — the accounting is faithful to the resolvers; precision is the resolver-precision lane. One
  side-effect worth their attention is in M2.
- **`measure_synth_precision`'s `(source, kind)` first-match** (`resolve/src/lib.rs:742-747, :781-790`) — same defect
  class; flagged (M2), not edited here.
- Postgres/Surreal conformance execution (feature + env gated).

---

## 7. Merge notes for the other lanes

- **M1 (all lanes touching `crates/wicked-estate/src/lib.rs`):** this lane edits `:929` (the `resolve_all` call → `resolve_all_with_coverage`) and deletes `:937-944`. The slice literal `:923-928` is untouched. If a lane changes the slice, the call on `:929` must read `let resolution = resolve_all_with_coverage(resolvers, &all_refs, &index)?;`.
- **M2 (resolver-precision):** (i) `measure_synth_precision` correlates edges to refs by `(source, kind)` first-match (`resolve/src/lib.rs:781-790`) — the same key the telemetry counter had. After S1, `Resolution` gives an exact per-ref answer; the synth-precision harness should use it. (ii) `resolve_all` is now a wrapper; new tests that need the unresolved set call `resolve_all_with_coverage`. **Wrapper removal is the tracked remainder (A5): in the post-lane integration merge of this review program (before the release that ships the fix), rename all 9 call sites and delete the wrapper** — each site is a one-line change; it is kept during the lanes only to avoid conflicts in the test module both lanes edit. (iii) Side-effect of the fix for your lane: today's location-keyed over-count accidentally surfaced sites 2..N of a *wrongly bound* relationship (defect #2) as "unresolved"; after this fix that accidental signal disappears — a wrong binding is fully invisible in `unresolved_refs`. (iv) When receiver inference lands, per-ref accounting (T8) already supports binding one receiver's site without cancelling the other's — no accounting change needed on your side, but every edge you emit must carry the ref's `(location, kind)` (D7). This lane appends its tests at the **end** of `mod tests` to minimise conflicts, and also edits `crates/wicked-estate-resolve/README.md` and `crates/wicked-estate-core/src/traits.rs` doc comments (F8) — flag if your lane touches either.
- **M3 (relative-imports):** the brief's fixture expectation "1 Imports edge, 0 unresolved rows for the second `import './mod'`" is unreachable at HEAD — no shipped resolver binds quoted relative specifiers; the only Imports edge is the extractor-local File→Import-node edge (`resolved_by=tree-sitter`), not derived from the ref. This lane proves the mechanism with a mock resolver (T5) and writes the e2e stats assertion so it holds for any Imports count. After the relative-import resolver lands, add `assert!(store.unresolved_refs_for_name("'./mod'").is_empty())` to the S4 e2e test. Your resolver must attach the ref's exact location **and kind** to every binding edge (D7; the review patch does, 1 hit) or its refs will stay unresolved under §2.1.
- **M4 (method-identity / extraction-gaps):** heritage and event-emit refs reuse the declaring node's span (`treesitter.rs:1964-1973, :2005`); multi-target clauses additionally share the kind (`typescript.scm:54-59, :67-71`). The collision pass makes accounting exact regardless; if you give each heritage target its own span, the collision pass simply stops firing for them (T4 and fixture B1 must still pass — T4 constructs shared-key refs directly, B1 will then be attributed without a re-run). A resolve-crate test may additionally assert that the extractor emits the two `implements A, B` refs at one Location, guarding this note's premise (F2, optional).
- **M5 (defect-#4 lane — wiring `RulesBridgeResolver`):** every rules-engine ref in a file is emitted at `(file, Span::ZERO, InvokedBy)` (`extra_edge.rs:360-366`), so all k schemes of one file share one attribution bucket, and each collided ref re-runs a `resolve()` that scans `all_nodes()` (`resolve/src/lib.rs:578-586`) — k single-ref re-runs per file. Acceptable only because k = schemes per file is small; **measure it when you wire the resolver**, and the durable fix is distinct spans per rules-engine ref.
- **All lanes:** any doc or test that quotes a specific unresolved count (e.g. "49") is stale after this lane; cite `docs/ENGINE-CONTRACT.md §2.1` rather than restating the definition.
