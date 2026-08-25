//! End-to-end: MANY repos in ONE graph.
//!
//! The bug these tests pin: two repos that both have `src/index.ts` mint identical `files.path`
//! rows AND identical SymbolIds (the relative path is embedded in the id), so indexing the second
//! into the first's db silently destroyed it — no error, no warning, `query alpha` → 0 matches.
//!
//! What is asserted here: labelled indexing co-locates them without collision, an un-labelled
//! second index is REFUSED instead of destroying anything, and an un-labelled index is otherwise
//! unchanged — same paths, same ids, same meta keys as before the label existed.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use wicked_estate::repo_scope;
use wicked_estate_core::GraphRead;
use wicked_estate_store::{GraphStoreMutExt, SqliteStore};

/// Two repos with the SAME relative path, one shared function name, one unique function name.
/// This is the exact collide reproduction from the bug report.
fn make_repo(dir: &Path, unique: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/index.ts"),
        format!("export function {unique}() {{ return shared(); }}\nexport function shared() {{ return 1; }}\n"),
    )
    .unwrap();
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_multirepo_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn names(store: &SqliteStore, name: &str) -> Vec<String> {
    wicked_estate::search(store, name)
        .unwrap()
        .into_iter()
        .map(|n| n.location.file)
        .collect()
}

fn indexed(store: &SqliteStore) -> BTreeSet<String> {
    store.indexed_files().unwrap().into_iter().collect()
}

#[test]
fn labelled_repos_coexist_without_collision() {
    let root = fresh_dir("coexist");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();

    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    let stats =
        wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    // Before the fix: files=1, nodes=3 — repoA overwritten.
    assert_eq!(stats.file_count, 2, "both repos' files must survive");
    assert_eq!(
        indexed(&store),
        BTreeSet::from([
            "repoa/src/index.ts".to_string(),
            "repob/src/index.ts".to_string()
        ])
    );

    // Both repos' unique symbols are queryable.
    assert_eq!(names(&store, "alpha"), vec!["repoa/src/index.ts"]);
    assert_eq!(names(&store, "beta"), vec!["repob/src/index.ts"]);

    // The COLLIDING name resolves to two distinct symbols, one per repo.
    let shared = wicked_estate::search(&store, "shared").unwrap();
    assert_eq!(shared.len(), 2, "shared() must be two symbols, not one");
    let ids: BTreeSet<&str> = shared.iter().map(|n| n.symbol.as_str()).collect();
    assert_eq!(ids.len(), 2, "SymbolIds must differ per repo: {ids:?}");
}

#[test]
fn per_repo_provenance_does_not_clobber() {
    let root = fresh_dir("provenance");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    let reg = repo_scope::registry(&store);
    assert_eq!(reg.len(), 2, "both repos must be registered: {reg:?}");
    assert_eq!(reg[0].label, "repoa");
    assert_eq!(reg[1].label, "repob");
    assert!(reg[0].root.ends_with("repoA"), "{:?}", reg[0].root);
    assert!(reg[1].root.ends_with("repoB"), "{:?}", reg[1].root);

    // The singular repo_* keys stay untouched in a multi-repo graph — there is no ONE commit to
    // report, and reporting the last-indexed repo's is what clobbering looked like.
    assert!(
        store.repo_info().unwrap().is_none(),
        "singular repo_info must not be written for labelled indexes"
    );
}

#[test]
fn unlabelled_second_repo_is_refused_and_nothing_is_lost() {
    let root = fresh_dir("guard");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path(&mut store, &root.join("repoA")).unwrap();

    let err = wicked_estate::index_path(&mut store, &root.join("repoB"))
        .expect_err("the silent overwrite must now be a refusal");
    let msg = err.to_string();
    assert!(msg.contains("REPO COLLISION"), "{msg}");
    assert!(msg.contains("--repo"), "the error must name the fix: {msg}");

    // The refusal is a no-op on the graph: repoA is exactly as it was.
    assert_eq!(names(&store, "alpha"), vec!["src/index.ts"]);
    assert!(names(&store, "beta").is_empty());
    assert_eq!(store.stats().unwrap().file_count, 1);
}

#[test]
fn label_cannot_be_reused_for_a_different_repo() {
    let root = fresh_dir("labelreuse");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("shared-name")).unwrap();

    let err = wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("shared-name"))
        .expect_err("a label already bound to another repo must be refused");
    assert!(err.to_string().contains("already bound"), "{err}");
    assert_eq!(names(&store, "alpha"), vec!["shared-name/src/index.ts"]);
}

#[test]
fn labelled_index_into_an_unlabelled_graph_is_refused() {
    let root = fresh_dir("mix");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path(&mut store, &root.join("repoA")).unwrap();

    let err = wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob"))
        .expect_err("mixing labelled and un-labelled content must be refused");
    assert!(err.to_string().contains("un-labelled content"), "{err}");
}

#[test]
fn re_indexing_one_repo_leaves_the_others_alone() {
    let root = fresh_dir("sweep");
    make_repo(&root.join("repoA"), "alpha");
    make_repo(&root.join("repoB"), "beta");
    let mut store = SqliteStore::open(root.join("shared.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    // Add a file to repoA and drop its original: the delete-sweep must stay inside `repoa/`.
    fs::write(
        root.join("repoA/src/other.ts"),
        "export function gamma() {}\n",
    )
    .unwrap();
    fs::remove_file(root.join("repoA/src/index.ts")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();

    assert_eq!(
        indexed(&store),
        BTreeSet::from([
            "repoa/src/other.ts".to_string(),
            "repob/src/index.ts".to_string()
        ]),
        "repoB's rows must survive repoA's re-index"
    );
    assert_eq!(names(&store, "beta"), vec!["repob/src/index.ts"]);
    assert!(
        names(&store, "alpha").is_empty(),
        "repoA's deleted file swept"
    );
}

/// The compatibility claim, proved rather than asserted: with no label, the pipeline stores the
/// same paths, the same SymbolIds, and the same meta keys it did before labels existed — and a
/// labelled index of the SAME tree differs from it in exactly one way: the `<label>/` prefix.
#[test]
fn unlabelled_indexing_is_unchanged() {
    let root = fresh_dir("compat");
    make_repo(&root.join("repoA"), "alpha");

    let mut plain = SqliteStore::open(root.join("plain.db")).unwrap();
    let plain_stats = wicked_estate::index_path(&mut plain, &root.join("repoA")).unwrap();
    let mut labelled = SqliteStore::open(root.join("labelled.db")).unwrap();
    let labelled_stats =
        wicked_estate::index_path_as(&mut labelled, &root.join("repoA"), Some("repoa")).unwrap();

    // Same graph shape either way.
    assert_eq!(plain_stats.node_count, labelled_stats.node_count);
    assert_eq!(plain_stats.edge_count, labelled_stats.edge_count);
    assert_eq!(plain_stats.file_count, labelled_stats.file_count);

    // Un-labelled paths are bare; labelled ones are the same paths under the label.
    assert_eq!(
        indexed(&plain),
        BTreeSet::from(["src/index.ts".to_string()])
    );
    let expected: BTreeSet<String> = indexed(&plain)
        .iter()
        .map(|p| format!("repoa/{p}"))
        .collect();
    assert_eq!(indexed(&labelled), expected);

    // Same for the ids: prefixing the path inside each SymbolId maps one set onto the other.
    let plain_ids: BTreeSet<String> = plain
        .all_nodes()
        .unwrap()
        .iter()
        .map(|n| n.symbol.as_str().replace("src/index", "repoa/src/index"))
        .collect();
    let labelled_ids: BTreeSet<String> = labelled
        .all_nodes()
        .unwrap()
        .iter()
        .map(|n| n.symbol.as_str().to_string())
        .collect();
    assert_eq!(plain_ids, labelled_ids);

    // Meta: the un-labelled db keeps the singular repo layout and gains NO multi-repo keys.
    assert!(
        repo_scope::registry(&plain).is_empty(),
        "an un-labelled index must not register a repo"
    );
    assert!(plain.meta_get_key("repo_labels").is_none());
    assert_eq!(
        plain.meta_get_key("indexed_version").as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "the singular indexed_version key must still be written"
    );
    assert!(plain.meta_get_key("indexed_root").is_some());
    assert!(plain.meta_get_key("extra_rules_digest").is_some());

    // Re-indexing the un-labelled db is still allowed and still incremental.
    let again = wicked_estate::index_path(&mut plain, &root.join("repoA")).unwrap();
    assert_eq!(again.node_count, plain_stats.node_count);
}

/// SCIP correlation matches a document's REPO-relative path against `nodes.file`. In a labelled
/// graph those differ by the `<label>/` prefix, so an un-labelled ingest silently correlates
/// nothing. (The fixture is the resolve crate's — one `.scip` file, not two.)
#[test]
fn scip_ingest_correlates_against_a_labelled_repo() {
    use std::io::Write as _;
    use wicked_estate_core::{
        Descriptor, GraphWrite, Language, Location, Node, NodeKind, Span, Symbol,
    };

    let fixture: &[u8] =
        include_bytes!("../../wicked-estate-resolve/tests/fixtures/sample-ts.scip");
    let root = fresh_dir("scip");
    let scip_path = root.join("index.scip");
    let mut f = fs::File::create(&scip_path).unwrap();
    f.write_all(fixture).unwrap();
    drop(f);

    // Two nodes as the labelled indexer would have stored them: paths under `repoa/`.
    let node = |name: &str, file: &str, l0: u32, l1: u32| {
        Node::new(
            Symbol::global(
                "ci-test",
                None,
                vec![Descriptor::method(format!("{file}::{name}"), None)],
            )
            .id(),
            NodeKind::Function,
            name,
            Language::new("typescript"),
            Location::new(
                file,
                Span {
                    start_byte: 0,
                    end_byte: 0,
                    start_line: l0,
                    start_col: 0,
                    end_line: l1,
                    end_col: 80,
                },
            ),
        )
    };
    let mut store = SqliteStore::open(root.join("scip.db")).unwrap();
    store
        .upsert_nodes(&[
            node("helper", "repoa/src/util.ts", 0, 0),
            node("run", "repoa/src/main.ts", 1, 2),
        ])
        .unwrap();

    let blind = wicked_estate::ingest_scip(&mut store, &root, &scip_path).unwrap();
    assert_eq!(
        blind, 0,
        "un-labelled correlation cannot match `repoa/…` nodes"
    );
    let scoped =
        wicked_estate::ingest_scip_as(&mut store, &root, &scip_path, Some("repoa")).unwrap();
    assert!(
        scoped > 0,
        "labelled correlation must find the precise edges"
    );
}
