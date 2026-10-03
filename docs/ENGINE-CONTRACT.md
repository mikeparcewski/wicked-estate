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
- **Semantic value flow** stores `flows_to` as `EdgeKind::Other("flows_to")` with the same
  invariant: `source` is the consumer and `target` is the producer. A user-facing producer →
  consumer lineage query therefore walks `Direction::Dependents` over only `flows_to` edges. The
  default `Lineage` query remains dependency lineage over `Calls` + `Imports` unless callers opt in
  with `relation = "flows_to"`. What such an edge **claims**, and how we know it, is §3.2.

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

1. **Unchanged importers/callers** — mostly closed by the back-fill pass (lane
   importer-backfill, #141): when a run (re-)extracts a definition name, parked refs with that
   exact `raw_name` are re-resolved (indexed lookup per name); when a run indexes a NEW file
   path, parked relative-import refs are re-resolved against the File map. A re-resolved row is
   deleted in the same batch that writes its edge; a still-parked row stays put, never
   re-inserted. Residual (the module-doc Known Limitations of `wicked-estate/src/lib.rs`): a
   ref parked for AMBIGUITY is re-checked only when a same-name definition is (re-)extracted —
   a deletion that makes the name unique does not trigger it — and back-fill resolves without
   hints (`ImportMapResolver` never fires on stored rows). Those refs still wait for their
   file to change or a full re-index.
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
| tree-sitter extractors (local edges) | `Parsed` | 1.0 | yes (extract phase) | intra-file `Contains`/`Defines`, plus `flows_to` edges whose capture declares `syntax` evidence (`assignment` and `property_read` → `value_preserving`, `expression` → `may_influence`), and the engine-emitted `return` flow (`value_preserving`; not a query-file capture), written before resolution; stored direction consumer→producer, `resolved_by = tree-sitter`, `Provenance::Parsed`, classified per §3.2 |
| tree-sitter **convention** flow (`tree-sitter-convention`) | `Heuristic` | 0.5 | yes (extract phase) | `flows_to` edges whose capture declares `convention` evidence — today Angular `@Input()` and `route.snapshot.paramMap.get(…)`. The AST proves the *shape*; it does **not** prove `@angular/core` identity or that the receiver is an `ActivatedRoute`, so these are heuristics, not parsed facts. Carries a stable `flow_rules` id (§3.2). Downgraded from `Parsed`/1.0 in TS-S1 |
| call-derived value flow (main pass) | inherited from resolved `Calls` edge | inherited from resolved `Calls` edge | yes (post-resolution, same index run) | derives `flows_to` call-argument and call-result edges only from exact-site `Calls` bindings with one unique accepted target; stored direction remains consumer→producer, while `Lineage` `relation = "flows_to"` walks dependents for semantic-forward producer→consumer output |
| call-derived value flow (back-fill) | inherited from resolved `Calls` edge | inherited from resolved `Calls` edge | yes (parked-ref back-fill, same index run) | when a previously parked call binds after another file appears, re-extracts call-site hints from stored source text and emits the same exact-site call-argument/call-result `flows_to` edges before deleting the parked ref |
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

On-demand LSP note (`docs/adr/ADR-009-intent-routed-lsp.md`): the Lsp row above is
deliberately unchanged by the phase-0 lsp.rs fixes (transport deadline timeout, didOpen) —
phase-0 changes **no activation**. ADR-009 defines the W3.6 consumer: position-anchored
edit-plane MCP tools + CLI twins that call `LspTier` on demand, outside every resolver
slice; the understand plane (BlastRadius/Lineage/SearchEntity/hotspots) never consults LSP.
When that consumer lands, only the Lsp row's notes cell changes, and its activation cell
stays out of the slice-guarded set.

### 3.2 Semantic value flow — what a `flows_to` edge claims (TS-S1)

Implemented by `wicked_estate_core::flow`. The relation tag `flows_to` is unchanged and public;
TS-S1 made its **meaning** machine-readable instead of leaving it in one scalar string.

#### The two orthogonal dimensions

A flow edge answers two independent questions. Conflating them is how a tool starts presenting a
0.5-confidence guess as a fact (agent rule R7).

| Dimension | Metadata key | Values | Meaning |
|---|---|---|---|
| **Flow semantics** — what the edge CLAIMS | `flow_semantics` | `value_preserving` | the producer's value becomes the consumer's value, *whole* |
| | | `may_influence` | the producer *contributes* to it. `const c = a + b` gives `c may_influence a` — it is **not** a claim that `c`'s complete value is `a` |
| **Evidence origin** — HOW we know | `flow_evidence` | `syntax` | the AST proves this fact at this site |
| | | `call_derived` | derived from a *resolved* `Calls` edge |
| | | `convention` | a framework naming/shape match the parser cannot prove |
| | | `scip` | **RESERVED, not emitted** — a verified SCIP projection (TS-S2) |
| | | `compiler` | **RESERVED, not emitted** — a framework compiler fact (TS-S3/TS-S4) |

Both keys hold an array sorted in the enum's **declared** order (the order of the table above, so `["value_preserving","may_influence"]`, not lexicographic), never a scalar — see "endpoint dedup" below. `constructs` and `flow_rules` are sorted lexicographically. Evidence
*strength* stays where this contract already put it: `confidence`, `provenance`, `resolved_by`.
The two reserved words exist so that a convention match can never later be relabelled as a
compiler proof; `FlowEvidence::is_emitted()` is the tripwire.

**What the evidence does not prove.** No flow edge carries CFG, SSA, path-sensitivity, alias or
heap reasoning. `a flows_to b` means "on some path, by the stated evidence, a value may reach b".
It does not mean it always does, nor that no other value does.

**Direction, again.** Stored `source = consumer`, `target = producer`. Semantic-forward lineage
("where does this value go?") walks `Direction::Dependents`. Do not reverse this for display
convenience; `Lineage` renames the ends in its response instead.

#### Call-derived flow inherits its cause

A call-argument or call-result edge takes the causal `Calls` edge's `confidence`, `provenance` and
`resolved_by` **wholesale**. The callee being *uniquely* selected is not evidence about how well
the call resolved: a 0.5 `name-resolver` binding yields 0.5 flow. Uniqueness never upgrades a
heuristic call to `Parsed`. Ambiguous or unresolved calls emit no flow at all.

#### Framework conventions are not compiler facts

Angular `@Input()` and `route.snapshot.paramMap.get('id')` are matched by *shape*. An identifier
named `Input` is not necessarily `@angular/core`'s `Input`; a receiver named `route` is not
necessarily an `ActivatedRoute`. Those edges are emitted at the `Heuristic` tier with
`resolved_by = tree-sitter-convention` and a stable rule id in `flow_rules`
(`typescript/convention/angular_input`, `typescript/convention/route_param`). Template wiring and
`@angular/core` identity are **not** claimed and are not resolved.

#### Rules stay data

The classification is declared in the query file, not in Rust:

```
@flow.<semantics>.<evidence>.<construct>     e.g. @flow.influence.syntax.expression
```

`treesitter.rs` parses those three segments structurally and knows nothing about what
`angular_input` means. The rule id is derived — `<language>/<evidence>/<construct>` — so a new
construct or a new language mints its own id with zero core change. A capture naming a reserved
(`scip`/`compiler`) or `call_derived` evidence class is **ignored**: it is never emitted, but it
does not fail the query load either (nor does a misspelt anchor), so a typo drops silently.

#### Endpoint dedup: why the vocabulary is set-valued

An edge is keyed `(source, target, kind)` (`Edge::dedup_key`); metadata, location and provenance
are not in the key, and every store's `upsert_edges` replaces the whole row at
`confidence >= stored` — i.e. **last-writer-wins on a tie**. Two flow facts sharing endpoints
therefore collapse. This is reachable in ordinary TypeScript via block-scoped shadowing:

```ts
function f(a: string, b: string) {
    const c = a + b;          // c -> a  may_influence,    byte 62
    if (b) { const c = a; }   // c -> a  value_preserving, byte 116
}
```

The two `c`s are **distinct variables** that share one value slot, because slot identity is
owner-scoped, not block-scoped (`f:local:c`). The merge keeps both facts, but a read of `c` after
the block (say a `return c`) sees the outer `c`, whose fact is `may_influence`: the merged
`value_preserving` belongs only to the inner `c`. Scope-sensitive slot identity is TS-S2 work.

Measured on `c4fa938`, exactly one survived (`construct="assignment"`, byte 116) and the
may-influence contribution vanished with nothing recording that it had been asserted.

`wicked_estate_core::flow::merge_flow_edges` folds such a group through a deterministic lattice
**before** the batch reaches a store: set union for every classification key, `max` confidence
(matching the stores' own `>=`, so the merge is upsert-stable), the minimum over every
contributing fact recorded in `flow_confidence_min` when it is below the edge's confidence (read
from the support rows and any prior key, so a second fold equals one fold over everything), and
the representative fact chosen by a total order that contains no insertion index. Every contributing fact keeps a row in `flow_support`
(`{construct, semantics, evidence, rule, confidence, resolved_by, file, line, start_byte,
end_byte}`), capped at 8 with `flow_support_truncated` (R4); a fact's identity includes its
confidence, and the representative's own row always survives the cap. The result is a pure
function of the input *set*, and folding in stages equals one fold, exactly up to the cap. Past
it, the dropped facts' identities are not kept: which other rows survive can depend on batching,
and `flow_support_truncated` sums each fold's drops, so it is exact only for a single fold.

The legacy scalar `metadata.construct` stays readable: it is the lexicographic minimum of
`constructs`. It is a lossy summary **by construction** — `constructs` is the whole truth.

**Scope.** This is a one-batch fold, not an occurrence table: it has no retirement semantics and
cannot merge across two batches that reach the store separately. The only reachable cross-batch
pairing is tree-sitter parsed facts vs. post-resolution call-derived facts, and those are
endpoint-disjoint — parsed flow joins two values owned by one callable (or a class field), while
call-derived flow joins the *callee's* parameter/return slot to the *caller's* local, so a
collision needs `caller == callee` **and** an assignment in the reverse direction, which is a
different dedup key. The authoritative, replaceable multi-support model is TS-S2A's seam.

### 3.3 Visibility matrix for synthetic value slots

Value slots (`metadata.value_role`) reuse ordinary `NodeKind`s and carry the bare source
identifier, so nothing else distinguishes a local named `map` from the function `map`. The single
predicate is `wicked_estate_core::flow::is_structural_symbol`. Each consumer's decision is
explicit — a node hidden from human-facing search is **not** automatically hidden everywhere.

| Surface | Value slots visible? | Mechanism |
|---|---|---|
| Raw storage / `export` / `nodes` CLI / `GraphStats` | **yes** | deliberate: a faithful view of storage must stay faithful. A filtered `export` would make the file an unreliable basis for diffing a graph |
| Exact `SymbolId` lookup (`RetrieveEntity`, `FetchContent`, `get_node`, `graph-view --focus <id>`) | **yes** | deliberate: you addressed this node |
| `Lineage relation=flows_to` (MCP) / `lineage --relation flows_to` (CLI) | **yes** | the explicit semantic query; this is the whole point. `flows` and the `confidence` summary list only flow hops whose two ends are both in the answer; `dependencies` and `flows` share one R4 budget, and a row dropped from either sets `truncated`. The CLI invokes the same tool and its `--json` is the same `RetrievalResult` plus the MCP server's own staleness line (`crates/wicked-estate/tests/lineage_cli.rs`) |
| `Lineage` start (MCP `symbol`) / `lineage --symbol` (CLI) | **yes** (exact `SymbolId` only) | no name resolution on either frontend, so a slot is reached only by its id — same reasoning as the exact-lookup row |
| `SearchEntity include_values=true` | **yes** | explicit opt-in. The default path is the one with a diagnostic naming the hidden count and this way back in; this path hides nothing, so it has none |
| Default name/FTS search (`SearchEntity`, `wicked_estate::search`, CLI `query`) | no | `find_seed_symbols` / `is_structural_symbol` |
| `ContextPack` / `ContextBundle` seeds | no | `find_seed_symbols` |
| `ContextPack` body (`render_context` tail-fill) | no | `is_structural_symbol` — TS-S1 (seeds were fixed in #207, the body was not) |
| `budget_context` neighbours + its supplementary FTS pass | no | `is_structural_symbol` — TS-S1 |
| Resolver candidates (any ref kind, incl. `Calls`) | no | `admissible_target` |
| Ranked symbols / `RankHotspots` / `important_symbols` / `pagerank.top` cache (write **and** read) | no | the `excluded` set in `pagerank_inner` + read-time hygiene for caches written by an older binary |
| **PageRank input graph** | **yes** | deliberate: value slots carry no `Calls`/`Imports` edge, so they are isolated vertices. Removing them from the input would renumber the uniform teleport denominator and change *every* real symbol's score. Keeping them in the input and filtering the output leaves eligible symbols' scores and order byte-identical |
| Communities / cluster summaries | no | excluded from `detect_communities`' node set — necessary because `package_bias > 0` rings every node in a directory together, which would wire locals into real communities |
| `SemanticSearch` | no | filtered at read, before `k` is applied (bounded over-fetch, with a diagnostic if the candidate cap still leaves fewer than `k`). **Residual:** embeddings are still computed for value slots, so they occupy ANN index space; filtering at write would need an embeddings backfill |
| `graph-view` roots | no | roots come from `important_symbols`; the `--focus` *by name* path is filtered (before its 5-seed cap, so same-name slots cannot crowd out the real symbol), `--focus` *by id* is not (see exact-lookup row) |
| `Path` (MCP) / `path` (CLI) | by name: no; by `SymbolId`: **yes** | a bare name resolves with `!is_value_flow_node()` (`path.rs`); pass the exact `SymbolId` to route from or to a slot |
| CLI `resolve <name>` | **yes** (pre-existing, not yet decided) | raw `find_symbols` by exact name, so e.g. `resolve runs` can return mostly slots. Crew's cross-repo symbol search calls it; filtering it is an open follow-up |
| `entrypoints` / `leaves` / `dead-code` | no | TS-S1. These match **100%** of value slots by construction (no `Calls`/`Imports` edge in either direction), so `dead-code` had become mostly synthetic noise |
| `BlastRadius` / `TraverseGraph` | **yes** | **unresolved, deliberately out of scope.** Blast radius follows every edge kind by locked contract (the design notes: a blast radius that only follows calls silently under-reports). Value slots hang off `File` by `Contains`, so a File-rooted blast radius surfaces them. Narrowing this needs an explicit contract decision, not a visibility patch. Seeds are already filtered, so `blast-radius <name>` does not start from one |

## 4. GraphStore contract

Read methods (`get_node`, `find_symbols`, `neighbors`, `traverse`, `stats`) are `&self`; mutation
(`begin_batch`/`commit_batch`/`upsert_*`) is `&mut self`. `traverse` is **bounded only**
(`max_depth` + `max_nodes` required; unbounded whole-graph walks are out — see research/09).
Any new store MUST pass `wicked_estate_core::conformance::graph_store_suite`.

**`remove_file` and shared Import nodes (incr-integrity lane).** `remove_file(f)` removes `f`'s
nodes, edges, and unresolved rows — with ONE exception: a `NodeKind::Import` node located in `f`
is **kept** when at least one *survivor* edge still targets it (an edge whose file is neither `''`
nor `f` and whose source node does not live in `f`). Import nodes are keyed by module specifier
and shared by every importer of the same spec; the unconditional delete used to strand every other
importer's `File→Import` edge (dangling after a deletion-only run, then silently pruned — a §2.1
violation). A kept node is re-homed — both its `file` column and its `location` — to the
deterministic MIN(file) over its survivor edges, so removing the LAST importer deletes it through
the normal path (no islands, no GC pass). Evaluated per `remove_file` call, never against a
batch-start snapshot. Pinned by the conformance kit's shared-Import section.

**Healing an already-damaged DB.** A graph written by a binary from before this fix may carry the
residual dangling edges; they keep the old fate (pruned with a `GRAPH-CLEANUP` log line on the
next run WITH changes). The documented heal is a one-off `wicked-estate index <path> --force` full
re-index.

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
