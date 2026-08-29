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

// ── Unresolved accounting (engine defect #3, docs/ENGINE-CONTRACT.md §2.1) ───────────────────

/// Fixture A — repeat call sites and repeat imports of BOUND relationships must not be
/// persisted as unresolved; a genuinely unbound name must keep its row.
///
/// HEAD baselines (location-keyed persistence, recorded before this fix): `g` = 2 rows
/// (sites 2 and 3 of the deduped Calls edge), `h` = 1 row.
///
/// `h` is also the direct Finding-7 regression (F7): same `from`, same kind, one bound
/// raw_name (`g`) and one unbound (`h`) — the exact under-count input of the retired
/// `(source, kind)` telemetry key from 7c9caf0.
#[test]
fn unresolved_accounting_repeat_sites_are_not_unresolved() {
    use wicked_estate_core::{EdgeKind, GraphRead};

    let dir = fresh_dir("unres_a");
    fs::write(
        dir.join("src/mod.ts"),
        "export function g() {}\nexport function k() {}\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.ts"),
        "import {g, k} from './mod';\nimport type {G} from './mod';\nexport function f() { g(); g(); g(); h(); k(); }\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    // Exactly one Calls edge lands on the node named `g` (three call sites, one relationship).
    let g_calls: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| {
            e.kind == EdgeKind::Calls
                && store
                    .get_node(&e.target)
                    .unwrap()
                    .is_some_and(|n| n.name == "g")
        })
        .collect();
    assert_eq!(g_calls.len(), 1, "one deduped Calls edge onto g");

    // No site of the bound f→g relationship is unresolved (HEAD wrote 2 rows here).
    assert!(
        store.unresolved_refs_for_name("g").unwrap().is_empty(),
        "repeat call sites of a bound relationship must not be unresolved"
    );
    // The genuinely unbound call keeps its row (honest coverage).
    assert_eq!(
        store.unresolved_refs_for_name("h").unwrap().len(),
        1,
        "a call to an undefined name stays unresolved"
    );

    // Persistence and stats agree: the store's total is exactly the rows we can enumerate.
    // The Imports rows are whatever the shipped resolvers leave (no relative-import resolver
    // in this lane — merge note M3): the identity holds for any count.
    let h_rows = store.unresolved_refs_for_name("h").unwrap().len() as u64;
    let import_rows = store.unresolved_refs_for_name("'./mod'").unwrap().len() as u64;
    assert_eq!(
        store.stats().unwrap().unresolved_ref_count,
        h_rows + import_rows,
        "stats total == sum of enumerable unresolved rows"
    );

    // Incremental: a changed file's rows are rebuilt from scratch, never accumulated (D3).
    fs::write(
        dir.join("src/main.ts"),
        "import {g, k} from './mod';\nimport type {G} from './mod';\nexport function f() { g(); g(); g(); g(); h(); h(); k(); }\n",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).expect("re-index after change");
    assert!(
        store.unresolved_refs_for_name("g").unwrap().is_empty(),
        "4th call site still not unresolved"
    );
    assert_eq!(
        store.unresolved_refs_for_name("h").unwrap().len(),
        2,
        "rows rebuilt (2 h sites), not accumulated"
    );

    // Deleting the file removes its unresolved rows via remove_file.
    fs::remove_file(dir.join("src/main.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).expect("re-index after delete");
    assert!(store.unresolved_refs_for_name("h").unwrap().is_empty());
    assert!(
        store
            .unresolved_refs_for_name("'./mod'")
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Fixture B1 — multi-target heritage clause: `class C implements A, B` puts BOTH Implements
/// refs at one `(location, kind)` (one query match per type_identifier, same class anchor), so
/// only the collision pass can attribute the single edge for `A` to the right ref.
///
/// HEAD baseline: 0 rows for `B` — the kind-less `HashSet<Location>` let the `A` edge's
/// location "cover" the `B` ref. This test is the end-to-end proof of the collision pass.
#[test]
fn unresolved_accounting_multi_target_heritage_collision() {
    use wicked_estate_core::GraphRead;

    let dir = fresh_dir("unres_b1");
    fs::write(
        dir.join("src/c1.ts"),
        "interface A {}\nclass C implements A, B {}\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    assert!(
        store.unresolved_refs_for_name("A").unwrap().is_empty(),
        "the bound Implements target A has no unresolved row"
    );
    assert_eq!(
        store.unresolved_refs_for_name("B").unwrap().len(),
        1,
        "the undefined Implements target B keeps exactly one row (HEAD wrote 0)"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Fixture B2 — mixed-kind heritage clause: `class C extends A implements B` puts an Extends
/// ref and an Implements ref at ONE span with DIFFERENT kinds. The kind-in-bucket key alone
/// fixes this case (no collision pass involved — do not re-attribute it to the pass).
///
/// HEAD baseline: 0 rows for `B` — the kind-less Location set let the Extends edge for `A`
/// "cover" the Implements ref for `B`.
#[test]
fn unresolved_accounting_kind_distinguishes_shared_span() {
    use wicked_estate_core::GraphRead;

    let dir = fresh_dir("unres_b2");
    fs::write(
        dir.join("src/c2.ts"),
        "class A {}\nclass C extends A implements B {}\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().expect("open sqlite");
    wicked_estate::index_path(&mut store, &dir).expect("index_path");

    assert!(
        store.unresolved_refs_for_name("A").unwrap().is_empty(),
        "the bound Extends target A has no unresolved row"
    );
    assert_eq!(
        store.unresolved_refs_for_name("B").unwrap().len(),
        1,
        "the undefined Implements target B keeps exactly one row (HEAD wrote 0)"
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

/// F2 add-half (incr-integrity lane): rename the target AND update the importer in the SAME
/// run → the importer's ref binds to the NEW file in that one run (the resolve index is built
/// from ALL nodes after the WRITE phase, so c.ts's File node is visible). Exactly one
/// File→File edge to src/c.ts, no unresolved row, no dangling edges.
#[test]
fn incremental_rename_with_updated_importer_rebinds_same_run() {
    use wicked_estate_core::GraphRead;
    let dir = fresh_dir("incr_rename_add");
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = 1;\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    // Rename b→c AND update the importer, then ONE index run.
    fs::rename(dir.join("src/b.ts"), dir.join("src/c.ts")).unwrap();
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './c';\nexport const a = 1;\n",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let rel: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.resolved_by == "relative-import")
        .collect();
    assert_eq!(
        rel.len(),
        1,
        "exactly one File→File edge after the same-run rename+update: {rel:?}"
    );
    assert_eq!(
        store
            .get_node(&rel[0].target)
            .unwrap()
            .expect("live target")
            .location
            .file,
        "src/c.ts",
        "the edge binds to the NEW file"
    );
    // Bound in the same run — no unresolved row for either spec.
    assert!(
        store.unresolved_refs_for_name("'./c'").unwrap().is_empty(),
        "'./c' bound, no park"
    );
    assert!(
        store.unresolved_refs_for_name("'./b'").unwrap().is_empty(),
        "the old spec's row was replaced by the importer's re-extraction"
    );
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
    let _ = fs::remove_dir_all(&dir);
}

/// F2 split two-run variant: run 1 renames the target only (importer unchanged → honest park,
/// remove-half); run 2 updates the importer → rebind. No dangling edges after EITHER run.
#[test]
fn incremental_rename_split_runs_park_then_rebind() {
    use wicked_estate_core::{EdgeKind, GraphRead};
    let dir = fresh_dir("incr_rename_split");
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = 1;\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let no_dangling = |store: &SqliteStore, when: &str| {
        for e in store.all_edges().unwrap() {
            assert!(
                store.get_node(&e.source).unwrap().is_some()
                    && store.get_node(&e.target).unwrap().is_some(),
                "dangling edge {when}: {} -> {} ({:?})",
                e.source.as_str(),
                e.target.as_str(),
                e.kind
            );
        }
    };
    let rel_edges = |store: &SqliteStore| -> Vec<(String, String)> {
        store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| e.resolved_by == "relative-import")
            .map(|e| (e.source.0.clone(), e.target.0.clone()))
            .collect()
    };

    // Run 1: rename only. The importer is unchanged → its ref RE-PARKS (rebinding a
    // non-updated './b' to c.ts would assert a false edge — kept honest, D7).
    fs::rename(dir.join("src/b.ts"), dir.join("src/c.ts")).unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    assert!(
        rel_edges(&store).is_empty(),
        "run 1: no File→File edge may survive the rename"
    );
    assert!(
        store
            .unresolved_refs_for_name("'./b'")
            .unwrap()
            .iter()
            .any(|r| r.kind == EdgeKind::Imports && r.location.file == "src/a.ts"),
        "run 1: honest park for the old spec"
    );
    no_dangling(&store, "after run 1");

    // Run 2: the importer catches up.
    fs::write(
        dir.join("src/a.ts"),
        "import { b } from './c';\nexport const a = 1;\n",
    )
    .unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let rel = rel_edges(&store);
    assert_eq!(rel.len(), 1, "run 2: rebound to the new file: {rel:?}");
    assert!(
        store.unresolved_refs_for_name("'./c'").unwrap().is_empty(),
        "run 2: bound, no park for './c'"
    );
    assert!(
        store.unresolved_refs_for_name("'./b'").unwrap().is_empty(),
        "run 2: the stale './b' park was dropped with a.ts's re-extraction"
    );
    no_dangling(&store, "after run 2");
    let _ = fs::remove_dir_all(&dir);
}
