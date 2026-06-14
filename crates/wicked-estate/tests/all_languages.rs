//! Pipeline-level coverage for EVERY wired language: write one real source file per language with
//! the correct extension, run the full `index_path` pipeline (extension→extractor dispatch →
//! extract → resolve → store), and assert each language contributed indexed nodes. This catches
//! integration-seam regressions (e.g. a missing extension→language mapping in `languages.toml`)
//! that the per-extractor unit tests in `wicked-estate-extract` cannot see.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::GraphRead;
use wicked_estate_store::SqliteStore;

/// (filename, source) for each wired language. Each snippet has at least one definition.
fn fixtures() -> Vec<(&'static str, &'static str)> {
    vec![
        ("a.rs", "fn a() {}\nfn b() { a(); }\n"),
        ("a.py", "def a():\n    pass\n\ndef b():\n    a()\n"),
        ("a.ts", "function a() {}\nfunction b() { a(); }\n"),
        (
            "a.tsx",
            "function a() { return null; }\nfunction b() { a(); }\n",
        ),
        ("a.js", "function a() {}\nfunction b() { a(); }\n"),
        ("a.go", "package main\nfunc a() {}\nfunc b() { a() }\n"),
        (
            "M.java",
            "class M {\n  void a() {}\n  void b() { a(); }\n}\n",
        ),
        ("a.c", "void a() {}\nvoid b() { a(); }\n"),
        ("a.cpp", "void a() {}\nvoid b() { a(); }\n"),
        ("a.cs", "class M {\n  void a() {}\n  void b() { a(); }\n}\n"),
        ("a.rb", "def a\nend\n\ndef b\n  a\nend\n"),
        ("a.sh", "a() { :; }\nb() { a; }\n"),
        (
            "a.json",
            "{\n  \"name\": \"x\",\n  \"version\": \"1.0\"\n}\n",
        ),
        ("a.yaml", "name: x\nversion: \"1.0\"\n"),
    ]
}

/// The languages we expect to see represented among the indexed nodes.
fn expected_languages() -> BTreeSet<&'static str> {
    [
        "rust",
        "python",
        "typescript",
        "tsx",
        "javascript",
        "go",
        "java",
        "c",
        "cpp",
        "csharp",
        "ruby",
        "bash",
        "json",
        "yaml",
    ]
    .into_iter()
    .collect()
}

fn fresh_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_all_langs_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn every_wired_language_indexes_through_the_pipeline() {
    let dir = fresh_dir();
    for (name, src) in fixtures() {
        fs::write(dir.join(name), src).unwrap();
    }

    let mut store = SqliteStore::in_memory().expect("sqlite");
    let stats = wicked_estate::index_path(&mut store, &dir).expect("index_path");

    // Every fixture file should have been recognized and indexed.
    assert_eq!(
        stats.file_count as usize,
        fixtures().len(),
        "every language fixture must be indexed as a file; got {} of {}",
        stats.file_count,
        fixtures().len()
    );

    // Collect the set of languages that actually produced nodes.
    let nodes = (&store as &dyn GraphRead).all_nodes().expect("all_nodes");
    let seen: BTreeSet<String> = nodes
        .iter()
        .map(|n| n.language.as_str().to_string())
        .collect();

    let expected = expected_languages();
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|l| !seen.contains(*l))
        .collect();
    assert!(
        missing.is_empty(),
        "these wired languages produced NO indexed nodes through the pipeline (extension→extractor \
         mapping gap?): {missing:?}. Languages seen: {seen:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}
