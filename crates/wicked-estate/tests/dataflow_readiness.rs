//! TS-S5 evidence: what the graph's `flows_to` layer says about the constructs a data-flow or taint
//! layer would need (ADR-014). Each assertion pins TODAY's behaviour — present, absent or
//! over-approximate — over qualified value identities and semantic-forward REACHABILITY, so the
//! ADR's readiness matrix is reproducible and a change to any row is a visible test diff.
//!
//! Scope: TypeScript only (`.ts`); TSX/JS share the query family but are not pinned here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use wicked_estate_core::{GraphRead, Node, flow_semantics_of, is_flow_edge};
use wicked_estate_store::SqliteStore;

/// One flow edge, semantic-forward: producer id → consumer id, with its semantics.
struct Flow {
    producer: String,
    consumer: String,
    semantics: Vec<String>,
    metadata_keys: Vec<String>,
    /// `(construct, semantics)` of every support row merged into this edge.
    supports: Vec<(String, String)>,
}

struct Graph {
    nodes: Vec<Node>,
    flows: Vec<Flow>,
}

/// Index the fixture once (tests run in parallel and would collide on one scratch root).
fn graph() -> &'static Graph {
    static G: std::sync::OnceLock<Graph> = std::sync::OnceLock::new();
    G.get_or_init(|| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dataflow_readiness");
        let root =
            std::env::temp_dir().join(format!("ci_dataflow_readiness_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        for f in ["src/cases.ts", "src/importer.ts"] {
            fs::copy(src.join(f), root.join(f)).unwrap();
        }
        let mut store = SqliteStore::open(root.join(".g.db")).unwrap();
        wicked_estate::index_path(&mut store, &root).unwrap();
        let flows = store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(is_flow_edge)
            .map(|e| Flow {
                // Stored consumer → producer (ENGINE-CONTRACT §3.2); report semantic-forward.
                producer: e.target.0.clone(),
                consumer: e.source.0.clone(),
                semantics: flow_semantics_of(&e)
                    .into_iter()
                    .map(|s| s.as_str().to_string())
                    .collect(),
                metadata_keys: e.metadata.keys().cloned().collect(),
                supports: e
                    .metadata
                    .get("flow_support")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .map(|row| {
                        let field = |k: &str| row[k].as_str().unwrap_or_default().to_string();
                        (field("construct"), field("semantics"))
                    })
                    .collect(),
            })
            .collect();
        let nodes = store.all_nodes().unwrap();
        let _ = fs::remove_dir_all(root);
        Graph { nodes, flows }
    })
}

/// The value node named `name` whose identity is owned by callable `owner` (the owner appears in
/// a value slot's qualified id, e.g. `…/destructuring().:param:obj:`); `None` when no such slot
/// exists — itself evidence (e.g. a callable whose return is not a tracked value has no
/// `.return` slot). More than one is a fixture error.
fn value_opt(owner: &str, name: &str) -> Option<String> {
    let hits: Vec<&Node> = graph()
        .nodes
        .iter()
        .filter(|n| {
            n.is_value_flow_node() && n.name == name && n.symbol.0.contains(&format!("{owner}()."))
        })
        .collect();
    assert!(
        hits.len() <= 1,
        "{owner}::{name}: {:?}",
        hits.iter().map(|n| &n.symbol.0).collect::<Vec<_>>()
    );
    hits.first().map(|n| n.symbol.0.clone())
}

fn value(owner: &str, name: &str) -> String {
    value_opt(owner, name).unwrap_or_else(|| panic!("no value slot {owner}::{name}"))
}

/// Semantic-forward reachability over every `flows_to` edge.
fn reaches(from: &str, to: &str) -> bool {
    let mut next: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for f in &graph().flows {
        next.entry(f.producer.as_str())
            .or_default()
            .push(f.consumer.as_str());
    }
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([from]);
    while let Some(n) = queue.pop_front() {
        if n == to {
            return true;
        }
        if seen.insert(n) {
            queue.extend(next.get(n).into_iter().flatten().copied());
        }
    }
    false
}

/// Present today: direct assignment chains, cross-file call arguments and results, the argument
/// into a sanitizer-like callee, and a closure's captured local.
#[test]
fn present_rows_are_reachable() {
    for (from, to) in [
        (
            value("assignment", "src"),
            value("assignment", "assignment.return"),
        ),
        (value("crossFile", "input"), value("assignment", "src")), // cross-file argument
        (
            value("assignment", "assignment.return"),
            value("crossFile", "viaImport"),
        ), // result
        (
            value("crossFile", "input"),
            value("crossFile", "crossFile.return"),
        ), // whole chain
        (value("sanitized", "raw"), value("sanitize", "raw")),     // into the sanitizer
        (value("closure", "src"), value("closure", "captured")),
    ] {
        assert!(reaches(&from, &to), "{from} should reach {to}");
    }
}

/// Absent today: each is a primitive a data-flow or taint layer would need (ADR-014 "minimum
/// missing primitives"). Checked as reachability, so a new intermediate slot cannot hide a flow.
/// A change that makes one reachable must update the ADR's matrix.
#[test]
fn missing_primitives_are_unreachable() {
    for (owner, from, to, why) in [
        (
            "loop",
            "items",
            "loop.return",
            "for-of element binding / loop-carried reassignment",
        ),
        (
            "promise",
            "src",
            "promise.return",
            "promise/callback resolution",
        ),
        (
            "property",
            "src",
            "property.return",
            "object-literal property write",
        ),
    ] {
        let from = value(owner, from);
        // A missing target slot is the strongest form of absence: nothing can reach it.
        if let Some(to) = value_opt(owner, to) {
            assert!(
                !reaches(&from, &to),
                "{why}: {from} now reaches {to} — update ADR-014"
            );
        }
    }
}

/// Over-approximate today: a branch-guarded reassignment is reported `value_preserving` with no
/// guard recorded anywhere on the edge, and the literal default is not a tracked value.
#[test]
fn branches_are_path_insensitive() {
    let (src, out) = (value("branch", "src"), value("branch", "out"));
    let edge = graph()
        .flows
        .iter()
        .find(|f| f.producer == src && f.consumer == out)
        .expect("src → out");
    assert_eq!(edge.semantics, vec!["value_preserving".to_string()]);
    assert!(
        !edge
            .metadata_keys
            .iter()
            .any(|k| k.contains("guard") || k.contains("condition") || k.contains("path")),
        "no path condition is recorded: {:?}",
        edge.metadata_keys
    );
    assert!(reaches(&src, &value("branch", "branch.return")));
    assert!(
        !graph()
            .flows
            .iter()
            .any(|f| f.producer.contains("branch().:param:flag")),
        "the guard `flag` contributes no flow"
    );
}

/// The one edge `producer → consumer`, semantic-forward.
fn edge(producer: &str, consumer: &str) -> &'static Flow {
    graph()
        .flows
        .iter()
        .find(|f| f.producer == producer && f.consumer == consumer)
        .unwrap_or_else(|| panic!("no edge {producer} → {consumer}"))
}

/// ADR-014 S5b (primitive 1, callee return composition): a returned call's result is influenced by
/// its receiver and arguments, so the sanitizer's result comes back out, and a returned
/// single-identifier arrow carries its captured local. Every new hop is `may_influence`.
#[test]
fn s5b_return_composition() {
    // Expected flows.
    for (from, to) in [
        (
            value("sanitize", "raw"),
            value("sanitize", "sanitize.return"),
        ),
        (value("sanitized", "raw"), value("sanitized", "clean")), // via the callee's result
        (
            value("sanitized", "raw"),
            value("sanitized", "sanitized.return"),
        ),
        (value("closure", "src"), value("closure", "closure.return")),
        (
            value("returnCallUnrelated", "raw"),
            value("returnCallUnrelated", "returnCallUnrelated.return"),
        ),
    ] {
        assert!(reaches(&from, &to), "{from} should reach {to}");
    }
    // Expected semantics of the new hops: contribution, never the value whole.
    for (producer, consumer, construct) in [
        (
            value("sanitize", "raw"),
            value("sanitize", "sanitize.return"),
            "return_call",
        ),
        (
            value("closure", "captured"),
            value("closure", "closure.return"),
            "return_closure",
        ),
    ] {
        let e = edge(&producer, &consumer);
        assert_eq!(e.semantics, vec!["may_influence".to_string()], "{producer}");
        assert_eq!(
            e.supports,
            vec![(construct.to_string(), "may_influence".to_string())]
        );
    }
    // Expected non-flows.
    assert!(!reaches(
        &value("returnCallUnrelated", "unrelated"),
        &value("returnCallUnrelated", "returnCallUnrelated.return")
    ));
    // A callback's `return` writes no slot of the enclosing function (the barrier law), and the
    // literal `return 'done'` mints none either.
    assert_eq!(value_opt("callbackReturn", "callbackReturn.return"), None);
    // Out of scope, pinned: a chained receiver contributes nothing.
    assert_eq!(value_opt("chainedReceiver", "chainedReceiver.return"), None);
    // Identifier arguments of a returned call each influence the result, directly.
    for arg in ["raw", "other"] {
        let e = edge(
            &value("returnCallArgs", arg),
            &value("returnCallArgs", "returnCallArgs.return"),
        );
        assert_eq!(
            e.supports,
            vec![("return_call".to_string(), "may_influence".to_string())]
        );
    }
    // A returned arrow's own parameter is no contributor: no slot at all.
    assert_eq!(
        value_opt("returnParamArrow", "returnParamArrow.return"),
        None
    );
}

/// A `return` inside a callable that mints no definition (a destructured arrow, a private arrow
/// field) belongs to no slot. Before the S5b review, the owned barrier accepted any arrow-valued
/// declarator or field, so the enclosing function or CLASS took the return.
#[test]
fn returns_of_undefined_callables_write_no_slot() {
    for owner in ["destructuredArrow", "PrivateArrow"] {
        let name = format!("{owner}.return");
        assert!(
            !graph()
                .nodes
                .iter()
                .any(|n| n.is_value_flow_node() && n.name == name),
            "{name} must not exist"
        );
    }
}

/// The readiness oracle (ADR-014 "Acceptance metrics"): every support row on every fixture edge
/// carries exactly the semantics its construct is declared with here. A construct missing from
/// the table fails, so a new one cannot land without its expected semantics.
#[test]
fn every_edge_matches_the_expected_semantics_table() {
    let expected: BTreeMap<&str, &str> = [
        ("assignment", "value_preserving"),
        ("reassignment", "value_preserving"),
        ("property_read", "value_preserving"),
        ("field_read", "value_preserving"),
        ("return", "value_preserving"),
        ("call_argument", "value_preserving"),
        ("call_result", "value_preserving"),
        ("expression", "may_influence"),
        ("return_call", "may_influence"),
        ("return_closure", "may_influence"),
        ("destructuring", "may_influence"),
    ]
    .into_iter()
    .collect();
    let mut seen = BTreeSet::new();
    for f in &graph().flows {
        assert!(
            !f.supports.is_empty(),
            "{} → {}: no support",
            f.producer,
            f.consumer
        );
        for (construct, semantics) in &f.supports {
            let want = expected
                .get(construct.as_str())
                .unwrap_or_else(|| panic!("construct {construct} is not in the table"));
            assert_eq!(
                semantics, want,
                "{construct}: {} → {}",
                f.producer, f.consumer
            );
            seen.insert(construct.clone());
        }
    }
    for construct in ["return_call", "return_closure", "destructuring"] {
        assert!(seen.contains(construct), "{construct} never fired");
    }
}

/// ADR-014 S5c (primitive 2, destructuring): a binding destructured from a value is a part of it,
/// so the hop is `may_influence`, never the value whole. Every binding shape of a flat pattern
/// takes part: shorthand, renamed, defaulted, rest, and array elements.
#[test]
fn s5c_destructuring() {
    assert!(reaches(
        &value("destructuring", "obj"),
        &value("destructuring", "destructuring.return")
    ));
    for (owner, from, to) in [
        ("destructuring", "obj", "k"),
        ("destructuringShapes", "obj", "alias"),
        ("destructuringShapes", "obj", "c"),
        ("destructuringShapes", "obj", "rest"),
        ("destructuringShapes", "arr", "first"),
        ("destructuringShapes", "arr", "others"),
    ] {
        let e = edge(&value(owner, from), &value(owner, to));
        assert_eq!(
            e.supports,
            vec![("destructuring".to_string(), "may_influence".to_string())],
            "{owner}: {from} → {to}"
        );
    }
    assert!(reaches(
        &value("destructuringShapes", "obj"),
        &value("destructuringShapes", "destructuringShapes.return")
    ));
    // Expected non-flows: another object's binding, a shadowing inner binding, a nested pattern.
    assert!(reaches(
        &value("destructuringOther", "other"),
        &value("destructuringOther", "destructuringOther.return")
    ));
    assert!(!reaches(
        &value("destructuringOther", "obj"),
        &value("destructuringOther", "destructuringOther.return")
    ));
    if let Some(ret) = value_opt("destructuringShadow", "destructuringShadow.return") {
        assert!(!reaches(&value("destructuringShadow", "obj"), &ret));
    }
    if let Some(ret) = value_opt("destructuringNested", "destructuringNested.return") {
        assert!(!reaches(&value("destructuringNested", "obj"), &ret));
    }
}
