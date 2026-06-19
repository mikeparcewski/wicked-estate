# tree-sitter-lotusscript

**LotusScript (IBM Notes/Domino)** grammar for tree-sitter.

Authored in-house (no upstream grammar exists). LotusScript is a BASIC/VBA dialect.

**File types:** `.lss`, `.ls`

**Extracts:** `Class`/`Sub`/`Function`/`Property`/`Type` definitions, `Call` statements, and calls; the `"…"`, `|…|`, and `{…}` string forms and `%REM` blocks.

This is a deliberately minimal **symbols + calls** subset built for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine — it delimits definitions and captures call sites so real source parses cleanly, and is validated by a corpus parse-gate rather than by full-language coverage.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_lotusscript::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
