# wicked-estate-store

Graph storage backends for [wicked-estate](https://github.com/mikeparcewski/wicked-estate).

Implements the `GraphStore` trait: an in-memory reference store (`MemStore`) and the durable SQLite backend (FTS5 full-text + `sqlite-vec` vector search) that powers local-first indexing and retrieval. One file, no server.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
