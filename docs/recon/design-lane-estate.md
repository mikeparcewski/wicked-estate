# Lane A — estate: DESIGN (knowledge-capability program)

> **Status:** DESIGN ONLY. No code changed. Every claim cites a real file:line.
> **Scope (charter §3):** expose `rank`/`clusters`/real-embedder/`annotate` over MCP; ship 3 code-graph
> skills (affordance, change-impact, rationale-archaeology); emit coarse events to wicked-bus via the
> SHARED Rust→bus seam that lanes B and C reuse.
> **Reviewer + antagonist will attack this.** Decisions are explicit with rationale, risks, and the
> riskiest assumptions named up front (§0).

---

## 0. TL;DR — decisions + the riskiest assumptions (attack these first)

**Decisions (one line each):**
- **D1.** Add **3 thin MCP tools** — `RankHotspots`, `Communities`, `Annotate` — to estate's existing
  registry (`all_tools` + `input_schema`, `wicked-estate-mcp/src/lib.rs:48,243`). Net surface goes
  **7 → 10** read tools + 1 mutating tool. No fat catalog. `SemanticSearch` stays exactly 1 tool.
- **D2.** **Fix the SemanticSearch mis-wiring** (it is *advertised* but **un-callable** today —
  `lib.rs:304` lists it, `lib.rs:337` dispatches against `all_tools()` which excludes it) and route it
  through `default_embedder()` (`wicked-estate/src/lib.rs:1147`) instead of the hardwired
  `with_hash_embedder` (`lib.rs:73,309`). Semantic is **additive**, gated on a real vector store; the
  lexical floor (`SearchEntity` FTS5/BM25) is untouched and remains first-class (C4).
- **D3.** **Skills via `skill://` bundling**, copied verbatim from memory-mcp
  (`wicked-memory/crates/wicked-memory-mcp/src/lib.rs:42,151,160`): a `const SKILLS: &[(name, desc,
  body)]` with `include_str!`, surfaced through `resources/list` + `resources/read`. Three skills:
  `power-moves`, `change-impact`, `rationale-archaeology`. estate ships **0 skills today**; this adds
  the capability to the MCP server (`capabilities.resources`), no new tool schemas.
- **D4.** **Events via a small Rust emit shim that shells out to the `wicked-bus` CLI**
  (`std::process::Command`), fire-and-forget, non-blocking, file/run-coarse. **Not** direct
  `bus.db` append, **not** an in-process Node binding. Three logical events
  (`estate.indexed`/`estate.drift`/`estate.annotated`) mapped to the bus contract as
  `wicked.estate.{indexed,drifted,annotated}` (the `wicked.<noun>.<past-verb>` regex,
  `wicked-bus/lib/validate.js:9`), `domain=wicked-estate`. The shim is the **shared seam B and C reuse**.

**The 5 assumptions an antagonist should target (ranked by blast radius):**
1. **A1 — "shell-out to `wicked-bus` is acceptable latency/coupling for an emit on the index hot
   path."** A spawn is ~5–30 ms. I argue it is amortized against a multi-second index and made
   non-blocking by detaching, but **this is the load-bearing call**. If the reviewer rejects subprocess
   on the indexer, the fallback (a thin appended NDJSON spool the bus drains) must be designed instead.
   (§3.3 weighs all three; the spool is the named fallback.)
2. **A2 — "the dimension-mismatch footgun is contained."** A store embedded with `HashEmbedder` (128-d,
   `retrieve/src/lib.rs:1726` note) cannot be queried by `FastEmbedder` (384-d) — cosine over mismatched
   dims is silent garbage. D2 makes the real embedder reachable; if index-time and query-time embedders
   disagree, results are **wrong, not empty** — the worst C4 failure. Mitigation in §1.3 (persist the
   embedder id in `meta`, refuse + loud-fail on mismatch). Antagonist: prove the guard is missing or
   bypassable.
3. **A3 — "`Annotate` over MCP doesn't become a graph-corruption / unbounded-write vector."** It is the
   only **mutating** tool on a server that is otherwise read-only (`GraphRead`-only handler signature,
   `mcp/src/lib.rs:323`). It needs a `GraphWrite` handle the server does not currently hold, plus R4
   value caps and append-not-upsert semantics (`traits.rs:180`). Antagonist: find the write that
   corrupts the graph or escapes the cap.
4. **A4 — "3 skills is the right cut, not 1 and not 6."** The charter says richness lives in skills
   (DEFINE §8), but skills are still context an agent pulls. I justify exactly 3 against the 3 lane-A
   acceptance outcomes (§6 of DEFINE). Antagonist: argue one is redundant with a tool, or that
   change-impact + rationale should be one skill.
5. **A5 — "coarse events are enough for lane C's reactive layer."** estate's `changes` log is
   deliberately file-granular (`change.rs` header, `traits.rs:163`). I emit per-run summaries, not
   per-file. If lane C needs per-file reaction it must drain `changes_since` (`traits.rs:111`) directly,
   not the bus. Antagonist: show a lane-C reaction that the coarse event starves.

---

## 1. MCP tool exposure (THIN)

### 1.0 What exists vs. what is reachable (the gap, grounded)

| Capability | Engine op exists? | Reachable over MCP today? | Evidence |
|---|---|---|---|
| rank / hotspots (T6) | yes — `ranked_symbols`, `important_symbols` | **NO** — CLI arm only | `rank/src/lib.rs:289`; `wicked-estate/src/lib.rs:846`; CLI `"rank"\|"hotspots"` arm `main.rs:872` |
| community detection (T12) | yes — `detect_communities` + `summarize_communities` | **NO** — CLI arm only | `rank/src/community.rs:347`; `rank/src/cluster_summary.rs:92`; CLI `"clusters"` arm `main.rs:1360` |
| semantic search (T5) | yes — `SemanticSearch` tool | **advertised but UN-CALLABLE** | listed `mcp/src/lib.rs:304`; dispatched against `all_tools()` (excludes it) `mcp/src/lib.rs:337`; hardwired hash embedder `lib.rs:73,309` |
| real embedder (T2) | yes — `default_embedder()` tiered FastEmbed→model2vec→hash | **NOT used by the server** | `wicked-estate/src/lib.rs:1147`; MCP builds `_sem_store_for_future_ctx` (note the `_`, unused) `mcp/src/main.rs:175` |
| annotate (T3 write) | yes — `GraphWrite::annotate` + typed `Annotation` | **NO** — CLI arm only, and MCP holds `&dyn GraphRead` (read-only) | `traits.rs:180`; `annotation.rs:131`; CLI `"annotate"` flags `main.rs:344`; handler is read-only `mcp/src/lib.rs:323` |
| find_by_requirement (annotate read) | yes — `GraphRead::find_by_requirement` / `annotations` | partial — `RetrieveEntity` inlines annotations already (`retrieve/src/lib.rs:562`), but no requirement-indexed lookup tool | `traits.rs:116,121`; `retrieve/src/lib.rs:262` |

**Design principle (DEFINE §8 thin-tools discipline + charter "Resist a fat surface"):** expose the
**verbs an agent cannot already reach**, denormalize their output so no N+1 follow-up is needed (the
pattern estate already uses, `retrieve/src/lib.rs:77,108`), and put method/orchestration in skills (§2),
not new tools. Net: **+3 tools**, not +8.

### 1.1 `RankHotspots` — centrality over the graph (T6)

- **DECISION:** new `RetrievalTool` impl `RankHotspots` in `wicked-estate-retrieve` (NOT a one-off in
  the MCP crate — it must satisfy the `RetrievalTool` trait `traits.rs:228` so it is testable in
  isolation and reusable by lane C). Register in both `all_tools()` lists and `input_schema`.
- **Method:** wraps `wicked_estate_rank::ranked_symbols(store, seeds, top_n)` (`rank/src/lib.rs:289`).
  Seeds optional → unseeded = global PageRank (the `rank` CLI behavior, `main.rs:872`); seeded =
  personalized (the Aider repo-map pattern already in `render_context`, `retrieve/src/lib.rs:1315`).
- **Why a tool, not a skill:** PageRank is a performant Rust primitive that beats the brain bar (the
  spec's "must clearly beat markdown+BM25+LLM"). An agent cannot reconstruct centrality from
  `SearchEntity`. This is the canonical "performant library = tool" case (spec premise, line 7).
- **Input schema** (slots into `input_schema(name)` `mcp/src/lib.rs:243` exactly like the others):
  ```json
  { "type": "object",
    "properties": {
      "seeds":  { "type": "array", "items": {"type":"string"},
                  "description": "Optional stable SymbolIds to personalize ranking (100x teleport weight). Empty = global PageRank." },
      "limit":  { "type": "integer", "default": 25, "maximum": 200,
                  "description": "Top-N symbols by score." } },
    "additionalProperties": false }
  ```
- **Output** (denormalized — no N+1, mirrors `BlastRadius.summary.top_by_pagerank` shape
  `retrieve/src/lib.rs:954`): `{ "ranked": [ {symbol,name,kind,file,line_1based,score} ], "total": N,
  "seeded": bool }` + the standard `staleness_note()` diagnostic (`retrieve/src/lib.rs:45`, R5).
- **C4 compliance:** scores are advisory, not facts; the diagnostic states "PageRank is a heuristic
  ranking, not a correctness signal" (R7 spirit). No new schema, no new edge kind.

### 1.2 `Communities` — community detection (T12)

- **DECISION:** new `RetrievalTool` `Communities` in `wicked-estate-retrieve`. Read-only. Wraps
  `detect_communities` + `summarize_communities` (`rank/src/community.rs:347`,
  `cluster_summary.rs:92`). It is the **`--json --summary` CLI path** (`main.rs` clusters arm builds
  exactly this object, ~`main.rs:1460`) lifted to MCP — same output contract, no new computation.
- **Graph mode only over MCP (DECISION + cut):** the CLI also has a `--weight semantic` DBSCAN/KMeans
  path (`main.rs:1372`, `semantic_clusters` `rank/src/semantic_cluster.rs:247`). **Do NOT expose the
  semantic clustering mode as a tool parameter in v1.** Rationale: (a) it requires a populated vector
  store + a matching embedder (same A2 footgun); (b) the *spec* uses T12 cluster as the **cold-start
  hint for emergent relation types** (spec C5, line 140; T12 line 180) — that is lane B's ontology
  expedition, which will call the engine op directly in combined mode, not estate's MCP. Keep estate's
  tool to the always-available graph community detection. This is the thin-surface discipline biting on
  purpose. (Re-open if lane B proves it needs semantic clusters *through* estate's MCP rather than the
  shared store.)
- **Input schema:**
  ```json
  { "type": "object",
    "properties": {
      "min_size":   { "type": "integer", "default": 2, "minimum": 2,
                      "description": "Drop communities smaller than this (CommunityParams.min_size)." },
      "resolution": { "type": "number",  "default": 1.0,
                      "description": "Louvain γ; >1 = smaller/tighter, <1 = coarser." },
      "limit":      { "type": "integer", "default": 50, "maximum": 200,
                      "description": "Max communities returned (largest-first)." } },
    "additionalProperties": false }
  ```
  (Maps to `CommunityParams` `community.rs:29`; `hierarchical`/`package_bias` omitted from the surface —
  available via CLI for power use, not worth a tool param. Thin.)
- **Output** (the existing summary JSON, `cluster_summary.rs:52`): `{ "communities": [ {id, size,
  label_candidates, dominant_files, modularity_contribution, members:[symbolId]} ], "total": N,
  "modularity": Q }` + staleness diagnostic. `label_candidates` = top-PageRank members
  (`cluster_summary.rs` `top_symbols`) so the agent gets ready-to-name groups (the spec's
  cluster→synthesize step, spec §3.4).
- **No mutation:** the CLI `--annotate` write path (`main.rs:1377` writes `community`-type annotations)
  is **deliberately excluded** from the tool. Clustering-as-cache mutation stays a CLI/indexer concern;
  the MCP tool is a pure read (R-rule posture: reads don't surprise the agent).

### 1.3 Real embedder on the semantic path — FIX the mis-wiring (T2/T5)

This is **two defects + one wiring change**, not a new tool.

- **Defect 1 (un-callable tool):** `tools/list` advertises `SemanticSearch` when `has_semantic_search`
  (`mcp/src/lib.rs:304-313`), but `handle_tools_call_ctx` finds the tool in `all_tools()`
  (`mcp/src/lib.rs:337`) — which **excludes** `SemanticSearch` by construction (`all_tools` doc,
  `lib.rs:46`). So a conformant agent that lists-then-calls gets `-32602 unknown tool 'SemanticSearch'`.
  This violates C4 loud-failure / R1 (advertise a capability you can't honor). **FIX:** the dispatch
  must use a registry that includes the live semantic instance when present.
- **Defect 2 (hash floor masquerading as semantic):** both the advertised description (built from a
  throwaway `MemStore` `with_hash_embedder`, `lib.rs:309`) and `all_tools_with_semantic`
  (`lib.rs:73`) hardwire `HashEmbedder`. Hash embeddings are **lexical, not semantic** (the test at
  `retrieve/src/lib.rs:3843` documents exactly this: hash fails the synonym property). So the one
  "semantic" path ships **non-semantic** quality and never beats the lexical floor — it *is* the
  lexical floor wearing a vector costume.
- **DECISION (the wiring):** in `wicked-estate-mcp/src/main.rs`, when a real vector store opens
  (`db_path != ":memory:"`, `main.rs:174`), build the semantic tool with
  `SemanticSearch::new(default_embedder(), sqlite_vec_store)` (`retrieve/src/lib.rs:2010`;
  `default_embedder()` `wicked-estate/src/lib.rs:1147` already does the tiered FastEmbed → model2vec →
  hash selection with **loud `EMBED-FALLBACK:` markers** on each downgrade). Pass that live tool into
  the handler so `tools/call` can dispatch it. Stop using `with_hash_embedder` as the server default.
- **Additive, never the floor (C4, charter, spec line 207):** `SearchEntity` (FTS5/BM25,
  `retrieve/src/lib.rs:337`) is untouched and stays the always-on, model-free floor. `SemanticSearch`
  appears **only** when (a) a real store is present **and** (b) embeddings exist. When the active
  embedder is the hash fallback, the tool **must self-label** in its description/diagnostic
  ("LEXICAL-FALLBACK: no semantic model loaded; results are lexical") so an agent never mistakes hash
  output for semantic recall. That is the C4 "loud failure / confidence visible" invariant applied to
  the embedder tier.
- **A2 mitigation — the dimension guard (load-bearing):** record the embedder identity + dim in store
  `meta` at `index --embeddings` time (the `GraphStoreMutExt::meta_set_key` seam already exists,
  `store/src/lib.rs:903`). At server start, if the query-time `default_embedder()` dim ≠ the stored dim,
  **do not advertise SemanticSearch** and emit a loud diagnostic ("EMBED-MISMATCH: store embedded with
  <id>/<dim>, runtime is <id>/<dim>; semantic search disabled, re-index with --embeddings"). Silent
  cosine over mismatched dims is the worst-case C4 violation (wrong, not empty) — this guard converts
  it to an honest absence. **This guard is new and must be designed/tested explicitly; its absence is
  the bug A2 predicts.**
- **No schema change to `semantic_search_schema()`** (`mcp/src/lib.rs:213`) — the surface is identical;
  only the embedder behind it becomes real and the advertise/dispatch/guard logic is fixed.

### 1.4 `Annotate` — typed-annotation WRITE over MCP (T3 write side)

The only **mutating** tool. estate has rich typed annotations (`annotation.rs:131` — `type/key/value/
confidence/provenance/author/ts/source_type/extraction_method/last_verified`) reachable for *read* via
`RetrieveEntity` (`retrieve/src/lib.rs:562`) but **not writable over MCP**, and unreachable by
requirement.

- **DECISION:** add **one** mutating tool `Annotate`, plus reuse the existing read seam for the lane-A
  acceptance outcome ("`annotate` writes a typed annotation retrievable via `find_by_requirement`",
  DEFINE §6). Because `find_by_requirement` writes go through `set_node_semantics` (`traits.rs:169`)
  while typed annotations go through `annotate` (`traits.rs:180`), the tool must cover **both** write
  surfaces or the acceptance outcome can't be met by one tool. **Resolution:** `Annotate` writes a
  typed annotation; a thin `op:"requirement"` variant routes to `set_node_semantics` so the
  retrievable-via-`find_by_requirement` outcome is satisfiable. Reads reuse `RetrieveEntity` (already
  inlines annotations) + a tiny `FindByRequirement` is **NOT** added as a separate tool (thin —
  `RetrieveEntity` + the CLI `by-requirement` cover read; over MCP the agent gets annotations inline on
  retrieve). *Open question OQ3 flags whether the requirement-read needs its own MCP tool for lane C.*
- **The architectural change `Annotate` forces (call this out for the reviewer):** the MCP handler is
  **read-only by type** — `handle_request(store: &dyn GraphRead, ...)` (`mcp/src/lib.rs:422`),
  `handle_tools_call_ctx(... store: &dyn GraphRead ...)` (`mcp/src/lib.rs:323`). A write tool needs a
  `&mut dyn GraphWrite` (or the `GraphStoreMutExt` the binary uses, `store/src/lib.rs:899`). **DECISION:**
  split dispatch — read tools keep `&dyn GraphRead`; the single write tool takes a mutable store handle
  threaded from `main.rs` (which already opens a writable `SqliteStore`, `mcp/src/main.rs:20,176`). The
  `RetrievalTool` trait (`invoke(&self, &dyn GraphRead, ...)`) **cannot** express a write, so `Annotate`
  is **NOT** a `RetrievalTool` — it is a separate `MutatingTool` concept handled on its own dispatch arm
  in the MCP crate. This keeps the read trait pure (the trait doc's "cannot accidentally mutate"
  guarantee, `traits.rs:5`) and confines mutation to one auditable arm.
- **Input schema:**
  ```json
  { "type": "object",
    "required": ["symbol", "key", "value"],
    "properties": {
      "symbol":     { "type": "string", "description": "Stable SymbolId to annotate (no-op if absent — annotate is INSERT, never creates a node)." },
      "type":       { "type": "string", "default": "note",
                      "description": "Annotation type (note|assumption|question|... or custom). Advisory class computed from type." },
      "key":        { "type": "string", "maxLength": 256 },
      "value":      { "type": "string", "maxLength": 8000,
                      "description": "R4-bounded. Truncated loudly if longer." },
      "confidence": { "type": "number", "default": 1.0, "minimum": 0, "maximum": 1 },
      "provenance": { "type": "string", "default": "mcp-agent" },
      "op":         { "type": "string", "enum": ["annotate","requirement"], "default": "annotate",
                      "description": "'requirement' routes value→set_node_semantics(requirement=value) for find_by_requirement." } },
    "additionalProperties": false }
  ```
- **Semantics (grounded in `traits.rs:180`):** `annotate` is a **bare INSERT, not an upsert** — the same
  symbol can carry many annotations; the store stamps `ts` when `0`; stable-SymbolId keying means
  annotations survive renames (ADR-002). `author` is forced to `"mcp-agent"` server-side (provenance
  honesty — an agent cannot impersonate `system`-derived authorship; `is_system_derived`
  `annotation.rs:84`). **No `delete` over MCP in v1** — deletion (`delete_annotations` `traits.rs:185`)
  is a maintenance/CLI concern; an agent that can append but not delete cannot corrupt history. Thin +
  safe.
- **A3 mitigations:** (1) R4 value cap (8000 chars, matching the source-bundle budgets); (2)
  append-only, no delete; (3) confidence + provenance **mandatory** on the written annotation (house
  guardrail "confidence + provenance on every edge" extended to annotations; the `Annotation`
  constructor already requires them, `annotation.rs:182`); (4) no-op on absent symbol (cannot inject
  orphan annotations). Antagonist target: a value that escapes the cap, or `op:"requirement"` writing a
  validated-flag it shouldn't.

### 1.5 Net surface (the thin-tools receipt)

`tools/list` goes from **7** (`mcp/src/lib.rs:48`, the `tools_list_returns_seven_tools` test
`mcp/src/lib.rs:543`) to **10 read tools** (+`RankHotspots`, +`Communities`, + a *correctly callable*
`SemanticSearch`) **+ 1 mutating tool** (`Annotate`). The 7→10 read count updates that conformance test
(retire-as-you-go, §8 of CLAUDE.md — the test that asserts "exactly 7" is changed in the same change,
not left stale). `Lineage` already exists in the retrieve crate (`retrieve/src/lib.rs:1018`) but is
**not** in `all_tools()` today — **out of scope for this lane** unless change-impact (§2.2) needs it
exposed; flagged OQ4.

---

## 2. Skills (estate ships NONE — adopt memory-mcp's `skill://` bundling)

### 2.0 The bundling mechanism (verbatim from the reference impl)

memory-mcp bundles skills **with the binary** as MCP resources — no separate server, no Python, no
filesystem dependency. The mechanism (`wicked-memory/crates/wicked-memory-mcp/src/lib.rs`):

```rust
// const table of (name, description, body) — body via include_str! so skills travel in the binary
const SKILLS: &[(&str, &str, &str)] = &[(
    "codebase-expedition",
    "…description…",
    include_str!("../skills/codebase-expedition/SKILL.md"),    // lib.rs:46
)];

// initialize advertises the resources capability:
"capabilities": { "tools": {}, "prompts": {}, "resources": {} }              // lib.rs:144
// resources/list → one entry per skill, uri = skill://<name>/SKILL.md       // lib.rs:151-159
// resources/read → match the uri, return { contents:[{uri,mimeType,text}] } // lib.rs:160-174
```

- **DECISION:** replicate this exactly in `wicked-estate-mcp`. Add `const SKILLS: &[(&str,&str,&str)]`
  with three `include_str!("../skills/<name>/SKILL.md")` entries; add `"resources": {}` to the
  `initialize` capabilities (`mcp/src/lib.rs:285` currently advertises only `{"tools":{}}`); add
  `resources/list` and `resources/read` arms to `handle_request_ctx` (`mcp/src/lib.rs:433` match).
  Skill `.md` files live under `crates/wicked-estate-mcp/skills/<name>/SKILL.md`.
- **Why bundle, not a tool (DEFINE §8):** skills are pulled **on demand** by the agent via
  `resources/read`; they cost **zero** always-on tool-schema context. "Leaning into skills *lowers*
  always-on context vs tools while giving agents *more* access" (DEFINE §8). estate's value is the
  tools; the *method of combining them* is the skill.
- **§5 consumer guardrail (CLAUDE.md):** each skill is "wired via a trait and referenced by a
  conformance/bench test" — the resources path gets a test mirroring memory's
  `skills_bundled_as_resources` (`wicked-memory/.../lib.rs:437`) so no orphan skill ships.

### 2.1 Skill: `power-moves` (the affordance skill)

- **Name:** `power-moves` (uri `skill://power-moves/SKILL.md`). The spec/charter call it
  "affordance/power-moves".
- **Teaches:** *what estate's surface can do and the high-leverage tool combos* — the affordances an
  agent won't discover from tool descriptions alone. Concretely:
  - The map of all 10+1 tools → the question each answers (`SearchEntity`=find by name,
    `BlastRadius`=what breaks if I change X, `Lineage`=what X depends on, `RankHotspots`=where the
    important code is, `Communities`=what the natural modules are, `ContextBundle`=one-shot budgeted
    context, `SemanticSearch`=concept search *when present*, `Annotate`=record a finding).
  - **Power combos:** "to onboard a repo: `RankHotspots` (unseeded) → `Communities` → `ContextBundle`
    on each community's `label_candidates`." "To understand a symbol: `RetrieveEntity` →
    `BlastRadius` + `Lineage` → `ContextBundle` seeded on it."
  - **The C4 reading rules:** always read the `STALENESS:` diagnostic; treat `R7-CONFIDENCE` /
    low-confidence edges as heuristics; `LEXICAL-FALLBACK`/`EMBED-MISMATCH` mean semantic is off — fall
    back to `SearchEntity`. This teaches the agent estate's honesty contract
    (`docs/agent-behavior-rules.md`, R1/R3/R5/R6/R7).
- **Why a skill:** this is judgment/method, not computation — exactly the "belongs in a skill, not a
  tool" bucket (spec §8). It is also lane C's seed for its "unified affordance" combined skill (DEFINE
  Lane C).

### 2.2 Skill: `change-impact`

- **Name:** `change-impact` (uri `skill://change-impact/SKILL.md`).
- **Teaches the method** the lane-A acceptance outcome demands ("the change-impact skill returns the
  correct dependents + governing rules for a seeded change", DEFINE §6):
  1. Resolve the changed symbol (`SearchEntity` → stable id).
  2. `BlastRadius` (dependents — what breaks; `retrieve/src/lib.rs:769`) **and** `Lineage`
     (dependencies — what it relies on; `retrieve/src/lib.rs:1018`) for the full two-sided picture.
  3. **Read the coverage honestly:** `BlastRadius` reports `unresolved_callers`
     (`retrieve/src/lib.rs:821`) — the skill teaches "blast radius is best-effort static resolution and
     MAY be incomplete" (the tool's own coverage diagnostic, `retrieve/src/lib.rs:884`). This is the C4
     "partial coverage reported, not hidden" discipline as *method*.
  4. **Governing rules:** traverse `edge_kinds=["invoked_by"]` / `["governs"]` (the rules-engine edges
     `TraverseGraph` documents, `retrieve/src/lib.rs:639`) to find ruleset/rule nodes governing the
     changed code; surface any `RulesInventory` hits.
  5. Triage with `RankHotspots` seeded on the change — highest-PageRank dependents first (the
     `top_by_pagerank` already in the blast summary, `retrieve/src/lib.rs:954`).
- **Why a skill, not a tool:** it is an *orchestration* of `BlastRadius`+`Lineage`+`TraverseGraph`+
  `RankHotspots` with judgment about coverage and rule-governance. Baking it into a tool would be the
  fat-surface anti-pattern. Lane C composes this with memory recall into its combined change-impact
  (DEFINE Lane C).

### 2.3 Skill: `rationale-archaeology` (uses `edge_history`)

- **Name:** `rationale-archaeology` (uri `skill://rationale-archaeology/SKILL.md`).
- **The hook — a real seam most tools ignore:** `GraphRead::edge_history(file)` (`traits.rs:104`)
  returns *superseded* edges a file produced at prior git versions, each tagged with its blob SHA —
  "the brain remembers old connections" (Wave 7). It is **NEVER traversed**, pure provenance lookup.
  **No MCP tool exposes it today.**
- **DECISION + dependency:** this skill needs `edge_history` reachable. Rather than a fat new tool,
  expose it as a **read field on a focused tool** — *but* the thin-surface rule says don't add a tool
  unless needed. Resolution: the skill drives the **CLI** `changes`/history path for v1
  (`changes_since` is already a CLI arm, `main.rs:1280`-area), and **OQ2** flags whether
  `rationale-archaeology` warrants a dedicated `EdgeHistory` MCP read tool for lane C's combined
  rationale skill. (I am explicitly *not* adding `EdgeHistory` as a tool in this lane to avoid surface
  creep; if lane C needs it over MCP, it's a one-line `input_schema` + `RetrievalTool` add then.)
- **Teaches:** *reconstruct why the code is shaped as it is, from evidence, not guesswork* —
  1. For a file/symbol, read its **annotations** (`RetrieveEntity` inlines them,
     `retrieve/src/lib.rs:562`) — especially `assumption`/`question` advisory types
     (`annotation.rs:74`) and their `provenance`/`source_type`/`last_verified` evidence envelope.
  2. Read `edge_history` (via CLI/OQ2) — *which dependencies existed before and disappeared* — the
     archaeology signal: a removed edge is a refactor scar.
  3. Cross-reference git provenance (`repo_info`, `traits.rs:100`; `file_git_sha`, `traits.rs:97`) to
     anchor each finding to a commit.
  4. **Capture the reconstructed rationale back** as an annotation via `Annotate`
     (`op:"annotate", type:"note"`, confidence < 1.0, provenance = "rationale-archaeology") — closing
     the loop so the next agent inherits the finding. This is the spec's "knowledge lives as a graph"
     premise made concrete on the code graph.
- **C4 compliance:** every reconstructed claim carries confidence + provenance (it is an annotation, so
  the constructor enforces it, `annotation.rs:182`); staleness via `last_verified`
  (`annotation.rs:164`); the skill explicitly teaches "this is reconstruction, mark confidence honestly."

### 2.4 Skill cut justification (answers A4)

Three skills map 1:1 to the three lane-A capability themes and the three acceptance outcomes (DEFINE
§6): **affordance** (drive the full surface), **change-impact** (the named seeded-change outcome),
**rationale-archaeology** (the `edge_history` / annotation-provenance differentiator that beats brain's
flat markdown). They are **disjoint methods**, not variations: power-moves is *breadth* (what can I
do), change-impact is *forward/backward dependency reasoning*, rationale-archaeology is *temporal/
provenance reasoning*. Merging change-impact + rationale would conflate "what breaks now" with "why it
got this way" — different questions, different tools (`BlastRadius` vs `edge_history`). Cutting below 3
drops a named acceptance outcome. Adding a 4th (e.g. a clustering/ontology skill) duplicates lane B's
ontology-expedition (DEFINE Lane B) — out of scope.

---

## 3. Events + the SHARED Rust→bus emit seam

### 3.0 Hard constraints (grounded)

- **wicked-bus is Node.js, not Rust.** It is an npm package with a `better-sqlite3` native peer
  dependency (`wicked-bus/README.md:24,150`). There is **no Rust crate** to link. estate is a single
  static Rust binary (CLAUDE.md "Locked decisions"). So the seam is **cross-runtime** by necessity.
- **The bus write is a non-trivial, versioned contract** — `validateEvent` (required fields, 128-char
  type cap, regex), idempotency-key (uuid), TTL → `expires_at` + `dedup_expires_at` math, a payload
  CAS sha, `registry_schema_version`, a 16-column INSERT (`wicked-bus/lib/emit.js:29,~83`;
  `lib/validate.js:9,31`). `schema_version > 1.x` is rejected (WB-005). This is owned by the bus and
  evolves independently.
- **Fire-and-forget is the bus's own contract:** "producers are non-blocking. The bus never slows the
  caller. If it's not installed, callers degrade gracefully" (`README.md:137`). The emit SKILL's
  reference pattern wraps everything in try/catch and "never throws from fire-and-forget"
  (`emit/SKILL.md:50-78`).
- **estate already has the non-blocking-emit shape and subprocess precedent.** Telemetry uses
  `emit_cli_span` → `sink.export_spans(...)` with `if let Err(e) = ... { eprintln!("telemetry: {e}") }`
  (`wicked-estate/src/main.rs:285,312`) — fire, log on failure, never propagate. estate shells out to
  `git` via `std::process::Command` in **4 places** (`main.rs:1844`, `lib.rs:189,1010`,
  `scip_auto.rs:152`) — subprocess is an **established, blessed** pattern here, not a new dependency
  class.

### 3.1 Mechanism — the three options, weighed

| Option | How | Verdict |
|---|---|---|
| **(a) Direct append to `bus.db`** | Open the bus's SQLite from Rust (`rusqlite`), `INSERT INTO events …` | **REJECTED.** Reimplements `validateEvent` + idempotency uuid + TTL math + payload-CAS-sha + `registry_schema_version` (`emit.js`, `validate.js`) in Rust, and **couples estate to the bus's private, versioned schema** — the moment the bus bumps `schema_version` (WB-005) or adds a column (it has 16, incl. `payload_cas_sha`, `registry_schema_version`), estate silently writes malformed rows. Violates the bus's own encapsulation. A WAL writer race against the bus daemon is a second hazard. |
| **(b) Shell out to `wicked-bus emit`** | `std::process::Command::new("npx").args(["wicked-bus","emit","--type",…,"--domain","wicked-estate","--payload",@file])` | **CHOSEN.** Uses the bus's **public CLI** (`README.md:37`, `emit/SKILL.md:108`) — validation, idempotency, TTL, schema-version are the bus's job, always correct. Mirrors estate's existing `git` subprocess pattern. Graceful degradation falls out for free: if `wicked-bus` isn't on `PATH`, the spawn errors → we `eprintln!` and continue (the `git` arms already handle `Command` failure with `.ok()` / `unwrap_or`, `lib.rs:189`). |
| **(c) Small in-process emit shim (Node addon / FFI)** | Link or embed a JS runtime | **REJECTED.** Adds a heavyweight runtime dependency to a single-static-binary engine (CLAUDE.md locked decision). Massive overkill for ≤3 events on coarse boundaries. |

**DECISION (D4):** Option **(b)** — a small Rust **`emit` shim** (a private `fn emit_bus_event(event_type,
subdomain, payload)` in the `wicked-estate` binary crate, beside `emit_cli_span`) that builds the args
and **spawns `wicked-bus emit` detached, non-blocking**, swallowing all errors with a loud-but-harmless
`eprintln!("bus-emit: …")`. **This shim is the SHARED seam** — its design (args, env override for the
bus binary, the event-name mapping, the fire-and-forget discipline) is what lanes B and C copy. To make
it literally shared rather than copy-pasted, **OQ1** proposes lifting it into a tiny
`wicked-estate-observe`-adjacent helper (estate already has `wicked-estate-observe`, `crates/`) so
memory + knowledge depend on one implementation. For *this lane* it lives in the estate binary; the
**contract** below is the shared artifact.

### 3.2 Non-blocking discipline (the exact shape)

```text
fn emit_bus_event(event_type, subdomain, payload_json):
    # 1. resolve the bus binary: env WICKED_BUS_BIN override, else "wicked-bus" (npx fallback)
    # 2. write payload to a temp file (avoids arg-length + shell-quoting on large payloads;
    #    the CLI supports --payload @file, emit/SKILL.md:119)
    # 3. spawn DETACHED:  Command::new(bin).args([... "--payload", "@<tmp>"])
    #        .stdin(null).stdout(null).stderr(piped).spawn()
    #    -> do NOT .wait() on the index hot path; the child outlives the call (fire-and-forget)
    # 4. on spawn Err: eprintln!("bus-emit: wicked-bus unavailable ({e}); event dropped (non-fatal)")
    #    -> NEVER propagate; emit is best-effort telemetry, identical posture to emit_cli_span
```

- **Why detached spawn, not `.output()` (answers A1):** `.output()` blocks for the child's full
  lifetime (5–30 ms+ incl. Node cold-start + `better-sqlite3`). On a bulk index that fires one summary
  event per run that is negligible; but to be safe the shim **does not wait** — it spawns and moves on,
  so even a slow bus never slows the indexer. The OS reaps the child. (If detached-orphan reaping is a
  concern on the reviewer's platform, the named fallback is a short bounded `.wait_timeout` of ~250 ms,
  still non-blocking-in-practice.)
- **A1 fallback (if subprocess is rejected outright):** estate appends events as **NDJSON to a spool
  file** (`~/.something-wicked/wicked-estate/emit-spool.ndjson`) and a tiny bus-side drainer (lane C,
  which already runs a bus-drain reactive layer, DEFINE Lane C) tails it and calls `emit`. This keeps
  estate pure-Rust + zero-runtime, at the cost of one more moving part on the consumer side. Designed,
  not chosen — (b) is simpler and uses the blessed `git`-style precedent.

### 3.3 Event set + bus mapping (coarse, per-run/file)

estate's `changes` log is file-granular *on purpose* — "one delta per changed/removed file, NOT per
node/edge, so the log never explodes" (`change.rs` header; `traits.rs:163,111`). The bus events are
**even coarser: one per command run** (a summary), because the bus is a control/announce plane, not the
delta log. Per-file reaction stays on `changes_since` (A5).

The spec's logical names (`estate.indexed`/`estate.drift`/`estate.annotated`) are **not legal bus
`event_type`s** — the regex is `^wicked\.[a-z0-9_]+(\.[a-z0-9_]+)*$` and the convention is
`wicked.<noun>.<past-tense-verb>` (`validate.js:9`; `naming/SKILL.md:25,31`). Mapping:

| Logical (spec) | Bus `event_type` | `domain` | `subdomain` | Fired at (real site) | Payload (coarse summary) |
|---|---|---|---|---|---|
| `estate.indexed` | `wicked.estate.indexed` | `wicked-estate` | `index.run` | end of the `"index"` CLI arm (`main.rs:589`, after `index_path` `lib.rs:223`) + the `watch` debounced re-index (`main.rs:1202,1236`) | `{ root, files_indexed, nodes, edges, embeddings:bool, commit:repo_info.head, db_path }` (counts from `GraphStats`, `repo_info` `traits.rs:100`) |
| `estate.drift` | `wicked.estate.drifted` | `wicked-estate` | `drift.detected` | end of the `"drift"` CLI arm (`main.rs:692`, after `estate_drift` `lib.rs:917`) **only when drift is non-empty** | `{ added, removed, changed, by_resource_kind, db_path }` (the `DriftReport`, `lib.rs:917`) |
| `estate.annotated` | `wicked.estate.annotated` | `wicked-estate` | `annotate.write` | after a successful `Annotate` MCP write (§1.4) **and** the CLI `annotate`/`clusters --annotate` arms (`main.rs:344,1377`) | `{ symbol, type, key, count_written, author, provenance }` — coarse: for `clusters --annotate` it is ONE event with `count_written=N`, not N events (explosion guard) |

- **Past-tense verb fix:** `indexed`/`drifted`/`annotated` are past tense per the convention (the
  naming SKILL's bad example `complete` → `completed`, `naming/SKILL.md:46`). `drift`→`drifted`,
  `annotate`→`annotated`.
- **Coarseness guarantee (answers A5):** at most **one** bus event per CLI run / per MCP write. A
  100-file index = 1 `wicked.estate.indexed`. A 50-community `--annotate` = 1 `wicked.estate.annotated`
  with `count_written=50`. The bus never sees per-node traffic. Lane C reactions that need per-file
  granularity (e.g. re-link exactly the changed files) **drain `changes_since`** (the file-granular log,
  `traits.rs:111`) keyed off the coarse "something indexed" wakeup — the event is the *doorbell*, the
  `changes` log is the *delta*. This is the explicit division the design takes a position on; if a
  lane-C reaction can't be expressed as "wake on doorbell, read the delta log," A5 is falsified and the
  event granularity must be reconsidered.
- **Idempotency:** pass an explicit `idempotency_key` (`emit/SKILL.md:92`) of
  `wicked-estate:<event_type>:<commit-sha or db-mtime>:<run-nanos>` so a retried/duplicate index run on
  the same commit doesn't double-fire downstream reactions (the bus dedups on the key,
  `emit.js` `dedupExpiresAt`).

### 3.4 What B and C reuse (the shared-seam contract)

The artifact lanes B + C consume is **not estate's internal counts** — it is the **shim contract**:
(1) shell-out-to-`wicked-bus emit`, detached, non-blocking; (2) `domain` = the producing package;
(3) `wicked.<noun>.<past-verb>` types; (4) `--payload @tmpfile` for large payloads; (5) explicit
idempotency key; (6) errors → `eprintln!`, never propagate; (7) coarse boundaries — one event per
run/operation, never per item. Lane B emits `wicked.knowledge.ingested` / `wicked.relation.typed` on
the same shim; lane C drains all of them. **OQ1** decides whether the shim is physically shared (a
helper crate) or a documented copy.

---

## 4. House-guardrail compliance check (CLAUDE.md)

| Guardrail | How this design honors it |
|---|---|
| **Rules as DATA, not code** | No new `match lang {}` arms; tools wrap existing engine ops; events are config-shaped (type/domain/subdomain strings), not per-event Rust branches. |
| **Confidence + provenance on every edge** | `Annotate` forces confidence + provenance (constructor `annotation.rs:182`); `RankHotspots`/`Communities` emit advisory scores with heuristic diagnostics (R7). No new edges emitted. |
| **Stable IDs only** | Tools key on `SymbolId`; `Annotate` is stable-id-keyed so annotations survive renames (`traits.rs:180`). No content-hash/line keys. |
| **Bounded traversal only** | `RankHotspots`/`Communities` don't traverse; change-impact skill drives `BlastRadius`/`Lineage` which already carry `max_depth`/`max_nodes` (`retrieve/src/lib.rs:798,1048`). |
| **Per-crate builds (§12)** | New tools land in `wicked-estate-retrieve` + `wicked-estate-mcp`; emit shim in `wicked-estate` binary. Each built `-p <crate>`. The 7→10 conformance test updated in-crate. |
| **Retire as you go (§8)** | `tools_list_returns_seven_tools` (`mcp/src/lib.rs:543`) is rewritten, not duplicated; the dead `with_hash_embedder` server default is **deleted**, not flag-guarded; `_sem_store_for_future_ctx` (the unused `_` binding, `main.rs:175`) is wired or removed. |
| **New module needs a consumer (§5)** | Each tool referenced by a behavior test; each skill referenced by a `resources` test (mirror `wicked-memory/.../lib.rs:437`); the emit shim referenced by an emit test + consumed by the index/drift/annotate arms. |
| **Feature flags last resort (§3)** | Real-embedder selection reuses the **existing** `fastembed`/`model2vec` Cargo features via `default_embedder()` (`lib.rs:1147`) — no new flag. SemanticSearch presence is runtime-gated on store+dim, not a flag. |
| **Agent-behavior R1/R3/R5/R6/R7** | All new read tools return `Ok` on empty (R1), report coverage (R3 — `Communities` empty-graph → empty array + diag), emit `STALENESS:` (R5), use loud markers `LEXICAL-FALLBACK`/`EMBED-MISMATCH`/`bus-emit:` (R6), surface confidence (R7). |
| **C4 lexical floor** | `SearchEntity` FTS5/BM25 untouched and first-class; semantic + clusters strictly additive and self-labeling when degraded. |

---

## 5. DECISIONS · RATIONALE · RISKS · OPEN-QUESTIONS (consolidated)

### DECISIONS
- **D1** 3 thin tools (`RankHotspots`, `Communities`, `Annotate`); `SemanticSearch` fixed not added.
- **D2** Fix the advertised-but-uncallable `SemanticSearch` + route it through `default_embedder()`;
  add the embedder-dimension guard; keep lexical floor first-class.
- **D3** `skill://` bundling (memory-mcp pattern) with 3 skills: `power-moves`, `change-impact`,
  `rationale-archaeology`; `resources/list`+`resources/read`; zero new tool schemas.
- **D4** Emit via a non-blocking detached `wicked-bus emit` subprocess shim (not bus.db append, not a
  Node binding); 3 coarse events `wicked.estate.{indexed,drifted,annotated}`, one per run/op; shim
  contract is the shared seam for B + C.

### RATIONALE (the load-bearing ones)
- Thin surface beats fat: skills carry method at zero always-on cost (DEFINE §8); only un-reachable
  verbs become tools.
- Subprocess-to-CLI keeps the bus's versioned validation/idempotency/TTL where it belongs and reuses
  estate's blessed `git` subprocess precedent — no schema coupling, free graceful degradation.
- Real embedder is *additive* and self-labeling; the dimension guard converts the silent-wrong-cosine
  failure into honest absence (C4).

### RISKS
- **R-A1** subprocess latency/orphan-reaping on the index path (mitigated by detached spawn; spool
  fallback designed).
- **R-A2** embedder dimension mismatch → silently wrong semantic results (mitigated by the meta-stored
  dim guard; **this guard is net-new and must be tested**).
- **R-A3** `Annotate` is the first mutating MCP tool → needs a `GraphWrite` handle the read-only handler
  lacks; risk of graph corruption / unbounded writes (mitigated: separate `MutatingTool` dispatch arm,
  append-only, no-delete, R4 value cap, forced author/provenance).
- **R-A4** skill-cut judgment (3) could be wrong (justified 1:1 against acceptance outcomes; disjoint
  methods).
- **R-A5** coarse events may starve a lane-C reaction (mitigated by the doorbell+delta-log split;
  falsifiable).
- **R6 (new)** changing `tools_list_returns_seven_tools` is a visible contract break for any existing
  estate MCP consumer expecting 7 — must be communicated, not just silently bumped.

### OPEN QUESTIONS
- **OQ1** Is the emit shim physically shared (lift into a helper crate consumed by estate+memory+
  knowledge) or a documented copy per lane? (Leaning: helper crate, since `wicked-estate-observe`
  already exists as the natural home, `crates/wicked-estate-observe`.)
- **OQ2** Does `rationale-archaeology` need a dedicated `EdgeHistory` **MCP** read tool, or is the CLI
  `changes`/history path enough for lane A (with lane C adding the tool when it composes the combined
  rationale skill)? (Leaning: CLI for lane A, defer the tool to keep the surface thin.)
- **OQ3** Does lane C's reactive re-link need a `FindByRequirement` MCP read tool, or do inline
  annotations on `RetrieveEntity` + the CLI `by-requirement` cover it? (Leaning: cover it; re-open if C
  needs requirement-indexed read over MCP.)
- **OQ4** Should `Lineage` (exists in retrieve, `retrieve/src/lib.rs:1018`, but absent from
  `all_tools()`) be promoted into `all_tools()` so the `change-impact` skill can call it over MCP? It is
  *needed by* the skill (§2.2 step 2). **This may force a 4th tool add** — flagged because it touches
  the "exactly 3 new tools" claim. (Leaning: yes, promote `Lineage` — it already exists and is required
  by an acceptance outcome; that makes it "+3 net new behaviors + 1 promotion," still thin.)
- **OQ5** Detached-spawn vs bounded-`wait_timeout(250ms)` for the emit shim — platform-dependent
  (orphan reaping). Decide with the reviewer's target OS in hand.

---

## 6. Falsifiers (how to prove this design wrong)
1. Demonstrate a single bus `emit` subprocess adds >100 ms to a representative index run → kills D4(b),
   forces the spool (A1).
2. Show the embedder-dimension guard can be bypassed so a hash-embedded store is queried with a
   384-d vector → C4 violation, D2 incomplete (A2).
3. Find an `Annotate` input that writes past the 8000-char cap, impersonates `author=system`, or
   mutates a node it shouldn't → A3.
4. Produce a lane-C reaction that needs per-file granularity and **cannot** be expressed as
   "wake on `wicked.estate.indexed`, read `changes_since`" → A5, event granularity wrong.
5. Show that `power-moves` ⊆ tool descriptions (i.e. it teaches nothing a good tool description
   doesn't) → A4, the skill is redundant.
