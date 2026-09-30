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
