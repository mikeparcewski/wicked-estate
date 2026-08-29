# Recon + plan: incr-integrity lane — shared Import-node dangle + rename re-resolve

Lane base: 764622f (`fix/import-node-dangle`, d7d3b58 ancestor-verified). Recon lenses: history,
consumers, tests, risks — all four verified at this base; every citation below re-opened in this
worktree unless marked (lens).

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
(`crates/wicked-estate/src/lib.rs:798-807` vs :1066-1075) — that is why the closure repro's 147
dangling edges PERSIST (142 on released d7d3b58; delta explained by #129 extraction gains). The
next run with any change silently prunes them (sqlite.rs:1794-1806) with **no re-park** — permanent
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
least one edge from ANOTHER file still targets it (indexed EXISTS on `edges(target)`,
schema.sql has the indexes). Rejected alternatives:
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

**D2 — Re-home the kept node; no separate orphan GC.**
On keep, update the node's `file` (+ location span) to the deterministic surviving referencing
edge with MIN(file) among edges targeting it from other files (the File→Import edge carries the
importer's location, treesitter.rs:2291-2300). Why: leaving `file` stale creates the island-node
trap (later `remove_file(last-importer)` can never match a node homed at a dead path → permanent
orphan, stats drift) and excludes the node from the scope-filtered InMemoryIndex (lib.rs:281-283).
Re-homing makes the mechanism self-terminating: when the last importer is removed, the node is
homed there, no other-file edges exist, the keep-check fails, and the normal delete fires — no new
trait method, no GC pass (§5: no orphan machinery without a consumer).

**D3 — Exception scoped to `NodeKind::Import` only.**
It is the only node class shared-by-construction (Symbol::global, no path component). Every other
node is file-scoped; widening the exception would change `remove_file` semantics for classes with
no defect. The SQL kind literal (`'"import"'` serde form) is pinned by a unit test asserting
`serde_json::to_string(&NodeKind::Import)` equals the literal used (§11 quote-leak scar).

**D4 — Keep-check is raw edge existence, repo-UNscoped.**
The import symbol has no repo label; a scope-filtered check would wrongly delete a node still
referenced cross-repo (repo_scope.rs wart). Raw `EXISTS (edges WHERE target=sid AND file != ?1)`.

**D5 — Step-3 FTS/embeddings delete becomes target-aware in the same change.**
sqlite.rs:1703-1731 deletes `nodes_fts` + `embeddings` for ALL syms with `n.file=?1` before the
node delete; a kept node must keep its FTS/embedding rows or it survives the graph but vanishes
from search. `edge_history` archival needs NO change: surviving importers' edges have
file=importer / source=importer, so they are outside the archived set (`file=?1 OR source IN
nodes-of-file`).

**D6 — Deletion-only runs prune before the early return.**
Run `prune_dangling_edges` (with the GRAPH-CLEANUP log line) on the `changed.is_empty()` path at
lib.rs:798-807, preserving the scheme-stamp semantics of that block exactly (#130). Why: the 147
persisted only because of this skip; post-D1 the Imports class prunes to 0 there by construction,
and the Calls class (a deleted file's own symbols) is cleaned deterministically-and-logged instead
of silently on the next unrelated run — same edges deleted, earlier and visible. R5 staleness
honesty improves; no behavior a consumer relies on regresses (verified: nothing reads danglers as
data — rank/traverse/graph-view all skip missing endpoints).

**D7 — F2: keep honest-park semantics for unchanged importers; deliver the missing tests only.**
Rebinding a NON-updated importer would assert a false edge ('./b' must not bind to c.ts). The
landed e2e.rs:377 assertions stand unchanged. New tests pin: (a) rename with importer updated in
the same run → edge to C, no unresolved row, 0 dangling (machinery exists, unpinned); (b) split
two-run rename (remove-run then add-run) → park in run 1, no dangling; rebind in run 2 only if the
importer changed. No new engine code for F2.

**D8 — No auto-heal for already-damaged post-764622f DBs; document `--force`.**
Crew/garden dev DBs carrying legacy dangles get them pruned (now deterministically + logged, D6)
exactly as today's next-change run would; the honest-park loss for those legacy edges is
pre-existing. A heal pass (re-extract files owning dangling edges) is extra machinery this lane
does not need; `--force` full re-index is the documented remedy in docs/ENGINE-CONTRACT.md (NOT
the CHANGELOG release section — release lane owns it).

**D9 — Conformance kit + trait doc amended in the SAME commit as all four store impls.**
conformance.rs:502-514 currently mandates the defective semantics ("remove_file must remove all
nodes whose location.file matches") and traits.rs:258-260 promises "Remove everything that
originated from file". Both are rescoped (non-Import unconditional; Import conditional on no
other-file references) and a shared-Import case is added to `graph_store_suite` so MemStore,
SqliteStore, PostgresStore, SurrealStore are pinned by one kit. Spine change ("change with care")
— the lane's adversarial-review step is the sign-off; no ADR governs remove_file today, so the
trait doc + ENGINE-CONTRACT sentence are the contract of record.

**D10 — Brief correction recorded.** "the importing file that FIRST minted it" is wrong; ownership
is last-writer-wins (sqlite.rs:389-396). Repro recipes must sequence runs so the to-be-removed
file is the CURRENT owner (index A → add B → remove B), else the test is vacuously green.

## Steps

**S1 — store seam: target-aware remove_file + conformance amendment (one commit).**
Files: `crates/wicked-estate-core/src/conformance.rs` (rescope :502-514 assertion; add
shared-Import case: two files import one spec, remove owner → node survives re-homed to the other
file, its edge intact, FTS row intact, 0 dangling; then remove the last importer → node deleted),
`crates/wicked-estate-core/src/traits.rs` (:258-260 doc), `crates/wicked-estate-store/src/sqlite.rs`
(Step 3 + Step 4 keep/re-home; kind-literal pin test), `crates/wicked-estate-store/src/lib.rs`
(MemStore :375-428 mirror), `crates/wicked-estate-store/src/postgres.rs` (:777 mirror),
`crates/wicked-estate-store/src/surreal.rs` (:204 mirror).
Tests: `cargo test -p wicked-estate-core` (54+ at base), `cargo test -p wicked-estate-store`
(132+ at base; new kit case green on MemStore + SqliteStore), `cargo build -p wicked-estate-store
--features postgres` (green at base, 1m09s).
Deletes: the unconditional remove-all-nodes-by-file conformance assertion and the trait-doc
sentence promising it (rescoped, not duplicated).

**S2 — engine: prune on deletion-only runs.**
Files: `crates/wicked-estate/src/lib.rs` (:798-807 — prune + GRAPH-CLEANUP log before the early
return; scheme-stamp block untouched).
Tests: new deletion-only test (S3 file) asserting 0 dangling edges immediately after a
delete-only run; existing `tests/id_scheme.rs` (5) pins the stamp semantics stays intact.
Deletes: none — additive integrity backstop (honest: this step is an addition, justified as the
closure-persistence fix; the silent-deferred prune behavior it replaces is behavioral, not code).

**S3 — engine regression tests.**
Files: new `crates/wicked-estate/tests/import_node_integrity.rs` — T1 sequenced-ownership
owner-delete (index a.ts importing node:crypto → add b.ts same spec → delete b.ts, deletion-only
run: node survives re-homed to a.ts, a.ts edge present, 0 dangling, stats idempotent on a 4th
no-op run); T2 delete the NON-owner (order-independence); T3 owner EDITED to drop the import
(other importers' edges survive same-run); `crates/wicked-estate/tests/multi_repo.rs` — cross-repo
shared-spec case (delete repo A's importer, repo B's edge survives); `crates/wicked-estate/tests/e2e.rs`
— F2 add-half test (rename b→c WITH a.ts updated, one run: exactly one File→File edge to src/c.ts,
no unresolved row for './c', 0 dangling) + split two-run variant. e2e.rs:377 assertions unchanged.
Tests: `cargo test -p wicked-estate` (153 at base + new).
Deletes: none (tests are additions; no existing assertion flips under honest-park, D7).

**S4 — docs retire-as-you-go.**
Files: `crates/wicked-estate/src/repo_scope.rs` (:22-27 wart paragraph rewritten — it becomes
false), `docs/recon/relative-imports.md` (:611 residual line updated),
`docs/ENGINE-CONTRACT.md` (remove_file shared-Import semantics + `--force` heal note),
`docs/plan/WAVE-PLAN.md` (W2.6 AC wording only if the sentence is now false).
Tests: none (docs); grep that no doc still claims the wart.
Deletes: the now-false wart paragraph and residual-risk line.

**S5 — measurements + full verify (before/after protocol).**
BEFORE = release binary (F1 only; F2 baseline = lane-base debug build per D10/F2 note). AFTER =
lane debug build. Corpus: copy of wicked-studio. Closure repro: add probe file importing a shared
external spec (probe becomes owner), remove it, deletion-only re-index → dangling 147-class → 0,
Import node intact + re-homed, all surviving File→Import edges present, node/edge counts otherwise
at baseline. Rename scenario: 0 dangling both variants. Incremental timing within seconds of
BEFORE. Suite matrix (all with lane CARGO_TARGET_DIR, per-crate only): core 54, store 132,
resolve 123, rank 52, wicked-estate 153, extract 473+1 pre-existing-ignored doctest, bench 15,
plus postgres feature build. sqlite3 queries recorded verbatim (kinds are JSON strings, `'"imports"'`).

## Compatibility + migration

- No schema change, no SYMBOL_ID_SCHEME bump, no SymbolId change: fresh-index graphs are
  byte-comparable to base (falsified by S5 counts/bench receipts if not).
- Consumers (rank, retrieve, blast-radius, crew `/repos/:id/graph/*`, studio, garden hooks) see
  restored numbers only — edges that today vanish silently now persist. No wire/shape change.
- GraphWrite contract text changes (spine): all four store impls + kit land in one commit (S1) so
  no cross-store drift window exists. Postgres executes the kit only under TEST_POSTGRES_URL;
  compile gate is mandatory, execution is env-gated as at base. Surreal likewise compile-gated.
- Existing damaged DBs: not auto-healed (D8); residual dangles are pruned deterministically with a
  GRAPH-CLEANUP log on the next run (including deletion-only runs, new); `--force` documented as
  the full heal. Pre-764622f DBs already self-heal via the scheme v1→v2 gate.
- Kept-node `gen` semantics unchanged: the epoch bump fires only on had-node-with-no-live-row
  (sqlite.rs:349-366); keeping the node correctly does NOT bump — matches the spec-string identity.

## Falsifier

The change is wrong if ANY of: (1) the S5 closure repro still shows ≥1 dangling edge, or the
shared Import node is missing, or any surviving importer's File→Import edge was deleted, after
removing the owning probe file; (2) T3 (owner-edit) leaves another importer's edge dangling or
pruned; (3) a fresh full index of wicked-studio differs from base in node/edge counts or bench
receipts (the fix must be a no-op off the removal path); (4) incremental timing regresses beyond
seconds on the studio corpus; (5) the conformance kit passes on one store and fails on another
(drift); (6) the F2 add-half test cannot assert the a→c edge after ONE index run (would prove the
lib.rs:1001-1013 structural-coverage claim false and require engine work this plan says is
unnecessary); (7) removing the LAST importer leaves an island Import node (would prove D2's
self-termination claim false).

## Not in scope

- Honest re-park of unchanged CALLERS' resolved Calls edges into a deleted/renamed file (Decision
  J filters kind==Imports; resolved sites have no unresolved row). Recorded as an open finding —
  "no silent loss" claims in this lane are scoped to Imports edges.
- Parked relative import whose target is later ADDED (lane C's documented D01-7 residual).
- Per-file Import nodes (needs its own ADR + scheme bump + bench program, D1).
- Auto-heal pass for legacy-damaged DBs (D8).
- The pre-existing 1-ignored doctest in wicked-estate-extract (present at base, not this lane's).
- MUST-NOT-TOUCH honored: version files / CHANGELOG release section, lsp.rs, plugin.rs, the
  admissibility deny-list block in resolve, .scm files — none appear in any step.

## Merge notes (other lanes)

- S1 touches `crates/wicked-estate-core/{conformance.rs,traits.rs}` (spine) and all four store
  impls — no lane in the MUST-NOT list owns them, but any lane also editing store files should
  rebase after this lands.
- `docs/ENGINE-CONTRACT.md` edit (S4) is a plausible collision point with other lanes' doc edits —
  the added sentence is self-contained; merge textually.
- The `--force` heal remedy belongs in ENGINE-CONTRACT/docs, NOT the CHANGELOG release section
  (release lane owns CHANGELOG).
- Open question deferred to review, not blocking: whether the kept-node re-home should also be
  exercised under history_enabled=true (no history test covers shared nodes today; S1's kit case
  can be extended there if the reviewer demands it).
