```
         _      _            _                 _        _       
         (_)    | |          | |               | |      | |      
__      ___  ___| | _____  __| |______ ___  ___| |_ __ _| |_ ___ 
\ \ /\ / / |/ __| |/ / _ \/ _` |______/ _ \/ __| __/ _` | __/ _ \
 \ V  V /| | (__|   <  __/ (_| |     |  __/\__ \ || (_| | ||  __/
  \_/\_/ |_|\___|_|\_\___|\__,_|      \___||___/\__\__,_|\__\___|
```

**Turn a repo — and its surrounding infrastructure/mainframe estate — into one queryable graph that
LLM agents can actually use.** Symbols, calls, imports, types, refs, and cross-domain estate links:
definitions, who-calls-X, blast-radius, scoped context. **Portable by design** — one static binary
that runs local-first on SQLite, or backs a shared, concurrent team graph on Postgres. Solo laptop to
enterprise CI fleet, same engine, same queries.

[![CI](https://github.com/mikeparcewski/wicked-estate/actions/workflows/ci.yml/badge.svg)](https://github.com/mikeparcewski/wicked-estate/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

> **Status:** v0.13.1 — `cargo test --workspace` is **1,000+ tests passing, 0 failed, 0 ignored**;
> 0 build warnings; clippy `-D warnings` clean. Greenfield, pre-1.0. **Published to crates.io.** See
> [FEATURES.md](./FEATURES.md) for the exhaustive, honestly-tagged capability inventory
> (✅ built / 🟡 partial / 🟦 designed-not-built).

---

## The foundation

wicked-estate is the **anchor** of a local-first foundation for AI coding agents — the code graph,
memory, and knowledge everything else queries. Around it sits the plumbing that makes that substrate
safe, event-driven, memorable, and governable. SQLite by default; no servers, no accounts, nothing
leaves your machine. The parts compose rather than lock in.

| Part | Role | Stack |
|---|---|---|
| **`wicked-estate`** (this repo) | the graph — symbols, calls, blast-radius, memory, knowledge, scoped context | Rust · crates.io |
| [`wicked-core`](https://github.com/mikeparcewski/wicked-core) | the runtime — single-writer store actor + live event stream so consumers never race on the shared DB | Rust |
| [`wicked-bus`](https://github.com/mikeparcewski/wicked-bus) | the event substrate — zero-infra, at-least-once, cursor-poll delivery on local SQLite | JS/ESM · npm |
| [`wicked-brain`](https://github.com/mikeparcewski/wicked-brain) | the memory — persistent knowledge on markdown + SQLite FTS5, no vector DB (bridge-period; folds into estate) | JS · npm |
| [`wicked-crew`](https://github.com/mikeparcewski/wicked-crew) | the governor — external daemon for deterministic, deny-dominates, phase-gated AI workflows with HITL | npm |

> Absorbed into this repo (not separate products): `wicked-memory` → `wicked-estate-memory`,
> `wicked-knowledge` → `wicked-estate-knowledge`, `wicked-overlay` → `wicked-estate-overlay`.

## Why

LLM coding agents waste turns grepping and re-reading files. wicked-estate gives them a precise,
ranked, **bounded** answer instead — "who calls this?", "what breaks if I change it?", "give me just
the context for this symbol" — across **100+ wired languages** (including legacy enterprise stacks:
VB6/VBA/VBScript/VB.NET, RPG, Delphi, ColdFusion, Progress ABL, PowerBuilder, Visual FoxPro,
LotusScript, Informix 4GL, Crystal Reports) plus a mainframe/IaC **estate** layer
(COBOL, JCL, RACF, IMS, MQ, Terraform, CloudFormation, …) that almost nothing else unifies into one
graph. Every edge carries `{confidence, provenance, resolved_by}`; heuristics are never presented as
facts.

## Install

```sh
# MCP server, from crates.io (the binary agents connect to):
cargo install wicked-estate-mcp

# Or straight from this repo (also builds the wicked-estate CLI):
cargo install --git https://github.com/mikeparcewski/wicked-estate wicked-estate-mcp

# Or build from source (gives you both the wicked-estate CLI and the MCP server):
git clone https://github.com/mikeparcewski/wicked-estate
cd wicked-estate && cargo build --release   # binaries in target/release/
```

Real **semantic** search is opt-in (keeps the default build dependency-free + offline):

```sh
cargo install wicked-estate-mcp --features model2vec   # static embeddings, light, no ONNX
cargo install wicked-estate-mcp --features fastembed   # contextual ONNX/BGE, highest quality
```

## Quickstart

```sh
# Index a repo into a local graph
wicked-estate index ./my-project --db graph.db

# Who/what depends on a symbol (blast radius = transitive dependents)
wicked-estate blast-radius MyClass --db graph.db

# Find a symbol + print its source
wicked-estate query handleRequest --db graph.db
wicked-estate source handleRequest --db graph.db

# Most important symbols (PageRank)
wicked-estate rank --db graph.db

# Semantic search (after: wicked-estate index … --embeddings, built with a semantic feature)
wicked-estate semantic "login security check" --db graph.db

# Stats (incl. git provenance), keep it watching, or stream changes
wicked-estate stats --db graph.db
wicked-estate watch ./my-project --db graph.db
```

## What it does (highlights — full list in [FEATURES.md](./FEATURES.md))

- **Code graph** — 100+ wired languages (tree-sitter); symbols, calls, imports,
  heritage. Languages are **data** (a manifest row + a `.scm` query) — adding one is zero core change.
- **Runtime language plugins** — drop a compiled tree-sitter grammar + `.scm` query + manifest into
  the plugins dir and it loads at startup, no recompile. The grammar is a separate artifact, never
  linked into the (MIT) core — so a grammar under a license incompatible with MIT (GPL, etc.) stays
  isolated. `wicked-estate plugins list` shows what's loaded; see [PLUGIN.md](./PLUGIN.md) and the
  [nginx example](./examples/plugins/nginx).
- **Precise blast-radius** — bounded reverse-reachability over *all* dependency edge kinds (not just
  calls), so it never silently under-reports.
- **Layered resolution** — name / scoped / import-map → SCIP (precise) → on-demand LSP. Two-phase
  EXTRACT → RESOLVE; resolution is swappable without re-parsing.
- **Estate mapping** — IaC (Terraform/CloudFormation/K8s) + mainframe (RACF security, IMS data, MQ
  messaging) as just-more-languages, joined cross-domain (e.g. the RACF profile that *protects* the
  dataset a JCL step *uses* — in one query). Drift = graph diff `iac` vs `live`.
- **Rules engine layer** — IBM ODM BAL/IRL, Camunda DMN, CLIPS/Jess, Drools GDST, Excel/XLSX decision tables, Salesforce Flow, AWS Config Rules, Azure Policy extracted into the same graph as code. NodeKind::{Rule,RuleSet,Condition,Action,Fact} + EdgeKind::{Governs,Evaluates,Produces,InvokedBy}. `RulesInventory` MCP tool lists engines + calling code. `RulesBridgeResolver` connects code call sites to real RuleSet nodes.
- **Hybrid + semantic retrieval** — graph + FTS5 core, embeddings an optional sidecar fused via RRF.
  Three embedder tiers: lexical (default) → model2vec (static, light) → fastembed (ONNX/BGE).
- **MCP server** — exposes **23 tools across 3 domains** to agents over JSON-RPC, following a
  strict runtime-behavior contract (cap output, report staleness, never error-early, label
  confidence): 10 estate tools (SearchEntity, RetrieveEntity, TraverseGraph, BlastRadius,
  FetchContent, ContextBundle, RulesInventory, RankHotspots, Communities, Lineage), 6 memory
  tools (memory.capture/recall/reflect/erase/learn/coverage), and 7 knowledge tools
  (knowledge.ingest/write/relate/recall/coverage/relate_code/recall_about_code).
- **Requirement ↔ code linking** — annotate nodes with `description` / `requirement` /
  `requirement_validated` and query by requirement.
- **Git-aware + incremental** — per-file git sha, incremental re-index, watch mode, a read-only
  edge-history log, and a resumable change-log (`subscribe`).
- **Multi-repo** — federated `cross-graph` search + blast-radius across many repo graphs.
- **Local or enterprise — same engine, swap one flag** — local-first **SQLite** by default (one
  file, zero setup, perfect for a laptop or a CI job); point `--db` at **Postgres**
  (`--features postgres`, `--db postgres://…`) for a shared team graph with concurrent writers and
  server-side traversal. No re-index, no query changes — storage is a backend, not a rewrite. The DB
  layer is swappable (the same seam SurrealDB will land behind).

## Architecture

The whole engine programs against **five traits** (`wicked-estate-core`); everything else is a
swappable impl behind a seam:

| Crate | Role |
|---|---|
| `wicked-estate-core` | types + the five traits (`Extractor`/`Resolver`/`GraphStore`/`Ranker`/`RetrievalTool`) + GraphStore conformance kit |
| `wicked-estate-extract` | tree-sitter + grammar-less extractors + W15 rules engine extractors |
| `wicked-estate-resolve` | reference resolvers (name/scoped/import-map/SCIP/estate/LSP) |
| `wicked-estate-store` | storage (SQLite default + in-memory reference) |
| `wicked-estate-rank` | PageRank importance |
| `wicked-estate-retrieve` | agent-facing retrieval tools + hybrid/semantic search |
| `wicked-estate-mcp` | MCP server (`wicked-estate-mcp` binary) |
| `wicked-estate` | the `wicked-estate` CLI binary |
| `wicked-estate-bench` | agent-eval benchmark harness |
| `wicked-estate-overlay` | internal — absorbed from wicked-overlay (XedgeStore cross-engine search layer) |
| `wicked-estate-memory-core` | internal — absorbed from wicked-memory (`MemoryApi` trait, `CaptureRequest`, `RecallQuery` types) |
| `wicked-estate-memory` | internal — absorbed from wicked-memory (memory engine impl) |
| `wicked-estate-knowledge` | internal — absorbed from wicked-knowledge (knowledge engine impl) |
| `wicked-estate-memory-api` | internal — absorbed from wicked-memory (shim crate for clean re-exports) |

## MCP

`wicked-estate-mcp` is a stdio MCP server (JSON-RPC 2.0) exposing **23 tools across 3 domains**:

- **Estate** (10 tools): `SearchEntity`, `RetrieveEntity`, `TraverseGraph`, `BlastRadius`, `FetchContent`, `ContextBundle`, `RulesInventory`, `RankHotspots`, `Communities`, `Lineage`
- **Memory** (6 tools): `memory.capture`, `memory.recall`, `memory.reflect`, `memory.erase`, `memory.learn`, `memory.coverage`
- **Knowledge** (7 tools): `knowledge.ingest`, `knowledge.write`, `knowledge.relate`, `knowledge.recall`, `knowledge.coverage`, `knowledge.relate_code`, `knowledge.recall_about_code`

Register it in **Claude Code, Cursor, Antigravity, or Codex**. Per-client config: **[docs/mcp-integration.md](./docs/mcp-integration.md)**.

```sh
wicked-estate index ./my-project --db .wicked-estate/graph.db
# Claude Code (or install the plugin: /plugin marketplace add mikeparcewski/wicked-estate):
claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/.wicked-estate/graph.db"
# Cursor / Antigravity: mcpServers in ~/.cursor/mcp.json or ~/.gemini/config/mcp_config.json
# Codex: a [mcp_servers.wicked-estate] table in ~/.codex/config.toml
```

## Adding a language

Languages are data, not code. Add a row to `crates/wicked-estate-extract/languages.toml`, drop a
`crates/wicked-estate-extract/src/queries/<lang>.scm` query (conventions: `@code_<kind>.def` +
`@code_<kind>.name`, `@call.function`, `@import`), and wire one `LangEntry`. The
`every_wired_query_compiles` test guards it. See [docs/add-lang.md](./docs/add-lang.md).

To add a language **without recompiling the core** — or to use a grammar under a license
incompatible with MIT — ship it as a **[runtime plugin](./PLUGIN.md)** instead: a compiled grammar +
query + manifest dropped into the plugins dir, loaded at startup. See the
[nginx example](./examples/plugins/nginx).

## Honest status (not yet true)

Per the project's "every done needs a still-not-done" rule:
- **External DB + OpenTelemetry**: the **Postgres** backend and the **OTLP** exporter are **built** ✅
  (Postgres conformance-passes — concurrent writers + server-side traversal are real) — but Postgres
  isn't yet benchmarked at scale vs. SQLite, OTel has no zero-to-dashboard guide yet, and SurrealDB
  is still designed-not-built.
- **Cloud collectors** (AWS/Azure/GCP): read-only interfaces + `tfstate` work; live collectors are
  observe-only stubs. Estate maturity trails the code-intelligence core.
- **Semantic embedder tests** are feature-gated (need a model download) — they run in the CI
  `semantic-embedders` lane, not the default suite.
- Niche mainframe modeling gaps (VSAM AIX, RECFM, COMP-3 usage metadata) and a few deferred languages
  (nim, pony, asm, odin).

## Docs

[FEATURES.md](./FEATURES.md) (full inventory) · [PLUGIN.md](./PLUGIN.md) (authoring runtime language
plugins) · [docs/ENGINE-CONTRACT.md](./docs/ENGINE-CONTRACT.md)
(invariants) · [docs/agent-behavior-rules.md](./docs/agent-behavior-rules.md) (the runtime contract)
· [docs/adr/](./docs/adr/) (decisions) · [docs/plan/WAVE-PLAN.md](./docs/plan/WAVE-PLAN.md) (tracker)
· [RELEASING.md](./RELEASING.md) (publishing to crates.io) · [docs/DESIGN-NOTES.md](./docs/DESIGN-NOTES.md)
(the design principles behind the engine).

## License

MIT © Michael Parcewski — see [LICENSE](./LICENSE).
