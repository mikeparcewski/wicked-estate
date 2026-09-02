//! Characterization / regression tests for ALL 14 wired tree-sitter languages.
//!
//! Each language has a realistic fixture under `tests/fixtures/` and a row in
//! `CASES`. The harness iterates the table and asserts:
//!  - specific definition names + `NodeKind`s are present,
//!  - specific call `raw_name`s appear in `refs` with `EdgeKind::Calls`,
//!  - specific import `raw_name`s appear in `refs` with `EdgeKind::Imports`,
//!  - a floor on total definition count,
//!  - no extraction error.
//!
//! Assertions are **order-independent** (`contains`/set-based) so they are not
//! brittle to capture order, but specific enough that a broken query (e.g. "go
//! calls stop being captured") fails CI.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::{IaCExtractor, TreeSitterExtractor};

// ── helpers ──────────────────────────────────────────────────────────────────

fn load_fixture(filename: &str, lang: &str) -> SourceFile {
    let path = format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/{}"),
        filename
    );
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {filename}: {e}"));
    SourceFile {
        path: path.clone(),
        language: Language::new(lang),
        text,
    }
}

/// Assert that `(name, kind)` appears in the extraction's non-File nodes.
#[track_caller]
fn assert_def(
    extraction: &wicked_estate_core::Extraction,
    lang: &str,
    name: &str,
    kind: &NodeKind,
) {
    let found = extraction
        .nodes
        .iter()
        .any(|n| !matches!(n.kind, NodeKind::File) && n.name == name && &n.kind == kind);
    assert!(
        found,
        "[{lang}] expected definition name={name:?} kind={kind:?} — not found.\n\
         Actual defs: {actual:?}",
        actual = extraction
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (n.name.as_str(), format!("{:?}", n.kind)))
            .collect::<Vec<_>>()
    );
}

/// Assert that at least `floor` non-File definition nodes exist.
#[track_caller]
fn assert_def_floor(extraction: &wicked_estate_core::Extraction, lang: &str, floor: usize) {
    let count = extraction
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File))
        .count();
    assert!(
        count >= floor,
        "[{lang}] expected >= {floor} definitions, got {count}.\n\
         Actual defs: {actual:?}",
        actual = extraction
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File))
            .map(|n| (n.name.as_str(), format!("{:?}", n.kind)))
            .collect::<Vec<_>>()
    );
}

/// §11 fleet-wide guard: within one extraction, no SymbolId may be emitted with more
/// than one distinct NodeKind among non-File nodes. The store's upsert is
/// last-write-wins (`ON CONFLICT(symbol) DO UPDATE SET … kind=excluded.kind`), so a
/// kind conflict here means the stored graph silently re-kinds a definition — the
/// D04-2 defect class (Go catch-all re-kinding Struct→TypeAlias; Python/Rust
/// Method-vs-Function double-emits; C self-typedef TypeAlias-vs-Struct). Same-kind
/// duplicates are deliberately NOT flagged: same-named symbols in different scopes
/// share ids until the method-identity lane adds enclosing-type identity.
#[track_caller]
fn assert_no_conflicting_def_ids(extraction: &wicked_estate_core::Extraction, lang: &str) {
    use std::collections::HashMap;
    let mut by_id: HashMap<&str, Vec<(String, u32)>> = HashMap::new();
    for n in extraction
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File))
    {
        by_id
            .entry(n.symbol.as_str())
            .or_default()
            .push((format!("{:?}", n.kind), n.location.span.start_line));
    }
    let mut conflicts: Vec<String> = by_id
        .iter()
        .filter(|(_, v)| {
            let first = &v[0].0;
            v.iter().any(|(k, _)| k != first)
        })
        .map(|(id, v)| format!("  {id} -> {v:?}"))
        .collect();
    conflicts.sort();
    assert!(
        conflicts.is_empty(),
        "[{lang}] SymbolId(s) emitted with conflicting NodeKinds — the store upsert \
         (last-write-wins) would silently re-kind these definitions:\n{}",
        conflicts.join("\n")
    );
}

/// Assert that NO non-File node with this name exists (negative pin for phantom
/// emissions — e.g. C++ forward declarations must not mint Class/Struct nodes).
#[track_caller]
fn assert_no_def(extraction: &wicked_estate_core::Extraction, lang: &str, name: &str) {
    let hits: Vec<String> = extraction
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == name)
        .map(|n| format!("{:?}", n.kind))
        .collect();
    assert!(
        hits.is_empty(),
        "[{lang}] expected NO definition named {name:?}, found kinds: {hits:?}"
    );
}

/// Assert a call ref with `raw_name` exists.
#[track_caller]
fn assert_call(extraction: &wicked_estate_core::Extraction, lang: &str, name: &str) {
    let found = extraction
        .refs
        .iter()
        .any(|r| r.raw_name == name && r.kind == EdgeKind::Calls);
    assert!(
        found,
        "[{lang}] expected Calls ref name={name:?} — not found.\n\
         Actual call refs: {actual:?}",
        actual = extraction
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| r.raw_name.as_str())
            .collect::<Vec<_>>()
    );
}

/// Assert an import ref with `raw_name` exists.
#[track_caller]
fn assert_import(extraction: &wicked_estate_core::Extraction, lang: &str, name: &str) {
    let found = extraction
        .refs
        .iter()
        .any(|r| r.raw_name == name && r.kind == EdgeKind::Imports);
    assert!(
        found,
        "[{lang}] expected Imports ref name={name:?} — not found.\n\
         Actual import refs: {actual:?}",
        actual = extraction
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .map(|r| r.raw_name.as_str())
            .collect::<Vec<_>>()
    );
}

/// Assert that an Import node with the given canonical name exists.
#[track_caller]
fn assert_import_node(extraction: &wicked_estate_core::Extraction, lang: &str, canonical: &str) {
    let found = extraction
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Import) && n.name == canonical);
    assert!(
        found,
        "[{lang}] expected Import node name={canonical:?} — not found.\n\
         Actual import nodes: {actual:?}",
        actual = extraction
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Import))
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>()
    );
}

// ── Rust ──────────────────────────────────────────────────────────────────────

#[test]
fn rust_characterization() {
    let lang = "rust";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.rs", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    assert_def(&ex, lang, "Point", &NodeKind::Struct);
    assert_def(&ex, lang, "Direction", &NodeKind::Enum);
    assert_def(&ex, lang, "Drawable", &NodeKind::Trait);
    assert_def(&ex, lang, "distance", &NodeKind::Function);
    assert_def(&ex, lang, "main_entry", &NodeKind::Function);
    // impl-block method: emitted ONCE by the general function pattern (§11 — the
    // impl-scoped duplicate Method pattern was deleted; kind stays Function, the
    // impl anchor supplies enclosing-type identity without emitting).
    assert_def(&ex, lang, "translate", &NodeKind::Function);
    // scm-anchors D3: one method per impl-anchor branch (zero-def-loss pins —
    // these defs must survive any future narrowing of the anchor alternation).
    assert_def(&ex, lang, "Rect", &NodeKind::Struct);
    assert_def(&ex, lang, "Holder", &NodeKind::Struct);
    assert_def(&ex, lang, "get", &NodeKind::Function); // impl Holder<T> (generic_type)
    assert_def(&ex, lang, "draw", &NodeKind::Function); // trait impls (plain/scoped/scoped-generic)
    // new kinds
    assert_def(&ex, lang, "MAX_DISTANCE", &NodeKind::Constant);
    assert_def(&ex, lang, "ORIGIN", &NodeKind::Constant);
    assert_def(&ex, lang, "Distance", &NodeKind::TypeAlias);
    assert_def_floor(&ex, lang, 8);

    // calls
    assert_call(&ex, lang, "helper");
    assert_call(&ex, lang, "distance");

    // imports (use declarations captured as raw paths)
    assert_import(&ex, lang, "std::fmt");
    assert_import(&ex, lang, "crate::utils::helper");
    // import nodes (task C)
    assert_import_node(&ex, lang, "std::fmt");
}

// ── Python ────────────────────────────────────────────────────────────────────

#[test]
fn python_characterization() {
    let lang = "python";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.py", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    assert_def(&ex, lang, "FileProcessor", &NodeKind::Class);
    assert_def(&ex, lang, "compute_hash", &NodeKind::Function);
    assert_def(&ex, lang, "run_pipeline", &NodeKind::Function);
    assert_def(&ex, lang, "__init__", &NodeKind::Function);
    assert_def(&ex, lang, "process", &NodeKind::Function);
    // new kinds: UPPER_CASE module-level constants
    assert_def(&ex, lang, "MAX_FILE_SIZE", &NodeKind::Constant);
    assert_def(&ex, lang, "DEFAULT_ENCODING", &NodeKind::Constant);
    assert_def_floor(&ex, lang, 8);

    // calls: processor.process() -> "process"; compute_hash(content) -> "compute_hash"
    assert_call(&ex, lang, "process");
    assert_call(&ex, lang, "compute_hash");

    // imports
    assert_import(&ex, lang, "os");
    assert_import(&ex, lang, "pathlib");
    // import nodes (task C)
    assert_import_node(&ex, lang, "os");
    assert_import_node(&ex, lang, "pathlib");
}

// ── TypeScript ────────────────────────────────────────────────────────────────

#[test]
fn typescript_characterization() {
    let lang = "typescript";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.ts", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions: interface, enum, class, methods, functions
    assert_def(&ex, lang, "Processor", &NodeKind::Interface);
    assert_def(&ex, lang, "Status", &NodeKind::Enum);
    assert_def(&ex, lang, "DataPipeline", &NodeKind::Class);
    assert_def(&ex, lang, "transform", &NodeKind::Function);
    assert_def(&ex, lang, "buildPipeline", &NodeKind::Function);
    // constructor is captured as a method
    assert_def(&ex, lang, "constructor", &NodeKind::Method);
    assert_def(&ex, lang, "process", &NodeKind::Method);
    // new kinds
    assert_def(&ex, lang, "ProcessorId", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "Callback", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "MAX_RETRIES", &NodeKind::Constant);
    assert_def(&ex, lang, "DEFAULT_TIMEOUT", &NodeKind::Constant);
    assert_def(&ex, lang, "createEmitter", &NodeKind::Function); // arrow fn
    assert_def(&ex, lang, "retryCount", &NodeKind::Variable);
    // scm-anchors D6: object-valued class fields (public + private name branch)
    assert_def(&ex, lang, "hooks", &NodeKind::Field);
    assert_def(&ex, lang, "#internals", &NodeKind::Field);
    assert_def_floor(&ex, lang, 10);

    // calls
    assert_call(&ex, lang, "transform");
    assert_call(&ex, lang, "emit");

    // imports — source paths as raw strings (with quotes)
    assert_import(&ex, lang, "'events'");
    assert_import(&ex, lang, "'fs'");
    // import nodes (task C) — canonical names stripped of quotes
    assert_import_node(&ex, lang, "events");
    assert_import_node(&ex, lang, "fs");
}

// ── TSX ───────────────────────────────────────────────────────────────────────

#[test]
fn tsx_characterization() {
    let lang = "tsx";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.tsx", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    assert_def(&ex, lang, "ButtonProps", &NodeKind::Interface);
    assert_def(&ex, lang, "Theme", &NodeKind::Enum);
    assert_def(&ex, lang, "ThemeProvider", &NodeKind::Class);
    assert_def(&ex, lang, "toggle", &NodeKind::Method);
    assert_def(&ex, lang, "Button", &NodeKind::Function);
    assert_def(&ex, lang, "App", &NodeKind::Function);
    // new kinds
    assert_def(&ex, lang, "ThemeMode", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "DEFAULT_THEME", &NodeKind::Constant);
    assert_def(&ex, lang, "handleClick", &NodeKind::Function); // arrow fn
    // scm-anchors D6: object-valued class fields (public + private name branch)
    assert_def(&ex, lang, "palette", &NodeKind::Field);
    assert_def(&ex, lang, "#cache", &NodeKind::Field);
    assert_def_floor(&ex, lang, 8);

    // calls
    assert_call(&ex, lang, "useState");
    assert_call(&ex, lang, "toggle");

    // imports
    assert_import(&ex, lang, "'react'");
    // import nodes (task C)
    assert_import_node(&ex, lang, "react");
}

// ── JavaScript ───────────────────────────────────────────────────────────────

#[test]
fn javascript_characterization() {
    let lang = "javascript";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.js", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions: class + methods + free functions
    assert_def(&ex, lang, "EventBus", &NodeKind::Class);
    assert_def(&ex, lang, "constructor", &NodeKind::Method);
    assert_def(&ex, lang, "on", &NodeKind::Method);
    assert_def(&ex, lang, "emit", &NodeKind::Method);
    assert_def(&ex, lang, "createBus", &NodeKind::Function);
    assert_def(&ex, lang, "processData", &NodeKind::Function);
    assert_def(&ex, lang, "serialize", &NodeKind::Function);
    // new kinds
    assert_def(&ex, lang, "MAX_LISTENERS", &NodeKind::Constant);
    assert_def(&ex, lang, "DEFAULT_DELAY", &NodeKind::Constant);
    assert_def(&ex, lang, "createHandler", &NodeKind::Function); // arrow fn
    assert_def(&ex, lang, "instanceCount", &NodeKind::Variable);
    // scm-anchors D6: object-valued class fields (public + private name branch)
    assert_def(&ex, lang, "hooks", &NodeKind::Field);
    assert_def(&ex, lang, "#internals", &NodeKind::Field);
    assert_def_floor(&ex, lang, 10);

    // calls
    assert_call(&ex, lang, "createBus");
    assert_call(&ex, lang, "emit");
    assert_call(&ex, lang, "serialize");

    // imports
    assert_import(&ex, lang, "'fs'");
    // import nodes (task C)
    assert_import_node(&ex, lang, "fs");
}

// ── Go ────────────────────────────────────────────────────────────────────────

#[test]
fn go_characterization() {
    let lang = "go";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.go", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    assert_def(&ex, lang, "NewCircle", &NodeKind::Function);
    assert_def(&ex, lang, "Area", &NodeKind::Method);
    assert_def(&ex, lang, "Distance", &NodeKind::Function);
    assert_def(&ex, lang, "Describe", &NodeKind::Function);
    // scm-anchors D4: one method per receiver-alternation branch — zero-def-loss
    // pins (R-DEF-LOSS): a future narrowing of the receiver alternation that
    // DROPS a shape's def turns one of these red.
    assert_def(&ex, lang, "Norm", &NodeKind::Method); // value: (p Point)
    assert_def(&ex, lang, "Len", &NodeKind::Method); // generic: (c Cache[K, V])
    assert_def(&ex, lang, "Get", &NodeKind::Method); // pointer-generic: (c *Cache[K, V])
    assert_def(&ex, lang, "Width", &NodeKind::Method); // parenthesized: (b (Bounds))
    assert_def(&ex, lang, "Height", &NodeKind::Method); // parenthesized-pointer: (b (*Bounds))
    assert_def(&ex, lang, "Cache", &NodeKind::Struct);
    // new kinds
    assert_def(&ex, lang, "Pi", &NodeKind::Constant);
    assert_def(&ex, lang, "MaxPoints", &NodeKind::Constant);
    assert_def(&ex, lang, "Degrees", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "defaultOrigin", &NodeKind::Variable);
    // struct captures (added to go.scm)
    assert_def(&ex, lang, "Point", &NodeKind::Struct);
    assert_def(&ex, lang, "Circle", &NodeKind::Struct);
    assert_def(&ex, lang, "Shape", &NodeKind::Interface);
    // D04-1: struct fields (multi-name `minX, minY float64` emits one Field per name)
    assert_def(&ex, lang, "X", &NodeKind::Field);
    assert_def(&ex, lang, "Y", &NodeKind::Field);
    assert_def(&ex, lang, "Radius", &NodeKind::Field);
    assert_def(&ex, lang, "minX", &NodeKind::Field);
    assert_def(&ex, lang, "minY", &NodeKind::Field);
    // D04-1/D04-10: defined (non-struct) types — deliberate TypeAlias approximation
    assert_def(&ex, lang, "UserID", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "Handler", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "Matrix", &NodeKind::TypeAlias);
    // D04-2 guard: the constrained type: alternation must NOT re-kind structs or
    // interfaces (a catch-all here turns assert_no_conflicting_def_ids red).
    assert_def_floor(&ex, lang, 15);

    // calls
    assert_call(&ex, lang, "Sqrt");
    assert_call(&ex, lang, "Sprintf");
    assert_call(&ex, lang, "Area");

    // imports — captured with surrounding quotes
    assert_import(&ex, lang, "\"fmt\"");
    assert_import(&ex, lang, "\"math\"");
    // import nodes (task C)
    assert_import_node(&ex, lang, "fmt");
    assert_import_node(&ex, lang, "math");
}

// ── Java ──────────────────────────────────────────────────────────────────────

#[test]
fn java_characterization() {
    let lang = "java";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.java", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions: classes + methods
    assert_def(&ex, lang, "DataProcessor", &NodeKind::Class);
    assert_def(&ex, lang, "PipelineRunner", &NodeKind::Class);
    assert_def(&ex, lang, "add", &NodeKind::Method);
    assert_def(&ex, lang, "process", &NodeKind::Method);
    assert_def(&ex, lang, "format", &NodeKind::Method);
    assert_def(&ex, lang, "count", &NodeKind::Method);
    assert_def(&ex, lang, "run", &NodeKind::Method);
    // D04-7: @interface declarations map to the generic interface role; their
    // elements are the annotation's members (methods)
    assert_def(&ex, lang, "Marker", &NodeKind::Interface);
    assert_def(&ex, lang, "value", &NodeKind::Method);
    assert_def(&ex, lang, "priority", &NodeKind::Method);
    // new kinds: fields (Java's `static final` in a class body is field_declaration,
    // not constant_declaration — constant_declaration is interface-only in the grammar)
    assert_def(&ex, lang, "MAX_ITEMS", &NodeKind::Field);
    assert_def(&ex, lang, "VERSION", &NodeKind::Field);
    assert_def(&ex, lang, "items", &NodeKind::Field);
    assert_def(&ex, lang, "processCount", &NodeKind::Field);
    assert_def_floor(&ex, lang, 10);

    // calls
    assert_call(&ex, lang, "add");
    assert_call(&ex, lang, "format");
    assert_call(&ex, lang, "process");

    // imports
    assert_import(&ex, lang, "java.util.ArrayList");
    assert_import(&ex, lang, "java.util.List");
    // import nodes (task C)
    assert_import_node(&ex, lang, "java.util.ArrayList");
    assert_import_node(&ex, lang, "java.util.List");
}

// ── C ─────────────────────────────────────────────────────────────────────────

#[test]
fn c_characterization() {
    let lang = "c";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.c", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // The C query captures every `struct Vector2` usage (type declarations inside
    // function bodies too), so we just pin the ones we care about:
    // top-level struct + functions. We use def_floor for robustness.
    assert_def(&ex, lang, "Vector2", &NodeKind::Struct);
    assert_def(&ex, lang, "Color", &NodeKind::Enum);
    assert_def(&ex, lang, "magnitude", &NodeKind::Function);
    assert_def(&ex, lang, "scale", &NodeKind::Function);
    assert_def(&ex, lang, "main", &NodeKind::Function);
    // new kinds: macros as constants, typedef as type_alias
    assert_def(&ex, lang, "MAX_VECTORS", &NodeKind::Constant);
    assert_def(&ex, lang, "EPSILON", &NodeKind::Constant);
    // §11: the self-naming idiom `typedef struct Vector2 Vector2;` no longer mints a
    // TypeAlias — it shared the Struct's SymbolId and the store re-kinded it. The
    // differently-named and non-tag forms still emit:
    assert_def(&ex, lang, "Vec2", &NodeKind::TypeAlias); // typedef struct Vector2 Vec2
    assert_def(&ex, lang, "uint", &NodeKind::TypeAlias); // typedef unsigned int uint
    let vector2_nodes = ex
        .nodes
        .iter()
        .filter(|n| n.name == "Vector2" && !matches!(n.kind, NodeKind::File))
        .count();
    assert_eq!(
        vector2_nodes, 1,
        "[c] `typedef struct Vector2 Vector2;` must not mint a second Vector2 node"
    );
    assert_def_floor(&ex, lang, 8);

    // calls
    assert_call(&ex, lang, "sqrt");
    assert_call(&ex, lang, "magnitude");
    assert_call(&ex, lang, "scale");
    assert_call(&ex, lang, "printf");

    // includes captured as imports
    assert_import(&ex, lang, "<stdio.h>");
    assert_import(&ex, lang, "<math.h>");
    // import nodes (task C) — angle brackets stripped
    assert_import_node(&ex, lang, "stdio.h");
    assert_import_node(&ex, lang, "math.h");
}

// ── C++ ───────────────────────────────────────────────────────────────────────

#[test]
fn cpp_characterization() {
    let lang = "cpp";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.cpp", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    assert_def(&ex, lang, "Vector3", &NodeKind::Struct);
    assert_def(&ex, lang, "Axis", &NodeKind::Enum);
    assert_def(&ex, lang, "Transform", &NodeKind::Class);
    assert_def(&ex, lang, "dot", &NodeKind::Function);
    assert_def(&ex, lang, "main", &NodeKind::Function);
    // new kinds
    assert_def(&ex, lang, "MAX_SCALE", &NodeKind::Constant); // #define
    assert_def(&ex, lang, "Scalar", &NodeKind::TypeAlias); // using Scalar = double
    assert_def(&ex, lang, "uint32", &NodeKind::TypeAlias); // typedef unsigned int uint32
    // D04-5/D6b: member prototypes (incl. the pure virtual, which parses as
    // field_declaration) and D6e out-of-line member definitions
    assert_def(&ex, lang, "bar", &NodeKind::Method);
    assert_def(&ex, lang, "reset", &NodeKind::Method); // proto + Counter::reset def
    assert_def(&ex, lang, "pure", &NodeKind::Method);
    assert_def(&ex, lang, "m", &NodeKind::Method);
    // D6c: member fields (plain / static / pointer / array declarators)
    assert_def(&ex, lang, "count", &NodeKind::Field);
    assert_def(&ex, lang, "shared", &NodeKind::Field);
    assert_def(&ex, lang, "ptr", &NodeKind::Field);
    assert_def(&ex, lang, "vals", &NodeKind::Field);
    assert_def(&ex, lang, "a", &NodeKind::Field);
    assert_def(&ex, lang, "x", &NodeKind::Field); // Vector3 member
    // D6a negatives: forward declarations and elaborated uses must not mint nodes
    assert_no_def(&ex, lang, "Widget");
    let vector3_nodes = ex
        .nodes
        .iter()
        .filter(|n| n.name == "Vector3" && !matches!(n.kind, NodeKind::File))
        .count();
    assert_eq!(
        vector3_nodes, 1,
        "[cpp] `struct Vector3 *elaborated_use;` must not mint a second Vector3 node"
    );
    assert_def_floor(&ex, lang, 8);

    // calls
    assert_call(&ex, lang, "multiply");
    assert_call(&ex, lang, "apply");
    assert_call(&ex, lang, "dot");

    // includes
    assert_import(&ex, lang, "<string>");
    assert_import(&ex, lang, "<cmath>");
    // import nodes (task C)
    assert_import_node(&ex, lang, "string");
    assert_import_node(&ex, lang, "cmath");
}

// ── C/C++ headers (.h → cpp routing, D04-6/D2) ───────────────────────────────

#[test]
fn h_characterization() {
    // Loaded through extractor_for_extension("h") ON PURPOSE — this is the
    // routing-sensitive path. Under the old c-row ownership the C grammar dropped
    // every class/namespace/method in the header (fixture.h: 12 declared / 3
    // emitted, `class Foo` gone).
    let lang = "cpp";
    let ex = wicked_estate_extract::treesitter::extractor_for_extension("h")
        .expect(".h must have an extractor")
        .extract(&load_fixture("sample.h", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    assert_def(&ex, lang, "Foo", &NodeKind::Class);
    assert_def(&ex, lang, "Bar", &NodeKind::Struct);
    assert_def(&ex, lang, "inlineDef", &NodeKind::Method);
    assert_def(&ex, lang, "definedHere", &NodeKind::Function);
    assert_def(&ex, lang, "bar", &NodeKind::Method);
    assert_def(&ex, lang, "reset", &NodeKind::Method);
    assert_def(&ex, lang, "pure", &NodeKind::Method);
    assert_def(&ex, lang, "m", &NodeKind::Method);
    assert_def(&ex, lang, "count", &NodeKind::Field);
    assert_def(&ex, lang, "shared", &NodeKind::Field);
    assert_def(&ex, lang, "a", &NodeKind::Field);
    // `int freestanding();` — free prototype, emitted since D6d landed under the
    // M4 Option A decision (wicked-estate#140): kind Function, declaration-marked.
    assert_def(&ex, lang, "freestanding", &NodeKind::Function);
    let freestanding = ex
        .nodes
        .iter()
        .find(|n| n.name == "freestanding")
        .expect("freestanding proto node");
    assert!(
        freestanding.is_declaration(),
        "[{lang}] the free prototype record must be a DECLARATION contribution"
    );
    assert_def_floor(&ex, lang, 12);

    assert_import(&ex, lang, "<cstdint>");
    assert_import_node(&ex, lang, "cstdint");
}

// ── C# ────────────────────────────────────────────────────────────────────────

#[test]
fn csharp_characterization() {
    let lang = "csharp";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.cs", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    assert_def(&ex, lang, "IFormatter", &NodeKind::Interface);
    assert_def(&ex, lang, "ProcessingMode", &NodeKind::Enum);
    assert_def(&ex, lang, "TextProcessor", &NodeKind::Class);
    assert_def(&ex, lang, "Pipeline", &NodeKind::Class);
    assert_def(&ex, lang, "Format", &NodeKind::Method);
    assert_def(&ex, lang, "Trim", &NodeKind::Method);
    assert_def(&ex, lang, "Log", &NodeKind::Method);
    assert_def(&ex, lang, "LogCount", &NodeKind::Method);
    assert_def(&ex, lang, "Run", &NodeKind::Method);
    // new kinds: C# const + readonly fields are all field_declaration in the grammar
    assert_def(&ex, lang, "MaxLength", &NodeKind::Field);
    assert_def(&ex, lang, "_log", &NodeKind::Field);
    assert_def(&ex, lang, "_callCount", &NodeKind::Field);
    assert_def(&ex, lang, "_formatter", &NodeKind::Field);
    // D04-8: properties — auto, expression-bodied, and computed forms all emit
    // (property role -> NodeKind::Field)
    assert_def(&ex, lang, "Id", &NodeKind::Field);
    assert_def(&ex, lang, "Name", &NodeKind::Field);
    assert_def(&ex, lang, "Total", &NodeKind::Field);
    assert_def_floor(&ex, lang, 12);

    // calls (invocation expressions captured as method-access or bare identifier)
    assert_call(&ex, lang, "Trim");
    assert_call(&ex, lang, "Log");
    assert_call(&ex, lang, "Format");

    // C# using_directive name: field only exists for the `using X = Y` alias form,
    // not for plain `using System;`. So no import refs or nodes from this fixture.
    // That is correct behavior — plain using is not captured as import ref/node.
}

// ── Ruby ──────────────────────────────────────────────────────────────────────

#[test]
fn ruby_characterization() {
    let lang = "ruby";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.rb", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    // Processing is a Ruby module → NodeKind::Module (correct mapping; old query mapped to Class)
    assert_def(&ex, lang, "Processing", &NodeKind::Module);
    assert_def(&ex, lang, "DataStore", &NodeKind::Class);
    assert_def(&ex, lang, "initialize", &NodeKind::Method);
    assert_def(&ex, lang, "add", &NodeKind::Method);
    assert_def(&ex, lang, "to_json", &NodeKind::Method);
    assert_def(&ex, lang, "normalize", &NodeKind::Method);
    assert_def(&ex, lang, "checksum", &NodeKind::Method);
    assert_def(&ex, lang, "run_store", &NodeKind::Method);
    // new kinds: Ruby UPPER_CASE constants
    assert_def(&ex, lang, "MAX_RECORDS", &NodeKind::Constant);
    assert_def(&ex, lang, "DEFAULT_ENCODING", &NodeKind::Constant);
    // D04-4: setters, operators, alias, alias_method, attr_* — all Methods, all
    // stored under their BARE names (leading `:` stripped at the def-name seam)
    assert_def(&ex, lang, "name=", &NodeKind::Method);
    assert_def(&ex, lang, "[]", &NodeKind::Method);
    assert_def(&ex, lang, "<=>", &NodeKind::Method);
    assert_def(&ex, lang, "==", &NodeKind::Method);
    assert_def(&ex, lang, "new_name", &NodeKind::Method); // alias new_name original
    assert_def(&ex, lang, "other_name", &NodeKind::Method); // alias_method :other_name, :original
    assert_def(&ex, lang, "balance", &NodeKind::Method); // attr_reader
    assert_def(&ex, lang, "label", &NodeKind::Method); // attr_accessor (1st symbol)
    assert_def(&ex, lang, "notes", &NodeKind::Method); // attr_accessor (2nd symbol)
    // FEAS-1 pin: alias_method must capture ONLY the new name — a second capture of
    // the OLD name would mint a spurious Method with the real method's SymbolId and
    // the SAME kind, which assert_no_conflicting_def_ids cannot see (same-kind).
    let original_nodes = ex
        .nodes
        .iter()
        .filter(|n| n.name == "original" && !matches!(n.kind, NodeKind::File))
        .count();
    assert_eq!(
        original_nodes, 1,
        "[ruby] exactly one node named `original` expected (the real def); \
         alias/alias_method must not re-emit the old name"
    );
    assert_def_floor(&ex, lang, 9);

    // calls
    assert_call(&ex, lang, "normalize");
    assert_call(&ex, lang, "generate");
    assert_call(&ex, lang, "hexdigest");
    assert_call(&ex, lang, "checksum");

    // The new ruby.scm captures require's string argument as @import.source,
    // so raw_name = "'json'" (the string literal with quotes).
    // Canonical import nodes have quotes stripped.
    assert_import(&ex, lang, "'json'");
    assert_import_node(&ex, lang, "json");
    assert_import_node(&ex, lang, "digest");
}

// ── Swift ─────────────────────────────────────────────────────────────────────

#[test]
fn swift_characterization() {
    let lang = "swift";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.swift", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // type definitions
    assert_def(&ex, lang, "Point", &NodeKind::Struct);
    assert_def(&ex, lang, "Box", &NodeKind::Class);
    assert_def(&ex, lang, "Container", &NodeKind::Class);
    assert_def(&ex, lang, "Mode", &NodeKind::Enum);
    // D04-3: properties — stored (`var x`), immutable (`let y`), computed
    // (`var sum { … }`), static (`static let origin`), and enum computed (`label`)
    assert_def(&ex, lang, "x", &NodeKind::Field);
    assert_def(&ex, lang, "y", &NodeKind::Field);
    assert_def(&ex, lang, "sum", &NodeKind::Field);
    assert_def(&ex, lang, "origin", &NodeKind::Field);
    assert_def(&ex, lang, "item", &NodeKind::Field);
    assert_def(&ex, lang, "count", &NodeKind::Field);
    assert_def(&ex, lang, "label", &NodeKind::Field);
    // D04-3: init/deinit — the def anchor needs a name capture; the anonymous
    // "init"/"deinit" tokens are the names
    assert_def(&ex, lang, "init", &NodeKind::Method);
    assert_def(&ex, lang, "deinit", &NodeKind::Method);
    // functions (methods inside type bodies stay Function — kind upgrade belongs
    // to enclosing-type identity, method-identity lane)
    assert_def(&ex, lang, "moved", &NodeKind::Function);
    assert_def(&ex, lang, "localScope", &NodeKind::Function);
    // D5 negative: function-local `let hidden` must NOT emit a Field — the
    // property patterns are scoped to class_body/enum_class_body
    assert!(
        !ex.nodes
            .iter()
            .any(|n| n.name == "hidden" && !matches!(n.kind, NodeKind::File)),
        "[swift] function-local `let hidden` must not be captured; got: {:?}",
        ex.nodes
            .iter()
            .filter(|n| n.name == "hidden")
            .map(|n| format!("{:?}", n.kind))
            .collect::<Vec<_>>()
    );
    assert_def_floor(&ex, lang, 15);

    // D04-9: the `extends` cap is now real — `class Container: Box` emits an
    // Extends ref (superclass/protocol conflation is a documented approximation)
    assert!(
        ex.refs
            .iter()
            .any(|r| r.raw_name == "Box" && r.kind == EdgeKind::Extends),
        "[swift] expected Extends ref -> Box; actual extends refs: {:?}",
        ex.refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Extends)
            .map(|r| r.raw_name.as_str())
            .collect::<Vec<_>>()
    );

    // imports
    assert_import(&ex, lang, "Foundation");
    assert_import_node(&ex, lang, "Foundation");
}

// ── Bash ──────────────────────────────────────────────────────────────────────

#[test]
fn bash_characterization() {
    let lang = "bash";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.sh", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // definitions
    assert_def(&ex, lang, "log_message", &NodeKind::Function);
    assert_def(&ex, lang, "validate_input", &NodeKind::Function);
    assert_def(&ex, lang, "process_file", &NodeKind::Function);
    assert_def(&ex, lang, "main", &NodeKind::Function);
    assert_def_floor(&ex, lang, 4);

    // calls
    assert_call(&ex, lang, "log_message");
    assert_call(&ex, lang, "validate_input");
    assert_call(&ex, lang, "process_file");
    assert_call(&ex, lang, "main");

    // bash has no imports — verify none are emitted (no spurious import refs)
    let import_count = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Imports)
        .count();
    assert_eq!(
        import_count, 0,
        "[{lang}] bash should emit 0 import refs, got {import_count}"
    );
}

// ── JSON ──────────────────────────────────────────────────────────────────────

#[test]
fn json_characterization() {
    let lang = "json";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.json", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // JSON: top-level object keys captured as Struct definitions
    assert_def(&ex, lang, "name", &NodeKind::Struct);
    assert_def(&ex, lang, "version", &NodeKind::Struct);
    assert_def(&ex, lang, "description", &NodeKind::Struct);
    assert_def(&ex, lang, "license", &NodeKind::Struct);
    assert_def(&ex, lang, "dependencies", &NodeKind::Struct);
    assert_def_floor(&ex, lang, 5);

    // JSON has no calls or imports
    let call_count = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
    assert_eq!(
        call_count, 0,
        "[{lang}] json should emit 0 call refs, got {call_count}"
    );
}

// ── CSS ───────────────────────────────────────────────────────────────────────

#[test]
fn css_characterization() {
    let lang = "css";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("css/sample.css", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // EG-COR-1 regression pin: pseudo-class selectors are REAL definition names —
    // the leading `:` must survive (a generic def-name colon strip once rewrote
    // `:root` → `root`, changing stored names + SymbolIds fleet-wide).
    assert_def(&ex, lang, ":root", &NodeKind::TypeAlias);
    assert_def(&ex, lang, ":focus-visible", &NodeKind::TypeAlias);
    // Ordinary selectors + @keyframes (Function role) still emit.
    assert_def(&ex, lang, ".btn", &NodeKind::TypeAlias);
    assert_def(&ex, lang, "fade-in", &NodeKind::Function);
    assert_def(&ex, lang, "spin", &NodeKind::Function);
    assert_def_floor(&ex, lang, 10);
}

// ── YAML ──────────────────────────────────────────────────────────────────────

#[test]
fn yaml_characterization() {
    let lang = "yaml";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("sample.yaml", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // YAML: top-level block-mapping keys captured as Struct definitions
    assert_def(&ex, lang, "name", &NodeKind::Struct);
    assert_def(&ex, lang, "version", &NodeKind::Struct);
    assert_def(&ex, lang, "description", &NodeKind::Struct);
    assert_def(&ex, lang, "license", &NodeKind::Struct);
    assert_def(&ex, lang, "settings", &NodeKind::Struct);
    // EG-COR-1 regression pin: a leading-colon symbol key (legacy Rails idiom)
    // is a REAL name — the def-name seam must not strip its `:`.
    assert_def(&ex, lang, ":adapter", &NodeKind::Struct);
    assert_def_floor(&ex, lang, 6);

    // YAML has no calls or imports
    let call_count = ex.refs.iter().filter(|r| r.kind == EdgeKind::Calls).count();
    assert_eq!(
        call_count, 0,
        "[{lang}] yaml should emit 0 call refs, got {call_count}"
    );
}

// ── W6.2 ORM / framework-aware extraction ─────────────────────────────────────

// ── SQLAlchemy ────────────────────────────────────────────────────────────────

#[test]
fn orm_sqlalchemy_models_and_columns() {
    let lang = "python";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("orm_sqlalchemy.py", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // ── Model classes captured as NodeKind::Class ───────────────────────────
    // Base, User, Post — all are class_definition nodes, existing query fires.
    assert_def(&ex, lang, "Base", &NodeKind::Class);
    assert_def(&ex, lang, "User", &NodeKind::Class);
    assert_def(&ex, lang, "Post", &NodeKind::Class);

    // ── SQLAlchemy 1.x Column() fields → NodeKind::Field ───────────────────
    assert_def(&ex, lang, "id", &NodeKind::Field);
    assert_def(&ex, lang, "username", &NodeKind::Field);
    assert_def(&ex, lang, "email", &NodeKind::Field);
    assert_def(&ex, lang, "bio", &NodeKind::Field);
    // relationship() also captured as a field (it's an ORM-level attribute)
    assert_def(&ex, lang, "posts", &NodeKind::Field);

    // ── SQLAlchemy 2.0 mapped_column() fields → NodeKind::Field ────────────
    // Note: `id` and `email` appear in both User and Post; assert_def uses
    // `any()` so it fires on the first match — the assertion just proves
    // at least one node of that name+kind exists, which is correct.
    assert_def(&ex, lang, "title", &NodeKind::Field);
    assert_def(&ex, lang, "body", &NodeKind::Field);
    assert_def(&ex, lang, "author_id", &NodeKind::Field);
    // relationship() in Post also captured
    assert_def(&ex, lang, "author", &NodeKind::Field);

    // ── Module-level constants still captured ──────────────────────────────
    assert_def(&ex, lang, "SCHEMA_VERSION", &NodeKind::Constant);
    assert_def(&ex, lang, "DEFAULT_POOL", &NodeKind::Constant);

    // ── Free function still captured ───────────────────────────────────────
    assert_def(&ex, lang, "create_tables", &NodeKind::Function);

    // ── Minimum total definitions ─────────────────────────────────────────
    // 3 classes + 10 fields + 1 function + 2 constants + 1 file = at least 16
    assert_def_floor(&ex, lang, 16);
}

// ── Django ORM ────────────────────────────────────────────────────────────────

#[test]
fn orm_django_models_and_fields() {
    let lang = "python";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("orm_django.py", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // ── Model classes captured as NodeKind::Class ───────────────────────────
    assert_def(&ex, lang, "Category", &NodeKind::Class);
    assert_def(&ex, lang, "Article", &NodeKind::Class);

    // ── Category fields → NodeKind::Field ──────────────────────────────────
    assert_def(&ex, lang, "name", &NodeKind::Field);
    assert_def(&ex, lang, "slug", &NodeKind::Field);
    assert_def(&ex, lang, "description", &NodeKind::Field);

    // ── Article fields → NodeKind::Field ───────────────────────────────────
    assert_def(&ex, lang, "title", &NodeKind::Field);
    assert_def(&ex, lang, "body", &NodeKind::Field);
    assert_def(&ex, lang, "pub_date", &NodeKind::Field);
    assert_def(&ex, lang, "updated_at", &NodeKind::Field);
    assert_def(&ex, lang, "views", &NodeKind::Field);
    assert_def(&ex, lang, "published", &NodeKind::Field);
    assert_def(&ex, lang, "category", &NodeKind::Field);
    assert_def(&ex, lang, "author", &NodeKind::Field);

    // ── Module-level constants still captured ──────────────────────────────
    assert_def(&ex, lang, "MAX_TITLE_LEN", &NodeKind::Constant);
    assert_def(&ex, lang, "DEFAULT_STATUS", &NodeKind::Constant);

    // ── Free function still captured ───────────────────────────────────────
    assert_def(&ex, lang, "get_published", &NodeKind::Function);

    // ── Minimum total definitions ─────────────────────────────────────────
    // 2 classes + 11 fields + 1 function + 2 constants + 1 file = at least 17
    assert_def_floor(&ex, lang, 17);

    // ── Negative: class-level non-ORM things (Meta subclass body) are NOT
    // emitted as Field nodes — they're class_definition nodes.
    // This is correct: we do not over-capture plain assignments.
}

// ── TypeORM ───────────────────────────────────────────────────────────────────

#[test]
fn orm_typeorm_entities_and_columns() {
    let lang = "typescript";
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&load_fixture("orm_typeorm.ts", lang))
        .expect("extraction must succeed");
    assert_no_conflicting_def_ids(&ex, lang);

    // ── @Entity decorated classes → NodeKind::Class ─────────────────────────
    assert_def(&ex, lang, "User", &NodeKind::Class);
    assert_def(&ex, lang, "Post", &NodeKind::Class);

    // ── @Column / @PrimaryGeneratedColumn / @CreateDateColumn / @UpdateDateColumn
    // / @OneToMany / @ManyToOne decorated properties → NodeKind::Field ───────
    // User fields
    assert_def(&ex, lang, "id", &NodeKind::Field);
    assert_def(&ex, lang, "username", &NodeKind::Field);
    assert_def(&ex, lang, "email", &NodeKind::Field);
    assert_def(&ex, lang, "role", &NodeKind::Field);
    assert_def(&ex, lang, "createdAt", &NodeKind::Field);
    assert_def(&ex, lang, "updatedAt", &NodeKind::Field);
    assert_def(&ex, lang, "posts", &NodeKind::Field);
    // Post fields
    assert_def(&ex, lang, "title", &NodeKind::Field);
    assert_def(&ex, lang, "body", &NodeKind::Field);
    assert_def(&ex, lang, "published", &NodeKind::Field);
    assert_def(&ex, lang, "author", &NodeKind::Field);

    // ── Module-level constant still captured ──────────────────────────────
    assert_def(&ex, lang, "DEFAULT_ROLE", &NodeKind::Constant);

    // ── Free function still captured ──────────────────────────────────────
    assert_def(&ex, lang, "findPublishedPosts", &NodeKind::Function);

    // ── Minimum total definitions ─────────────────────────────────────────
    // 2 entity classes + 11 fields + 1 function + 1 constant + 1 file = at least 16
    assert_def_floor(&ex, lang, 16);
}

// ── IaC: CloudFormation ───────────────────────────────────────────────────────

#[test]
fn cloudformation_extracts_resources_as_other_resource_nodes() {
    let lang = "cloudformation";
    let sf = load_fixture("sample.cfn.yaml", lang);
    let ex = IaCExtractor::cloudformation()
        .extract(&sf)
        .expect("CFN extraction must succeed");

    // Every resource must be NodeKind::Other("resource").
    let resource_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
        .collect();

    assert_eq!(
        resource_nodes.len(),
        2,
        "expected 2 resource nodes (AppBucket, AppQueue); got {:?}",
        resource_nodes
            .iter()
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>()
    );

    // Names must be the logical IDs.
    let names: Vec<&str> = resource_nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"AppBucket"),
        "AppBucket not found; got {names:?}"
    );
    assert!(
        names.contains(&"AppQueue"),
        "AppQueue not found; got {names:?}"
    );

    // Type signatures must be set.
    let bucket = resource_nodes
        .iter()
        .find(|n| n.name == "AppBucket")
        .unwrap();
    assert_eq!(
        bucket.signature.as_deref(),
        Some("AWS::S3::Bucket"),
        "AppBucket signature should be AWS::S3::Bucket"
    );
    let queue = resource_nodes
        .iter()
        .find(|n| n.name == "AppQueue")
        .unwrap();
    assert_eq!(
        queue.signature.as_deref(),
        Some("AWS::SQS::Queue"),
        "AppQueue signature should be AWS::SQS::Queue"
    );

    // Contains edges: one per resource.
    let contains: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .collect();
    assert_eq!(
        contains.len(),
        2,
        "expected 2 Contains edges; got {}",
        contains.len()
    );

    // File node exists.
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::File),
        "file node missing"
    );

    // Best-effort: !Ref AppBucket appears somewhere in the fixture; at least one Calls ref.
    let ref_names: Vec<&str> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        !ref_names.is_empty(),
        "expected at least one !Ref captured as Calls; got none"
    );
    assert!(
        ref_names.contains(&"AppBucket"),
        "!Ref AppBucket not captured; refs={ref_names:?}"
    );
}

#[test]
fn cloudformation_for_language_api() {
    // IaCExtractor::for_language must return Some for "cloudformation".
    let ex = IaCExtractor::for_language("cloudformation");
    assert!(ex.is_some(), "for_language('cloudformation') returned None");

    // It must return None for unknown names.
    let unknown = IaCExtractor::for_language("cobol");
    assert!(unknown.is_none(), "for_language('cobol') should be None");
}

// ── IaC: Kubernetes ───────────────────────────────────────────────────────────

#[test]
fn kubernetes_extracts_resources_from_multidoc_manifest() {
    let lang = "kubernetes";
    let sf = load_fixture("sample.k8s.yaml", lang);
    let ex = IaCExtractor::kubernetes()
        .extract(&sf)
        .expect("k8s extraction must succeed");

    // Two documents → two resource nodes.
    let resource_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
        .collect();

    assert_eq!(
        resource_nodes.len(),
        2,
        "expected 2 resource nodes (my-deployment, my-service); got {:?}",
        resource_nodes
            .iter()
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>()
    );

    let names: Vec<&str> = resource_nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"my-deployment"),
        "my-deployment not found; got {names:?}"
    );
    assert!(
        names.contains(&"my-service"),
        "my-service not found; got {names:?}"
    );

    // Signatures must be the k8s kind strings.
    let deployment = resource_nodes
        .iter()
        .find(|n| n.name == "my-deployment")
        .unwrap();
    assert_eq!(
        deployment.signature.as_deref(),
        Some("Deployment"),
        "my-deployment signature should be Deployment"
    );
    let service = resource_nodes
        .iter()
        .find(|n| n.name == "my-service")
        .unwrap();
    assert_eq!(
        service.signature.as_deref(),
        Some("Service"),
        "my-service signature should be Service"
    );

    // Contains edges: one per resource.
    let contains: Vec<_> = ex
        .local_edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .collect();
    assert_eq!(
        contains.len(),
        2,
        "expected 2 Contains edges; got {}",
        contains.len()
    );

    // File node exists.
    assert!(
        ex.nodes.iter().any(|n| n.kind == NodeKind::File),
        "file node missing"
    );

    // No import refs — k8s has no equivalent.
    assert!(
        ex.refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .count()
            == 0,
        "k8s extraction should not emit import refs"
    );
}

#[test]
fn kubernetes_for_language_api() {
    let ex = IaCExtractor::for_language("kubernetes");
    assert!(ex.is_some(), "for_language('kubernetes') returned None");
}

// ── IaC: inline unit tests (no fixture file needed) ──────────────────────────

#[test]
fn cloudformation_inline_minimal() {
    let cfn = concat!(
        "Resources:\n",
        "  VPC:\n",
        "    Type: AWS::EC2::VPC\n",
        "  Subnet:\n",
        "    Type: AWS::EC2::Subnet\n",
        "    Properties:\n",
        "      VpcId: !Ref VPC\n",
    );
    let sf = SourceFile {
        path: "infra/vpc.yaml".to_string(),
        language: Language::new("cloudformation"),
        text: cfn.to_string(),
    };
    let ex = IaCExtractor::cloudformation()
        .extract(&sf)
        .expect("inline CFN must succeed");
    let resource_names: Vec<&str> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        resource_names.contains(&"VPC"),
        "VPC resource missing; got {resource_names:?}"
    );
    assert!(
        resource_names.contains(&"Subnet"),
        "Subnet resource missing; got {resource_names:?}"
    );
    // !Ref VPC should appear as a Calls ref
    let ref_vpc = ex
        .refs
        .iter()
        .any(|r| r.raw_name == "VPC" && r.kind == EdgeKind::Calls);
    assert!(ref_vpc, "!Ref VPC not captured as Calls ref");
}

#[test]
fn kubernetes_inline_single_document() {
    let k8s = concat!(
        "apiVersion: v1\n",
        "kind: ConfigMap\n",
        "metadata:\n",
        "  name: app-config\n",
        "data:\n",
        "  key: value\n",
    );
    let sf = SourceFile {
        path: "k8s/config.yaml".to_string(),
        language: Language::new("kubernetes"),
        text: k8s.to_string(),
    };
    let ex = IaCExtractor::kubernetes()
        .extract(&sf)
        .expect("inline k8s must succeed");
    let resource_names: Vec<&str> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        resource_names,
        vec!["app-config"],
        "expected [app-config]; got {resource_names:?}"
    );
    let cm = ex.nodes.iter().find(|n| n.name == "app-config").unwrap();
    assert_eq!(
        cm.signature.as_deref(),
        Some("ConfigMap"),
        "signature should be ConfigMap"
    );
}

#[test]
fn yaml_without_resources_emits_no_cfn_resources() {
    // Generic YAML (no Resources: block) must not emit resource nodes when run
    // through the CFN extractor.
    let generic = "name: foo\nversion: 1\n";
    let sf = SourceFile {
        path: "config.yaml".to_string(),
        language: Language::new("cloudformation"),
        text: generic.to_string(),
    };
    let ex = IaCExtractor::cloudformation()
        .extract(&sf)
        .expect("must not error");
    let resources: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Other("resource".to_string()))
        .collect();
    assert!(
        resources.is_empty(),
        "non-CFN YAML must not produce resource nodes; got {resources:?}"
    );
}

// ── Cross-kind SymbolId collisions — re-pinned after symbol-id scheme 2 ────────
//
// These two tests were authored by the extraction-gaps lane as executable
// known-defect pins (asserting the then-colliding ids). The method-identity
// lane's type-nested identity (scheme 2) landed and separated the ids that
// have an ENCLOSING Type anchor, so — per the pins' own flip instructions —
// they now assert the fix where it applies, and keep pinning the residual
// file-scope collision that scheme 2 deliberately does not own (the D6d
// header/impl + free-function identity seam, program follow-up).

/// EG-COR-2 (RESOLVED by scheme 2): a package-level `const X` and a struct
/// field `X` used to emit the SAME SymbolId with CONFLICTING kinds (Constant
/// vs Field), and the store upsert silently re-kinded one. Type-nested
/// identity now nests the field under its enclosing struct's Type anchor
/// (`…/X.` vs `…/S#X.`), so the two definitions mint DISTINCT ids.
#[test]
fn go_const_vs_struct_field_symbolids_are_distinct() {
    let lang = "go";
    let sf = SourceFile {
        path: "probe_collide.go".to_string(),
        language: Language::new(lang),
        text: "package p\nconst X = 1\ntype S struct { X int }\n".to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let xs: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "X")
        .collect();
    assert_eq!(
        xs.len(),
        2,
        "[{lang}] expected const X + field X, got {xs:?}"
    );
    let kinds: std::collections::HashSet<String> =
        xs.iter().map(|n| format!("{:?}", n.kind)).collect();
    assert_eq!(
        kinds,
        ["Constant".to_string(), "Field".to_string()].into(),
        "[{lang}] expected Constant + Field emissions"
    );
    // The fix (symbol-id scheme 2): distinct ids — the field nests under S#.
    assert_ne!(
        xs[0].symbol, xs[1].symbol,
        "[{lang}] REGRESSION: const X and field X share one SymbolId again — \
         scheme-2 type-nested identity stopped separating them"
    );
}

/// EG-R1-1, RESOLVED in two steps: scheme 2 nested the in-class prototype
/// under `Foo#`; the scm-anchors D8 qualifier owner now nests the out-of-line
/// `void Foo::reset()` definition under `Foo#` too — so the previously-pinned
/// residual (out-of-line def sharing the free function's `…/reset().`) is
/// FLIPPED to distinct ids per the pin's own instruction. The proto and the
/// out-of-line def now share ONE id — asserted NEUTRALLY: single-id member
/// semantics pending the program's M4 header/impl identity decision (see
/// `cpp_member_proto_def_cross_file_single_id_hazard`); if that decision is
/// distinct-decl identity, the equality assertion flips WITH the decision.
/// Template-scoped members anchor under the bare type via the template_type
/// branch; decltype-scoped qualifiers keep their def OWNERLESS (R-DEF-LOSS —
/// a def may lose its owner, never its extraction).
#[test]
fn cpp_out_of_line_member_vs_free_function_collision_known_defect() {
    let lang = "cpp";
    let text = "class Foo { public: void reset(); };\nvoid Foo::reset() {}\nvoid reset() {}\n\
                template <typename T> class Bar { public: void tinit(); };\n\
                template <typename T> void Bar<T>::tinit() {}\n\
                struct Q { int q; };\nQ qv;\nvoid decltype(qv)::weird() {}\n";
    let sf = SourceFile {
        path: "probe_collide.cpp".to_string(),
        language: Language::new(lang),
        text: text.to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let resets: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "reset")
        .collect();
    assert_eq!(
        resets.len(),
        3,
        "[{lang}] expected proto + out-of-line def + free fn, got {resets:?}"
    );
    let kinds: Vec<String> = resets.iter().map(|n| format!("{:?}", n.kind)).collect();
    assert!(
        kinds.iter().filter(|k| *k == "Method").count() == 2
            && kinds.iter().filter(|k| *k == "Function").count() == 1,
        "[{lang}] expected 2x Method + 1x Function, got {kinds:?}"
    );
    let free_id = resets
        .iter()
        .find(|n| format!("{:?}", n.kind) == "Function")
        .map(|n| n.symbol.clone())
        .expect("free fn node");
    // With the D8 owner, BOTH the in-class proto and the out-of-line def nest
    // under Foo# — two nodes, one id shape.
    let nested: Vec<_> = resets
        .iter()
        .filter(|n| n.symbol.as_str().contains("Foo#"))
        .collect();
    assert_eq!(
        nested.len(),
        2,
        "[{lang}] proto AND out-of-line def must both nest under Foo#; got {resets:?}"
    );
    // Locate the out-of-line def by SPAN (both Foo# nodes are Methods now).
    let out_of_line_off = text.find("void Foo::reset").unwrap() as u32;
    let out_of_line = resets
        .iter()
        .find(|n| n.location.span.start_byte == out_of_line_off)
        .expect("out-of-line member def node");
    let proto = nested
        .iter()
        .find(|n| n.location.span.start_byte != out_of_line_off)
        .expect("in-class proto node");
    // Single-id assertion — the M4 decision recorded Option A (one logical
    // symbol, wicked-estate#152), so this is now the correct PERMANENT
    // assertion, no longer a pending-decision hedge: the proto and the
    // out-of-line def are one symbol; the store's multi-file contribution
    // table carries the per-file provenance.
    assert_eq!(
        out_of_line.symbol, proto.symbol,
        "[{lang}] proto/def must mint ONE id (M4 Option A) — distinct decl \
         identity would be an id-shape (scheme) change"
    );
    // THE FLIP (per the original pin's instruction): the out-of-line member no
    // longer shares the free function's id.
    assert_ne!(
        out_of_line.symbol, free_id,
        "[{lang}] REGRESSION: Foo::reset collapsed into the free reset's id again"
    );
    // Template-scoped out-of-line member anchors under the bare type name.
    let tinits: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "tinit")
        .collect();
    assert_eq!(
        tinits.len(),
        2,
        "[{lang}] proto + out-of-line tinit expected"
    );
    for n in &tinits {
        assert_eq!(
            n.symbol.as_str(),
            "ts-cpp . . . probe_collide/Bar#tinit().",
            "[{lang}] Bar<T>::tinit must anchor under Bar# (template_type branch)"
        );
    }
    // R-DEF-LOSS: a decltype-scoped qualifier keeps its DEF, ownerless (flat).
    let weird = ex
        .nodes
        .iter()
        .find(|n| !matches!(n.kind, NodeKind::File) && n.name == "weird")
        .expect("decltype-scoped def must still extract (zero-def-loss)");
    assert_eq!(
        weird.symbol.as_str(),
        "ts-cpp . . . probe_collide/weird().",
        "[{lang}] decltype scope degrades to an ownerless module-flat def"
    );
}

/// scm-anchors D8 / M4 CONVENTION PIN (formerly HAZARD PIN F13, retired per its
/// own flip instruction): the M4 decision recorded **Option A — one logical
/// symbol** (scratch/proposals/ESTATE-M4-DECISION-BRIEF.md, wicked-estate#152).
/// A member declared in `foo.h` (D6b in-class prototype) and defined
/// out-of-line in `foo.cpp` minting ONE SymbolId across TWO files —
/// `module_path` strips one extension, so both files share module `foo` — is
/// now the RECORDED CONVENTION, not a hazard: the store keeps per-(symbol,
/// file) contributions (`node_files`), derives a definition-preferred primary
/// instead of last-write-wins, and `remove_file` re-homes a node whose other
/// contributing file survives. The former store consequences (file flap /
/// cross-file delete / digest-skip data loss) are pinned FIXED by the store
/// conformance suite this pin retired into:
/// `wicked_estate_core::conformance::multi_file_contribution_suite`, run
/// against every shipped backend (Mem/Sqlite/Postgres). This test keeps the
/// extract-level half of the contract: both files MUST keep minting the same
/// id — distinct decl identity would be an id-shape (scheme) change.
#[test]
fn cpp_member_proto_def_cross_file_single_id_hazard() {
    let lang = "cpp";
    let header = SourceFile {
        path: "foo.h".to_string(),
        language: Language::new(lang),
        text: "class Foo { public: void reset(); };\n".to_string(),
    };
    let src = SourceFile {
        path: "foo.cpp".to_string(),
        language: Language::new(lang),
        text: "void Foo::reset() {}\n".to_string(),
    };
    let extractor = TreeSitterExtractor::for_language(lang).unwrap();
    let hx = extractor.extract(&header).expect("header extraction");
    let cx = extractor.extract(&src).expect("cpp extraction");
    let proto = hx
        .nodes
        .iter()
        .find(|n| !matches!(n.kind, NodeKind::File) && n.name == "reset")
        .expect("header proto node");
    let def = cx
        .nodes
        .iter()
        .find(|n| !matches!(n.kind, NodeKind::File) && n.name == "reset")
        .expect("out-of-line def node");
    assert_eq!(
        proto.symbol, def.symbol,
        "[{lang}] M4 CONVENTION BROKEN: the .h proto and .cpp def must mint ONE \
         cross-file id (Option A, wicked-estate#152) — distinct decl identity \
         is an id-shape change requiring a scheme bump, not a silent flip"
    );
    assert_eq!(
        proto.symbol.as_str(),
        "ts-cpp . . . foo/Foo#reset().",
        "[{lang}] both files share module `foo` (one extension stripped)"
    );
}

/// Review round 2 (R2-COR-1) — the D8 qualifier ambiguity, NAMESPACE direction,
/// FLIPPED per its own M4-gated instruction: the M4 decision recorded
/// **Option A — one logical symbol** (ADR-002 third amendment,
/// wicked-estate#152/#140), so this pin now asserts the RECORDED CONVENTION:
/// one id, with the kind reconciled deterministically by the store.
///
/// The grammar ambiguity itself is unchanged and unfixable at query level —
/// `qualified_identifier.scope` parses class and namespace qualifiers
/// identically (`namespace_identifier`), so `void ns::helper(int) {}` at file
/// scope still mints kind Method while the in-namespace `void helper() {}`
/// definition mints kind Function, and (new since D6d landed) the in-namespace
/// prototype `void helper(int);` mints a THIRD record: kind Function, marked as
/// a DECLARATION contribution (`is_declaration` metadata). All three share
/// `<module>/ns#helper().` — that single id is the convention, not a defect.
/// The store's contribution table derives ONE deterministic primary kind from
/// the preferred contribution (definition before declaration, lexicographic
/// file tiebreak; within one file, that file's extraction stream), replacing
/// the last-write-wins re-kind flap the original pin recorded — pinned
/// store-side by `wicked_estate_core::conformance::multi_file_contribution_suite`.
///
/// STILL-OPEN residual, named so it is not mistaken for resolved: the two
/// `helper`s are DIFFERENT OVERLOADS (`helper()` vs `helper(int)`) collapsing
/// into one id because the overload `disambiguator` stays `None` — a separately
/// pinned scheme change (`identity_disambiguator_is_none`, ADR-002 §Accepted
/// residuals). M4/Option A did NOT fix overload identity.
#[test]
fn cpp_namespace_qualified_free_fn_cross_kind_collision_known_defect() {
    let lang = "cpp";
    let sf = SourceFile {
        path: "probe_ns_free.cpp".to_string(),
        language: Language::new(lang),
        text: "namespace ns {\nvoid helper() {}\nvoid helper(int);\n}\n\
               void ns::helper(int x) {}\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let helpers: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "helper")
        .collect();
    assert_eq!(
        helpers.len(),
        3,
        "[{lang}] expected in-namespace def + D6d free prototype + qualified \
         free def; got {helpers:?}"
    );
    // One logical symbol (M4 Option A): every record shares the id.
    for n in &helpers {
        assert_eq!(
            n.symbol.as_str(),
            "ts-cpp . . . probe_ns_free/ns#helper().",
            "[{lang}] M4 CONVENTION BROKEN: all three records must mint ONE id \
             (Option A) — distinct identity is an id-shape (scheme) change"
        );
    }
    // Exactly one record is the declaration contribution — the D6d prototype.
    let decls: Vec<_> = helpers.iter().filter(|n| n.is_declaration()).collect();
    assert_eq!(
        decls.len(),
        1,
        "[{lang}] exactly the prototype record carries is_declaration"
    );
    assert_eq!(
        format!("{:?}", decls[0].kind),
        "Function",
        "[{lang}] the free prototype mints kind Function"
    );
    // The raw extraction stream keeps the cross-kind DEFINITION pair — the
    // grammar cannot separate class from namespace qualifiers; the store's
    // preferred-contribution rule (not this stream) decides the primary kind.
    let def_kinds: std::collections::HashSet<String> = helpers
        .iter()
        .filter(|n| !n.is_declaration())
        .map(|n| format!("{:?}", n.kind))
        .collect();
    assert_eq!(
        def_kinds,
        ["Function".to_string(), "Method".to_string()].into(),
        "[{lang}] RAW-STREAM SHAPE CHANGED: the definition records still carry \
         the Function/Method pair (grammar ambiguity) — if the grammar or the \
         qualifier capture now disambiguates, update ADR-002's convention record"
    );
}

// ── D6d: free-function prototype emission (wicked-estate#140, M4 Option A) ────
//
// The per-parent anchored pattern set from docs/recon/extraction-gaps.md §D6(d):
// translation_unit / preproc_ifdef / preproc_if / declaration_list /
// template_declaration. The anchoring is the false-positive guard — the
// review's naive `(declaration (function_declarator (identifier)))` also fired
// on body-local prototypes and most-vexing-parse object declarations, both of
// which sit under compound_statement and match NO per-parent pattern.

/// The extraction-gaps probe, pinned: the review's translation_unit-only anchor
/// captured 0 prototypes in include-guarded headers (everything sits under
/// preproc_ifdef); the per-parent set captures 3/3 — plus the braced
/// `extern "C"` block (declaration_list) and `#if` blocks. Every prototype:
/// kind Function, DECLARATION-marked, id identical to what the definition mints.
#[test]
fn cpp_free_proto_include_guarded_header_per_parent_anchors() {
    let lang = "cpp";
    let sf = SourceFile {
        path: "api.h".to_string(),
        language: Language::new(lang),
        text: "#ifndef API_H\n#define API_H\n\
               int alpha(void);\n\
               namespace ns { int beta(); }\n\
               template <typename T> T gamma(T v);\n\
               #endif\n\
               extern \"C\" {\nvoid c_api(void);\n}\n\
               #if defined(FEATURE_X)\nint under_if(int);\n#endif\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    // The probe's 3/3 (guarded TU scope, namespace body, template) + the two
    // extra parents.
    for (name, id) in [
        ("alpha", "ts-cpp . . . api/alpha()."),
        ("beta", "ts-cpp . . . api/ns#beta()."),
        ("gamma", "ts-cpp . . . api/gamma()."),
        ("c_api", "ts-cpp . . . api/c_api()."),
        ("under_if", "ts-cpp . . . api/under_if()."),
    ] {
        let n = ex
            .nodes
            .iter()
            .find(|n| !matches!(n.kind, NodeKind::File) && n.name == name)
            .unwrap_or_else(|| panic!("[{lang}] free prototype `{name}` must emit a node"));
        assert_eq!(
            n.symbol.as_str(),
            id,
            "[{lang}] `{name}` must mint the SAME id its definition would (Option A join)"
        );
        assert_eq!(
            format!("{:?}", n.kind),
            "Function",
            "[{lang}] `{name}` mints kind Function"
        );
        assert!(
            n.is_declaration(),
            "[{lang}] `{name}` must be a DECLARATION contribution (is_declaration metadata)"
        );
    }
}

/// The adversarial-review false-positive guards (extra.cpp, findings.json D04
/// attack): the naive pattern fired on a body-local prototype
/// (`int localProto(int);`) and a most-vexing-parse object declaration
/// (`Foo f(Foo());`) — both inside a function body. The per-parent anchoring
/// excludes compound_statement, so NEITHER emits: 0 false positives on the
/// probe corpus. (A most-vexing-parse declaration at TU scope still matches —
/// an ACCEPTED residual recorded in ADR-002 and cpp.scm: per [dcl.ambig.res]
/// it genuinely IS a function declaration.)
#[test]
fn cpp_free_proto_body_local_and_most_vexing_parse_guarded() {
    let lang = "cpp";
    let sf = SourceFile {
        path: "extra.cpp".to_string(),
        language: Language::new(lang),
        text: "class Foo {\npublic:\n  Foo() = default;\n  virtual void pure() = 0;\n};\n\
               void body() {\n  int localProto(int);\n  Foo f(Foo());\n}\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    // Guard 1: the body-local prototype must NOT emit.
    assert_no_def(&ex, lang, "localProto");
    // Guard 2: the most-vexing-parse object declaration must NOT emit.
    assert_no_def(&ex, lang, "f");
    // The genuine defs around them still extract (the guard is anchoring, not
    // a blanket declaration suppression).
    assert_def(&ex, lang, "Foo", &NodeKind::Class);
    assert_def(&ex, lang, "body", &NodeKind::Function);
    assert_def(&ex, lang, "pure", &NodeKind::Method);
}

/// S11-style cross-file join (M4 Option A, the D6d identity contract): a free
/// prototype in `api.h` and its definition in `api.cpp` mint ONE SymbolId —
/// the proto JOINS the definition's existing id (zero id churn; the store's
/// contribution table keeps per-file provenance and prefers the definition as
/// primary — proven through the real index path in
/// crates/wicked-estate/tests/free_proto_emission.rs). The prototype record is
/// declaration-marked; the definition record is not.
#[test]
fn cpp_free_proto_def_cross_file_single_id_join() {
    let lang = "cpp";
    let header = SourceFile {
        path: "api.h".to_string(),
        language: Language::new(lang),
        text: "#ifndef API_H\n#define API_H\nint compute(int a, int b);\n#endif\n".to_string(),
    };
    let src = SourceFile {
        path: "api.cpp".to_string(),
        language: Language::new(lang),
        text: "int compute(int a, int b) { return a + b; }\n".to_string(),
    };
    let extractor = TreeSitterExtractor::for_language(lang).unwrap();
    let hx = extractor.extract(&header).expect("header extraction");
    let cx = extractor.extract(&src).expect("cpp extraction");
    let proto = hx
        .nodes
        .iter()
        .find(|n| !matches!(n.kind, NodeKind::File) && n.name == "compute")
        .expect("header proto node");
    let def = cx
        .nodes
        .iter()
        .find(|n| !matches!(n.kind, NodeKind::File) && n.name == "compute")
        .expect("definition node");
    assert_eq!(
        proto.symbol, def.symbol,
        "[{lang}] proto and def must mint ONE id (module strips one extension)"
    );
    assert_eq!(
        proto.symbol.as_str(),
        "ts-cpp . . . api/compute().",
        "[{lang}] both files share module `api`"
    );
    assert!(
        proto.is_declaration(),
        "[{lang}] the header record is the DECLARATION contribution"
    );
    assert!(
        !def.is_declaration(),
        "[{lang}] the impl record is the DEFINITION contribution (primary)"
    );
    assert_eq!(
        format!("{:?}", proto.kind),
        "Function",
        "[{lang}] proto kind Function — no cross-kind flap for free functions"
    );
}

/// scm-anchors D3 (scheme 3): impl-block methods nest under the impl's `type:`
/// name via the NON-EMITTING `@code_struct.anchor` — two impls' same-named
/// methods mint DISTINCT ids, and every alternation branch (plain, generic,
/// scoped, scoped-generic) anchors under the bare type name.
#[test]
fn rust_impl_methods_nest_under_type() {
    let lang = "rust";
    let sf = SourceFile {
        path: "probe_impl.rs".to_string(),
        language: Language::new(lang),
        text: "struct A;\nstruct B;\n\
               impl A { fn save(&self) {} }\n\
               impl B { fn save(&self) {} }\n\
               trait Tr { fn draw(&self); }\n\
               struct H<T>(T);\n\
               impl<T> H<T> { fn get(&self) {} }\n\
               impl Tr for crate::ext::W { fn draw(&self) {} }\n\
               impl<T> Tr for crate::ext::G<T> { fn draw(&self) {} }\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let all: Vec<&str> = ex.nodes.iter().map(|n| n.symbol.as_str()).collect();
    // Every anchor branch nests under the bare type name.
    for sym in [
        "ts-rust . . . probe_impl/A#save().", // type_identifier
        "ts-rust . . . probe_impl/B#save().", // type_identifier (2nd impl)
        "ts-rust . . . probe_impl/H#get().",  // generic_type
        "ts-rust . . . probe_impl/W#draw().", // scoped_type_identifier
        "ts-rust . . . probe_impl/G#draw().", // generic_type over scoped_type_identifier
    ] {
        assert!(
            all.contains(&sym),
            "[{lang}] expected {sym}; symbols = {all:?}"
        );
    }
    // The anchor is non-emitting: no phantom node named after a type at an impl
    // range — exactly one node each for A and B (the struct defs).
    for ty in ["A", "B"] {
        let n = ex
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == ty)
            .count();
        assert_eq!(
            n, 1,
            "[{lang}] impl anchor must not mint a second `{ty}` node"
        );
    }
}

/// scm-anchors D3 residual, pinned: two trait impls on ONE type still collide —
/// `impl Ta for Foo` and `impl Tb for Foo` both anchor under Foo (the `trait:`
/// field is deliberately not captured), so both `fmt` defs mint `Foo#fmt().`.
/// Distinguishing them needs a trait-qualified descriptor or a disambiguator
/// (`identity_disambiguator_is_none` pins None) — a program-level identity
/// convention, not a query edit. When one is recorded, flip this to assert
/// DISTINCT ids per trait impl.
#[test]
fn rust_same_type_trait_impls_collision_known_defect() {
    let lang = "rust";
    let sf = SourceFile {
        path: "probe_trait_impls.rs".to_string(),
        language: Language::new(lang),
        text: "struct Foo;\ntrait Ta { fn fmt(&self); }\ntrait Tb { fn fmt(&self); }\n\
               impl Ta for Foo { fn fmt(&self) {} }\n\
               impl Tb for Foo { fn fmt(&self) {} }\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let fmts: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "fmt")
        .collect();
    assert_eq!(
        fmts.len(),
        2,
        "[{lang}] two fmt defs expected; got {fmts:?}"
    );
    assert_eq!(
        fmts[0].symbol, fmts[1].symbol,
        "[{lang}] KNOWN DEFECT RESOLVED? two trait impls' same-named methods no \
         longer share Foo#fmt(). — a trait-qualified identity convention landed. \
         Flip this pin to assert distinct ids."
    );
    assert_eq!(
        fmts[0].symbol.as_str(),
        "ts-rust . . . probe_trait_impls/Foo#fmt().",
        "[{lang}] the merged id is the type-nested one"
    );
}

/// scm-anchors D4 (scheme 3): Go receiver methods nest under the receiver's
/// base type name via `@code_method.owner` — two types' same-named methods
/// mint DISTINCT ids, and every receiver-alternation branch resolves to the
/// same bare type name (value and pointer receivers share one shape).
#[test]
fn go_receiver_methods_nest_under_receiver_type() {
    let lang = "go";
    let sf = SourceFile {
        path: "probe_recv.go".to_string(),
        language: Language::new(lang),
        text: "package p\n\
               type A struct{}\ntype B struct{}\ntype C[K any] struct{}\n\
               func (a A) M()    {}\n\
               func (b B) M()    {}\n\
               func (a *A) P()   {}\n\
               func (c C[K]) G() {}\n\
               func (c *C[K]) H() {}\n\
               func (a (A)) Q()  {}\n\
               func (a (*A)) R() {}\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let all: Vec<&str> = ex.nodes.iter().map(|n| n.symbol.as_str()).collect();
    // One id shape per branch — value/pointer/parenthesized receivers of one
    // type all anchor under the same bare name.
    for sym in [
        "ts-go . . . probe_recv/A#M().", // value: (a A)
        "ts-go . . . probe_recv/B#M().", // value, second type — the collision fix
        "ts-go . . . probe_recv/A#P().", // pointer: (a *A)
        "ts-go . . . probe_recv/C#G().", // generic: (c C[K])
        "ts-go . . . probe_recv/C#H().", // pointer-generic: (c *C[K])
        "ts-go . . . probe_recv/A#Q().", // parenthesized: (a (A))
        "ts-go . . . probe_recv/A#R().", // parenthesized-pointer: (a (*A))
    ] {
        assert!(
            all.contains(&sym),
            "[{lang}] expected {sym}; symbols = {all:?}"
        );
    }
    // The headline distinctness: A#M() != B#M().
    let ms: std::collections::HashSet<&str> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "M")
        .map(|n| n.symbol.as_str())
        .collect();
    assert_eq!(
        ms.len(),
        2,
        "[{lang}] REGRESSION: two receiver types' M() share one SymbolId; got {ms:?}"
    );
}

/// Ruby singleton-method identity (scm-anchors D5), FLIPPED after the fix:
/// the non-emitting `(self)` singleton_class anchor + the OPTIONAL
/// `object: (self)? @code_method.owner` splice make BOTH singleton spellings
/// (`def self.m` and `class << self; def m`) converge on `C#self#<name>().`,
/// distinct from the instance `C#<name>().` — previously all three merged
/// into one SymbolId (pinned by the first version of this test).
/// R-DEF-LOSS pins kept: `def C.k` / `def obj.j` (non-self receivers) still
/// extract, ownerless — and `def C.k` therefore still merges with instance
/// `def k` (the pinned residual, flip instruction inline below).
#[test]
fn ruby_singleton_vs_instance_collision_known_defect() {
    let lang = "ruby";
    let sf = SourceFile {
        path: "probe_singleton.rb".to_string(),
        language: Language::new(lang),
        text: "class C\n\
               \x20 def m; end\n\
               \x20 def self.m; end\n\
               \x20 def n; end\n\
               \x20 class << self\n\
               \x20   def n; end\n\
               \x20   def s; end\n\
               \x20 end\n\
               \x20 def self.s; end\n\
               \x20 def k; end\n\
               \x20 def C.k; end\n\
               \x20 def obj.j; end\n\
               end\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let distinct = |name: &str| -> std::collections::HashSet<String> {
        ex.nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == name)
            .map(|n| n.symbol.as_str().to_string())
            .collect()
    };
    // FIXED (was the pinned defect): instance `def m` and `def self.m` mint
    // DISTINCT ids — the owner splice nests the singleton method under self.
    let ms = distinct("m");
    assert_eq!(
        ms.len(),
        2,
        "[{lang}] REGRESSION: `def m` vs `def self.m` merged again; got {ms:?}"
    );
    assert!(ms.contains("ts-ruby . . . probe_singleton/C#m()."));
    assert!(ms.contains("ts-ruby . . . probe_singleton/C#self#m()."));
    // FIXED: `class << self; def n` no longer merges with instance `def n` —
    // the non-emitting singleton_class anchor nests it under self.
    let ns = distinct("n");
    assert_eq!(
        ns.len(),
        2,
        "[{lang}] REGRESSION: `class << self` member vs instance method merged \
         again; got {ns:?}"
    );
    assert!(ns.contains("ts-ruby . . . probe_singleton/C#n()."));
    assert!(ns.contains("ts-ruby . . . probe_singleton/C#self#n()."));
    // Both singleton spellings of `s` converge on the SAME id — C#self#s(). —
    // the reason the anchor is named "self" (one Ruby class-method, one id).
    let ss = distinct("s");
    assert_eq!(
        ss,
        ["ts-ruby . . . probe_singleton/C#self#s().".to_string()].into(),
        "[{lang}] `def self.s` and `class << self; def s` must mint ONE shape"
    );
    // R-DEF-LOSS pins: non-self singleton receivers EXTRACT today (ownerless,
    // nested under C by containment) — the owner edit must not drop them.
    assert_def(&ex, lang, "j", &NodeKind::Method); // def obj.j
    let ks: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "k")
        .collect();
    assert_eq!(
        ks.len(),
        2,
        "[{lang}] instance `def k` AND `def C.k` must both extract; got {ks:?}"
    );
    // Residual (kept after the fix, own flip instruction): `def C.k` stays
    // ownerless and still merges with instance `def k` — an owner splice for
    // constant receivers would mint C#C#k()., an unrecorded convention.
    assert_eq!(
        distinct("k").len(),
        1,
        "[{lang}] KNOWN RESIDUAL RESOLVED? `def C.k` no longer merges with \
         instance `def k` — a constant-receiver owner convention landed; \
         re-point this residual pin."
    );
}

/// Fleet-audit hit (scm-anchors S7 / merge note M6), pinned: Swift
/// `extension Foo { func m() {} }` is uncaptured as a container — swift.scm
/// keyword-gates class_declaration on "class"/"struct"/"enum", and an
/// extension's `name:` is a `user_type`, not a `type_identifier` — so
/// extension methods stay module-flat and TWO extensions' same-named methods
/// share one SymbolId (the exact Rust-impl F1 shape). Now expressible as pure
/// query data: a NON-EMITTING `@code_struct.anchor` on the extension's
/// user_type (the scheme-3 role this lane added). FLIP INSTRUCTION: when the
/// Swift anchor lands, assert A#run(). != B#run(). instead.
#[test]
fn swift_extension_methods_collision_known_defect() {
    let lang = "swift";
    let sf = SourceFile {
        path: "probe_ext.swift".to_string(),
        language: Language::new(lang),
        text: "struct A {}\nstruct B {}\n\
               extension A { func run() {} }\n\
               extension B { func run() {} }\n"
            .to_string(),
    };
    let ex = TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&sf)
        .expect("extraction must succeed");
    let runs: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::File) && n.name == "run")
        .collect();
    assert_eq!(
        runs.len(),
        2,
        "[{lang}] both extension methods must extract (module-flat); got {runs:?}"
    );
    assert_eq!(
        runs[0].symbol, runs[1].symbol,
        "[{lang}] KNOWN DEFECT RESOLVED? two extensions' same-named methods no \
         longer collide — the extension .anchor landed. Flip this pin to \
         assert distinct ids."
    );
}
