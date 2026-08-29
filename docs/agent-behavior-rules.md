# Agent-Behavior Rules (ported from the A/B-validated findings)

**Wave:** W0.7 · **Source:** the design notes

These are empirical constraints prior art learned from A/B testing real agent sessions. They are
not opinions — they are how an agent *actually* reacts to a wicked-estateligence tool. Every
`RetrievalTool` and the MCP server (W4.3) MUST implement them. They are cheap to honor and
expensive to relearn.

## R1 — Never return `isError: true` early in a session
An error response early causes **session-wide abandonment**: the agent stops trusting the tool and
reverts to grep/read for the rest of the session. If the graph can't answer, return a *successful*
empty/partial result with a `diagnostic`, not an error.

## R2 — Unindexed/empty → expose *zero* tools, not erroring tools
If the graph isn't built for the current repo, the MCP server should advertise no tools rather than
tools that fail. A tool that exists must work.

## R3 — Partial coverage is WORSE than none
A graph that covers some languages/files but silently omits others misleads the agent into thinking
it has the whole picture. Always surface coverage as a `diagnostic` ("graph covers TS/Python; 3
Go files not indexed") so the agent knows when to fall back. Track coverage as a first-class signal.
Unresolved references are defined once in `docs/ENGINE-CONTRACT.md` §2.1; the coverage line counts
them per site.

## R4 — Cap tool output (~25K chars)
Beyond ~25K characters the agent externalizes/ignores the output. Rank (PageRank, W4.1) and budget
(elided stubs — signatures + docstrings, not bodies, W4.2). Prefer a tight, ranked answer over a
complete dump.

## R5 — Always report staleness
Embed `commits_behind` (git rev-list since DB mtime) in every response's `diagnostics`
(prior art pattern). A silently-stale graph is a correctness hazard.

## R6 — Loud fallback marker
When the agent must read files because the graph couldn't answer, emit a visible `GRAPH-FALLBACK:`
marker and count it. Fallback rate is the coverage-gap KPI (prior art Rule 31, W7.3).

Sibling markers for the edit-plane LSP tools (ADR-009, same channel, same loudness rule):
`LSP-FALLBACK:` when a missing language server makes the tool serve labeled graph results
instead of live LSP answers, and `LSP-TRUNCATED: <served>/<total>` when a References answer
was capped (R4). Both ride `RetrievalResult.diagnostics` (MCP) / an additive JSON field
(CLI) — never extra stdout lines, never stderr.

## R7 — Confidence is visible, low-confidence is labeled
Heuristic/low-confidence edges must be marked so the agent weighs them appropriately; never present
a 0.5-confidence synthesized edge as if it were a 1.0 SCIP fact.

---
*Enforcement: `RetrievalResult.diagnostics` is the channel for R3/R5/R6/R7; R1/R2/R4 are server +
tool-impl responsibilities verified by behavior tests in W4.3.*
