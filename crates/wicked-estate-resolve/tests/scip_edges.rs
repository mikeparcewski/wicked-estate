//! Integration tests for [`wicked_estate_resolve::scip_edges`].
//!
//! These tests use the committed `tests/fixtures/sample-ts.scip` file and do NOT require
//! `scip-typescript` to be installed at test time. The fixture was produced by running
//! `npx @sourcegraph/scip-typescript@0.4.0 index` on a minimal TypeScript project containing:
//!
//!   src/util.ts  — `export function helper(x: number): number { return x + 1; }`
//!   src/main.ts  — `import { helper } from './util'; export function run(): number { return helper(1); }`
//!
//! The fixture encodes:
//!   - `src/util.ts` line 0 col 16-22: Definition of `helper()`.
//!   - `src/main.ts` line 1 col 16-19: Definition of `run()`.
//!   - `src/main.ts` line 1 col 39-45: Reference to `helper()` (inside `run`).
//!
//! Our `scip_edges` must produce the precise edge:
//!   `run` (src/main.ts) → `helper` (src/util.ts)  EdgeKind::Calls  confidence 1.0
//!   resolved_by == "scip-typescript"

use wicked_estate_core::{Descriptor, EdgeKind, Language, Location, Node, NodeKind, Span, Symbol};
use wicked_estate_resolve::scip_edges;

/// Build a synthetic Node for the test, mimicking what tree-sitter extraction would produce.
fn make_node(name: &str, file: &str, start_line: u32, end_line: u32) -> Node {
    // Use a SCIP-style symbol id matching the fixture's package coordinates, so that
    // if the caller ever wants to correlate IDs, the structure is consistent.
    // For this test we only care that source/target are DISTINCT and match our assertions.
    let sym = Symbol::global(
        "ci-test",
        None,
        vec![Descriptor::method(format!("{file}::{name}"), None)],
    )
    .id();

    let span = Span {
        start_byte: 0,
        end_byte: 0,
        start_line,
        start_col: 0,
        end_line,
        end_col: 80,
    };

    Node::new(
        sym,
        NodeKind::Function,
        name,
        Language::new("typescript"),
        Location::new(file, span),
    )
}

/// The core assertion: loading the fixture and two synthetic nodes produces exactly one
/// `run → helper` Calls edge at confidence 1.0 with `resolved_by == "scip-typescript"`.
#[test]
fn scip_edges_emits_precise_run_to_helper_edge() {
    let fixture = include_bytes!("fixtures/sample-ts.scip");

    // The fixture has:
    //   src/util.ts  helper()  definition at line 0, col 16-22
    //   src/main.ts  run()     definition at line 1, col 16-19
    //   src/main.ts  line 1 col 39-45: reference to helper() — inside run's body
    //
    // We give our synthetic nodes spans that contain those SCIP occurrence lines so the
    // "smallest-containing" heuristic picks the right node.
    //
    // helper is defined on line 0 of src/util.ts → span [0, 0] (single line)
    let helper_node = make_node("helper", "src/util.ts", 0, 0);
    // run is defined on line 1 of src/main.ts, the reference to helper is also on line 1
    let run_node = make_node("run", "src/main.ts", 1, 2);

    let nodes = vec![helper_node.clone(), run_node.clone()];
    let edges = scip_edges(fixture, &nodes).expect("scip_edges must not fail on valid fixture");

    // Filter to the edge we care about: run → helper.
    let call_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.source == run_node.symbol && e.target == helper_node.symbol)
        .collect();

    assert_eq!(
        call_edges.len(),
        1,
        "expected exactly one run→helper edge; got {} total edges: {:#?}",
        edges.len(),
        edges
    );

    let edge = call_edges[0];
    assert_eq!(
        edge.kind,
        EdgeKind::Calls,
        "target is a function → EdgeKind::Calls expected"
    );
    assert!(
        (edge.confidence.get() - 1.0).abs() < 1e-6,
        "SCIP tier must produce confidence 1.0, got {}",
        edge.confidence.get()
    );
    assert_eq!(
        edge.resolved_by, "scip-typescript",
        "resolved_by must be 'scip-typescript'"
    );

    // The edge location should point into src/main.ts at the reference occurrence.
    let loc = edge
        .location
        .as_ref()
        .expect("scip edge must carry a location");
    assert_eq!(loc.file, "src/main.ts");
    assert_eq!(loc.span.start_line, 1, "reference is on line 1 (0-based)");
}

/// Nodes with spans that DON'T contain the occurrence lines produce no edges — the
/// correlator must NOT emit phantom edges when no node encloses the reference.
#[test]
fn scip_edges_skips_when_no_enclosing_node() {
    let fixture = include_bytes!("fixtures/sample-ts.scip");

    // Give both nodes spans on lines that don't overlap with any occurrence in the fixture.
    let helper_node = make_node("helper", "src/util.ts", 99, 100);
    let run_node = make_node("run", "src/main.ts", 99, 100);

    let nodes = vec![helper_node, run_node];
    let edges = scip_edges(fixture, &nodes).expect("must not error");

    assert!(
        edges.is_empty(),
        "no node contains an occurrence line → no edges expected, got {:#?}",
        edges
    );
}

/// Empty node list → empty edges, no panic.
#[test]
fn scip_edges_with_empty_nodes_returns_empty() {
    let fixture = include_bytes!("fixtures/sample-ts.scip");
    let edges = scip_edges(fixture, &[]).expect("must not error");
    assert!(edges.is_empty());
}

/// Corrupt bytes produce an error, not a panic.
#[test]
fn scip_edges_returns_error_on_corrupt_bytes() {
    let result = scip_edges(b"\xff\xfe\xfd\x00corrupt", &[]);
    assert!(result.is_err(), "corrupt payload must return Err");
}
