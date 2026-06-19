//! Informix 4GL grammar validation — corpus parse-gate + extraction-count (RPG/ABL contract).
//! In-house grammar (`vendor/tree-sitter-informix4gl`); no usable upstream tree-sitter grammar exists.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/tree-sitter-informix4gl/corpus/sample1.4gl"
)];

#[test]
fn informix4gl_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_informix4gl::LANGUAGE.into())
        .expect("informix4gl grammar must load");
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
fn informix4gl_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("informix4gl")
        .expect("informix4gl registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.4gl".to_string(),
            language: Language::new("informix4gl"),
            text: src,
        })
        .expect("informix4gl extraction");

    // MAIN entry + FUNCTION get_married + REPORT summary → all function-kind nodes.
    let fns: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        fns.iter().any(|n| n.eq_ignore_ascii_case("MAIN")),
        "MAIN entry missing; got {fns:?}"
    );
    assert!(
        fns.contains(&"get_married"),
        "FUNCTION get_married missing; got {fns:?}"
    );
    assert!(
        fns.contains(&"summary"),
        "REPORT summary missing; got {fns:?}"
    );

    // Calls: CALL get_married, RUN "logger.sh" is a string (not captured), lookupSpouse() call.
    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"get_married"),
        "CALL get_married missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"lookupSpouse"),
        "lookupSpouse call missing; got {calls:?}"
    );
}
