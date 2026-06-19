# tree-sitter-abl

**Progress OpenEdge ABL (4GL)** grammar for tree-sitter.

Authored in-house: the comprehensive upstream grammar ships a ~97 MB `parser.c` (too large to vendor near GitHub's 100 MiB limit), so this is a minimal symbols+calls subset (~112 KB `parser.c`).

**File types:** `.p`, `.w`, `.i` (ABL classes are `.cls`, dispatched by language name)

**Extracts:** `CLASS`/`INTERFACE`/`METHOD`/`CONSTRUCTOR`/`FUNCTION`/`PROCEDURE` definitions, `RUN` statements, and calls.

This is a deliberately minimal **symbols + calls** subset built for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine — it delimits definitions and captures call sites so real source parses cleanly, and is validated by a corpus parse-gate rather than by full-language coverage.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_abl::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
