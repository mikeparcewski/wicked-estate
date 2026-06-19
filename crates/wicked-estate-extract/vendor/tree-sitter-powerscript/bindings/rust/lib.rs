//! Rust binding for the wicked_estate in-house PowerBuilder PowerScript tree-sitter grammar.
//!
//! Authored in-house (no upstream grammar exists; the reference is the ANTLR grammars-v4
//! PowerBuilder grammar) via the template-extrapolate method — a minimal symbols+calls subset
//! (type / function / subroutine / event / on definitions + calls), validated by a corpus
//! parse-gate + extraction-count assertions in `wicked-estate-extract`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_powerscript() -> *const ();
}

/// The PowerBuilder PowerScript `LanguageFn`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_powerscript) };

/// Generated node-type metadata (for query authoring / tooling).
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("PowerScript grammar must load");
    }
}
