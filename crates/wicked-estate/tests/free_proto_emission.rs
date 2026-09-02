//! D6d free-function prototype emission through the REAL index path (wicked-estate#140,
//! M4 = Option A — one logical symbol, wicked-estate#152).
//!
//! The identity contract these tests prove end-to-end (extractor → engine → store):
//!
//! - **h+cpp**: a header prototype and its impl-file definition mint ONE SymbolId; the store
//!   records one CONTRIBUTION per file and derives the primary from the DEFINITION record
//!   (`is_declaration` metadata, definition-preferred) — the node reads as the `.cpp`
//!   definition, and removing either file leaves the node alive on the survivor. Zero id
//!   churn: the prototype JOINS the id the definition already minted before D6d.
//! - **h-only**: a header prototype with NO definition mints the id alone, as a
//!   DECLARATION-primary node — the declared-but-not-yet-defined API surface is visible.
//! - **h+c**: NAMED residual, pinned honestly — `.h` routes to the C++ grammar (`ts-cpp`
//!   scheme) while `.c` routes to the C grammar (`ts-c` scheme), so a C header prototype and
//!   its `.c` definition mint TWO nodes (different scheme prefix ⇒ different ids). D6d makes
//!   the declared C API surface visible; unifying it with the `ts-c` definition across the
//!   grammar seam would be an id-shape change (out of Option A's zero-churn bounds) and stays
//!   open — recorded in ADR-002 §Accepted residuals.
//!
//! Stores are TEMP-only (in-memory sqlite); nothing touches a user DB.

use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{GraphRead, Node, NodeKind};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("d6d_proto_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

const HEADER: &str = "#ifndef API_H\n#define API_H\nint compute(int a, int b);\n#endif\n";
const IMPL_CPP: &str = "int compute(int a, int b) { return a + b; }\n";
const IMPL_C: &str = "int compute(int a, int b) { return a + b; }\n";

/// All non-File nodes named `name`.
fn defs_named(store: &SqliteStore, name: &str) -> Vec<Node> {
    store
        .all_nodes()
        .unwrap()
        .into_iter()
        .filter(|n| n.kind != NodeKind::File && n.name == name)
        .collect()
}

/// h-only: a header prototype with no definition mints the id alone and is a
/// DECLARATION-primary node.
#[test]
fn header_only_proto_is_declaration_primary() {
    let dir = fresh_dir("h_only");
    fs::write(dir.join("src/api.h"), HEADER).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let nodes = defs_named(&store, "compute");
    assert_eq!(nodes.len(), 1, "exactly one node for the lone prototype");
    let n = &nodes[0];
    assert_eq!(n.kind, NodeKind::Function, "free prototype mints Function");
    assert_eq!(
        n.location.file, "src/api.h",
        "homed at its only contributor"
    );
    assert!(
        n.is_declaration(),
        "with no definition contribution the primary IS the declaration record"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// h+cpp: ONE node, definition-primary, and the contribution table carries both
/// files — proven behaviorally through deletion-only runs in BOTH directions
/// (the exact F7 data-loss path D6d was deferred over, now dead: remove one
/// file of the pair and the node survives re-homed to the other).
#[test]
fn header_plus_cpp_single_node_definition_primary_survives_either_removal() {
    let dir = fresh_dir("h_cpp");
    fs::write(dir.join("src/api.h"), HEADER).unwrap();
    fs::write(dir.join("src/api.cpp"), IMPL_CPP).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    // ONE logical symbol (Option A): the proto joined the definition's id.
    let nodes = defs_named(&store, "compute");
    assert_eq!(
        nodes.len(),
        1,
        "proto + def must be ONE node (one SymbolId, one row); got {:?}",
        nodes
            .iter()
            .map(|n| (&n.symbol.0, &n.location.file))
            .collect::<Vec<_>>()
    );
    // Definition-primary: NOT lexicographic luck — the declaration flag decides.
    let n = &nodes[0];
    assert_eq!(
        n.location.file, "src/api.cpp",
        "the DEFINITION contribution is the primary (definition-preferred, not last-write-wins)"
    );
    assert!(
        !n.is_declaration(),
        "the projected record is the definition's, not the header's"
    );
    let symbol = n.symbol.clone();

    // Remove the HEADER (deletion-only run): the node survives on the definition.
    fs::remove_file(dir.join("src/api.h")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let n = store
        .get_node(&symbol)
        .unwrap()
        .expect("node must survive header removal — the definition still contributes");
    assert_eq!(n.location.file, "src/api.cpp");
    assert!(!n.is_declaration());

    // Restore the header, re-index, then remove the DEFINITION: the node
    // survives RE-HOMED to the declaration contribution (both files were
    // recorded as contributors — this is the S11 "shared id, both files
    // contributed" fact, proven through the real path).
    fs::write(dir.join("src/api.h"), HEADER).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    fs::remove_file(dir.join("src/api.cpp")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let n = store
        .get_node(&symbol)
        .unwrap()
        .expect("node must survive definition removal — the header still declares it");
    assert_eq!(
        n.location.file, "src/api.h",
        "re-homed to the surviving declaration contribution"
    );
    assert!(
        n.is_declaration(),
        "the surviving primary is the declaration record"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Same-id stability across incremental runs: whichever file is touched last,
/// the primary stays the definition — the last-write-wins file flap (store
/// mechanism 1 of the F7 triad) is dead for proto/def pairs.
#[test]
fn reindex_order_does_not_flap_the_primary() {
    let dir = fresh_dir("noflap");
    fs::write(dir.join("src/api.h"), HEADER).unwrap();
    fs::write(dir.join("src/api.cpp"), IMPL_CPP).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    // Touch ONLY the header (a comment edit) and re-run: the header is the last
    // writer, but the primary must remain the definition.
    fs::write(dir.join("src/api.h"), format!("{HEADER}// touched\n")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let nodes = defs_named(&store, "compute");
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].location.file, "src/api.cpp",
        "a header re-extraction must NOT steal the primary from the definition"
    );
    assert!(!nodes[0].is_declaration());
    let _ = fs::remove_dir_all(&dir);
}

/// h+c — the NAMED cross-grammar residual (do not mistake this for the h+cpp
/// join): `.h` → ts-cpp, `.c` → ts-c, so the header prototype and the C
/// definition mint TWO nodes with different scheme prefixes. D6d's gain here is
/// that the declared C API surface is now visible at all; the unification
/// across the grammar seam is explicitly OPEN (an id-shape change, out of
/// Option A's zero-churn bounds — ADR-002 §Accepted residuals).
#[test]
fn header_plus_c_stays_two_nodes_cross_grammar_residual() {
    let dir = fresh_dir("h_c");
    fs::write(dir.join("src/api.h"), HEADER).unwrap();
    fs::write(dir.join("src/api.c"), IMPL_C).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let mut nodes = defs_named(&store, "compute");
    nodes.sort_by(|a, b| a.symbol.0.cmp(&b.symbol.0));
    assert_eq!(
        nodes.len(),
        2,
        "RESIDUAL SHAPE CHANGED: .h (ts-cpp) and .c (ts-c) mint two nodes today — if these \
         now unify, that is a cross-grammar identity change: record it in ADR-002 before \
         shipping (it churns every C symbol id)"
    );
    let proto = nodes
        .iter()
        .find(|n| n.location.file == "src/api.h")
        .expect("the header prototype node — D6d's visible C API surface");
    assert!(proto.symbol.0.starts_with("ts-cpp "), "header rides ts-cpp");
    assert!(proto.is_declaration());
    let def = nodes
        .iter()
        .find(|n| n.location.file == "src/api.c")
        .expect("the C definition node");
    assert!(def.symbol.0.starts_with("ts-c "), "impl rides ts-c");
    assert!(!def.is_declaration());
    let _ = fs::remove_dir_all(&dir);
}

/// Within-file definition preference (found by the lane-B adversarial probe): a
/// definition followed by a same-file REDECLARATION (`int f(int a) {...}` then
/// `int f(int);` — legal C/C++) must stay definition-primary. The store keeps one
/// contribution per (symbol, file) with last-record-wins, so the extractor emits
/// declaration-marked records FIRST (stable sort in treesitter.rs pass 2) — the
/// trailing prototype must not demote the file's contribution to a declaration.
/// Both source orders are pinned.
#[test]
fn same_file_trailing_redeclaration_stays_definition_primary() {
    let dir = fresh_dir("wf_order");
    // def first, redeclaration after — the order that used to demote.
    fs::write(
        dir.join("src/one.cpp"),
        "int foo(int a) { return a; }\nint foo(int);\n",
    )
    .unwrap();
    // decl first, def after — the common order, must also be definition-primary.
    fs::write(
        dir.join("src/two.cpp"),
        "int bar(int);\nint bar(int a) { return a; }\n",
    )
    .unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    for name in ["foo", "bar"] {
        let nodes = defs_named(&store, name);
        assert_eq!(nodes.len(), 1, "one node for `{name}`");
        assert!(
            !nodes[0].is_declaration(),
            "`{name}`: a same-file redeclaration must NOT demote the definition \
             to a declaration-primary record (within-file definition preference)"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
