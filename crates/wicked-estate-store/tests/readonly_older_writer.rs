//! wicked-estate#601: a graph the 0.35.10 release wrote, with no newer writer opened since, must
//! open through the read-only currency check and answer every read. 0.35.11 added `edge_files`
//! (#543), which only writes touch, and its absence refused every read-only reader.
//!
//! `tests/fixtures/store_v0_35_10.db` is the 0.35.10 CLI's own output for one TypeScript file
//! (`tests/fixture-tools/make_store_v0_35_10.sh`), not a hand-made store.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use wicked_estate_core::{
    Direction, EdgeKind, GraphRead, SupportOwner, SymbolQuery, TraversalSpec,
};
use wicked_estate_store::SqliteStore;

fn fixture_copy(dir: &Path) -> PathBuf {
    let db = dir.join("graph.db");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store_v0_35_10.db"),
        &db,
    )
    .unwrap();
    db
}

fn object_names(db: &Path) -> BTreeSet<String> {
    let c = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut s = c
        .prepare("SELECT name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
        .unwrap();
    s.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// The fixture is the 0.35.10 shape: next to a graph this build creates, it lacks exactly the
/// objects 0.35.11 added, and its one extra object is the 0.35.x FTS shadow-table guard trigger,
/// which this build's writer opens drop (#570) and its readers do not need.
#[test]
fn the_fixture_is_a_0_35_10_graph_601() {
    let tmp = tempfile::tempdir().unwrap();
    let old = fixture_copy(tmp.path());
    let fresh = tmp.path().join("fresh.db");
    drop(SqliteStore::open(&fresh).unwrap());
    let (old, fresh) = (object_names(&old), object_names(&fresh));
    let missing: Vec<&str> = fresh.difference(&old).map(String::as_str).collect();
    assert_eq!(missing, ["edge_files", "idx_edge_files_file"]);
    let extra: Vec<&str> = old.difference(&fresh).map(String::as_str).collect();
    assert_eq!(extra, ["nodes_fts_rowid_guard"]);
}

/// The read-only currency check opens the 0.35.10 graph without writing a byte, and every read
/// answers from it.
#[test]
fn a_graph_an_older_writer_left_opens_read_only_and_answers_601() {
    let tmp = tempfile::tempdir().unwrap();
    let db = fixture_copy(tmp.path());
    let before = std::fs::read(&db).unwrap();
    let store = SqliteStore::open_readonly_current(&db)
        .unwrap_or_else(|e| panic!("a 0.35.10 graph must open read-only: {e}"));

    let by_name = |name: &str| {
        store
            .find_symbols(&SymbolQuery {
                exact_name: Some(name.to_string()),
                ..Default::default()
            })
            .unwrap()
    };
    let alpha = by_name("alpha");
    assert_eq!(alpha.len(), 1, "{alpha:?}");
    let alpha = &alpha[0];
    assert_eq!(by_name("beta").len(), 1);
    assert!(store.get_node(&alpha.id).unwrap().is_some());
    assert_eq!(
        store
            .find_symbols(&SymbolQuery {
                text: Some("alpha".to_string()),
                ..Default::default()
            })
            .unwrap()
            .len(),
        1,
        "full-text search"
    );
    assert_eq!(store.count_symbols(&SymbolQuery::default()).unwrap(), 3);
    assert!(
        !store
            .find_symbols_by_name_prefix("al", None)
            .unwrap()
            .is_empty()
    );
    let out = store.neighbors(&alpha.id, Direction::Dependencies).unwrap();
    assert!(
        out.iter().any(|e| e.kind == EdgeKind::Calls),
        "alpha calls beta: {out:?}"
    );
    let _ = store
        .traverse(&alpha.id, &TraversalSpec::blast_radius(3))
        .unwrap();
    let _ = store
        .traverse_multi(
            std::slice::from_ref(&alpha.id),
            &TraversalSpec::blast_radius(3),
        )
        .unwrap();
    assert_eq!(store.all_nodes().unwrap().len(), 3);
    assert_eq!(store.all_edges().unwrap().len(), 3);
    assert_eq!(store.indexed_files().unwrap(), ["src/a.ts"]);
    assert!(store.file_digest("src/a.ts").unwrap().is_some());
    let _ = store.file_git_sha("src/a.ts").unwrap();
    let _ = store.repo_info().unwrap();
    let _ = store.edge_history("src/a.ts").unwrap();
    let _ = store.file_content("src/a.ts").unwrap();
    let _ = store.file_call_refs("src/a.ts").unwrap();
    let _ = store.symbol_source(alpha).unwrap();
    let _ = store.changes_since(0).unwrap();
    let _ = store.node_semantics(&alpha.id).unwrap();
    let _ = store.semantics_in_files(&["src/a.ts"]).unwrap();
    let _ = store.find_by_requirement("REQ-1").unwrap();
    let _ = store.annotations(&alpha.id).unwrap();
    let _ = store.annotations_by_type("note").unwrap();
    let _ = store.annotations_stale_since(0).unwrap();
    let _ = store.symbol_epoch(&alpha.id).unwrap();
    let _ = store.support_owners().unwrap();
    let _ = store.capabilities();
    assert_eq!(
        store
            .find_structural_symbols(&SymbolQuery::default())
            .unwrap()
            .len(),
        3
    );
    for e in &out {
        let _ = store.edge_supports(&e.source, &e.target, &e.kind).unwrap();
    }
    let _ = store
        .support_generation(&SupportOwner::new("scip", "s1").unwrap())
        .unwrap();
    let _ = store.unresolved_refs_for_name("gamma").unwrap();
    let _ = store
        .count_unresolved_refs_from(std::slice::from_ref(&alpha.id))
        .unwrap();
    let stats = store.stats().unwrap();
    assert_eq!((stats.node_count, stats.edge_count), (3, 3));
    drop(store);
    assert!(
        std::fs::read(&db).unwrap() == before,
        "a read-only open wrote"
    );
}

/// Only objects reads never touch are excused: a 0.35.10 graph that also lacks a table reads use
/// is still refused, naming it.
#[test]
fn a_graph_missing_a_table_reads_use_is_still_refused_601() {
    let tmp = tempfile::tempdir().unwrap();
    let db = fixture_copy(tmp.path());
    Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TABLE search_misses;")
        .unwrap();
    let err = SqliteStore::open_readonly_current(&db)
        .err()
        .expect("a graph without search_misses is refused");
    assert!(
        err.to_string().contains("search_misses")
            && err.to_string().contains("predates this build"),
        "{err}"
    );
}
