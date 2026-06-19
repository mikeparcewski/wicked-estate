//! Crystal Reports formula grammar validation — corpus parse-gate + extraction-count.
//! In-house grammar (`vendor/tree-sitter-crystal-formula`); no upstream grammar exists. A Crystal
//! formula is an expression fragment, so the symbols are variable declarations and the calls are
//! `{@formula}` references + built-in/function calls.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/tree-sitter-crystal-formula/corpus/sample1.crf"
)];

#[test]
fn crystal_formula_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_crystal_formula::LANGUAGE.into())
        .expect("crystal_formula grammar must load");
    for path in CORPUS {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let tree = parser.parse(src.as_bytes(), None).expect("parse tree");
        assert!(
            !tree.root_node().has_error(),
            "PARSE GATE FAILED: {path} contains ERROR/MISSING nodes",
        );
    }
}

#[test]
fn crystal_formula_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("crystal_formula")
        .expect("crystal_formula registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.crf".to_string(),
            language: Language::new("crystal_formula"),
            text: src,
        })
        .expect("crystal_formula extraction");

    // Variable declarations: sCircles, weekStart, holidays, header.
    let vars: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Variable)
        .map(|n| n.name.as_str())
        .collect();
    for v in ["sCircles", "weekStart", "holidays", "header"] {
        assert!(vars.contains(&v), "variable '{v}' missing; got {vars:?}");
    }

    // Calls: {@ReportHeader} formula reference + ToText() built-in call.
    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.iter().any(|c| c.contains("ReportHeader")),
        "{{@ReportHeader}} formula ref missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"ToText"),
        "ToText() call missing; got {calls:?}"
    );
}
