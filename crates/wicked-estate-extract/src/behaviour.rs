//! Body-shape classification: is a definition real behaviour, or a stub, a data shape or test
//! support? (wicked-core#1375, #1235; the requirements-graph measurement on AutoGPT found about 30%
//! of the units its coverage share counted were not behaviour.)
//!
//! The answer is recorded on the definition's node as two metadata keys:
//!
//! - [`BEHAVIOUR_KEY`] (`"behaviour"`): `"real"`, `"stub"`, `"data"` or `"test_support"`.
//! - [`BEHAVIOUR_REASON_KEY`] (`"behaviour_reason"`): the shape that decided it (absent for `real`).
//!
//! Consumers (wicked-core's requirements denominator) leave `stub`, `data` and `test_support` units
//! out of the behaviour count. Leaving out a real behaviour would inflate coverage, so every rule
//! here is a POSITIVE match on a known parse shape. Anything not recognised is `real`, and a
//! language or kind this module does not inspect gets no key at all (read as real).
//!
//! Inspected: callables and classes/structs in Python, TypeScript, TSX, JavaScript and Rust.

use std::sync::LazyLock;

use tree_sitter::Node as TsNode;

/// Metadata key: the unit's behaviour class (`real` | `stub` | `data` | `test_support`).
pub const BEHAVIOUR_KEY: &str = "behaviour";
/// Metadata key: why the unit is not `real` (a short snake_case code).
pub const BEHAVIOUR_REASON_KEY: &str = "behaviour_reason";

/// The class one definition falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// Executable logic (or a shape this module does not recognise).
    Real,
    /// A body that does nothing yet: empty, `pass`, `...`, `NotImplementedError`, `todo!()`.
    Stub(&'static str),
    /// A data shape: fields only, an empty class, a constructor that only stores its arguments.
    Data(&'static str),
    /// Test code: a test directory or file, or a Rust `#[cfg(test)]` / `#[test]` item.
    TestSupport(&'static str),
}

impl Behaviour {
    /// The `behaviour` value.
    #[must_use]
    pub fn class(self) -> &'static str {
        match self {
            Behaviour::Real => "real",
            Behaviour::Stub(_) => "stub",
            Behaviour::Data(_) => "data",
            Behaviour::TestSupport(_) => "test_support",
        }
    }

    /// The `behaviour_reason` value (`None` for real behaviour).
    #[must_use]
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Behaviour::Real => None,
            Behaviour::Stub(r) | Behaviour::Data(r) | Behaviour::TestSupport(r) => Some(r),
        }
    }

    /// Write both keys onto a node's metadata.
    pub fn stamp(self, metadata: &mut serde_json::Map<String, serde_json::Value>) {
        metadata.insert(BEHAVIOUR_KEY.to_string(), self.class().into());
        if let Some(r) = self.reason() {
            metadata.insert(BEHAVIOUR_REASON_KEY.to_string(), r.into());
        }
    }
}

/// A test directory: a whole segment (`tests/`, `e2e/`, `__mocks__/`, …), a named kind of test
/// (`integration_tests/`, `unit-tests/`, `load_test/`) or a test-helper segment (`test_utils/`,
/// `test-helpers/`). An arbitrary `*_tests/` or `*_e2e/` is NOT: `ab_tests/`,
/// `statistical_tests/` and `crypto_e2e/` are product code (wicked-core decides a `*_e2e/` from
/// the files beside it).
static TEST_DIR: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(^|/)(tests?|__tests__|__mocks__|specs?|e2e|fixtures?|testdata|(unit|integration|functional|system|acceptance|smoke|regression|contract|component|end[_-]to[_-]end|api|ui|perf|load|stress)[_-]tests?|tests?[_-](utils?|helpers?|support|fixtures?|common))/",
    )
    .expect("test-dir pattern compiles")
});
/// A test file by name (case kept: `Contest.py` is not a test).
static TEST_FILE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\.(test|spec)\.[^/]+$|(^|/)test_[^/]+\.py$|(^|/)tests?\.py$|(^|/)conftest\.py$|(^|/)tests\.rs$|_test\.(py|go|rs)$|_tests\.rs$",
    )
    .expect("test-file pattern compiles")
});

/// Whether a repo-relative path is test code by directory or file name. Public so a consumer can
/// apply the same rule to a node indexed before the key existed.
#[must_use]
pub fn test_path_reason(path: &str) -> Option<&'static str> {
    let norm = path.replace('\\', "/");
    if TEST_DIR.is_match(&norm) {
        Some("test_dir")
    } else if TEST_FILE.is_match(&norm) {
        Some("test_file")
    } else {
        None
    }
}

/// The node kinds (`PendingDef.kind`) this module inspects.
fn inspected_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function" | "method" | "constructor" | "class" | "struct"
    )
}

/// Classify one definition. `start..end` is the byte range of the def's anchor node in `tree`.
/// `None` when the language or kind is not inspected (no key is written).
#[must_use]
pub fn classify(
    lang: &str,
    path: &str,
    kind: &str,
    tree: &tree_sitter::Tree,
    start: usize,
    end: usize,
    src: &[u8],
) -> Option<Behaviour> {
    let family = match lang {
        "python" => Family::Python,
        "typescript" | "tsx" | "javascript" => Family::Ts,
        "rust" => Family::Rust,
        _ => return None,
    };
    if !inspected_kind(kind) {
        return None;
    }
    if let Some(r) = test_path_reason(path) {
        return Some(Behaviour::TestSupport(r));
    }
    let def_kinds: &[&str] = match family {
        Family::Python => &["function_definition", "class_definition"],
        Family::Ts => &[
            "function_declaration",
            "generator_function_declaration",
            "method_definition",
            "variable_declarator",
            "public_field_definition",
            "field_definition",
            "class_declaration",
            "abstract_class_declaration",
            "class",
        ],
        Family::Rust => &["function_item", "struct_item"],
    };
    let Some(node) = anchor(tree, start, end, def_kinds) else {
        return Some(Behaviour::Real);
    };
    Some(match family {
        Family::Python => python(node, src),
        Family::Ts => ts(node, src),
        Family::Rust => rust(node, src),
    })
}

enum Family {
    Python,
    Ts,
    Rust,
}

/// The def's syntax node: the innermost node spanning exactly `start..end` whose kind is one of
/// `kinds`. Several nodes can share the range (a class body `block` holding one method spans
/// exactly that method), so the walk goes up through every same-range node.
fn anchor<'t>(
    tree: &'t tree_sitter::Tree,
    start: usize,
    end: usize,
    kinds: &[&str],
) -> Option<TsNode<'t>> {
    let mut n = tree.root_node().descendant_for_byte_range(start, end)?;
    loop {
        if n.start_byte() != start || n.end_byte() != end {
            return None;
        }
        if kinds.contains(&n.kind()) {
            return Some(n);
        }
        n = n.parent()?;
    }
}

fn text<'a>(n: TsNode<'_>, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

fn named_children(n: TsNode<'_>) -> Vec<TsNode<'_>> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

/// Most nodes a value check walks before it gives up (and answers "not a plain shape").
const WALK_BUDGET: usize = 4096;
/// Deepest nesting a value check follows before it answers "not a plain shape".
const MAX_DEPTH: usize = 12;

/// Whether `n` or any descendant has one of `kinds`. Iterative, with a node budget: a tree too big
/// to walk answers `true`, which every caller reads as "keep it real".
fn contains_kind(n: TsNode<'_>, kinds: &[&str]) -> bool {
    let mut cursor = n.walk();
    let mut seen = 0usize;
    loop {
        seen += 1;
        if seen > WALK_BUDGET || kinds.contains(&cursor.node().kind()) {
            return true;
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.node() == n {
                return false;
            }
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
}

// ── Python ──────────────────────────────────────────────────────────────────

fn python(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    match node.kind() {
        "function_definition" => py_function(node, src),
        "class_definition" => py_class(node, src),
        _ => Behaviour::Real,
    }
}

/// A block's statements without comments and a leading docstring.
fn py_statements(block: TsNode<'_>) -> Vec<TsNode<'_>> {
    let mut out: Vec<TsNode<'_>> = named_children(block)
        .into_iter()
        .filter(|s| s.kind() != "comment")
        .collect();
    if out.first().is_some_and(|s| py_is_docstring(*s)) {
        out.remove(0);
    }
    out
}

/// A leading string literal. An f-string with an interpolation runs code, so it is no docstring.
fn py_is_docstring(s: TsNode<'_>) -> bool {
    s.kind() == "expression_statement"
        && s.named_child_count() == 1
        && s.named_child(0).is_some_and(|e| {
            matches!(e.kind(), "string" | "concatenated_string")
                && !contains_kind(e, &["interpolation"])
        })
}

/// The stub shape of a function body, if any.
fn py_stub_body(block: TsNode<'_>, src: &[u8]) -> Option<&'static str> {
    let stmts = py_statements(block);
    if stmts.is_empty() {
        return Some("empty_body");
    }
    if stmts.iter().all(|s| {
        s.kind() == "pass_statement"
            || (s.kind() == "expression_statement"
                && s.named_child_count() == 1
                && s.named_child(0).is_some_and(|e| e.kind() == "ellipsis"))
    }) {
        return Some("pass_only");
    }
    if stmts.len() == 1
        && stmts[0].kind() == "raise_statement"
        && py_raises_not_implemented(stmts[0], src)
    {
        return Some("not_implemented");
    }
    None
}

/// `raise NotImplementedError` / `raise NotImplementedError("message")`: no cause, and the
/// message runs nothing (a literal, or an f-string of names: `f"{self.__class__.__name__} ..."`).
fn py_raises_not_implemented(raise: TsNode<'_>, src: &[u8]) -> bool {
    if raise.child_by_field_name("cause").is_some() || raise.named_child_count() != 1 {
        return false;
    }
    let Some(e) = raise.named_child(0) else {
        return false;
    };
    let (callee, args) = match e.kind() {
        "identifier" => (e, None),
        "call" => match (
            e.child_by_field_name("function"),
            e.child_by_field_name("arguments"),
        ) {
            (Some(f), Some(a)) => (f, Some(a)),
            _ => return false,
        },
        _ => return false,
    };
    let args_ok = args.is_none_or(|a| {
        let xs: Vec<_> = named_children(a)
            .into_iter()
            .filter(|x| x.kind() != "comment")
            .collect();
        xs.len() <= 1 && xs.iter().all(|x| py_message(*x))
    });
    callee.kind() == "identifier" && text(callee, src) == "NotImplementedError" && args_ok
}

/// A message string whose interpolations (if any) are names or attribute chains only.
fn py_message(v: TsNode<'_>) -> bool {
    match v.kind() {
        "string" => named_children(v).into_iter().all(|c| {
            c.kind() != "interpolation"
                || c.child_by_field_name("expression")
                    .is_some_and(|x| py_name_chain(x, 0))
        }),
        "concatenated_string" => named_children(v).into_iter().all(py_message),
        _ => false,
    }
}

/// `a` / `a.b.c`.
fn py_name_chain(v: TsNode<'_>, depth: usize) -> bool {
    depth <= MAX_DEPTH
        && match v.kind() {
            "identifier" => true,
            "attribute" => v
                .child_by_field_name("object")
                .is_some_and(|o| py_name_chain(o, depth + 1)),
            _ => false,
        }
}

/// A signature that runs code when the def is evaluated or called: a call or lambda in a default
/// or an annotation (`def acquire(token=issue_token())`).
fn py_signature_runs_code(def: TsNode<'_>) -> bool {
    def.child_by_field_name("parameters")
        .is_some_and(|p| contains_kind(p, &["call", "lambda", "await"]))
}

/// The decorators on a definition (empty when it has none).
fn py_decorators(def: TsNode<'_>) -> Vec<TsNode<'_>> {
    def.parent()
        .filter(|p| p.kind() == "decorated_definition")
        .map(|p| {
            named_children(p)
                .into_iter()
                .filter(|c| c.kind() == "decorator")
                .collect()
        })
        .unwrap_or_default()
}

/// The dotted name of a decorator (`@abc.abstractmethod` → `abc.abstractmethod`), and whether it
/// is called (`@dataclass(frozen=True)`).
fn py_decorator_head<'t, 's>(
    d: TsNode<'t>,
    src: &'s [u8],
) -> Option<(&'s str, Option<TsNode<'t>>)> {
    let e = d.named_child(0)?;
    match e.kind() {
        "identifier" | "attribute" if py_name_chain(e, 0) => Some((text(e, src), None)),
        "call" => {
            let f = e.child_by_field_name("function")?;
            py_name_chain(f, 0).then(|| (text(f, src), e.child_by_field_name("arguments")))
        }
        _ => None,
    }
}

/// Method decorators that change how a stub is bound or declared, never what it does.
fn py_inert_method_decorator(d: TsNode<'_>, src: &[u8]) -> bool {
    let Some((head, args)) = py_decorator_head(d, src) else {
        return false;
    };
    args.is_none()
        && (matches!(
            head,
            "abstractmethod"
                | "abc.abstractmethod"
                | "abstractproperty"
                | "abc.abstractproperty"
                | "overload"
                | "typing.overload"
                | "override"
                | "typing.override"
                | "typing_extensions.override"
                | "property"
                | "staticmethod"
                | "classmethod"
                | "cached_property"
                | "functools.cached_property"
        ) || head.ends_with(".setter")
            || head.ends_with(".getter")
            || head.ends_with(".deleter"))
}

fn py_function(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    let Some(body) = node.child_by_field_name("body") else {
        return Behaviour::Real;
    };
    // A decorator can supply the behaviour (`@heartbeat def tick(): pass`), and a default can run
    // code: only inert decorators and a plain signature leave the body to decide.
    if py_signature_runs_code(node)
        || !py_decorators(node)
            .into_iter()
            .all(|d| py_inert_method_decorator(d, src))
    {
        return Behaviour::Real;
    }
    if let Some(r) = py_stub_body(body, src) {
        return Behaviour::Stub(r);
    }
    let is_init = node
        .child_by_field_name("name")
        .is_some_and(|n| text(n, src) == "__init__");
    if is_init && py_owner_class(node).is_some() && py_field_only_init(body, src) {
        return Behaviour::Data("field_only_constructor");
    }
    Behaviour::Real
}

/// The class a method is defined in (`def` → its block → the class), past a decorator.
fn py_owner_class(def: TsNode<'_>) -> Option<TsNode<'_>> {
    let mut up = def.parent()?;
    if up.kind() == "decorated_definition" {
        up = up.parent()?;
    }
    let class = up.parent()?;
    (up.kind() == "block" && class.kind() == "class_definition").then_some(class)
}

/// An `__init__` body of only `self.x = <simple>` stores and `super().__init__(<simple>)`.
/// Delegating to the base constructor hides nothing: the base's own `__init__` is a unit and is
/// classified on its own body.
fn py_field_only_init(body: TsNode<'_>, src: &[u8]) -> bool {
    let stmts = py_statements(body);
    !stmts.is_empty()
        && stmts.iter().all(|s| {
            if s.kind() == "pass_statement" {
                return true;
            }
            if s.kind() != "expression_statement" || s.named_child_count() != 1 {
                return false;
            }
            let Some(e) = s.named_child(0) else {
                return false;
            };
            match e.kind() {
                "assignment" => {
                    let left_ok = e.child_by_field_name("left").is_some_and(|l| {
                        l.kind() == "attribute"
                            && l.child_by_field_name("object")
                                .is_some_and(|o| o.kind() == "identifier" && text(o, src) == "self")
                    });
                    // An annotation-only `self.x: T` has no right side; that is a store too.
                    let right_ok = e
                        .child_by_field_name("right")
                        .is_none_or(|r| py_simple_value(r));
                    left_ok && right_ok
                }
                "call" => py_is_super_init(e, src),
                _ => false,
            }
        })
}

/// A name, an attribute chain, a literal or an empty container: storing it runs no logic.
fn py_simple_value(v: TsNode<'_>) -> bool {
    match v.kind() {
        "identifier" | "integer" | "float" | "true" | "false" | "none" => true,
        "attribute" => py_name_chain(v, 0),
        "string" | "concatenated_string" => !contains_kind(v, &["interpolation"]),
        "unary_operator" => v
            .child_by_field_name("argument")
            .is_some_and(|a| matches!(a.kind(), "integer" | "float")),
        "list" | "dictionary" | "tuple" | "set" => v.named_child_count() == 0,
        _ => false,
    }
}

/// `super().__init__(<simple args>)` (also `*args`, `**kwargs`, `k=<simple>`).
fn py_is_super_init(call: TsNode<'_>, src: &[u8]) -> bool {
    let Some(f) = call.child_by_field_name("function") else {
        return false;
    };
    if f.kind() != "attribute" {
        return false;
    }
    let obj_ok = f
        .child_by_field_name("object")
        .is_some_and(|o| o.kind() == "call" && text(o, src).replace(' ', "") == "super()");
    let attr_ok = f
        .child_by_field_name("attribute")
        .is_some_and(|a| text(a, src) == "__init__");
    let args_ok = call.child_by_field_name("arguments").is_some_and(|args| {
        named_children(args).into_iter().all(|a| match a.kind() {
            "keyword_argument" => a.child_by_field_name("value").is_some_and(py_simple_value),
            "list_splat" | "dictionary_splat" => {
                a.named_child(0).is_some_and(|x| x.kind() == "identifier")
            }
            "comment" => true,
            _ => py_simple_value(a),
        })
    });
    obj_ok && attr_ok && args_ok
}

/// Class decorators that only generate data plumbing (`__init__`, `__eq__`, ordering), with
/// declarative arguments only (`@dataclass(frozen=True)`).
fn py_dataclass_like_decorator(d: TsNode<'_>, src: &[u8]) -> bool {
    let Some((head, args)) = py_decorator_head(d, src) else {
        return false;
    };
    matches!(
        head,
        "dataclass"
            | "dataclasses.dataclass"
            | "attr.s"
            | "attr.attrs"
            | "attrs.define"
            | "attrs.frozen"
            | "attr.define"
            | "attr.frozen"
            | "functools.total_ordering"
            | "total_ordering"
            | "unique"
            | "enum.unique"
            | "final"
            | "typing.final"
    ) && args.is_none_or(|a| py_decl_args(a, src, 0))
}

/// Calls that DECLARE a field rather than compute one (pydantic, dataclasses, attrs, enum).
const PY_DECL_CALLS: &[&str] = &[
    "Field",
    "field",
    "dataclasses.field",
    "attr.ib",
    "attrs.field",
    "attr.field",
    "PrivateAttr",
    "ConfigDict",
    "SettingsConfigDict",
    "auto",
    "enum.auto",
];

/// A field value that declares a shape and runs no product code: a simple value, a container of
/// declared values, or a declaring call (`Field(default=None, description="…")`) whose arguments
/// are declared values. A `default_factory` must be a plain type (`list`, `dict`, …): any other
/// factory runs code each time the shape is built (`default_factory=time.monotonic`).
fn py_decl_value(v: TsNode<'_>, src: &[u8], depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match v.kind() {
        "ellipsis" => true,
        "list" | "tuple" | "set" => named_children(v)
            .into_iter()
            .all(|c| c.kind() == "comment" || py_decl_value(c, src, depth + 1)),
        "dictionary" => named_children(v).into_iter().all(|p| {
            p.kind() == "comment"
                || (p.kind() == "pair"
                    && p.child_by_field_name("key")
                        .is_some_and(|k| py_decl_value(k, src, depth + 1))
                    && p.child_by_field_name("value")
                        .is_some_and(|x| py_decl_value(x, src, depth + 1)))
        }),
        "call" => {
            let callee_ok = v
                .child_by_field_name("function")
                .is_some_and(|f| py_name_chain(f, 0) && PY_DECL_CALLS.contains(&text(f, src)));
            callee_ok
                && v.child_by_field_name("arguments")
                    .is_some_and(|a| py_decl_args(a, src, depth + 1))
        }
        _ => py_simple_value(v),
    }
}

fn py_decl_args(args: TsNode<'_>, src: &[u8], depth: usize) -> bool {
    named_children(args).into_iter().all(|a| match a.kind() {
        "comment" => true,
        "keyword_argument" => {
            let factory = a
                .child_by_field_name("name")
                .is_some_and(|k| text(k, src) == "default_factory");
            a.child_by_field_name("value").is_some_and(|x| {
                if factory {
                    x.kind() == "identifier"
                        && matches!(
                            text(x, src),
                            "list"
                                | "dict"
                                | "set"
                                | "tuple"
                                | "frozenset"
                                | "str"
                                | "int"
                                | "float"
                                | "bool"
                        )
                } else {
                    py_decl_value(x, src, depth)
                }
            })
        }
        _ => py_decl_value(a, src, depth),
    })
}

/// What a class body holds, for the data / interface tests.
enum PyMember {
    /// A field declaration (`name: T`, `name: T = <declared value>`, `NAME = "v"`).
    Field,
    /// A method whose body is a stub.
    StubMethod,
    /// Anything else (a real method, a statement, a computed value, a decorated non-method).
    Other,
}

fn py_member(s: TsNode<'_>, src: &[u8]) -> PyMember {
    match s.kind() {
        "pass_statement" => PyMember::Field,
        "expression_statement" => {
            let Some(e) = s.named_child(0) else {
                return PyMember::Other;
            };
            match e.kind() {
                "ellipsis" => PyMember::Field,
                "assignment" => {
                    let left_ok = e
                        .child_by_field_name("left")
                        .is_some_and(|l| l.kind() == "identifier");
                    let right_ok = e
                        .child_by_field_name("right")
                        .is_none_or(|r| py_decl_value(r, src, 0));
                    if left_ok && right_ok {
                        PyMember::Field
                    } else {
                        PyMember::Other
                    }
                }
                _ => PyMember::Other,
            }
        }
        "function_definition" => py_method(s, src),
        "decorated_definition" => match s.child_by_field_name("definition") {
            Some(d) if d.kind() == "function_definition" => py_method(d, src),
            // A nested data class keeps its decorators (checked by `py_class`).
            Some(d) if d.kind() == "class_definition" => py_nested_class(d, src),
            _ => PyMember::Other,
        },
        // A nested data class (`class Config:` on a pydantic model) is part of the shape.
        "class_definition" => py_nested_class(s, src),
        _ => PyMember::Other,
    }
}

fn py_nested_class(c: TsNode<'_>, src: &[u8]) -> PyMember {
    match py_class(c, src) {
        Behaviour::Data(_) => PyMember::Field,
        _ => PyMember::Other,
    }
}

fn py_method(f: TsNode<'_>, src: &[u8]) -> PyMember {
    match py_function(f, src) {
        Behaviour::Stub(_) => PyMember::StubMethod,
        _ => PyMember::Other,
    }
}

fn py_class(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    let Some(body) = node.child_by_field_name("body") else {
        return Behaviour::Real;
    };
    // A decorator other than a dataclass-like one may register or wrap the class, and a call in
    // the bases (`metaclass=register()`, `Base(...)`) runs code: keep it real.
    if !py_decorators(node)
        .into_iter()
        .all(|d| py_dataclass_like_decorator(d, src))
        || node
            .child_by_field_name("superclasses")
            .is_some_and(|b| contains_kind(b, &["call", "lambda"]))
    {
        return Behaviour::Real;
    }
    let stmts = py_statements(body);
    if stmts.is_empty()
        || stmts.iter().all(|s| {
            s.kind() == "pass_statement"
                || (s.kind() == "expression_statement"
                    && s.named_child(0).is_some_and(|e| e.kind() == "ellipsis"))
        })
    {
        return Behaviour::Data("empty_class");
    }
    let members: Vec<PyMember> = stmts.iter().map(|s| py_member(*s, src)).collect();
    if members.iter().any(|m| matches!(m, PyMember::Other)) {
        return Behaviour::Real;
    }
    if members.iter().any(|m| matches!(m, PyMember::StubMethod)) {
        Behaviour::Stub("interface_only")
    } else {
        Behaviour::Data("fields_only_class")
    }
}

// ── TypeScript / TSX / JavaScript ───────────────────────────────────────────

fn ts(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" | "method_definition" => {
            ts_callable(node, src)
        }
        // `const f = () => {}` / `handler = () => {}`: the arrow function is the value.
        "variable_declarator" | "public_field_definition" | "field_definition" => {
            match node.child_by_field_name("value") {
                Some(v) if matches!(v.kind(), "arrow_function" | "function_expression") => {
                    ts_callable(v, src)
                }
                _ => Behaviour::Real,
            }
        }
        "class_declaration" | "abstract_class_declaration" | "class" => ts_class(node, src),
        _ => Behaviour::Real,
    }
}

/// Statements of a `statement_block`, without comments and empty statements.
fn ts_statements(block: TsNode<'_>) -> Vec<TsNode<'_>> {
    named_children(block)
        .into_iter()
        .filter(|s| !matches!(s.kind(), "comment" | "empty_statement"))
        .collect()
}

fn ts_stub_body(block: TsNode<'_>, src: &[u8]) -> Option<&'static str> {
    let stmts = ts_statements(block);
    if stmts.is_empty() {
        return Some("empty_body");
    }
    if stmts.len() == 1 && ts_throws_not_implemented(stmts[0], src) {
        return Some("not_implemented");
    }
    None
}

/// `throw new Error("not implemented")` exactly: the constructor is `Error` or
/// `NotImplementedError`, and the one argument is a literal saying it is not implemented.
fn ts_throws_not_implemented(s: TsNode<'_>, src: &[u8]) -> bool {
    if s.kind() != "throw_statement" {
        return false;
    }
    let Some(e) = s.named_child(0).filter(|e| e.kind() == "new_expression") else {
        return false;
    };
    let ctor_ok = e.child_by_field_name("constructor").is_some_and(|c| {
        c.kind() == "identifier" && matches!(text(c, src), "Error" | "NotImplementedError")
    });
    let msg_ok = e.child_by_field_name("arguments").is_some_and(|a| {
        let xs = named_children(a);
        xs.len() == 1
            && match xs[0].kind() {
                "string" => true,
                "template_string" => !contains_kind(xs[0], &["template_substitution"]),
                _ => false,
            }
            && {
                let t = text(xs[0], src).to_ascii_lowercase();
                t.contains("not implemented") || t.contains("not yet implemented")
            }
    });
    ctor_ok && msg_ok
}

/// Whether a member of a class body carries the `static` keyword.
fn ts_is_static(member: TsNode<'_>) -> bool {
    (0..member.child_count())
        .filter_map(|i| member.child(i))
        .any(|c| c.kind() == "static")
}

/// An instance constructor of a class (not a static method or an object-literal method that
/// happens to be named `constructor`).
fn ts_is_constructor(node: TsNode<'_>, src: &[u8]) -> bool {
    node.kind() == "method_definition"
        && node
            .child_by_field_name("name")
            .is_some_and(|n| n.kind() == "property_identifier" && text(n, src) == "constructor")
        && node.parent().is_some_and(|p| p.kind() == "class_body")
        && !ts_is_static(node)
}

/// A parameter list that runs code on a call: a call, `new`, `await` or a function in a default
/// (`function acquire(token = issueToken()) {}`).
fn ts_signature_runs_code(node: TsNode<'_>) -> bool {
    node.child_by_field_name("parameters").is_some_and(|p| {
        contains_kind(
            p,
            &[
                "call_expression",
                "new_expression",
                "await_expression",
                "arrow_function",
                "function_expression",
            ],
        )
    })
}

fn ts_callable(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    let Some(body) = node.child_by_field_name("body") else {
        return Behaviour::Real;
    };
    if body.kind() != "statement_block" || ts_signature_runs_code(node) {
        // An expression-bodied arrow function computes its value; a default can run code.
        return Behaviour::Real;
    }
    let ctor = ts_is_constructor(node, src);
    if let Some(r) = ts_stub_body(body, src) {
        // `constructor(private readonly a: A) {}` stores its parameters: a data shape.
        if ctor && r == "empty_body" && ts_has_parameter_properties(node) {
            return Behaviour::Data("field_only_constructor");
        }
        return Behaviour::Stub(r);
    }
    if ctor && ts_field_only_ctor(body) {
        return Behaviour::Data("field_only_constructor");
    }
    Behaviour::Real
}

fn ts_has_parameter_properties(ctor: TsNode<'_>) -> bool {
    // `private x`, `readonly x`, `override x`: the modifier makes the parameter a stored field.
    // `readonly` is an anonymous token on the parameter, so all children are walked.
    ctor.child_by_field_name("parameters").is_some_and(|ps| {
        named_children(ps).into_iter().any(|p| {
            (0..p.child_count()).filter_map(|i| p.child(i)).any(|ch| {
                matches!(
                    ch.kind(),
                    "accessibility_modifier" | "readonly" | "override_modifier"
                )
            })
        })
    })
}

/// A constructor body of only `this.x = <simple>` stores and `super(<simple>)`. Delegating to the
/// base constructor hides nothing: the base's constructor is a unit of its own.
fn ts_field_only_ctor(body: TsNode<'_>) -> bool {
    let stmts = ts_statements(body);
    !stmts.is_empty()
        && stmts.iter().all(|s| {
            if s.kind() != "expression_statement" {
                return false;
            }
            let Some(e) = s.named_child(0) else {
                return false;
            };
            match e.kind() {
                "assignment_expression" => {
                    let left_ok = e.child_by_field_name("left").is_some_and(|l| {
                        l.kind() == "member_expression"
                            && l.child_by_field_name("object")
                                .is_some_and(|o| o.kind() == "this")
                    });
                    let right_ok = e
                        .child_by_field_name("right")
                        .is_some_and(|r| ts_simple_value(r, 0));
                    left_ok && right_ok
                }
                "call_expression" => {
                    e.child_by_field_name("function")
                        .is_some_and(|f| f.kind() == "super")
                        && e.child_by_field_name("arguments").is_some_and(|a| {
                            named_children(a).into_iter().all(|x| {
                                ts_simple_value(x, 0)
                                    || (x.kind() == "spread_element"
                                        && x.named_child(0)
                                            .is_some_and(|i| i.kind() == "identifier"))
                            })
                        })
                }
                _ => false,
            }
        })
}

/// A name, a member chain, a literal, or a container of those: storing it runs no logic.
fn ts_simple_value(v: TsNode<'_>, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match v.kind() {
        "identifier" | "number" | "true" | "false" | "null" | "undefined" | "this" | "string" => {
            true
        }
        "template_string" => !contains_kind(v, &["template_substitution"]),
        "member_expression" => {
            v.child_by_field_name("object")
                .is_some_and(|o| ts_simple_value(o, depth + 1))
                && v.child_by_field_name("property")
                    .is_some_and(|p| p.kind() == "property_identifier")
        }
        "array" => named_children(v)
            .into_iter()
            .all(|c| c.kind() == "comment" || ts_simple_value(c, depth + 1)),
        "object" => named_children(v).into_iter().all(|p| {
            p.kind() == "comment"
                || p.kind() == "shorthand_property_identifier"
                || (p.kind() == "pair"
                    && p.child_by_field_name("key").is_some_and(|k| {
                        matches!(k.kind(), "property_identifier" | "string" | "number")
                    })
                    && p.child_by_field_name("value")
                        .is_some_and(|x| ts_simple_value(x, depth + 1)))
        }),
        "unary_expression" => v
            .child_by_field_name("argument")
            .is_some_and(|a| a.kind() == "number"),
        // `x = [] as Item[]`, `x = 0 satisfies N`: the type part runs nothing.
        "as_expression" | "satisfies_expression" | "parenthesized_expression" => v
            .named_child(0)
            .is_some_and(|x| ts_simple_value(x, depth + 1)),
        _ => false,
    }
}

fn ts_class(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    // A decorated class (Angular `@Component`, TypeORM `@Entity`) gets behaviour from the
    // decorator and its template, and a call in `extends` runs code: never a data shape.
    let decorated = named_children(node)
        .into_iter()
        .any(|c| c.kind() == "decorator")
        || node.parent().is_some_and(|p| {
            p.kind() == "export_statement"
                && named_children(p)
                    .into_iter()
                    .any(|c| c.kind() == "decorator")
        });
    let heritage_runs = named_children(node).into_iter().any(|c| {
        c.kind() == "class_heritage" && contains_kind(c, &["call_expression", "new_expression"])
    });
    if decorated || heritage_runs {
        return Behaviour::Real;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return Behaviour::Real;
    };
    let members: Vec<TsNode<'_>> = named_children(body)
        .into_iter()
        .filter(|m| !matches!(m.kind(), "comment" | ";"))
        .collect();
    if members.is_empty() {
        return Behaviour::Data("empty_class");
    }
    let mut stub_method = false;
    for m in &members {
        match m.kind() {
            "public_field_definition" | "field_definition" => {
                // A plain name and a plain initializer (or none): `count = 0`, `name: string`.
                let name_ok = m
                    .child_by_field_name("name")
                    .or_else(|| m.child_by_field_name("property"))
                    .is_some_and(|n| {
                        matches!(
                            n.kind(),
                            "property_identifier" | "private_property_identifier" | "string"
                        )
                    });
                let value_ok = m
                    .child_by_field_name("value")
                    .is_none_or(|v| ts_simple_value(v, 0));
                let decorated = named_children(*m)
                    .into_iter()
                    .any(|c| c.kind() == "decorator");
                if !name_ok || !value_ok || decorated {
                    return Behaviour::Real;
                }
            }
            "index_signature" => {}
            "abstract_method_signature" | "method_signature" => stub_method = true,
            "method_definition" => match ts_callable(*m, src) {
                Behaviour::Data(_) => {}
                Behaviour::Stub(_) if !ts_is_constructor(*m, src) => stub_method = true,
                Behaviour::Stub(_) => {}
                _ => return Behaviour::Real,
            },
            _ => return Behaviour::Real,
        }
    }
    if stub_method {
        Behaviour::Stub("interface_only")
    } else {
        Behaviour::Data("fields_only_class")
    }
}

// ── Rust ────────────────────────────────────────────────────────────────────

fn rust(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    if let Some(r) = rust_test_item(node, src) {
        return Behaviour::TestSupport(r);
    }
    match node.kind() {
        // A struct declares fields; derives generate trait plumbing, not product rules.
        "struct_item" => Behaviour::Data("struct_decl"),
        "function_item" => rust_function(node, src),
        _ => Behaviour::Real,
    }
}

/// The attribute items directly before an item (`#[cfg(test)]`, `#[test]`), skipping comments,
/// whitespace removed.
fn rust_attributes(item: TsNode<'_>, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = item.prev_named_sibling();
    while let Some(s) = cur {
        match s.kind() {
            "attribute_item" => out.push(
                text(s, src)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>(),
            ),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        cur = s.prev_named_sibling();
    }
    out
}

/// Test code inside a product file: the item or an enclosing item carries `#[cfg(test)]`, or the
/// function itself carries `#[test]` / `#[<runtime>::test]`.
fn rust_test_item(node: TsNode<'_>, src: &[u8]) -> Option<&'static str> {
    if node.kind() == "function_item" {
        let attrs = rust_attributes(node, src);
        if attrs
            .iter()
            .any(|a| a == "#[test]" || (a.starts_with("#[") && a.ends_with("::test]")))
        {
            return Some("test_attribute");
        }
    }
    let mut cur = Some(node);
    while let Some(n) = cur {
        if matches!(
            n.kind(),
            "mod_item" | "function_item" | "impl_item" | "struct_item" | "trait_item"
        ) && rust_attributes(n, src).iter().any(|a| a == "#[cfg(test)]")
        {
            return Some("cfg_test_module");
        }
        cur = n.parent();
    }
    None
}

/// Function attributes that never add behaviour (`#[instrument]`, a proc macro, can: keep real).
fn rust_inert_attribute(a: &str) -> bool {
    let inner = a
        .trim_start_matches("#[")
        .trim_end_matches(']')
        .split(['(', '='])
        .next()
        .unwrap_or("");
    matches!(
        inner,
        "inline"
            | "must_use"
            | "allow"
            | "warn"
            | "deny"
            | "expect"
            | "doc"
            | "deprecated"
            | "cold"
            | "track_caller"
            | "cfg"
            | "cfg_attr"
            | "rustfmt::skip"
    )
}

/// `todo!()` / `unimplemented!("message")`: a placeholder whose payload is at most one string
/// literal (a format argument would run code).
static RUST_PLACEHOLDER_ARGS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"^[(\[{]\s*("([^"\\]|\\.)*")?\s*[)\]}]$"#).expect("placeholder pattern")
});

fn rust_function(node: TsNode<'_>, src: &[u8]) -> Behaviour {
    let Some(body) = node.child_by_field_name("body") else {
        return Behaviour::Real;
    };
    if !rust_attributes(node, src)
        .iter()
        .all(|a| rust_inert_attribute(a))
    {
        return Behaviour::Real;
    }
    let items: Vec<TsNode<'_>> = named_children(body)
        .into_iter()
        .filter(|s| !matches!(s.kind(), "line_comment" | "block_comment"))
        .collect();
    if items.is_empty() {
        return Behaviour::Stub("empty_body");
    }
    if items.len() == 1 {
        let mut e = items[0];
        if e.kind() == "expression_statement" {
            match e.named_child(0) {
                Some(inner) => e = inner,
                None => return Behaviour::Real,
            }
        }
        if e.kind() == "macro_invocation" {
            let name = e
                .child_by_field_name("macro")
                .map(|m| text(m, src))
                .unwrap_or("");
            let args_ok = named_children(e)
                .into_iter()
                .find(|c| c.kind() == "token_tree")
                .is_some_and(|t| RUST_PLACEHOLDER_ARGS.is_match(text(t, src)));
            if matches!(name, "todo" | "unimplemented") && args_ok {
                return Behaviour::Stub("not_implemented");
            }
        }
        if e.kind() == "struct_expression" && rust_forwarding_constructor(node, e, src) {
            return Behaviour::Data("field_only_constructor");
        }
    }
    Behaviour::Real
}

/// `fn new(a: A, b: B) -> Self { Self { a, b } }`: a struct literal of the function's own type
/// (`Self` or the impl's type) whose every field forwards a parameter (or is `None`). A literal or
/// a constant value is a decision (`Decision::Denied { reason: "suspended" }`), so it stays real.
fn rust_forwarding_constructor(func: TsNode<'_>, e: TsNode<'_>, src: &[u8]) -> bool {
    let Some(name) = e.child_by_field_name("name") else {
        return false;
    };
    let impl_type = func
        .parent()
        .filter(|p| p.kind() == "declaration_list")
        .and_then(|d| d.parent())
        .filter(|i| i.kind() == "impl_item")
        .and_then(|i| i.child_by_field_name("type"))
        .map(|t| {
            let t = text(t, src);
            t.split('<').next().unwrap_or(t).trim().to_string()
        });
    let own_type = name.kind() == "type_identifier"
        && (text(name, src) == "Self" || impl_type.as_deref() == Some(text(name, src)));
    if !own_type {
        return false;
    }
    let params: Vec<&str> = func
        .child_by_field_name("parameters")
        .map(|ps| {
            named_children(ps)
                .into_iter()
                .filter(|p| p.kind() == "parameter")
                .filter_map(|p| p.child_by_field_name("pattern"))
                .filter(|p| p.kind() == "identifier")
                .map(|p| text(p, src))
                .collect()
        })
        .unwrap_or_default();
    let Some(body) = e.child_by_field_name("body") else {
        return false;
    };
    let fields = named_children(body);
    !fields.is_empty()
        && fields.into_iter().all(|f| match f.kind() {
            "shorthand_field_initializer" => f
                .named_child(0)
                .is_some_and(|i| params.contains(&text(i, src))),
            "field_initializer" => f.child_by_field_name("value").is_some_and(|v| {
                v.kind() == "identifier" && {
                    let t = text(v, src);
                    t == "None" || params.contains(&t)
                }
            }),
            "line_comment" | "block_comment" => true,
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::treesitter::TreeSitterExtractor;
    use wicked_estate_core::{Extractor, Language, SourceFile};

    /// `(key, behaviour, reason)` for every node the extractor tagged. The key is the symbol's
    /// last path segment without its descriptor suffix (`A#__init__`, `Point#new`, `helper`):
    /// node order is not document order, so members are told apart by their owner.
    fn tags(path: &str, lang: &str, code: &str) -> Vec<(String, String, Option<String>)> {
        let ex = TreeSitterExtractor::for_language(lang)
            .expect("language registered")
            .extract(&SourceFile {
                path: path.into(),
                language: Language::new(lang),
                text: code.into(),
            })
            .expect("extracts");
        ex.nodes
            .iter()
            .filter_map(|n| {
                let b = n.metadata.get(BEHAVIOUR_KEY)?.as_str()?.to_string();
                let r = n
                    .metadata
                    .get(BEHAVIOUR_REASON_KEY)
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let sym = n.symbol.as_str();
                let key = sym
                    .rsplit('/')
                    .next()
                    .unwrap_or(sym)
                    .trim_end_matches(['#', '.', '(', ')'])
                    .to_string();
                Some((key, b, r))
            })
            .collect()
    }

    fn tag_of(all: &[(String, String, Option<String>)], name: &str) -> (String, Option<String>) {
        let owned = format!("#{name}");
        let hits: Vec<_> = all
            .iter()
            .filter(|(k, _, _)| k == name || k.ends_with(&owned))
            .collect();
        assert_eq!(hits.len(), 1, "one node named {name}: {all:?}");
        (hits[0].1.clone(), hits[0].2.clone())
    }

    fn is(all: &[(String, String, Option<String>)], name: &str, class: &str, reason: Option<&str>) {
        assert_eq!(
            tag_of(all, name),
            (class.to_string(), reason.map(str::to_string)),
            "{name}"
        );
    }

    #[test]
    fn python_stubs() {
        let code = r#"
def a():
    pass

def b():
    """Doc only."""

def c():
    ...

def d(x):
    """Doc."""
    raise NotImplementedError("later")

def e():
    raise NotImplementedError

def f():
    raise ValueError("not supported")

def g(x):
    return x + 1
"#;
        let t = tags("pkg/m.py", "python", code);
        is(&t, "a", "stub", Some("pass_only"));
        is(&t, "b", "stub", Some("empty_body"));
        is(&t, "c", "stub", Some("pass_only"));
        is(&t, "d", "stub", Some("not_implemented"));
        is(&t, "e", "stub", Some("not_implemented"));
        // Raising a real error is a rule ("X is not supported"), not a stub.
        is(&t, "f", "real", None);
        is(&t, "g", "real", None);
    }

    /// Every class here holds one method, so its body `block` spans exactly that method: the
    /// anchor lookup must find the function, not the block (a first cut read every lone method
    /// as real).
    #[test]
    fn python_field_only_init_and_its_near_misses() {
        let code = r#"
class A:
    def __init__(self, x, y=None, *args, **kwargs):
        """Store."""
        super().__init__(*args, name=x, **kwargs)
        self.x = x
        self.y: int = y
        self.z = None
        self.items = []
        self.cfg = settings.default

class B:
    def __init__(self, x):
        self.x = x or 3

class C:
    def __init__(self, url):
        self.conn = connect(url)

class D:
    def __init__(self, x):
        self.x = x
        if x:
            self.y = 1

class E:
    def setup(self, x):
        self.x = x
"#;
        let t = tags("pkg/m.py", "python", code);
        is(&t, "A#__init__", "data", Some("field_only_constructor"));
        // A default, a call (a side effect) and a branch are all logic.
        for c in ["B", "C", "D"] {
            is(&t, &format!("{c}#__init__"), "real", None);
        }
        // Only `__init__` is a constructor.
        is(&t, "setup", "real", None);
    }

    #[test]
    fn python_data_shapes() {
        let code = r#"
class Empty(Exception):
    pass

class Doc(Exception):
    """Raised when."""

class Model(BaseModel):
    """A shape."""
    name: str
    count: int = 0
    tags: list[str] = Field(default_factory=list)

    class Config:
        extra = "forbid"

class Color(Enum):
    RED = "red"
    BLUE = "blue"

@dataclass(frozen=True)
class Point:
    x: int
    y: int

@register
class Plugin:
    name = "p"

class Lazy:
    key = lambda self: self.name

class Stamped(BaseModel):
    node_id: str
    produced_at: float = Field(default_factory=time.monotonic, exclude=True)

class Validated(BaseModel):
    name: str

    @validator("name")
    def check(cls, v):
        if not v:
            raise ValueError("empty")
        return v

class Port(Protocol):
    def send(self, msg: str) -> None: ...

    def close(self) -> None:
        """Close."""

class Base(ABC):
    @abstractmethod
    def run(self):
        raise NotImplementedError
"#;
        let t = tags("pkg/m.py", "python", code);
        is(&t, "Empty", "data", Some("empty_class"));
        is(&t, "Doc", "data", Some("empty_class"));
        is(&t, "Model", "data", Some("fields_only_class"));
        is(&t, "Config", "data", Some("fields_only_class"));
        is(&t, "Color", "data", Some("fields_only_class"));
        is(&t, "Point", "data", Some("fields_only_class"));
        // A registering decorator, a lambda value and a real method keep the class real.
        is(&t, "Plugin", "real", None);
        is(&t, "Lazy", "real", None);
        // A computed default runs code when the shape is built (codex label on the AutoGPT check).
        is(&t, "Stamped", "real", None);
        is(&t, "Validated", "real", None);
        is(&t, "check", "real", None);
        is(&t, "Port", "stub", Some("interface_only"));
        is(&t, "send", "stub", Some("pass_only"));
        is(&t, "Base", "stub", Some("interface_only"));
        is(&t, "run", "stub", Some("not_implemented"));
    }

    #[test]
    fn test_paths_are_test_support() {
        let t = tags(
            "backend/integration_tests/provider.py",
            "python",
            "def make():\n    return build()\n",
        );
        is(&t, "make", "test_support", Some("test_dir"));
        for p in [
            "a/integration_tests/h.py",
            "a/unit-tests/h.py",
            "a/load_test/h.py",
            "a/test_utils/h.py",
            "a/test-helpers/h.py",
            "a/__mocks__/h.py",
            "tests/h.py",
            "a/e2e/h.py",
        ] {
            assert_eq!(test_path_reason(p), Some("test_dir"), "{p}");
        }
        for p in [
            "a/test_x.py",
            "a/conftest.py",
            "src/tests.rs",
            "src/dispute_tests.rs",
            "a/b.test.ts",
            "a/x_test.go",
        ] {
            assert_eq!(test_path_reason(p), Some("test_file"), "{p}");
        }
        // Product code that only looks like a test word stays product.
        for p in [
            "a/e2e_encryption/keys.py",
            "a/ab_test/assign.py",
            // codex r1: an arbitrary suffix is no evidence (core judges `*_e2e/` by its files).
            "a/ab_tests/assign.py",
            "a/statistical_tests/welch.py",
            "a/crypto_e2e/encrypt.ts",
            "backend/executor/billing_e2e/provider.py",
            "a/latest/h.py",
            "a/contest/h.py",
            "a/testing/h.py",
            "a/test_runner/main.py",
            "src/test.rs",
            "a/Contest.py",
        ] {
            assert_eq!(test_path_reason(p), None, "{p}");
        }
    }

    #[test]
    fn typescript_shapes() {
        let code = r#"
export function a(): void {}
function b(): number { throw new Error("Not implemented"); }
function c(x: number): number { if (x < 0) { throw new Error("negative"); } return x; }
const d = () => {};
const e = (x: number) => x * 2;

class Shape {
  name: string;
  count = 0;
}

class Holder {
  constructor(private readonly svc: Service) {}
}

class Stores extends Base {
  x: number;
  constructor(x: number, y: string) {
    super(x);
    this.x = x;
    this.y = y;
  }
}

class Calls {
  constructor(url: string) {
    this.conn = connect(url);
  }
}

class Live {
  run(): number { return 1; }
}

abstract class Port {
  abstract send(msg: string): void;
}

class Handler {
  onClick = () => { this.n += 1; };
}

@Component({ selector: "x", template: "<b>hi</b>" })
export class Comp {
  title = "x";
}
"#;
        let t = tags("src/m.ts", "typescript", code);
        is(&t, "a", "stub", Some("empty_body"));
        is(&t, "b", "stub", Some("not_implemented"));
        is(&t, "c", "real", None);
        is(&t, "d", "stub", Some("empty_body"));
        is(&t, "e", "real", None);
        is(&t, "Shape", "data", Some("fields_only_class"));
        is(&t, "Holder", "data", Some("fields_only_class"));
        is(&t, "Live", "real", None);
        is(&t, "run", "real", None);
        is(&t, "Port", "stub", Some("interface_only"));
        is(&t, "Handler", "real", None);
        is(&t, "Comp", "real", None);
        // Parameter properties, `this.x = x` stores, and a call (logic) in the constructor.
        is(
            &t,
            "Holder#constructor",
            "data",
            Some("field_only_constructor"),
        );
        is(
            &t,
            "Stores#constructor",
            "data",
            Some("field_only_constructor"),
        );
        is(&t, "Calls#constructor", "real", None);
        is(&t, "Stores", "data", Some("fields_only_class"));
        is(&t, "Calls", "real", None);
    }

    #[test]
    fn javascript_shapes() {
        let code = r#"
class Plain {
  constructor(a) { this.a = a; }
}
class Busy {
  run() { return this.a + 1; }
  noop() {}
}
function later() { throw new Error("not implemented yet"); }
"#;
        let t = tags("src/m.js", "javascript", code);
        is(&t, "Plain", "data", Some("fields_only_class"));
        is(
            &t,
            "Plain#constructor",
            "data",
            Some("field_only_constructor"),
        );
        is(&t, "Busy", "real", None);
        is(&t, "Busy#noop", "stub", Some("empty_body"));
        is(&t, "later", "stub", Some("not_implemented"));
    }

    /// codex r1 on the local diff: shapes that look empty or declarative but run code. Each one
    /// must stay `real` (excluding it would inflate coverage).
    #[test]
    fn executable_look_alikes_stay_real() {
        let py = r#"
def pay():
    f"{charge_card()}"

def deny():
    raise NotImplementedError(revoke_access())

def deny2():
    raise NotImplementedError from revoke_access()

def later():
    raise NotImplementedError(f"{self.__class__.__name__} cannot ping")

def acquire(token=issue_token()):
    pass

@heartbeat
def tick():
    pass

@abstractmethod
def shape():
    pass

def __init__(self, x):
    self.x = x

class Session:
    token = issue_token()

class Policy:
    enabled = balance > 100

class Plugin(metaclass=register_meta()):
    pass

@dataclass(repr=register_schema())
class Rec:
    x: int
"#;
        let t = tags("pkg/m.py", "python", py);
        for name in [
            "pay", "deny", "deny2", "acquire", "tick", "__init__", "Session", "Policy", "Plugin",
            "Rec",
        ] {
            is(&t, name, "real", None);
        }
        // A message of names runs nothing; an inert decorator keeps the stub.
        is(&t, "later", "stub", Some("not_implemented"));
        is(&t, "shape", "stub", Some("pass_only"));

        let ts_code = r#"
function deny() { throw report("new Error: not implemented"); }
function deny2() { throw (revokeAccess(), new Error("not implemented")); }
function deny3() { throw new ErrorPolicy("not implemented"); }
function acquire(token = issueToken()) {}
class Session { token = issueToken(); }
class Registry { static ready = registerPlugin(); }
class Computed { [registerPlugin()] = 0; }
class Plugin extends registerBase() {}
class Ledger { static constructor(value) { this.total = value; } }
"#;
        let t = tags("src/m.ts", "typescript", ts_code);
        for name in [
            "deny", "deny2", "deny3", "acquire", "Session", "Registry", "Computed", "Plugin",
            "Ledger",
        ] {
            is(&t, name, "real", None);
        }
        is(&t, "Ledger#constructor", "real", None);

        let rs = r#"
struct Decision;
impl Decision {
    fn authorize() -> Decision { Decision::Denied { reason: "account suspended" } }
}
struct Limits { max_transfer: u32 }
impl Limits {
    fn limits() -> Limits { Limits { max_transfer: 1000 } }
    fn deny(&self) { todo!("{}", revoke_access()) }
    #[instrument]
    fn operation(&self) {}
}
"#;
        let t = tags("src/m.rs", "rust", rs);
        for name in ["authorize", "limits", "deny", "operation"] {
            is(&t, name, "real", None);
        }
    }

    #[test]
    fn tsx_shapes() {
        let code = r#"
export const Noop = () => {};
export function App() { return <div>{count}</div>; }
class Props { title = "x"; }
"#;
        let t = tags("src/app.tsx", "tsx", code);
        is(&t, "Noop", "stub", Some("empty_body"));
        is(&t, "App", "real", None);
        is(&t, "Props", "data", Some("fields_only_class"));
    }

    /// A deeply nested initializer is not walked to the end: the budget answers "real".
    #[test]
    fn a_deep_initializer_is_bounded_and_real() {
        // One bracket per line, so the minified-file guard does not skip the file.
        let depth = 2000;
        let code = format!(
            "class Deep {{\n x = {}0{};\n}}\n",
            "[\n".repeat(depth),
            "\n]".repeat(depth)
        );
        let t = tags("src/deep.ts", "typescript", &code);
        is(&t, "Deep", "real", None);
    }

    #[test]
    fn rust_shapes_and_test_modules() {
        let code = r#"
pub struct Point { x: i32, y: i32 }

impl Point {
    pub fn new(x: i32, y: i32) -> Self { Self { x, y } }
    pub fn origin() -> Self { Point { x: 0, y: 0 } }
    pub fn scaled(x: i32) -> Self { Self { x: x * 2, y: 0 } }
    pub fn later(&self) -> i32 { todo!() }
    pub fn noop(&self) {}
    pub fn dist(&self) -> i32 { self.x.abs() + self.y.abs() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn helper() -> Point { Point::new(1, 2) }

    #[test]
    fn checks() { assert_eq!(helper().dist(), 3); }
}

#[cfg(not(test))]
fn prod_only() -> i32 { 1 + 1 }

#[tokio::test]
async fn top_level_test() { run().await; }
"#;
        let t = tags("src/geo.rs", "rust", code);
        is(&t, "Point", "data", Some("struct_decl"));
        is(&t, "new", "data", Some("field_only_constructor"));
        // Literal fields are a decision, not a forwarded argument (codex r1).
        is(&t, "origin", "real", None);
        is(&t, "scaled", "real", None);
        is(&t, "later", "stub", Some("not_implemented"));
        is(&t, "noop", "stub", Some("empty_body"));
        is(&t, "dist", "real", None);
        is(&t, "helper", "test_support", Some("cfg_test_module"));
        is(&t, "checks", "test_support", Some("test_attribute"));
        is(&t, "prod_only", "real", None);
        is(&t, "top_level_test", "test_support", Some("test_attribute"));
    }

    #[test]
    fn other_languages_and_kinds_get_no_key() {
        let t = tags("src/m.go", "go", "package m\nfunc A() {}\n");
        assert!(t.is_empty(), "{t:?}");
        let t = tags("pkg/m.py", "python", "LIMIT = 3\n");
        assert!(t.is_empty(), "a constant is not inspected: {t:?}");
    }
}
