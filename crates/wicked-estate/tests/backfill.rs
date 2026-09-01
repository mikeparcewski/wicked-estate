//! Lane importer-backfill (wicked-estate#141): previously-PARKED references must re-resolve
//! into real edges when a later index run brings their target into the graph — the reverse of
//! Decision J (which re-parks refs when their target is DELETED). Pins the full arc on temp
//! stores + temp dirs: park → target appears → back-fill edge (correct endpoints/provenance);
//! idempotency (re-runs create no duplicate edges, no duplicate parked rows); a target that
//! never arrives stays parked and countable; a labelled run never back-fills across repos.
//!
//! The store keeps ONE direction of parked rows (`from` = the referencing side); there is no
//! in-ref table to cover separately — blast-radius coverage derives the reverse view from the
//! same rows.

use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead, NodeKind};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_backfill_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

/// All `(source file, target file)` pairs of File→File edges minted by the relative-import
/// resolver — the back-fill lane's provenance-pinned observable.
fn relative_import_edges(store: &SqliteStore) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.resolved_by == "relative-import")
        .map(|e| {
            let src = store.get_node(&e.source).unwrap().expect("source node");
            let tgt = store.get_node(&e.target).unwrap().expect("target node");
            assert!(
                matches!(tgt.kind, NodeKind::File),
                "relative-import target must be a File node, got {:?}",
                tgt.kind
            );
            (src.location.file, tgt.location.file)
        })
        .collect();
    out.sort();
    out
}

/// The parked relative-import rows as `(file, raw_name)` pairs — the countable park ledger.
fn parked_imports(store: &SqliteStore) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = store
        .parked_relative_import_refs()
        .unwrap()
        .into_iter()
        .map(|r| (r.location.file, r.raw_name))
        .collect();
    out.sort();
    out
}

/// The full arc for the import lane: park → target file appears in a LATER run → the parked
/// ref becomes a real File→File edge with correct endpoints and provenance, its parked row is
/// retired, and re-ingesting creates no duplicates of either.
#[test]
fn parked_relative_import_backfills_when_target_appears() {
    let dir = fresh_dir("import_arc");
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport function ga() { return b; }\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().unwrap();

    // Run 1: the target is absent → the ref PARKS (honest hole), no edge.
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        relative_import_edges(&store),
        vec![],
        "no target on disk → no File→File edge"
    );
    assert_eq!(
        parked_imports(&store).len(),
        1,
        "the './b' ref must park, and park exactly once"
    );

    // Run 2: the target appears. a.ts is UNCHANGED — the main pass never re-extracts it, so
    // the edge below can only come from the back-fill pass.
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        relative_import_edges(&store),
        vec![("src/a.ts".to_string(), "src/b.ts".to_string())],
        "the parked ref must back-fill into a File→File edge with correct endpoints"
    );
    assert_eq!(
        parked_imports(&store),
        vec![],
        "a back-filled ref's parked row must be retired in the same run"
    );

    // Run 3 (idempotency): touch the target so the run does real work again. Still exactly one
    // edge, still zero parked rows — re-running back-fill duplicates nothing.
    fs::write(dir.join("src/b.ts"), "export const b = 2;\n").unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        relative_import_edges(&store),
        vec![("src/a.ts".to_string(), "src/b.ts".to_string())],
        "re-ingest must not duplicate the back-filled edge"
    );
    assert_eq!(parked_imports(&store), vec![]);

    let _ = fs::remove_dir_all(&dir);
}

/// The name lane: an unchanged file's CALL ref parked against a missing symbol back-fills the
/// run a changed file defines that symbol (the PER-5 "unchanged caller" half).
#[test]
fn parked_call_backfills_when_definition_appears() {
    let dir = fresh_dir("call_arc");
    fs::write(
        dir.join("src/caller.ts"),
        "export function gg() { return helperFn(); }\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().unwrap();

    // Run 1: helperFn is defined nowhere → the Calls ref parks.
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(
        store.unresolved_refs_for_name("helperFn").unwrap().len(),
        1,
        "the call to the missing helperFn must park exactly once"
    );

    // Run 2: a NEW file defines helperFn; caller.ts stays unchanged.
    fs::write(
        dir.join("src/helper.ts"),
        "export function helperFn() { return 1; }\n",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    assert_eq!(
        store.unresolved_refs_for_name("helperFn").unwrap().len(),
        0,
        "the parked call must be retired once its target exists"
    );
    let call_edges: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.kind == EdgeKind::Calls
                && store
                    .get_node(&e.target)
                    .unwrap()
                    .is_some_and(|n| n.name == "helperFn")
        })
        .collect();
    assert_eq!(
        call_edges.len(),
        1,
        "exactly one back-filled Calls edge to helperFn, got {call_edges:?}"
    );
    let src = store
        .get_node(&call_edges[0].source)
        .unwrap()
        .expect("source node");
    assert_eq!(
        src.location.file, "src/caller.ts",
        "the edge's dependent end must be the unchanged caller"
    );

    // Idempotency for the name lane: touch the definition file; still one edge, zero rows.
    fs::write(
        dir.join("src/helper.ts"),
        "export function helperFn() { return 2; }\n",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(store.unresolved_refs_for_name("helperFn").unwrap().len(), 0);
    let n_edges = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.kind == EdgeKind::Calls
                && store
                    .get_node(&e.target)
                    .unwrap()
                    .is_some_and(|n| n.name == "helperFn")
        })
        .count();
    assert_eq!(n_edges, 1, "re-ingest must not duplicate the Calls edge");

    let _ = fs::remove_dir_all(&dir);
}

/// Completing a rename across runs (the issue's §"Still open" example): the importer's text was
/// fixed to `./c` in an earlier run (parked — c.ts absent), and a later run renames b.ts → c.ts.
/// The rename run must back-fill the importer's edge even though the importer never changed
/// again. Also pins coexistence with Decision J (the delete side of the same rename).
#[test]
fn rename_completion_backfills_fixed_importer() {
    let dir = fresh_dir("rename_arc");
    fs::write(
        dir.join("src/a.ts"),
        "import { c } from './c';\nexport function ga() { return c; }\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.ts"), "export const c = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();

    // Run 1: a.ts already points at './c' but only b.ts exists → parks.
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(relative_import_edges(&store), vec![]);
    assert_eq!(parked_imports(&store).len(), 1);

    // Run 2: the rename lands (b.ts deleted, c.ts created with the same content).
    fs::rename(dir.join("src/b.ts"), dir.join("src/c.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    assert_eq!(
        relative_import_edges(&store),
        vec![("src/a.ts".to_string(), "src/c.ts".to_string())],
        "the rename run must back-fill the fixed importer's edge to the new path"
    );
    assert_eq!(parked_imports(&store), vec![]);

    let _ = fs::remove_dir_all(&dir);
}

/// A ref whose target NEVER arrives stays parked — and stays countable at exactly one row no
/// matter how many later ingests (with new files, i.e. with the back-fill import lane active)
/// re-examine it. Never re-inserted, never silently dropped.
#[test]
fn never_arriving_target_stays_parked_without_duplicates() {
    let dir = fresh_dir("ghost_arc");
    fs::write(
        dir.join("src/waits.ts"),
        "import { g } from './ghost';\nexport function w() { return g; }\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert_eq!(parked_imports(&store).len(), 1);

    // Two later runs, each adding an unrelated NEW file so the back-fill import lane runs and
    // re-examines the parked ref each time.
    for (i, name) in ["src/other1.ts", "src/other2.ts"].iter().enumerate() {
        fs::write(dir.join(name), format!("export const o{i} = {i};\n")).unwrap();
        wicked_estate::index_path(&mut store, &dir).unwrap();
        let parked = parked_imports(&store);
        assert_eq!(
            parked.len(),
            1,
            "run {}: the ghost ref must stay parked exactly once, got {parked:?}",
            i + 2
        );
        assert_eq!(parked[0].0, "src/waits.ts");
    }
    assert_eq!(
        relative_import_edges(&store),
        vec![],
        "nothing may bind to a target that never arrived"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Repo scoping: a labelled run's back-fill must not bind ANOTHER repo's parked refs to its own
/// nodes — edges do not resolve across repos (repo_scope contract). repoA's parked call stays
/// parked when repoB later gains a same-name definition.
#[test]
fn backfill_never_crosses_repo_labels() {
    let root = fresh_dir("scope_arc");
    fs::create_dir_all(root.join("repoA/src")).unwrap();
    fs::create_dir_all(root.join("repoB/src")).unwrap();
    fs::write(
        root.join("repoA/src/caller.ts"),
        "export function gg() { return helperFn(); }\n",
    )
    .unwrap();
    fs::write(root.join("repoB/src/base.ts"), "export const base = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("repoa")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();
    assert_eq!(
        store.unresolved_refs_for_name("helperFn").unwrap().len(),
        1,
        "repoA's call must park"
    );

    // A later repoB run (base.ts unchanged → the back-fill pass is active) adds helperFn.
    fs::write(
        root.join("repoB/src/helper.ts"),
        "export function helperFn() { return 1; }\n",
    )
    .unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("repob")).unwrap();

    assert_eq!(
        store.unresolved_refs_for_name("helperFn").unwrap().len(),
        1,
        "repoA's parked call must NOT be consumed by repoB's definition"
    );
    let cross: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.kind == EdgeKind::Calls
                && store
                    .get_node(&e.target)
                    .unwrap()
                    .is_some_and(|n| n.name == "helperFn")
        })
        .collect();
    assert!(
        cross.is_empty(),
        "no cross-repo Calls edge may exist: {cross:?}"
    );

    let _ = fs::remove_dir_all(&root);
}
