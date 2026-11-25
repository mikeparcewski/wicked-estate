//! estate#492: local-binding detection follows JS scoping where #488 cut it short. A `var`
//! redeclaring a parameter IS the parameter, so the last `var` assignment before the call is the
//! value it sees; a class static block is a `var` scope of its own. Each case runs for calls and
//! JSX against an unrelated global `Card` in `other.tsx`, and asserts the exact targets.
//!
//! Same input, five builds (`Main499` = after #499, the build #498/#501 change; the #498/#501
//! cases at the end assert `Pre488`, `Main499` and `Head` only, and return early on the others). [`BUILD`] names the build this copy asserts; the baselines are this
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
//! #495 / #494 / #504 cases (`Head` only; `other.Card` in the first two rows is the ordinary
//! unimported-name fallback any free name gets, not the out-of-scope local the bug bound):
//! | case | main 591286c | Head |
//! |------|--------------|------|
//! | hidden_block_function | caller.Card (false) | other.Card |
//! | body_var_in_parameter_default | caller.Card (false) | other.Card |
//! | static_import_value_alias | parked | other.Card |
//! | body_import_value_alias | parked | other.Card |
//! | local_value_alias | parked | local Local arrow |
//! | loop_back_edge | parked | local arrow |
//! | finally_write | stale arrow | parked |
//! | many_writes_after_kill | stale arrow | parked |
//! | nested_pattern_default | stale arrow in inner default | parked in inner default |
//!
//! The cost tests (#497, #501) are counted-work unit tests in wicked-estate-extract's
//! treesitter.rs (`write_lookup_is_linear_497`, `lexical_shadow_is_linear_497`,
//! `shared_declarations_are_read_once_per_file_501`), not wall-clock ratios here.

#[allow(dead_code)]
#[derive(PartialEq)]
enum Build {
    Pre488,
    Main,
    Main493,
    /// After #499 (#497), the build #498/#501 change.
    Main499,
    Head,
}

/// Builds with #492's scoping (#493 and this change).
fn scoped() -> bool {
    matches!(BUILD, Build::Main493 | Build::Main499 | Build::Head)
}

const BUILD: Build = Build::Head;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use wicked_estate_core::{EdgeKind, GraphRead};
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
    /// Every Calls edge's target, one entry per edge.
    targets: Vec<String>,
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
    let targets = all
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| e.target.as_str().to_string())
        .collect();
    let parked_local = store
        .unresolved_refs_for_name("local-binding:Card")
        .unwrap()
        .len();
    Graph {
        calls,
        targets,
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
        // Since #498 a loop's write is conditional (the loop may not run): the arrow stays a
        // possible value, as before #488.
        let may = BUILD == Build::Head && tag.starts_with("for_");
        if BUILD == Build::Pre488 || stale || may {
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

// ── #498: a var/let binding in the call's own function is read through its writes ──────────
// ── #501: a pattern's write takes effect at its element; a right-hand side does not see it ──

/// Index `body` (with `READ` the use) as two callers `ViaCall` / `ViaJsx` taking `params`, and
/// assert one edge to the local arrow when `edge`, none otherwise. Runs on `Pre488`, `Main499`
/// and `Head` only.
fn case(tag: &str, params: &str, body: &str, edge: [bool; 3]) {
    let want = match BUILD {
        Build::Pre488 => edge[0],
        Build::Main499 => edge[1],
        Build::Head => edge[2],
        Build::Main | Build::Main493 => return,
    };
    let g = index(
        tag,
        &format!(
            "declare const on: boolean;\n\
             declare const next: any;\n\
             export function ViaCall({params}) {{ {} }}\n\
             export function ViaJsx({params}) {{ {} }}\n",
            body.replace("READ", "Card()"),
            body.replace("READ", "<Card />"),
        ),
        &CALLERS,
    );
    for c in CALLERS {
        if want {
            assert_one(tag, &g, c, "src/caller/Card().");
        } else {
            assert_none(tag, &g, c);
        }
    }
}

#[test]
fn body_var_destructed() {
    let body = "var Card = () => null; [Card] = [null]; return READ;";
    case("body_var_destructed", "", body, [true, true, false]);
}

#[test]
fn body_var_reassigned() {
    let body = "var Card = () => null; Card = null; return READ;";
    case("body_var_reassigned", "", body, [true, true, false]);
}

#[test]
fn body_let_reassigned() {
    let body = "let Card = () => null; Card = null; return READ;";
    case("body_let_reassigned", "", body, [true, true, false]);
}

#[test]
fn hoisted_var_reassigned() {
    let body = "if (on) { var Card = () => null; } Card = null; return READ;";
    case("hoisted_var_reassigned", "", body, [true, true, false]);
}

/// A conditional write leaves the arrow a possible value: the edge stays.
#[test]
fn body_var_conditional() {
    let body = "var Card = () => null; if (on) Card = null; return READ;";
    case("body_var_conditional", "", body, [true, true, true]);
}

/// A write in a loop body may not run: the edge stays.
#[test]
fn body_var_loop_written() {
    let body = "var Card = () => null; for (const x of [1]) { Card = null; } return READ;";
    case("body_var_loop_written", "", body, [true, true, true]);
}

/// The same rule for a redeclared parameter: a conditional write keeps the arrow possible.
#[test]
fn param_conditional() {
    let body = "{ var Card = () => null; } if (on) Card = null; return READ;";
    case("param_conditional", "Card: any", body, [true, false, true]);
}

/// #501: a default after `Card` in the same pattern reads the element just assigned.
#[test]
fn pattern_default_after() {
    let body = "{ var Card = () => null; } let x: any; [Card, x = READ] = next; return x;";
    case(
        "pattern_default_after",
        "Card: any",
        body,
        [true, true, false],
    );
}

/// A default before `Card` in the pattern still reads the arrow.
#[test]
fn pattern_default_before() {
    let body = "{ var Card = () => null; } let x: any; [x = READ, Card] = next; return x;";
    case(
        "pattern_default_before",
        "Card: any",
        body,
        [true, true, true],
    );
}

/// The iterable is evaluated before its loop binding: it reads the arrow.
#[test]
fn for_iterable_read() {
    let body = "{ var Card = () => null; } for ([Card] of [READ]) {} return null;";
    case("for_iterable_read", "Card: any", body, [true, true, true]);
}

/// #501's case: in a static block, a for-of pattern default reads the element just assigned.
#[test]
fn static_for_default() {
    let tag = "static_for_default";
    let stale = match BUILD {
        Build::Pre488 => false,
        Build::Main499 => true,
        Build::Head => false,
        Build::Main | Build::Main493 => return,
    };
    let g = index(
        tag,
        "declare const next: any;\n\
         export class ViaCall { static { { var Card = () => null; } let x: any; for ([Card, x = Card()] of next) {} } }\n\
         export class ViaJsx { static { { var Card = () => null; } let x: any; for ([Card, x = <Card />] of next) {} } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        if stale {
            assert_one(tag, &g, c, &format!("src/caller/{c}#Card()."));
        } else {
            assert_none(tag, &g, c);
        }
    }
}

/// Guaranteed writes in other syntactic forms replace the arrow too.
#[test]
fn paren_write() {
    let body = "{ var Card = () => null; } (Card = null); return READ;";
    case("paren_write", "Card: any", body, [true, false, false]);
}

#[test]
fn sequence_write() {
    let body = "{ var Card = () => null; } Card = null, 0; return READ;";
    case("sequence_write", "Card: any", body, [true, false, false]);
}

#[test]
fn for_init_write() {
    let body = "{ var Card = () => null; } for (Card = null; false; ) {} return READ;";
    case("for_init_write", "Card: any", body, [true, false, false]);
}

/// A `let` in a nested block: its own writes decide its value.
#[test]
fn nested_let_reassigned() {
    let body = "{ let Card = () => null; Card = null; return READ; }";
    case("nested_let_reassigned", "", body, [true, true, false]);
}

/// Conditional writes keep the arrow possible even when there are more than 32.
#[test]
fn many_conditional_writes() {
    let body = format!(
        "let Card = () => null; {} return READ;",
        "if (on) Card = null; ".repeat(33)
    );
    case("many_conditional_writes", "", &body, [true, true, true]);
}

/// A class field initializer runs at construction, after the `let` arrow exists: the edge stays
/// on every build.
#[test]
fn class_field_reads_later() {
    let tag = "class_field_reads_later";
    if matches!(BUILD, Build::Main | Build::Main493) {
        return;
    }
    let g = index(
        tag,
        "export function ViaCall() { class CCall { value = Card(); } let Card = () => null; return new CCall(); }\n\
         export function ViaJsx() { class CJsx { value = <Card />; } let Card = () => null; return new CJsx(); }\n",
        &CALLERS,
    );
    let to_card = g
        .targets
        .iter()
        .filter(|t| t.as_str() == "ts-tsx . . . src/caller/Card().")
        .count();
    assert_eq!(to_card, 2, "{tag}: {:?}", g.targets);
}

/// `x = c ? x : y` may keep the arrow: the edge stays.
#[test]
fn self_kept() {
    let body = "let Card = () => null; Card = on ? Card : null; return READ;";
    case("self_kept", "", body, [true, true, true]);
}

/// A write in a declaration's initializer always runs.
#[test]
fn declarator_value_write() {
    let body = "{ var Card = () => null; } const ignored = (Card = null); return READ;";
    case(
        "declarator_value_write",
        "Card: any",
        body,
        [true, false, false],
    );
}

/// Edges to the local arrow from classes `C<n>` reading `READ` in `body`, counted over the
/// whole file; `want` per build (Pre488, Main499, Head).
fn class_case(tag: &str, body: &str, want: [usize; 3]) {
    let want = match BUILD {
        Build::Pre488 => want[0],
        Build::Main499 => want[1],
        Build::Head => want[2],
        Build::Main | Build::Main493 => return,
    };
    let g = index(
        tag,
        &format!(
            "export function ViaCall(Card: any) {{ {} }}\n\
             export function ViaJsx(Card: any) {{ {} }}\n",
            body.replace("READ", "Card()").replace("CLS", "CCall"),
            body.replace("READ", "<Card />").replace("CLS", "CJsx"),
        ),
        &CALLERS,
    );
    let to_card = g
        .targets
        .iter()
        .filter(|t| t.as_str() == "ts-tsx . . . src/caller/Card().")
        .count();
    assert_eq!(to_card, want, "{tag}: {:?}", g.targets);
}

/// A static field initializer runs where its class stands: after the arrow, it reads it.
#[test]
fn static_field_after() {
    let body = "Card = null; { var Card = () => null; } class CLS { static value = READ; }";
    class_case("static_field_after", body, [2, 2, 2]);
}

/// ... and before the arrow, it reads the supplied parameter.
#[test]
fn static_field_before() {
    let body = "class CLS { static value = READ; } { var Card = () => null; }";
    class_case("static_field_before", body, [2, 0, 0]);
}

/// #495: a strict-mode block function is reachable only inside its block.
#[test]
fn hidden_block_function() {
    if BUILD != Build::Head {
        return;
    }
    let g = index(
        "hidden_block_function",
        "export function ViaCall() { { function Card() {} } return Card(); }\n\
         export function ViaJsx() { { function Card() {} } return <Card />; }\n\
         export function Inside() { function Card() {} return Card(); }\n",
        &["ViaCall", "ViaJsx", "Inside"],
    );
    // No edge to the block-scoped `caller.Card`; a free name keeps the ordinary cross-file
    // name fallback, as any unbound name does.
    for c in CALLERS {
        assert_one("hidden_block_function", &g, c, "src/other/Card().");
    }
    assert_one("hidden_block_function", &g, "Inside", "src/caller/Card().");
}

/// #495: a body var is invisible while default parameters are evaluated.
#[test]
fn body_var_in_parameter_default() {
    if BUILD != Build::Head {
        return;
    }
    let g = index(
        "body_var_in_parameter_default",
        "export function ViaCall(x = Card()) { var Card = () => null; return x; }\n\
         export function ViaJsx(x = <Card />) { var Card = () => null; return x; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_one("body_var_in_parameter_default", &g, c, "src/other/Card().");
    }
}

/// #494: static-block aliases follow the imported binding, including TS wrappers.
#[test]
fn static_import_value_alias() {
    if BUILD != Build::Head {
        return;
    }
    let g = index(
        "static_import_value_alias",
        "import { Card as Selected } from './other';\n\
         export class ViaCall { static { { var Card = Selected; } Card(); } }\n\
         export class ViaJsx { static { \
         { var Card = (Selected! as any) satisfies any; } void (<Card />); } }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_one("static_import_value_alias", &g, c, "src/other/Card().");
    }
}

/// #494: body declarators and assignments can alias an imported value.
#[test]
fn body_import_value_alias() {
    if BUILD != Build::Head {
        return;
    }
    let g = index(
        "body_import_value_alias",
        "import { Card as Selected } from './other';\n\
         export function ViaCall() { var Card = Selected; return Card(); }\n\
         export function ViaJsx() { var Card; Card = (Selected); return <Card />; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_one("body_import_value_alias", &g, c, "src/other/Card().");
    }
}

/// #494: aliases join the initializer's local definition rather than a global homonym.
#[test]
fn local_value_alias() {
    if BUILD != Build::Head {
        return;
    }
    let g = index(
        "local_value_alias",
        "export function ViaCall() { const Local = () => null; var Card = Local; return Card(); }\n\
         export function ViaJsx() { const Local = () => null; \
         let Selected = Local; var Card = Selected; return <Card />; }\n",
        &CALLERS,
    );
    for c in CALLERS {
        assert_one("local_value_alias", &g, c, "src/caller/Local().");
    }
    for (tag, body, want) in [
        (
            "alias_snapshot",
            "const Local = () => null; let Selected = Local; var Card = Selected; \
             Selected = null; return Card();",
            Some("src/caller/Local()."),
        ),
        (
            "alias_four_hops",
            "const Local = () => null; const D = Local; const C = D; const B = C; \
             var Card = B; return Card();",
            Some("src/caller/Local()."),
        ),
        (
            "alias_five_hops",
            "const Local = () => null; const E = Local; const D = E; const C = D; \
             const B = C; var Card = B; return Card();",
            None,
        ),
        (
            "alias_cycle",
            "const Card = Next; const Next = Card; return Card();",
            None,
        ),
        (
            "alias_multiple_values",
            "var Card = null; if (on) Card = Selected; return Card();",
            None,
        ),
        (
            "alias_local_shadow",
            "const Selected = null; var Card = Selected; return Card();",
            None,
        ),
        (
            "alias_destructured_value",
            "const { Card } = Selected; return Card();",
            None,
        ),
    ] {
        let g = index(
            tag,
            &format!(
                "import {{ Card as Selected }} from './other'; \
                 export function ViaCall() {{ {body} }}",
            ),
            &["ViaCall"],
        );
        if let Some(path) = want {
            assert_one(tag, &g, "ViaCall", path);
        } else {
            assert_none(tag, &g, "ViaCall");
        }
    }
}

fn head_write_case(tag: &str, body: &str, arrow: bool) {
    if BUILD != Build::Head {
        return;
    }
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
        if arrow {
            assert_one(tag, &g, c, "src/caller/Card().");
        } else {
            assert_none(tag, &g, c);
        }
    }
}

/// #504: later iterations can read a write after the call in source order.
#[test]
fn loop_back_edge() {
    for (tag, body) in [
        (
            "loop_back_edge",
            "for (let i = 0; i < 2; i++) { if (i) void (READ); var Card = () => null; }",
        ),
        (
            "for_of_back_edge",
            "for (const i of [0, 1]) { if (i) void (READ); var Card = () => null; }",
        ),
        (
            "while_back_edge",
            "let i = 0; while (i++ < 2) { if (i > 1) void (READ); var Card = () => null; }",
        ),
        (
            "do_back_edge",
            "let i = 0; do { if (i) void (READ); var Card = () => null; } while (i++ < 1);",
        ),
        (
            "loop_init_does_not_kill_back_edge",
            "for (Card = null; on;) { void (READ); var Card = () => null; }",
        ),
    ] {
        head_write_case(tag, body, true);
    }
    head_write_case(
        "loop_back_edge_killed",
        "for (;;) { Card = null; void (READ); var Card = () => null; }",
        false,
    );
}

/// #504: a finally write is guaranteed before a call after its try statement.
#[test]
fn finally_write() {
    head_write_case(
        "finally_write",
        "var Card = () => null; try {} finally { { Card = null; } } return READ;",
        false,
    );
}

/// #504: 38 conditional writes cannot revive the arrow killed by the preceding write.
#[test]
fn many_writes_after_kill() {
    let body = format!(
        "var Card = () => null; Card = null; {} return READ;",
        "if (on) Card = null; ".repeat(38)
    );
    head_write_case("many_writes_after_kill", &body, false);
}

/// #504: a nested element takes effect before a sibling's default, but after an outer default.
#[test]
fn nested_pattern_default() {
    head_write_case(
        "nested_pattern_inner",
        "var Card = () => null; let x; [[Card, x = READ] = [null]] = next; return x;",
        false,
    );
    head_write_case(
        "nested_pattern_outer",
        "var Card = () => null; let x; [[Card, x] = [READ]] = next; return x;",
        true,
    );
}
