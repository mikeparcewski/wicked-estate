# Spec: path query — the route from A to B

- **Status:** Shipped
- **Owner:** eugenelim
- **Plan:** [`plan.md`](plan.md)
- **Constrained by:** `docs/ENGINE-CONTRACT.md` (edge direction, bounded traversal), `docs/agent-behavior-rules.md` (R3, R4, R5, R7), `docs/adr/ADR-003-storage-backends.md` (no backend-specific query)
- **Brief:** none
- **Source:** GitHub issue #192 — "Feature: a path query — return the route from A to B, not just that B is reachable", filed by an outside integrator against v0.16.7. The scope this spec accepts is that issue's Ask paragraph: a `wicked-estate path <from> <to> [--max-depth N] [--json]` CLI command and an MCP `Path` tool alongside `TraverseGraph`, each returning the ordered hops with per-edge confidence.
- **Discovery:** none
- **Contract:** MCP tool `Path` (estate domain); CLI command `wicked-estate path`
- **Shape:** service

## Outcome

An agent asking "how does this HTTP handler reach the database write?" gets the
ordered hops — the intermediate functions it can then read — instead of a
reachability set it must search itself. Success is one CLI command and one MCP
tool that each return the route with every hop's confidence and provenance
attached, so the agent can tell a precise hop from a heuristic one.

## What Changes

- `Subgraph::shortest_path` — a pure, deterministic BFS over an already-returned
  subgraph, in `crates/wicked-estate-core/src/query.rs`
- `path_between` + `PathResult` — name-to-name path lookup over one bounded
  `traverse` call, in **`crates/wicked-estate-core/src/path.rs`**. It lives in
  core because both consumers must reach it and `wicked-estate-retrieve` does
  not depend on `wicked-estate` (the edge runs the other way); it needs only
  core types. *Amended after spec approval, owner-authorized 2026-09-30: the
  originally approved location, `crates/wicked-estate/src/lib.rs`, is
  unreachable from the tool's crate without a dependency cycle.*
- `wicked-estate path <from> <to>` — new CLI command, in `crates/wicked-estate/src/main.rs`
- `Path` — new estate `RetrievalTool`, in `crates/wicked-estate-retrieve/src/lib.rs`
- MCP registration, plus the tool-count assertions, messages and test names
  inside `crates/wicked-estate-mcp/`'s three test files — which move because a
  twelfth estate tool makes them fail, not as documentation work
- A rustdoc correction on `Subgraph::truncated`, which wrongly claims the depth
  cap sets it, in `crates/wicked-estate-core/src/query.rs`
- **Not here:** the tool-count and roster refresh across `README.md`,
  `FEATURES.md`, `CLAUDE.md`, `docs/getting-started.md`,
  `docs/mcp-integration.md`, `crates/wicked-estate-mcp/README.md` and `site/`.
  The owner split it into its own change; this spec touches no tool count and
  no roster **in any document**. The in-crate counts and the `ESTATE_TOOLS`
  roster inside the three MCP test files are the named exception, and they move
  because the build requires it.

## Durable Outputs

| Semantic role | Applicability | Destination | Owner | Expected evidence | Closeout condition |
| --- | --- | --- | --- | --- | --- |
| User promise (CLI) | New user-invoked command | CLI `--help`; the CLI usage sections of `README.md` and `docs/getting-started.md` (neither carries a tool count) | work-loop | `path` appears in help output and both usage sections | Command documented with its flags and bound behaviour |
| User promise (MCP) | New agent-facing tool | The tool's own `description` and `inputSchema`, surfaced through `tools/list` | work-loop | `Path` present in `tools/list` with a schema requiring `from` and `to` | An agent can discover and call it without any document changing |
| Current product truth | Three MCP test files assert the tool count and go red without the bump; one core rustdoc states a falsehood this change depends on | `crates/wicked-estate-mcp/tests/` + `src/lib.rs`; `crates/wicked-estate-core/src/query.rs` | work-loop | `cargo test -p wicked-estate-mcp` green at the new values; the corrected `truncated` rustdoc | No tool count or roster in any document was touched; the refresh is carried by the named follow-up |
| Release history | User-visible feature addition | `CHANGELOG.md` | work-loop | Unreleased entry naming both surfaces | Entry present under the next version |
| Interface compatibility | Purely additive; no existing tool, trait, or schema changes | n/a | work-loop | `GraphRead` diff is empty | No `traits.rs` change and no change under `crates/wicked-estate-store/src/` (the seam's tests do add a file under that crate's `tests/`) |

## Agent Rules

### Always do

- Reach the store through exactly one bounded `traverse` call per start
  candidate; the path search itself is in-memory over that result.
- Carry `{confidence, provenance, resolved_by}` on every hop that is emitted.
- Admit a hop only when both of its endpoints are nodes of the returned
  subgraph. Backends differ in which frontier-exterior edges they induce, so an
  unfiltered walk over `Subgraph::edges` answers differently per backend.
- Report the bound honestly: when no path is found, say whether the search hit
  its depth frontier or its node budget, and never let a bounded absence read as
  proof that no route exists. The depth bound is derived by the path layer from
  the traversal's own `depths` map — the stores set `Subgraph::truncated` from
  the node cap alone.

### Ask first

- Adding a `path`-shaped method to the `GraphRead` trait (that reaches four
  store backends and the conformance kit).
- Any change to `TraverseGraph`'s request or response shape.
- Raising the estate tool count beyond the one tool this spec adds.

### Never do

- Issue one store query per visited node (the N-statements-per-node ban).
- Emit an unbounded search: both `max_depth` and `max_nodes` are always set.
- Present a path found only through low-confidence edges as a fact without the
  R7 confidence marker.
- Delete, weaken, or `#[ignore]` an existing tool-count assertion to make the
  new tool fit.

## Testing Strategy

- **`Subgraph::shortest_path` (core BFS, including tie-break determinism and
  the no-path case): TDD.** A compressible invariant over a hand-built subgraph;
  unit tests, no store needed.
- **`path_between` (name resolution, bound plumbing, bound-flag honesty):
  TDD**, exercised as an integration test against real `MemStore` and
  `SqliteStore` instances. The seam is in `wicked-estate-core`, which cannot
  depend on the store crate, so its tests live in
  `crates/wicked-estate-store/tests/` — the behaviour only proves out across
  the seam-to-store boundary anyway.
- **MCP `Path` tool (request parsing, response shape, diagnostics): TDD**, unit
  tests against an in-memory store in `wicked-estate-retrieve`.
- **MCP registration and tool counts: goal-based check** — the existing
  `tools/list` count tests in `sc009.rs`, `conformance_schemas.rs`, and
  `wicked-estate-mcp/src/lib.rs` turn green at the new numbers.
- **CLI `wicked-estate path`: Visual / manual QA**, exercised by an end-to-end
  test in `crates/wicked-estate/tests/path_cli.rs` — a spawn harness following
  the `repo_flag_cli.rs` precedent, because exit codes, usage lines and `--help`
  are properties of the binary an in-process test cannot reach. It indexes a
  fixture repo with a known call chain and asserts the printed hops; plus a
  recorded run of the real built binary on that fixture. The renderers
  themselves are asserted in-process against a sink in `main.rs`, since two
  branches cannot be provoked through a spawned binary at all.

## Acceptance Criteria

- [x] Given a graph holding a directed chain of `Calls` edges from symbol `A` to
      symbol `B`, `wicked-estate path A B` prints the hops in order from `A` to
      `B`, one line per hop, each line naming the hop's source symbol, target
      symbol, edge kind, and confidence.
- [x] `wicked-estate path A B --json` prints exactly one JSON document to stdout
      whose `hops` array holds one object per hop in order from `A` to `B`, each
      object carrying `source`, `target`, `kind`, `confidence`, `provenance`,
      and `resolved_by`. `source` and `target` are **denormalized objects with
      the same six fields the MCP tool emits** — `symbol`, `name`, `kind`,
      `file`, `line`, `line_1based` — never bare id strings, so a script reading
      `--json` learns each hop's file and line without a second command.
      *Owner-decided 2026-09-30; the criterion previously left the form open.*
- [x] Rendering the CLI `--json` endpoints issues no store lookup: they are
      built from `PathResult::endpoints`, which the traversal already produced.
- [x] Given no path from `A` to `B` within the bound, both output modes report
      that no path was found; the JSON document's `hops` is `[]` and its `found`
      field is `false`.
- [x] Given a chain from `A` to `B` longer than `--max-depth`, the JSON
      document's `depth_bounded` field is `true` and the text output states that
      the walk reached its depth frontier, so the absence of a route is bounded
      rather than proven. `depth_bounded` is `true` exactly when some candidate
      traversal's `depths` map holds at least one node at distance `max_depth`.
- [x] Given a graph whose whole reachable set is shallower than `--max-depth`,
      `depth_bounded` is `false`, and given a chain of exactly `--max-depth`
      hops that reaches `B`, `depth_bounded` is `true` even though the route was
      found.
- [x] Given a traversal whose node budget was exhausted, the JSON document's
      `node_bounded` field is `true`, carrying that traversal's `truncated`
      flag; across several candidate traversals it is the disjunction of theirs.
- [x] Given a multi-match `from` where exactly one candidate's traversal reached
      its depth frontier and no route was found, `depth_bounded` is `true` — not
      the winning or last candidate's value.

> **How the bound flags combine across candidates.** A multi-match name yields
> one bounded traversal per `from` candidate (D2), so there are N `depths` maps
> and N `truncated` flags. Both flags are the **conservative disjunction**:
> `depth_bounded` is `true` if *any* candidate traversal reached its depth
> frontier, and `node_bounded` is `true` if *any* candidate traversal was
> node-capped. Reporting only the winning or last candidate's flags would let a
> query whose other candidate walk was cut off return
> `found: false, depth_bounded: false, node_bounded: false` — which the note
> below licenses a reader to treat as proof that no route exists, the exact R3
> failure the Agent Rules forbid. A found-conditioned rule would also be sound,
> since the licensed inference only bites on absence, but it would contradict
> the unconditional over-approximation stated below; the disjunction is the
> smallest rule consistent with both.
>
> **What the two bound flags mean.** `depth_bounded` says *the walk reached its
> depth frontier*, not *a route was cut off*: it is a deliberate conservative
> over-approximation, true whenever any node sits at exactly `max_depth`, even
> when the frontier had nothing left to expand and even when a route was found.
> A consumer may conclude from `depth_bounded: false` together with
> `node_bounded: false` and `found: false` that no route exists in the graph; it
> may not conclude from `depth_bounded: true` that one does exist further out.
> `node_bounded` is **backend-approximate**, and by more than a fencepost. The
> backends budget on *different populations*: `MemStore` counts `sub_nodes`,
> which grows only for ids that have a stored `Node`
> (`crates/wicked-estate-store/src/lib.rs:751`, gated push at `:757`), while
> `SqliteStore` counts `depths`, which includes every reached interned symbol —
> edge-only symbols with no node row included — and excludes the start
> (`crates/wicked-estate-store/src/sqlite.rs:2743`, `cte_reach` at
> `:1741-1747`); `PostgresStore` uses a `max_nodes + 1` fencepost
> (`crates/wicked-estate-store/src/postgres.rs:1624`).
>
> **This bounds the equality claim.** Once the node budget actually binds, the
> backends retain *different node sets* — SQLite's `ORDER BY depth … LIMIT`
> keeps a globally shallowest cut, MemStore keeps a BFS-discovery prefix — and
> under D1a, which admits a hop only when both endpoints are subgraph nodes, a
> hop admissible on one backend can be inadmissible on the other. So the hop
> sequence, `found`, and `depth_bounded` are equal across backends **only while
> the node budget does not bind**. The equality criterion below is stated for
> that envelope and does not exercise the cap. Outside it, no equality is
> claimed. Reconciling the stores' budgeting is out of scope here and is
> recorded under Follow-ons.

- [x] `wicked-estate path A --json` exits non-zero with a usage line naming both
      operands: `--json` is consumed as a flag, leaving one operand, and a
      one-operand invocation is a usage error.
- [x] `wicked-estate path` with no operands, and `wicked-estate path A`, each
      exit non-zero with that same usage line.
- [x] An operand beginning with `--` exits non-zero with a usage line rather
      than being resolved as a symbol name, so `wicked-estate path --max-depth 4`
      never treats `--max-depth` as the `from` operand.
- [x] `wicked-estate path` appears in `wicked-estate --help` output with its
      `--max-depth` and `--json` flags.
- [x] The MCP tool `Path` appears in `tools/list` and `input_schema("Path")`
      returns a schema requiring `from` and `to`.
- [x] `from` and `to` each accept either an exact symbol name or a `SymbolId`
      string, resolved in that order: a value matching a symbol name resolves by
      name, and a value matching no symbol name but naming a live node resolves
      as that node's id. Both the CLI and the `Path` tool accept both forms.
- [x] When `from`, `to`, or both resolve to several symbols, the returned route
      is the shortest over every `(from-candidate, to-candidate)` pair; ties
      between equally short routes resolve to the same pair on every run for a
      given graph.
- [x] A `from` or `to` value matching neither a symbol name nor a live node id
      yields `found: false` plus a statement of which side was unresolved; it
      never yields a bare empty result. On the CLI that is a top-level
      `"unresolved": "from" | "to"` key in the `--json` document (and a text
      line in text mode); on MCP it is a diagnostic naming the side. An
      unresolvable input and a proven absence must not produce identical
      output on either surface — that is the R3 failure this criterion exists
      to prevent.
- [x] A resolvable pair with no route between them yields `found: false` with
      the CLI's `"unresolved"` key `null` and no unresolved diagnostic on MCP,
      so a reader can tell the two cases apart.
- [x] An MCP `tools/call` for `Path` over a graph with a chain from `A` to `B`
      returns `content.hops` in order from `A` to `B`, each hop carrying
      denormalized `source` and `target` objects (`symbol`, `name`, `kind`,
      `file`, `line`, `line_1based` — the six fields `endpoint_json` emits) plus
      `kind`, `confidence`, `provenance`, and `resolved_by`.
- [x] Every `Path` response — found, not found, or bounded — carries the same
      staleness diagnostic every other estate tool emits (R5), so a `Path` reply
      never reads as fresher than the index behind it.
- [x] When any hop on a returned path has confidence below 0.5, the `Path`
      response diagnostics include an `R7-CONFIDENCE` line naming the count of
      such hops.
- [x] When `Path` finds no route and either `depth_bounded` or `node_bounded` is
      `true`, its diagnostics include a line stating that the search was bounded
      and the absence of a path is not proof of no path; when both are `false`
      that line is absent.
- [x] `tools/list` with all four stores open returns exactly 31 tools (12 estate
      + 7 memory + 8 knowledge + 4 proposal). The count is 31 because
      `SemanticSearch` is not in the list at these sites: `sc009.rs`'s
      all-four-stores assertion and both unified count tests wire semantic as
      `None` and fail the dim-guard, and none of the three is feature-gated — so
      this criterion governs a `fastembed` or `model2vec` build too, and none of
      those assertions may be gated or weakened to accommodate one. The single
      exception is
      `conf_tool_count_11_estate_7_memory_7_knowledge_4_proposal`, which already
      carries `#[cfg(not(any(feature = "fastembed", feature = "model2vec")))]`;
      that gate is pre-existing and stays as it is.
- [x] `Path` survives `--readonly` mode: it appears in the read-only
      `tools/list` and a `tools/call` for it is not refused.
- [x] The path search issues exactly one `traverse` call per resolved start
      symbol and no per-node store query; a test counts store traversal calls
      through a recording wrapper and asserts one call for a single-match name.
- [x] For one graph built identically in a `MemStore` and a `SqliteStore`,
      including a chain that extends past `max_depth`, the path query returns
      the identical hop sequence, the identical `found`, and the identical
      `depth_bounded` from both stores, **on a graph whose traversal does not
      exhaust `max_nodes`** — outside that envelope the backends retain
      different node sets and no equality is claimed, per the bound-flag note
      above. The graph must include a name matching
      several symbols with two equally short routes, because that is the only
      shape on which the stores' own candidate ordering diverges — without it,
      removing the candidate sort leaves every test green. `node_bounded` is
      excluded from this equality by the backend-approximation note above, not
      by oversight.
- [x] Every hop the path query returns has both endpoints present in the
      traversal's own node set, so every endpoint can be denormalized without a
      further store lookup.
- [x] With no `--max-depth` flag, `wicked-estate path` searches to depth 12 —
      the same default `blast-radius` uses — observable as a 12-hop chain being
      found and a 13-hop chain not being found.
- [x] `--max-depth N` for any `N` from 1 through 16 inclusive runs the search at
      depth `N`, observable as a route of `N` hops being found on a chain of
      exactly `N` hops. For `N` from 2 through 16 the same chain is not found at
      `--max-depth N - 1`. The pair is not observable at `N = 1`, because
      `--max-depth 0` is a usage error rather than a not-found result; at `N = 1`
      the observation is the found half alone.
- [x] `--max-depth` and `--json` are consumed as flags wherever they appear in
      the argument list, before either operand is read: `wicked-estate path
      --max-depth 4 A B` and `wicked-estate path A B --max-depth 4` both resolve
      `from = A`, `to = B`, and depth 4, and neither leaves a stray operand.
- [x] `--max-depth 17` (the smallest input above the ceiling) runs at depth 16
      rather than erroring, observable as a 16-hop chain still being found.
- [x] `--max-depth 0` and `--max-depth abc` each exit non-zero with a message
      naming the accepted range `1..=16`.

> **Limit ordering for the four depth-limit criteria above** — the default-12
> criterion, the `--max-depth N` range criterion, the `--max-depth 17` clamp
> criterion, and the `--max-depth 0` / non-integer rejection criterion. (The
> fifth, flag-consumption, states no numeric limit and needs no ordering.) `max_depth` binds first on
> the CLI: the CLI fixes `max_nodes` at 5 000 while `max_depth` never exceeds
> 16, so a 16-hop walk reaches the depth frontier before the node budget on any
> graph whose frontier stays under 5 000 nodes. Both caps are enforced inside
> `GraphRead::traverse`, but only `max_nodes` sets `Subgraph::truncated` — which
> is why depth exhaustion is reported through the separately derived
> `depth_bounded` field. On MCP the `max_nodes` request field (default 1 000,
> max 5 000) may bind first instead, and each bound is reported through its own
> response field.

- [x] A `Path` response for a 16-hop chain whose symbol ids, names, and file
      paths are each 200 characters stays under 25 000 characters (the R4
      budget), measured over **what the agent receives**: the serialized JSON
      content block plus the diagnostics block the MCP layer appends alongside
      it. Measured worst case is ~23.9K — ~23.5K of content plus under ~400
      characters of diagnostics — so the `max_depth ≤ 16` clamp is the
      enforcement mechanism and no truncation of `Path` is required.
- [x] This spec changes **no tool count and no estate-tool roster in any
      document** — that is, anywhere outside the three files named in the two
      criteria below (`crates/wicked-estate-mcp/src/lib.rs`,
      `crates/wicked-estate-mcp/tests/sc009.rs`,
      `crates/wicked-estate-mcp/tests/conformance_schemas.rs`), where the counts
      and the `ESTATE_TOOLS` roster are build-necessary and must move. A diff
      hunk that edits a tool count, or adds `Path` to a roster enumeration, in
      any file outside those three fails this criterion. The documentation
      refresh belongs to the separately filed follow-up named under Follow-ons.
      The check is a reviewer reading the diff: five review rounds established
      that no regex closes the set of ways prose states a number, so this spec
      makes no claim one does.
      *Superseded at merge (maintainers' review of #221): the documentation
      refresh was folded into this PR, so the tool counts and estate rosters in
      `README.md`, `CLAUDE.md`, `docs/`, the MCP crate README and the site now
      include `Path`.*
- [x] `cargo test -p wicked-estate-mcp` passes with every tool-count assertion
      reading its new value (estate 12, total 30, read-only surface 20,
      unconditional floor 12, floor-plus-semantic 13), and no assertion
      weakened, `#[ignore]`d, or deleted to accommodate the new tool. These
      live in test files, not documentation: a twelfth estate tool makes them
      fail, so they move here or the workspace is red.
- [x] No count-bearing prose, assertion message, or test name in
      `crates/wicked-estate-mcp/src/lib.rs`,
      `crates/wicked-estate-mcp/tests/sc009.rs`, or
      `crates/wicked-estate-mcp/tests/conformance_schemas.rs` still states a
      pre-change count. The five test-name renames are
      `tools_list_returns_eleven_unconditional_tools`,
      `tools_list_returns_twelve_with_semantic_available`,
      `unified_tools_list_without_domains_returns_11_tools`,
      `unified_tools_list_with_domains_returns_29_tools`, and
      `conf_tool_count_11_estate_7_memory_7_knowledge_4_proposal`. Verified by
      reading those three files.
- [x] `README.md` and `docs/getting-started.md` each show the `wicked-estate
      path` command in their CLI usage section — the sections that carry no
      tool count — and `CHANGELOG.md` carries an unreleased entry naming the
      CLI command and the MCP tool.
- [x] `crates/wicked-estate-core/src/query.rs`'s rustdoc for
      `Subgraph::truncated` no longer claims `max_depth` sets it, because no
      store does: every backend sets it from the `max_nodes` cap alone.
      Correcting it is in scope here because this change is the first to depend
      on that distinction.
      *Superseded at merge: #222 (wicked-estate#190) landed first and made
      `truncated` exactly `node_cap_reached || depth_horizon_reached`, so its
      rustdoc follows #222 and `path_between` reads `node_cap_reached` for the
      node bound.*

## Follow-ons

The first entry is **owner-requested and to be filed**; the three
`work-loop: not filed` entries below it are **deliberately unfiled**. Each of
those three is a real observation this change surfaced but does not require,
and the work-loop's DECIDE rule creates no durable follow-on for excluded work
unless the owner asks — they are recorded here so the next reader knows they
were seen and declined, not missed. No acceptance criterion depends on any
entry in this section.

- **owner-requested, to file:** the repository-wide tool-total refresh —
  `29 → 30` at `README.md:157`, `README.md:200`, `docs/getting-started.md:24`,
  `CLAUDE.md:15`, `CLAUDE.md:243`, the estate descriptor at `CLAUDE.md:237`,
  and the `site/` strings — plus `docs/mcp-integration.md`'s and
  `FEATURES.md`'s pre-existing stale totals (`24 tools across 3 domains`) and
  `docs/mcp-integration.md`'s stale `Memory (6 tools)` roster, and
  `crates/wicked-estate-mcp/README.md:21`'s "the ten always-on estate
  `RetrievalTool` instances" — already stale at eleven before this change. The owner moved
  this out of this spec's scope; it is to be filed through `work-intake` as its
  own change. By repository precedent the `site/` half lands in a `docs(site)`
  commit at release (the count moved to 29 in `b881d6e`, separately from
  `f6a0165`, the feature commit that moved `README.md`).
- work-loop: not filed — the backends budget `Subgraph::truncated` on different
  populations (`MemStore` on nodes with a stored `Node`, `SqliteStore` on every
  reached interned symbol, `PostgresStore` on a `max_nodes + 1` fencepost), so
  beyond `node_bounded` differing, the retained node sets differ once the cap
  binds. Pre-existing; this change reports the flag and bounds its equality
  claim accordingly, it does not create the divergence.
- work-loop: not filed — `--direction dependents` for `wicked-estate path`
  (walking the route backwards from a dependency to its dependent). Out of the
  accepted scope; file through `work-intake` only if an integrator asks.
- work-loop: not filed — k-shortest-paths / all-paths enumeration. Issue #192
  asks for *the* route; enumeration is a separate contract with its own output
  budget problem.

## Assumptions

- The reporter's use case is forward (`dependencies`) direction only: from a
  dependent toward its dependency. The path seam therefore exposes only that
  orientation and takes no direction parameter, so no unhandled direction
  variant is reachable. A reverse-direction path is a follow-on, not a gap in
  this contract.
- Tie-breaking between two equally short paths may be arbitrary as long as it is
  deterministic for a given graph; no consumer has asked for a ranked choice.
