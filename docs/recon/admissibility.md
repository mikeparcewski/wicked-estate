# Recon + plan: admissibility residuals (closure findings 2 & 3)

Lane: `fix/admissibility-residuals` (base `764622f`, descendant of `d7d3b58`).
Scope: two residuals from the 2026-08-28 closure suite
(`estate-review/review-artifacts/closure-suite.json`, `.closure[0].findings[1-2]`,
`.closure[0].not_done[1]`). Written per §10 after three-lens recon (history,
consumers, tests/risks); every citation below was opened in this worktree at `764622f`.

Revision 2 (post-attack): resolves ADM-ATT-1/ADM-ATT-2/ATT-1/ATT-2/ATK-1 (major) and
the minors (ADM-ATT-3/4/5, ATT-3/4, ATK-2/3/4). Headline changes: measurement scope
widened from Calls-only to the FULL `(source, target, kind)` edge set with stratified
adjudication; the agent-eval benchmark is RUN before/after, not predicted; the k8s
end-to-end fixture is replaced by unit pins (InfraResolver's exclusive-resource-name
bind makes the e2e form infeasible — verified, see D-3); the matrix generator script
joins S2 and its json special-case is retired; S4 uses a new fixture instead of
mutating `fixture_a`; red tests land in the same commit as their fix.

---

## 1. Findings acted on

### F-A — 402 Calls edges bind TS/tsx/bash call sites to JSON `struct` nodes (minor)

- Reproduced on `/Users/michael.parcewski/Projects/command_iq`: 374 typescript + 21 tsx
  + 7 bash Calls edges → json nodes of kind `"struct"`, all `resolved_by=name-resolver`
  at 0.6; 395 target the JSON key `optional` in `scripts/ci/authz-public-allowlist.json`
  (every zod `.optional()` site), 7 target `pnpm`. Also reproduced on a 2-file fixture
  (`measure/fixjson/repo`: `schema.ts` + `config.json` with colliding keys).
- Mechanism (all verified in-tree):
  - `json.scm` mints top-level JSON keys as `Struct` nodes and emits **zero call refs**
    (`crates/wicked-estate-extract/src/queries/json.scm` — whole file), so no legitimate
    Calls edge can target a json node.
  - `NodeKind::Struct` deliberately passes the D1 Calls deny-list (`new X()` sites,
    `crates/wicked-estate-resolve/src/lib.rs:121-146`).
  - `json` has **no `languages.toml` row** (it is wired only in the built-in
    `LangEntry` table, `crates/wicked-estate-extract/src/treesitter.rs:601-606`), so
    `language_family("json")` is `None` and `family_compatible`
    (`resolve/src/lib.rs:157-166`) allows: it blocks only `(Some, Some)` with differing
    families.
  - Only `NameResolver` can mint the edge: `ScopedNameResolver` and `ImportMapResolver`
    additionally retain `is_callable` for Calls (`resolve/src/lib.rs:230-235, 451-453`),
    which excludes Struct.
- This is a **deviation from the seam's recorded intent**, not a decided behavior:
  D5 in `docs/recon/resolver-precision.md` claims "typescript→json are blocked" on the
  theory that every manifest language has an own-name family — true for every manifest
  row (`extract/src/lib.rs:167-173`), false for json because the row is missing. The
  sibling residual for resource nodes is already documented at
  `resolve/src/lib.rs:539-545` ("resource nodes carry non-manifest language tags, so
  the guard allows them") — `kubernetes`/`cloudformation` are logical languages
  registered outside the manifest (`treesitter.rs:2513-2545`), kind `Other("resource")`.

### F-B — `unresolved_refs` stores `start_line` only; exact-site duplicate proofs need on-disk adjudication (info)

- SQLite persists `(from_sym, raw_name, kind, file, line)` and reconstructs a zeroed
  span on read (`store/src/schema.sql:87-105`, `store/src/sqlite.rs:1620-1645`,
  read ~:2302-2340); Postgres mirrors the same five columns
  (`store/src/postgres.rs:195-204, 754-770, 1377+`). MemStore keeps the full struct
  (`store/src/lib.rs:370-372`) and SurrealStore serializes the whole ref
  (`surreal.rs:188-196`) — spans already round-trip there.
- `UnresolvedRef` already carries the full `Span` in memory
  (`core/src/refs.rs`, `core/src/node.rs`); the write sites are
  `wicked-estate/src/lib.rs:1042` (resolve pass, per-ref since PR #125) and `:1355`
  (tfstate ingest). No change needed above the store layer.
- Closure evidence: 1215 line-level "duplicate" groups on command_iq were all real
  distinct same-line sites; the 0-artifact claim required 44 manual on-disk
  adjudications. A span column makes it a pure SQL proof.

---

## 2. Decisions

### D-1: The rule is neither of the brief's two options — fix the DATA, keep the D5 guard unchanged

**Decision:** zero seam-code change. Close the family gap with data:
a `json` manifest row + own-name family registration for the IaC logical languages.

- **Rule B ("family-None targets inadmissible for Calls") — REJECTED.** Directly
  falsified in-repo: cobol/jcl/hlasm have no manifest rows (family None on **both**
  ends) and their Calls joins are pinned by
  `crates/wicked-estate/tests/cross_language_estate.rs` and by
  `family_guard_allows_unknown_family_jcl_to_cobol` (resolve unit test); the
  `family_compatible` doc (`resolve/src/lib.rs:153-156`) records that a strict guard
  "would kill the shipped JCL/HLASM→COBOL joins". It would also block Calls→Synthetic
  and every plugin-grammar target (plugin languages are manifest-absent,
  `treesitter.rs:1272-1274`) — a silent recall cliff. Do not re-propose.
- **Rule A ("tier=document never a Calls target") — REJECTED as specced.** json has no
  manifest row, so it has no tier to key on; `toml` is `tier="structural"` yet mints
  Calls-admissible Variable nodes; `ExtractTier` has zero behavioral consumers today;
  and the resolve crate has no tier transport (would need a new `SymbolIndex` method on
  the spine plus a 16-row tier audit). Materially larger change for the same 402 edges.
- **Asymmetric guard (source `Some` → target `None` blocked) — REJECTED.** Kills
  Calls→Synthetic and code→plugin-language binds; unmeasured recall cliff; not needed
  for the finding.
- **Chosen fix restores D5's stated intent as written:** with a json row, every
  measured source (typescript/tsx/bash — all manifest, all own-family) is blocked from
  json targets by the *existing* guard. "Rules as DATA, not code" (Universal Don'ts).

### D-2: json gets a `languages.toml` row

`name = "json"`, `ext = ["json"]`, `grammar = "tree-sitter-json"`, `tier = "document"`,
`caps = ["symbols"]`, no `family` line (own-name family per `extract/src/lib.rs:171-173`).

- Extraction routing is untouched: `extractor_for_extension` reads the built-in
  `LANG_TABLE`, not the manifest (`treesitter.rs:1261-1274`); manifest `by_extension`
  has no production file-routing caller. The existing `arm` row (`ext = ["arm.json"]`,
  same grammar, `languages.toml:723-729`) is unaffected.
- Manifest tests hold: `covers_language_parity` is a `>= 73` floor —
  `registry().len()` goes **113 → 114** (113 `[[language]]` rows counted at `764622f`;
  the earlier "78" was wrong — ADM-ATT-5/ATK-2);
  `manifest_is_well_formed` requires only a `tree-sitter-*`/`arborium-*` grammar
  (satisfied); `js_family_languages_share_one_family` untouched;
  `json_characterization` (extract tests) pins extraction behavior and must not change.
- `tier = "document"` is kept deliberately (ADM-ATT-4 considered): yaml and html —
  both of which mint symbol nodes from `.scm` queries — are the established
  config/markup precedent (`languages.toml:543-547`, `:236-240`, both
  `tier = "document"`, `caps = ["symbols"]`), and the generated matrix's own tier key
  reads "`document` — Config/markup — symbols only (YAML, JSON, HTML, HCL, …)". The
  outlier is the `ExtractTier::Document` docstring ("no code symbols",
  `extract/src/lib.rs:116-117`) — it is amended in S2 to match shipped behavior
  ("symbols only, no call/import refs"), so the shipped label and the shipped behavior
  agree. `ExtractTier` has zero behavioral consumers (grep verified), so this is doc
  honesty only.
- D13 hazard (manifest regeneration dropping hand-added rows): pin the row with a unit
  test in the extract tests mod, same pattern as the js-family guard.
- **Matrix generator (ATT-2):** `scripts/gen-coverage-matrix.py` special-cases json
  precisely because "manifest row not yet added" (`:229`, hardcoded rows at `:261-262`
  and in the wired-summary loop). Verified mechanism: `iac_only` is computed as
  wired-names − manifest-names, so adding the row makes json drop OUT of `iac_only`
  and the special-cases go **dead** (no double-listing occurs — the attack's
  double-list claim is wrong on mechanism, its fix is right on §8 grounds). The dead
  special-cases are deleted in the same change (retire-as-you-go: the special case IS
  what the manifest row replaces), the doc regenerated, and S2 verifies **exactly one
  json row per table** and no k8s/cfn double-listing. Manifest tier/caps match the
  script's hardcoded values (`document`, S) so the json row content is unchanged;
  the "Wired but missing from languages.toml" section disappears (it existed only for
  json) — an intended doc improvement, recorded here so the diff reads as designed.

### D-3: kubernetes/cloudformation get own-name families via a logical-language list, NOT manifest rows

The brief names kubernetes explicitly and §11 requires propagating the class fix.
But manifest rows are wrong for them: they have no extensions (routed by content
sniff, `wicked-estate/src/lib.rs:194-201`) and `scripts/gen-coverage-matrix.py`
already special-cases them — rows would double-list them in the generated matrix.

**Decision:** `wicked-estate-extract` exposes a single-source
`pub const LOGICAL_LANGUAGES: &[&str] = &["cloudformation", "kubernetes"]` next to
`IaCExtractor` (`treesitter.rs:2513-2545`), and **`IaCExtractor::for_language` is
rewired to match against that const** instead of its own string literals
(`treesitter.rs:2527-2531`) — one list, two consumers (ADM-ATT-3: a parallel
hand-copied list would drift exactly the way the missing json row did). A pin test
asserts `for_language(name).is_some()` for exactly the const's members and `None`
otherwise. The `InMemoryIndex` families map (`wicked-estate/src/lib.rs:287-293`,
built once per resolve pass from `registry()`) is extended with these names →
own-name family. No per-candidate `registry()` call (it re-parses the embedded TOML
every call — would be a real perf regression).

- **Scope of the claim, corrected (ATK-1):** this blocks the **name-resolver path**
  only. `InfraResolver` is in the production slice (`wicked-estate/src/lib.rs:1025`)
  and binds a code Calls ref to a resource node at Parsed/1.0 **without any family
  check** whenever the raw name resolves exclusively to resource nodes (the
  `!from_is_resource && has_any_code_candidate` guard skips only the MIXED case — the
  exclusive case is a deliberate CFN-`!Ref` carve-out, `resolve/src/lib.rs:596-625`).
  A ts call site whose undefined callee name collides with a k8s `metadata.name` is
  by construction an exclusive-resource name, so it binds via InfraResolver at 1.0
  regardless of families — a separate, pre-existing behavior, now documented as the
  kubernetes residual in §6, NOT closed by this lane. What D-3 removes is the
  NameResolver 0.6 bind (which, where InfraResolver also fires, was already deduped
  away by the 1.0 edge). Consequence: **no end-to-end fixture can observe the k8s
  family registration** — the pins are unit-level (S1/S2).
- **Source-side flip (ATT-1b), measured not assumed:** registering families for
  k8s/cfn also flips the SOURCE side — refs FROM IaC nodes to other-family targets go
  `(None, Some)`→allow to `(Some, Some-differing)`→deny on the name-resolver paths.
  cfn extraction mints `EdgeKind::Calls` refs from cloudformation nodes (`!Ref` sites,
  `treesitter.rs:2761-2772`); the legitimate exclusive-resource binds ride
  InfraResolver (no family guard — survive), but any NameResolver-bound
  IaC-source→code edge is removed. These removals land in the full-kind edge diff
  (S0/S5) and their stratum is adjudicated explicitly; a legitimate one firing is a
  falsifier (§5.5).
- IaC extraction mints `Contains`/`Evaluates`/`Other("depends_on")`, never Calls
  targets; same-family k8s→k8s allowance is pinned at the unit level
  (`family_compatible(Some("kubernetes"), Some("kubernetes"))` allows — S2).
- **tfstate stays family-None deliberately** — D5/F7 lists it among "must keep
  resolving"; drift joins may depend on it. Same for cobol/rpg (manifest-absent by
  design; adding rows would change unmeasured mainframe behavior). Not in scope.

### D-4: span columns are `start_byte` + `end_byte`, `INTEGER NOT NULL DEFAULT 0`

- Bytes alone are a complete exact-site discriminator (two `.sort()` on one line have
  distinct byte offsets); cols/end_line add width with no consumer — W11-slim spirit
  says minimal.
- `DEFAULT 0`, not NULL: matches the existing `Span::ZERO` sentinel that synthetic refs
  (RulesBridge/extra_edge, `extra_edge.rs:360-366`) legitimately carry forever, keeps
  readers non-`Option`, and backfills pre-existing rows without a data rewrite.
- The conformance assertion pins **exactly the persisted subset**
  (`start_line`, `start_byte`, `end_byte`) with a non-zero span, so MemStore/Surreal
  (full fidelity) and SQLite/Postgres (typed columns) pass for the same reason.

### D-5: migration is explicit ALTERs on BOTH backends; the gates are NOT relied on

The brief's "old DBs re-extract anyway under the id_scheme gate" is false in both parts:
- `CREATE TABLE IF NOT EXISTS` never reshapes an existing table (documented,
  `sqlite.rs:52-66`); without ALTERs the widened INSERT hard-fails on every existing DB.
- The id_scheme gate is already satisfied at scheme 2 (stamped by `764622f` itself),
  and the version gate fires only at the next `CARGO_PKG_VERSION` change — version
  files are forbidden to this lane.

**Decision:** SQLite: two PRAGMA-presence-guarded `ALTER TABLE unresolved_refs ADD
COLUMN` in `migrate_schema` (existing pattern, `sqlite.rs:52-160`, runs at every open;
table-absent guard included) + updated DDL in `schema.sql`. Postgres:
`ALTER TABLE unresolved_refs ADD COLUMN IF NOT EXISTS` ×2 in the bootstrap DDL
(existing idiom, `postgres.rs:160-165`) + updated CREATE. Old rows carry span 0 until
their file is re-persisted or a `--force`/version-bump re-extract — stated honestly
in the PR (§7), not hidden.

### D-6: BEFORE baseline = a build of clean `764622f`, not the designated release binary

The protocol's BEFORE binary (`wicked-estate/target/release/wicked-estate`, mtime
Aug 28 17:51) predates the admissibility seam (`0e5f4ca`, committed Aug 29): its
command_iq output still contains 62 Calls→Import edges and 241 python→ts/js binds that
are impossible at HEAD. Diffing against it would claim PR #126's wins as this lane's.
**Deviation recorded:** measure BEFORE with a binary built from this worktree at
`764622f` prior to any change; keep the release-binary DBs only as review-parity
context. Corpora: command_iq for the 402 class (studio/crew are vacuous — 0 instances
verified — and serve as the no-loss controls).

### D-7: duplicate-site SQL proof is span-zero-aware

Synthetic refs and pre-migration rows carry `(0,0)` legitimately. The mechanical proof
excludes them:

```sql
SELECT file, line, raw_name, kind, start_byte, end_byte, COUNT(*) c
FROM unresolved_refs
WHERE NOT (start_byte = 0 AND end_byte = 0)
GROUP BY 1,2,3,4,5,6 HAVING c > 1;   -- expect 0 rows on a fresh index
```

### D-8: Postgres verification is attempted, and its absence is stated, never papered over

`postgres_conformance`/`team_runtime` run 0 tests without `TEST_POSTGRES_URL` +
`--features postgres`. The implement step checks for a reachable Postgres (docker);
if none, the PR states "PG span path compiles (`--features postgres` build) but the
live conformance run is unverified on this machine — command:
`TEST_POSTGRES_URL=… cargo test -p wicked-estate-store --features postgres`" per §7.
Surreal conformance (`--features surrealdb`, kv-mem, no service) IS run locally since
the kit changes.

### D-9: purge of stale scheme-2 DBs is parked

DBs indexed between `764622f` and this fix keep the 402 edges and zero spans until the
queued 0.15.0 version bump fires the force-full gate (`wicked-estate/src/lib.rs:590-602`)
or a `--force` re-index. Version files/CHANGELOG are MUST-NOT-TOUCH for this lane; the
bump is the release owner's dependent not-yet-done and is named in the PR body.

### D-10: measurement scope is the FULL `(source, target, kind)` edge set, all kinds — Calls-only diffs are insufficient (ADM-ATT-1, ATT-1)

`family_compatible` runs for EVERY ref kind in `NameResolver`
(`resolve/src/lib.rs:64`) and `ScopedNameResolver` (`:250`) — only `ImportMapResolver`
gates on Calls (`:434`) — and non-Calls refs deny only `NodeKind::Import` targets
(`:123-125`), so a `Struct` json key is admissible to a non-Calls ref today. The fix
therefore also removes ts/tsx/bash→json edges of **non-Calls kinds** (e.g. a
NameResolver-bound Imports ref whose raw name uniquely matches a json key — legal TS
under `resolveJsonModule`), and D-3's source-side flip removes IaC-source→code binds.
A Calls-only diff is blind to both.

**Decision:**
- S0 and S5 dump the **full** edge set per corpus:
  `(source_language, target_language, kind, source_symbol, target_symbol)` — every
  kind, not just Calls.
- The removed set AND the added set are stratified by
  `(source_language, target_language, kind)`; **every stratum** is represented in the
  adjudication sample (20 samples minimum, at least one per stratum; a stratum with
  more distinct target names gets proportionally more).
- **ts→json Imports name-binds, policy decided up front:** if adjudication finds
  removed ts→json Imports binds, the loss is recorded explicitly and **accepted** —
  the all-kinds scope of the family guard for json is consistent with existing D5
  behavior for bash/toml (a ts→toml Imports name-bind is already blocked today), and
  the evidence-based path for json imports — `RelativeImportResolver`'s exact-path
  File→File bind at 0.9 (PR #127) — carries **no family guard** (zero
  `family_compatible` hits in `relative_import.rs`, verified) and survives untouched.
  S1's fixture pins that survival explicitly. What is lost is only the 0.6
  name-coincidence bind, which matches ANY same-named key in ANY json file in the
  repo — not evidence.
- Same-family JS control (studio/crew) stays, now over all kinds, not just Calls.

### D-11: the agent-eval benchmark is RUN, before and after — never predicted (ADM-ATT-2)

The previous revision predicted "bench coverage% falls" in prose. That violates §9
("the agent-eval benchmark must not regress" is a gate judged on a measurement) and
"the verdict is the verdict". `wicked-estate-bench` is runnable
(`crates/wicked-estate-bench/src/main.rs:17`, takes repo paths;
`blast_radius_coverage_pct` at `src/capability.rs` uses unresolved counts in the
denominator — this change moves edges down and unresolved up, both directions).

**Decision:** S0 runs
`cargo run -p wicked-estate-bench -- <command_iq> <studio> <crew>` from clean
`764622f`; S5 repeats it verbatim from the changed tree (same corpora, same lane
`CARGO_TARGET_DIR`). The receipt is the per-metric, per-repo delta
(`edge_count`, `unresolved_ref_count`, `blast_radius_coverage_pct`, `index_ms`), and
if a metric moves, the receipt ties the movement to the removed noise-edge strata
(denominator honesty) — the PR carries the **measured** numbers. If nothing moves
(plausible: the bench's top-symbol probe may not touch a json-collision name), that
is the cheap proof the §9 gate holds. Whether the corpus contains collision instances
is thereby measured, not asserted.

---

## 3. Steps

All commands run with
`CARGO_TARGET_DIR=/private/tmp/claude-501/.../ws/admissibility/target`, per-crate only.

**Commit granularity (ATT-3/ATK-4):** red tests never land alone. S1(a) is committed
WITH S2 and S1(b) WITH S3 — one commit each, red test + fix together, so every commit
on the branch is green per §9. The red-first evidence (the test failing at the
pre-fix tree) is captured as recorded command output in the PR body, not as a red
commit in history.

### S0 — BEFORE receipts (no code change yet)
- **Files:** none (measure/ DBs + a saved copy of the base binary under `measure/bin/`).
- **Change:** `cargo build -p wicked-estate` at clean `764622f`; copy the binary; index
  command_iq, studio, crew into `measure/{before-*}.db`; dump per-corpus: the 402-class
  count (`edges e JOIN nodes tn ON tn.symbol=e.target WHERE e.kind='"calls"' AND
  tn.language IN ('json','kubernetes','cloudformation')` — kinds are JSON strings),
  **the FULL edge set per D-10** (`source_language, target_language, kind,
  source_symbol, target_symbol` — every kind, joined via `nodes` on both ends), and
  the same-family (javascript) all-kinds edge set for studio/crew;
  `/usr/bin/time -p` the command_iq index. **Benchmark BEFORE run per D-11:**
  `cargo run -p wicked-estate-bench -- <command_iq> <studio> <crew>` from clean
  `764622f`, JSON output saved under `measure/bench-before.json`.
- **Tests:** n/a (measurement). Proof = recorded commands + counts.
- **Deletes:** nothing.

### S1 — failing tests first (committed with their fixes — see commit granularity)
- **Files:** `crates/wicked-estate/tests/admissibility_data_targets.rs` (new),
  `crates/wicked-estate-core/src/conformance.rs` (unresolved block, additive append).
- **Change:** (a) integration test, `resolver_precision_index.rs` as the model: inline
  fixture `schema.ts` (`.optional()`/`.parse()` call sites) + `config.json` (colliding
  keys) → assert **0** Calls edges whose target node language is `json`, the
  same-family TS call still resolves, AND (ADM-ATT-1) a ts relative import of
  `./config.json` still yields its File→File edge (`RelativeImportResolver` has no
  family guard — verified, zero `family_compatible` hits in `relative_import.rs`).
  **No k8s end-to-end fixture** (ATK-1: infeasible — InfraResolver binds
  exclusive-resource names at 1.0 with no family check, so the 0-edges assertion is
  red before AND after; the NameResolver edge it would observe is deduped away by the
  1.0 bind). The k8s leg is pinned at the unit level instead (S2): (i)
  `family_compatible(Some("javascript"), kubernetes-node-family)` denies, (ii)
  `family_compatible(Some("kubernetes"), Some("kubernetes"))` allows (same-family k8s
  refs keep resolving), (iii) `InMemoryIndex` families map contains
  kubernetes/cloudformation → own-name family.
  (b) conformance kit: upsert an `UnresolvedRef` with a non-zero span, read back via
  `unresolved_refs_for_name`, assert `start_line`/`start_byte`/`end_byte` round-trip.
- **Tests:** (a) and (b) are verified red at the pre-fix tree (output recorded for the
  PR body), then land WITH S2 and S3 respectively. Kit change is enforced on
  MemStore/SQLite by `store/tests/conformance.rs`, on Surreal/PG by their
  feature-gated suites.
- **Deletes:** nothing (additive assertions; existing kit assertions untouched).

### S2 — F-A fix: family data (lands in one commit with S1(a))
- **Files:** `crates/wicked-estate-extract/languages.toml` (json row per D-2),
  `crates/wicked-estate-extract/src/lib.rs` (pin test: json row exists,
  `family()=="json"`, tier Document; `ExtractTier::Document` docstring at :116-117
  amended per D-2 — "symbols only", matching yaml/html shipped behavior),
  `crates/wicked-estate-extract/src/treesitter.rs` (`LOGICAL_LANGUAGES` const near
  `IaCExtractor`; `for_language` rewired to match the const per D-3/ADM-ATT-3; pin
  test: `for_language` is `Some` for exactly the const's members),
  `crates/wicked-estate/src/lib.rs` (families map extension at :287-293; comment at
  :265-269 updated; unit pin: families map carries kubernetes/cloudformation),
  `crates/wicked-estate-resolve/src/lib.rs` (doc comments in the lane-owned deny-list
  block: :539-545 residual claim narrowed per ATK-1 — "name-resolver path blocked;
  InfraResolver's exclusive-resource-name bind is a separate pre-existing behavior";
  :153-156 example list updated; unit pins: `family_compatible` denies
  javascript↔kubernetes, allows kubernetes↔kubernetes),
  `scripts/gen-coverage-matrix.py` (ATT-2: delete the dead json special-cases — the
  `manifest_only_extra` json row and the wired-summary `elif name == "json"` arm),
  `docs/language-coverage-matrix.md` (regenerated — verify **exactly one json row per
  table** and no k8s/cfn double-listing).
- **Change:** as D-2/D-3. No resolver logic edited (doc comments + unit tests only in
  the resolve crate).
- **Tests:** `cargo test -p wicked-estate-extract` (parity floor ≥73 —
  `registry().len()` 113 → 114, `manifest_is_well_formed`,
  `js_family_languages_share_one_family`, `json_characterization` unchanged,
  `for_language` pin), `cargo test -p wicked-estate-resolve` (all D1/D5 pins incl.
  `family_guard_allows_unknown_family_jcl_to_cobol` — the canary — plus the new k8s
  family unit pins), `cargo test -p wicked-estate` (cross_language_estate,
  resolver_precision_index, relative_imports, rules_bridge_index, multi_repo,
  id_scheme, e2e, S1(a) now green incl. the File→File json-import survival assert).
  `python3 scripts/gen-coverage-matrix.py --check` green. Clippy + fmt per crate.
- **Deletes (§8):** the stale doc-comment claims — `resolve/src/lib.rs:539-545`
  ("resource nodes … the guard allows them" — narrowed, not erased: the InfraResolver
  carve-out stays true and is now named) and the `wicked-estate/src/lib.rs:265-269`
  non-manifest-returns-None wording for json/k8s/cfn; the generator's json
  special-cases (dead once the manifest row exists); the `ExtractTier::Document`
  "no code symbols" falsehood. The migration's delete is the falsehood, not code:
  the fix is data.

### S3 — F-B fix: span columns (lands in one commit with S1(b))
- **Files:** `crates/wicked-estate-store/src/schema.sql` (DDL + W11 comment amended),
  `crates/wicked-estate-store/src/sqlite.rs` (migrate_schema ALTERs; upsert binds
  `span.start_byte`/`span.end_byte`; reader fills them; :1621-1625 comment amended;
  new legacy-table migration test copying the `sqlite_legacy_untyped_row_backfills_to_note`
  pattern at :4194; extend `sqlite_unresolved_refs_roundtrip_typed_columns` :3697),
  `crates/wicked-estate-store/src/postgres.rs` (DDL + `ADD COLUMN IF NOT EXISTS` ×2,
  upsert, reader).
- **Change:** as D-4/D-5. Edits stay strictly inside unresolved_refs DDL/upsert/read —
  `remove_file`/prune bodies (incr-integrity lane) untouched; the per-file DELETE is
  column-agnostic and needs no edit.
- **Tests:** `cargo test -p wicked-estate-store` (conformance now green on SQLite;
  legacy migration test; roundtrip extended;
  `sqlite_unresolved_refs_count_only_no_data_column` stays green — no blob returns);
  `cargo test -p wicked-estate-store --features surrealdb`; PG per D-8;
  `cargo test -p wicked-estate-core`. Clippy + fmt.
- **Deletes (§8):** the zero-fill of `start_byte`/`end_byte` in the SQLite/PG readers
  (replaced by column reads) and the "full span … intentionally NOT persisted" claims
  in `schema.sql:91-95` / `sqlite.rs:1621-1625` (amended to name the persisted subset
  and why bytes were added).

### S4 — duplicate-site proof as a pinned consumer
- **Files:** `crates/wicked-estate/tests/unresolved_accounting_cli.rs` (a **new**
  fixture + test — `fixture_a` is NOT edited).
- **Change:** ATK-3 — `fixture_a`'s shape is pinned ("h once (undefined)",
  `unresolved_accounting_cli.rs:15`, and "1 unresolved call(s) reference 'h'" at
  :76-80; the same Fixture A shape comes from the e2e suite); doubling `h()` breaks
  those pins. Instead: a new fixture with two same-line sites of a **fresh** undefined
  name (e.g. `q(); q();` in its own temp repo), index via the CLI, open the produced
  DB with `SqliteStore::open` + `unresolved_refs_for_name`, assert the two rows carry
  **distinct `start_byte`** — the within-line discriminator pinned end-to-end; record
  the D-7 SQL run against the S5 AFTER command_iq DB (expect 0 rows) in the PR
  evidence.
- **Tests:** `cargo test -p wicked-estate --test unresolved_accounting_cli` (new test
  green, `stats_and_blast_radius_report_per_reference_unresolved` untouched and green).
- **Deletes:** nothing (the deleted artifact is the manual adjudication protocol —
  closure `not_done[1]` closes).

### S5 — AFTER receipts + adjudication
- **Files:** none (measure/ DBs, PR body).
- **Change:** rebuild; fresh-index command_iq/studio/crew with the AFTER binary.
  Receipts: (1) 402-class query = **0** on command_iq; (2) **full `(source_language,
  target_language, kind, source, target)` edge-set diff vs S0 per D-10 — all kinds** —
  removed AND added sets stratified by `(source_language, target_language, kind)`,
  every stratum represented in the adjudication (≥20 samples; design prior for
  Calls→json: json.scm mints no callables ⇒ 0 legitimate losses; ts→json Imports
  losses recorded and adjudicated under the D-10 policy; IaC-source strata adjudicated
  explicitly per D-3); added-set adjudicated too (excluding json candidates can
  unshadow new binds — by design, pinned by
  `scoped_family_retain_unshadows_same_family_homonym`); (3) studio/crew same-family
  (javascript) **all-kinds** `(source,target,kind)` sets byte-identical to S0;
  (4) index wall-time within noise of S0; (5) D-7 SQL proof = 0 rows;
  (6) `.schema unresolved_refs` diff; (7) **benchmark AFTER run per D-11** — same
  command and corpora as S0, saved to `measure/bench-after.json`, per-metric per-repo
  delta table in the PR (movement tied to the removed strata, or "no movement" as the
  §9 proof).
- **Tests:** the queries themselves; every command recorded verbatim.
- **Deletes:** nothing.

### S6 — docs + contract
- **Files:** `docs/ENGINE-CONTRACT.md` (§2.1: the per-ref row now persists
  `start_byte`/`end_byte`, DEFAULT 0 = unknown/synthetic; §3.1 resolver table rows
  reread — resolver set unchanged, `slice_matches_engine_contract_table` must not
  move), `docs/benchmarks/capability-report.md` + `docs/benchmarks/multi-repo-validation.md`
  (ATT-4: annotate the pinned figures as "measured pre-<this-PR> (Calls→data-target
  class present); post-fix deltas in the PR for `measure/bench-after.json` corpora" —
  annotation, not regeneration: the docs pin a different corpus set than this lane's
  receipts and regenerating them is the release owner's receipt convention; the choice
  is named in the PR body).
- **Tests:** `cargo test -p wicked-estate` (slice test), doc grep for the amended
  claims.
- **Deletes:** the §2.1 sentence implying line-only site identity.

---

## 4. Compatibility + migration

- **Stored graphs:** additive columns with `DEFAULT 0`; both backends ALTER at open
  (SQLite `migrate_schema` on every open; PG bootstrap DDL at connect). Old DBs open,
  write, and read without error; pre-existing rows read back span-zero (the honest
  sentinel — same value synthetic refs carry by design). No re-index is forced by this
  lane (version bump forbidden); stale scheme-2 DBs keep the 402 edges until the
  0.15.0 bump or `--force` (D-9, named in the PR).
- **Consumers:** every reader of unresolved data consumes counts
  (`retrieve/src/lib.rs:827`, `main.rs:1358/1421`, bench `capability.rs:328`; crew
  `graph.ts:1006-1019` parses `{dependents, unresolved}` only; studio/garden read
  those surfaces) — no shape change. Number movement on command_iq-class corpora
  (Calls edges down, `unresolved_ref_count` up as those sites re-park, coverage%
  possibly down) is **measured, not predicted**: the D-11 before/after benchmark runs
  produce the per-metric deltas and the §9 gate is judged on those measured numbers
  in the PR. rank/pagerank cache and isolated/leaf listings shift only on affected
  repos after re-index.
- **MemStore/Surreal:** already full-fidelity; only gain the kit assertion.

## 5. Falsifier

The plan is wrong if any of these fires:
1. `cargo test -p wicked-estate --test cross_language_estate` or
   `family_guard_allows_unknown_family_jcl_to_cobol` fails after S2 — the fix
   over-blocks family-None participants (rule-B failure mode).
2. The S1(a) json fixture still finds a Calls→json edge after S2 — the data fix
   under-blocks (families map not reaching NameResolver). A red **k8s unit pin**
   means the families-map extension or the `family_compatible` behavior didn't land;
   note (ATK-1): an end-to-end Calls→kubernetes edge is NOT a falsifier — it is
   InfraResolver's pre-existing exclusive-resource-name bind (§6), which this lane
   does not touch.
3. The S1(b) conformance assertion fails on any store after S3 — span persistence is
   not real on that backend.
4. **Any** S0→S5 edge-set difference (all kinds, per D-10) whose target language is
   NOT in {json, kubernetes, cloudformation} AND whose source language is not
   kubernetes/cloudformation — the change leaked outside its declared blast radius.
   Studio/crew same-family all-kinds sets differing is the specialized instance.
5. The removed-edge adjudication finds a legitimate removed edge: a legitimate
   Calls→json bind (the json.scm "no callables" prior is wrong), OR a legitimate
   removed edge with an IaC-language source, OR a legitimate non-Calls removal
   outside the D-10 recorded-and-accepted ts→json Imports policy — the rule scope
   must be reconsidered, not silently absorbed.
6. The legacy-table migration test fails — old DBs would hard-fail at first index.
7. The D-11 benchmark moves in a direction or magnitude the removed strata cannot
   account for (e.g. coverage% falls on a corpus with zero removed edges) — the
   change has an unexplained side effect and §9 is not satisfied by the receipt.

## 6. Not in scope

- cobol/rpg/tfstate manifest rows or family changes (D-3; F7 "must keep resolving").
- **InfraResolver's exclusive-resource-name carve-out (ATK-1)** — a code Calls ref
  whose name resolves exclusively to resource nodes binds at Parsed/1.0 with no
  family check (`resolve/src/lib.rs:596-625`, the deliberate CFN-`!Ref` path). This
  is the remaining kubernetes residual: a ts call to an undefined name colliding with
  a k8s `metadata.name` still binds, via InfraResolver, before AND after this lane.
  Pre-existing, separate behavior; changing it would need its own recall analysis of
  the `!Ref` join. Documented in the resolve-crate doc comments (S2), not closed here.
- The Other(_) residual for **plugin-grammar** languages (unknown-family by nature) —
  narrowed, documented, not closed.
- A tier-based admissibility rule / `SymbolIndex::language_tier` transport (rejected
  as specced; would be a formal D14 generalization with its own ADR).
- The stale-edge purge gate / 0.15.0 version bump (D-9; version files forbidden).
- toml: **verified not a gap** — toml has a manifest row (own-name family), so ts/tsx/
  bash→toml is already blocked by D5; no change.
- `remove_file`/prune/import-node minting, `lsp.rs`, `plugin.rs` (other lanes / forbidden).

## 7. Merge notes (other lanes / integrator)

- **incr-integrity lane:** this lane edits `sqlite.rs` (upsert at :1620, directly above
  `remove_file` :1646), `postgres.rs` (unresolved DDL/upsert near the delete paths),
  `store/src/lib.rs` (:370 vs the remove path :412-419), and `conformance.rs`
  (unresolved block :299-338 vs the remove_file assertions :516-534). Function-disjoint
  but line-adjacent — this lane's edits stay strictly inside unresolved upsert/read/DDL
  and the kit's unresolved block (appended, not interleaved). Merge order indifferent;
  conflicts, if any, are context-line only.
- **Any concurrently-measuring lane:** the json fix moves ~402 refs from edges into
  unresolved_refs on command_iq-class corpora — unresolved counts RISE. Baseline on
  fresh DBs from the same base commit or deltas contaminate (same hazard
  resolver-precision's §8 merge note recorded).
- **Release owner:** the 0.15.0 bump is the purge vehicle for pre-fix scheme-2 DBs
  (D-9); until then shipped DBs may serve the 402 edges and zero spans.
- **Protocol deviation recorded (D-6):** the designated BEFORE release binary predates
  PR #126 and was not used for the residual's receipts; base-`764622f` builds were.

---

## 8. Attack round — disposition

Every major is resolved in-plan: ADM-ATT-1/ATT-1 → D-10 + S0/S5 + falsifiers 4-5 +
the S1 File→File survival assert; ADM-ATT-2 → D-11 + S0/S5 bench runs + falsifier 7;
ATT-2 → S2 file list + special-case deletion + single-row check; ATK-1 → D-3
rewording, S1 unit pins replacing the k8s e2e leg, §6 residual entry, falsifier 2
correction. Minors adopted: ADM-ATT-3 (single-source `LOGICAL_LANGUAGES` const,
`for_language` rewired, pin test), ADM-ATT-4 (tier=document kept, `ExtractTier`
docstring amended — see D-2), ADM-ATT-5/ATK-2 (113 → 114), ATT-3/ATK-4 (red tests
land with their fixes; red-first output in the PR body), ATT-4 (S6 bench-doc
annotation), ATK-3 (new S4 fixture, `fixture_a` untouched).

**Partially rejected (mechanism only): ATT-2's double-listing claim.** Verified in
`scripts/gen-coverage-matrix.py`: `iac_only` is computed as wired-names minus
manifest-names (the `iac_only = [n for n in wired_names if n not in {lang["name"] …}]`
comprehension), so once json has a manifest row it drops out of `iac_only` and both
hardcoded json rows become **dead code, not duplicates** — the regenerated doc would
carry exactly one json row even without touching the script. The prescribed fix is
adopted anyway: §8 retire-as-you-go says the dead special-case (whose own comment
says "manifest row not yet added") is deleted in the same change that adds the row,
and S2's check asserts exactly one json row per table so the claim is proven, not
argued.
