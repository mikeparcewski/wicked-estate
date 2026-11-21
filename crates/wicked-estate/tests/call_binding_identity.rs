//! estate#486 (local-binding half): a call — `Card()` or `<Card />` — whose callee is a locally
//! bound name (a parameter, a destructured prop, a local `const`) never binds an unrelated global
//! `Card` in another file; a local definition binds exactly itself. Before the fix the caller was a
//! blast-radius dependent of `other.tsx::Card`. Aliased imports are unchanged (see
//! `alias_matches_main.rs`).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_bind_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

/// What the index holds: every Calls edge as (caller, "file::callee"), every node name, and how
/// many refs are parked under each written name.
struct Graph {
    calls: Vec<(String, String)>,
    names: Vec<String>,
    store: SqliteStore,
}

impl Graph {
    fn of(store: SqliteStore) -> Self {
        let nodes: HashMap<_, _> = store
            .all_nodes()
            .unwrap()
            .into_iter()
            .map(|n| (n.symbol.clone(), n))
            .collect();
        let mut calls: Vec<(String, String)> = store
            .all_edges()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .filter_map(|e| {
                let from = nodes.get(&e.source)?;
                let to = nodes.get(&e.target)?;
                Some((
                    from.name.clone(),
                    format!("{}::{}", to.location.file, to.name),
                ))
            })
            .collect();
        calls.sort();
        let names = nodes.values().map(|n| n.name.clone()).collect();
        Graph {
            calls,
            names,
            store,
        }
    }

    fn calls_from(&self, caller: &str) -> Vec<&str> {
        assert!(
            self.names.iter().any(|n| n == caller),
            "the caller {caller} must exist"
        );
        self.calls
            .iter()
            .filter(|(from, _)| from == caller)
            .map(|(_, to)| to.as_str())
            .collect()
    }

    fn parked(&self, raw_name: &str) -> usize {
        self.store.unresolved_refs_for_name(raw_name).unwrap().len()
    }
}

/// One project: `a.tsx` exports `Actual`, `other.tsx` an unrelated `Card`, plus the caller file.
fn project(tag: &str, caller: &str) -> Graph {
    project_with(tag, caller, &[])
}

fn project_with(tag: &str, caller: &str, extra: &[(&str, &str)]) -> Graph {
    let dir = fresh_dir(tag);
    for (path, text) in extra {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::write(
        dir.join("src/a.tsx"),
        "export function Actual() { return null; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/other.tsx"),
        "export function Card() { return null; }\n",
    )
    .unwrap();
    fs::write(dir.join("src/caller.tsx"), caller).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    Graph::of(store)
}

const OTHER_CARD: &str = "src/other.tsx::Card";

#[test]
fn a_locally_bound_name_never_binds_a_global_for_calls_and_jsx() {
    let cases = [
        (
            "prop",
            "export function ViaCall({ Card }: { Card: () => null }) { return Card(); }\n\
             export function ViaJsx({ Card }: { Card: () => null }) { return <Card />; }\n",
        ),
        (
            "renamed_prop",
            "export function ViaCall({ c: Card }: any) { return Card(); }\n\
             export function ViaJsx({ c: Card }: any) { return <Card />; }\n",
        ),
        (
            "default_prop",
            "const Fallback = () => null;\n\
             export function ViaCall({ Card = Fallback }: any) { return Card(); }\n\
             export function ViaJsx({ Card = Fallback }: any) { return <Card />; }\n",
        ),
        (
            "param",
            "export function ViaCall(Card: () => null) { return Card(); }\n\
             export function ViaJsx(Card: () => null) { return <Card />; }\n",
        ),
        (
            "arrow_param",
            "export const ViaCall = (Card: any) => Card();\n\
             export const ViaJsx = (Card: any) => <Card />;\n",
        ),
        (
            "local_const",
            "declare function pick(): () => null;\n\
             export function ViaCall() { const Card = pick(); return Card(); }\n\
             export function ViaJsx() { const Card = pick(); return <Card />; }\n",
        ),
        (
            "callback_param",
            "export function ViaCall(xs: any[]) { return xs.map((Card) => Card()); }\n\
             export function ViaJsx(xs: any[]) { return xs.map((Card) => <Card />); }\n",
        ),
        (
            "hoisted_var",
            "export function ViaCall(s: any) { if (s) { var Card = s; } return Card(); }\n\
             export function ViaJsx(s: any) { if (s) { var Card = s; } return <Card />; }\n",
        ),
        (
            "for_var",
            "export function ViaCall(xs: any[]) { for (var Card of xs) {} return Card(); }\n\
             export function ViaJsx(xs: any[]) { for (var Card of xs) {} return <Card />; }\n",
        ),
        (
            // A definition inside the parameter list's default is not the parameter's binding.
            "param_default_def",
            "export function ViaCall(Card: any, u = () => { function Card() { return 1; } }) { return Card(); }\n\
             export function ViaJsx(Card: any, u = () => { function Card() { return 1; } }) { return <Card />; }\n",
        ),
        (
            // A same-named function hidden in another scope of the caller is not the binding.
            "hidden_nested",
            "export function ViaCall(Card: any) { function u() { function Card() { return 1; } } return Card(); }\n\
             export function ViaJsx(Card: any) { function u() { function Card() { return 1; } } return <Card />; }\n",
        ),
    ];
    for (tag, caller) in cases {
        let g = project(tag, caller);
        for name in ["ViaCall", "ViaJsx"] {
            let to = g.calls_from(name);
            assert!(
                !to.iter().any(|t| t.ends_with("::Card")),
                "{tag}: {name}'s Card is a local value binding, bound to no Card: {:?}",
                g.calls
            );
        }
        assert_eq!(
            g.parked("local-binding:Card"),
            2,
            "{tag}: both calls are extracted and parked as local bindings"
        );
    }
}

#[test]
fn a_local_definition_binds_exactly_itself() {
    // Nested, captured from the enclosing function, and shadowing an aliased import.
    let g = project(
        "local_defs",
        "import { Actual as Card } from './a';\n\
         export function Outer() { function Card() { return null; } return Card(); }\n\
         export function Closure() {\n\
           const Card = () => null;\n\
           function Page() { return <Card />; }\n\
           return Page();\n\
         }\n\
         export function Shared() { var Card; function Card() { return null; } return Card(); }\n",
    );
    for caller in ["Outer", "Page", "Shared"] {
        let to = g.calls_from(caller);
        assert!(
            to.contains(&"src/caller.tsx::Card"),
            "{caller} calls its local Card: {:?}",
            g.calls
        );
        assert!(!to.contains(&OTHER_CARD), "{caller}: {:?}", g.calls);
        assert!(
            !to.contains(&"src/a.tsx::Actual"),
            "{caller}: {:?}",
            g.calls
        );
    }
}

#[test]
fn true_edges_are_kept_for_imports_and_top_level_names() {
    // A plain (non-aliased) import still binds its module's export.
    let g = project(
        "plain_import",
        "import { Card } from './other';\n\
         export function ViaCall() { return Card(); }\n\
         export function ViaJsx() { return <Card />; }\n",
    );
    for caller in ["ViaCall", "ViaJsx"] {
        assert!(
            g.calls_from(caller).contains(&OTHER_CARD),
            "{caller} imports other.tsx's Card: {:?}",
            g.calls
        );
    }
    // A same-file top-level component is a module binding, not a local one.
    let g = project(
        "top_level",
        "function Local() { return null; }\n\
         export function ViaCall() { return Local(); }\n\
         export function ViaJsx() { return <Local />; }\n",
    );
    for caller in ["ViaCall", "ViaJsx"] {
        assert!(
            g.calls_from(caller).contains(&"src/caller.tsx::Local"),
            "{caller} renders the top-level Local: {:?}",
            g.calls
        );
    }
    // A property KEY is not a binding: `{ Card: c }` binds `c`, so `Card()` is still the import.
    let g = project(
        "key_not_binding",
        "import { Card } from './other';\n\
         export function ViaCall({ Card: c }: any) { c; return Card(); }\n",
    );
    assert!(
        g.calls_from("ViaCall").contains(&OTHER_CARD),
        "a property key binds nothing: {:?}",
        g.calls
    );
}

/// A parked ref keeps no hints, and an incremental index re-resolves it by name when a new symbol
/// of that name appears: a local binding's identity must survive in the name itself.
#[test]
fn a_back_fill_never_binds_a_local_binding_to_a_new_homonym() {
    let dir = fresh_dir("backfill");
    let db = dir.join("g.db");
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(
        repo.join("src/caller.tsx"),
        "export function Holder({ Card: Slot }: any) { return <Slot />; }\n\
         export function Prop({ Card }: any) { return Card(); }\n",
    )
    .unwrap();
    fs::write(
        repo.join("src/other.tsx"),
        "export function Unrelated() { return null; }\n",
    )
    .unwrap();
    {
        let mut store = SqliteStore::open(&db).unwrap();
        wicked_estate::index_path(&mut store, &repo).unwrap();
    }
    // Only other.tsx changes: it now defines a `Card` (and a `Slot`).
    fs::write(
        repo.join("src/other.tsx"),
        "export function Card() { return null; }\nexport function Slot() { return null; }\n",
    )
    .unwrap();
    let mut store = SqliteStore::open(&db).unwrap();
    wicked_estate::index_path(&mut store, &repo).unwrap();
    let g = Graph::of(store);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        g.parked("local-binding:Card"),
        1,
        "Prop's call stays parked"
    );
    assert_eq!(
        g.parked("local-binding:Slot"),
        1,
        "Holder's call stays parked"
    );
    for caller in ["Holder", "Prop"] {
        let to = g.calls_from(caller);
        assert!(
            !to.contains(&OTHER_CARD) && !to.contains(&"src/other.tsx::Slot"),
            "{caller} must not gain a back-filled edge to a new homonym: {:?}",
            g.calls
        );
    }
}

#[test]
fn java_overloads_keep_body_evidence_and_bind_every_member() {
    let dir = fresh_dir("java_overloads");
    fs::write(
        dir.join("src/I.java"),
        "interface I { default void m(int x) {} void m(); }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Caller.java"),
        "class Caller { void run(I i) { i.m(1); } }\n",
    )
    .unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let _ = fs::remove_dir_all(&dir);

    let nodes = store.all_nodes().unwrap();
    let mut overloads: Vec<_> = nodes.iter().filter(|n| n.name == "m").collect();
    overloads.sort_by_key(|n| n.location.span.start_byte);
    assert_eq!(overloads.len(), 2, "both Java overloads survive the store");
    assert!(overloads[0].symbol.as_str().ends_with("/I#m()."));
    assert!(overloads[1].symbol.as_str().ends_with("/I#m(+1)."));
    assert_eq!(overloads[0].metadata["has_body"], serde_json::json!(true));
    assert_eq!(overloads[1].metadata["has_body"], serde_json::json!(false));

    let caller = nodes.iter().find(|n| n.name == "run").unwrap();
    let calls: Vec<_> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Calls && e.source == caller.symbol)
        .collect();
    assert_eq!(calls.len(), 2, "a call binds the whole overload family");
    for overload in overloads {
        let edge = calls.iter().find(|e| e.target == overload.symbol).unwrap();
        assert_eq!(edge.metadata["overloads"], serde_json::json!(2));
    }
    assert!(store.unresolved_refs_for_name("m").unwrap().is_empty());
}

#[test]
fn typescript_merge_evidence_survives_the_store_winner() {
    for (tag, source, kinds) in [
        (
            "class_first",
            "class X { m() {} }\ninterface X { m(): void }\n",
            serde_json::json!(["class", "interface"]),
        ),
        (
            "interface_first",
            "interface X { m(): void }\nclass X { m() {} }\n",
            serde_json::json!(["interface", "class"]),
        ),
    ] {
        let dir = fresh_dir(tag);
        fs::write(dir.join("src/merge.ts"), source).unwrap();
        let mut store = SqliteStore::in_memory().unwrap();
        wicked_estate::index_path(&mut store, &dir).unwrap();
        let _ = fs::remove_dir_all(&dir);

        let nodes = store.all_nodes().unwrap();
        let merged = nodes.iter().find(|n| n.name == "X").unwrap();
        assert_eq!(merged.metadata["merged_kinds"], kinds, "{tag}");
        let method = nodes.iter().find(|n| n.name == "m").unwrap();
        assert_eq!(
            method.metadata["has_body"],
            serde_json::json!(true),
            "{tag}"
        );
        assert!(!method.metadata.contains_key("merged_kinds"), "{tag}");
    }
}
