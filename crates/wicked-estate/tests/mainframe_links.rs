//! Mainframe links, end to end through the real binary: `index` then the user's queries.
//!
//! `tests/fixtures/mainframe/` is the 5-file estate from the mainframe-links bug report (COBOL
//! ORDPGM + TAXPGM, copybook ORDREC, JCL ORDJOB, HLASM SHIPPGM). On 0.32.1 four of its links were
//! missing: `EXEC CICS LINK` (a homonym boundary node made SHIPPGM ambiguous, and the source was
//! never a node), `EXEC SQL … FROM ORDERS` (the edge came from a file anchor that was not a node,
//! and the index pruned it), `COPY ORDREC` (nothing emitted it) and the dynamic `CALL WS-PGM`.
//!
//! `tests/fixtures/mainframe-grammarless/` has one source per grammar-less extractor (JCL, HLASM,
//! BMS, RACF, IMS DBD+PSB, MQSC, Drools DRL, Blaze BRL) plus COBOL with its text supplement. The
//! guard test indexes it and asserts that the index prunes NOTHING and that every documented edge
//! kind of every extractor is stored — so an extractor that mints a ref or edge from an id that is
//! not a node fails here instead of silently losing the edge.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use wicked_estate_core::{EdgeKind, GraphRead, Node, NodeKind, Symbol};
use wicked_estate_store::SqliteStore;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_mainframe_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&dest).unwrap();
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// Index `root` into `db`; returns stderr. Panics on a non-zero exit.
fn index(root: &Path, db: &Path) -> String {
    let out = Command::new(bin())
        .arg("index")
        .arg(root)
        .arg("--db")
        .arg(db)
        .output()
        .expect("spawn wicked-estate index");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "index failed: {stderr}");
    stderr
}

fn query(db: &Path, args: &[&str]) -> serde_json::Value {
    let out = Command::new(bin())
        .args(args)
        .arg("--json")
        .arg("--db")
        .arg(db)
        .output()
        .expect("spawn wicked-estate");
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{args:?} stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn dependent_names(db: &Path, name: &str) -> BTreeSet<String> {
    query(db, &["blast-radius", name])["dependents"]
        .as_array()
        .expect("dependents")
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect()
}

fn path_names(db: &Path, from: &str, to: &str) -> Vec<String> {
    let v = query(db, &["path", from, to]);
    assert_eq!(v["found"], true, "no path {from} -> {to}: {v}");
    let hops = v["hops"].as_array().unwrap();
    let mut names = vec![hops[0]["source"]["name"].as_str().unwrap().to_string()];
    names.extend(
        hops.iter()
            .map(|h| h["target"]["name"].as_str().unwrap().to_string()),
    );
    names
}

fn assert_nothing_pruned(stderr: &str) {
    assert!(
        !stderr.contains("pruned"),
        "the index pruned dangling edges — an extractor minted a ref or edge from an id that is \
         not a node:\n{stderr}"
    );
}

#[test]
fn brief_fixture_links_resolve_through_the_user_queries() {
    let tmp = scratch("brief");
    let db = tmp.0.join("graph.db");
    let stderr = index(&fixture("mainframe"), &db);
    assert_nothing_pruned(&stderr);

    // "who touches table ORDERS" — and, by kind, who READS it.
    assert!(dependent_names(&db, "ORDERS").contains("ORDPGM"));
    let readers = query(
        &db,
        &[
            "traverse",
            "ORDERS",
            "--direction",
            "dependents",
            "--edge-kinds",
            "other:reads",
        ],
    );
    assert!(
        readers["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["source"]["name"] == "ORDPGM" && e["kind"]["other"] == "reads"),
        "{readers}"
    );

    // "who LINKs to SHIPPGM": the HLASM program, now the only node of that name.
    assert!(dependent_names(&db, "SHIPPGM").contains("ORDPGM"));

    // "what uses copybook ORDREC", and the blast radius of a copybook field.
    assert!(dependent_names(&db, "ORDREC").contains("ORDPGM"));
    assert!(dependent_names(&db, "ORD-ID").contains("ORDPGM"));

    // JCL STEP1 → ORDPGM → SHIPPGM → TAXPGM, as two legs + TAXPGM's blast radius (the shortest
    // STEP1 → TAXPGM path is the direct `CALL 'TAXPGM'`, which is correct).
    assert_eq!(
        path_names(&db, "STEP1", "SHIPPGM"),
        ["STEP1", "ORDPGM", "SHIPPGM"]
    );
    assert_eq!(path_names(&db, "SHIPPGM", "TAXPGM"), ["SHIPPGM", "TAXPGM"]);
    let tax = dependent_names(&db, "TAXPGM");
    for who in ["SHIPPGM", "ORDPGM", "STEP1"] {
        assert!(
            tax.contains(who),
            "{who} not in TAXPGM's blast radius: {tax:?}"
        );
    }

    // The dynamic CALL WS-PGM ('PRICEPGM'): no PRICEPGM in this repo, so it parks under the
    // prefixed name, and WS-PGM says why it resolved to that name.
    let store = SqliteStore::open(&db).unwrap();
    assert_eq!(
        store
            .unresolved_refs_for_name("cobol-dynamic-call:PRICEPGM")
            .unwrap()
            .len(),
        1
    );
    let nodes = store.all_nodes().unwrap();
    assert!(
        !nodes
            .iter()
            .any(|n| n.kind == NodeKind::Other("cics_program".into()))
    );
    let ws_pgm = nodes.iter().find(|n| n.name == "WS-PGM").unwrap();
    assert_eq!(ws_pgm.metadata["dynamic_call"]["value"], "PRICEPGM");
}

#[test]
fn dynamic_call_binds_at_heuristic_and_backfills_when_the_program_appears() {
    let tmp = scratch("dyn");
    let root = tmp.0.join("repo");
    fs::create_dir_all(&root).unwrap();
    copy_tree(&fixture("mainframe"), &root);
    let db = tmp.0.join("graph.db");
    index(&root, &db);

    fs::write(
        root.join("cbl/PRICEPGM.cbl"),
        "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PRICEPGM.\n       PROCEDURE DIVISION.\n       MAIN-PARA.\n           GOBACK.\n",
    )
    .unwrap();
    let stderr = index(&root, &db);
    assert_nothing_pruned(&stderr);

    let store = SqliteStore::open(&db).unwrap();
    let nodes: HashMap<_, Node> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol.clone(), n))
        .collect();
    let edge = store
        .all_edges()
        .unwrap()
        .into_iter()
        .find(|e| {
            e.kind == EdgeKind::Calls && nodes.get(&e.target).is_some_and(|n| n.name == "PRICEPGM")
        })
        .expect("ORDPGM -calls-> PRICEPGM after the back-fill");
    assert_eq!(nodes[&edge.source].name, "ORDPGM");
    assert_eq!(edge.resolved_by, "cobol-dynamic-call");
    assert!((edge.confidence.get() - 0.5).abs() < 1e-6, "Heuristic tier");
    assert!(
        store
            .unresolved_refs_for_name("cobol-dynamic-call:PRICEPGM")
            .unwrap()
            .is_empty()
    );
}

/// (source file prefix, edge kind) pairs every grammar-less extractor documents, and the COBOL
/// supplement's. The guard: each must be stored after indexing the fixture.
fn documented_edges() -> Vec<(&'static str, EdgeKind)> {
    let o = |t: &str| EdgeKind::Other(t.to_string());
    vec![
        // jcl.rs
        ("jcl/", EdgeKind::Contains),
        ("jcl/", EdgeKind::Calls),
        ("jcl/", o("uses")),
        // hlasm.rs (CSECT CALL, a CALL before the first CSECT, BMS mapset → map)
        ("asm/", EdgeKind::Calls),
        ("bms/", EdgeKind::Contains),
        // racf.rs + the estate RACF join
        ("racf/", o("permits")),
        ("racf/", o("member_of")),
        ("racf/", o("protects")),
        // ims.rs (DBD and PSB)
        ("ims/CUSTDBD", EdgeKind::Contains),
        ("ims/CUSTDBD", o("parent")),
        ("ims/ORDPSB", o("accesses")),
        ("ims/ORDPSB", o("sensitive_to")),
        // mq.rs
        ("mq/", o("resolves_to")),
        // cobol_links.rs (+ the tree-sitter COBOL extractor)
        ("cbl/", EdgeKind::Calls),
        ("cbl/", EdgeKind::Imports),
        ("cbl/", EdgeKind::References),
        ("cbl/", o("reads")),
        ("cbl/", o("writes")),
        ("cbl/", o("uses")),
        ("cpy/", EdgeKind::Contains),
        // drl.rs, blaze_brl.rs
        ("rules/lending.drl", EdgeKind::Contains),
        ("rules/account.brl", EdgeKind::Contains),
    ]
}

#[test]
fn grammarless_extractors_drop_no_edges_and_store_every_documented_kind() {
    let tmp = scratch("guard");
    let db = tmp.0.join("graph.db");
    let stderr = index(&fixture("mainframe-grammarless"), &db);
    assert_nothing_pruned(&stderr);

    let store = SqliteStore::open(&db).unwrap();
    let nodes: HashMap<_, Node> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol.clone(), n))
        .collect();
    let edges = store.all_edges().unwrap();
    let stored: BTreeSet<(String, String)> = edges
        .iter()
        .map(|e| {
            (
                nodes[&e.source].location.file.clone(),
                format!("{:?}", e.kind),
            )
        })
        .collect();
    let mut missing = Vec::new();
    for (prefix, kind) in documented_edges() {
        let k = format!("{kind:?}");
        if !stored
            .iter()
            .any(|(f, sk)| f.starts_with(prefix) && *sk == k)
        {
            missing.push(format!("{prefix} {k}"));
        }
    }
    assert!(
        missing.is_empty(),
        "documented edges not stored: {missing:?}\nstored: {stored:?}"
    );

    let name = |s: &wicked_estate_core::SymbolId| nodes[s].name.clone();
    let has = |from: &str, kind: &EdgeKind, to: &str| {
        edges
            .iter()
            .any(|e| &e.kind == kind && name(&e.source) == from && name(&e.target) == to)
    };
    let o = |t: &str| EdgeKind::Other(t.to_string());
    // A CALL before the first CSECT comes from the File node.
    assert!(
        edges
            .iter()
            .any(|e| e.source == Symbol::file("asm/STUB.asm").id()
                && e.kind == EdgeKind::Calls
                && name(&e.target) == "PAYCALC")
    );
    // IMS PSB ORDPSB accesses CUSTDB; RACF PERMIT from an undefined group keeps its edge.
    assert!(has("ORDPSB", &o("accesses"), "CUSTDB"));
    assert!(has("AUDITGRP", &o("permits"), "PROD.PAY.**"));
    // JCL in-stream PROC: STEP2 EXEC PAYPROC resolves to it.
    assert!(has("STEP2", &EdgeKind::Calls, "PAYPROC"));
    // CICS maps resolve to the BMS source; the node is its definition, not the boundary.
    assert!(has("PAYCALC", &o("writes"), "PAYMAP"));
    assert!(has("PAYCALC", &o("reads"), "PAYMAP"));
    let map = nodes
        .values()
        .find(|n| n.name == "PAYMAP" && n.kind == NodeKind::Other("bms_map".into()))
        .expect("PAYMAP is defined by the BMS source");
    assert!(map.location.file.starts_with("bms/"), "{map:?}");
    // Db2 read AND write of the same table; CICS LINK to the HLASM program; dynamic CALL.
    assert!(has("PAYCALC", &o("reads"), "PAYHIST"));
    assert!(has("PAYCALC", &o("writes"), "PAYHIST"));
    assert!(has("PAYCALC", &EdgeKind::Calls, "STUB"));
    assert!(has("PAYCALC", &EdgeKind::Calls, "RATEPGM"));
    // DD binding: JCL defines PAYCALC.PAYIN, COBOL's ASSIGN TO PAYIN uses it, so the dataset's
    // blast radius reaches the program.
    assert!(has("PAYCALC", &o("uses"), "PAYCALC.PAYIN"));
    assert!(has("PAYCALC.PAYIN", &o("uses"), "PROD.PAY.INPUT"));
    let bind = nodes.values().find(|n| n.name == "PAYCALC.PAYIN").unwrap();
    assert!(
        !bind.is_declaration() && bind.location.file.starts_with("jcl/"),
        "{bind:?}"
    );
    assert!(dependent_names(&db, "PROD.PAY.INPUT").contains("PAYCALC"));
    // Copybook field use → the copybook's field; copybook → program.
    assert!(has("PAYCALC", &EdgeKind::References, "PAY-ID"));
    assert!(dependent_names(&db, "PAYREC").contains("PAYCALC"));
}
