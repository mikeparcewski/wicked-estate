# wicked_estate — Getting Started Guide

Welcome to **wicked_estate**, a fast, local-first code intelligence engine that turns any
repository into a queryable graph of symbols, calls, imports, and types.

---

## Table of Contents

- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Architecture Overview](#architecture-overview)
- [Supported Languages](#supported-languages)
- [Contributing](#contributing)

---

## Prerequisites

Before you begin, ensure you have the following installed:

- **Rust 1.78+** — install via [rustup](https://rustup.rs/)
- **SQLite 3.39+** — usually pre-installed on macOS and most Linux distros
- **Git 2.30+**

Optional but recommended:

- `fd` and `ripgrep` for faster file discovery
- A language server (e.g. `rust-analyzer`) for LSP-tier resolution

---

## Installation

### From crates.io

```bash
cargo install wicked-estate
```

### From source

```bash
git clone https://github.com/example/wicked-estate.git
cd wicked-estate
cargo build --release
# binary lands at ./target/release/we
```

Verify the installation:

```bash
we --version
# wicked-estate 0.1.0 (rev: a1b2c3d)
```

---

## Quick Start

Index a repository in one command:

```bash
we index --root /path/to/my-project --db ./my-project.db
```

Query it:

```bash
# Who calls `handle_payment`?
we query callers handle_payment

# What does `PaymentService` import?
we query imports PaymentService

# Blast radius of renaming `Money`
we query blast-radius Money
```

---

## Configuration

Create a `wicked.toml` at the project root:

```toml
[index]
root       = "."
db         = ".wicked/graph.db"
exclude    = ["target/**", "node_modules/**", "**/*.min.js"]

[languages]
enabled    = ["rust", "typescript", "python", "go"]

[resolution]
tiers      = ["tags", "import-map", "tsg"]
lsp_on_demand = true

[retrieval]
embeddings = false   # opt-in sidecar
fts_weight = 0.6
graph_weight = 0.4
```

---

## Architecture Overview

wicked_estate is built around **five traits** that form a fixed spine:

| Trait | Role |
|-------|------|
| `Extractor` | Parse source files into `Node`/`Edge` tuples using tree-sitter |
| `Resolver` | Resolve `UnresolvedRef` → `SymbolId` across resolution tiers |
| `GraphStore` | Persist and query the graph (`MemStore` or `SqliteStore`) |
| `Ranker` | Score nodes via personalized PageRank over the call graph |
| `RetrievalTool` | Expose a 3-tool agent API (define / callers / context) |

The pipeline is strictly two-phase: **EXTRACT** then **RESOLVE**. Resolution is swappable
and never requires re-parsing. See [ENGINE-CONTRACT.md](docs/ENGINE-CONTRACT.md) for the
full behavioural specification.

---

## Supported Languages

wicked_estate ships with tree-sitter grammars for **73 languages**.
A sampling of first-class supported languages:

1. Rust
2. TypeScript / JavaScript
3. Python
4. Go
5. Java
6. C / C++
7. C#
8. Ruby
9. Kotlin
10. Swift

Adding a new language requires only:

- A tree-sitter grammar crate
- A `.scm` query file under `crates/wicked-estate-extract/queries/<lang>/`
- A row in `languages.toml`

**Zero core Rust changes required.**

---

## Contributing

We follow a strict **spine-before-fan-out** discipline. Before opening a PR:

1. Read `docs/DESIGN-NOTES.md` and the relevant ADR.
2. Ensure `cargo test --workspace` is green (0 warnings, 0 ignored tests).
3. Add or update the conformance test if you touch `GraphStore`.
4. Delete what you replace — no grandfathering.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full process.

> "Rigor is the product." — project motto
