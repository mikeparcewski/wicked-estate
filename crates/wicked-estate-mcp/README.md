# wicked-estate-mcp

MCP server exposing the [wicked-estate](https://github.com/mikeparcewski/wicked-estate) retrieval tools.

A Model Context Protocol server that hands an LLM agent the wicked-estate retrieval surface — definitions, who-calls-X, blast-radius, scoped context, search — over a code + infrastructure estate graph. Point your MCP-capable client at it and ask questions about a codebase.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
