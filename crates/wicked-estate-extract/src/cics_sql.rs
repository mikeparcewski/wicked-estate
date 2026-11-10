//! CICS + embedded-SQL (Db2) links for COBOL source — the standalone [`Extractor`] face of
//! [`crate::cobol_links`].
//!
//! The index pipeline does not use this type: it runs the tree-sitter COBOL extractor and then
//! [`crate::cobol_links::supplement`] over the same extraction, so every link starts from the real
//! program Module node. This extractor is for callers that want the EXEC CICS / EXEC SQL / COPY
//! links of one file without the grammar: it parses the file with the COBOL grammar itself and
//! returns the full supplemented extraction (File node, program, paragraphs, fields, links).
//!
//! Until 0.33.x this module emitted every ref from a file-level `synthetic("cobol-pgm", path)` anchor
//! that no extractor ever emitted as a node, so the index pruned its Db2 edges as dangling, and a
//! `cics_program` homonym node made every CICS LINK target ambiguous. Both are gone; see
//! `cobol_links` for what is stored now.

use crate::treesitter::TreeSitterExtractor;
use wicked_estate_core::{Error, Extraction, Extractor, Language, Result, SourceFile};

/// COBOL extraction plus the EXEC CICS / EXEC SQL / COPY links ([`crate::cobol_links`]).
pub struct CicsSqlExtractor;

impl CicsSqlExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CicsSqlExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for CicsSqlExtractor {
    fn languages(&self) -> Vec<Language> {
        vec![Language::new("cobol")]
    }

    fn extract(&self, file: &SourceFile) -> Result<Extraction> {
        let base = TreeSitterExtractor::for_language("cobol")
            .ok_or_else(|| Error::Invalid("the cobol grammar is not registered".into()))?;
        let mut ex = base.extract(file)?;
        crate::cobol_links::supplement(file, &mut ex);
        Ok(ex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wicked_estate_core::{EdgeKind, NodeKind, Symbol, edge_tags};

    #[test]
    fn standalone_extract_anchors_on_the_program_module() {
        let ex = CicsSqlExtractor::new()
            .extract(&SourceFile {
                path: "PAYCOB.cbl".to_string(),
                language: Language::new("cobol"),
                text: "       IDENTIFICATION DIVISION.
       PROGRAM-ID. PAYCOB.
       PROCEDURE DIVISION.
       MAIN-PARA.
           EXEC CICS LINK PROGRAM('PAYPGM') END-EXEC.
           EXEC SQL SELECT NAME INTO :WS-NAME FROM CUSTOMER END-EXEC.
"
                .to_string(),
            })
            .expect("extract");
        let pgm = ex
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Module && n.name == "PAYCOB")
            .expect("program module")
            .symbol
            .clone();
        assert!(
            ex.refs
                .iter()
                .any(|r| r.raw_name == "PAYPGM" && r.kind == EdgeKind::Calls && r.from == pgm)
        );
        let customer = Symbol::synthetic("db2", "CUSTOMER").id();
        assert!(ex.local_edges.iter().any(|e| e.source == pgm
            && e.target == customer
            && e.kind == edge_tags::other(edge_tags::READS)));
        assert!(ex.nodes.iter().any(|n| n.symbol == customer));
    }
}
