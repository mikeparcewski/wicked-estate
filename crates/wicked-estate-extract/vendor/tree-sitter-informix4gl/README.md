# tree-sitter-informix4gl

**Informix 4GL (I4GL)** grammar for tree-sitter.

Authored in-house (no usable upstream grammar exists; the [grammars-v4 informix](https://github.com/antlr/grammars-v4/tree/master/informix) ANTLR grammar was the reference).

**File types:** `.4gl`

**Extracts:** `MAIN`, `FUNCTION`, `REPORT` definitions, `CALL`/`RUN` statements, and calls; embedded SQL is parsed loosely.

This is a deliberately minimal **symbols + calls** subset built for the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) code-graph engine — it delimits definitions and captures call sites so real source parses cleanly, and is validated by a corpus parse-gate rather than by full-language coverage.

## Usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&wicked_estate_tree_sitter_informix4gl::LANGUAGE.into()).unwrap();
let tree = parser.parse(source, None).unwrap();
```

Part of [wicked-estate](https://github.com/mikeparcewski/wicked-estate) — a code + infrastructure
estate graph for LLM agents. MIT licensed.
