//! End-to-end: write real source files → index through the full pipeline → query + blast-radius.
//! Exercises EXTRACT (tree-sitter) → RESOLVE (name resolver) → STORE (SQLite) → traverse.

use std::fs;
use std::path::PathBuf;
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_e2e_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

#[test]
fn end_to_end_index_resolve_blast_radius() {
    let dir = fresh_dir("mixed");
    // Rust: a 3-deep call chain util <- service <- handler.
    fs::write(
        dir.join("src/a.rs"),
        "fn util() {}\nfn service() { util(); }\nfn handler() { service(); }\n",
    )
    .unwrap();
    // Python: a function + a caller (cross-language coverage).
    fs::write(
        dir.join("b.py"),
        "def helper():\n    pass\n\ndef run():\n    helper()\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    let stats = wicked_estate::index_path(&mut store, &dir).expect("index_path");

    // 5 functions + 2 file nodes.
    assert!(
        stats.node_count >= 6,
        "expected >=6 nodes, got {}",
        stats.node_count
    );
    // contains (5) + resolved calls (service->util, handler->service, run->helper = 3).
    assert!(
        stats.edge_count >= 5,
        "expected >=5 edges, got {}",
        stats.edge_count
    );

    // Blast radius of `util` = its transitive dependents: service (1 hop) + handler (2 hops).
    let deps = wicked_estate::blast_radius_by_name(&store, "util", 8).expect("blast radius");
    let names: Vec<&str> = deps.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"service"),
        "service should be a dependent of util, got {names:?}"
    );
    assert!(
        names.contains(&"handler"),
        "handler is a transitive dependent, got {names:?}"
    );

    // Cross-language: the Python function is indexed and findable.
    assert_eq!(
        wicked_estate::search(&store, "helper")
            .expect("search")
            .len(),
        1,
        "python helper indexed"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ── Wave 2.6: incremental indexing tests ──────────────────────────────────────

/// First index, then re-index with NO changes → node count must not change, and the second
/// index must be much faster (or at least complete without re-processing files).
#[test]
fn incremental_unchanged_skips_work() {
    let dir = fresh_dir("incr_unchanged");
    fs::write(dir.join("a.rs"), "fn foo() {}\nfn bar() { foo(); }\n").unwrap();
    fs::write(dir.join("b.rs"), "fn baz() {}\n").unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");

    // First index.
    let stats1 = wicked_estate::index_path(&mut store, &dir).expect("first index");
    assert!(
        stats1.node_count >= 3,
        "expected >=3 nodes after first index, got {}",
        stats1.node_count
    );

    // Second index with IDENTICAL files (no changes).
    let stats2 = wicked_estate::index_path(&mut store, &dir).expect("second index");
    assert_eq!(
        stats1.node_count, stats2.node_count,
        "unchanged re-index must not change node count: {} → {}",
        stats1.node_count, stats2.node_count
    );
    assert_eq!(
        stats1.edge_count, stats2.edge_count,
        "unchanged re-index must not change edge count: {} → {}",
        stats1.edge_count, stats2.edge_count
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Modify one file → re-index → the modified file's symbols are updated; the OTHER file's symbols
/// are preserved and NOT duplicated.
#[test]
fn incremental_modified_file_updates_symbols() {
    let dir = fresh_dir("incr_modified");
    fs::write(dir.join("stable.rs"), "fn stable_fn() {}\n").unwrap();
    fs::write(dir.join("changed.rs"), "fn old_fn() {}\n").unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");

    // First index.
    wicked_estate::index_path(&mut store, &dir).expect("first index");

    // Verify old_fn exists, new_fn does not.
    assert_eq!(
        wicked_estate::search(&store, "old_fn").unwrap().len(),
        1,
        "old_fn after first index"
    );
    assert_eq!(
        wicked_estate::search(&store, "new_fn").unwrap().len(),
        0,
        "new_fn should not exist yet"
    );
    assert_eq!(
        wicked_estate::search(&store, "stable_fn").unwrap().len(),
        1,
        "stable_fn after first index"
    );

    // Modify changed.rs — rename the function.
    fs::write(dir.join("changed.rs"), "fn new_fn() {}\n").unwrap();

    // Second index.
    wicked_estate::index_path(&mut store, &dir).expect("second index");

    // old_fn must be gone; new_fn must exist; stable_fn must still exist (unchanged file).
    assert_eq!(
        wicked_estate::search(&store, "old_fn").unwrap().len(),
        0,
        "old_fn must be removed after modification"
    );
    assert_eq!(
        wicked_estate::search(&store, "new_fn").unwrap().len(),
        1,
        "new_fn must appear after modification"
    );
    assert_eq!(
        wicked_estate::search(&store, "stable_fn").unwrap().len(),
        1,
        "stable_fn must be preserved (unchanged file)"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Delete a file → re-index → its symbols are removed from the store.
#[test]
fn incremental_deleted_file_removes_symbols() {
    let dir = fresh_dir("incr_deleted");
    fs::write(dir.join("keep.rs"), "fn kept_fn() {}\n").unwrap();
    fs::write(dir.join("delete_me.rs"), "fn doomed_fn() {}\n").unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");

    // First index.
    wicked_estate::index_path(&mut store, &dir).expect("first index");
    assert_eq!(
        wicked_estate::search(&store, "doomed_fn").unwrap().len(),
        1,
        "doomed_fn after first index"
    );
    assert_eq!(
        wicked_estate::search(&store, "kept_fn").unwrap().len(),
        1,
        "kept_fn after first index"
    );

    // Delete one file.
    fs::remove_file(dir.join("delete_me.rs")).unwrap();

    // Second index.
    wicked_estate::index_path(&mut store, &dir).expect("second index after deletion");

    // doomed_fn must be gone; kept_fn must still exist.
    assert_eq!(
        wicked_estate::search(&store, "doomed_fn").unwrap().len(),
        0,
        "doomed_fn must be removed when file deleted"
    );
    assert_eq!(
        wicked_estate::search(&store, "kept_fn").unwrap().len(),
        1,
        "kept_fn must be preserved"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ── Lane relative-imports S7: importer re-extraction for DELETED targets ──────

/// Rename the TARGET of a relative import: the importer must be re-extracted and its ref
/// re-parked — no stale edge, no dangling edge, an unresolved row for the old spec (PER-5,
/// D01-4; a rename is delete-B + new-C).
#[test]
fn incremental_target_rename_reparks_importer() {
    use wicked_estate_core::{EdgeKind, GraphRead};
    let dir = fresh_dir("incr_rename");
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = 1;\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let rel_edges = |store: &SqliteStore| -> Vec<(String, String)> {
        store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| e.resolved_by == "relative-import")
            .map(|e| (e.source.0.clone(), e.target.0.clone()))
            .collect()
    };
    let before = rel_edges(&store);
    assert_eq!(before.len(), 1, "a.ts → b.ts bound: {before:?}");

    // Rename b.ts → c.ts and re-index.
    fs::rename(dir.join("src/b.ts"), dir.join("src/c.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let after = rel_edges(&store);
    assert!(
        after.is_empty(),
        "no relative-import edge may survive the rename: {after:?}"
    );
    // No dangling edges anywhere (every endpoint resolves to a live node).
    for e in store.all_edges().unwrap() {
        assert!(
            store.get_node(&e.source).unwrap().is_some()
                && store.get_node(&e.target).unwrap().is_some(),
            "dangling edge: {} -> {} ({:?})",
            e.source.as_str(),
            e.target.as_str(),
            e.kind
        );
    }
    // The importer was re-extracted and its ref RE-PARKED.
    let parked = store.unresolved_refs_for_name("'./b'").unwrap();
    assert!(
        parked
            .iter()
            .any(|r| r.kind == EdgeKind::Imports && r.location.file == "src/a.ts"),
        "an unresolved row for a.ts \"'./b'\" must exist after the rename: {parked:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Modify (not delete) the target: the importer's edge survives by store semantics and the
/// importer is NOT re-extracted — the change log carries no entry for a.ts (deleted-only
/// forcing, ATT-INV-2).
#[test]
fn incremental_target_modified_keeps_importer_edge() {
    use wicked_estate_core::GraphRead;
    let dir = fresh_dir("incr_modify");
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = 1;\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let cursor = store
        .changes_since(0)
        .unwrap()
        .last()
        .map(|c| c.seq)
        .unwrap_or(0);

    // Modify b.ts in place; re-index.
    fs::write(
        dir.join("src/b.ts"),
        "export const b = 2;\nexport const b2 = 3;\n",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    // Edge intact.
    let rel: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == "relative-import")
        .collect();
    assert_eq!(rel.len(), 1, "a.ts → b.ts must survive a target MODIFY");

    // a.ts was NOT re-extracted: the new change-log tail has entries for b.ts only.
    let tail = store.changes_since(cursor).unwrap();
    assert!(
        !tail.is_empty(),
        "b.ts modification must be logged: {tail:?}"
    );
    assert!(
        tail.iter().all(|c| c.target != "src/a.ts"),
        "modifying the TARGET must not force the importer (deleted-only scope): {tail:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Deleting a target forces its DIRECT importers only — no transitive cascade (Decision J
/// step 2): a imports b, z imports a; deleting b re-extracts a but never touches z.
#[test]
fn incremental_delete_does_not_cascade() {
    use wicked_estate_core::GraphRead;
    let dir = fresh_dir("incr_cascade");
    fs::write(
        dir.join("src/z.ts"),
        "import { a } from './a';\nexport const z = 1;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = 1;\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let cursor = store
        .changes_since(0)
        .unwrap()
        .last()
        .map(|c| c.seq)
        .unwrap_or(0);

    fs::remove_file(dir.join("src/b.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let tail = store.changes_since(cursor).unwrap();
    assert!(
        tail.iter().any(|c| c.target == "src/a.ts"),
        "the direct importer a.ts must be re-extracted: {tail:?}"
    );
    assert!(
        tail.iter().all(|c| c.target != "src/z.ts"),
        "z.ts (importer-of-the-importer) must NOT be touched — single-pass, no cascade: {tail:?}"
    );
    // z's own edge to a survives (a's File node was re-created under the same id).
    let z_edges: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == "relative-import" && e.source.0.contains("z.ts"))
        .collect();
    assert_eq!(z_edges.len(), 1, "z → a must survive: {z_edges:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// The residual hole, documented as an ASSERTION (not skipped): an importer whose ref was
/// PARKED (target absent at index time) is not re-resolved when the target is later added —
/// no edge exists to discover it by (module doc "Known limitations", D01-7 audit).
#[test]
fn incremental_importer_of_new_target_stays_parked_until_touched() {
    use wicked_estate_core::GraphRead;
    let dir = fresh_dir("incr_parked");
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = 1;\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert!(
        !store.unresolved_refs_for_name("'./b'").unwrap().is_empty(),
        "ref parked while the target is absent"
    );

    // Add the target and re-index: a.ts is unchanged, so its parked ref stays parked.
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let rel: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == "relative-import")
        .collect();
    assert!(
        rel.is_empty(),
        "documented residual: the parked ref is NOT re-resolved until a.ts changes: {rel:?}"
    );
    assert!(
        !store.unresolved_refs_for_name("'./b'").unwrap().is_empty(),
        "the unresolved row persists"
    );
    let _ = fs::remove_dir_all(&dir);
}
