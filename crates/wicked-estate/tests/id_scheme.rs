//! The symbol-id scheme gate (ADR-002 amendment — type-nested definition identity).
//!
//! The bug these tests pin: scheme 2 changed every type-member's SymbolId, but the binary
//! version did NOT change, so the `indexed_version` gate never fires and unchanged digests skip
//! every file — a previously-indexed DB would silently mix flat v1 ids with nested v2 ids.
//! The per-repo `id_scheme` meta key forces a full re-extraction instead, and it is written
//! only AFTER the re-extraction completes so an interrupted migration re-fires the gate.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use wicked_estate::repo_scope;
use wicked_estate_core::{
    EdgeKind, GraphRead, GraphWrite, Language, Location, Node, NodeKind, Span, SymbolId,
};
use wicked_estate_extract::SYMBOL_ID_SCHEME;
use wicked_estate_store::{GraphStoreMutExt, SqliteStore};

/// The doc03 collision fixture: two classes and an interface each defining `save`, an
/// object-literal `save`, and a method-local `const save = () =>` arrow.
const FIXTURE_TS: &str = r#"import { other } from "./other";
export class Repo { save(): void {} update(): void { this.save(); other.save(); } }
export class Cache {
  save(): void {}
  flush(): void { this.save(); const cb = () => this.save(); const save = () => {}; save(); }
}
export interface Store { save(): void; }
export const lit = { save() {}, run() { this.save(); } };
export function top() { const r = new Repo(); r.update(); }
"#;

fn make_repo(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/fixture.ts"), FIXTURE_TS).unwrap();
}

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_idscheme_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A v1-shaped (flat, 2-descriptor) node at `rel` — the id shape scheme 2 no longer mints for a
/// type member. `rel` MUST be the exact path the indexer stores: `remove_file` deletes
/// `WHERE file = ?1`, so a wrong path would make the node survive for a path reason, not a gate
/// reason.
fn flat_v1_node(sym: &str, name: &str, rel: &str) -> Node {
    Node::new(
        SymbolId::from(sym),
        NodeKind::Method,
        name,
        Language::new("typescript"),
        Location::new(rel, Span::ZERO),
    )
}

fn symbols_named(store: &SqliteStore, name: &str) -> HashSet<String> {
    wicked_estate::search(store, name)
        .unwrap()
        .into_iter()
        .map(|n| n.symbol.as_str().to_string())
        .collect()
}

/// The read-back storage path of the indexed fixture file (MI-A4: never guess it).
fn stored_rel_path(store: &SqliteStore) -> String {
    wicked_estate::search(store, "Repo")
        .unwrap()
        .into_iter()
        .next()
        .expect("Repo node must exist after indexing")
        .location
        .file
}

/// A same-version DB whose rows were minted under the old scheme must be fully re-extracted on
/// the next index WITHOUT --force. This is ALSO the interrupted-migration test: because the key
/// is written last, the state a crash leaves behind (digests current, v1 rows present, key still
/// old) is exactly the state constructed here, and the gate must fire on it.
#[test]
fn same_version_old_scheme_db_is_fully_reextracted_without_force() {
    let root = fresh_dir("old_scheme");
    make_repo(&root.join("repo"));
    let mut store = SqliteStore::open(root.join("g.db")).unwrap();
    wicked_estate::index_path(&mut store, &root.join("repo")).unwrap();
    let rel = stored_rel_path(&store);

    // Simulate the pre-migration state through the store API: old scheme key + a flat v1 id
    // that scheme 2 never mints (update nests as Repo#update().). Digests stay current, so
    // without the gate every file would be skipped.
    store.meta_set_key("id_scheme", "1");
    store
        .upsert_nodes(&[flat_v1_node(
            "ts-typescript . . . src/fixture/update().",
            "update",
            &rel,
        )])
        .unwrap();

    wicked_estate::index_path(&mut store, &root.join("repo")).unwrap();

    let updates = symbols_named(&store, "update");
    assert!(
        !updates.contains("ts-typescript . . . src/fixture/update()."),
        "the stale flat v1 id must be purged by the forced re-extraction; got {updates:?}"
    );
    assert!(
        updates.contains("ts-typescript . . . src/fixture/Repo#update()."),
        "the type-nested v2 id must exist; got {updates:?}"
    );
    assert_eq!(
        store.meta_get_key("id_scheme").as_deref(),
        Some(SYMBOL_ID_SCHEME),
        "the scheme key is stamped only after the re-extraction completed"
    );
}

/// Mirrors `the_binary_version_is_recorded_per_repo`: a labelled run writes
/// `repo:<label>:id_scheme` and never the bare key — one repo's scheme must not answer for
/// every other repo's staleness.
#[test]
fn id_scheme_is_recorded_per_repo() {
    let root = fresh_dir("per_repo");
    make_repo(&root.join("repoA"));
    make_repo(&root.join("repoB"));
    let mut store = SqliteStore::open(root.join("g.db")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoA"), Some("va")).unwrap();
    wicked_estate::index_path_as(&mut store, &root.join("repoB"), Some("vb")).unwrap();

    for label in ["va", "vb"] {
        let key = repo_scope::meta_key(Some(label), "id_scheme");
        assert_eq!(
            store.meta_get_key(&key).as_deref(),
            Some(SYMBOL_ID_SCHEME),
            "'{label}' must record its own id scheme under {key}"
        );
    }
    assert!(
        store.meta_get_key("id_scheme").is_none(),
        "a labelled run must not write the SHARED key"
    );
}

/// A fresh DB is already scheme-2: the gate must not fire (nothing previously indexed), and the
/// key is stamped by the end of the first run.
#[test]
fn fresh_db_writes_scheme_without_forcing() {
    let root = fresh_dir("fresh");
    make_repo(&root.join("repo"));
    let mut store = SqliteStore::open(root.join("g.db")).unwrap();
    assert!(store.meta_get_key("id_scheme").is_none());

    let s1 = wicked_estate::index_path(&mut store, &root.join("repo")).unwrap();
    assert_eq!(
        store.meta_get_key("id_scheme").as_deref(),
        Some(SYMBOL_ID_SCHEME),
        "first index of a fresh DB must record the current scheme"
    );

    // A second unchanged index must be a no-op (the gate must not fire on a current DB).
    let s2 = wicked_estate::index_path(&mut store, &root.join("repo")).unwrap();
    assert_eq!(
        s1.node_count, s2.node_count,
        "an unchanged re-index of a scheme-current DB must not churn nodes"
    );
}

/// The store-level collision fix (the one that catches the ON CONFLICT(symbol) merge): four
/// distinct `save` nodes survive storage, and the doc03 false edge — Cache.flush's this.save()
/// "resolving" into Repo.save at 0.65 — is gone (the resolver parks on ambiguity instead).
#[test]
fn store_keeps_same_named_methods_of_different_types_apart() {
    let root = fresh_dir("store_apart");
    make_repo(&root.join("repo"));
    let mut store = SqliteStore::open(root.join("g.db")).unwrap();
    wicked_estate::index_path(&mut store, &root.join("repo")).unwrap();

    let saves = symbols_named(&store, "save");
    let expected: HashSet<String> = [
        "ts-typescript . . . src/fixture/Repo#save().",
        "ts-typescript . . . src/fixture/Cache#save().",
        "ts-typescript . . . src/fixture/Store#save().",
        // lit.save + the method-local arrow merge into the one documented flat residual.
        "ts-typescript . . . src/fixture/save().",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(
        saves, expected,
        "exactly 4 save nodes: 3 type-nested + 1 flat residual"
    );

    // The doc03 false edge must not exist; the refs park as unresolved instead (R7: visible).
    let false_edge = store.all_edges().unwrap().into_iter().any(|e| {
        e.kind == EdgeKind::Calls
            && e.source.as_str() == "ts-typescript . . . src/fixture/Cache#flush()."
            && e.target.as_str() == "ts-typescript . . . src/fixture/Repo#save()."
    });
    assert!(
        !false_edge,
        "Cache#flush() must not carry a 0.65 Calls edge into Repo#save()."
    );
    assert!(
        !store.unresolved_refs_for_name("save").unwrap().is_empty(),
        "the parked save() calls must be visible as unresolved rows, not silently absent"
    );
}

/// BR-2, decisive for the gate predicate: a pre-version DB (nodes + digests, NO indexed_version
/// key, NO id_scheme key — the state `maybe_warn_version_mismatch` calls a "pre-version
/// database") must still fire the gate. Under the rejected `prev_version.is_some()` predicate
/// this test fails: the gate stays silent, the digest-matching file is skipped, and the flat
/// node survives.
#[test]
fn pre_version_db_still_fires_scheme_gate() {
    let root = fresh_dir("pre_version");
    make_repo(&root.join("repo"));

    // Throwaway pass: learn the exact rel path + digest the indexer stores.
    let mut probe = SqliteStore::open(root.join("probe.db")).unwrap();
    wicked_estate::index_path(&mut probe, &root.join("repo")).unwrap();
    let rel = stored_rel_path(&probe);
    let digest = probe
        .file_digest(&rel)
        .unwrap()
        .expect("digest row must exist for the indexed file");
    drop(probe);

    // Build the pre-version-shaped DB via the store API only — never index_path.
    let mut store = SqliteStore::open(root.join("g.db")).unwrap();
    store
        .upsert_nodes(&[flat_v1_node(
            "ts-typescript . . . src/fixture/update().",
            "update",
            &rel,
        )])
        .unwrap();
    store.set_file_digest(&rel, &digest).unwrap();
    assert!(store.meta_get_key("indexed_version").is_none());
    assert!(store.meta_get_key("id_scheme").is_none());

    wicked_estate::index_path(&mut store, &root.join("repo")).unwrap();

    let updates = symbols_named(&store, "update");
    assert!(
        !updates.contains("ts-typescript . . . src/fixture/update()."),
        "pre-version DBs hold the STALEST rows — the gate must fire for them too; got {updates:?}"
    );
    assert!(
        updates.contains("ts-typescript . . . src/fixture/Repo#update()."),
        "the nested id must exist after the forced re-extraction; got {updates:?}"
    );
    assert_eq!(
        store.meta_get_key("id_scheme").as_deref(),
        Some(SYMBOL_ID_SCHEME)
    );
}
