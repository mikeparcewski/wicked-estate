//! estate#486: import aliases bind only within their imported module, for calls and JSX.
//! Keep the two reviewer repros of the withdrawn rename-to-original fix: an external-package
//! alias must stay parked even after back-fill, and an export-side alias must keep its real target.

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
    let store = index(&db, &repo);
    for caller in CALLERS {
        assert_eq!(calls_from(&store, caller), Vec::<String>::new(), "{caller}");
    }
    drop(store);
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
        store
            .unresolved_refs_for_name("import-alias:Card:Actual")
            .unwrap()
            .len(),
        2,
        "both calls stay parked under the encoded alias name"
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

/// The alias must bind the imported original, never a global homonym of its local name.
#[test]
fn a_plain_alias_resolves_to_its_imported_original() {
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
            vec!["src/a.tsx::Actual".to_string()],
            "{caller}"
        );
    }
}

#[test]
fn a_python_from_import_alias_resolves_to_its_imported_original() {
    let dir = fresh_dir("python_alias");
    let (db, repo) = (dir.join("g.db"), dir.join("repo"));
    write(
        &repo,
        &[
            ("src/a.py", "def actual():\n    return None\n"),
            ("src/other.py", "def card():\n    return None\n"),
            (
                "src/caller.py",
                "from a import actual as card\ndef via_call():\n    return card()\n",
            ),
        ],
    );
    let store = index(&db, &repo);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        calls_from(&store, "via_call"),
        vec!["src/a.py::actual".to_string()]
    );
}

#[test]
fn a_parameter_shadowing_an_import_alias_stays_local() {
    let dir = fresh_dir("shadow_alias");
    let (db, repo) = (dir.join("g.db"), dir.join("repo"));
    write(
        &repo,
        &[
            ("src/a.tsx", "export function Actual() { return null; }\n"),
            ("src/other.tsx", "export function Card() { return null; }\n"),
            (
                "src/caller.tsx",
                "import { Actual as Card } from './a';\n\
                 export function ViaCall(Card) { return Card(); }\n\
                 export function ViaJsx({ Card }) { return <Card />; }\n\
                 export function Unshadowed() { return Card(); }\n",
            ),
        ],
    );
    let store = index(&db, &repo);
    let _ = fs::remove_dir_all(&dir);
    for caller in CALLERS {
        assert_eq!(calls_from(&store, caller), Vec::<String>::new(), "{caller}");
    }
    assert_eq!(
        calls_from(&store, "Unshadowed"),
        vec!["src/a.tsx::Actual".to_string()]
    );
    assert_eq!(
        store
            .unresolved_refs_for_name("local-binding:Card")
            .unwrap()
            .len(),
        2
    );
}
