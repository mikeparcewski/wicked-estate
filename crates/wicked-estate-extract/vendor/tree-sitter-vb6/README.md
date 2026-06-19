# tree-sitter-vb6

**Visual Basic 6.0** grammar for tree-sitter.

Vendored from [joannefan/tree-sitter-vb6](https://github.com/joannefan/tree-sitter-vb6) (correct `Call`-keyword handling + `Attribute VB_Name` module names).

**File types:** `.bas`, `.cls`, `.frm`, `.ctl`

**Extracts:** modules, subs/functions, properties, `Implements`, and call sites.

Vendored for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine. The grammar is compiled from the committed `parser.c`; wicked-estate queries it for definitions and call sites.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_vb6::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
