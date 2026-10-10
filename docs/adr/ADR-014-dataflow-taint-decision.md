# ADR-014: Does the evidence justify data-flow or taint analysis? (TS-S5)

- **Status:** Proposed. The recommendation is below; the go/no-go is an operator decision (#279).
- **Date:** 2026-10-10
- **Builds on:** TS-S1 (`flows_to` semantics), TS-S2A (the support plane), TS-S2C (#275, the semantic-evidence envelope), TS-S3/S4 (#277/#278, Angular compiler bindings and events).
- **Evidence:** `crates/wicked-estate/tests/dataflow_readiness.rs`, which pins today's behaviour per construct; `crates/wicked-estate/tests/angular_bindings.rs`; ENGINE-CONTRACT §3.2–§3.5.

## Decision to make
Three options, judged per producer tier:
1. **Stop** at exact symbols and references plus evidence-bearing semantic edges, which is what ships today.
2. Add **bounded intraprocedural value-flow summaries**.
3. Add **interprocedural source/sink/sanitizer taint analysis**.

## Readiness matrix (per real producer)

| Producer | Exact refs | Explicit calls | Value flow | Control flow | Framework model |
|---|---|---|---|---|---|
| tree-sitter TypeScript (base plane; TSX/JS share the query family but are **not pinned** by the readiness test) | name-resolved, heuristic tiers | heuristic `Calls`, parsed sites | **partial**, see below | **none** | Angular `@Input`/route convention (Heuristic) |
| scip-typescript 0.4.0 (S2C) | **exact** (`References`) | **none**: SCIP has no call role | none | none | none |
| other SCIP indexers (Java/Kotlin, C#/VB, C/C++, …) | adapter-supported, **integration unverified** (no pinned sample) | none | none | none | none |
| Angular compiler 22.2.2 adapter (S3/S4) | definitions only (classes, handler methods), for correlation | none (event delivery is `event-listens`) | **exact at resolved template endpoints** (inputs, outputs, `$event`); unresolved reads are counted, and transformed or derived values are `may_influence` | none | **compiler-resolved** templates (aliases, inheritance, signals, transforms, host directives) |
| COBOL, PL/SQL, ABAP, RPG | **contract fixtures only** | PL/SQL fixture only | none | none | none |

**What the value-flow layer covers today** (`dataflow_readiness.rs`, reproducible). Rows are checked as semantic-forward **reachability** over qualified value identities, not as single named edges, so an intermediate slot cannot hide a flow:

| Construct | Today | Pinned by |
|---|---|---|
| assignment chains | ✅ present (`src → a → b → return`) | `present_rows_are_reachable` |
| cross-file call argument and result | ✅ present when the call resolves uniquely | `present_rows_are_reachable` |
| closure-captured local | ✅ captured, ❌ not through the arrow's return | `missing_primitives_are_unreachable` |
| destructuring (`const {k} = obj`) | ❌ absent | `missing_primitives_are_unreachable` |
| loops (`for … of`, loop-carried `acc = acc + it`) | ❌ absent | `missing_primitives_are_unreachable` |
| promises and callbacks | ❌ absent | `missing_primitives_are_unreachable` |
| object-literal property writes | ❌ absent (reads are path-keyed slots) | `missing_primitives_are_unreachable` |
| sanitizer-like calls (`clean = sanitize(raw)`) | ⚠️ the argument enters, the result does **not** come back out when the callee returns an expression | `missing_primitives_are_unreachable` |
| branches | ⚠️ **path-insensitive**: `if (flag) out = src` reports `value_preserving`, with no guard and no literal default | `branches_are_path_insensitive` |
| imports | ✅ through resolved calls | `present_rows_are_reachable` |
| Angular inputs, outputs, `$event` | ✅ compiler-exact at the template boundary; the `EventEmitter.emit(x)` producer side is absent | `angular_bindings.rs` |
| stale snapshot replacement | ✅ support-plane laws on every backend | conformance suites |

## Recommendation: **Option 1 now. Option 2 only behind one bounded prototype. Option 3 no-go.**

**Why Option 3 (taint) is no-go, falsifiably.** Taint on this layer would **miss** flows through properties, destructuring, loops, closures and promises. All are unreachable on the readiness fixture, so a "no taint found" answer would be unsound. It would also have no sanitizer model at all: a sanitizer's result does not even flow back, and a flowing result alone would still not establish sanitizer semantics. Path sensitivity is *not* a prerequisite for a conservative taint analysis, but recording no guard means the analysis could not explain a sanitized branch either.

Presenting that as security analysis violates agent rule R7 (a heuristic must never be presented as a proof).

**Falsifier:** the no-go is reopened only on **positive** evidence, not merely by inverting today's absence assertions. That means a source/sink/sanitizer fixture with expected findings and expected non-findings, passing on the readiness fixture and on one real corpus, with the kill criteria below met.

**Why Option 2 is not decided yet.** Intraprocedural summaries (param → return within one callable) would close the sanitizer-result, destructuring and loop rows inside one function. Property writes stay out of scope. Their cost is unknown on the real corpora: persistence size, incremental invalidation, and the response budget. A prototype is justified because it resolves exactly that one named uncertainty, the cost.

**Why Option 1 is honest now.** Exact references (S2C), compiler-exact framework bindings (S3/S4) and evidence-bearing `flows_to` already answer "what can this value reach, by what evidence". They carry `flow_semantics`, `flow_evidence`, confidence and provenance, and they never claim completeness.

## Prohibited claims and non-goals
- No "taint analysis", "security data-flow", "sanitizer verification" or "CodeQL-like" claims, in docs, sites or tool descriptions.
- No claim that any legacy language (COBOL, PL/SQL, ABAP, RPG) is analysable. Those are contract fixtures.
- No claim that a `flows_to` chain is exhaustive or path-sensitive. `value_preserving` means "on some path, whole".
- Non-goals: CFG/SSA construction, alias analysis, an RxJS or async model, and source/sink rule packs.

## Minimum missing primitives (for Option 2)
1. **Callee return composition:** a return of a call-expression result (`return raw.replace(...)`) as a `may_influence` hop from the call's receiver and arguments.
2. Destructuring bindings (`{k} = obj` → `obj.k` property slot → `k`).
3. Loop element binding (`for (x of xs)`: `xs → x`) and loop-carried reassignment.
4. Closure returns through arrow bodies.

Promises, property writes and path conditions are explicitly **out** of Option 2. The prototype evaluates **primitives 1–3**. Primitive 4 (closure returns) is evaluated only if 1–3 graduate. After graduation, these remain unsupported: closures (until primitive 4), promises and callbacks, property writes, path conditions, and aliasing.

## Bounded prototype (only if the operator picks "Option 2 prototype")
- **Scope:** primitives 1–3 only, TypeScript only, as tree-sitter query data plus the existing call-derived pass. No new storage and no new response fields.
- **The one uncertainty it resolves:** whether the edge growth and index time stay within budget.
- **Timebox:** one session for S5b and one for S5c. Anything still failing after that is a kill.
- **Measurement procedure** (recorded in the ADR with the commit SHAs):
  - **Corpora:** estate's own tree at the baseline commit, plus the 905-file TypeScript corpus pinned by its revision, which is not on this host and has to be supplied.
  - **Baseline:** the same commit without the change.
  - **Runs:** each run is `wicked-estate index --force` into a fresh DB; take the median of 3 for timings.
  - **Edges:** the `flows_to` count from `stats --json`; the denominator is the baseline's `flows_to` count.
  - **Persistence:** DB file size.
  - **Incremental:** the re-index time after touching one file.
  - **Lineage:** a fixed set of 10 `lineage --relation flows_to` queries (listed in the ADR), recording each document's size and `truncated` flag.
- **Acceptance metrics:**
  - `flows_to` growth ≤ 25 %; full-index time ≤ +15 %; incremental re-index ≤ +15 %; DB size ≤ +10 %;
  - no Lineage query newly truncated at the default depth;
  - 0 new `value_preserving` edges from a `may_influence` construct. The oracle is the readiness fixture's **expected table**, extended with the prototype's constructs and their expected semantics, so "wrong" means "differs from the table", not a reviewer's opinion.
- **Kill criteria:** any metric missed, any readiness-table mismatch, or the timebox exceeded. The prototype is then deleted, not flagged off (CLAUDE.md §3, §8), and the measured numbers are recorded here.

## Consequences
- **Option 1:** no API, storage, budget or semver change. The readiness test becomes the regression guard: a row cannot flip silently.
- **Option 2, if it goes ahead later:** additive `flows_to` edges with existing metadata, and a `SYMBOL_ID_SCHEME` bump only if new slot kinds are minted. That forces a re-extract, which is automatic. A minor version on 0.x. It touches no support-plane rows, and producers keep their own owners.
- **Option 3:** would need an analysis-snapshot owner under the S2A plane, with retraction and erasure semantics, a rule-pack format, a path-condition model and a new response envelope. That is a multi-wave product, not a slice, and it is not authorized here.

## Staged plan (each slice fits one session)
- **S5a (this ADR):** the decision and the readiness test.
- **S5b (only if Option 2 is chosen):** primitive 1 (callee return composition), with the readiness row flipped and metrics recorded.
- **S5c:** primitives 2–3, with metrics re-measured against the kill criteria.
- **S5d:** decide whether Option 2 graduates, or is deleted. The recorded graduation evidence is the metrics table, the readiness diff and the corpus revisions.

## Next-session prompt
> Read docs/adr/ADR-014-dataflow-taint-decision.md and the operator's decision on #279. If the operator chose "Option 2 prototype", implement S5b only: make `return <call-expression>` contribute `may_influence` flows from the call's receiver and arguments into `<fn>.return` in `crates/wicked-estate-extract/src/queries/typescript.scm` (data, no per-language Rust), flip the `sanitize.return → clean` row in `crates/wicked-estate/tests/dataflow_readiness.rs`, and measure it with ADR-014's procedure on estate's own graph and on the pinned 905-file TypeScript corpus. If any metric fails or the timebox is exceeded, delete the change and record the numbers in the ADR. If the operator chose Option 1, set ADR-014's status to Accepted (Option 1), record the decision with its date and a link to it, and close #279. Change nothing else.
