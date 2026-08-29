//! Full-override gate, BOTH signals: `override = true` AND the language named in
//! `WICKED_ESTATE_PLUGIN_OVERRIDE` flips precedence — plugin grammar + query in use at both
//! lookup sites. Plus the extension-claim rules: an armed claim on the overridden language's own
//! extension is honored, a claim on a FOREIGN built-in's extension (`py`) is refused (double
//! opt-in is per captured language), and a non-built-in extension claim passes.
//!
//! One registry configuration for this file (cc-gated).

mod util;

use wicked_estate_core::{Extraction, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;
use wicked_estate_extract::treesitter::extractor_for_extension;

/// nginx conf the nginx grammar+query extracts Module nodes from — a POSITIVE probe that the
/// plugin pair (not the built-in) is in use.
const NGINX_CONF: &str = "http {\n    server {\n        listen 80;\n    }\n}\n";

fn extract(x: &TreeSitterExtractor, path: &str, lang: &str, text: &str) -> Extraction {
    x.extract(&SourceFile {
        path: path.to_string(),
        language: Language::new(lang),
        text: text.to_string(),
    })
    .expect("extract")
}

fn is_plugin_grammar(ex: &Extraction) -> bool {
    ex.nodes
        .iter()
        .any(|n| n.kind == NodeKind::Module && n.name == "http")
}

#[test]
fn both_signals_arm_the_grammar_override() {
    let root = std::env::temp_dir().join(format!("we-ov-both-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    if !util::build_nginx_plugin(
        &root.join("tsov"),
        "name = \"typescript\"\nlibrary = \"libnginx\"\nsymbol = \"tree_sitter_nginx\"\nextensions = [\"ts\", \"py\", \"customext\"]\nquery = \"nginx.scm\"\noverride = true\n",
    ) {
        eprintln!("SKIP: no `cc` available to build the override dylib");
        return;
    }

    // SAFETY: before any registry access; set_var is unsafe in edition 2024.
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &root);
        std::env::set_var("WICKED_ESTATE_PLUGIN_OVERRIDE", "typescript");
    }

    let armed = wicked_estate_extract::plugin::grammar_override_for_name("typescript")
        .expect("both signals must arm the override");
    // The cross-language capture rule filtered the claims: own ext + non-built-in survive,
    // python-owned `py` dropped.
    assert_eq!(armed.extensions, vec!["ts", "customext"]);
    assert!(
        wicked_estate_extract::plugin::listings().iter().any(|l| {
            l.status
                .as_deref()
                .is_some_and(|s| s.contains("grammar(typescript) [armed]"))
        }),
        "the armed state must be visible in the listings"
    );

    // for_language: plugin grammar + query in use.
    let by_name = TreeSitterExtractor::for_language("typescript").expect("typescript available");
    assert!(
        is_plugin_grammar(&extract(&by_name, "a.ts", "typescript", NGINX_CONF)),
        "for_language must return the PLUGIN grammar+query when armed"
    );

    // Armed ext claim honored: `.ts` dispatches to the plugin pair.
    let by_ext = extractor_for_extension("ts").expect("ts dispatched");
    assert!(
        is_plugin_grammar(&extract(&by_ext, "b.ts", "typescript", NGINX_CONF)),
        "an armed override's own extension claim must be honored at extension dispatch"
    );

    // Foreign-owned claim refused: `.py` still dispatches to built-in python.
    assert!(
        wicked_estate_extract::plugin::grammar_override_for_ext("py").is_none(),
        "the python-owned `py` claim must have been dropped"
    );
    let py = extractor_for_extension("py").expect("py dispatched");
    let ex = extract(&py, "g.py", "python", "def g():\n    pass\n");
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "g"),
        "python must stay built-in — a typescript override can never hijack .py"
    );

    // Non-built-in claim passes: `.customext` dispatches to the plugin pair.
    let custom = extractor_for_extension("customext").expect("customext dispatched via override");
    assert!(
        is_plugin_grammar(&extract(&custom, "c.customext", "typescript", NGINX_CONF)),
        "a non-built-in extension claim on an armed override must dispatch to the plugin"
    );
}
