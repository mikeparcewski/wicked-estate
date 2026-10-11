# ADR-014: Does the evidence justify data-flow or taint analysis? (TS-S5)

- **Status:** **Accepted: Option 2** (bounded intraprocedural value-flow summaries), decided by the operator on 2026-10-10 (#279). Option 3 stays no-go; its positive-evidence reopening criterion is unchanged.
- **Operator ruling (verbatim):** "data flow - evidence is important".
- **Extended 2026-10-10 (S6):** after 0.25.0 the operator read its limits as "makes this sound like a useless feature" (relayed by the program brief): most TypeScript data moves through callbacks, `await` and properties. S6 closes those gaps **inside** the same bounded intraprocedural Option 2, under the same gates and kill rule. See "Extension S6".
- **Ruled 2026-10-11 (#279), "I want the full graph":** keep every data-flow slice (S5b, S5c, S5d, S6a, S6b, S6d); Lineage returns the complete graph through paging, with nothing silently cut off. The "0 newly truncated Lineage" gate becomes "every Lineage query is completely retrievable", the DB-size gate becomes cost per stored edge (vite's +14.7 % DB for +66 % `flows_to` is accepted by the operator), and the speed gates are unchanged. Addendum (verbatim): "only things we should truncate is dead end nodes that have no value - but has to be hard proof of that". See "The full graph (2026-10-11)".
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
| closure-captured local | ✅ captured; ✅ through a RETURNED arrow or function expression (S6d, primitive 4; S5b carried only `return () => captured`): the captured values among its return contributors (expression body or a direct block `return`, the `callable_return_contributors` shapes), `may_influence`. A binding declared inside the closure (its parameters, its locals, a nested callback's parameters, an inner function or class name) contributes nothing, so a captured value copied into a closure local first is lost (conservative). A closure bound to a const and then returned ❌ | `s5b_return_composition`, `s6d_closure_returns` |
| destructuring (`const {k} = obj`) | ✅ flat patterns over an identifier (shorthand, renamed, defaulted, rest, array elements), `may_influence` (S5c); nested patterns and member/call sources ❌ | `s5c_destructuring` |
| loops (`for … of`, loop-carried `acc = acc + it`) | ✅ `for…of` over an identifier binds each element (identifier, flat array or shorthand object pattern); `acc = acc + it` and `acc += it` combine their identifier operands; all `may_influence` (S5d). `for…in` keys ❌ by design | `s5d_loop_binding` |
| inline array callbacks (`map`/`forEach`/`filter`/`find`/`some`/`every`/`flatMap`/`reduce`) | ✅ an inline arrow or function expression passed first: its first parameter binds one element of an identifier or `this.field` receiver (`reduce`: the second, its first seeded by an identifier initial value); its return (expression body or a direct block `return`) becomes the result of `map`/`flatMap`/`reduce`; `filter`/`find` select the receiver's elements. All `may_influence` (S6a). A named callback ❌ (interprocedural); `some`/`every`/`forEach` results carry nothing | `s6a_inline_callbacks` |
| `await` and `.then` | ✅ `const r = await p` / `return await p`; `await f(x)` composes from the call's identifier receiver and arguments (like `return_call`), and a uniquely resolved callee's return reaches the awaited binding (`await_result`, engine). `p.then(v => …)` binds the promise (an identifier, `this.field`, a composed call, or a chained inline `.then`'s callback return) to `v`, and the callback's return to the chain result; a resolved callee reaches `v` as `then_result`. All `may_influence` (S6b); `then_*` are `convention` evidence (the name proves no Promise), `then_result` capped at Heuristic confidence. A `.then` call no longer composes from its receiver in `await_call` or S5b's `return_call` (that bypassed the callback). Named callbacks, `.catch` and the rejection handler ❌ | `s6b_await_and_then` |
| object-literal property writes | ❌ absent (reads are path-keyed slots). S6c (local object properties) was built, measured and **deleted** under the kill rule: 0 edges on both corpora (see "S6c: local object properties") | `missing_primitives_are_unreachable` |
| sanitizer-like calls (`clean = sanitize(raw)`) | ✅ the argument enters, and the result comes back out (S5b): `return raw.replace(..)` is a `may_influence` hop from the identifier receiver and arguments. A chained receiver (`raw.trim().x()`) is ❌. This is value flow, **not** a sanitizer model | `s5b_return_composition` |
| branches | ⚠️ **path-insensitive**: `if (flag) out = src` reports `value_preserving`, with no guard and no literal default | `branches_are_path_insensitive` |
| imports | ✅ through resolved calls | `present_rows_are_reachable` |
| Angular inputs, outputs, `$event` | ✅ compiler-exact at the template boundary; the `EventEmitter.emit(x)` producer side is absent | `angular_bindings.rs` |
| stale snapshot replacement | ✅ support-plane laws on every backend | conformance suites |

## Decision (2026-10-10)
The operator ruled "data flow - evidence is important" on #279. That selects **Option 2**: primitives 1–3 are built as the bounded slices S5b–S5d below, and each is measured against this ADR's acceptance metrics and kill criteria. Evidence decides. A slice that misses any gate is deleted, not flagged off, and its numbers are recorded here. **Option 3 (interprocedural taint) stays no-go.** It reopens only on the positive evidence named under "Falsifier". The prohibited claims below still apply in full: Option 2 adds value-flow summaries. It does not add taint analysis, sanitizer verification or a completeness claim.

**Corpus substitution.** The 905-file TypeScript corpus named in the procedure is not identified anywhere in this repository (the CHANGELOG cites only "a 905-file TypeScript repo"), and it is not on the build host. The measurement therefore uses a pinned **public** Angular corpus of comparable size: `Teradata/covalent` at `438c297e399dd9cae6243f0955d78f04c9875c21` (860 non-declaration `.ts` files). The baseline and each candidate are measured on the same machine and corpus revision. Results are recorded under "Measurements" as each slice lands.

## Extension S6 (2026-10-10): callbacks, `await`, local properties, closures
The program brief extends Option 2 to the constructs most TypeScript data actually moves through, **within one function**: no taint, no interprocedural heap and no alias model. Each is one slice and one PR, measured against the merged state before it under the gates and kill criteria below; a miss deletes the slice.
- **S6a, inline array callbacks:** an inline arrow or function expression passed to `map`/`forEach`/`filter`/`find`/`some`/`every`/`flatMap`/`reduce` binds the receiver's element to its parameter, and its return to the result where the method returns it. A named callback stays out: binding its parameters would be interprocedural.
- **S6b, `await` and `.then`:** `const r = await f(x)` composes like return composition; `p.then(v => …)` binds the promise's source to `v`, and the callback's return to the chain result.
- **S6c, local object properties:** `o.k = raw` or `{k: raw}`, then `o.k`, for a local `const`/`let` object of one function. If the object escapes or is reassigned, no edge. **Deleted 2026-10-10** (zero yield on both corpora; see its measurement section).
- **S6d, closure returns** beyond `return () => x` (the rest of primitive 4).

Every new edge is `may_influence`, and the semantics table in `dataflow_readiness.rs` grows with each construct. Promises, properties and callbacks therefore leave the Option 2 exclusions below to the extent each slice graduates; path conditions, aliasing and everything interprocedural stay out.

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

### S5d: loop element binding and loop-carried reassignment
Baseline `0b5d2d3` (the S5c slice's code, = main `7c6d876`). Candidate `26c6de2` (code-identical to this slice after its rebase). Workflow run `38066772500`, branch `measure/ts-s5d`.

| metric | Covalent `438c297` | estate tree `0b5d2d3` | gate |
|---|---|---|---|
| `flows_to` edges | 1496 → 1502 (**+0.40 %**) | 138 → 140 (+1.45 %) | ≤ +25 % |
| full index, median of 3 | 7.33 s → 7.20 s (−1.70 %) | 5.56 s → 5.52 s (−0.60 %) | ≤ +15 % |
| incremental re-index, median of 3 | 0.96 s → 0.98 s (+2.70 %) | 2.06 s → 2.08 s (+0.85 %) | ≤ +15 % |
| DB size | 80,199,680 → 80,269,312 B (+0.09 %) | 55,439,360 → 55,439,360 B (0.00 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 | 0 of 115 | 0 |
| `value_preserving` rows from a new construct | 0 (4 `loop_element`, 2 `augmented_assignment`) | 0 (1 `loop_element`, 1 `reassignment_expression`) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S5d passes every gate.** Its yield on Covalent is small (6 edges). That Angular code iterates mostly through `forEach`/`map` callbacks, which stay out of Option 2 with every other callback. `reassignment_expression` never fired on Covalent.

### S6a: inline array callbacks
Baseline `490c547` (main = 0.25.0). Workflow branch `measure/ts-s6a`.

**First candidate `e0c59ee` (run `38078353963`): missed the incremental gate on Covalent**, 0.70 s → 0.83 s (**+18.91 %**, all three runs 0.82–0.84 s against 0.70–0.72 s). Full index was flat (−0.90 %), so the cost was fixed per process, not per file: it encoded the callback-return shapes as six nested copies of one alternation, and that query is costlier to compile. Its numbers are kept here, per the kill rule. Within the slice's session, the shapes moved into one Rust walk (`callable_return_contributors`, captured once as `@flow.producer.callable_return`) with the same expected table. The re-measurement:

That re-measurement (candidate `4d689c3`, run `38078966290`) passed: incremental +7.06 %. The review fixes that followed (convention evidence, a named function expression's own name, a commented `return`, the `reduce` parameter shapes) were measured again on the code that lands:

Candidate `b4660ba`, run `38079969140`.

| metric | Covalent `438c297` | estate tree `490c547` | gate |
|---|---|---|---|
| `flows_to` edges | 1502 → 1589 (**+5.79 %**) | 150 → 152 (+1.33 %) | ≤ +25 % |
| full index, median of 3 | 7.63 s → 7.76 s (+1.61 %) | 5.50 s → 5.61 s (+1.94 %) | ≤ +15 % |
| incremental re-index, median of 3 | 1.00 s → 1.07 s (+7.20 %) | 2.10 s → 2.18 s (+4.14 %) | ≤ +15 % |
| DB size | 80,142,336 → 80,785,408 B (+0.80 %) | 55,558,144 → 55,660,544 B (+0.18 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 (1 truncated in both) | 0 of 120 | 0 |
| `value_preserving` rows from a new construct | 0 (70 `callback_element`, 9 `callback_return`, 7 `callback_select`) | 0 (2 `callback_element`) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S6a passes every gate.** `callback_accumulator` (the `reduce` seed) never fired on either corpus; its only firing is the readiness fixture. The slice adds 87 Covalent edges in one slice, against 108 for S5b–S5d together. The incremental cost is still mostly fixed per process (+0.07 s), so later slices are watched for the same effect. The `callback_*` constructs are `convention` evidence (Heuristic tier): a method name does not prove an array receiver.

### S6b: `await` and `.then`
Baseline `b4660ba` (the S6a code, = main `1cfa3e3`). Candidate `2d0de1a` (code-identical to this slice after its rebase). Workflow run `38081602880`, branch `measure/ts-s6b`.

| metric | Covalent `438c297` | estate tree `b4660ba` | gate |
|---|---|---|---|
| `flows_to` edges | 1589 → 1600 (**+0.69 %**) | 199 → 203 (+2.01 %) | ≤ +25 % |
| full index, median of 3 | 6.54 s → 6.64 s (+1.60 %) | 5.44 s → 5.51 s (+1.18 %) | ≤ +15 % |
| incremental re-index, median of 3 | 0.99 s → 1.11 s (+11.88 %) | 2.20 s → 2.25 s (+2.34 %) | ≤ +15 % |
| DB size | 80,785,408 → 80,826,368 B (+0.05 %) | 56,012,800 → 56,045,568 B (+0.06 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 (1 truncated in both) | 0 of 144 | 0 |
| `value_preserving` rows from a new construct | 0 (11 `await_call`) | 0 (1 `await_call`, 2 `then_value`, 1 `then_return`) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S6b passes every gate**, but its yield on Covalent is small: 11 edges, all `await_call`. A source count at the pinned commit explains why. 186 of Covalent's `.then(` calls are `.then(() => …)`: test sequencing with no value to bind. Only 3 bindings take an `await`ed value. Its 86 `.subscribe(` calls are RxJS, which S6b does not model. The other constructs fire only on the readiness fixture, which is part of estate's tree (`await_value`, `await_result` and `then_result` not even there: the tree is measured at the baseline commit, before this slice's fixture cases). The incremental cost rose by a fixed +0.12 s per process (query compile), within the gate.

### S6c: local object properties
Baseline `d06d673` (the S6b code, = main `cfbfcb5`). Candidate `a64d53a` (code-identical to this slice after its rebase). Workflow run `38082650676`, branch `measure/ts-s6c`.

| metric | Covalent `438c297` | estate tree `d06d673` | gate |
|---|---|---|---|
| `flows_to` edges | 1600 → 1600 (**+0.00 %**) | 251 → 253 (+0.80 %) | ≤ +25 % |
| full index, median of 3 | 7.31 s → 7.32 s (+0.22 %) | 5.66 s → 5.70 s (+0.80 %) | ≤ +15 % |
| incremental re-index, median of 3 | 1.16 s → 1.17 s (+1.37 %) | 2.25 s → 2.29 s (+1.46 %) | ≤ +15 % |
| DB size | 80,830,464 → 80,859,136 B (+0.04 %) | 56,233,984 → 56,295,424 B (+0.11 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 (1 truncated in both) | 0 of 178 | 0 |
| `value_preserving` rows from a new construct | 0 (none fired) | 0 (1 `object_literal`, 1 `return_property`) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S6c passes every gate, with zero yield on Covalent.** Its only firings are the readiness fixture's `property()` row, which is part of estate's tree. A real object literal is built to be passed on: returned, handed to a call, spread or assigned to a field. The no-alias gate correctly refuses every such object, and nothing that is left (a literal only ever read back as `o.k` in the same body) fired on Covalent. A property flow that follows an object INTO a call or a field needs a heap or alias model, which this ADR keeps out.

**Deleted (2026-10-10).** The operator asked for a second real TypeScript corpus for the two zero-yield slices; a slice that still found 0 edges there would be deleted. On vitejs/vite `3b72997` (552 `.ts` + 17 `.tsx`, Node/TS tooling with plain object literals and returned closures), S6c again emitted **0 edges**: `object_literal`, `property_write` and `return_property` never fired (run `38092981779`, S6b → S6c: `flows_to` 7436 → 7436). Vite's object literals are built to be passed on (plugin and option objects returned or handed to a call), and S6c's no-alias gate correctly refuses all of them. **S6c is deleted**, code and tests: the `object_property` captures, `object_property_path`, `is_local_object` and the `object_roots` gate. Its readiness row reverts to ❌, with `missing_primitives_are_unreachable` restored. The numbers above stay as the record. A property flow that follows an object INTO a call or a field needs an alias model, which this ADR keeps out.

### S6d: closure returns (primitive 4)
Baseline `ad7c3de` (the S6c code, = main `85ece2d`). Candidate `f71a21d` (code-identical to this slice after its rebase). Workflow run `38083252587`, branch `measure/ts-s6d`.

| metric | Covalent `438c297` | estate tree `ad7c3de` | gate |
|---|---|---|---|
| `flows_to` edges | 1600 → 1600 (**+0.00 %**) | 265 → 265 (0.00 %) | ≤ +25 % |
| full index, median of 3 | 6.24 s → 6.38 s (+2.26 %) | 5.22 s → 5.19 s (−0.67 %) | ≤ +15 % |
| incremental re-index, median of 3 | 1.01 s → 0.97 s (−3.32 %) | 2.08 s → 2.05 s (−1.46 %) | ≤ +15 % |
| DB size | 80,687,104 → 80,773,120 B (+0.11 %) | 56,528,896 → 56,553,472 B (+0.04 %) | ≤ +10 % |
| Lineage, 10 fixed queries newly truncated | 0 | 0 | 0 |
| Lineage sweep newly truncated | 0 of 520 (1 truncated in both) | 0 of 195 | 0 |
| `value_preserving` rows from a `return_closure` | 0 (none fired) | 0 (1, the S5b fixture row) | 0 |
| capped-away new-construct rows (inconclusive) | 0 | 0 | 0 |

**Verdict: S6d passes every gate, with zero yield on Covalent.** It replaces S5b's narrow pattern with one mechanism (the same `callable_return_contributors` walk S6a and S6b use), and the S5b fixture row still holds. S6d emitted no `return_closure` flow on Covalent within its supported shapes. That is an absent flow, not an audit showing the construct is absent. No other corpus was measured.

### Cumulative (S5b–S6d) and the S6 graduation
All four S6 slices passed every gate against the state before them, so primitives 4–7 graduated (closures, inline callbacks, `await`/`.then`, local object properties). Local object properties (S6c) were later deleted for zero yield on a second corpus; see "S6c". The same runner pairs measured the merged result against the two releases. Candidate `cf4414b` is the S6d code.

| metric (Covalent `438c297`) | 0.24.0 `890cc39` → S6d (run `38083837701`) | 0.25.0 `490c547` → S6d (run `38083840625`) |
|---|---|---|
| `flows_to` edges | 1394 → 1600 (**+14.78 %**) | 1502 → 1600 (**+6.52 %**) |
| full index, median of 3 | 7.36 s → 7.36 s (+0.04 %) | 7.18 s → 7.24 s (+0.88 %) |
| incremental re-index, median of 3 | 0.96 s → 1.18 s (**+23.25 %**) | 0.98 s → 1.17 s (**+19.40 %**) |
| DB size | +1.12 % | +0.72 % |
| Lineage newly truncated (10 fixed + 520 sweep) | 0 | 0 |
| `value_preserving` rows from a new construct | 0 | 0 |

On estate's own tree: 107 → 126 edges against 0.24.0 (+17.76 %), and 150 → 158 against 0.25.0 (+5.33 %). Incremental re-index rose +8.12 % and +8.80 % there.

**The cumulative incremental cost exceeds the per-slice gate.** Each slice stayed within +15 % of the slice before it: S6a +7.20 %, S6b +11.88 %, S6c +1.37 %, S6d −3.32 %. Added up, Covalent's one-file re-index costs about 0.2 s more than 0.25.0. Full index does not move. That is consistent with a fixed per-process cost, most plausibly compiling the larger TypeScript query (the S6a first candidate's +0.13 s pointed the same way), but no profile isolates it yet. The gates are defined per slice, so no slice is killed. The number is recorded so that a later change can buy it back. Profile first (an isolated query-construction benchmark); if compile cost is confirmed, one candidate is moving the callback-parameter alternations into one Rust capture, the way `callable_return_contributors` did for returns.

### Cumulative cost: profile and fix (2026-10-10)
**Profile** (run `38093054271`, branch `measure/w8-profile`): `perf record -g` of one Covalent one-file re-index (release profile with symbols), plus an isolated query-construction benchmark (`extractor_for_extension` per extension, best of 5, same runner).
- **Query compilation is the dominant hotspot.** `ts_query_new` takes **27.7 %** of the sampled CPU at 0.24.0 and **43.6 %** at 0.26.0 (inclusive; almost all of it `ts_query__perform_analysis`). SQLite and JSON are the next hotspots.
- The TypeScript query takes about 110 ms to construct at 0.24.0 and about 280 ms at 0.26.0, because S5b–S6d grew it.
- **The cause is how often it was compiled.** `index_path` compiled the query of every extension present in the repo before it knew which files had changed, once per extension. So `.ts`, `.mts` and `.cts` each compiled the same TypeScript query, `.js`/`.mjs`/`.cjs` each compiled JavaScript, and TSX and the rest were compiled too.
- Over the extensions found in the checkout, the benchmark's per-extension times sum to 0.67 s at 0.24.0 and 1.17 s at 0.26.0. That sum is a synthetic aggregate, not measured indexing overhead.

**Fix:** a per-pass `ExtractorCache`.
- It resolves each extension's dispatch without compiling.
- It shares one slot per dispatch key. With built-in dispatch (no grammar override), `.ts`/`.mts`/`.cts` share one TypeScript query.
- It compiles a query the first time a file of that dispatch is extracted.
- One exception: an additive plugin's query is not verified at load, so it compiles when its extension is first checked, even if no file of it changed.
- So, apart from additive plugins, a re-index compiles only the languages it extracts. The budget test pins the TypeScript-only case: one `.ts` edit, an empty plugin registry and no callers to re-extract.

**Evidence for the fix**
- Node and edge counts are unchanged on both corpora (run `38095443235`). That is a count comparison, not a content diff of the graphs.
- `tests/query_compile_budget.rs` pins the compiles per pass: 3 for a full index of `.ts`/`.mts`/`.cts`/`.js`/`.py`, 0 for an unchanged re-index, and 1 after one `.ts` edit. It failed on the previous code with 5 (run `38095277660`).

| median of 3 | Covalent `438c297` | vitejs/vite `3b72997` |
|---|---|---|
| incremental re-index, 0.26.0 → fix (run `38095443235`) | 1.13 s → 0.99 s (−12.37 %) | 3.03 s → 2.02 s (−33.26 %) |
| **incremental re-index, 0.24.0 → fix** (run `38095434122`) | 0.95 s → 1.03 s (**+8.32 %**) | 2.14 s → 2.13 s (**−0.85 %**) |
| full index, 0.26.0 → fix | 7.28 s → 7.24 s (−0.56 %) | 13.97 s → 12.95 s (−7.36 %) |
| full index, 0.24.0 → fix | 7.28 s → 7.32 s (+0.56 %) | 13.14 s → 13.66 s (+3.94 %) |

**Against 0.24.0, the pre-data-flow baseline, the cumulative incremental cost is now under the +15 % gate on both corpora.** On Covalent about +0.08 s remains. The most plausible cause is the larger TypeScript query (the benchmark puts the difference at about 170 ms per compile), but that has not been isolated after the fix.

**Yield on Covalent:** S6a 87 edges, S6b 11, S6c 0, S6d 0. Inline array callbacks carry most of it. `await`/`.then` add little there, because Covalent's promises are mostly value-free test sequencing (see S6b). RxJS Observables are not modelled. Within the supported shapes, S6c and S6d emitted nothing there: an object literal that is passed on needs an alias model, and no supported closure-return shape fired. No other TypeScript corpus was measured at the time; see "Two corpora" below.

### Two corpora (2026-10-10): the S6c/S6d check and every gate on vite
The operator asked for a second real TypeScript corpus in a different style, for the two zero-yield slices, with every gate re-measured there. A slice that still found 0 edges would be deleted. The corpus is **vitejs/vite `3b72997d42e7dd2b31898d0e68461e3025e836b6`**: 552 `.ts` + 17 `.tsx`, Node/TS tooling built from plain object literals, returned closures and `async` functions. Its touch file is `packages/vite/src/node/server/index.ts`. Each slice was measured against the slice before it, on one runner, median of 3, with the same procedure.

**Per-slice `flows_to` yield and outcome**

| slice | Covalent `438c297` | vite `3b72997` (run) | outcome |
|---|---|---|---|
| S5b return composition | +83 | +775 (`38093668858`) | kept |
| S5c destructuring | +19 | +230 (`38095313839`) | kept |
| S5d loops | +6 | +239 (`38095331701`) | kept |
| S6a inline callbacks | +87 | +391 (`38092930218`) | kept |
| S6b `await` / `.then` | +11 | +1302 (`38092957443`) | kept |
| S6c local object properties | 0 | **0** (`38092981779`) | **deleted** (#319) |
| S6d closure returns | 0 | +44, of which 24 carry `return_closure` (`38092995748`) | kept |

**The other gates per slice on vite.** Every slice except S6c and S6d failed the Lineage gate as it was then written ("0 newly truncated" at the default depth). Fixed / sweep queries newly truncated: S5b 1 / 100, S5c 0 / 9, S5d 1 / 2, S6a 0 / 6, S6b 0 / 24. A Lineage reply was cut at its 25K-char response budget, or at the default depth of 8, and a larger, more connected graph reaches more of both. The cut was reported (`truncated: true`), never silent, but the rest of the answer could not be retrieved. S6b also measured +16.14 % on the per-slice incremental gate, against S6a. The later compile fix ("Cumulative cost: profile and fix") brought the cumulative incremental cost under the gate; S6b's own per-slice timing was not re-measured after that fix. Every slice passed `flows_to` growth, full index, DB size and semantics.
- **Resolved by the operator's ruling on #279 (2026-10-11):** all five slices stay, and the Lineage gate is replaced by "every Lineage query is completely retrievable". 0.28.0 meets it through paging and a default depth at the ceiling; see "The full graph (2026-10-11)".

**Cumulative, the merged state `eb407b1` (S6c deleted, #318 compile fix)**

| metric | Covalent: 0.24.0 → `eb407b1` (run `38097535976`) | vite: 0.24.0 → `eb407b1` (same run) | Covalent: 0.25.0 → `eb407b1` (run `38097553701`) | vite: 0.25.0 → `eb407b1` (same run) | gate |
|---|---|---|---|---|---|
| `flows_to` edges | 1394 → 1600 (+14.78 %) | 4499 → 7480 (**+66.26 %**) | 1502 → 1600 (+6.52 %) | 5743 → 7480 (**+30.25 %**) | ≤ +25 % |
| full index | +4.18 % | +3.40 % | +5.66 % | −0.49 % | ≤ +15 % |
| **incremental re-index** | 0.63 s → 0.67 s (**+6.91 %**) | 1.34 s → 1.37 s (**+1.90 %**) | +5.35 % | −6.69 % | ≤ +15 % |
| DB size | +1.21 % | **+14.73 %** | +0.64 % | +8.49 % | ≤ +10 % |
| Lineage newly truncated (fixed / sweep) | 0 / 0 of 520 | **2 / 141** of 2829 | 0 / 0 | **0 / 30** | 0 |
| `value_preserving` rows from a new construct | 0 | 0 | 0 | 0 | 0 |

**The cumulative incremental cost is under the +15 % gate against 0.24.0**, the pre-data-flow baseline, on both corpora. This was the operator's ask, and it is measured against 0.24.0, not only 0.25.0. The gates are defined per slice. On vite, the cumulative `flows_to` growth (+66 %) and DB size (+14.7 %) are the sum of slices that each passed those two gates. The operator accepted both on #279 (2026-10-11), and the DB gate is now a cost per stored edge, which falls on vite (see "The full graph").

Still unsupported:
- named (non-inline) callbacks, which would be interprocedural;
- RxJS and every other non-Promise async model;
- objects that escape;
- path conditions;
- aliasing.

Option 3 stays no-go.

### The full graph (2026-10-11)
The operator ruled on #279: "I want the full graph". Every data-flow slice stays, and no query may be cut off.

**Why answers were cut.** A diagnostic run (`38104899420`, branch `measure/w8d4-trunc`) classified every truncated `flows_to` Lineage query on both corpora, at depth 8 and at depth 24:
- No query hit the node cap (5,000).
- At depth 24, every cut was the 25K-char reply budget alone: 2 fixed queries on Covalent, and 10 fixed plus 141 sweep queries on vite.
- At the old default depth of 8, the depth horizon also cut 1 Covalent sweep query, 4 vite fixed queries and 109 vite sweep queries.

**What changed (estate 0.28.0)**
- **Paging.** `Lineage` and `BlastRadius` (MCP, plus the `lineage` and `blast-radius` CLIs) page their answer.
  - The 25K budget bounds one reply, not the answer.
  - A reply that does not complete the answer has `truncated: true` and a `next_cursor`. Passing that back as `cursor` (or `--cursor`) returns the next rows of the same answer. The last page has `next_cursor: null`.
  - Rows are ordered (depth, symbol), and `flows` by row, so pages from separate processes join exactly.
  - A cursor carries a fingerprint over the tool, the effective query and every row. A cursor from another query, or from before the graph changed, is refused: pages are never spliced from two answers.
  - `total` counts the whole answer, and `page` says which rows a reply holds.
  - A row wider than the budget is carried whole on its own page, with a diagnostic; nothing is cut.
- **Default depth.** `Lineage` defaults to its ceiling (24), so the default query is not cut at a horizon the caller never chose. A horizon or node cap that still bites stays reported (`depth_horizon_reached`, `node_cap_reached`, and `truncated` on the last page). Paging never claims a bounded walk is the whole graph.
- **Consumers.** garden's estate shim (`scripts/_estate_client.py`, `call`) follows every cursor and joins the pages, so a seat gets the whole answer, with `complete` in the result. The `search` skill tells the seat to read `complete`. The MCP schemas advertise `cursor`.

**Measured (run `38106571743`, 0.27.0 `aa957f9` → paging `57f4726`, same runner).** The measurement follows every cursor and checks that the pages join into the answer's `total` for both arrays, with no row twice. An earlier run on `062f0c9` (`38105645690`) gave the same counts.

| | Covalent `438c297` | vitejs/vite `3b72997` |
|---|---|---|
| fixed queries not completely retrievable | 2 → **0** (at most 2 pages) | 10 → **0** (at most 7 pages) |
| sweep queries not completely retrievable | 1 of 520 → **0** | 146 of 2829 → **0** |
| largest page (content only) | 23,408 chars | 23,533 chars |
| `flows_to` edges | 1600 → 1600 | 7480 → 7480 |
| full / incremental index | −1.19 % / +0.68 % | −1.54 % / +0.85 % |
| DB bytes per stored edge | −0.09 % | +0.05 % |

**Every measured Lineage query is completely retrievable on both corpora.** That is the gate's query set: default-depth `flows_to` queries from the 10 fixed producers plus every baseline `Parameter` root. The 2 vite queries that 0.24.0 → 0.26.0 newly truncated, and every other cut query in that set, now page to their full answer. Dependency-mode Lineage and other roots were not swept.

**DB cost per stored edge.** From 0.24.0 to `eb407b1`: Covalent 2,767 → 2,759 bytes per edge (−0.31 %), vite 2,557 → 2,473 (−3.29 %). The raw vite DB grew +14.7 % for +66 % `flows_to`, which the operator accepted on #279. Cheaper edge encoding was not attempted; the ruling made it optional.

**Pruning (the operator's addendum).** The rule is: "only things we should truncate is dead end nodes that have no value - but has to be hard proof of that". **No pruning rule ships.**
- A candidate such as "a value slot with no incident `flows_to` edge" cannot change a `flows_to` Lineage answer.
- But it still has a symbol identity: `SearchEntity` lists it with `include_values`, and its `File` reaches it through `Contains`. So the proof that it carries no information for any query fails, and the answer-equivalence evidence the addendum requires (every Lineage, blast-radius and `flows_to` query on both corpora identical with pruning on and off) was not produced for any rule.
- A future pruning rule must state its predicate here, prove equivalence on both corpora, and record the nodes pruned and the DB and time saved. A rule that fails equivalence is dropped, not tuned.

**Bounds that remain.** Two bounds are explicit and reported, never silent: the depth ceiling (24) and the traversal node cap (5,000). Neither binds any measured query on either corpus at the new default.

### Graduation (S5b–S5d together)
All three slices passed every gate. **Primitives 1–3 graduate.** Cumulative on Covalent against `2b6bf7f`: `flows_to` 1394 → 1502 (+7.7 %), and full index 7.15 s → 7.20 s, each slice measured against the one before it. Still unsupported:
- every closure shape other than the single-identifier returned arrow;
- promises and callbacks;
- property writes;
- path conditions;
- aliasing.

Option 3 stays no-go. Primitive 4 (closure returns, beyond the narrow S5b form) may now be evaluated under the same gates.

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

Promises, property writes and path conditions were explicitly **out** of Option 2; the 2026-10-10 extension (see "Extension S6") brings promises and callbacks in, one gated slice each. Its local-object property slice (S6c) was deleted for zero yield. Path conditions stay out. The prototype evaluates **primitives 1–3**, plus one narrow case of primitive 4 that the program pulled forward into S5b: a returned arrow whose body is a single identifier (`return () => captured`), as `may_influence`, measured under the same gates. The rest of primitive 4 is evaluated only if 1–3 graduate. After graduation, these remain unsupported: every other closure shape (until primitive 4), promises and callbacks, property writes, path conditions, and aliasing.

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
  - **Lineage:** a fixed set of 10 `lineage --relation flows_to` queries (listed in the ADR) plus the sweep over every baseline `Parameter` slot, each paged to completion through `next_cursor`, recording the largest page's size and whether the answer was completely retrievable.
- **Acceptance metrics** (restated by the operator's ruling on #279, 2026-10-11):
  - `flows_to` growth ≤ 25 % per slice; full-index time ≤ +15 %; incremental re-index ≤ +15 % (the speed gates, unchanged);
  - DB cost per stored edge (DB bytes ÷ stored edges) ≤ +10 %; the raw DB size is recorded;
  - every Lineage query is completely retrievable: its pages join into its whole answer, no reply is silently cut, and no depth horizon or node cap cuts the default query;
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
> Read docs/adr/ADR-014-dataflow-taint-decision.md (Accepted: Option 2; S5b–S5d and S6a/S6b/S6d graduated and are kept by the operator's ruling on #279, and S6c was deleted for zero yield on two corpora; see "Measurements", "Cumulative cost: profile and fix", "Two corpora" and "The full graph (2026-10-11)"). Lineage and BlastRadius answers are paged (`next_cursor`), Lineage defaults to depth 24, and every measured Lineage query (default-depth `flows_to` from the 10 fixed producers and every baseline `Parameter` root) is completely retrievable on both corpora. No pruning rule ships; a future one needs the proof and the answer-equivalence evidence the addendum requires. Option 3 stays no-go unless its positive-evidence falsifier is met.
