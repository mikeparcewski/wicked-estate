# tree-sitter-cfml

**CFML (ColdFusion)** grammar for tree-sitter — both the tag dialect and CFScript.

Vendored from [cfmleditor/tree-sitter-cfml](https://github.com/cfmleditor/tree-sitter-cfml). The upstream `cfquery` SQL dialect is omitted (SQL is covered by a dedicated grammar).

**File types:** `.cfm`, `.cfc` (tag), `.cfs` (script)

**Extracts:** `<cffunction>`/`<cfcomponent>` tags, embedded `<cfscript>`, script `component { function … }`, and call sites. Exposes `LANGUAGE_CFML` + `LANGUAGE_CFSCRIPT`.

Vendored for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine. The grammar is compiled from the committed `parser.c`; wicked-estate queries it for definitions and call sites.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_cfml::LANGUAGE_CFML.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
