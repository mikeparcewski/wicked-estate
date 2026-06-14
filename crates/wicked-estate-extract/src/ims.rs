//! IMS DBD/PSB extractor — **line/card-oriented, NO tree-sitter grammar**.
//!
//! IMS (Information Management System) source is macro-assembler style: a fixed card format
//! `LABEL  MACRO  KEYWORD=VALUE,...` — the same column convention as HLASM.  No tree-sitter
//! grammar exists and none is needed: the semantics are entirely in the macro opcode + keyword
//! operands.  This extractor covers the data-estate mapping path (ADR-004):
//!
//! | Macro            | What it maps                      | Node kind          |
//! |------------------|-----------------------------------|--------------------|
//! | `DBD`            | IMS database definition           | `ims_database`     |
//! | `SEGM`           | Segment (record type) in a DBD    | `ims_segment`      |
//! | `PCB TYPE=DB`    | PSB program→database access       | (ref only)         |
//! | `SENSEG`         | PSB view: sensitive segment       | (ref only)         |
//!
//! # Card format
//!
//! `*` in column 1 = comment (same as HLASM).  A label is present iff column 1 is non-blank.
//! The macro opcode is the **second whitespace token** if a label is present, else the **first**.
//! Operands follow as a single `KW=VALUE,...` token.
//!
//! # KEYWORD=VALUE parsing
//!
//! Values may be bare identifiers (`NAME=CUSTDB`), parenthesized expressions
//! (`PARENT=((SEG,SNGL))`), or quoted strings.  For the estate-mapping purpose we need only the
//! *first identifier token* inside the value.  A single [`LazyLock`] regex handles all forms.
//!
//! # IMS PSB anchoring
//!
//! PSBs are source files that contain one or more `PCB`/`SENSEG` macros but no `DBD` macro.  The
//! "from" anchor for PSB-originated refs is `Symbol::synthetic("ims-psb", &file.path)` — PSB
//! granularity is sufficient because a PSB always maps to a single COBOL/PL-I program.

use wicked_estate_core::{
    Edge, EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, ResolutionTier,
    Result, SourceFile, Span, Symbol, SymbolId, UnresolvedRef,
};
use regex::Regex;
use std::sync::LazyLock;

// ── Compiled regex ────────────────────────────────────────────────────────────

/// Extracts the first identifier token from any `KEYWORD=<value>` value string.
///
/// Handles:
/// - bare identifier:          `CUSTDB`              → `CUSTDB`
/// - parenthesized expression: `((SEG,SNGL))`        → `SEG`
/// - parenthesized single:     `(CUSTOMER)`          → `CUSTOMER`
/// - `PARENT=0` / `PARENT=*`  → returns `"0"` / `"*"` (caller filters these)
///
/// The pattern `[A-Za-z0-9#$@_-]+` covers IMS names (alphanumeric + common mainframe specials).
static RE_FIRST_IDENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9#$@_*-]+").expect("RE_FIRST_IDENT must compile"));

// ── Extractor ─────────────────────────────────────────────────────────────────

/// Extractor for IMS DBD (Database Definition) and PSB (Program Specification Block) source.
/// Grammar-less; parses the macro-card format line by line.
pub struct ImsExtractor;

impl ImsExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ImsExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for ImsExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("ims")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut local_edges: Vec<Edge> = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();

        // Track the most-recently-opened DBD so we can emit DBD→SEGM Contains edges.
        let mut current_dbd: Option<SymbolId> = None;

        // The PSB-level anchor (lazy — created on first PCB/SENSEG in a file without a DBD).
        let psb_anchor = || Symbol::synthetic("ims-psb", file.path.as_str()).id();

        for (i, raw) in file.text.lines().enumerate() {
            // `*` in column 1 is a comment line; blank lines are skipped.
            if raw.starts_with('*') || raw.trim().is_empty() {
                continue;
            }

            // The macro keyword is a known IMS verb; an optional label precedes it (col 1) and real
            // source indents inconsistently, so locate the macro BY KEYWORD rather than by column.
            const IMS_MACROS: &[&str] = &[
                "DBD", "SEGM", "PCB", "SENSEG", "FIELD", "LCHILD", "DATASET", "PSBGEN", "DBDGEN",
                "XDFLD", "AREA",
            ];
            let toks: Vec<&str> = raw.split_whitespace().collect();
            let Some(idx) = toks
                .iter()
                .position(|t| IMS_MACROS.contains(&t.to_ascii_uppercase().as_str()))
            else {
                continue;
            };
            let macro_op = toks[idx];
            let operands = toks.get(idx + 1).copied().unwrap_or("");
            let loc = Location::new(&file.path, line_span(i));

            match macro_op.to_ascii_uppercase().as_str() {
                // ── DBD ─────────────────────────────────────────────────────
                // `DBD  NAME=<db>,...`
                // Emits a node of kind `ims_database`; becomes the current DBD for SEGM scoping.
                "DBD" => {
                    if let Some(db_name) = kw_value(operands, "NAME") {
                        let sym =
                            Symbol::synthetic("ims-dbd", format!("{}::{}", file.path, db_name))
                                .id();
                        let mut node = Node::new(
                            sym.clone(),
                            NodeKind::Other("ims_database".to_string()),
                            db_name,
                            Language::new("ims"),
                            loc,
                        );
                        node.signature = Some("DBD".to_string());
                        nodes.push(node);
                        current_dbd = Some(sym);
                    }
                }

                // ── SEGM ────────────────────────────────────────────────────
                // `SEGM  NAME=<seg>,PARENT=<p>,...`
                // Emits a node of kind `ims_segment`.  If PARENT is present and not `0`/`*`,
                // emits an UnresolvedRef from segment → parent with EdgeKind::Other("parent").
                // Also emits a Contains local edge from the current DBD → segment.
                "SEGM" => {
                    let Some(seg_name) = kw_value(operands, "NAME") else {
                        continue;
                    };
                    let sym =
                        Symbol::synthetic("ims-segm", format!("{}::{}", file.path, seg_name)).id();
                    let mut node = Node::new(
                        sym.clone(),
                        NodeKind::Other("ims_segment".to_string()),
                        seg_name,
                        Language::new("ims"),
                        loc.clone(),
                    );
                    node.signature = Some("SEGM".to_string());
                    nodes.push(node);

                    // DBD → SEGM Contains edge (intra-file, Parsed confidence).
                    if let Some(dbd) = &current_dbd {
                        local_edges.push(Edge::new(
                            dbd.clone(),
                            sym.clone(),
                            EdgeKind::Contains,
                            ResolutionTier::Parsed,
                            "ims",
                        ));
                    }

                    // Segment hierarchy: PARENT=<p> (skip 0 and * = root segment markers).
                    if let Some(parent_name) = kw_value(operands, "PARENT") {
                        if parent_name != "0" && parent_name != "*" {
                            refs.push(UnresolvedRef::new(
                                sym,
                                parent_name,
                                EdgeKind::Other("parent".to_string()),
                                loc,
                            ));
                        }
                    }
                }

                // ── PCB ─────────────────────────────────────────────────────
                // `PCB  TYPE=DB,DBDNAME=<db>,...`
                // Emits an UnresolvedRef from the PSB file anchor → <db> with
                // EdgeKind::Other("accesses").  TYPE=GSAM / TYPE=TP have no DBDNAME — skip them.
                "PCB" => {
                    // Only TYPE=DB carries a DBDNAME (TYPE=GSAM/TP connect to sequential/MFS).
                    if let Some(db_type) = kw_value(operands, "TYPE") {
                        if !db_type.eq_ignore_ascii_case("DB") {
                            continue;
                        }
                    }
                    if let Some(dbdname) = kw_value(operands, "DBDNAME") {
                        refs.push(UnresolvedRef::new(
                            psb_anchor(),
                            dbdname,
                            EdgeKind::Other("accesses".to_string()),
                            loc,
                        ));
                    }
                }

                // ── SENSEG ──────────────────────────────────────────────────
                // `SENSEG  NAME=<seg>,...`
                // The PSB's sensitive-segment view.  Emits an UnresolvedRef from the PSB anchor
                // → <seg> with EdgeKind::Other("sensitive_to").
                "SENSEG" => {
                    if let Some(seg_name) = kw_value(operands, "NAME") {
                        refs.push(UnresolvedRef::new(
                            psb_anchor(),
                            seg_name,
                            EdgeKind::Other("sensitive_to".to_string()),
                            loc,
                        ));
                    }
                }

                // Unknown / unhandled macros (DBDGEN, PSBGEN, FIELD, LCHILD, …) → skip.
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

// ── Helpers ───────────────────────────────────────────────────────────────────

/// One-line span anchor (IMS is line-oriented; byte offsets within the card are not tracked).
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

/// Extract the value for `keyword` from an IMS operand string `KW=VALUE,...`.
///
/// Returns the **first identifier token** inside the value so that all of the following collapse
/// to the plain segment name:
///
/// ```text
/// NAME=CUSTOMER
/// PARENT=CUSTOMER
/// PARENT=(CUSTOMER,SNGL)
/// PARENT=((CUSTOMER,SNGL))
/// ```
///
/// `PARENT=0` → `Some("0")` (caller decides whether this is a root marker).
fn kw_value(operands: &str, keyword: &str) -> Option<String> {
    // Locate `KEYWORD=` (case-insensitive search).
    let upper = operands.to_ascii_uppercase();
    let needle = format!("{}=", keyword.to_ascii_uppercase());
    let kw_pos = upper.find(&needle)?;
    let after = &operands[kw_pos + needle.len()..];

    // Extract the first identifier token from whatever follows the `=`.
    RE_FIRST_IDENT
        .find(after)
        .map(|m| after[m.start()..m.end()].to_ascii_uppercase())
        .filter(|s| !s.is_empty())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ims(text: &str) -> Extraction {
        ImsExtractor::new()
            .extract(&SourceFile {
                path: "TEST.dbd".to_string(),
                language: Language::new("ims"),
                text: text.to_string(),
            })
            .expect("ims extract")
    }

    fn extract_psb(text: &str) -> Extraction {
        ImsExtractor::new()
            .extract(&SourceFile {
                path: "TEST.psb".to_string(),
                language: Language::new("ims"),
                text: text.to_string(),
            })
            .expect("ims psb extract")
    }

    // ── kw_value unit tests ───────────────────────────────────────────────────

    #[test]
    fn kw_value_bare_ident() {
        assert_eq!(
            kw_value("NAME=CUSTDB,ACCESS=HDAM", "NAME"),
            Some("CUSTDB".to_string())
        );
    }

    #[test]
    fn kw_value_parenthesized() {
        // PARENT=((SEG,SNGL)) — common IMS syntax; first ident is SEG.
        assert_eq!(
            kw_value("NAME=ORDER,PARENT=((CUSTOMER,SNGL))", "PARENT"),
            Some("CUSTOMER".to_string())
        );
    }

    #[test]
    fn kw_value_root_marker_zero() {
        assert_eq!(
            kw_value("NAME=CUSTOMER,PARENT=0", "PARENT"),
            Some("0".to_string())
        );
    }

    #[test]
    fn kw_value_missing_keyword() {
        assert_eq!(kw_value("NAME=CUSTDB,ACCESS=HDAM", "PARENT"), None);
    }

    // ── DBD / SEGM (database hierarchy) ──────────────────────────────────────

    /// DBD snippet from the spec:
    ///   DBDFILE  DBD   NAME=CUSTDB,ACCESS=HDAM
    ///            SEGM  NAME=CUSTOMER,PARENT=0
    ///            SEGM  NAME=ORDER,PARENT=CUSTOMER
    #[test]
    fn dbd_emits_ims_database_node() {
        let ex = extract_ims(
            "DBDFILE  DBD   NAME=CUSTDB,ACCESS=HDAM\n\
             *comment line\n\
             \n\
                      SEGM  NAME=CUSTOMER,PARENT=0\n\
                      SEGM  NAME=ORDER,PARENT=CUSTOMER\n",
        );
        assert!(
            ex.nodes.iter().any(
                |n| n.name == "CUSTDB" && n.kind == NodeKind::Other("ims_database".to_string())
            ),
            "DBD NAME=CUSTDB must emit an ims_database node; nodes={:?}",
            ex.nodes.iter().map(|n| &n.name).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn segm_emits_ims_segment_nodes() {
        let ex = extract_ims(
            "DBDFILE  DBD   NAME=CUSTDB,ACCESS=HDAM\n\
                      SEGM  NAME=CUSTOMER,PARENT=0\n\
                      SEGM  NAME=ORDER,PARENT=CUSTOMER\n",
        );
        let names: Vec<&str> = ex
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Other("ims_segment".to_string()))
            .map(|n| n.name.as_str())
            .collect();
        assert!(
            names.contains(&"CUSTOMER"),
            "SEGM NAME=CUSTOMER must emit an ims_segment node; got {names:?}",
        );
        assert!(
            names.contains(&"ORDER"),
            "SEGM NAME=ORDER must emit an ims_segment node; got {names:?}",
        );
    }

    #[test]
    fn segm_with_parent_emits_parent_ref() {
        let ex = extract_ims(
            "DBDFILE  DBD   NAME=CUSTDB,ACCESS=HDAM\n\
                      SEGM  NAME=CUSTOMER,PARENT=0\n\
                      SEGM  NAME=ORDER,PARENT=CUSTOMER\n",
        );
        let parent_ref = ex
            .refs
            .iter()
            .find(|r| r.kind == EdgeKind::Other("parent".to_string()) && r.raw_name == "CUSTOMER")
            .expect("ORDER must emit a parent ref to CUSTOMER");
        // The ref must originate from the ORDER segment symbol.
        assert!(
            parent_ref.from.as_str().contains("ORDER"),
            "parent ref 'from' must reference the ORDER segment; from={}",
            parent_ref.from,
        );
    }

    #[test]
    fn segm_parent_zero_does_not_emit_ref() {
        let ex = extract_ims(
            "DBDFILE  DBD   NAME=CUSTDB\n\
                      SEGM  NAME=CUSTOMER,PARENT=0\n",
        );
        assert!(
            ex.refs
                .iter()
                .all(|r| r.kind != EdgeKind::Other("parent".to_string())),
            "PARENT=0 (root) must NOT emit a parent ref",
        );
    }

    #[test]
    fn dbd_segm_contains_edge_emitted() {
        let ex = extract_ims(
            "DBDFILE  DBD   NAME=CUSTDB\n\
                      SEGM  NAME=CUSTOMER,PARENT=0\n",
        );
        assert!(
            ex.local_edges.iter().any(|e| e.kind == EdgeKind::Contains),
            "DBD must emit a Contains edge to each SEGM",
        );
    }

    // ── PSB (program access) ──────────────────────────────────────────────────

    /// PSB snippet from the spec:
    ///          PCB   TYPE=DB,DBDNAME=CUSTDB,PROCOPT=GO
    ///          SENSEG NAME=CUSTOMER,PARENT=0
    #[test]
    fn pcb_type_db_emits_accesses_ref() {
        let ex = extract_psb(
            "         PCB   TYPE=DB,DBDNAME=CUSTDB,PROCOPT=GO\n\
             \n\
                      SENSEG NAME=CUSTOMER,PARENT=0\n",
        );
        let acc = ex
            .refs
            .iter()
            .find(|r| r.kind == EdgeKind::Other("accesses".to_string()) && r.raw_name == "CUSTDB")
            .expect("PCB TYPE=DB must emit an accesses ref to CUSTDB");
        // Verify the anchor is the PSB file anchor.
        assert!(
            acc.from.as_str().contains("ims-psb"),
            "accesses ref must originate from the ims-psb anchor; from={}",
            acc.from,
        );
    }

    #[test]
    fn senseg_emits_sensitive_to_ref() {
        let ex = extract_psb(
            "         PCB   TYPE=DB,DBDNAME=CUSTDB,PROCOPT=GO\n\
                      SENSEG NAME=CUSTOMER,PARENT=0\n",
        );
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Other("sensitive_to".to_string())
                    && r.raw_name == "CUSTOMER"),
            "SENSEG NAME=CUSTOMER must emit a sensitive_to ref",
        );
    }

    #[test]
    fn pcb_type_gsam_skipped() {
        // TYPE=GSAM is a sequential dataset PCB — no DBDNAME, must not panic or emit a ref.
        let ex = extract_psb("         PCB   TYPE=GSAM,PROCOPT=GO\n");
        assert!(
            ex.refs
                .iter()
                .all(|r| r.kind != EdgeKind::Other("accesses".to_string())),
            "PCB TYPE=GSAM must not emit an accesses ref",
        );
    }

    #[test]
    fn pcb_type_db_no_dbdname_skipped() {
        // Malformed TYPE=DB with no DBDNAME — lenient: skip, no panic.
        let ex = extract_psb("         PCB   TYPE=DB,PROCOPT=GO\n");
        assert!(
            ex.refs
                .iter()
                .all(|r| r.kind != EdgeKind::Other("accesses".to_string())),
            "PCB TYPE=DB without DBDNAME must not emit an accesses ref",
        );
    }

    #[test]
    fn comment_and_blank_lines_skipped() {
        let ex = extract_ims("* full-line comment\n\nDBDFILE  DBD  NAME=X\n");
        // Only the DBD node should be present; comment/blank produce nothing.
        assert_eq!(ex.nodes.len(), 1, "comment and blank lines must be skipped");
    }

    #[test]
    fn languages_returns_ims() {
        let langs = ImsExtractor::new().languages();
        assert_eq!(langs, vec![Language::new("ims")]);
    }

    #[test]
    fn parenthesized_parent_value_resolved() {
        // PARENT=((SEG,SNGL)) — both parens layers stripped; first ident is SEG.
        let ex = extract_ims(
            "DBDFILE  DBD   NAME=DB1\n\
                      SEGM  NAME=CHILD,PARENT=((PARENT1,SNGL))\n",
        );
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Other("parent".to_string())
                    && r.raw_name == "PARENT1"),
            "PARENT=((PARENT1,SNGL)) must resolve to PARENT1",
        );
    }
}
