//! Control for `query_override.rs` — pins the fixture premise: the BUILT-IN typescript query
//! does NOT capture a `namespace` declaration (`internal_module`). If a future change adds the
//! pattern to `typescript.scm`, this fails loudly and the override fixture must move to the next
//! probe construct (`declare module`, Ruby `define_method`) instead of the override test passing
//! vacuously.
//!
//! One registry configuration for this file: an EMPTY plugins dir.

use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

#[test]
fn builtin_typescript_query_misses_namespace_declarations() {
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
            text: "namespace Util {\n  export function f(): void {}\n}\n".to_string(),
        })
        .expect("extract");

    assert!(
        !ex.nodes.iter().any(|n| n.kind == NodeKind::Namespace),
        "PREMISE DEAD: built-in typescript.scm now captures `namespace` — move the override \
         fixture to the next probe construct; got {:?}",
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
