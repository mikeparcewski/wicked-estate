//! TS-S2C end to end: real tree-sitter indexing, then semantic evidence through the support plane.
//!
//! * The real scip-typescript 0.4.0 sample (`wicked-estate-resolve/tests/fixtures/…`) is indexed
//!   with the production pipeline and its `index.scip` ingested: references stay references,
//!   nothing lands on a value slot, re-ingest advances the generation, and an empty snapshot
//!   restores the base plane exactly.
//! * The four legacy-language **contract fixtures** (`tests/fixtures/evidence/`, see its README)
//!   go through the same entry point. They are contract fixtures, not integrations.

use std::fs;
use std::path::{Path, PathBuf};

use wicked_estate::{ingest_scip_report_as, ingest_semantic_evidence, scip_snapshot};
use wicked_estate_core::evidence::{EvidenceSkip, SemanticEvidence};
use wicked_estate_core::{
    Edge, EdgeKind, GraphRead, GraphWrite, Language, Location, Node, NodeKind, Provenance, Span,
    SymbolId,
};
use wicked_estate_store::SqliteStore;

const SAMPLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../wicked-estate-resolve/tests/fixtures/scip-typescript-0.4.0"
);

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_evidence_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Copy the sample project (sources + index) into a scratch root.
fn sample_root(tag: &str) -> PathBuf {
    let root = fresh_dir(tag);
    fs::create_dir_all(root.join("src")).unwrap();
    for f in [
        "src/util.ts",
        "src/main.ts",
        "tsconfig.json",
        "package.json",
    ] {
        fs::copy(Path::new(SAMPLE).join(f), root.join(f)).unwrap();
    }
    fs::copy(
        Path::new(SAMPLE).join("index.scip"),
        root.join("index.scip"),
    )
    .unwrap();
    root
}

fn node_named(store: &SqliteStore, name: &str, kind: NodeKind) -> Node {
    let found: Vec<Node> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .filter(|n| n.name == name && n.kind == kind && !n.is_value_flow_node())
        .collect();
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    found.into_iter().next().unwrap()
}

fn scip_edges(store: &SqliteStore) -> Vec<Edge> {
    store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == "scip-typescript")
        .collect()
}

#[test]
fn the_real_index_projects_references_never_calls_and_never_value_slots() {
    let root = sample_root("real");
    let mut store = SqliteStore::open(root.join("g.db")).unwrap();
    wicked_estate::index_path(&mut store, &root).unwrap();
    let base_calls: Vec<Edge> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .collect();
    assert!(
        !base_calls.is_empty(),
        "tree-sitter's own Calls are the base plane"
    );

    let scip = root.join("index.scip");
    let report = ingest_scip_report_as(&mut store, &root, &scip, None).unwrap();
    assert_eq!(report.calls_projected, 0, "{report:?}");
    assert_eq!(report.generation, Some(1));
    assert_eq!(
        report.definitions_mapped, 5,
        "helper, LIMIT, Greeter, greet, run: {report:?}"
    );

    let edges = scip_edges(&store);
    assert!(!edges.is_empty());
    assert_eq!(
        report.edges_projected,
        edges.len(),
        "edges_projected counts public edges, not sites"
    );
    assert!(report.references_projected > report.edges_projected);
    assert!(
        edges.iter().all(|e| e.kind == EdgeKind::References),
        "SCIP has no call role: {edges:?}"
    );
    for e in &edges {
        assert_eq!(e.provenance, Provenance::Scip);
        for end in [&e.source, &e.target] {
            let n = store.get_node(end).unwrap().expect("endpoint is a node");
            assert!(!n.is_value_flow_node(), "{e:?} lands on a value slot");
        }
    }

    // `const f = helper;` and `helper(LIMIT)` are both run → helper references: two facts, one edge.
    let run = node_named(&store, "run", NodeKind::Function);
    let helper = node_named(&store, "helper", NodeKind::Function);
    let rows = store
        .edge_supports(&run.symbol, &helper.symbol, &EdgeKind::References)
        .unwrap();
    let mut lines: Vec<u32> = rows
        .iter()
        .map(|r| r.fact.location.as_ref().unwrap().span.start_line)
        .collect();
    lines.sort();
    assert_eq!(lines, vec![5, 6]);
    assert!(
        rows.iter()
            .all(|r| r.owner.producer == "scip-typescript" && r.owner.snapshot == ".:index.scip")
    );
    // The import specifier is a module-level use: it comes from the File node.
    let main = node_named(&store, "src/main.ts", NodeKind::File);
    assert_eq!(
        store
            .edge_supports(&main.symbol, &helper.symbol, &EdgeKind::References)
            .unwrap()
            .len(),
        1
    );
    // The base plane's own run → helper call is untouched.
    let after: Vec<Edge> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(after, base_calls);

    // Re-ingesting the unchanged index advances the generation and changes nothing public.
    let again = ingest_scip_report_as(&mut store, &root, &scip, None).unwrap();
    assert_eq!((again.generation, again.replayed), (Some(2), false));
    assert_eq!(scip_edges(&store), edges);

    // An empty snapshot from the same owner retracts every fact it held.
    let empty = SemanticEvidence {
        facts: vec![],
        documents: vec![],
        ..wicked_estate_resolve::scip_evidence(
            &fs::read(&scip).unwrap(),
            &scip_snapshot(&root, &scip, None),
        )
        .unwrap()
        .evidence
    };
    let r = ingest_semantic_evidence(&mut store, Some(&root), &empty, None).unwrap();
    assert_eq!((r.generation, r.projected()), (Some(3), 0));
    assert!(scip_edges(&store).is_empty());
    let calls: Vec<Edge> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(
        calls, base_calls,
        "the base plane survives every replacement"
    );
}

#[test]
fn the_snapshot_names_the_index_path_not_its_contents() {
    let root = fresh_dir("snap");
    fs::create_dir_all(root.join(".scip")).unwrap();
    fs::write(root.join(".scip/typescript.scip"), b"").unwrap();
    assert_eq!(
        scip_snapshot(&root, &root.join(".scip/typescript.scip"), Some("web")),
        "web:.scip/typescript.scip"
    );
    let outside = fresh_dir("snap_outside").join("other.scip");
    fs::write(&outside, b"").unwrap();
    assert_eq!(scip_snapshot(&root, &outside, None), ".:other.scip");
}

// ── Contract fixtures (not integrations) ──────────────────────────────────────────────────────

fn fixture(name: &str) -> SemanticEvidence {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/evidence")
        .join(name);
    serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap()
}

fn n(id: &str, kind: NodeKind, name: &str, file: &str, lines: (u32, u32)) -> Node {
    Node::new(
        SymbolId(id.into()),
        kind,
        name,
        Language::new("legacy"),
        Location::new(
            file,
            Span {
                start_byte: 0,
                end_byte: 0,
                start_line: lines.0,
                start_col: 0,
                end_line: lines.1,
                end_col: 80,
            },
        ),
    )
}

fn store_with(tag: &str, nodes: &[Node]) -> SqliteStore {
    let mut store = SqliteStore::open(fresh_dir(tag).join("g.db")).unwrap();
    store.upsert_nodes(nodes).unwrap();
    store
}

fn edges_of(store: &SqliteStore, producer: &str) -> Vec<(String, String, EdgeKind)> {
    let mut v: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == producer)
        .map(|e| (e.source.0, e.target.0, e.kind))
        .collect();
    v.sort();
    v
}

#[test]
fn cobol_copybook_items_belong_to_the_copybook_and_calls_need_the_capability() {
    let mut store = store_with(
        "cobol",
        &[
            n(
                "PAYROLL",
                NodeKind::File,
                "src/PAYROLL.cbl",
                "src/PAYROLL.cbl",
                (0, 0),
            ),
            n(
                "CALC-TAX",
                NodeKind::Function,
                "CALC-TAX",
                "src/PAYROLL.cbl",
                (20, 30),
            ),
            n(
                "EMPREC",
                NodeKind::File,
                "copy/EMPREC.cpy",
                "copy/EMPREC.cpy",
                (0, 0),
            ),
            n(
                "EMP-SALARY",
                NodeKind::Field,
                "EMP-SALARY",
                "copy/EMPREC.cpy",
                (3, 3),
            ),
        ],
    );
    let r = ingest_semantic_evidence(
        &mut store,
        None,
        &fixture("cobol-copybook.contract.json"),
        None,
    )
    .unwrap();
    assert_eq!(
        edges_of(&store, "contract-fixture-cobol-ls"),
        vec![("CALC-TAX".into(), "EMP-SALARY".into(), EdgeKind::References)]
    );
    assert_eq!(r.skipped.get(&EvidenceSkip::UndeclaredCapability), Some(&1));
}

#[test]
fn plscope_calls_are_trusted_and_case_distinct_signatures_stay_distinct() {
    let mut store = store_with(
        "plsql",
        &[
            n(
                "HR_PKG",
                NodeKind::File,
                "db/hr_pkg.pkb",
                "db/hr_pkg.pkb",
                (0, 0),
            ),
            n(
                "hr.RAISE",
                NodeKind::Function,
                "RAISE",
                "db/hr_pkg.pkb",
                (2, 6),
            ),
            n(
                "hr.\"Raise\"",
                NodeKind::Function,
                "Raise",
                "db/hr_pkg.pkb",
                (8, 12),
            ),
            n(
                "hr.GIVE_BONUS",
                NodeKind::Function,
                "GIVE_BONUS",
                "db/hr_pkg.pkb",
                (14, 20),
            ),
        ],
    );
    let ev = fixture("plsql-plscope.contract.json");
    let r = ingest_semantic_evidence(&mut store, None, &ev, None).unwrap();
    assert_eq!(
        edges_of(&store, "contract-fixture-plscope"),
        vec![
            (
                "hr.GIVE_BONUS".into(),
                "hr.\"Raise\"".into(),
                EdgeKind::Calls
            ),
            ("hr.GIVE_BONUS".into(), "hr.RAISE".into(), EdgeKind::Calls),
        ]
    );
    assert_eq!(r.generation, Some(7), "the producer's own generation");
    assert_eq!(r.skipped.get(&EvidenceSkip::DynamicTarget), Some(&1));
    let call = store
        .all_edges()
        .unwrap()
        .into_iter()
        .find(|e| e.resolved_by == "contract-fixture-plscope")
        .unwrap();
    assert_eq!(
        call.provenance,
        Provenance::Compiler,
        "a compiler fact is not SCIP"
    );
    // Replaying generation 7 is idempotent; an older one is refused.
    assert!(
        ingest_semantic_evidence(&mut store, None, &ev, None)
            .unwrap()
            .replayed
    );
    let stale = SemanticEvidence {
        generation: Some(6),
        ..ev
    };
    assert!(ingest_semantic_evidence(&mut store, None, &stale, None).is_err());
}

#[test]
fn abap_namespaces_keep_same_named_methods_apart() {
    let mut store = store_with(
        "abap",
        &[
            n(
                "ABC",
                NodeKind::File,
                "src/#abc#cl_pay.clas.abap",
                "src/#abc#cl_pay.clas.abap",
                (0, 0),
            ),
            n(
                "/ABC/CL_PAY=>CALC",
                NodeKind::Method,
                "CALC",
                "src/#abc#cl_pay.clas.abap",
                (10, 15),
            ),
            n(
                "/ABC/CL_PAY=>RUN",
                NodeKind::Method,
                "RUN",
                "src/#abc#cl_pay.clas.abap",
                (20, 25),
            ),
            n(
                "XYZ",
                NodeKind::File,
                "src/#xyz#cl_pay.clas.abap",
                "src/#xyz#cl_pay.clas.abap",
                (0, 0),
            ),
            n(
                "/XYZ/CL_PAY=>CALC",
                NodeKind::Method,
                "CALC",
                "src/#xyz#cl_pay.clas.abap",
                (10, 15),
            ),
        ],
    );
    ingest_semantic_evidence(
        &mut store,
        None,
        &fixture("abap-namespace.contract.json"),
        None,
    )
    .unwrap();
    assert_eq!(
        edges_of(&store, "contract-fixture-abap-xref"),
        vec![
            (
                "/ABC/CL_PAY=>RUN".into(),
                "/ABC/CL_PAY=>CALC".into(),
                EdgeKind::References
            ),
            (
                "/ABC/CL_PAY=>RUN".into(),
                "/XYZ/CL_PAY=>CALC".into(),
                EdgeKind::References
            ),
        ]
    );
}

#[test]
fn rpg_object_level_references_project_nothing() {
    let mut store = store_with(
        "rpg",
        &[n(
            "PAYCALC",
            NodeKind::File,
            "qrpglesrc/paycalc.rpgle",
            "qrpglesrc/paycalc.rpgle",
            (0, 0),
        )],
    );
    let r = ingest_semantic_evidence(
        &mut store,
        None,
        &fixture("rpg-dsppgmref.contract.json"),
        None,
    )
    .unwrap();
    assert_eq!(r.projected(), 0);
    assert_eq!(r.skipped.get(&EvidenceSkip::UnknownTarget), Some(&1));
    assert_eq!(r.skipped.get(&EvidenceSkip::UndeclaredCapability), Some(&1));
    assert!(edges_of(&store, "contract-fixture-dsppgmref").is_empty());
}

#[test]
fn an_invalid_envelope_writes_nothing() {
    let mut store = store_with(
        "invalid",
        &[n(
            "HR_PKG",
            NodeKind::File,
            "db/hr_pkg.pkb",
            "db/hr_pkg.pkb",
            (0, 0),
        )],
    );
    let mut ev = fixture("plsql-plscope.contract.json");
    ev.schema_version = 2;
    assert!(ingest_semantic_evidence(&mut store, None, &ev, None).is_err());
    assert!(store.support_owners().unwrap().is_empty());
}
