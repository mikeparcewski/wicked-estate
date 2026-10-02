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
        let br = wicked_estate::blast_radius_by_name(&store, "f", 12).unwrap();
        let _ = fs::remove_dir_all(&dir);
        // Depth 12 over this two-file fixture must not be a floor — if it were, the
        // with/without-import comparison below would be comparing two truncations.
        assert!(
            !br.truncated(),
            "fixture must fit inside depth 12 (wicked-estate#190)"
        );
        br.dependents.into_iter().map(|n| n.symbol.0).collect()
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

/// The registry cross-check promised by docs/recon/relative-imports.md Decision B (round-1
/// RI-R1-2): the resolver matches conventions rows to importers by the EXTRACT REGISTRY's
/// `language.name` (`relative_import.rs`: an importer whose language has no row is silently
/// skipped — `else { continue }`). `languages.toml` is script-generated, so a future rename of
/// `javascript`/`tsx`/`typescript` would silently kill relative-import binding for that
/// language with no failing test. Pin the coupling from the conventions side.
#[test]
fn import_conventions_languages_exist_in_registry() {
    let conv = wicked_estate_resolve::relative_import::ImportConventions::embedded();
    let names = conv.language_names();
    assert!(
        !names.is_empty(),
        "embedded import-conventions.toml must have rows"
    );
    for name in names {
        assert!(
            wicked_estate_extract::by_name(name).is_some(),
            "import-conventions.toml row '{name}' has no language named '{name}' in the \
             extract registry (languages.toml) — every ref of that language would silently \
             skip relative-import resolution"
        );
    }
}

/// One end-to-end bind each for a `.tsx` and a `.js` IMPORTER through the real registry +
/// `index_path` (round-1 RI-R1-2: the e2e fixture importers above are all `.ts`, so a
/// registry-name drift for `tsx`/`javascript` was invisible).
#[test]
fn tsx_and_js_importers_bind_through_the_real_registry() {
    use wicked_estate_core::{EdgeKind, GraphRead};

    let dir = fresh_dir("tsx_js_importers");
    fs::write(
        dir.join("src/app.tsx"),
        "import { helper } from './helper';\nexport function App() { return helper(); }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/helper.tsx"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/node.js"),
        "const { pure } = require('./pure');\nfunction run() { return pure(); }\nmodule.exports = { run };\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/pure.js"),
        "module.exports = { pure: () => 1 };\n",
    )
    .unwrap();

    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();

    let mut bound: Vec<(String, String)> = store
        .all_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EdgeKind::Imports && e.resolved_by == "relative-import")
        .map(|e| {
            let src = store.get_node(&e.source).unwrap().expect("source node");
            let tgt = store.get_node(&e.target).unwrap().expect("target node");
            (src.location.file, tgt.location.file)
        })
        .collect();
    bound.sort();

    assert_eq!(
        bound,
        vec![
            ("src/app.tsx".to_string(), "src/helper.tsx".to_string()),
            ("src/node.js".to_string(), "src/pure.js".to_string()),
        ],
        "a .tsx importer and a .js importer must each bind via their registry language name"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// wicked-estate#190 follow-up: the depth flag must describe the rows blast-radius returns.
///
/// Twenty `.ts` files form an import-only chain (`m01` imports `targetFn` from `m00`, `m02`
/// imports from `m01`, …) with no calls. The all-kinds walk from `targetFn` crosses the depth-12
/// horizon only through File→File `Imports` edges, and the code_dependents projection drops those
/// import-transit Files. `--depth 40` returns the same single row, so reporting "CUT AT depth=12 —
/// more dependents exist" was false. That shape was 426 of 434 flags on a real TypeScript repo.
#[test]
fn import_only_chain_past_the_horizon_is_not_a_depth_cut() {
    let dir = fresh_dir("import_chain");
    fs::write(
        dir.join("src/m00.ts"),
        "export function targetFn(): number { return 1; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m01.ts"),
        "import { targetFn } from './m00';\nexport const v01 = 1;\n",
    )
    .unwrap();
    for i in 2..20 {
        fs::write(
            dir.join(format!("src/m{i:02}.ts")),
            format!(
                "import {{ v{p:02} }} from './m{p:02}';\nexport const v{i:02} = 1;\n",
                p = i - 1
            ),
        )
        .unwrap();
    }
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let at12 = wicked_estate::blast_radius_by_name(&store, "targetFn", 12).unwrap();
    let at24 = wicked_estate::blast_radius_by_name(&store, "targetFn", 24).unwrap();
    let _ = fs::remove_dir_all(&dir);

    let ids = |br: &wicked_estate::BlastRadius| -> std::collections::BTreeSet<String> {
        br.dependents.iter().map(|n| n.symbol.0.clone()).collect()
    };
    assert_eq!(
        ids(&at12),
        ids(&at24),
        "fixture premise: a deeper walk returns the identical dependent set"
    );
    assert!(
        !at12.depth_horizon_reached && !at12.truncated(),
        "only import-transit Files lie past depth 12, and blast-radius drops them; the result \
         is complete, not a floor: {:?}",
        ids(&at12)
    );
}

/// The control for the test above: a real CALL chain across the same kind of files is still
/// reported as cut at the horizon, and stops being cut once the walk is deep enough.
#[test]
fn call_chain_past_the_horizon_is_still_a_depth_cut() {
    let dir = fresh_dir("call_chain");
    fs::write(
        dir.join("src/c00.ts"),
        "export function f00(): number { return 1; }\n",
    )
    .unwrap();
    for i in 1..20 {
        fs::write(
            dir.join(format!("src/c{i:02}.ts")),
            format!(
                "import {{ f{p:02} }} from './c{p:02}';\nexport function f{i:02}(): number {{ return f{p:02}(); }}\n",
                p = i - 1
            ),
        )
        .unwrap();
    }
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let shallow = wicked_estate::blast_radius_by_name(&store, "f00", 5).unwrap();
    let deep = wicked_estate::blast_radius_by_name(&store, "f00", 24).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(
        shallow.depth_horizon_reached,
        "callers exist past depth 5; the cut must be reported"
    );
    assert!(
        deep.dependents.len() > shallow.dependents.len(),
        "fixture premise: the deeper walk finds more callers"
    );
    assert!(
        !deep.depth_horizon_reached,
        "the whole chain fits in 24 hops"
    );
}
