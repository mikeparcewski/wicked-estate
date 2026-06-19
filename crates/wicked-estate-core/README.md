# wicked-estate-core

The spine of [wicked-estate](https://github.com/mikeparcewski/wicked-estate): shared graph types + the five traits every wicked_estate crate implements.

Defines the core data model — `Node`, `Edge`, `NodeKind`, `EdgeKind`, `SymbolId`, `Span` — and the two-phase extraction staging types (`UnresolvedRef`, `Extraction`), plus the `Extractor`, `GraphStore`, `Resolver`, `Ranker`, and retrieval traits the rest of the workspace builds on. No I/O, no parsers — just the contracts.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
