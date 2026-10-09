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
| `Compiler` | 1.0 | a compiler or toolchain catalog's semantic evidence (§3.5) — precise for exactly what its producer profile declares |
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
| `scip` (`scip_evidence` → §3.5) | `Scip` | 1.0 | no — separate `wicked-estate scip` command, requires external `index.scip` bytes | writes the **support plane** (§3.4), owner `(tool_info.name, <repo label or .>:<index path>)`; emits `References` only — SCIP has no call role, so it never emits `Calls` (TS-S2C). Tree-sitter's own edges are the base plane and stay untouched |
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
(`scip`/`compiler`) or `call_derived` evidence class, or a misspelt `@flow.*` anchor, never
classifies, and it **fails the query load** for a plugin or an override (#235). A query-only or
grammar override falls back to the built-in, loudly. A plugin language does not load. A typo can
therefore never drop flow facts silently. Every shipped `.scm` is pinned by a test that walks its
`@flow.*` captures.

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

The two `c`s are **distinct variables**. Until #216 they shared one value slot, because slot
identity was owner-scoped, not block-scoped (`f:local:c`): the merge kept both facts, but a read of
`c` after the block (say a `return c`) saw the outer `c`, whose fact is `may_influence`, while the
merged `value_preserving` belonged only to the inner `c`.

Since #216 (id scheme 4) slot identity follows the binding. A reference resolves to the innermost
declaration of its name whose scope contains it (`@flow.scope*` / `@flow.declare.*` in the query
files). A binding of a nested block or callback is `{owner}:local:{name}@{n}`, where `n` is the
scope's ordinal inside the owner, so a line shift keeps the id. A binding of the owner's own body
keeps `{owner}:local:{name}`, and the owner's parameter is `{owner}:param:{name}`. The two `c`s
above are now `f:local:c` and `f:local:c@1`, and each carries only its own fact. The merge lattice
below still governs any remaining collision.

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
different dedup key. The authoritative, replaceable multi-support model is §3.4 (TS-S2A); a
producer that needs retraction writes there instead of folding into this batch.

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
| CLI `supports edge` (TS-S2A) | **yes** (exact `SymbolId`s only) | the authoritative rows behind one addressed edge; no name resolution, so a slot appears only when its id is passed — same reasoning as the exact-lookup row |
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
| CLI `resolve <name>` / `query --json` | no (default); **yes** with `resolve --include-values` | `is_structural_symbol` on the exact-name `find_symbols` result (#234) — `resolve runs` on a 905-file repo returned 63 slots beside 1 function. `--include-values` is the explicit way back in, mirroring `SearchEntity include_values=true`. Crew's cross-repo symbol search shells out to `resolve <name> --json` and now gets the structural rows |
| `entrypoints` / `leaves` / `dead-code` | no | TS-S1. These match **100%** of value slots by construction (no `Calls`/`Imports` edge in either direction), so `dead-code` had become mostly synthetic noise |
| `BlastRadius` / `TraverseGraph` | **yes** | **unresolved, deliberately out of scope.** Blast radius follows every edge kind by locked contract (the design notes: a blast radius that only follows calls silently under-reports). Value slots hang off `File` by `Contains`, so a File-rooted blast radius surfaces them. Narrowing this needs an explicit contract decision, not a visibility patch. Seeds are already filtered, so `blast-radius <name>` does not start from one |
| CLI `blast-radius --json` `confidence` envelope (#194) | follows the rows | `{min, avg, edge_count}` over the edges that admitted the returned rows: the source is a row and the target is a node the walk reached; `Contains`/`Defines` are excluded. When slots are rows (File-rooted), their `flows_to` admission edges count, as the locked "every edge kind" contract implies. MCP `BlastRadius`'s envelope still averages every walked edge, including `Contains`, so the two can differ on the same graph |
| `graph-view` edges (#194) | no | edges are `Calls`/`Imports` between selected nodes only, keyed `(src, tgt, kind)`. Slots carry neither kind, and selection is the `graph-view` roots row above |

### 3.4 Authoritative, replaceable edge support (TS-S2A)

Implemented by `wicked_estate_core::support`, `GraphWrite::replace_edge_supports` and
`GraphRead::{edge_supports, support_generation, support_owners}`, on every store (MemStore,
SqliteStore, PostgresStore, SurrealStore), and exposed on the CLI as `wicked-estate supports`. §3.2's fold is a one-batch, bounded display: it cannot retract a
fact a producer stopped asserting, and a fact the `flow_support` cap evicted is gone from it. The
**support plane** is the authoritative set a later producer (TS-S2 SCIP, TS-S3/S4 Angular) owns
and replaces — and any other producer (compiler, language server, database, repository,
framework): nothing in the plane is language-specific.

| Question | Contract |
|---|---|
| **Who owns a fact** | exactly one `SupportOwner { producer, snapshot }` — two non-empty opaque strings. `producer` names the asserting system, `snapshot` the unit it re-emits whole (one SCIP index / project root, one compilation unit). |
| **Fact identity (the opaque boundary)** | a fact is a `SupportFact { fact_id, edge }`; within its owner it is identified by `(dedup_key, fact_id)`. `fact_id` is **producer-owned and opaque**: storage compares it byte-for-byte and never parses, trims, case-folds or Unicode-normalizes it — nor any `SymbolId` — so two languages or toolchains whose display names coincide never collide when their ids differ, and a new producer's id scheme needs no core change. The only rule is non-empty and NUL-free (Postgres `TEXT` cannot hold NUL). `SupportFact::from_edge` uses the fact's canonical content (key-sorted JSON) as its id, for a producer without ids of its own. The same id re-asserted with different content is a change (retracted + asserted); one id with two contents in one submission is rejected. The input is a set — order and duplicates never change what is stored or shown. |
| **Replacement** | `replace_edge_supports(owner, generation, facts)` makes `facts` the owner's **complete** set. Facts it held and does not re-assert are retracted; `facts = []` retracts all of them. No occurrence-by-occurrence delete exists. |
| **Generations** | per owner, `u64` in `0..=i64::MAX`, never backwards. Higher → applies. Equal + same set → **idempotent replay** (`replayed: true`, nothing written). Equal + different set → `Error::Invalid` "generation conflict". Lower → `Error::Invalid` "stale generation". An empty replacement keeps the generation, so a stale replay cannot resurrect retracted support. |
| **Atomicity** | all-or-nothing on every backend, inside or outside an open batch: SQLite a `SAVEPOINT`, Postgres a transaction (a `SAVEPOINT` inside a batch), MemStore validate-then-apply, SurrealStore one `BEGIN … COMMIT` query. A rejected or failed replacement does not roll back the caller's batch — unless the engine itself aborts the whole transaction (SQLite `SQLITE_FULL`/`IOERR`/`NOMEM`), which is reported with the original error. |
| **Independence** | no operation on one owner modifies another owner's facts or generation (re-projection reads every owner's facts, by design). Two producers supporting one public edge each retract only their own. |
| **Concurrency** | MemStore, SQLite and SurrealStore are single-writer. PostgresStore (`shared_writers: true`) serializes the support plane with a transaction-scoped advisory lock: `replace_edge_supports` takes it exclusive; `upsert_edges`, `remove_file` and `prune_dangling_edges` take it shared, so a replacement never interleaves with them (generation check-then-write, cross-owner re-projection, first-support base capture). Outside a batch those three calls run in a transaction of their own, so the lock covers the whole call, not one statement. Pinned by `postgres_concurrent_support_replacements_serialize` and `postgres_replacement_rolls_back_a_failure_inside_the_transaction`. |
| **Projection** | the public edge of a supported key is a pure function of the SET `(base contribution, every owner's facts)` (`support::project_edge`, which orders its input by canonical content, `support::fact_key`, before folding, so a tie in §3.2's representative order is never decided by row order), recomputed from those rows on every change — never from the previously projected edge. `flows_to` folds through §3.2's `merge_flow_edges`, so the public envelope is exactly TS-S1's; any other kind takes the max-confidence fact (then max `evidence_count`, then min `fact_key`). |
| **Sample vs identity** | `flow_support` on the projected edge is explanatory and bounded. Identity is `edge_supports(...)`, ordered `(producer, snapshot, fact_id)` by byte order on every backend (Postgres/Surreal sort in Rust, not by the database collation). Evicting a row from the sample cannot change identity, a later projection, or a later representative. |
| **Base plane** | `upsert_edges` is unchanged for unsupported keys. When a key gains its first support, the stored edge is kept aside as its *base contribution*; `upsert_edges` on a supported key updates that contribution by the usual `>=` / evidence rule and re-projects; `remove_file` and `prune_dangling_edges` retire it by exactly the predicate they apply to edges. When the last support is retracted the base contribution is restored byte-for-byte (its stored JSON text, not a re-serialization). |
| **Files and dangling endpoints** | support is producer-owned: `remove_file` and `prune_dangling_edges` never delete a support fact, and a supported edge they remove is re-projected (`prune` does not count it). A projected edge's *owning file* (SQLite/Postgres/Surreal `edges.file`; MemStore's equivalent) is its base contribution's file, or `''` without one — never a support fact's site — so removing a fact's site file does not delete the edge, and a fact's site never counts as a surviving importer in the shared-Import keep (§4). A supported edge whose endpoint is absent stays visible until its owner retracts it. Support naming a `SymbolId` the graph never saw interns it in the store's symbol table (SQLite `symbols`); that row is not a node, and it outlives the retraction, as the store never deletes interned symbols. |
| **Erasure** | `remove_nodes` (SQLite, Postgres) deletes every support fact and base contribution naming an erased symbol. The owner's generation is kept, so replaying the same generation then reports a conflict — bump it. |

**Exactness.** For a key whose base contribution is absent or a single fact, and whose facts are
single (un-merged) edges, `flow_support.len() + flow_support_truncated` equals the number of
distinct support *rows*: the projection is one fold over the authoritative set. A row is keyed by
§3.2's support order, so facts that differ only outside it (line/column, provenance,
`evidence_count`, extra metadata) are distinct facts but one row — count facts with
`edge_supports`. A *pre-merged* fact (an
edge that already carries `flow_support`) contributes its recorded rows and its recorded
truncation, which §3.2 sums; past the cap that count is "at least", and overlapping pre-merged
inputs over-count by the overlap (pinned by `support_replacement_suite`). Staged replacement
(any history of generations) equals a one-shot replacement of the final set exactly; a pre-merged
submission projects like its raw facts only up to the cap, and its support identity is the one
submitted fact, not the facts it folded.

**Migration.** Additive on every backend: SQLite and Postgres create `support_owners`,
`edge_supports` and `edge_base` with `CREATE TABLE IF NOT EXISTS` on open (Postgres keys a fact by
`fact_hash` = SHA-1 of the opaque `fact_id`, because a btree entry is capped at ~2.7 KB); SurrealStore
defines `support_owner`, `edge_support`, `edge_base`. No stored edge is rewritten, and while the
tables are empty every pre-TS-S2A path takes its original branch (plus one `EXISTS` probe per
`upsert_edges`/`remove_file`/`prune_dangling_edges` call; Postgres also takes the shared lock).
With support present, `remove_file`/`prune` touch only the supported rows their own predicate
deletes — no whole-table scan. A pre-TS-S2A SQLite file opened
read-only reads as "no support". Downgrade: an older binary ignores the tables, but it will treat a
projected edge as a plain edge — retract support (`replace_edge_supports(owner, next, [])`) first.

**Edge history.** With history on, `remove_file` archives the public rows its predicate matches —
for a supported key, the projected edge (it is re-projected right after, so history records a
version that is still live). A base contribution retired from a key whose projected row is NOT
deleted is not archived separately.

**CLI.** `wicked-estate supports owners | edge --source --target --kind | retract --producer
--snapshot` is the operator surface: `owners` lists every owner and generation (including emptied
ones), `edge` the authoritative rows behind one public edge (exact `SymbolId`s, no name
resolution), and `retract` is `replace_edge_supports(owner, generation + 1, [])` — the documented
way to clear an owner, e.g. before a downgrade. `--json` documents equal what the store returns
through the trait (`crates/wicked-estate/tests/supports_cli.rs`); the whole document stays under
the one 25K-char R4 budget (rows dropped in order, exact `total`, `truncated`). Strict argv
(nothing opens a store on a bad flag), and a missing or zero-length graph is refused, never
created.

**What it does not do.** The one producer path that writes support is §3.5's semantic-evidence ingest (TS-S2C; the SCIP adapter today, TS-S3/S4's Angular producer next). No retrieval tool,
MCP tool or existing budget changes: `Lineage` and every other surface read the projected edge
through the existing read paths. Support is not exposed over MCP, and the CLI bridge is
untouched (`supports` is not a RetrievalTool).

### 3.5 Semantic evidence — one envelope for every precise producer (TS-S2C)

Implemented by `wicked_estate_core::evidence` (envelope, validation, correlation, projection),
`wicked_estate_resolve::scip_evidence` (the SCIP adapter) and `wicked_estate::{ingest_semantic_evidence,
ingest_scip_report_as}`. Every producer — a SCIP indexer, a compiler, a database catalog — hands the
engine the same versioned `SemanticEvidence` document, and the engine alone decides what it proves.

| Question | Contract |
|---|---|
| **Envelope** | `{schema_version: 1, producer: {name, version, class: index \| compiler, capabilities}, snapshot, generation?, documents: [{path, position_encoding}], facts}`. Any other `schema_version` is rejected and every object denies unknown fields, so a v1 reader never drops a later meaning silently. An invalid envelope writes nothing. |
| **Facts** | `definition {fact_id, symbol, name, site}`, `reference {fact_id, symbol, site, roles?}`, `call {fact_id, site, target: exact{symbol} \| ambiguous{candidates} \| dynamic}`. A `site` is `{document, range}` — 0-based, half-open, columns in the document's `position_encoding` (`utf8`, `utf16`, `utf32`, `unspecified`); a fact must cite a declared document and a range that does not end before it starts. Document paths are repository-relative with `/` separators (no `..`, `.`, empty segment, drive letter or `\`). |
| **Opaque identities** | `producer`, `snapshot`, `fact_id` and every `symbol` are compared byte for byte — never trimmed, case-folded or Unicode-normalized (`Raise` ≠ `RAISE`, NFC ≠ NFD). Only empty or NUL values are rejected, and an owner part (`producer`, `snapshot`) may not be whitespace-only (§3.4's `SupportOwner` rule) — whitespace inside any id is kept and significant. One `fact_id` with two different facts is rejected; exact duplicates are one fact. |
| **Capabilities** | `definitions`, `references`, `calls`. A fact of an undeclared kind is never projected (counted `undeclared_capability`). |
| **Correlation** | candidates are the document's **structural** nodes (value slots, `File` and `Import` nodes excluded). A definition maps to the unique innermost candidate whose span contains the whole site **and** whose `name` equals the fact's `name`. A reference/call site's source is the unique innermost candidate containing the site, or the document's `File` node for a module-level use. "Innermost" means no other candidate nests inside it; two equal or crossing spans are ambiguous. A symbol whose definitions map to two nodes is ambiguous everywhere. Columns in `utf16`/`utf32` are converted to the graph's UTF-8 byte columns against the document's source text when it is readable; otherwise they are used as given and counted (`positions_unconverted`). The input is a set: order never decides. |
| **What projects** | `reference` → `References`, whatever shape the target has. `call` → `Calls` only when the profile declares `calls`, the site is valid, and the target is `exact` and correlates — a declared capability without that site evidence emits nothing. Tier: `Scip` for an `index` producer, `Compiler` for a `compiler` one; `resolved_by` = the producer name; metadata `evidence_producer_version`, `evidence_fact` (`reference`/`call`), and `evidence_roles` when the producer gave any. Definitions are correlation evidence only (the base plane owns `Contains`/`Defines`). Self-references are dropped; a self-call (recursion) is kept. |
| **What never projects** | everything else is counted in the `EvidenceReport` by reason — `document_not_in_graph`, `unmapped_definition`, `ambiguous_definition`, `ambiguous_source`, `unknown_target` (external, local, unmapped), `ambiguous_target`, `dynamic_target`, `self_reference`, `malformed_range` (also a site that cannot exist in the document's known text), and the adapter's `generated`, `forward_definition`, `module_symbol`. The report also counts `edges_projected`, the distinct public edges the projected facts support (several sites can support one edge). No node or target is fabricated. |
| **Ownership and replacement** | accepted facts are §3.4 support facts owned by `(producer.name, snapshot)` and written with one `replace_edge_supports`: complete replacement, producer isolation, last-support restoration, endpoint erasure — unchanged. `snapshot` must be unique within one graph (include the repository for a multi-repo graph). The generation is the envelope's own when it has one (stale/equal rules unchanged), else the owner's stored generation + 1 (1 for a new owner) — which orders ingestion, not source freshness. An empty envelope retracts everything the owner held. |
| **SCIP adapter** | profile `{name: tool_info.name or "scip", version: tool_info.version or "unknown", class: index, capabilities: [definitions, references]}` — **never `calls`**: SCIP's `SymbolRole` has no call role, so `f()` and `const g = f` are the same role-less occurrence. Typed ranges (SCIP 0.9+) win over the deprecated `repeated int32 range`. `Generated` occurrences, bare `ForwardDefinition`s and module symbols (trailing `/`) are dropped and counted; a malformed range is counted, never clamped to a zero span. A definition is named after the symbol's last descriptor; `local N` symbols are qualified by their document (they are document-scoped in SCIP) and named by the document's `display_name` when there is one. Snapshot: `<repo label or .>:<index path relative to the root, or its file name>`. |

**Support matrix (what is real today).**

| Producer | Status | Definitions | References | Calls |
|---|---|---|---|---|
| scip-typescript 0.4.0 | **real**: pinned sample index (`wicked-estate-resolve/tests/fixtures/scip-typescript-0.4.0`), ingested end to end | yes | yes | **no** (no call role) |
| any other SCIP indexer (scip-java 0.13.1 Java/Kotlin, scip-dotnet 0.2.14 C#/VB, scip-clang 0.4.0 C/C++, rust-analyzer, scip-go, scip-python, …) | same adapter, **no pinned sample** in this repo | yes | yes | **no** |
| COBOL, PL/SQL, ABAP, RPG | **contract fixtures only** (`wicked-estate/tests/fixtures/evidence/`) — no producer integration exists | fixture | fixture | PL/SQL fixture only (modelled on PL/Scope `USAGE = 'CALL'`) |

A contract fixture proves that the envelope can carry a toolchain's identity shapes, not that the
language is supported, precise or taint-ready. None of this is data-flow or taint analysis: a
`References` or `Calls` edge says *what is used or invoked where*, nothing about values.

**Migration.** Before 0.24.0 the `scip` command wrote `Calls`/`References` into the base plane
(start-line correlation over all nodes, so since TS-S1 it could land on value slots). Those
base edges are file-owned: the version bump's forced full re-extract on the next `index` retires
them, and `scip` then writes support. No schema change.

**Not yet.** No CLI for a raw envelope (`wicked-estate evidence ingest`) and no MCP tool: the library
entry point is `ingest_semantic_evidence`. No document digests and no stored record of unprojected
facts (the report is returned, not persisted).

## 4. GraphStore contract

Read methods (`get_node`, `find_symbols`, `neighbors`, `traverse`, `stats`) are `&self`; mutation
(`begin_batch`/`commit_batch`/`upsert_*`) is `&mut self`. `traverse` is **bounded only**
(`max_depth` + `max_nodes` required; unbounded whole-graph walks are out — see research/09).
Any new store MUST pass `wicked_estate_core::conformance::graph_store_suite`, and — run on a fresh
store — `multi_file_contribution_suite` and `support_replacement_suite` (§3.4).

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
