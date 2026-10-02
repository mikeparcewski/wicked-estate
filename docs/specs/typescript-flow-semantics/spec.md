# Spec: TypeScript flow semantics and synthetic-value containment (TS-S1)

- **Status:** Shipped
- **Owner:** eu.gene.lim
- **Amends:** [`docs/specs/typescript-value-lineage/spec.md`](../typescript-value-lineage/spec.md) (PR #207, `3257648`)
- **Constrained by:** [`docs/ENGINE-CONTRACT.md`](../../ENGINE-CONTRACT.md) §3.2/§3.3, [`docs/adr/ADR-002-stable-symbol-identity.md`](../../adr/ADR-002-stable-symbol-identity.md), [`docs/agent-behavior-rules.md`](../../agent-behavior-rules.md) R4/R7
- **Shape:** data

## Outcome

A caller reading a `flows_to` edge can tell what it *claims* (value-preserving vs. may-influence)
apart from how we *know* it (syntax / call-derived / framework convention), with honest confidence
on each; and synthetic value slots are contained to the surfaces that asked for them, under one
published predicate and one published matrix.

## What Changes

- `flows_to` keeps its tag and its default behaviour. Its classification becomes set-valued,
  machine-readable metadata (`flow_semantics`, `flow_evidence`, `constructs`, `flow_rules`,
  `flow_support`), declared in query files as `@flow.<semantics>.<evidence>.<construct>`.
- Framework-convention matches (Angular `@Input()`, `route.snapshot.paramMap.get(…)`) drop from
  `Parsed`/1.0 to `Heuristic`/0.5 with `resolved_by = tree-sitter-convention` and a stable rule id.
- Colliding flow facts merge through a deterministic, insertion-order-independent lattice instead
  of being silently overwritten by the stores' `>=` upsert.
- `Lineage relation=flows_to` returns a per-hop `flows` array carrying that evidence (R7).
- One predicate, `is_structural_symbol`, gates every structural surface; the per-consumer decision
  for all of them is published as a matrix.

## Agent Rules

### Always do

- Keep `source = dependent/consumer`, `target = dependency/producer`; forward lineage walks
  `Direction::Dependents`.
- Inherit the causal `Calls` edge's confidence/provenance/`resolved_by` on call-derived flow.
- Declare a construct's classification in the `.scm` capture name, never in a `match language` arm.

### Never do

- Emit `scip` or `compiler` evidence before TS-S2 / TS-S3 land it.
- Present a convention match as a compiler-proven or `@angular/core`-resolved fact.
- Build CFG, SSA, alias/heap modelling, path sensitivity, or taint.
- Blanket-filter raw export, `stats`, or the PageRank input graph to hide a synthetic node.

## Acceptance Criteria

- [x] **AC-S1-01.** Flow semantics and evidence origin are separately readable on every `flows_to`
  edge, with reserved `scip`/`compiler` vocabulary present in the type and emitted by nothing.
- [x] **AC-S1-02.** Two facts sharing `(source, target, kind)` but differing in class, site or
  provenance merge with no loss and no insertion-order dependence; the weakest contributing
  confidence stays visible.
- [x] **AC-S1-03.** A contribution to a binary expression is `may_influence`; an assignment,
  property read, return, call argument and call result are `value_preserving`.
- [x] **AC-S1-04.** Angular convention edges carry `Heuristic`/0.5, `tree-sitter-convention`, and a
  stable `flow_rules` id; non-Angular direct syntax flow stays `Parsed`/1.0 `tree-sitter`.
- [x] **AC-S1-05.** Call-derived flow equals its causal `Calls` edge's confidence, provenance and
  `resolved_by`; uniqueness never upgrades it.
- [x] **AC-S1-06.** Every entry of the §3.3 visibility matrix behaves as published, including the
  rows that deliberately keep value slots visible.
- [x] **AC-S1-07.** Explicit `flows_to` traversal still returns value nodes and now renders their
  per-hop evidence; exact-`SymbolId` lookup still retrieves one.
- [x] **AC-S1-08.** The `Calls` set and the structural graph are unchanged for the same fixture,
  and full vs. incremental indexes of one tree still agree.
- [x] **AC-S1-09.** Every PR #207 regression invariant still holds.
- [x] **AC-S1-10.** Workspace build (0 warnings), tests, clippy `-D warnings`, fmt, and GraphStore
  conformance pass.

## Rollout

No flag, no schema migration. Persisted edges written by an earlier binary keep the former
classification until their file is re-extracted: a release-version bump forces a full re-extract on
the next `index`; a same-version development binary needs `wicked-estate index <path> --force`
once. The `pagerank.top` cache is additionally cleaned at read time, because it outlives a code fix.

## Risks

- The convention downgrade changes a published confidence. Mitigated by versioning the assertions
  rather than deleting them, and by the §3.2 contract text.
- `merge_flow_edges` folds one batch only. A cross-batch equal-confidence collision would still be
  lossy; the reachable pairing is audited endpoint-disjoint in §3.2, and the general case is
  TS-S2A's seam.
- `BlastRadius`/`TraverseGraph` still surface value slots via `File`→value `Contains`. Narrowing
  the all-edge-kind blast-radius contract is an explicit decision, deliberately not taken here.

## Changelog

- 2026-10-01: TS-S1 implemented against base `c4fa938`.
