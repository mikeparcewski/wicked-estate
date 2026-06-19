# wicked-estate-observe

OTLP HTTP exporter backend for [wicked-estate](https://github.com/mikeparcewski/wicked-estate).

Optional OpenTelemetry (OTLP/HTTP) export of wicked-estate's tracing/metrics so indexing and retrieval can be observed in a standard tracing backend. Off the hot path; wire it in when you need visibility.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
