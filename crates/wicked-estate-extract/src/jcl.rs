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
//! DD/DSN dataset references are intentionally out of scope here (the program call graph is the
//! value); they are a clean follow-up on the same seam.

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
        let mut current_step: Option<SymbolId> = None;

        for (i, raw) in file.text.lines().enumerate() {
            // JCL statements begin in columns 1-2 with `//`. `//*` is a comment; `//` alone is a
            // null/delimiter statement; `/*` is end-of-data.
            let Some(body) = raw.strip_prefix("//") else {
                continue;
            };
            if body.starts_with('*') || body.trim().is_empty() {
                continue;
            }

            // Card form: NAME OPERATION OPERANDS  (whitespace-separated).
            let mut toks = body.split_whitespace();
            let Some(name) = toks.next() else { continue };
            let Some(op) = toks.next() else { continue };
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
                        loc,
                    );
                    node.signature = Some("JOB".to_string());
                    nodes.push(node);
                    current_job = Some(sym);
                }
                "EXEC" => {
                    let step_sym =
                        Symbol::synthetic("jcl", format!("{}::{}::{}", file.path, name, i)).id();
                    let mut node = Node::new(
                        step_sym.clone(),
                        NodeKind::Other("step".to_string()),
                        name.to_string(),
                        Language::new("jcl"),
                        loc.clone(),
                    );
                    node.signature = Some(format!("EXEC {operands}"));
                    nodes.push(node);
                    current_step = Some(step_sym.clone());

                    // job → step (Contains): blast-radius/containment.
                    if let Some(job) = &current_job {
                        local_edges.push(Edge::new(
                            job.clone(),
                            step_sym.clone(),
                            EdgeKind::Contains,
                            ResolutionTier::Parsed,
                            "jcl",
                        ));
                    }

                    // step → invoked program/PROC (Calls): the cross-language link.
                    if let Some(target) = parse_exec_target(operands) {
                        refs.push(UnresolvedRef::new(
                            step_sym.clone(),
                            target,
                            EdgeKind::Calls,
                            loc,
                        ));
                    }
                }
                "DD" => {
                    // DD DSN=dataset → the step uses a dataset (VSAM/GDG/PDS/QSAM). The DSN value
                    // (including a GDG `(+1)` or PDS `(MEMBER)` suffix) becomes a SHARED dataset
                    // node, with a step→dataset edge (source = dependent step, target = dependency
                    // dataset) so blast-radius on a dataset finds every job/step that touches it.
                    if let Some(dsn) = parse_dsn(operands) {
                        let ds_sym = Symbol::synthetic("jcl-dataset", dsn.as_str()).id();
                        if let Some(from) = current_step.clone().or_else(|| current_job.clone()) {
                            local_edges.push(Edge::new(
                                from,
                                ds_sym.clone(),
                                EdgeKind::Other("uses".to_string()),
                                ResolutionTier::Parsed,
                                "jcl",
                            ));
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

        Ok(Extraction {
            nodes,
            local_edges,
            refs,
        })
    }
}

/// One-line span anchor (JCL is line-oriented; byte offsets within the card are not tracked).
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

/// From an `EXEC` operand string, return the invoked program or PROC name.
/// `PGM=MYCOBOL,REGION=4M` → `MYCOBOL` ; `MYPROC,PARM=…` → `MYPROC` ; `PROC=X` / keyword-only → None.
fn parse_exec_target(operands: &str) -> Option<String> {
    let first = operands.split(',').next()?.trim();
    if let Some(pgm) = first.strip_prefix("PGM=") {
        let p = pgm.trim();
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn proc_invocation_emits_calls_ref() {
        let ex = extract("//J JOB\n//S EXEC MYPROC\n");
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "MYPROC" && r.kind == EdgeKind::Calls),
            "EXEC <proc> must emit a Calls ref to the proc"
        );
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
}
