//! COBOL text supplement — the links the arborium-cobol grammar cannot see.
//!
//! The grammar parses `EXEC CICS … END-EXEC` and `EXEC SQL … END-EXEC` as opaque `exec_statement`
//! nodes, and `COPY` is a compiler-directing statement processed before parsing. This pass reads
//! the same source text (comment lines blanked, byte offsets preserved) and adds what those
//! constructs mean to the tree-sitter [`Extraction`] of the file:
//!
//! | Source construct | Stored as |
//! |---|---|
//! | `EXEC CICS LINK/XCTL PROGRAM('P')` | `Calls` ref program → `P` (any language; parks if absent) |
//! | `EXEC CICS SEND MAP('M') [MAPSET('S')]` | `other:writes` edge program → shared `cics-map:S.M` |
//! | `EXEC CICS RECEIVE MAP('M') [MAPSET('S')]` | `other:reads` edge program → shared `cics-map:S.M` |
//! | `EXEC SQL SELECT … FROM T` (and cursors, subselects, joins) | `other:reads` edge program → shared `db2:T` |
//! | `EXEC SQL INSERT INTO / UPDATE / DELETE FROM / MERGE INTO T` | `other:writes` edge program → `db2:T` |
//! | `EXEC SQL DECLARE T TABLE` (DCLGEN) | the DEFINITION record of `db2:T` |
//! | `COPY C` / `EXEC SQL INCLUDE C` | `Imports` edge program → shared `cobol-copybook:C` |
//! | a file with no PROGRAM-ID (a copybook) | the DEFINITION record of `cobol-copybook:<STEM>` + `Contains` its nodes |
//! | `SELECT F ASSIGN TO DD` | `other:uses` edge program → shared `ddname:<PGM>.<DD>` (JCL defines it) |
//! | `CALL VAR` / `PROGRAM(VAR)`, VAR holding one literal | `Calls` ref `cobol-dynamic-call:<LIT>` (Heuristic) |
//! | `:HOSTVAR`, CICS `COMMAREA()/INTO()/FROM()` operands | `References` refs to the data item |
//!
//! **Every source is a real node** of this extraction: the program Module whose span holds the
//! construct (nested programs pick the innermost), else the first program, else the copybook node.
//! A shared target is minted by the referencing file as a BOUNDARY record
//! ([`Node::as_boundary`]) whose reason says what source would define it; the defining file's
//! record wins in the store when it is indexed (see [`crate::contrib`]).
//!
//! Tiers: COPY, maps and DD bindings name their target literally → `Parsed`. Table names come from a
//! regex over SQL text → `Heuristic`. Dynamic-call targets are inferred → `Heuristic` (bound by
//! `wicked-estate-resolve`'s `DynamicCallResolver`).

use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;
use wicked_estate_core::refs::COBOL_DYNAMIC_CALL_PREFIX;
use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Language, Location, Node, NodeKind, ResolutionTier, SourceFile,
    Span, Symbol, SymbolId, UnresolvedRef, edge_tags,
};

const RESOLVED_BY: &str = "cobol-text";

static RE_EXEC_CICS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)\bEXEC\s+CICS\s+(.*?)\bEND-EXEC").expect("RE_EXEC_CICS"));
static RE_EXEC_SQL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)\bEXEC\s+SQL\s+(.*?)\bEND-EXEC").expect("RE_EXEC_SQL"));
static RE_EXEC_ANY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)\bEXEC\s+(?:SQL|CICS|DLI)\b.*?\bEND-EXEC").expect("RE_EXEC_ANY")
});
/// A CICS option `NAME(value)`: group 1 = option, 2/3 = quoted literal, 4 = bare operand.
static RE_CICS_OPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b([A-Z][A-Z-]*)\s*\(\s*(?:'([^']*)'|"([^"]*)"|([^()'"]*?))\s*\)"#)
        .expect("RE_CICS_OPT")
});
static RE_SQL_LINE_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)--[^\n]*").expect("RE_SQL_LINE_COMMENT"));
static RE_SQL_BLOCK_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("RE_SQL_BLOCK_COMMENT"));
static RE_SQL_HOSTVAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":([A-Za-z][A-Za-z0-9-]*)").expect("RE_SQL_HOSTVAR"));
static RE_SQL_FROM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:FROM|JOIN|USING)\s+").expect("RE_SQL_FROM"));
static RE_COPY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|[\s.])COPY\s+(?:'([^']+)'|"([^"]+)"|([A-Za-z0-9#$@][A-Za-z0-9#$@-]*))"#)
        .expect("RE_COPY")
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
    Regex::new(r#"(?is)\bMOVE\s+(?:'([^']*)'|"([^"]*)"|(\S+))\s+TO\s+"#).expect("RE_MOVE")
});
static RE_SET_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\bSET\s+([A-Za-z0-9][A-Za-z0-9-]*)\s+TO\s+ENTRY\s+(?:'([^']*)'|"([^"]*)"|([A-Za-z0-9][A-Za-z0-9-]*))"#,
    )
    .expect("RE_SET_ENTRY")
});
static RE_PROGRAM_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z0-9#$@][A-Z0-9#$@_-]{0,29}$").expect("RE_PROGRAM_NAME"));
static RE_DDNAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z#$@][A-Z0-9#$@]{0,7}$").expect("RE_DDNAME"));

/// Statement verbs and clause words that end a `MOVE … TO` target list.
const STOP_WORDS: &[&str] = &[
    "ACCEPT",
    "ADD",
    "CALL",
    "CLOSE",
    "COMPUTE",
    "CONTINUE",
    "DELETE",
    "DISPLAY",
    "DIVIDE",
    "ELSE",
    "END-EVALUATE",
    "END-IF",
    "END-PERFORM",
    "END-READ",
    "EVALUATE",
    "EXEC",
    "EXIT",
    "GO",
    "GOBACK",
    "IF",
    "INITIALIZE",
    "INSPECT",
    "MOVE",
    "MULTIPLY",
    "NOT",
    "OPEN",
    "PERFORM",
    "READ",
    "RELEASE",
    "RETURN",
    "REWRITE",
    "SEARCH",
    "SET",
    "SORT",
    "START",
    "STOP",
    "STRING",
    "SUBTRACT",
    "UNSTRING",
    "WHEN",
    "WRITE",
];
/// Figurative constants: initialisation, never a program name — ignored as assignments.
const FIGURATIVE: &[&str] = &[
    "SPACE",
    "SPACES",
    "ZERO",
    "ZEROS",
    "ZEROES",
    "LOW-VALUE",
    "LOW-VALUES",
    "HIGH-VALUE",
    "HIGH-VALUES",
    "QUOTE",
    "QUOTES",
    "NULL",
    "NULLS",
    "ALL",
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
];

/// Add the COBOL text links to `ex`, the tree-sitter extraction of `file`. See the module docs.
pub fn supplement(file: &SourceFile, ex: &mut Extraction) {
    let clean = blank_comments(&file.text);
    let no_exec = RE_EXEC_ANY.replace_all(&clean, |c: &regex::Captures| blank(&c[0]));
    let lines = LineIndex::new(&clean);
    let mut p = Pass {
        path: &file.path,
        lines: &lines,
        anchors: Anchors::of(file, ex, &clean),
        out: Out::default(),
        lits: Literals::build(&no_exec),
        ddnames: HashSet::new(),
        dynamic: HashMap::new(),
    };
    if let Some((sym, node, contains)) = p.anchors.copybook.take() {
        ex.local_edges.extend(contains);
        p.out.nodes.push(node);
        p.anchors.fallback = sym;
    }
    p.exec_cics(&clean);
    p.exec_sql(&clean);
    p.copies(&no_exec);
    p.assigns(&no_exec);
    p.dynamic_calls(&no_exec, ex);
    p.finish(ex);
}

// ── text helpers ───────────────────────────────────────────────────────────────────────────────

/// Same-length copy of `s` with every byte replaced by a space, newlines kept.
fn blank(s: &str) -> String {
    s.bytes()
        .map(|b| if b == b'\n' { '\n' } else { ' ' })
        .collect()
}

/// Blank COBOL comments, keeping byte offsets: a fixed-format comment line (`*` or `/` in the
/// indicator column 7) and a floating `*>` comment to end of line.
fn blank_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let b = line.as_bytes();
        if b.len() >= 7 && matches!(b[6], b'*' | b'/') {
            out.push_str(&blank(line));
        } else if let Some(i) = line.find("*>") {
            out.push_str(&line[..i]);
            out.push_str(&blank(&line[i..]));
        } else {
            out.push_str(line);
        }
    }
    out
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

// ── anchors ────────────────────────────────────────────────────────────────────────────────────

struct Anchors {
    /// (symbol, PROGRAM-ID, start byte, end byte) of every program in the file.
    modules: Vec<(SymbolId, String, usize, usize)>,
    /// The source when no program holds the construct: the copybook node, else the File node.
    fallback: SymbolId,
    /// A copybook file's definition record + its `Contains` edges, taken by [`supplement`].
    copybook: Option<(SymbolId, Node, Vec<Edge>)>,
}

impl Anchors {
    fn of(file: &SourceFile, ex: &Extraction, clean: &str) -> Self {
        let modules: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Module)
            .map(|n| {
                let s = &n.location.span;
                (
                    n.symbol.clone(),
                    n.name.to_ascii_uppercase(),
                    s.start_byte as usize,
                    s.end_byte as usize,
                )
            })
            .collect();
        let copybook = if modules.is_empty() && !RE_PROGRAM_ID.is_match(clean) {
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

    /// The innermost program whose span holds `byte`, else the first program, else the fallback.
    fn at(&self, byte: usize) -> (SymbolId, Option<&str>) {
        let inner = self
            .modules
            .iter()
            .filter(|(_, _, s, e)| *s <= byte && byte < *e)
            .min_by_key(|(_, _, s, e)| e - s)
            .or_else(|| self.modules.first());
        match inner {
            Some((sym, name, _, _)) => (sym.clone(), Some(name.as_str())),
            None => (self.fallback.clone(), None),
        }
    }
}

/// A copybook's member name: the file stem, uppercased (`cpy/ordrec.cpy` → `ORDREC`).
fn copybook_name(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = base.split('.').next().unwrap_or(base);
    stem.to_ascii_uppercase()
}

fn copybook_symbol(name: &str) -> SymbolId {
    Symbol::synthetic("cobol-copybook", name).id()
}

// ── literal assignments (dynamic CALL) ─────────────────────────────────────────────────────────

/// What a program-name variable can hold, read from its own source.
#[derive(Default)]
struct Literals {
    /// UPPERCASE data name → (literal or None for a non-literal source, what assigned it, line).
    by_name: HashMap<String, Vec<(Option<String>, String)>>,
}

#[derive(Debug, PartialEq)]
enum Decision {
    Resolved { value: String, evidence: String },
    Unresolved { reason: String },
}

impl Literals {
    fn build(text: &str) -> Self {
        let lines = LineIndex::new(text);
        let line_of = |b: usize| lines.pos(b).0 + 1;
        let mut lit = Self::default();
        // VALUE clauses: data descriptions only, i.e. before PROCEDURE DIVISION.
        let data_end = RE_PROCEDURE_DIVISION
            .find(text)
            .map_or(text.len(), |m| m.start());
        for c in RE_VALUE.captures_iter(&text[..data_end]) {
            let name = c[1].to_ascii_uppercase();
            if name == "FILLER" {
                continue;
            }
            let value = c.get(3).or_else(|| c.get(4)).map_or("", |m| m.as_str());
            let at = format!("VALUE (line {})", line_of(c.get(0).unwrap().start()));
            lit.add(name, Some(value.to_string()), at);
        }
        for c in RE_MOVE.captures_iter(text) {
            let whole = c.get(0).unwrap();
            let line = line_of(whole.start());
            let src = match (c.get(1).or_else(|| c.get(2)), c.get(3)) {
                (Some(l), _) => Some(l.as_str().to_string()),
                (None, Some(tok)) => {
                    let t = tok.as_str().to_ascii_uppercase();
                    if FIGURATIVE.contains(&t.as_str()) {
                        continue;
                    }
                    if t.bytes().all(|b| b.is_ascii_digit()) {
                        Some(t)
                    } else {
                        None
                    }
                }
                (None, None) => continue,
            };
            let what = match &src {
                Some(_) => format!("MOVE (line {line})"),
                None => format!("MOVE {} (line {line})", &c[3]),
            };
            for target in move_targets(&text[whole.end()..]) {
                lit.add(target, src.clone(), what.clone());
            }
        }
        for c in RE_SET_ENTRY.captures_iter(text) {
            let line = line_of(c.get(0).unwrap().start());
            let value = c
                .get(2)
                .or_else(|| c.get(3))
                .map(|m| m.as_str().to_string());
            let what = match &value {
                Some(_) => format!("SET ENTRY (line {line})"),
                None => format!("SET ENTRY {} (line {line})", &c[4]),
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
                    "no literal VALUE, MOVE or SET ENTRY for {var_u} in this source (it may be set \
                     in a copybook or at run time)"
                ),
            };
        };
        if let Some((_, what)) = entries.iter().find(|(v, _)| v.is_none()) {
            return Decision::Unresolved {
                reason: format!("{var_u} is assigned from a non-literal: {what}"),
            };
        }
        let values: BTreeSet<String> = entries
            .iter()
            .filter_map(|(v, _)| v.as_deref())
            .map(|v| v.trim().to_ascii_uppercase())
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

/// The receiving items of a `MOVE … TO` (the text right after `TO`): identifiers up to the period
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
        } else if STOP_WORDS.contains(&tok.as_str()) {
            break;
        } else if !tok.is_empty()
            && tok
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
        {
            out.push(tok);
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
    lines: &'a LineIndex,
    anchors: Anchors,
    out: Out,
    lits: Literals,
    ddnames: HashSet<String>,
    /// Dynamic-call variable (UPPERCASE) → the decision, for field metadata.
    dynamic: HashMap<String, Decision>,
}

impl Pass<'_> {
    fn loc(&self, start: usize, end: usize) -> Location {
        Location::new(self.path, self.lines.span(start, end))
    }

    #[allow(clippy::too_many_arguments)]
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
        self.out.refs.push(UnresolvedRef::new(
            from,
            name.to_ascii_uppercase(),
            EdgeKind::References,
            loc,
        ));
    }

    /// A program-name operand: a literal is a by-name `Calls` ref; a data name goes through the
    /// literal map (a single literal → the dynamic-call ref; otherwise the call parks under the
    /// variable's own name, and the reason lands on the variable's field node).
    fn program_call(&mut self, from: SymbolId, operand: &str, literal: bool, loc: Location) {
        let name = operand.trim().to_ascii_uppercase();
        if name.is_empty() {
            return;
        }
        if literal {
            self.out
                .refs
                .push(UnresolvedRef::new(from, name, EdgeKind::Calls, loc));
            return;
        }
        let raw = match self.decide(&name) {
            Some(value) => format!("{COBOL_DYNAMIC_CALL_PREFIX}{value}"),
            None => name,
        };
        self.out
            .refs
            .push(UnresolvedRef::new(from, raw, EdgeKind::Calls, loc));
    }

    /// Decide a dynamic-call variable once; `Some(program)` when it holds a single literal.
    fn decide(&mut self, var: &str) -> Option<String> {
        let d = self
            .dynamic
            .entry(var.to_ascii_uppercase())
            .or_insert_with(|| self.lits.decide(var));
        match d {
            Decision::Resolved { value, .. } => Some(value.clone()),
            Decision::Unresolved { .. } => None,
        }
    }

    // ── EXEC CICS ──────────────────────────────────────────────────────────────────────────
    fn exec_cics(&mut self, text: &str) {
        for c in RE_EXEC_CICS.captures_iter(text) {
            let whole = c.get(0).unwrap();
            let body = c.get(1).unwrap().as_str();
            let (from, _) = self.anchors.at(whole.start());
            let loc = self.loc(whole.start(), whole.end());
            let verb = body
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            let mut opts: HashMap<String, (String, bool)> = HashMap::new();
            for o in RE_CICS_OPT.captures_iter(body) {
                let key = o[1].to_ascii_uppercase();
                let (val, literal) = match o.get(2).or_else(|| o.get(3)) {
                    Some(l) => (l.as_str().to_string(), true),
                    None => (
                        o.get(4).map_or("", |m| m.as_str()).trim().to_string(),
                        false,
                    ),
                };
                opts.entry(key).or_insert((val, literal));
            }
            match verb.as_str() {
                "LINK" | "XCTL" => {
                    if let Some((p, literal)) = opts.get("PROGRAM").cloned() {
                        self.program_call(from.clone(), &p, literal, loc.clone());
                    }
                }
                "SEND" | "RECEIVE" => {
                    if let Some(map) = self.map_name(opts.get("MAP")) {
                        let set = self
                            .map_name(opts.get("MAPSET"))
                            .unwrap_or_else(|| map.clone());
                        let kind = if verb == "SEND" {
                            edge_tags::WRITES
                        } else {
                            edge_tags::READS
                        };
                        let sym = Symbol::synthetic("cics-map", format!("{set}.{map}")).id();
                        self.edge(
                            from.clone(),
                            sym.clone(),
                            edge_tags::other(kind),
                            ResolutionTier::Parsed,
                            loc.clone(),
                            &[("via", "exec-cics"), ("command", verb.as_str())],
                        );
                        self.boundary(
                            sym,
                            "cics_map",
                            &map,
                            loc.clone(),
                            format!(
                                "CICS {verb} MAP: no BMS mapset source (DFHMSD/DFHMDI) defining \
                                 map {map} in mapset {set} is indexed"
                            ),
                        );
                    }
                }
                _ => {}
            }
            for key in ["COMMAREA", "INTO", "FROM"] {
                if let Some((v, false)) = opts.get(key).cloned() {
                    let name = v.split_whitespace().next().unwrap_or("").to_string();
                    self.reference(from.clone(), &name, loc.clone());
                }
            }
        }
    }

    /// A MAP/MAPSET operand: a literal, or a data name holding one literal.
    fn map_name(&mut self, opt: Option<&(String, bool)>) -> Option<String> {
        let (v, literal) = opt?;
        let v = v.trim().to_ascii_uppercase();
        if v.is_empty() {
            return None;
        }
        if *literal {
            Some(v)
        } else {
            match self.lits.decide(&v) {
                Decision::Resolved { value, .. } => Some(value),
                Decision::Unresolved { .. } => None,
            }
        }
    }

    // ── EXEC SQL ───────────────────────────────────────────────────────────────────────────
    fn exec_sql(&mut self, text: &str) {
        let mut defined: HashSet<String> = HashSet::new();
        let mut used: Vec<(String, Location)> = Vec::new();
        for c in RE_EXEC_SQL.captures_iter(text) {
            let whole = c.get(0).unwrap();
            let (from, _) = self.anchors.at(whole.start());
            let loc = self.loc(whole.start(), whole.end());
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
                | "BEGIN" | "END" | "CALL" | "SET" => continue,
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
            let mut tables: Vec<(String, &str)> = Vec::new();
            if let Some(t) = &write_target {
                tables.push((t.clone(), edge_tags::WRITES));
            }
            for m in RE_SQL_FROM.find_iter(&body) {
                if !is_statement_level_or_subselect(&body, m.start()) {
                    continue;
                }
                // `DELETE FROM T`: that FROM names the write target, already recorded.
                let delete_from = statement == "DELETE"
                    && body[..m.start()].trim().eq_ignore_ascii_case("DELETE");
                for (k, t) in table_list(&body[m.end()..]).into_iter().enumerate() {
                    if !(delete_from && k == 0) {
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
                format!("Db2 table named in EXEC SQL; no DCLGEN (EXEC SQL DECLARE {t} TABLE) source is indexed"),
            );
        }
    }

    // ── COPY / INCLUDE ─────────────────────────────────────────────────────────────────────
    fn copies(&mut self, text: &str) {
        for c in RE_COPY.captures_iter(text) {
            let m = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).unwrap();
            let (from, _) = self.anchors.at(m.start());
            let loc = self.loc(m.start(), m.end());
            let name = copybook_name(m.as_str());
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
    fn assigns(&mut self, text: &str) {
        for c in RE_SELECT_ASSIGN.captures_iter(text) {
            let whole = c.get(0).unwrap();
            let target = c.get(2).or_else(|| c.get(3)).or_else(|| c.get(4)).unwrap();
            let raw = target.as_str().to_ascii_uppercase();
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
            let (from, pgm) = self.anchors.at(whole.start());
            let Some(pgm) = pgm.map(str::to_string) else {
                continue;
            };
            self.ddnames.insert(raw.clone());
            self.ddnames.insert(dd.clone());
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
    fn dynamic_calls(&mut self, text: &str, ex: &mut Extraction) {
        let vars: BTreeSet<String> = RE_DYN_CALL
            .captures_iter(text)
            .map(|c| c[1].to_ascii_uppercase())
            .collect();
        for var in vars {
            let Some(value) = self.decide(&var) else {
                continue;
            };
            for r in ex.refs.iter_mut() {
                if r.kind == EdgeKind::Calls && r.raw_name.eq_ignore_ascii_case(&var) {
                    r.raw_name = format!("{COBOL_DYNAMIC_CALL_PREFIX}{value}");
                }
            }
        }
    }

    fn finish(mut self, ex: &mut Extraction) {
        // Field metadata: why a dynamic call did or did not resolve.
        for n in ex.nodes.iter_mut().filter(|n| n.kind == NodeKind::Field) {
            if let Some(d) = self.dynamic.get(&n.name.to_ascii_uppercase()) {
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
        // References: never to a paragraph/section (PERFORM/GO TO labels) or a DD name, and one per
        // (source, name) — edges dedupe by key anyway, so per-site copies only grow the parked set.
        let labels: HashSet<String> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .map(|n| n.name.to_ascii_uppercase())
            .collect();
        let mut seen: HashSet<(SymbolId, String)> = HashSet::new();
        let mut refs = std::mem::take(&mut ex.refs);
        refs.append(&mut self.out.refs);
        refs.retain(|r| {
            if r.kind != EdgeKind::References {
                return true;
            }
            let name = r.raw_name.to_ascii_uppercase();
            !labels.contains(&name)
                && !self.ddnames.contains(&name)
                && seen.insert((r.from.clone(), name))
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

/// `FROM` at statement level, or inside a parenthesised subselect — not `EXTRACT(… FROM x)` or
/// `TRIM(LEADING ' ' FROM x)`.
fn is_statement_level_or_subselect(body: &str, at: usize) -> bool {
    let mut depth = 0i32;
    for (i, ch) in body[..at].char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' if depth > 0 => depth -= 1,
            '(' => {
                return body[i + 1..]
                    .trim_start()
                    .get(..6)
                    .is_some_and(|w| w.eq_ignore_ascii_case("SELECT"));
            }
            _ => {}
        }
    }
    true
}

/// `T1 [AS] [alias], T2 …` after a FROM/JOIN/USING, up to the first clause word.
fn table_list(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = rest.as_bytes();
    let mut i = 0;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && is_ident_byte(bytes[i]) {
            i += 1;
        }
        let name = rest[start..i].trim_end_matches('.').to_ascii_uppercase();
        if !is_table_name(&name) {
            break;
        }
        out.push(name);
        // optional [AS] alias
        let mut j = i;
        for _ in 0..2 {
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
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
        i = j;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
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
        let ord_id: Vec<_> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::References && r.raw_name == "ORD-ID")
            .collect();
        assert_eq!(ord_id.len(), 1, "deduped per (from, name)");
        assert_eq!(ord_id[0].from, pgm);
    }

    #[test]
    fn dynamic_call_single_value_rewrites_to_prefixed_ref() {
        let ex = run("cbl/ORDPGM.cbl", ORDPGM);
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Calls && r.raw_name == "cobol-dynamic-call:PRICEPGM"),
            "{:?}",
            ex.refs.iter().map(|r| &r.raw_name).collect::<Vec<_>>()
        );
        assert!(
            !ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Calls && r.raw_name == "WS-PGM")
        );
        let f = ex.nodes.iter().find(|n| n.name == "WS-PGM").unwrap();
        assert_eq!(f.metadata["dynamic_call"]["status"], "resolved");
        assert_eq!(f.metadata["dynamic_call"]["value"], "PRICEPGM");
    }

    #[test]
    fn literal_map_decisions() {
        let lit = Literals::build(
            "       01 A PIC X(8) VALUE 'ONE'.
       01 B PIC X(8) VALUE 'ONE'.
       01 C PIC X(8).
       01 D PIC X(8) VALUE SPACES.
       01 E PIC X(8).
       PROCEDURE DIVISION.
           MOVE 'TWO' TO B.
           MOVE 'SOLO' TO C D.
           MOVE WS-X TO E.
           SET F TO ENTRY 'PTRPGM'.
           MOVE SPACES TO A.
",
        );
        assert!(matches!(lit.decide("A"), Decision::Resolved { ref value, .. } if value == "ONE"));
        assert!(
            matches!(lit.decide("B"), Decision::Unresolved { ref reason } if reason.contains("2 literal values"))
        );
        assert!(matches!(lit.decide("C"), Decision::Resolved { ref value, .. } if value == "SOLO"));
        assert!(matches!(lit.decide("D"), Decision::Resolved { ref value, .. } if value == "SOLO"));
        assert!(
            matches!(lit.decide("E"), Decision::Unresolved { ref reason } if reason.contains("non-literal"))
        );
        assert!(
            matches!(lit.decide("F"), Decision::Resolved { ref value, .. } if value == "PTRPGM")
        );
        assert!(
            matches!(lit.decide("G"), Decision::Unresolved { ref reason } if reason.contains("no literal"))
        );
    }

    #[test]
    fn ambiguous_dynamic_call_parks_with_reason_on_the_field() {
        let ex = run(
            "cbl/P.cbl",
            "       IDENTIFICATION DIVISION.
       PROGRAM-ID. P.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       01 WS-PGM PIC X(8) VALUE 'AAA'.
       PROCEDURE DIVISION.
       MAIN-PARA.
           MOVE 'BBB' TO WS-PGM.
           CALL WS-PGM.
           EXEC CICS XCTL PROGRAM(WS-PGM) END-EXEC.
",
        );
        assert!(
            !ex.refs
                .iter()
                .any(|r| r.raw_name.starts_with(COBOL_DYNAMIC_CALL_PREFIX))
        );
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Calls && r.raw_name == "WS-PGM")
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
    fn sql_read_write_classification() {
        let ex = run(
            "cbl/P.cbl",
            "       IDENTIFICATION DIVISION.
       PROGRAM-ID. P.
       PROCEDURE DIVISION.
       MAIN-PARA.
           EXEC SQL INSERT INTO AUDIT (A) SELECT X FROM SRC END-EXEC.
           EXEC SQL UPDATE ACCT SET BAL = 0 WHERE ID IN
                (SELECT ID FROM CLOSED) END-EXEC.
           EXEC SQL DELETE FROM LOG WHERE TS < CURRENT DATE END-EXEC.
           EXEC SQL DECLARE C1 CURSOR FOR SELECT A.X, B.Y FROM T1 A, T2 B
                WHERE EXTRACT(YEAR FROM A.D) = 2020 FOR UPDATE OF X END-EXEC.
           EXEC SQL FETCH FROM C1 INTO :H END-EXEC.
           EXEC SQL SELECT QTY INTO :Q FROM ORDERS END-EXEC.
",
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
        assert_eq!(kinds("AUDIT"), BTreeSet::from([w.clone()]));
        assert_eq!(kinds("SRC"), BTreeSet::from([r.clone()]));
        assert_eq!(kinds("ACCT"), BTreeSet::from([w.clone()]));
        assert_eq!(kinds("CLOSED"), BTreeSet::from([r.clone()]));
        assert_eq!(kinds("LOG"), BTreeSet::from([w.clone()]));
        assert_eq!(kinds("T1"), BTreeSet::from([r.clone()]));
        assert_eq!(kinds("T2"), BTreeSet::from([r.clone()]));
        assert_eq!(kinds("ORDERS"), BTreeSet::from([r.clone()]));
        for bogus in ["C1", "A.D", "Q", "H", "ID", "OF"] {
            assert!(kinds(bogus).is_empty(), "{bogus} is not a table");
        }
    }

    #[test]
    fn cics_maps_and_dclgen_and_include() {
        let ex = run(
            "cbl/P.cbl",
            "       IDENTIFICATION DIVISION.
       PROGRAM-ID. P.
       PROCEDURE DIVISION.
       MAIN-PARA.
           EXEC SQL INCLUDE SQLCA END-EXEC.
           EXEC SQL INCLUDE DCLORD END-EXEC.
           EXEC CICS SEND MAP('ORDMAP') MAPSET('ORDSET') ERASE END-EXEC.
           EXEC CICS RECEIVE MAP('ORDMAP') MAPSET('ORDSET') INTO(WS-IN) END-EXEC.
           EXEC CICS SEND MAP('SOLO') END-EXEC.
",
        );
        let map = Symbol::synthetic("cics-map", "ORDSET.ORDMAP").id();
        assert!(edge_to(&ex, &edge_tags::other(edge_tags::WRITES), &map).is_some());
        assert!(edge_to(&ex, &edge_tags::other(edge_tags::READS), &map).is_some());
        assert!(
            edge_to(
                &ex,
                &edge_tags::other(edge_tags::WRITES),
                &Symbol::synthetic("cics-map", "SOLO.SOLO").id()
            )
            .is_some()
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
            "       IDENTIFICATION DIVISION.
       PROGRAM-ID. P.
       PROCEDURE DIVISION.
       MAIN-PARA.
      *    EXEC SQL SELECT A FROM GHOST END-EXEC.
      *    COPY GHOSTCPY.
           GOBACK.
",
        );
        assert!(
            !ex.nodes
                .iter()
                .any(|n| n.name == "GHOST" || n.name == "GHOSTCPY")
        );
    }
}
