//! #447: exported mainframe members are either indexed correctly or reported with a reason.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use wicked_estate_core::{GraphRead, Language, NodeKind, Symbol};
use wicked_estate_store::SqliteStore;

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mainframe-export"),
        &root,
    );
    let db = tmp.path().join("graph.db");
    (tmp, root, db)
}

fn index(root: &Path, db: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
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

fn not_indexed_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("NOT-INDEXED:"))
        .collect()
}

fn assert_not_indexed_report(stderr: &str) {
    let expected = [
        (
            "ca7",
            "scheduler definition: no extractor yet (wicked-estate#447)",
            "app/scheduler/CARDDEMO.ca7",
        ),
        (
            "controlm",
            "scheduler definition: no extractor yet (wicked-estate#447)",
            "app/scheduler/CARDDEMO.controlm",
        ),
        (
            "csd",
            "CICS CSD resource definitions: no extractor yet (wicked-estate#447)",
            "app/csd/CARDDEMO.CSD",
        ),
        (
            "ctl",
            "mainframe control card (not VB6): no extractor yet (wicked-estate#447)",
            "app/ctl/REPROCT.ctl",
        ),
        (
            "ddl",
            "Db2 DDL: no DDL extractor yet (wicked-estate#447)",
            "app/ddl/TRNTYPE.ddl",
        ),
        (
            "mac",
            "HLASM macro library member: no macro extractor yet (wicked-estate#447)",
            "app/asm/CDMAC.mac",
        ),
        ("txt", "no extractor for this file type", "README.txt"),
    ];
    let mut lines: Vec<String> = expected
        .iter()
        .map(|(ext, reason, path)| {
            format!("NOT-INDEXED: .{ext}: 1 file(s): {reason} (e.g. {path})")
        })
        .collect();
    lines.push("NOT-INDEXED: 7 of 10 walked file(s) not indexed".to_string());
    assert_eq!(not_indexed_lines(stderr), lines, "{stderr}");
    for path in [
        "app/proc/TRNPROC.prc",
        "app/dcl/DCLTRTYP.dcl",
        "vb/Gauge.ctl",
    ] {
        assert!(
            not_indexed_lines(stderr)
                .iter()
                .all(|line| !line.contains(path)),
            "indexed file reported as skipped: {stderr}"
        );
    }
}

#[test]
fn prc_is_indexed_as_a_jcl_proc() {
    // Red on main: .prc has no extension mapping, so neither the PROC nor its step exists.
    let (_tmp, root, db) = fixture();
    index(&root, &db);
    let store = SqliteStore::open(&db).unwrap();
    let nodes = store.all_nodes().unwrap();
    for (name, kind) in [("TRNPROC", "jcl_proc"), ("STEP05", "step")] {
        assert!(
            nodes.iter().any(|node| node.name == name
                && node.kind == NodeKind::Other(kind.to_string())
                && node.location.file == "app/proc/TRNPROC.prc"),
            "missing {kind} {name}: {nodes:?}"
        );
    }
    assert!(
        store
            .indexed_files()
            .unwrap()
            .contains(&"app/proc/TRNPROC.prc".to_string())
    );
}

#[test]
fn dcl_is_indexed_as_a_cobol_copybook_with_its_db2_table() {
    // Red on main: .dcl is not claimed by COBOL, so its copybook and table are absent.
    let (_tmp, root, db) = fixture();
    index(&root, &db);
    let store = SqliteStore::open(&db).unwrap();
    let nodes = store.all_nodes().unwrap();
    let path = "app/dcl/DCLTRTYP.dcl";
    assert!(nodes.iter().any(|node| node.name == "DCLTRTYP"
        && node.kind == NodeKind::Other("copybook".to_string())
        && node.language == Language::new("cobol")
        && node.location.file == path));
    // cobol_links::Pass::exec_sql uppercases DECLARE's table token and preserves its
    // qualification in both the name and Symbol::synthetic("db2", ...).
    let table = nodes
        .iter()
        .find(|node| node.symbol == Symbol::synthetic("db2", "CARDDEMO.TRANSACTION_TYPE").id())
        .expect("DCLGEN table definition");
    assert_eq!(table.name, "CARDDEMO.TRANSACTION_TYPE");
    assert_eq!(table.kind, NodeKind::Other("db2_table".to_string()));
    assert_eq!(table.language, Language::new("cobol"));
    assert_eq!(table.location.file, path);
    assert!(!table.is_declaration());
    assert!(store.indexed_files().unwrap().contains(&path.to_string()));
}

#[test]
fn mainframe_ctl_is_not_read_as_vb6_and_vb6_ctl_still_is() {
    // Red on main: extension-only dispatch indexes the IDCAMS control card as a VB6 file.
    let (_tmp, root, db) = fixture();
    let stderr = index(&root, &db);
    let store = SqliteStore::open(&db).unwrap();
    let nodes = store.all_nodes().unwrap();
    let files = store.indexed_files().unwrap();
    assert!(
        !nodes
            .iter()
            .any(|node| node.location.file == "app/ctl/REPROCT.ctl")
    );
    assert!(!files.contains(&"app/ctl/REPROCT.ctl".to_string()));
    assert!(files.contains(&"vb/Gauge.ctl".to_string()));
    // VB6_QUERY captures sub_declaration as @code_function, including an empty Sub body.
    assert!(nodes.iter().any(|node| node.name == "Refresh"
        && node.kind == NodeKind::Function
        && node.language == Language::new("vb6")
        && node.location.file == "vb/Gauge.ctl"));
    assert_not_indexed_report(&stderr);
}

#[test]
fn unindexed_files_are_reported_with_a_reason() {
    // Red on main: unsupported exports are silently dropped; there are no NOT-INDEXED lines.
    let (_tmp, root, db) = fixture();
    assert_not_indexed_report(&index(&root, &db));
}

#[test]
fn second_index_run_with_no_changes_still_reports_unindexed_files() {
    // Red on main: the no-change exit also emits no NOT-INDEXED diagnostics.
    let (_tmp, root, db) = fixture();
    let first = index(&root, &db);
    let second = index(&root, &db);
    assert_not_indexed_report(&second);
    assert_eq!(not_indexed_lines(&first), not_indexed_lines(&second));
}
