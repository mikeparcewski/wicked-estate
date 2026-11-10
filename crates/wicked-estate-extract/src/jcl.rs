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
                        loc,
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
                    let sym =
                        Symbol::synthetic("jcl", format!("{}::PROC::{}", file.path, name)).id();
                    let mut node = Node::new(
                        sym.clone(),
                        NodeKind::Module,
                        name.to_string(),
                        Language::new("jcl"),
                        loc.clone(),
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
                        loc.clone(),
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

    #[test]
    fn instream_proc_is_a_module_that_contains_its_steps() {
        // Fails on main: `//PAYPROC PROC` emitted nothing, so `EXEC PAYPROC` had no target.
        let ex = extract(
            "//PAYJOB JOB (A)\n//PAYPROC PROC\n//PSTEP EXEC PGM=STUB\n//         PEND\n//STEP2 EXEC PAYPROC\n",
        );
        let proc_node = ex
            .nodes
            .iter()
            .find(|n| n.name == "PAYPROC" && n.kind == NodeKind::Module)
            .expect("PROC module");
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
        assert!(
            ex.refs
                .iter()
                .any(|r| r.from == step2.symbol && r.raw_name == "PAYPROC")
        );
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
            ex.refs.iter().any(|r| r.raw_name == "PAYPROC"),
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
}
