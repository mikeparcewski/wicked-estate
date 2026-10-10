# ADR-014: Does the evidence justify data-flow or taint analysis? (TS-S5)

- **Status:** **Accepted: Option 2** (bounded intraprocedural value-flow summaries), decided by the operator on 2026-10-10 (#279). Option 3 stays no-go; its positive-evidence reopening criterion is unchanged.
- **Operator ruling (verbatim):** "data flow - evidence is important".
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
| closure-captured local | ✅ captured; ✅ through a returned no-parameter, single-identifier arrow (`return () => captured`, `may_influence`, S5b); every other closure shape ❌ | `s5b_return_composition` |
| destructuring (`const {k} = obj`) | ✅ flat patterns over an identifier (shorthand, renamed, defaulted, rest, array elements), `may_influence` (S5c); nested patterns and member/call sources ❌ | `s5c_destructuring` |
| loops (`for … of`, loop-carried `acc = acc + it`) | ❌ absent | `missing_primitives_are_unreachable` |
| promises and callbacks | ❌ absent | `missing_primitives_are_unreachable` |
| object-literal property writes | ❌ absent (reads are path-keyed slots) | `missing_primitives_are_unreachable` |
| sanitizer-like calls (`clean = sanitize(raw)`) | ✅ the argument enters, and the result comes back out (S5b): `return raw.replace(..)` is a `may_influence` hop from the identifier receiver and arguments. A chained receiver (`raw.trim().x()`) is ❌. This is value flow, **not** a sanitizer model | `s5b_return_composition` |
| branches | ⚠️ **path-insensitive**: `if (flag) out = src` reports `value_preserving`, with no guard and no literal default | `branches_are_path_insensitive` |
| imports | ✅ through resolved calls | `present_rows_are_reachable` |
| Angular inputs, outputs, `$event` | ✅ compiler-exact at the template boundary; the `EventEmitter.emit(x)` producer side is absent | `angular_bindings.rs` |
| stale snapshot replacement | ✅ support-plane laws on every backend | conformance suites |

## Decision (2026-10-10)
The operator ruled "data flow - evidence is important" on #279. That selects **Option 2**: primitives 1–3 are built as the bounded slices S5b–S5d below, and each is measured against this ADR's acceptance metrics and kill criteria. Evidence decides. A slice that misses any gate is deleted, not flagged off, and its numbers are recorded here. **Option 3 (interprocedural taint) stays no-go.** It reopens only on the positive evidence named under "Falsifier". The prohibited claims below still apply in full: Option 2 adds value-flow summaries. It does not add taint analysis, sanitizer verification or a completeness claim.

**Corpus substitution.** The 905-file TypeScript corpus named in the procedure is not identified anywhere in this repository (the CHANGELOG cites only "a 905-file TypeScript repo"), and it is not on the build host. The measurement therefore uses a pinned **public** Angular corpus of comparable size: `Teradata/covalent` at `438c297e399dd9cae6243f0955d78f04c9875c21` (860 non-declaration `.ts` files). The baseline and each candidate are measured on the same machine and corpus revision. Results are recorded under "Measurements" as each slice lands.

## Measurements
Procedure: `scripts/measure-dataflow.py` (its `--self-test` pins the semantics gate), run by a scratch-branch workflow on one GitHub-hosted `ubuntu-latest` runner. Both binaries are built there with the shipped release profile (LTO, one codegen unit) into one target dir, then measured interleaved, base then candidate, median of 3. The estate-tree corpus is `git archive` of the baseline commit. The 10 fixed Lineage queries are the baseline graph's 10 largest `flows_to` producers by out-degree (ids recorded in each run's artifact). The sweep runs one Lineage query per baseline `Parameter` slot. All queries use the default depth.

### S5b: callee return composition (+ single-identifier returned arrow)
Baseline `2b6bf7f` (main). Candidate `2c14000` (code-identical to the merged slice; the rebase only added this ADR's text). Workflow run `38065211483`, branch `measure/ts-s5b`.

| metric | Covalent `438c297` (860 `.ts`) | estate tree `2b6bf7f` | gate |
|---|---|---|---|
| `flows_to` edges | 1394 → 1477 (**+5.95 %**) | 107 → 116 (+8.41 %) | ≤ +25 % |
| full index, median of 3 | 7.15 s → 7.22 s (+1.05 %) | 5.53 s → 5.45 s (−1.27 %) | ≤ +15 % |
| incremental re-index, median of 3 | 0.96 s → 0.94 s (−2.10 %) | 2.09 s → 2.05 s (−2.06 %) | ≤ +15 % |
| DB size | 79,826,944 → 80,138,240 B (+0.39 %) | 55,271,424 → 55,087,104 B (−0.33 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 (2 were truncated in both) | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 (1 truncated in both) | 0 of 97 | 0 |
| `value_preserving` rows from a new construct | 0 (71 `return_call` rows, all `may_influence`) | 0 (6 `return_call`, 1 `return_closure`) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S5b passes every gate.** `return_closure` never fired on Covalent; its only firing in either corpus is the readiness fixture in estate's own tree. The readiness table diff is in `dataflow_readiness.rs`: `s5b_return_composition`, `every_edge_matches_the_expected_semantics_table`, and `returns_of_undefined_callables_write_no_slot`. The last is a pre-existing false return attribution (a destructured arrow or a private arrow field), reproduced with the 0.24.0 extractor and fixed in this slice.

### S5c: destructuring
Baseline `4470b52` (the S5b slice's code, = main `b9a0348`). Candidate `0b5d2d3` (code-identical to this slice after its rebase). Workflow run `38066120495`, branch `measure/ts-s5c`.

| metric | Covalent `438c297` | estate tree `4470b52` | gate |
|---|---|---|---|
| `flows_to` edges | 1477 → 1496 (**+1.29 %**) | 123 → 124 (+0.81 %) | ≤ +25 % |
| full index, median of 3 | 7.29 s → 7.16 s (−1.73 %) | 5.48 s → 5.48 s (−0.06 %) | ≤ +15 % |
| incremental re-index, median of 3 | 0.94 s → 0.95 s (+0.97 %) | 2.04 s → 2.03 s (−0.57 %) | ≤ +15 % |
| DB size | 80,158,720 → 80,220,160 B (+0.08 %) | 55,480,320 → 55,357,440 B (−0.22 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 | 0 of 109 | 0 |
| `value_preserving` rows from a new construct | 0 (19 `destructuring` rows, all `may_influence`) | 0 (1) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S5c passes every gate.** Readiness diff: `s5c_destructuring`, plus the `destructuring` row of the semantics table.

## Original recommendation (superseded by the decision above): Option 1 now, Option 2 only behind one bounded prototype, Option 3 no-go

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

Promises, property writes and path conditions are explicitly **out** of Option 2. The prototype evaluates **primitives 1–3**, plus one narrow case of primitive 4 that the program pulled forward into S5b: a returned arrow whose body is a single identifier (`return () => captured`), as `may_influence`, measured under the same gates. The rest of primitive 4 is evaluated only if 1–3 graduate. After graduation, these remain unsupported: every other closure shape (until primitive 4), promises and callbacks, property writes, path conditions, and aliasing.

## Bounded prototype (selected: Option 2)
- **Scope:** primitives 1–3 (plus the single-identifier returned arrow above), TypeScript only, as tree-sitter query data plus the existing call-derived pass. No new storage and no new response fields.
- **The one uncertainty it resolves:** whether the edge growth and index time stay within budget.
- **Timebox:** one session each for S5b, S5c and S5d. A slice still failing after its session is a kill: it is deleted and its numbers are recorded here.
- **Measurement procedure** (recorded in the ADR with the commit SHAs):
  - **Corpora:** estate's own tree at the baseline commit, plus a pinned TypeScript corpus. The original 905-file corpus could not be identified (see "Corpus substitution"), so the pinned public substitute `Teradata/covalent@438c297e399dd9cae6243f0955d78f04c9875c21` stands in for it. Its numbers are reported as Covalent numbers, never as numbers for the original corpus.
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
- **Kill criteria:** any metric missed, any readiness-table mismatch, or the timebox exceeded. The failing **slice** (S5b, S5c or S5d, with everything that slice added) is then deleted, not flagged off (CLAUDE.md §3, §8), and the measured numbers are recorded here. Slices that already passed their own gates stay. Each later slice is measured against the merged state before it, so its numbers are its own.

## Consequences
- **Option 1:** no API, storage, budget or semver change. The readiness test becomes the regression guard: a row cannot flip silently.
- **Option 2, if it goes ahead later:** additive `flows_to` edges with existing metadata, and a `SYMBOL_ID_SCHEME` bump only if new slot kinds are minted. That forces a re-extract, which is automatic. A minor version on 0.x. It touches no support-plane rows, and producers keep their own owners.
- **Option 3:** would need an analysis-snapshot owner under the S2A plane, with retraction and erasure semantics, a rule-pack format, a path-condition model and a new response envelope. That is a multi-wave product, not a slice, and it is not authorized here.

## Staged plan (each slice fits one session)
- **S5a (this ADR):** the decision and the readiness test.
- **S5b:** primitive 1 (callee return composition), with the readiness row flipped and metrics recorded. It also carries the single-identifier returned arrow pulled forward from primitive 4 (see "Minimum missing primitives"). Every other closure shape stays out.
- **S5c:** primitive 2 (destructuring), measured against the kill criteria.
- **S5d:** primitive 3 (loop element binding and loop-carried reassignment), measured against the kill criteria. Each slice lands as its own PR, so a gate miss deletes exactly that slice (see "Kill criteria"). Graduation evidence is the metrics table, the readiness diff and the corpus revisions.

## Next-session prompt
> Read docs/adr/ADR-014-dataflow-taint-decision.md (Accepted: Option 2) and its "Measurements" section. Implement the next unlanded slice of S5b–S5d only, as TypeScript query data in `crates/wicked-estate-extract/src/queries/typescript.scm` plus the smallest engine change it needs. Flip its row in `crates/wicked-estate/tests/dataflow_readiness.rs` into a positive test with expected flows, expected non-flows and the expected semantics of every edge: `value_preserving` only for a whole-value transfer, `may_influence` for a contribution or transformation. Every summary stays path-insensitive whatever its semantics. Then measure it with this ADR's procedure against the baseline commit on both corpora (estate's own tree and the pinned Covalent substitute). A gate miss deletes the slice, and its numbers are recorded here either way.
