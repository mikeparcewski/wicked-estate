//! estate#486 descope guard: an aliased import (`import { Actual as Card }`) resolves EXACTLY as on
//! main, for calls and JSX. This file passes unchanged on main; it pins main's edges on the same
//! inputs, including the two reviewer repros of the withdrawn alias renaming (an external-package
//! alias plus a back-filled homonym; an export-side alias). The third case pins main's known false
//! edge, #486's alias half, which stays open; fixing it will change that expectation deliberately.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use wicked_estate_core::{EdgeKind, GraphRead};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_alias_main_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("repo/src")).unwrap();
    d
}

/// The Calls edges out of `caller`, as sorted "file::callee" strings.
fn calls_from(store: &SqliteStore, caller: &str) -> Vec<String> {
    let nodes: HashMap<_, _> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol.clone(), n))
        .collect();
    assert!(
        nodes.values().any(|n| n.name == caller),
        "the caller {caller} must exist"
    );
    let mut out: Vec<String> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .filter_map(|e| {
            let from = nodes.get(&e.source)?;
            let to = nodes.get(&e.target)?;
            (from.name == caller).then(|| format!("{}::{}", to.location.file, to.name))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn write(repo: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        fs::write(repo.join(path), text).unwrap();
    }
}

fn index(db: &Path, repo: &Path) -> SqliteStore {
    let mut store = SqliteStore::open(db).unwrap();
    wicked_estate::index_path(&mut store, repo).unwrap();
    store
}

const CALLERS: [&str; 2] = ["ViaCall", "ViaJsx"];

/// Reviewer repro 1: an unchanged caller aliases an external package's `Actual` and defines its
/// own `Actual`; a later run adds another file's `Actual`. Main binds neither call to any `Actual`,
/// before or after the back-fill.
#[test]
fn an_external_alias_gains_no_edge_when_a_homonym_of_its_export_appears() {
    let dir = fresh_dir("external_backfill");
    let (db, repo) = (dir.join("g.db"), dir.join("repo"));
    write(
        &repo,
        &[
            (
                "src/caller.tsx",
                "import { Actual as Card } from 'some-ui-lib';\n\
                 export function Actual() { return null; }\n\
                 export function ViaCall() { return Card(); }\n\
                 export function ViaJsx() { return <Card />; }\n",
            ),
            (
                "src/other.tsx",
                "export function Unrelated() { return null; }\n",
            ),
        ],
    );
    let _ = index(&db, &repo);
    write(
        &repo,
        &[(
            "src/other.tsx",
            "export function Actual() { return null; }\n",
        )],
    );
    let store = index(&db, &repo);
    let _ = fs::remove_dir_all(&dir);
    for caller in CALLERS {
        assert_eq!(calls_from(&store, caller), Vec::<String>::new(), "{caller}");
    }
    assert_eq!(
        store.unresolved_refs_for_name("Card").unwrap().len(),
        2,
        "both calls stay parked under the local name"
    );
}

/// Reviewer repro 2: an export-side alias meets an import-side alias. Main binds both calls to the
/// real `Card`.
#[test]
fn an_import_alias_of_an_export_alias_keeps_the_real_definition() {
    let dir = fresh_dir("export_alias");
    let (db, repo) = (dir.join("g.db"), dir.join("repo"));
    write(
        &repo,
        &[
            (
                "src/a.tsx",
                "function Card() { return null; }\nexport { Card as Actual };\n",
            ),
            (
                "src/caller.tsx",
                "import { Actual as Card } from './a';\n\
                 export function ViaCall() { return Card(); }\n\
                 export function ViaJsx() { return <Card />; }\n",
            ),
        ],
    );
    let store = index(&db, &repo);
    let _ = fs::remove_dir_all(&dir);
    for caller in CALLERS {
        assert_eq!(
            calls_from(&store, caller),
            vec!["src/a.tsx::Card".to_string()],
            "{caller}"
        );
    }
}

/// Main's behaviour for a plain alias with an unrelated homonym of the LOCAL name: it binds that
/// homonym (#486's alias half, open, W10). Pinned so the descope provably leaves it unchanged.
#[test]
fn a_plain_alias_resolves_by_its_local_name_as_on_main() {
    let dir = fresh_dir("plain_alias");
    let (db, repo) = (dir.join("g.db"), dir.join("repo"));
    write(
        &repo,
        &[
            ("src/a.tsx", "export function Actual() { return null; }\n"),
            ("src/other.tsx", "export function Card() { return null; }\n"),
            (
                "src/caller.tsx",
                "import { Actual as Card } from './a';\n\
                 export function ViaCall() { return Card(); }\n\
                 export function ViaJsx() { return <Card />; }\n",
            ),
        ],
    );
    let store = index(&db, &repo);
    let _ = fs::remove_dir_all(&dir);
    for caller in CALLERS {
        assert_eq!(
            calls_from(&store, caller),
            vec!["src/other.tsx::Card".to_string()],
            "{caller}"
        );
    }
}
