# DES-MEM-FACETED-001 — Intent-Driven Faceted Worker Memory

**Status:** Draft, review-clean — reworked after v0 review (16/20 confirmed) then re-verified: **10/10 refinements confirmed, 0 blockers, no v0-regression**. Read side (Phases 1–3) build-ready as written; the two write-side majors (process-global `--readonly` → two-MCP split §5.3; untrusted session captures → §5.5) are folded in below. Awaiting human gate before build.
**Follows:** DES-GROUNDING-001 (`wicked-core/.product/`) §2 — the memory/knowledge follow-on
**Primary crates:** `wicked-estate-memory-core`, `wicked-estate-memory`, `wicked-estate-mcp` (estate); seams in `wicked-core` (`execute_wrapped.rs`) + `wicked-crew` (launch chain, acceptance/promotion)
**Review record:** `scratch/mem-faceted-design-review/{design.json,findings.md}` (v0); re-verification run `wf_0e81fdfd-09e`

---

## 1. Problem & model (locked with the user)

Governed workers ground reads in the estate index (DES-GROUNDING-001), but **memory** is single-axis: `memory.recall` walks one hierarchical `Scope` (ancestor-inheritance or one subtree prefix, `memory/src/lib.rs:133-167`). Real agent learnings are **multi-dimensional** and cut across each other:

- a **CLI** quirk (codex needs workspace-write) is reusable in *every* repo,
- a **repo** build gotcha is specific to that repo,
- a **tool** behavior (estate-mcp) travels across repos and projects,
- a **user** preference and a **project** decision are their own axes.

**The model** (the user's words): each learning is tagged at capture to **its natural axis**; recall reads the **session's intent** and includes the *unique combination* it needs — *"include what is needed, nothing else."* No per-axis hard/soft policy: intent naturally includes cross-cutting axes (they're in the session) and excludes irrelevant ones (another repo/project simply isn't in the intent).

Estate already proves this pattern in a **sibling domain**: `rules.recall` is faceted (`language`/`layer`/`framework`), orthogonal, wildcard-matched, specificity-ranked (`wicked-estate-retrieve/src/rules_recall.rs:96-104`). This design ports that model onto memory — an **extension**, not a new store.

---

## 2. What v0 got right, and what the review killed

The 28-agent recon→design→review workflow confirmed the **read-side** is real and buildable, and **killed the v0 write/safety mechanism** (evidence reproduced verbatim at every cited line). The corrected design keeps the first and replaces the second.

| v0 claim | Verdict | Correction (this doc) |
|---|---|---|
| Facets in `node.metadata`, AND-composed at the existing `scope.admits` rerank seam | ✅ real | Kept — §4 |
| `facet_matches` "reuse" | ⚠️ port-with-change | It's a ~15-line **port with a modified None-arm** (exclude-on-unsatisfiable), not verbatim — §4.3 |
| Write wall "enforced by PATH; `--readonly` belt-and-suspenders" | ❌ **fiction** | Global stores are plain SQLite files a worker can raw-open. **The wall is Boundary 1** (OS sandbox: worktree = only writable root). `--readonly` only closes the *tool surface*. — §5.1 |
| Session store **outside** the worktree (`WICKED_HOME/sessions/…`) so reap doesn't lose it | ❌ **contradicts Boundary 1** | Boundary 1 denies writes outside the worktree. Session store lives **inside** the worktree; harvest **before** reap. — §5.2 |
| Worker MCP "reads UNION [session + global --readonly]" via config | ❌ not expressible | `MemoryEngine` holds a *single* `store` (`memory/src/lib.rs:62-64`); `--readonly` is process-global (`mcp/src/main.rs:73-75`). Union-read is **new engine work** (`UnionMemStore`). — §5.3 |
| Captures "auto-faceted with the run intent" at the gate-hook | ❌ no interception | The governance gate-hook is allow/deny only (`execute_wrapped.rs:705-707`) — it can't rewrite a capture. **Facets are agent-declared** (natural axis); only the **session scope** is env-stamped by the estate MCP process. — §5.4 |
| Auto-facet = stamp the **whole intent** on every capture | ❌ **would break the model** | Stamping `{cli:codex, repo:x}` on a codex quirk makes it repo-x-only → it would *not* travel to repo:z. Facets must be the **natural axis** the capturer chooses. — §5.4 |
| `repo`/`tool` "observable today" at the intent seam | ❌ false | `GovernanceContext` has **no** `repo_ref` (`workflow.rs:101-152`); `allowed_skills` are **skill** ids, not tool ids. Only `cli` is observable today; `repo`/`project`/`user`/`tool` need plumbing/rederivation. — §6 |
| `from_node` reads facets "via serde default" | ❌ fiction | `Memory` has **no** serde; `to_node`/`from_node` are hand-written (`memory-core/src/lib.rs:111-112,165-245`). Backfill = **explicit** empty-map default in `from_node`. — §4.1 |
| `RecallQuery` "not `#[non_exhaustive]`, no external constructors" | ❌ both wrong | It **is** `#[derive(Serialize,Deserialize)]` with an in-tree literal constructor (`memory-core/src/lib.rs:287`); adding a field is a compile break — fix all constructors in the same change. — §4.4 |
| Specificity folded into the score | ⚠️ seam can't see it | `Candidate` (`recall.rs:26-34`) carries no facet/specificity field — thread `matched_axis_count` into the score at the rerank gate. — §4.3 |

The 4 non-confirmed findings are not treated as blockers.

---

## 3. Architecture at a glance

Two independent partition mechanisms on every memory, unchanged store schema:

- **Scope** (hierarchical, unchanged) — tenancy / write-isolation: `org > project > session:<run_id>`. Existing `ScopeFilter::Ancestors|Subtree` (`memory/src/lib.rs:133-167`) unchanged.
- **Facets** (orthogonal, new) — `BTreeMap<Axis,String>` in `node.metadata` under a new `facets` key — the **intent-matching** dimension.

**Read path:** wicked-core derives the session **intent tuple** + a task-text query → `memory.recall {query, intent, token_budget}` → candidates AND-composed at the *one* existing per-candidate rerank gate: `scope_filter.admits(mem.scope) && facet_admits(mem.facets, intent)` → specificity-boosted score → injected into the worker prompt.

**Write path (gated on `os_sandbox`):** worker captures land in an **in-worktree session store** (Boundary 1 makes the worktree the only writable root; the global store, outside it, is kernel-denied). Facets are agent-declared; session scope is env-stamped. At **acceptance** (evaluator≠creator, before deliver/reap) an operator promotion step re-homes proven captures to the global store at their natural facet home.

---

## 4. Read side (estate) — shippable independent of the write side

### 4.1 Facet storage + backfill
- Add `facets: BTreeMap<Axis,String>` to `Memory` (`memory-core/src/lib.rs:112-125`); `Axis` is a validated lowercase token, id non-empty. `BTreeMap` = deterministic serialization (mirrors rules' `Targets`).
- New `FACETS` meta_key (`:94-107`); persisted in the opaque `node.metadata` JSON blob (`node.rs:97,121`; `schema.sql:44`) — **zero store/schema change**, identical to how rules store `Targets` and memory stores `tier`/`scope`.
- `to_node` writes the map; **`from_node` explicitly defaults a missing `facets` key to an empty map** (there is no serde on `Memory` — `:111-112,165-245`). Empty facets ⇒ specificity 0 ⇒ always admitted ⇒ legacy memories behave exactly as today. **No `MEM_SCHEMA_VERSION` bump** (metadata-only).

### 4.2 Facet vocabulary
Closed-but-extensible, aligned to the intent axes: `user:<id>`, `cli:<key>` (claude/codex/antigravity/opencode/pi/copilot), `repo:<name>`, `project:<id>`, `tool:<id>` (e.g. `tool:estate-mcp`, `tool:wicked-garden-mem`); extensible to `skill:<id>`, `org:<id>`. Validation is **fail-loud at the MCP wire** (mirrors `scope` `parse_strict`, `tools/memory.rs:88-90`). Scope representation is **unchanged**.

### 4.3 Faceted recall predicate + ranking
- Port `facet_matches` (`rules_recall.rs:96-104`) into `memory-core` as **`facet_admits(mem_facets, intent)`** — for each **present** facet on the memory, the intent must carry that axis with the **same value**; an absent facet on the memory is a wildcard. **Modified None-arm (the review's correction):** a memory that *constrains* an axis the intent does **not** carry (e.g. `user:bob` with no session user) is **EXCLUDED** — "include what is needed, nothing else", and it prevents cross-user leakage. (This is *not* verbatim `facet_matches`, which includes-on-None.)
- **Specificity** = count of matched present facets (`{cli:codex,repo:X}`=2, `{repo:X}`=1, `{}`=0). Thread `matched_axis_count` onto `Candidate` (`recall.rs:26-34`) and fold it into the score as a **multiplicative boost** `(1 + β·matched)` alongside the existing `rrf·tier·recency·salience` (`recall.rs:37-39`) — a boost, **not** a hard primary sort (a hard sort would bury a highly-relevant global memory under a marginally-relevant 2-facet one).
- **AND-compose with scope at the one existing seam** where `scope.admits` already filters each candidate (`memory/src/lib.rs:600-631`): `pass ⇔ scope_filter.admits(mem.scope) && facet_admits(mem.facets, intent)`.

### 4.4 `RecallQuery` / `CaptureRequest`
- `CaptureRequest` is already `#[non_exhaustive]` (`:252`) — add `facets` additively.
- `RecallQuery` **is** `#[derive(Serialize,Deserialize)]` and has an in-tree struct-literal constructor (`:287`). Adding `intent` is a compile break → add `#[non_exhaustive] + Default` **and fix every in-tree constructor in the same change**.

### 4.5 MCP surface (fail-loud)
- `memory.capture`: add `facets[]` (validated).
- `memory.recall`: add `intent{}` (axis→value map).
- `memory.coverage`: add `by_facet` (`mcp/src/lib.rs:912-914`).
- Reuse the `RetrievalResult` envelope + R1(empty=OK+diagnostic)/R4(limit+loud truncation)/R5(staleness) conventions as-is.

**Ships as:** faceted capture + intent recall in-process and over MCP, fully back-compatible (legacy scope-only memories unchanged). Useful immediately for **operator/agent** memories and, once §6 wires intent, for worker grounding.

---

## 5. Write side — a built-in proposal queue

> **Design update (2026-09-06, supersedes the capture-file/promotion design AND §5.3's two-MCP split; defers §5.3 `UnionMemStore`).** A JSONL capture-file is a per-solution staging hack (every consumer re-implements parse+validate+promote, and it drags in worktree-reap races + deliver-leak fixes). Replace it with a **first-class proposal queue** in the estate graph. Agents **`propose`** — a *safe* write, because a proposal is inert (never recalled/applied) until approved — and **approval promotes** the payload to the active store. The queue is **type-generic** (memory + policy + future), so it is the one primitive the whole "governed knowledge" surface reuses ([[governed-knowledge-surface]]).

### 5.0 The proposal-queue primitive
- **Storage — a `proposal` node kind** (mirrors how a memory is a `Node` with `kind=Other("memory")`): `kind=Other("proposal")`, metadata `{ kind_type, payload (JSON), facets, provenance, state, created_at }`.
  - `kind_type`: `"memory"` | `"policy:<steering_type>"` | future.
  - `payload`: type-specific (memory: `{content, tier}`; policy: `{rule, severity, …}`).
  - `facets`: agent-declared orthogonal facets (Phase 1 `Facets`).
  - `provenance`: run/unit/agent/interaction id — **authority-stamped by the MCP from launch env** (`WICKED_RUN_*`), never trusted from the caller.
  - `state`: `pending | approved | rejected`.
  Proposal nodes are **not** memory/rule nodes, so `memory.recall`/`rules.recall` never surface them — a proposal is inert until approved.
- **Tools (estate MCP):**
  - **`propose`** — write a `pending` proposal node. This is the worker/agent write surface, and it is **safe by construction** (inert until approved). **Amend `--readonly` to ALLOW `propose`** while still refusing the 8 active-store write tools — the wall is about *active* stores; `propose` touches none. Spam is a nuisance, not pollution: proposals carry provenance (attributable, purgeable), and the approval gate rejects junk.
  - **`list_proposals`** — query the queue (filter by `kind_type` / `state` / facets). The UI approval-queue's source.
  - **`approve`** — promote a `pending` proposal to the **active store, routed by `kind_type`**: `memory` → the estate memory store (reuse `memory.capture` with the proposal's facets + stamped provenance); `policy:*` → the steering/rules store (see §5.2 — first cut may hand off to crew's steering-write). Mark the proposal `approved` (or delete). Operator surface — **NOT** exposed to the `--readonly` worker.
  - **`reject`** — mark `rejected` (or delete). Operator surface.
- **Why this is better than capture-file/promotion:** proposals are **in the DB the moment they're made** (no worktree-reap race, no leak-safety fix, no per-solution JSONL parser); the worker gets a *safe* write instead of no write; **it decouples the write side from `os_sandbox` entirely** (the worker never touches an active store; the queue is inert regardless of the sandbox); and one primitive serves memory + policy + future types.
- **Approval model** ([[governed-knowledge-surface]]): auto-approve-with-provenance (proposals land as `pending`; a human confirms in the UI, or a rule auto-approves) is the default; evaluator-review is an option per type/severity. Approval is where "untrusted → trusted" happens; the propose write never is.

### 5.1 The Boundary-1 wall still applies to the ACTIVE stores (raw-write floor)
Global memory/knowledge are plain SQLite files at `{WICKED_HOME}/{memory,knowledge}.db` (`mcp/src/main.rs:273-279`). `--readonly` (process-global, `:73-75`) and the bash-indexer deny only close the **sanctioned tool surface + named binary** — a worker with filesystem write can raw-open the file. **The hermetic write wall is Boundary 1's OS sandbox** (DES-INPUT-GOV-008): its profile is `(deny file-write*)` then `(allow file-write* (subpath …))` per armed root (`validator.rs:515-535`). The armed write set is the **worktree (primary) + launcher-declared deliverable `extra_write_roots` + (only for an estate-home graph) the exact `<estate_root>/<key>/` graph dir** (`execute_wrapped.rs:1100-1117`). The global stores live in a **different filesystem tree** from all of these (`{WICKED_HOME:-~/.wicked}/…` vs the worktree vs `~/.wicked-estate/repo-graphs/<key>/`), so they are **kernel-denied**. `--readonly` is belt-and-suspenders on the tool path.

**Consequence:** worker memory write-back is only hermetic when `os_sandbox` is ON. With it OFF, `execute_wrapped.rs:805-824` sets `sandbox=None` and nothing prevents a raw file write — so **write-back is hard-DISABLED (not merely "recall-only posture") until the sandbox is on**; workers read global `--readonly` only. This makes safe faceted write-back a concrete value driver for the `os_sandbox` rollout.

### 5.2 In-worktree session store, harvest as an in-run pre-deliver step
The per-run **session store** lives **inside the worktree** (`<worktree>/.wicked-session/memory.db`) so the sandbox permits the worker to write it (the worktree is subpath-allowed; workers get no system-temp carve-out — the temp allowances at `validator.rs:538,618` are `NetworkPolicy::Deny`-gated and workers pass `Allow`). Two pins:
- **No leak into the delivered PR.** The deliver step is `git add -u` (tracked paths only, `deliver.ts:194`) plus crew#434's classifier that denylists `*.db`/`*.db-wal`/`*.db-shm`. Belt-and-suspenders: add `.wicked-session/` to the worktree `.gitignore` at session-store creation so an untracked session DB can never be staged.
- **Harvest is an in-run phase, before deliver.** crew's acceptance *surface* is a post-terminal read-only route (`GET /runs/:id/acceptance`, `routes.ts:1549`) that may run after the worktree is reaped. Promotion (§5.5) must therefore be an **in-run phase scheduled before the deliver phase** (deliver is appended last, `adapter.ts:1058-1083`; reap at `deliver.ts:146`/`delivery-index.ts:116`/`routes.ts:1239`), so it reads the live worktree. A run that fails before that phase loses its (unproven) session captures — acceptable.

### 5.3 Two seams: a writable session MCP + a `UnionMemStore` for reads
`--readonly` is a single **process-global** arg-scan (`mcp/src/main.rs:73-75`), so one estate-MCP process cannot be readonly-for-global yet writable-for-session. Resolve the write path with **two MCP handles**, not a relaxation of `--readonly`:
- **Writes** → a **second estate-MCP process, NOT `--readonly`, with `WICKED_MEMORY_DB` pointed at the in-worktree session DB** (the server already honors `WICKED_MEMORY_DB`/`WICKED_KNOWLEDGE_DB`, `main.rs:274-277`). Its only writable target is the session store; the global path is never handed to it, and Boundary 1 denies it anyway.
- **Reads** → the existing global MCP stays `--readonly`; to let a worker recall its **own** session captures mid-run alongside global, introduce a **`UnionMemStore`** (session R/W + global R/O) whose `recall` merges both. `MemoryEngine` holds a single `store: Box<dyn MemStore>` (`memory/src/lib.rs:62-64`), and the fan-out seam is `with_backend(Box<dyn MemStore>)` (`:189`) — so this is a **real new store**, not a trivial add: `MemStore: GraphStore` (`store.rs:12`) is a ~45-method surface (`traits.rs:336`) that **must pass the GraphStore conformance kit** (multi-file-contribution, symbol_epoch reuse). Scope Phase 4 accordingly.

### 5.4 Facets are agent-declared; only session scope is env-stamped
There is no gate-hook interception that can rewrite a capture (`execute_wrapped.rs:705-707` is allow/deny). And stamping the **whole intent** would be wrong: a codex quirk stamped `{cli:codex, repo:x}` becomes repo-x-only and would **not** travel to repo:z — breaking the model. Therefore:
- **Facets = agent-declared natural axis.** The run intent is already injected into the worker prompt (it knows the vocabulary); the agent tags each capture with the axis the learning is *about*. The MCP validates fail-loud. (This validation is a *convenience*, not a security boundary — see §5.5.)
- **Session scope = env-stamped, reusing the existing grammar.** wicked-core already computes a per-run scope via `resolve_scope` = `wicked-agent/<session_id>/…` (`wicked-core/src/scope.rs:75-80`). The session-store MCP stamps `scope` under that existing namespace (e.g. `wicked-agent/<session_id>/shared`) rather than inventing a new `session:<run_id>` kind — so capture scope aligns with the scope the intent seam already knows.
- **Mis-tag mitigation:** a wrong-but-valid axis loses recall; promotion (§5.5) is where an evaluator **reviews and corrects the facet home** before anything graduates to global.

### 5.5 Promotion (crew) — treat session captures as UNTRUSTED
The session store is **fully worker-forgeable**: a worker can bypass the MCP and raw-write the session DB (it's in the writable worktree), so any facet, scope, or provenance *in* the session store is an unverified worker claim. Graduation to global is therefore the **only** path a worker learning reaches global, run as the in-run pre-deliver phase (§5.2), operator/evaluator-gated (evaluator≠creator), and it treats session rows as untrusted input:
- **Authority-stamp provenance** — the promoter stamps which run/unit produced the memory from its *own* authority; it never copies worker-written provenance.
- **Re-validate facets at the gate** — the evaluator confirms/repairs the natural-axis facet home rather than trusting the session row's tag.
- **Selective + re-homed** — only reviewed captures graduate, dropping the `wicked-agent/<session_id>/…` session scope and writing at their natural facet home in global. Never automatic-on-success. The load-bearing isolation is Boundary 1 (physical wall) + this gate — not the MCP's fail-loud validation.

---

## 6. Intent derivation (core + crew) — mostly plumbing

At the single seam `arm_input_governance` (`execute_wrapped.rs:1712-1852`), assemble the intent tuple + task-text query (`unit.description + session.problem` → the FTS/vector **query**, not a facet):

| Axis | Today | Work |
|---|---|---|
| `cli` | ✅ `input.unit.assigned_cli` (`:668-673`) | none |
| `repo` | ⚠️ `repoRef` already reaches `LaunchRunInput` (`types.ts:45`) but stops before `GovernanceContext` | thread the existing `repoRef` the last hop → `GovernanceContext` (or derive from `code_graph_db`) — less than a new field |
| `tool` | ⚠️ `allowed_skills` are **skill** ids | derive from the **wired MCP surface** (estate-mcp is wired), not `allowed_skills` |
| `project` | ⚠️ `projectId` already on `LaunchRunInput` (`types.ts:33`) but stops at membership filing (`adapter.ts:963-970`) | thread the existing `projectId` the last hop → `GovernanceContext` |
| `user` | ❌ absent from the entire launch chain | genuinely new identity field: `LaunchRunInput` → `LaunchOptions` → `AgentSession` → `GovernanceContext` |

Read-side value lands incrementally: **cli-only intent ships first** (cli-specific learnings travel across repos — the highest-value, lowest-plumbing axis), then repo/tool, then project/user.

---

## 7. Phasing (each row a shippable deliverable)

| Phase | Scope | Ships | Gate |
|---|---|---|---|
| **1 — Facet core** | estate `memory-core`: `facets` field + meta_key + hand-written `to_node`/`from_node` backfill; `facet_admits` (modified None-arm) + specificity on `Candidate`; `RecallQuery` `#[non_exhaustive]`+Default + fix constructors | faceted capture/recall in-process, back-compat | conformance + unit tests |
| **2 — MCP surface** | estate `mcp`: `facets[]`/`intent{}`/`by_facet` schema + fail-loud validation; R1/R4/R5 behavior tests | faceted capture/recall over MCP | behavior tests |
| **3 — Read intent (cli)** | core: derive `{cli}` + task query at `arm_input_governance`; inject recalled block into the worker prompt (`:2263-2310`) | workers recall global faceted memory as grounding (cli axis) | — |
| **4 — Union-read engine** | estate: `UnionMemStore` (session R/W + global R/O) | prereq for write-side; worker can recall its own session captures | conformance |
| **5 — Session capture** | estate mcp + core: second (writable, non-`--readonly`) session MCP via `WICKED_MEMORY_DB`; in-worktree session store (+ `.gitignore`); env-stamped session scope (existing grammar); agent-declared facets | workers capture faceted learnings into an isolated store | **`os_sandbox` ON** |
| **6 — Intent plumbing** | core + crew: thread existing `repoRef`/`projectId` the last hop + new `user`; fix `tool` axis | full-dimensional intent | — |
| **7 — Promotion** | crew: evaluator-gated **in-run pre-deliver** harvest → global at natural facet home; **session rows treated as untrusted** (authority-stamped provenance, re-validated facets) | proven learnings graduate | evaluator≠creator |
| **8 — Facet index (opt/perf)** | estate: `MemExt` facet side-table + index; `MEM_SCHEMA_VERSION` bump | scale beyond local-first volume | when volume warrants |

Phases 1–3 deliver worker grounding-from-memory with **no** dependence on `os_sandbox`. Phases 5+7 (write-back + graduation) are the `os_sandbox`-gated half.

---

## 8. Open decisions

1. **`repo`/`project` — facet vs scope authority.** `project` can be a scope segment today; pick **facet** as authoritative for intent-matching (scope stays tenancy) to avoid double-representation drift.
2. **`os_sandbox` coupling.** Confirm write-back (Phase 5) is hard-gated on `os_sandbox` ON, i.e. the write-side rollout rides the sandbox rollout decision. (Recommended — it's the only hermetic wall.)
3. **Session store lifecycle.** In-worktree `.wicked-session/memory.db`, harvested at acceptance (recommended) — accepts loss of unproven captures on early reap.
4. **Legacy scope→facet backfill.** Coexist without auto-migration (recommended); optional one-shot `reflect`-style re-tag for high-value legacy memories.
5. **Promotion granularity + policy.** Per-memory operator review at acceptance (recommended) vs per-facet-bucket / automatic-on-success (rejected — reintroduces the wall breach).
6. **Specificity β.** Boost weight for `matched_axis_count`; start conservative so global memories aren't buried.

---

## 9. Risks

- **`os_sandbox` OFF ⇒ no hermetic write wall.** Write-back is hard-disabled (recall-only) until the sandbox is on; otherwise a compromised/injected worker can raw-write the global store.
- **Session store is worker-forgeable.** A worker can bypass the MCP and raw-write its in-worktree session DB, so facet/scope/provenance there are unverified claims — promotion (§5.5) must authority-stamp provenance and re-validate facets, never copy them.
- **Facet post-filter is O(candidates)** (`sqlite.rs:2633-2678`, same as rules/scope today) — fine local-first, degrades at team volume → Phase 8.
- **Vector recall is a brute-force full scan** (`sqlite.rs:1325-1378`); facets only post-filter the ANN set, so a highly-specific facet with few memories can be starved → cap-aware retrieval / Phase 8.
- **Agent facet mis-tag** loses recall; contained by fail-loud vocabulary validation + promotion-time facet review.
- **Promotion is the deliberate wall breach** — must stay evaluator-gated, provenanced, never automatic.
- **`RecallQuery` constructor break** — contained by fixing all in-tree constructors in Phase 1.

---

## 10. Reuse ledger (grounded)

**Reuse:** `node.metadata` opaque storage (no schema change); the per-candidate `scope.admits` rerank seam (`memory/src/lib.rs:600-631`) as the AND-compose point; `facet_matches` shape (`rules_recall.rs:96-104`) as the base for `facet_admits`; the `RetrievalResult` envelope + R1/R4/R5 conventions; `find_symbols` kind+scope_prefix pushdown (facets stay post-fetch); `Symbol::synthetic` memory-node identity.
**New:** `facets` field + meta_key + hand-written backfill; `facet_admits` (modified None-arm) + specificity threading; MCP `facets[]`/`intent{}`/`by_facet`; `UnionMemStore`; in-worktree session store + env-stamped session scope; core intent derivation; crew identity plumbing + acceptance promotion.
