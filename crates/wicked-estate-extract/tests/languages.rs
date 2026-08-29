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
    // impl-scoped duplicate Method pattern was deleted; kind stays Function until
    // the method-identity lane adds enclosing-type identity).
    assert_def(&ex, lang, "translate", &NodeKind::Function);
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
    assert_def_floor(&ex, lang, 10);

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
    assert_def_floor(&ex, lang, 5);

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
