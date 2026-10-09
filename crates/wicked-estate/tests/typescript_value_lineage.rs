use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{
    Edge, EdgeKind, GraphRead, Provenance, RetrievalTool, SymbolId, edge_tags,
};
use wicked_estate_retrieve::Lineage;
use wicked_estate_store::SqliteStore;

fn indexed_typescript(tag: &str, source: &str) -> (PathBuf, SqliteStore) {
    indexed_typescript_files(tag, &[("fixture.ts", source)])
}

fn indexed_typescript_files(tag: &str, files: &[(&str, &str)]) -> (PathBuf, SqliteStore) {
    let root = std::env::temp_dir().join(format!(
        "typescript_value_lineage_{tag}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (path, source) in files {
        fs::write(root.join(path), source).unwrap();
    }
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
        .filter(|edge| edge.kind == edge_tags::other(edge_tags::FLOWS_TO))
        .map(|edge| {
            // Stored source is the consumer and target is the producer.
            (names[&edge.target].clone(), names[&edge.source].clone())
        })
        .collect()
}

fn semantic_flow_edges(store: &SqliteStore) -> Vec<Edge> {
    GraphRead::all_edges(store)
        .unwrap()
        .into_iter()
        .filter(|edge| edge.kind == edge_tags::other(edge_tags::FLOWS_TO))
        .collect()
}

fn edge_site_key(edge: &Edge) -> Option<(String, u32, u32, u32)> {
    edge.location.as_ref().map(|location| {
        (
            location.file.clone(),
            location.span.start_line,
            location.span.start_byte,
            location.span.end_byte,
        )
    })
}

fn semantic_flow_symbol_pairs(store: &SqliteStore) -> BTreeSet<(SymbolId, SymbolId)> {
    GraphRead::all_edges(store)
        .unwrap()
        .into_iter()
        .filter(|edge| edge.kind == edge_tags::other(edge_tags::FLOWS_TO))
        .map(|edge| {
            // Stored source is the consumer and target is the producer.
            (edge.target, edge.source)
        })
        .collect()
}

fn symbol_ids_named(store: &SqliteStore, name: &str) -> BTreeSet<SymbolId> {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|node| node.name == name)
        .map(|node| node.symbol)
        .collect()
}

fn one_symbol_named_with(store: &SqliteStore, name: &str, needle: &str) -> SymbolId {
    let matches: Vec<_> = GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|node| node.name == name && node.symbol.as_str().contains(needle))
        .map(|node| node.symbol)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one symbol named {name:?} containing {needle:?}, got {matches:?}"
    );
    matches.into_iter().next().unwrap()
}

fn value_role_symbols(store: &SqliteStore, name: &str, role: &str) -> BTreeSet<SymbolId> {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|node| {
            node.name == name
                && node
                    .metadata
                    .get("value_role")
                    .and_then(|value| value.as_str())
                    == Some(role)
        })
        .map(|node| node.symbol)
        .collect()
}

fn assert_stable_owner_partitioned_value_role(
    before: &SqliteStore,
    after_line_shift: &SqliteStore,
    name: &str,
    role: &str,
    owner_needles: &[&str],
) {
    let before_symbols = value_role_symbols(before, name, role);
    let shifted_symbols = value_role_symbols(after_line_shift, name, role);
    assert_eq!(
        before_symbols.len(),
        owner_needles.len(),
        "{role} value {name:?} must be partitioned by logical owner; got {before_symbols:?}"
    );
    assert_eq!(
        shifted_symbols.len(),
        owner_needles.len(),
        "{role} value {name:?} must survive line shift with the same owner count; got {shifted_symbols:?}"
    );
    assert_eq!(
        before_symbols, shifted_symbols,
        "{role} value {name:?} identities must survive unrelated line shifts"
    );
    for owner in owner_needles {
        let matches: Vec<_> = before_symbols
            .iter()
            .filter(|symbol| symbol.as_str().contains(owner))
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "{role} value {name:?} must have exactly one symbol owned by {owner:?}; got {before_symbols:?}"
        );
    }
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

#[test]
fn direct_typeflow_edges_carry_parsed_evidence() {
    let source = r#"
        class CustomerComponent {
            load(customer: any, a: string) {
                let c = a;
                const id = customer.id;
            }
        }
    "#;
    let (root, store) = indexed_typescript("evidence", source);
    let pairs = semantic_flow_name_pairs(&store);
    for expected in [("a", "c"), ("customer.id", "id")] {
        assert!(
            pairs.contains(&(expected.0.to_string(), expected.1.to_string())),
            "missing semantic-forward flow {expected:?}; got {pairs:?}"
        );
    }

    for edge in semantic_flow_edges(&store) {
        assert!(
            edge.location.is_some(),
            "flows_to edge must carry syntax location: {edge:?}"
        );
        assert!(
            edge.metadata.contains_key("construct"),
            "flows_to edge must carry metadata.construct: {edge:?}"
        );
        assert_eq!(edge.provenance, Provenance::Parsed);
        assert_eq!(edge.resolved_by, "tree-sitter");
        assert!(
            (edge.confidence.get() - 1.0).abs() < f32::EPSILON,
            "parsed flows_to confidence must be 1.0: {edge:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn value_identity_is_stable_across_line_shifts_and_owner_partitioned() {
    let source = r#"
        import { Input } from '@angular/core';
        class One {
            @Input() tenantId!: string;
            same(paramOnly: string, customer: any, route: any) {
                const routeId = route.snapshot.paramMap.get('id');
                const localValue = routeId;
                const propValue = customer.id;
                return propValue;
            }
        }
        class Two {
            @Input() tenantId!: string;
            same(paramOnly: string, customer: any, route: any) {
                const routeId = route.snapshot.paramMap.get('id');
                const localValue = routeId;
                const propValue = customer.id;
                return propValue;
            }
        }
    "#;
    let shifted = format!("\n\n{source}");
    let (root_a, store_a) = indexed_typescript("identity_a", source);
    let (root_b, store_b) = indexed_typescript("identity_b", &shifted);

    for (name, role) in [
        ("localValue", "Local"),
        ("paramOnly", "Parameter"),
        ("same.return", "Return"),
        ("customer.id", "Property"),
        ("AngularInput:tenantId", "AngularInput"),
        ("RouteParam:id", "RouteParam"),
    ] {
        assert_stable_owner_partitioned_value_role(
            &store_a,
            &store_b,
            name,
            role,
            &["One#", "Two#"],
        );
    }

    let _ = fs::remove_dir_all(root_a);
    let _ = fs::remove_dir_all(root_b);
}

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

#[test]
fn cross_file_service_call_uses_resolved_exact_call_site_for_value_flow() {
    let service = r#"
        export class CustomerService {
            getCustomer(id: string): string { return id; }
        }
    "#;
    let component = r#"
        import { CustomerService } from './service';
        class CustomerComponent {
            loadCustomer(service: CustomerService, customerId: string) {
                const customer = service.getCustomer(customerId);
            }
        }
    "#;
    let (root, store) = indexed_typescript_files(
        "cross_file_call",
        &[("service.ts", service), ("component.ts", component)],
    );
    let pairs = semantic_flow_name_pairs(&store);
    for expected in [
        ("customerId", "id"),
        ("id", "getCustomer.return"),
        ("getCustomer.return", "customer"),
    ] {
        assert!(
            pairs.contains(&(expected.0.to_string(), expected.1.to_string())),
            "missing cross-file semantic-forward flow {expected:?}; got {pairs:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn call_derived_typeflow_edges_carry_resolver_site_evidence() {
    let source = r#"
        class CustomerService {
            getCustomer(id: string): string { return id; }
        }
        class CustomerComponent {
            loadCustomer(service: CustomerService, customerId: string) {
                const customer = service.getCustomer(customerId);
            }
        }
    "#;
    let (root, store) = indexed_typescript("call_evidence", source);

    let mut resolved_calls_by_site: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for edge in GraphRead::all_edges(&store)
        .unwrap()
        .into_iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
    {
        let site = edge_site_key(&edge).expect("resolved Calls edge must carry exact site");
        resolved_calls_by_site.entry(site).or_default().push(edge);
    }

    let call_edges: Vec<_> = semantic_flow_edges(&store)
        .into_iter()
        .filter(|edge| {
            matches!(
                edge.metadata.get("construct").and_then(|v| v.as_str()),
                Some("call_argument" | "call_result")
            )
        })
        .collect();
    assert!(
        !call_edges.is_empty(),
        "expected call-derived flows_to edges"
    );
    let constructs: BTreeSet<_> = call_edges
        .iter()
        .filter_map(|edge| {
            edge.metadata
                .get("construct")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(
        constructs,
        BTreeSet::from(["call_argument".to_string(), "call_result".to_string()]),
        "expected both call argument and call result flows; got {constructs:?}"
    );
    for edge in call_edges {
        let site = edge_site_key(&edge).unwrap_or_else(|| {
            panic!("call-derived flows_to edge must carry exact site: {edge:?}")
        });
        let matching_calls = resolved_calls_by_site
            .get(&site)
            .unwrap_or_else(|| panic!("missing resolved Calls edge at site {site:?} for {edge:?}"));
        assert_eq!(
            matching_calls.len(),
            1,
            "test fixture should have one resolved Calls edge at site {site:?}; got {matching_calls:?}"
        );
        let call_edge = &matching_calls[0];
        assert_eq!(
            edge.location, call_edge.location,
            "call-derived flows_to edge must inherit exact Calls location"
        );
        assert!(
            (edge.confidence.get() - call_edge.confidence.get()).abs() < f32::EPSILON,
            "call-derived flows_to confidence must match resolved Calls confidence: flow {edge:?}, call {call_edge:?}"
        );
        assert_eq!(
            edge.provenance, call_edge.provenance,
            "call-derived flows_to provenance must match resolved Calls provenance"
        );
        assert_eq!(
            edge.resolved_by, call_edge.resolved_by,
            "call-derived flows_to resolved_by must match resolved Calls resolver"
        );
        assert!(
            edge.metadata.contains_key("construct"),
            "call-derived flows_to edge must carry metadata.construct: {edge:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unresolved_and_ambiguous_calls_emit_no_parameter_or_return_flow_targets() {
    let source = r#"
        class One {
            normalize(id: string): string { return id; }
        }
        class Two {
            normalize(id: string): string { return id; }
        }
        class CustomerComponent {
            loadCustomer(raw: string) {
                const first = normalize(raw);
                missing(raw);
            }
        }
    "#;
    let (root, store) = indexed_typescript("call_negative", source);
    let pairs = semantic_flow_name_pairs(&store);
    for forbidden in [("raw", "id"), ("normalize.return", "first")] {
        assert!(
            !pairs.contains(&(forbidden.0.to_string(), forbidden.1.to_string())),
            "ambiguous or unresolved call emitted forbidden flow {forbidden:?}; got {pairs:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn supported_call_arguments_keep_original_parameter_slots() {
    let source = r#"
        function normalize(skip: number, id: string): string { return id; }
        class CustomerComponent {
            loadCustomer(raw: string) {
                const customer = normalize(0, raw);
            }
        }
    "#;
    let (root, store) = indexed_typescript("arg_slots", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("raw".to_string(), "id".to_string())),
        "supported second argument should flow to second parameter; got {pairs:?}"
    );
    assert!(
        !pairs.contains(&("raw".to_string(), "skip".to_string())),
        "unsupported first argument must not shift raw onto first parameter; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn angular_fixture_flows_to_lineage_recovers_route_and_input_chains() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &fixture).expect("index Angular TypeScript fixture");

    let route_sources = symbol_ids_named(&store, "RouteParam:id");
    assert_eq!(
        route_sources.len(),
        1,
        "fixture should have one stable RouteParam:id source"
    );
    let route_source = route_sources.iter().next().unwrap().as_str().to_string();
    let lineage = Lineage
        .invoke(
            &store,
            &serde_json::json!({"symbol": route_source, "depth": 8, "relation": "flows_to"}),
        )
        .unwrap();
    let route_symbols: BTreeSet<_> = lineage.content["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| SymbolId(node["symbol"].as_str().unwrap().to_string()))
        .collect();

    let route_id =
        one_symbol_named_with(&store, "routeId", "CustomerComponent#load().:local:routeId");
    let customer_id =
        one_symbol_named_with(&store, "customerId", "CustomerComponent#:field:customerId");
    let load_customer_id =
        one_symbol_named_with(&store, "id", "CustomerComponent#loadCustomer().:local:id");
    let service_get_customer_id =
        one_symbol_named_with(&store, "id", "CustomerService#getCustomer().:local:id");
    let service_return = one_symbol_named_with(
        &store,
        "getCustomer.return",
        "CustomerService#getCustomer().:return:value",
    );

    for expected in [
        route_id.clone(),
        customer_id.clone(),
        load_customer_id.clone(),
        service_get_customer_id.clone(),
        service_return.clone(),
    ] {
        assert!(
            route_symbols.contains(&expected),
            "RouteParam:id semantic lineage missing {expected:?}; got {route_symbols:?}"
        );
    }

    let pairs = semantic_flow_symbol_pairs(&store);
    for (producer, consumer) in [
        (
            route_sources.iter().next().unwrap().clone(),
            route_id.clone(),
        ),
        (route_id.clone(), customer_id.clone()),
        (customer_id.clone(), load_customer_id.clone()),
        (load_customer_id.clone(), service_get_customer_id.clone()),
        (service_get_customer_id.clone(), service_return.clone()),
    ] {
        assert!(
            pairs.contains(&(producer.clone(), consumer.clone())),
            "missing exact owner-partitioned semantic edge {producer:?} -> {consumer:?}; got {pairs:?}"
        );
    }
    assert!(
        lineage.content["confidence"]["edge_count"]
            .as_u64()
            .unwrap()
            >= 4,
        "route lineage should traverse the persisted fixture chain"
    );

    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("AngularInput:tenantId".to_string(), "tenantId".to_string())),
        "Angular input chain missing from persisted flows_to graph; got {pairs:?}"
    );

    let default = Lineage
        .invoke(
            &store,
            &serde_json::json!({"symbol": route_source, "depth": 8}),
        )
        .unwrap();
    assert_eq!(
        default.content["total"].as_u64(),
        Some(0),
        "default Lineage behavior must not follow flows_to unless relation is selected"
    );
}

#[test]
fn incremental_callee_only_edit_preserves_call_derived_value_flow() {
    let service_v1 = r#"
        export class CustomerService {
            getCustomer(id: string): string { return id; }
        }
    "#;
    let component = r#"
        import { CustomerService } from './service';
        class CustomerComponent {
            loadCustomer(service: CustomerService, customerId: string) {
                const customer = service.getCustomer(customerId);
                return customer;
            }
        }
    "#;
    let (root, mut store) = indexed_typescript_files(
        "incremental_callee_flow",
        &[("service.ts", service_v1), ("component.ts", component)],
    );

    let customer_id = one_symbol_named_with(
        &store,
        "customerId",
        "CustomerComponent#loadCustomer().:local:customerId",
    );
    let service_id = one_symbol_named_with(&store, "id", "CustomerService#getCustomer().:local:id");
    let service_return = one_symbol_named_with(
        &store,
        "getCustomer.return",
        "CustomerService#getCustomer().:return:value",
    );
    let customer = one_symbol_named_with(
        &store,
        "customer",
        "CustomerComponent#loadCustomer().:local:customer",
    );

    let assert_call_flow = |store: &SqliteStore| {
        let pairs = semantic_flow_symbol_pairs(store);
        for (producer, consumer) in [
            (customer_id.clone(), service_id.clone()),
            (service_id.clone(), service_return.clone()),
            (service_return.clone(), customer.clone()),
        ] {
            assert!(
                pairs.contains(&(producer.clone(), consumer.clone())),
                "missing call-derived flow after incremental index {producer:?} -> {consumer:?}; got {pairs:?}"
            );
        }
    };
    assert_call_flow(&store);

    let service_v2 = r#"
        export class CustomerService {
            getCustomer(id: string): string {
                // callee-only implementation edit; parameter identity is unchanged.
                return id;
            }
        }
    "#;
    fs::write(root.join("service.ts"), service_v2).unwrap();
    wicked_estate::index_path(&mut store, &root).expect("incremental re-index after callee edit");

    assert_call_flow(&store);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn incremental_backfill_adds_call_value_flow_for_previously_parked_call() {
    let caller = r#"
        class CustomerComponent {
            loadCustomer(customerId: string) {
                const customer = getCustomer(customerId);
                return customer;
            }
        }
    "#;
    let (root, mut store) =
        indexed_typescript_files("incremental_backfill_flow", &[("component.ts", caller)]);

    let callee = r#"
        export function getCustomer(id: string): string {
            return id;
        }
    "#;
    fs::write(root.join("service.ts"), callee).unwrap();
    wicked_estate::index_path(&mut store, &root).expect("incremental re-index after callee add");

    let customer_id = one_symbol_named_with(
        &store,
        "customerId",
        "CustomerComponent#loadCustomer().:local:customerId",
    );
    let service_id = one_symbol_named_with(&store, "id", "getCustomer().:local:id");
    let service_return =
        one_symbol_named_with(&store, "getCustomer.return", "getCustomer().:return:value");
    let customer = one_symbol_named_with(
        &store,
        "customer",
        "CustomerComponent#loadCustomer().:local:customer",
    );

    let pairs = semantic_flow_symbol_pairs(&store);
    for (producer, consumer) in [
        (customer_id, service_id.clone()),
        (service_id, service_return.clone()),
        (service_return, customer),
    ] {
        assert!(
            pairs.contains(&(producer.clone(), consumer.clone())),
            "missing backfilled call-derived flow {producer:?} -> {consumer:?}; got {pairs:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

// ── Regression guards for the wicked-estate#207 review findings ──────────────────────────────
//
// Every test below fails on the pre-fix branch. They are the tests whose absence let a graph-
// quality defect ship green: the PR's own suite asserts what the feature ADDS, these assert what
// it must not DISTURB.

/// The (source, target, kind) set of the whole stored graph — the exact comparison the review's
/// real-repo index diff made, reduced to a fixture.
fn edge_triples(store: &SqliteStore) -> BTreeSet<(String, String, String)> {
    GraphRead::all_edges(store)
        .unwrap()
        .into_iter()
        .map(|edge| {
            (
                edge.source.0,
                edge.target.0,
                serde_json::to_string(&edge.kind).unwrap(),
            )
        })
        .collect()
}

fn value_flow_symbols(store: &SqliteStore) -> BTreeSet<SymbolId> {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|node| node.is_value_flow_node())
        .map(|node| node.symbol)
        .collect()
}

/// C1 — a synthetic value slot must never be the target of a `Calls` edge. On a real 905-file
/// TypeScript repo the missing guard minted 1,077 false `calls` edges onto 8 locals, gave a local
/// named `map` 915 dependents and made it the repo's top-ranked symbol.
#[test]
fn no_calls_edge_targets_a_value_node() {
    let assert_no_value_call_targets = |store: &SqliteStore, label: &str| {
        let value_symbols = value_flow_symbols(store);
        assert!(
            !value_symbols.is_empty(),
            "{label} must mint value nodes for this guard to mean anything"
        );
        let offenders: Vec<_> = GraphRead::all_edges(store)
            .unwrap()
            .into_iter()
            .filter(|edge| edge.kind == EdgeKind::Calls && value_symbols.contains(&edge.target))
            .map(|edge| (edge.source.0, edge.target.0))
            .collect();
        assert!(
            offenders.is_empty(),
            "{label}: Calls edges must never target a synthetic value slot; got {offenders:?}"
        );
    };

    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &fixture).expect("index Angular TypeScript fixture");
    assert_no_value_call_targets(&store, "committed Angular fixture");

    // The committed fixture alone is NOT a guard: it holds no value node whose bare name is also
    // called anywhere, which is precisely why AC-0005 held in the unit fixture and failed on a
    // real 905-file repo. The name collision has to be in the corpus.
    let (root, collision) = indexed_typescript_files("calls_target_guard", &COLLISION_FILES);
    assert_no_value_call_targets(&collision, "name-collision corpus");
    let _ = fs::remove_dir_all(root);
}

/// A local named `map` in one file, called as `map(...)` from two others — the real shape
/// (`src/interactive/takes.ts` vs. every `map(...)` call in the repo), and the corpus C1 and C3
/// both need: the committed fixture has no such collision.
const COLLISION_FILES: [(&str, &str); 3] = [
    (
        "takes.ts",
        r#"
        export function asksByVersion(rows: string): string {
            const map = rows;
            return map;
        }
        "#,
    ),
    (
        "consumer_one.ts",
        r#"
        export function render(items: string): string {
            const out = map(items);
            return out;
        }
        "#,
    ),
    (
        "consumer_two.ts",
        r#"
        export function reload(items: string): string {
            const again = map(items);
            return again;
        }
        "#,
    ),
];

/// C1 in the shape that actually bit: a bare local whose name collides with a called function in
/// another file.
#[test]
fn a_local_named_like_a_callee_absorbs_no_calls() {
    let (root, store) = indexed_typescript_files("local_name_collision", &COLLISION_FILES);

    let local_map = one_symbol_named_with(&store, "map", "asksByVersion().:local:map:");
    let dependents: Vec<_> = GraphRead::neighbors(
        &store,
        &local_map,
        wicked_estate_core::Direction::Dependents,
    )
    .unwrap()
    .into_iter()
    .filter(|edge| edge.kind == EdgeKind::Calls)
    .map(|edge| edge.source.0)
    .collect();
    assert!(
        dependents.is_empty(),
        "a local must not acquire Calls dependents; got {dependents:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// C13 — value nodes must not form an island: each one is contained by its File, the way an
/// ordinary local is (Contains stays File→node, D4). Before this, 0 of 3,403 value nodes on a real
/// repo had a containment parent and the ONLY edge joining a real code node to the value graph was
/// the spurious `Calls` edges C1 removes.
#[test]
fn value_nodes_are_reachable_from_a_real_node() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &fixture).expect("index Angular TypeScript fixture");

    let value_symbols = value_flow_symbols(&store);
    assert!(!value_symbols.is_empty(), "fixture must mint value nodes");
    let mut orphans = Vec::new();
    for symbol in &value_symbols {
        let parents: Vec<_> =
            GraphRead::neighbors(&store, symbol, wicked_estate_core::Direction::Dependents)
                .unwrap()
                .into_iter()
                .filter(|edge| edge.kind == EdgeKind::Contains)
                .collect();
        if parents.is_empty() {
            orphans.push(symbol.0.clone());
        } else {
            for edge in parents {
                let parent = GraphRead::get_node(&store, &edge.source).unwrap();
                assert_eq!(
                    parent.map(|node| node.kind),
                    Some(wicked_estate_core::NodeKind::File),
                    "value containment must come from the File node (D4)"
                );
            }
        }
    }
    assert!(
        orphans.is_empty(),
        "every value node needs a containment parent; orphans: {orphans:?}"
    );
}

/// C2 — a leaf edit must force only the callee's DIRECT callers into re-extraction. The transitive
/// reverse-`Calls` fixed point this replaces turned a one-line edit to a leaf test file into a
/// 719-of-905-file, 20.7 s, 1.2 GB re-index on a real repo — three times its own full index — and
/// `wicked-estate watch` paid it on every save.
#[test]
fn incremental_leaf_edit_forces_only_direct_callers() {
    let leaf = r#"
        export function leaf(seed: string): string {
            return seed;
        }
    "#;
    let mid = r#"
        import { leaf } from './leaf';
        export function mid(seed: string): string {
            const midValue = leaf(seed);
            return midValue;
        }
    "#;
    let top = r#"
        import { mid } from './mid';
        export function top(seed: string): string {
            const topValue = mid(seed);
            return topValue;
        }
    "#;
    let roof = r#"
        import { top } from './top';
        export function roof(seed: string): string {
            const roofValue = top(seed);
            return roofValue;
        }
    "#;
    let (root, mut store) = indexed_typescript_files(
        "incremental_leaf_scope",
        &[
            ("leaf.ts", leaf),
            ("mid.ts", mid),
            ("top.ts", top),
            ("roof.ts", roof),
        ],
    );

    // The chain must actually be resolved, or the forcing logic has nothing to walk.
    let calls = GraphRead::all_edges(&store)
        .unwrap()
        .into_iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .count();
    assert!(
        calls >= 3,
        "fixture chain must resolve its calls; got {calls}"
    );

    let cursor = GraphRead::changes_since(&store, 0)
        .unwrap()
        .iter()
        .map(|change| change.seq)
        .max()
        .unwrap_or(0);

    fs::write(root.join("leaf.ts"), format!("{leaf}\n// one-line edit\n")).unwrap();
    wicked_estate::index_path(&mut store, &root).expect("incremental re-index after leaf edit");

    let touched: BTreeSet<String> = GraphRead::changes_since(&store, cursor)
        .unwrap()
        .into_iter()
        .map(|change| change.target)
        .collect();
    assert_eq!(
        touched,
        ["leaf.ts", "mid.ts"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>(),
        "only the edited leaf and its DIRECT caller may be re-extracted"
    );

    // …and the one-hop invariant the forcing exists for still holds.
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("seed".to_string(), "seed".to_string())),
        "call-derived flow into the edited callee's parameter must survive; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// C3 — one tree, one graph: a full index and a full index followed by a touch-and-re-index must
/// produce the same `(source, target, kind)` set. They diverged by 520 `calls` edges on a real
/// repo, so `blast-radius map` answered 915, 2 or 0 depending only on how you had indexed.
#[test]
fn full_and_incremental_index_of_one_tree_agree() {
    let (root, mut incremental) = indexed_typescript_files("full_vs_incremental", &COLLISION_FILES);
    let full_only = {
        let mut store = SqliteStore::in_memory().expect("open sqlite");
        wicked_estate::index_path(&mut store, &root).expect("independent full index");
        edge_triples(&store)
    };

    // Touch the file that OWNS the value node; its callers live in other files and are not
    // re-extracted, which is how 519 `calls` edges into `:local:map:` vanished on a real repo.
    let touched = format!("{}\n// unrelated comment\n", COLLISION_FILES[0].1);
    fs::write(root.join("takes.ts"), touched).unwrap();
    wicked_estate::index_path(&mut incremental, &root).expect("incremental re-index");
    // Restore the byte-identical content and re-index, so both graphs describe the same tree.
    fs::write(root.join("takes.ts"), COLLISION_FILES[0].1).unwrap();
    wicked_estate::index_path(&mut incremental, &root).expect("incremental re-index back");

    let incremental_triples = edge_triples(&incremental);
    let lost: Vec<_> = full_only.difference(&incremental_triples).collect();
    let gained: Vec<_> = incremental_triples.difference(&full_only).collect();
    assert!(
        lost.is_empty() && gained.is_empty(),
        "full and incremental graphs of one tree must be identical; lost {lost:?}, gained {gained:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// #229 — a leaf edit forces its direct caller `mid` to re-extract (one hop), and `remove_file`
/// deletes every edge SOURCED at `mid`'s nodes — including the call_argument edge from `top`'s
/// argument into `mid`'s parameter, which `top` (unchanged, not re-extracted) owns. The incremental
/// graph lost exactly those edges (446 on a 564-file TS repo); it must equal a full index.
#[test]
fn incremental_edit_keeps_call_derived_edges_owned_by_unforced_callers_229() {
    let files = [
        (
            "leaf.ts",
            "export function leaf(seed: string): string {\n    return seed;\n}\n",
        ),
        (
            "mid.ts",
            "import { leaf } from './leaf';\nexport function mid(seed: string): string {\n    const midValue = leaf(seed);\n    return midValue;\n}\n",
        ),
        (
            "top.ts",
            "import { mid } from './mid';\nexport function top(seed: string): string {\n    const topValue = mid(seed);\n    return topValue;\n}\n",
        ),
    ];
    let (root, mut incremental) = indexed_typescript_files("incremental_collateral_229", &files);
    fs::write(
        root.join("leaf.ts"),
        format!("{}// one-line edit\n", files[0].1),
    )
    .unwrap();
    wicked_estate::index_path(&mut incremental, &root).expect("incremental re-index");

    let full = {
        let mut store = SqliteStore::in_memory().expect("open sqlite");
        wicked_estate::index_path(&mut store, &root).expect("full index of the edited tree");
        edge_triples(&store)
    };
    let inc = edge_triples(&incremental);
    let lost: Vec<_> = full.difference(&inc).collect();
    let gained: Vec<_> = inc.difference(&full).collect();
    assert!(
        lost.is_empty() && gained.is_empty(),
        "incremental must equal full after a leaf edit; lost {lost:?}, gained {gained:?}"
    );
    // The edge the bug dropped: the call_argument hop `top`'s `seed` → `mid`'s parameter, sourced
    // at mid's parameter node but owned by top's call site.
    assert!(
        GraphRead::all_edges(&incremental).unwrap().iter().any(|e| {
            e.kind == edge_tags::other(edge_tags::FLOWS_TO)
                && e.source.0.contains("mid().:local:seed")
                && e.location.as_ref().is_some_and(|l| l.file == "top.ts")
        }),
        "top's argument must still flow into mid's parameter after the leaf edit"
    );
    let _ = fs::remove_dir_all(root);
}

/// C5a — the canonical Angular/RxJS shape. A `return` inside a callback is the callback's value;
/// attributing it to the enclosing method asserted, at confidence 1.00, that `loadCustomer`
/// returns the subscribe payload when it returns an entirely different local.
#[test]
fn callback_return_is_not_the_enclosing_methods_return() {
    let source = r#"
        export class CustomerComponent {
            cached: string = "";
            loadCustomer(id: string, svc: any): string {
                svc.get(id).subscribe((customer: string) => { return customer; });
                const fallback = this.cached;
                return fallback;
            }
        }
    "#;
    let (root, store) = indexed_typescript("callback_return", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("fallback".to_string(), "loadCustomer.return".to_string())),
        "the method's own return value must still be recorded; got {pairs:?}"
    );
    assert!(
        !pairs.contains(&("customer".to_string(), "loadCustomer.return".to_string())),
        "a callback's return must not be attributed to the enclosing method; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// C5a — the guard is scoped to ANONYMOUS callables: an arrow bound to a name is its own
/// definition, so its `return` is still its own return value.
#[test]
fn named_arrow_function_keeps_its_return_flow() {
    let source = r#"
        export const pick = (chosen: string): string => {
            return chosen;
        };
    "#;
    let (root, store) = indexed_typescript("named_arrow_return", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("chosen".to_string(), "pick.return".to_string())),
        "a named arrow function's return must still flow; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// C5d — argument→parameter joins are by explicit slot. With a destructured first parameter the
/// compressed index space stored `token <== ctx` (a location and confidence 0.65 on a flow that
/// does not exist) and dropped the real `secret -> token` hop entirely.
#[test]
fn call_arguments_join_parameters_by_slot_not_by_capture_order() {
    let source = r#"
        export function send({ trace }: any, token: string): void {}
        export function caller(ctx: any, secret: string): void {
            send(ctx, secret);
        }
    "#;
    let (root, store) = indexed_typescript("slot_exact_join", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        !pairs.contains(&("ctx".to_string(), "token".to_string())),
        "argument 0 must not land on the parameter in slot 1; got {pairs:?}"
    );
    assert!(
        pairs.contains(&("secret".to_string(), "token".to_string())),
        "argument 1 must reach the parameter in slot 1; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// C5e — a callable's parameter slots are its OWN. The byte-range scan this replaces gave `outer`
/// its nested `inner`'s parameter, so `outer(a, b)`'s second argument (binding an optional
/// parameter nothing captures) landed on `inner`'s `y`.
#[test]
fn nested_and_child_parameters_are_not_absorbed_by_the_owner() {
    let source = r#"
        export function outer(x: string, flag?: string): void {
            const y = x;
            function inner(y: string): void {}
            inner(x);
        }
        export function drive(a: string, b: string): void {
            outer(a, b);
        }
    "#;
    let (root, store) = indexed_typescript("nested_params", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        !pairs.contains(&("b".to_string(), "y".to_string())),
        "an optional parameter must lose its hop, not land on a nested callable's parameter; got {pairs:?}"
    );
    assert!(
        pairs.contains(&("a".to_string(), "x".to_string())),
        "the capturable first argument must still reach slot 0; got {pairs:?}"
    );

    // …and a class never takes its methods' parameters.
    let class_source = r#"
        export class Svc {
            constructor(http: string) {}
            fetch(a: string, b: string): void {}
        }
    "#;
    let (class_root, class_store) = indexed_typescript("class_params", class_source);
    let class_params: Vec<_> = GraphRead::all_nodes(&class_store)
        .unwrap()
        .into_iter()
        .filter(|node| node.kind == wicked_estate_core::NodeKind::Class)
        .filter_map(|node| node.metadata.get("value_params").cloned())
        .collect();
    assert!(
        class_params.is_empty(),
        "a class must not carry its methods' parameters; got {class_params:?}"
    );
    let _ = fs::remove_dir_all(class_root);
    let _ = fs::remove_dir_all(root);
}

/// Adjudication of a codex-cli review finding on the C5a barrier: a NAMED definition nested
/// inside an anonymous callback is still its own owner, so its `return` must survive.
#[test]
fn a_named_definition_nested_in_a_callback_keeps_its_return() {
    let source = r#"
        export function outerFn(items: any, value: string): void {
            items.map(() => {
                function inner(): string {
                    return value;
                }
                return inner;
            });
        }
    "#;
    let (root, store) = indexed_typescript("nested_def_in_callback", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("value".to_string(), "inner.return".to_string())),
        "a named function nested in a callback must keep its own return flow; got {pairs:?}"
    );
    assert!(
        !pairs.contains(&("value".to_string(), "outerFn.return".to_string())),
        "…and it must not leak to the enclosing definition; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// Adjudication of a codex-cli review finding on the C2 gate: when the store holds NO `flows_to`
/// edge yet, an edit that makes an existing callee flow-capable must still force its callers —
/// their call-site facts are transient, so re-extracting only the callee can never mint the
/// argument→parameter edge, and the incremental graph would diverge from a full index.
#[test]
fn an_edit_that_first_makes_a_callee_flow_capable_forces_its_callers() {
    // Nothing here produces a flows_to edge: the callee's only parameter is destructured, and
    // no function returns a bare identifier.
    let callee_v1 = r#"
        export function send({ trace }: any): void {}
    "#;
    let caller = r#"
        import { send } from './callee';
        export function drive(secret: string): void {
            send(secret);
        }
    "#;
    let (root, mut store) = indexed_typescript_files(
        "flow_capable_transition",
        &[("callee.ts", callee_v1), ("caller.ts", caller)],
    );
    assert!(
        semantic_flow_edges(&store).is_empty(),
        "precondition: the store must hold no flows_to edge yet"
    );

    // The callee's parameter becomes capturable. The caller is untouched on disk.
    fs::write(
        root.join("callee.ts"),
        "\n        export function send(trace: string): void {}\n    ",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &root).expect("incremental re-index");

    let incremental = semantic_flow_name_pairs(&store);

    let mut fresh = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut fresh, &root).expect("full index of the same tree");
    let full = semantic_flow_name_pairs(&fresh);

    assert_eq!(
        incremental, full,
        "incremental and full value-flow graphs of one tree must agree"
    );
    assert!(
        full.contains(&("secret".to_string(), "trace".to_string())),
        "the newly capturable parameter must receive the caller's argument; got {full:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// #210 — a callee with no `return <ident>` has no return endpoint, so a call into it mints no
/// `call_result` hop (it used to be minted and then pruned as dangling, orphaning the consumer);
/// the argument hop into its parameter is unaffected.
#[test]
fn call_into_a_void_callee_mints_no_call_result_hop_210() {
    let source = "export function voidFn(input: string): void {}\n\
                  export function caller(input: string): void { const dangling = voidFn(input); }\n";
    let (root, store) = indexed_typescript("void_callee_210", source);
    let names: BTreeMap<SymbolId, String> = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol, n.name))
        .collect();
    for e in GraphRead::all_edges(&store).unwrap() {
        assert!(
            names.contains_key(&e.source) && names.contains_key(&e.target),
            "no edge may point at a missing node: {e:?}"
        );
        if e.kind == edge_tags::other(edge_tags::FLOWS_TO) {
            assert_ne!(
                names[&e.source], "dangling",
                "a void callee has no result to flow into `dangling`: {e:?}"
            );
        }
    }
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("input".to_string(), "input".to_string())),
        "the argument hop survives; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// #208 / #214 — a property read's identity is its `object.property` path (a prettier wrap and
/// optional chaining mint the SAME node), and the node is `Synthetic`, not a function-owned `Field`.
#[test]
fn property_reads_are_path_keyed_synthetic_nodes_208_214() {
    let source = "export function read(customer: any, raw: any): void {\n\
                  \x20   const a = customer.id;\n\
                  \x20   const b = customer\n\
                  \x20       .id;\n\
                  \x20   const c = raw?.name;\n\
                  \x20   const d = raw.name;\n\
                  }\n";
    let (root, store) = indexed_typescript("property_identity_208", source);
    let props: Vec<_> = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .filter(|n| n.symbol.0.contains(":property:"))
        .collect();
    let names: BTreeSet<String> = props.iter().map(|n| n.name.clone()).collect();
    assert_eq!(
        names,
        ["customer.id", "raw.name"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>(),
        "formatting and `?.` never change a property read's identity; got {props:?}"
    );
    assert_eq!(props.len(), 2, "one node per property path: {props:?}");
    assert!(
        props
            .iter()
            .all(|n| n.kind == wicked_estate_core::NodeKind::Synthetic),
        "a property read is Synthetic, not a Field: {props:?}"
    );
    let pairs = semantic_flow_name_pairs(&store);
    for local in ["a", "b"] {
        assert!(pairs.contains(&("customer.id".to_string(), local.to_string())));
    }
    for local in ["c", "d"] {
        assert!(pairs.contains(&("raw.name".to_string(), local.to_string())));
    }
    let _ = fs::remove_dir_all(root);
}

/// #217 — a reassignment is a value hop: `tainted` reaches `out`, so the lineage of the return
/// value is no longer silently partial.
#[test]
fn reassignment_is_a_value_hop_217() {
    let source = "export function flow(trusted: string, tainted: string): string {\n\
                  \x20   let out = trusted;\n\
                  \x20   out = tainted;\n\
                  \x20   return out;\n\
                  }\n";
    let (root, store) = indexed_typescript("reassignment_217", source);
    let pairs = semantic_flow_name_pairs(&store);
    assert!(
        pairs.contains(&("trusted".to_string(), "out".to_string())),
        "{pairs:?}"
    );
    assert!(
        pairs.contains(&("tainted".to_string(), "out".to_string())),
        "the reassignment must be recorded; got {pairs:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// #209 — two call sites in one owner passing the same identifier mint one value node; which
/// site's location it keeps must not depend on hash order, so repeated fresh indexes of one tree
/// store identical node and edge locations.
#[test]
fn value_flow_locations_are_deterministic_across_indexes_209() {
    let source = "export function sink(v: string): void {}\n\
                  export function twice(x: string): void {\n\
                  \x20   sink(x);\n\
                  \x20   sink(x);\n\
                  \x20   const y = x;\n\
                  \x20   sink(y);\n\
                  \x20   sink(y);\n\
                  }\n";
    let snapshot = |tag: &str| {
        let (root, store) = indexed_typescript(tag, source);
        let nodes: BTreeMap<String, (u32, u32)> = GraphRead::all_nodes(&store)
            .unwrap()
            .into_iter()
            .map(|n| {
                (
                    n.symbol.0,
                    (n.location.span.start_byte, n.location.span.end_byte),
                )
            })
            .collect();
        let edges: BTreeSet<(String, String, Option<u32>)> = GraphRead::all_edges(&store)
            .unwrap()
            .into_iter()
            .map(|e| {
                (
                    e.source.0,
                    e.target.0,
                    e.location.map(|l| l.span.start_byte),
                )
            })
            .collect();
        let _ = fs::remove_dir_all(root);
        (nodes, edges)
    };
    let first = snapshot("determinism_209_a");
    for tag in [
        "determinism_209_b",
        "determinism_209_c",
        "determinism_209_d",
    ] {
        assert_eq!(
            snapshot(tag),
            first,
            "index {tag} stored different locations"
        );
    }
}
