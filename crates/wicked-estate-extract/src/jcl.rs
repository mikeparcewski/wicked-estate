//! JCL (Job Control Language) extractor — **line/card-oriented, NO tree-sitter grammar**.
//!
//! No tree-sitter grammar exists for JCL (or RPG / PL-I / Natural / REXX / HLASM), and authoring one
//! would not help: JCL is a fixed *card* format, not an expression grammar — borrowing a JS/Python
//! grammar transfers nothing. But the [`Extractor`] seam is generic (tree-sitter is just one impl;
//! `IaCExtractor`/`TfstateCollector` already parse without a grammar), so JCL is a first-class
//! language via a ~100-line line parser. This is the pattern for ANY grammar-less legacy format.
//!
//! # What it extracts
//! - `//NAME JOB ...`            → a [`NodeKind::Module`] node (the job).
//! - `//STEP EXEC PGM=PROG,...`  → a `NodeKind::Other("step")` node + a `Contains` edge job→step
//!   **and an `EdgeKind::Calls` ref step→PROG** — the high-value link: when `PROG` is a COBOL
//!   program node, this wires the cross-language **JCL → COBOL** call graph ("what job runs this
//!   program?" / blast-radius across the estate).
//! - `//STEP EXEC PROCNAME`      → step + a `Calls` ref to the invoked PROC.
//!
//! - `//NAME PROC` (in-stream, ended by `// PEND`, or a cataloged PROC member) → a
//!   [`NodeKind::Module`] named NAME that `Contains` its steps, so `EXEC NAME` resolves to it.
//! - `//DD DD DSN=…` → a shared `dataset` node + a step→dataset `other:uses` edge.
//! - `//DD DD …` in a step that runs `PGM=P` → the DEFINITION record of the shared DD binding
//!   `ddname:P.DD` (kind `dd_binding`, step `Contains` it, it `uses` the dataset). A COBOL
//!   `SELECT f ASSIGN TO DD` in program P links to the same id (`cobol_links`), so blast radius on
//!   a dataset reaches the program that reads it through its DD name.

use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
    Result, SourceFile, Span, Symbol, SymbolId, UnresolvedRef,
};

/// Extractor for JCL job streams. Grammar-less; parses the `//` card format line by line.
pub struct JclExtractor;

impl JclExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JclExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for JclExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("jcl")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut local_edges: Vec<Edge> = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        let mut current_job: Option<SymbolId> = None;
        let mut current_proc: Option<SymbolId> = None;
        let mut current_step: Option<SymbolId> = None;
        let mut current_pgm: Option<String> = None;
        // The DD binding the next unnamed concatenation card adds a dataset to.
        let mut last_dd: Option<SymbolId> = None;
        let extents = Extents::scan(&file.text);

        for (i, raw) in file.text.lines().enumerate() {
            // JCL statements begin in columns 1-2 with `//`. `//*` is a comment; `//` alone is a
            // null/delimiter statement; `/*` is end-of-data.
            let Some(body) = raw.strip_prefix("//") else {
                continue;
            };
            if body.starts_with('*') || body.trim().is_empty() {
                continue;
            }

            // An unnamed `//         DD DSN=…` card continues the previous DD (a concatenation).
            let concat = body.starts_with([' ', '\t']);
            // Card form: NAME OPERATION OPERANDS  (whitespace-separated).
            let mut toks = body.split_whitespace();
            let Some(name) = toks.next() else { continue };
            if concat && name == "DD" {
                let operands = toks.next().unwrap_or("");
                let loc = Location::new(&file.path, line_span(i));
                if let (Some(dsn), Some(step)) = (parse_dsn(operands), &current_step) {
                    let ds_sym = Symbol::synthetic("jcl-dataset", dsn.as_str()).id();
                    for from in [Some(step.clone()), last_dd.clone()].into_iter().flatten() {
                        local_edges.push(
                            Edge::new(
                                from,
                                ds_sym.clone(),
                                EdgeKind::Other("uses".to_string()),
                                ResolutionTier::Parsed,
                                "jcl",
                            )
                            .with_location(loc.clone()),
                        );
                    }
                    let mut node = Node::new(
                        ds_sym,
                        NodeKind::Other("dataset".to_string()),
                        dsn,
                        Language::new("jcl"),
                        loc,
                    );
                    node.signature = Some("DATASET".to_string());
                    nodes.push(node);
                }
                continue;
            }
            let Some(op) = toks.next() else {
                // `//         PEND` — the end of an in-stream PROC.
                if name == "PEND" {
                    current_proc = None;
                    current_step = None;
                    current_pgm = None;
                    last_dd = None;
                }
                continue;
            };
            let operands = toks.next().unwrap_or("");
            let loc = Location::new(&file.path, line_span(i));

            match op {
                "JOB" => {
                    let sym = Symbol::synthetic("jcl", format!("{}::{}", file.path, name)).id();
                    let mut node = Node::new(
                        sym.clone(),
                        NodeKind::Module,
                        name.to_string(),
                        Language::new("jcl"),
                        Location::new(&file.path, extents.span(i, Stmt::Job)),
                    );
                    node.signature = Some("JOB".to_string());
                    nodes.push(node);
                    current_job = Some(sym);
                    current_proc = None;
                    current_step = None;
                    current_pgm = None;
                    last_dd = None;
                }
                "PROC" => {
                    // The DEFINITION of the shared PROC id an `EXEC name` / `EXEC PROC=name` links
                    // to. Not a Module: `EXEC PGM=X` names a program and must never bind (or be
                    // made ambiguous by) a PROC that shares its name, a common naming convention.
                    let sym = proc_symbol(name);
                    let mut node = Node::new(
                        sym.clone(),
                        NodeKind::Other("jcl_proc".to_string()),
                        name.to_string(),
                        Language::new("jcl"),
                        Location::new(&file.path, extents.span(i, Stmt::Proc)),
                    );
                    node.signature = Some("PROC".to_string());
                    nodes.push(node);
                    if let Some(job) = &current_job {
                        local_edges.push(
                            Edge::new(
                                job.clone(),
                                sym.clone(),
                                EdgeKind::Contains,
                                ResolutionTier::Parsed,
                                "jcl",
                            )
                            .with_location(loc.clone()),
                        );
                    }
                    current_proc = Some(sym);
                    current_step = None;
                    current_pgm = None;
                    last_dd = None;
                }
                "PEND" => {
                    current_proc = None;
                    current_step = None;
                    current_pgm = None;
                    last_dd = None;
                }
                "EXEC" => {
                    let step_sym =
                        Symbol::synthetic("jcl", format!("{}::{}::{}", file.path, name, i)).id();
                    let mut node = Node::new(
                        step_sym.clone(),
                        NodeKind::Other("step".to_string()),
                        name.to_string(),
                        Language::new("jcl"),
                        Location::new(&file.path, extents.span(i, Stmt::Exec)),
                    );
                    node.signature = Some(format!("EXEC {operands}"));
                    nodes.push(node);
                    current_step = Some(step_sym.clone());
                    current_pgm = operands
                        .split(',')
                        .next()
                        .and_then(|f| f.trim().strip_prefix("PGM="))
                        .map(str::trim)
                        .filter(|p| !p.is_empty() && !p.contains(['*', '.', '&']))
                        .map(str::to_ascii_uppercase);

                    // job/PROC → step (Contains): blast-radius/containment.
                    if let Some(owner) = current_proc.as_ref().or(current_job.as_ref()) {
                        local_edges.push(
                            Edge::new(
                                owner.clone(),
                                step_sym.clone(),
                                EdgeKind::Contains,
                                ResolutionTier::Parsed,
                                "jcl",
                            )
                            .with_location(loc.clone()),
                        );
                    }

                    // step → invoked program (Calls by name: the cross-language link), or → the
                    // shared PROC id (a direct edge; the PROC source defines it, else a boundary).
                    if let Some(target) = parse_exec_target(operands) {
                        if current_pgm.is_some() || operands.starts_with("PGM=") {
                            refs.push(UnresolvedRef::new(
                                step_sym.clone(),
                                target,
                                EdgeKind::Calls,
                                loc,
                            ));
                        } else {
                            let proc_sym = proc_symbol(&target);
                            local_edges.push(
                                Edge::new(
                                    step_sym.clone(),
                                    proc_sym.clone(),
                                    EdgeKind::Calls,
                                    ResolutionTier::Parsed,
                                    "jcl",
                                )
                                .with_location(loc.clone()),
                            );
                            nodes.push(
                                Node::new(
                                    proc_sym,
                                    NodeKind::Other("jcl_proc".to_string()),
                                    target.to_ascii_uppercase(),
                                    Language::new("jcl"),
                                    loc,
                                )
                                .as_boundary(format!(
                                    "EXEC {target}: no PROC source (in-stream or cataloged) named \
                                     {target} is indexed"
                                )),
                            );
                        }
                    }
                }
                "DD" => {
                    // DD DSN=dataset → the step uses a dataset (VSAM/GDG/PDS/QSAM). The DSN value
                    // (including a GDG `(+1)` or PDS `(MEMBER)` suffix) becomes a SHARED dataset
                    // node, with a step→dataset edge (source = dependent step, target = dependency
                    // dataset) so blast-radius on a dataset finds every job/step that touches it.
                    let dsn = parse_dsn(operands);
                    let ds_sym = dsn
                        .as_deref()
                        .map(|d| Symbol::synthetic("jcl-dataset", d).id());
                    // The DD binding of the step's program (`ddname:<PGM>.<DD>`).
                    let dd = name.to_ascii_uppercase();
                    last_dd = None;
                    if let (Some(pgm), Some(step)) = (&current_pgm, &current_step) {
                        if is_ddname(&dd) {
                            let bind = crate::cobol_links::dd_binding_symbol(pgm, &dd);
                            let mut node = Node::new(
                                bind.clone(),
                                NodeKind::Other("dd_binding".to_string()),
                                format!("{pgm}.{dd}"),
                                Language::new("jcl"),
                                loc.clone(),
                            );
                            node.signature = Some(format!("DD {operands}"));
                            node.metadata
                                .insert("program".into(), serde_json::Value::String(pgm.clone()));
                            node.metadata
                                .insert("ddname".into(), serde_json::Value::String(dd.clone()));
                            nodes.push(node);
                            last_dd = Some(bind.clone());
                            local_edges.push(
                                Edge::new(
                                    step.clone(),
                                    bind.clone(),
                                    EdgeKind::Contains,
                                    ResolutionTier::Parsed,
                                    "jcl",
                                )
                                .with_location(loc.clone()),
                            );
                            if let Some(ds) = &ds_sym {
                                local_edges.push(
                                    Edge::new(
                                        bind,
                                        ds.clone(),
                                        EdgeKind::Other("uses".to_string()),
                                        ResolutionTier::Parsed,
                                        "jcl",
                                    )
                                    .with_location(loc.clone()),
                                );
                            }
                        }
                    }
                    if let (Some(dsn), Some(ds_sym)) = (dsn, ds_sym) {
                        if let Some(from) = current_step.clone().or_else(|| current_job.clone()) {
                            local_edges.push(
                                Edge::new(
                                    from,
                                    ds_sym.clone(),
                                    EdgeKind::Other("uses".to_string()),
                                    ResolutionTier::Parsed,
                                    "jcl",
                                )
                                .with_location(loc.clone()),
                            );
                        }
                        let mut node = Node::new(
                            ds_sym,
                            NodeKind::Other("dataset".to_string()),
                            dsn,
                            Language::new("jcl"),
                            loc,
                        );
                        node.signature = Some("DATASET".to_string());
                        nodes.push(node);
                    }
                }
                _ => {}
            }
        }

        crate::contrib::settle(&mut nodes);
        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

/// The statement kinds that bound a node's extent.
#[derive(Clone, Copy, PartialEq)]
enum Stmt {
    Job,
    Proc,
    Exec,
    Pend,
}

/// Line-start byte offsets plus the JOB/PROC/EXEC/PEND boundary lines, so a node can span its whole
/// statement (card, continuations, DDs, in-stream data, comments) instead of a one-line anchor.
struct Extents {
    line_starts: Vec<usize>,
    /// Byte offset where each line's content ends (before its terminator).
    line_ends: Vec<usize>,
    text_len: usize,
    bounds: Vec<(usize, Stmt)>,
}

impl Extents {
    fn scan(text: &str) -> Self {
        let mut line_starts = Vec::new();
        let mut line_ends = Vec::new();
        let mut bounds = Vec::new();
        let mut pos = 0;
        // A statement whose operand field ends with a comma continues on the next `//` card; that
        // continuation carries operands/comments only (no name, no operation), so it must never be
        // read as a boundary.
        let mut continued = false;
        // In-stream data (`DD *` / `DD DATA`): every record up to the delimiter belongs to the data,
        // not to the job stream, so no card inside it is a boundary. `(delimiter, slashes_are_data)`;
        // `slashes_are_data` holds for `DD DATA` and whenever `DLM=` is set — then even `//` records
        // are data (`DD *` without `DLM=` instead ends at the next `//` card).
        let mut in_stream: Option<(String, bool)> = None;
        for (i, raw) in text.lines().enumerate() {
            line_starts.push(pos);
            // `lines()` strips `\n` or `\r\n`; advance past the real terminator.
            pos += raw.len();
            line_ends.push(pos);
            if text[pos..].starts_with("\r\n") {
                pos += 2;
            } else if text[pos..].starts_with('\n') {
                pos += 1;
            }

            // Inside in-stream data: consume records until the delimiter (or, for `DD *` without
            // `DLM=`, the next `//` card, which is itself a real statement to be processed below).
            if let Some((delim, slashes_are_data)) = in_stream.clone() {
                if raw.starts_with(delim.as_str()) {
                    in_stream = None;
                    continue;
                }
                if !slashes_are_data && raw.starts_with("//") {
                    in_stream = None;
                } else {
                    continue;
                }
            }

            let Some(body) = raw.strip_prefix("//") else {
                continue;
            };
            // `//*` is a comment and `//` alone is null/delimiter; neither breaks a continuation.
            if body.starts_with('*') || body.trim().is_empty() {
                continue;
            }

            let was_continued = continued;
            // The name field starts in column 3; a blank column 3 means no name, so the operation is
            // the first token. A continuation card carries only operand/comment text.
            let has_name = !body.starts_with([' ', '\t']);
            let mut toks = body.split_whitespace();
            let (op, operand) = if was_continued {
                (None, toks.next())
            } else if has_name {
                let _name = toks.next();
                (toks.next(), toks.next())
            } else {
                (toks.next(), toks.next())
            };
            // Does this card's operand field continue onto the next card?
            continued = operand.is_some_and(|o| o.ends_with(','));
            if was_continued {
                continue;
            }

            if let Some(kind) = match op {
                Some("JOB") => Some(Stmt::Job),
                Some("PROC") => Some(Stmt::Proc),
                Some("PEND") => Some(Stmt::Pend),
                Some("EXEC") => Some(Stmt::Exec),
                _ => None,
            } {
                bounds.push((i, kind));
            } else if op == Some("DD") {
                // `DD *` / `DD DATA` open in-stream data; `SYSOUT=*` and the like do not.
                if let Some(operand) = operand {
                    let first = operand.split(',').next().unwrap_or("");
                    if first == "*" || first == "DATA" {
                        let dlm = parse_dlm(operand);
                        let slashes_are_data = first == "DATA" || dlm.is_some();
                        in_stream =
                            Some((dlm.unwrap_or_else(|| "/*".to_string()), slashes_are_data));
                    }
                }
            }
        }
        Self {
            line_starts,
            line_ends,
            text_len: text.len(),
            bounds,
        }
    }

    /// The extent of the `kind` statement at `line`: its first byte through the end of the last
    /// line that belongs to it. A step ends before the next JOB/PROC/EXEC/PEND; a job before the
    /// next JOB/PROC; a PROC after its PEND (or before the next JOB/PROC, or at end of file).
    fn span(&self, line: usize, kind: Stmt) -> Span {
        let n = self.line_starts.len();
        let end_line_excl = self
            .bounds
            .iter()
            .filter(|(l, _)| *l > line)
            .find_map(|&(l, k)| match (kind, k) {
                (Stmt::Exec, _) => Some(l),
                (_, Stmt::Job | Stmt::Proc) => Some(l),
                (Stmt::Proc, Stmt::Pend) => Some(l + 1),
                _ => None,
            })
            .unwrap_or(n)
            .min(n);
        let end_byte = self
            .line_starts
            .get(end_line_excl)
            .copied()
            .unwrap_or(self.text_len);
        let last = end_line_excl - 1;
        Span {
            start_byte: self.line_starts[line] as u32,
            end_byte: end_byte as u32,
            start_line: line as u32,
            start_col: 0,
            end_line: last as u32,
            end_col: (self.line_ends[last] - self.line_starts[last]) as u32,
        }
    }
}

/// One-line span anchor for nodes that really are a single card (DD, dataset) and for edges.
fn line_span(line: usize) -> Span {
    let l = line as u32;
    Span {
        start_byte: 0,
        end_byte: 0,
        start_line: l,
        start_col: 0,
        end_line: l,
        end_col: 0,
    }
}

/// The shared id of a JCL procedure: its in-stream or cataloged `//NAME PROC` defines it, and every
/// `EXEC NAME` / `EXEC PROC=NAME` links to it.
fn proc_symbol(name: &str) -> SymbolId {
    Symbol::synthetic("jcl-proc", name.to_ascii_uppercase()).id()
}

/// A DD statement name that a program can `ASSIGN TO`: 1–8 alphanumeric/national characters
/// (`STEP.DD` overrides and concatenation continuations are not bindings).
fn is_ddname(dd: &str) -> bool {
    let b = dd.as_bytes();
    (1..=8).contains(&b.len())
        && (b[0].is_ascii_alphabetic() || matches!(b[0], b'#' | b'$' | b'@'))
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'#' | b'$' | b'@'))
}

/// From an `EXEC` operand string, return the invoked program or PROC name.
/// `PGM=MYCOBOL,REGION=4M` → `MYCOBOL` ; `MYPROC,PARM=…` → `MYPROC` ; `PROC=X` / keyword-only → None.
fn parse_exec_target(operands: &str) -> Option<String> {
    let first = operands.split(',').next()?.trim();
    if let Some(pgm) = first.strip_prefix("PGM=") {
        let p = pgm.trim();
        return (!p.is_empty()).then(|| p.to_string());
    }
    if let Some(p) = first.strip_prefix("PROC=") {
        let p = p.trim();
        return (!p.is_empty()).then(|| p.to_string());
    }
    // A bare first operand with no `=` is a cataloged-procedure invocation (`EXEC MYPROC`).
    if !first.is_empty() && !first.contains('=') {
        return Some(first.to_string());
    }
    None
}

/// Extract the dataset name from a DD operand string.
/// `DSN=PROD.CUST.VSAM,DISP=SHR` → `PROD.CUST.VSAM`. The value ends at the first comma that is NOT
/// inside parentheses, so GDG (`PROD.GDG(+1)`) and PDS members (`PROD.LIB(MEMBER)`) are kept whole.
/// Returns `None` for non-dataset DDs (`SYSOUT=A`, `DUMMY`, `*` inline data).
fn parse_dsn(operands: &str) -> Option<String> {
    let start = ["DSNAME=", "DSN="]
        .iter()
        .find_map(|k| operands.find(k).map(|p| p + k.len()))?;
    let rest = &operands[start..];
    let mut depth = 0i32;
    let mut end = rest.len();
    for (j, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                end = j;
                break;
            }
            _ => {}
        }
    }
    let dsn = rest[..end].trim();
    (!dsn.is_empty()).then(|| dsn.to_string())
}

/// The in-stream data delimiter from a `DD *` / `DD DATA` operand: the characters after `DLM=`
/// (default `/*` when absent). `DATA,DLM=@@` → `@@`; surrounding quotes, if any, are stripped.
fn parse_dlm(operand: &str) -> Option<String> {
    operand
        .split(',')
        .find_map(|f| f.strip_prefix("DLM="))
        .map(|v| v.trim_matches('\'').to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped after AWS CardDemo app/jcl/PRTCATBL.jcl (Apache-2.0).
    const PRTCATBL: &str = "//PRTCATBL JOB 'PRTCAT',CLASS=A,MSGCLASS=0\n\
//* Unload the category table\n\
//STEP05 EXEC PROC=REPROC,\n\
//         CNTLLIB=&HLQ..CNTL\n\
//* comment between cards\n\
//PRIMARY DD DISP=SHR,\n\
//         DSN=&HLQ..TRANCATG.PS\n\
//OUTFILE DD DSN=&HLQ..TRANCATG.BKUP(+1),\n\
//         DISP=(NEW,CATLG,DELETE)\n\
//STEP10 EXEC PGM=SORT\n\
//SYMNAMES DD *\n\
TRAN-CAT-TYPE,1,2,ZD\n\
/*\n\
//SYSIN DD *\n\
 SORT FIELDS=(TRAN-CAT-TYPE,A)\n\
/*\n";

    #[test]
    fn step_spans_cover_their_whole_statement() {
        for eol in ["\n", "\r\n"] {
            let text = PRTCATBL.replace('\n', eol);
            let x = extract(&text);
            let node = |n: &str, kind: NodeKind| {
                x.nodes
                    .iter()
                    .find(|nd| nd.name == n && nd.kind == kind)
                    .unwrap_or_else(|| panic!("node {n}"))
                    .location
                    .span
            };
            let step = |n: &str| node(n, NodeKind::Other("step".to_string()));
            let lines: Vec<&str> = text.split_inclusive('\n').collect();
            let off = |l: usize| lines[..l].iter().map(|s| s.len()).sum::<usize>();
            let s5 = step("STEP05");
            let s10 = step("STEP10");
            assert_eq!((s5.start_line, s5.end_line), (2, 8));
            assert_eq!((s10.start_line, s10.end_line), (9, 15));
            assert_eq!(
                (s5.end_col, s10.end_col),
                ("//         DISP=(NEW,CATLG,DELETE)".len() as u32, 2)
            );
            assert_eq!(
                (s5.start_byte as usize, s5.end_byte as usize),
                (off(2), off(9))
            );
            assert_eq!(
                (s10.start_byte as usize, s10.end_byte as usize),
                (off(9), text.len())
            );
            let slice = &text[s5.start_byte as usize..s5.end_byte as usize];
            assert!(slice.starts_with("//STEP05 EXEC") && slice.contains("TRANCATG.BKUP"));
            assert!(!slice.contains("STEP10"));
            let slice = &text[s10.start_byte as usize..s10.end_byte as usize];
            assert!(slice.contains("SORT FIELDS=") && slice.trim_end().ends_with("/*"));
            let job = node("PRTCATBL", NodeKind::Module);
            assert_eq!((job.start_byte, job.end_byte as usize), (0, text.len()));
        }
    }

    #[test]
    fn proc_span_includes_pend_and_stops_at_next_proc() {
        let text = "//J JOB\n//P1 PROC\n//S1 EXEC PGM=A\n//   PEND\n//S2 EXEC PGM=B\n//P2 PROC\n//S3 EXEC PGM=C\n";
        let x = extract(text);
        let slice = |n: &str| {
            let s = &x
                .nodes
                .iter()
                .find(|nd| nd.name == n)
                .unwrap()
                .location
                .span;
            &text[s.start_byte as usize..s.end_byte as usize]
        };
        assert_eq!(slice("P1"), "//P1 PROC\n//S1 EXEC PGM=A\n//   PEND\n");
        assert_eq!(slice("S1"), "//S1 EXEC PGM=A\n");
        assert_eq!(slice("P2"), "//P2 PROC\n//S3 EXEC PGM=C\n");
    }

    // The extent of a node named `n`, as the exact source slice its span covers.
    fn slice_of<'a>(text: &'a str, x: &Extraction, n: &str) -> &'a str {
        let s = x
            .nodes
            .iter()
            .find(|nd| nd.name == n)
            .unwrap_or_else(|| panic!("node {n}"))
            .location
            .span;
        &text[s.start_byte as usize..s.end_byte as usize]
    }

    #[test]
    fn pend_with_a_trailing_comment_still_ends_the_proc() {
        // P2-1: the operation field (not the first token) decides the boundary, so `PEND` followed
        // by a comment is recognised; the PROC extent includes the PEND card and stops before S2.
        const FIX: &str =
            "//J JOB\n//P1 PROC\n//S1 EXEC PGM=A\n//         PEND  end of proc\n//S2 EXEC PGM=B\n";
        for eol in ["\n", "\r\n"] {
            let text = FIX.replace('\n', eol);
            let x = extract(&text);
            let p1 = slice_of(&text, &x, "P1");
            assert!(
                p1.contains("PEND  end of proc"),
                "PROC spans its PEND card: {p1:?}"
            );
            assert!(
                !p1.contains("S2"),
                "PROC stops before the next step: {p1:?}"
            );
        }
    }

    #[test]
    fn an_unnamed_exec_is_a_boundary_not_absorbed_by_the_prior_step() {
        // P2-1: a blank column 3 means no name, so `EXEC` is the operation and opens a new step.
        // The previous step's extent must end before the unnamed EXEC card.
        const FIX: &str = "//J JOB\n//S1 EXEC PGM=A\n//         EXEC PGM=B\n";
        for eol in ["\n", "\r\n"] {
            let text = FIX.replace('\n', eol);
            let x = extract(&text);
            let s1 = slice_of(&text, &x, "S1");
            assert_eq!(
                s1,
                format!("//S1 EXEC PGM=A{eol}"),
                "S1 ends before the unnamed EXEC"
            );
        }
    }

    #[test]
    fn a_continuation_card_is_not_read_as_an_exec_boundary() {
        // P2-1: `//S EXEC PGM=A,` continues (operand ends with a comma), so the next card is
        // operand/comment text — the word EXEC inside a PARM value must not open a new step.
        const FIX: &str =
            "//J JOB\n//S EXEC PGM=A,\n//           PARM='RUN EXEC AGAIN'\n//S2 EXEC PGM=B\n";
        for eol in ["\n", "\r\n"] {
            let text = FIX.replace('\n', eol);
            let x = extract(&text);
            let s = slice_of(&text, &x, "S");
            assert!(
                s.contains("PARM='RUN EXEC AGAIN'"),
                "the step spans its continuation card: {s:?}"
            );
            assert!(
                !s.contains("S2"),
                "the step stops before the next step: {s:?}"
            );
        }
    }

    #[test]
    fn dd_data_embedded_jcl_cards_do_not_end_the_enclosing_step() {
        // P2-2: inside `DD DATA`, records that begin with `//` are data until `/*`, so an embedded
        // JOB/EXEC must not bound the step; its extent runs through `/*` and stops before STEP2.
        const FIX: &str = "//J JOB\n//STEP1 EXEC PGM=X\n//SYSUT1 DD DATA\n//INNER JOB\n//RUN EXEC PGM=Z\n/*\n//STEP2 EXEC PGM=Y\n";
        for eol in ["\n", "\r\n"] {
            let text = FIX.replace('\n', eol);
            let x = extract(&text);
            let s1 = slice_of(&text, &x, "STEP1");
            assert!(
                s1.contains("//INNER JOB"),
                "embedded JOB stays inside the step: {s1:?}"
            );
            assert!(
                s1.contains("//RUN EXEC PGM=Z"),
                "embedded EXEC stays inside: {s1:?}"
            );
            assert!(
                s1.trim_end().ends_with("/*"),
                "the step runs through the delimiter: {s1:?}"
            );
            assert!(
                !s1.contains("STEP2"),
                "the step stops before the next real step: {s1:?}"
            );
        }
    }

    #[test]
    fn dd_data_with_dlm_ends_only_at_the_named_delimiter() {
        // P2-2: `DLM=@@` makes `@@` the delimiter, so even `/*` is data; the step runs to the `@@`
        // card and stops before STEP2.
        const FIX: &str = "//J JOB\n//STEP1 EXEC PGM=X\n//SYSIN DD DATA,DLM=@@\n//INNER JOB\n/*\n@@\n//STEP2 EXEC PGM=Y\n";
        for eol in ["\n", "\r\n"] {
            let text = FIX.replace('\n', eol);
            let x = extract(&text);
            let s1 = slice_of(&text, &x, "STEP1");
            assert!(
                s1.contains("//INNER JOB"),
                "embedded card is data under DLM: {s1:?}"
            );
            assert!(
                s1.contains("/*"),
                "`/*` is data, not the delimiter, under DLM: {s1:?}"
            );
            assert!(
                s1.contains("@@"),
                "the step spans through the DLM delimiter card: {s1:?}"
            );
            assert!(
                !s1.contains("STEP2"),
                "the step stops before the next real step: {s1:?}"
            );
        }
    }

    #[test]
    fn parse_dlm_reads_the_delimiter_or_defaults() {
        assert_eq!(parse_dlm("DATA,DLM=@@"), Some("@@".to_string()));
        assert_eq!(parse_dlm("*,DLM='$$'"), Some("$$".to_string()));
        assert_eq!(parse_dlm("DATA"), None);
        assert_eq!(parse_dlm("*"), None);
    }

    fn extract(text: &str) -> Extraction {
        JclExtractor::new()
            .extract(&SourceFile {
                path: "JOB1.jcl".to_string(),
                language: Language::new("jcl"),
                text: text.to_string(),
            })
            .expect("jcl extract")
    }

    #[test]
    fn parse_exec_target_variants() {
        assert_eq!(
            parse_exec_target("PGM=IEFBR14"),
            Some("IEFBR14".to_string())
        );
        assert_eq!(
            parse_exec_target("PGM=MYCOBOL,REGION=4M"),
            Some("MYCOBOL".to_string())
        );
        assert_eq!(
            parse_exec_target("MYPROC,PARM=X"),
            Some("MYPROC".to_string())
        );
        assert_eq!(parse_exec_target("COND=(0,NE)"), None);
    }

    #[test]
    fn job_and_step_nodes_emitted() {
        let ex = extract(
            "//PAYROLL JOB (ACCT),'RUN'\n//*comment\n//STEP1 EXEC PGM=PAYCALC\n//OUT DD SYSOUT=A\n",
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAYROLL" && n.kind == NodeKind::Module),
            "expected a JOB module node"
        );
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "STEP1" && n.kind == NodeKind::Other("step".to_string())),
            "expected a step node"
        );
    }

    #[test]
    fn exec_pgm_emits_calls_ref_to_program() {
        // The cross-language link: STEP1 → PAYCALC (a program, often a COBOL node).
        let ex = extract("//PAYROLL JOB\n//STEP1 EXEC PGM=PAYCALC\n");
        let call = ex
            .refs
            .iter()
            .find(|r| r.kind == EdgeKind::Calls && r.raw_name == "PAYCALC")
            .expect("expected a Calls ref STEP1 → PAYCALC");
        assert_eq!(call.kind, EdgeKind::Calls);
    }

    #[test]
    fn job_contains_step_edge() {
        let ex = extract("//J JOB\n//S1 EXEC PGM=P\n");
        assert!(
            ex.local_edges.iter().any(|e| e.kind == EdgeKind::Contains),
            "expected a job→step Contains edge"
        );
    }

    #[test]
    fn proc_invocation_emits_calls_edge_to_the_shared_proc_id() {
        // A cataloged PROC that is not indexed: the step still calls its shared id, a boundary.
        let ex = extract("//J JOB\n//S EXEC MYPROC\n");
        assert!(
            ex.local_edges
                .iter()
                .any(|e| e.target == proc_symbol("MYPROC") && e.kind == EdgeKind::Calls),
            "EXEC <proc> must call the proc"
        );
        let node = ex
            .nodes
            .iter()
            .find(|n| n.symbol == proc_symbol("MYPROC"))
            .expect("boundary PROC node");
        assert!(node.is_declaration());
    }

    #[test]
    fn comments_and_blank_cards_skipped() {
        let ex = extract("//*just a comment\n//\n/*\nnot a jcl line\n");
        assert!(ex.nodes.is_empty(), "no statements → no nodes");
    }

    #[test]
    fn parse_dsn_variants() {
        // Plain dataset, GDG generation, and PDS member — the parenthesized suffix stays whole.
        assert_eq!(
            parse_dsn("DSN=PROD.CUST.VSAM,DISP=SHR"),
            Some("PROD.CUST.VSAM".to_string())
        );
        assert_eq!(
            parse_dsn("DSN=PROD.GDG(+1),DISP=(NEW,CATLG)"),
            Some("PROD.GDG(+1)".to_string())
        );
        assert_eq!(
            parse_dsn("DSNAME=PROD.LIB(MEMBER),DISP=SHR"),
            Some("PROD.LIB(MEMBER)".to_string())
        );
        assert_eq!(parse_dsn("SYSOUT=A"), None);
    }

    #[test]
    fn dd_dsn_emits_dataset_node_and_uses_edge() {
        // VSAM/GDG/PDS coverage: a DD DSN= becomes a dataset node + a step→dataset "uses" edge.
        let ex = extract("//J JOB\n//S1 EXEC PGM=READER\n//IN DD DSN=PROD.CUST.VSAM,DISP=SHR\n");
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.kind == NodeKind::Other("dataset".to_string())
                    && n.name == "PROD.CUST.VSAM"),
            "expected a dataset node for the DD DSN",
        );
        assert!(
            ex.local_edges
                .iter()
                .any(|e| e.kind == EdgeKind::Other("uses".to_string())),
            "expected a step→dataset uses edge",
        );
    }

    #[test]
    fn instream_proc_is_a_module_that_contains_its_steps() {
        // Fails on main: `//PAYPROC PROC` emitted nothing, so `EXEC PAYPROC` had no target.
        let ex = extract(
            "//PAYJOB JOB (A)\n//PAYPROC PROC\n//PSTEP EXEC PGM=STUB\n//         PEND\n//STEP2 EXEC PAYPROC\n",
        );
        let proc_node = ex
            .nodes
            .iter()
            .find(|n| n.name == "PAYPROC" && n.kind == NodeKind::Other("jcl_proc".into()))
            .expect("PROC node");
        assert!(!proc_node.is_declaration(), "the in-stream PROC defines it");
        let pstep = ex.nodes.iter().find(|n| n.name == "PSTEP").unwrap();
        let step2 = ex.nodes.iter().find(|n| n.name == "STEP2").unwrap();
        let contains = |from: &SymbolId, to: &SymbolId| {
            ex.local_edges
                .iter()
                .any(|e| &e.source == from && &e.target == to && e.kind == EdgeKind::Contains)
        };
        assert!(
            contains(&proc_node.symbol, &pstep.symbol),
            "PROC contains its step"
        );
        let job = ex.nodes.iter().find(|n| n.name == "PAYJOB").unwrap();
        assert!(
            contains(&job.symbol, &step2.symbol),
            "after PEND, steps belong to the job"
        );
        assert!(ex.local_edges.iter().any(|e| e.source == step2.symbol
            && e.target == proc_node.symbol
            && e.kind == EdgeKind::Calls));
    }

    #[test]
    fn dd_in_a_pgm_step_defines_the_shared_dd_binding() {
        let ex = extract(
            "//J JOB\n//S1 EXEC PGM=PAYCALC\n//PAYIN DD DSN=PROD.PAY.INPUT,DISP=SHR\n//SYSOUT DD SYSOUT=*\n//S2 EXEC MYPROC\n//X DD DSN=A.B\n",
        );
        let bind = crate::cobol_links::dd_binding_symbol("PAYCALC", "PAYIN");
        let node = ex
            .nodes
            .iter()
            .find(|n| n.symbol == bind)
            .expect("dd binding");
        assert!(!node.is_declaration());
        assert_eq!(node.name, "PAYCALC.PAYIN");
        let ds = Symbol::synthetic("jcl-dataset", "PROD.PAY.INPUT").id();
        assert!(ex.local_edges.iter().any(|e| e.source == bind
            && e.target == ds
            && e.kind == EdgeKind::Other("uses".into())));
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.symbol == crate::cobol_links::dd_binding_symbol("PAYCALC", "SYSOUT"))
        );
        assert_eq!(
            ex.nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Other("dd_binding".into()))
                .count(),
            2,
            "a PROC-invocation step has no program to bind"
        );
    }

    #[test]
    fn concatenated_dd_and_proc_keyword_and_located_edges() {
        let ex = extract(
            "//J JOB\n//S1 EXEC PGM=P\n//IN DD DSN=A.ONE,DISP=SHR\n//         DD DSN=A.TWO,DISP=SHR\n//S2 EXEC PROC=PAYPROC\n",
        );
        let bind = crate::cobol_links::dd_binding_symbol("P", "IN");
        for dsn in ["A.ONE", "A.TWO"] {
            let ds = Symbol::synthetic("jcl-dataset", dsn).id();
            assert!(
                ex.local_edges
                    .iter()
                    .any(|e| e.source == bind && e.target == ds),
                "the binding uses {dsn}"
            );
        }
        assert!(
            ex.local_edges
                .iter()
                .any(|e| e.target == proc_symbol("PAYPROC") && e.kind == EdgeKind::Calls),
            "EXEC PROC=PAYPROC"
        );
        // Every edge carries the card's location, so `remove_file` retires it with the JCL even
        // when both endpoints are shared nodes that survive.
        assert!(
            ex.local_edges
                .iter()
                .all(|e| e.location.as_ref().is_some_and(|l| l.file == "JOB1.jcl"))
        );
    }

    #[test]
    fn a_proc_named_like_a_program_never_shadows_exec_pgm() {
        // A PROC named after the program it runs is a common convention. `EXEC PGM=PAYCALC` must
        // stay a by-name ref to the program, and no PROC node may be a Module homonym of it.
        let ex = extract(
            "//PAYCALC PROC\n//S EXEC PGM=PAYCALC\n//         PEND\n//J JOB\n//S1 EXEC PAYCALC\n",
        );
        assert!(
            ex.nodes
                .iter()
                .all(|n| !(n.name == "PAYCALC" && n.kind == NodeKind::Module))
        );
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "PAYCALC" && r.kind == EdgeKind::Calls)
        );
        assert!(
            ex.local_edges
                .iter()
                .any(|e| e.target == proc_symbol("PAYCALC") && e.kind == EdgeKind::Calls)
        );
    }
}
