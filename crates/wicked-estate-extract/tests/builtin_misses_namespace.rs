//! Control for `query_override.rs` — pins the fixture premise: the BUILT-IN typescript query
//! does NOT capture an ambient function signature. Namespace capture is built in as of #377, so
//! the override fixture uses `declare function` instead. If the built-in adds that capture, this
//! fails loudly so the override fixture cannot pass vacuously.
//!
//! One registry configuration for this file: an EMPTY plugins dir.

use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

#[test]
fn builtin_typescript_query_misses_ambient_function_signatures() {
    let empty = std::env::temp_dir().join(format!("we-qov-control-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    // SAFETY: before any registry access; set_var is unsafe in edition 2024.
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &empty);
        std::env::remove_var("WICKED_ESTATE_PLUGIN_OVERRIDE");
    }

    let x = TreeSitterExtractor::for_language("typescript").expect("built-in typescript");
    let ex = x
        .extract(&SourceFile {
            path: "a.ts".to_string(),
            language: Language::new("typescript"),
            text: "declare function provided(): void;\nexport function f(): void {}\n".to_string(),
        })
        .expect("extract");

    assert!(
        !ex.nodes.iter().any(|n| n.name == "provided"),
        "PREMISE DEAD: built-in typescript.scm now captures ambient functions — move the \
         override fixture to another probe construct; got {:?}",
        ex.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );
    // Sanity: the built-in still captures the function, so the miss above is not a parse failure.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "f"),
        "built-in extraction must work in the control"
    );
}
