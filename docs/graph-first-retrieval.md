# Graph-First Retrieval Discipline (W7.3)

**Wave:** W7.3  
**Source:** prior art Rule 31 (adapted) — the design notes §4  
**Enforced by:** R3 (partial-coverage) + R6 (fallback marker) in `docs/agent-behavior-rules.md`

---

## What it says

Every agent that reads code intelligence state — symbol definitions, call relationships, import
graphs, blast-radius estimates — **MUST query the code graph first** using the MCP tools or CLI.

Direct file reads (`cat`, `grep`, text search over source files) are **fallback only**.  
Fallback must be:

1. **Announced** with a loud `GRAPH-FALLBACK:` prefix (R6).
2. **Explained** with a reason and the query that failed.
3. **Counted** — fallback rate is the primary graph-coverage-gap KPI.

This is not a preference. It is a hard discipline enforced by the tool contract.

---

## Why this matters — the atrophy loop

The prior art project documented an 8-sprint failure mode (S286.85–S286.117) called the
"atrophy paper-bag": agents defaulted to reading raw files because it was easier than
querying an imperfect graph. Disuse caused the graph to atrophy from lack of feedback.
A sparse graph made direct file reads _more_ appealing, completing the doom loop.

For `wicked_estate` the same trap is present. If the MCP tools are bypassed whenever the
graph is incomplete, the incompleteness never surfaces as a signal and never gets fixed.

The discipline makes completeness visible: every fallback is evidence that the graph is
missing something. Recurring same-reason fallbacks feed directly into the language
coverage backlog.

---

## The query path (what agents MUST use first)

The `wicked_estate` MCP server (W4.3) exposes five retrieval tools.  These are the required
first-call for any wicked-estateligence question:

| Tool | Use for | CLI equivalent |
|---|---|---|
| `SearchEntity` | Find a symbol by name/kind/file pattern | `wicked-estate query <name>` |
| `TraverseGraph` | Follow edges from a symbol (callers, callees, imports) | `wicked-estate blast-radius <name>` (dependents) |
| `RetrieveEntity` | Get the full node record for a known symbol ID | `wicked-estate query <name>` |
| `BlastRadius` | Count transitive dependents of a symbol | `wicked-estate blast-radius <name>` |
| `ContextPack` | Build a ranked, token-budgeted context for an agent | (composite, W4.3) |

These tools return `RetrievalResult { content, diagnostics }`.  The `diagnostics` field
carries staleness (`commits_behind`), coverage gaps, and `GRAPH-FALLBACK:` markers when
the graph itself had to fall back internally.

---

## The fallback marker protocol

When a tool or agent **cannot** get the answer from the graph and must read source files
directly, it MUST emit a marker in this format — no exceptions:

```
GRAPH-FALLBACK: query="<what was asked>" reason="<why the graph couldn't answer>" path="<file(s) read>"
```

**Examples:**

```
GRAPH-FALLBACK: query="callers of parse_import" reason="symbol not indexed (Go file outside scan root)" path="src/go/parser.go"
```

```
GRAPH-FALLBACK: query="blast-radius of SchemaRegistry" reason="graph db stale by 14 commits" path="src/registry/schema.rs,src/ingest/loader.rs"
```

The marker MUST appear **at the top of the response** before any content derived from
the file read, so it is visible even if output is truncated (R4 — 25K char cap).

---

## Coverage-gap signal

The fallback rate per query class is tracked as a first-class KPI.  The intended flow:

```
GRAPH-FALLBACK emitted
    → captured in agent session log
    → aggregated by query class + reason
    → recurring reasons → filed as coverage backlog items (language, symbol kind, resolution tier)
    → coverage extension ships
    → fallback rate for that class drops
```

This is how the graph stays alive rather than atrophying.  An agent that never emits
`GRAPH-FALLBACK:` is either always finding answers in the graph (good) or silently
ignoring the discipline (bad — this is what R6 is designed to detect).

---

## Tie-in to agent-behavior rules

| Rule | How graph-first retrieval enforces it |
|---|---|
| **R3 — Partial coverage is WORSE than none** | If the graph covers some languages but not others, a tool that doesn't declare this makes the agent think it has the full picture. The fallback marker + diagnostics coverage field expose exactly what is missing. |
| **R5 — Always report staleness** | Tools include `commits_behind` in diagnostics. A graph that is 20 commits behind should be queried but its answers annotated as potentially stale — not bypassed silently. |
| **R6 — Loud fallback marker** | Defined here. Fallback rate is the graph-coverage-gap KPI. |
| **R7 — Confidence is visible** | Low-confidence edges (heuristic, tags tier) are labeled in graph output. An agent that bypasses the graph and guesses from file content has no confidence signal at all. |

---

## Concrete examples

### Example 1 — Finding callers of a function

**Wrong (direct grep, no marker):**
```
grep -r "parse_import" src/
```

**Wrong (fallback without marker):**
The agent reads `src/parser.rs` and reports the callers it finds there.

**Correct (graph-first):**
```
SearchEntity(name="parse_import", kind="Function")
→ returns NodeId: sym_3a9f...
TraverseGraph(root=sym_3a9f, direction=Dependents, max_depth=3, max_nodes=100)
→ returns 7 callers with confidence and location
```

If `SearchEntity` returns empty (symbol not indexed):
```
GRAPH-FALLBACK: query="callers of parse_import" reason="symbol not in graph — file may not be indexed yet" path="src/parser.rs"
```

---

### Example 2 — Assessing blast radius before a refactor

**Wrong:**
```
git grep -n "SchemaRegistry" | wc -l   # → "37 uses, should be safe"
```
This counts text matches, not semantic dependents. It misses indirect callers,
re-exports, and dynamic dispatch.

**Correct (graph-first):**
```
BlastRadius(symbol="SchemaRegistry", max_depth=5, max_nodes=200)
→ returns: 14 resolved dependents; coverage line reports 3 unresolved references
```

The coverage line reads:
```
coverage: 14 resolved dependent(s); 3 unresolved call(s) reference 'SchemaRegistry'
— best-effort static resolution, MAY be incomplete (precise tier pending)
```

The agent reads this literally: 14 is a lower bound. 3 more may exist. The answer is
not "37 uses" — it is "14 confirmed + 3 unresolved."

---

### Example 3 — Building context for a targeted edit

**Wrong:**
```
Read("src/ingest/loader.rs")
Read("src/ingest/schema.rs")
Read("src/registry/schema.rs")
# → 3200 lines dumped into context
```

**Correct (graph-first):**
```
ContextPack(focal_symbol="SchemaRegistry", budget_chars=8000, include_callers=true)
→ returns ranked, signature-only context for 8 symbols; full bodies only for
  the 2 highest-PageRank callers; total: 6100 chars
```

This respects R4 (25K cap) and R7 (confidence visible) while keeping context tight.

---

## What graph-first does NOT mean

- It does not mean "never read files." Files are the fallback when the graph cannot
  answer. The fallback is legitimate; the silence about it is not.
- It does not mean "block on a missing graph." If the graph is absent, tools fail-open
  (R2). The developer is not blocked — but fallbacks are marked.
- It does not mean "trust the graph blindly." The coverage and staleness diagnostics
  exist precisely because the graph can be incomplete. Read them.

---

## Enforcement

The current enforcement level is **operational discipline** (documented, not automated).

Future waves:

| Wave | Enforcement |
|---|---|
| W7.3 (now) | Discipline documented; tool contract specifies `GRAPH-FALLBACK:` marker |
| W8+ | Fallback rate tracked in session logs; flagged in agent-eval benchmark |
| W9+ | Pre-merge check: sessions with >20% fallback rate without coverage tickets are flagged |

The prior art project found that ADVISORY enforcement is empirically ignored (CDR-025: "ignored
across 130+ sprints").  The staggered ladder above reflects that — the goal is BLOCK
semantics at W9, not advisory.

---

*See also: `docs/agent-behavior-rules.md` R3/R5/R6/R7 · `docs/governance.md` · `docs/ENGINE-CONTRACT.md` §5 (wire contracts) · W7.2 gate: `scripts/blast-check-pre-commit.sh`*
