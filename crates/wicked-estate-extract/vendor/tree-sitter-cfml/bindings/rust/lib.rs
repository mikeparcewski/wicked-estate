//! Rust binding for the vendored CFML grammars (cfmleditor/tree-sitter-cfml).
//!
//! Exposes two `LanguageFn`s:
//!   - `LANGUAGE_CFML`     — the tag-based grammar (`<cffunction>`, `<cfcomponent>`, `.cfm`/.cfc tags)
//!   - `LANGUAGE_CFSCRIPT` — the script grammar (`component { function … }`, `<cfscript>` bodies)
//!
//! The upstream `cfquery` SQL dialect grammar is intentionally not vendored.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_cfml() -> *const ();
    fn tree_sitter_cfscript() -> *const ();
}

/// The tag-based CFML `LanguageFn`.
pub const LANGUAGE_CFML: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cfml) };

/// The CFScript `LanguageFn`.
pub const LANGUAGE_CFSCRIPT: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cfscript) };

/// Generated node-type metadata (for query authoring / tooling).
pub const CFML_NODE_TYPES: &str = include_str!("../../cfml/src/node-types.json");
pub const CFSCRIPT_NODE_TYPES: &str = include_str!("../../cfscript/src/node-types.json");

#[cfg(test)]
mod tests {
    #[test]
    fn cfml_grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE_CFML.into())
            .expect("CFML tag grammar must load");
    }

    #[test]
    fn cfscript_grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE_CFSCRIPT.into())
            .expect("CFScript grammar must load");
    }
}
