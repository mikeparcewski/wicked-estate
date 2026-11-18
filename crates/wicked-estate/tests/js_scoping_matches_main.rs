//! estate#492: local-binding detection follows JS scoping where #488 cut it short. A `var`
//! redeclaring a parameter IS the parameter, so the last `var` assignment before the call is the
//! value it sees; a class static block is a `var` scope of its own. Each case runs for calls and
//! JSX against an unrelated global `Card` in `other.tsx`, and asserts the exact targets.
//!
//! Same input, three builds. [`BUILD`] names the build this copy asserts; the two baselines are
//! this file with only that line changed, run by CI on throwaway branches of those commits:
//! | case            | before #488 (`Pre488`) | main after #488 (`Main`) | this change (`Head`) |
//! |-----------------|------------------------|--------------------------|----------------------|
//! | param_var       | local arrow            | none (the bug)           | local arrow          |
//! | param_var_after | local arrow            | none                     | none                 |
//! | var_function    | local fn               | local fn                 | local fn             |
//! | static_arrow    | none (a tie)           | none (a tie)             | each class's arrow   |
//! | static_value    | other.tsx              | other.tsx                | none (parked)        |

#[allow(dead_code)]
#[derive(PartialEq)]
enum Build {
    Pre488,
    Main,
    Head,
}

const BUILD: Build = Build::Head;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_js_scope_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

struct Graph {
    /// Per caller: the sorted target symbol ids of its Calls edges.
    calls: HashMap<String, Vec<String>>,
    parked_local: usize,
}

/// Index `caller.tsx` beside `other.tsx` (an unrelated global `Card`).
fn index(tag: &str, caller: &str, callers: &[&str]) -> Graph {
    let dir = fresh_dir(tag);
    fs::write(
        dir.join("src/other.tsx"),
        "export function Card() { return null; }\n",
    )
    .unwrap();
    fs::write(dir.join("src/caller.tsx"), caller).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    let nodes: HashMap<_, _> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol.clone(), n))
        .collect();
    let all = store.all_edges().unwrap();
    let calls = callers
        .iter()
        .map(|caller| {
            assert!(
                nodes.values().any(|n| n.name == *caller),
                "{tag}: the caller {caller} must exist"
            );
            let mut to: Vec<String> = all
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls)
                .filter(|e| nodes.get(&e.source).is_some_and(|n| n.name == *caller))
                .map(|e| e.target.as_str().to_string())
                .collect();
            to.sort();
            to.dedup();
            (caller.to_string(), to)
        })
        .collect();
    let parked_local = store
        .unresolved_refs_for_name("local-binding:Card")
        .unwrap()
        .len();
    Graph {
        calls,
        parked_local,
    }
}

/// Exactly one target, in `file`, whose symbol ends with `suffix`.
fn assert_one(tag: &str, g: &Graph, caller: &str, file: &str, suffix: &str) {
    let to = &g.calls[caller];
    assert!(
        to.len() == 1 && to[0].contains(file) && to[0].ends_with(suffix),
        "{tag}: {caller} -> {to:?}, want one {file} target ending {suffix}"
    );
}

fn assert_none(tag: &str, g: &Graph, caller: &str) {
    assert!(
        g.calls[caller].is_empty(),
        "{tag}: {caller} -> {:?}",
        g.calls[caller]
    );
}

const CALLERS: [&str; 2] = ["ViaCall", "ViaJsx"];

/// A block `var` redeclaring a parameter, assigned before the call: the call reaches its arrow.
#[test]
fn param_var() {
    let g = index(
        "param_var",
        "export function ViaCall(Card: any) { { var Card = () => null; } return Card(); }\n\
         export function ViaJsx(Card: any) { { var Card = () => null; } return <Card />; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Main => assert_none("param_var", &g, c),
            _ => assert_one("param_var", &g, c, "src/caller", "/Card()."),
        }
    }
}

/// The same `var` assigned AFTER the call: the call sees the supplied parameter.
#[test]
fn param_var_after() {
    let g = index(
        "param_var_after",
        "export function ViaCall(Card: any) { const r = Card(); { var Card = () => null; } return r; }\n\
         export function ViaJsx(Card: any) { const r = <Card />; { var Card = () => null; } return r; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Pre488 => assert_one("param_var_after", &g, c, "src/caller", "/Card()."),
            _ => assert_none("param_var_after", &g, c),
        }
    }
}

/// A block `var` and a body function declaration are one binding: the function.
#[test]
fn var_function() {
    let g = index(
        "var_function",
        "export function ViaCall() { { var Card: any; } function Card() { return null; } return Card(); }\n\
         export function ViaJsx() { { var Card: any; } function Card() { return null; } return <Card />; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_one("var_function", &g, c, "src/caller", "/Card().");
    }
}

/// A static block's nested `var` arrow: each class's call reaches its own arrow.
#[test]
fn static_arrow() {
    let g = index(
        "static_arrow",
        "declare const on: boolean;\n\
         export class ViaCall { static { if (on) { var Card = () => null; } Card(); } }\n\
         export class ViaJsx { static { if (on) { var Card = () => null; } void (<Card />); } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Head => assert_one(
                "static_arrow",
                &g,
                c,
                "src/caller",
                &format!("/{c}#Card()."),
            ),
            _ => assert_none("static_arrow", &g, c),
        }
    }
}

/// A static block's nested `var` value: local to the block, so no global `Card`; both calls park.
#[test]
fn static_value() {
    let g = index(
        "static_value",
        "declare const on: boolean;\n\
         declare function make(): any;\n\
         export class ViaCall { static { if (on) { var Card = make(); } Card(); } }\n\
         export class ViaJsx { static { if (on) { var Card = make(); } void (<Card />); } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Head => assert_none("static_value", &g, c),
            _ => assert_one("static_value", &g, c, "src/other", "/Card()."),
        }
    }
    if BUILD == Build::Head {
        assert_eq!(
            g.parked_local, 2,
            "static_value: both calls park as local bindings"
        );
    }
}
