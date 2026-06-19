//! Rust binding for the wicked_estate in-house Crystal Reports formula tree-sitter grammar.
//!
//! Authored in-house (no upstream grammar exists) from the Crystal-Syntax references — a minimal
//! symbols+calls subset (variable declarations + `{@formula}` / function-call references),
//! validated by a corpus parse-gate in `wicked-estate-extract`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_crystal_formula() -> *const ();
}

/// The Crystal Reports formula `LanguageFn`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_crystal_formula) };

/// Generated node-type metadata (for query authoring / tooling).
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Crystal Reports formula grammar must load");
    }
}
