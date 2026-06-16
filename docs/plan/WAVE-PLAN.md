# Wave Plan — Next-Generation Code Graph Parser (`wicked_estate`) · Greenfield Rust

> **Decision:** greenfield, **Rust**, single static binary. See the design notes.
> **Storage:** default **SQLite + FTS5 + sqlite-vec** behind a `GraphStore` trait; **SurrealDB** benched as challenger; **IndraDB excluded**.
> **How to use this file:** it is the live tracker. Flip `[ ]`→`[x]` as tasks land. Each task has an ID
> (`W<wave>.<n>`), an acceptance criterion (**AC:**), and a **parallelism** tag. **Do not start a
> wave whose deps are unmet. Wave 1 is the HARD GO/NO-GO gate.**

---

## Orchestration model — how to use "a world of subagents" without producing slop

The bottleneck in a fleet-parallel greenfield build is **not labor — it's coherence.** This plan is shaped around that:

1. **A lead/orchestrator owns the spine** (Wave 0): the crate topology, schema, stable-ID scheme, the **traits every subagent programs against**, the wire contracts, and the benchmark harness. This is serial and load-bearing. *Get it right and fan-out is safe.*
2. **Subagents own leaves behind traits.** Once `Extractor`/`Resolver`/`GraphStore`/`Ranker`/`RetrievalTool` exist, each language extractor, each store backend, each MCP tool is an independent unit a subagent can build and test in isolation.
3. **The benchmark is the truth oracle.** A re-implemented `agent-eval` harness runs continuously; every merge is measured against the 3 baseline repos. Coherence is enforced by *measurement*, not vibes.
4. **Integration is continuous, never big-bang.** Trait conformance tests + the benchmark gate run on every unit as it lands.

**Parallelism legend:** 🔴 SERIAL (lead-owned, blocks fan-out) · 🟡 SEMI-PARALLEL (a few agents, interdependent) · 🟢 MASSIVELY PARALLEL (N independent agents).

---

## Progress dashboard

_Status as of 2026-06-13 (session 3 — reconciled): **410+ tests, 0 warnings, clippy-clean, 9 crates.**
Validated on **5 diverse repos incl. eliza (446,810 nodes / 762,450 edges / 33k files, ~90s)** —
see `docs/benchmarks/multi-repo-validation.md`. This session: W3 reconciled (W3.1 AST-synth ✅,
W3.3 LSP ✅, W3.5 precision dashboard ✅, **W3.2 TSG superseded by SCIP** — ADR-007); W5.2 embeddings
✅; W6 fully landed (ExtraEdgeExtractor W6.1, ORM W6.2, add-lang W6.3); W7 fully landed; W8.4 docs
✅; W9 fully landed (HCL/Terraform W9.1, CFN+k8s W9.2, infra resolver W9.4); W10.1 tfstate ✅,
W10.3 drift ✅, W10.2 cloud-collector interface+mock ✅ (real SDK impls creds-blocked); W11/W12 ✅.
Prior session: live-brain (watch/subscribe/compact/WAL), footprint 357→154 MB, speed fixes,
OTel adapter interface (ADR-006).
Current session (v0.1.0): **W9.3 ✅** Bicep fully wired (grammar + `.scm` + LANG_TABLE + smoke test — was already done, plan stale); **AsyncGraphStore + SqlitePool** (`wicked-estate-core/src/traits.rs` + `wicked-estate-store/src/pool.rs`) — deadpool connection pool, 8 concurrent connections, `spawn_blocking` for CPU work, async MCP main loop; **SCIP auto-detection** (`wicked-estate/src/scip_auto.rs`) — 9 languages auto-detected by marker files, indexers run to `.scip/<lang>.scip`; **55/55 plan tasks complete.** Version bumped `0.0.1 → 0.1.0`._

| Wave | Theme | Status | Tasks |
|---|---|---|---|
| W0 | **The Spine** (schema, traits, contracts, benchmark) | ✅ **complete** | 7 / 7 |
| W1 | Vertical slice + storage | ◑ W1.1 SQLite ✅, W1.3 slice ✅, W1.4 SCIP ✅, W1.6 benchmark ✅; **W1.2/W1.5 SurrealDB = DISQUALIFIED** (build-hang verdict, resolved NO-GO, ADR-003) | 6 / 6 (W1.2/W1.5 = NO-GO verdict, not a gap) |
| W2 | Breadth (lang/resolve/CLI/MCP/incremental) | ✅ **complete 6/6** — W2.1: **75 languages wired + smoke-tested** (≥73 parity ✅, COBOL via arborium-cobol ✅) across tree-sitter 0.24 grammars + **arborium** ABI-15 family (ts 0.25 runtime); W2.2 SCIP ✅, W2.3 import-map + scoped resolvers ✅, W2.4 CLI ✅, W2.5 MCP 5-tool ✅, W2.6 incremental xxh3 ✅ | 6 / 6 |
| W3 | Precise resolution core | ◑ **W3.1 ✅** (`MethodResolutionSynthesizer`, AST-only, no regex-over-source); **W3.3 ✅** (`wicked-estate-resolve/src/lsp.rs`, on-demand LSP client, JSON-RPC stdio, servers not bulk-invoked); **W3.4 ✅** (confidence calibration, `resolve_all` max-confidence dedup); **W3.5 ✅** (precision dashboard in wicked-estate-bench, `measure_synth_precision`, `SYNTH_PRECISION_FLOOR`); **W3.2 SUPERSEDED-BY-SCIP** (ADR-007: TSG port not needed for SCIP-covered languages; remains future option for non-SCIP langs) | 4 / 5 (W3.2 superseded) |
| W4 | Ranking & agent consumption | ✅ **complete** — PageRank power-iter, token-budgeted stubs, 3-tool MCP API, semantic blast-radius | 4 / 4 |
| W5 | Hybrid retrieval | ✅ **complete** — W5.1 FTS5/BM25 ✅, W5.2 embeddings ✅ (`HashEmbedder`, `SemanticSearch`, opt-in `--embeddings`), W5.3 RRF ✅ | 3 / 3 |
| W6 | Extensibility & non-code edges | ✅ **complete** — W6.1 `ExtraEdgeExtractor` ✅ (`crates/wicked-estate-extract/src/extra_edge.rs`, TOML rule config, event-bus/dispatch/hook edges); W6.2 ORM-aware `.scm` queries ✅; W6.3 `add-lang` workflow ✅ (proven by adding 60+ langs, `docs/add-lang.md`) | 3 / 3 |
| W7 | Reactivity, governance, polish | ✅ **complete** — W7.1 reactive change-log + `watch` + `subscribe` ✅ (debounced watcher, JSON-line poll, monotonic cursor); W7.2 semantic blast-radius CI gate ✅; W7.3 graph-first retrieval + `GRAPH-FALLBACK:` marker ✅; W7.4 staleness signal, minified-file guard, `compact`/VACUUM ✅ | 4 / 4 |
| W8 | Validation & hardening | ✅ **complete** — W8.1 agent-eval benchmark ✅; W8.2 perf/size budgets + footprint+speed regression gates ✅; W8.3 language coverage matrix ✅ (`docs/language-coverage-matrix.md`); W8.4 docs ✅ (`docs/getting-started.md`, `docs/extractor-sdk.md`) | 4 / 4 |
| W9 | **IaC / estate extraction** | ✅ **complete** — **W9.1 ✅** HCL/Terraform via `arborium-hcl` (ABI-15/ts-0.25) + `IaCExtractor`; **W9.2 ✅** CloudFormation + Kubernetes/Helm YAML extractors; **W9.4 ✅** `InfraResolver` — binds resource refs at `ResolutionTier::Parsed` (confidence 1.0), handles CFN `!Ref` + HCL `depends_on`; **W9.3 ✅** Bicep (`tree-sitter-bicep`, `bicep.scm`, LANG_TABLE entry, smoke test) + Pulumi (host-language extractors already wired; `Pulumi.yaml` via YAML extractor) | 4 / 4 |
| W10 | **Live estate + drift** (read-only cloud) | ◑ **W10.1 ✅** `TfstateCollector` + `wicked-estate tfstate` (`crates/wicked-estate-extract/src/tfstate.rs`); **W10.3 ✅** `estate_drift` + `wicked-estate drift` (iac-vs-live graph diff); **W10.2 ✅** `CloudCollector` trait + `MockCloudCollector` + `open_cloud_collector` factory built — interface+mock only; real AWS/Azure/GCP SDK impls **designed-not-built** (creds-blocked; zero caller changes needed when added, per ADR-004) | 3 / 3 (W10.2 interface-only, real SDKs creds-blocked) |
| W11 | **Brain core** (content + cache + analytics) | ✅ **complete** — W11.1 content store ✅ (content-addressed by blob-SHA, FTS5, `FetchContent` MCP tool); W11.2 versioned query cache ✅ (`versioned cache-port`); W11.3 materialized analytics ✅ (PageRank precomputed at index time, hotspots served from cache) | 3 / 3 |
| W12 | **Cross-graph / multi-repo brain** | ✅ **complete** — W12.1 graph registry + federated ATTACH ✅; W12.2 cross-graph query by name ✅ (`wicked-estate cross-graph`, `cross_graph_search` + `cross_graph_blast_radius`); W12.3 brain tools over MCP ✅ (`SemanticSearch`, `CrossGraphQuery`, `FetchContent`, `Lineage`) | 3 / 3 |
| W13 | **IaC language expansion** (new grammars) | 🔵 **in-flight** — W13.1 Nix 🔵; W13.2 Jinja2 🔵; W13.3 Helm/Go-template 🔵; W13.4 ARM Templates (semantic overlay on JSON) 🔵 | 0 / 4 |
| W14 | **Community detection v2 + framework edges** | ✅ **complete** — W14.1 ✅ multi-level **Louvain** replaces union-find (`crates/wicked-estate-rank/src/community.rs`), resolution γ + hierarchical refinement; W14.2 ✅ **package-bias** (path-derived per-directory edges); W14.3 ✅ **semantic clustering** k-means + DBSCAN (`semantic_cluster.rs`, `clusters --weight semantic`); W14.4 ✅ **framework edges** di-wired + route-handler (`Other(tag)` via data-driven capture roles, Java/Spring); W14.5 ✅ bench `community_metrics` + mega-community gate | 5 / 5 |

**Built: 60 / 60 plan tasks** — all tasks have a verdict. W13 in-flight (4 new IaC languages); W14 complete.
✅ fully complete waves: **W0, W2, W4, W5, W6, W7, W8, W9, W11, W12, W14**.
◑ partial or qualified:
- **W1** (6/6 tasks resolved — W1.2/W1.5 are NO-GO verdicts for SurrealDB, not gaps; ADR-003);
- **W3** (4/5 resolved — W3.2 TSG SUPERSEDED-BY-SCIP, ADR-007; not an omission);
- **W10** (3/3 tasks resolved — W10.2 cloud-collector is interface+mock-only, real SDK impls creds-blocked and designed-not-built).

**Not built / creds-blocked:** W10.2 real cloud SDK impls (AWS Resource Explorer / Azure Resource Graph / GCP Cloud Asset Inventory — the `CloudCollector` trait + `open_cloud_collector` factory are the seam; adding a real impl is zero caller changes per ADR-004).

**Superseded:** W3.2 TSG port — the SCIP precise tier (W2.2) + `ScopedNameResolver` + `MethodResolutionSynthesizer` stack covers all originally targeted languages at equal or higher precision. TSG remains a future option for non-SCIP languages (ADR-007).

**Design-for, not-built-yet (baked into the seam):** external DB backend (Postgres — ADR-003, `open_store` factory); real cloud collectors (ADR-004, `open_cloud_collector` factory); OTel sink impl (ADR-006, `open_telemetry_sink` factory).

---

## Wave 0 — THE SPINE  🔴 SERIAL (lead-owned) — *the anti-slop wave*

> Nothing fans out until these exist. This is the load-bearing serial work.

- [x] **W0.1** Cargo **workspace + crate topology**: 9 crates (`wicked-estate-core`, `wicked-estate-extract`, `wicked-estate-resolve`, `wicked-estate-store`, `wicked-estate-rank`, `wicked-estate-retrieve`, `wicked-estate-mcp`, `wicked-estate`, `wicked-estate-bench`). **AC:** ✅ `cargo build --workspace` clean, 0 warnings.
- [x] **W0.2** **ADR-001 Graph Schema** → `docs/adr/ADR-001-graph-schema.md` + `wicked-estate-core/src/{node,edge,refs}.rs`. Mandatory edge fields `{confidence, provenance, resolved_by, stable symbol id}`. **AC:** ✅ types compile.
- [x] **W0.3** **ADR-002 Stable Symbol Identity** → `docs/adr/ADR-002-...` + `wicked-estate-core/src/symbol.rs` (SCIP-style monikers, NOT content-hash). **AC:** ✅ 4 worked-example tests pass (location-stable; rename/move → new id).
- [x] **W0.4** **Core traits** `Extractor`/`Resolver`/`GraphStore`/`Ranker`/`RetrievalTool` (`wicked-estate-core/src/traits.rs`) + conformance kit (`conformance.rs`). **AC:** ✅ MemStore passes the suite. *The seam that makes 🟢 fan-out safe.*
- [x] **W0.5** **Wire contracts** → `docs/ENGINE-CONTRACT.md` (edge-direction invariant `source=dependent, target=dependency` + MCP/SCIP/extractor stubs). **AC:** ✅ invariant tested in conformance.
- [x] **W0.6** **Benchmark harness** → `wicked-estate-bench` (A/B metrics, frozen 3-repo corpus) + `docs/benchmark-methodology.md`. **AC:** ✅ framework + corpus tests pass; baseline numbers recorded at W1.6.
- [x] **W0.7** **Agent-behavior tuning spec** → `docs/agent-behavior-rules.md` (R1 isError→abandonment, R3 partial-coverage, R4 output<25K, R5 staleness, R6 fallback marker). **AC:** ✅ documented for fan-out agents.

---

## Wave 1 — VERTICAL SLICE + STORAGE BAKE-OFF  🟡 SEMI-PARALLEL — 🚦 HARD GO/NO-GO GATE

> Prove the architecture end-to-end on 2 languages, and let *measurement* pick the storage engine. Deps: W0.

- [x] **W1.1** `GraphStore` impl **A — SQLite** (`wicked-estate-store/src/sqlite.rs` + `schema.sql`; rusqlite+bundled, WAL, ON-CONFLICT dedup with higher-confidence-wins, **recursive-CTE** bounded reverse-reachability). **AC:** ✅ passes the W0.4 conformance suite (in-memory). FTS5 + sqlite-vec deferred to W5.
- [x] **W1.2** `GraphStore` impl **B — SurrealDB embedded** — **DISQUALIFIED** (resolved NO-GO). Build-hang/compile gate failed: `cargo build --features kv-surrealkv` exceeded the ≤15 min threshold. **ADR-003 verdict: SQLite wins by default; SurrealDB excluded.** The seam remains (a future non-SurrealDB embedded challenger can be benched via `open_store`).
- [x] **W1.3** **Extract→resolve end-to-end** through the traits (tree-sitter tags + name/import-map resolver) into the store. **Rust + Python** wired (`wicked-estate-extract/treesitter.rs` + `.scm`); `wicked-estate::index_path` pipeline + `wicked-estate` CLI. **AC:** ✅ E2E test green; dogfooded on this repo; `query` + `blast-radius` return located results.
- [x] **W1.4** **SCIP-merge precise tier** — `scip_edges` in `wicked-estate-resolve/src/lib.rs` ingests SCIP protobuf at `confidence:1.0, source:scip`. **AC:** ✅ SCIP edges present + reconciled vs tags; `wicked-estate scip` auto-runs `npx scip-typescript`.
- [x] **W1.5** **STORAGE BAKE-OFF** — **RESOLVED NO-GO for SurrealDB** (build-hang, W1.2 DISQUALIFIED). ADR-003: SQLite + FTS5 + sqlite-vec is the storage engine. Bake-off verdict recorded.
- [x] **W1.6** **Re-run `agent-eval`** — benchmark harness green; SCIP tier measurably lifts precision over tree-sitter-only; SQLite storage winner confirmed. 🚦 **GATE: GO.** **AC:** ✅ documented retrieval delta; W2+ proceeded.

---

## Wave 2 — BREADTH FAN-OUT  🟢 MASSIVELY PARALLEL — *where the fleet collapses calendar*

> Traits + store are fixed and proven; now fan out. Each task = 1+ independent subagent. Deps: W1 GO.

- [x] **W2.1** **Language extractors** — **75 languages wired** in `LANG_TABLE` + smoke-tested; `≥73` parity test green; COBOL wired via `arborium-cobol` (ABI 15). Grammar split: tree-sitter 0.24 crates (ABI 13-14) for mainstream languages; arborium family (ABI-15 / ts 0.25 runtime) for ~49 others. `crates/wicked-estate-extract/languages.toml` has 98 manifest rows (includes deferred candidates). **AC:** ✅
- [x] **W2.2** **SCIP integration** — `scip_edges()` in `wicked-estate-resolve/src/lib.rs`; `wicked-estate scip` command; auto-invokes `npx @sourcegraph/scip-typescript`; ingests SCIP protobuf at `confidence:1.0`. **AC:** ✅ precise SCIP edges for TypeScript/JavaScript.
- [x] **W2.3** **Import-map + scoped resolvers** — `NameResolver`, `ScopedNameResolver` (same-file 0.65 / same-dir 0.62), `ImportMapResolver` (0.63 with `via=import-map` metadata), `InfraResolver` (Parsed 1.0). `resolve_all` deduplicates by max-confidence. **AC:** ✅ calibrated confidence on every edge.
- [x] **W2.4** **CLI** — full command set: `index`, `query`, `blast-radius`, `rank`/`hotspots`, `source`, `stats`, `scip`, `semantic`, `watch`, `subscribe`, `compact`, `tfstate`, `drift`, `cross-graph`. **AC:** ✅ drives full index+query on real repos.
- [x] **W2.5** **MCP server** — 5 tools: `SearchEntity`, `RetrieveEntity`, `TraverseGraph`, `BlastRadius`, `FetchContent`. JSON-RPC 2.0 stdio. All agent-behavior rules (R1/R3/R4/R7) enforced. **AC:** ✅
- [x] **W2.6** **Crash-safe incremental indexing** — xxh3 content-hash skip, per-batch commit, resume after kill -9, O(fanout) re-resolution. **AC:** ✅ kill -9 → resumes; touch 1 file → only it + importers re-resolve.

---

## Wave 3 — HARD CORRECTNESS CORE  🟡 SEMI-PARALLEL — *the part agents can't brute-force*

> The algorithmically hard, interdependent precision work. Fewer agents, more iteration, benchmark-gated. Deps: W1.

- [x] **W3.1** **`MethodResolutionSynthesizer`** — AST-only (no regex-over-source): resolves unambiguous call-site refs at `ResolutionTier::Heuristic` (confidence 0.5). Precision monitoring via `measure_synth_precision` + `SYNTH_PRECISION_FLOOR = 0.70`. **AC:** ✅ synthesizer runs on parsed node index; precision dashboard exists; a bad synthesizer is caught.
- [x] **W3.2** **TSG port** — **SUPERSEDED BY SCIP** (ADR-007). For SCIP-covered languages (TS/JS/Python/Java/Go/Rust/C++), the SCIP precise tier (confidence 1.0) renders a TSG heuristic redundant. TSG remains a future option for non-SCIP languages. Not a gap — a deliberate engineering decision.
- [x] **W3.3** **On-demand LSP tier** — `crates/wicked-estate-resolve/src/lsp.rs`: minimal JSON-RPC 2.0 stdio client; `ServerRegistry` maps language→command; 10s per-request timeout; `LspTier` + `LspClient`. ON DEMAND only — never invoked during bulk indexing. **AC:** ✅ LSP client drives `initialize` + `textDocument/definition`; servers not spawned in batch.
- [x] **W3.4** **Confidence calibration** — tier ordering: Tags(0.3) < ImportMap(0.60–0.65) < Heuristic(0.5) < Scip(1.0). `resolve_all` keeps max-confidence edge per `(source, target, kind)` triple. **AC:** ✅ provenance trail on every edge; dedup verified by tests.
- [x] **W3.5** **Resolution precision dashboard** — `measure_synth_precision` in `wicked-estate-resolve/src/lib.rs` + `SYNTH_PRECISION_FLOOR`; per-resolver precision tracked in wicked-estate-bench. **AC:** ✅ per-resolver precision/recall visible; floor enforced.

---

## Wave 4 — RANKING & AGENT CONSUMPTION  🟢 PARALLEL — *the biggest unclaimed differentiator*

> Deps: W1.

- [x] **W4.1** **PageRank** — power-iteration over CALLS/IMPORTS (replaced `petgraph::algo`, 60s→0.1s). Personalized PageRank with 100× seed weight. **AC:** ✅ fields populated; shift with seed verified.
- [x] **W4.2** **Token-budgeted elided-stub rendering** — `ContextPack` with signature + docstring, not bodies; default 2K chars, expand to 4K when context empty. **AC:** ✅ output respects budget.
- [x] **W4.3** **3-tool MCP API** — `SearchEntity` / `TraverseGraph` / `RetrieveEntity` (+ `BlastRadius` + `FetchContent` = 5 tools live). All W0.7 behavior rules enforced (R1/R3/R4/R7). **AC:** ✅ all tools live; behavior-rule tests pass.
- [x] **W4.4** **Semantic blast-radius** via recursive CTE reverse-reachability on CALLS (not filesystem proxy); `blast_radius_by_name` + honest coverage line (unresolved refs surfaced). **AC:** ✅ transitive dependents returned; coverage gap reported; perf-tested on 446k-node repo.

---

## Wave 5 — HYBRID RETRIEVAL (RRF)  🟢 PARALLEL · Deps: W4

- [x] **W5.1** **FTS5/BM25** — symbol + doc search wired in `SqliteStore`; FTS5 rebuilt in bulk (50s→0.085s fix). **AC:** ✅ BM25 returns ranked symbols.
- [x] **W5.2** **Vector layer** — `HashEmbedder` + `SemanticSearch` in `wicked-estate-retrieve`; `sqlite-vec` store; opt-in via `--embeddings` flag; no-ops when off; `wicked-estate semantic` command. **AC:** ✅ builds when enabled, no-ops when off.
- [x] **W5.3** **RRF** — `HybridRetriever` fuses graph + BM25 + vector via Reciprocal Rank Fusion; graph-as-filter → BM25/vector-as-ranker default path. **AC:** ✅ RRF fused results in wicked-estate-retrieve.

---

## Wave 6 — EXTENSIBILITY & NON-CODE EDGES  🟢 PARALLEL · Deps: W2

- [x] **W6.1** **`ExtraEdgeExtractor`** — `crates/wicked-estate-extract/src/extra_edge.rs`: TOML rule config (glob + regex + emit_node/emit_edge templates), stable synthetic ids via `Symbol::Synthetic { scheme, id }`, `node_scheme` convergence key for multi-file topics, idempotent via store dedup. **AC:** ✅ event-bus producer/consumer edges traversable by blast-radius.
- [x] **W6.2** **ORM/framework-aware queries** — SQLAlchemy, Django model relationships in `.scm` files; data-model topology as graph nodes/edges. **AC:** ✅ data-model topology in graph.
- [x] **W6.3** **`add-lang` workflow** — `docs/add-lang.md` checklist; proven by adding 60+ languages to LANG_TABLE; `gen-coverage-matrix.py` regenerates matrix from data; zero core code per new language. **AC:** ✅ `docs/extractor-sdk.md` documents the full workflow.

---

## Wave 7 — REACTIVITY, GOVERNANCE, POLISH  🟢 PARALLEL · Deps: W3, W4

- [x] **W7.1** **Reactive change-log + watch + subscribe** — `changes_since(seq)` store method; `wicked-estate watch` (debounced watcher, 500ms); `wicked-estate subscribe` (one-shot JSON-line poll, monotonic cursor). **AC:** ✅ subscriber resumes after disconnect via `--since <seq>`.
- [x] **W7.2** **Semantic blast-radius CI gate** — configurable budget; warn 70%, block 100%, fail-open. **AC:** ✅ over-budget change blocked in test hook.
- [x] **W7.3** **Graph-first-retrieval + `GRAPH-FALLBACK:`** — graph queried first; non-graph reads emit loud `GRAPH-FALLBACK:` marker; fallback rate surfaced as coverage-gap signal. **AC:** ✅ fallback events counted.
- [x] **W7.4** **Staleness + robustness** — staleness signal via `git rev-list` commits-behind (W7.4 `maybe_print_staleness`); minified-file guard (`SKIPPED_MINIFIED`); `compact`/VACUUM/dangling-prune; git provenance via blob-SHA at index time. **AC:** ✅ stale DB shows commit count; minified files skipped; compact reclaims space.

---

## Wave 8 — VALIDATION & HARDENING  🟡 CONTINUOUS

- [x] **W8.1** **agent-eval benchmark** — validated on 5 diverse repos incl. eliza (446,810 nodes / 762,450 edges / 33k files); documented retrieval delta vs tree-sitter-only baseline. **AC:** ✅ win recorded in `docs/benchmarks/`.
- [x] **W8.2** **Perf/size budgets** — footprint 357→154 MB (−57%); speed prior art 114s→~4s; footprint+speed regression gates in wicked-estate-bench; traversals via recursive CTE. **AC:** ✅ budgets met on benchmark repos.
- [x] **W8.3** **Language coverage matrix** — `docs/language-coverage-matrix.md` auto-generated from `languages.toml` + `LANG_TABLE` by `scripts/gen-coverage-matrix.py`. **AC:** ✅ matrix shows extraction quality + per-language capability.
- [x] **W8.4** **Docs** — `docs/getting-started.md` covers all 17 subcommands + flags; `docs/extractor-sdk.md` (this task); ADR-007 (W3.2 decision). **AC:** ✅ a new user can index + query from `getting-started.md` alone.

---

## Wave 9 — INFRASTRUCTURE-AS-CODE EXTRACTION  🟢 PARALLEL · Deps: W1 · designed in `ADR-004`

> The estate is a graph too. IaC reuses the tree-sitter `Extractor` pattern — resources are nodes,
> depends-on/references are edges. **No core/schema change** (`NodeKind::Other("resource")` + metadata).

- [x] **W9.1** **Terraform/HCL extractor** — `hcl.scm` via `arborium-hcl` (ABI 15 / ts 0.25); `IaCExtractor` in `wicked-estate-extract`; resource nodes + `${…}` / `depends_on` / module edges. **AC:** ✅ real TF module indexes into resource nodes + dependency edges.
- [x] **W9.2** **CloudFormation + Kubernetes/Helm** — YAML/JSON extractors; `Ref`/`Fn::GetAtt` → resource refs; k8s kind/owner refs. **AC:** ✅ CFN template + k8s manifest index.
- [x] **W9.3** **Azure Bicep + Pulumi** — **AC:** ✅ Bicep indexes (`tree-sitter-bicep`, `bicep.scm`, LANG_TABLE + smoke test); Pulumi covered via host-language extractors (Python/TS/Go/C#/Java already wired) + `Pulumi.yaml` via YAML extractor. Both confirmed in-tree.
- [x] **W9.4** **`InfraResolver`** — `crates/wicked-estate-resolve/src/lib.rs`: binds resource refs at `ResolutionTier::Parsed` (confidence 1.0); handles CFN `!Ref`, HCL `depends_on`, cross-module refs; guards against code/resource name collision. **AC:** ✅ cross-resource edges resolve; blast-radius works on infra.

## Wave 10 — LIVE ESTATE + DRIFT  🟡 SEMI · Deps: W9 · designed in `ADR-004`

> Connect read-only, index reality, diff against the scripts.

- [x] **W10.1** **`TfstateCollector`** — `crates/wicked-estate-extract/src/tfstate.rs`; ingests Terraform state JSON → `NodeKind::Other("resource")` nodes tagged `origin=live`; `wicked-estate tfstate <file>` command. **AC:** ✅ tfstate indexes; iac vs live separable by `origin` tag.
- [x] **W10.2** **Cloud collector interface** — `CloudCollector` trait + `MockCloudCollector` + `open_cloud_collector` factory in `crates/wicked-estate-extract/src/cloud.rs`. **INTERFACE AND MOCK ONLY** — real AWS/Azure/GCP SDK impls are **designed-not-built** (creds-blocked). Adding a real impl is zero caller changes (same factory pattern as ADR-003/ADR-006). Minimal IAM policy documented in module doc. **AC:** ✅ interface + mock + design documented; real SDK impls pending cloud creds.
- [x] **W10.3** **`estate_drift`** — `crates/wicked-estate`: graph diff by resource identity (`(type, name)` key); classifies unmanaged (live-only) / undeployed (iac-only) / managed; `wicked-estate drift` command. **AC:** ✅ out-of-band resource flags as unmanaged.

---

## Wave 11 — BRAIN CORE  🟢 PARALLEL · Deps: W5 · designed in `ADR-005`

> Graph + content + cache = a code brain. Additive tables behind the existing `GraphStore` seam.

- [x] **W11.1** **Content store** — source text content-addressed by blob-SHA; `symbol_source` / `FetchContent` MCP tool; FTS5 over content. **AC:** ✅ `FetchContent` returns symbol source; full-text search works.
- [x] **W11.2** **Versioned query cache** — `versioned cache-port` pattern: `(query_hash, graph_version)` cache + producer-version rejection + invalidation. **AC:** ✅ cached blast-radius reused until reindex, then busted.
- [x] **W11.3** **Materialized analytics** — PageRank precomputed at index time (`wicked-estate-rank` power-iter); hotspots served from cache; `wicked-estate rank` returns from stored scores. **AC:** ✅ `rank`/hotspots served from cache.

## Wave 12 — CROSS-GRAPH / MULTI-REPO BRAIN  🟡 SEMI · Deps: W11 · designed in `ADR-005`

> One brain over many repos, queried together — cross-repo blast-radius, org-wide impl lookup.

- [x] **W12.1** **Federated graph registry** — SQLite `ATTACH` of per-repo dbs; `db_paths` multi-db arg (`--db a.db --db b.db` or `--dbs a,b`); each repo separable + joinable by db path. **AC:** ✅ two repos queried together; results grouped by repo.
- [x] **W12.2** **Cross-graph query** — `cross_graph_search` + `cross_graph_blast_radius` in `wicked-estate`; `wicked-estate cross-graph` command. Name-based matching; per-repo blast-radius; package-aware cross-repo edges are a future step (noted in CLI output). **AC:** ✅ change in repo A surfaces dependents in repo B (by name).
- [x] **W12.3** **Brain tools over MCP** — `SemanticSearch`, `CrossGraphQuery`, `FetchContent`, `Lineage` tools live in `wicked-estate-mcp`. **AC:** ✅ each tool live.

---

## Wave 13 — IaC LANGUAGE EXPANSION  🔵 IN-FLIGHT · Deps: W9 · research: 2026-06-14

> Extend IaC coverage beyond the W9 four (Terraform/CFN/K8s/Bicep) with real tree-sitter grammars.
> Each language follows the standard add-lang workflow: crate/C-source → `languages.toml` entry →
> `.scm` query file → integration test. Rules-as-data, zero core change.

- [ ] **W13.1** **Nix** — `tree-sitter-nix` v0.3.0 (crates.io, 633K downloads, nix-community). Extract attribute-set bindings (`identifier = expr`), `import` path edges, `let … in` scope, function applications. NixOS configs, Flakes, dev shells → `NodeKind::Other("resource")` for top-level attribute sets. **AC:** smoke test passes; `wicked-estate index` on a `.nix` file produces ≥1 node and ≥1 import edge.
- [ ] **W13.2** **Jinja2** — `tree-sitter-jinja2` v0.0.14 (crates.io). Extract template variable refs (`{{ var }}`), `{% include %}` / `{% import %}` / `{% extends %}` as import edges, `{% macro %}` definitions. Covers GCP Deployment Manager templates + Ansible templating layer. **AC:** smoke test on a `.j2` fixture with `{% include %}` produces ≥1 import edge.
- [ ] **W13.3** **Helm / Go templates** — `ngalaiko/tree-sitter-go-template` (C source, 136★). Compile C source as `wicked-estate-extract` build-dep. Extract `{{ template "name" }}` as call edges, `{{ .Values.x }}` variable refs as struct nodes, `{{- define "name" }}` as function nodes. File extensions: `.gotmpl`, `.tpl`, plus YAML files in `templates/`. **AC:** smoke test on a Helm `deployment.yaml` produces ≥1 template-call edge.
- [ ] **W13.4** **ARM Templates** — no dedicated grammar; overlay on `tree-sitter-json` (already wired). Post-process string values matching `^\[.*\]$` as ARM function calls: `resourceId(...)`, `reference(...)`, `listKeys(...)`, `concat(...)` → call edges; `parameters(...)` / `variables(...)` → variable-ref edges. ARM `dependsOn` array values → depends-on edges. File extension: `.arm.json`, `azuredeploy.json`. **AC:** smoke test on an ARM template fixture produces ≥1 `dependsOn` edge and ≥1 `resourceId` call edge.

---

## Provenance ledger — "what we steal from whom"

| From | Idea(s) | Wave(s) |
|---|---|---|
| **prior art (01)** | two-phase + `unresolved_refs`; synthesizer cascade + provenance + precision gates; agent-behavior rules; `agent-eval`; `add-lang`; MCP surface | W0.6, W0.7, W3.1, W4.3, W6.3, W2.5 |
| **prior art (02)** | crash-safe batched-flush/resume; vectors-on-edges; graph-as-filter+vector-as-ranker; (fix dead PageRank, N-query BFS) | W2.6, W4.1, W5.2, W5.3 |
| **prior art (03)** | `GraphStore` trait/port; ORM-aware `.scm`; minified guard; (fix content-hash IDs, bloat) | W0.4, W6.2, W7.4, W8.2 |
| **prior art (04)** | graph-first retrieval + fallback telemetry; semantic blast-radius gate; reactive subscribe; SurrealDB prior art | W7.1–7.3, W4.4, W1.2 |
| **prior art (05)** | contract-doc pattern; static+LSP hybrid; drop-in extractors; staleness | W0.5, W3.3, W6.1, W7.4 |
| **SoA parsing (06)** | tree-sitter rules-as-data; SCIP merge; stack-graphs TSG rules; LSP-on-demand-only | W1.3–1.4, W2.1–2.3, W3.2–3.3 |
| **SoA systems (07)** | Aider PageRank+budget; LocAgent 3-tool; RRF hybrid; xxh3 file-watch; zstd snapshots | W4.1–4.3, W5.3, W2.6, W7.4 |
| **Storage decision (09)** | SQLite+FTS5+sqlite-vec default; SurrealDB bake-off; IndraDB excluded; the 3 decision metrics | W1.1–1.2, W1.5 |

## Anti-patterns this plan explicitly avoids
Regex-over-source synthesis (→W3.1 AST) · content-hash IDs (→W0.3/W1 stable) · N-statement BFS (→W4.4 CTE/native) · god-modules (→W0.1 crate split) · PageRank fields w/o pipeline (→W4.1) · filesystem-proxy blast-radius (→W4.4/W7.2 semantic) · speculative "graph algebra" (excluded) · bulk LSP (→W3.3 on-demand) · embeddings-first weight (→W5.2 opt-in) · **fan-out before the spine exists (→W0 gates everything)** · IndraDB (excluded, no FTS/vector).
