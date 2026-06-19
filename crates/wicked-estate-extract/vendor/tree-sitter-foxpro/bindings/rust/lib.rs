//! Rust binding for the wicked_estate in-house Visual FoxPro tree-sitter grammar.
//!
//! Authored in-house (no upstream grammar exists; the reference is the vfp2py ANTLR grammar) via
//! the template-extrapolate method — a minimal symbols+calls subset (PROCEDURE/FUNCTION/DEFINE
//! CLASS + function-call-syntax calls), validated by a corpus parse-gate + extraction-count
//! assertions in `wicked-estate-extract`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_foxpro() -> *const ();
}

/// The Visual FoxPro `LanguageFn`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_foxpro) };

/// Generated node-type metadata (for query authoring / tooling).
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Visual FoxPro grammar must load");
    }
}
