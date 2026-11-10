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
    let to_price: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind != EdgeKind::Contains)
        .filter(|e| nodes.get(&e.target).is_some_and(|n| n.name == "PRICEPGM"))
        .collect();
    assert_eq!(
        to_price.len(),
        1,
        "only the dynamic-call resolver binds PRICEPGM: {to_price:?}"
    );
    let edge = to_price[0].clone();
    assert_eq!(edge.kind, EdgeKind::Calls);
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

/// (source file prefix, source node kind, edge kind) triples every grammar-less extractor
/// documents, and the COBOL supplement's. The guard: each must be stored after indexing the
/// fixture, from a source of that kind. File-node sources never count: the pipeline adds
/// File→node `Contains` edges for every grammar-less file, which would mask a dropped
/// extractor edge.
fn documented_edges() -> Vec<(&'static str, NodeKind, EdgeKind)> {
    let o = |t: &str| EdgeKind::Other(t.to_string());
    let k = |t: &str| NodeKind::Other(t.to_string());
    vec![
        // jcl.rs: JOB/PROC → step, step → program/PROC, step / DD binding → dataset
        ("jcl/", NodeKind::Module, EdgeKind::Contains),
        ("jcl/", k("step"), EdgeKind::Calls),
        ("jcl/", k("step"), o("uses")),
        ("jcl/", k("dd_binding"), o("uses")),
        // hlasm.rs (CSECT CALL; BMS mapset → map)
        ("asm/", NodeKind::Module, EdgeKind::Calls),
        ("bms/", k("bms_mapset"), EdgeKind::Contains),
        // racf.rs + the estate RACF join
        ("racf/", k("racf_group"), o("permits")),
        ("racf/", k("racf_identity"), o("permits")),
        ("racf/", k("racf_user"), o("member_of")),
        ("racf/", k("racf_dataset_profile"), o("protects")),
        // ims.rs (DBD and PSB)
        ("ims/CUSTDBD", k("ims_database"), EdgeKind::Contains),
        ("ims/CUSTDBD", k("ims_segment"), o("parent")),
        ("ims/ORDPSB", k("ims_psb"), o("accesses")),
        ("ims/ORDPSB", k("ims_psb"), o("sensitive_to")),
        // mq.rs
        ("mq/", k("mq_queue"), o("resolves_to")),
        // cobol_links.rs (+ the tree-sitter COBOL extractor)
        ("cbl/", NodeKind::Module, EdgeKind::Calls),
        ("cbl/", NodeKind::Module, EdgeKind::Imports),
        ("cbl/", NodeKind::Module, EdgeKind::References),
        ("cbl/", NodeKind::Module, o("reads")),
        ("cbl/", NodeKind::Module, o("writes")),
        ("cbl/", NodeKind::Module, o("uses")),
        ("cpy/", k("copybook"), EdgeKind::Contains),
        // drl.rs, blaze_brl.rs
        ("rules/lending.drl", NodeKind::RuleSet, EdgeKind::Contains),
        ("rules/account.brl", NodeKind::RuleSet, EdgeKind::Contains),
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
    let stored: BTreeSet<(String, String, String)> = edges
        .iter()
        .map(|e| {
            let src = &nodes[&e.source];
            (
                src.location.file.clone(),
                format!("{:?}", src.kind),
                format!("{:?}", e.kind),
            )
        })
        .collect();
    let mut missing = Vec::new();
    for (prefix, src_kind, kind) in documented_edges() {
        let (sk, ek) = (format!("{src_kind:?}"), format!("{kind:?}"));
        if !stored
            .iter()
            .any(|(f, s, e)| f.starts_with(prefix) && *s == sk && *e == ek)
        {
            missing.push(format!("{prefix} {sk} {ek}"));
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

/// Shared ids across incremental runs: remove the defining file and the node falls back to the
/// referencing file's boundary record (the program's edges stay); the defining file's own edges
/// go with it. Restore it and the definition wins again.
#[test]
fn shared_ids_fall_back_to_the_boundary_when_the_definition_is_removed() {
    let tmp = scratch("life");
    let root = tmp.0.join("repo");
    fs::create_dir_all(&root).unwrap();
    copy_tree(&fixture("mainframe-grammarless"), &root);
    let db = tmp.0.join("graph.db");
    index(&root, &db);

    let bms = fs::read_to_string(root.join("bms/PAYSET.bms")).unwrap();
    let jcl = fs::read_to_string(root.join("jcl/PAYJOB.jcl")).unwrap();
    fs::remove_file(root.join("bms/PAYSET.bms")).unwrap();
    fs::remove_file(root.join("jcl/PAYJOB.jcl")).unwrap();
    assert_nothing_pruned(&index(&root, &db));

    let snapshot = |db: &Path| {
        let store = SqliteStore::open(db).unwrap();
        let nodes: HashMap<_, Node> = store
            .all_nodes()
            .unwrap()
            .into_iter()
            .map(|n| (n.symbol.clone(), n))
            .collect();
        let name = |id: &wicked_estate_core::SymbolId| {
            nodes
                .get(id)
                .map_or_else(|| format!("<missing {id}>"), |n| n.name.clone())
        };
        let edges: Vec<(String, String, String)> = store
            .all_edges()
            .unwrap()
            .into_iter()
            .map(|e| (name(&e.source), format!("{:?}", e.kind), name(&e.target)))
            .collect();
        (nodes, edges)
    };
    let (nodes, edges) = snapshot(&db);
    let dangling: Vec<_> = edges
        .iter()
        .filter(|(a, _, b)| a.starts_with("<missing") || b.starts_with("<missing"))
        // Pre-existing (0.32.1 too): the estate RACF `protects` join edge carries no file, and a
        // deletion-only run skips the dangling prune, so it outlives the deleted dataset until
        // the next changed run. Tracked in the mainframe residuals issue.
        .filter(|(_, k, _)| !k.contains("protects"))
        .collect();
    assert!(
        dangling.is_empty(),
        "dangling after the deletion-only run: {dangling:?}"
    );
    let map = nodes
        .values()
        .find(|n| n.name == "PAYMAP")
        .expect("the map node survives");
    assert!(
        map.is_declaration() && map.location.file.starts_with("cbl/"),
        "{map:?}"
    );
    let bind = nodes.values().find(|n| n.name == "PAYCALC.PAYIN").unwrap();
    assert!(bind.is_declaration(), "{bind:?}");
    let has = |f: &str, t: &str| edges.iter().any(|(a, _, b)| a == f && b == t);
    assert!(has("PAYCALC", "PAYMAP") && has("PAYCALC", "PAYCALC.PAYIN"));
    assert!(
        !has("PAYCALC.PAYIN", "PROD.PAY.INPUT"),
        "the JCL's binding → dataset edge left with the JCL: {edges:?}"
    );

    fs::write(root.join("bms/PAYSET.bms"), bms).unwrap();
    fs::write(root.join("jcl/PAYJOB.jcl"), jcl).unwrap();
    assert_nothing_pruned(&index(&root, &db));
    let (nodes, edges) = snapshot(&db);
    let map = nodes.values().find(|n| n.name == "PAYMAP").unwrap();
    assert!(
        !map.is_declaration() && map.location.file.starts_with("bms/"),
        "{map:?}"
    );
    assert!(
        edges
            .iter()
            .any(|(a, _, b)| a == "PAYCALC.PAYIN" && b == "PROD.PAY.INPUT")
    );
}
