//! ADR-014 "Cumulative": the incremental re-index cost was a fixed per-process price, paid by
//! compiling the tree-sitter query of EVERY extension present in the repo (and `.ts`/`.mts`/`.cts`
//! each separately) before knowing which files changed. An index pass now compiles a language's
//! query at most once, and only for the languages of the files it actually extracts.
//!
//! One `#[test]` in its own test binary: the compile counter is process-global, so a parallel
//! test in the same binary would race it.

use std::fs;
use std::path::Path;

use wicked_estate_extract::treesitter::query_compile_count;
use wicked_estate_store::SqliteStore;

fn compiles_during(store: &mut SqliteStore, root: &Path) -> usize {
    let before = query_compile_count();
    wicked_estate::index_path(store, root).expect("index");
    query_compile_count() - before
}

#[test]
fn an_index_pass_compiles_each_extracted_language_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(
        root.join("a.ts"),
        "export function a(x: number) { return x + 1; }\n",
    )
    .unwrap();
    fs::write(
        root.join("b.mts"),
        "export const b = (y: string) => y.trim();\n",
    )
    .unwrap();
    fs::write(root.join("c.cts"), "export function c() { return 3; }\n").unwrap();
    fs::write(root.join("d.js"), "function d(z) { return z; }\n").unwrap();
    fs::write(root.join("e.py"), "def e(w):\n    return w\n").unwrap();
    let mut store = SqliteStore::open(root.join(".g.db")).expect("store");

    // typescript (one query for .ts, .mts and .cts), javascript, python.
    assert_eq!(compiles_during(&mut store, root), 3, "full index");
    // Nothing changed: nothing is extracted, so nothing is compiled.
    assert_eq!(compiles_during(&mut store, root), 0, "unchanged re-index");
    // One `.ts` file edited: only the typescript query.
    fs::write(
        root.join("a.ts"),
        "export function a(x: number) { return x + 2; }\n",
    )
    .unwrap();
    assert_eq!(compiles_during(&mut store, root), 1, "one-file re-index");
}
