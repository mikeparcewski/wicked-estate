# Recon + plan — release v0.15.0 (lane release-v0150)

Branch `release/v0.15.0`, base `d7d3b58`, HEAD at plan time `764622f` (all five engine
lanes #125/#126/#127/#129/#130 + site #121/#122/#124/#128 merged; verified
`git merge-base --is-ancestor d7d3b58 HEAD` → ok).

## Findings acted on (all verified in this worktree)

1. **The bump automation only touches the root manifest; 40 internal pins are stale at
   `"0.14.2"` and break exactly at this minor boundary.**
   `.github/workflows/release.yml` "bump workspace version" step runs a python `re.sub`
   over `pathlib.Path("Cargo.toml")` ONLY (full-semver `version = "X.Y.Z"` strings),
   then `cargo check --workspace`, commits `chore(release): vX.Y.Z`, tags, pushes main.
   Root carries `[workspace.package] version = "0.14.6"` (Cargo.toml:21) and
   `wicked-estate-core`/`-store` workspace-dependency pins at `"0.14.6"` (Cargo.toml:32-33).
   Twelve crate manifests carry **40** internal path-dep pins at `version = "0.14.2"`
   (verified `grep -rn 'version = "0.14.2"' crates/*/Cargo.toml | wc -l` → 40; files:
   bench, knowledge, memory-core, mcp, memory-api, memory, rank, retrieve, overlay,
   resolve, store, wicked-estate). Cargo caret semantics: `^0.14.2` = `>=0.14.2,<0.15.0`
   — satisfied by every 0.14.x release, **excluded by 0.15.0**. On `cargo publish` the
   path components are stripped and the verify build resolves those reqs against
   crates.io, i.e. against the OLD published 0.14.6 crates. `wicked-estate` 0.15.0 calls
   `resolve_all_with_coverage` (crates/wicked-estate-resolve/src/lib.rs:788 — the only
   remaining `pub fn resolve_all*`), which does not exist at 0.14.6 → verify fails
   mid-way through the publish sequence, after leaf crates are already irreversibly
   published at 0.15.0. RELEASING.md §1 mandates keeping the pins in sync; the repo
   drifted from its own protocol since 0.14.3. `wicked-estate-extract` is NOT affected:
   it uses `wicked-estate-core.workspace = true` (extract/Cargo.toml:14) and pins only
   vendored grammars at their own 0.1.x versions (extract/Cargo.toml:117,121,…) — those
   stay.

2. **0.15.0 is mandated, and lockstep is structural.** CHANGELOG [Unreleased] Removed
   bullet ends "**Public-API removal: under cargo 0.x semver the next release must be
   `0.15.0`, not `0.14.7`.**" (CHANGELOG.md:6). The `resolve_all` edges-only wrapper is
   a second pub-API removal (#130). Every workspace crate uses
   `version.workspace = true` (e.g. core/Cargo.toml:3), so all 14 published crates +
   bench (`publish = false`, bench/Cargo.toml:4) move to 0.15.0 together; the 11
   vendored grammar crates version independently and are untouched by release.yml's
   regex.

3. **CHANGELOG [Unreleased] mis-scopes the release.** It contains entries that already
   shipped: PostgresStore torn-read fix (#104, 758e75f) and `WICKED_RUNTIME` seam
   (#105, e841c91) are ancestors of tag v0.14.5; multi-repo `--repo` + the
   silent-destruction refusal (#117, 42a1040) are ancestors of v0.14.6 (verified
   `git merge-base --is-ancestor` for all three). The section headers jump
   `[Unreleased]` → `[0.14.2] — 2026-07-30` (CHANGELOG.md:3,24) — no 0.14.3–0.14.6
   sections were ever written (release.yml never touches CHANGELOG). There is also no
   site entry and no explicit Removed bullet for the `resolve_all` wrapper.

4. **No migration runbook file exists.** `grep -rn runbook docs CHANGELOG.md README.md`
   → zero hits. The migration story lives in
   `docs/adr/ADR-002-stable-symbol-identity.md:135` ("### Migration — `SYMBOL_ID_SCHEME`
   and the `id_scheme` gate") and the CHANGELOG scheme-2 entry. Mechanism correction to
   the brief: for a stock 0.14.6 DB the **`indexed_version` gate** fires
   (crates/wicked-estate/src/lib.rs:590-601, any `CARGO_PKG_VERSION` change per repo);
   the `id_scheme` gate (lib.rs:632-639, key written only after completion) is the
   crash-idempotent backstop and the only protection for same-version binaries. The
   CHANGELOG must link a real path, not a phantom one.

5. **#130 accidentally reverted #128's site work — the brief's premise "Content.tsx
   says 0.14.6" is false at this HEAD.** `git diff --stat b25ccb4 764622f -- site` =
   Content.tsx 156 lines (17+/150−), home.spec.ts, reduced-motion.spec.ts — a pure
   revert of #128 (deletes the `#binary` long-tail section, restores the falsified
   "7 resolution tiers · Parsed → SCIP → LSP" copy that #128 existed to remove, drops
   the e2e assertions). #129 touched no site files (`git show --stat bda76b7 -- site`
   → empty); nothing in #130's message mentions site. Content.tsx currently says
   `v0.14.4` at lines 17, 141, 944, 1038.

6. **The brief's bench cosmetic defect is already gone at this HEAD, but would
   reappear.** `grep -rn 'scratchpad\|/private' docs/benchmarks/` → rc=1 (no hits).
   capability-report.md was regenerated at #125 on wicked-studio + wicked-crew with
   `/Users/...` paths (report lines 37, 105) and has no axios row; the generator embeds
   the literal CLI path in `**Path:**` lines, so running against scratchpad clones
   would recreate the defect class. Residual real defect: coverage-matrix.md still
   opens with a stale `## axios` section (coverage-matrix.md:7) from the pre-#125
   corpus — report and matrix describe different corpora. The bench has **no clone
   code**: `baseline_corpus()` (bench/src/lib.rs:99-119) is a spec with no callers;
   main.rs takes repo paths as args. The committed resolver_breakdown predates #126/#127
   — no `relative-import` or rules-bridge rows yet (report lines 96-101, 161-167).

7. **README.md:28 says "Status: v0.14.5 — … 1,100+ tests"; repo CLAUDE.md header says
   v0.13.1; plugin manifests say 0.13.1.** No mechanism syncs any of them.

## Decisions

| # | Question | Decision | Why |
|---|---|---|---|
| D1 | Do all workspace crates (incl. wicked-estate-mcp) move to 0.15.0 in lockstep? | **Yes — automatic.** | Every crate has `version.workspace = true`; only bench is `publish = false` (bench/Cargo.toml:4). publish.sh's comment claiming mcp is unpublished is stale — crates.io has wicked-estate-mcp 0.14.6 and no manifest carries `publish = false` except bench. |
| D2 | Bump via release.yml or manually in the branch? | **Manually in the branch; never dispatch release.yml from this lane.** | release.yml pushes straight to main and its regex only rewrites the ROOT Cargo.toml — it cannot fix the 40 stale per-crate pins (finding 1). Pre-bumping in the branch makes a later release.yml dispatch a harmless no-op-commit + tag (its `git diff --cached --quiet \|\|` guard), but tagging the merged sha directly is recommended (merge note M3). |
| D3 | Which files carry the bump? | Root Cargo.toml:21,:32,:33 (0.14.6→0.15.0) + all 40 per-crate pins (`"0.14.2"`→`"0.15.0"`) + Cargo.lock via `cargo check --workspace`. Vendored grammar versions untouched. | Finding 1. This is the single change that keeps the tag job from an irreversible partial publish. |
| D4 | Convert [Unreleased] verbatim into [0.15.0]? | **No — split by tag ancestry.** Retro-create `[0.14.5]` (torn-read #104, WICKED_RUNTIME #105, conformance cleanup) and `[0.14.6]` (multi-repo `--repo` + silent-destruction refusal #117); the rest becomes `[0.15.0] — 2026-08-29`. | Finding 3: verbatim conversion fabricates release notes provably false against the tags. |
| D5 | Migration runbook link | **Write `docs/MIGRATION-0.15.md`** (release collateral, this lane's to own) consolidating the gate mechanics from ADR-002:135-160 + lib.rs:590-601/:632-639, and link it from the [0.15.0] scheme-2 entry alongside ADR-002 §Migration. State the ACTUAL mechanism: cross-version re-extraction is the `indexed_version` gate; `id_scheme` is the crash-idempotent same-version backstop. Cover: annotations orphan, xedges epoch-drop, `--embeddings` re-run, `wicked-estate scip` re-run, MCP-only users must run `index` (the server never migrates on its own). | Finding 4: the CHANGELOG's "see the migration runbook" must point at a file that exists; the brief's id_scheme-only phrasing is mechanically wrong for stock 0.14.6 DBs. |
| D6 | Site version strings in this bump? | **Yes, after restoring #128.** Step order: restore #128's three reverted site files from b25ccb4 (`git checkout b25ccb4 -- site/src/components/Content.tsx site/tests/e2e/home.spec.ts site/tests/e2e/reduced-motion.spec.ts` — safe: #129 has no site diff and #130's site diff is a pure revert), then bump the four version strings to v0.15.0, re-verifying the grounded figures (language count from languages.toml post-#129 `.h` re-routing, tool count) so "Grounded to v0.15.0" is true. Reasoning for bumping pre-tag: merge and tag are one orchestrated motion; the exposure window (merged-but-untagged) is minutes; the site deploys only from main; and the current strings are already two releases stale, proving the wait-for-tag policy never executes. Stamping v0.15.0 onto the UN-restored copy is forbidden — it would ground the falsified "7 resolution tiers" claim to the new release. | Finding 5. If the orchestrator rules the restoration out-of-lane, the site commit is separable — drop it and record the regression (merge note M1); do NOT keep only the string bump. |
| D7 | Bench corpus for the committed 0.15.0 receipts | **wicked-studio + wicked-crew only** (the corpora the committed BEFORE report was generated on, `/Users/...` paths — no scratchpad path can land in the doc). Do NOT chase axios/flask/tree-sitter: no clone code exists (`baseline_corpus()` has zero callers, bench/src/lib.rs:99-119), no committed BEFORE rows exist for them, and the report's own corpus-change note declares cross-corpus rows non-comparable. Record the bench README/methodology drift + orphan `baseline_corpus` as follow-ups, not lane edits. | Finding 6; brief's "the corpus clones axios/flask/tree-sitter" premise does not match the code. |
| D8 | Bench build profile | **Release profile** (`cargo run -p wicked-estate-bench --release`, lane CARGO_TARGET_DIR). | The committed report's timing columns (studio 2398 ms, crew 1500 ms) are release-built; a debug regeneration would fake a 5-20x perf regression into a release artifact. |
| D9 | plugin.json / marketplace.json / CLAUDE.md header (0.13.1) | **Do not touch.** | No script syncs them, no test reads them, and whether the installer's tag-fired conformance checks the plugin version is unverifiable from this repo. Recorded as follow-up F2 rather than guessed at. |
| D10 | README.md:28 status line | **Restamp to v0.15.0 with the REAL test count from this branch's gate run** (step S8), not the stale "1,100+". | §7: bumping the version while keeping an unverified count fabricates evidence. |
| D11 | Prevent pin-drift recurrence | **Add a guard test** `crates/wicked-estate/tests/version_pins.rs`: parse `crates/*/Cargo.toml`, assert every `wicked-estate-*` dep pin (excluding `wicked-estate-tree-sitter-*`) equals `env!("CARGO_PKG_VERSION")`. Plus one sentence in RELEASING.md §1 noting release.yml does NOT sync the per-crate pins. | The stale-pin hazard is the only release failure mode with zero coverage; this turns it into a red `cargo test --workspace`. Test-only file — not engine source. |

## Steps

**S1 — version bump.**
Files: `Cargo.toml` (:21,:32,:33), the 12 crate manifests listed in finding 1 (40 pins),
`Cargo.lock` (via `cargo check --workspace` under the lane CARGO_TARGET_DIR).
Test: `grep -rn 'version = "0.14' Cargo.toml crates/*/Cargo.toml` → 0 rows;
`cargo check --workspace` clean; Cargo.lock shows 15 workspace crates at 0.15.0.
Deletes: the stale 0.14.2/0.14.6 pin values.

**S2 — pin-drift guard.**
Files: new `crates/wicked-estate/tests/version_pins.rs`; one sentence in `RELEASING.md` §1.
Test: `cargo test -p wicked-estate --test version_pins` green; red when any pin is
reverted (verify once by temporarily flipping one pin, then restore).
Deletes: nothing (net-new guard).

**S3 — CHANGELOG restructure + migration runbook.**
Files: `CHANGELOG.md` (split per D4; add Removed bullet for the `resolve_all` wrapper;
add a Site line for #121/#122/#124/#128; date [0.15.0] — 2026-08-29; migration sentence
linking `docs/MIGRATION-0.15.md` + ADR-002 §Migration); new `docs/MIGRATION-0.15.md`
(per D5).
Test: `git merge-base --is-ancestor` receipts for every moved entry quoted in the
commit message; `grep -n 'MIGRATION-0.15' CHANGELOG.md` → hit; the linked path exists.
Deletes: the mis-scoped placement of #104/#105/#117 under the 0.15.0 heading.

**S4 — site restore + version strings** (per D6, separable commit).
Files: `site/src/components/Content.tsx`, `site/tests/e2e/home.spec.ts`,
`site/tests/e2e/reduced-motion.spec.ts` (restore from b25ccb4), then Content.tsx
version strings → v0.15.0 with re-verified grounded figures (languages.toml row count,
MCP tool count).
Test: `git diff b25ccb4 -- site` after restore shows ONLY the version-string +
re-grounding edits; `grep -n '7 resolution tiers' site/src` → 0;
`grep -c 'v0.15.0' site/src/components/Content.tsx` matches the intended count.
Site e2e run if the site toolchain is available locally; otherwise CI.
Deletes: the falsified "7 resolution tiers · Parsed → SCIP → LSP" copy (again) and the
stale v0.14.4 strings.

**S5 — README status line** (per D10). Files: `README.md:28`.
Test: the stated test count equals S8's recorded total. Deletes: the stale
"v0.14.5 … 1,100+" claim.

**S6 — bench re-baseline** (per D7/D8).
Command: `CARGO_TARGET_DIR=<lane-target> cargo run --release -p wicked-estate-bench --bin wicked-estate-bench -- /Users/michael.parcewski/Projects/wicked/wicked-studio /Users/michael.parcewski/Projects/wicked/wicked-crew`
run from the worktree root (writes both `docs/benchmarks/capability-report.md` and
`docs/benchmarks/coverage-matrix.md` there). Record in the commit: the exact command,
`git rev-parse HEAD` of both corpora, and the BEFORE row values.
Diff keys (BEFORE from the committed #125 report): studio 4,671 nodes / 9,229 edges /
34,083 unresolved; crew 2,933 / 5,392 / 16,322; resolver_breakdown studio
scoped-name 1,838 / name 1,245 / import-map 178, crew 1,100 / 726. AFTER must add
`relative-import` rows (0.9 band) and rules-bridge attribution; unresolved counts drop
(#125 accounting); any scoped-name drop is the documented scheme-2 correction —
verdict rule = docs/benchmarks/README.md "2026-08 — symbol-id scheme 2 re-baseline note".
Test: `grep -rn 'scratchpad\|/private' docs/benchmarks/` → 0 hits post-regeneration;
both artifacts describe the same corpus.
Deletes: the stale `## axios` section in coverage-matrix.md and every stale number in
capability-report.md.

**S7 — memory recall gate.** Same binary with `--recall` per docs/benchmarks/README.md;
record pass/fail verbatim. If it cannot complete (model download etc.), record exactly
why + partial evidence (§7) — do not fake it.

**S8 — gates.** `cargo fmt --all -- --check`, `cargo build --workspace --all-targets`
(0 warnings), `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` — all under the lane CARGO_TARGET_DIR (workspace-wide is
permitted in THIS lane's isolated target). Record exact counts. Known flake:
`wicked-estate-bench footprint_and_speed_within_ceilings` asserts >20 nodes/s
wall-clock and failed twice under concurrent lane builds (17.6) while passing idle in
0.67 s — on failure, rerun solo; never lower the floor (docs/TESTING.md). Postgres
conformance / embedder suites self-skip locally (postgres_conformance.rs:86,140) —
branch-push CI is the authoritative green for those.

## Compatibility + migration

- **Published crates:** 0.15.0 lockstep across the 14 published workspace crates;
  grammars unchanged. After S1, the publish verify resolves internal deps at ^0.15.0 —
  the partial-publish failure mode is closed.
- **Stored graphs:** first `index` of each repo under a 0.15.0 binary forces full
  re-extraction via the `indexed_version` gate (lib.rs:590-601); the `id_scheme` gate
  covers same-version binaries and crash re-fires (key written after completion,
  lib.rs:804/:1086 per #130). NOT carried over: annotations on churned ids, overlay/
  memory xedges (epoch-dropped), embeddings (`--embeddings` re-run), SCIP edges
  (`wicked-estate scip` re-run). MCP-only users serve the old graph until someone runs
  `index`. All of this lands in docs/MIGRATION-0.15.md (S3).
- **Consumers:** crew/garden/studio pin no estate version and probe at runtime — for
  them 0.15.0 is a numbers change (unresolved down, relative-import edges up,
  SymbolIds re-minted), not a wire-shape change.

## Falsifier

If, after S1/S2, `cargo test -p wicked-estate --test version_pins` passes while any
`crates/*/Cargo.toml` still pins a `wicked-estate-*` dep at ≠0.15.0, the guard is wrong
and the release is still publish-broken — checked by the independent
`grep -rn 'version = "0.14' Cargo.toml crates/*/Cargo.toml` → must be 0 rows.
Release-level falsifier for the orchestrator: after tagging, a publish.yml run must
EXIST for the tag within minutes (the v0.14.6 tag produced zero runs — publish.yml's
own header documents it); recovery = workflow_dispatch with `v0.15.0`.

## Not in scope (this lane)

- Engine source changes of any kind (other lanes own them; S2's guard is a test file,
  not engine source).
- Fixing bench README/methodology drift (documented CLI flags `--corpus`/`--db` don't
  match main.rs; orphan `baseline_corpus()`, §5) — follow-up F1.
- plugin.json / marketplace.json / repo CLAUDE.md header versions (D9) — follow-up F2.
- Making release.yml sync the per-crate pins, or migrating internal deps to
  `[workspace.dependencies]`; fixing publish.sh's stale mcp/memory-api comment and
  RELEASING.md's stale 9-crate order list beyond the one D11 sentence — follow-up F3.
- Backfilling [0.14.3]/[0.14.4] CHANGELOG sections (tags exist, but nothing in
  [Unreleased] belongs to them; retro sections limited to what D4 requires).
- Pushing, tagging, publishing — orchestrator.
- Relativising the bench report's `**Path:**` output in code (a bench-crate change) —
  D7 avoids the defect by corpus choice; durable fix is follow-up F1.

## Merge notes for the orchestrator / other lanes

- **M1 — #130 reverted #128's site work** (finding 5): the lane base carries a site
  regression (falsified tiers copy back, #binary section gone, e2e reverted, v0.14.4).
  S4 restores it here; if that is ruled out-of-lane, the regression must be fixed
  somewhere before the site next deploys from main — and the brief's "Content.tsx says
  0.14.6 post-#128" premise should be corrected in any other lane brief that inherited it.
- **M2 — the brief's bench premises are stale**: no scratchpad path exists in
  docs/benchmarks at this HEAD, and no code clones axios/flask/tree-sitter (D7).
- **M3 — tagging**: prefer tagging the merged PR sha directly. Dispatching release.yml
  with 0.15.0 also works after this branch (regex no-ops, empty-diff guard skips the
  commit, it still tags + pushes), but it pushes to main on its own and adds nothing.
  Either way, verify a publish.yml run started (see Falsifier).
- **M4 — one committed scratchpad path exists elsewhere**:
  docs/recon/method-identity.md:483 (owned by the method-identity lane's artifacts) —
  not touched here.
