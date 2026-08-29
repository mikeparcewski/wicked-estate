//! Non-override plugins still NEVER shadow a built-in (ADR-009 keeps tier 1 additive), and a
//! library-less non-override manifest is refused — two real legs, neither vacuous:
//!
//! - always: a LIBRARY-LESS manifest with no override fields is refused by the
//!   library-required rule (asserted via `find_by_name` returning none — the refusal itself,
//!   not an accidentally-empty registry) and built-in extraction is unchanged;
//! - cc-gated: a REAL dylib plugin named `typescript` with an unarmed `ts` extension claim
//!   loads additively, and the built-in still wins at BOTH lookup sites.
//!
//! One registry configuration for this file.

mod util;

use wicked_estate_core::{Extraction, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;
use wicked_estate_extract::treesitter::extractor_for_extension;

const TS_FIXTURE: &str = "function f(): void {}\n";

fn extract_ts(x: &TreeSitterExtractor) -> Extraction {
    x.extract(&SourceFile {
        path: "a.ts".to_string(),
        language: Language::new("typescript"),
        text: TS_FIXTURE.to_string(),
    })
    .expect("extract")
}

fn is_builtin_ts(ex: &Extraction) -> bool {
    // The built-in typescript query captures `function f`; the nginx grammar+query would parse
    // this text as garbage and mint no Function node — a positive probe, not an absence probe.
    ex.nodes
        .iter()
        .any(|n| n.kind == NodeKind::Function && n.name == "f")
}

#[test]
fn additive_plugins_never_shadow_builtins() {
    let root = std::env::temp_dir().join(format!("we-noshadow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // Leg 1 fixture (always): library-less, non-override manifest — must be refused.
    let nolib = root.join("nolib");
    std::fs::create_dir_all(&nolib).unwrap();
    std::fs::write(
        nolib.join("plugin.toml"),
        "name = \"fakelang\"\nquery = \"q.scm\"\n",
    )
    .unwrap();
    std::fs::write(nolib.join("q.scm"), "(comment) @code_function.def").unwrap();

    // Leg 2 fixture (cc-gated): a real dylib named `typescript` claiming `ts`, NO override flags.
    let cc = util::build_nginx_plugin(
        &root.join("tsplug"),
        "name = \"typescript\"\nlibrary = \"libnginx\"\nsymbol = \"tree_sitter_nginx\"\nextensions = [\"ts\"]\nquery = \"nginx.scm\"\n",
    );

    // SAFETY: before any registry access; set_var is unsafe in edition 2024.
    unsafe {
        std::env::set_var("WICKED_ESTATE_PLUGINS", &root);
        std::env::remove_var("WICKED_ESTATE_PLUGIN_OVERRIDE");
    }

    // Leg 1: the library-required refusal fired — the manifest parsed but never registered.
    assert!(
        wicked_estate_extract::plugin::find_by_name("fakelang").is_none(),
        "a library-less non-override manifest must be refused"
    );

    if cc {
        // Leg 2: the dylib plugin IS in the registry (loaded additively, never refused) ...
        assert!(
            wicked_estate_extract::plugin::find_by_name("typescript").is_some(),
            "the dylib plugin must load additively"
        );
    } else {
        eprintln!("SKIP: no `cc` available — dylib no-shadow leg not built");
        assert!(wicked_estate_extract::plugin::find_by_name("typescript").is_none());
    }

    // ... and the BUILT-IN wins at both lookup sites regardless.
    let by_name = TreeSitterExtractor::for_language("typescript").expect("typescript available");
    assert!(
        is_builtin_ts(&extract_ts(&by_name)),
        "for_language must return the BUILT-IN typescript extractor"
    );
    let by_ext = extractor_for_extension("ts").expect("ts dispatched");
    assert!(
        is_builtin_ts(&extract_ts(&by_ext)),
        "extractor_for_extension must return the BUILT-IN typescript extractor (unarmed ext \
         claim not honored)"
    );
}
