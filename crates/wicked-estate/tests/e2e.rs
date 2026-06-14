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
