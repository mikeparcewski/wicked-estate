# wicked-estate-extract

Tree-sitter and grammar-less source extractors for [wicked-estate](https://github.com/mikeparcewski/wicked-estate).

Turns source files into graph nodes (definitions) and unresolved references (calls/imports/heritage) across 90+ languages — a data-driven `languages.toml` registry maps file extensions to a tree-sitter grammar + a `.scm` query. Includes vendored/in-house grammars for legacy enterprise languages (RPG, the VB family, CFML, Progress ABL, PowerBuilder, Visual FoxPro, LotusScript, Crystal Reports formulas, Informix 4GL) and grammar-less extractors for the mainframe/IaC estate (COBOL copybooks, JCL, CloudFormation, …). Optional cloud collectors (AWS/Azure/GCP) are feature-gated.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
