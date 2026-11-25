//! COBOL text supplement — the links the arborium-cobol grammar cannot see.
//!
//! The grammar parses `EXEC CICS … END-EXEC` and `EXEC SQL … END-EXEC` as opaque `exec_statement`
//! nodes, and `COPY` is a compiler-directing statement processed before parsing. This pass reads
//! the same source and adds what those constructs mean to the tree-sitter [`Extraction`] of the
//! file:
//!
//! | Source construct | Stored as |
//! |---|---|
//! | `EXEC CICS LINK/XCTL PROGRAM('P')` | `Calls` ref paragraph → `P` (any language; parks if absent) |
//! | `EXEC CICS SEND MAP('M') [MAPSET('S')]` | `other:writes` edge program → shared `cics-map:S.M` |
//! | `EXEC CICS RECEIVE MAP('M') [MAPSET('S')]` | `other:reads` edge program → shared `cics-map:S.M` |
//! | `EXEC SQL SELECT … FROM T` (and cursors, subselects, joins, MERGE `USING`) | `other:reads` edge program → shared `db2:T` |
//! | `EXEC SQL INSERT INTO / UPDATE / DELETE FROM / MERGE INTO T` | `other:writes` edge program → `db2:T` |
//! | `EXEC SQL DECLARE T TABLE` (DCLGEN) | the DEFINITION record of `db2:T` |
//! | `COPY C` / `EXEC SQL INCLUDE C` | `Imports` edge program → shared `cobol-copybook:C` |
//! | a file with no PROGRAM-ID (a copybook) | the DEFINITION record of `cobol-copybook:<STEM>` + `Contains` its nodes |
//! | `SELECT F ASSIGN TO DD` | `other:uses` edge program → shared `ddname:<PGM>.<DD>` (JCL defines it) |
//! | `CALL VAR` / `PROGRAM(VAR)`, VAR holding one literal | `Calls` ref `cobol-dynamic-call:<LIT>` (Heuristic) |
//! | the same, VAR with several values / a non-literal / none | `Calls` ref `cobol-dynamic-var:<VAR>` (parked; reason on VAR's field) |
//! | `:HOSTVAR`, CICS `COMMAREA()/INTO()/FROM()` operands | `References` refs paragraph → the data item |
//!
//! **Lexical view.** Every construct is matched on a same-length copy of the source in which
//! comment lines (indicator `*` or `/`), floating `*>` comments, the fixed-format sequence and
//! identification areas (columns 1–6 and 73+; free format is `>>SOURCE FORMAT FREE`) and the
//! CONTENTS of every literal are blanked. So a `COPY` or an `EXEC SQL` inside a `DISPLAY` literal
//! is not code. Literal values are read back from the comment-blanked copy at the same offsets.
//!
//! **Logical source.** A free-format source, or one with a continued literal, is read as its
//! [`LogicalSource`]: fixed-format cards with each continued literal on one line. The grammar
//! parses it, this pass reads it, and every span maps back to the file's own bytes and lines.
//!
//! **Ref roles.** Every COBOL ref carries its role in its hints
//! ([`wicked_estate_core::refs::cobol_ref`]): a data use (with its `OF`/`IN` qualifiers and its
//! program's COPY list), a procedure label, or a program call. `tag_roles` reads it from the
//! tree and drops the labels the data-name query also matches; `wicked-estate-resolve` binds each
//! role only to its own kind of target.
//!
//! **Every source is a real node** of this extraction. Calls and data uses (the `Calls` and
//! `References` refs above) start from the innermost paragraph or section holding the construct,
//! as the tree-sitter extractor's PERFORM and data-name refs do. One call site is one caller:
//! a call is never also stored from its program (that would count it twice). Lineage reaches a
//! program's callee programs through the paragraphs the program runs itself (the extractor's
//! program → paragraph `Calls` edges). Every other link starts from the program Module whose span
//! holds the construct (nested programs pick the innermost), else the first program, else the
//! copybook node, else the File node. Literal inference (dynamic calls,
//! map names), labels and DD names are scoped to that program. A shared target is minted by the referencing file as a BOUNDARY record
//! ([`Node::as_boundary`]) whose reason says what source would define it; the defining file's
//! record wins in the store when it is indexed (see [`crate::contrib`]).
//!
//! Tiers: COPY, literal maps and DD bindings name their target literally → `Parsed`. Table names
//! come from a pattern pass over SQL text, and maps named through a variable are inferred →
//! `Heuristic`. Dynamic-call targets are inferred → `Heuristic` (bound by `wicked-estate-resolve`'s
//! `DynamicCallResolver`).

use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;
use wicked_estate_core::refs::COBOL_DYNAMIC_CALL_PREFIX;
use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Language, Location, Node, NodeKind, ResolutionTier, SourceFile,
    Span, Symbol, SymbolId, UnresolvedRef, edge_tags,
};

const RESOLVED_BY: &str = "cobol-text";

/// `raw_name` prefix of a dynamic call that does NOT resolve to one literal: the variable's own
/// name, so the call stays parked (and visible) instead of binding by name to a program or
/// paragraph that happens to share the variable's spelling. The reason is on the variable's field
/// node (`metadata.dynamic_call`). No resolver binds this prefix.
pub const COBOL_DYNAMIC_VAR_PREFIX: &str = "cobol-dynamic-var:";

static RE_SOURCE_FREE: LazyLock<Regex> = LazyLock::new(|| {
    // The directive starts its line (after an optional sequence area); one inside a comment or a
    // literal does not switch the format. Micro Focus spells it `$SET [...] SOURCEFORMAT"FREE"`.
    Regex::new(concat!(
        r"(?im)^[0-9 ]*(?:>>\s*SOURCE\s+(?:FORMAT\s+)?(?:IS\s+)?FREE\b|",
        r#"\$\s*SET\b[^\n]*?\bSOURCEFORMAT\s*(?:"\s*FREE\s*"|'\s*FREE\s*'|\(\s*FREE\s*\)))"#,
    ))
    .expect("RE_SOURCE_FREE")
});
static RE_EXEC_CICS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)\bEXEC\s+CICS\s+(.*?)\bEND-EXEC").expect("RE_EXEC_CICS"));
static RE_EXEC_SQL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)\bEXEC\s+SQL\s+(.*?)\bEND-EXEC").expect("RE_EXEC_SQL"));
static RE_EXEC_ANY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)\bEXEC\s+(?:SQL|CICS|DLI)\b.*?\bEND-EXEC").expect("RE_EXEC_ANY")
});
/// The start of a CICS option `NAME(`; the operand is read with balanced parentheses.
static RE_CICS_OPT_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([A-Z][A-Z0-9-]*)\s*\(").expect("RE_CICS_OPT_HEAD"));
static RE_SQL_HOSTVAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":([A-Za-z][A-Za-z0-9-]*)").expect("RE_SQL_HOSTVAR"));
static RE_SQL_FROM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:FROM|JOIN)\s+").expect("RE_SQL_FROM"));
static RE_SQL_USING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bUSING\s+").expect("RE_SQL_USING"));
/// A common-table-expression name: `WITH name [(cols)] AS (` or `, name [(cols)] AS (`.
static RE_SQL_CTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:\bWITH\s+(?:RECURSIVE\s+)?|,\s*)([A-Za-z_][A-Za-z0-9_$#@]*)\s*(?:\([^()]*\)\s*)?AS\s*\(",
    )
    .expect("RE_SQL_CTE")
});
static RE_COPY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|[\s.])COPY\s+(?:'([^']+)'|"([^"]+)"|([A-Za-z0-9#$@][A-Za-z0-9#$@-]*))"#)
        .expect("RE_COPY")
});
/// COPY … REPLACING / REPLACE pseudo-text `==…==`: replacement text, not source.
static RE_PSEUDO_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)==.*?==").expect("RE_PSEUDO_TEXT"));
/// `FROM FINAL|NEW|OLD TABLE (INSERT INTO T …)`: a data-change table reference writes T.
static RE_SQL_DATA_CHANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:FINAL|NEW|OLD)\s+TABLE\s*\(\s*(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM|MERGE\s+INTO)\s+([A-Za-z_][A-Za-z0-9_$#@.]*)",
    )
    .expect("RE_SQL_DATA_CHANGE")
});
static RE_PROGRAM_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bPROGRAM-ID\b").expect("RE_PROGRAM_ID"));
static RE_PROCEDURE_DIVISION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bPROCEDURE\s+DIVISION\b").expect("RE_PROCEDURE_DIVISION"));
static RE_SELECT_ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)\bSELECT\s+(?:OPTIONAL\s+)?([A-Za-z0-9][A-Za-z0-9-]*)\s+ASSIGN\s+(?:TO\s+)?(?:'([^']+)'|"([^"]+)"|([A-Za-z0-9#$@][A-Za-z0-9#$@-]*))"#,
    )
    .expect("RE_SELECT_ASSIGN")
});
static RE_DYN_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s.])CALL\s+([A-Za-z][A-Za-z0-9-]*)").expect("RE_DYN_CALL")
});
static RE_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)(?:^|\s)\d{1,2}\s+([A-Za-z0-9][A-Za-z0-9-]*)\b([^.'"]*?)\bVALUES?\s+(?:IS\s+|ARE\s+)?(?:'([^']*)'|"([^"]*)")"#,
    )
    .expect("RE_VALUE")
});
static RE_MOVE: LazyLock<Regex> = LazyLock::new(|| {
    // Group 1 is the whole sending operand (any expression: `ALL 'X'`, `FUNCTION TRIM(A)`,
    // `CORRESPONDING A`); only a lone literal records a value, anything else invalidates.
    // `MOVE` as a whole COBOL word (not the tail of `WS-MOVE`); a `.` inside the operand is only a
    // decimal point when a digit follows it.
    Regex::new(r"(?is)(?:^|[^A-Za-z0-9-])MOVE\s+((?:[^.]|\.[0-9])*?)\s+TO\s+").expect("RE_MOVE")
});
static RE_SET_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\bSET\s+([A-Za-z0-9][A-Za-z0-9-]*(?:\s+[A-Za-z0-9][A-Za-z0-9-]*)*)\s+TO\s+ENTRY\s+(?:'([^']*)'|"([^"]*)"|([A-Za-z0-9][A-Za-z0-9-]*))"#,
    )
    .expect("RE_SET_ENTRY")
});
static RE_PROGRAM_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z#$@][A-Za-z0-9#$@_-]{0,29}$").expect("RE_PROGRAM_NAME"));
static RE_DDNAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z#$@][A-Z0-9#$@]{0,7}$").expect("RE_DDNAME"));

/// COBOL statement verbs and clause words that end a `MOVE … TO` receiving list (any `END-…` too).
const STOP_WORDS: &[&str] = &[
    "ACCEPT",
    "ADD",
    "ALLOCATE",
    "ALTER",
    "CALL",
    "CANCEL",
    "CLOSE",
    "COMMIT",
    "COMPUTE",
    "CONTINUE",
    "DELETE",
    "DISABLE",
    "DISPLAY",
    "DIVIDE",
    "ELSE",
    "ENABLE",
    "ENTRY",
    "EVALUATE",
    "EXEC",
    "EXIT",
    "FREE",
    "GENERATE",
    "GO",
    "GOBACK",
    "IF",
    "INITIALIZE",
    "INITIATE",
    "INSPECT",
    "INVOKE",
    "JSON",
    "MERGE",
    "MOVE",
    "MULTIPLY",
    "NOT",
    "ON",
    "OPEN",
    "PERFORM",
    "PURGE",
    "RAISE",
    "READ",
    "RECEIVE",
    "RELEASE",
    "RESUME",
    "RETURN",
    "REWRITE",
    "ROLLBACK",
    "SEARCH",
    "SEND",
    "SET",
    "SORT",
    "START",
    "STOP",
    "STRING",
    "SUBTRACT",
    "SUPPRESS",
    "TERMINATE",
    "THEN",
    "UNLOCK",
    "UNSTRING",
    "USE",
    "WHEN",
    "WRITE",
    "XML",
];
/// Words that end a `FROM t [alias]` table list or are not table names.
const SQL_STOP: &[&str] = &[
    "WHERE",
    "GROUP",
    "ORDER",
    "HAVING",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "CROSS",
    "OUTER",
    "ON",
    "UNION",
    "FETCH",
    "FOR",
    "WITH",
    "EXCEPT",
    "INTERSECT",
    "SET",
    "VALUES",
    "USING",
    "WHEN",
    "OPTIMIZE",
    "QUERYNO",
    "SKIP",
    "ISOLATION",
    "INTO",
    "SELECT",
    "AS",
    "FINAL",
    "TABLE",
    "LATERAL",
    "OLD",
    "NEW",
    "END-EXEC",
    "DESCRIPTOR",
];

/// Whether a line-start directive selects free format, outside comments and literals.
pub(crate) fn source_is_free_format(text: &str) -> bool {
    RE_SOURCE_FREE.is_match(text)
}

/// Add the COBOL text links to `ex`, the tree-sitter extraction of `file`. See the module docs.
/// A free-format or continued-literal source is read as its [`LogicalSource`], like the grammar
/// reads it, and every span maps back to the file at the end.
pub fn supplement(file: &SourceFile, ex: &mut Extraction) {
    let Some(logical) = LogicalSource::of(&file.text) else {
        supplement_text(file, ex);
        return;
    };
    logical.to_logical(ex);
    let as_parsed = SourceFile {
        text: logical.text.clone(),
        ..file.clone()
    };
    supplement_text(&as_parsed, ex);
    logical.to_file(ex, &file.text);
}

fn supplement_text(file: &SourceFile, ex: &mut Extraction) {
    let free = source_is_free_format(&file.text);
    let (clean, code) = lexical_views(&file.text, free);
    // COPY … REPLACING / REPLACE pseudo-text is replacement text, never source, for every pass.
    let code = RE_PSEUDO_TEXT
        .replace_all(&code, |c: &regex::Captures| blank(&c[0]))
        .into_owned();
    let code_no_exec = RE_EXEC_ANY
        .replace_all(&code, |c: &regex::Captures| blank(&c[0]))
        .into_owned();
    let lines = LineIndex::new(&code);
    let mut anchors = Anchors::of(file, ex, &code);
    if let Some((sym, node, contains)) = anchors.copybook.take() {
        ex.local_edges.extend(contains);
        anchors.member = Some(node.name.clone());
        ex.nodes.push(node);
        anchors.fallback = sym;
    }
    let lits = Literals::per_program(&anchors, &code_no_exec, &clean, &lines);
    let mut p = Pass {
        path: &file.path,
        clean: &clean,
        lines: &lines,
        anchors,
        out: Out::default(),
        lits,
        decisions: HashMap::new(),
        ddnames: HashSet::new(),
        copies: HashMap::new(),
    };
    p.exec_cics(&code);
    p.exec_sql(&code);
    p.copies(&code_no_exec);
    p.assigns(&code_no_exec);
    p.dynamic_calls(&code_no_exec, ex);
    p.finish(ex);
}

/// The end of a PROCEDURE DIVISION: the next program's IDENTIFICATION DIVISION or an END PROGRAM.
static RE_PROCEDURE_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:IDENTIFICATION|ID)\s+DIVISION\b|\bEND\s+PROGRAM\b")
        .expect("RE_PROCEDURE_END")
});

/// The text the COBOL grammar parses: `text` with every `COPY …` statement inside a PROCEDURE
/// DIVISION turned into `EXIT` plus spaces, its terminating period kept (same length; line breaks
/// kept), or `None` when there is none. See [`copy_statement_end`] for where a COPY ends.
///
/// arborium-cobol cannot parse what follows such a COPY: `COPY CSSETATY REPLACING ==A== BY ==B==`
/// is an error, and after `COPY 'CSSTRPFY' .` the next paragraph header is an error, so that
/// paragraph gets no node. `EXIT .` parses and is followed by a header, and a statement still
/// stands where the COPY was, so the paragraph and program spans keep covering it. A COPY yields
/// no node or edge from the tree (this module reads COPY links from the source text), so nothing
/// is lost; spans and names are still read from the original text at the same offsets, and the
/// stub classifier re-reads that text, so a paragraph holding only a COPY is not an EXIT stub.
pub(crate) fn grammar_view(text: &str) -> Option<String> {
    let free = source_is_free_format(text);
    let (_, code) = lexical_views(text, free);
    // Pseudo-text is one COPY operand: spaces (line breaks kept) then `~` on its closing `=`, so
    // it is one word that ends where the pseudo-text ends. EXEC blocks hold no COPY statement
    // (`EXEC CICS ISSUE COPY` is not one).
    let code = RE_PSEUDO_TEXT
        .replace_all(&code, |c: &regex::Captures| {
            let mut b = blank(&c[0]);
            let end = b.len();
            b.replace_range(end - 1..end, "~");
            b
        })
        .into_owned();
    let code = RE_EXEC_ANY
        .replace_all(&code, |c: &regex::Captures| blank(&c[0]))
        .into_owned();
    // (COPY's first byte, the end of what is hidden, whether that end is its own period)
    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    let mut at = 0;
    while let Some(div) = RE_PROCEDURE_DIVISION.find_at(&code, at) {
        let end = RE_PROCEDURE_END
            .find_at(&code, div.end())
            .map_or(code.len(), |m| m.start());
        for c in RE_COPY.find_iter(&code[div.end()..end]) {
            let from = div.end() + c.start();
            // The match may start on the separator before COPY.
            let from = from + code[from..].find(['C', 'c']).unwrap_or(0);
            // Ranges are disjoint: a COPY inside the last one's REPLACING operands is not another.
            if ranges.last().is_some_and(|&(_, stop, _)| from < stop) {
                continue;
            }
            let named = div.end() + c.end();
            let (stop, period) = match copy_statement_end(&code, named, end) {
                Ok(p) => (p, true),
                Err(clause_end) => (clause_end, false),
            };
            ranges.push((from, stop, period));
        }
        at = end.max(div.end());
    }
    if ranges.is_empty() {
        return None;
    }
    let mut view = text.as_bytes().to_vec();
    for (from, to, period) in ranges {
        // `COPY` → `EXIT`; the rest up to (not including) the period → spaces, line breaks kept.
        view[from..from + 4].copy_from_slice(b"EXIT");
        for b in &mut view[from + 4..to] {
            if *b != b'\n' && *b != b'\r' {
                *b = b' ';
            }
        }
        // A COPY with no period of its own (a compile error) hides its words through its last
        // complete clause, and gets a period on that word's last byte (never a line break), so
        // a header after it parses.
        if !period {
            view[to - 1] = b'.';
        }
    }
    // Only ASCII bytes were written over whole statements' bytes, so this is valid UTF-8.
    String::from_utf8(view).ok()
}

/// `Ok`: the byte of the period that ends the COPY statement whose member name ends at `named`.
/// `Err`: it has none (a compile error); the end of its last complete clause (`named` at least).
///
/// The words after the member must be the statement's own clauses, in order:
/// `[OF|IN library] [SUPPRESS] [REPLACING {[LEADING|TRAILING] operand BY operand}…]`, and the
/// period may only follow a complete clause. A literal is one word in `code` (its contents are
/// `~`), and so is pseudo-text (see [`grammar_view`]). Any other word (the next statement, or the
/// next paragraph's header) means no period of its own. One forward pass with a fixed state, so
/// the scan stops within the statement.
fn copy_statement_end(code: &str, named: usize, end: usize) -> Result<usize, usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum At {
        /// After the member name or a library: OF/IN, SUPPRESS, REPLACING or the period.
        Head,
        /// After OF/IN: the library name.
        Library,
        /// After REPLACING or LEADING/TRAILING: a first operand.
        First,
        /// After a first operand: BY.
        By,
        /// After BY: a second operand.
        Second,
        /// After a complete pair: another pair or the period.
        Pair,
    }
    let mut at = At::Head;
    let mut library = false;
    let mut complete = named;
    let bytes = code.as_bytes();
    let sep = |b: u8| b.is_ascii_whitespace() || b == b',' || b == b';';
    let mut i = named;
    while i < end {
        if sep(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < end && !sep(bytes[i]) {
            i += 1;
        }
        // A period ends the statement when a separator or the end follows it.
        let (word, period) = match code[start..i].strip_suffix('.') {
            Some(w) => (w, Some(i - 1)),
            None => (&code[start..i], None),
        };
        if !word.is_empty() {
            let w = word.to_ascii_uppercase();
            at = match (at, w.as_str()) {
                (At::Head, "OF" | "IN") if !library => {
                    library = true;
                    At::Library
                }
                (At::Head, "SUPPRESS") => At::Head,
                (At::Head, "REPLACING") => At::First,
                (At::Library, w) if !is_copy_keyword(w) => At::Head,
                (At::First | At::Pair, "LEADING" | "TRAILING") => At::First,
                (At::First | At::Pair, w) if !is_copy_keyword(w) => At::By,
                (At::By, "BY") => At::Second,
                (At::Second, w) if !is_copy_keyword(w) => At::Pair,
                _ => return Err(complete),
            };
            if matches!(at, At::Head | At::Pair) {
                complete = start + word.len();
            }
        }
        if let Some(p) = period {
            return if matches!(at, At::Head | At::Pair) {
                Ok(p)
            } else {
                Err(complete)
            };
        }
    }
    Err(complete)
}

/// The words of a COPY statement's own clauses.
fn is_copy_keyword(w: &str) -> bool {
    matches!(
        w,
        "OF" | "IN"
            | "SUPPRESS"
            | "REPLACING"
            | "BY"
            | "LEADING"
            | "TRAILING"
            | "COPY"
            | "EXEC"
            | "END-EXEC"
    )
}

// ── lexical views ──────────────────────────────────────────────────────────────────────────────

/// Same-length copy of `s` with every byte replaced by a space, newlines kept.
fn blank(s: &str) -> String {
    s.bytes()
        .map(|b| if b == b'\n' { '\n' } else { ' ' })
        .collect()
}

fn blank_bytes(v: &mut [u8], from: usize, to: usize) {
    for x in &mut v[from..to] {
        if *x != b'\n' {
            *x = b' ';
        }
    }
}

/// Two same-length views of COBOL source: `clean` (comments and, in fixed format, the sequence
/// and identification areas blanked) and `code` (`clean` with every literal's CONTENTS replaced
/// by `~`; the quote characters stay). SQL comments are blanked in both views before matching
/// EXEC blocks. Offsets are identical in the source and both views. Whole comment/literal
/// contents and ASCII delimiters are overwritten with ASCII, so both stay valid UTF-8.
fn lexical_views(text: &str, free: bool) -> (String, String) {
    let mut clean: Vec<u8> = text.as_bytes().to_vec();
    let mut code: Vec<u8> = clean.clone();
    let mut start = 0usize;
    let mut after_exec = false;
    let mut in_sql = false;
    let mut sql_block_comment = false;
    for line in text.split_inclusive('\n') {
        let end = start + line.len();
        let body_end = start + line.trim_end_matches(['\n', '\r']).len();
        let mut from = start;
        let mut stop = body_end;
        // A fixed-format card: columns 1-6 hold only digits or spaces (the sequence area). A
        // line that does not look like a card (free-format code from column 1) is left whole.
        let b = line.as_bytes();
        let card = !free && b.len() > 6 && b[..6].iter().all(|c| c.is_ascii_digit() || *c == b' ');
        if card {
            blank_bytes(&mut clean, start, start + 6);
            blank_bytes(&mut code, start, start + 6);
            if matches!(b[6], b'*' | b'/') {
                blank_bytes(&mut clean, start, body_end);
                blank_bytes(&mut code, start, body_end);
                start = end;
                continue;
            }
            // Columns 73-80 (identification area): only on a card of at most 80 columns whose
            // column 72 is blank, so code that runs past column 72 is never cut.
            if has_identification_area(&b[..body_end - start]) {
                blank_bytes(&mut clean, start + 72, body_end);
                blank_bytes(&mut code, start + 72, body_end);
                stop = start + 72;
            }
            from = start + 7;
        }
        // Literals, COBOL comments and SQL comments, left to right. SQL state survives cards;
        // only their code areas participate, so comments/identification cannot end a block.
        let mut i = from;
        while i < stop {
            let c = clean[i];
            if sql_block_comment {
                let begin = i;
                while i < stop {
                    if clean[i] == b'*' && i + 1 < stop && clean[i + 1] == b'/' {
                        i += 2;
                        sql_block_comment = false;
                        break;
                    }
                    i += 1;
                }
                blank_bytes(&mut clean, begin, i);
                blank_bytes(&mut code, begin, i);
            } else if in_sql && c == b'/' && i + 1 < stop && clean[i + 1] == b'*' {
                blank_bytes(&mut clean, i, i + 2);
                blank_bytes(&mut code, i, i + 2);
                i += 2;
                sql_block_comment = true;
            } else if in_sql && c == b'-' && i + 1 < stop && clean[i + 1] == b'-' {
                blank_bytes(&mut clean, i, stop);
                blank_bytes(&mut code, i, stop);
                break;
            } else if c == b'\'' || c == b'"' {
                after_exec = false;
                let mut j = i + 1;
                while j < stop {
                    if clean[j] == c {
                        if j + 1 < stop && clean[j + 1] == c {
                            j += 2; // a doubled quote is an escaped quote
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                let close = j.min(stop);
                for x in &mut code[i + 1..close] {
                    *x = b'~';
                }
                i = close + 1;
            } else if c == b'*' && i + 1 < stop && clean[i + 1] == b'>' {
                blank_bytes(&mut clean, i, body_end);
                blank_bytes(&mut code, i, body_end);
                break;
            } else if is_ident_byte(c) && c != b'.' {
                let begin = i;
                while i < stop
                    && is_ident_byte(clean[i])
                    && clean[i] != b'.'
                    && !(in_sql && clean[i] == b'-' && i + 1 < stop && clean[i + 1] == b'-')
                {
                    i += 1;
                }
                let word = &clean[begin..i];
                if in_sql {
                    if word.eq_ignore_ascii_case(b"END-EXEC") {
                        in_sql = false;
                    }
                } else {
                    in_sql = after_exec && word.eq_ignore_ascii_case(b"SQL");
                    after_exec = !in_sql && word.eq_ignore_ascii_case(b"EXEC");
                }
            } else {
                if !c.is_ascii_whitespace() {
                    after_exec = false;
                }
                i += 1;
            }
        }
        start = end;
    }
    let to_string = |v: Vec<u8>| match String::from_utf8(v) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    (to_string(clean), to_string(code))
}

/// Columns 73-80 are identification only on a card of at most 80 columns with column 72 blank.
pub(crate) fn has_identification_area(body: &[u8]) -> bool {
    body.len() > 72 && body.len() <= 80 && body[71] == b' '
}

// ── logical source ─────────────────────────────────────────────────────────────────────────────

/// `len` logical bytes at `at` came from the file bytes `from..from + len`, one to one (the
/// content may be blanked).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    at: usize,
    from: usize,
    len: usize,
}

/// Code bytes a fixed-format card holds (columns 8-72).
const CARD_CODE_COLS: usize = 65;

/// The source the COBOL grammar and the text pass read when it differs from the file:
///
/// - **free format** (`>>SOURCE FORMAT FREE`, `$SET SOURCEFORMAT"FREE"`): each line becomes a
///   fixed-format card (code from column 8, a `*` or `/` comment line in the indicator column), a
///   line longer than a card is split at a blank outside literals and comments, and directive
///   lines are blanked. The grammar reads columns 1-6 as the sequence area and 73+ as the
///   identification area, so free-format code from column 1 would otherwise lose its program.
/// - **continued literals** (fixed format): a literal still open at the end of a card, continued
///   by a `-` card whose first non-blank is the same quote, is joined onto one line, so
///   `PROGRAM('SHIP` + `-    'PGM')` reads `PROGRAM('SHIPPGM')`. The rest of the continuation card
///   follows on its own line, at its own columns. The first card's part runs to column 72, or to
///   the end of a shorter line (trailing blanks an editor dropped are not restored).
///
/// Every file byte the logical text holds is in a [`Run`], in file order, so every span maps back
/// to the file's own bytes and lines ([`LogicalSource::to_file`]). Added bytes (card padding,
/// line breaks) map to the nearest file byte.
pub struct LogicalSource {
    pub text: String,
    runs: Vec<Run>,
    file_len: usize,
}

#[derive(Default)]
struct LogicalBuilder<'a> {
    src: &'a [u8],
    out: Vec<u8>,
    runs: Vec<Run>,
}

impl LogicalBuilder<'_> {
    fn copy(&mut self, from: usize, to: usize) {
        if to <= from {
            return;
        }
        let at = self.out.len();
        self.out.extend_from_slice(&self.src[from..to]);
        match self.runs.last_mut() {
            Some(r) if r.at + r.len == at && r.from + r.len == from => r.len += to - from,
            _ => self.runs.push(Run {
                at,
                from,
                len: to - from,
            }),
        }
    }

    fn copy_blank(&mut self, from: usize, to: usize) {
        let at = self.out.len();
        self.copy(from, to);
        blank_bytes(&mut self.out, at, at + (to - from));
    }

    fn add(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    /// End a line that holds a joined literal. One of 73-80 columns with column 72 blank would
    /// read as a card with an identification area, and the text pass would cut the literal's
    /// tail: pad it past column 80.
    fn end_joined_line(&mut self) {
        let start = self
            .out
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        if has_identification_area(&self.out[start..]) {
            let len = self.out.len() - start;
            self.pad(81 - len);
        }
        self.add("\n");
    }

    fn pad(&mut self, cols: usize) {
        self.out.extend(std::iter::repeat_n(b' ', cols));
    }

    /// Lay every free-format line out as a fixed-format card (see [`LogicalSource`]). `mf`: Micro
    /// Focus free format, where `*` or `/` in column 1 starts a comment line; in standard free
    /// format only `*>` does, and a `*` or `/` there is an operator continuing an expression.
    fn free_format(&mut self, mf: bool) {
        let src = self.src;
        let mut start = 0;
        while start < src.len() {
            let end = src[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(src.len(), |i| start + i + 1);
            let mut body_end = end;
            while body_end > start && matches!(src[body_end - 1], b'\n' | b'\r') {
                body_end -= 1;
            }
            let body = &src[start..body_end];
            let lead = body.iter().take_while(|b| b.is_ascii_whitespace()).count();
            let rest = &body[lead..];
            if rest.is_empty() {
                self.copy(start, end);
            } else if rest.starts_with(b">>") || rest.starts_with(b"$") {
                // A compiler directive is not code.
                self.copy_blank(start, body_end);
                self.copy(body_end, end);
            } else if mf
                && lead == 0
                && (body[0] == b'/' || (body[0] == b'*' && body.get(1) != Some(&b'>')))
            {
                // A comment line: its marker lands in the indicator column.
                self.pad(6);
                self.copy(start, end);
            } else {
                self.pad(7);
                let mut s = start;
                while body_end - s > CARD_CODE_COLS {
                    let Some(p) = card_split(src, s, body_end) else {
                        break;
                    };
                    self.copy(s, p);
                    self.add("\n");
                    self.pad(7);
                    s = p + 1;
                }
                self.copy(s, end);
            }
            start = end;
        }
    }

    /// Join fixed-format continued literals (see [`LogicalSource`]); false when there is none.
    fn continued_literals(&mut self) -> bool {
        let src = self.src;
        let mut lines: Vec<(usize, usize)> = Vec::new(); // (start, end incl. newline)
        let mut start = 0;
        while start < src.len() {
            let end = src[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(src.len(), |i| start + i + 1);
            lines.push((start, end));
            start = end;
        }
        let stop = |i: usize| -> usize {
            let (s, e) = lines[i];
            let mut b = e;
            while b > s && matches!(src[b - 1], b'\n' | b'\r') {
                b -= 1;
            }
            // Column 72 in bytes, moved back to a character boundary so the logical text stays
            // valid UTF-8 and the offset map exact.
            let mut t = s + (b - s).min(72);
            while t > s && t < src.len() && (src[t] & 0xC0) == 0x80 {
                t -= 1;
            }
            t
        };
        let card = |i: usize| -> Option<&[u8]> {
            let (s, e) = lines[i];
            let b = &src[s..e];
            (b.len() > 7 && b[..6].iter().all(|c| c.is_ascii_digit() || *c == b' ')).then_some(b)
        };
        // The quote that opens the literal a continuation card at `i` carries on, and where.
        let continuation = |i: usize, q: u8| -> Option<usize> {
            let b = card(i)?;
            if b[6] != b'-' {
                return None;
            }
            let s = lines[i].0;
            let at = (s + 7..stop(i)).find(|&p| src[p] != b' ')?;
            (src[at] == q).then_some(at)
        };
        // The continuation of a literal open on line `i`: the next line that is not a comment
        // card or blank, when it is a continuation card carrying the same quote.
        let continuation_after = |i: usize, q: u8| -> Option<(usize, usize)> {
            let k = (i + 1..lines.len()).find(|&k| {
                let (s, e) = lines[k];
                let blank = src[s..e].iter().all(u8::is_ascii_whitespace);
                !blank && !card(k).is_some_and(|b| matches!(b[6], b'*' | b'/'))
            })?;
            continuation(k, q).map(|at| (k, at))
        };
        let mut joined = false;
        let mut i = 0;
        let mut col = 0; // where line i's unread code starts (after a joined literal)
        while i < lines.len() {
            let (s, e) = lines[i];
            let open = card(i)
                .filter(|b| !matches!(b[6], b'*' | b'/'))
                .and_then(|_| open_literal(src, s + col.max(7), stop(i)));
            let Some((q, (first, next))) =
                open.and_then(|q| continuation_after(i, q).map(|c| (q, c)))
            else {
                self.copy(s + col, e);
                i += 1;
                col = 0;
                continue;
            };
            joined = true;
            self.copy(s + col, stop(i));
            let (mut j, mut at) = (first, next);
            loop {
                let close = literal_close(src, at + 1, stop(j), q);
                match close {
                    Some(c) => {
                        self.copy(at + 1, c + 1);
                        self.end_joined_line();
                        col = c + 1 - lines[j].0;
                        self.pad(col);
                        i = j;
                        break;
                    }
                    None => {
                        self.copy(at + 1, stop(j));
                        match continuation_after(j, q) {
                            Some((k, n)) => {
                                j = k;
                                at = n;
                            }
                            None => {
                                self.end_joined_line();
                                i = j + 1;
                                col = 0;
                                break;
                            }
                        }
                    }
                }
            }
        }
        joined
    }
}

/// Whether any line is a fixed-format continuation card (`-` in column 7): the cheap test that
/// spares most sources the logical-source build.
fn has_continuation_card(text: &str) -> bool {
    text.split('\n').any(|l| {
        let b = l.as_bytes();
        b.len() > 7 && b[6] == b'-' && b[..6].iter().all(|c| c.is_ascii_digit() || *c == b' ')
    })
}

/// The quote of a literal still open at `stop`, scanning `src[from..stop]` outside literals.
fn open_literal(src: &[u8], from: usize, stop: usize) -> Option<u8> {
    let mut open: Option<u8> = None;
    let mut i = from;
    while i < stop {
        let c = src[i];
        match open {
            Some(q) if c == q => open = None, // a doubled quote closes and reopens
            Some(_) => {}
            None if c == b'\'' || c == b'"' => open = Some(c),
            None if c == b'*' && src.get(i + 1) == Some(&b'>') => return None,
            None => {}
        }
        i += 1;
    }
    open
}

/// The closing quote of a literal whose contents start at `from` (a doubled quote is content).
fn literal_close(src: &[u8], from: usize, stop: usize, q: u8) -> Option<usize> {
    let mut i = from;
    while i < stop {
        if src[i] == q {
            if i + 1 < stop && src[i + 1] == q {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The last blank in `src[s..=s + CARD_CODE_COLS]` outside literals and before a `*>` comment or
/// an SQL `--` comment: where a free-format line longer than a card breaks onto the next card. A
/// comment runs to the end of its line, so nothing after it moves (any `--` counts, as the SQL
/// lexer reads it).
fn card_split(src: &[u8], s: usize, body_end: usize) -> Option<usize> {
    let limit = (s + CARD_CODE_COLS).min(body_end);
    let mut quote: Option<u8> = None;
    let mut best = None;
    for i in s..=limit.min(body_end - 1) {
        let c = src[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'\'' || c == b'"' => quote = Some(c),
            None if c == b'*' && src.get(i + 1) == Some(&b'>') => return best,
            None if c == b'-' && src.get(i + 1) == Some(&b'-') => {
                return best;
            }
            None if c == b' ' && i > s => best = Some(i),
            None => {}
        }
    }
    best
}

impl LogicalSource {
    /// `None` when the file is already fixed-format cards with no continued literal: the grammar
    /// and the text pass read the file itself.
    pub fn of(text: &str) -> Option<Self> {
        let mut b = LogicalBuilder {
            src: text.as_bytes(),
            ..Default::default()
        };
        if let Some(m) = RE_SOURCE_FREE.find(text) {
            b.free_format(m.as_str().contains('$'));
        } else if !has_continuation_card(text) || !b.continued_literals() {
            return None;
        }
        // Every cut is at an ASCII byte or a character boundary; should one not be, the file is
        // read as it is rather than through an inexact map.
        let text_out = String::from_utf8(b.out).ok()?;
        Some(Self {
            text: text_out,
            runs: b.runs,
            file_len: text.len(),
        })
    }

    /// The file byte a span starting at logical byte `b` starts at.
    fn file_start(&self, b: usize) -> usize {
        let k = self.runs.partition_point(|r| r.at + r.len <= b);
        match self.runs.get(k) {
            Some(r) if r.at <= b => r.from + (b - r.at),
            Some(r) => r.from,
            None => self.file_len,
        }
    }

    /// The file byte a span ending (exclusive) at logical byte `b` ends at.
    fn file_end(&self, b: usize) -> usize {
        let k = self.runs.partition_point(|r| r.at < b);
        k.checked_sub(1).map_or(0, |k| {
            let r = self.runs[k];
            r.from + (b - r.at).min(r.len)
        })
    }

    fn logical_start(&self, f: usize) -> usize {
        let k = self.runs.partition_point(|r| r.from + r.len <= f);
        match self.runs.get(k) {
            Some(r) if r.from <= f => r.at + (f - r.from),
            Some(r) => r.at,
            None => self.text.len(),
        }
    }

    fn logical_end(&self, f: usize) -> usize {
        let k = self.runs.partition_point(|r| r.from < f);
        k.checked_sub(1).map_or(0, |k| {
            let r = self.runs[k];
            r.at + (f - r.from).min(r.len)
        })
    }

    /// Map every span of `ex` (parsed from [`Self::text`]) back to the file `file_text`.
    pub fn to_file(&self, ex: &mut Extraction, file_text: &str) {
        let lines = LineIndex::new(file_text);
        Self::map(ex, |s| {
            let a = self.file_start(s.start_byte as usize);
            let b = self.file_end(s.end_byte as usize).max(a);
            lines.span(a, b)
        });
    }

    /// Map every span of `ex` (file bytes) onto [`Self::text`].
    pub fn to_logical(&self, ex: &mut Extraction) {
        let lines = LineIndex::new(&self.text);
        Self::map(ex, |s| {
            let a = self.logical_start(s.start_byte as usize);
            let b = self.logical_end(s.end_byte as usize).max(a);
            lines.span(a, b)
        });
    }

    fn map(ex: &mut Extraction, f: impl Fn(&Span) -> Span) {
        let one = |s: &mut Span| {
            if *s != Span::ZERO {
                *s = f(s);
            }
        };
        for n in &mut ex.nodes {
            one(&mut n.location.span);
            // The File node's unparsed region (scanner patch 4) is a span of the parsed text too.
            let key = crate::treesitter::COBOL_UNPARSED_SPAN;
            if let Some(mut s) = n
                .metadata
                .get(key)
                .and_then(|v| serde_json::from_value::<Span>(v.clone()).ok())
            {
                one(&mut s);
                n.metadata.insert(
                    key.to_string(),
                    serde_json::to_value(s).expect("a Span serializes"),
                );
            }
        }
        for e in &mut ex.local_edges {
            if let Some(l) = &mut e.location {
                one(&mut l.span);
            }
        }
        for r in &mut ex.refs {
            one(&mut r.location.span);
        }
    }
}

// ── ref roles ──────────────────────────────────────────────────────────────────────────────────

use wicked_estate_core::refs::cobol_ref::{
    COPIES as COPIES_KEY, COPYBOOK as COPYBOOK_KEY, DATA as ROLE_DATA, LABEL as ROLE_LABEL,
    OPEN as OPEN_KEY, OWNER_PROGRAM as PROGRAM_KEY, PARENTS as PARENTS_KEY,
    PROGRAM as ROLE_PROGRAM, PROGRAMS as PROGRAMS_KEY, QUALIFIERS as QUALIFIERS_KEY,
    ROLE as ROLE_KEY,
};

fn hint(r: &mut UnresolvedRef, key: &str, value: serde_json::Value) {
    r.hints.insert(key.to_string(), value);
}

/// The nearest of `node` and its ancestors (at most `depth` up) whose kind is in `kinds`.
fn up<'t>(
    node: tree_sitter::Node<'t>,
    kinds: &[&str],
    depth: usize,
) -> Option<tree_sitter::Node<'t>> {
    let mut n = Some(node);
    for _ in 0..=depth {
        let cur = n?;
        if kinds.contains(&cur.kind()) {
            return Some(cur);
        }
        n = cur.parent();
    }
    None
}

fn upper_text(n: tree_sitter::Node, src: &[u8]) -> String {
    n.utf8_text(src).unwrap_or("").trim().to_ascii_uppercase()
}

/// Tag each COBOL ref of a tree-sitter extraction with its role by its place in the tree, drop
/// the procedure labels the data-name query also matches (`GO TO P`, `PERFORM P` are qualified
/// words too), and record each data item's groups on its Field node. Runs on the parsed text,
/// before any span is mapped back to the file.
pub(crate) fn tag_roles(
    tree: &tree_sitter::Tree,
    src: &[u8],
    nodes: &mut [Node],
    refs: &mut Vec<UnresolvedRef>,
) {
    let root = tree.root_node();
    refs.retain_mut(|r| {
        let span = &r.location.span;
        let Some(at) =
            root.descendant_for_byte_range(span.start_byte as usize, span.end_byte as usize)
        else {
            return true;
        };
        match r.kind {
            EdgeKind::References => {
                let Some(q) = up(at, &["qualified_word"], 2) else {
                    return true;
                };
                if q.parent()
                    .is_some_and(|p| matches!(p.kind(), "label" | "alter_option"))
                {
                    return false; // a procedure name, never a data use
                }
                let mut cursor = q.walk();
                let qualifiers: Vec<serde_json::Value> = q
                    .named_children(&mut cursor)
                    .filter(|w| w.kind() == "WORD")
                    .skip(1)
                    .map(|w| serde_json::Value::String(upper_text(w, src)))
                    .collect();
                hint(r, ROLE_KEY, ROLE_DATA.into());
                if !qualifiers.is_empty() {
                    hint(r, QUALIFIERS_KEY, serde_json::Value::Array(qualifiers));
                }
            }
            EdgeKind::Calls => {
                let ctx = up(
                    at,
                    &[
                        "call_statement",
                        "perform_procedure",
                        "perform_statement_call_proc",
                        "goto_statement",
                    ],
                    4,
                );
                match ctx.map(|n| n.kind()) {
                    Some("call_statement") => hint(r, ROLE_KEY, ROLE_PROGRAM.into()),
                    Some(_) => hint(r, ROLE_KEY, ROLE_LABEL.into()),
                    None => {}
                }
            }
            _ => {}
        }
        true
    });
    let parents = data_item_parents(root, src);
    for n in nodes.iter_mut().filter(|n| n.kind == NodeKind::Field) {
        if let Some((names, open)) = parents.get(&(n.location.span.start_byte as usize)) {
            let names = names
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect();
            n.metadata
                .insert(PARENTS_KEY.to_string(), serde_json::Value::Array(names));
            if *open {
                n.metadata
                    .insert(OPEN_KEY.to_string(), serde_json::Value::Bool(true));
            }
        }
    }
}

/// Every `data_description` (by start byte) → (its groups and FD, innermost first; whether its
/// outermost group in this file is not an 01/77 level). Level numbers nest the items: an item
/// belongs to the nearest earlier item of a lower level, and 01/77 start a new record.
fn data_item_parents(root: tree_sitter::Node, src: &[u8]) -> HashMap<usize, (Vec<String>, bool)> {
    let mut out = HashMap::new();
    // (level, name) of the open groups; the FD (or none) the current record belongs to.
    let mut stack: Vec<(u32, String)> = Vec::new();
    let mut record_list: Option<usize> = None;
    let mut fd: Option<String> = None;
    let mut cursor = root.walk();
    let mut todo = vec![root];
    while let Some(n) = todo.pop() {
        if n.kind() == "data_description" {
            let mut c = n.walk();
            let kids: Vec<_> = n.named_children(&mut c).collect();
            let level = kids
                .iter()
                .find(|k| k.kind() == "level_number")
                .and_then(|k| upper_text(*k, src).parse::<u32>().ok());
            let name = kids
                .iter()
                .find(|k| k.kind() == "entry_name")
                .map(|k| upper_text(*k, src))
                .unwrap_or_default();
            // A new record list (WORKING-STORAGE, an FD's records) starts a new nesting.
            let list = up_past(n, "data_description");
            if list.map(|p| p.id()) != record_list {
                record_list = list.map(|p| p.id());
                stack.clear();
                fd = list.and_then(|p| file_description_name(p, src));
            }
            if let Some(level) = level {
                if matches!(level, 1 | 77) {
                    stack.clear();
                } else if !matches!(level, 66 | 88) {
                    while stack.last().is_some_and(|(l, _)| *l >= level) {
                        stack.pop();
                    }
                }
                // A 66 RENAMES item belongs to its record (the 01), an 88 condition to the item
                // right before it (a 77 too), every other item to its open groups.
                let held = if level == 66 {
                    &stack[..stack.len().min(1)]
                } else {
                    &stack[..]
                };
                let mut names: Vec<String> = held
                    .iter()
                    .rev()
                    .map(|(_, s)| s.clone())
                    .filter(|s| !s.is_empty() && s != "FILLER")
                    .collect();
                let open = stack
                    .first()
                    .map_or(!matches!(level, 1 | 77), |(l, _)| !matches!(l, 1 | 77));
                names.extend(fd.clone());
                out.insert(n.start_byte(), (names, open));
                if !matches!(level, 66 | 88) {
                    stack.push((level, name));
                }
            }
        }
        let kids: Vec<_> = n.named_children(&mut cursor).collect();
        todo.extend(kids.into_iter().rev());
    }
    out
}

/// The nearest ancestor of `n` whose kind is not `kind`.
fn up_past<'t>(n: tree_sitter::Node<'t>, kind: &str) -> Option<tree_sitter::Node<'t>> {
    let mut p = n.parent();
    while let Some(x) = p.filter(|x| x.kind() == kind) {
        p = x.parent();
    }
    p
}

/// The FD/SD name a record list belongs to: the list's `file_description` parent, or the
/// `file_description_entry` right before it.
fn file_description_name(list: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let fd = list
        .parent()
        .filter(|p| p.kind() == "file_description")
        .or_else(|| {
            list.prev_named_sibling()
                .filter(|p| p.kind() == "file_description_entry")
        })?;
    let mut c = fd.walk();
    let word = fd
        .named_children(&mut c)
        .find(|k| k.kind() == "WORD")
        .or_else(|| {
            let mut c2 = fd.walk();
            fd.named_children(&mut c2)
                .find(|k| k.kind() == "file_description_entry")
                .and_then(|e| {
                    let mut c3 = e.walk();
                    e.named_children(&mut c3).find(|k| k.kind() == "WORD")
                })
        })?;
    Some(upper_text(word, src))
}

/// Byte offset → line/column (0-based, like the tree-sitter spans).
struct LineIndex(Vec<usize>);

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self(starts)
    }
    fn pos(&self, byte: usize) -> (u32, u32) {
        let line = self.0.partition_point(|&s| s <= byte).saturating_sub(1);
        (line as u32, (byte - self.0[line]) as u32)
    }
    fn line1(&self, byte: usize) -> u32 {
        self.pos(byte).0 + 1
    }
    fn span(&self, start: usize, end: usize) -> Span {
        let (sl, sc) = self.pos(start);
        let (el, ec) = self.pos(end);
        Span {
            start_byte: start as u32,
            end_byte: end as u32,
            start_line: sl,
            start_col: sc,
            end_line: el,
            end_col: ec,
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'#' | b'@' | b'.' | b'-')
}

/// A literal operand's value (the match covers the CONTENTS), read from the comment-blanked view
/// at the same offsets, with a doubled quote unescaped.
fn literal_at(clean: &str, m: regex::Match) -> String {
    let q = if clean[..m.start()].ends_with('"') {
        "\""
    } else {
        "'"
    };
    clean[m.start()..m.end()].replace(&q.repeat(2), q)
}

// ── anchors ────────────────────────────────────────────────────────────────────────────────────

/// Index of a program in [`Anchors::modules`]; `None` = no program (a copybook).
type Prog = Option<usize>;

struct Anchors {
    /// (symbol, PROGRAM-ID, start byte, end byte) of every program in the file.
    modules: Vec<(SymbolId, String, usize, usize)>,
    /// (symbol, start byte, end byte) of every paragraph and section in the file.
    units: Vec<(SymbolId, usize, usize)>,
    /// The source when no program holds the construct: the copybook node, else the File node.
    fallback: SymbolId,
    /// A copybook file's definition record + its `Contains` edges, taken by [`supplement`].
    copybook: Option<(SymbolId, Node, Vec<Edge>)>,
    /// The member name when this file is a copybook.
    member: Option<String>,
}

impl Anchors {
    fn of(file: &SourceFile, ex: &Extraction, code: &str) -> Self {
        let modules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Module)
            .map(|n| {
                let s = &n.location.span;
                (
                    n.symbol.clone(),
                    n.name.clone(),
                    s.start_byte as usize,
                    s.end_byte as usize,
                )
            })
            .collect();
        let units = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .map(|n| {
                let s = &n.location.span;
                (n.symbol.clone(), s.start_byte as usize, s.end_byte as usize)
            })
            .collect();
        let copybook = if modules.is_empty() && !RE_PROGRAM_ID.is_match(code) {
            let name = copybook_name(&file.path);
            let sym = copybook_symbol(&name);
            let loc = Location::new(&file.path, Span::ZERO);
            let mut node = Node::new(
                sym.clone(),
                NodeKind::Other("copybook".to_string()),
                name,
                Language::new("cobol"),
                loc.clone(),
            );
            node.signature = Some("COPYBOOK".to_string());
            let contains = ex
                .nodes
                .iter()
                .filter(|n| n.kind != NodeKind::File)
                .map(|n| {
                    Edge::new(
                        sym.clone(),
                        n.symbol.clone(),
                        EdgeKind::Contains,
                        ResolutionTier::Parsed,
                        RESOLVED_BY,
                    )
                    .with_location(loc.clone())
                })
                .collect();
            Some((sym, node, contains))
        } else {
            None
        };
        Self {
            modules,
            units,
            fallback: Symbol::file(&file.path).id(),
            copybook,
            member: None,
        }
    }

    /// The innermost program whose span holds `byte`; none outside every program (the construct
    /// then starts from the copybook or File node).
    fn prog(&self, byte: usize) -> Prog {
        self.modules
            .iter()
            .enumerate()
            .filter(|(_, (_, _, s, e))| *s <= byte && byte < *e)
            .min_by_key(|(_, (_, _, s, e))| e - s)
            .map(|(i, _)| i)
    }

    /// The source of a call or data use at `byte`: the innermost paragraph or section holding it
    /// (the tree-sitter extractor's rule for PERFORM, CALL and data names), else [`Self::at`]'s.
    fn unit(&self, byte: usize) -> SymbolId {
        self.units
            .iter()
            .filter(|(_, s, e)| *s <= byte && byte < *e)
            .min_by_key(|(_, s, e)| e - s)
            .map_or_else(|| self.at(byte).0, |(sym, _, _)| sym.clone())
    }

    /// The PROGRAM-IDs of the programs that hold `byte`, innermost first.
    fn chain(&self, byte: usize) -> Vec<String> {
        let mut held: Vec<_> = self
            .modules
            .iter()
            .filter(|(_, _, s, e)| *s <= byte && byte < *e)
            .collect();
        held.sort_by_key(|(_, _, s, e)| e - s);
        held.into_iter().map(|(_, name, ..)| name.clone()).collect()
    }

    /// The source node for a construct at `byte`, and its PROGRAM-ID when it is a program.
    fn at(&self, byte: usize) -> (SymbolId, Option<&str>) {
        match self.prog(byte) {
            Some(i) => (self.modules[i].0.clone(), Some(self.modules[i].1.as_str())),
            None => (self.fallback.clone(), None),
        }
    }
}

/// A copybook's member name: the file stem, uppercased (`cpy/ordrec.cpy` → `ORDREC`).
fn copybook_name(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    stem.to_ascii_uppercase()
}

fn copybook_symbol(name: &str) -> SymbolId {
    Symbol::synthetic("cobol-copybook", name).id()
}

// ── literal assignments (dynamic CALL, variable map names) ─────────────────────────────────────

/// What each data name can hold, read from ONE program's own source.
#[derive(Default)]
struct Literals {
    /// UPPERCASE data name → (literal, or None for any other source; what assigned it).
    by_name: HashMap<String, Vec<(Option<String>, String)>>,
}

#[derive(Debug, Clone, PartialEq)]
enum Decision {
    Resolved { value: String, evidence: String },
    Unresolved { reason: String },
}

impl Literals {
    /// One literal map per program (its span, minus the spans of programs nested in it), or one
    /// for the whole file when it has no program.
    fn per_program(
        anchors: &Anchors,
        code: &str,
        clean: &str,
        lines: &LineIndex,
    ) -> HashMap<Prog, Literals> {
        let mut out = HashMap::new();
        if anchors.modules.is_empty() {
            out.insert(None, Self::build(code, clean, 0, lines));
            return out;
        }
        let len = code.len();
        for (i, (_, _, s, e)) in anchors.modules.iter().enumerate() {
            let (s, e) = ((*s).min(len), (*e).min(len));
            if !code.is_char_boundary(s) || !code.is_char_boundary(e) {
                continue;
            }
            let mut own: Vec<u8> = code.as_bytes()[s..e].to_vec();
            for (j, (_, _, ns, ne)) in anchors.modules.iter().enumerate() {
                let (ns, ne) = (*ns, (*ne).min(len));
                if j != i && ns >= s && ne <= e && (ns, ne) != (s, e) {
                    blank_bytes(&mut own, ns - s, ne - s);
                }
            }
            let own = String::from_utf8(own).unwrap_or_default();
            out.insert(Some(i), Self::build(&own, &clean[s..e], s, lines));
        }
        out
    }

    /// `code`/`clean` are the same slice of the two views, starting at byte `base` of the file.
    fn build(code: &str, clean: &str, base: usize, lines: &LineIndex) -> Self {
        let line_of = |b: usize| lines.line1(base + b);
        let mut lit = Self::default();
        // VALUE clauses: data descriptions only, i.e. before this program's PROCEDURE DIVISION.
        // A figurative VALUE (SPACES) is only the initial state and is not recorded.
        let data_end = RE_PROCEDURE_DIVISION
            .find(code)
            .map_or(code.len(), |m| m.start());
        for c in RE_VALUE.captures_iter(&code[..data_end]) {
            let name = c[1].to_ascii_uppercase();
            if name == "FILLER" {
                continue;
            }
            let Some(m) = c.get(3).or_else(|| c.get(4)) else {
                continue;
            };
            let at = format!("VALUE (line {})", line_of(c.get(0).unwrap().start()));
            lit.add(name, Some(literal_at(clean, m)), at);
        }
        // MOVE: a literal source records a value; anything else (an identifier, a figurative
        // constant, a number) records that the variable can hold something else.
        for c in RE_MOVE.captures_iter(code) {
            let whole = c.get(0).unwrap();
            let op = c.get(1).unwrap();
            let line = line_of(op.start()); // `whole` may start on the previous line's newline
            let lead = op.as_str().len() - op.as_str().trim_start().len();
            let t = op.as_str().trim();
            let quoted = |q: char| t.len() >= 2 && t.starts_with(q) && t.ends_with(q);
            let lone_literal =
                (quoted('\'') || quoted('"')) && !t[1..t.len() - 1].contains(['\'', '"']);
            let (src, what) = if lone_literal {
                // `code` masks the contents; read them from `clean` at the same offsets.
                let start = op.start() + lead + 1;
                let q = &t[..1];
                let inner = clean[start..start + t.len() - 2].replace(&q.repeat(2), q);
                (Some(inner), format!("MOVE (line {line})"))
            } else {
                let shown = clean[op.start()..op.end()].trim().to_string();
                (None, format!("MOVE {shown} (line {line})"))
            };
            for target in move_targets(&code[whole.end()..]) {
                lit.add(target, src.clone(), what.clone());
            }
        }
        for c in RE_SET_ENTRY.captures_iter(code) {
            let line = line_of(c.get(0).unwrap().start());
            let (value, what) = match c.get(2).or_else(|| c.get(3)) {
                Some(m) => (
                    Some(literal_at(clean, m)),
                    format!("SET ENTRY (line {line})"),
                ),
                None => (None, format!("SET ENTRY {} (line {line})", &c[4])),
            };
            let targets: Vec<_> = c[1].split_whitespace().collect();
            // The receiver capture can span earlier SET statements; only the last one applies.
            let first = targets
                .iter()
                .rposition(|target| target.eq_ignore_ascii_case("SET"))
                .map_or(0, |i| i + 1);
            for target in &targets[first..] {
                lit.add(target.to_ascii_uppercase(), value.clone(), what.clone());
            }
        }
        lit
    }

    fn add(&mut self, name: String, value: Option<String>, what: String) {
        self.by_name.entry(name).or_default().push((value, what));
    }

    fn decide(&self, var: &str) -> Decision {
        let var_u = var.to_ascii_uppercase();
        let Some(entries) = self.by_name.get(&var_u) else {
            return Decision::Unresolved {
                reason: format!(
                    "no literal VALUE, MOVE or SET ENTRY for {var_u} in this program's source (it \
                     may be set in a copybook or at run time)"
                ),
            };
        };
        if let Some((_, what)) = entries.iter().find(|(v, _)| v.is_none()) {
            return Decision::Unresolved {
                reason: format!("{var_u} is also assigned from a non-literal: {what}"),
            };
        }
        let values: BTreeSet<String> = entries
            .iter()
            .filter_map(|(v, _)| v.as_deref())
            .map(|v| v.trim().to_string())
            .collect();
        match values.len() {
            1 => {
                let value = values.into_iter().next().unwrap();
                if !RE_PROGRAM_NAME.is_match(&value) {
                    return Decision::Unresolved {
                        reason: format!("{var_u} holds '{value}', which is not a program name"),
                    };
                }
                let evidence = entries
                    .iter()
                    .map(|(_, w)| w.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Decision::Resolved { value, evidence }
            }
            n => Decision::Unresolved {
                reason: format!(
                    "{var_u} has {n} literal values: {}",
                    values.into_iter().collect::<Vec<_>>().join(", ")
                ),
            },
        }
    }
}

/// The receiving items of a `MOVE … TO` (the code right after `TO`): identifiers up to the period
/// that ends the sentence or the next statement verb. `OF/IN <qualifier>` and subscripts are skipped.
fn move_targets(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() || bytes[i] == b',' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b',' | b'.' | b'(')
        {
            i += 1;
        }
        let tok = rest[start..i].to_ascii_uppercase();
        if skip_next {
            skip_next = false;
        } else if tok == "OF" || tok == "IN" {
            skip_next = true;
        } else if STOP_WORDS.contains(&tok.as_str()) || tok.starts_with("END-") {
            break;
        } else if tok
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        {
            out.push(tok);
        } else if !tok.is_empty() {
            break; // a literal or an operator is not a receiving item: the list has ended
        }
        // A receiver (or its OF/IN qualifier) can carry a spaced subscript or reference
        // modifier. Consume the whole group before looking for the next receiver or verb.
        if i < bytes.len() && bytes[i] == b'(' {
            let mut depth = 0usize;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        if i < bytes.len() && bytes[i] == b'.' {
            break;
        }
    }
    out
}

// ── the pass ───────────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Out {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    edge_keys: HashSet<(SymbolId, SymbolId, EdgeKind)>,
    refs: Vec<UnresolvedRef>,
}

struct Pass<'a> {
    path: &'a str,
    clean: &'a str,
    lines: &'a LineIndex,
    anchors: Anchors,
    out: Out,
    lits: HashMap<Prog, Literals>,
    /// (program, UPPERCASE variable) → the decision, cached; also feeds field metadata.
    decisions: HashMap<(Prog, String), Decision>,
    /// (program, UPPERCASE DD name or ASSIGN operand): not data uses.
    ddnames: HashSet<(Prog, String)>,
    /// Source node (a program, or the copybook/File node) → the copybooks it `COPY`s/`INCLUDE`s.
    copies: HashMap<SymbolId, BTreeSet<String>>,
}

/// A CICS option operand: the literal's value, or the data item a bare operand names.
#[derive(Clone)]
struct Operand {
    value: String,
    literal: bool,
}

impl Pass<'_> {
    fn loc(&self, start: usize, end: usize) -> Location {
        Location::new(self.path, self.lines.span(start, end))
    }

    fn edge(
        &mut self,
        from: SymbolId,
        to: SymbolId,
        kind: EdgeKind,
        tier: ResolutionTier,
        loc: Location,
        meta: &[(&str, &str)],
    ) {
        if !self
            .out
            .edge_keys
            .insert((from.clone(), to.clone(), kind.clone()))
        {
            return;
        }
        let mut e = Edge::new(from, to, kind, tier, RESOLVED_BY).with_location(loc);
        for (k, v) in meta {
            e.metadata.insert(
                (*k).to_string(),
                serde_json::Value::String((*v).to_string()),
            );
        }
        self.out.edges.push(e);
    }

    fn boundary(&mut self, sym: SymbolId, kind: &str, name: &str, loc: Location, reason: String) {
        let node = Node::new(
            sym,
            NodeKind::Other(kind.to_string()),
            name,
            Language::new("cobol"),
            loc,
        )
        .as_boundary(reason);
        self.out.nodes.push(node);
    }

    fn reference(&mut self, from: SymbolId, name: &str, loc: Location) {
        let name = name.trim();
        if name.is_empty() || !name.as_bytes()[0].is_ascii_alphabetic() {
            return;
        }
        let name = name.to_ascii_uppercase();
        let mut r = UnresolvedRef::new(from, name, EdgeKind::References, loc);
        hint(&mut r, ROLE_KEY, ROLE_DATA.into());
        self.out.refs.push(r);
    }

    /// Decide `var` in program `prog` once.
    fn decide(&mut self, prog: Prog, var: &str) -> Decision {
        let key = (prog, var.to_ascii_uppercase());
        if let Some(d) = self.decisions.get(&key) {
            return d.clone();
        }
        let d = self
            .lits
            .get(&prog)
            .map_or_else(|| Literals::default().decide(var), |l| l.decide(var));
        self.decisions.insert(key, d.clone());
        d
    }

    /// The `Calls` ref name for a dynamic program-name variable in `prog`.
    fn dynamic_ref_name(&mut self, prog: Prog, var: &str) -> String {
        match self.decide(prog, var) {
            Decision::Resolved { value, .. } => {
                format!("{COBOL_DYNAMIC_CALL_PREFIX}{}", value.to_ascii_uppercase())
            }
            Decision::Unresolved { .. } => {
                format!("{COBOL_DYNAMIC_VAR_PREFIX}{}", var.to_ascii_uppercase())
            }
        }
    }

    // ── EXEC CICS ──────────────────────────────────────────────────────────────────────────
    fn exec_cics(&mut self, code: &str) {
        for c in RE_EXEC_CICS.captures_iter(code) {
            let whole = c.get(0).unwrap();
            let body = c.get(1).unwrap();
            let prog = self.anchors.prog(whole.start());
            let (from, program_id) = self.anchors.at(whole.start());
            let program_id = program_id.map(str::to_ascii_uppercase);
            let loc = self.loc(whole.start(), whole.end());
            let verb = body
                .as_str()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            let opts = cics_options(code, self.clean, body.start(), body.end());
            match verb.as_str() {
                "LINK" | "XCTL" => {
                    if let Some(op) = opts.get("PROGRAM").cloned() {
                        let raw = if op.literal {
                            op.value.trim().to_ascii_uppercase()
                        } else {
                            self.dynamic_ref_name(prog, &op.value)
                        };
                        if !raw.is_empty() {
                            // From the paragraph or section it is in, else the program; never
                            // from both (one call site, one caller). From the program itself, a
                            // LINK or XCTL of the program is a self-edge: no ref.
                            let unit = self.anchors.unit(whole.start());
                            let own = op.literal && program_id.as_deref() == Some(raw.as_str());
                            if !(unit == from && own) {
                                let mut r =
                                    UnresolvedRef::new(unit, raw, EdgeKind::Calls, loc.clone());
                                hint(&mut r, ROLE_KEY, ROLE_PROGRAM.into());
                                self.out.refs.push(r);
                            }
                        }
                    }
                }
                "SEND" | "RECEIVE" => self.cics_map(prog, &from, &verb, &opts, &loc),
                _ => {}
            }
            for key in ["COMMAREA", "INTO", "FROM"] {
                if let Some(op) = opts.get(key).filter(|o| !o.literal).cloned() {
                    let unit = self.anchors.unit(whole.start());
                    self.reference(unit, &op.value, loc.clone());
                }
            }
        }
    }

    fn cics_map(
        &mut self,
        prog: Prog,
        from: &SymbolId,
        verb: &str,
        opts: &HashMap<String, Operand>,
        loc: &Location,
    ) {
        let Some((map, map_inferred)) = self.map_operand(prog, opts.get("MAP")) else {
            return;
        };
        let (set, set_inferred) = match opts.get("MAPSET") {
            // An explicit MAPSET that cannot be read leaves the target unknown: no edge.
            Some(op) => match self.map_operand(prog, Some(op)) {
                Some(s) => s,
                None => return,
            },
            None => (map.clone(), false), // CICS: no MAPSET = a mapset named like the map
        };
        let kind = if verb == "SEND" {
            edge_tags::WRITES
        } else {
            edge_tags::READS
        };
        let inferred = map_inferred || set_inferred;
        let tier = if inferred {
            ResolutionTier::Heuristic
        } else {
            ResolutionTier::Parsed
        };
        let sym = Symbol::synthetic("cics-map", format!("{set}.{map}")).id();
        let mut meta = vec![("via", "exec-cics"), ("command", verb)];
        if inferred {
            meta.push(("heuristic", "map-name-literal"));
        }
        self.edge(
            from.clone(),
            sym.clone(),
            edge_tags::other(kind),
            tier,
            loc.clone(),
            &meta,
        );
        self.boundary(
            sym,
            "cics_map",
            &map,
            loc.clone(),
            format!(
                "CICS {verb} MAP: no BMS mapset source (DFHMSD/DFHMDI) defining map {map} in \
                 mapset {set} is indexed"
            ),
        );
    }

    /// A MAP/MAPSET operand: `(name, inferred)` for a literal or a data name holding one literal.
    fn map_operand(&mut self, prog: Prog, op: Option<&Operand>) -> Option<(String, bool)> {
        let op = op?;
        let v = op.value.trim().to_ascii_uppercase();
        if v.is_empty() {
            return None;
        }
        if op.literal {
            return Some((v, false));
        }
        match self.decide(prog, &v) {
            Decision::Resolved { value, .. } => Some((value.to_ascii_uppercase(), true)),
            Decision::Unresolved { .. } => None,
        }
    }

    // ── EXEC SQL ───────────────────────────────────────────────────────────────────────────
    fn exec_sql(&mut self, code: &str) {
        let mut defined: HashSet<String> = HashSet::new();
        let mut used: Vec<(String, Location)> = Vec::new();
        for c in RE_EXEC_SQL.captures_iter(code) {
            let whole = c.get(0).unwrap();
            let (from, _) = self.anchors.at(whole.start());
            let loc = self.loc(whole.start(), whole.end());
            let sql = c.get(1).unwrap();
            let body = sql_identifiers(sql.as_str(), &self.clean[sql.start()..sql.end()]);
            let words: Vec<String> = body
                .split_whitespace()
                .take(4)
                .map(|w| w.to_ascii_uppercase())
                .collect();
            let w = |i: usize| words.get(i).map_or("", String::as_str);

            for h in RE_SQL_HOSTVAR.captures_iter(&body) {
                let unit = self.anchors.unit(whole.start());
                self.reference(unit, &h[1], loc.clone());
            }
            match w(0) {
                "INCLUDE" => {
                    let member = w(1).trim_end_matches('.');
                    if !matches!(member, "SQLCA" | "SQLDA" | "") {
                        self.copy_link(from, member, loc);
                    }
                    continue;
                }
                "DECLARE" if w(2) == "TABLE" => {
                    let t = w(1).to_string();
                    let sym = Symbol::synthetic("db2", t.as_str()).id();
                    let mut node = Node::new(
                        sym,
                        NodeKind::Other("db2_table".to_string()),
                        t.clone(),
                        Language::new("cobol"),
                        loc,
                    );
                    node.signature = Some("DECLARE TABLE".to_string());
                    self.out.nodes.push(node);
                    defined.insert(t);
                    continue;
                }
                "FETCH" | "OPEN" | "CLOSE" | "COMMIT" | "ROLLBACK" | "CONNECT" | "WHENEVER"
                | "BEGIN" | "END" | "CALL" | "SET" | "PREPARE" | "EXECUTE" | "DESCRIBE"
                | "RELEASE" | "SAVEPOINT" | "LOCK" | "GRANT" | "REVOKE" => continue,
                _ => {}
            }
            let statement = w(0).to_string();
            let write_target = match statement.as_str() {
                "INSERT" | "MERGE" if w(1) == "INTO" => Some(w(2).to_string()),
                "UPDATE" => Some(w(1).to_string()),
                "DELETE" if w(1) == "FROM" => Some(w(2).to_string()),
                _ => None,
            }
            .map(|t| t.split(['(', ',']).next().unwrap_or("").to_string())
            .filter(|t| is_table_name(t));
            // Host variables (`:WS-FROM`) are COBOL names with hyphens: mask them so a keyword
            // inside one is not a clause.
            let body = RE_SQL_HOSTVAR
                .replace_all(&body, |c: &regex::Captures| blank(&c[0]))
                .into_owned();
            let ctx = paren_context(&body);
            let ctes: HashSet<String> = RE_SQL_CTE
                .captures_iter(&body)
                .filter(|m| ctx[m.get(0).unwrap().start()].is_none())
                .map(|m| m[1].to_ascii_uppercase())
                .collect();
            let mut tables: Vec<(String, &str)> = Vec::new();
            if let Some(t) = &write_target {
                tables.push((t.clone(), edge_tags::WRITES));
            }
            for m in RE_SQL_DATA_CHANGE.captures_iter(&body) {
                let t = m[1].to_ascii_uppercase();
                if is_table_name(&t) {
                    tables.push((t, edge_tags::WRITES));
                }
            }
            let mut heads: Vec<regex::Match> = RE_SQL_FROM.find_iter(&body).collect();
            if statement == "MERGE" {
                heads.extend(RE_SQL_USING.find_iter(&body));
            }
            for m in heads {
                if !statement_level_or_subselect(&body, &ctx, m.start()) {
                    continue;
                }
                // `DELETE FROM T`: that FROM names the write target, already recorded.
                let delete_from = statement == "DELETE"
                    && body[..m.start()].trim().eq_ignore_ascii_case("DELETE");
                for (k, t) in table_list(&body[m.end()..]).into_iter().enumerate() {
                    if !(delete_from && k == 0) && !ctes.contains(&t) {
                        tables.push((t, edge_tags::READS));
                    }
                }
            }
            for (t, kind) in tables {
                let sym = Symbol::synthetic("db2", t.as_str()).id();
                self.edge(
                    from.clone(),
                    sym,
                    edge_tags::other(kind),
                    ResolutionTier::Heuristic,
                    loc.clone(),
                    &[("via", "exec-sql"), ("statement", statement.as_str())],
                );
                used.push((t, loc.clone()));
            }
        }
        for (t, loc) in used {
            if defined.contains(&t) {
                continue;
            }
            let sym = Symbol::synthetic("db2", t.as_str()).id();
            self.boundary(
                sym,
                "db2_table",
                &t,
                loc,
                format!(
                    "Db2 table named in EXEC SQL; no DCLGEN (EXEC SQL DECLARE {t} TABLE) source is \
                     indexed"
                ),
            );
        }
    }

    // ── COPY / INCLUDE ─────────────────────────────────────────────────────────────────────
    fn copies(&mut self, code: &str) {
        for c in RE_COPY.captures_iter(code) {
            let m = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).unwrap();
            let (from, _) = self.anchors.at(m.start());
            let loc = self.loc(m.start(), m.end());
            let raw = if c.get(3).is_some() {
                m.as_str().to_string()
            } else {
                literal_at(self.clean, m)
            };
            let name = copybook_name(&raw);
            self.copy_link(from, &name, loc);
        }
    }

    fn copy_link(&mut self, from: SymbolId, member: &str, loc: Location) {
        let name = member.trim().trim_end_matches('.').to_ascii_uppercase();
        if name.is_empty() {
            return;
        }
        let sym = copybook_symbol(&name);
        if sym == from {
            return;
        }
        self.copies
            .entry(from.clone())
            .or_default()
            .insert(name.clone());
        self.edge(
            from,
            sym.clone(),
            EdgeKind::Imports,
            ResolutionTier::Parsed,
            loc.clone(),
            &[("via", "copy")],
        );
        self.boundary(
            sym,
            "copybook",
            &name,
            loc,
            format!("COPY {name}: no copybook source named {name} is indexed"),
        );
    }

    // ── SELECT … ASSIGN TO <ddname> ────────────────────────────────────────────────────────
    fn assigns(&mut self, code: &str) {
        for c in RE_SELECT_ASSIGN.captures_iter(code) {
            let whole = c.get(0).unwrap();
            let target = c.get(2).or_else(|| c.get(3)).or_else(|| c.get(4)).unwrap();
            let raw = if c.get(4).is_some() {
                target.as_str().to_ascii_uppercase()
            } else {
                literal_at(self.clean, target).trim().to_ascii_uppercase()
            };
            if matches!(
                raw.as_str(),
                "DISK" | "PRINTER" | "DISPLAY" | "DYNAMIC" | "EXTERNAL" | "USING"
            ) {
                continue;
            }
            let dd = raw.rsplit('-').next().unwrap_or(&raw).to_string();
            if !RE_DDNAME.is_match(&dd) {
                continue;
            }
            let prog = self.anchors.prog(whole.start());
            let (from, pgm) = self.anchors.at(whole.start());
            let Some(pgm) = pgm.map(|p| p.to_ascii_uppercase()) else {
                continue;
            };
            self.ddnames.insert((prog, raw.clone()));
            self.ddnames.insert((prog, dd.clone()));
            let loc = self.loc(whole.start(), whole.end());
            let fd = c[1].to_ascii_uppercase();
            let sym = dd_binding_symbol(&pgm, &dd);
            self.edge(
                from,
                sym.clone(),
                edge_tags::other("uses"),
                ResolutionTier::Parsed,
                loc.clone(),
                &[("via", "select-assign"), ("file", fd.as_str())],
            );
            let mut node = Node::new(
                sym,
                NodeKind::Other("dd_binding".to_string()),
                format!("{pgm}.{dd}"),
                Language::new("cobol"),
                loc,
            )
            .as_boundary(format!(
                "SELECT {fd} ASSIGN TO {dd}: no JCL DD {dd} in a step that runs {pgm} is indexed"
            ));
            node.metadata
                .insert("program".into(), serde_json::Value::String(pgm.clone()));
            node.metadata
                .insert("ddname".into(), serde_json::Value::String(dd.clone()));
            node.metadata
                .insert("file".into(), serde_json::Value::String(fd));
            self.out.nodes.push(node);
        }
    }

    // ── dynamic CALL ───────────────────────────────────────────────────────────────────────
    /// Rewrite the tree-sitter `Calls VAR` ref of each `CALL VAR` SITE (matched by byte offset,
    /// so `CALL 'VAR'` and `PERFORM VAR` are never touched). One pass over the refs.
    fn dynamic_calls(&mut self, code: &str, ex: &mut Extraction) {
        let sites: HashMap<usize, String> = RE_DYN_CALL
            .captures_iter(code)
            .map(|c| {
                let m = c.get(1).unwrap();
                (m.start(), m.as_str().to_ascii_uppercase())
            })
            .collect();
        if sites.is_empty() {
            return;
        }
        for r in ex.refs.iter_mut() {
            if r.kind != EdgeKind::Calls {
                continue;
            }
            let at = r.location.span.start_byte as usize;
            let Some(var) = sites.get(&at) else {
                continue;
            };
            if !r.raw_name.eq_ignore_ascii_case(var) {
                continue;
            }
            let prog = self.anchors.prog(at);
            r.raw_name = self.dynamic_ref_name(prog, var);
        }
    }

    fn finish(mut self, ex: &mut Extraction) {
        // Field metadata: why a dynamic call did or did not resolve (the variable's own program).
        for n in ex.nodes.iter_mut().filter(|n| n.kind == NodeKind::Field) {
            let prog = self.anchors.prog(n.location.span.start_byte as usize);
            if let Some(d) = self.decisions.get(&(prog, n.name.to_ascii_uppercase())) {
                let v = match d {
                    Decision::Resolved { value, evidence } => serde_json::json!({
                        "status": "resolved", "value": value, "evidence": evidence,
                        "tier": "heuristic",
                    }),
                    Decision::Unresolved { reason } => {
                        serde_json::json!({ "status": "unresolved", "reason": reason })
                    }
                };
                n.metadata.insert("dynamic_call".to_string(), v);
            }
        }
        // References are never to a DD name the program ASSIGNs. Procedure labels never become
        // References at all ([`tag_roles`]). Kept per site (ENGINE-CONTRACT §2.1).
        let mut refs = std::mem::take(&mut ex.refs);
        refs.append(&mut self.out.refs);
        refs.retain(|r| {
            if r.kind != EdgeKind::References {
                return true;
            }
            let key = (
                self.anchors.prog(r.location.span.start_byte as usize),
                r.raw_name.to_ascii_uppercase(),
            );
            !self.ddnames.contains(&key)
        });
        // Ownership: a data item or paragraph records its program (the innermost) or its
        // copybook; a data use or label records its programs (innermost first) or its copybook,
        // and the copybooks its program (and every program that holds it) COPYs.
        let str_list = |v: Vec<String>| {
            serde_json::Value::Array(v.into_iter().map(serde_json::Value::String).collect())
        };
        for n in ex
            .nodes
            .iter_mut()
            .filter(|n| matches!(n.kind, NodeKind::Field | NodeKind::Function))
        {
            let owner = match &self.anchors.member {
                Some(m) => Some((COPYBOOK_KEY, m.clone())),
                None => self
                    .anchors
                    .chain(n.location.span.start_byte as usize)
                    .into_iter()
                    .next()
                    .map(|p| (PROGRAM_KEY, p)),
            };
            if let Some((key, name)) = owner {
                n.metadata
                    .insert(key.to_string(), serde_json::Value::String(name));
            }
        }
        for r in &mut refs {
            let role = r.hints.get(ROLE_KEY).and_then(|v| v.as_str());
            if !matches!(role, Some(ROLE_DATA | ROLE_LABEL)) {
                continue;
            }
            let at = r.location.span.start_byte as usize;
            match &self.anchors.member {
                Some(m) => hint(r, COPYBOOK_KEY, m.clone().into()),
                None => {
                    let chain = self.anchors.chain(at);
                    if !chain.is_empty() {
                        hint(r, PROGRAMS_KEY, str_list(chain));
                    }
                }
            }
            let holders: Vec<&SymbolId> = match self
                .anchors
                .modules
                .iter()
                .filter(|(_, _, s, e)| *s <= at && at < *e)
                .map(|(sym, ..)| sym)
                .collect::<Vec<_>>()
            {
                v if v.is_empty() => vec![&self.anchors.fallback],
                v => v,
            };
            let copies: BTreeSet<&String> = holders
                .into_iter()
                .filter_map(|h| self.copies.get(h))
                .flatten()
                .collect();
            if !copies.is_empty() {
                let list = copies
                    .into_iter()
                    .map(|c| serde_json::Value::String(c.clone()))
                    .collect();
                hint(r, COPIES_KEY, serde_json::Value::Array(list));
            }
        }
        ex.refs = refs;
        // Shared-id records: a definition beats a boundary; never shadow a tree-sitter node.
        let existing: HashSet<SymbolId> = ex.nodes.iter().map(|n| n.symbol.clone()).collect();
        let mut mine = std::mem::take(&mut self.out.nodes);
        mine.retain(|n| !existing.contains(&n.symbol));
        crate::contrib::settle(&mut mine);
        ex.nodes.extend(mine);
        ex.local_edges.append(&mut self.out.edges);
    }
}

/// CICS options of the command body `code[start..end]`: `NAME(operand)` with balanced
/// parentheses, so `PROGRAM(PGMS(1))` is the option PROGRAM naming the item PGMS. A literal
/// operand's value is read from `clean`.
fn cics_options(code: &str, clean: &str, start: usize, end: usize) -> HashMap<String, Operand> {
    let bytes = code.as_bytes();
    let mut out = HashMap::new();
    let mut pos = start;
    while pos < end {
        let Some(m) = RE_CICS_OPT_HEAD.captures(&code[pos..end]) else {
            break;
        };
        let head = m.get(0).unwrap();
        let open = pos + head.end(); // just after '('
        let mut depth = 1usize;
        let mut close = open;
        while close < end {
            match bytes[close] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            close += 1;
        }
        let inner_code = code[open..close].trim();
        let inner_clean = clean[open..close].trim();
        let literal = inner_code.starts_with(['\'', '"']) && inner_clean.len() >= 2;
        let value = if literal {
            let q = &inner_clean[..1];
            let body = inner_clean
                .strip_prefix(q)
                .and_then(|s| s.strip_suffix(q))
                .unwrap_or(&inner_clean[1..]);
            body.replace(&q.repeat(2), q)
        } else {
            // The data item a bare operand names: `RECS(1)` → RECS.
            inner_code
                .split(|c: char| c.is_whitespace() || c == '(')
                .next()
                .unwrap_or("")
                .to_string()
        };
        out.entry(m[1].to_ascii_uppercase())
            .or_insert(Operand { value, literal });
        pos = close + 1;
    }
    out
}

/// The shared id a COBOL `ASSIGN TO <dd>` and a JCL `//<dd> DD` in a step running `<pgm>` both mint.
pub fn dd_binding_symbol(pgm: &str, dd: &str) -> SymbolId {
    Symbol::synthetic(
        "ddname",
        format!("{}.{}", pgm.to_ascii_uppercase(), dd.to_ascii_uppercase()),
    )
    .id()
}

fn is_table_name(t: &str) -> bool {
    let b = t.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|&c| is_ident_byte(c) && c != b'-')
        && !SQL_STOP.contains(&t.to_ascii_uppercase().as_str())
}

/// Restore ordinary SQL delimited identifiers from the same offsets in `clean`. Single-quoted
/// literals stay masked; unsupported delimited names keep their quotes and masked contents.
fn sql_identifiers(code: &str, clean: &str) -> String {
    let mut body = String::with_capacity(code.len());
    let mut from = 0;
    while let Some(open) = code[from..].find('"').map(|i| from + i) {
        body.push_str(&code[from..open]);
        let Some(close) = code[open + 1..].find('"').map(|i| open + 1 + i) else {
            body.push_str(&code[open..]);
            return body;
        };
        let name = &clean[open + 1..close];
        if !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'#' | b'@'))
        {
            body.push_str(name);
        } else {
            body.push_str(&code[open..=close]);
        }
        from = close + 1;
    }
    body.push_str(&code[from..]);
    body
}

/// For each byte of `body`, the offset of the innermost unclosed `(` before it (one forward pass).
fn paren_context(body: &str) -> Vec<Option<usize>> {
    let mut stack: Vec<usize> = Vec::new();
    let mut ctx = Vec::with_capacity(body.len() + 1);
    for (i, b) in body.bytes().enumerate() {
        ctx.push(stack.last().copied());
        match b {
            b'(' => stack.push(i),
            b')' => {
                stack.pop();
            }
            _ => {}
        }
    }
    ctx.push(stack.last().copied());
    ctx
}

/// `FROM` at statement level, or inside a parenthesised subselect — not `EXTRACT(… FROM x)` or
/// `TRIM(LEADING ' ' FROM x)`.
fn statement_level_or_subselect(body: &str, ctx: &[Option<usize>], at: usize) -> bool {
    match ctx.get(at).copied().flatten() {
        None => true,
        Some(open) => {
            let before = body[..open].trim_end().to_ascii_uppercase();
            body[open + 1..]
                .trim_start()
                .get(..6)
                .is_some_and(|w| w.eq_ignore_ascii_case("SELECT"))
                // A parenthesised joined table: `FROM (T1 JOIN T2 ON …)`.
                || before.ends_with("FROM")
                || before.ends_with("JOIN")
                || before.ends_with(',')
        }
    }
}

/// `T1 [AS] [alias], (subselect) [AS] alias, T2 …` after a FROM/JOIN/USING, up to the first
/// clause word. Derived tables are skipped (their own FROM is matched separately) and the list
/// resumes after them.
fn table_list(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = rest.as_bytes();
    let skip_ws = |mut i: usize| {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    };
    let mut i = 0;
    loop {
        i = skip_ws(i);
        if i < bytes.len() && bytes[i] == b'(' {
            // A derived table `(SELECT …)` is skipped (its FROM is matched on its own); a joined
            // table group `(T1 JOIN T2 …)` contributes its leading table list.
            let inner = rest[i + 1..].trim_start();
            if !inner
                .get(..6)
                .is_some_and(|w| w.eq_ignore_ascii_case("SELECT"))
            {
                out.extend(table_list(inner));
            }
            let mut depth = 0usize;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        } else {
            let start = i;
            while i < bytes.len() && is_ident_byte(bytes[i]) {
                i += 1;
            }
            let name = rest[start..i].trim_end_matches('.').to_ascii_uppercase();
            // A remaining quote is a masked, unsupported identifier part. Do not report its
            // preceding schema as a table (e.g. APP."MY TABLE").
            if !is_table_name(&name) || bytes.get(i) == Some(&b'"') {
                break;
            }
            out.push(name);
        }
        // optional [AS] alias
        let mut j = i;
        for _ in 0..2 {
            j = skip_ws(j);
            let s = j;
            while j < bytes.len() && is_ident_byte(bytes[j]) {
                j += 1;
            }
            let w = rest[s..j].to_ascii_uppercase();
            if w == "AS" {
                continue;
            }
            if w.is_empty() || SQL_STOP.contains(&w.as_str()) {
                j = s;
            }
            break;
        }
        i = skip_ws(j);
        if i < bytes.len() && bytes[i] == b',' {
            i += 1;
            continue;
        }
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::Extractor;

    fn run(path: &str, text: &str) -> Extraction {
        let sf = SourceFile {
            path: path.to_string(),
            language: Language::new("cobol"),
            text: text.to_string(),
        };
        let mut ex = crate::treesitter::TreeSitterExtractor::for_language("cobol")
            .expect("cobol grammar")
            .extract(&sf)
            .expect("parse");
        supplement(&sf, &mut ex);
        ex
    }

    fn paragraph_sym(ex: &Extraction, name: &str) -> SymbolId {
        ex.nodes
            .iter()
            .find(|n| n.kind == NodeKind::Function && n.name == name)
            .unwrap_or_else(|| panic!("paragraph {name}: {:?}", names(ex)))
            .symbol
            .clone()
    }

    fn module_sym(ex: &Extraction, name: &str) -> SymbolId {
        ex.nodes
            .iter()
            .find(|n| n.kind == NodeKind::Module && n.name == name)
            .unwrap_or_else(|| panic!("module {name}: {:?}", names(ex)))
            .symbol
            .clone()
    }

    fn names(ex: &Extraction) -> Vec<(String, String)> {
        ex.nodes
            .iter()
            .map(|n| (format!("{:?}", n.kind), n.name.clone()))
            .collect()
    }

    fn edge_to(ex: &Extraction, kind: &EdgeKind, target: &SymbolId) -> Option<Edge> {
        ex.local_edges
            .iter()
            .find(|e| &e.kind == kind && &e.target == target)
            .cloned()
    }

    fn calls(ex: &Extraction) -> Vec<String> {
        ex.refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| r.raw_name.clone())
            .collect()
    }

    /// A one-program source with `data` as WORKING-STORAGE and `body` after MAIN-PARA.
    fn program(data: &str, body: &str) -> String {
        format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. P.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n{data}       PROCEDURE DIVISION.\n       MAIN-PARA.\n{body}"
        )
    }

    pub(crate) const ORDPGM: &str = "       IDENTIFICATION DIVISION.
       PROGRAM-ID. ORDPGM.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       COPY ORDREC.
       01 WS-PGM PIC X(8) VALUE 'PRICEPGM'.
       PROCEDURE DIVISION.
       MAIN-PARA.
           MOVE 1 TO ORD-ID.
           PERFORM CALC-PARA.
           CALL 'TAXPGM' USING ORD-REC.
           EXEC CICS LINK PROGRAM('SHIPPGM') COMMAREA(ORD-REC)
           END-EXEC.
           EXEC SQL SELECT QTY INTO :ORD-QTY FROM ORDERS
                WHERE ID = :ORD-ID END-EXEC.
           STOP RUN.
       CALC-PARA.
           CALL WS-PGM USING ORD-REC.
";

    #[test]
    fn every_source_is_a_real_node_not_a_file_anchor() {
        // Fails on main: refs/edges came from `synthetic("cobol-pgm", path)`, never a node.
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        let pgm = module_sym(&ex, "ORDPGM");
        let link: Vec<_> = ex.refs.iter().filter(|r| r.raw_name == "SHIPPGM").collect();
        // A call starts from the paragraph that makes it, only (one site, one caller); a table
        // read from the program.
        let sources: BTreeSet<_> = link.iter().map(|r| r.from.clone()).collect();
        assert_eq!(sources, BTreeSet::from([paragraph_sym(&ex, "MAIN-PARA")]));
        assert!(link.iter().all(|r| r.kind == EdgeKind::Calls));
        let reads = edge_to(
            &ex,
            &edge_tags::other(edge_tags::READS),
            &Symbol::synthetic("db2", "ORDERS").id(),
        )
        .expect("reads ORDERS");
        assert_eq!(reads.source, pgm);
        let anchor = Symbol::synthetic("cobol-pgm", "cbl/ORDPGM.cbl").id();
        assert!(ex.refs.iter().all(|r| r.from != anchor));
        assert!(ex.local_edges.iter().all(|e| e.source != anchor));
    }

    #[test]
    fn no_cics_program_homonym_node() {
        // Fails on main: a `cics_program` node SHIPPGM made the name ambiguous with the real one.
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Other("cics_program".into()))
        );
    }

    #[test]
    fn copy_links_program_to_shared_copybook_id_and_copybook_defines_it() {
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        let pgm = module_sym(&ex, "ORDPGM");
        let cb = copybook_symbol("ORDREC");
        let e = edge_to(&ex, &EdgeKind::Imports, &cb).expect("program imports ORDREC");
        assert_eq!(e.source, pgm);
        let decl = ex
            .nodes
            .iter()
            .find(|n| n.symbol == cb)
            .expect("boundary record");
        assert!(decl.is_declaration());
        assert!(
            decl.metadata
                .contains_key(wicked_estate_core::node::BOUNDARY_REASON_METADATA_KEY)
        );

        let cpy = run(
            "cpy/ORDREC.cpy",
            "       01 ORD-REC.\n          05 ORD-ID PIC 9(8).\n          05 ORD-QTY PIC 9(4).\n",
        );
        let def = cpy
            .nodes
            .iter()
            .find(|n| n.symbol == cb)
            .expect("definition record");
        assert!(!def.is_declaration());
        assert_eq!(def.name, "ORDREC");
        let ord_id = cpy
            .nodes
            .iter()
            .find(|n| n.name == "ORD-ID")
            .unwrap()
            .symbol
            .clone();
        assert!(
            cpy.local_edges
                .iter()
                .any(|e| e.kind == EdgeKind::Contains && e.source == cb && e.target == ord_id)
        );
    }

    #[test]
    fn field_uses_become_reference_refs_but_not_paragraph_labels() {
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        let para = paragraph_sym(&ex, "MAIN-PARA");
        let refs: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::References)
            .map(|r| r.raw_name.as_str())
            .collect();
        for want in ["ORD-ID", "ORD-REC", "ORD-QTY"] {
            assert!(refs.contains(&want), "missing References {want}: {refs:?}");
        }
        assert!(
            !refs.contains(&"CALC-PARA"),
            "a PERFORM label is not a data use: {refs:?}"
        );
        assert!(
            ex.refs
                .iter()
                .filter(|r| r.kind == EdgeKind::References && r.raw_name == "ORD-ID")
                .all(|r| r.from == para),
            "a data use starts from the paragraph that makes it (the MOVE and the SQL host var)"
        );
    }

    #[test]
    fn lowercase_sql_host_variable_reference_is_folded() {
        let text = program(
            "       01 ws-price pic 9.\n",
            "           exec sql select a into :ws-price from t1 end-exec.\n",
        )
        .to_ascii_lowercase();
        let ex = run("cbl/P.cbl", &text);
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Field && n.name == "WS-PRICE")
        );
        let refs: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::References)
            .map(|r| r.raw_name.as_str())
            .collect();
        assert!(refs.contains(&"WS-PRICE"), "{refs:?}");
        assert!(!refs.contains(&"ws-price"), "{refs:?}");
    }

    #[test]
    fn redefines_and_occurs_depending_are_references() {
        // The generic qualified-word capture replaced the explicit REDEFINES/OCCURS patterns.
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 CNT PIC 9(4).\n       01 A PIC X(10).\n       01 B REDEFINES A PIC X(10).\n       01 T.\n          05 E PIC X OCCURS 1 TO 9 DEPENDING ON CNT.\n",
                "           GOBACK.\n",
            ),
        );
        let refs: BTreeSet<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::References)
            .map(|r| r.raw_name.as_str())
            .collect();
        assert!(refs.contains("A") && refs.contains("CNT"), "{refs:?}");
    }

    #[test]
    fn dynamic_call_single_value_rewrites_to_prefixed_ref() {
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        let c = calls(&ex);
        assert!(
            c.contains(&"cobol-dynamic-call:PRICEPGM".to_string()),
            "{c:?}"
        );
        assert!(!c.contains(&"WS-PGM".to_string()));
        let f = ex.nodes.iter().find(|n| n.name == "WS-PGM").unwrap();
        assert_eq!(f.metadata["dynamic_call"]["status"], "resolved");
        assert_eq!(f.metadata["dynamic_call"]["value"], "PRICEPGM");
    }

    #[test]
    fn set_entry_assigns_every_receiver() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 P1 USAGE PROGRAM-POINTER.\n       01 P2 USAGE PROGRAM-POINTER.\n",
                "           SET P1 P2 TO ENTRY 'PRICEPGM'.\n           CALL P2.\n",
            ),
        );
        let c = calls(&ex);
        assert!(
            c.contains(&format!("{COBOL_DYNAMIC_CALL_PREFIX}PRICEPGM")),
            "{c:?}"
        );
    }

    #[test]
    fn set_entry_receivers_start_after_last_set() {
        let text = program(
            "       01 WS-FLAG PIC X(8).\n       01 P1 USAGE PROGRAM-POINTER.\n",
            "           SET WS-FLAG TO TRUE\n           SET P1 TO ENTRY 'PRICEPGM'\n           CALL WS-FLAG\n           CALL P1\n",
        );
        let ex = run("cbl/P.cbl", &text);
        let c = calls(&ex);
        assert!(
            c.contains(&"cobol-dynamic-call:PRICEPGM".to_string()),
            "{c:?}"
        );
        assert!(
            c.contains(&"cobol-dynamic-var:WS-FLAG".to_string()),
            "{c:?}"
        );
        let flag_call = text.find("CALL WS-FLAG").unwrap();
        let flag_names: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| {
                r.kind == EdgeKind::Calls
                    && (flag_call..flag_call + "CALL WS-FLAG".len())
                        .contains(&(r.location.span.start_byte as usize))
            })
            .map(|r| r.raw_name.as_str())
            .collect();
        // From MAIN-PARA only: one call site, one caller.
        assert_eq!(flag_names, vec!["cobol-dynamic-var:WS-FLAG"]);
    }

    #[test]
    fn move_receivers_skip_spaced_and_nested_parentheses() {
        for receiver in ["WS-A(I + 1)", "WS-A((I + 1) * 2)", "WS-A(I : 1 + J)"] {
            let ex = run(
                "cbl/P.cbl",
                &program(
                    "       01 WS-A PIC X(8) OCCURS 10 TIMES.\n       01 WS-PGM PIC X(8).\n",
                    &format!(
                        "           MOVE 'PRICEPGM' TO {receiver} WS-PGM.\n           CALL WS-PGM.\n"
                    ),
                ),
            );
            let c = calls(&ex);
            assert!(
                c.contains(&format!("{COBOL_DYNAMIC_CALL_PREFIX}PRICEPGM")),
                "{receiver}: {c:?}"
            );
        }
    }

    fn lit(data: &str, body: &str) -> Literals {
        let text = program(data, body);
        let (clean, code) = lexical_views(&text, false);
        Literals::build(&code, &clean, 0, &LineIndex::new(&code))
    }

    #[test]
    fn literal_map_decisions() {
        let l = lit(
            "       01 A PIC X(8) VALUE 'ONE'.\n       01 B PIC X(8) VALUE 'ONE'.\n       01 C PIC X(8).\n       01 D PIC X(8) VALUE SPACES.\n       01 E PIC X(8).\n       01 H PIC X(8) VALUE 'ONE'.\n",
            "           MOVE 'TWO' TO B.\n           MOVE 'SOLO' TO C D.\n           MOVE WS-X TO E.\n           SET F TO ENTRY 'PTRPGM'.\n           MOVE SPACES TO H.\n",
        );
        let value = |v: &str| match l.decide(v) {
            Decision::Resolved { value, .. } => value,
            Decision::Unresolved { reason } => format!("UNRESOLVED: {reason}"),
        };
        assert_eq!(value("A"), "ONE");
        assert!(value("B").contains("2 literal values"));
        assert_eq!(value("C"), "SOLO");
        assert_eq!(value("D"), "SOLO", "VALUE SPACES is only the initial state");
        assert!(value("E").contains("non-literal"));
        assert_eq!(value("F"), "PTRPGM");
        assert!(value("G").contains("no literal"));
        assert!(
            value("H").contains("non-literal"),
            "MOVE SPACES changes the variable"
        );
    }

    #[test]
    fn move_targets_stop_at_the_next_statement() {
        // Fails before the fold: CANCEL was not a stop word, so WS-PGM looked assigned 'AAA'.
        let l = lit(
            "       01 WS-PGM PIC X(8) VALUE 'BBB'.\n",
            "           MOVE 'AAA' TO WS-STATUS\n           CANCEL WS-PGM\n           CALL WS-PGM.\n",
        );
        assert!(
            matches!(l.decide("WS-PGM"), Decision::Resolved { ref value, .. } if value == "BBB")
        );
    }

    #[test]
    fn ambiguous_dynamic_call_parks_under_the_variable_with_reason() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 WS-PGM PIC X(8) VALUE 'AAA'.\n",
                "           MOVE 'BBB' TO WS-PGM.\n           CALL WS-PGM.\n           EXEC CICS XCTL PROGRAM(WS-PGM) END-EXEC.\n",
            ),
        );
        let c = calls(&ex);
        assert!(
            !c.iter().any(|r| r.starts_with(COBOL_DYNAMIC_CALL_PREFIX)),
            "{c:?}"
        );
        assert_eq!(
            c.iter()
                .filter(|r| *r == "cobol-dynamic-var:WS-PGM")
                .count(),
            2,
            "the CALL and the XCTL, each from MAIN-PARA only: {c:?}"
        );
        assert_eq!(c.len(), 2, "{c:?}");
        let f = ex.nodes.iter().find(|n| n.name == "WS-PGM").unwrap();
        assert_eq!(f.metadata["dynamic_call"]["status"], "unresolved");
        assert!(
            f.metadata["dynamic_call"]["reason"]
                .as_str()
                .unwrap()
                .contains("AAA, BBB")
        );
    }

    #[test]
    fn literal_call_with_a_variable_spelling_is_not_rewritten() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 WS-PGM PIC X(8) VALUE 'AAA'.\n",
                "           CALL 'WS-PGM'.\n           CALL WS-PGM.\n",
            ),
        );
        let c = calls(&ex);
        assert!(
            c.contains(&"WS-PGM".to_string()),
            "the literal CALL keeps its name: {c:?}"
        );
        assert!(c.contains(&"cobol-dynamic-call:AAA".to_string()), "{c:?}");
    }

    #[test]
    fn literal_inference_is_scoped_to_its_own_program() {
        let text = "       IDENTIFICATION DIVISION.
       PROGRAM-ID. A.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       01 WS-PGM PIC X(8) VALUE 'AAA'.
       PROCEDURE DIVISION.
       MAIN-PARA.
           CALL WS-PGM.
       END PROGRAM A.
       IDENTIFICATION DIVISION.
       PROGRAM-ID. B.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       01 WS-PGM PIC X(8) VALUE 'BBB'.
       PROCEDURE DIVISION.
       MAIN-PARA.
           CALL WS-PGM.
       END PROGRAM B.
";
        let ex = run("cbl/AB.cbl", text);
        let modules = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Module)
            .count();
        assert_eq!(
            modules,
            2,
            "the grammar splits the two programs: {:?}",
            names(&ex)
        );
        let pairs: BTreeSet<(String, String)> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| {
                // Each call starts from its paragraph (or its program, before any paragraph); the
                // pair names the program holding it.
                let from = ex.nodes.iter().find(|n| n.symbol == r.from).unwrap();
                assert!(
                    matches!(from.kind, NodeKind::Function | NodeKind::Module),
                    "{}",
                    from.name
                );
                let at = r.location.span.start_byte;
                let prog = ex
                    .nodes
                    .iter()
                    .filter(|n| n.kind == NodeKind::Module)
                    .filter(|n| n.location.span.start_byte <= at && at < n.location.span.end_byte)
                    .min_by_key(|n| n.location.span.end_byte - n.location.span.start_byte)
                    .unwrap();
                (prog.name.clone(), r.raw_name.clone())
            })
            .collect();
        assert!(
            pairs.contains(&("A".into(), "cobol-dynamic-call:AAA".into())),
            "{pairs:?}"
        );
        assert!(
            pairs.contains(&("B".into(), "cobol-dynamic-call:BBB".into())),
            "{pairs:?}"
        );
        assert!(
            !pairs.contains(&("A".into(), "cobol-dynamic-call:BBB".into())),
            "{pairs:?}"
        );
    }

    #[test]
    fn review_round_two_lexical_and_sql_cases() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 WS-PGM PIC X(8) VALUE 'PRICEPGM'.\n",
                "      * >>SOURCE FORMAT FREE\n           MOVE ALL 'X' TO WS-PGM.\n           CALL WS-PGM.\n           COPY REAL REPLACING == COPY GHOST. == BY == X ==.\n           EXEC SQL SELECT :WS-FROM MYCOL FROM REALT END-EXEC.\n           EXEC SQL SELECT * FROM (T1 INNER JOIN T2 ON T1.ID = T2.ID)\n                END-EXEC.\n           EXEC SQL SELECT ID INTO :WS-ID FROM FINAL TABLE\n                (INSERT INTO ORDERS(QTY) VALUES(:WS-QTY)) END-EXEC.\n",
            ),
        );
        let c = calls(&ex);
        assert!(
            c.contains(&"cobol-dynamic-var:WS-PGM".to_string()),
            "MOVE ALL 'X' changes WS-PGM: {c:?}"
        );
        let l = lit(
            "       01 A PIC X(8) VALUE 'AAA'.\n       01 WS-MOVE PIC X.\n       01 B PIC X(8).\n",
            "           MOVE 1.5 TO A.\n           DISPLAY WS-MOVE\n           MOVE 'BBB' TO B\n",
        );
        assert!(
            matches!(l.decide("A"), Decision::Unresolved { .. }),
            "a decimal MOVE changes A"
        );
        assert!(
            matches!(l.decide("B"), Decision::Resolved { ref value, .. } if value == "BBB"),
            "WS-MOVE is not a MOVE statement"
        );
        let sql_in_pseudo = run(
            "cbl/Q.cbl",
            &program(
                "",
                "           COPY REAL2 REPLACING\n             == EXEC SQL SELECT * FROM GHOSTT END-EXEC ==\n             BY == X ==.\n",
            ),
        );
        assert!(!sql_in_pseudo.nodes.iter().any(|n| n.name == "GHOSTT"));
        let targets: BTreeSet<(String, String)> = ex
            .local_edges
            .iter()
            .filter_map(|e| {
                ex.nodes
                    .iter()
                    .find(|n| n.symbol == e.target)
                    .map(|n| (n.name.clone(), format!("{:?}", e.kind)))
            })
            .collect();
        let names: BTreeSet<&str> = targets.iter().map(|(n, _)| n.as_str()).collect();
        for want in ["REAL", "REALT", "T1", "T2", "ORDERS"] {
            assert!(names.contains(want), "{want}: {targets:?}");
        }
        for bogus in ["GHOST", "MYCOL", "FINAL"] {
            assert!(!names.contains(bogus), "{bogus}: {targets:?}");
        }
        let w = format!("{:?}", edge_tags::other(edge_tags::WRITES));
        assert!(targets.contains(&("ORDERS".to_string(), w)), "{targets:?}");
    }

    #[test]
    fn literals_and_sequence_areas_are_not_code() {
        // Fails before the fold: COPY / EXEC SQL inside literals minted edges, and the sequence
        // area made `000200` the copybook name.
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 WS-MSG PIC X(30) VALUE 'FILE COPY DONE *> X'.\n",
                "           DISPLAY 'EXEC SQL SELECT X FROM GHOST END-EXEC'.\n           EXEC SQL SELECT 'FROM FAKE' FROM ORDERS WHERE A = '--' END-EXEC.\n000100     COPY\n000200         ORDREC.\n",
            ),
        );
        let targets: BTreeSet<String> = ex
            .local_edges
            .iter()
            .filter_map(|e| ex.nodes.iter().find(|n| n.symbol == e.target))
            .map(|n| n.name.clone())
            .collect();
        for bogus in ["DONE", "GHOST", "FAKE", "000200"] {
            assert!(
                !targets.contains(bogus),
                "{bogus} is not a link: {targets:?}"
            );
        }
        assert!(
            targets.contains("ORDERS") && targets.contains("ORDREC"),
            "{targets:?}"
        );
    }

    #[test]
    fn sql_read_write_classification() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "",
                "           EXEC SQL INSERT INTO AUDIT (A) SELECT X FROM SRC END-EXEC.
           EXEC SQL UPDATE ACCT SET BAL = 0 WHERE ID IN
                (SELECT ID FROM CLOSED) END-EXEC.
           EXEC SQL DELETE FROM LOG WHERE TS < CURRENT DATE END-EXEC.
           EXEC SQL DECLARE C1 CURSOR FOR SELECT A.X, B.Y FROM T1 A, T2 B
                WHERE EXTRACT(YEAR FROM A.D) = 2020 FOR UPDATE OF X END-EXEC.
           EXEC SQL FETCH FROM C1 INTO :H END-EXEC.
           EXEC SQL SELECT QTY INTO :Q FROM ORDERS END-EXEC.
           EXEC SQL WITH RECENT AS (SELECT * FROM HIST) SELECT * FROM RECENT END-EXEC.
           EXEC SQL SELECT * FROM (SELECT * FROM INNERT) X, AFTERT END-EXEC.
           EXEC SQL EXECUTE S USING DESCRIPTOR :D END-EXEC.
           EXEC SQL MERGE INTO TGT USING SRC2 ON 1 = 1 END-EXEC.
",
            ),
        );
        let kinds = |t: &str| -> BTreeSet<String> {
            let sym = Symbol::synthetic("db2", t).id();
            ex.local_edges
                .iter()
                .filter(|e| e.target == sym)
                .map(|e| format!("{:?}", e.kind))
                .collect()
        };
        let w = format!("{:?}", edge_tags::other(edge_tags::WRITES));
        let r = format!("{:?}", edge_tags::other(edge_tags::READS));
        for (t, k) in [
            ("AUDIT", &w),
            ("SRC", &r),
            ("ACCT", &w),
            ("CLOSED", &r),
            ("LOG", &w),
            ("T1", &r),
            ("T2", &r),
            ("ORDERS", &r),
            ("HIST", &r),
            ("INNERT", &r),
            ("AFTERT", &r),
            ("TGT", &w),
            ("SRC2", &r),
        ] {
            assert_eq!(kinds(t), BTreeSet::from([k.clone()]), "{t}");
        }
        for bogus in [
            "C1",
            "A.D",
            "Q",
            "H",
            "ID",
            "OF",
            "RECENT",
            "DESCRIPTOR",
            "X",
        ] {
            assert!(kinds(bogus).is_empty(), "{bogus} is not a table");
        }
    }

    #[test]
    fn sql_line_comment_does_not_end_exec_block() {
        let text = program(
            "",
            "           EXEC SQL SELECT A INTO :H FROM T1 -- END-EXEC here\n                WHERE X IN (SELECT B FROM T2) END-EXEC.\n",
        );
        let ex = run("cbl/P.cbl", &text);
        let (_, code) = lexical_views(&text, false);
        assert_eq!(RE_EXEC_SQL.captures_iter(&code).count(), 1);
        let body = RE_EXEC_SQL.captures(&code).unwrap();
        assert!(
            body[1].contains("FROM T2"),
            "one block includes the subselect"
        );
        for table in ["T1", "T2"] {
            assert!(
                edge_to(
                    &ex,
                    &edge_tags::other(edge_tags::READS),
                    &Symbol::synthetic("db2", table).id(),
                )
                .is_some(),
                "missing read of {table}"
            );
        }
    }

    #[test]
    fn sql_block_comment_hides_cics_call() {
        for comment in [
            "/* EXEC CICS LINK PROGRAM('XPGM') END-EXEC */",
            "/* EXEC CICS LINK\n                PROGRAM('XPGM') END-EXEC */",
        ] {
            let ex = run(
                "cbl/P.cbl",
                &program(
                    "",
                    &format!("           EXEC SQL {comment} SELECT A INTO :H FROM T1 END-EXEC.\n"),
                ),
            );
            assert!(!calls(&ex).iter().any(|name| name == "XPGM"));
            assert!(
                edge_to(
                    &ex,
                    &edge_tags::other(edge_tags::READS),
                    &Symbol::synthetic("db2", "T1").id(),
                )
                .is_some()
            );
        }
    }

    #[test]
    fn sql_literal_comment_marker_keeps_table_read() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "",
                "           EXEC SQL SELECT A INTO :H FROM T1 WHERE C = '--' END-EXEC.\n",
            ),
        );
        assert!(
            edge_to(
                &ex,
                &edge_tags::other(edge_tags::READS),
                &Symbol::synthetic("db2", "T1").id(),
            )
            .is_some()
        );
    }

    #[test]
    fn sql_delimited_qualified_identifier_reads_table() {
        for comment in ["", "/* shift */ -- shift again\n                "] {
            let ex = run(
                "cbl/P.cbl",
                &program(
                    "",
                    &format!(
                        "           EXEC SQL {comment}SELECT C INTO :H FROM \"APP\".\"ORDERS\" END-EXEC.\n"
                    ),
                ),
            );
            assert!(
                edge_to(
                    &ex,
                    &edge_tags::other(edge_tags::READS),
                    &Symbol::synthetic("db2", "APP.ORDERS").id(),
                )
                .is_some()
            );
        }
    }

    #[test]
    fn sql_delimited_identifier_writes_table() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "",
                "           EXEC SQL INSERT INTO \"T1\" VALUES (1) END-EXEC.\n",
            ),
        );
        assert!(
            edge_to(
                &ex,
                &edge_tags::other(edge_tags::WRITES),
                &Symbol::synthetic("db2", "T1").id(),
            )
            .is_some()
        );
    }

    #[test]
    fn sql_nonordinary_delimited_identifier_stays_missing() {
        for table in [
            "\"MY TABLE\"",
            "\"APP\".\"MY TABLE\"",
            "\"MY TABLE\".\"T1\"",
        ] {
            let ex = run(
                "cbl/P.cbl",
                &program(
                    "",
                    &format!("           EXEC SQL SELECT C INTO :H FROM {table} END-EXEC.\n"),
                ),
            );
            assert!(
                !ex.nodes
                    .iter()
                    .any(|n| n.kind == NodeKind::Other("db2_table".to_string())),
                "unsupported table {table} must not produce a partial name"
            );
        }
    }

    #[test]
    fn sql_comments_preserve_lexical_offsets() {
        for newline in ["\n", "\r\n"] {
            let src = concat!(
                "000100 exec\n",
                "000200 sql SELECT '--', 'é/*' FROM T1-- END-EXEC\n",
                "000300 /* comment\n",
                "      * END-EXEC */ ignored card\n",
                "000400 EXEC CICS LINK PROGRAM('XPGM') END-EXEC */\n",
                "000500 WHERE C = '/*' END-EXEC.\n",
                "000600 DISPLAY '--outside'.\n",
            )
            .replace('\n', newline);
            let (clean, code) = lexical_views(&src, false);
            for view in [&clean, &code] {
                assert_eq!(view.len(), src.len());
                assert_eq!(
                    view.match_indices('\n').map(|(i, _)| i).collect::<Vec<_>>(),
                    src.match_indices('\n').map(|(i, _)| i).collect::<Vec<_>>()
                );
                assert!(!view.contains("comment") && !view.contains("XPGM"));
                assert!(view.contains("WHERE C ="));
            }
            assert!(clean.contains("'--'") && clean.contains("'é/*'"));
            assert!(clean.contains("'--outside'"));
        }
    }

    #[test]
    fn cics_maps_and_dclgen_and_include() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 WS-MAP PIC X(7) VALUE 'VARMAP'.\n       01 WS-SET PIC X(7).\n",
                "           EXEC SQL INCLUDE SQLCA END-EXEC.
           EXEC SQL INCLUDE DCLORD END-EXEC.
           EXEC CICS SEND MAP('ORDMAP') MAPSET('ORDSET') ERASE END-EXEC.
           EXEC CICS RECEIVE MAP('ORDMAP') MAPSET('ORDSET') INTO(WS-IN) END-EXEC.
           EXEC CICS SEND MAP('SOLO') END-EXEC.
           EXEC CICS SEND MAP(WS-MAP) MAPSET('VSET') END-EXEC.
           EXEC CICS SEND MAP('LOST') MAPSET(WS-SET) END-EXEC.
",
            ),
        );
        let map = Symbol::synthetic("cics-map", "ORDSET.ORDMAP").id();
        let writes = edge_tags::other(edge_tags::WRITES);
        let e = edge_to(&ex, &writes, &map).expect("SEND writes the map");
        assert_eq!(e.provenance, wicked_estate_core::Provenance::Parsed);
        assert!(edge_to(&ex, &edge_tags::other(edge_tags::READS), &map).is_some());
        assert!(
            edge_to(
                &ex,
                &writes,
                &Symbol::synthetic("cics-map", "SOLO.SOLO").id()
            )
            .is_some()
        );
        let inferred = edge_to(
            &ex,
            &writes,
            &Symbol::synthetic("cics-map", "VSET.VARMAP").id(),
        )
        .expect("a map named through a single-literal variable");
        assert_eq!(
            inferred.provenance,
            wicked_estate_core::Provenance::Heuristic
        );
        assert!(
            !ex.nodes.iter().any(|n| n.name == "LOST"),
            "an explicit MAPSET that cannot be read is not the default mapset"
        );
        assert!(edge_to(&ex, &EdgeKind::Imports, &copybook_symbol("DCLORD")).is_some());
        assert!(edge_to(&ex, &EdgeKind::Imports, &copybook_symbol("SQLCA")).is_none());
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::References && r.raw_name == "WS-IN")
        );

        let dcl = run(
            "cpy/DCLORD.cpy",
            "           EXEC SQL DECLARE ORDERS TABLE\n           ( ID INTEGER NOT NULL ) END-EXEC.\n",
        );
        let t = dcl
            .nodes
            .iter()
            .find(|n| n.symbol == Symbol::synthetic("db2", "ORDERS").id())
            .expect("table def");
        assert!(!t.is_declaration());
    }

    #[test]
    fn nested_cics_operands_keep_option_and_item() {
        // Fails before the fold: the option regex matched the inner `PGMS(1)` / `RECS(1)`.
        let ex = run(
            "cbl/P.cbl",
            &program(
                "",
                "           EXEC CICS LINK PROGRAM(PGMS(1)) COMMAREA(RECS(1)) END-EXEC.\n",
            ),
        );
        let c = calls(&ex);
        assert!(c.contains(&"cobol-dynamic-var:PGMS".to_string()), "{c:?}");
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::References && r.raw_name == "RECS")
        );
    }

    #[test]
    fn select_assign_links_program_to_dd_binding() {
        let ex = run(
            "cbl/PAYCALC.cbl",
            "       IDENTIFICATION DIVISION.
       PROGRAM-ID. PAYCALC.
       ENVIRONMENT DIVISION.
       INPUT-OUTPUT SECTION.
       FILE-CONTROL.
           SELECT PAY-FILE ASSIGN TO PAYIN.
           SELECT OUT-FILE ASSIGN TO UT-S-PAYOUT.
       PROCEDURE DIVISION.
       MAIN-PARA.
           GOBACK.
",
        );
        let pgm = module_sym(&ex, "PAYCALC");
        for dd in ["PAYIN", "PAYOUT"] {
            let e = edge_to(
                &ex,
                &edge_tags::other("uses"),
                &dd_binding_symbol("PAYCALC", dd),
            )
            .unwrap_or_else(|| panic!("uses {dd}"));
            assert_eq!(e.source, pgm);
        }
        assert!(
            !ex.refs.iter().any(|r| r.raw_name == "PAYIN"),
            "a DD name is not a data use"
        );
    }

    #[test]
    fn comment_lines_are_ignored() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "",
                "      *    EXEC SQL SELECT A FROM GHOST END-EXEC.\n      *    COPY GHOSTCPY.\n           GOBACK.\n",
            ),
        );
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.name == "GHOST" || n.name == "GHOSTCPY")
        );
    }

    /// #475: a Micro Focus free-format directive keeps columns 73-80 available to COPY.
    #[test]
    fn cobol_475_micro_focus_free_format_keeps_identification_columns_as_code() {
        for directive in [
            "$SET SOURCEFORMAT\"FREE\"",
            "$SET SOURCEFORMAT(FREE)",
            "$SET SOURCEFORMAT 'FREE'",
        ] {
            let body = format!("{:72}COPY C.\n", "");
            let text = format!("{directive}\n{}", program("", &body));
            let ex = run("cbl/P.cbl", &text);
            assert!(edge_to(&ex, &EdgeKind::Imports, &copybook_symbol("C")).is_some());
        }
    }

    /// #465 / #473: lexical views cut identification labels only on the existing card shape.
    #[test]
    fn cobol_465_lexical_views_identification_area_unchanged() {
        for label in ["PROG.CBL", "CBACT-01", "V1.2"] {
            let src = format!("{:<72}{label}\n", "           EXIT.");
            let (clean, code) = lexical_views(&src, false);
            assert_eq!(clean.len(), src.len());
            assert_eq!(code.len(), src.len());
            assert!(clean[72..].trim().is_empty(), "{clean:?}");
            assert!(code[72..].trim().is_empty(), "{code:?}");
        }
        for src in [
            format!("{:<72}MOVE A TO B.\n", "           EXIT."),
            format!("{:<71}.V1.2\n", "           EXIT"),
        ] {
            let (clean, code) = lexical_views(&src, false);
            assert_eq!(&clean[72..], &src[72..]);
            assert_eq!(&code[72..], &src[72..]);
        }
    }

    #[test]
    fn grammar_view_hides_a_copy_up_to_its_own_period_only() {
        let line = |l: &str| format!("       {l}\n");
        let head = "       PROCEDURE DIVISION.\n".to_string();
        // Terminated, through OF / SUPPRESS / REPLACING: up to its period.
        let text = head.clone()
            + &line("COPY CSUTLDPY OF SYSLIB SUPPRESS REPLACING ==A== BY ==B== X BY Y.")
            + &line("NEXT-PARA.");
        let view = grammar_view(&text).unwrap();
        assert_eq!(view.len(), text.len());
        assert!(view.contains("EXIT"));
        assert!(!view.contains("CSUTLDPY") && !view.contains("REPLACING"));
        assert!(view.contains("NEXT-PARA."));
        // No period of its own: only `COPY <member>`; the next header and statement keep theirs.
        let text = head.clone()
            + &line("COPY CSUTLDPY")
            + &line("AFTER-PARA.")
            + &line("    MOVE A TO B.");
        let view = grammar_view(&text).unwrap();
        assert!(
            view.contains("AFTER-PARA.") && view.contains("MOVE A TO B."),
            "{view}"
        );
        // Many unterminated COPYs: each hides only itself (linear, ranges disjoint).
        let mut text = head;
        for _ in 0..50 {
            text += &line("COPY BOOK");
            text += &line("    MOVE A TO B");
        }
        text += &line(".");
        let view = grammar_view(&text).unwrap();
        assert_eq!(view.matches("EXIT").count(), 50);
        assert_eq!(view.matches("MOVE A TO B").count(), 50);
        // Clauses out of order, or a repeated library, are not this COPY's: member only.
        let text = "       PROCEDURE DIVISION.\n".to_string()
            + &line(&format!("COPY BOOK{}", " OF".repeat(2000)))
            + &line("NEXT-PARA.");
        let view = grammar_view(&text).unwrap();
        assert_eq!(view.len(), text.len());
        assert!(view.contains("NEXT-PARA.") && view.contains(" OF OF"));
        assert_eq!(
            view.matches('\n').count(),
            text.matches('\n').count(),
            "line breaks are kept"
        );
    }

    #[test]
    fn lexical_views_keep_offsets_and_mask_literal_contents() {
        let src = "000100 MOVE 'A*>B' TO X. *> note\n      * comment line\n       DISPLAY \"IT\"\"S\" 'é'.\n";
        let (clean, code) = lexical_views(src, false);
        assert_eq!(clean.len(), src.len());
        assert_eq!(code.len(), src.len());
        assert!(
            code.contains("'~~~~'") && clean.contains("'A*>B'"),
            "{code:?}"
        );
        assert!(!clean.contains("note") && !clean.contains("comment") && !clean.contains("000100"));
        assert!(code.contains("\"~~~~~\""), "{code:?}");
    }

    // ── wicked-estate#545: ref roles ───────────────────────────────────────────────────────

    fn hint_str<'a>(r: &'a UnresolvedRef, key: &str) -> Option<&'a str> {
        r.hints.get(key).and_then(|v| v.as_str())
    }

    fn hint_list(r: &UnresolvedRef, key: &str) -> Vec<String> {
        r.hints
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn refs_named<'a>(ex: &'a Extraction, kind: EdgeKind, name: &str) -> Vec<&'a UnresolvedRef> {
        ex.refs
            .iter()
            .filter(|r| r.kind == kind && r.raw_name == name)
            .collect()
    }

    /// A file of the resolver's role fixtures (`wicked-estate-resolve/tests/fixtures/cobol_roles`).
    fn role_fixture(rel: &str) -> String {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wicked-estate-resolve/tests/fixtures/cobol_roles");
        std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    fn custpgm() -> Extraction {
        run("cbl/CUSTPGM.cbl", &role_fixture("cbl/CUSTPGM.cbl"))
    }

    /// The role fixtures' sources, in index order.
    const ROLE_FIXTURES: &[&str] = &[
        "cbl/ACCTPGM.cbl",
        "cbl/AMBPGM.cbl",
        "cbl/CUSTPGM.cbl",
        "cbl/DATEPGM.cbl",
        "cbl/FRAGPGM.cbl",
        "cbl/OTHREC.cpy",
        "cbl/OTHPGM.cbl",
        "cbl/RECUR.cbl",
        "cbl/RECUR2.cbl",
        "cbl/SELFPGM.cbl",
        "cbl/SHIPPGM.cbl",
        "cbl/TWOPGM.cbl",
        "cpy/ACCTFRAG.cpy",
        "cpy/ACCTREC.cpy",
        "cpy/CUSTFRAG.cpy",
        "cpy/CUSTREC.cpy",
        "cpy/DATEPROC.cpy",
        "cpy/DATEREC.cpy",
        "cpy/ORDFRAG.cpy",
        "cpy/ORDREC.cpy",
    ];

    /// The resolver's role tests read `extractions.json`, this extractor's own output for the
    /// fixture sources. Regenerate it with `UPDATE_COBOL_ROLE_FIXTURE=1`.
    #[test]
    fn role_fixture_extractions_are_this_extractors_output() {
        let all: Vec<Extraction> = ROLE_FIXTURES
            .iter()
            .map(|rel| run(rel, &role_fixture(rel)))
            .collect();
        let json = serde_json::to_string_pretty(&all).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wicked-estate-resolve/tests/fixtures/cobol_roles/extractions.json");
        if std::env::var_os("UPDATE_COBOL_ROLE_FIXTURE").is_some() {
            std::fs::write(&path, &json).unwrap();
        }
        let stored = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            stored == json,
            "extractions.json is stale: rerun with UPDATE_COBOL_ROLE_FIXTURE=1"
        );
    }

    /// Row 3: a qualified data use carries its qualifiers and its program's COPY list.
    #[test]
    fn row_3_qualified_data_uses_carry_qualifiers_and_copies() {
        let ex = custpgm();
        let ids = refs_named(&ex, EdgeKind::References, "ID");
        assert_eq!(ids.len(), 2, "{:?}", ex.refs);
        let mut quals: Vec<Vec<String>> =
            ids.iter().map(|r| hint_list(r, QUALIFIERS_KEY)).collect();
        quals.sort();
        assert_eq!(
            quals,
            [vec!["CUSTOMER".to_string()], vec!["ORDER-REC".to_string()]]
        );
        for r in ids {
            assert_eq!(hint_str(r, ROLE_KEY), Some(ROLE_DATA));
            assert_eq!(
                hint_list(r, COPIES_KEY),
                ["CUSTFRAG", "CUSTREC", "ORDFRAG", "ORDREC"]
            );
            assert_eq!(hint_list(r, PROGRAMS_KEY), ["CUSTPGM"]);
        }
        // A copybook item records its groups; a fragment (no 01 level) is open to its includer.
        let cpy = run(
            "cpy/CUSTREC.cpy",
            "       01 CUSTOMER.\n          05 CUST-KEY.\n             10 ID PIC 9(8).\n          05 NAME PIC X(20).\n",
        );
        let field = |ex: &Extraction, name: &str| {
            ex.nodes
                .iter()
                .find(|n| n.kind == NodeKind::Field && n.name == name)
                .unwrap_or_else(|| panic!("{name}: {:?}", names(ex)))
                .clone()
        };
        assert_eq!(
            field(&cpy, "ID").metadata[PARENTS_KEY],
            serde_json::json!(["CUST-KEY", "CUSTOMER"])
        );
        assert_eq!(
            field(&cpy, "NAME").metadata[PARENTS_KEY],
            serde_json::json!(["CUSTOMER"])
        );
        assert!(!field(&cpy, "ID").metadata.contains_key(OPEN_KEY));
        let frag = run("cpy/FRAG.cpy", "          05 ID PIC 9(8).\n");
        assert_eq!(
            field(&frag, "ID").metadata[OPEN_KEY],
            serde_json::json!(true)
        );
    }

    /// Rows 4 and 14: a data use spelled like a paragraph of its own program stays a data use
    /// (it was dropped with the label), and a procedure label never becomes a References ref.
    #[test]
    fn row_14_data_use_survives_a_same_named_paragraph_and_labels_are_never_references() {
        let ex = custpgm();
        let calc = refs_named(&ex, EdgeKind::References, "CALC");
        assert_eq!(calc.len(), 1, "the MOVE 3 TO CALC data use: {:?}", ex.refs);
        assert_eq!(hint_str(calc[0], ROLE_KEY), Some(ROLE_DATA));
        for label in ["CHECK-PARA", "CHECK-EXIT", "MAIN-PARA"] {
            assert!(
                refs_named(&ex, EdgeKind::References, label).is_empty(),
                "{label}"
            );
        }
        // A PERFORM or GO TO of a paragraph of its own division is a local call from the
        // paragraph that makes it (wicked-estate#579), bound to the paragraph, never a data use.
        let para = |name: &str| {
            ex.nodes
                .iter()
                .find(|n| n.kind == NodeKind::Function && n.name == name)
                .unwrap_or_else(|| panic!("{name}: {:?}", names(&ex)))
                .symbol
                .clone()
        };
        let main = para("MAIN-PARA");
        for label in ["CALC", "CHECK-PARA", "CHECK-EXIT"] {
            let target = para(label);
            assert!(
                ex.local_edges
                    .iter()
                    .any(|e| e.kind == EdgeKind::Calls && e.source == main && e.target == target),
                "PERFORM {label}: {:?}",
                ex.local_edges
            );
            assert!(
                refs_named(&ex, EdgeKind::Calls, label).is_empty(),
                "{label}"
            );
        }
        // Host variables and CICS operands are data uses too.
        let customer = refs_named(&ex, EdgeKind::References, "CUSTOMER");
        assert!(
            customer
                .iter()
                .any(|r| hint_str(r, ROLE_KEY) == Some(ROLE_DATA))
        );
    }

    /// Row 17: CALL 'X' and LINK PROGRAM('X') are program calls, whatever paragraph shares X.
    #[test]
    fn row_17_program_calls_are_tagged_program() {
        let ex = custpgm();
        let calls = refs_named(&ex, EdgeKind::Calls, "SHIPPGM");
        assert_eq!(calls.len(), 2, "{:?}", ex.refs);
        assert!(
            calls
                .iter()
                .all(|r| hint_str(r, ROLE_KEY) == Some(ROLE_PROGRAM))
        );
    }

    /// Level 66 belongs to its record, an 88 to the item before it (a 77 too).
    #[test]
    fn special_levels_record_their_owner() {
        let ex = run(
            "cpy/LEVELS.cpy",
            "       01 R1.\n          05 R2.\n             10 A PIC X.\n       66 ALIAS RENAMES R2.\n       77 STATUS-A PIC X.\n          88 READY VALUE 'Y'.\n",
        );
        let meta = |name: &str| {
            ex.nodes
                .iter()
                .find(|n| n.kind == NodeKind::Field && n.name == name)
                .unwrap_or_else(|| panic!("{name}: {:?}", names(&ex)))
                .metadata
                .clone()
        };
        assert_eq!(meta("A")[PARENTS_KEY], serde_json::json!(["R2", "R1"]));
        assert_eq!(meta("ALIAS")[PARENTS_KEY], serde_json::json!(["R1"]));
        assert_eq!(meta("READY")[PARENTS_KEY], serde_json::json!(["STATUS-A"]));
        assert!(!meta("READY").contains_key(OPEN_KEY));
        assert_eq!(meta("A")[COPYBOOK_KEY], serde_json::json!("LEVELS"));
    }

    /// `PERFORM P IN SECT` names the paragraph P, and data items and paragraphs record their
    /// program.
    #[test]
    fn section_qualified_perform_names_the_paragraph() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 W PIC 9.\n",
                "           PERFORM CHECK-PARA IN CHECK-SECT.\n           GOBACK.\n       CHECK-SECT SECTION.\n       CHECK-PARA.\n           EXIT.\n",
            ),
        );
        // The paragraph is in this division: a local call bound to CHECK-PARA (#579).
        let para = ex.nodes.iter().find(|n| n.name == "CHECK-PARA").unwrap();
        assert!(
            ex.local_edges
                .iter()
                .any(|e| e.kind == EdgeKind::Calls && e.target == para.symbol),
            "{:?}",
            ex.local_edges
        );
        assert_eq!(para.metadata[PROGRAM_KEY], serde_json::json!("P"));
    }

    /// A PERFORM or GO TO label that names no paragraph of its division stays a ref by name, with
    /// the label role and its program.
    #[test]
    fn an_unresolved_perform_or_go_to_label_is_a_label_ref() {
        let ex = run(
            "cbl/P.cbl",
            &program(
                "       01 W PIC 9.\n",
                "           PERFORM NO-SUCH-PARA IN CHECK-SECT\n           GO TO NOT-HERE.\n       CHECK-SECT SECTION.\n       CHECK-PARA.\n           EXIT.\n",
            ),
        );
        for name in ["NO-SUCH-PARA", "NOT-HERE"] {
            let r = refs_named(&ex, EdgeKind::Calls, name);
            assert_eq!(r.len(), 1, "{name}: {:?}", calls(&ex));
            assert_eq!(hint_str(r[0], ROLE_KEY), Some(ROLE_LABEL), "{name}");
            assert_eq!(hint_list(r[0], PROGRAMS_KEY), ["P"], "{name}");
            assert!(
                refs_named(&ex, EdgeKind::References, name).is_empty(),
                "{name}"
            );
        }
    }

    // ── wicked-estate#544: logical source ──────────────────────────────────────────────────

    fn at<'a>(text: &'a str, span: &Span) -> &'a str {
        &text[span.start_byte as usize..span.end_byte as usize]
    }

    /// Row 2: a free-format program written from column 1 gets its Module, and every span is the
    /// file's own bytes and lines (a line longer than a card is split, not cut).
    #[test]
    fn free_format_column_1_program_gets_its_module_with_exact_spans() {
        let long = format!("    MOVE 1 TO {} CALL 'TAXPGM'.\n", "W-COUNT ".repeat(9));
        let text = format!(
            ">>SOURCE FORMAT FREE\nIDENTIFICATION DIVISION.\nPROGRAM-ID. FREEPGM.\nDATA DIVISION.\n\
             WORKING-STORAGE SECTION.\n01 W-COUNT PIC 9(4).\n*> a comment\nPROCEDURE DIVISION.\n\
             MAIN-PARA.\n{long}    CALL 'SUBPGM'.\n    STOP RUN.\n"
        );
        let ex = run("cbl/FREEPGM.cbl", &text);
        let module = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Module && n.name == "FREEPGM")
            .unwrap_or_else(|| panic!("{:?}", names(&ex)));
        let s = &module.location.span;
        assert!(
            at(&text, s).starts_with("IDENTIFICATION DIVISION."),
            "{s:?}"
        );
        assert_eq!((s.start_line, s.start_col), (1, 0));
        // A call starts from the paragraph that makes it (wicked-estate#579).
        let main_para = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Function && n.name == "MAIN-PARA")
            .unwrap_or_else(|| panic!("{:?}", names(&ex)))
            .symbol
            .clone();
        for (name, line) in [("TAXPGM", 9), ("SUBPGM", 10)] {
            let r = refs_named(&ex, EdgeKind::Calls, name);
            assert_eq!(r.len(), 1, "{name}: {:?}", calls(&ex));
            assert_eq!(at(&text, &r[0].location.span), format!("'{name}'"));
            assert_eq!(r[0].location.span.start_line, line);
            assert_eq!(r[0].from, main_para);
        }
        let para = ex.nodes.iter().find(|n| n.name == "MAIN-PARA").unwrap();
        assert_eq!(para.location.span.start_line, 8);
        assert_eq!(para.location.span.start_col, 0);
        assert!(!refs_named(&ex, EdgeKind::References, "W-COUNT").is_empty());
    }

    /// Row 11: a literal continued on a `-` card is read whole, by the grammar (CALL) and by the
    /// text pass (LINK), and the ref spans the file's own bytes across both cards.
    #[test]
    fn continued_literal_program_names_join() {
        let card = |code: &str| format!("{code:>72}\n");
        let text = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. ORDPGM.\n       PROCEDURE DIVISION.\n       MAIN-PARA.\n{}      -    'PGM') END-EXEC.\n{}      -    'PGM'.\n           EXEC CICS XCTL PROGRAM('PAY\n      -    'PGM') END-EXEC.\n           GOBACK.\n",
            card("EXEC CICS LINK PROGRAM('SHIP"),
            card("CALL 'TAX"),
        );
        let ex = run("cbl/ORDPGM.cbl", &text);
        // Each call starts from the paragraph that makes it (wicked-estate#579).
        let pgm = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Function && n.name == "MAIN-PARA")
            .unwrap_or_else(|| panic!("{:?}", names(&ex)))
            .symbol
            .clone();
        for (name, from) in [("SHIPPGM", "'SHIP"), ("TAXPGM", "'TAX"), ("PAYPGM", "'PAY")] {
            let r = refs_named(&ex, EdgeKind::Calls, name);
            assert_eq!(r.len(), 1, "{name}: {:?}", calls(&ex));
            assert_eq!(r[0].from, pgm);
            assert_eq!(hint_str(r[0], ROLE_KEY), Some(ROLE_PROGRAM));
            let s = &r[0].location.span;
            assert!(s.end_line > s.start_line, "{name} spans both cards: {s:?}");
            assert!(text[s.start_byte as usize..].contains(from));
        }
        assert!(
            calls(&ex)
                .iter()
                .all(|c| !c.starts_with("SHIP ") && c != "SHIP" && c != "TAX")
        );
        let main = ex.nodes.iter().find(|n| n.name == "MAIN-PARA").unwrap();
        assert_eq!(main.location.span.start_line, 3);
    }

    /// A joined literal whose first part ends in blanks at column 72 keeps its tail in the text
    /// pass (the joined line must not look like a card with an identification area).
    #[test]
    fn continued_literal_with_a_blank_column_72_keeps_its_tail() {
        let first = format!("{:<72}\n", "           EXEC CICS LINK PROGRAM('SHIPPGM");
        let text = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. ORDPGM.\n       PROCEDURE DIVISION.\n{first}      -    'X') END-EXEC.\n           GOBACK.\n"
        );
        let ex = run("cbl/ORDPGM.cbl", &text);
        let link: Vec<_> = calls(&ex)
            .into_iter()
            .filter(|c| c.starts_with("SHIPPGM"))
            .collect();
        assert_eq!(link.len(), 1, "{:?}", calls(&ex));
        assert!(
            link[0].ends_with('X'),
            "the tail after the blanks is kept: {link:?}"
        );
    }

    /// Standard free format: a `*` or `/` in column 1 is an operator continuing an expression,
    /// and a long line is never split inside an SQL `--` comment.
    #[test]
    fn free_format_operators_and_sql_comments_stay_what_they_are() {
        let sql = format!(
            "    EXEC SQL SELECT A INTO :W FROM REALTAB -- {0} FROM GHOST\n    END-EXEC.\n    EXEC SQL SELECT A INTO :W FROM REALTAB--{0} FROM GHOST\n    END-EXEC.\n",
            "x ".repeat(30)
        );
        let text = format!(
            ">>SOURCE FORMAT FREE\nIDENTIFICATION DIVISION.\nPROGRAM-ID. P.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 W PIC 9.\n01 BASE PIC 9.\n01 RATE PIC 9.\nPROCEDURE DIVISION.\nMAIN-PARA.\n    COMPUTE W = BASE\n* RATE.\n{sql}    GOBACK.\n"
        );
        let ex = run("cbl/P.cbl", &text);
        assert!(
            !refs_named(&ex, EdgeKind::References, "RATE").is_empty(),
            "{:?}",
            ex.refs
        );
        let ghost = Symbol::synthetic("db2", "GHOST").id();
        assert!(ex.local_edges.iter().all(|e| e.target != ghost));
        assert!(
            edge_to(
                &ex,
                &edge_tags::other(edge_tags::READS),
                &Symbol::synthetic("db2", "REALTAB").id()
            )
            .is_some()
        );
    }

    /// A comment card between a literal and its continuation does not stop the join.
    #[test]
    fn continued_literal_joins_across_a_comment_card() {
        let text = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. ORDPGM.\n       PROCEDURE DIVISION.\n{:>72}\n      * a comment\n\n      -    'P\n      * another\n      -    'PGM') END-EXEC.\n           GOBACK.\n",
            "EXEC CICS LINK PROGRAM('SHI",
        );
        let ex = run("cbl/ORDPGM.cbl", &text);
        assert_eq!(
            refs_named(&ex, EdgeKind::Calls, "SHIPPGM").len(),
            1,
            "{:?}",
            calls(&ex)
        );
    }

    /// Column 72 inside a multibyte character: the map stays exact for everything after it.
    #[test]
    fn continued_literal_cut_inside_a_multibyte_character_keeps_spans_exact() {
        let first = format!("{}\u{e9}\n", " ".repeat(11) + "DISPLAY '" + &"A".repeat(51));
        assert_eq!(first.find('\u{e9}'), Some(71));
        let text = format!(
            "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. P.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n       01 W PIC 9.\n       PROCEDURE DIVISION.\n{first}      -    'B'.\n           MOVE 1 TO W.\n"
        );
        let l = LogicalSource::of(&text).expect("a continued literal");
        assert!(l.text.is_char_boundary(l.text.len()));
        let ex = run("cbl/P.cbl", &text);
        let w = refs_named(&ex, EdgeKind::References, "W");
        assert!(!w.is_empty(), "{:?}", ex.refs);
        assert!(w.iter().all(|r| at(&text, &r.location.span) == "W"));
    }

    #[test]
    fn logical_source_maps_file_bytes_both_ways() {
        let text = "       PROCEDURE DIVISION.\n           CALL 'AB\n      -    'CD' USING X.\n           GOBACK.\n";
        let l = LogicalSource::of(text).expect("a continued literal");
        assert!(l.text.contains("CALL 'ABCD'"), "{:?}", l.text);
        let using = text.find("USING").unwrap();
        let lu = l.logical_start(using);
        assert_eq!(&l.text[lu..lu + 5], "USING");
        assert_eq!(l.file_start(lu), using);
        assert_eq!(l.file_end(l.logical_end(using + 5)), using + 5);
        let call = l.text.find("'ABCD'").unwrap();
        assert_eq!(l.file_start(call), text.find("'AB").unwrap());
        assert_eq!(l.file_end(call + 6), text.find("'CD'").unwrap() + 4);
        assert!(
            LogicalSource::of(ORDPGM).is_none(),
            "fixed format, nothing continued"
        );
    }
}
