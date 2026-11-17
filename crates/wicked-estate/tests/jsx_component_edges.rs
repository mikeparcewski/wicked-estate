//! estate#481: a React component rendered only as JSX must have the rendering component as a
//! blast-radius dependent. Before the fix, `<SourceSheet …/>` produced no edge, so BlastRadius on
//! `SourceSheet` answered 0 dependents (wicked-studio's SourceSheet / SourcesPage).

use std::fs;
use std::path::PathBuf;
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_jsx_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn dependents(dir: &PathBuf, name: &str) -> Vec<String> {
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
    assert!(deps.iter().any(|n| n == "SourcesPage"), "SourcesPage renders SourceSheet: {deps:?}");
    assert!(deps.iter().any(|n| n == "Shell"), "Shell renders SourcesPage (transitive): {deps:?}");
}

#[test]
fn a_component_rendered_as_jsx_in_a_jsx_file_has_its_renderer_as_a_dependent() {
    let dir = fresh_dir("jsx");
    fs::write(dir.join("src/Card.jsx"), "export function Card() { return <div />; }\n").unwrap();
    fs::write(
        dir.join("src/List.jsx"),
        "import { Card } from './Card';\nexport function List() { return <ul><Card /></ul>; }\n",
    )
    .unwrap();
    let deps = dependents(&dir, "Card");
    let _ = fs::remove_dir_all(&dir);
    assert!(deps.iter().any(|n| n == "List"), "List renders Card: {deps:?}");
}

#[test]
fn an_intrinsic_tag_does_not_bind_to_a_same_named_function() {
    let dir = fresh_dir("intrinsic");
    fs::write(dir.join("src/util.tsx"), "export function section() { return 1; }\n").unwrap();
    fs::write(
        dir.join("src/Page.tsx"),
        "export function Page() { return <section />; }\n",
    )
    .unwrap();
    let deps = dependents(&dir, "section");
    let _ = fs::remove_dir_all(&dir);
    assert!(!deps.iter().any(|n| n == "Page"), "<section> is an HTML element, not a call of section(): {deps:?}");
}
