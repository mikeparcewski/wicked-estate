//! estate#492: local-binding detection follows JS's own scoping order. A `var` redeclaring a
//! parameter IS the parameter, so `function Parent(Card) { { var Card = () => null } return Card() }`
//! keeps main's Parent -> Card edge; a class static block is a `var` scope of its own. Each case
//! runs for calls and JSX and asserts the exact edge set; the cases marked "as on main" pass
//! unchanged on main.

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

/// Index `caller.tsx` beside `other.tsx` (an unrelated global `Card`); return the sorted
/// "file::callee" Calls edges out of each named caller.
fn edges(tag: &str, caller: &str, callers: &[&str]) -> Vec<(String, Vec<String>)> {
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
    callers
        .iter()
        .map(|caller| {
            assert!(
                nodes.values().any(|n| n.name == *caller),
                "{tag}: the caller {caller} must exist"
            );
            let mut to: Vec<String> = all
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls)
                .filter_map(|e| {
                    let from = nodes.get(&e.source)?;
                    let to = nodes.get(&e.target)?;
                    (from.name == *caller).then(|| format!("{}::{}", to.location.file, to.name))
                })
                .collect();
            to.sort();
            to.dedup();
            (caller.to_string(), to)
        })
        .collect()
}

fn assert_all(tag: &str, got: Vec<(String, Vec<String>)>, want: &[&str]) {
    let want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
    for (caller, to) in got {
        assert_eq!(to, want, "{tag}: {caller}");
    }
}

/// As on main: a block `var` redeclaring the parameter is the parameter's binding, and its
/// arrow is the definition the call reaches.
#[test]
fn a_block_var_redeclaring_a_parameter_binds_its_definition_as_on_main() {
    let got = edges(
        "param_var",
        "export function ViaCall(Card: any) { { var Card = () => null; } return Card(); }\n\
         export function ViaJsx(Card: any) { { var Card = () => null; } return <Card />; }\n",
        &["ViaCall", "ViaJsx"],
    );
    assert_all("param_var", got, &["src/caller.tsx::Card"]);
}

/// As on main: a block `var` and a body-level function declaration are one binding.
#[test]
fn a_block_var_sharing_a_body_function_binds_the_function_as_on_main() {
    let got = edges(
        "var_function",
        "export function ViaCall() { { var Card: any; } function Card() { return null; } return Card(); }\n\
         export function ViaJsx() { { var Card: any; } function Card() { return null; } return <Card />; }\n",
        &["ViaCall", "ViaJsx"],
    );
    assert_all("var_function", got, &["src/caller.tsx::Card"]);
}

/// As on main: a static block's `var` arrow is the definition its call reaches.
#[test]
fn a_static_block_var_arrow_binds_its_definition_as_on_main() {
    let got = edges(
        "static_arrow",
        "declare const on: boolean;\n\
         export class ViaCall { static { if (on) { var Card = () => null; } Card(); } }\n\
         export class ViaJsx { static { if (on) { var Card = () => null; } void (<Card />); } }\n",
        &["ViaCall", "ViaJsx"],
    );
    assert_all("static_arrow", got, &["src/caller.tsx::Card"]);
}

/// The fix (main binds other.tsx's `Card` here): a static block's `var` value is local to the
/// block, so its call binds no global.
#[test]
fn a_static_block_var_value_binds_no_global() {
    let got = edges(
        "static_value",
        "declare const on: boolean;\n\
         declare function make(): any;\n\
         export class ViaCall { static { if (on) { var Card = make(); } Card(); } }\n\
         export class ViaJsx { static { if (on) { var Card = make(); } void (<Card />); } }\n",
        &["ViaCall", "ViaJsx"],
    );
    for (caller, to) in got {
        assert!(
            !to.iter().any(|t| t.ends_with("::Card")),
            "static_value: {caller}: {to:?}"
        );
    }
}
