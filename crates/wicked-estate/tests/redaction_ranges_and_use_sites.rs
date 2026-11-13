//! Project documents, estate slice: per-file redaction ranges (`GraphRead::file_redactions`) and
//! per-site use edges (`GraphRead::use_sites`), both recorded by the index into additive SQLite
//! tables and read back after the store is closed and reopened.

use std::collections::HashSet;
use std::path::Path;

use wicked_estate::index_path;
use wicked_estate_core::{EdgeKind, GraphRead, GraphWrite, Node, UseSite};
use wicked_estate_store::{MemStore, SqliteStore};

fn node_named(store: &SqliteStore, name: &str) -> Node {
    GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node named {name}"))
}

fn index(db: &Path, root: &Path) {
    let mut store = SqliteStore::open(db).unwrap();
    index_path(&mut store, root).unwrap();
}

#[test]
fn masked_ranges_cover_the_literal_survive_reopen_and_are_replaced_on_reindex() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let db = tmp.path().join("graph.db");
    std::fs::write(root.join("app.json"), "{\"n\": 4242}").unwrap();
    index(&db, &root);

    let store = SqliteStore::open(&db).unwrap();
    // The stored text is masked (redact.rs numeric mask: 4242 -> 1000) ...
    assert_eq!(
        store.file_content("app.json").unwrap().as_deref(),
        Some("{\"n\": 1000}")
    );
    // ... and the recorded range covers the literal's bytes 6..10.
    let got = store.file_redactions("app.json").unwrap();
    let ranges = got.expect("certified");
    assert!(
        ranges.iter().any(|&(s, e)| s <= 6 && e >= 10),
        "no range covers the literal: {ranges:?}"
    );
    // A file the index never recorded is not certified.
    assert_eq!(store.file_redactions("nope.json").unwrap(), None);
    drop(store);

    // Incremental re-index of the changed file replaces its ranges.
    std::fs::write(root.join("app.json"), "{\"n\": 777}").unwrap();
    index(&db, &root);
    let store = SqliteStore::open(&db).unwrap();
    let got = store.file_redactions("app.json").unwrap();
    let ranges = got.expect("certified");
    assert!(
        ranges.iter().any(|&(s, e)| s <= 6 && e >= 9) && ranges.iter().all(|&(_, e)| e <= 9),
        "{ranges:?}"
    );
}

#[test]
fn a_file_with_nothing_masked_is_certified_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let db = tmp.path().join("graph.db");
    std::fs::write(root.join("lib.rs"), "pub fn plain() {}\n").unwrap();
    index(&db, &root);
    let store = SqliteStore::open(&db).unwrap();
    assert_eq!(store.file_redactions("lib.rs").unwrap(), Some(Vec::new()));
}

#[test]
fn text_written_without_ranges_is_not_certified() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("graph.db");
    let mut store = SqliteStore::open(&db).unwrap();
    store.set_file_content("a.json", "{\"n\": 1000}").unwrap();
    assert_eq!(store.file_redactions("a.json").unwrap(), None);
    store.set_file_redactions("a.json", &[(6, 10)]).unwrap();
    assert_eq!(
        store.file_redactions("a.json").unwrap(),
        Some(vec![(6, 10)])
    );
    // Any later write of the text drops the ranges: they described the text before it, even when
    // the bytes written are the same.
    store.set_file_content("a.json", "{\"n\": 1000}").unwrap();
    assert_eq!(store.file_redactions("a.json").unwrap(), None);
    // remove_file drops them with the file.
    store.set_file_redactions("a.json", &[]).unwrap();
    assert_eq!(store.file_redactions("a.json").unwrap(), Some(Vec::new()));
    store.remove_file("a.json").unwrap();
    assert_eq!(store.file_redactions("a.json").unwrap(), None);
    // A backend that records nothing reads None.
    let mut mem = MemStore::new();
    mem.set_file_content("a.json", "x").unwrap();
    mem.set_file_redactions("a.json", &[(0, 1)]).unwrap();
    assert_eq!(mem.file_redactions("a.json").unwrap(), None);
}

const COBOL: &str = concat!(
    "       IDENTIFICATION DIVISION.\n",
    "       PROGRAM-ID. DEMO.\n",
    "       DATA DIVISION.\n",
    "       WORKING-STORAGE SECTION.\n",
    "       01  WS-SHARED         PIC 9(4).\n",
    "       01  WS-ONLY-ONE       PIC 9(4).\n",
    "       01  WS-ONLY-TWO       PIC 9(4).\n",
    "       PROCEDURE DIVISION.\n",
    "       PARA-ONE.\n",
    "           MOVE 1 TO WS-SHARED.\n",
    "           MOVE 2 TO WS-ONLY-ONE.\n",
    "       PARA-TWO.\n",
    "           MOVE 3 TO WS-SHARED.\n",
    "           MOVE 4 TO WS-ONLY-TWO.\n",
    "           STOP RUN.\n",
);

fn line_of(needle: &str) -> u32 {
    COBOL
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not in the program")) as u32
}

#[test]
fn each_paragraph_using_a_data_item_has_its_own_site_after_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let db = tmp.path().join("graph.db");
    std::fs::write(root.join("DEMO.cbl"), COBOL).unwrap();
    index(&db, &root);

    // Closed and reopened: the sites come from the store, not from the indexing run.
    let store = SqliteStore::open(&db).unwrap();
    let para_one = node_named(&store, "PARA-ONE");
    let para_two = node_named(&store, "PARA-TWO");
    let sites = |item: &str| {
        let target = node_named(&store, item).symbol;
        GraphRead::use_sites(&store, &target).unwrap()
    };
    // COBOL attributes a paragraph's data references to the program, so a site's own LOCATION
    // says which paragraph used the item. Membership is decided by BYTES: a paragraph's byte span
    // is exact (it ends where the next begins), its end line is not.
    let covers = |para: &Node, site: &UseSite| {
        let (p, u) = (para.location.span, site.location.span);
        p.start_byte <= u.start_byte && u.end_byte <= p.end_byte
    };

    let shared = sites("WS-SHARED");
    assert_eq!(shared.len(), 2, "{shared:#?}");
    let want_one = line_of("MOVE 1 TO WS-SHARED");
    let want_two = line_of("MOVE 3 TO WS-SHARED");
    let lines: Vec<u32> = shared.iter().map(|s| s.location.span.start_line).collect();
    assert_eq!(lines, vec![want_one, want_two], "{shared:#?}");
    assert!(covers(&para_one, &shared[0]) && !covers(&para_two, &shared[0]));
    assert!(covers(&para_two, &shared[1]) && !covers(&para_one, &shared[1]));
    assert!(shared.iter().all(|s| s.location.file == "DEMO.cbl"));

    // Different items: each item has only the site of the paragraph that uses it.
    let only_one = sites("WS-ONLY-ONE");
    assert_eq!(only_one.len(), 1, "{only_one:#?}");
    assert_eq!(
        only_one[0].location.span.start_line,
        line_of("MOVE 2 TO WS-ONLY-ONE")
    );
    assert!(covers(&para_one, &only_one[0]) && !covers(&para_two, &only_one[0]));
    let only_two = sites("WS-ONLY-TWO");
    assert_eq!(only_two.len(), 1, "{only_two:#?}");
    assert_eq!(
        only_two[0].location.span.start_line,
        line_of("MOVE 4 TO WS-ONLY-TWO")
    );
    assert!(covers(&para_two, &only_two[0]) && !covers(&para_one, &only_two[0]));

    // The stored graph is still deduplicated: one edge per (source, target, kind), so the two
    // sites of the shared item sit under ONE References edge from the program.
    let edges = GraphRead::all_edges(&store).unwrap();
    let keys: HashSet<_> = edges.iter().map(|e| e.dedup_key()).collect();
    assert_eq!(keys.len(), edges.len(), "stored edges are unique per key");
    let shared_id = node_named(&store, "WS-SHARED").symbol;
    let to_shared = edges
        .iter()
        .filter(|e| e.target == shared_id && e.kind == EdgeKind::References)
        .count();
    assert_eq!(to_shared, 1);
    drop(store);

    // Re-index after an edit: the file's sites are replaced, not accumulated.
    std::fs::write(root.join("DEMO.cbl"), format!("{COBOL}\n")).unwrap();
    index(&db, &root);
    let store = SqliteStore::open(&db).unwrap();
    let target = node_named(&store, "WS-SHARED").symbol;
    assert_eq!(GraphRead::use_sites(&store, &target).unwrap().len(), 2);
}

#[test]
fn removing_a_file_drops_its_sites_and_other_backends_have_none() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let db = tmp.path().join("graph.db");
    std::fs::write(root.join("DEMO.cbl"), COBOL).unwrap();
    index(&db, &root);
    let mut store = SqliteStore::open(&db).unwrap();
    let target = node_named(&store, "WS-SHARED").symbol;
    assert_eq!(GraphRead::use_sites(&store, &target).unwrap().len(), 2);
    store.remove_file("DEMO.cbl").unwrap();
    assert!(GraphRead::use_sites(&store, &target).unwrap().is_empty());
    let none = GraphRead::use_sites(&MemStore::new(), &target).unwrap();
    assert!(none.is_empty());
}
