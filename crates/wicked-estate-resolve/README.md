# wicked-estate-resolve

Cross-file reference resolvers: binds `UnresolvedRef` values emitted by extractors to graph edges using layered confidence, from cheap name-matching through precise SCIP ingestion and on-demand LSP.

## What it does

- `NameResolver` binds an unresolved call/import to a project symbol when the name resolves uniquely; skips ambiguous names (defers to precise tiers).
- `ScopedNameResolver` prefers same-file (0.65) then same-directory (0.62) candidates before falling back to cross-file (0.60); records the disambiguation reason in edge metadata.
- `ImportMapResolver` uses the per-file `hints["imports"]` map recorded during extraction to narrow ambiguous same-name candidates to the specific imported file (confidence 0.63).
- `InfraResolver` binds IaC resource-to-resource `depends_on` references at Parsed confidence (1.0) without interfering with code resolvers.
- `resolve_all` runs multiple resolvers and deduplicates by `(source, target, kind)`, keeping the highest-confidence edge.
- `scip_edges` ingests a SCIP `index.scip` protobuf and emits confidence-1.0 edges by correlating SCIP occurrences to tree-sitter-derived nodes.
- `lsp` provides an on-demand JSON-RPC stdio client for `typescript-language-server`, `rust-analyzer`, and `pyright-langserver` — on-demand single-symbol queries only, never bulk. A client library by design: no `Resolver` impl, no edge emission; the on-demand consumer (MCP/CLI definition/references tool) is the W3.6 follow-up.

## Key types / traits

| Item | Description |
|---|---|
| `NameResolver` | `Resolver` impl: unique-name binding at ImportMap tier (0.6). |
| `ScopedNameResolver` | `Resolver` impl: scope-aware disambiguation (same-file 0.65, same-dir 0.62, cross-file 0.60). |
| `ImportMapResolver` | `Resolver` impl: import-map scoped binding (0.63, `metadata["via"]="import-map"`). |
| `InfraResolver` | `Resolver` impl: IaC resource deps at Parsed tier (1.0). |
| `resolve_all(resolvers, refs, index)` | Run N resolvers, deduplicate edges keeping max confidence. |
| `scip_edges(index_bytes, nodes)` | Parse a SCIP index protobuf; emit `ResolutionTier::Scip` edges. |
| `RulesBridgeResolver` | W15.13 — connects code call sites to real RuleSet nodes. Handles `UnresolvedRef`s with `raw_name = "rules-engine:<scheme>"` emitted by `ExtraEdgeExtractor`. Queries all `NodeKind::RuleSet` nodes and emits `InvokedBy` edges at `ResolutionTier::Heuristic`. |

## Usage

```rust
use wicked_estate_resolve::{
    resolve_all, ImportMapResolver, InfraResolver, NameResolver, RulesBridgeResolver,
    ScopedNameResolver,
};
use wicked_estate_core::Resolver;

// The production index/watch slice (dedup keeps max confidence; order is irrelevant to the result).
let resolvers: &[&dyn Resolver] = &[
    &NameResolver,
    &ScopedNameResolver,
    &ImportMapResolver,
    &InfraResolver,
    &RulesBridgeResolver,
];
let edges = resolve_all(resolvers, &unresolved_refs, &symbol_index)?;
```

`MethodResolutionSynthesizer` (and the `measure_synth_precision`/`SynthPrecision`/
`SYNTH_PRECISION_FLOOR` precision monitor) was retired 2026-08-28: its emit set was a strict
subset of `ScopedNameResolver`'s Calls path at lower confidence, so it could never add an edge
(ADR-007 superseding note).

## Crate features

No optional feature flags. SCIP protobuf ingestion (`scip` + `protobuf` deps) is always compiled. LSP support is always compiled but invoked on-demand only.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
