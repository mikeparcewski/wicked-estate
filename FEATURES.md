# wicked-estate — Feature Inventory

**wicked-estate** is a greenfield Rust engine that turns a repository — and its surrounding
infrastructure/mainframe **estate** — into one queryable graph (symbols, calls, imports, types,
refs, resources) so LLM agents get fast, precise intelligence: definitions, who-calls-X,
blast-radius, scoped context, and cross-domain estate links. Local-first, single static binary,
tree-sitter + SQLite.

This document is an exhaustive inventory — no stone left unturned. Each feature is tagged:
**✅ built+tested** · **🟡 partial / not-yet-benchmarked** · **🟦 designed, not built** (the seam exists).

> **Status at time of writing (2026-08-29, main):** `cargo test --workspace` = **1,370 passing,
> 0 failed, 1 ignored** (one marker-ignored doc-test); `cargo build --workspace` = 0 warnings;
> `cargo clippy --workspace --all-targets -D warnings` clean.
> Binaries: `wicked-estate` + `wicked-estate-mcp`. 15 crates, `wicked-estate-core … wicked-estate-observe`.

---

## 1. Architecture — the trait spine + 15 crates

The whole engine programs against **five traits** in `wicked-estate-core` (the spine); everything else is a
swappable impl behind a seam. This is why the work fanned out safely in parallel.

| Crate | Role |
|---|---|
| `wicked-estate-core` | Types + the five traits + the GraphStore **conformance kit** (the spine) |
| `wicked-estate-extract` | `Extractor` impls — tree-sitter (104 wired of the 115 manifest languages) + grammar-less line/macro extractors |
| `wicked-estate-resolve` | `Resolver` impls — name / scoped / import-map / relative-import / infra / rules-bridge / SCIP / estate; LSP client library (consumer designed, ADR-009) |
| `wicked-estate-store` | `GraphStore` impls — `MemStore` (reference) + `SqliteStore` (default) |
| `wicked-estate-rank` | `Ranker` — personalized PageRank over CALLS/IMPORTS |
| `wicked-estate-retrieve` | `RetrievalTool` impls — the agent-facing query API + RRF hybrid + embedders |
| `wicked-estate-mcp` | MCP server exposing the retrieval tools over JSON-RPC |
| `wicked-estate` | the `wicked-estate` binary |
| `wicked-estate-bench` | agent-eval benchmark harness + capability matrix (the truth oracle) |
| `wicked-estate-observe` | OTLP HTTP/JSON exporter (`OtlpSink`) + emission sites (§13) |
| `wicked-estate-overlay` | internal — absorbed from wicked-overlay (XedgeStore cross-engine search layer) |
| `wicked-estate-memory-core` | internal — absorbed from wicked-memory (`MemoryApi` trait + request/query types) |
| `wicked-estate-memory` | internal — absorbed from wicked-memory (memory engine impl) |
| `wicked-estate-knowledge` | internal — absorbed from wicked-knowledge (knowledge engine impl) |
| `wicked-estate-memory-api` | internal — absorbed from wicked-memory (shim crate for clean re-exports, ADR-008) |

**The five traits** (`wicked-estate-core/src/traits.rs`): ✅
- `Extractor` — source file → `Extraction { nodes, local_edges, refs }`
- `Resolver` — `UnresolvedRef[]` + `SymbolIndex` → `Edge[]`
- `GraphRead` / `GraphWrite` (+ `GraphStore` supertrait) — storage
- `Ranker` — importance scoring
- `RetrievalTool` — agent-facing query, JSON in/out

**Core type system** (`wicked-estate-core`): ✅
- `Symbol` / `SymbolId` — **stable identity** (scheme + name; never content-hash or line number — ADR-002)
- `Node` + `NodeKind` (20 variants: File, Module, Namespace, Class, Struct, Enum, Interface, Trait, Function, Method, Constructor, Field, Constant, Variable, Parameter, TypeAlias, Macro, Import, Synthetic, `Other(String)`)
- `Edge` + `EdgeKind` (12 variants: Contains, Defines, Calls, Imports, References, Instantiates, Implements, Extends, Overrides, HasType, Returns, `Other(String)`)
- `Confidence` (f32, clamped 0–1) + `Provenance` + `ResolutionTier` — **every edge carries confidence + provenance + resolved_by** (no bare edges; heuristic edges never presented as fact)
- `Edge::new(src, tgt, kind, tier, resolved_by)` — derives confidence + provenance from the tier
- `UnresolvedRef` / `Extraction` — two-phase EXTRACT → RESOLVE staging
- **Edge direction invariant**: `source = dependent`, `target = dependency`. Blast-radius = transitive dependents.

**GraphStore conformance kit** (`wicked-estate-core/src/conformance.rs`): ✅ — a shared test battery every store
MUST pass (direction invariant, bounded reverse-reachability, depth caps, semantics round-trip, …).
Both `MemStore` and `SqliteStore` pass it.

---

## 2. Language extraction

**Coverage: 115 languages in the manifest** (80 structural / 18 tags / 17 document — the manifest,
`languages.toml`, is the canonical count), **104 wired** for tree-sitter extraction + **6
grammar-less mainframe extractors**.

### Rules-as-data (the core design) ✅
- Languages are **data**, not code: `wicked-estate-extract/languages.toml` (one row per language) + a
  `queries/<name>.scm` tree-sitter query file. A new language = a manifest row + a `.scm` + one
  `LangEntry` — **zero core logic change**. No per-language `match lang { … }` arms anywhere.
- The **capability matrix is generated** from the manifest (`docs/language-coverage-matrix.md` via
  `scripts/gen-coverage-matrix.py`) — not hand-maintained.
- A `≥73` parity test gates regression against the language set.
- `every_wired_query_compiles` test — guards that **every** wired `.scm` compiles against its
  grammar (a query referencing a missing node type would silently disable a language; this catches it).

### tree-sitter extractor (`TreeSitterExtractor`) ✅
- Generic, driven by `.scm` capture conventions: `@code_<kind>.def` + `@code_<kind>.name` for
  definitions; `@call.function` / `@call.method` for calls; `@import` / `@import.source` for imports;
  `@code_extends`/`@code_implements` heritage anchors.
- Emits a `File` node per source file + `Contains` edges to its definitions.
- **Quote-stripping at the shared seam** — string-literal call targets (e.g. COBOL `CALL 'SUB'`) are
  de-quoted so they resolve cross-file/cross-language.
- Skips minified / oversized files.
- Wired grammar families: official `tree-sitter-*` crates + the **arborium** ABI-15 grammar family
  (uniform `language()` API), on the tree-sitter 0.25 runtime (ABI 13–15).
- Notable recently-wired: `swift` (ABI-15, un-deferred after the 0.25 bump), `solidity`, `d`,
  `verilog`, `vhdl`, `thrift`, `starlark`, `cuda`, `arduino`, `apex`, `racket` — each query authored
  against the grammar's real `node-types.json` and verified to extract.

### Grammar-less mainframe extractors (`wicked-estate-extract/src/*.rs`) ✅
The `Extractor` trait is generic — tree-sitter is just one impl. Card/line formats with no grammar
get hand-written line/macro extractors:
- **JCL** (`jcl.rs`) — JOB→Module, in-stream/cataloged `PROC`→Module that contains its steps,
  `EXEC PGM=`/`EXEC proc`→step + cross-language Calls ref, `DD DSN=`→dataset node + `uses` edge
  (handles GDG `(+1)` / PDS `(MEMBER)` suffixes), and a `dd_binding` `<PGM>.<DD>` per DD of a
  program step, which the COBOL `SELECT … ASSIGN TO <DD>` of that program links to.
- **HLASM assembler** (`hlasm.rs`) — CSECT/START/RSECT→Module, DSECT, CALL/EXTRN/WXTRN→Calls refs
  (before the first CSECT: from the File node). BMS map source (`.bms`): DFHMSD→`bms_mapset`,
  DFHMDI→`bms_map`, the definition of the map a CICS `SEND/RECEIVE MAP` links to.
- **RACF security** (`racf.rs`) — RDEFINE→profile, ADDSD→dataset profile, ADDGROUP/ADDUSER,
  PERMIT→`permits`, CONNECT→`member_of`; a PERMIT/CONNECT id with no ADDUSER/ADDGROUP keeps its edge
  through a boundary node with a reason. (security estate)
- **IMS DB** (`ims.rs`) — DBD→database, SEGM→segment (+ `Contains` + `parent`), PSB→`ims_psb` node
  (named by `PSBGEN PSBNAME=`) with PCB→`accesses` and SENSEG→`sensitive_to`. Macro detection is **keyword-based** (robust to indentation). (data estate)
- **MQ MQSC** (`mq.rs`) — DEFINE/ALTER QLOCAL/QREMOTE/QALIAS/QMODEL→queue, CHANNEL, TOPIC;
  QREMOTE RNAME / QALIAS TARGET→`resolves_to`; continuation-line joining; names de-quoted. (messaging estate)
- **COBOL text supplement** (`cobol_links.rs`) — what the grammar can't see, every link from the real
  program Module: `EXEC CICS LINK/XCTL`→`Calls` the program (any language), `SEND`/`RECEIVE MAP`→
  `writes`/`reads` the map, `EXEC SQL`→`reads`/`writes` the `db2_table` (SELECT/cursor/subselect vs
  INSERT/UPDATE/DELETE/MERGE), `COPY`/`EXEC SQL INCLUDE`→`Imports` the copybook (a copybook file
  defines it and `Contains` its fields), `SELECT … ASSIGN TO`→`uses` the JCL DD binding, field uses
  →`References`, and a dynamic `CALL VAR` whose variable holds one literal→`Calls` at Heuristic 0.5.
  A shared target with no defining source indexed is a boundary node with a `boundary_reason`.

### Advanced COBOL / copybook ✅ (via `cobol.scm`)
- PROGRAM-ID → named Module (so JCL/HLASM cross-program refs resolve to it).
- Paragraphs / sections → Function; CALL / PERFORM → call refs.
- Data items → Field nodes (COMP / COMP-3 / Zoned / Signed usages enter the graph).
- `REDEFINES` → reference edge; `OCCURS … DEPENDING ON` → reference to the counter field.

### In-house grammar authoring ✅
- Method: template from a battle-tested grammar (JS/Python) → `npx tree-sitter-cli generate` →
  vendored `parser.c` + `build.rs` (cc) + Rust binding + corpus **parse-gate** (zero ERROR nodes) +
  **extraction-count comparison** = "manufactured battle-testing."
- Delivered (legacy enterprise stacks with no usable upstream grammar): **RPG IV** (free-format),
  **Progress OpenEdge ABL** (upstream parser.c was ~97MB — too big to vendor), **PowerBuilder
  PowerScript**, **Visual FoxPro**, **LotusScript**, **Informix 4GL**, and **Crystal Reports
  formulas** — each `vendor/tree-sitter-<lang>/`, excluded from the workspace, with a
  `<lang>_grammar.rs` parse-gate + extraction test. (CFML, VB6/VBA/VBScript are vendored from
  upstream grammars; VB.NET, Delphi/Pascal use crates.io grammars.)

### Runtime language plugins ✅ (`src/plugin.rs`, see [PLUGIN.md](./PLUGIN.md))
- A language can be added **without compiling it into the core**: drop a directory —
  `lib<name>.{so,dylib,dll}` (compiled tree-sitter grammar) + `<name>.scm` (query) + `plugin.toml`
  (manifest) — into `$WICKED_ESTATE_PLUGINS` (default `~/.wicked-estate/plugins`).
- Loaded at startup via `libloading` (`dlopen`), ABI-checked (13–15), and registered. Precedence is
  three-tiered (ADR-010): built-in < query-only override (`override_query` in the manifest — user
  `.scm`, shipped grammar) < full grammar override (`override = true` AND the language named in
  `WICKED_ESTATE_PLUGIN_OVERRIDE`). An additive plugin (no override fields) still cannot shadow a
  built-in; an unloadable or ABI-incompatible plugin is skipped with a warning, never aborts; a
  broken override query falls back to the built-in, loudly. Any override change forces a full
  re-extraction at the next index (per-repo `plugin_overrides` audit key).
- **License isolation:** the grammar is a separate binary artifact, never linked into the MIT core at
  build time, so a grammar under an incompatible license (GPL, etc.) stays isolated. The only added
  deps are `libloading` (ISC) + `tree-sitter-language` (MIT) — both permissive.
- `wicked-estate plugins list` enumerates loaded plugins. Worked example:
  [`examples/plugins/nginx`](./examples/plugins/nginx) (Apache-2.0, deliberately ≠ the MIT core).

### Rules engine extractors ✅ (W15)
Business-rules logic is extracted into the **same graph as code**, so cross-domain queries work
(e.g. "what code calls this ODM rule set?"). NodeKind `{Rule, RuleSet, Condition, Action, Fact}` +
EdgeKind `{Governs, Evaluates, Produces, InvokedBy}`; `RulesBridgeResolver` connects code call sites
to real `RuleSet` nodes, and the `RulesInventory` MCP tool lists engines + calling code.

| Extractor | Source | Captures |
|-----------|--------|----------|
| `OdmExtractor` | IBM ODM BAL/IRL text | RuleSet, Rule, Condition, Action |
| `CamundaDmnExtractor` | Camunda DMN XML (`.dmn`) | RuleSet, Rule, Condition, Action |
| `DroolsGdstExtractor` | Drools GDST XML (`.gdst`) | RuleSet, Condition, Action |
| `ClipsExtractor` | CLIPS/Jess S-expressions (`.clp`) | RuleSet, Rule, Condition, Action, Fact |
| `ExcelRulesExtractor` | Excel decision tables (`.xlsx`) | RuleSet, Rule, Condition, Action, Fact |
| `SalesforceFlowExtractor` | Salesforce Flow XML (`.flow-meta.xml`) | RuleSet, Rule, Condition |
| `AwsConfigRuleExtractor` / `AzurePolicyExtractor` | AWS Config / Azure Policy JSON | Rule, Condition, Action |

XML/Excel extractors are feature-gated (`xml-rules` / `excel-rules`). Blocked for lack of a grammar or
specimens: Drools DRL, OPA/Rego, Corticon, FICO Blaze.

### Known niche gaps 🟡
- VSAM **AIX** alternate-index modeling; ESDS/RRDS vs KSDS access-method distinction.
- **RECFM** (VB/FBA) record formats in JCL DCB — not parsed.
- COMP-3 *usage metadata* surfaced as queryable node attributes (fields exist; PIC/USAGE not yet a field).
- Deferred languages: `nim` (no crate), `pony` (grammar binding is tree-sitter 0.20 — needs a build-shim),
  `asm`/`odin` (3-strikes, parked).

---

## 3. Resolution (two-phase EXTRACT → RESOLVE)

Resolution is swappable and **never requires re-parsing**. Resolvers consume `UnresolvedRef`s +
a `SymbolIndex` and emit confidence-rated `Edge`s. `resolve_all_with_coverage` — the **single**
entry point since 0.15.0 (the edges-only `resolve_all` wrapper was removed; per-ref coverage
accounting is not optional) — runs the cascade, attributes edges to references per site, and
dedups by `(source, target, kind)`, keeping the highest-confidence edge.

**Resolution tiers** (`ResolutionTier`, cheap→precise, each with a default confidence): ✅
`Parsed` (1.0) · `Tags` (0.3) · `ImportMap` (0.6) · `Heuristic` (0.5) · `Tsg` (0.8) · `Scip` (1.0) · `Lsp` (1.0).

**Resolver impls** (`wicked-estate-resolve`):
- `NameResolver` ✅ — binds a ref to a project symbol by **unique** name (ambiguous → deferred).
- `ScopedNameResolver` ✅ — prefers same-file then same-directory candidate (records the reason in edge metadata).
- `ImportMapResolver` ✅ — uses the per-file import map (`UnresolvedRef.hints["imports"]`) to cut same-name ambiguity.
- `RelativeImportResolver` ✅ — binds quoted relative specifiers (`./x`, `../y`) in JS/TS/TSX to their
  target File node as `Imports` edges (`resolved_by = relative-import`, ImportMap tier with a per-edge
  0.9 override; exact joined-path match, root-guarded, ambiguity parks).
- `InfraResolver` ✅ — resolves IaC/tfstate resource refs (resource-to-resource only).
- `RulesBridgeResolver` ✅ — connects code call sites to real `RuleSet` nodes (`rules-engine:*`
  InvokedBy edges; wired into the production `index` slice since 0.15.0).
- `scip_evidence()` ✅ — translates a SCIP index (`index.scip`) into the TS-S2C semantic-evidence envelope; `ingest_scip_report_as` correlates it against structural nodes and writes **precise `References`** into the support plane (`docs/ENGINE-CONTRACT.md` §3.5). SCIP has no call role, so it never emits `Calls`; trusted calls come only from a producer that declares `calls` and supplies call sites.
- `estate_edges()` ✅ — **cross-domain estate join** (see §7): RACF profiles → datasets / MQ assets by **RACF generic-pattern matching** (`%` / `*` / `**`, most-specific-wins), exact→Parsed, generic→Heuristic.
- **On-demand LSP client library** (`lsp.rs`) 🟡 — a working JSON-RPC stdio client driving installed
  language servers (typescript-language-server, rust-analyzer, pyright) for precise single-symbol
  definition/refs/hover. **Built as a library; it has no production caller yet** — the sanctioned
  consumer is the intent-routed **edit plane** (`Definition`/`References`/`Hover`, exactly one
  `(file, line, col)` per call; designed in ADR-009, implementation is the W3.6 lane). The locked
  decision stands: **on-demand only — never bulk**; the understand plane (BlastRadius, Lineage,
  search) never consults LSP.

Production `index`/`watch` resolver slice (activation table: `docs/ENGINE-CONTRACT.md` §3.1):
`NameResolver → ScopedNameResolver → ImportMapResolver → RelativeImportResolver → InfraResolver →
RulesBridgeResolver` — order-independent (dedup keeps the max-confidence edge per key). SCIP edges
ingest separately via `wicked-estate scip`; LSP joins when the ADR-009 edit plane lands.

---

## 4. Storage

### `SqliteStore` (default backend) ✅
SQLite + WAL. Passes the full GraphStore conformance kit on-disk and in-memory (`:memory:`).
- **Schema**: nodes / edges / unresolved_refs / files / symbols (intern table) / content / embeddings / edge_history / meta / change-log + FTS5.
- **Symbol interning** ✅ — `symbols(sid, sym UNIQUE)`; nodes/edges/refs reference symbols by integer
  `sid` FK (`intern(sym)->i64`). Cuts on-disk footprint when a symbol string recurs.
- **FTS5 full-text search** ✅ — bulk DELETE+INSERT rebuild (`bulk_rebuild_fts_for_files`), chunked at
  16 000 to stay under SQLite's 32 766 variable limit on huge repos.
- **Content-addressed source storage** ✅ — file bodies stored once, **zstd-compressed**, keyed by
  git sha; `symbol_source()` / `file_content()` reconstruct slices on demand.
- **Vector embeddings** ✅ — `embeddings(symbol, dim, vec)` (little-endian f32 blob); `set_embedding`,
  `embedding`, `nearest` (cosine top-k). Dimension is per-row (embedder-consistent).
- **Compaction** ✅ — `compact` prunes orphan embeddings/content, stale cache, edge-history beyond
  retention, then VACUUMs; reports `CompactStats`.
- **Dangling-edge prune** ✅ — `prune_dangling_edges` (loud `GRAPH-CLEANUP:` marker on cleanup).
- **Capabilities negotiation** ✅ — `StoreCapabilities { full_text_search, vector_search,
  server_side_traversal, transactional_batch, shared_writers }`; retrieval negotiates against it.
- **Bounded traversal** ✅ — `traverse` uses a bounded recursive CTE with `max_depth` + `max_nodes` +
  `min_confidence` + edge-kind filter. No unbounded whole-graph walks.

### `MemStore` (reference impl) ✅
In-memory GraphStore; passes the same conformance kit; used as the vector store for `:memory:` and in tests.

### `PostgresStore` (`--features postgres`) ✅
Full `GraphRead` + `GraphWrite` + `GraphStoreMutExt` implementation backed by PostgreSQL.
- `open_store("postgres://...")` / `open_store("postgresql://...")` — same factory arm, zero
  caller changes vs. SQLite.
- **`shared_writers: true`** — multiple processes write concurrently (SQLite is single-writer).
- **`server_side_traversal: true`** — `WITH RECURSIVE` CTE traversal in-DB.
- FTS via `ILIKE`; schema created automatically on first connect.
- Passes the full `GraphStore` conformance suite (`TEST_POSTGRES_URL` required; skips gracefully
  without it so `cargo test --workspace` is always offline-safe).
- Global process-wide `OnceLock<Runtime>` keeps the connection-pool keepalive tasks alive across
  blocking calls (important detail: per-call runtime teardown kills pool background tasks).
- `SurrealStore` (`--features surrealdb`) is the W1.5 bake-off challenger — built and
  conformance-tested behind its feature flag, but NOT wired into the `open_store` factory (a
  `surrealdb://` spec errors) and with no bake-off verdict yet; `IndraDB` excluded (ADR-003).

---

## 5. Retrieval — the agent-facing API

`RetrievalTool` impls (`wicked-estate-retrieve`), JSON in/out, exposed via MCP:
- `SearchEntity` ✅ — name/FTS symbol search.
- `RetrieveEntity` ✅ — fetch a symbol + its immediate neighborhood.
- `TraverseGraph` ✅ — bounded graph walk.
- `BlastRadius` ✅ — transitive **dependents** of a symbol (now follows **all** edge kinds, not just
  Calls — so estate `uses`/`protects`/`accesses` dependents surface; closes the silent-under-report bug).
- `Lineage` ✅ — dependency/ancestry direction.
- `ContextBundle` ✅ — the MCP-exposed scoped, ranked context bundle (source + callers + dependencies).
- `ContextPack` ✅ — budget-bounded context variant (in-crate; not on the MCP surface).
- `FetchContent` ✅ — source slice retrieval.
- `RulesInventory` ✅ — lists rule engines + rule sets and the code that calls them (§2 rules layer).
- `rules.recall` ✅ — faceted, severity-ordered recall of conformance `Rule` nodes (`PAT-*`/`POL-*`);
  facets: language/layer/framework (wildcard), severity/rule_type (exact), scope subtree prefix,
  projects (a project rule only when its project is named; omitted = global rules only).
- `RankHotspots` ✅ — top symbols by PageRank × change-frequency churn.
- `Communities` ✅ — detected symbol communities (graph clusters).
- `SemanticSearch` ✅ — embedding ANN (optional; needs a VectorStore).

**Hybrid retrieval** ✅ — graph + FTS5 core, embeddings an **optional** sidecar, fused via **RRF**
(reciprocal-rank fusion; `hybrid_search`, `semantic_search`, `cosine_similarity`).

### Embedding tiers (configurable ladder) ✅
- **`HashEmbedder`** (default) — deterministic FNV bag-of-words, zero-dep, offline. Lexical, not semantic.
- **`Model2VecEmbedder`** (`--features model2vec`) — static distilled embeddings (default
  `minishlab/potion-base-8M`), **no ONNX runtime**, ~10–100× faster, configurable via
  `CI_MODEL2VEC_MODEL`. Real (static) semantics.
- **`FastEmbedder`** (`--features fastembed`) — contextual ONNX/BGE (`bge-small-en-v1.5`, 384-dim),
  highest quality, heaviest (pulls `ort`).
- `default_embedder()` picks **fastembed > model2vec > hash**, with a loud `EMBED-FALLBACK:` marker if
  a model can't load (never silently presents lexical results as semantic).
- Both semantic tiers have a permanent **semantic-quality test** (related-but-zero-shared-token text
  embeds closer than unrelated — the property the lexical default cannot satisfy).

### Agent runtime-behavior contract ✅ (`docs/agent-behavior-rules.md`, R1–R7)
Every `RetrievalTool` obeys: R1 never `isError:true` early (→ session abandonment) · R2 unindexed →
expose *zero* tools, not erroring ones · R3 partial coverage is worse than none · R4 cap output ~25K
chars (rank + budget) · R5 always report staleness · R6 loud `GRAPH-FALLBACK:` marker when the agent
must fall back to reading files · R7 confidence visible, low-confidence labeled.

---

## 6. Ranking ✅
`wicked-estate-rank` — (personalized) **PageRank** over CALLS/IMPORTS via power-iteration (empty seeds = uniform =
global PR). Used to rank "most important symbols" (`rank` command) and to budget ContextPack output.
(Replaced an O(V·E)/iter petgraph path: 59.8s → 0.107s on the bench corpus.)

---

## 7. Estate mapping (ADR-004)

Treats infrastructure + mainframe as **just more languages/collectors** feeding the same graph.

- **IaC as languages** ✅ — Terraform/HCL, CloudFormation, Kubernetes (+ Bicep/ARM/Pulumi as grammars):
  resources = nodes, depends-on = edges, **no schema change**. CFN/K8s sniff-dispatch on file content.
- **Live cloud state** 🟡 — read-only `Collector` seam (AWS/Azure/GCP/tfstate).
  - `tfstate` command — CLI-facing, indexes a downloaded `.tfstate` file. ✅
  - `AwsCollector` (`--features cloud-aws`) — Resource Explorer v2 + EC2/IAM supplemental; standard
    AWS credential chain; resource types normalized `aws_ec2_instance` etc. ✅
  - `AzureCollector` (`--features cloud-azure`) — Azure Resource Graph `resources` query;
    `DefaultAzureCredential`. ✅
  - `GcpCollector` (`--features cloud-gcp`) — compiles, type-tests pass, returns `Err` at runtime;
    full google-cloud-asset-v1 wiring is pending a crate API migration (Wave 9). 🟡
  - CLI `collect-live` command and org-wide drift sweep are Wave 9/10.
  - **No secret storage** — runtime auth headers never persisted (ADR-004 §5).
- **Drift** ✅(logic)/🟡 — `drift` command = graph diff by **resource identity** between `origin=iac`
  and `origin=live` (`estate_drift` → `DriftReport`).
- **Mainframe estate** ✅ — RACF (security), IMS DBD/PSB (data), MQ MQSC (messaging) extractors (§2).
- **Cross-domain join** ✅ — `estate_edges` links the **same physical asset across domains**: a RACF
  dataset profile `protects` a JCL dataset; a RACF `RDEFINE MQQUEUE` `protects` an MQ queue — by RACF
  generic-pattern matching, most-specific-wins. So `blast-radius <dataset>` returns the JCL step that
  *uses* it **and** the RACF profile that *protects* it in one query.

---

## 8. Cross-language & semantic linking

- **Cross-language reference resolution** ✅ — a reference in language A resolves to a definition in
  language B by name. Permanent regression test (`cross_language_estate.rs`, 9 assertions):
  JCL `EXEC PGM=`→COBOL, HLASM `CALL`→COBOL, COBOL `CALL 'x'`→COBOL, JCL+RACF→dataset (uses+protects),
  RACF+MQSC→queue (protects + resolves_to), IMS DBD/SEGM hierarchy.
- **Requirement ↔ functionality linking** ✅ — three semantic columns on every node:
  `description` (what is it?), `requirement` (the requirement it matches), `requirement_validated`
  (bool). API: `set_node_semantics` (partial update), `node_semantics`, `find_by_requirement`
  (exact-string match) — `semantics` + `by-requirement` CLI commands. (Note: requirement lookup is
  exact-string; the semantic judgment is the human/agent annotation.)
- **Embedding semantic search** ✅ — meaning-based ANN over symbol text (see §5 tiers).

---

## 9. MCP server ✅
`wicked-estate-mcp` — JSON-RPC: `tools/list` returns the tools with derived JSON Schema;
`tools/call` dispatches + wraps results in the MCP envelope. **33 tools across 3 domains** with all
stores open: 12 unconditional estate read tools (`all_tools()` — SearchEntity, RetrieveEntity,
TraverseGraph, Path, BlastRadius, FetchContent, ContextBundle, RulesInventory, `rules.recall`,
RankHotspots, Communities, Lineage), 7 `memory.*` tools plus the 4 `proposal.*` queue tools, and 10
`knowledge.*` tools. `SemanticSearch` is a 34th, listed only when embeddings are present
(`live_semantic_search` → `handle_request_with_semantic`). Estate-only mode (no memory/knowledge
domains) lists the 12 estate tools (13 with `SemanticSearch`). `--readonly` omits the 11 write tools
(`memory.capture`/`erase`/`learn`/`reflect`, `knowledge.ingest`/`write`/`relate`/`relate_code`/`replace_source`,
`proposal.approve`/`reject`) from `tools/list` and rejects them in `tools/call`; `proposal.submit` stays (it only queues). Rule authorship flows only through human-merged doc
PRs (ADR-012). Implements the R1–R7 behavior contract.

---

## 9a. Memory and knowledge ✅
Two stores beside the code graph, each its own SQLite file (`memory.db`, `knowledge.db`), joined to code by the
cross-engine edge store (`xedge.db`, `wicked-estate-overlay`). Memory is what happened and what you told it;
knowledge is what estate read (docs, and the structure of live data sources). Each store has its own tools
(`memory.*`, `knowledge.*`), so a result's store is the tool that returned it.

### Memory ✅ (`wicked-estate-memory`, 7 tools)
- **Scoped capture and recall** ✅ — hierarchical scopes (`org:acme/agent:claude`); tiers
  `working|episodic|semantic|procedural|archival`. `memory.recall` is token-budgeted and inherits from ancestor
  scopes (`scope_prefix` flips it to the subtree); each item carries `memory_id`, `scope`, `content`, `tier`, `score`.
- **Operator memories** ✅ — an explicit operator scope applies across projects; project memories stay in their project.
- **Facts linked to code** ✅ — `memory.learn` stores a fact and links it to code symbols in one step.
- **Reflection** ✅ — `memory.reflect` deletes working memories older than 24 h first (a summary built over one is
  removed, then rebuilt from its surviving durable sources), distils episodes into facts, promotes reinforced facts
  to skills, and archives aged low-salience memories.
- **Browse and erase** ✅ — `memory.list` (the whole in-scope set with facets, for a management surface),
  `memory.coverage` (counts by tier and kind), `memory.erase` (one id, or everything under a scope prefix).
- **Propose, then approve** ✅ — `proposal.submit` queues an inert `memory`, `knowledge` or `policy:<type>`
  proposal (safe under `--readonly`; provenance is stamped by the server, never taken from args);
  `proposal.list`; `proposal.approve` promotes it (a `policy:*` proposal is handed back for steering);
  `proposal.reject`.

### Knowledge ✅ (`wicked-estate-knowledge`, 10 tools)
- **Documents** ✅ — `knowledge.ingest` (a document as doc + chunk nodes), `knowledge.write` (one doc, section,
  chunk or concept), `knowledge.relate` (typed relation with confidence and provenance), `knowledge.relate_code`
  (a link to code symbols), `knowledge.coverage` (counts by class).
- **Hybrid recall** ✅ — `knowledge.recall` fuses FTS and vector hits (RRF); `knowledge.recall_about_code` returns
  knowledge linked to given code symbols. Items carry `class` (`doc|section|chunk|concept`), `source`, `scope`,
  `deps` and `untrusted`.
- **Live-source structure** ✅ — `knowledge.replace_source` writes one COMPLETE generation of a live source's
  structure (one doc per relation or tool, with sections: names, types, keys, declared enum literals; never rows)
  under scope `source:<id>`. The knowledge side of a generation is one transaction: absent docs are tombstoned,
  and a failed generation commits no knowledge change and no tombstone (a document's links, below, are staged
  before it as hidden rows, which the source's next generation cleans up). An older generation is refused, and
  the same number must carry the same content (fingerprint). A write tool (refused under `--readonly`).
- **The source catalog** ✅ — `knowledge.source_catalog`: one source's complete typed catalog, paged (byte-bounded,
  fixed per generation); a pinned generation that moved is an error, never a mixed page. Its text is marked
  untrusted (data, never instructions).
- **Visible-filtered reads** ✅ — every knowledge read but `coverage` takes `visible` (the `source:<id>`, `repo:<id>`
  and `project:<id>` deps a caller may see). An item is returned only when all its deps are visible, checked inside
  each candidate list before the `k` cut, RRF and the token budget, so hidden knowledge never crowds visible
  knowledge out. A hidden source answers like an unknown one. Knowledge with no deps (everything written before
  the knowledge plane) is returned as before.
- **Typed edges and `knowledge.neighbors`** ✅ — typed edges `describes`, `joins`, `governed_by`, `measures`,
  `tracked_in`, `addresses`, `closes`, `derived_from` between knowledge nodes, code symbols and control endpoints;
  `knowledge.neighbors` walks them (depth 1-3, at most 100 nodes and 200 edges, `truncated` when a bound bites),
  leaving out any node or edge whose deps are not all visible.
- **Data shapes through the proposal queue** ✅ — a `knowledge` proposal may carry `relate[]` (links to knowledge
  nodes) and `relate_code[]` (links to code symbols, each naming its repo; a proposal filed under a `repo` facet may
  link only that repo's code). Approval derives the node's deps from its endpoints and
  refuses a declared `deps` that differs; it is journalled, so a half-done approval is never visible. A
  write-mode server's start-up sweep completes an interrupted approval whose proposal is approved, and rolls back
  any other once its journal is older than 120 s (a younger one stays hidden until a later sweep or a retry).
- **Trackers and the `control` store** ✅ (DES-PM-001 PM-1) — a project-management tracker (crew registers GitHub
  Issues or Jira) is a catalog of its own: `replace_source` takes a tracker id (`trk_…`) and everything about it
  carries the dep `source:<tracker id>`. The `control` store names crew's endpoints as link ends: `project:<id>`,
  `repo:<id>`, `pr:<repo id>#<n>` and `run:<id>`. A document may own typed `links[]` (`tracked_in`, `addresses`,
  `closes`, …) to control or knowledge ends; each link is stamped with its document's generation, so a reader
  keeps it only while that generation is live: a write interrupted before the knowledge transaction commits
  leaves only hidden rows, and one interrupted after it leaves the new generation visible (stale rows stay hidden
  until the next generation cleans them up).
  `knowledge.neighbors` accepts `store: "control"` as a start; a control end is shown only when its own deps are
  visible. No new tool (the count stays 33).

---

## 10. CLI (`wicked-estate`) ✅
`index <path> [--db] [--repo <label>] [--history] [--embeddings] [--force]` · `scip <root>
[--scip-file]` · `tfstate <file>` · `import-telemetry` · `drift` · `query <name>` ·
`blast-radius <name>` · `rank` (alias `hotspots`) · `stats` (incl. git provenance) · `graph-view` ·
`source <name>` · `semantic <query>` · `cross-graph <name> --db a --db b …` · `compact` ·
`watch <path> [--history]` · `subscribe [--since <seq>]` · `semantics <symbol> [--description]
[--requirement] [--validated]` · `by-requirement <REQ>` · `clusters` · `context` · `annotate` ·
`annotations` · `stale-annotations` · `fingerprint` · `changed-since` · `entrypoints` · `leaves` ·
`dead-code` · `nodes` · `resolve` · `correspond` · `export` · `plugins list`.

---

## 11. Git integration & incremental indexing ✅
- **Repo provenance** — `collect_repo_info` shells git once per field (commit, branch, remote, dirty);
  never panics if git absent/not-a-repo. Surfaced by `stats`.
- **Per-file git sha** — `file_git_sha`; content rows keyed by sha.
- **Incremental index** — only changed/new files re-extracted (unchanged skipped by digest); `remove_file`
  clears nodes/edges/content/embeddings + archives edges to history — **target-aware** since 0.15.0:
  a shared `Import` node that other files' edges still reference is kept and re-homed, not deleted (#132).
- **Edge-history (read-only log)** — opt-in `--history`; old edges archived by `git_sha` on file change
  (`edge_history(file)` → `HistoricalEdge[]`), pruned beyond retention by `compact`.
- **Watch mode** — initial index then reactive re-index on file changes (notify/debouncer).
- **Change-log / subscribe** — `changes_since(cursor)` + `subscribe --since <seq>` emits JSONL
  `{seq, op, target}` + a resumable `next_seq` cursor.

---

## 12. Multi-repo federation ✅
`cross-graph <name> --db a.db --db b.db …` — federated search + blast-radius across multiple repo
graphs, each result tagged by source DB (org-wide blast-radius across repos).

---

## 13. Observability (OTel adapter + OTLP exporter) ✅
`wicked-estate-core/src/observability.rs` — OpenTelemetry-standard adapter: Resource,
InstrumentationScope, TraceId/SpanId/SpanContext, SpanKind/Status/Event/Link/SpanData,
InstrumentKind, AggregationTemporality, KeyValue/AttributeValue. (ADR-006)

`wicked-estate-observe` crate — OTLP HTTP/JSON exporter (`OtlpSink`) + emission sites wired ✅:
- **`OtlpSink`** — blocking reqwest HTTP/JSON; best-effort (never aborts the main operation on
  collector failure). Configured via `WICKED_OTEL_ENDPOINT` + optional `WICKED_OTEL_HEADERS`.
- **`init_sink_from_env()`** — reads env vars; falls back to `NoopSink` (zero overhead) when
  `WICKED_OTEL_ENDPOINT` is unset.
- **`InMemorySink`** — in-process capture for tests and integration.
- **Emission sites wired**: `index_path` span (files/nodes/edges/duration), per-file extract
  counter (by language), resolve counter (resolved/unresolved/tier), MCP tool spans (tool name /
  result size / cache hit), MCP cache hit-rate gauge, `store_fts` rebuild latency gauge.
- Service name: `wicked_estate`.

---

## 14. Footprint & performance ✅
Footprint discipline is a hard constraint ("can't bloat people's disks"):
- zstd content-addressing + symbol interning + slim typed `unresolved_refs` columns.
- Demonstrated: footprint 357 → 111 MB; index speed 114s → ~4s; PageRank 59.8s → 0.107s on the corpus.
- A footprint-per-node gate (scaled fixture) guards regressions.
- Heavy/optional deps (ONNX via fastembed) are **feature-gated off by default** so the default binary
  stays slim and the default build is offline.

---

## 15. Benchmark harness ✅ (capability) · 🟡 (agent A/B: arithmetic only, no runner)
`wicked-estate-bench` runs a **capability benchmark** per repo: index speed, footprint, edge and
resolver breakdowns, a language matrix, blast-radius latency, and a matched-population **direct
reference resolution** diagnostic (`null` when its denominator cannot be read). It also has a
**memory recall@5 gate** (`--recall`, exit 1 below the gate). Sanity-gated, not a frozen numeric
baseline. The agent A/B types (`EvalReport`, `TaskOutcome`) are aggregation arithmetic with tests;
there is **no** runner that executes baseline-vs-treatment agent tasks, and no golden-set scorer.
An unknown flag or a missing path is a usage error, and a repo that fails to benchmark fails the run.

---

## 16. Quality gates & CI ✅
- Gates (every change): `cargo build --workspace` (0 warnings) · `cargo test --workspace` (1,370 at
  time of writing — the count is stale after any new crate; re-run before re-claiming) ·
  `cargo clippy --workspace --all-targets -D warnings` · GraphStore conformance · capability bench (sanity-gated).
- **No grandfathering**: 0 `#[allow]`, 0 `#[ignore]`; lints fixed in code.
- **GitHub Actions CI** (`.github/workflows/ci.yml`): `gate` job (fmt · build · clippy · test, offline)
  + `semantic-embedders` job (feature-gated model2vec + fastembed tests with HF model caching).
- `scripts/blast-check-pre-commit.sh` — blast-radius invariant pre-commit lint.

---

## 17. Locked decisions (ADRs)
- **ADR-001** graph schema · **ADR-002** stable symbol identity (no content-hash IDs; scheme-3
  type-nested definition ids since 0.15.0) ·
  **ADR-003** storage backends (SQLite default; SurrealDB challenger; IndraDB excluded; external-DB seam) ·
  **ADR-004** infrastructure/estate mapping · **ADR-005** code-graph-as-a-brain ·
  **ADR-006** observability OTel adapter · **ADR-007** TSG superseded by SCIP ·
  **ADR-008** memory-api shim retention · **ADR-009** intent-routed LSP (edit plane = the only
  sanctioned LSP consumer; W3.6) · **ADR-010** parser-plugin overrides (query-only + double-opt-in
  full) · **ADR-012** rule authorship (no `rules.write` MCP tool — human-merged doc PRs only).
- Competitive analysis behind every decision: `docs/DESIGN-NOTES.md`.

---

## 18. Honest gaps (not yet true — a caller may assume otherwise)
- **PostgresStore**: built and conformance-passes ✅. `shared_writers` and `server_side_traversal`
  are real. Not yet benchmarked at scale vs. SQLite; no SurrealDB bake-off result.
- **Observability**: OTLP exporter + emission sites wired ✅. Not yet a zero-to-dashboard guide;
  semantic embedder traces not wired; no dedicated `wicked-estate observe` CLI command.
- **Cloud collectors**: AWS + Azure functional ✅. GCP stub only (runtime `Err`) — full
  google-cloud-asset-v1 wiring is pending a crate migration (Wave 9). CLI `collect-live` command
  and org-wide drift sweep are Wave 9/10. GCP callers may assume `open_cloud_collector("gcp")`
  works — it does not yet.
- **Semantic embedder tests** are **feature-gated** (need a model download) — they run in the CI
  `semantic-embedders` lane, **not** in the default workspace suite.
- **Semantic retrieval quality** proven by an ordering property on small inputs, **not** a retrieval
  benchmark; the agent-eval benchmark still uses the lexical default embedder.
- **`find_by_requirement`** is exact-string, not fuzzy.
- Niche mainframe modeling gaps: VSAM AIX, ESDS/RRDS distinction, RECFM VB/FBA, COMP-3 usage metadata.
- Deferred languages: nim, pony, asm, odin.
- Several Don'ts (no content-hash IDs, no unbounded traverse, every edge carries provenance) are
  enforced by review/tests but **not yet dedicated CI lints**.

---

*Generated as a full inventory of the wicked-estate engine. Live build tracker: `docs/plan/WAVE-PLAN.md`.
Behavior contract: `docs/agent-behavior-rules.md`. Engine invariants: `docs/ENGINE-CONTRACT.md`.*
