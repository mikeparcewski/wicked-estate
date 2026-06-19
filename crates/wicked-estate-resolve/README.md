# wicked-estate-resolve

Cross-file and cross-language reference resolvers for [wicked-estate](https://github.com/mikeparcewski/wicked-estate).

Takes the `UnresolvedRef`s emitted by extraction and binds them to definition nodes — resolving calls, imports, and heritage within and across files, and across language boundaries (JCL `EXEC PGM` → COBOL, `CALL` → COBOL, modern imports). This is what turns a pile of per-file symbols into a connected graph.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
