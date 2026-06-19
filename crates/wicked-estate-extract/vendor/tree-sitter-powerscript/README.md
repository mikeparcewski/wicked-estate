# tree-sitter-powerscript

**PowerBuilder PowerScript** grammar for tree-sitter.

Authored in-house (no upstream grammar exists; the [grammars-v4 powerbuilder](https://github.com/antlr/grammars-v4/tree/master/powerbuilder) ANTLR grammar was the reference).

**File types:** `.sru`, `.srw`, `.srf`, `.sra`, `.srm`

**Extracts:** `type … from … end type` object declarations (with ancestor heritage), function/subroutine/event/`on` bodies, and calls.

This is a deliberately minimal **symbols + calls** subset built for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine — it delimits definitions and captures call sites so real source parses cleanly, and is validated by a corpus parse-gate rather than by full-language coverage.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_powerscript::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
