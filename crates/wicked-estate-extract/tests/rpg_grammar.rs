//! RPG grammar validation — the "right testing" that substitutes for upstream battle-testing.
//!
//! The free-format RPG IV grammar (`vendor/tree-sitter-rpg`) was authored in-house — no upstream
//! tree-sitter grammar exists for RPG. Its trust basis is THIS file, two checks:
//!
//! 1. **Corpus parse-gate** — every real sample file must parse with ZERO `ERROR`/`MISSING` nodes.
//!    That is precisely what "battle-tested" reduces to, made into a pass/fail check. Confidence is
//!    bounded by corpus breadth (add files to raise it); the gate makes the bound explicit.
//! 2. **Extraction-count comparison** — the grammar must yield the same *shape* of symbols/calls
//!    that the other 79 languages' extraction does (the behavioral oracle), including NOT emitting
//!    false call edges for type keywords like `packed(11:2)`.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/tree-sitter-rpg/corpus/sample1.rpgle"
    ),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/tree-sitter-rpg/corpus/sample2.rpgle"
    ),
];

#[test]
fn rpg_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_rpg::LANGUAGE.into())
        .expect("rpg grammar must load");
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
fn rpg_extraction_counts_match_expected() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("rpg")
        .expect("rpg registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.rpgle".to_string(),
            language: Language::new("rpg"),
            text: src,
        })
        .expect("rpg extraction");

    // sample1 defines exactly 2 procedures: calcTotal, main.
    let procs = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .count();
    assert_eq!(procs, 2, "expected 2 RPG procedures, got {procs}");

    // Real calls only (NOT type-keywords): calcTotal, doSomething, logmsg, logmsg, logIt = 5.
    let calls = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
    assert_eq!(calls, 5, "expected 5 RPG call refs, got {calls}");

    // Soundness: type keywords must NEVER appear as call edges (the keyword_arg vs call_expression
    // split). A false edge here would corrupt blast-radius.
    for noise in ["packed", "char", "int", "usage", "dftactgrp", "actgrp"] {
        assert!(
            !ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Calls && r.raw_name == noise),
            "type-keyword '{noise}' must NOT be emitted as a call edge",
        );
    }
}
