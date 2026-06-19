//! Visual FoxPro grammar validation — corpus parse-gate + extraction-count (RPG/ABL contract).
//! In-house grammar (`vendor/tree-sitter-foxpro`); no upstream tree-sitter grammar exists.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/tree-sitter-foxpro/corpus/sample1.prg"
)];

#[test]
fn foxpro_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_foxpro::LANGUAGE.into())
        .expect("foxpro grammar must load");
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
fn foxpro_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("foxpro")
        .expect("foxpro registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.prg".to_string(),
            language: Language::new("foxpro"),
            text: src,
        })
        .expect("foxpro extraction");

    // DEFINE CLASS CustomerForm.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "CustomerForm"),
        "class CustomerForm missing; got {:?}",
        ex.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );

    // PROCEDURE/FUNCTION defs: Init, cmdSave_Click, ComputeTotal (method), InitApp, LogMessage.
    let fns: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    for f in [
        "Init",
        "cmdSave_Click",
        "ComputeTotal",
        "InitApp",
        "LogMessage",
    ] {
        assert!(fns.contains(&f), "definition '{f}' missing; got {fns:?}");
    }

    // Function-call-syntax calls: InitApp() (=InitApp()), LoadDefaults(), ValidateForm(), etc.
    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"InitApp"),
        "=InitApp() call missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"ValidateForm"),
        "ValidateForm call missing; got {calls:?}"
    );
}
