//! Full-override gate, signal 1 of 2 alone: `override = true` in the manifest WITHOUT the
//! language named in `WICKED_ESTATE_PLUGIN_OVERRIDE` is INERT — built-in grammar + query stay in
//! use at both lookup sites, and the inert state is visible in the listings.
//!
//! One registry configuration for this file (cc-gated; the four-way arming matrix is also unit
//! tested as a pure fn in plugin.rs, so this gate is covered even without cc).

mod util;

use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;
use wicked_estate_extract::treesitter::extractor_for_extension;

#[test]
fn manifest_flag_alone_is_inert() {
    let root = std::env::temp_dir().join(format!("we-ov-manifestonly-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    if !util::build_nginx_plugin(
        &root.join("tsov"),
        "name = \"typescript\"\nlibrary = \"libnginx\"\nsymbol = \"tree_sitter_nginx\"\nextensions = [\"ts\"]\nquery = \"nginx.scm\"\noverride = true\n",
    ) {
        eprintln!("SKIP: no `cc` available to build the override dylib");
        return;
    }

    // SAFETY: before any registry access; set_var is unsafe in edition 2024.
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &root);
        std::env::remove_var("WICKED_ESTATE_PLUGIN_OVERRIDE");
    }

    assert!(
        wicked_estate_extract::plugin::grammar_override_for_name("typescript").is_none(),
        "without the env var, the override must not arm"
    );
    assert!(
        wicked_estate_extract::plugin::listings().iter().any(|l| {
            l.status
                .as_deref()
                .is_some_and(|s| s.contains("INERT") && s.contains("grammar(typescript)"))
        }),
        "the inert state must be visible in the listings"
    );

    // Built-in in use at both sites.
    let probe = |x: &TreeSitterExtractor| {
        let ex = x
            .extract(&SourceFile {
                path: "a.ts".to_string(),
                language: Language::new("typescript"),
                text: "function f(): void {}\n".to_string(),
            })
            .expect("extract");
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "f")
    };
    assert!(
        probe(&TreeSitterExtractor::for_language("typescript").expect("typescript available")),
        "for_language must stay built-in"
    );
    assert!(
        probe(&extractor_for_extension("ts").expect("ts dispatched")),
        "extension dispatch must stay built-in (unarmed claim not honored)"
    );
}
