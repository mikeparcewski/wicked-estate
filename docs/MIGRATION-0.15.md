# Migrating stored graphs to 0.15.0

0.15.0 changes how definition SymbolIds are minted (symbol-id **scheme 3**, type-nested
identity plus the `.anchor`/`.owner` query capture roles — ADR-002 amendment, #130/#136;
scheme 2 was its unreleased first cut, superseded in place, so a released 0.14.x DB
migrates 1→3 in one pass). Every previously-indexed repo must be fully re-extracted; ids
minted under the old flat scheme are re-minted. This runbook is what the
CHANGELOG's "migration runbook" link points at. Design rationale:
[`docs/adr/ADR-002-stable-symbol-identity.md`](adr/ADR-002-stable-symbol-identity.md)
§"Migration — `SYMBOL_ID_SCHEME` and the `id_scheme` gate".

## What fires, and when

Migration is **not automatic on upgrade** — it happens on the next `wicked-estate index`
of each repo, driven by two independent, **per-repo** gates
(`crates/wicked-estate/src/lib.rs`, `index_path_as`):

1. **`indexed_version` gate** — the store remembers the binary version that last indexed
   each repo; any `CARGO_PKG_VERSION` change (0.14.x → 0.15.0) forces a full
   re-extraction of that repo and prints `VERSION CHANGE detected (v0.14.x → v0.15.0):
   forcing full re-extraction`. For a stock 0.14.x DB this is the gate that fires.
2. **`id_scheme` gate** — the crash-idempotent backstop, and the only protection when the
   version did NOT change (e.g. dev builds, or a pre-version DB that has nodes + digests
   but no `indexed_version` key). The scheme key is written only **after** re-extraction
   completes, so an interrupted migration re-fires the gate on the next run instead of
   leaving a permanently mixed graph.

No schema migration: the database file itself opens unchanged; only rows are rebuilt.

## Multi-repo databases: the partial-migration window

**Both gates are per-repo** (`repo_scope::meta_key(repo, …)`). In a shared labelled DB
(`--db one.db --repo <label>`), each label migrates only when *that label* is next
indexed. Between the first re-indexed label and the last, the DB serves **mixed
scheme-1/scheme-3 SymbolIds** — queries work, but ids for not-yet-migrated repos are
still flat, and cross-checking ids across labels is unreliable. Do not conclude the DB
"migrated" because one label printed the re-extraction line:

- **Re-index EVERY label** under the 0.15.0 binary:
  `wicked-estate index <root> --db <f> --repo <label>` for each label.
- **Re-run `wicked-estate scip --repo <label>`** per label, AFTER that label's re-index
  (the forced pass removes the confidence-1.0 SCIP edges file-by-file).
- **Re-inject xedges per label** after its re-index (event-bus / command edges keyed to
  old ids are epoch-dropped, see below).
- Treat any agent-held `--symbol` id from a 0.14.x session as stale, for every label,
  until the whole DB is done.

## What is NOT carried over (documented loss, no guessed re-key)

- **Annotations** on churned ids survive as orphans under the old id.
- **Overlay/memory/knowledge xedges** are epoch-dropped at read — re-inject after
  re-indexing.
- **Embeddings** — re-run `wicked-estate index … --embeddings`.
- **SCIP edges** — re-run `wicked-estate scip <root>` (with `--repo <label>` in a
  labelled DB) after the forced re-extract.

## What IS preserved: injected & synthetic (non-extractor) nodes

The forced full re-extraction — whichever gate fires — **preserves every node the extractor
did not create**: rows injected directly into the store (e.g. wicked-apps-core
event/work-unit/conformance nodes) survive byte-identical, along with their annotations.
**Mechanism:** deletion is strictly file-keyed. Both delete paths — the changed-file rewrite
and the deleted-file sweep — call `remove_file(<path>)` for paths derived from the walk of
current source files and the store's `files`/digest table, and `remove_file` deletes only
rows keyed on that path (it never touches the annotations table). Injected nodes carry
fabricated `location.file` paths (`event/<id>`, `work_unit/<id>`, …) that appear in
neither, so no delete path can reach them.

Empirically proven in the estate-review closure suite
(`estate-review/review-artifacts/closure-suite.json`, `closure[3]`, lane `core-shared-db`,
`pass: true`; RESOLUTION-PROGRAM.md status: "release-checklist #7 CLOSED"): a constructed
DB (tinyrepo + 3 injected apps-core-shaped synthetic rows) was pushed through the forced
`id_scheme` full re-extract and all 3 synthetic rows came out byte-identical, with the
annotation on the synthetic node still live (while the annotation on a re-minted code id
orphaned, as documented above); corroborated on a copy of the real shared DB — all 2,188
wicked-apps synthetic nodes unchanged beside the freshly indexed code nodes. Honesty note:
that proof ran on the integration-era binary (its gate fired v1→v2), not the released
scheme-3 binary; the delete paths it exercises are unchanged in 0.15.0 (`remove_file` got
strictly more conservative since — shared Import nodes now survive owner deletion, #132).

## MCP-only deployments

The MCP server **never migrates on its own** — it serves whatever graph is on disk. An
upgraded `wicked-estate-mcp` binary against a 0.14.x DB serves the old scheme-1 graph
until someone runs `wicked-estate index` against it (once per repo label). Schedule that
index as part of the upgrade, not as a lazy follow-up.

## Downgrade / same-version hazards

- Do **not** run a pre-scheme binary of the same version against a migrated DB — it
  re-mints flat ids for any changed file (it does not read the scheme key, and the
  version gate cannot fire at equal versions). If one did: `wicked-estate index <root>
  --force`.
- Expect Calls-edge counts to drop and unresolved counts to rise on collision-heavy
  files: the removed edges are the 0.65 false-precision merges scheme 3 exists to kill —
  they are now honestly parked as unresolved (see the 0.15.0 CHANGELOG entry and
  `docs/benchmarks/README.md`, "symbol-id scheme 3 re-baseline note").
