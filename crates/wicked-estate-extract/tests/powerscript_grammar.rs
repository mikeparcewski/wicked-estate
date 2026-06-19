//! PowerBuilder PowerScript grammar validation — corpus parse-gate + extraction-count (RPG/ABL
//! contract). In-house grammar (`vendor/tree-sitter-powerscript`); no upstream grammar exists.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/tree-sitter-powerscript/corpus/sample1.sru"
)];

#[test]
fn powerscript_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_powerscript::LANGUAGE.into())
        .expect("powerscript grammar must load");
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
fn powerscript_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("powerscript")
        .expect("powerscript registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.sru".to_string(),
            language: Language::new("powerscript"),
            text: src,
        })
        .expect("powerscript extraction");

    // The object type → a class node (its from-ancestor is n_base).
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "pfc_n_cst_environment"),
        "type pfc_n_cst_environment missing; got {:?}",
        ex.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );

    // Function bodies of_refresh + of_getosinfo.
    let fns: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        fns.contains(&"of_refresh"),
        "function of_refresh missing; got {fns:?}"
    );
    assert!(
        fns.contains(&"of_getosinfo"),
        "function of_getosinfo missing; got {fns:?}"
    );

    // The event body is captured as a method.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Method && n.name == "pfc_osversioninfodecode"),
        "event pfc_osversioninfodecode missing"
    );

    // Calls: GetComputerName(...), of_GetEnvironment(), This.of_GetOSInfo() etc.
    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"GetComputerName"),
        "GetComputerName call missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"of_GetEnvironment"),
        "of_GetEnvironment call missing; got {calls:?}"
    );
}
