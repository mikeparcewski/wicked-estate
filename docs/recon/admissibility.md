# Recon + plan: admissibility residuals (closure findings 2 & 3)

Lane: `fix/admissibility-residuals` (base `764622f`, descendant of `d7d3b58`).
Scope: two residuals from the 2026-08-28 closure suite
(`estate-review/review-artifacts/closure-suite.json`, `.closure[0].findings[1-2]`,
`.closure[0].not_done[1]`). Written per §10 after three-lens recon (history,
consumers, tests/risks); every citation below was opened in this worktree at `764622f`.

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
- Manifest tests hold: `covers_language_parity` is a `>= 73` floor;
  `manifest_is_well_formed` requires only a `tree-sitter-*`/`arborium-*` grammar
  (satisfied); `js_family_languages_share_one_family` untouched;
  `json_characterization` (extract tests) pins extraction behavior and must not change.
- D13 hazard (manifest regeneration dropping hand-added rows): pin the row with a unit
  test in the extract tests mod, same pattern as the js-family guard.

### D-3: kubernetes/cloudformation get own-name families via a logical-language list, NOT manifest rows

The brief names kubernetes explicitly and §11 requires propagating the class fix.
But manifest rows are wrong for them: they have no extensions (routed by content
sniff, `wicked-estate/src/lib.rs:194-201`) and `scripts/gen-coverage-matrix.py`
already special-cases them — rows would double-list them in the generated matrix.

**Decision:** `wicked-estate-extract` exposes
`pub fn logical_languages() -> &'static [&'static str]` (`["kubernetes",
"cloudformation"]`, a const next to `IaCExtractor` where the names are defined,
`treesitter.rs:2513-2545`); the `InMemoryIndex` families map
(`wicked-estate/src/lib.rs:287-293`, built once per resolve pass from `registry()`)
is extended with these names → own-name family. No per-candidate `registry()` call
(it re-parses the embedded TOML every call — would be a real perf regression).

- Safe: `InfraResolver` binds resource refs at 1.0 and does not use the family guard
  (no `admissible_target`/`family_compatible` call sites outside the three name
  resolvers); IaC extraction mints `Contains`/`Evaluates`/`Other("depends_on")`, never
  Calls targets; same-family k8s→k8s refs still allow.
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

---

## 3. Steps

All commands run with
`CARGO_TARGET_DIR=/private/tmp/claude-501/.../ws/admissibility/target`, per-crate only.

### S0 — BEFORE receipts (no code change yet)
- **Files:** none (measure/ DBs + a saved copy of the base binary under `measure/bin/`).
- **Change:** `cargo build -p wicked-estate` at clean `764622f`; copy the binary; index
  command_iq, studio, crew into `measure/{before-*}.db`; dump per-corpus: the 402-class
  count (`edges e JOIN nodes tn ON tn.symbol=e.target WHERE e.kind='"calls"' AND
  tn.language IN ('json','kubernetes','cloudformation')` — kinds are JSON strings),
  the full Calls `(source,target)` set, and same-family (javascript) Calls set for
  studio/crew; `/usr/bin/time -p` the command_iq index.
- **Tests:** n/a (measurement). Proof = recorded commands + counts.
- **Deletes:** nothing.

### S1 — failing tests first
- **Files:** `crates/wicked-estate/tests/admissibility_data_targets.rs` (new),
  `crates/wicked-estate-core/src/conformance.rs` (unresolved block, additive append).
- **Change:** (a) integration test, `resolver_precision_index.rs` as the model: inline
  fixture `schema.ts` (`.optional()`/`.parse()` call sites) + `config.json` (colliding
  keys) → assert **0** Calls edges whose target node language is `json` AND the
  same-family TS call still resolves; second fixture: k8s manifest (`kind:` +
  `metadata.name` named like a TS call) → **0** Calls edges to language `kubernetes`.
  (b) conformance kit: upsert an `UnresolvedRef` with a non-zero span, read back via
  `unresolved_refs_for_name`, assert `start_line`/`start_byte`/`end_byte` round-trip.
- **Tests:** both fail at `764622f` (that is the point); (a) goes green after S2,
  (b) after S3. Kit change is enforced on MemStore/SQLite by
  `store/tests/conformance.rs`, on Surreal/PG by their feature-gated suites.
- **Deletes:** nothing (additive assertions; existing kit assertions untouched).

### S2 — F-A fix: family data
- **Files:** `crates/wicked-estate-extract/languages.toml` (json row per D-2),
  `crates/wicked-estate-extract/src/lib.rs` (pin test: json row exists,
  `family()=="json"`, tier Document), `crates/wicked-estate-extract/src/treesitter.rs`
  (`logical_languages()` const+fn near `IaCExtractor`),
  `crates/wicked-estate/src/lib.rs` (families map extension at :287-293; comment at
  :265-269 updated), `crates/wicked-estate-resolve/src/lib.rs` (doc comments only:
  :539-545 residual claim narrowed to plugin/unknown-language targets; :153-156
  example list updated), `docs/language-coverage-matrix.md` (regenerated via
  `scripts/gen-coverage-matrix.py` — verify no k8s/cfn double-listing).
- **Change:** as D-2/D-3. No resolver logic edited.
- **Tests:** `cargo test -p wicked-estate-extract` (parity ≥73 now 78,
  `manifest_is_well_formed`, `js_family_languages_share_one_family`,
  `json_characterization` unchanged), `cargo test -p wicked-estate-resolve` (all
  D1/D5 pins incl. `family_guard_allows_unknown_family_jcl_to_cobol` — the canary),
  `cargo test -p wicked-estate` (cross_language_estate, resolver_precision_index,
  relative_imports, rules_bridge_index, multi_repo, id_scheme, e2e, S1(a) now green).
  Clippy + fmt per crate.
- **Deletes (§8):** the stale doc-comment claims — `resolve/src/lib.rs:539-545`
  ("resource nodes … the guard allows them") and the `wicked-estate/src/lib.rs:265-269`
  non-manifest-returns-None wording for json/k8s/cfn. The migration's delete is the
  falsehood, not code: the fix is data.

### S3 — F-B fix: span columns
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
- **Files:** `crates/wicked-estate/tests/unresolved_accounting_cli.rs` (fixture gains
  two same-line sites, e.g. `h(); h();`).
- **Change:** assert the two rows carry **distinct `start_byte`** — the within-line
  discriminator is now pinned end-to-end; record the D-7 SQL run against the S5 AFTER
  command_iq DB (expect 0 rows) in the PR evidence.
- **Tests:** `cargo test -p wicked-estate --test unresolved_accounting_cli`.
- **Deletes:** nothing (the deleted artifact is the manual adjudication protocol —
  closure `not_done[1]` closes).

### S5 — AFTER receipts + adjudication
- **Files:** none (measure/ DBs, PR body).
- **Change:** rebuild; fresh-index command_iq/studio/crew with the AFTER binary.
  Receipts: (1) 402-class query = **0** on command_iq; (2) full Calls edge-set diff vs
  S0 — **removed** set stratified adjudication (every distinct target name, 20 samples
  total; design prior: json.scm mints no callables ⇒ 0 legitimate losses), **added**
  set adjudicated too (excluding json candidates can unshadow new binds — by design,
  pinned by `scoped_family_retain_unshadows_same_family_homonym`); (3) studio/crew
  same-family (javascript) Calls `(source,target)` sets byte-identical to S0;
  (4) index wall-time within noise of S0; (5) D-7 SQL proof = 0 rows;
  (6) `.schema unresolved_refs` diff.
- **Tests:** the queries themselves; every command recorded verbatim.
- **Deletes:** nothing.

### S6 — docs + contract
- **Files:** `docs/ENGINE-CONTRACT.md` (§2.1: the per-ref row now persists
  `start_byte`/`end_byte`, DEFAULT 0 = unknown/synthetic; §3.1 resolver table rows
  reread — resolver set unchanged, `slice_matches_engine_contract_table` must not move).
- **Tests:** `cargo test -p wicked-estate` (slice test), doc grep for the amended claims.
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
  those surfaces) — no shape change. Expected number movement on command_iq-class
  corpora: Calls edge count −402-class, `unresolved_ref_count` rises as those sites
  re-park, bench coverage% falls — the honest direction; pre-explained in the PR so
  the §9 gate is judged on the right claim. rank/pagerank cache and
  isolated/leaf listings shift only on affected repos after re-index.
- **MemStore/Surreal:** already full-fidelity; only gain the kit assertion.

## 5. Falsifier

The plan is wrong if any of these fires:
1. `cargo test -p wicked-estate --test cross_language_estate` or
   `family_guard_allows_unknown_family_jcl_to_cobol` fails after S2 — the fix
   over-blocks family-None participants (rule-B failure mode).
2. The S1(a) fixture test still finds a Calls→json/kubernetes edge after S2 — the
   data fix under-blocks (e.g. families map not reaching NameResolver).
3. The S1(b) conformance assertion fails on any store after S3 — span persistence is
   not real on that backend.
4. Studio/crew same-family Calls edge sets differ S0→S5 — legitimate-edge loss.
5. The removed-edge adjudication finds one legitimate Calls→json bind — the json.scm
   "no callables" prior is wrong and the rule must be reconsidered.
6. The legacy-table migration test fails — old DBs would hard-fail at first index.

## 6. Not in scope

- cobol/rpg/tfstate manifest rows or family changes (D-3; F7 "must keep resolving").
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
