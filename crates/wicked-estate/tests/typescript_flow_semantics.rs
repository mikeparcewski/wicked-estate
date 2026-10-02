//! TS-S1 regression suite: the `flows_to` relation's CLAIM is separable from its EVIDENCE, the
//! two survive endpoint deduplication, and synthetic value slots behave exactly as the published
//! visibility matrix (`docs/ENGINE-CONTRACT.md` §3.3) says they do.
//!
//! The PR #207 suite (`typescript_value_lineage.rs`) is left intact as historical evidence of what
//! that revision proved. Where TS-S1 deliberately changed an observable claim, the new assertion
//! lives here and names the change; nothing there was deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_json::json;
use wicked_estate_core::flow::{
    CONSTRUCT_KEY, FLOW_CONFIDENCE_MIN_KEY, FLOW_CONSTRUCTS_KEY, FLOW_EVIDENCE_KEY, FLOW_RULES_KEY,
    FLOW_SEMANTICS_KEY, FLOW_SUPPORT_KEY,
};
use wicked_estate_core::{
    Edge, EdgeKind, GraphRead, Node, Provenance, Ranker, RetrievalTool, SymbolId, edge_tags,
};
use wicked_estate_retrieve::Lineage;
use wicked_estate_store::SqliteStore;

// ── fixtures ─────────────────────────────────────────────────────────────────

fn indexed(tag: &str, source: &str) -> (PathBuf, SqliteStore) {
    let root = std::env::temp_dir().join(format!(
        "typescript_flow_semantics_{tag}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("fixture.ts"), source).unwrap();
    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &root).expect("index TypeScript fixture");
    (root, store)
}

fn flow_edges(store: &SqliteStore) -> Vec<Edge> {
    GraphRead::all_edges(store)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == edge_tags::other(edge_tags::FLOWS_TO))
        .collect()
}

fn names(store: &SqliteStore) -> BTreeMap<SymbolId, String> {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol, n.name))
        .collect()
}

/// The flow edge from `consumer` to `producer`, named as the source reads (not as it is stored).
fn flow(store: &SqliteStore, consumer: &str, producer: &str) -> Edge {
    let names = names(store);
    let hits: Vec<Edge> = flow_edges(store)
        .into_iter()
        .filter(|e| {
            names.get(&e.source).map(String::as_str) == Some(consumer)
                && names.get(&e.target).map(String::as_str) == Some(producer)
        })
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {consumer} <- {producer} flow edge, got {hits:?}"
    );
    hits.into_iter().next().unwrap()
}

fn strings(edge: &Edge, key: &str) -> BTreeSet<String> {
    edge.metadata
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

// ── AC-S1-01 / AC-S1-03: the two dimensions are separate and honest ──────────

/// Before TS-S1 both of these facts were a single opaque `construct` string at confidence 1.0, so
/// a caller could not tell "c IS a" from "c is built from a" at all.
#[test]
fn flow_semantics_are_separate_from_evidence_origin() {
    let source = r#"
        function build(a: string, b: string) {
            const whole = a;
            const mixed = a + b;
        }
    "#;
    let (root, store) = indexed("dimensions", source);

    let preserving = flow(&store, "whole", "a");
    assert_eq!(
        strings(&preserving, FLOW_SEMANTICS_KEY),
        set(&["value_preserving"]),
        "`const whole = a` carries a's value whole"
    );
    assert_eq!(strings(&preserving, FLOW_EVIDENCE_KEY), set(&["syntax"]));

    let influence = flow(&store, "mixed", "a");
    assert_eq!(
        strings(&influence, FLOW_SEMANTICS_KEY),
        set(&["may_influence"]),
        "an operand of `a + b` contributes to the value; it is not the value"
    );
    assert_eq!(
        strings(&influence, FLOW_EVIDENCE_KEY),
        set(&["syntax"]),
        "evidence origin is independent of the claim: both are AST-proven"
    );

    // Same evidence, different claim — which is the whole point of splitting them.
    assert_eq!(
        strings(&preserving, FLOW_EVIDENCE_KEY),
        strings(&influence, FLOW_EVIDENCE_KEY)
    );
    assert_ne!(
        strings(&preserving, FLOW_SEMANTICS_KEY),
        strings(&influence, FLOW_SEMANTICS_KEY)
    );

    let _ = fs::remove_dir_all(root);
}

// ── AC-S1-02: endpoint deduplication is non-lossy and order-independent ──────

/// The audit that drove the representation choice. Block-scoped shadowing puts a may-influence and
/// a value-preserving fact on ONE `(source, target, kind)`; on `c4fa938` the stores' `>=` upsert
/// kept whichever landed last and the other vanished with no record it had been asserted.
#[test]
fn shadowed_endpoint_collision_keeps_both_classes_and_both_sites() {
    let source = r#"
        function f(a: string, b: string) {
            const c = a + b;
            if (b) {
                const c = a;
            }
        }
    "#;
    let (root, store) = indexed("collision", source);

    let edge = flow(&store, "c", "a");
    assert_eq!(
        strings(&edge, FLOW_SEMANTICS_KEY),
        set(&["may_influence", "value_preserving"]),
        "both classifications must survive the collision: {:?}",
        edge.metadata
    );
    assert_eq!(
        strings(&edge, FLOW_CONSTRUCTS_KEY),
        set(&["assignment", "expression"])
    );

    let support = edge.metadata[FLOW_SUPPORT_KEY].as_array().unwrap();
    assert_eq!(
        support.len(),
        2,
        "each contributing site must stay explainable: {support:?}"
    );
    let sites: BTreeSet<u64> = support
        .iter()
        .map(|s| s["start_byte"].as_u64().unwrap())
        .collect();
    assert_eq!(sites.len(), 2, "the two sites must be distinguishable");

    // The pre-existing public scalar stays readable and deterministic (not last-writer-wins).
    assert_eq!(edge.metadata[CONSTRUCT_KEY], "assignment");

    let _ = fs::remove_dir_all(root);
}

/// The merged result must be a function of the input SET. Re-indexing the same tree from scratch
/// must reproduce byte-identical flow metadata, not a different winner.
#[test]
fn collision_merge_is_stable_across_independent_indexes() {
    let source = r#"
        function f(a: string, b: string) {
            const c = a + b;
            if (b) {
                const c = a;
            }
        }
    "#;
    let (root, first) = indexed("collision_stable", source);
    let mut second = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut second, &root).unwrap();

    let a = flow(&first, "c", "a");
    let b = flow(&second, "c", "a");
    assert_eq!(a.metadata, b.metadata, "the lattice must be deterministic");
    assert_eq!(a.confidence.get(), b.confidence.get());
    assert_eq!(a.resolved_by, b.resolved_by);

    let _ = fs::remove_dir_all(root);
}

// ── AC-S1-04: convention is not proof ────────────────────────────────────────

/// Amends PR #207's AC-0003 evidence claim. `@Input()` and the `route.snapshot.paramMap.get(…)`
/// chain are SHAPE matches: the AST proves the syntax, not that `Input` is `@angular/core`'s or
/// that `route` is an `ActivatedRoute`. They shipped at Parsed/1.0 and are now Heuristic/0.5.
#[test]
fn angular_convention_flow_is_heuristic_with_a_stable_rule_id() {
    let source = r#"
        import { Input } from '@angular/core';
        class CustomerComponent {
            @Input() tenantId!: string;
            load(route: any) {
                const routeId = route.snapshot.paramMap.get('id');
            }
        }
    "#;
    let (root, store) = indexed("convention", source);

    for (consumer, producer, rule) in [
        (
            "tenantId",
            "AngularInput:tenantId",
            "typescript/convention/angular_input",
        ),
        (
            "routeId",
            "RouteParam:id",
            "typescript/convention/route_param",
        ),
    ] {
        let edge = flow(&store, consumer, producer);
        assert_eq!(
            strings(&edge, FLOW_EVIDENCE_KEY),
            set(&["convention"]),
            "{consumer} must be classified as a convention match"
        );
        assert_eq!(edge.provenance, Provenance::Heuristic, "{consumer}");
        assert_eq!(edge.resolved_by, "tree-sitter-convention", "{consumer}");
        assert!(
            (edge.confidence.get() - 0.5).abs() < f32::EPSILON,
            "{consumer} must not claim 1.0: {edge:?}"
        );
        assert_eq!(strings(&edge, FLOW_RULES_KEY), set(&[rule]));
        assert!(
            edge.location.is_some(),
            "{consumer} must keep its exact source site"
        );
    }

    let _ = fs::remove_dir_all(root);
}

/// The complement: non-Angular direct syntax flow is genuinely AST-proven and stays Parsed/1.0.
/// The downgrade must be scoped to convention evidence, not applied to every flow edge.
#[test]
fn direct_syntax_flow_keeps_parsed_evidence() {
    let source = r#"
        class CustomerComponent {
            load(customer: any, a: string) {
                let c = a;
                const id = customer.id;
            }
        }
    "#;
    let (root, store) = indexed("syntax_parsed", source);

    for edge in flow_edges(&store) {
        assert_eq!(strings(&edge, FLOW_EVIDENCE_KEY), set(&["syntax"]));
        assert_eq!(edge.provenance, Provenance::Parsed, "{edge:?}");
        assert_eq!(edge.resolved_by, "tree-sitter");
        assert!(
            (edge.confidence.get() - 1.0).abs() < f32::EPSILON,
            "{edge:?}"
        );
    }

    let _ = fs::remove_dir_all(root);
}

// ── AC-S1-05: call-derived flow inherits its cause ───────────────────────────

#[test]
fn call_derived_flow_inherits_the_calls_edge_and_never_upgrades_it() {
    let source = r#"
        class CustomerService {
            getCustomer(id: string): string { return id; }
        }
        class CustomerComponent {
            loadCustomer(service: CustomerService, firstId: string) {
                const first = service.getCustomer(firstId);
            }
        }
    "#;
    let (root, store) = indexed("call_derived", source);

    let calls: Vec<Edge> = GraphRead::all_edges(&store)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .collect();
    let call = calls
        .iter()
        .find(|e| {
            names(&store)
                .get(&e.target)
                .map(String::as_str)
                .unwrap_or_default()
                == "getCustomer"
        })
        .expect("resolved call into getCustomer");

    let derived: Vec<Edge> = flow_edges(&store)
        .into_iter()
        .filter(|e| strings(e, FLOW_EVIDENCE_KEY) == set(&["call_derived"]))
        .collect();
    assert!(
        !derived.is_empty(),
        "the resolved call must derive argument/return flow"
    );

    for edge in &derived {
        assert_eq!(
            edge.confidence.get(),
            call.confidence.get(),
            "call-derived flow must inherit the causal call's confidence, not claim Parsed: {edge:?}"
        );
        assert_eq!(edge.provenance, call.provenance, "{edge:?}");
        assert_eq!(edge.resolved_by, call.resolved_by, "{edge:?}");
        assert_eq!(
            strings(edge, FLOW_SEMANTICS_KEY),
            set(&["value_preserving"]),
            "an argument binds its parameter whole: {edge:?}"
        );
        assert!(
            strings(edge, FLOW_RULES_KEY)
                .iter()
                .all(|r| r.starts_with("engine/call_derived/")),
            "{edge:?}"
        );
    }
    assert!(
        call.confidence.get() < 1.0,
        "this fixture's call is heuristically resolved — if that changes the test above stops \
         proving that uniqueness does not upgrade evidence (measured: {})",
        call.confidence.get()
    );

    let _ = fs::remove_dir_all(root);
}

/// Two sites in one caller binding the same argument to the same parameter share
/// `(source, target, kind)`. The merge must keep both sites explainable instead of silently
/// electing one site's location as THE location.
#[test]
fn repeated_identical_call_sites_keep_every_supporting_site() {
    let source = r#"
        function normalize(id: string): string { return id; }
        function caller(raw: string) {
            normalize(raw);
            normalize(raw);
        }
    "#;
    let (root, store) = indexed("repeat_sites", source);

    let edge = flow(&store, "id", "raw");
    let support = edge.metadata[FLOW_SUPPORT_KEY].as_array().unwrap();
    let sites: BTreeSet<u64> = support
        .iter()
        .map(|s| s["start_byte"].as_u64().unwrap())
        .collect();
    assert_eq!(
        sites.len(),
        2,
        "both call sites must stay visible: {support:?}"
    );

    let _ = fs::remove_dir_all(root);
}

// ── AC-S1-07: the evidence reaches a caller ──────────────────────────────────

#[test]
fn lineage_flows_to_renders_per_hop_evidence() {
    let source = r#"
        import { Input } from '@angular/core';
        class CustomerComponent {
            @Input() tenantId!: string;
        }
    "#;
    let (root, store) = indexed("lineage_evidence", source);

    let producer = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .find(|n| n.name == "AngularInput:tenantId")
        .expect("the external input node")
        .symbol;

    let result = Lineage
        .invoke(
            &store,
            &json!({"symbol": producer.as_str(), "depth": 8, "relation": "flows_to"}),
        )
        .unwrap();

    let flows = result.content["flows"]
        .as_array()
        .expect("flows_to mode must return per-hop evidence");
    assert_eq!(flows.len(), 1, "{flows:?}");
    let hop = &flows[0];
    assert_eq!(hop["producer"], producer.as_str());
    assert_eq!(hop[FLOW_EVIDENCE_KEY], json!(["convention"]));
    assert_eq!(hop[FLOW_SEMANTICS_KEY], json!(["value_preserving"]));
    assert_eq!(
        hop[FLOW_RULES_KEY],
        json!(["typescript/convention/angular_input"])
    );
    assert_eq!(hop["resolved_by"], "tree-sitter-convention");
    assert!((hop["confidence"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    assert!(hop["file"].is_string() && hop["line"].is_number());

    // R7 — "confidence is visible, low-confidence is labeled". The existing `R7-CONFIDENCE`
    // diagnostic fires strictly BELOW 0.5 and a convention edge sits exactly at the Heuristic
    // default, so the per-hop row above is what makes this hop's weakness legible. The aggregate
    // must agree rather than averaging it away.
    assert!(
        (result.content["confidence"]["min"].as_f64().unwrap() - 0.5).abs() < 1e-6,
        "the weakest hop must reach the caller: {:?}",
        result.content["confidence"]
    );

    // Default lineage is untouched by any of this.
    let default = Lineage
        .invoke(&store, &json!({"symbol": producer.as_str(), "depth": 8}))
        .unwrap();
    assert_eq!(default.content["total"].as_u64(), Some(0));
    assert!(
        default.content.get("flows").is_none(),
        "the evidence array is specific to the flows_to relation"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn exact_symbol_id_lookup_still_returns_a_value_slot() {
    let source = r#"
        function f(a: string) {
            const c = a;
        }
    "#;
    let (root, store) = indexed("exact_id", source);

    let slot = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .find(|n| n.name == "c" && n.is_value_flow_node())
        .expect("the local value slot");

    let fetched = GraphRead::get_node(&store, &slot.symbol)
        .unwrap()
        .expect("exact SymbolId lookup must keep working");
    assert_eq!(fetched.symbol, slot.symbol);

    // …while the bare NAME does not reach it.
    assert!(
        wicked_estate::search(&store, "c").unwrap().is_empty(),
        "a bare name must never resolve to a value slot"
    );

    let _ = fs::remove_dir_all(root);
}

// ── AC-S1-06: the published visibility matrix ────────────────────────────────

fn value_slots(store: &SqliteStore) -> BTreeSet<SymbolId> {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(Node::is_value_flow_node)
        .map(|n| n.symbol)
        .collect()
}

const MATRIX_FIXTURE: &str = r#"
    class CustomerService {
        getCustomer(id: string): string { return id; }
    }
    class CustomerComponent {
        load(service: CustomerService, id: string) {
            const first = service.getCustomer(id);
            const second = first;
        }
    }
"#;

#[test]
fn value_slots_are_absent_from_every_structural_surface() {
    let (root, mut store) = indexed("matrix_hidden", MATRIX_FIXTURE);
    let slots = value_slots(&store);
    assert!(!slots.is_empty(), "the fixture must produce value slots");

    // Ranked symbols / hotspots / important_symbols / the pagerank.top cache.
    let ranked = wicked_estate_rank::ranked_symbols(&store as &dyn GraphRead, &[], 1000).unwrap();
    for (id, _) in &ranked {
        assert!(!slots.contains(id), "ranked symbols must exclude {id:?}");
    }
    for (node, _) in wicked_estate::important_symbols(&store, 1000).unwrap() {
        assert!(
            !node.is_value_flow_node(),
            "important_symbols must exclude {:?}",
            node.symbol
        );
    }

    // Communities — including the package-bias ring, which is what pulls singletons back in.
    let params = wicked_estate_rank::CommunityParams {
        package_bias: 0.5,
        min_size: 1,
        ..Default::default()
    };
    for community in wicked_estate_rank::detect_communities(&store, &params).unwrap() {
        for id in community {
            assert!(!slots.contains(&id), "communities must exclude {id:?}");
        }
    }

    // Structural shape queries — these matched 100% of value slots before TS-S1.
    for (label, found) in [
        ("entrypoints", store.entrypoint_nodes().unwrap()),
        ("leaves", store.leaf_nodes().unwrap()),
        ("dead-code", store.isolated_nodes().unwrap()),
    ] {
        for node in found {
            assert!(
                !node.is_value_flow_node(),
                "{label} must exclude {:?}",
                node.symbol
            );
        }
    }

    // Default name/FTS search and budgeted context.
    assert!(wicked_estate::search(&store, "id").unwrap().is_empty());
    for node in wicked_estate_retrieve::budget_context(&store, "id", 20_000).unwrap() {
        assert!(
            !node.is_value_flow_node(),
            "budget_context must exclude {:?}",
            node.symbol
        );
    }

    let _ = &mut store;
    let _ = fs::remove_dir_all(root);
}

#[test]
fn value_slots_stay_visible_where_the_matrix_says_they_do() {
    let (root, store) = indexed("matrix_visible", MATRIX_FIXTURE);
    let slots = value_slots(&store);

    // Raw storage view: export and stats must stay faithful.
    let all: BTreeSet<SymbolId> = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .map(|n| n.symbol)
        .collect();
    assert!(
        slots.iter().all(|s| all.contains(s)),
        "all_nodes (the export/stats view) must not hide storage"
    );
    assert_eq!(
        GraphRead::stats(&store).unwrap().node_count,
        all.len() as u64,
        "stats counts what is stored"
    );

    // The PageRank INPUT keeps them, so eligible symbols' scores are unchanged by their presence.
    // (The output filter above is what hides them.)
    let scored = wicked_estate_rank::PageRank::new()
        .rank(&store as &dyn GraphRead, &[])
        .unwrap();
    assert!(
        slots.iter().any(|s| scored.contains_key(s)),
        "value slots stay in the PageRank input graph"
    );

    let _ = fs::remove_dir_all(root);
}

/// The ranked ORDER of eligible symbols is the structural-ranking contract. Hiding a synthetic
/// slot is the intended correction; renumbering real symbols would not be.
#[test]
fn hiding_value_slots_does_not_reorder_eligible_symbols() {
    let (root, store) = indexed("rank_order", MATRIX_FIXTURE);
    let slots = value_slots(&store);

    let scored = wicked_estate_rank::PageRank::new()
        .rank(&store as &dyn GraphRead, &[])
        .unwrap();
    let mut expected: Vec<(SymbolId, f32)> = scored
        .into_iter()
        .filter(|(id, _)| !slots.contains(id))
        .collect();
    expected.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.0.cmp(&b.0.0))
    });

    let ranked = wicked_estate_rank::ranked_symbols(&store as &dyn GraphRead, &[], 1000).unwrap();
    let ranked_ids: Vec<&SymbolId> = ranked.iter().map(|(id, _)| id).collect();
    let expected_ids: Vec<&SymbolId> = expected
        .iter()
        .map(|(id, _)| id)
        .filter(|id| ranked_ids.contains(id))
        .collect();
    assert_eq!(
        ranked_ids, expected_ids,
        "the surviving symbols must keep the scores and order they had WITH the value slots in \
         the input graph"
    );

    let _ = fs::remove_dir_all(root);
}

// ── AC-S1-08: nothing structural moved ───────────────────────────────────────

#[test]
fn the_calls_set_is_unchanged_by_flow_classification() {
    let (root, store) = indexed("calls_set", MATRIX_FIXTURE);
    let names = names(&store);
    let calls: BTreeSet<(String, String)> = GraphRead::all_edges(&store)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| {
            (
                names.get(&e.source).cloned().unwrap_or_default(),
                names.get(&e.target).cloned().unwrap_or_default(),
            )
        })
        .collect();

    assert_eq!(
        calls,
        BTreeSet::from([("load".to_string(), "getCustomer".to_string())]),
        "the structural Calls set for this fixture is fixed; flow classification must not add, \
         drop or retarget a call"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn full_and_incremental_indexes_agree_on_flow_classification() {
    let root = std::env::temp_dir().join(format!(
        "typescript_flow_semantics_incremental_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("fixture.ts"), MATRIX_FIXTURE).unwrap();

    let mut incremental = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut incremental, &root).unwrap();
    fs::write(
        root.join("fixture.ts"),
        format!("// touched\n{MATRIX_FIXTURE}"),
    )
    .unwrap();
    wicked_estate::index_path(&mut incremental, &root).unwrap();

    let mut full = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut full, &root).unwrap();

    let shape = |store: &SqliteStore| -> BTreeSet<(String, String, String)> {
        let names = names(store);
        flow_edges(store)
            .into_iter()
            .map(|e| {
                (
                    names.get(&e.source).cloned().unwrap_or_default(),
                    names.get(&e.target).cloned().unwrap_or_default(),
                    serde_json::to_string(&e.metadata).unwrap(),
                )
            })
            .collect()
    };
    assert_eq!(
        shape(&incremental),
        shape(&full),
        "incremental and full flow graphs — metadata included — must agree"
    );

    let _ = fs::remove_dir_all(root);
}

// ── Reindex transition: a stale contract edge must actually heal ─────────────

/// Changed derivation does not repair already-persisted edges by itself. This exercises the real
/// same-development-version path (`wicked-estate index <path> --force`, which clears the per-file
/// digests so every file re-extracts) and proves a stale classification is replaced.
#[test]
fn force_reindex_rewrites_an_edge_written_under_the_old_contract() {
    let source = r#"
        import { Input } from '@angular/core';
        class CustomerComponent {
            @Input() tenantId!: string;
        }
    "#;
    let (root, mut store) = indexed("force_reindex", source);

    // Simulate a graph written by the 0.17.0 binary: Parsed/1.0, scalar construct, no classes.
    let stale = {
        let mut edge = flow(&store, "tenantId", "AngularInput:tenantId");
        edge.confidence = wicked_estate_core::Confidence::new(1.0);
        edge.provenance = Provenance::Parsed;
        edge.resolved_by = "tree-sitter".to_string();
        edge.metadata = Default::default();
        edge.metadata
            .insert(CONSTRUCT_KEY.to_string(), json!("angular_input"));
        edge
    };
    wicked_estate_core::GraphWrite::begin_batch(&mut store).unwrap();
    wicked_estate_core::GraphWrite::upsert_edges(&mut store, &[stale]).unwrap();
    wicked_estate_core::GraphWrite::commit_batch(&mut store).unwrap();
    let before = flow(&store, "tenantId", "AngularInput:tenantId");
    assert_eq!(before.provenance, Provenance::Parsed, "stale state staged");

    // A same-version re-index alone does NOT heal it: the file digest is unchanged.
    wicked_estate::index_path(&mut store, &root).unwrap();
    assert_eq!(
        flow(&store, "tenantId", "AngularInput:tenantId").provenance,
        Provenance::Parsed,
        "same-version incremental index must be documented as insufficient, not silently assumed \
         to heal an old DB"
    );

    // `--force` clears the per-file digests, which is what makes the file re-extract.
    store.clear_file_digests_under(None).unwrap();
    wicked_estate::index_path(&mut store, &root).unwrap();

    let healed = flow(&store, "tenantId", "AngularInput:tenantId");
    assert_eq!(healed.provenance, Provenance::Heuristic);
    assert_eq!(healed.resolved_by, "tree-sitter-convention");
    assert_eq!(strings(&healed, FLOW_EVIDENCE_KEY), set(&["convention"]));
    assert!(healed.metadata.get(FLOW_CONFIDENCE_MIN_KEY).is_none());

    let _ = fs::remove_dir_all(root);
}

/// The `pagerank.top` cache outlives a code fix: a cache written by the 0.17.0 binary still holds
/// synthetic slots after this change lands, until the DB is re-indexed. `important_symbols` must
/// clean it at READ time — the same hygiene the File/Import rows already get.
#[test]
fn a_stale_pagerank_cache_cannot_serve_a_value_slot() {
    use wicked_estate_store::GraphStoreMutExt;

    let (root, mut store) = indexed("stale_cache", MATRIX_FIXTURE);
    let slots = value_slots(&store);
    assert!(!slots.is_empty());

    // Write the cache a pre-TS-S1 binary would have written: value slots ranked first.
    let poisoned: Vec<(String, f32)> = slots
        .iter()
        .map(|s| (s.0.clone(), 1.0_f32))
        .chain(std::iter::once((
            "ts-typescript . . . fixture/CustomerService#getCustomer().".to_string(),
            0.1,
        )))
        .collect();
    store.cache_put_key("pagerank.top", &serde_json::to_string(&poisoned).unwrap());

    let served = wicked_estate::important_symbols(&store, 1000).unwrap();
    assert!(
        !served.is_empty(),
        "read-time hygiene must clean the cache, not empty it"
    );
    for (node, _) in &served {
        assert!(
            !node.is_value_flow_node(),
            "a stale cache must not serve {:?}",
            node.symbol
        );
    }

    let _ = fs::remove_dir_all(root);
}
