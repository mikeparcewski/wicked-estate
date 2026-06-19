# tree-sitter-vbscript

**VBScript** grammar for tree-sitter.

Vendored from [JJK96/tree-sitter-vbscript](https://github.com/JJK96/tree-sitter-vbscript).

**File types:** `.vbs`, `.wsf`

**Extracts:** subs/functions, classes, and call sites (WSH / Classic ASP).

Vendored for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine. The grammar is compiled from the committed `parser.c`; wicked-estate queries it for definitions and call sites.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_vbscript::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
