# tree-sitter-vba

**Visual Basic for Applications (VBA)** grammar for tree-sitter.

Vendored from [tmepple/tree-sitter-vba](https://github.com/tmepple/tree-sitter-vba).

**File types:** `.vba` (also `.bas`/`.cls`/`.frm` shared with VB6)

**Extracts:** subs/functions, properties, types, and call sites in Office macros.

Vendored for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine. The grammar is compiled from the committed `parser.c`; wicked-estate queries it for definitions and call sites.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_vba::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
