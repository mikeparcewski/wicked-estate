# tree-sitter-rpg

Free-format **RPG IV (ILE RPG)** grammar for tree-sitter.

Authored in-house — no upstream tree-sitter grammar exists for RPG.

**File types:** `.rpgle`, `.sqlrpgle`

**Extracts:** procedures (`dcl-proc`), data structures (`dcl-ds`), standalone fields/constants (`dcl-s`/`dcl-c`), file decls (`dcl-f`), and procedure calls.

This is a deliberately minimal **symbols + calls** subset built for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine — it delimits definitions and captures call sites so real source parses cleanly, and is validated by a corpus parse-gate rather than by full-language coverage.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_rpg::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
