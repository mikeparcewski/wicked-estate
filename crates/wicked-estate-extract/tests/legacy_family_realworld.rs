//! Real-world integration tests for the "adopt an existing grammar" legacy enterprise family:
//! Delphi/Object Pascal (tree-sitter-pascal), CFML (vendored tree-sitter-cfml), and Progress
//! OpenEdge ABL (vendored tree-sitter-abl). Distinct from the in-house grammars (RPG + the
//! PowerBuilder/FoxPro/LotusScript/Crystal/Informix family), which carry their own corpus
//! parse-gate tests.

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

fn names(ex: &wicked_estate_core::Extraction) -> Vec<String> {
    ex.nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File)
        .map(|n| n.name.clone())
        .collect()
}

fn calls(ex: &wicked_estate_core::Extraction) -> Vec<String> {
    ex.refs
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.raw_name.clone())
        .collect()
}

// ── Delphi / Object Pascal ─────────────────────────────────────────────────────

/// A realistic Delphi unit: interface/implementation split, a class with constructor,
/// destructor, and methods, plus qualified method implementations that call each other.
#[test]
fn delphi_unit_realistic() {
    let ex = extract(
        "pascal",
        "AccountManager.pas",
        r#"unit AccountManager;

interface

type
  TAccount = class(TPersistent)
  private
    FBalance: Currency;
    procedure Validate(Amount: Currency);
  public
    constructor Create;
    function GetBalance: Currency;
    procedure Deposit(Amount: Currency);
  end;

implementation

constructor TAccount.Create;
begin
  FBalance := 0;
end;

procedure TAccount.Validate(Amount: Currency);
begin
  if Amount <= 0 then
    raise Exception.Create('bad amount');
end;

function TAccount.GetBalance: Currency;
begin
  Result := FBalance;
end;

procedure TAccount.Deposit(Amount: Currency);
begin
  Validate(Amount);
  FBalance := FBalance + Amount;
end;

end.
"#,
    );

    let ns = names(&ex);

    // Unit name → Module node.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Module && n.name == "AccountManager"),
        "unit module 'AccountManager' missing; got {ns:?}"
    );

    // Class → a named type node.
    assert!(
        ex.nodes.iter().any(|n| n.name == "TAccount"),
        "class 'TAccount' missing; got {ns:?}"
    );

    // Methods are captured (interface decls and/or qualified implementations).
    for m in ["Create", "GetBalance", "Deposit", "Validate"] {
        assert!(
            ns.iter().any(|n| n == m || n == &format!("TAccount.{m}")),
            "method '{m}' missing; got {ns:?}"
        );
    }

    // Deposit calls Validate.
    assert!(
        calls(&ex).contains(&"Validate".to_string()),
        "expected a call to Validate; got {:?}",
        calls(&ex)
    );
}

// ── CFML (ColdFusion) ──────────────────────────────────────────────────────────

/// A realistic tag-based .cfc component: <cfcomponent> with several <cffunction> tags,
/// each calling other functions via <cfset>.
#[test]
fn cfml_tag_component_realistic() {
    let ex = extract(
        "cfml",
        "UserService.cfc",
        r#"<cfcomponent name="UserService" extends="BaseService">
    <cffunction name="getUser" access="public" returntype="any">
        <cfargument name="userId" type="numeric" required="true">
        <cfset var result = queryUser(arguments.userId)>
        <cfreturn result>
    </cffunction>
    <cffunction name="saveUser" access="public">
        <cfargument name="user" type="struct">
        <cfset validateUser(arguments.user)>
        <cfreturn true>
    </cffunction>
</cfcomponent>
"#,
    );

    // Component → class node.
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Class && n.name == "UserService"),
        "component 'UserService' missing; got {:?}",
        names(&ex)
    );

    // Both cffunction tags captured as functions.
    for f in ["getUser", "saveUser"] {
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Function && n.name == f),
            "function '{f}' missing; got {:?}",
            names(&ex)
        );
    }

    // Calls inside <cfset> bodies are detected.
    let c = calls(&ex);
    assert!(
        c.contains(&"queryUser".to_string()),
        "queryUser call missing; got {c:?}"
    );
    assert!(
        c.contains(&"validateUser".to_string()),
        "validateUser call missing; got {c:?}"
    );
}

/// A realistic script-based component (.cfs): `component { function … }` with cross-calls.
#[test]
fn cfscript_component_realistic() {
    let ex = extract(
        "cfscript",
        "Calc.cfs",
        r#"component {
    public numeric function add(required numeric a, required numeric b) {
        return a + b;
    }
    function compute() {
        return add(2, 3) + helper();
    }
}
"#,
    );

    for f in ["add", "compute"] {
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Function && n.name == f),
            "function '{f}' missing; got {:?}",
            names(&ex)
        );
    }

    let c = calls(&ex);
    assert!(
        c.contains(&"add".to_string()),
        "add call missing; got {c:?}"
    );
    assert!(
        c.contains(&"helper".to_string()),
        "helper call missing; got {c:?}"
    );
}
