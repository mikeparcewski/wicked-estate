//! JCL (Job Control Language) extractor — **logical statements, NO tree-sitter grammar**.
//!
//! No tree-sitter grammar exists for JCL (or RPG / PL-I / Natural / REXX / HLASM), and authoring one
//! would not help: JCL is a fixed *card* format, not an expression grammar — borrowing a JS/Python
//! grammar transfers nothing. But the [`Extractor`] seam is generic (tree-sitter is just one impl;
//! `IaCExtractor`/`TfstateCollector` already parse without a grammar), so JCL is a first-class
//! language via a logical-statement reader. This is the pattern for grammar-less legacy formats.
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

/// Extractor for JCL job streams. Grammar-less; reads logical statements from `//` cards.
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
        let mut job_name = "";
        let mut proc_name = None;
        let mut step_counts = std::collections::HashMap::new();
        let mut current_step: Option<SymbolId> = None;
        let mut current_pgm: Option<String> = None;
        // The DD binding the next unnamed concatenation card adds a dataset to.
        let mut last_dd: Option<SymbolId> = None;
        let statements = read_statements(&file.text);
        let extents = Extents::scan(&file.text, &statements);

        for statement in &statements {
            let i = statement.first_line;
            let name = statement.name;
            let op = statement.operation;
            let operands = statement.operands.as_str();
            // An unnamed `//         DD DSN=…` card continues the previous DD (a concatenation).
            if name.is_empty() && op == "DD" {
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
                    job_name = name;
                    current_proc = None;
                    proc_name = None;
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
                    proc_name = Some(name);
                    current_step = None;
                    current_pgm = None;
                    last_dd = None;
                }
                "PEND" => {
                    current_proc = None;
                    proc_name = None;
                    current_step = None;
                    current_pgm = None;
                    last_dd = None;
                }
                "EXEC" => {
                    let target = parse_exec_target(operands);
                    let name = if name.is_empty() {
                        target.as_deref().unwrap_or("EXEC")
                    } else {
                        name
                    };
                    let owner = proc_name.unwrap_or(job_name);
                    let count = step_counts
                        .entry((owner, name.to_string()))
                        .or_insert(0usize);
                    *count += 1;
                    let suffix = if *count == 1 {
                        String::new()
                    } else {
                        format!("#{count}")
                    };
                    let step_sym =
                        Symbol::synthetic("jcl", format!("{}::{owner}::{name}{suffix}", file.path))
                            .id();
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
                    let first = split_operands(operands)[0];
                    let program = operand_value(first, &["PGM"]);
                    current_pgm = program
                        .as_deref()
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
                    if let Some(target) = target {
                        if program.is_some() {
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

/// A complete statement, excluding any following in-stream data records.
struct Statement<'a> {
    name: &'a str,
    operation: &'a str,
    operands: String,
    first_line: usize,
    last_line: usize,
}

#[derive(Clone, Copy)]
enum Continuation {
    Operands,
    String,
    Comment,
}

/// Statement text occupies columns 1-71; column 72 and sequence columns are separate.
fn card_text(raw: &str) -> &str {
    let mut end = raw.len().min(71);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    &raw[..end]
}

fn word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (&text[..end], text[end..].trim_start())
}

/// Read through the first blank outside a quoted string. Doubled apostrophes escape an apostrophe.
fn operand_field(text: &str, mut quoted: bool) -> (&str, bool) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if quoted && bytes.get(i + 1) == Some(&b'\'') => i += 2,
            b'\'' => {
                quoted = !quoted;
                i += 1;
            }
            b if b.is_ascii_whitespace() && !quoted => break,
            _ => i += 1,
        }
    }
    (&text[..i], quoted)
}

fn continuation(raw: &str, operand: &str, quoted: bool) -> Option<Continuation> {
    if quoted && card_text(raw).len() == 71 {
        Some(Continuation::String)
    } else if !quoted && operand.ends_with(',') {
        Some(Continuation::Operands)
    } else if raw
        .as_bytes()
        .get(71)
        .is_some_and(|b| !b.is_ascii_whitespace())
    {
        Some(Continuation::Comment)
    } else {
        None
    }
}

/// Read logical statements for extraction and extents, excluding in-stream data records.
/// `DD DATA` ends only at its delimiter. `DD *` ends at its delimiter or the next `//` card,
/// even with `DLM=` (JES2); that `//` card is read as a real statement. Columns 72-80 are not
/// statement text: column 72 can mark continuation, and columns 73-80 hold sequence numbers.
fn read_statements(text: &str) -> Vec<Statement<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut statements = Vec::new();
    let mut in_stream: Option<(String, bool)> = None;
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        if let Some((delimiter, slashes_are_data)) = &in_stream {
            if raw.starts_with(delimiter.as_str()) {
                in_stream = None;
                i += 1;
                continue;
            }
            if *slashes_are_data || !raw.starts_with("//") {
                i += 1;
                continue;
            }
            in_stream = None;
        }
        let Some(body) = card_text(raw).strip_prefix("//") else {
            i += 1;
            continue;
        };
        if body.starts_with('*') || body.trim().is_empty() {
            i += 1;
            continue;
        }
        let (name, rest) = if body.starts_with([' ', '\t']) {
            ("", body)
        } else {
            word(body)
        };
        let (operation, rest) = word(rest);
        let (operand, quoted) = operand_field(rest, false);
        let mut statement = Statement {
            name,
            operation,
            operands: operand.to_string(),
            first_line: i,
            last_line: i,
        };
        let mut pending = continuation(raw, operand, quoted);
        i += 1;
        while let Some(mode) = pending {
            // Comments between continuation cards do not terminate the logical statement.
            while i < lines.len() && lines[i].starts_with("//*") {
                i += 1;
            }
            let Some(&next) = lines.get(i) else {
                break;
            };
            let next_text = card_text(next);
            let Some(body) = next_text.strip_prefix("//") else {
                break;
            };
            if !body.starts_with([' ', '\t']) || body.trim().is_empty() {
                break;
            }
            if matches!(mode, Continuation::Comment) {
                pending = next
                    .as_bytes()
                    .get(71)
                    .filter(|b| !b.is_ascii_whitespace())
                    .map(|_| Continuation::Comment);
            } else {
                let field = if matches!(mode, Continuation::String) {
                    next_text.get(15..).unwrap_or("")
                } else {
                    body.trim_start()
                };
                let (operand, quoted) = operand_field(field, matches!(mode, Continuation::String));
                statement.operands.push_str(operand);
                pending = continuation(next, operand, quoted);
            }
            statement.last_line = i;
            i += 1;
        }
        if operation == "DD" {
            let parts = split_operands(&statement.operands);
            if let Some(&first) = parts.first() {
                if first == "*" || first == "DATA" {
                    in_stream = Some((
                        parse_dlm(&statement.operands).unwrap_or_else(|| "/*".to_string()),
                        first == "DATA",
                    ));
                }
            }
        }
        statements.push(statement);
    }
    statements
}

/// The statement kinds that bound a node's extent.
#[derive(Clone, Copy, PartialEq)]
enum Stmt {
    Job,
    Proc,
    Exec,
    Pend,
}

/// Byte offsets and JOB/PROC/EXEC/PEND boundaries from the shared logical-statement reader.
/// Nodes span their continuations, DDs, in-stream data and comments; the reader handles data
/// delimiters before these boundaries are collected.
struct Extents {
    line_starts: Vec<usize>,
    /// Byte offset where each line's content ends (before its terminator).
    line_ends: Vec<usize>,
    text_len: usize,
    bounds: Vec<(usize, usize, Stmt)>,
}

impl Extents {
    fn scan(text: &str, statements: &[Statement<'_>]) -> Self {
        let mut line_starts = Vec::new();
        let mut line_ends = Vec::new();
        let mut pos = 0;
        for raw in text.lines() {
            line_starts.push(pos);
            // `lines()` strips `\n` or `\r\n`; advance past the real terminator.
            pos += raw.len();
            line_ends.push(pos);
            if text[pos..].starts_with("\r\n") {
                pos += 2;
            } else if text[pos..].starts_with('\n') {
                pos += 1;
            }
        }
        let bounds = statements
            .iter()
            .filter_map(|statement| {
                let kind = match statement.operation {
                    "JOB" => Stmt::Job,
                    "PROC" => Stmt::Proc,
                    "EXEC" => Stmt::Exec,
                    "PEND" => Stmt::Pend,
                    _ => return None,
                };
                Some((statement.first_line, statement.last_line, kind))
            })
            .collect();
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
            .filter(|(first, _, _)| *first > line)
            .find_map(|&(first, last, k)| match (kind, k) {
                (Stmt::Exec, _) => Some(first),
                (_, Stmt::Job | Stmt::Proc) => Some(first),
                (Stmt::Proc, Stmt::Pend) => Some(last + 1),
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

/// Split only at commas outside strings and parentheses, preserving quoted values verbatim.
fn split_operands(operands: &str) -> Vec<&str> {
    let bytes = operands.as_bytes();
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if quoted && bytes.get(i + 1) == Some(&b'\'') => {
                i += 2;
                continue;
            }
            b'\'' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => depth = depth.saturating_sub(1),
            b',' if !quoted && depth == 0 => {
                parts.push(operands[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(operands[start..].trim());
    parts
}

/// Decode a whole quoted value, including JCL's doubled-apostrophe escape.
fn operand_value(operands: &str, keys: &[&str]) -> Option<String> {
    split_operands(operands).into_iter().find_map(|part| {
        let (key, value) = part.split_once('=')?;
        if !keys.contains(&key.trim()) {
            return None;
        }
        let value = value.trim();
        let quoted = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\''));
        let value = if let Some(quoted) = quoted {
            quoted.replace("''", "'")
        } else {
            value.to_string()
        };
        (!value.is_empty()).then_some(value)
    })
}

/// The EXEC target comes only from its first operand: `PGM=`, `PROC=`, or a bare PROC name.
/// Later keyword operands are overrides and cannot select the target.
fn parse_exec_target(operands: &str) -> Option<String> {
    let first = *split_operands(operands).first()?;
    operand_value(first, &["PGM", "PROC"])
        .or_else(|| (!first.is_empty() && !first.contains('=')).then(|| first.to_string()))
}

/// A DD's dataset name, preserving parenthesized GDG generations and PDS members.
fn parse_dsn(operands: &str) -> Option<String> {
    operand_value(operands, &["DSN", "DSNAME"])
}

/// A DD's explicit in-stream delimiter; the reader supplies `/*` when absent.
fn parse_dlm(operands: &str) -> Option<String> {
    operand_value(operands, &["DLM"])
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

    fn assert_first_step_slice(prefix: &str, eol: &str) -> Extraction {
        let expected = prefix.replace('\n', eol);
        let text = format!("//J JOB{eol}{expected}//S2 EXEC PGM=B{eol}");
        let x = extract(&text);
        assert_eq!(slice_of(&text, &x, "S1"), expected);
        assert_eq!(slice_of(&text, &x, "S2"), format!("//S2 EXEC PGM=B{eol}"));
        assert_eq!(
            x.nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Other("step".into()))
                .count(),
            2
        );
        x
    }

    #[test]
    fn quoted_exec_operands_and_column_16_string_continuation() {
        let first = "//S1 EXEC PGM=A,PARM='";
        let continued = format!(
            "{first}{}\n//* between continuation cards\n//             WORLD'\n//OUT DD SYSOUT=*\n",
            "H".repeat(71 - first.len())
        );
        for eol in ["\n", "\r\n"] {
            for (prefix, parm) in [
                (
                    "//S1 EXEC PGM=A,PARM='HELLO, WORLD'\n//OUT DD SYSOUT=*\n",
                    "'HELLO, WORLD'".to_string(),
                ),
                (
                    continued.as_str(),
                    format!("'{}WORLD'", "H".repeat(71 - first.len())),
                ),
            ] {
                let x = assert_first_step_slice(prefix, eol);
                let s1 = x.nodes.iter().find(|n| n.name == "S1").unwrap();
                assert_eq!(s1.signature, Some(format!("EXEC PGM=A,PARM={parm}")));
            }
        }
    }

    #[test]
    fn column_72_comment_continuation_does_not_open_a_step() {
        let card = format!("{:<71}X", "//S1 EXEC PGM=A  trailing comment");
        let prefix = format!("{card}\n//         EXEC will run tomorrow\n//OUT DD SYSOUT=*\n");
        for eol in ["\n", "\r\n"] {
            let x = assert_first_step_slice(&prefix, eol);
            assert!(!x.nodes.iter().any(|n| n.name == "EXEC"));
        }
    }

    #[test]
    fn continued_dd_data_delimiter_skips_embedded_job() {
        let prefix = concat!(
            "//S1 EXEC PGM=A\n//SYSIN DD DATA,\n//         DLM=@@\n",
            "//INNER JOB\n/*\n//AFTER JOB\n@@\n"
        );
        for eol in ["\n", "\r\n"] {
            let x = assert_first_step_slice(prefix, eol);
            assert!(
                !x.nodes
                    .iter()
                    .any(|n| n.name == "INNER" || n.name == "AFTER")
            );
        }
    }

    #[test]
    fn quoted_comma_dd_data_delimiter() {
        let prefix =
            "//S1 EXEC PGM=A\n//SYSIN DD DATA,DLM=',,'\n//INNER JOB\n/*\n//AFTER JOB\n,,\n";
        for eol in ["\n", "\r\n"] {
            let x = assert_first_step_slice(prefix, eol);
            assert!(
                !x.nodes
                    .iter()
                    .any(|n| n.name == "INNER" || n.name == "AFTER")
            );
        }
    }

    #[test]
    fn dd_star_with_custom_delimiter_ends_at_next_statement() {
        let prefix = "//S1 EXEC PGM=A\n//SYSIN DD *,DLM=@@\ndata\n";
        for eol in ["\n", "\r\n"] {
            assert_first_step_slice(prefix, eol);
        }
    }

    #[test]
    fn statement_reader_preserves_documented_data_and_column_boundaries() {
        for eol in ["\n", "\r\n"] {
            for (operands, data, expected) in [
                ("DATA", "//INNER EXEC PGM=X\n/*\n", vec!["SYSIN", "S"]),
                (
                    "DATA,DLM=@@",
                    "//INNER EXEC PGM=X\n/*\n@@\n",
                    vec!["SYSIN", "S"],
                ),
                ("*", "data\n/*\n", vec!["SYSIN", "S"]),
                ("*,DLM=@@", "data\n@@\n", vec!["SYSIN", "S"]),
                ("*", "//INNER EXEC PGM=X\n", vec!["SYSIN", "INNER", "S"]),
                (
                    "*,DLM=@@",
                    "//INNER EXEC PGM=X\n",
                    vec!["SYSIN", "INNER", "S"],
                ),
            ] {
                let prefix = "//S EXEC PGM=";
                let program = "A".repeat(71 - prefix.len());
                let text = format!("//SYSIN DD {operands}\n{data}{prefix}{program}X12345678\n")
                    .replace('\n', eol);
                let statements = read_statements(&text);
                assert_eq!(
                    statements.iter().map(|s| s.name).collect::<Vec<_>>(),
                    expected,
                    "{operands} under {eol:?}"
                );
                let last = statements.last().unwrap();
                assert_eq!(last.operation, "EXEC");
                assert_eq!(last.operands, format!("PGM={program}"));
            }
        }
    }

    #[test]
    fn continued_dsn_links_step_and_dd_binding_to_dataset() {
        let prefix = "//S1 EXEC PGM=A\n//PRIMARY DD DISP=SHR,\n//         DSN=A.B.C\n";
        for eol in ["\n", "\r\n"] {
            let x = assert_first_step_slice(prefix, eol);
            let step = x.nodes.iter().find(|n| n.name == "S1").unwrap();
            let dataset = x.nodes.iter().find(|n| n.name == "A.B.C").unwrap();
            let binding = crate::cobol_links::dd_binding_symbol("A", "PRIMARY");
            assert_eq!(dataset.location.span, line_span(2));
            let dd = x.nodes.iter().find(|n| n.symbol == binding).unwrap();
            assert_eq!(dd.location.span, line_span(2));
            for from in [&step.symbol, &binding] {
                let edge = x
                    .local_edges
                    .iter()
                    .find(|e| {
                        &e.source == from
                            && e.target == dataset.symbol
                            && e.kind == EdgeKind::Other("uses".into())
                    })
                    .expect("uses edge from step and binding");
                assert_eq!(edge.location.as_ref().unwrap().span, line_span(2));
            }
        }
    }

    #[test]
    fn operand_splitter_respects_quotes_escapes_and_parentheses() {
        assert_eq!(
            split_operands("PGM=A,PARM='HELLO, WORLD',COND=(0,NE,(1,EQ)),DLM=',,'"),
            vec![
                "PGM=A",
                "PARM='HELLO, WORLD'",
                "COND=(0,NE,(1,EQ))",
                "DLM=',,'",
            ]
        );
        assert_eq!(
            split_operands("PARM='IT''S, HERE',DSN=A.B"),
            vec!["PARM='IT''S, HERE'", "DSN=A.B"]
        );
        assert_eq!(parse_dlm("DATA,DLM=',,'"), Some(",,".into()));
        assert_eq!(parse_dlm("DATA,DLM='''@'"), Some("'@".into()));
        assert_eq!(parse_dsn("PARM='DSN=WRONG',DSNAME=A.B"), Some("A.B".into()));
        assert_eq!(parse_dsn("XDSN=WRONG"), None);
        assert_eq!(
            parse_exec_target("PGM=A,PARM='PGM=WRONG'"),
            Some("A".into())
        );
        assert_eq!(parse_exec_target("MYPROC,PGM=X"), Some("MYPROC".into()));
        assert_eq!(
            parse_exec_target("PROC=MYPROC,PGM=X"),
            Some("MYPROC".into())
        );
        assert_eq!(parse_exec_target("MYPROC,PROC=X"), Some("MYPROC".into()));
        assert_eq!(parse_exec_target("PARM='PGM=WRONG',PGM=A"), None);
    }

    #[test]
    fn step_ids_survive_comment_insertion_and_disambiguate_within_owner() {
        let original = "//J JOB\n//S EXEC PGM=A\n";
        let commented = "//J JOB\n//* comment\n//S EXEC PGM=A\n";
        let repeated = concat!(
            "//S EXEC PGM=A\n//J JOB\n//S EXEC PGM=A\n//S EXEC PGM=B\n",
            "//S EXEC PGM=C\n//P PROC\n//S EXEC PGM=A\n"
        );
        for eol in ["\n", "\r\n"] {
            let ids = |text: &str| {
                extract(&text.replace('\n', eol))
                    .nodes
                    .into_iter()
                    .filter(|n| n.kind == NodeKind::Other("step".into()))
                    .map(|n| n.symbol)
                    .collect::<Vec<_>>()
            };
            assert_eq!(ids(original), ids(commented));
            assert_eq!(
                ids(repeated),
                ["", "J", "J", "J", "P"]
                    .into_iter()
                    .zip(["S", "S", "S#2", "S#3", "S"])
                    .map(|(owner, step)| {
                        Symbol::synthetic("jcl", format!("JOB1.jcl::{owner}::{step}")).id()
                    })
                    .collect::<Vec<_>>()
            );
        }
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
            let steps: Vec<_> = x
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Other("step".into()))
                .collect();
            assert_eq!(
                steps.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
                ["S1", "B"]
            );
            let b = steps[1];
            assert_eq!(b.symbol, Symbol::synthetic("jcl", "JOB1.jcl::J::B").id());
            assert!(
                x.refs
                    .iter()
                    .any(|r| r.from == b.symbol && r.raw_name == "B" && r.kind == EdgeKind::Calls)
            );
            assert!(x.nodes.iter().all(|n| !n.name.is_empty()));
        }
    }

    #[test]
    fn unnamed_exec_display_names_share_the_step_id_suffix_rule() {
        let x = extract(concat!(
            "//J JOB\n//B EXEC PGM=A\n//         EXEC PGM=B\n//         EXEC PGM=B\n",
            "//         EXEC MYPROC\n//         EXEC PROC=MYPROC\n//         EXEC COND=(0,NE)\n"
        ));
        let steps: Vec<_> = x
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("step".into()))
            .collect();
        assert_eq!(steps.len(), 6);
        for (step, (name, id_name)) in steps.iter().zip([
            ("B", "B"),
            ("B", "B#2"),
            ("B", "B#3"),
            ("MYPROC", "MYPROC"),
            ("MYPROC", "MYPROC#2"),
            ("EXEC", "EXEC"),
        ]) {
            assert_eq!(step.name, name);
            assert_eq!(
                step.symbol,
                Symbol::synthetic("jcl", format!("JOB1.jcl::J::{id_name}")).id()
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
    fn proc_symbolic_overrides_do_not_select_the_exec_target_or_dd_program() {
        for operands in ["MYPROC,PGM=X", "PROC=MYPROC,PGM=X", "MYPROC,PROC=X"] {
            let ex = extract(&format!("//J JOB\n//S EXEC {operands}\n//IN DD DSN=A.B\n"));
            let step = ex.nodes.iter().find(|n| n.name == "S").unwrap();
            assert!(ex.local_edges.iter().any(|e| e.source == step.symbol
                && e.target == proc_symbol("MYPROC")
                && e.kind == EdgeKind::Calls));
            assert!(!ex.refs.iter().any(|r| r.kind == EdgeKind::Calls));
            assert!(
                !ex.nodes
                    .iter()
                    .any(|n| n.kind == NodeKind::Other("dd_binding".into()))
            );
        }
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
