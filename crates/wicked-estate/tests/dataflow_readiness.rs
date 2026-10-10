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
            "destructuring",
            "obj",
            "destructuring.return",
            "destructuring",
        ),
        (
            "loop",
            "items",
            "loop.return",
            "for-of element binding / loop-carried reassignment",
        ),
        (
            "closure",
            "src",
            "closure.return",
            "closure return through an arrow",
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
        (
            "sanitized",
            "raw",
            "sanitized.return",
            "result of a callee that returns an expression",
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
