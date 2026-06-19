# tree-sitter-crystal-formula

**Crystal Reports formula language (Crystal Syntax)** grammar for tree-sitter.

Authored in-house (no upstream grammar exists). Note: this is the SAP Crystal Reports *formula* language, **not** the Crystal programming language.

**File types:** `.crf` (a convention — formulas usually live inside `.rpt` binaries)

**Extracts:** variable declarations (`Local`/`Global`/`Shared`), `{@Formula}` references (the formula-to-formula call graph), `{Table.Field}`/`{?Param}` references, and function calls.

This is a deliberately minimal **symbols + calls** subset built for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine — it delimits definitions and captures call sites so real source parses cleanly, and is validated by a corpus parse-gate rather than by full-language coverage.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_crystal_formula::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
