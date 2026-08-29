# Engine Contract

The hard invariants every crate must honor. Borrowed from the `prior art-contract.md`
pattern: pin the surprising things *once* so no one re-derives them wrong. Violations are caught
by `wicked_estate_core::conformance::graph_store_suite` and the edge-direction tests.

## 1. Edge-direction invariant (the one people get wrong)

```
            depends on
   source ───────────────▶ target
 (dependent)            (dependency)
```

- "A **calls** B"  → `Edge { source: A, target: B, kind: Calls }`
- "A **imports** B" → `Edge { source: A, target: B, kind: Imports }`

Therefore:
- **Dependencies of X** (what X needs) = edges where `source == X` → `Direction::Dependencies`.
- **Dependents of X** (who needs X) = edges where `target == X` → `Direction::Dependents`.
- **Blast radius of X** ("what breaks if I change X?") = transitive **dependents** =
  reverse-reachability following edges where `target == X`, then their sources, recursively.

This matches the hard-won `DEPENDENTS_BY = "target"` (a spike there caught a latent
direction bug in a reference impl — the design notes). `MemStore` and every
future store are verified against it in conformance.

## 2. Two-phase pipeline

```
 EXTRACT (per file, parallel)            RESOLVE (whole project, once)
 ───────────────────────────            ─────────────────────────────
 SourceFile ──Extractor──▶ Extraction   UnresolvedRef[] ──Resolver──▶ Edge[]
                           ├─ nodes              ▲                      │
                           ├─ local_edges        └── SymbolIndex ───────┘
                           └─ refs (UnresolvedRef)
```

- Extractors are **stateless and per-file** — no cross-file knowledge, so they parallelize.
- `local_edges` are intra-file facts known at parse time (`Contains`, `Defines`) at confidence 1.0.
- Cross-file references are emitted as `UnresolvedRef` (by name + hints) and bound later.
- Resolvers are **swappable**: changing resolution never requires re-parsing.

### 2.1 Unresolved references — the one definition

> A reference is **unresolved** iff no resolver emitted an edge **attributed to it** — an edge
> carrying the reference's exact `(location, kind)` — after per-ref re-resolution of references
> that share `(location, kind)`. One `unresolved_refs` row per unresolved reference (per site).

This is the only place the definition is written out; every other surface cites it.

**How attribution works** (`wicked_estate_resolve::resolve_all_with_coverage`): references are
bucketed once by `(location.file, location.span, kind)`. An output edge with a location binds the
single reference at its exact `(location, kind)`. When several references share one
`(location, kind)` — multi-target heritage clauses (`class C implements A, B`), rules-engine refs
at `Span::ZERO` — a single edge at that key is ambiguous, so the **collision pass** re-runs the
resolver with a single-ref slice for each still-unbound reference of that key and binds the ones
that yield an edge at their `(location, kind)`.

**Resolver contract** (also on the `Resolver` trait doc): a binding edge carries the reference's
exact location **and** kind; an edge with a different kind, or `location: None`, binds nothing
(it is still returned and may survive dedup). `resolve()` must be deterministic per ref —
calling it with a single-ref slice must give that ref's portion of the batch answer — because
the accounting re-runs it per ref for shared-key references.

**Consequences:**

- Repeat sites of a **bound** relationship (the 2nd..Nth call from one function to one target,
  a repeated import of one module) are NOT unresolved. Site multiplicity of a bound relationship
  is **persisted nowhere** — no count, no locations list.
- Every site of an **unbound** relationship keeps its own row (honest, per-site coverage: a name
  with zero candidates — a test framework's `expect` — keeps every row). The persisted row is
  `(from_sym, raw_name, kind, file, line, start_byte, end_byte)` — site identity is byte-exact,
  so two same-line sites (`q(); q();`) are distinguishable in SQL with no on-disk adjudication.
  `start_byte = end_byte = 0` means unknown/synthetic (`Span::ZERO` refs — RulesBridge,
  extra-edge rules — and rows persisted before the byte columns existed carry it legitimately).
- Consumers of this definition: persistence (`index_path` → `upsert_unresolved_refs`), the
  `wicked_estate.resolve.unresolved` telemetry counter, `unresolved_refs_for_name`
  (blast-radius coverage — text CLI, `--json`, and the MCP `BlastRadius` tool), and
  `GraphStats.unresolved_ref_count` (`stats` prints it as `unresolved=N`).

**Computed per resolve pass.** Rows reflect the last pass over each file: changed and deleted
files hit `remove_file` (which drops their unresolved rows) before re-extraction. Exceptions,
all documented:

1. **Unchanged importers** — a ref in an unchanged file stays unresolved until that file changes
   or a full re-index runs (the module-doc Known Limitation of `wicked-estate/src/lib.rs`).
2. **`scip` ingest does not prune** — SCIP edges land, but existing `unresolved_refs` rows are
   pruned only on the next re-index of their file. Deferred, not impossible: a Calls-only prune
   by `(from_sym, kind, target.name == raw_name)` is the named follow-up; a general prune fails
   on Imports (quoted specifier vs canonical node name).
3. **`tfstate` ingest** persists collector refs without a resolve pass — by design (no resolvers
   exist for them).

**Re-index across versions.** A version bump forces full re-extraction on the next `index` (the
per-repo `indexed_version` meta key is compared to the binary version), so stored graphs
self-heal across releases. A **same-version** binary re-extracts only changed files — mixing
definitions silently — so after upgrading a dev build in place, run `wicked-estate index <path>
--force` once.

**Known consumer-side coarseness (pre-existing):** `unresolved_refs_for_name` matches by written
name only — it counts Imports rows in a "call(s)" line and sums across co-located repos in a
labelled graph.

## 3. Confidence tiers (cheap → precise)

| Tier | Default confidence | Who emits it |
|---|---|---|
| `Parsed` | 1.0 | direct AST facts (contains/defines) |
| `Scip` / `Lsp` | 1.0 | precise indexers / on-demand LSP |
| `Tsg` | 0.8 | stack-graphs name resolution |
| `ImportMap` | 0.6 | import-map heuristics |
| `Heuristic` | 0.5 | synthesizers / other heuristics |
| `Tags` | 0.3 | tree-sitter tags only |

On a `(source, target, kind)` collision the **higher-confidence** edge wins (`Edge::dedup_key`).

`RelativeImportResolver` (`resolved_by = relative-import`) emits File→File `Imports` edges under
the `ImportMap` tier with a **per-edge override of 0.9** (an exact joined-path match, adjudicated
on disk; 0.9 not 1.0 because `tsconfig.paths`, symlinks, and case-insensitive filesystems are
unseen). By design this wins `resolve_all_with_coverage`'s max-confidence dedup over a Tsg-default (0.8)
`Imports` edge — a future precise `Imports` emitter must exceed 0.9 or revisit that decision.

### 3.1 Tier activation (derived from the `index_path` resolver slice in `crates/wicked-estate/src/lib.rs`)

Which edge producers actually RUN, per entry point. "yes (slice)" rows are exactly the members of
the production resolver slice — guarded against drift by
`wicked-estate`'s `tests::slice_matches_engine_contract_table`, which parses the slice literal
(anchored by its `// Activation table:` comment) and this table.

| resolver id | tier | confidence | activation | notes |
|---|---|---|---|---|
| tree-sitter extractors (local edges) | `Parsed` | 1.0 | yes (extract phase) | intra-file `Contains`/`Defines`, written before resolution |
| `name-resolver` | `ImportMap` | 0.60 | yes (slice) | unique-name binding; kind deny-list runs pre-uniqueness, cross-family guard post-uniqueness |
| `scoped-name-resolver` | `ImportMap` | 0.60 / 0.62 / 0.65 | yes (slice) | callable-only for Calls; same-file / same-dir / cross-file ranking; family guard pre-ranking |
| `import-map-resolver` | `ImportMap` | 0.63 | yes (slice) | `hints["imports"]`-scoped binding, `via=import-map` |
| `relative-import` | `ImportMap` | 0.9 (per-edge override) | yes (slice) | quoted relative JS/TS specifiers → target File node; exact joined-path match, root-guarded; ambiguity parks (see the override note above §3.1) |
| `infra-resolver` | `Parsed` | 1.0 | yes (slice) | IaC resource refs only (resource-to-resource, or exclusively-resource names) |
| `rules-bridge-resolver` | `Heuristic` | 0.5 | yes (slice) | `rules-engine:*` refs → every `RuleSet` node (N×M by design; no engine-scheme match yet). Overwrites the extractor's own synthetic-RuleSet `InvokedBy` edge on equal confidence (sqlite upsert `>=`) — asserted by `tests/rules_bridge_index.rs` |
| `estate-racf` (`estate_edges`) | `Parsed` / `Heuristic` | 1.0 / 0.5 | yes (estate pass, same index run) | RACF profile → protected assets, exact→Parsed / generic→Heuristic |
| extra-edge rules (`ExtraEdgeExtractor`) | `Heuristic` | 0.5 | yes (extract phase) | `Provenance::Extractor(rule)`; drop-in `.wicked-estate-extractors/*.toml` |
| `scip` (`scip_edges`) | `Scip` | 1.0 | no — separate `wicked-estate scip` command, requires external `index.scip` bytes | precise tier; dominates on dedup |
| `Tsg` | `Tsg` | 0.8 | no production path | enum variant only, no `Resolver` impl (superseded — ADR-007) |
| `Lsp` (`lsp.rs`) | `Lsp` | 1.0 | no production path | client library by design (locked: on-demand only, never bulk); no `Resolver` impl, no edge emission; consumer = W3.6 follow-up |
| `ast-synth-method` | `Heuristic` | 0.5 | retired 2026-08-28 | emit set ⊂ `scoped-name-resolver`; never in any production slice (ADR-007 superseding note) |

Re-index note: a resolver change is not retroactive on an existing DB — `index` re-resolves
changed files only. A `CARGO_PKG_VERSION` bump forces a full re-extract on the next `index`;
`wicked-estate index --force` is the manual path.

## 4. GraphStore contract

Read methods (`get_node`, `find_symbols`, `neighbors`, `traverse`, `stats`) are `&self`; mutation
(`begin_batch`/`commit_batch`/`upsert_*`) is `&mut self`. `traverse` is **bounded only**
(`max_depth` + `max_nodes` required; unbounded whole-graph walks are out — see research/09).
Any new store MUST pass `wicked_estate_core::conformance::graph_store_suite`.

## 5. Rules engine node and edge kinds (W15)

Rules engine entities use first-class `NodeKind` and `EdgeKind` variants:

| NodeKind | Meaning |
|---|---|
| `Rule` | An individual rule (if/then, when/then, allow/deny, decision row) |
| `RuleSet` | A rule container: package, ruleset, policy, decision model |
| `Condition` | The LHS / when / if clause of a rule |
| `Action` | The RHS / then / effect clause of a rule |
| `Fact` | An entity or fact type the rule operates on (data model) |

| EdgeKind | Direction | Meaning |
|---|---|---|
| `Governs` | `Rule → code symbol` | Rule constrains or applies to a code symbol |
| `Evaluates` | `Rule → Fact` | Rule reads/matches on a Fact type (LHS binding) |
| `Produces` | `Rule → Fact` | Rule asserts or modifies a Fact type (RHS output) |
| `InvokedBy` | `code call site → RuleSet` | Code triggers the rules engine at this call site |

Edge direction follows the standard invariant: `source` = dependent, `target` = dependency.
- `InvokedBy`: the call site (source) depends on the RuleSet (target).
- `Governs`: the Rule (source) governs the code symbol (target) — blast-radius of the symbol surfaces the Rule as a dependent.

## 6. Wire contracts (stubs — filled in their waves)

- **MCP tools** (W4.3): the agent surface is `SearchEntity` / `TraverseGraph` / `RetrieveEntity`
  (+ `blast_radius`), each a `RetrievalTool`. Tools return `RetrievalResult { content, diagnostics }`
  where `diagnostics` carries staleness / coverage / `GRAPH-FALLBACK:` markers.
- **SCIP ingestion** (W1.4): SCIP indexer output is normalized into our `Symbol` scheme on ingest;
  merged edges get `provenance = Scip, confidence = 1.0`.
- **Extractor plugin ABI** (W6.1): drop-in extractors in `.wicked-estate-extractors/` emit nodes/edges
  with `provenance = Extractor(name)` and idempotent ids.
