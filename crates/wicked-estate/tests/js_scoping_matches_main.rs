//! estate#492: local-binding detection follows JS scoping where #488 cut it short. A `var`
//! redeclaring a parameter IS the parameter, so the last `var` assignment before the call is the
//! value it sees; a class static block is a `var` scope of its own. Each case runs for calls and
//! JSX against an unrelated global `Card` in `other.tsx`, and asserts the exact targets.
//!
//! Same input, four builds. [`BUILD`] names the build this copy asserts; the baselines are this
//! file with only that line changed, pushed for the PR on throwaway branches of those commits,
//! where estate's ordinary CI runs them (nothing here automates that):
//! | case             | before #488 (`Pre488`) | after #488 (`Main`) | after #493 (`Main493`) | this change (`Head`) |
//! |------------------|------------------------|---------------------|------------------------|----------------------|
//! | param_var        | local arrow            | none (the bug)      | local arrow            | local arrow          |
//! | param_var_after  | local arrow            | none                | none                   | none                 |
//! | var_function     | local fn               | local fn            | local fn               | local fn             |
//! | static_arrow     | none (a tie)           | none (a tie)        | each class's arrow     | each class's arrow   |
//! | static_value     | other.tsx              | other.tsx           | none (parked)          | none (parked)        |
//! | reassigned       | local arrow            | none                | none                   | none                 |
//! | nested_replaced  | local arrow            | none                | none                   | none                 |
//! | catch_assigned   | local arrow            | none                | none                   | none                 |
//! | static_before    | none (a tie)           | none (a tie)        | none (parked)          | none (parked)        |
//! | array_destructed | local arrow            | none                | stale arrow (#497)     | none                 |
//! | object_destructed| local arrow            | none                | stale arrow (#497)     | none                 |
//! | for_destructed   | local arrow            | none                | none                   | none                 |
//! | nonnull_destructed| local arrow           | none                | stale arrow (#497)     | none                 |
//! | destructed_after | local arrow            | none                | local arrow            | local arrow          |
//! | destructed_overwritten | local arrow      | none                | local arrow            | local arrow          |
//! | destructed_shadowed | local arrow         | none                | local arrow            | local arrow          |
//! | logical_kept     | local arrow            | none                | local arrow            | local arrow          |
//! | destructed_rhs   | local arrow            | none                | local arrow            | local arrow          |
//! | update_written   | local arrow            | none                | stale arrow            | none                 |
//! | as_written       | local arrow            | none                | stale arrow            | none                 |
//! | nested_before_kept | local arrow          | none                | none                   | none                 |
//!
//! `write_lookup_size` (#497): on `Head` the bound file costs under twice the unbound one; on
//! `Main493` (the per-call scan) over twice, which shows the test catches it. Not timed on
//! `Main`, whose unbound path rescans the body per call (#493 memoised it).

#[allow(dead_code)]
#[derive(PartialEq)]
enum Build {
    Pre488,
    Main,
    Main493,
    Head,
}

/// Builds with #492's scoping (#493 and this change).
fn scoped() -> bool {
    matches!(BUILD, Build::Main493 | Build::Head)
}

const BUILD: Build = Build::Head;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wicked_estate_core::{EdgeKind, Extractor, GraphRead, Language, SourceFile};
use wicked_estate_extract::TreeSitterExtractor;
use wicked_estate_store::SqliteStore;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ci_js_scope_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("src")).unwrap();
    d
}

struct Graph {
    /// Per caller: the sorted target symbol ids of its Calls edges.
    calls: HashMap<String, Vec<String>>,
    parked_local: usize,
}

/// Index `caller.tsx` beside `other.tsx` (an unrelated global `Card`).
fn index(tag: &str, caller: &str, callers: &[&str]) -> Graph {
    let dir = fresh_dir(tag);
    fs::write(
        dir.join("src/other.tsx"),
        "export function Card() { return null; }\n",
    )
    .unwrap();
    fs::write(dir.join("src/caller.tsx"), caller).unwrap();
    let mut store = SqliteStore::in_memory().unwrap();
    wicked_estate::index_path(&mut store, &dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    let nodes: HashMap<_, _> = store
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.symbol.clone(), n))
        .collect();
    let all = store.all_edges().unwrap();
    let calls = callers
        .iter()
        .map(|caller| {
            assert!(
                nodes.values().any(|n| n.name == *caller),
                "{tag}: the caller {caller} must exist"
            );
            let mut to: Vec<String> = all
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls)
                .filter(|e| nodes.get(&e.source).is_some_and(|n| n.name == *caller))
                .map(|e| e.target.as_str().to_string())
                .collect();
            to.sort();
            to.dedup();
            (caller.to_string(), to)
        })
        .collect();
    let parked_local = store
        .unresolved_refs_for_name("local-binding:Card")
        .unwrap()
        .len();
    Graph {
        calls,
        parked_local,
    }
}

/// Exactly one target: the symbol `ts-tsx . . . <path>`.
fn assert_one(tag: &str, g: &Graph, caller: &str, path: &str) {
    let want = format!("ts-tsx . . . {path}");
    assert_eq!(g.calls[caller], vec![want], "{tag}: {caller}");
}

fn assert_none(tag: &str, g: &Graph, caller: &str) {
    assert!(
        g.calls[caller].is_empty(),
        "{tag}: {caller} -> {:?}",
        g.calls[caller]
    );
}

const CALLERS: [&str; 2] = ["ViaCall", "ViaJsx"];

/// A block `var` redeclaring a parameter, assigned before the call: the call reaches its arrow.
#[test]
fn param_var() {
    let g = index(
        "param_var",
        "export function ViaCall(Card: any) { { var Card = () => null; } return Card(); }\n\
         export function ViaJsx(Card: any) { { var Card = () => null; } return <Card />; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Main => assert_none("param_var", &g, c),
            _ => assert_one("param_var", &g, c, "src/caller/Card()."),
        }
    }
}

/// The same `var` assigned AFTER the call: the call sees the supplied parameter.
#[test]
fn param_var_after() {
    let g = index(
        "param_var_after",
        "export function ViaCall(Card: any) { const r = Card(); { var Card = () => null; } return r; }\n\
         export function ViaJsx(Card: any) { const r = <Card />; { var Card = () => null; } return r; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Pre488 => assert_one("param_var_after", &g, c, "src/caller/Card()."),
            _ => assert_none("param_var_after", &g, c),
        }
    }
}

/// A block `var` and a body function declaration are one binding: the function.
#[test]
fn var_function() {
    let g = index(
        "var_function",
        "export function ViaCall() { { var Card: any; } function Card() { return null; } return Card(); }\n\
         export function ViaJsx() { { var Card: any; } function Card() { return null; } return <Card />; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_one("var_function", &g, c, "src/caller/Card().");
    }
}

/// A static block's nested `var` arrow: each class's call reaches its own arrow.
#[test]
fn static_arrow() {
    let g = index(
        "static_arrow",
        "declare const on: boolean;\n\
         export class ViaCall { static { if (on) { var Card = () => null; } Card(); } }\n\
         export class ViaJsx { static { if (on) { var Card = () => null; } void (<Card />); } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            _ if scoped() => assert_one("static_arrow", &g, c, &format!("src/caller/{c}#Card().")),
            _ => assert_none("static_arrow", &g, c),
        }
    }
}

/// A static block's nested `var` value: local to the block, so no global `Card`; both calls park.
#[test]
fn static_value() {
    let g = index(
        "static_value",
        "declare const on: boolean;\n\
         declare function make(): any;\n\
         export class ViaCall { static { if (on) { var Card = make(); } Card(); } }\n\
         export class ViaJsx { static { if (on) { var Card = make(); } void (<Card />); } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            _ if scoped() => assert_none("static_value", &g, c),
            _ => assert_one("static_value", &g, c, "src/other/Card()."),
        }
    }
    if scoped() {
        assert_eq!(
            g.parked_local, 2,
            "static_value: both calls park as local bindings"
        );
    }
}

/// Pre-#488 bound by name; #488 dropped every parameter call; this change sees the assignment.
fn assert_dropped_since_488(tag: &str, g: &Graph, callers: &[&str]) {
    for c in callers {
        match BUILD {
            Build::Pre488 => assert_one(tag, g, c, "src/caller/Card()."),
            _ => assert_none(tag, g, c),
        }
    }
}

/// A plain reassignment after the `var` arrow: the call sees `null`, not the arrow.
#[test]
fn reassigned() {
    let g = index(
        "reassigned",
        "export function ViaCall(Card: any) { { var Card = () => null; } Card = null; return Card(); }\n\
         export function ViaJsx(Card: any) { { var Card = () => null; } Card = null; return <Card />; }\n",
        &CALLERS,
    );
    assert_dropped_since_488("reassigned", &g, &CALLERS);
}

/// A nested function runs at an unknown time: with two assignments it binds neither.
#[test]
fn nested_replaced() {
    let g = index(
        "nested_replaced",
        "export function ViaCall(Card: any) { { var Card = () => null; } function InnerCall() { return Card(); } { var Card = null; } return InnerCall(); }\n\
         export function ViaJsx(Card: any) { { var Card = () => null; } function InnerJsx() { return <Card />; } { var Card = null; } return InnerJsx(); }\n",
        &["ViaCall", "ViaJsx", "InnerCall", "InnerJsx"],
    );
    assert_dropped_since_488("nested_replaced", &g, &["InnerCall", "InnerJsx"]);
    assert_one("nested_replaced", &g, "ViaCall", "src/caller/InnerCall().");
    assert_one("nested_replaced", &g, "ViaJsx", "src/caller/InnerJsx().");
}

/// A `var` initializer inside `catch (Card)` assigns the catch binding, not the parameter.
#[test]
fn catch_assigned() {
    let g = index(
        "catch_assigned",
        "export function ViaCall(Card: any) { try { throw null; } catch (Card) { var Card = () => null; } return Card(); }\n\
         export function ViaJsx(Card: any) { try { throw null; } catch (Card) { var Card = () => null; } return <Card />; }\n",
        &CALLERS,
    );
    assert_dropped_since_488("catch_assigned", &g, &CALLERS);
}

/// A static block's call before its `var` arrow is assigned sees `undefined`.
#[test]
fn static_before() {
    let g = index(
        "static_before",
        "export class ViaCall { static { Card(); { var Card = () => null; } } }\n\
         export class ViaJsx { static { void (<Card />); { var Card = () => null; } } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_none("static_before", &g, c);
    }
    if scoped() {
        assert_eq!(
            g.parked_local, 2,
            "static_before: both calls park as local bindings"
        );
    }
}

/// A destructuring write after the `var` arrow: which value it stores is not tracked, so the
/// binding is unknown and the call parks, as after #488 (#497).
fn destructed(tag: &str, write: &str) {
    let g = index(
        tag,
        &format!(
            "declare const next: any;\n\
             export function ViaCall(Card: any) {{ {{ var Card = () => null; }} {write}; return Card(); }}\n\
             export function ViaJsx(Card: any) {{ {{ var Card = () => null; }} {write}; return <Card />; }}\n"
        ),
        &CALLERS,
    );
    for c in CALLERS {
        // `for ([Card] of …)` already masked the arrow on #493; the assignment forms did not.
        let stale = BUILD == Build::Main493 && !tag.starts_with("for_");
        if BUILD == Build::Pre488 || stale {
            assert_one(tag, &g, c, "src/caller/Card().");
        } else {
            assert_none(tag, &g, c);
        }
    }
}

#[test]
fn array_destructed() {
    destructed("array_destructed", "[Card] = next");
}

#[test]
fn object_destructed() {
    destructed("object_destructed", "({ Card } = next)");
}

#[test]
fn nonnull_destructed() {
    destructed("nonnull_destructed", "[Card!] = next");
}

/// A param redeclared by a block `var` arrow, with `READ` the use: the call reaches the arrow on
/// every build but #488's, which dropped every parameter call.
fn keeps_arrow(tag: &str, body: &str) {
    let g = index(
        tag,
        &format!(
            "export function ViaCall(Card: any) {{ {} }}\n\
             export function ViaJsx(Card: any) {{ {} }}\n",
            body.replace("READ", "Card()"),
            body.replace("READ", "<Card />"),
        ),
        &CALLERS,
    );
    for c in CALLERS {
        match BUILD {
            Build::Main => assert_none(tag, &g, c),
            _ => assert_one(tag, &g, c, "src/caller/Card()."),
        }
    }
}

/// A destructuring write after the call does not change what the call saw.
#[test]
fn destructed_after() {
    keeps_arrow(
        "destructed_after",
        "{ var Card = () => null; } const r = READ; [Card] = [null]; return r;",
    );
}

/// The `var` arrow overwrites an earlier destructuring write.
#[test]
fn destructed_overwritten() {
    keeps_arrow(
        "destructed_overwritten",
        "[Card] = [null]; { var Card = () => null; } return READ;",
    );
}

/// A destructuring write to a block's own `let Card` is that binding's.
#[test]
fn destructed_shadowed() {
    keeps_arrow(
        "destructed_shadowed",
        "{ var Card = () => null; } { let Card: any; [Card] = [null]; } return READ;",
    );
}

/// `||=` keeps a callable value.
#[test]
fn logical_kept() {
    keeps_arrow(
        "logical_kept",
        "{ var Card = () => null; } (Card) ||= null; return READ;",
    );
}

#[test]
fn for_destructed() {
    destructed("for_destructed", "for ([Card] of next) {}");
}

/// A plain write the use precedes: `Card++` and `(Card as any) = …` replace the arrow.
#[test]
fn update_written() {
    destructed("update_written", "Card++");
}

#[test]
fn as_written() {
    destructed("as_written", "(Card as any) = null");
}

/// A destructuring write takes effect after its right-hand side, which still reads the arrow.
#[test]
fn destructed_rhs() {
    keeps_arrow(
        "destructed_rhs",
        "{ var Card = () => null; } [Card] = [READ]; return null;",
    );
}

/// A nested function called before the arrow exists never sees it: `||=` is one more write it
/// cannot order, as on #493.
#[test]
fn nested_before_kept() {
    let g = index(
        "nested_before_kept",
        "export function ViaCall(Card: any) { function InnerCall() { return Card(); } const r = InnerCall(); { var Card = () => null; } Card ||= null; return r; }\n\
         export function ViaJsx(Card: any) { function InnerJsx() { return <Card />; } const r = InnerJsx(); { var Card = () => null; } Card ||= null; return r; }\n",
        &["ViaCall", "ViaJsx", "InnerCall", "InnerJsx"],
    );
    assert_dropped_since_488("nested_before_kept", &g, &["InnerCall", "InnerJsx"]);
    assert_one(
        "nested_before_kept",
        &g,
        "ViaCall",
        "src/caller/InnerCall().",
    );
    assert_one("nested_before_kept", &g, "ViaJsx", "src/caller/InnerJsx().");
}

/// The faster of two extractions of `text`.
fn extract_time(text: &str) -> Duration {
    let ex = TreeSitterExtractor::for_language("tsx").unwrap();
    let file = SourceFile {
        path: "src/big.tsx".into(),
        language: Language::new("tsx"),
        text: text.into(),
    };
    (0..2)
        .map(|_| {
            let t = Instant::now();
            ex.extract(&file).unwrap();
            t.elapsed()
        })
        .min()
        .unwrap()
}

/// #497: a call reads its scope's writes once per file and binary-searches them, so a file of N
/// writes and N calls to a redeclared parameter costs what the same file costs when the name is
/// not a parameter (main's path). A per-call copy and scan of every write is quadratic in N.
#[test]
fn write_lookup_size() {
    if BUILD == Build::Main {
        return;
    }
    // At 10k the old per-call scan measured 1.9x on a CI runner (1.60 s vs 0.83 s); its extra
    // cost grows with n squared, the shared cost with n, so 30k puts it well past 2x.
    let n = 30_000;
    let body = "Card = null; Card();\n".repeat(n);
    let file = |param: &str| {
        format!("export function P({param}: any) {{ {{ var Card = () => null; }}\n{body}}}\n")
    };
    let bound = extract_time(&file("Card"));
    let unbound = extract_time(&file("Other"));
    eprintln!("write_lookup_size: bound {bound:?}, unbound {unbound:?}");
    if BUILD == Build::Main493 {
        assert!(
            bound > unbound * 2,
            "the per-call scan: {bound:?} vs {unbound:?}"
        );
    } else {
        assert!(
            bound < unbound * 2,
            "the bound file must cost what the unbound one does: {bound:?} vs {unbound:?}"
        );
    }
}

/// #497: a block of N `let`s around a write costs one pass, not N copies of N names.
#[test]
fn lexical_shadow_size() {
    let n = 10_000;
    let lets: String = (0..n).map(|i| format!("let z{i}: any;\n")).collect();
    let file = |param: &str| {
        format!(
            "export function P({param}: any) {{ {{ var Card = () => null; }}\n{{\n{lets}Card = null;\nCard();\n}} }}\n"
        )
    };
    let bound = extract_time(&file("Card"));
    let unbound = extract_time(&file("Other"));
    eprintln!("lexical_shadow_size: bound {bound:?}, unbound {unbound:?}");
    assert!(
        bound < unbound * 2,
        "the bound file must cost what the unbound one does: {bound:?} vs {unbound:?}"
    );
}
