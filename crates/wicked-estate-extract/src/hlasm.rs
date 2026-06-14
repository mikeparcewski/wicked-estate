//! IBM High-Level Assembler (HLASM) extractor — **line/column-oriented, NO tree-sitter grammar**.
//!
//! No tree-sitter grammar exists for HLASM (`arborium-asm` is x86/GAS, a different dialect). HLASM
//! is a fixed *card* format — `LABEL  OPCODE  OPERANDS  comments` — so a line parser is the right
//! tool, on the same generic [`Extractor`] seam as the JCL extractor. It extracts:
//! - `LABEL CSECT/START/RSECT` → a section node (the assembler "module"/entry).
//! - `LABEL DSECT`             → a data-section node.
//! - `CALL routine`            → a call ref (the cross-program link — e.g. an assembler stub that
//!   `CALL`s a COBOL program; resolves to that program's node).
//! - `EXTRN/WXTRN name,...`    → external-symbol references (programs/data resolved at link-edit).
//!
//! Macros, full operand grammar, and DC/DS storage are out of scope (the section + call graph is
//! the value). Comments: `*` in column 1, or `.*` macro comments.

use wicked_estate_core::{
    EdgeKind, Extraction, Extractor, Language, Location, Node, NodeKind, Result, SourceFile, Span,
    Symbol, SymbolId, UnresolvedRef,
};

/// Extractor for HLASM assembler source. Grammar-less; parses the card format line by line.
pub struct HlasmExtractor;

impl HlasmExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HlasmExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for HlasmExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("hlasm")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let mut nodes: Vec<Node> = Vec::new();
        let local_edges = Vec::new();
        let mut refs: Vec<UnresolvedRef> = Vec::new();
        let mut current_section: Option<SymbolId> = None;

        for (i, raw) in file.text.lines().enumerate() {
            // `*` in column 1 is a comment line; blank lines are skipped.
            if raw.starts_with('*') || raw.trim().is_empty() {
                continue;
            }
            // A label is present iff column 1 is non-blank (the first token is the label).
            let labeled = !raw.starts_with([' ', '\t']);
            let mut toks = raw.split_whitespace();
            let (label, opcode) = if labeled {
                let label = toks.next();
                (label, toks.next())
            } else {
                (None, toks.next())
            };
            let Some(opcode) = opcode else { continue };
            let operands = toks.next().unwrap_or("");
            let loc = Location::new(&file.path, line_span(i));

            match opcode.to_ascii_uppercase().as_str() {
                "CSECT" | "START" | "RSECT" => {
                    let name = label.unwrap_or("MAIN");
                    let sym = Symbol::synthetic("hlasm", format!("{}::{}", file.path, name)).id();
                    let mut node = Node::new(
                        sym.clone(),
                        NodeKind::Module,
                        name.to_string(),
                        Language::new("hlasm"),
                        loc,
                    );
                    node.signature = Some(opcode.to_ascii_uppercase());
                    nodes.push(node);
                    current_section = Some(sym);
                }
                "DSECT" => {
                    if let Some(name) = label {
                        let sym =
                            Symbol::synthetic("hlasm", format!("{}::{}", file.path, name)).id();
                        let mut node = Node::new(
                            sym,
                            NodeKind::Other("dsect".to_string()),
                            name.to_string(),
                            Language::new("hlasm"),
                            loc,
                        );
                        node.signature = Some("DSECT".to_string());
                        nodes.push(node);
                    }
                }
                "CALL" => {
                    // CALL routine[,(parms)] → the called routine is the first operand token.
                    if let Some(target) = operands.split(',').next().map(str::trim) {
                        if !target.is_empty() {
                            refs.push(UnresolvedRef::new(
                                from_or_file(&current_section, file),
                                target.to_string(),
                                EdgeKind::Calls,
                                loc,
                            ));
                        }
                    }
                }
                "EXTRN" | "WXTRN" => {
                    // EXTRN A,B,C → external symbols this module references (resolved at link-edit).
                    for ext in operands.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        refs.push(UnresolvedRef::new(
                            from_or_file(&current_section, file),
                            ext.to_string(),
                            EdgeKind::Calls,
                            loc.clone(),
                        ));
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

/// The reference source: the current section if one is open, else a file-level anchor.
fn from_or_file(current: &Option<SymbolId>, file: &SourceFile) -> SymbolId {
    current
        .clone()
        .unwrap_or_else(|| Symbol::synthetic("hlasm", format!("{}::FILE", file.path)).id())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str) -> Extraction {
        HlasmExtractor::new()
            .extract(&SourceFile {
                path: "PROG.asm".to_string(),
                language: Language::new("hlasm"),
                text: text.to_string(),
            })
            .expect("hlasm extract")
    }

    #[test]
    fn csect_becomes_module_node() {
        let ex = extract("PAYROLL  CSECT\n         BALR  12,0\n         END\n");
        assert!(
            ex.nodes
                .iter()
                .any(|n| n.name == "PAYROLL" && n.kind == NodeKind::Module),
            "CSECT must produce a module node",
        );
    }

    #[test]
    fn call_emits_call_ref() {
        // The cross-program link: an assembler routine CALLing a (often COBOL) program.
        let ex = extract("MAINPGM  CSECT\n         CALL  COBSUB,(PARM)\n         END\n");
        assert!(
            ex.refs
                .iter()
                .any(|r| r.kind == EdgeKind::Calls && r.raw_name == "COBSUB"),
            "CALL COBSUB must emit a call ref",
        );
    }

    #[test]
    fn extrn_emits_external_refs() {
        let ex = extract("MOD      CSECT\n         EXTRN EXTA,EXTB\n         END\n");
        let names: Vec<&str> = ex
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| r.raw_name.as_str())
            .collect();
        assert!(
            names.contains(&"EXTA") && names.contains(&"EXTB"),
            "EXTRN names: {names:?}"
        );
    }

    #[test]
    fn comment_lines_skipped() {
        let ex = extract("* a comment in column 1\n         END\n");
        assert!(ex.nodes.is_empty() && ex.refs.is_empty());
    }
}
