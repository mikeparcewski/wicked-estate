# Recon + plan: incr-integrity lane — shared Import-node dangle + rename re-resolve

Lane base: 764622f (`fix/import-node-dangle`, d7d3b58 ancestor-verified). Recon lenses: history,
consumers, tests, risks — all four verified at this base; every citation below re-opened in this
worktree unless marked (lens). **Revision 2** after adversarial attack (invariants / blast-radius /
feasibility, all "revise"): the four major issues (I1, I2, A1, FEAS-1) and all eight minors are
resolved — see the Attack-revision log at the end for the issue→change map. The headline change:
the deletion-only prune (old D6/S2) is DROPPED, not gated — the attack proved it shipped a real
permanent-loss regression (branch-flip restore of Calls edges) while buying nothing any
measurement gate needs; its test survives and passes under D1 alone.

## Findings acted on

**F1 — dangling shared Import-node edges (MAJOR, closure-suite large-repo lane).**
The Import node is a specifier-keyed GLOBAL symbol (`import/<spec>/`, no path, no repo label)
minted per importer with `location = the importing file` (`crates/wicked-estate-extract/src/treesitter.rs:2271-2300`).
Node ownership is **last-writer-wins**, not first-minter as the brief states: upsert does
`ON CONFLICT(symbol) DO UPDATE SET ... file=excluded.file` (`crates/wicked-estate-store/src/sqlite.rs:389-396`).
`remove_file` deletes nodes strictly by file column (`DELETE FROM nodes WHERE file=?1`,
sqlite.rs:1746-1748; MemStore `crates/wicked-estate-store/src/lib.rs:375-428`; postgres.rs:777;
surreal.rs:204), so removing the current owner kills the node shared by every other importer,
stranding their File→Import edges. A deletion-only run then early-returns BEFORE the Task D prune
(`crates/wicked-estate/src/lib.rs:799-807` vs :1066-1075) — that is why the closure repro's 147
dangling edges PERSIST (142 on released d7d3b58; delta explained by #129 extraction gains). The
next run with any change silently prunes them (sqlite.rs:1794-1803) with **no re-park** — permanent
honest-accounting loss for unchanged importers (ENGINE-CONTRACT §2.1 violation).
**Wider trigger surface than the review recorded** (risks lens, reproduced on the 0.14.6 release
binary): no deletion needed — merely EDITING the owner file so it no longer imports the spec
strands all other importers in the SAME run (remove_file per changed file at lib.rs:812-816, then
prune at :1066 deletes their edges silently). Cross-repo variant: the symbol carries no repo label
(`repo_scope.rs:22-27` documents the wart), so repo A's delete can strand repo B's edges.
Decision J cannot see this class: forced-importer discovery walks only File→File Imports edges
into the deleted file's File node (lib.rs:748-770), never the specifier-keyed Import node.

**F2 — rename re-resolve hole (PER-5 / D01-4).**
Substantially closed at this base by lane C's #127 (c31d196): direct importers of DELETED files are
force-re-extracted (lib.rs:748-795), the resolve index is built from ALL nodes AFTER the WRITE
phase upserts (lib.rs:1001-1013), and the remove-half test exists
(`crates/wicked-estate/tests/e2e.rs:377-431`, `incremental_target_rename_reparks_importer`: honest
park, no dangling). PER-5/D01-4 predate #127. What is missing is only the **add-half test**
(rename B→C with the importer updated to './c' in the same run → File→File edge to C) — the
machinery exists but is unpinned. Note: the BEFORE release binary (built Aug 28) predates c31d196
(Aug 29), so F2 baselines must use the lane-base debug build, not the release binary.

## Decisions

**D1 — Fix F1 at the store seam: target-aware `remove_file` (keep + re-home), not an indexer patch.**
`remove_file(f)` keeps a node when (a) `kind == Import`, (b) `location.file == f`, and (c) at
least one SURVIVOR edge (per the D4 predicate) still targets it (indexed EXISTS on `edges(target)`,
schema.sql:83-85 has the indexes). Rejected alternatives:
- *Per-file Import nodes*: relitigates lane C's Decision F (relative-imports.md:192-198 — SymbolId
  churn across 77 languages, annotations/memory orphaning, fixture contract); needs a
  SYMBOL_ID_SCHEME bump forcing full re-extraction of every user DB (95.2s on command_iq per
  closure-suite), moves node population / PageRank mass / bench receipts. Program-scale, not a lane fix.
- *Prune-then-re-resolve importers* (Decision-J extension to Import targets): O(fan-in) re-parse —
  146 importers for `node:crypto` on command_iq; hub specs approach full-re-index cost, violating
  the "incremental timing unchanged" gate and Decision J's own recorded rationale (lib.rs:25-30).
- *Indexer-only fix scoped to deleted files*: misses the owner-EDIT trigger reproduced by the risks
  lens — the fix must live where the node dies, which is `remove_file`.
Store-seam keep is a pure restoration for every consumer (rank/retrieve/CLI/crew/studio/garden):
no id change, no schema change, no bench movement, O(1) indexed check per removed file.
D1 alone closes the closure repro on the deletion-only path: the removed owner's OWN edges are
deleted by remove_file itself (`file=?1 OR source IN nodes-of-file`), the shared node survives for
everyone else, so nothing dangles — no engine-side prune change is needed (see D6).

**D2 — Re-home the kept node; no separate orphan GC.**
On keep, update the node's home to the deterministic MIN(file) over the SURVIVOR edge set (D4
predicate; the File→Import edge carries the importer's location, treesitter.rs:2291-2300).
Re-home rewrites BOTH representations atomically: the `nodes.file` COLUMN (what remove_file and
the keep-check match on, sqlite.rs:1746-1748) AND `location.file` + span inside the `data` JSON
blob (what every reader deserializes — get_node/all_nodes read `data` only, sqlite.rs:1999-2026,
:2282; the scope-filtered InMemoryIndex keys on `location.file`, lib.rs:281-283). A column-only
update leaves readers seeing the dead path; a JSON-only update makes `remove_file(new-home)` never
match — either half alone re-creates the island/exclusion traps this decision exists to prevent.
MemStore and surreal hold a single representation, no split to keep in sync.
Why re-home at all: leaving `file` stale creates the island-node trap (later
`remove_file(last-importer)` can never match a node homed at a dead path → permanent orphan,
stats drift). Re-homing makes the mechanism self-terminating: when the last importer is removed,
the node is homed there, no survivor edges exist, the keep-check fails, and the normal delete
fires — no new trait method, no GC pass (§5: no orphan machinery without a consumer).
**Batch semantics**: the keep-check and re-home are evaluated PER remove_file CALL against the
edge rows as they stand at that call, never against a batch-start snapshot — the engine removes
deleted files in one loop inside one batch (lib.rs:771-779), and a snapshot would re-home onto a
doomed sibling. Pinned by the batch kit cases in S1.

**D3 — Exception scoped to `NodeKind::Import` only.**
It is the only node class shared-by-construction (Symbol::global, no path component). Every other
node is file-scoped; widening the exception would change `remove_file` semantics for classes with
no defect. The SQL kind literal (`'"import"'` serde form) is pinned by a unit test asserting
`serde_json::to_string(&NodeKind::Import)` equals the literal used (§11 quote-leak scar).

**D4 — ONE survivor-edge predicate, repo-UNscoped, computed once per remove_file call.**
SURVIVOR edge for candidate node `sid` in `remove_file(?1)` :=
`target = sid AND file NOT IN ('', ?1) AND source NOT IN (SELECT symbol FROM nodes WHERE file = ?1)`.
- Repo-UNscoped: the import symbol has no repo label; a scope-filtered check would wrongly delete
  a node still referenced cross-repo (repo_scope.rs wart). The multi_repo case in S3 pins this.
- `file != ''` excluded: `edges.file` is `TEXT NOT NULL DEFAULT ''` (schema.sql:77) and `''` sorts
  before every real path — a locationless edge admitted into MIN(file) would re-home the node to
  path `''`, which no `remove_file('')` ever matches → permanent orphan. If ONLY ''-file edges
  reference the node, it is DELETED (those edges then dangle and Task D prunes them — the
  pre-existing behavior for that schema-legal-but-unminted shape; today only treesitter File→Import
  edges target Import nodes and all carry real files, so this is hardening, not a live case).
- `source NOT IN nodes-of-?1` excluded: an edge whose file column differs from ?1 but whose source
  lives in the removed file is deleted by the same Step-4 statement (`file=?1 OR source IN
  nodes-of-file`, sqlite.rs:1746-1758) — counting it as a survivor would keep a node whose last
  reference is about to die in the same call.
- Computed ONCE per remove_file call and reused at all three consumption points: the Step-3
  FTS/embeddings skip, the Step-4 node-delete keep, and the re-home MIN(file) target. Step 3 runs
  BEFORE the edge delete and Step 4 after; two independently-evaluated predicates straddling the
  edge delete can diverge (orphaned FTS/embedding rows, or a searchable ghost) — one computation
  cannot. Pinned by a store-level kit case that plants a locationless ('') edge targeting the
  Import node.

**D5 — Step-3 FTS/embeddings delete becomes target-aware in the same change.**
sqlite.rs:1703-1731 deletes `nodes_fts` + `embeddings` for ALL syms with `n.file=?1` before the
node delete; a kept node must keep its FTS/embedding rows or it survives the graph but vanishes
from search. The skip is implemented INSIDE remove_file's Step-3 sym collection (exclude kept
syms from the delete list), never as a delete-then-re-insert — re-inserting would duplicate the
FTS row when the owner is edited-and-still-imports (incremental runs re-upsert via
upsert_nodes_skip_fts + bulk_rebuild_fts_for_files, lib.rs:985; the rebuild deletes keyed by
CURRENT nodes.file, sqlite.rs:457-483). The kit asserts exactly-one nodes_fts row for the kept
symbol after that scenario (SqliteStore leg; MemStore has no FTS) and embeddings-row survival
alongside. `edge_history` archival needs NO change: surviving importers' edges have
file=importer / source=importer, so they are outside the archived set (`file=?1 OR source IN
nodes-of-file`).

**D6 — NO prune on the changed-empty path (revision: the old D6 is dropped).**
The original plan added `prune_dangling_edges` before the `changed.is_empty()` early return
(lib.rs:799-807). The attack falsified it three ways and it is removed entirely, not gated:
1. **Wrong trigger (I1/FEAS-1)**: `changed.is_empty()` is also the pure nothing-changed path —
   the most common invocation (every watch tick on an up-to-date DB, main.rs:2083/2115; every
   `index` on an unchanged repo). The prune is an O(edges) double anti-join DELETE
   (sqlite.rs:1794-1803); putting it there taxes every no-op run and adds a per-run non-fatal
   warning on read-only DBs.
2. **Permanent-loss regression (A1)**: today a deletion-only run leaves unchanged callers' Calls
   edges into the deleted file dangling but ALIVE; restoring the file (branch flip back) revives
   the targets under the same deterministic SymbolIds during the WRITE phase, which runs before
   the Task D prune at :1066 — the edges self-heal. A prune on the deletion-only run deletes them
   permanently: the callers are unchanged, resolved Calls sites have no unresolved row and no
   re-park (this plan's own not-in-scope bullet 1), so nothing ever re-creates them. Earlier
   silent loss is strictly worse than a transient dangle that self-heals.
3. **Buys nothing (A1)**: post-D1 the Imports class cannot dangle on this path at all (D1 note),
   the closure repro hits 0 dangling under D1 alone (the probe has no dependents), and both
   rename scenarios force the importer into `changed` so the existing :1066 prune already runs.
   No measurement gate needs the prune.
The deletion-only 0-dangling TEST is kept (S2) — it passing under D1 alone is the evidence the
fix sits at the right seam. Deferred-prune semantics for the pre-existing Calls class on
deletion-only runs stay exactly as today (transient, restore-healable, pruned on the next changed
run); scoping a prune to deleted-and-not-restored targets would need new store API with no
consumer — out of scope.

**D7 — F2: keep honest-park semantics for unchanged importers; deliver the missing tests only.**
Rebinding a NON-updated importer would assert a false edge ('./b' must not bind to c.ts). The
landed e2e.rs:377 assertions stand unchanged. New tests pin: (a) rename with importer updated in
the same run → edge to C, no unresolved row, 0 dangling (machinery exists, unpinned); (b) split
two-run rename (remove-run then add-run) → park in run 1, no dangling; rebind in run 2 only if the
importer changed. No new engine code for F2.

**D8 — No auto-heal for already-damaged post-764622f DBs; document `--force`.**
Crew/garden dev DBs carrying legacy dangles get them pruned exactly as today: silently-deferred to
the next run WITH changes (Task D at lib.rs:1066, logged GRAPH-CLEANUP there). The honest-park
loss for those legacy edges is pre-existing. A heal pass (re-extract files owning dangling edges)
is extra machinery this lane does not need; `--force` full re-index is the documented remedy in
docs/ENGINE-CONTRACT.md (NOT the CHANGELOG release section — release lane owns it).

**D9 — Conformance kit + trait doc amended in the SAME commit as all four store impls.**
conformance.rs:502-514 currently mandates the defective semantics ("remove_file must remove all
nodes whose location.file matches") and traits.rs:258-260 promises "Remove everything that
originated from file". Both are rescoped (non-Import unconditional; Import conditional on no
survivor edges per D4) and the shared-Import cases are added to `graph_store_suite` so MemStore,
SqliteStore, PostgresStore, SurrealStore are pinned by one kit. Spine change ("change with care")
— the lane's adversarial-review step is the sign-off; no ADR governs remove_file today, so the
trait doc + ENGINE-CONTRACT sentence are the contract of record.
**Surreal is NOT a mirror of the sqlite shape** (I4): its remove_file deletes edges
`WHERE src=$sym OR tgt=$sym` per deleted symbol (surreal.rs:236-240) — for a removed Import node
that deletes the OTHER importers' edges outright (immediate loss, not a dangle) — and it discovers
nodes by client-side full-scan JSON parse. The surreal impl must therefore (a) keep the node AND
(b) suppress the tgt-edge delete for kept nodes, with a keep-check that scans edge_rel by tgt.
Surreal kit execution is env-gated (compile gate mandatory), so the one-commit atomicity claim is
code-reviewed, not test-enforced, for that store — recorded in compat.

**D10 — Brief correction recorded.** "the importing file that FIRST minted it" is wrong; ownership
is last-writer-wins (sqlite.rs:389-396). Repro recipes must sequence runs so the to-be-removed
file is the CURRENT owner (index A → add B → remove B), else the test is vacuously green.

## Steps

**S1 — store seam: target-aware remove_file + conformance amendment (one commit).**
Files: `crates/wicked-estate-core/src/conformance.rs` (rescope :502-514 assertion; add kit cases
below), `crates/wicked-estate-core/src/traits.rs` (:258-260 doc),
`crates/wicked-estate-store/src/sqlite.rs` (Step 3 + Step 4 keep/re-home per D2/D4/D5;
kind-literal pin test), `crates/wicked-estate-store/src/lib.rs` (MemStore :375-428),
`crates/wicked-estate-store/src/postgres.rs` (:777 — same dual-representation rewrite as sqlite),
`crates/wicked-estate-store/src/surreal.rs` (:204-240 — divergent shape per D9: keep node AND
suppress tgt-edge delete; keep-check scans edge_rel by tgt).
Kit cases (all in `graph_store_suite`, one commit with the four impls):
- shared-Import owner removal: two files import one spec, remove owner → node survives, re-homed
  (asserted via all_nodes/get_node `location.file` — the JSON path — so a column-only re-home
  fails), other importer's edge intact, FTS + embeddings rows intact (sqlite leg), 0 dangling;
  THEN remove the re-homed file (last importer) → node deleted (proves the COLUMN moved too),
  0 dangling, no island.
- batch (A2): one-run batch delete of owner + one other importer with a third importer surviving →
  node survives homed at the remaining live importer, its edge intact, 0 dangling; one-run batch
  delete of ALL importers → node gone, 0 dangling, no island.
- locationless-edge hardening (I2): plant an edge with `file=''` targeting the Import node; it is
  NOT a survivor — node deletion and re-home ignore it per D4.
- owner-edited-still-imports (D5): remove_file(owner) + re-upsert owner in the same batch →
  exactly one nodes_fts row for the kept symbol afterwards (SqliteStore leg).
- history variant (A5): the owner-removal case runs once with history_enabled=true → the owner's
  File→Import edge is archived, the kept node untouched, surviving edges outside the archive set.
Tests: `cargo test -p wicked-estate-core` (54+ at base), `cargo test -p wicked-estate-store`
(132+ at base; new kit cases green on MemStore + SqliteStore), `cargo build -p wicked-estate-store
--features postgres` (green at base, 1m09s).
Deletes: the unconditional remove-all-nodes-by-file conformance assertion and the trait-doc
sentence promising it (rescoped, not duplicated).

**S2 — deletion-only integrity test (NO engine change — revision).**
Files: `crates/wicked-estate/tests/import_node_integrity.rs` (test only; `crates/wicked-estate/src/lib.rs`
is NOT touched — the old S2 prune is dropped per D6).
Tests: deletion-only test asserting 0 dangling edges immediately after a delete-only run — must
pass under D1 alone (its passing IS the evidence the fix is at the right seam); plus a no-op-run
assertion: a second consecutive fully-unchanged run performs no graph-cleanup writes and returns
identical stats (pins the early-return fast path staying write-free). Existing `tests/id_scheme.rs`
(5) pins the stamp semantics stays intact (that block is untouched).
Deletes: none.

**S3 — engine regression tests.**
Files: new `crates/wicked-estate/tests/import_node_integrity.rs` — T1 sequenced-ownership
owner-delete (index a.ts importing node:crypto → add b.ts same spec → delete b.ts, deletion-only
run: node survives re-homed to a.ts, a.ts edge present, 0 dangling, stats idempotent on a 4th
no-op run); T2 delete the NON-owner (order-independence); T3 owner EDITED to drop the import
(other importers' edges survive same-run); T4 batch delete of owner + non-owner in ONE run
(engine-level pin of the A2 kit case); `crates/wicked-estate/tests/multi_repo.rs` — cross-repo
shared-spec case: delete repo A's importer → repo B's edge survives AND the node's post-re-home
`file` is asserted exactly (deterministic MIN — pins the cross-repo ownership flip, A4);
`crates/wicked-estate/tests/e2e.rs` — F2 add-half test (rename b→c WITH a.ts updated, one run:
exactly one File→File edge to src/c.ts, no unresolved row for './c', 0 dangling) + split two-run
variant. e2e.rs:377 assertions unchanged.
Tests: `cargo test -p wicked-estate` (153 at base + new).
Deletes: none (tests are additions; no existing assertion flips under honest-park, D7).

**S4 — docs retire-as-you-go.**
Files: `crates/wicked-estate/src/repo_scope.rs` (:22-27 — rewrite ONLY the dangle sentence
("Removing that file leaves the other repos' edges to it dangling…"), which D1 falsifies; the
shared-id-shape / last-writer-wins ownership wart STAYS (still true — the upsert is unchanged) and
GAINS the new third ownership-mutation path: re-home on removal, MIN(file) possibly crossing repo
prefixes, same class as last-writer-wins), `docs/recon/relative-imports.md` (:611 residual line
updated), `docs/ENGINE-CONTRACT.md` (remove_file shared-Import semantics + `--force` heal note),
`docs/plan/WAVE-PLAN.md` (W2.6 AC wording only if the sentence is now false).
Tests: none (docs); grep that no doc still claims the dangle sentence.
Deletes: the now-false dangle sentence and residual-risk line — NOT the ownership-wart paragraph.

**S5 — measurements + full verify (before/after protocol).**
Binaries by purpose (FEAS-2):
- Dangling-count reproduction (F1): BEFORE = release binary (profile-irrelevant for counts);
  AFTER = lane debug build.
- Incremental TIMING gate: BEFORE = a debug build of the lane BASE commit (764622f) built in the
  lane CARGO_TARGET_DIR (same profile, same machine) vs the lane debug AFTER build — never
  debug-vs-release.
- F2 baselines: lane-base debug build (the release binary predates c31d196).
Corpus: copy of wicked-studio. Closure repro: add probe file importing a shared external spec
(probe becomes owner via last-writer-wins), remove it, deletion-only re-index → dangling
147-class → 0 (under D1 alone — no engine prune exists on that path, D6), Import node intact +
re-homed, all surviving File→Import edges present, node/edge counts otherwise at baseline. Rename
scenario: 0 dangling both variants. Incremental timing within seconds of the same-profile
baseline; additionally assert the no-op run (second consecutive unchanged index) shows no
GRAPH-CLEANUP line and unchanged timing class. Suite matrix (all with lane CARGO_TARGET_DIR,
per-crate only): core 54, store 132, resolve 123, rank 52, wicked-estate 153, extract 473+1
pre-existing-ignored doctest, bench 15, plus postgres feature build. Bench gate (I5):
`cargo test -p wicked-estate-bench` runs the harness suite; the §9 agent-eval no-regression gate
is satisfied separately by comparing the bench receipts in the AFTER run against the lane-base
receipts (same debug profile, lane target dir) — both the command and the receipt diff are
recorded in measure/. sqlite3 queries recorded verbatim (kinds are JSON strings, `'"imports"'`).

## Compatibility + migration

- No schema change, no SYMBOL_ID_SCHEME bump, no SymbolId change: fresh-index graphs are
  byte-comparable to base (falsified by S5 counts/bench receipts if not).
- Consumers (rank, retrieve, blast-radius, crew `/repos/:id/graph/*`, studio, garden hooks) see
  restored numbers only — edges that today vanish silently now persist. No wire/shape change.
- Engine behavior off the store seam is UNCHANGED: no new prune site, the changed-empty fast path
  stays graph-write-free (D6), deletion-only Calls-class dangles keep today's transient,
  restore-healable, pruned-on-next-changed-run semantics.
- Cross-repo re-home (A4): re-home preserves — and can more frequently exercise — the existing
  cross-repo file-ownership wart: MIN(file) can move the node's `file` across repo prefixes, so a
  path-prefix-scoped view can show an edge whose target node is filtered out. Pre-existing class
  (last-writer-wins already assigns the column across repos arbitrarily); scoped views were and
  remain subject to it. Resolution is immune (Import nodes are never candidates,
  resolve/lib.rs:109,122). The multi_repo test pins the post-re-home owner deterministically.
- GraphWrite contract text changes (spine): all four store impls + kit land in one commit (S1) so
  no cross-store drift window exists. Postgres executes the kit only under TEST_POSTGRES_URL;
  compile gate is mandatory, execution is env-gated as at base. Surreal likewise compile-gated —
  and per D9 its divergent edge-delete shape means the atomicity claim is code-reviewed, not
  test-enforced, for surreal specifically.
- Existing damaged DBs: not auto-healed (D8); residual dangles keep today's fate — pruned with the
  GRAPH-CLEANUP log on the next run WITH changes (Task D); `--force` documented as the full heal.
  Pre-764622f DBs already self-heal via the scheme v1→v2 gate.
- Kept-node `gen` semantics unchanged: the epoch bump fires only on had-node-with-no-live-row
  (sqlite.rs:349-366); keeping the node correctly does NOT bump — matches the spec-string identity.

## Falsifier

The change is wrong if ANY of: (1) the S5 closure repro still shows ≥1 dangling edge, or the
shared Import node is missing, or any surviving importer's File→Import edge was deleted, after
removing the owning probe file — WITHOUT any engine prune on the deletion-only path (if the repro
needs a prune to reach 0, D1 is at the wrong seam); (2) T3 (owner-edit) leaves another importer's
edge dangling or pruned; (3) a fresh full index of wicked-studio differs from base in node/edge
counts or bench receipts (the fix must be a no-op off the removal path); (4) incremental timing
regresses beyond seconds against the SAME-PROFILE baseline, or a fully-unchanged run performs
graph-cleanup work; (5) the conformance kit passes on one store and fails on another (drift);
(6) the F2 add-half test cannot assert the a→c edge after ONE index run (would prove the
lib.rs:1001-1013 structural-coverage claim false and require engine work this plan says is
unnecessary); (7) removing the LAST importer — including via a one-run batch that removes ALL
importers — leaves an island Import node (would prove D2's self-termination claim false);
(8) a locationless ('') edge is admitted as a survivor (node re-homed to '' or kept alive by it).

## Not in scope

- Honest re-park of unchanged CALLERS' resolved Calls edges into a deleted/renamed file (Decision
  J filters kind==Imports; resolved sites have no unresolved row). Recorded as an open finding —
  "no silent loss" claims in this lane are scoped to Imports edges. Corollary (A1): this is
  precisely why NO prune may run on deletion-only paths — see D6.
- A prune scoped to deleted-and-not-restored targets (would need new store API, no consumer).
- Parked relative import whose target is later ADDED (lane C's documented D01-7 residual).
- Per-file Import nodes (needs its own ADR + scheme bump + bench program, D1).
- Auto-heal pass for legacy-damaged DBs (D8).
- The pre-existing 1-ignored doctest in wicked-estate-extract (present at base, not this lane's).
- MUST-NOT-TOUCH honored: version files / CHANGELOG release section, lsp.rs, plugin.rs, the
  admissibility deny-list block in resolve, .scm files — none appear in any step.

## Attack-revision log (revision 2)

Majors — all accepted:
- **A1 (blast-radius) resolves I1 (invariants) and FEAS-1 (feasibility) by subsumption**: the old
  D6/S2 prune-on-deletion-only is DROPPED, not gated. A1 proved the prune converts today's
  restore-healable transient Calls dangles into permanent loss (branch-flip scenario: delete-only
  run prunes; restore revives targets in the WRITE phase before :1066, but the edges are already
  gone and nothing re-creates them) while contributing nothing to any measurement gate (closure
  repro reaches 0 under D1 alone; renames force the importer into `changed`). Dropping it also
  eliminates I1/FEAS-1's no-op-run tax outright (the changed-empty path at lib.rs:799-807 gains
  no code at all). The S2 test is kept and must pass under D1 alone; a no-op-run write-free
  assertion is added (S2, falsifier 4).
- **I2**: D4 rewritten to ONE pinned survivor predicate (`file NOT IN ('', ?1)` and
  `source NOT IN nodes-of-?1`), computed once per remove_file call, reused at Step-3 skip /
  Step-4 keep / re-home; ''-only-referenced nodes are deleted; locationless-edge kit case added
  (S1, falsifier 8).

Minors — all accepted:
- **I3 + FEAS-3**: D2 now mandates the dual-representation rewrite (nodes.file column AND data
  JSON location) with kit assertions exercising both paths (read-back via all_nodes JSON; then
  remove the re-homed file to prove the column moved); embeddings assertion added next to FTS.
- **I4**: D9 + S1 + compat name surreal's divergent shape (tgt-edge delete suppression) and record
  its env-gated kit execution as code-reviewed-only atomicity.
- **I5**: S5 states the bench command and the explicit agent-eval receipt comparison (§9 gate).
- **A2**: batch kit cases (owner+importer with survivor; ALL importers) + engine T4; D2 pins
  per-call evaluation, never batch-snapshot; falsifier 7 extended to the batch shape.
- **A3**: S4 rescoped — only the dangle sentence dies; the ownership wart stays and gains the
  re-home path.
- **A4**: compat sentence on the cross-repo ownership flip; multi_repo asserts the post-re-home
  file deterministically.
- **A5**: history_enabled=true kit variant + exactly-one-FTS-row assertion (closes the old open
  question in merge notes).
- **FEAS-2**: S5 timing baseline is same-profile (lane-base debug build in the lane target dir);
  the release binary is used only for the dangling-count repro.

## Merge notes (other lanes)

- S1 touches `crates/wicked-estate-core/{conformance.rs,traits.rs}` (spine) and all four store
  impls — no lane in the MUST-NOT list owns them, but any lane also editing store files should
  rebase after this lands.
- `docs/ENGINE-CONTRACT.md` edit (S4) is a plausible collision point with other lanes' doc edits —
  the added sentence is self-contained; merge textually.
- The `--force` heal remedy belongs in ENGINE-CONTRACT/docs, NOT the CHANGELOG release section
  (release lane owns CHANGELOG).
- `crates/wicked-estate/src/lib.rs` is no longer edited by this lane at all (old S2 dropped) —
  one fewer collision surface.
