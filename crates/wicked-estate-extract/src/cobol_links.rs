//! COBOL text supplement — the links the arborium-cobol grammar cannot see.
//!
//! The grammar parses `EXEC CICS … END-EXEC` and `EXEC SQL … END-EXEC` as opaque `exec_statement`
//! nodes, and `COPY` is a compiler-directing statement processed before parsing. This pass reads
//! the same source and adds what those constructs mean to the tree-sitter [`Extraction`] of the
//! file:
//!
//! | Source construct | Stored as |
//! |---|---|
//! | `EXEC CICS LINK/XCTL PROGRAM('P')` | `Calls` ref program → `P` (any language; parks if absent) |
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
//! | `:HOSTVAR`, CICS `COMMAREA()/INTO()/FROM()` operands | `References` refs to the data item |
//!
//! **Lexical view.** Every construct is matched on a same-length copy of the source in which
//! comment lines (indicator `*` or `/`), floating `*>` comments, the fixed-format sequence and
//! identification areas (columns 1–6 and 73+; free format is `>>SOURCE FORMAT FREE`) and the
//! CONTENTS of every literal are blanked. So a `COPY` or an `EXEC SQL` inside a `DISPLAY` literal
//! is not code. Literal values are read back from the comment-blanked copy at the same offsets.
//!
//! **Every source is a real node** of this extraction: the program Module whose span holds the
//! construct (nested programs pick the innermost), else the first program, else the copybook node,
//! else the File node. Literal inference (dynamic calls, map names), labels and DD names are scoped
//! to that program. A shared target is minted by the referencing file as a BOUNDARY record
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
    // literal does not switch the format.
    Regex::new(r"(?im)^[0-9 ]*>>\s*SOURCE\s+(?:FORMAT\s+)?(?:IS\s+)?FREE\b")
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
static RE_SQL_LINE_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)--[^\n]*").expect("RE_SQL_LINE_COMMENT"));
static RE_SQL_BLOCK_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("RE_SQL_BLOCK_COMMENT"));
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
        r#"(?i)\bSET\s+([A-Za-z0-9][A-Za-z0-9-]*)\s+TO\s+ENTRY\s+(?:'([^']*)'|"([^"]*)"|([A-Za-z0-9][A-Za-z0-9-]*))"#,
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

/// Add the COBOL text links to `ex`, the tree-sitter extraction of `file`. See the module docs.
pub fn supplement(file: &SourceFile, ex: &mut Extraction) {
    let free = RE_SOURCE_FREE.is_match(&file.text);
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
    };
    p.exec_cics(&code);
    p.exec_sql(&code);
    p.copies(&code_no_exec);
    p.assigns(&code_no_exec);
    p.dynamic_calls(&code_no_exec, ex);
    p.finish(ex);
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
/// by `~`; the quote characters stay). Offsets are identical in the source and both views. Only
/// whole lines, whole literal contents or line tails are overwritten with ASCII, so both stay
/// valid UTF-8.
fn lexical_views(text: &str, free: bool) -> (String, String) {
    let mut clean: Vec<u8> = text.as_bytes().to_vec();
    let mut code: Vec<u8> = clean.clone();
    let mut start = 0usize;
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
            if body_end - start > 72 && body_end - start <= 80 && b[71] == b' ' {
                blank_bytes(&mut clean, start + 72, body_end);
                blank_bytes(&mut code, start + 72, body_end);
                stop = start + 72;
            }
            from = start + 7;
        }
        // Literals and floating `*>` comments, left to right.
        let mut i = from;
        while i < stop {
            let c = clean[i];
            if c == b'\'' || c == b'"' {
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
            } else {
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
    /// The source when no program holds the construct: the copybook node, else the File node.
    fallback: SymbolId,
    /// A copybook file's definition record + its `Contains` edges, taken by [`supplement`].
    copybook: Option<(SymbolId, Node, Vec<Edge>)>,
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
            fallback: Symbol::file(&file.path).id(),
            copybook,
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
            lit.add(c[1].to_ascii_uppercase(), value, what);
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
    for raw in rest.split(|c: char| c.is_whitespace() || c == ',') {
        if raw.is_empty() {
            continue;
        }
        let ends = raw.ends_with('.');
        let tok = raw.trim_end_matches('.');
        let tok = tok.split('(').next().unwrap_or(tok).to_ascii_uppercase();
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
        if ends {
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
        self.out
            .refs
            .push(UnresolvedRef::new(from, name, EdgeKind::References, loc));
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
            let (from, _) = self.anchors.at(whole.start());
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
                            self.out.refs.push(UnresolvedRef::new(
                                from.clone(),
                                raw,
                                EdgeKind::Calls,
                                loc.clone(),
                            ));
                        }
                    }
                }
                "SEND" | "RECEIVE" => self.cics_map(prog, &from, &verb, &opts, &loc),
                _ => {}
            }
            for key in ["COMMAREA", "INTO", "FROM"] {
                if let Some(op) = opts.get(key).filter(|o| !o.literal).cloned() {
                    self.reference(from.clone(), &op.value, loc.clone());
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
            // Literal contents are already masked, so `--` and `/*` here are real comments.
            let no_lc = RE_SQL_LINE_COMMENT.replace_all(c.get(1).unwrap().as_str(), " ");
            let body = RE_SQL_BLOCK_COMMENT.replace_all(&no_lc, " ").into_owned();
            let words: Vec<String> = body
                .split_whitespace()
                .take(4)
                .map(|w| w.to_ascii_uppercase())
                .collect();
            let w = |i: usize| words.get(i).map_or("", String::as_str);

            for h in RE_SQL_HOSTVAR.captures_iter(&body) {
                self.reference(from.clone(), &h[1], loc.clone());
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
        // References are never to a paragraph/section of the same program (PERFORM / GO TO labels
        // are qualified words too) or to a DD name it ASSIGNs. Kept per site (ENGINE-CONTRACT §2.1).
        let mut labels: HashSet<(Prog, String)> = HashSet::new();
        for n in ex.nodes.iter().filter(|n| n.kind == NodeKind::Function) {
            let prog = self.anchors.prog(n.location.span.start_byte as usize);
            labels.insert((prog, n.name.to_ascii_uppercase()));
        }
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
            !labels.contains(&key) && !self.ddnames.contains(&key)
        });
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
            if !is_table_name(&name) {
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
    fn every_source_is_the_program_module_not_a_file_anchor() {
        // Fails on main: refs/edges came from `synthetic("cobol-pgm", path)`, never a node.
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        let pgm = module_sym(&ex, "ORDPGM");
        let link = ex
            .refs
            .iter()
            .find(|r| r.raw_name == "SHIPPGM")
            .expect("LINK ref");
        assert_eq!(link.from, pgm);
        assert_eq!(link.kind, EdgeKind::Calls);
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
        let pgm = module_sym(&ex, "ORDPGM");
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
                .all(|r| r.from == pgm)
        );
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
            "{c:?}"
        );
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
                let from = ex.nodes.iter().find(|n| n.symbol == r.from).unwrap();
                (from.name.clone(), r.raw_name.clone())
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
}
