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
