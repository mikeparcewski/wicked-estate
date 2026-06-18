//! Progress OpenEdge ABL grammar validation — the "right testing" that substitutes for upstream
//! battle-testing (the same contract as the in-house RPG grammar).
//!
//! The minimal ABL grammar (`vendor/tree-sitter-abl`) was authored in-house because the
//! comprehensive upstream grammar ships a ~97MB parser.c (too large to vendor near the 100MiB git
//! limit). Its trust basis is THIS file, two checks:
//!
//! 1. **Corpus parse-gate** — every sample file parses with ZERO `ERROR`/`MISSING` nodes.
//! 2. **Extraction-count comparison** — the grammar yields the same *shape* of symbols/calls the
//!    other languages' extraction does (the behavioral oracle).

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/tree-sitter-abl/corpus/sample1.cls"
    ),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/tree-sitter-abl/corpus/sample2.p"
    ),
];

#[test]
fn abl_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_abl::LANGUAGE.into())
        .expect("abl grammar must load");
    for path in CORPUS {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let tree = parser
            .parse(src.as_bytes(), None)
            .expect("parse produced a tree");
        assert!(
            !tree.root_node().has_error(),
            "PARSE GATE FAILED: {path} contains ERROR/MISSING nodes",
        );
    }
}

#[test]
fn abl_class_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("abl")
        .expect("abl registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.cls".to_string(),
            language: Language::new("abl"),
            text: src,
        })
        .expect("abl extraction");

    // CLASS → a class node named by its qualified name.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "FunkyWorld.Util.KittyLogHelper"),
        "class node missing; got {:?}",
        ex.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );

    // Constructor + three methods.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Constructor && n.name == "KittyLogHelper"),
        "constructor missing"
    );
    let methods = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Method)
        .count();
    assert_eq!(
        methods, 3,
        "expected 3 ABL methods (setup, computeTotal, sumItems), got {methods}"
    );

    // RUN initialize + Object() + sumItems() calls are captured.
    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"initialize"),
        "RUN initialize call missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"sumItems"),
        "sumItems call missing; got {calls:?}"
    );
}

#[test]
fn abl_procedure_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[1]).expect("read sample2");
    let ex = TreeSitterExtractor::for_language("abl")
        .expect("abl registered")
        .extract(&SourceFile {
            path: "sample2.p".to_string(),
            language: Language::new("abl"),
            text: src,
        })
        .expect("abl extraction");

    let fn_names: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    // FUNCTION calcLineTotal + PROCEDURE processOrder + PROCEDURE postLedger.
    for f in ["calcLineTotal", "processOrder", "postLedger"] {
        assert!(
            fn_names.contains(&f),
            "definition '{f}' missing; got {fn_names:?}"
        );
    }

    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    // calcLineTotal() call + RUN postLedger + RUN processOrder.
    assert!(
        calls.contains(&"calcLineTotal"),
        "calcLineTotal call missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"postLedger"),
        "RUN postLedger missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"processOrder"),
        "RUN processOrder missing; got {calls:?}"
    );
}
