//! LotusScript grammar validation — corpus parse-gate + extraction-count (same contract as RPG/ABL).
//! The in-house grammar (`vendor/tree-sitter-lotusscript`) was authored because no upstream
//! tree-sitter grammar exists for LotusScript.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

const CORPUS: &[&str] = &[concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/tree-sitter-lotusscript/corpus/sample1.lss"
)];

#[test]
fn lotusscript_corpus_parses_with_zero_errors() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&wicked_estate_tree_sitter_lotusscript::LANGUAGE.into())
        .expect("lotusscript grammar must load");
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
fn lotusscript_extraction_shape() {
    let src = std::fs::read_to_string(CORPUS[0]).expect("read sample1");
    let ex = TreeSitterExtractor::for_language("lotusscript")
        .expect("lotusscript registered in LANG_TABLE")
        .extract(&SourceFile {
            path: "sample1.lss".to_string(),
            language: Language::new("lotusscript"),
            text: src,
        })
        .expect("lotusscript extraction");

    // Two classes.
    let classes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Class)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        classes.contains(&"iCalItem"),
        "class iCalItem missing; got {classes:?}"
    );
    assert!(
        classes.contains(&"iCalFeed"),
        "class iCalFeed missing; got {classes:?}"
    );

    // Subs + functions (New, setUid, addItem subs; getUid, toString, render functions).
    let fns: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    for f in ["New", "setUid", "getUid", "toString", "addItem", "render"] {
        assert!(fns.contains(&f), "definition '{f}' missing; got {fns:?}");
    }

    // Property Get Summary.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Field && n.name == "Summary"),
        "property 'Summary' missing"
    );

    // Calls: Call appendLine(...) + escapeLine() + item.toString() etc.
    let calls: Vec<_> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"appendLine"),
        "Call appendLine missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"escapeLine"),
        "escapeLine call missing; got {calls:?}"
    );
}
