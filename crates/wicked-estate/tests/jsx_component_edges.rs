//! estate#481: a React component rendered only as JSX must have the rendering component as a
//! blast-radius dependent. Before the fix, `<SourceSheet …/>` produced no edge, so BlastRadius on
//! `SourceSheet` answered 0 dependents (wicked-studio's SourceSheet / SourcesPage).

use std::fs;
use std::path::{Path, PathBuf};
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_jsx_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn dependents(dir: &Path, name: &str) -> Vec<String> {
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, dir).unwrap();
    let br = wicked_estate::blast_radius_by_name(&store, name, 12).unwrap();
    let mut names: Vec<String> = br.dependents.into_iter().map(|n| n.name).collect();
    names.sort();
    names
}

#[test]
fn a_component_rendered_as_jsx_has_its_renderer_as_a_dependent() {
    let dir = fresh_dir("tsx");
    fs::write(
        dir.join("src/SourceSheet.tsx"),
        "export function SourceSheet(props: { open: boolean }) { return <aside>{props.open}</aside>; }\n",
    )
    .unwrap();
    // The ESM spelling studio uses: './SourceSheet.js' for a .tsx file.
    fs::write(
        dir.join("src/SourcesPage.tsx"),
        "import { SourceSheet } from './SourceSheet.js';\n\
         export function SourcesPage() { return <section><SourceSheet open={true} /></section>; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Shell.tsx"),
        "import { SourcesPage } from './SourcesPage.js';\n\
         export const Shell = () => <main><SourcesPage></SourcesPage></main>;\n",
    )
    .unwrap();
    let deps = dependents(&dir, "SourceSheet");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        deps.iter().any(|n| n == "SourcesPage"),
        "SourcesPage renders SourceSheet: {deps:?}"
    );
    assert!(
        deps.iter().any(|n| n == "Shell"),
        "Shell renders SourcesPage (transitive): {deps:?}"
    );
}

#[test]
fn a_component_rendered_as_jsx_in_a_jsx_file_has_its_renderer_as_a_dependent() {
    let dir = fresh_dir("jsx");
    fs::write(
        dir.join("src/Card.jsx"),
        "export function Card() { return <div />; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/List.jsx"),
        "import { Card } from './Card';\nexport function List() { return <ul><Card /></ul>; }\n",
    )
    .unwrap();
    let deps = dependents(&dir, "Card");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        deps.iter().any(|n| n == "List"),
        "List renders Card: {deps:?}"
    );
}

#[test]
fn an_intrinsic_tag_does_not_bind_to_a_same_named_function() {
    let dir = fresh_dir("intrinsic");
    fs::write(
        dir.join("src/util.tsx"),
        "export function section() { return 1; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Page.tsx"),
        "export function Page() { return <section />; }\n",
    )
    .unwrap();
    let deps = dependents(&dir, "section");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !deps.iter().any(|n| n == "Page"),
        "<section> is an HTML element, not a call of section(): {deps:?}"
    );
}

#[test]
fn a_member_element_does_not_bind_to_another_modules_same_named_export() {
    // `<Ns.Widget/>` names ns's Widget; its leaf name alone must not make Page a dependent of other's Widget.
    let dir = fresh_dir("member");
    fs::write(
        dir.join("src/ns.tsx"),
        "export function Widget() { return <i />; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/other.tsx"),
        "export function Widget() { return <b />; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Page.tsx"),
        "import * as Ns from './ns';\nimport { Widget } from './other';\nexport function Page() { return <Ns.Widget />; }\nexport const keep = Widget;\n",
    )
    .unwrap();
    let deps = dependents(&dir, "Widget");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !deps.iter().any(|n| n == "Page"),
        "<Ns.Widget/> must not make Page a dependent of a leaf-named Widget: {deps:?}"
    );
}

#[test]
fn underscore_and_dollar_components_are_components() {
    let dir = fresh_dir("underscore");
    fs::write(
        dir.join("src/Row.tsx"),
        "export function _Row() { return <tr />; }\nexport function $Cell() { return <td />; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Table.tsx"),
        "import { _Row, $Cell } from './Row.js';\nexport function Table() { return <table><_Row /><$Cell /></table>; }\n",
    )
    .unwrap();
    let row = dependents(&dir, "_Row");
    let cell = dependents(&dir, "$Cell");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        row.iter().any(|n| n == "Table"),
        "_Row rendered by Table: {row:?}"
    );
    assert!(
        cell.iter().any(|n| n == "Table"),
        "$Cell rendered by Table: {cell:?}"
    );
}
