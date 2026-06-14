//! Rust binding for the wicked_estate free-format RPG IV tree-sitter grammar.
//!
//! Authored in-house (no upstream grammar existed for RPG) via the template-extrapolate method,
//! validated by a corpus parse-gate + extraction-count assertions in `wicked-estate-extract`. Exposes the
//! standard `LANGUAGE: LanguageFn` so callers use `tree_sitter_rpg::LANGUAGE.into()`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_rpg() -> *const ();
}

/// The free-format RPG IV `LanguageFn`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_rpg) };

/// Generated node-type metadata (for query authoring / tooling).
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("free-format RPG grammar must load");
    }
}
