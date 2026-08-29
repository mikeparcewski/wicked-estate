//! Lane relative-imports: end-to-end pins for the RelativeImportResolver + the blast-radius
//! contains-aware transit rule (docs/recon/relative-imports.md S4/S6).

use std::fs;
use std::path::PathBuf;
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_relimp_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

/// The mandated blast-radius regression (brief property (g)): the blast radius of a function in
/// an imported file must not change SIZE OR MEMBERSHIP when only import edges are added. Two
/// identical repos, one with the `import` line and one without: `f`'s dependents are
/// {g, File a.ts, File b.ts} either way — the caller, and both contains-holding files (exact
/// pre-File→File-edge parity, Decision G/FEAS-1).
#[test]
fn blast_radius_size_unchanged_for_function_in_imported_file() {
    let deps_with = |import_line: bool, tag: &str| -> std::collections::BTreeSet<String> {
        let dir = fresh_dir(tag);
        let a_body = if import_line {
            "import { f } from './b';\nexport function g() { return f(); }\n"
        } else {
            "export function g() { return f(); }\n"
        };
        fs::write(dir.join("src/a.ts"), a_body).unwrap();
        fs::write(dir.join("src/b.ts"), "export function f() { return 1; }\n").unwrap();

        let mut store = SqliteStore::in_memory().unwrap();
        wicked_estate::index_path(&mut store, &dir).unwrap();
        let deps = wicked_estate::blast_radius_by_name(&store, "f", 12).unwrap();
        let _ = fs::remove_dir_all(&dir);
        deps.into_iter().map(|n| n.symbol.0).collect()
    };

    let with_import = deps_with(true, "br_with");
    let without_import = deps_with(false, "br_without");
    assert_eq!(
        with_import.len(),
        without_import.len(),
        "dependent COUNT must not change when only import edges are added:\nwith:    {with_import:?}\nwithout: {without_import:?}"
    );
    assert_eq!(
        with_import, without_import,
        "dependent SET must not change when only import edges are added"
    );
}

/// End-to-end resolver wiring (S6): a temp fixture = the review's edge-corpus layout UNION
/// edge-corpus2's ./c + ./foo2 cases PLUS a dynamic import() and a TS import=require (the
/// read-only corpora contain no such sites — FEAS-2). Expected: 14 binds / 3 parks.
#[test]
fn relative_imports_bind_file_to_file() {
    use wicked_estate_core::{EdgeKind, GraphRead};

    let dir = fresh_dir("bind_e2e");
    let w = |rel: &str, body: &str| {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    };
    w(
        "src/main.ts",
        concat!(
            "import { T } from './foo.d.ts';\n",    // 1  bind src/foo.d.ts
            "import { u } from './utils/index';\n", // 2  bind src/utils/index.ts
            "import { i } from './index';\n",       // 3  bind src/index.ts
            "import { a } from './a';\n",           // 4  bind src/a.ts (over a/index.ts)
            "import './styles.css';\n",             // 5  bind src/styles.css (literal)
            "import data from './data.json';\n",    // 6  bind src/data.json (literal)
            "export * from './y';\n",               // 7  bind src/y.ts (export-from)
            "const z = require('./z');\n",          // 8  bind src/z.js (require)
            "import { w } from './w';\n",           // 9  bind src/w.ts
            "import { q } from './q.js';\n",        // 10 bind src/q.ts (remap)
            "import { b } from './b';\n",           // 11 bind src/b.ts (over b.css)
            "import { c } from './c';\n",           // 12 bind src/c/index.ts (dir index)
            "const dyn = import('./dyn');\n",       // 13 bind src/dyn.ts (dynamic import)
            "import req = require('./req');\n",     // 14 bind src/req.ts (import=require)
            "import { foo2 } from './foo2';\n",     // PARK: only site/src/foo2.ts exists
        ),
    );
    w(
        "src/deep/nested/esc.ts",
        "import { x } from '../../../../escape/x';\nimport { v } from '../../../../../vv';\n", // 2 PARKs
    );
    for (rel, body) in [
        ("src/w.ts", "export const w = 1;\n"),
        ("src/q.ts", "export const q = 1;\n"),
        ("src/y.ts", "export const y = 1;\n"),
        ("src/index.ts", "export const i = 1;\n"),
        ("src/a.ts", "export const a = 1;\n"),
        ("src/a/index.ts", "export const a2 = 1;\n"),
        ("src/b.ts", "export const b = 1;\n"),
        ("src/b.css", ".b{}\n"),
        ("src/styles.css", ".x{color:red}\n"),
        ("src/data.json", "{\"k\":1}\n"),
        ("src/foo.d.ts", "export type T = number;\n"),
        ("src/utils/index.ts", "export const u = 1;\n"),
        ("src/c/index.ts", "export const c = 1;\n"),
        ("src/z.js", "module.exports = {z:1};\n"),
        ("src/dyn.ts", "export const dyn = 1;\n"),
        ("src/req.ts", "export const req = 1;\n"),
        ("site/src/foo2.ts", "export const foo2 = 1;\n"),
        ("escape/x.ts", "export const x = 1;\n"),
    ] {
        w(rel, body);
    }

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    // Collect the relative-import File→File edges as (source path, target path).
    let mut bound: Vec<(String, String)> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.resolved_by == "relative-import")
        .map(|e| {
            let src = store.get_node(&e.source).unwrap().expect("source node");
            let tgt = store.get_node(&e.target).unwrap().expect("target node");
            assert!(
                matches!(tgt.kind, wicked_estate_core::NodeKind::File),
                "target must be a File node: {:?}",
                tgt.kind
            );
            assert!((e.confidence.get() - 0.9).abs() < 1e-6, "0.9 override");
            (src.location.file, tgt.location.file)
        })
        .collect();
    bound.sort();

    let mut expected: Vec<(String, String)> = [
        ("src/main.ts", "src/foo.d.ts"),
        ("src/main.ts", "src/utils/index.ts"),
        ("src/main.ts", "src/index.ts"),
        ("src/main.ts", "src/a.ts"),
        ("src/main.ts", "src/styles.css"),
        ("src/main.ts", "src/data.json"),
        ("src/main.ts", "src/y.ts"),
        ("src/main.ts", "src/z.js"),
        ("src/main.ts", "src/w.ts"),
        ("src/main.ts", "src/q.ts"),
        ("src/main.ts", "src/b.ts"),
        ("src/main.ts", "src/c/index.ts"),
        ("src/main.ts", "src/dyn.ts"),
        ("src/main.ts", "src/req.ts"),
    ]
    .iter()
    .map(|(s, t)| (s.to_string(), t.to_string()))
    .collect();
    expected.sort();

    assert_eq!(
        bound, expected,
        "exactly the 14 expected binds — no suffix/root-escape false-binds, no parks among them"
    );

    // The 3 parks: unresolved rows exist, and no relative-import edge involves their targets.
    for spec in ["'./foo2'", "'../../../../escape/x'", "'../../../../../vv'"] {
        let rows = store.unresolved_refs_for_name(spec).unwrap();
        assert!(
            !rows.is_empty(),
            "{spec} must be PARKED (unresolved row present)"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}
