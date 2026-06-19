# wicked-estate-resolve

Cross-file reference resolvers: binds `UnresolvedRef` values emitted by extractors to graph edges using layered confidence, from cheap name-matching through precise SCIP ingestion and on-demand LSP.

## What it does

- `NameResolver` binds an unresolved call/import to a project symbol when the name resolves uniquely; skips ambiguous names (defers to precise tiers).
- `ScopedNameResolver` prefers same-file (0.65) then same-directory (0.62) candidates before falling back to cross-file (0.60); records the disambiguation reason in edge metadata.
- `ImportMapResolver` uses the per-file `hints["imports"]` map recorded during extraction to narrow ambiguous same-name candidates to the specific imported file (confidence 0.63).
- `InfraResolver` binds IaC resource-to-resource `depends_on` references at Parsed confidence (1.0) without interfering with code resolvers.
- `MethodResolutionSynthesizer` fills remaining gaps with a Heuristic (0.5) edge when exactly one callable candidate exists.
- `resolve_all` runs multiple resolvers and deduplicates by `(source, target, kind)`, keeping the highest-confidence edge.
- `scip_edges` ingests a SCIP `index.scip` protobuf and emits confidence-1.0 edges by correlating SCIP occurrences to tree-sitter-derived nodes.
- `lsp` provides an on-demand JSON-RPC stdio client for `typescript-language-server`, `rust-analyzer`, and `pyright-langserver` — on-demand single-symbol queries only, never bulk.

## Key types / traits

| Item | Description |
|---|---|
| `NameResolver` | `Resolver` impl: unique-name binding at ImportMap tier (0.6). |
| `ScopedNameResolver` | `Resolver` impl: scope-aware disambiguation (same-file 0.65, same-dir 0.62, cross-file 0.60). |
| `ImportMapResolver` | `Resolver` impl: import-map scoped binding (0.63, `metadata["via"]="import-map"`). |
| `InfraResolver` | `Resolver` impl: IaC resource deps at Parsed tier (1.0). |
| `MethodResolutionSynthesizer` | `Resolver` impl: Heuristic fallback (0.5) for unique callable candidates. |
| `resolve_all(resolvers, refs, index)` | Run N resolvers, deduplicate edges keeping max confidence. |
| `scip_edges(index_bytes, nodes)` | Parse a SCIP index protobuf; emit `ResolutionTier::Scip` edges. |
| `SynthPrecision` | Precision measurement result for a synthesizer against a gold-labelled ref set. |
| `SYNTH_PRECISION_FLOOR` | `0.7` — minimum acceptable synthesizer precision. |

## Usage

```rust
use wicked_estate_resolve::{
    resolve_all, ImportMapResolver, ScopedNameResolver, NameResolver, MethodResolutionSynthesizer,
};
use wicked_estate_core::Resolver;

// Recommended order: highest-confidence first so lower-confidence edges are naturally lost on dedup.
let resolvers: &[&dyn Resolver] = &[
    &ImportMapResolver,
    &ScopedNameResolver,
    &NameResolver,
    &MethodResolutionSynthesizer,
];
let edges = resolve_all(resolvers, &unresolved_refs, &symbol_index)?;
```

## Crate features

No optional feature flags. SCIP protobuf ingestion (`scip` + `protobuf` deps) is always compiled. LSP support is always compiled but invoked on-demand only.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
