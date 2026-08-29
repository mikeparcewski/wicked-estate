//! Import-capture coverage for the JS/TS/TSX query files (lane relative-imports S3,
//! review doc 01 D01-7): `export … from`, `require()`, dynamic `import()` and (TS/TSX)
//! `import x = require()` must each produce an `Imports` ref with the QUOTED specifier as
//! `raw_name` — exactly the shape the plain `import_statement` capture already produces —
//! plus a deduped `NodeKind::Import` node per distinct specifier.
//!
//! Everything here is `.scm` data (`typescript.scm`, `tsx.scm`, `javascript.scm`); no Rust
//! extractor change — `classify_capture` already routes `@import` / `@import.source`.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::treesitter::TreeSitterExtractor;

fn extract(lang: &str, file_name: &str, code: &str) -> wicked_estate_core::Extraction {
    TreeSitterExtractor::for_language(lang)
        .unwrap_or_else(|| panic!("extractor for {lang}"))
        .extract(&SourceFile {
            path: file_name.to_string(),
            language: Language::new(lang),
            text: code.to_string(),
        })
        .expect("extraction must succeed")
}

/// The fixture body shared by all three languages (the TS-only line is appended per case).
const COMMON: &str =
    "export * from './y';\nconst z = require('./z');\nconst dyn = import('./dyn');\n";
const TS_ONLY: &str = "import req = require('./req');\n";

fn assert_import_ref_and_node(ex: &wicked_estate_core::Extraction, spec: &str, form: &str) {
    let quoted = format!("'{spec}'");
    assert!(
        ex.refs
            .iter()
            .any(|r| r.kind == EdgeKind::Imports && r.raw_name == quoted),
        "{form}: expected an Imports ref with raw_name {quoted}; got {:?}",
        ex.refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .map(|r| &r.raw_name)
            .collect::<Vec<_>>()
    );
    assert!(
        ex.nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Import) && n.name == spec),
        "{form}: expected an Import node named {spec} (language_integration gate shape)"
    );
}

#[test]
fn typescript_captures_all_four_forms() {
    let ex = extract("typescript", "reexports.ts", &format!("{COMMON}{TS_ONLY}"));
    assert_import_ref_and_node(&ex, "./y", "export * from");
    assert_import_ref_and_node(&ex, "./z", "require()");
    assert_import_ref_and_node(&ex, "./dyn", "dynamic import()");
    assert_import_ref_and_node(&ex, "./req", "import = require()");
}

#[test]
fn tsx_captures_all_four_forms() {
    let ex = extract("tsx", "reexports.tsx", &format!("{COMMON}{TS_ONLY}"));
    assert_import_ref_and_node(&ex, "./y", "export * from");
    assert_import_ref_and_node(&ex, "./z", "require()");
    assert_import_ref_and_node(&ex, "./dyn", "dynamic import()");
    assert_import_ref_and_node(&ex, "./req", "import = require()");
}

#[test]
fn javascript_captures_all_three_forms() {
    let ex = extract("javascript", "reexports.js", COMMON);
    assert_import_ref_and_node(&ex, "./y", "export * from");
    assert_import_ref_and_node(&ex, "./z", "require()");
    assert_import_ref_and_node(&ex, "./dyn", "dynamic import()");
}

#[test]
fn named_export_from_is_an_import_too() {
    let ex = extract(
        "typescript",
        "a.ts",
        "export { a, b as c } from './named';\n",
    );
    assert_import_ref_and_node(&ex, "./named", "export {..} from");
}

#[test]
fn plain_calls_and_exports_do_not_become_imports() {
    // The require() gate (#eq?) and the export-with-source shape must not catch ordinary
    // calls or local exports.
    let ex = extract(
        "typescript",
        "a.ts",
        "export const k = 1;\nfunction go() { return notRequire('./nope'); }\n",
    );
    assert!(
        ex.refs.iter().all(|r| r.kind != EdgeKind::Imports),
        "no Imports refs expected, got {:?}",
        ex.refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .map(|r| &r.raw_name)
            .collect::<Vec<_>>()
    );
    assert!(
        !ex.nodes.iter().any(|n| matches!(n.kind, NodeKind::Import)),
        "no Import nodes expected"
    );
}

#[test]
fn same_specifier_across_forms_dedupes_to_one_import_node() {
    // `import './dup'` + `require('./dup')` name the same module: two refs, ONE Import node
    // (the extractor's seen_imports dedup must hold across the new capture forms).
    let ex = extract(
        "typescript",
        "a.ts",
        "import './dup';\nconst d = require('./dup');\n",
    );
    let refs = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Imports && r.raw_name == "'./dup'")
        .count();
    assert_eq!(refs, 2, "one Imports ref per site");
    let nodes = ex
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Import) && n.name == "./dup")
        .count();
    assert_eq!(nodes, 1, "same module → single Import node");
}
