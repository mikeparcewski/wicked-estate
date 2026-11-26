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

/// A .cfs file that ends inside a `queryExecute("` string. The cfscript scanner read query text
/// until the closing `"` with no end-of-input check, so the parse never returned (#322's scanner
/// review). The parse runs on its own thread, so a regression fails this test after the bound
/// instead of hanging the run; the spinning thread ends with the test process.
#[test]
fn cfscript_file_ending_inside_a_query_string_extracts() {
    let text = "component {\n    function load() {\n        return queryExecute(\"SELECT id FROM t";
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // The unterminated string leaves the whole file one parse error, so no symbol is
        // expected: the extraction only has to return.
        let ex = extract("cfscript", "Load.cfs", text);
        let _ = done.send(ex.nodes.len());
    });
    match result.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(_) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("the extraction did not return within 30 s")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("the extraction panicked")
        }
    }
}

/// Extract `text` as CFML on a worker thread and answer the extraction, or fail after `secs`
/// instead of hanging the run (the spinning thread ends with the test process).
fn cfml_within(shape: &str, text: String, secs: u64) -> wicked_estate_core::Extraction {
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(extract("cfml", "Page.cfm", &text));
    });
    match result.recv_timeout(std::time::Duration::from_secs(secs)) {
        Ok(ex) => ex,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{shape}: the extraction did not return within {secs} s")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{shape}: the extraction panicked")
        }
    }
}

/// estate#631: cfml's `scan_comment` counts `<!--` nesting. On a comment that never closes it
/// scanned to the end of the file and failed, and error recovery retried it at every later `<!`,
/// each scan running to the end again: quadratic (20,000 openers took 4.4 s in the pinned CLI,
/// four times longer per doubling). Each shape here has 60,000 to 100,000 openers, one or two a
/// line.
#[test]
fn cfml_unterminated_or_unbalanced_comments_extract_in_linear_time_631() {
    for (shape, text) in [
        ("unterminated openers", "<!--\n".repeat(100_000)),
        (
            "unbalanced nested comments",
            "<!-- a <!-- b -->\n".repeat(30_000),
        ),
    ] {
        // Short lines, so the minified-file guard (any line over 50,000 characters) does not
        // skip the parse: a skipped parse answers no nodes at all, not even the File node.
        let ex = cfml_within(shape, text, 30);
        assert!(
            ex.nodes.iter().any(|n| n.kind == NodeKind::File),
            "{shape}: no File node, so the parse was skipped; got {:?}",
            names(&ex)
        );
    }
}

/// estate#631: an unbalanced comment ends at its last `-->`, as an HTML comment does, so the tags
/// after it still extract. (A comment with no `-->` at all runs to the end of the file.)
#[test]
fn cfml_tags_after_an_unbalanced_comment_still_extract_631() {
    let ex = cfml_within(
        "a comment with one more opener than closer",
        "<!-- a <!-- b -->\n<cfcomponent name=\"Kept\">\n    <cffunction name=\"stillHere\">\n    </cffunction>\n</cfcomponent>\n".to_string(),
        30,
    );
    assert!(
        ex.nodes
            .iter()
            .any(|n| n.kind == NodeKind::Function && n.name == "stillHere"),
        "function 'stillHere' missing after the comment; got {:?}",
        names(&ex)
    );
}
