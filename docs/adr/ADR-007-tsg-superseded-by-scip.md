# ADR-007 — Stack-Graphs TSG Port (W3.2) Superseded for SCIP-Covered Languages

**Status:** Accepted
**Date:** 2026-06-13
**Supersedes:** W3.2 task as specified in the original wave plan
**Wave:** W3

---

## Context

Wave 3 originally included a task (W3.2) to port stack-graphs TSG name-resolution rules for
JS/TS/Python/Java as file-isolated subgraphs with path-stitching. The rationale at the time was
that stack-graphs would provide precise cross-file resolution for these mainstream languages
without requiring a per-project language server.

Since that task was written, the following has landed:

1. **W2.2 — SCIP precise tier** (`crates/wicked-estate-resolve/src/lib.rs::scip_edges`): ingests a SCIP
   index produced by `scip-typescript`, `scip-python`, or similar indexers. Edges carry
   `ResolutionTier::Scip` with `confidence:1.0`. The `wicked-estate scip <root>` command auto-runs
   `npx @sourcegraph/scip-typescript` if no `index.scip` is present. This is the highest-fidelity
   resolution tier available — strictly more precise than TSG path-stitching.

2. **W3.1 — `MethodResolutionSynthesizer`** (`crates/wicked-estate-resolve/src/lib.rs`): an AST-based
   synthesizer that resolves unambiguous call-site references at `ResolutionTier::Heuristic`
   (confidence 0.5). It operates on the parsed node index, not raw source text, fixing the
   regex-over-source anti-pattern from prior art.

3. **`ScopedNameResolver`** (`crates/wicked-estate-resolve/src/lib.rs`): file-scope and directory-scope
   name resolution within the `ImportMap` tier (confidence 0.60–0.65). Handles the majority of
   same-file and same-directory calls without a precise tier.

4. **W3.5 — Resolution precision dashboard** (`crates/wicked-estate-bench/src/lib.rs`): per-resolver
   precision/recall tracked over the benchmark corpus, including `measure_synth_precision` with
   a 70% floor enforced by `SYNTH_PRECISION_FLOOR`.

The resolver stack as shipped is:
```
ImportMapResolver (0.63) → ScopedNameResolver (0.60–0.65) → NameResolver (0.60)
  → MethodResolutionSynthesizer (0.50, Heuristic)
  → SCIP tier (1.0, Scip)  [after scip ingestion]
```

---

## Decision

**The full stack-graphs TSG port (W3.2) is superseded for all SCIP-covered languages.**

For TypeScript, JavaScript, Python, Java, Go, Rust, C++, and other languages with mature SCIP
indexers, the SCIP precise tier provides superior resolution quality (confidence:1.0) compared
to what a TSG port would deliver. Adding a TSG layer beneath SCIP for these languages adds
engineering cost with no measurable precision benefit.

**Rationale:**
- SCIP edges are ground-truth: emitted by the language's own type-checker or compiler (e.g.
  `typescript-language-server` via `scip-typescript`). A TSG heuristic cannot improve on this.
- The deduplication in `resolve_all` keeps the highest-confidence edge per `(source, target,
  kind)` triple. Any TSG edge would be discarded where a SCIP edge already exists.
- Stack-graphs require authoring per-language TSG rules — non-trivial work that provides
  diminishing returns for SCIP-covered languages.

---

## Remaining scope for TSG

A full stack-graphs port remains a **future option for non-SCIP languages**: languages where
no mature SCIP indexer exists and where TSG would materially improve cross-file resolution
beyond what `ScopedNameResolver` + `ImportMapResolver` provide. Candidates include COBOL,
Fortran, legacy VBA, and domain-specific languages with no SCIP tooling.

This is not on the critical path. If added:

- TSG rules go in `crates/wicked-estate-resolve/src/tsg/` as data files (not compiled per-language match arms).
- The `Resolver` trait is already the right seam — a `TsgResolver` implements it and plugs into
  `resolve_all` at `ResolutionTier::TSG` (between `ImportMap` and `Scip`).
- Entry condition: a language lacks SCIP coverage AND `ScopedNameResolver` precision on the
  benchmark corpus is below an acceptable floor for that language.

---

## Impact on the Wave Plan

W3.2 is re-classified as **SUPERSEDED-BY-SCIP** in the WAVE-PLAN dashboard. It is not an
omission or a gap — it is a deliberate decision that the delivered SCIP tier + `ScopedNameResolver`
+ `MethodResolutionSynthesizer` stack makes the W3.2 TSG work redundant for the language set
originally targeted. The task remains recorded in the plan as a future option.

W3.1 (AST-synth), W3.3 (on-demand LSP), W3.4 (confidence calibration), and W3.5 (precision
dashboard) are all delivered and green. W3 is complete except for the superseded W3.2.

---

## References

- `crates/wicked-estate-resolve/src/lib.rs` — `ScopedNameResolver`, `MethodResolutionSynthesizer`,
  `scip_edges`, `resolve_all`, `measure_synth_precision`, `SYNTH_PRECISION_FLOOR`
- `crates/wicked-estate-resolve/src/lsp.rs` — on-demand LSP tier (W3.3)
- the design notes — stack-graphs analysis
- `docs/plan/WAVE-PLAN.md` W3 — original task set
- `docs/ENGINE-CONTRACT.md` — resolution tier ordering

---

## Superseded note — 2026-08-28 (resolver-precision review)

The 2026-08-28 adversarial review (estate-review, Doc 02 / D02-7) found two factual errors in the
Context section above. The original text is kept unedited for history; this note corrects it.

**1. "The resolver stack as shipped" was never the production slice.** The stack listed above
(`ImportMapResolver → ScopedNameResolver → NameResolver → MethodResolutionSynthesizer → SCIP`)
never matched `index_path`'s resolver slice. The real slice, as shipped and as of this note, is:

```
NameResolver → ScopedNameResolver → ImportMapResolver → RelativeImportResolver
    → InfraResolver → RulesBridgeResolver
```

(`crates/wicked-estate/src/lib.rs`, guarded against drift by
`tests::slice_matches_engine_contract_table`; the per-tier activation table lives in
`docs/ENGINE-CONTRACT.md` §3.1 — that table is the drift-guarded source of truth for the slice;
this listing is informative. `resolve_all_with_coverage` dedups by max confidence, so order within
the slice never affected results.)

**2. `MethodResolutionSynthesizer` was retired 2026-08-28 and the precision dashboard never
existed in bench.** The synthesizer was never in any production slice on any branch
(`git log --all -S MethodResolutionSynthesizer`), and its emit set was a strict subset of
`ScopedNameResolver`'s Calls path at lower confidence (0.5 < 0.60), so it could never add an
edge — 0 synthetic edges across the measurement corpora. W3.5's claim that per-resolver precision
is "tracked in wicked-estate-bench" was false: `measure_synth_precision`/`SYNTH_PRECISION_FLOOR`
had zero bench references. Both were deleted (see CHANGELOG, unreleased → 0.15.0).

**The TSG-superseded-by-SCIP decision itself stands**, re-grounded on the two pillars that are
real: SCIP edges are ground-truth at confidence 1.0 and dominate on dedup, and
`ScopedNameResolver` covers the same-file/same-dir majority. The synthesizer was never load-bearing
for this decision.

**`lsp.rs` status (W3.3, same review — D02-5):** the on-demand LSP tier is a *client library* by
design — it has no `Resolver` impl and no `Edge` emission path, per the locked decision "LSP is
on-demand only, never bulk". Its W3.3 AC is met by `tests/lsp_live.rs` (probe-and-skip against
installed servers). The on-demand consumer (an MCP/CLI single-symbol definition/references tool,
`resolve.lsp` span) is designed-but-unwired, tracked as W3.6 in the wave plan.
