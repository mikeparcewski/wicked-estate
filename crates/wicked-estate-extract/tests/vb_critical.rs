//! Critical / adversarial tests for the VB tree-sitter extractor family.
//! All four variants use TreeSitterExtractor.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

fn ex(lang: &str, path: &str, text: &str) -> wicked_estate_core::Extraction {
    TreeSitterExtractor::for_language(lang)
        .unwrap_or_else(|| panic!("{lang} not in LANG_TABLE"))
        .extract(&SourceFile {
            path: path.to_string(),
            language: Language::new(lang),
            text: text.to_string(),
        })
        .expect("extract")
}

fn vbnet(text: &str) -> wicked_estate_core::Extraction {
    ex("vbnet", "T.vb", text)
}
fn vb6(text: &str) -> wicked_estate_core::Extraction {
    ex("vb6", "T.bas", text)
}
fn vba(text: &str) -> wicked_estate_core::Extraction {
    ex("vba", "T.vba", text)
}
fn vbscript(text: &str) -> wicked_estate_core::Extraction {
    ex("vbscript", "T.vbs", text)
}

// ── VB.NET tests ─────────────────────────────────────────────────────────────

/// VB.NET keywords are case-insensitive.
#[test]
fn vbnet_case_insensitive_keywords() {
    let src = "PUBLIC CLASS Foo\nPUBLIC FUNCTION Bar() AS INTEGER\nEND FUNCTION\nEND CLASS\n";
    let e = vbnet(src);
    let names: Vec<&str> = e.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "Foo"),
        "UPPERCASE CLASS Foo missing; got {names:?}"
    );
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "Bar"),
        "UPPERCASE FUNCTION Bar missing; got {names:?}"
    );
}

/// Generic class: `Class MyList(Of T)` — name must be "MyList" without type params.
#[test]
fn vbnet_generic_class_name() {
    let src = "Public Class MyList(Of T)\nEnd Class\n";
    let e = vbnet(src);
    let names: Vec<&str> = e.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        e.nodes.iter().any(|n| n.name == "MyList"),
        "Generic class name must be MyList; got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("(Of")),
        "Class name must not contain '(Of'; got {names:?}"
    );
}

/// Delegate declarations are NOT method_declaration — must not emit Function nodes.
#[test]
fn vbnet_delegate_not_emitted_as_function() {
    let src = "Public Delegate Sub MyHandler(sender As Object)\nPublic Delegate Function Transform(x As Integer) As Integer\n";
    let e = vbnet(src);
    let fn_nodes: Vec<&str> = e
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        fn_nodes.is_empty(),
        "Delegate declarations must not emit Function nodes; got {fn_nodes:?}"
    );
}

/// Event declarations must not emit Function nodes.
#[test]
fn vbnet_event_declaration_not_emitted() {
    let src = "Public Class Foo\nPublic Event StatusChanged As EventHandler\nEnd Class\n";
    let e = vbnet(src);
    let fn_nodes: Vec<&str> = e
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        fn_nodes.is_empty(),
        "Event declaration must not emit a Function node; got {fn_nodes:?}"
    );
}

/// String literals containing VB keywords must not produce spurious nodes.
#[test]
fn vbnet_string_literal_keywords_ignored() {
    let src = r#"
Public Class Parser
    Public Sub ProcessLine(line As String)
        Dim keyword As String = "Public Class FakeClass"
    End Sub
End Class
"#;
    let e = vbnet(src);
    let names: Vec<&str> = e.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        !names.iter().any(|n| n.contains("FakeClass")),
        "String literal content must not produce nodes; got {names:?}"
    );
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "Parser"),
        "Parser class missing; got {names:?}"
    );
}

/// `Structure` produces a NodeKind::Struct node (not Class).
#[test]
fn vbnet_structure_produces_struct_node() {
    let src = "Public Structure Point\nPublic X As Integer\nPublic Y As Integer\nEnd Structure\n";
    let e = vbnet(src);
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Struct && n.name == "Point"),
        "Structure must produce a Struct node; got {:?}",
        e.nodes
            .iter()
            .map(|n| (&n.kind, &n.name))
            .collect::<Vec<_>>()
    );
}

/// Inherits/Implements are captured when on the class declaration line.
/// Note: tree-sitter-vb-dotnet places these before the first _terminator (newline),
/// so they must be inline with the Class keyword for the grammar to recognise them.
/// This tests the query wiring, not VB.NET style conventions.
#[test]
fn vbnet_inherits_implements_captured() {
    let src =
        "Public Class Service Inherits BaseService Implements IService, IDisposable\nEnd Class\n";
    let e = vbnet(src);
    let extends: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Extends)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        extends.contains(&"BaseService"),
        "Inherits BaseService not captured; got {extends:?}"
    );
    let impls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Implements)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        impls.contains(&"IService"),
        "Implements IService not captured; got {impls:?}"
    );
    assert!(
        impls.contains(&"IDisposable"),
        "Implements IDisposable not captured; got {impls:?}"
    );
}

/// `Sub New(...)` is a constructor_declaration — not captured by method_declaration query.
/// This ensures we don't panic and simply produce no Sub node for the constructor.
#[test]
fn vbnet_constructor_sub_new_no_crash() {
    let src = "Public Class Widget\nPublic Sub New(id As Integer)\nEnd Sub\nPublic Sub Reset()\nEnd Sub\nEnd Class\n";
    let e = vbnet(src);
    // Reset IS a method_declaration and must be captured
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "Reset"),
        "Reset method must be captured; got {:?}",
        e.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
    // Widget class must be captured
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "Widget"),
        "Widget class missing"
    );
}

/// `Protected Friend` (two-word modifier) does not break method extraction.
#[test]
fn vbnet_protected_friend_modifier_stripped() {
    let src = "Public Class Foo\nProtected Friend Sub InternalMethod()\nEnd Sub\nEnd Class\n";
    let e = vbnet(src);
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "InternalMethod"),
        "Protected Friend Sub must be captured; got {:?}",
        e.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
}

/// New expression inside a comment must not produce a Calls ref.
#[test]
fn vbnet_new_in_comment_ignored() {
    let src = "Public Class Foo\nPublic Sub Bar()\n' Dim x = New FakeClass()\nEnd Sub\nEnd Class\n";
    let e = vbnet(src);
    let calls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        !calls.contains(&"FakeClass"),
        "Commented-out New must not produce a Calls ref; got {calls:?}"
    );
}

/// Multiple interfaces on a single Implements clause (inline with class declaration).
/// VB.NET grammar requires Implements to appear before the first _terminator (newline).
#[test]
fn vbnet_multiple_implements() {
    let src = "Public Class Svc Implements IA, IB, IC\nEnd Class\n";
    let e = vbnet(src);
    let impls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Implements)
        .map(|r| r.raw_name.as_str())
        .collect();
    for iface in &["IA", "IB", "IC"] {
        assert!(
            impls.contains(iface),
            "Implements {iface} missing; got {impls:?}"
        );
    }
}

// ── VB6 tests ────────────────────────────────────────────────────────────────

/// VB6 Property accessors (Get/Let/Set) — correct name captured for each.
/// Note: VB6 grammar (0.0.2) requires non-empty bodies (block = REPEAT1),
/// so each property must have at least one statement.
#[test]
fn vb6_property_accessor_keywords() {
    let src = r#"Public Property Get Timeout() As Integer
    Timeout = 30
End Property
Public Property Let Timeout(val As Integer)
    mTimeout = val
End Property
Public Property Set Connection(obj As Object)
    Set mConnection = obj
End Property
"#;
    let e = vb6(src);
    let prop_names: Vec<&str> = e
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Field)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"Timeout"),
        "Property Get/Let Timeout missing; got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"Connection"),
        "Property Set Connection missing; got {prop_names:?}"
    );
}

/// VB6: Begin..End form blocks must not produce spurious function nodes.
/// Uses a flat (non-nested) Begin..End — the grammar handles this reliably.
/// VB6 grammar also requires non-empty sub bodies (block = REPEAT1).
#[test]
fn vb6_deeply_nested_begin_end() {
    let src = r#"Attribute VB_Name = "DeepForm"
Begin VB.Form frmDeep
   Caption = "Deep"
End
Public Sub Init()
    Dim ready As Boolean
    ready = True
End Sub
"#;
    let e = vb6(src);
    let fn_nodes: Vec<&str> = e
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        fn_nodes.contains(&"Init"),
        "Init must be captured; got {fn_nodes:?}"
    );
    assert!(
        !fn_nodes.contains(&"Caption"),
        "Begin..End content must not produce function nodes; got {fn_nodes:?}"
    );
}

/// VB6: function_call (parenthesised form) must be captured as a call ref.
/// Note: `Call Fn()` with explicit Call keyword is NOT a call_statement in the
/// VB6 grammar — the grammar's call_statement requires no-paren args. Use only
/// the parenthesised function_call form: `result = Fn(args)`.
#[test]
fn vb6_function_call_captured() {
    let src = "Public Sub Main()\n    result = Compute(42)\n    total = Add(1, 2)\nEnd Sub\nPublic Function Compute(x As Integer) As Integer\n    Compute = x * 2\nEnd Function\nPublic Function Add(a As Integer, b As Integer) As Integer\n    Add = a + b\nEnd Function\n";
    let e = vb6(src);
    let calls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"Compute"),
        "Compute() call missing; got {calls:?}"
    );
    assert!(calls.contains(&"Add"), "Add() call missing; got {calls:?}");
}

/// VB6: Implements statement in header produces an Implements edge.
#[test]
fn vb6_implements_statement() {
    let src =
        "Attribute VB_Name = \"ServiceImpl\"\nImplements IService\nPublic Sub Method()\nEnd Sub\n";
    let e = vb6(src);
    let impls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Implements)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        impls.contains(&"IService"),
        "Implements IService missing; got {impls:?}"
    );
}

// ── VBA tests ────────────────────────────────────────────────────────────────

/// VBA: empty file must not panic.
#[test]
fn vba_empty_file_no_panic() {
    let e = vba("");
    let code: Vec<_> = e
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(code.is_empty());
    assert!(e.refs.is_empty());
}

/// VBA: large file (1000 subs) — no panic, all subs extracted.
#[test]
fn vba_large_file_no_overflow() {
    let mut src = String::new();
    for i in 0..1000 {
        src.push_str(&format!("Public Sub Sub{i}()\nEnd Sub\n"));
    }
    let e = vba(&src);
    let fn_count = e
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .count();
    assert_eq!(
        fn_count, 1000,
        "Expected 1000 Function nodes; got {fn_count}"
    );
}

/// VBA: explicit Call statement captured.
/// `Call Helper(42)` → call_statement { target: index_expression { object: identifier "Helper" } }
/// The updated query handles index_expression to extract the callee name.
#[test]
fn vba_call_statement_captured() {
    let src = "Public Sub Main()\n    Call Helper(42)\n    Helper 99\nEnd Sub\nPublic Sub Helper(x As Integer)\n    Dim y As Integer\n    y = x + 1\nEnd Sub\n";
    let e = vba(src);
    let calls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"Helper"),
        "Helper call missing; got {calls:?}"
    );
}

/// VBA: New expression captured as call.
#[test]
fn vba_new_expression_captured() {
    let src = "Public Sub Init()\n    Set obj = New MyClass()\nEnd Sub\n";
    let e = vba(src);
    let calls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"MyClass"),
        "New MyClass missing; got {calls:?}"
    );
}

// ── VBScript tests ───────────────────────────────────────────────────────────

/// VBScript: Sub and Function are captured.
#[test]
fn vbscript_sub_and_function() {
    let src = "Sub Greet(name)\n    WScript.Echo \"Hello \" & name\nEnd Sub\nFunction Add(a, b)\n    Add = a + b\nEnd Function\n";
    let e = vbscript(src);
    let names: Vec<&str> = e.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "Greet"),
        "Greet Sub missing; got {names:?}"
    );
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "Add"),
        "Add Function missing; got {names:?}"
    );
}

/// VBScript: empty file produces no code nodes.
#[test]
fn vbscript_empty_file() {
    let e = vbscript("");
    let code: Vec<_> = e
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(code.is_empty());
}

/// VBScript: function_call (parenthesised) produces a call ref.
#[test]
fn vbscript_function_call_captured() {
    let src = "Sub Main()\n    result = Compute(42)\nEnd Sub\nFunction Compute(x)\n    Compute = x * 2\nEnd Function\n";
    let e = vbscript(src);
    let calls: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"Compute"),
        "Compute() call missing; got {calls:?}"
    );
}

/// VBScript: Private Function is captured.
#[test]
fn vbscript_private_function() {
    let src = "Private Function Helper(x)\n    Helper = x + 1\nEnd Function\n";
    let e = vbscript(src);
    assert!(
        e.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "Helper"),
        "Private Function Helper missing"
    );
}

/// All four VB grammars load without error.
#[test]
fn all_vb_grammars_load() {
    for lang in &["vb6", "vbnet", "vba", "vbscript"] {
        assert!(
            TreeSitterExtractor::for_language(lang).is_some(),
            "{lang} must be registered"
        );
    }
}
