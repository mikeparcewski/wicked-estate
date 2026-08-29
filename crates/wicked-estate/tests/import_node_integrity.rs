//! Shared Import-node integrity across incremental runs (incr-integrity lane).
//!
//! The defect these tests pin: a `NodeKind::Import` node is keyed by module SPECIFIER
//! (`import/<spec>/`, no path), shared by every file importing the same spec, and homed at
//! whichever importer wrote it LAST (upsert is `ON CONFLICT ... SET file=excluded.file` —
//! last-writer-wins, pinned by `owner_is_last_writer` below). Removing or editing the owner
//! deleted the shared node and stranded every other importer's File→Import edge: dangling
//! after a deletion-only run (83 on a wicked-studio probe repro, 147-class on command_iq),
//! then silently pruned with no re-park on the next changed run.
//!
//! The fix lives at the store seam (`remove_file` keeps + re-homes a shared Import node with
//! survivor edges) — every 0-dangling assertion here holds on a DELETION-ONLY run, where NO
//! engine prune executes (the `changed.is_empty()` early return fires before Task D). That is
//! the evidence the fix is at the right seam, not masked by a prune.

use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead, Node, NodeKind};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_impnode_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

const IMPORTING: &str = "import crypto from 'node:crypto';\nexport const v = 1;\n";

/// The shared Import node for `node:crypto`, if present.
fn crypto_import_node(store: &SqliteStore) -> Option<Node> {
    let mut found: Vec<Node> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .filter(|n| n.kind == NodeKind::Import && n.name == "node:crypto")
        .collect();
    assert!(
        found.len() <= 1,
        "the spec-keyed Import node must be a SINGLE shared node; got {:?}",
        found
            .iter()
            .map(|n| (&n.symbol.0, &n.location.file))
            .collect::<Vec<_>>()
    );
    found.pop()
}

/// Every edge whose source or target is not a live node.
fn dangling_edges(store: &SqliteStore) -> Vec<(String, String, EdgeKind)> {
    store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| {
            store.get_node(&e.source).unwrap().is_none()
                || store.get_node(&e.target).unwrap().is_none()
        })
        .map(|e| (e.source.0, e.target.0, e.kind))
        .collect()
}

/// Files holding a live File→Import edge into the shared node.
fn importer_files(store: &SqliteStore, target: &Node) -> Vec<String> {
    let mut v: Vec<String> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.target == target.symbol)
        .filter_map(|e| store.get_node(&e.source).unwrap())
        .map(|n| n.location.file)
        .collect();
    v.sort();
    v
}

/// D10 pin: ownership of the shared node is LAST-writer-wins — the repro tests below sequence
/// runs so the to-be-removed file is the CURRENT owner, and this assertion is what keeps them
/// from going vacuously green if ownership semantics ever change.
#[test]
fn owner_is_last_writer() {
    let dir = fresh_dir("owner");
    fs::write(dir.join("src/a.ts"), IMPORTING).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        crypto_import_node(&store)
            .expect("import node")
            .location
            .file,
        "src/a.ts",
        "sole importer owns the node"
    );

    fs::write(dir.join("src/b.ts"), IMPORTING).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        crypto_import_node(&store)
            .expect("import node")
            .location
            .file,
        "src/b.ts",
        "the LAST writer owns the shared node (ON CONFLICT ... file=excluded.file)"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// T1 — owner DELETED, deletion-only run: the shared node survives re-homed to the unchanged
/// importer, whose edge stays live; 0 dangling with NO prune on this path; a subsequent no-op
/// run is stat-identical and write-free.
#[test]
fn owner_delete_keeps_shared_node_for_unchanged_importer() {
    let dir = fresh_dir("t1");
    fs::write(dir.join("src/a.ts"), IMPORTING).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    // Sequence ownership: b.ts written in a LATER run becomes the owner (asserted, not assumed).
    fs::write(dir.join("src/b.ts"), IMPORTING).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        crypto_import_node(&store)
            .expect("import node")
            .location
            .file,
        "src/b.ts",
        "precondition: the file we are about to delete is the CURRENT owner"
    );

    // DELETION-ONLY run: a.ts unchanged, b.ts gone → `changed` is empty, the engine early-returns
    // before Task D — every assertion below holds under the store seam alone (D1).
    fs::remove_file(dir.join("src/b.ts")).unwrap();
    let stats3 = wicked_estate::index_path(&mut store, &dir).unwrap();

    let node = crypto_import_node(&store)
        .expect("the shared Import node must SURVIVE its owner's deletion");
    assert_eq!(
        node.location.file, "src/a.ts",
        "kept node re-homed to the surviving importer"
    );
    assert_eq!(
        importer_files(&store, &node),
        vec!["src/a.ts".to_string()],
        "exactly the unchanged importer's File→Import edge remains live"
    );
    assert_eq!(
        dangling_edges(&store),
        Vec::new(),
        "0 dangling immediately after the deletion-only run — with NO prune on this path"
    );

    // No-op run (falsifier 4): identical stats, no change-log writes, still 0 dangling.
    let cursor = store
        .changes_since(0)
        .unwrap()
        .last()
        .map(|c| c.seq)
        .unwrap_or(0);
    let stats4 = wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        stats3.node_count, stats4.node_count,
        "no-op run: node count"
    );
    assert_eq!(
        stats3.edge_count, stats4.edge_count,
        "no-op run: edge count"
    );
    assert_eq!(
        stats3.file_count, stats4.file_count,
        "no-op run: file count"
    );
    assert_eq!(
        stats3.unresolved_ref_count, stats4.unresolved_ref_count,
        "no-op run: unresolved count"
    );
    assert_eq!(
        store.changes_since(cursor).unwrap(),
        Vec::new(),
        "a fully-unchanged run performs no graph writes (empty change log delta)"
    );
    assert_eq!(dangling_edges(&store), Vec::new(), "no-op run: still clean");
    let _ = fs::remove_dir_all(&dir);
}

/// T2 — NON-owner deleted (order-independence): the node stays homed at the owner; only the
/// deleted importer's edge goes away, and it goes away cleanly.
#[test]
fn non_owner_delete_leaves_node_and_owner_edge_untouched() {
    let dir = fresh_dir("t2");
    fs::write(dir.join("src/a.ts"), IMPORTING).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    fs::write(dir.join("src/b.ts"), IMPORTING).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap(); // owner: src/b.ts

    fs::remove_file(dir.join("src/a.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let node = crypto_import_node(&store).expect("node survives a non-owner deletion");
    assert_eq!(node.location.file, "src/b.ts", "owner unchanged");
    assert_eq!(
        importer_files(&store, &node),
        vec!["src/b.ts".to_string()],
        "only the owner's edge remains"
    );
    assert_eq!(dangling_edges(&store), Vec::new(), "0 dangling");
    let _ = fs::remove_dir_all(&dir);
}

/// T3 — owner EDITED to drop the import (no deletion anywhere): the wider trigger surface.
/// remove_file(owner) runs on the changed-file path, the owner's re-extraction no longer mints
/// the node, and Task D's prune DOES run this time — the unchanged importer's edge must survive
/// both.
#[test]
fn owner_edited_to_drop_import_keeps_other_importers_edge() {
    let dir = fresh_dir("t3");
    fs::write(dir.join("src/a.ts"), IMPORTING).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    fs::write(dir.join("src/b.ts"), IMPORTING).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap(); // owner: src/b.ts

    fs::write(dir.join("src/b.ts"), "export const v = 2;\n").unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let node = crypto_import_node(&store)
        .expect("the shared node must survive the owner dropping its import");
    assert_eq!(
        node.location.file, "src/a.ts",
        "re-homed to the still-importing file"
    );
    assert_eq!(
        importer_files(&store, &node),
        vec!["src/a.ts".to_string()],
        "the unchanged importer's edge survives the owner edit AND the same-run prune"
    );
    assert_eq!(dangling_edges(&store), Vec::new(), "0 dangling");
    let _ = fs::remove_dir_all(&dir);
}

/// T4 — one-run BATCH delete of owner + a non-owner with a third importer surviving (the
/// engine-level pin of the conformance kit's per-call batch case): the node ends homed at the
/// survivor regardless of removal order inside the batch.
#[test]
fn batch_delete_of_owner_and_non_owner_rehomes_to_survivor() {
    let dir = fresh_dir("t4");
    fs::write(dir.join("src/a.ts"), IMPORTING).unwrap();
    fs::write(dir.join("src/c.ts"), IMPORTING).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    fs::write(dir.join("src/b.ts"), IMPORTING).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap(); // owner: src/b.ts

    // Delete owner + one non-owner in the SAME run (one removal batch, two remove_file calls).
    fs::remove_file(dir.join("src/a.ts")).unwrap();
    fs::remove_file(dir.join("src/b.ts")).unwrap();
    let _stats = wicked_estate::index_path(&mut store, &dir).unwrap();

    let node = crypto_import_node(&store).expect("node survives the batch: c.ts still imports");
    assert_eq!(
        node.location.file, "src/c.ts",
        "per-call evaluation inside the batch ends homed at the live survivor"
    );
    assert_eq!(
        importer_files(&store, &node),
        vec!["src/c.ts".to_string()],
        "only the survivor's edge remains"
    );
    assert_eq!(dangling_edges(&store), Vec::new(), "0 dangling");

    // Self-termination: delete the last importer too → the node goes with it, no island.
    fs::remove_file(dir.join("src/c.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert!(
        crypto_import_node(&store).is_none(),
        "removing the LAST importer deletes the shared node (no island)"
    );
    assert_eq!(dangling_edges(&store), Vec::new(), "0 dangling at the end");
    let _ = fs::remove_dir_all(&dir);
}
