# Spec: TypeScript value lineage

- **Status:** Shipped
- **Owner:** eu.gene.lim
- **Plan:** [`plan.md`](plan.md)
- **Constrained by:** [`docs/ENGINE-CONTRACT.md`](../../ENGINE-CONTRACT.md), [`docs/adr/ADR-002-stable-symbol-identity.md`](../../adr/ADR-002-stable-symbol-identity.md)
- **Brief:** none
- **Discovery:** none
- **Contract:** none
- **Shape:** data

## Outcome

Callers tracing an Angular route parameter or conventional `@Input()` can recover conservative intra-TypeScript value lineage through locals, fields, callable parameters, return values, and resolved call arguments. Every reported hop is persisted in the normal graph with stable endpoints, source evidence, confidence, provenance, and resolver identity.

## What Changes

- TypeScript value-bearing declarations and external Angular inputs become stable graph nodes in the existing Tree-sitter extraction.
- Assignments, expression dependencies, property reads, call arguments, and return propagation become `flows_to` graph relationships.
- `Lineage` can select semantic value flow while retaining its existing dependency-lineage behavior by default.
- The Angular fixture and focused graph-semantic regressions live in the existing end-to-end test suite.

## Durable Outputs

| Semantic role | Applicability | Destination | Owner | Expected evidence | Closeout condition |
| --- | --- | --- | --- | --- | --- |
| Current architecture | Value-flow direction and pipeline ownership are engine invariants | [`docs/ENGINE-CONTRACT.md`](../../ENGINE-CONTRACT.md) | engine maintainers | contract text agrees with conformance and integration tests | documented direction matches stored edges and traversal |
| User-facing promise | `Lineage` gains an optional semantic relation | [`docs/getting-started.md`](../../getting-started.md) | estate tool maintainers | documented request and response are exercised end to end | example request recovers the fixture chain |
| Delivery contract | The slice needs durable approval and review evidence | this spec and `plan.md` | work-loop owner | approvals, gates, review artifacts | close-work verifies shipped status and evidence |

## Agent Rules

### Always do

- Preserve `source = dependent/consumer` and `target = dependency/producer`; semantic forward flow walks `Direction::Dependents`.
- Use `EdgeKind::Other("flows_to")`, stable `SymbolId` construction, and the existing EXTRACT → RESOLVE → STORE → RETRIEVE pipeline.
- Put a source location, construct metadata, confidence, provenance, and `resolved_by` on every flow edge.
- Leave ambiguous or unresolved calls without parameter or return flow targets.

### Ask first

- Any change to the core edge-direction invariant, persistent store schema, or published default behavior of `Lineage`.
- Any new dependency or TypeScript-specific compiled resolution path that cannot be expressed through generic query captures and existing symbol-resolution results.
- Any expansion beyond the constructs and exact Angular source forms named in this spec.

### Never do

- Build CFG, SSA, path-sensitive flow, alias/heap modeling, taint tracking, or source/sink security analysis.
- Add template, HTTP, RxJS, forms, NgRx, Signals, `@Output`, parent/child wiring, or transformation semantics.
- Create an Angular-specific store, query engine, or second symbol-resolution system.

## Testing Strategy

- **Graph semantics (AC-0001, AC-0002, AC-0003):** TDD through Rust tests whose endpoint, direction, ambiguity, and evidence assertions fail on an incorrect graph.
- **Persisted retrieval (AC-0004):** TDD through an end-to-end integration test that indexes real `.ts` fixtures through `index_path`, reads the normal `GraphStore`, and invokes `Lineage`.
- **Compatibility and identity (AC-0005, AC-0006):** TDD default-lineage and identity regressions that fail on changed dependency results or unstable/merged value-node IDs.
- **Repository health (AC-0007):** goal-based workspace build, test, formatting, clippy, and GraphStore conformance commands because these are aggregate gates rather than one behavior assertion.

## Acceptance Criteria

- [x] **AC-0001.** For the closed set of explicit TypeScript expressions `const a = b`, `let c = a`, `this.field = value`, `const c = a + b`, and `const id = customer.id`, stored `flows_to` relationships recover respectively `b → a`, `a → c`, `value → this.field`, both `a → c` and `b → c`, and `customer.id → id`, with parsed evidence at each syntax site.
- [x] **AC-0002.** For each exact call site of a uniquely resolved function or method, every supported argument contributes to the corresponding declared parameter and a returned value contributes to that site's assignment target; `normalize(raw)` recovers `raw → normalize.id → normalize.return → x`, two calls from one caller to the same callee retain their distinct argument evidence, and an ambiguous or unresolved call emits no parameter or return flow target.
- [x] **AC-0003.** Conventional `@Input() tenantId` and `route.snapshot.paramMap.get('id')` create stable external source nodes named `AngularInput:tenantId` and `RouteParam:id` whose parsed flow relationships reach their declared property or assignment target.
- [x] **AC-0004.** Indexing the Angular fixture through the production pipeline persists and retrieves forward semantic lineage equivalent to `RouteParam:id → routeId → CustomerComponent.customerId → CustomerComponent.loadCustomer.id → CustomerService.getCustomer.id`; the same graph contains `AngularInput:tenantId → CustomerComponent.tenantId`.
- [x] **AC-0005.** Existing dependency lineage results remain unchanged when the semantic relation is not selected.
- [x] **AC-0006.** New local, parameter, return, property-read, and external-source identities are unchanged by unrelated line shifts and remain partitioned by logical owner; same-named locals in different callables and same-named `@Input()` fields in different components do not merge.
- [x] **AC-0007.** The workspace build, test, clippy, formatting, and GraphStore conformance gates pass without new warnings or ignored tests.

## Amendments

- **2026-10-01 — partially superseded by [`docs/specs/typescript-flow-semantics/spec.md`](../typescript-flow-semantics/spec.md) (TS-S1).** Every acceptance criterion above was met at the revision that shipped it (`3257648`) and stays checked as historical evidence. Two of its observable claims have since changed on purpose, and the assertions that pinned them are versioned, not deleted:
  - **AC-0001/AC-0003 evidence.** The Angular `@Input()` and `route.snapshot.paramMap.get(…)` constructs were emitted at `Parsed`/1.0. They are convention matches, not compiler-proven facts, and are now emitted at `Heuristic`/0.5 with `resolved_by = tree-sitter-convention`. Non-Angular direct syntax flow is unchanged at `Parsed`/1.0.
  - **`metadata.construct`.** Still present and readable, but it is now the lexicographic minimum of the complete `constructs` set. A scalar was proven lossy: two facts sharing `(source, target, kind)` collapsed last-writer-wins and one classification disappeared.

  The counts quoted in the PR #207 description (8,600 `Calls`, the PageRank top-25 result, the 1,474-test total) are observations from that revision, not acceptance thresholds.

## Follow-ons

none — Slice 2 and every explicitly excluded adjacent semantic domain remain outside this delivery and are not queued by this spec.

## Assumptions

none
