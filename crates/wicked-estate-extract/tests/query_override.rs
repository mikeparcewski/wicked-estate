//! ADR-010 tier 2, proven in ONE registry configuration (the registry is a process-wide
//! `OnceLock`, so file boundaries = configuration boundaries): the plugins dir holds a SUPERSET
//! query override for `typescript` (built-in query + an `internal_module` pattern) and a
//! NAMESPACE-ONLY override for `tsx` (legal — overrides match per LANG_TABLE entry, so this is
//! not a duplicate).
//!
//! - `.ts` fixture → namespace AND function present: the override *captures* what the built-in
//!   misses (the control test `builtin_misses_namespace.rs` pins that premise).
//! - `.tsx` fixture → namespace present, function ABSENT: wholesale *replacement*, not a merge.

use std::path::PathBuf;
use std::sync::OnceLock;

use wicked_estate_core::{Extraction, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;
use wicked_estate_extract::treesitter::extractor_for_extension;

const BUILTIN_TS_QUERY: &str = include_str!("../src/queries/typescript.scm");
const NAMESPACE_PATTERN: &str =
    "\n(internal_module\n  name: (identifier) @code_namespace.name\n) @code_namespace.def\n";
const FIXTURE: &str = "namespace Util {\n  export function f(): void {}\n}\n";

/// Build the plugins dir and point the process-wide registry at it — exactly once, before any
/// registry access from either test.
fn plugins_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("we-qov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let ts = root.join("ts-superset");
        std::fs::create_dir_all(&ts).unwrap();
        std::fs::write(
            ts.join("plugin.toml"),
            "name = \"ts-superset\"\nquery = \"typescript.scm\"\noverride_query = \"typescript\"\n",
        )
        .unwrap();
        std::fs::write(
            ts.join("typescript.scm"),
            format!("{BUILTIN_TS_QUERY}{NAMESPACE_PATTERN}"),
        )
        .unwrap();

        let tsx = root.join("tsx-namespace-only");
        std::fs::create_dir_all(&tsx).unwrap();
        std::fs::write(
            tsx.join("plugin.toml"),
            "name = \"tsx-namespace-only\"\nquery = \"tsx.scm\"\noverride_query = \"tsx\"\n",
        )
        .unwrap();
        std::fs::write(tsx.join("tsx.scm"), NAMESPACE_PATTERN).unwrap();

        // SAFETY: before any registry access; set_var is unsafe in edition 2024.
        unsafe {
            std::env::set_var("WICKED_ESTATE_PLUGINS", &root);
            std::env::remove_var("WICKED_ESTATE_PLUGIN_OVERRIDE");
        }
        root
    })
}

fn extract(lang: &str, path: &str) -> Extraction {
    let x = TreeSitterExtractor::for_language(lang)
        .unwrap_or_else(|| panic!("{lang} must stay available under an override"));
    x.extract(&SourceFile {
        path: path.to_string(),
        language: Language::new(lang),
        text: FIXTURE.to_string(),
    })
    .expect("extract")
}

fn has(ex: &Extraction, kind: &NodeKind, name: &str) -> bool {
    ex.nodes.iter().any(|n| &n.kind == kind && n.name == name)
}

#[test]
fn superset_override_captures_what_the_builtin_misses() {
    plugins_root();
    let ex = extract("typescript", "a.ts");
    assert!(
        has(&ex, &NodeKind::Namespace, "Util"),
        "the override's internal_module pattern must mint the namespace node; got {:?}",
        ex.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );
    assert!(
        has(&ex, &NodeKind::Function, "f"),
        "the superset override keeps every built-in capture"
    );

    // Extension dispatch delegates to for_language, so `.ts` picks up the override too.
    let x = extractor_for_extension("ts").expect("ts stays dispatched");
    let ex2 = x
        .extract(&SourceFile {
            path: "b.ts".to_string(),
            language: Language::new("typescript"),
            text: FIXTURE.to_string(),
        })
        .unwrap();
    assert!(has(&ex2, &NodeKind::Namespace, "Util"));
}

#[test]
fn override_is_wholesale_replacement_not_a_merge() {
    plugins_root();
    // The tsx override carries ONLY the namespace pattern — the built-in function pattern is
    // deliberately absent, and with it the function capture must disappear.
    let ex = extract("tsx", "a.tsx");
    assert!(
        has(&ex, &NodeKind::Namespace, "Util"),
        "namespace-only override captures the namespace"
    );
    assert!(
        !has(&ex, &NodeKind::Function, "f"),
        "an override REPLACES the built-in query — a pattern it lacks is a construct it stops \
         extracting; got {:?}",
        ex.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );
}
