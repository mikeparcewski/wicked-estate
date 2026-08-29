//! Full-override gate, signal 2 of 2 alone: `WICKED_ESTATE_PLUGIN_OVERRIDE` naming a language
//! whose plugin manifest does NOT set `override = true` does nothing — the plugin stays a plain
//! additive plugin and the built-in wins at both lookup sites.
//!
//! One registry configuration for this file (cc-gated; the arming matrix is also unit tested as
//! a pure fn in plugin.rs).

mod util;

use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;
use wicked_estate_extract::treesitter::extractor_for_extension;

#[test]
fn env_var_alone_is_inert() {
    let root = std::env::temp_dir().join(format!("we-ov-envonly-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    if !util::build_nginx_plugin(
        &root.join("tsov"),
        "name = \"typescript\"\nlibrary = \"libnginx\"\nsymbol = \"tree_sitter_nginx\"\nextensions = [\"ts\"]\nquery = \"nginx.scm\"\n",
    ) {
        eprintln!("SKIP: no `cc` available to build the override dylib");
        return;
    }

    // SAFETY: before any registry access; set_var is unsafe in edition 2024.
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &root);
        std::env::set_var("WICKED_ESTATE_PLUGIN_OVERRIDE", "typescript");
    }

    assert!(
        wicked_estate_extract::plugin::grammar_override_for_name("typescript").is_none(),
        "without `override = true` in the manifest, the env var must arm nothing"
    );

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
        "extension dispatch must stay built-in"
    );
}
