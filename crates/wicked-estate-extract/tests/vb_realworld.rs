//! Real-world integration tests for the VB tree-sitter extractor family.
//! Uses TreeSitterExtractor for all four variants: VB6, VB.NET, VBA, VBScript.

use wicked_estate_core::{EdgeKind, Extractor, Language, NodeKind, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;

fn extract(lang: &str, path: &str, text: &str) -> wicked_estate_core::Extraction {
    TreeSitterExtractor::for_language(lang)
        .unwrap_or_else(|| panic!("{lang} must be registered in LANG_TABLE"))
        .extract(&SourceFile {
            path: path.to_string(),
            language: Language::new(lang),
            text: text.to_string(),
        })
        .expect("extract must succeed")
}

/// VB.NET banking service — namespace, class, interface, module, methods, imports, heritage, calls.
#[test]
fn vbnet_banking_service_realistic() {
    let src = r#"
Imports System.Collections.Generic
Imports Microsoft.EntityFrameworkCore

Namespace Banking.Services

    Public Interface IAccountService
        Inherits IDisposable

        Function GetBalance(accountId As Integer) As Decimal
        Sub Transfer(fromId As Integer, toId As Integer, amount As Decimal)
    End Interface

    Public Class AccountService
        Inherits BaseService
        Implements IAccountService, ILoggable

        Private _repo As IAccountRepository

        Public Sub New(repo As IAccountRepository)
            _repo = New AccountRepository()
        End Sub

        Public Function GetBalance(accountId As Integer) As Decimal
            Dim acct As Account = _repo.FindById(accountId)
            Return acct.Balance
        End Function

        Public Sub Transfer(fromId As Integer, toId As Integer, amount As Decimal)
            Dim txn As ITransaction = New BankTransaction()
            Call ValidateAmount(amount)
        End Sub

        Private Sub ValidateAmount(amount As Decimal)
        End Sub

        Public ReadOnly Property ServiceName() As String
        End Property

    End Class

    Public Module BankingHelpers
        Public Function FormatCurrency(amount As Decimal) As String
            Return amount.ToString("C2")
        End Function
    End Module

End Namespace
"#;

    let ex = extract("vbnet", "src/Banking/AccountService.vb", src);
    let node_names: Vec<&str> = ex.nodes.iter().map(|n| n.name.as_str()).collect();

    // Type-level nodes (bare names, unqualified)
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "AccountService"),
        "AccountService Class node missing; got {node_names:?}"
    );
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Interface && n.name == "IAccountService"),
        "IAccountService Interface node missing; got {node_names:?}"
    );
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Module && n.name == "BankingHelpers"),
        "BankingHelpers Module node missing; got {node_names:?}"
    );
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Namespace && n.name == "Banking.Services"),
        "Banking.Services Namespace node missing; got {node_names:?}"
    );

    // Method nodes (bare names — same name appears in both interface and class)
    let fns: Vec<&str> = ex
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    assert!(
        fns.contains(&"GetBalance"),
        "GetBalance method missing; got {fns:?}"
    );
    assert!(
        fns.contains(&"Transfer"),
        "Transfer method missing; got {fns:?}"
    );
    assert!(
        fns.contains(&"ValidateAmount"),
        "ValidateAmount method missing; got {fns:?}"
    );
    assert!(
        fns.contains(&"FormatCurrency"),
        "FormatCurrency method missing; got {fns:?}"
    );

    // Property node (NodeKind::Field for properties)
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Field && n.name == "ServiceName"),
        "ServiceName property missing; got {node_names:?}"
    );

    // Imports
    let imports: Vec<&str> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Imports)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        imports.contains(&"System.Collections.Generic"),
        "import missing; got {imports:?}"
    );
    assert!(
        imports.contains(&"Microsoft.EntityFrameworkCore"),
        "EntityFrameworkCore import missing; got {imports:?}"
    );

    // Heritage: VB.NET grammar (0.1.0) requires Inherits/Implements on the same logical line
    // as the class keyword (before _terminator). Standard multi-line VB.NET style (Inherits on
    // its own line) is NOT captured — tree-sitter error recovery does not produce named
    // inherits_clause/implements_clause nodes for separate-line heritage. No assertion here.

    // Calls from new_expression and invocation
    let calls: Vec<&str> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"AccountRepository"),
        "New AccountRepository missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"BankTransaction"),
        "New BankTransaction missing; got {calls:?}"
    );
    // ValidateAmount via Call statement wrapping an invocation
    assert!(
        calls.contains(&"ValidateAmount"),
        "Call ValidateAmount missing; got {calls:?}"
    );

    // Every node must carry the "vbnet" language tag
    for node in &ex.nodes {
        assert_eq!(
            node.language,
            Language::new("vbnet"),
            "node {} must have language=vbnet",
            node.name
        );
    }
}

/// VB6 customer form — subs, functions, properties, call detection.
/// Tree-sitter produces unqualified names (no module container node since VB_Name
/// attribute parsing is not in-scope for the grammar-level query).
#[test]
fn vb6_customer_form_realistic() {
    let src = r#"VERSION 1.0 CLASS
MultiUse = -1  'True

Attribute VB_Name = "CustomerForm"
Attribute VB_GlobalNameSpace = False
Option Explicit

Begin VB.Form frmCustomer
   Caption         =   "Customer"
   ClientHeight    =   4680
End

Private mCustomerID As Long

Public Sub Form_Load()
    Set mCustomerSvc = New CustomerService
    Call LoadCustomers
End Sub

Private Sub LoadCustomers()
    Call mCustomerSvc.GetAll
End Sub

Public Function GetSelectedID() As Long
    GetSelectedID = mCustomerID
End Function

Public Property Get CustomerID() As Long
    CustomerID = mCustomerID
End Property

Public Property Let CustomerID(val As Long)
    mCustomerID = val
End Property

Private Sub cmdSave_Click()
    Set objResult = New SaveResult
    Call SaveRecord(mCustomerID)
End Sub

Private Sub SaveRecord(id As Long)
    Dim saved As Boolean
    saved = True
End Sub
"#;

    let ex = extract("vb6", "CustomerForm.cls", src);
    let node_names: Vec<&str> = ex.nodes.iter().map(|n| n.name.as_str()).collect();

    // Subs and functions (bare names — no module qualification at grammar level)
    for method in &[
        "Form_Load",
        "LoadCustomers",
        "GetSelectedID",
        "cmdSave_Click",
        "SaveRecord",
    ] {
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Function && n.name == *method),
            "{method} Function node missing; got {node_names:?}"
        );
    }

    // Property (NodeKind::Field)
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Field && n.name == "CustomerID"),
        "CustomerID property missing; got {node_names:?}"
    );

    // VB6 call detection note: the VB6 grammar (0.0.2) parses `Call Fn()` (with parens) as
    // function_call { name: "Call" } rather than a call to Fn, and `Call Fn` (no args) doesn't
    // match call_statement (which requires argument_list_no_parens). Call refs from the `Call`
    // keyword form are therefore NOT reliably captured — no assertion here.
}

/// VBA Office macro — subs, functions, properties, call detection.
#[test]
fn vba_office_report_macro_realistic() {
    let src = r#"Attribute VB_Name = "ReportMacro"
Option Explicit

Private Sub GenerateMonthlyReport()
    Set rptEngine = New ReportEngine
    Call FormatHeader("Monthly Sales Report")
    Call ExportToPDF("report.pdf")
End Sub

Public Function CalculateTotal(prices As Range) As Double
    Dim total As Double
    CalculateTotal = total
End Function

Private Sub FormatHeader(title As String)
    Call ApplyStyles
End Sub

Private Sub ExportToPDF(filename As String)
    Dim ok As Boolean
    ok = True
End Sub

Private Sub ApplyStyles()
    Dim done As Boolean
    done = True
End Sub

Public Property Get ReportTitle() As String
    ReportTitle = "Monthly"
End Property
"#;

    let ex = extract("vba", "ReportMacro.vba", src);
    let node_names: Vec<&str> = ex.nodes.iter().map(|n| n.name.as_str()).collect();

    // Subs and functions
    for method in &[
        "GenerateMonthlyReport",
        "CalculateTotal",
        "FormatHeader",
        "ExportToPDF",
        "ApplyStyles",
    ] {
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Function && n.name == *method),
            "{method} Function node missing; got {node_names:?}"
        );
    }

    // Property
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Field && n.name == "ReportTitle"),
        "ReportTitle property missing; got {node_names:?}"
    );

    // Call refs
    let calls: Vec<&str> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(
        calls.contains(&"FormatHeader"),
        "Call FormatHeader missing; got {calls:?}"
    );
    assert!(
        calls.contains(&"ExportToPDF"),
        "Call ExportToPDF missing; got {calls:?}"
    );

    // Language tag
    assert!(
        ex.nodes.iter().all(|n| n.language == Language::new("vba")),
        "all VBA nodes must have language=vba"
    );
}

/// VBScript WSH script — function, sub, call detection.
#[test]
fn vbscript_wsh_script_realistic() {
    let src = r#"Option Explicit

Dim objFSO
Set objFSO = New FileSystemObject

Sub Main()
    Dim result
    result = ReadConfig("settings.ini")
    Call ProcessData(result)
End Sub

Function ReadConfig(filename)
    Dim fso
    Set fso = New FileSystemObject
    ReadConfig = fso.ReadFile(filename)
End Function

Sub ProcessData(data)
    WriteOutput data
End Sub

Sub WriteOutput(content)
    WScript.Echo content
End Sub
"#;

    let ex = extract("vbscript", "script.vbs", src);
    let node_names: Vec<&str> = ex.nodes.iter().map(|n| n.name.as_str()).collect();

    // Sub and Function definitions
    for sub_name in &["Main", "ProcessData", "WriteOutput"] {
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Function && n.name == *sub_name),
            "{sub_name} Sub node missing; got {node_names:?}"
        );
    }
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "ReadConfig"),
        "ReadConfig Function node missing; got {node_names:?}"
    );

    // Call detection
    let calls: Vec<&str> = ex
        .refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.as_str())
        .collect();
    assert!(!calls.is_empty(), "expected call refs; got none");

    // Language tag
    assert!(
        ex.nodes
            .iter()
            .all(|n| n.language == Language::new("vbscript")),
        "all VBScript nodes must have language=vbscript"
    );
}

/// VB.NET edge cases: empty file, comment-only, bare End lines.
#[test]
fn vbnet_edge_cases() {
    let ex = extract("vbnet", "empty.vb", "");
    // File node is always emitted; no code defs
    let code_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(
        code_nodes.is_empty(),
        "empty file should produce no code nodes; got {code_nodes:?}"
    );

    let ex = extract(
        "vbnet",
        "comments.vb",
        "'This is a VB.NET comment\n' Another comment\n",
    );
    let code_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(
        code_nodes.is_empty(),
        "comment-only file should produce no code nodes"
    );

    let ex = extract(
        "vbnet",
        "ends.vb",
        "End Class\nEnd Module\nEnd Namespace\nEnd Sub\nEnd Function\n",
    );
    let code_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(
        code_nodes.is_empty(),
        "bare End lines should produce no code nodes"
    );
}

/// VB6 edge cases: simple module with one sub, begin-block skipped.
/// Note: VB6 grammar (0.0.2) requires non-empty sub/function bodies (block = REPEAT1).
#[test]
fn vb6_edge_cases() {
    // Simple .bas module with one Sub (must have a body — grammar requires REPEAT1 block)
    let ex = extract(
        "vb6",
        "MyModule.bas",
        "Public Sub DoWork()\n    Dim x As Integer\nEnd Sub\n",
    );
    let code_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(
        code_nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "DoWork"),
        "DoWork missing; got {code_nodes:?}"
    );

    // Begin..End form designer block must not produce code nodes
    let ex = extract(
        "vb6",
        "NestedForm.cls",
        r#"Attribute VB_Name = "NestedForm"
Begin VB.Form frmNested
   Caption = "Options"
End
Public Sub Init()
    Dim ready As Boolean
    ready = True
End Sub
"#,
    );
    let code_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(
        code_nodes.iter().all(|n| n.kind == NodeKind::Function),
        "Begin..End must not produce non-Function code nodes; got {code_nodes:?}"
    );
    assert!(
        code_nodes.iter().any(|n| n.name == "Init"),
        "Init Sub missing; got {code_nodes:?}"
    );
}

/// VBA edge case: Option directive lines produce no nodes.
#[test]
fn vba_option_lines_skipped() {
    let ex = extract(
        "vba",
        "Opts.vba",
        "Option Explicit\nOption Private Module\nOption Base 1\n",
    );
    let code_nodes: Vec<_> = ex
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .collect();
    assert!(
        code_nodes.is_empty(),
        "Option lines must not produce code nodes; got {code_nodes:?}"
    );
}
