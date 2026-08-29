# wicked-estate — unified MCP foundation (v0.15.0)

## Contributor docs

Contributor docs: [`docs/`](./docs/)

Invariants and contracts: [`docs/ENGINE-CONTRACT.md`](./docs/ENGINE-CONTRACT.md).
Extension recipes: [`docs/add-lang.md`](./docs/add-lang.md) (new language),
[`docs/extractor-sdk.md`](./docs/extractor-sdk.md) (drop-in edge rules),
[`PLUGIN.md`](./PLUGIN.md) (runtime grammar plugins). Decision records:
[`docs/adr/`](./docs/adr/). Onboarding: [`docs/getting-started.md`](./docs/getting-started.md).
Upgrading from 0.14.x: [`docs/MIGRATION-0.15.md`](./docs/MIGRATION-0.15.md).
This pointer block is the machine-readable anchor agents grep for.

Rust MCP server: **code graph + memory + knowledge** in a single binary, 24 tools across 3 domains.
Turns a repo into a queryable graph (symbols, calls, imports, edges injected by event bus and commands)
and pairs it with a semantic memory store and a wiki/document knowledge store — all local-first,
zero infra, SQLite-backed. Built **fleet-parallel behind a fixed trait spine**. Live design:
`.product/DES-001-technical-design.md`.

> Guardrails distilled from prior systems and hard-won experience. They are not style preferences —
> each one is a scar. The trait spine prevents structural slop; these rules prevent process slop.

---

## Voice

Factual, brutally honest, engineering-focused. Not nice. Not agreeable.

**Do not:** agree to appease · soften bad news · hedge when evidence exists · open with "Great
question" / "You're absolutely right" · say "we" for your mistakes · pad with restatement.
**Do:** lead with what changes the next action · disagree with evidence · give exact counts
(tests passing, warnings, bench numbers) · separate verifiably-true from not-yet-true · say
"I don't know." Mandate: ship a working, benchmarked engine. Niceness is friction.

---

## Working Style

### §1 — Spine before fan-out
Resolution/extraction/storage all program against the five traits in `wicked-estate-core/src/traits.rs`.
**Never fan out work before the seam it depends on exists and its conformance test is green.**
For anything touching 3+ crates: map the trait seams first, separate the public trait from the
private impl, carve parallel-safe *file-disjoint* chunks, validate mid-flight. The spine (Wave 0)
is why fan-out is safe; do not erode it.

### §2 — Estimates are wall-clock
Dispatch a subagent per file-disjoint chunk. Each language extractor, each resolver tier, each
MCP tool is an independent chunk behind a trait — a "20-language" task is 20 parallel chunks, not
a serial slog. Serialize **only** real cross-cutting deps: the schema, the traits, the contract,
the chosen storage engine.

### §3 — Feature flags are last resort
Don't hide behind `FOO_ENABLED`. Justified only when progressive rollout is real, rollback is
known, and flag removal is in the same change. Default: validate and ship without a flag.

### §4 — Read history before non-trivial work
The five reviewed systems already made most of the mistakes. Before a non-trivial change, read
the relevant `docs/DESIGN-NOTES.md`, the `docs/adr/`, and `git log -S "<symbol>"`. We are
consolidating known art, not reinventing — cite which finding you're acting on.

### §5 — New crate/module needs a consumer
A new module is incomplete until something wires it via a trait **and** a conformance or bench
test references it. No orphan crates. A `// TODO` without a `WAVE-PLAN` task id is not acceptable.
Reviewer check: what reads this? If nothing — push back.

### §6 — Pipeline the next layer
Keep three layers in flight: (1) execution (code/agents), (2) recon (read-only agents writing
plans to `docs/recon/<topic>.md`), (3) independent validation (a validator with no shared
context). Do not idle-wait. Anti-pattern: sequential do → analyze → do → analyze.

### §7 — Every "done" needs a "still not done"
Every done claim requires an **evidence path**, a **falsifier**, and the **dependent not-yet-done**
work. Lead status with: (1) what's verifiably true + evidence, (2) what's not yet true that a
caller may assume is, (3) the next falsifiable claim.

| ❌ Wrong | ✅ Right |
|---|---|
| "Storage is done." | "`SqliteStore` passes conformance in-memory (W1.1). On-disk WAL untested; FTS5/sqlite-vec are W5; no perf numbers until the W1.5 bake-off." |
| "Tests pass." | "`cargo test --workspace` green at `<sha>`, 8 tests, 0 warnings. Count is stale after any new crate — re-run before re-claiming." |

### §8 — Retire as you go
Every migration deletes what it replaces in the **same change**. Delete the method/flag/file —
don't early-return it, don't leave it `#[allow(dead_code)]`. Reviewer question: what did this
change delete? If nothing — it's an addition, not a migration. Non-atomic exception needs a
`WAVE-PLAN` task id + deadline.

### §9 — Green is non-negotiable; slow is a defect
`cargo build --workspace` (0 warnings) + `cargo test --workspace` + `cargo clippy -D warnings` +
**GraphStore conformance** stay green every change; from W1.6 the **agent-eval benchmark must not
regress**. Never delete a test, lower the bar, or `#[ignore]` to go faster — profile and fix the
cause. Rigor is the product.

### §10 — Three strikes, then recon
After **3 failed attempts** at the same problem: STOP. No 4th variation. Send read-only recon
first — `git log -S "<symbol>"`, the `docs/DESIGN-NOTES.md`, the ADRs, prior session history —
and write findings to `docs/recon/<topic>.md` before attempt #4. The 3rd failure is evidence the
**model of the problem is wrong**, not the patch.

### §11 — Propagate every lesson across the fleet
A fix learned in one language/extractor is a **hypothesis about all the others**. The moment you
fix a defect *class* in one chunk, re-audit the already-done chunks for the same class **before**
claiming done — extractors are built fleet-parallel by isolated agents, so a bug in one almost
always has silent siblings. Fix at the **shared seam** (the trait, the generic extractor) over N
copies. Scar: MQ stored object names with surrounding quotes (`'PAY.IN'`) → unqueryable by real
name + broke RACF↔MQ matching; the same quote-leak was latent in COBOL `CALL 'SUB'` — fixed once
in the tree-sitter call path, covering every language at once.

### §12 — Parallel agents: per-crate builds, base-guard, lane-disjoint
Fan-out is the point (§2), but the mechanics have scars. **(a) Per-crate builds only.** A
fanned-out agent runs `cargo build/test/clippy -p <crate>`, NEVER `--workspace` — N agents each
building the whole workspace once filled the disk (117 GB, `ENOSPC` everywhere) and wedged every
build. **(b) Base-guard every isolated worktree.** Worktree isolation has branched agents from a
*stale* HEAD (the session's start commit, not the latest); an agent silently working a stale base
wastes the run. Make step 0 of every worktree agent: `git rev-parse HEAD` must equal the dispatch
SHA, else `git reset --hard <sha>` (a fresh worktree has nothing to lose). **(c) Lane-disjoint or
serialize.** In a *shared* checkout, an agent editing crate X breaks any concurrent agent whose
crate depends on X (uncommitted-broken state compiles into the dependent). Only crate-/file-disjoint
work is safe concurrently there; for same-crate or dependency-chain work use isolated worktrees
(each its own target dir) and merge after. **(d) Worktree commits use `--no-verify`** (the
`cargo fmt --all` pre-commit hook fails inside worktrees on the vendored grammar); the authoritative
`cargo fmt --all --check` runs on `main` at integration. Establish the seam and commit it green
*before* fan-out (§1) so every chunk builds against stable signatures.

---

## Universal Don'ts

- **No grandfathering.** Warnings, clippy lints, and failing/ignored tests go to **0 by fixing
  code**, never by `#[allow(...)]`, `#[ignore]`, or skipping. Done = 0 warnings, 0 ignored,
  conformance + bench green.
- **The verdict is the verdict.** Conformance / bake-off / benchmark / gate results never change
  based on who runs them or how urgent the moment is. A NO-GO is a NO-GO for everyone.
- **Rules as DATA, not code.** Resolution/extraction logic lives in `.scm` query files, TSG rules,
  and config — never compiled per-language `match lang { ... }` arms. (This is the lesson that
  archived a major per-language parser; see the design notes.) Writing per-language logic in Rust? Stop —
  make it data. A new language should be a new grammar + query file, **zero core change**.
- **Confidence + provenance on every edge.** Never emit an `Edge` without
  `{confidence, provenance, resolved_by}`. Never present a heuristic edge as a fact (agent rule R7).
- **Stable IDs only.** Never key a node by content hash or line number (ADR-002) — that was
  the rename-breaks-everything bug.
- **Bounded traversal only.** Every `traverse` carries `max_depth` + `max_nodes`. No unbounded
  whole-graph walks; use `WITH RECURSIVE` / a real traversal, never N-statements-per-node.
- **Never claim dead code without history.** `git log -S "<symbol>"` first. "No callers" can mean
  an incomplete migration, not permission to delete.
- **Don't rebuild the spine** — reuse it (below).

---

## Don't rebuild — reuse the spine

| Thing | Location |
|---|---|
| `Symbol` / `SymbolId` (stable identity) | `crates/wicked-estate-core/src/symbol.rs` |
| `Node` / `NodeKind` / `Edge` / `EdgeKind` / `Confidence` / `Provenance` / `ResolutionTier` | `crates/wicked-estate-core/src/{node,edge}.rs` |
| `Edge::new(tier, …)` — sets confidence + provenance from the tier | `crates/wicked-estate-core/src/edge.rs` |
| The five traits (`Extractor`/`Resolver`/`GraphStore`/`Ranker`/`RetrievalTool`) | `crates/wicked-estate-core/src/traits.rs` |
| `UnresolvedRef` / `Extraction` (two-phase staging) | `crates/wicked-estate-core/src/refs.rs` |
| **GraphStore conformance kit** — every store MUST pass it | `crates/wicked-estate-core/src/conformance.rs` |
| `MemStore` (reference) / `SqliteStore` (default) | `crates/wicked-estate-store/src/{lib,sqlite}.rs` |
| Benchmark harness + frozen corpus | `crates/wicked-estate-bench/src/lib.rs` |

---

## Locked decisions — do not relitigate

See the ADRs / contract for rationale; reopen only with new evidence.

- **Greenfield Rust, single static binary.** (the design notes)
- **Storage:** SQLite + FTS5 + sqlite-vec is the local-first default; **Postgres is built behind
  `--features postgres`** (`PostgresStore` — concurrent team writers + server-side `WITH RECURSIVE`
  traversal, same `open_store` factory, no re-index); **SurrealDB** is the W1.5 bake-off
  challenger (`SurrealStore`, built + conformance-tested behind `--features surrealdb`; no
  `open_store` factory arm, no bake-off verdict); **IndraDB excluded.** Storage lives behind `GraphRead` + `GraphWrite` (+ the
  `GraphStore` supertrait) and a single `open_store(spec)` factory, so a backend drops in as one
  module + one factory arm with **zero caller changes**. Retrieval negotiates `StoreCapabilities`.
  (the design notes, `docs/adr/ADR-003`)
- **Stable symbol identity**, not content-hash. (`docs/adr/ADR-002`)
- **Edge direction:** `source = dependent`, `target = dependency`. Blast-radius = dependents.
  (`docs/ENGINE-CONTRACT.md`)
- **Two-phase EXTRACT → RESOLVE**; resolution is swappable and never requires re-parsing.
- **Layered resolution** — the production `index`/`watch` slice is name → scoped → import-map →
  relative-import → infra → rules-bridge (order-independent; dedup keeps the max-confidence edge;
  activation table in `docs/ENGINE-CONTRACT.md` §3.1), with SCIP as the precise ingestion tier
  (TSG superseded, `docs/adr/ADR-007`). The LSP client (`lsp.rs`) is a built library whose only
  sanctioned consumer is the intent-routed edit plane (`docs/adr/ADR-009`, W3.6 — designed, not
  yet wired). **LSP is on-demand only, never bulk.**
- **Language coverage:** parity with the set (**114 in the manifest today** — `languages.toml` is
  the canonical count; the parity test gates a `≥73` floor) **and add-more-without-surgery**
  — languages are *data* (`crates/wicked-estate-extract/languages.toml`): a row + a `<name>.scm` file, no core
  change. The capability matrix is **generated** from that data (the thing prior art
  hand-maintained); precise axes (extends-vs-implements, cross-file refs) come from the resolution
  tiers, not tree-sitter.
- **Hybrid retrieval** = graph + FTS5 core, embeddings an **optional** sidecar fused via RRF.
- **Estate mapping is in scope** (designed, `ADR-004`): IaC (Terraform/CFN/ARM/Bicep/K8s/Pulumi) is
  *just more languages* (resources = nodes, depends-on = edges, no schema change); live cloud state
  via a read-only `Collector` (AWS/Azure/GCP/tfstate, **observe-only, no secret storage**); **drift** =
  graph diff by resource identity between `origin=iac` and `origin=live`. Build path: Waves W9/W10.

---

## The product's runtime behavior contract

How the *shipped tool* must behave toward consuming agents is governed by
`docs/agent-behavior-rules.md` (R1 `isError`→session abandonment · R3 partial-coverage is worse
than none · R4 output < 25K chars · R5 always report staleness · R6 loud `GRAPH-FALLBACK:` marker ·
R7 confidence visible). Every `RetrievalTool` implements them; they are verified by behavior tests
in W4.3. (Distinct from this file, which governs how *we build* wicked_estate.)

---

## Commands & gates

```bash
cargo build --workspace                                # must be 0 warnings
cargo test  --workspace                                # conformance + unit + bench-type tests
cargo clippy --workspace --all-targets -- -D warnings  # lint gate
cargo fmt --all                                        # format
```

**Gates that stay green per change:** workspace build (0 warnings) · all tests · `GraphStore`
conformance suite · (from W1.6) the agent-eval benchmark must not regress. Several Don'ts above are
candidates for dedicated CI lints (no content-hash IDs, no unbounded `traverse`, every edge carries
provenance) — track them as a future WAVE task before relying on review alone.

---

## Crate topology

```
crates/
  wicked-estate-core/         types + the five traits + conformance kit   (the spine — change with care)
  wicked-estate-extract/      tree-sitter Extractor impls (one module per language)
  wicked-estate-resolve/      Resolver impls: name/scoped/import-map/relative-import/infra/rules-bridge + SCIP ingest + LSP client library (ADR-009)
  wicked-estate-store/        GraphStore impls: MemStore, SqliteStore (+ SurrealDB bake-off)
  wicked-estate-rank/         Ranker: (personalized) PageRank over CALLS/IMPORTS
  wicked-estate-retrieve/     RetrievalTool: graph+FTS5+RRF hybrid retrieval (11 estate tools)
  wicked-estate-overlay/      XedgeStore: injected cross-repo edges (event→consumer, cmd→agent)
  wicked-estate-memory-core/  Memory types + MemoryApi trait + fuzzy/salience/scope logic
  wicked-estate-memory/       MemoryEngine + consolidation + cross-recall (6 memory tools)
  wicked-estate-memory-api/   Re-export shim for memory public API
  wicked-estate-knowledge/    KnowledgeEngine: wiki ingest/recall/relate/coverage (7 knowledge tools)
  wicked-estate-mcp/          MCP server — 24 tools across 3 domains; main.rs 4-store init
  wicked-estate/              `wicked-estate` watcher binary (emit events on file change)
  wicked-estate-bench/        agent-eval benchmark harness — the truth oracle
```

## Pointers

`README.md` · `.product/DES-001-technical-design.md` (design) · `.product/TEST-001-test-strategy.md` ·
`docs/ENGINE-CONTRACT.md` · `docs/adr/` · `docs/DESIGN-NOTES.md` ·
`docs/agent-behavior-rules.md` · `docs/benchmark-methodology.md`
