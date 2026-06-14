# Governance Index (W7.2 + W7.3)

This file indexes the two governance artifacts added in Wave 7 and explains how
they work together to keep the engine honest in a real development workflow.

---

## Artifacts

| Artifact | Wave | File |
|---|---|---|
| Semantic blast-radius pre-commit gate | W7.2 | `scripts/blast-check-pre-commit.sh` |
| Graph-first retrieval discipline | W7.3 | `docs/graph-first-retrieval.md` |

---

## What problem each artifact solves

### W7.2 — The "silent wide refactor" problem

An engineer (or an LLM agent) changes the signature of a heavily-called function and
commits.  Downstream callers break.  The developer only learns about the blast radius
after the fact — in CI, in review, or in prod.

`scripts/blast-check-pre-commit.sh` gates the commit.  Before `git commit` completes,
it:

1. Extracts changed top-level symbol names from the staged diff (best-effort grep over
   `+` lines for `fn`, `struct`, `class`, `def`, `func`, `type`, `interface`, `trait`,
   `impl`, `enum`).
2. Runs `wicked-estate blast-radius <name> --db .wicked-estate/graph.db` for each.
3. Sums the resolved-dependent counts across all changed symbols.
4. Compares the total to `CI_BLAST_BUDGET` (default: 50):
   - **≥ 70% of budget → WARN** (printed to stderr, commit proceeds).
   - **≥ 100% of budget → BLOCK** (exit 1, commit rejected).
5. Surfaces the `coverage:` line from every blast-radius call verbatim, so the
   developer sees that the count is a **lower bound** (best-effort static resolution,
   unresolved references reported separately).

**Fail-open semantics:** if `wicked-estate` is not built, `.wicked-estate/graph.db` does not
exist, or any individual symbol call times out (default: 10s), the gate skips rather
than blocks.  Infra absence is never a reason to stop a commit.

Install the hook:
```bash
ln -sf "$(pwd)/scripts/blast-check-pre-commit.sh" .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

### W7.3 — The "atrophy paper-bag" problem

Agents (and developers) default to `grep` / `cat` / file reads because they are always
available.  The code graph gets bypassed, receives no feedback, and atrophies.  A sparse
graph is bypassed even more.  The loop completes.  prior art observed this over 8 sprints
before codifying Rule 31 to break it.

`docs/graph-first-retrieval.md` codifies the discipline for `wicked_estate`:

- Graph query (MCP tools or CLI) is the **required first call** for any wicked-estateligence
  question.
- File reads are fallback only, and must be announced with a loud `GRAPH-FALLBACK:`
  marker (R6 from `docs/agent-behavior-rules.md`).
- Fallback rate per query class is the primary graph-coverage-gap KPI — it drives the
  language and resolution-tier backlog.

---

## How they interlock in a real workflow

```
                                 COMMIT
                                   │
            ┌──────────────────────▼──────────────────────┐
            │         blast-check-pre-commit.sh (W7.2)     │
            │                                              │
            │  1. extract changed symbols from staged diff │
            │  2. wicked-estate blast-radius <sym> per symbol │
            │  3. surface coverage: line (honest estimate)  │
            │  4. sum resolved dependents vs. budget       │
            │     70%+ → WARN; 100%+ → BLOCK              │
            │     graph/binary missing → FAIL-OPEN notice  │
            └──────────────────────┬──────────────────────┘
                                   │ commit proceeds
                                   ▼
                              AGENT WORK
                                   │
            ┌──────────────────────▼──────────────────────┐
            │      graph-first retrieval discipline (W7.3) │
            │                                              │
            │  query:  SearchEntity / TraverseGraph /      │
            │          RetrieveEntity / BlastRadius /      │
            │          ContextPack                         │
            │                                              │
            │  fallback only:  GRAPH-FALLBACK: marker +    │
            │                  reason + path               │
            │                                              │
            │  fallback rate → coverage-gap backlog        │
            └──────────────────────────────────────────────┘
```

The pre-commit gate forces the graph to be populated (because a missing graph means you
lose the blast-radius signal on every change).  The retrieval discipline forces the graph
to be *used* (because agents that bypass it provide no feedback).  Both are required for
the graph to stay alive and accurate over time.

---

## Engine-contract alignment

Both artifacts are grounded in the engine contract:

| Contract point | Governance enforcement |
|---|---|
| Blast radius = transitive dependents following `target == X` edges (`docs/ENGINE-CONTRACT.md` §1) | W7.2 gate uses `wicked-estate blast-radius` which follows this exact definition |
| `coverage:` line on every blast-radius call (R3 — partial coverage worse than none) | W7.2 surfaces the line verbatim; gate commentary calls it a lower bound |
| `GRAPH-FALLBACK:` marker when graph can't answer (R6) | W7.3 codifies the marker format and makes fallback rate the coverage-gap KPI |
| Confidence visible on every edge (R7) | W7.3 examples show that `ContextPack` / `TraverseGraph` carry confidence; grep carries none |
| Staleness reported (R5) | W7.3 requires agents to read `commits_behind` in diagnostics and annotate stale answers rather than silently bypass |

---

## Limitations (honest accounting)

**W7.2 — symbol extraction is heuristic.** The hook greps for common definition keywords.
It will miss symbols defined via macros, traits via `impl Foo for Bar` (captured as
`Bar` not `Foo`), lambdas, and symbols defined in files added without a recognizable
keyword pattern. The graph resolves the semantic blast radius correctly for the symbols
it *does* find; the heuristic only affects which symbols are queried. Future improvement:
use `wicked-estate query` over changed file paths to enumerate symbols precisely (W8+).

**W7.3 — enforcement is currently operational.** The discipline is documented and the
tool contract specifies the `GRAPH-FALLBACK:` marker, but no automated tooling counts
fallbacks or gates on rate yet. That is W8+/W9+ work. The prior art lesson is clear: ADVISORY
enforcement is empirically ignored; the goal is BLOCK semantics by W9.

---

*See also: `docs/agent-behavior-rules.md` (the five rules that W7.2+W7.3 enforce) ·
`docs/ENGINE-CONTRACT.md` (the edge-direction and confidence invariants) ·
`docs/plan/WAVE-PLAN.md` (W7 tracker)*
