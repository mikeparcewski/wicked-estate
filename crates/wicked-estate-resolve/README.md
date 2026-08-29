# wicked-estate-resolve

Cross-file reference resolvers: binds `UnresolvedRef` values emitted by extractors to graph edges using layered confidence, from cheap name-matching through precise SCIP ingestion and on-demand LSP.

## What it does

- `NameResolver` binds an unresolved call/import to a project symbol when the name resolves uniquely; skips ambiguous names (defers to precise tiers).
- `ScopedNameResolver` prefers same-file (0.65) then same-directory (0.62) candidates before falling back to cross-file (0.60); records the disambiguation reason in edge metadata.
- `ImportMapResolver` uses the per-file `hints["imports"]` map recorded during extraction to narrow ambiguous same-name candidates to the specific imported file (confidence 0.63).
- `RelativeImportResolver` binds quoted relative JS/TS import specifiers (`'./foo'`, `'../bar'`) to their target File node by exact joined-path match, root-guarded against the repo/label root — `ImportMap` tier with a per-edge confidence override of 0.9 (`resolved_by = "relative-import"`); ambiguity parks the reference.
- `InfraResolver` binds IaC resource-to-resource `depends_on` references at Parsed confidence (1.0) without interfering with code resolvers.
- `resolve_all_with_coverage` runs multiple resolvers, deduplicates edges by `(source, target, kind)` keeping the highest-confidence edge, and returns the unresolved references under the one definition in `docs/ENGINE-CONTRACT.md` §2.1 (a reference is unresolved iff no resolver emitted an edge attributed to it — same `(location, kind)`).
- `scip_edges` ingests a SCIP `index.scip` protobuf and emits confidence-1.0 edges by correlating SCIP occurrences to tree-sitter-derived nodes.
- `lsp` provides an on-demand JSON-RPC stdio client for `typescript-language-server`, `rust-analyzer`, and `pyright-langserver` — on-demand single-symbol queries only, never bulk. A client library by design: no `Resolver` impl, no edge emission; the on-demand consumer (MCP/CLI definition/references tool) is the W3.6 follow-up.

## Key types / traits

| Item | Description |
|---|---|
| `NameResolver` | `Resolver` impl: unique-name binding at ImportMap tier (0.6). |
| `ScopedNameResolver` | `Resolver` impl: scope-aware disambiguation (same-file 0.65, same-dir 0.62, cross-file 0.60). |
| `ImportMapResolver` | `Resolver` impl: import-map scoped binding (0.63, `metadata["via"]="import-map"`). |
| `RelativeImportResolver` | `Resolver` impl: relative JS/TS import specifiers → target File node (ImportMap tier, 0.9 per-edge override, `metadata["via"]="relative-path"`). |
| `InfraResolver` | `Resolver` impl: IaC resource deps at Parsed tier (1.0). |
| `Resolution` | `{ edges, unresolved }` — one resolve pass's full output. |
| `resolve_all_with_coverage(resolvers, refs, index)` | Run N resolvers; deduplicated edges + per-reference unresolved set (ENGINE-CONTRACT §2.1). |
| `scip_edges(index_bytes, nodes)` | Parse a SCIP index protobuf; emit `ResolutionTier::Scip` edges. |
| `RulesBridgeResolver` | W15.13 — connects code call sites to real RuleSet nodes. Handles `UnresolvedRef`s with `raw_name = "rules-engine:<scheme>"` emitted by `ExtraEdgeExtractor`. Queries all `NodeKind::RuleSet` nodes and emits `InvokedBy` edges at `ResolutionTier::Heuristic`. |

## Usage

```rust
use wicked_estate_resolve::{
    resolve_all_with_coverage, ImportMapResolver, InfraResolver, NameResolver,
    RelativeImportResolver, RulesBridgeResolver, ScopedNameResolver,
};
use wicked_estate_core::Resolver;

// The production index/watch slice (dedup keeps max confidence; order is irrelevant to the
// result). The drift-guarded activation table is docs/ENGINE-CONTRACT.md §3.1.
let relative = RelativeImportResolver::new(scope.as_deref()); // repo/label root guard
let resolvers: &[&dyn Resolver] = &[
    &NameResolver,
    &ScopedNameResolver,
    &ImportMapResolver,
    &relative,
    &InfraResolver,
    &RulesBridgeResolver,
];
let resolution = resolve_all_with_coverage(resolvers, &unresolved_refs, &symbol_index)?;
// resolution.edges     → deduplicated edges (highest confidence per (source, target, kind))
// resolution.unresolved → references no resolver bound (persist per ENGINE-CONTRACT §2.1)
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
