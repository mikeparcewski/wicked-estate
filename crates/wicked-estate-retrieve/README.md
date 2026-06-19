# wicked-estate-retrieve

Agent-facing retrieval tools with hybrid and semantic search for [wicked-estate](https://github.com/mikeparcewski/wicked-estate).

The query surface an LLM agent actually calls: definitions, who-calls-X, blast-radius, scoped context, and hybrid (keyword + optional semantic) search over the code graph. Semantic embeddings are optional and feature-gated to keep the build light.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
