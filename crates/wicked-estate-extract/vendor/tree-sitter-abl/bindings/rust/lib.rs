//! Rust binding for the wicked_estate in-house Progress OpenEdge ABL tree-sitter grammar.
//!
//! Authored in-house: the comprehensive upstream grammar (usagi-coffee/tree-sitter-abl) ships a
//! ~97MB generated parser.c — too large to vendor near GitHub's 100MiB limit — so this is a
//! minimal symbols+calls subset (CLASS/INTERFACE/METHOD/CONSTRUCTOR/FUNCTION/PROCEDURE + RUN +
//! calls), validated by a corpus parse-gate + extraction-count assertions in `wicked-estate-extract`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_abl() -> *const ();
}

/// The Progress OpenEdge ABL `LanguageFn`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_abl) };

/// Generated node-type metadata (for query authoring / tooling).
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("ABL grammar must load");
    }
}
