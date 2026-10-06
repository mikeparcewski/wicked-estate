# Spec: authoritative fact-support replacement (TS-S2A)

- **Status:** Implementing
- **Owner:** eu.gene.lim
- **Builds on:** [`docs/specs/typescript-flow-semantics/spec.md`](../typescript-flow-semantics/spec.md) (TS-S1) and TS-S1B (`a0ce2f9`); rebased onto release 0.20.0 (`925adec`)
- **Constrained by:** [`docs/ENGINE-CONTRACT.md`](../../ENGINE-CONTRACT.md) §3.2/§3.4/§4, [`docs/adr/ADR-002-stable-symbol-identity.md`](../../adr/ADR-002-stable-symbol-identity.md), [`docs/adr/ADR-003-storage-backends.md`](../../adr/ADR-003-storage-backends.md), [`docs/agent-behavior-rules.md`](../../agent-behavior-rules.md) R4/R7
- **Shape:** storage contract

## Outcome

A producer that re-emits a snapshot can make that snapshot's facts its complete support set, and
every fact it stopped asserting disappears from the public edge. Other producers and the base
plane are untouched. Identity comes from authoritative rows, never from the bounded
`flow_support` sample.

## What Changes

- `wicked_estate_core::support` adds `SupportFact` (the opaque, producer-owned `fact_id` + the
  edge it asserts — the boundary later language adapters build against), `SupportOwner`,
  `SupportOwnerState`, `EdgeSupport`, `SupportReplacement`, the shared planner
  (`normalize_facts`, `plan_replacement`), and the projection (`project_edge`).
- `wicked-estate supports owners | edge | retract` — the CLI face of the same trait methods (a
  bespoke arm; the RetrievalTool bridge is untouched).
- `GraphWrite::replace_edge_supports` and `GraphRead::{edge_supports, support_generation,
  support_owners}` are required trait methods, implemented by MemStore, SqliteStore, PostgresStore and SurrealStore
  and delegated by every wrapper (`OverlayReader`, `OverlayMemStore`, test doubles).
- `upsert_edges`, `remove_file`, `prune_dangling_edges` and `remove_nodes` honour the support
  plane on every store. With no support stored, each takes its original branch.
- Additive tables on SQLite/Postgres/Surreal. No edge row is rewritten by migration.

The contract text is `docs/ENGINE-CONTRACT.md` §3.4.

## Agent Rules

### Always do

- Decide every replacement with `plan_replacement`: generation and idempotence rules live in one
  place, never per store.
- Re-project from authoritative rows (base contribution + every owner's facts), never from the
  previously projected edge.
- Retire a base contribution by exactly the predicate the same operation applies to edges.

### Never do

- Derive support identity, counts or a representative from `flow_support`.
- Let one owner's write modify another owner's facts or generation.
- Emit `scip`/`compiler` evidence or wire a producer here (TS-S2/TS-S3 do).

## Acceptance Criteria

Every criterion is driven by `conformance::support_replacement_suite`, run on MemStore,
SqliteStore (with and without edge history), SurrealStore and PostgresStore, unless a
backend-specific test is named.

- [x] **AC-S2A-01.** Identical replay (any order, duplicates) is a no-op that reports `replayed`.
- [x] **AC-S2A-02.** `{a,b}` → `{b,c}` retracts only `a`, keeps `b` exactly once.
- [x] **AC-S2A-03.** An empty replacement retracts all the owner's support, keeps its generation, and a
  stale replay cannot resurrect it.
- [x] **AC-S2A-04.** Two producers supporting one edge stay independent.
- [x] **AC-S2A-05.** Invalid-fact, generation-conflict and stale-generation failures change nothing,
  inside and outside a batch. A storage failure in the middle of a write rolls back too
  (`sqlite::tests::support_replacement_rolls_back_a_mid_write_storage_failure`, trigger-injected).
- [x] **AC-S2A-06.** Input and owner order do not affect stored or public results.
- [x] **AC-S2A-07.** Staged and one-shot replacement agree: exactly for history, and only up to the cap
  for pre-merged input. The past-cap over-count is pinned as the boundary.
- [x] **AC-S2A-08.** Counts, extrema, representative, cap and truncation are stable across repeated folds
  and join-then-retract cycles.
- [x] **AC-S2A-09.** Evicting a fact from the sample cannot change identity or a later representative.
- [x] **AC-S2A-10.** The base plane coexists: base writes never erase support; `remove_file`/`prune`
  never delete support; retracting the last support restores the base edge exactly.
- [x] **AC-S2A-11.** A pre-TS-S2A SQLite database migrates with every stored edge byte-identical and the
  complete pre-existing envelope intact
  (`sqlite::tests::pre_support_database_migrates_with_the_public_envelope_intact`).
- [x] **AC-S2A-12.** Erasure removes support naming an erased symbol (`sqlite::tests::remove_nodes_erases_support_rows`).
- [x] **AC-S2A-15.** Fact identity is producer-owned and opaque: ids differing only by whitespace,
  case or Unicode normalization are distinct and returned exactly; identical content under two
  ids is two facts; a changed fact under one id is retract + assert; one id with two contents is
  rejected; two toolchains sharing the display name `User` and the fact id `User` never collide
  (suite §15–§16, `support::tests::fact_ids_are_opaque`). `support_owners` is ordered and lists
  emptied owners (§17).
- [x] **AC-S2A-16.** CLI parity: `supports owners|edge|retract --json` equals the store's answer;
  output fits the one R4 budget; strict argv fails before any store opens; a missing or
  zero-length graph (bare path or `sqlite://`) is refused by every subcommand and never created
  (`crates/wicked-estate/tests/supports_cli.rs`, hermetic: inherited `WICKED_*`/`OTEL_*`/`GIT_*`
  cleared, no global git config).
- [x] **AC-S2A-14.** Representative ties are decided by the fact set (suite §11); `remove_file`'s
  source-in-file predicate and `prune` retire base contributions (§12, §13); support never keeps
  a shared Import node alive (§14); concurrent Postgres replacements serialize
  (`postgres_concurrent_support_replacements_serialize`).
- [x] **AC-S2A-13.** New public types are `#[non_exhaustive]` (`compile_fail` doctest) and every
  serialized field is pinned (`support::tests::public_types_serialize_every_field`).

## Rollout

No flag. The schema change is additive and created on open. A graph with no support behaves
exactly as before. Downgrade after support was written: retract it first, because an older
binary treats a projected edge as a plain edge.

## Risks

- `GraphRead`/`GraphWrite` gain required methods, which breaks out-of-tree implementors (semver:
  minor bump on 0.x, recorded under **Changed (breaking)**).
- A supported edge whose endpoint node is gone stays visible until its owner retracts it. This is
  deliberate (only the producer may retract), but a stale producer leaves dangling edges that
  blast radius follows; `wicked-estate supports owners` / `retract` is the operator's recourse.
- Postgres: two batches that each took the shared support lock and then both request the
  exclusive one deadlock; Postgres aborts one with an error. Unbatched base-plane writes are now
  covered (each runs in its own transaction).
- With history enabled, `remove_file` archives the projected edge of a supported key it deletes
  even though the heal re-projects it (history records the file's version at removal), and a
  base contribution retired without its projected row being deleted is not archived.
- Cost baseline (release build, on-disk SQLite, 100K `flows_to` facts, one owner): first
  insert 7.3 s; identical replay 2.2 s; same set at the next generation 2.0 s; retracting 1,000
  2.3 s. The ~2 s floor is planning: re-validating the incoming set and re-deriving the stored
  set's content from its JSON. A stored content-digest column would remove the second half if
  a producer's scale makes it matter.
- `supports retract` writes without a confirmation prompt, like the other CLI write commands
  (`annotate`); it refuses an unknown owner.

**Resolved in this change:** injected mid-write failures now prove atomicity on SQLite (trigger),
Postgres (trigger, inside and outside a batch) and SurrealStore (field `ASSERT` inside the
`BEGIN … COMMIT`), each with a falsifier (removing the savepoint/transaction turns it red). The
SurrealStore predicate-DELETE defect was audited beyond `edge_base`: the pre-existing `edge_rel`
delete in `remove_file` had the same silent no-op for location-less edges sourced from the
removed file; it now selects and deletes by key, and `graph_store_suite` pins both predicate
halves for every store.

## Changelog

- 2026-10-03: TS-S2A implemented against base `c250fef`.
- 2026-10-04: rebased onto 0.20.0 (`925adec`); opaque producer-owned `fact_id`, `support_owners`,
  and the `supports` CLI added.
- 2026-10-04: risk pass — Postgres/Surreal in-transaction failure tests, Postgres implicit
  transaction for unbatched base writes, SurrealStore `remove_file` source-predicate fix, cost
  baseline.
