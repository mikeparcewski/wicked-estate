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
//! Inspected: callables and classes/structs in Python, TypeScript, TSX, JavaScript and Rust, and COBOL
//! paragraphs and sections (a body of only EXIT, CONTINUE or GOBACK is a stub).
//!
//! [`HAS_BODY_KEY`] separately records implementation evidence for selected callable grammars.

use std::sync::LazyLock;

use tree_sitter::Node as TsNode;

/// Metadata key: the unit's behaviour class (`real` | `stub` | `data` | `test_support`).
pub const BEHAVIOUR_KEY: &str = "behaviour";
/// Metadata key: why the unit is not `real` (a short snake_case code).
pub const BEHAVIOUR_REASON_KEY: &str = "behaviour_reason";
/// Metadata key: whether an inspected callable contribution has an implementation body.
pub const HAS_BODY_KEY: &str = "has_body";

/// Inspect a callable's exact anchor range. Other languages and kinds have unknown body evidence.
#[must_use]
pub fn has_body(
    lang: &str,
    kind: &str,
    tree: &tree_sitter::Tree,
    start: usize,
    end: usize,
    decl: bool,
) -> Option<bool> {
    if !matches!(kind, "function" | "method" | "constructor")
        || !matches!(
            lang,
            "java"
                | "csharp"
                | "apex"
                | "php"
                | "kotlin"
                | "scala"
                | "typescript"
                | "tsx"
                | "javascript"
        )
    {
        return None;
    }
    if decl {
        return Some(false);
    }
    let Some(node) = anchor(
        tree,
        start,
        end,
        &[
            "method_declaration",
            "constructor_declaration",
            "local_function_statement",
            "annotation_type_element_declaration",
            "function_definition",
            "function_declaration",
            "generator_function_declaration",
            "secondary_constructor",
            "method_definition",
            "method_signature",
            "abstract_method_signature",
            "function_signature",
            "variable_declarator",
            "public_field_definition",
            "field_definition",
        ],
    ) else {
        return Some(false);
    };
    Some(match (lang, node.kind()) {
        ("kotlin", "function_declaration") => named_children(node)
            .iter()
            .any(|c| c.kind() == "function_body"),
        ("kotlin", "secondary_constructor") => {
            named_children(node).iter().any(|c| c.kind() == "block")
        }
        (
            "typescript" | "tsx" | "javascript",
            "variable_declarator" | "public_field_definition" | "field_definition",
        ) => node
            .child_by_field_name("value")
            .is_some_and(|v| matches!(v.kind(), "arrow_function" | "function_expression")),
        _ => node.child_by_field_name("body").is_some(),
    })
}

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
        "cobol" => Family::Cobol,
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
        Family::Cobol => crate::treesitter::COBOL_HEADERS,
    };
    let Some(node) = anchor(tree, start, end, def_kinds) else {
        return Some(Behaviour::Real);
    };
    Some(match family {
        Family::Python => python(node, src),
        Family::Ts => ts(node, src),
        Family::Rust => rust(node, src),
        Family::Cobol => cobol(node, src),
    })
}

enum Family {
    Python,
    Ts,
    Rust,
    Cobol,
}

// ── COBOL ───────────────────────────────────────────────────────────────────

/// 0-based column where Area B starts (after 6 sequence columns, the indicator column and
/// Area A, columns 8-11). A paragraph or section header starting there or later is a statement
/// the grammar misread as a header.
const COBOL_AREA_B_COLUMN: usize = 11;

/// COBOL verbs and reserved words that are never a real paragraph or section name: a header the
/// grammar built from one of these is a statement it misread as a header (Micro Focus does not
/// enforce Area A/B, so this holds in any column). Matched against the header's whole name token.
const COBOL_VERB_HEADERS: &[&str] = &[
    "COMMIT",
    "ROLLBACK",
    "EXIT",
    "CONTINUE",
    "GOBACK",
    "STOP",
    "MOVE",
    "PERFORM",
    "CALL",
    "DISPLAY",
    "READ",
    "WRITE",
    "REWRITE",
    "DELETE",
    "OPEN",
    "CLOSE",
    "START",
    "RETURN",
    "RELEASE",
    "ACCEPT",
    "COMPUTE",
    "ADD",
    "SUBTRACT",
    "MULTIPLY",
    "DIVIDE",
    "SET",
    "INITIALIZE",
    "INSPECT",
    "STRING",
    "UNSTRING",
    "SEARCH",
    "SORT",
    "MERGE",
    "EVALUATE",
    "IF",
    "GO",
    "ALTER",
    "CANCEL",
];

/// The words an exit-only paragraph's body may carry in Area A/B (columns 8-72), besides periods
/// and the header line itself: the `EXIT` / `EXIT PARAGRAPH` / `EXIT SECTION` / `EXIT PROGRAM`,
/// `CONTINUE` and `GOBACK` phrases.
const COBOL_EXIT_WORDS: &[&str] = &[
    "EXIT",
    "PARAGRAPH",
    "SECTION",
    "PROGRAM",
    "CONTINUE",
    "GOBACK",
];

/// A paragraph or section with at least one statement and only EXIT / CONTINUE / GOBACK
/// statements, periods and comments may be a stub. Empty bodies, other statements, parse errors,
/// headers in Area B or named by verbs, and errors after the division stay real. A free-format
/// directive keeps every candidate real. Otherwise the source must have blank/digit columns 1-6,
/// a blank, absent or comment indicator in column 7, and only exit-only text in columns 8-72.
/// Floating comments starting in columns 8-72 run to end of line; any other non-blank tail must
/// be one word under the links pass's identification rule (at most 80 columns, column 72 blank).
/// The scan ends before the next header, or at the line start of the next non-extra token after
/// the enclosing division (EOF if absent), never before the division's last physical line ends.
/// END DECLARATIVES ends the scan. Anything uncertain stays real.
fn cobol(header: TsNode<'_>, src: &[u8]) -> Behaviour {
    let Some(end) = crate::treesitter::cobol_body_end(header) else {
        return Behaviour::Real;
    };
    if header.start_position().column >= COBOL_AREA_B_COLUMN || cobol_header_is_verb(header, src) {
        return Behaviour::Real;
    }
    let mut statements = 0usize;
    let mut ended_by_header = false;
    let mut next = header.next_sibling();
    while let Some(n) = next {
        next = n.next_sibling();
        if crate::treesitter::is_cobol_header(n) {
            // A header the grammar built from a verb (`COMMIT.`, `ROLLBACK.`) is a statement it
            // misread, in any column. Any other header ends the span, whatever column it starts in.
            if cobol_header_is_verb(n, src) {
                return Behaviour::Real;
            }
            ended_by_header = true;
            break;
        }
        if n.start_byte() > end {
            break;
        }
        // Every child, named or not, zero-width at the parent's end included.
        if n.is_error() || n.is_missing() || n.has_error() {
            return Behaviour::Real;
        }
        if !n.is_named() {
            continue;
        }
        match n.kind() {
            "period" | "comment" => {}
            "exit_statement" | "continue_statement" | "goback_statement" => statements += 1,
            _ => return Behaviour::Real,
        }
    }
    // A declaratives paragraph's errors are only those inside its own `procedure_declaratives`
    // (a MISSING period on `END DECLARATIVES`, say); an ERROR or MISSING in the main procedure
    // code that follows a valid `END DECLARATIVES.` does not make it real.
    let scan_root = header
        .parent()
        .filter(|p| p.kind() == "procedure_declaratives")
        .unwrap_or_else(|| {
            let mut root = header;
            while let Some(p) = root.parent() {
                root = p;
            }
            root
        });
    if !ended_by_header && error_after_division(scan_root, end) {
        return Behaviour::Real;
    }
    if statements > 0 {
        let Ok(source) = std::str::from_utf8(src) else {
            return Behaviour::Real;
        };
        // The grammar reads fixed columns and silently drops code that starts in column 1 (free
        // format, continuations): read the paragraph's own source and keep it real unless every
        // line of it is fixed-format shape and holds only exit-only statements.
        if crate::cobol_links::source_is_free_format(source)
            || !cobol_exit_only_source_ok(header, end, ended_by_header, src)
        {
            return Behaviour::Real;
        }
        Behaviour::Stub("exit_only")
    } else {
        Behaviour::Real
    }
}

/// The uppercased name of a paragraph or section header (`name` field), if any.
fn cobol_header_name(header: TsNode<'_>, src: &[u8]) -> Option<String> {
    header
        .child_by_field_name("name")
        .map(|n| text(n, src).to_ascii_uppercase())
}

/// Whether a header's whole name is a COBOL verb or reserved word — a statement the grammar
/// misread as a header, in any column.
fn cobol_header_is_verb(header: TsNode<'_>, src: &[u8]) -> bool {
    cobol_header_name(header, src).is_some_and(|name| COBOL_VERB_HEADERS.contains(&name.as_str()))
}

/// Validate source after `cobol` has ruled out a free-format directive with the links detector.
/// Scan from the header's line to the line before the next header, or to the start of the line
/// holding the first non-extra node after the enclosing division (EOF if absent), never shortening
/// the division's last physical line. END DECLARATIVES still stops the scan. Every line, including
/// the header, must have blank/digit columns 1-6. Column 7 is `*` or `/` for a skipped comment,
/// otherwise blank or absent. In columns 8-72, after the header itself, strip a floating `*>`
/// comment before checking the tail: that comment consumes the rest of the line. Any other
/// non-blank tail is identification only as one word under the links pass's shared card rule (at
/// most 80 columns, column 72 blank). Remaining source holds only exit-only words, spaces and
/// periods; any doubt keeps the paragraph real.
fn cobol_exit_only_source_ok(
    header: TsNode<'_>,
    end: usize,
    ended_by_header: bool,
    src: &[u8],
) -> bool {
    let line_start = |at: usize| {
        src[..at.min(src.len())]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1)
    };
    let line_begin = line_start(header.start_byte());
    let end = end.min(src.len());
    // A following header starts its own span on its own line. The last paragraph's span reaches
    // at least the end of the physical line its division ends on, then on to the line of the next
    // non-extra node after the division (or EOF): lines the grammar dropped there (a column-1
    // statement read as a comment, a statement past column 72) are read too.
    let region_end = if ended_by_header || end == 0 || src[end - 1] == b'\n' {
        line_start(end)
    } else {
        src[end..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(src.len(), |i| end + i)
    };
    let region_end = if ended_by_header {
        region_end
    } else {
        let mut ancestor = header.parent();
        let mut next_token = None;
        'ancestors: while let Some(node) = ancestor {
            let mut next = node.next_sibling();
            while let Some(sibling) = next {
                if !sibling.is_extra() {
                    next_token = Some(sibling.start_byte());
                    break 'ancestors;
                }
                next = sibling.next_sibling();
            }
            ancestor = node.parent();
        }
        region_end.max(next_token.map_or(src.len(), line_start))
    };
    let region_end = region_end.max(line_begin);
    // When the span ends at the next header, that header's own line is dropped from the scan. A
    // free-format statement the grammar misread as a header (e.g. `CALL   SUBPROG.` in column 1,
    // where it drops `CALL` and reads `SUBPROG.` as a header) leaves real text in the boundary
    // line's sequence area; a genuine fixed-format following header has blank or digit columns 1-6
    // there. Require blank-or-digit columns 1-6 on the boundary line, else keep the paragraph real.
    if ended_by_header {
        let boundary = line_start(end);
        let boundary_end = src[boundary..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(src.len(), |i| boundary + i);
        let prefix = String::from_utf8_lossy(&src[boundary..boundary_end]);
        if !prefix
            .chars()
            .take(6)
            .all(|c| c == ' ' || c.is_ascii_digit())
        {
            return false;
        }
    }
    let header_end_col = header.end_byte().min(src.len()) - line_begin;
    let region = String::from_utf8_lossy(&src[line_begin..region_end]);
    for (idx, raw) in region.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let body = line.as_bytes();
        // Fixed-format shape: columns 1-6 are blank or digits (sequence area).
        if !body
            .iter()
            .take(6)
            .all(|&c| c == b' ' || c.is_ascii_digit())
        {
            return false;
        }
        // Comment line: indicator column 7 is `*` or `/`.
        if matches!(body.get(6), Some(b'*') | Some(b'/')) {
            continue;
        }
        // Any other indicator (`D` debug, `-` continuation) is not plain exit-only source.
        if body.get(6).is_some_and(|&c| c != b' ') {
            return false;
        }
        // Columns 8-72; on the header line, only what follows the header itself.
        let from = if idx == 0 { header_end_col.max(7) } else { 7 };
        let to = 72.min(body.len());
        // A floating `*>` comment in columns 8-72 consumes the tail too.
        let comment = body[from.min(to)..body.len().min(73)]
            .windows(2)
            .position(|pair| pair == b"*>");
        // Columns 73+: blank, or one label word on a card with an identification area. A label
        // never holds a blank, so `GO TO X` there is a statement the grammar dropped.
        let tail = String::from_utf8_lossy(body.get(72..).unwrap_or_default());
        let tail = tail.trim();
        if comment.is_none()
            && !tail.is_empty()
            && (tail.contains(char::is_whitespace)
                || !crate::cobol_links::has_identification_area(body))
        {
            return false;
        }
        let area_end = comment.map_or(to, |p| from.min(to) + p);
        let area = String::from_utf8_lossy(&body[from.min(to)..area_end]);
        let area_norm = area
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_uppercase();
        if area_norm == "END DECLARATIVES" {
            break;
        }
        if !cobol_area_exit_only(&area) {
            return false;
        }
    }
    true
}

/// Whether an Area A/B fragment holds only exit-only statement words, spaces and periods.
fn cobol_area_exit_only(area: &str) -> bool {
    let mut word = String::new();
    for c in area.chars() {
        if c.is_ascii_alphabetic() {
            word.push(c.to_ascii_uppercase());
        } else if c == ' ' || c == '.' {
            if !word.is_empty() {
                if !COBOL_EXIT_WORDS.contains(&word.as_str()) {
                    return false;
                }
                word.clear();
            }
        } else {
            return false;
        }
    }
    word.is_empty() || COBOL_EXIT_WORDS.contains(&word.as_str())
}

/// Whether an ERROR or MISSING node lies at or after `division_end` within `scan_root`, before
/// the next `program_definition`: error recovery closes the division early and parks the rest of
/// the paragraph's statements there.
fn error_after_division(scan_root: TsNode<'_>, division_end: usize) -> bool {
    let root = scan_root;
    let mut stop = usize::MAX;
    let mut errors = Vec::new();
    let mut cursor = root.walk();
    let mut seen = 0usize;
    'walk: loop {
        let n = cursor.node();
        seen += 1;
        if seen > WALK_BUDGET * 64 {
            return true;
        }
        if n.end_byte() >= division_end {
            if n.kind() == "program_definition" && n.start_byte() >= division_end {
                stop = stop.min(n.start_byte());
            }
            if (n.is_error() || n.is_missing()) && n.start_byte() >= division_end {
                errors.push(n.start_byte());
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    errors.into_iter().any(|e| e < stop)
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

const PY_CONSTRAINT_KEYS: &[&str] = &[
    "gt",
    "ge",
    "lt",
    "le",
    "multiple_of",
    "min_length",
    "max_length",
    "pattern",
    "regex",
    "min_items",
    "max_items",
    "unique_items",
    "max_digits",
    "decimal_places",
    "allow_inf_nan",
    "strict",
    "const",
    "frozen",
    "validator",
    "validators",
    "converter",
];

const PY_CONSTRAINT_CALLS: &[&str] = &[
    "constr",
    "conint",
    "confloat",
    "condecimal",
    "conlist",
    "conset",
    "confrozenset",
    "conbytes",
    "condate",
    "StringConstraints",
    "Gt",
    "Ge",
    "Lt",
    "Le",
    "MultipleOf",
    "MinLen",
    "MaxLen",
    "Len",
    "Interval",
    "Predicate",
];

/// A declaring call with a constraint keyword states a product rule (#434).
fn py_decl_call_constrains(call: TsNode<'_>, src: &[u8]) -> bool {
    call.child_by_field_name("function")
        .is_some_and(|f| py_name_chain(f, 0) && PY_DECL_CALLS.contains(&text(f, src)))
        && call.child_by_field_name("arguments").is_some_and(|args| {
            named_children(args).into_iter().any(|a| {
                a.kind() == "keyword_argument"
                    && a.child_by_field_name("name")
                        .is_some_and(|n| PY_CONSTRAINT_KEYS.contains(&text(n, src)))
            })
        })
}

fn py_annotation_constrains(node: TsNode<'_>, src: &[u8], depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    if node.kind() == "identifier" && text(node, src) == "Literal" {
        return true;
    }
    if node.kind() == "call"
        && (py_decl_call_constrains(node, src)
            || node.child_by_field_name("function").is_some_and(|f| {
                py_name_chain(f, 0)
                    && text(f, src)
                        .rsplit('.')
                        .next()
                        .is_some_and(|name| PY_CONSTRAINT_CALLS.contains(&name))
            }))
    {
        return true;
    }
    named_children(node)
        .into_iter()
        .any(|c| py_annotation_constrains(c, src, depth + 1))
}

fn py_restricting_setting(name: &str, value: TsNode<'_>, src: &[u8]) -> bool {
    match name {
        "extra" => matches!(text(value, src), "\"forbid\"" | "'forbid'" | "Extra.forbid"),
        "frozen" | "strict" => value.kind() == "true",
        "allow_mutation" => value.kind() == "false",
        _ => false,
    }
}

fn py_restricting_keyword(arg: TsNode<'_>, src: &[u8]) -> bool {
    arg.kind() == "keyword_argument"
        && arg.child_by_field_name("name").is_some_and(|name| {
            arg.child_by_field_name("value")
                .is_some_and(|value| py_restricting_setting(text(name, src), value, src))
        })
}

fn py_restricting_model_config(assignment: TsNode<'_>, src: &[u8]) -> bool {
    assignment
        .child_by_field_name("left")
        .is_some_and(|l| l.kind() == "identifier" && text(l, src) == "model_config")
        && assignment.child_by_field_name("right").is_some_and(|v| {
            v.kind() == "call"
                && v.child_by_field_name("function")
                    .is_some_and(|f| matches!(text(f, src), "ConfigDict" | "SettingsConfigDict"))
                && v.child_by_field_name("arguments").is_some_and(|args| {
                    named_children(args)
                        .into_iter()
                        .any(|a| py_restricting_keyword(a, src))
                })
        })
}

fn py_restricting_nested_config(class: TsNode<'_>, src: &[u8]) -> bool {
    class
        .child_by_field_name("name")
        .is_some_and(|n| text(n, src) == "Config")
        && class.child_by_field_name("body").is_some_and(|body| {
            py_statements(body).into_iter().any(|s| {
                s.kind() == "expression_statement"
                    && s.named_child(0).is_some_and(|a| {
                        a.kind() == "assignment"
                            && a.child_by_field_name("left").is_some_and(|name| {
                                name.kind() == "identifier"
                                    && a.child_by_field_name("right").is_some_and(|v| {
                                        py_restricting_setting(text(name, src), v, src)
                                    })
                            })
                    })
            })
        })
}

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
                && !py_decl_call_constrains(v, src)
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
                    if e.child_by_field_name("type")
                        .is_some_and(|t| py_annotation_constrains(t, src, 0))
                        || py_restricting_model_config(e, src)
                    {
                        return PyMember::Other;
                    }
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
    if py_restricting_nested_config(c, src) {
        return PyMember::Other;
    }
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
        || node.child_by_field_name("superclasses").is_some_and(|b| {
            contains_kind(b, &["call", "lambda"])
                || named_children(b)
                    .into_iter()
                    .any(|a| py_restricting_keyword(a, src))
        })
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

    /// Fixed-format COBOL: `lines` get sequence numbers in columns 1-6 and 73-80; a line starting
    /// with `*` is a comment line (indicator column 7).
    fn fixed(lines: &[&str]) -> String {
        let mut out = String::new();
        for (i, l) in lines.iter().enumerate() {
            let body = if let Some(c) = l.strip_prefix('*') {
                format!("      *{c}")
            } else {
                format!("       {l}")
            };
            out.push_str(&format!(
                "{:06}{:<66}{:08}\n",
                (i + 1) * 10,
                body[6..].to_string(),
                (i + 1) * 10
            ));
        }
        out
    }

    fn cobol_tags(lines: &[&str]) -> std::collections::BTreeMap<String, (String, Option<String>)> {
        tags("src/PROG.cbl", "cobol", &fixed(lines))
            .into_iter()
            .map(|(k, b, r)| (k.rsplit('#').next().unwrap_or(&k).to_string(), (b, r)))
            .collect()
    }

    fn expect(
        got: &std::collections::BTreeMap<String, (String, Option<String>)>,
        name: &str,
        class: &str,
        reason: Option<&str>,
    ) {
        let (b, r) = got
            .get(name)
            .unwrap_or_else(|| panic!("no node {name}: {got:?}"));
        assert_eq!(b, class, "{name}: {got:?}");
        assert_eq!(r.as_deref(), reason, "{name}");
    }

    #[test]
    fn cobol_exit_only_paragraphs_are_stubs_and_the_rest_real() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "DATA DIVISION.",
            "WORKING-STORAGE SECTION.",
            "01 WS-A PIC X(10).",
            "PROCEDURE DIVISION.",
            "0000-MAIN.",
            "    MOVE 'A' TO WS-A.",
            "    PERFORM 1000-EXIT-DOT.",
            "    PERFORM 1100-SPLIT-EXIT.",
            "    PERFORM 1200-CONT.",
            "    PERFORM 1300-GOBACK.",
            "    PERFORM 1400-EXIT-PROG.",
            "    PERFORM 1500-EXIT-PARA.",
            "    PERFORM 2000-MOVE-EXIT.",
            "    PERFORM 2100-PERFORM-ONLY.",
            "    PERFORM 2200-INLINE.",
            "    PERFORM 2300-STOP.",
            "    PERFORM 2400-COPY.",
            "    STOP RUN.",
            "1000-EXIT-DOT.",
            "    EXIT.",
            "1100-SPLIT-EXIT.",
            "*   nothing to do here",
            "    EXIT",
            "    .",
            "1200-CONT.",
            "    CONTINUE.",
            "1300-GOBACK.",
            "    GOBACK.",
            "1400-EXIT-PROG.",
            "    EXIT PROGRAM.",
            "1500-EXIT-PARA.",
            "    EXIT PARAGRAPH.",
            "2000-MOVE-EXIT.",
            "    MOVE 'B' TO WS-A.",
            "    EXIT.",
            "2100-PERFORM-ONLY.",
            "    PERFORM 1000-EXIT-DOT.",
            "2200-INLINE.",
            "    PERFORM UNTIL WS-A = 'X'",
            "        CONTINUE",
            "    END-PERFORM.",
            "2300-STOP.",
            "    STOP RUN.",
            "2400-COPY.",
            "    COPY CPYBOOK.",
            "    EXIT.",
            "9999-LAST-EXIT.",
            "    EXIT.",
        ]);
        for n in [
            "1000-EXIT-DOT",
            "1100-SPLIT-EXIT",
            "1200-CONT",
            "1300-GOBACK",
            "1400-EXIT-PROG",
            "1500-EXIT-PARA",
            "9999-LAST-EXIT",
        ] {
            expect(&got, n, "stub", Some("exit_only"));
        }
        for n in [
            "0000-MAIN",
            "2000-MOVE-EXIT",
            "2100-PERFORM-ONLY",
            "2200-INLINE",
            "2300-STOP",
            "2400-COPY",
        ] {
            expect(&got, n, "real", None);
        }
    }

    /// A statement the grammar cannot parse (ERROR node) keeps its paragraph real, even beside an
    /// EXIT. The grammar's error recovery swallows every header after it, so this paragraph is
    /// last in its own fixture.
    #[test]
    fn cobol_error_node_keeps_paragraph_real() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "    EXIT.",
            "B-ERR.",
            "    EXIT.",
            "    ENTRY 'X' USING Y.",
        ]);
        expect(&got, "A-PARA", "stub", Some("exit_only"));
        expect(&got, "B-ERR", "real", None);
    }

    /// Free format: the grammar reads a column-1 `CALL X.` as a sequence area and drops it, so
    /// the directive keeps every paragraph of the file real.
    #[test]
    fn cobol_free_format_directive_keeps_paragraphs_real() {
        for directive in [
            ">>SOURCE FORMAT FREE",
            ">>source free",
            "$SET SOURCEFORMAT\"FREE\"",
        ] {
            let code = format!(
                "{directive}\n       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\nCALL X.\n           EXIT.\n"
            );
            let got = tags("src/PROG.cbl", "cobol", &code);
            is(&got, "A-PARA", "real", None);
        }
    }

    /// #475 / #473.2a: free directives keep COMMIT and ROLLBACK past column 72 real.
    #[test]
    fn cobol_475_free_directive_statement_word_past_column_72_keeps_paragraph_real() {
        for directive in [">>SOURCE FORMAT FREE", "$SET SOURCEFORMAT\"FREE\""] {
            for word in ["COMMIT", "ROLLBACK"] {
                let statement = format!("{:<72}{word}", "           CONTINUE");
                let code = format!(
                    "{directive}\n       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n{statement}\n           .\n       B-PARA.\n           EXIT.\n"
                );
                let got = tags("src/PROG.cbl", "cobol", &code);
                is(&got, "A-PARA", "real", None);
            }
        }
    }

    /// #473.2b: debug and continuation indicators cannot certify an exit-only paragraph.
    #[test]
    fn cobol_473_non_blank_column_7_keeps_paragraph_real() {
        for indicator in ['D', '-'] {
            let code = format!(
                "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n           EXIT.\n      {indicator}    CONTINUE.\n"
            );
            let got = tags("src/PROG.cbl", "cobol", &code);
            is(&got, "A-PARA", "real", None);
        }
    }

    /// #473.3: dropped statements after the last paragraph's EXIT keep that paragraph real.
    #[test]
    fn cobol_473_dropped_line_after_last_paragraph_keeps_it_real() {
        for dropped in [
            "MOVE '*' TO WS-X.".to_string(),
            format!("{:72}MOVE A TO B.", ""),
        ] {
            let code = format!(
                "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n           EXIT.\n{dropped}\n"
            );
            let got = tags("src/PROG.cbl", "cobol", &code);
            is(&got, "A-PARA", "real", None);
        }
    }

    /// #473.5: a floating comment extending beyond column 80 does not add behaviour.
    #[test]
    fn cobol_473_floating_comment_past_column_72_stays_stub() {
        let comment = "comment text ".repeat(8);
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n           EXIT. *> {comment}\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "stub", Some("exit_only"));
    }

    /// #465.1 / #473.6: punctuation in a card's identification label is not executable code.
    #[test]
    fn cobol_465_identification_labels_in_columns_73_to_80_stay_stub() {
        for label in ["PROG.CBL", "CBACT-01", "V1.2"] {
            let code = fixed(&[
                "IDENTIFICATION DIVISION.",
                "PROGRAM-ID. PROG.",
                "PROCEDURE DIVISION.",
                "A-PARA.",
                "    EXIT.",
            ])
            .lines()
            .map(|line| format!("{}{label}\n", &line[..72]))
            .collect::<String>();
            let got = tags("src/PROG.cbl", "cobol", &code);
            is(&got, "A-PARA", "stub", Some("exit_only"));
        }
    }

    /// #473.1: code past column 72 on a column-1 header line keeps every node real.
    #[test]
    fn cobol_473_header_line_statement_past_column_72_keeps_all_nodes_real() {
        let header = format!("{:72}MOVE A TO B.", "AAAAAAA-PARA.");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n{header}\n           EXIT.\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        assert!(!got.is_empty(), "{got:?}");
        assert!(got.iter().all(|(_, class, _)| class != "stub"), "{got:?}");
    }

    /// #473.1: a statement in columns 1-6 before a header keeps every node real.
    #[test]
    fn cobol_473_statement_in_sequence_area_keeps_all_nodes_real() {
        let code = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\nCALL X A-PARA.\n           EXIT.\n";
        let got = tags("src/PROG.cbl", "cobol", code);
        assert!(!got.is_empty(), "{got:?}");
        assert!(got.iter().all(|(_, class, _)| class != "stub"), "{got:?}");
    }

    /// #465.1: alphabetic sequence labels stay real because fixed format is uncertain.
    #[test]
    fn cobol_465_letters_in_sequence_area_stay_real() {
        let code = fixed(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "    EXIT.",
        ])
        .lines()
        .map(|line| format!("ABCDEF{}\n", &line[6..]))
        .collect::<String>();
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// #473.2a: statement text beyond column 80 cannot be a card identification area.
    #[test]
    fn cobol_473_tail_past_column_80_keeps_paragraph_real() {
        let statement = format!("{:<72}MOVE A TO B.", "           EXIT.");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n{statement}\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// #473.2a: a non-blank column 72 before a tail cannot certify an exit-only card.
    #[test]
    fn cobol_473_tail_with_non_blank_column_72_keeps_paragraph_real() {
        let statement = format!("{:<71}.V1.2", "           EXIT");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n{statement}\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// #475: without a directive, a several-word statement in columns 73-80 of a card is not a
    /// label, so the paragraph stays real.
    #[test]
    fn cobol_475_statement_with_blanks_in_columns_73_to_80_keeps_paragraph_real() {
        let statement = format!("{:<72}GO TO X.", "           EXIT.");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n{statement}\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// Source-text guard: a `>>SOURCE IS FREE` directive the file-level regex never matched still
    /// leaves a paragraph real, because the grammar drops the column-1 `CALL X.` and its own
    /// source shows real code the fixed columns hid.
    #[test]
    fn cobol_free_format_is_directive_keeps_paragraph_real() {
        let code = "       >>SOURCE IS FREE\n       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\nCALL X.\n           EXIT.\n";
        let got = tags("src/PROG.cbl", "cobol", code);
        is(&got, "A-PARA", "real", None);
    }

    /// Source-text guard: free-format code with no directive at all, whose column-1 `CALL X.` the
    /// grammar drops, stays real on its source alone.
    #[test]
    fn cobol_free_format_no_directive_keeps_paragraph_real() {
        let code = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\nCALL X.\n           EXIT.\n";
        let got = tags("src/PROG.cbl", "cobol", code);
        is(&got, "A-PARA", "real", None);
    }

    /// Free format: `MOVE '*' TO WS-X.` starting in column 1 puts the `*` in column 7. Column 7
    /// is the comment indicator only on a fixed-format line; this is free-format real code the
    /// grammar dropped, so the paragraph stays real.
    #[test]
    fn cobol_free_format_star_in_column_7_keeps_paragraph_real() {
        let code = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\nMOVE '*' TO WS-X.\n           EXIT.\n";
        let got = tags("src/PROG.cbl", "cobol", code);
        is(&got, "A-PARA", "real", None);
    }

    /// Free format: a real statement past column 72 on the paragraph's last line is dropped by
    /// the grammar and must not be cut off from the source scan — the paragraph stays real.
    #[test]
    fn cobol_free_format_statement_past_column_72_keeps_paragraph_real() {
        let mut last = String::from("           EXIT.");
        while last.len() < 72 {
            last.push(' ');
        }
        last.push_str("MOVE A TO B.");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n{last}\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// Free format proper: `EXIT.` starting in column 1 (columns 1-6 not blank/digits), padded
    /// past column 72 and followed by a real statement, must be real — never a false stub. The
    /// grammar reads columns 1-6 as the sequence area, so it finds no statement to bound and the
    /// paragraph is kept real before the exit-only scan is even reached (regression guard for the
    /// column-1 case the fixed-shaped `..._statement_past_column_72` test does not cover).
    #[test]
    fn cobol_free_format_column_1_exit_statement_past_column_72_keeps_paragraph_real() {
        let mut last = String::from("EXIT.");
        while last.len() < 72 {
            last.push(' ');
        }
        last.push_str("MOVE A TO B.");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n{last}\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// Free format: a header line (columns 1-6 hold text, so it is not fixed-format shape) padded
    /// to column 72 whose tail past column 72 holds `MOVE A TO B.`, then `EXIT.`. The paragraph
    /// is real.
    #[test]
    fn cobol_free_format_padded_header_with_statement_past_column_72_is_real() {
        let mut header = String::from("MAIN-X A-PARA.");
        while header.len() < 72 {
            header.push(' ');
        }
        header.push_str("MOVE A TO B.");
        let code = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n{header}\n           EXIT.\n"
        );
        let got = tags("src/PROG.cbl", "cobol", &code);
        is(&got, "A-PARA", "real", None);
    }

    /// Codex shape: free format, `A-PARA.` then `EXIT.`, then `CALL   SUBPROG.` starting in
    /// column 1. The grammar drops the column-1 `CALL` and reads `SUBPROG.` as the next header, so
    /// A-PARA's span ends on that line; its columns 1-6 hold `CALL  ` (not blank/digits), so the
    /// dropped statement keeps A-PARA real instead of a false exit-only stub.
    #[test]
    fn cobol_free_format_call_header_boundary_keeps_paragraph_real() {
        let code = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n           EXIT.\nCALL   SUBPROG.\n";
        let got = tags("src/PROG.cbl", "cobol", code);
        is(&got, "A-PARA", "real", None);
    }

    /// A fixed-format following header (columns 1-6 blank or digits on its line) still ends the
    /// preceding EXIT-only paragraph's span and lets it be an exit_only stub — the boundary-line
    /// prefix guard only rejects a boundary line whose columns 1-6 hold real text.
    #[test]
    fn cobol_fixed_format_following_header_keeps_preceding_paragraph_stub() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "    EXIT.",
            "B-PARA.",
            "    DISPLAY 'X'.",
        ]);
        expect(&got, "A-PARA", "stub", Some("exit_only"));
        expect(&got, "B-PARA", "real", None);
    }

    /// The span ends at the next header whatever column it starts in: a header in columns 9-11
    /// leaves the preceding EXIT-only paragraph a stub.
    #[test]
    fn cobol_following_header_in_columns_9_to_11_ends_exit_only_span() {
        let code = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. PROG.\n       PROCEDURE DIVISION.\n       A-PARA.\n           EXIT.\n         B-PARA.\n           DISPLAY 'X'.\n";
        let got = tags("src/PROG.cbl", "cobol", code);
        is(&got, "A-PARA", "stub", Some("exit_only"));
        is(&got, "B-PARA", "real", None);
    }

    /// Fixed format: a `*` comment line in column 7 beside an EXIT body is still exit-only.
    #[test]
    fn cobol_fixed_format_comment_in_column_7_stays_stub() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "C-PARA.",
            "* nothing to do",
            "    EXIT.",
        ]);
        expect(&got, "C-PARA", "stub", Some("exit_only"));
    }

    /// Source-text guard: a directive text inside a `*` comment or a `DISPLAY` literal no longer
    /// turns classification off for the whole file — the genuinely exit-only paragraph beside it
    /// is still a stub.
    #[test]
    fn cobol_directive_in_comment_or_literal_still_classifies() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "*>>SOURCE FORMAT FREE",
            "    DISPLAY '>>SOURCE FORMAT FREE'.",
            "B-PARA.",
            "    EXIT.",
        ]);
        expect(&got, "A-PARA", "real", None);
        expect(&got, "B-PARA", "stub", Some("exit_only"));
    }

    /// `COMMIT.` / `ROLLBACK.` in column 8 (Area A, not Area B) between EXITs are statements the
    /// grammar misreads as paragraph headers: the verb-name guard keeps every unit real.
    #[test]
    fn cobol_verb_named_headers_in_area_a_keep_paragraphs_real() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "    EXIT.",
            "COMMIT.",
            "    EXIT.",
            "ROLLBACK.",
            "    EXIT.",
        ]);
        for n in ["A-PARA", "COMMIT", "ROLLBACK"] {
            expect(&got, n, "real", None);
        }
    }

    /// Declaratives boundary: `error_after_division` stops at a valid `END DECLARATIVES.`, so an
    /// ERROR parked by a later incomplete `MOVE` in the main procedure does not make the
    /// declaratives paragraph real.
    #[test]
    fn cobol_valid_end_declaratives_keeps_declaratives_paragraph_stub() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "DECLARATIVES.",
            "D-SEC SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON F.",
            "D-PARA.",
            "    EXIT.",
            "END DECLARATIVES.",
            "M-SEC SECTION.",
            "M-PARA.",
            "    MOVE",
        ]);
        expect(&got, "D-PARA", "stub", Some("exit_only"));
    }

    /// `COMMIT.` / `ROLLBACK.` in Area B between EXITs are statements the grammar misreads as
    /// paragraph headers: every unit they touch stays real.
    #[test]
    fn cobol_area_b_headers_keep_paragraphs_real() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "    EXIT.",
            "    COMMIT.",
            "    EXIT.",
            "    ROLLBACK.",
            "    EXIT.",
        ]);
        for n in ["A-PARA", "COMMIT", "ROLLBACK"] {
            if got.contains_key(n) {
                expect(&got, n, "real", None);
            }
        }
        expect(&got, "A-PARA", "real", None);
    }

    /// A trailing incomplete MOVE makes the grammar close the division early and park an ERROR
    /// after it: the last paragraph stays real.
    #[test]
    fn cobol_trailing_error_keeps_last_paragraph_real() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-PARA.",
            "    EXIT.",
            "B-PARA.",
            "    EXIT.",
            "    MOVE",
        ]);
        expect(&got, "B-PARA", "real", None);
    }

    /// `END DECLARATIVES` without its period gets an unnamed zero-width MISSING at the end of the
    /// declaratives: the paragraph before it stays real.
    #[test]
    fn cobol_missing_period_after_end_declaratives_keeps_paragraph_real() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "DECLARATIVES.",
            "D-SEC SECTION.",
            "    USE AFTER STANDARD ERROR PROCEDURE ON F.",
            "D-PARA.",
            "    EXIT.",
            "END DECLARATIVES",
            "M-SEC SECTION.",
            "M-PARA.",
            "    DISPLAY 'X'.",
        ]);
        expect(&got, "D-PARA", "real", None);
    }

    #[test]
    fn cobol_section_bodies() {
        let got = cobol_tags(&[
            "IDENTIFICATION DIVISION.",
            "PROGRAM-ID. PROG.",
            "PROCEDURE DIVISION.",
            "A-SEC SECTION.",
            "    EXIT.",
            "A-PARA.",
            "    DISPLAY 'X'.",
            "B-SEC SECTION.",
            "B-PARA.",
            "    DISPLAY 'Y'.",
            "C-SEC SECTION.",
            "C-PARA.",
            "    EXIT.",
            "D-PARA.",
            "    PERFORM UNTIL WS-A = 'X'",
            "        EXIT PERFORM",
            "    END-PERFORM.",
            "E-PARA.",
            "    EXIT PERFORM CYCLE.",
        ]);
        expect(&got, "A-SEC", "stub", Some("exit_only"));
        expect(&got, "A-PARA", "real", None);
        expect(&got, "B-SEC", "real", None);
        expect(&got, "B-PARA", "real", None);
        expect(&got, "C-PARA", "stub", Some("exit_only"));
        expect(&got, "D-PARA", "real", None);
        expect(&got, "E-PARA", "real", None);
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
        is(&t, "Model", "real", None);
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
    fn python_field_constraints_are_behaviour() {
        for field in [
            "mode: Literal[\"chatgpt\"]",
            "status: Literal[\"pending\", \"complete\"]",
            "mode: typing.Literal[\"chatgpt\"]",
            "mode: Optional[Literal[\"chatgpt\"]]",
            "name: str = Field(pattern=\"^[a-z]+$\")",
            "count: int = Field(ge=0)",
            "count: Annotated[int, Field(gt=0)]",
            "name: str = attr.ib(validator=None)",
        ] {
            let code = format!("class M(ProjectModel):\n    {field}\n");
            let t = tags("pkg/m.py", "python", &code);
            is(&t, "M", "real", None);
        }
        for field in [
            "name: str",
            "name: str = Field(default=None, description=\"x\")",
            "model_config = ConfigDict(populate_by_name=True)",
            "model_config = SettingsConfigDict(extra=\"allow\", allow_mutation=True)",
        ] {
            let code = format!("class M:\n    {field}\n");
            let t = tags("pkg/m.py", "python", &code);
            is(&t, "M", "data", Some("fields_only_class"));
        }
    }

    #[test]
    fn python_annotation_constraint_calls_are_behaviour() {
        for call in [
            "constr()",
            "conint()",
            "confloat()",
            "condecimal()",
            "conlist(int)",
            "conset(int)",
            "confrozenset(int)",
            "conbytes()",
            "condate()",
            "StringConstraints()",
            "annotated_types.Gt(0)",
            "Ge(0)",
            "Lt(10)",
            "Le(10)",
            "MultipleOf(2)",
            "MinLen(1)",
            "MaxLen(10)",
            "Len(1, 10)",
            "Interval(gt=0)",
            "Predicate(bool)",
        ] {
            let code = format!("@dataclass\nclass M:\n    value: Annotated[int, {call}]\n");
            let t = tags("pkg/m.py", "python", &code);
            is(&t, "M", "real", None);
        }
        let t = tags(
            "pkg/m.py",
            "python",
            "class M(TypedDict):\n    x: Literal[1]\n",
        );
        is(&t, "M", "real", None);
    }

    #[test]
    fn python_restricting_config_is_behaviour() {
        for setting in [
            "extra=\"forbid\"",
            "extra=Extra.forbid",
            "frozen=True",
            "strict=True",
            "allow_mutation=False",
        ] {
            for config in ["ConfigDict", "SettingsConfigDict"] {
                let code = format!("class M(BaseModel):\n    model_config = {config}({setting})\n");
                let t = tags("pkg/m.py", "python", &code);
                is(&t, "M", "real", None);
            }
            let assignment = setting.replace('=', " = ");
            let code = format!("class M:\n    class Config:\n        {assignment}\n");
            let t = tags("pkg/m.py", "python", &code);
            is(&t, "M", "real", None);
            let code = format!("class M(BaseModel, {setting}):\n    name: str\n");
            let t = tags("pkg/m.py", "python", &code);
            is(&t, "M", "real", None);
        }
        let code = r#"
class Auth(BaseModel):
    mode: Literal["chatgpt"]
    model_config = ConfigDict(extra="forbid")

class Frozen(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)

class Plain(BaseModel):
    name: str
    class Config:
        orm_mode = True

class Validated(BaseModel):
    name: str
    @pydantic.field_validator("name")
    def check(cls, value):
        pass
"#;
        let t = tags("pkg/m.py", "python", code);
        for name in ["Auth", "Frozen", "Validated"] {
            is(&t, name, "real", None);
        }
        is(&t, "Plain", "data", Some("fields_only_class"));
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
