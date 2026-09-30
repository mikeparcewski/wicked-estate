# Plan: TypeScript value lineage

- **Spec:** [`spec.md`](spec.md)
- **Status:** Done
- **Repository anchors:** [`CLAUDE.md`](../../../CLAUDE.md), [`docs/ENGINE-CONTRACT.md`](../../ENGINE-CONTRACT.md), [`crates/wicked-estate-extract/src/treesitter.rs`](../../../crates/wicked-estate-extract/src/treesitter.rs), [`crates/wicked-estate-resolve/src/lib.rs`](../../../crates/wicked-estate-resolve/src/lib.rs), [`crates/wicked-estate-retrieve/src/lib.rs`](../../../crates/wicked-estate-retrieve/src/lib.rs); analogous query-driven framework edges and their tests in `treesitter.rs`; end-to-end resolver and lineage precedents in `crates/wicked-estate/tests/resolver_precision_index.rs` and the `Lineage` unit tests. Deviation: value-flow traversal reverses the stored dependent edge direction to present producer-to-consumer lineage.

## Approach

Extend the generic Tree-sitter capture convention with value-definition and flow-site roles used first by `typescript.scm`. Emit same-file syntactic dependencies directly; carry call argument and call-result facts as semantic `UnresolvedRef` records, then correlate them with exact-location call bindings while the existing resolver output still retains site multiplicity, before relationship dedup. Store ordinary attributed edges and add an opt-in `Lineage` relation that traverses them in semantic-forward order.

## Constraints

- [`docs/ENGINE-CONTRACT.md`](../../ENGINE-CONTRACT.md) owns edge direction, two-phase resolution, confidence tiers, and bounded traversal.
- [`docs/adr/ADR-002-stable-symbol-identity.md`](../../adr/ADR-002-stable-symbol-identity.md) forbids content-, byte-, and line-based identity.
- `CLAUDE.md` requires query/config data rather than per-language Rust dispatch, reuse of `Other(String)`, and full workspace gates.
- No dependency, database schema, Angular storage behavior, or precise-tier integration is added.

## Construction tests

**Integration tests:** production `index_path` over TypeScript fixtures proves AC1–AC4 from persisted nodes and edges; `Lineage::invoke` proves forward semantic traversal while its existing suite proves default compatibility.

**Manual verification:** inspect one serialized fixture edge per construct for `location`, `metadata.construct`, confidence, provenance, and `resolved_by`; no separate interactive surface is required.

## Durable-output map

| Durable output | Tasks | Implementation evidence | Closeout evidence |
| --- | --- | --- | --- |
| Engine contract | T1, T2 | graph-semantic tests and contract diff | stored direction and traversal agree |
| Lineage user guide | T3 | retrieval unit and end-to-end tests | documented request reproduces fixture chain |
| Delivery contract | T1–T3 | gates and review artifacts | spec and plan lifecycle complete |

## Design (LLD)

### Design decisions

- `flows_to` remains `EdgeKind::Other("flows_to")`; a first-class enum variant would change the core serialized enum without a storage or query benefit. Traces to: AC-0001–AC-0005.

Owned by: T1, T2, T3

- Stored direction remains consumer → producer. Semantic-forward lineage selects `Direction::Dependents`; reversing storage would violate the engine contract. Traces to: AC-0004, AC-0005.
- Call flow consumes exact-site `Calls` bindings from the existing resolver stack before `(source, target, kind)` relationship dedup. It does not choose a callable independently, so ambiguity behavior remains owned by the current resolvers while repeated same-caller/same-callee sites stay distinguishable. Traces to: AC-0002, AC-0004.

### Data & schema

TypeScript locals, parameters, fields, return values, textual property reads, and Angular external values use existing `Node`, `NodeKind`, metadata, and stable `SymbolId` facilities. Their identities derive from logical owner symbols plus value descriptors, never spans; display labels are not identity. Flow edges use ordinary `Edge` fields; `metadata.construct` distinguishes assignment, expression, argument, return, Angular input, and route parameter evidence. No database migration occurs. Traces to: AC-0001–AC-0004, AC-0006.

Owned by: T1, T2

### Interfaces & contracts

`Lineage` keeps its current request behavior when no relation selector is present. Selecting `relation = "flows_to"` changes only the bounded edge filter and traversal direction for that invocation. Traces to: AC-0004, AC-0005.

Owned by: T3

## Tasks

### T1: Parsed TypeScript value nodes and direct flow edges satisfy AC1 and AC3

**Depends on:** none

**Touches:** `crates/wicked-estate-core/src/edge_tags.rs`, `crates/wicked-estate-extract/src/queries/typescript.scm`, `crates/wicked-estate-extract/src/treesitter.rs`, `crates/wicked-estate-extract/tests/**`

**Tests:**
- TDD tests index variable assignment, component assignment, multiple RHS dependencies, property reads, `@Input()`, and snapshot route parameters, then assert stable endpoints, stored direction, and evidence (AC-0001, AC-0003).
- Identity regressions shift unrelated lines and repeat local/input display names under different logical owners (AC-0006).
- Regression assertions keep existing TypeScript definitions, imports, and calls unchanged.
- Exact initial red stub in `crates/wicked-estate/tests/typescript_value_lineage.rs` (the task fills the remaining AC-0001, AC-0003, and AC-0006 assertions after this persisted-boundary test earns red):

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead};
use wicked_estate_store::SqliteStore;

fn indexed_typescript(tag: &str, source: &str) -> (PathBuf, SqliteStore) {
    let root = std::env::temp_dir().join(format!(
        "typescript_value_lineage_{tag}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("fixture.ts"), source).unwrap();
    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &root).expect("index TypeScript fixture");
    (root, store)
}

fn semantic_flow_name_pairs(store: &SqliteStore) -> BTreeSet<(String, String)> {
    let names: BTreeMap<_, _> = GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .map(|node| (node.symbol, node.name))
        .collect();
    GraphRead::all_edges(store)
        .unwrap()
        .into_iter()
        .filter(|edge| edge.kind == EdgeKind::Other("flows_to".to_string()))
        .map(|edge| {
            // Stored source is the consumer and target is the producer.
            (names[&edge.target].clone(), names[&edge.source].clone())
        })
        .collect()
}

#[test]
fn direct_and_external_typescript_value_flow_is_persisted() {
    let source = r#"
        import { Input } from '@angular/core';
        class CustomerComponent {
            @Input() tenantId!: string;
            customerId = '';
            load(route: any, b: string) {
                const a = b;
                const c = a + b;
                this.customerId = c;
                const routeId = route.snapshot.paramMap.get('id');
            }
        }
    "#;
    let (root, store) = indexed_typescript("direct", source);
    let pairs = semantic_flow_name_pairs(&store);
    for expected in [
        ("b", "a"),
        ("a", "c"),
        ("b", "c"),
        ("c", "customerId"),
        ("AngularInput:tenantId", "tenantId"),
        ("RouteParam:id", "routeId"),
    ] {
        assert!(
            pairs.contains(&(expected.0.to_string(), expected.1.to_string())),
            "missing semantic-forward flow {expected:?}; got {pairs:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}
```

**Approach:** Add generic capture roles and stable lexical ownership metadata in the shared Tree-sitter extractor; TypeScript syntax remains in `typescript.scm`.

**Done when:** the focused extractor and graph-semantic tests for AC-0001, AC-0003, and AC-0006 pass.

### T2: Existing call resolution drives parameter and return flow without false targets

**Depends on:** T1

**Touches:** `crates/wicked-estate-resolve/src/lib.rs`, `crates/wicked-estate/src/lib.rs`, `crates/wicked-estate/tests/**`

**Tests:**
- TDD integration tests cover local method arguments, cross-file service arguments, parameter-to-return-to-assignment, repeated same-caller/same-callee sites with different arguments, and ambiguous/unresolved calls (AC-0002).
- The end-to-end fixture asserts every call-derived edge inherits or conservatively bounds the resolved call's evidence (AC-0002, AC-0004).
- Exact initial red stub appended to `crates/wicked-estate/tests/typescript_value_lineage.rs` after T1 (the task fills the precise provenance and negative assertions after this exact-site test earns red):

```rust
#[test]
fn resolved_calls_retain_per_site_argument_and_return_flow() {
    let source = r#"
        class CustomerService {
            getCustomer(id: string): string { return id; }
        }
        class CustomerComponent {
            loadCustomer(service: CustomerService, firstId: string, secondId: string) {
                const first = service.getCustomer(firstId);
                const second = service.getCustomer(secondId);
                unresolved(firstId);
            }
        }
    "#;
    let (root, store) = indexed_typescript("calls", source);
    let pairs = semantic_flow_name_pairs(&store);
    for expected in [
        ("firstId", "id"),
        ("secondId", "id"),
        ("id", "getCustomer.return"),
        ("getCustomer.return", "first"),
        ("getCustomer.return", "second"),
    ] {
        assert!(
            pairs.contains(&(expected.0.to_string(), expected.1.to_string())),
            "missing exact-site call flow {expected:?}; got {pairs:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}
```

**Approach:** Correlate semantic flow facts with exact-location `Calls` bindings before the resolver's relationship dedup discards repeat-site multiplicity, then join the resolved callable to its owned parameter/return nodes; no new resolver independently chooses a callable.

**Done when:** AC-0002 tests pass and ambiguous call fixtures contain no parameter or return flow target.

### T3: Persisted Angular flow is queryable through Lineage and documented

**Depends on:** T2

**Touches:** `crates/wicked-estate-retrieve/src/lib.rs`, `crates/wicked-estate-mcp/src/lib.rs`, `crates/wicked-estate/tests/fixtures/**`, `crates/wicked-estate/tests/**`, `docs/ENGINE-CONTRACT.md`, `docs/getting-started.md`

**Tests:**
- TDD retrieval test selects `relation = "flows_to"` and asserts bounded semantic-forward depth and confidence while the existing default lineage tests stay green (AC-0004, AC-0005).
- End-to-end fixture indexes `CustomerComponent` and `CustomerService`, then asserts the complete route and input chains from persisted graph data (AC-0004).
- Exact initial red stub in the existing `Lineage` test module in `crates/wicked-estate-retrieve/src/lib.rs` (the task adds the fixture-chain and MCP-schema assertions after this selector/direction test earns red):

```rust
#[test]
fn lineage_flows_to_walks_from_producer_to_consumer_without_changing_default() {
    let mut store = MemStore::new();
    store.begin_batch().unwrap();
    store
        .upsert_nodes(&[
            make_node("producer", "route_id", NodeKind::Variable, "fixture.ts", 1),
            make_node("consumer", "customer_id", NodeKind::Field, "fixture.ts", 2),
        ])
        .unwrap();
    store
        .upsert_edges(&[Edge::new(
            SymbolId("consumer".to_string()),
            SymbolId("producer".to_string()),
            EdgeKind::Other("flows_to".to_string()),
            ResolutionTier::Parsed,
            "test-fixture",
        )])
        .unwrap();
    store.commit_batch().unwrap();

    let semantic = Lineage
        .invoke(
            &store,
            &json!({"symbol": "producer", "depth": 8, "relation": "flows_to"}),
        )
        .unwrap();
    let semantic_names: Vec<_> = semantic.content["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(semantic_names, vec!["customer_id"]);

    let default = Lineage
        .invoke(&store, &json!({"symbol": "producer", "depth": 8}))
        .unwrap();
    assert_eq!(default.content["total"].as_u64(), Some(0));
}
```

**Done when:** the documented `Lineage` request recovers AC-0004 through the normal store and the AC-0007 repository gates pass.

## Rollout

The capability ships directly with no flag, migration, infrastructure, or deployment sequencing. Rollback is the code and documentation revert; existing stores require a normal forced re-index to gain or remove extracted flow nodes and edges.

## Risks

- Over-broad identifier capture could create shortcut or declaration-self edges; graph-semantic tests assert exact flow pairs for each supported expression.
- Call argument flow is only as precise as the existing call edge; derived confidence and provenance must not overstate that resolution.
- Relationship dedup intentionally drops bound call-site multiplicity, so semantic derivation must finish against exact-site bindings first.
- Stable lexical identities must include callable ownership without changing the existing identities of classes, functions, methods, fields, imports, or calls.

## Changelog

- 2026-09-29: spec approved by eu.gene.lim
- 2026-09-29: plan approved by eu.gene.lim
