# Plan: path query — the route from A to B

- **Spec:** [`spec.md`](spec.md)
- **Status:** Done
- **Repository anchors:** `docs/ENGINE-CONTRACT.md` (edge direction `source = dependent`,
  bounded-traversal rule) and `CLAUDE.md` (Universal Don'ts: bounded traversal only, no
  N-statements-per-node, confidence + provenance on every edge). Analogous implementations:
  the blast-radius triple — `wicked_estate::blast_radius_by_name`
  (`crates/wicked-estate/src/lib.rs:1426`), its CLI arm (`crates/wicked-estate/src/main.rs:1342`),
  and the `BlastRadius` retrieval tool alongside `TraverseGraph`
  (`crates/wicked-estate-retrieve/src/lib.rs:584`). Corresponding tests / construction path:
  `crates/wicked-estate-mcp/src/lib.rs` (`all_tools`, `input_schema`, the `tools/call` dispatch
  match, `response_cacheable`), `crates/wicked-estate-mcp/tests/sc009.rs`,
  `crates/wicked-estate-mcp/tests/conformance_schemas.rs`, `crates/wicked-estate/tests/e2e.rs`.
  Named uncertainty: whether a path query eventually belongs on `GraphRead` as a server-side
  query. This plan deliberately does not add one — see Design decision D1.

## Approach

The path is reconstructed from a single bounded traversal, not from a fresh search
over the store. `GraphRead::traverse` already returns every edge it walked, each
carrying `{confidence, provenance, resolved_by}`; a breadth-first search over that
returned `Subgraph` yields the ordered hops with no extra query and no backend
change. That is the whole trick, and it is why the diff touches neither the five
traits nor any of the four store backends.

Order of operations: the pure BFS lands in `wicked-estate-core` first, because it is
the shared seam both consumers use (§11 — fix once at the seam, not twice). The
sibling core module `crates/wicked-estate-core/src/path.rs` wraps it with name
resolution and bound plumbing — in core, not in `wicked-estate`, because
`wicked-estate-retrieve` cannot depend on that crate (D8); this change adds no
library function to `wicked-estate` at all.
The CLI command and the MCP `Path` tool then sit on top, each shaped like its
blast-radius sibling so an integrator reads one familiar envelope. The mechanical
tail is narrower than it first looks. Eleven estate tools become twelve, which
the three MCP test files assert and which therefore *must* move here — the
workspace is red otherwise. Every **documentation** count and roster is
deliberately **not** here: the owner split that into its own change after five
review rounds established that no regex can gate prose counts, and that a
scope fence drawn through those documents fails the same way. This spec
therefore touches no tool count and no roster **in any document**, which is why
it needs no fence. The in-crate counts and the `ESTATE_TOOLS` roster inside the
three MCP test files are the named exception above, and they move because the
build requires it.

*Superseded at merge (maintainers' review of #221): the documentation count and
roster refresh was folded into this change after all, so `README.md`,
`CLAUDE.md`, `docs/`, the MCP crate README and the site now count and list
`Path`. Every statement below that excludes documentation counts (the
durable-output map, T6 and Risks) describes the plan as written, not what
landed; see the matching note in `spec.md`.*

The riskiest part is not the algorithm — it is the count bump. Three separate test
files pin the tool roster, and two of them state the arithmetic in their failure
messages. Missing one leaves a red workspace; softening one to make the new tool
fit is the thing the spec's `Never do` forbids.

## Constraints

- `docs/ENGINE-CONTRACT.md` — edge direction is `source = dependent`,
  `target = dependency`. A forward path from A to B follows `source → target`.
- `docs/adr/ADR-003-storage-backends.md` — a backend drops in as one module plus
  one factory arm with zero caller changes. Adding a trait method would invert
  that: four backends plus the conformance kit would change for one feature.
- `docs/agent-behavior-rules.md` — R3 (partial coverage is worse than none),
  R4 (output under 25K chars), R5 (always report staleness), R7 (confidence
  visible).
- `CLAUDE.md` Universal Don'ts — bounded traversal only; never one statement per
  node; never an edge without confidence and provenance.

## Construction tests

**Integration tests:**
- `crates/wicked-estate/tests/path_cli.rs` — a spawn harness that indexes a fixture
  with a known call chain and runs the `path` command end-to-end, assert the ordered hops and the
  `--json` document shape.
- A store-call-counting test (T2) proving one `traverse` per resolved start symbol.

**Manual verification:**
- Build the release binary and run `wicked-estate path <from> <to>` and
  `--json` against the fixture T3 indexes — the `util <- service <- handler`
  chain from `crates/wicked-estate/tests/e2e.rs`, extended to the depths the
  bound criteria need; record stdout and exit code in the verification ledger.
- Drive the MCP server over stdio with a `tools/call` for `Path` and record the
  returned envelope.

## Durable-output map

| Durable output | Tasks | Implementation evidence | Closeout evidence |
| --- | --- | --- | --- |
| User promise (CLI) — `--help` plus the count-free CLI usage sections | T3, T6 | e2e test asserting help text lists `path`; recorded manual run | `path` documented with `--max-depth` and `--json` |
| User promise (MCP) — the tool's own description and schema | T4, T5 | `tools/list` test naming `Path`; schema test | An agent discovers and calls it without any document changing |
| Current product truth — three MCP test files; one core rustdoc | T1, T5 | `cargo test -p wicked-estate-mcp` green at the new values; an enumerated read of the three files for stale prose; the corrected `truncated` rustdoc | No document's tool count or roster was touched |
| Release history — `CHANGELOG.md` | T6 | Unreleased entry naming both surfaces | Entry present |
| Interface compatibility — no trait or store change | T1–T5 (verified once the last of them lands; T2 adds the cited test file, so the check is meaningless against T1's diff alone) | `git diff --stat` shows no change under `crates/wicked-estate-store/src/` and no `traits.rs` change (the seam's tests do add `crates/wicked-estate-store/tests/path_backends.rs` — core cannot depend on the store crate, D8) | Diff confirms additive-only |

## Design (LLD)

### Design decisions

**D1 — the path search is a pure function over a returned `Subgraph`, not a new
`GraphRead` method.** Traces to the AC forbidding any `GraphRead`/`GraphWrite`
addition. `traverse` already returns the walked edges with full provenance, so a
BFS over that result answers the question in one round-trip. A trait method would
reach `MemStore`, `SqliteStore`, `PostgresStore`, `SurrealStore` and the
conformance kit — five implementations and a new conformance contract — to
recompute what the store has already handed back. If a future caller needs a path
over a neighbourhood too large to return, that is the evidence that reopens D1.

**D1a — the path search admits only edges whose endpoints are both nodes of the
returned subgraph.** Traces to the cross-backend-equality and
endpoint-denormalization ACs. The backends induce different edge sets at the
frontier: `SqliteStore::traverse` collects every neighbour of every anchor
(`crates/wicked-estate-store/src/sqlite.rs:2755-2766`), so its `edges` include
hops leaving the `max_depth` frontier to symbols that appear in neither `depths`
nor `nodes`; `MemStore::traverse` `continue`s at the depth frontier before
pushing any edge (`crates/wicked-estate-store/src/lib.rs:724-727`) — but
MemStore emits endpoint-less edges by another route: `sub_edges.push` at `:747`
runs *before* the node-budget check at `:751` and before the
`self.nodes.get(&next)` lookup at `:757`, so at the node cap, or for any id with
no stored node, it too returns an edge whose endpoint is absent from `nodes`.
The admission filter is not a SQLite workaround: both backends need it, for
different reasons. An unfiltered walk over
`Subgraph::edges` therefore finds a `max_depth + 1`-hop route on SQLite that
MemStore reports absent, and that extra hop has no `Node` to denormalize.
Restricting the adjacency to the subgraph's own node set makes the hop
sequence, `found`, and `depth_bounded` identical on every backend, and makes
every emitted endpoint denormalizable from data already in hand. `node_bounded`
is the one field this does not equalize, and the equality itself holds only
while the node budget does not bind: the backends budget on different
populations and retain different node sets at the cap, so beyond that envelope
a hop admissible on one can be inadmissible on the other. A pre-existing
divergence this change reports and bounds rather than creates (spec Follow-ons
and the bound-flag note).

**D1b — depth exhaustion is derived by the path layer, not read off
`Subgraph::truncated`.** Traces to the `depth_bounded` ACs. Both stores set
`truncated` from the `max_nodes` cap alone (`sqlite.rs:2743`,
`crates/wicked-estate-store/src/lib.rs:751-752`); the depth cap never sets it.
Reporting a depth-exhausted no-path result as `truncated: false` would present a
bounded absence as proof of absence — the R3 failure the spec's Agent Rules
forbid. `depth_bounded` is computed from the traversal's own `depths` map: true
when any reached node sits at distance `max_depth`. That derivation is identical
on every backend because `depths` is what the conformance kit pins.

**D2 — one `traverse` per resolved start symbol; never per node.** Traces to the
one-call and multi-match ACs. `search()` may return several symbols for a name,
on either side. Each `from` candidate gets one bounded traversal; within each
resulting subgraph the BFS targets the whole `to` candidate set and stops at
whichever is reached first, and the shortest route over all
`(from-candidate, to-candidate)` pairs wins.

**`GraphRead::traverse_multi` was considered and is unusable here.** The trait
already exposes a multi-seed traversal (`crates/wicked-estate-core/src/traits.rs:114`)
whose doc makes a set-seeded override mandatory for `SqliteStore`, so N
candidates could in principle cost one query rather than N. It cannot serve
this seam: the fold merges `depths` by **MIN across seeds** (`traits.rs:134-139`)
and removes every seed from `depths` (`:142-145`), so a node sitting at
`max_depth` from candidate A and at depth 2 from candidate B surfaces as 2 — and
A's frontier touch, which is exactly what D1b defines `depth_bounded` from, is
unrecoverable. `truncated` does merge by OR (`:140`), so `traverse_multi` would
serve `node_bounded` alone, but using it for that while still running
per-candidate walks for `depth_bounded` pays the traversal cost twice and breaks
the one-call-per-candidate criterion. A union subgraph would also admit nodes
beyond `max_depth` from the actual `from` candidate, which D1a would then let
into a hop. The N-call cost stays a recorded risk rather than a defect.

Ties need an ordering the *stores do not supply*: `MemStore::find_symbols` sorts
by `SymbolId` string (`crates/wicked-estate-store/src/lib.rs:690`) while
`SqliteStore`'s exact-name arm is `ORDER BY symbol`, and `nodes.symbol` is the
autoincrement integer sid (`crates/wicked-estate-store/src/schema.sql:29,39`) —
insertion order. Taking `search()`'s order would therefore pick a different
winning pair per backend. `path_between` sorts both candidate lists by
`SymbolId` string before searching, so **which `from` candidate's traversal is
searched, and in what order, is a property of the data** rather than of the
store's `find_symbols` ordering. An equal-distance tie *among `to` candidates
inside one traversal* is decided by a different rule: BFS visitation order over
the adjacency D4 sorts. Both are data properties, so multi-match ties are
**inside** the cross-backend equality claim — but the criterion pinning
deterministic tie resolution rests on **D4** for the target side and on this
sort for the source side. Naming only this sort, as an earlier draft did, cited
a mechanism that does not decide the target-side tie. D4 is a separate, narrower rule: it orders the
subgraph adjacency, not the candidate lists. A per-node `neighbors()`
walk would be the N-statements-per-node anti-pattern.

**D3 — `Path` is a peer of `TraverseGraph`, not a mode of it.** Traces to the
`tools/list` and schema ACs. `TraverseGraph`'s request and response shapes are a
published contract; adding a `to` field that silently changes the response shape
would break the envelope every existing consumer parses.

**D4 — determinism by explicit tie-break.** Traces to the ordered-hops ACs. BFS
over a `HashMap`-backed adjacency would return different equally-short paths
across runs. The adjacency is built from the subgraph's `edges` in a stable sorted
order — by `(target id, kind, source id)` for a dependencies walk — so the same
graph always yields the same path.

**D5 — one input contract for both surfaces: name first, then symbol id.**
Traces to the input-form ACs. `search()` is exact-name-only
(`crates/wicked-estate/src/lib.rs:1412-1419`), but the ids an agent already holds
come from `SearchEntity` and `TraverseGraph` responses, so a name-only contract
would reject the most common input an agent has. Resolution tries the name first
(preserving `blast-radius`'s matching semantics) and falls back to
`get_node(SymbolId)`.

**D6 — the path seam takes no direction parameter.** Traces to the spec's
Assumptions. `Direction` has three variants including `Both`, for which a path
search has no defined meaning; taking the enum would put an unhandled variant on
a public `wicked-estate-core` API guarded only by a doc-comment precondition.
The seam instead follows `source → target` — the dependency orientation, the
only one this change consumes. A reverse-direction path is a Follow-on that
would extend the seam deliberately rather than inherit an undefined case.

**D8 — the shared seam lives in `wicked-estate-core`, because the tool's crate
cannot reach `wicked-estate`.** `crates/wicked-estate-retrieve/Cargo.toml`
depends on core, rank and store — not on `wicked-estate`, which depends on
*it* (`crates/wicked-estate/Cargo.toml:24`). Putting `path_between` in
`crates/wicked-estate/src/lib.rs` would leave `Path` unable to call it without
a Cargo cycle, and the only cycle-free alternative is re-implementing
resolution and traversal inside the tool — the two-copies drift D7 rejects.
`path_between` and `PathResult` therefore live in
`crates/wicked-estate-core/src/path.rs`; they need only `GraphRead`,
`SymbolQuery`, `TraversalSpec`, `Subgraph`, `Edge` and `Node`, all already in
core, so the spine gains no dependency. Owner authorized this amendment to the
approved spec on 2026-09-30.

**D9 — the two surfaces render endpoints from the same `PathResult`, not the
same function.** `edge_json` and `endpoint_json` are private to
`wicked-estate-retrieve` (`crates/wicked-estate-retrieve/src/lib.rs:85`,
`:116`), so the CLI cannot call them, and promoting them would push MCP
response shaping into a crate the CLI depends on for unrelated reasons. `Path`
keeps using `edge_json` — the seam that matters for MCP consistency with
`TraverseGraph` — and the CLI renders the same six endpoint fields from
`PathResult::endpoints` in `main.rs`. The shared thing is the *data*
(`PathResult`), which both surfaces get from one seam; the duplication is a
small renderer, and the two envelopes already differ.

**D7 — the caller owns both budgets; the seam takes them, never fixes them.**
Traces to the CLI `max_nodes` value and the MCP `max_nodes` field. A seam that
hardcoded 5 000 would make the MCP default of 1 000 and its clamp unreachable
through the seam, forcing the tool to duplicate resolution and traversal — the
two-copies drift §11 names. `path_between` therefore takes `max_nodes`
alongside `max_depth`, the CLI passes its fixed 5 000, and the tool passes its
clamped request field.

### Interfaces & contracts

`crates/wicked-estate-core/src/query.rs`:

```rust
impl Subgraph {
    /// The shortest hop sequence from `from` to `to` following `source → target`
    /// (the dependency orientation), or `None` when this subgraph holds no such
    /// route. Only edges whose endpoints are both nodes of this subgraph are
    /// walked, so the answer does not depend on which frontier-exterior edges a
    /// backend chose to induce.
    /// `targets` is the whole resolved candidate set; the search stops at
    /// whichever candidate is reached first — what D2 requires, and what a
    /// single-target signature cannot express. A one-element slice is the
    /// ordinary case.
    pub fn shortest_path(&self, from: &SymbolId, targets: &[SymbolId]) -> Option<Vec<Edge>>;
}
```

`crates/wicked-estate-core/src/path.rs` (new module — D8):

```rust
/// The route from `from` to `to` following dependency edges, within `max_depth`
/// hops and `max_nodes` visited nodes. Each endpoint is an exact symbol name
/// or, failing that, a `SymbolId`. The caller owns both budgets: the CLI passes
/// 5 000 nodes, the `Path` tool passes its clamped request field (D7).
pub fn path_between(
    store: &dyn GraphRead,
    from: &str,
    to: &str,
    max_depth: u32,
    max_nodes: usize,
) -> Result<PathResult>;

pub struct PathResult {
    pub hops: Vec<Edge>,
    /// Every node that appears as a hop endpoint, from the traversal that found
    /// the route. The MCP tool seeds `endpoint_json`'s cache from this, exactly
    /// as `TraverseGraph` seeds it from `subgraph.nodes`
    /// (`crates/wicked-estate-retrieve/src/lib.rs:722-726`). Without it the
    /// denormalizing layer falls through to `store.get_node` per endpoint — up
    /// to 17 queries on a 16-hop path — which the "no per-node store query"
    /// criterion forbids. D1a already guarantees both endpoints of every
    /// admitted hop are subgraph nodes, so this costs no extra lookup to build.
    pub endpoints: Vec<Node>,
    /// A route was found. `false` with both bounds clear means a proven absence;
    /// `false` with either bound set means the absence is bounded, not proven.
    pub found: bool,
    /// Some candidate traversal reached its depth frontier — a node sits at
    /// `max_depth` in its `depths` map. Derived here, because no store sets
    /// `Subgraph::truncated` from the depth cap (D1b). With a multi-match
    /// `from` this is the **disjunction** over every candidate traversal, never
    /// the winning candidate's value alone: reporting one walk's flag for a
    /// search that ran several would let a bounded absence read as proven (R3).
    pub depth_bounded: bool,
    /// The store's node budget capped a walk (`Subgraph::truncated`). With a
    /// multi-match `from` this is the **disjunction** over every candidate
    /// traversal, for the same R3 reason as `depth_bounded`.
    ///
    /// **Backend-approximate, and by more than a fencepost.** The backends
    /// budget on different populations: `MemStore` counts nodes that have a
    /// stored `Node`, `SqliteStore` counts every reached interned symbol
    /// (edge-only ones included), and `PostgresStore` uses a `max_nodes + 1`
    /// fencepost. Once the budget binds they also retain different node *sets*
    /// — a globally shallowest cut versus a BFS-discovery prefix — so results
    /// are comparable across backends only while it does not bind. See the
    /// spec's bound-flag note for the equality envelope; this field is excluded
    /// from the cross-backend equality criterion for that reason.
    pub node_bounded: bool,
    /// Which endpoint failed to resolve, if either did.
    pub unresolved: Option<String>,
}
```

MCP `Path` request: `{ "from": "<name-or-id>", "to": "<name-or-id>",
"depth": 8, "max_nodes": 1000 }` — `from` and `to` required; `depth` default 8,
max 16; `max_nodes` default 1 000, max 5 000.

MCP `Path` response `content`: `{ "hops": [ { "source": {symbol,name,kind,file,
line,line_1based}, "target": {…}, "kind": "calls", "confidence": 1.0,
"provenance": "parsed", "resolved_by": "scip-rust" }, … ], "found": true,
"depth_bounded": false, "node_bounded": false }`. Endpoints are denormalized exactly as `TraverseGraph`
denormalizes them, through the same `edge_json` helper.

CLI: `wicked-estate path <from> <to> [--max-depth N] [--json] [--db …]`.

**Argument contract.** The binary's shared parser pushes every token it does not
recognise into `positional` (`crates/wicked-estate/src/main.rs:1002`), so
`--max-depth` and `--json` arrive there as ordinary strings and the
`blast-radius` arm's "read `--json` out of `positional`" shape is not enough for
a command with two operands. The `path` arm therefore walks `positional` once
and classifies every token before reading either operand:

- `--max-depth` consumes the following token as its value; a missing or
  non-integer value, or a value outside `1..=16` other than a clamp-eligible
  value above 16, is a usage error.
- `--json` is a valueless flag.
- Every remaining token is an operand, in order: first `from`, then `to`.
- A remaining token beginning with `--` is a usage error, never an operand.
- Exactly two operands are required; zero, one, or three or more is a usage
  error naming both operands.

Default depth is 12 (the `blast-radius` default); `max_nodes` is fixed at 5 000.
Text mode prints one line per hop; `--json` prints exactly one document
(`{"from","to","hops":[…],"found":bool,"depth_bounded":bool,"node_bounded":bool,"unresolved":null|"from"|"to"}`) and suppresses the
staleness and version notices, matching the `blast-radius` arm.

### Failure, edge cases & resilience

- `from` and `to` resolve to the same symbol → zero hops, `found: true`.
- `from` or `to` unresolvable by name and by id → `found: false` with
  `unresolved` naming that side, never a silent empty result (R3).
- No route found and either bound set → the bounded-absence diagnostic.
- `max_depth` above 16 clamps; `0` or a non-integer is a usage error.

### Quality attributes (NFRs)

- One store traversal per resolved start symbol (AC-pinned).
- Response under the 25 000-character R4 budget measured over content plus the
  appended diagnostics block, bounded by `max_depth ≤ 16` hops (AC-pinned with
  a 200-character-identifier worst case; measured worst case ~23.9K).

## Tasks

### T1: `Subgraph::shortest_path` returns the ordered hops, deterministically

**Depends on:** none

**Touches:** `crates/wicked-estate-core/src/query.rs`

**Also in this task:** correct the `Subgraph::truncated` rustdoc at
`crates/wicked-estate-core/src/query.rs:66`, which reads "True if a cap
(`max_depth` / `max_nodes`) truncated the result". No store sets it from the
depth cap (`crates/wicked-estate-store/src/lib.rs:751`,
`crates/wicked-estate-store/src/sqlite.rs:2743`,
`crates/wicked-estate-store/src/postgres.rs:1624`), and D1b builds
`depth_bounded` on that fact — so an implementer reading the field doc would
reach the opposite conclusion. This is the same file and the same change
(§8, retire as you go).

**Tests:**
- A chain `A→B→C→D` of `Calls` edges: `shortest_path(A, D)` returns three edges
  in order `A→B`, `B→C`, `C→D`.
- A diamond `A→B→D`, `A→C→D`: the result has exactly two hops, and two runs over
  the same subgraph return the identical hop sequence (D4 determinism).
- `shortest_path(A, A)` returns `Some(vec![])`.
- A subgraph with no route from `A` to `Z` returns `None`.
- **Edge admission (D1a):** a subgraph whose `edges` contain a hop `D→E` where
  `E` is absent from `nodes` returns `None` for `shortest_path(A, E)` and still
  returns the `A→…→D` route for `shortest_path(A, D)`. This is the SQLite
  frontier-exterior case reproduced as a unit test.

**Approach:**
- Build the adjacency from `self.edges` in a stable sorted order so BFS is
  deterministic (D4), admitting an edge only when both endpoints are in the
  subgraph's node set (D1a). The orientation is fixed at `source → target`
  (D6) — the seam takes no direction parameter.

**Done when:** the five tests above are green and
`cargo test -p wicked-estate-core` passes with 0 warnings.

### T2: `path_between` in core resolves names and issues one traversal per start

**Depends on:** T1

**Touches:** `crates/wicked-estate-core/src/path.rs`,
`crates/wicked-estate-core/src/lib.rs` (module declaration),
`crates/wicked-estate-store/tests/path_backends.rs`

**Where the tests live, and why they are not in core.** `wicked-estate-core`
has no store dependency — only `proptest` as a dev-dependency — so a core unit
test cannot construct a `MemStore` or a `SqliteStore`. The seam's tests
therefore live in `crates/wicked-estate-store/tests/path_backends.rs`, the
first crate that has both backends and already hosts `conformance.rs`. This
satisfies §5: the new core module has a consumer and a test that references
it.

**Tests:**
- Against a `MemStore` holding an `A→B→C` chain, `path_between(store, "A", "C", 8, 5_000)`
  returns two hops in order with their confidences intact.
- A `GraphRead` wrapper that delegates to `MemStore` and counts `traverse` calls:
  one resolved start name yields exactly one `traverse` call and zero
  `neighbors` calls.
- A `from` name resolving to two symbols, one of which reaches the target: the
  shorter route is returned.
- A `to` name resolving to two symbols at different distances: the nearer one
  wins, and two runs pick the same one (D2).
- A `from` given as a `SymbolId` string that is not any symbol's name resolves
  through the id fallback and returns the same hops as the name form (D5).
- A `from` matching neither a name nor a live node id yields `found: false` and
  `unresolved` naming the `from` side, without error.
- A chain longer than `max_depth`: `hops` empty, `found` false, and
  `depth_bounded` true — asserted against **both** a `MemStore` and a
  `SqliteStore` built from the same graph, with the two results compared for
  equality (D1a, D1b).
- The same MemStore-versus-SqliteStore comparison over a graph that **also**
  holds a name matching several symbols with two equally short routes, inserted
  in an order whose SQLite sid sequence differs from `SymbolId` string order.
  Both stores must return the identical hops. Without this shape the candidate
  sort (D2) could be deleted with every other test still green, because the
  stores' own ordering diverges only on multi-match.
- A graph whose traversal exhausts `max_nodes` sets `node_bounded` true and
  carries `Subgraph::truncated` through unchanged.
- **`endpoints` is populated (D8/D9).** For every returned hop, both its
  `source` and its `target` appear in `PathResult::endpoints` as a `Node`. D1a
  already guarantees both endpoints are subgraph nodes, so an empty or partial
  `endpoints` is a defect in this task, not an unavoidable gap — and without
  this assertion T2 can close green while T4's zero-`get_node` test is the only
  thing that would notice.
- **`node_bounded` aggregates as a disjunction too.** A `from` name matching
  two symbols where exactly one candidate's traversal is node-capped and no
  route is found: `found` false and `node_bounded` true. Symmetric to the
  `depth_bounded` case below and needed for the same reason — without it, an
  implementation can fold one flag with `|=` and read the other off the winning
  candidate, passing every other test while emitting a proven-absence signal
  for a bounded search (R3).
- **Bound flags aggregate as a disjunction.** A `from` name matching two
  symbols where exactly one candidate's traversal reaches its depth frontier,
  and no route is found from either: `found` false and `depth_bounded` true.
  An implementation reporting the winning or last candidate's flag returns
  `false` here and passes every other bound-flag test in this task, while
  emitting a proven-absence signal for a bounded search (R3).
- **The over-approximation's falsifier (D1b).** On a chain of exactly
  `max_depth` hops that *does* reach the target: `found` true **and**
  `depth_bounded` true together. On a strictly shallower graph whose whole
  reachable set sits inside the bound: `depth_bounded` false. Without this pair
  an implementation computing `depth_bounded` as "no route and the frontier was
  reached" passes every other test while violating the contract.

**Approach:**
- Resolve each end by an exact-name `SymbolQuery` first, then fall back to
  `get_node(SymbolId)` (D5). Note `wicked_estate::search()` itself is **not**
  reachable — it lives in a crate core cannot depend on (D8) — so core inlines
  the same three-line `SymbolQuery { exact_name: Some(name), ..Default::default() }`
  lookup it performs. Matching semantics stay identical to `blast-radius`
  because the query is identical, not because the helper is shared.
- Sort both candidate lists by `SymbolId` string before searching, so the
  winning pair is a property of the data rather than of the backend's
  `find_symbols` ordering (D2).
- Compute `depth_bounded` from the returned `depths` map rather than reading
  `Subgraph::truncated`, which no store sets from the depth cap (D1b).
- Take `max_nodes` from the caller (D7) and pass it straight into the
  `TraversalSpec`; the seam fixes neither budget.

**Done when:** `cargo build -p wicked-estate-core` is warning-free and
`cargo test -p wicked-estate-store` passes, including the call-count test, the
`endpoints`-populated test, and the MemStore-versus-SqliteStore equality test.

### T3: `wicked-estate path <from> <to>` prints the route

**Depends on:** T2

**Touches:** `crates/wicked-estate/src/main.rs`, `crates/wicked-estate/tests/path_cli.rs`

**Tests:**
- e2e: index a fixture with a known chain; the text output lists the hops in
  order with kind and confidence on each line.
- e2e: `--json` output parses as exactly one document with `hops`, `found`,
  `depth_bounded`, and `node_bounded`, and emits no staleness notice.
- e2e: each `--json` hop's `source` and `target` is an object carrying
  `symbol`, `name`, `kind`, `file`, `line` and `line_1based` — the same six
  fields the MCP tool emits (owner decision, 2026-09-30).
- e2e: an unresolvable `from` prints `found: false` with `unresolved: "from"`,
  and a *resolvable* pair with no route prints `found: false` with
  `unresolved: null` — the two must not be byte-identical (R3).
- **Reviewer check, not an e2e assertion:** the `main.rs` render path builds
  endpoint objects from `PathResult::endpoints` and calls no `get_node`. An
  e2e test cannot observe this — `crates/wicked-estate/tests/e2e.rs` runs
  in-process against the library and cannot import a binary target, and a
  spawned binary exposes no seam at which to substitute a counting
  `GraphRead`. T4's in-crate wrapper covers the MCP side, where the property
  *is* observable.
- e2e: `path A`, `path` alone, and `path A --json` each exit non-zero with the
  usage line naming both operands — the last because `--json` is consumed as a
  flag, leaving one operand.
- e2e: `path --max-depth 4 A B` and `path A B --max-depth 4` both resolve
  `from = A`, `to = B`, depth 4.
- e2e: `path --max-depth 4` exits non-zero rather than resolving `--max-depth`
  as an operand.
- e2e: with no `--max-depth`, a 12-hop chain is found and a 13-hop chain is not
  (the default is 12).
- e2e: `--max-depth 17` runs at depth 16 rather than erroring (a 16-hop chain is
  still found); `--max-depth 0` and `--max-depth abc` each exit non-zero naming
  the accepted range `1..=16`.
- e2e: `--help` output contains the `path` line.

**Approach:**
- Take the `"blast-radius"` arm's *store and output* conventions only — the
  `--json`-suppresses-notices rule, the `emit_cli_span` observability call, the
  `open_store_ext` handle. Its **argument** shape does not carry over: that arm
  has one operand and reads `--json` out of `positional` by scanning for it,
  which silently mis-parses a two-operand command. Implement the argument
  contract in Interfaces & contracts above instead, classifying every
  `positional` token before reading either operand.

**Done when:** the e2e tests are green and a recorded manual run of the release
binary shows exit code 0 and the hops in the per-hop shape the acceptance
criterion pins — one line per hop, each naming source, target, edge kind and
confidence — against a fixture the repository defines: the Rust
`util <- service <- handler` call chain that `crates/wicked-estate/tests/e2e.rs`
writes and indexes (`end_to_end_index_resolve_blast_radius`), extended in this
task's own fixture to the depths the bound criteria need. For
`wicked-estate path handler util` that is two lines, the first naming
`handler` → `service` and the second `service` → `util`, each with its kind and
confidence. The exact column layout is build-time detail; the four facts per
line and the one-line-per-hop shape are not.

### T4: the `Path` retrieval tool returns denormalized hops with diagnostics

**Depends on:** T2

**Touches:** `crates/wicked-estate-retrieve/src/lib.rs`

**Tests:**
- `invoke` over a chain returns `content.hops` in order, each hop's `source` and
  `target` denormalized to `{symbol,name,kind,file,line,line_1based}`.
- A missing `from` or `to` returns an empty result with a diagnostic naming the
  missing field (the `TraverseGraph` required-field pattern).
- An **unresolvable value** — a `from` present in the request but matching
  neither a symbol name nor a live node id — returns `found: false` with a
  diagnostic naming the unresolved side. A distinct path from the missing-field
  case above; the AC binds both surfaces.
- A hop below 0.5 confidence produces an `R7-CONFIDENCE` diagnostic naming the
  count.
- No route plus `depth_bounded` or `node_bounded` produces the bounded-absence
  diagnostic; no route with both clear does not.
- A `from` given as a `SymbolId` resolves through the id fallback (D5).
- Every response — found, not found, and bounded — carries the staleness
  diagnostic (R5), matching every other estate tool.
- **No per-endpoint store query at the rendering layer.** A `GraphRead` wrapper
  counting `get_node` calls: rendering a 16-hop path issues **zero**, because
  the cache is seeded from `PathResult::endpoints`. This assertion belongs here,
  not only in T2 — T2's counter sits at `path_between`, which never
  denormalizes, so it cannot catch an unseeded cache.
- A 16-hop chain with 200-character symbols, names, and paths stays under
  25 000 characters measured over **content block plus the diagnostics block**,
  the same quantity the acceptance criterion names — not the content block
  alone. The diagnostics are ~400 characters of a ~23.9K total, so measuring
  only content would leave the entire remaining margin unchecked.
- `depth` above 16 clamps to 16; `max_nodes` above 5 000 clamps to 5 000.

**Approach:**
- Reuse `edge_json` and the node cache from the `TraverseGraph` implementation so
  both tools denormalize identically (§11 — one seam, not two copies). Seed that
  cache from `PathResult::endpoints` before rendering any hop; `TraverseGraph`
  seeds from `subgraph.nodes` at `crates/wicked-estate-retrieve/src/lib.rs:722-726`
  and this is the same step. Unseeded, `endpoint_json` falls through to
  `store.get_node` per endpoint. This applies to **the MCP surface only** —
  `edge_json` and `endpoint_json` are private to this crate, so the CLI cannot
  call them and renders its own endpoint objects from the same
  `PathResult::endpoints` (D9).

**Done when:** `cargo test -p wicked-estate-retrieve` passes.

### T5: MCP exposes `Path` and the tool counts read 12 estate / 30 total

**Depends on:** T4

**Touches:** `crates/wicked-estate-mcp/src/lib.rs`,
`crates/wicked-estate-mcp/tests/sc009.rs`,
`crates/wicked-estate-mcp/tests/conformance_schemas.rs`,
`crates/wicked-estate-mcp/tests/conformance/schemas/Path.json`

**Tests:**
- `tools/list` with all four stores returns exactly 31 tools.
- `input_schema("Path")` returns a schema whose `required` is `["from","to"]`.
- A `tools/call` for `Path` returns hops through the MCP envelope.
- `Path` is present in the `--readonly` `tools/list` and its call is not refused.
- The estate-only list returns 12 tools.
- Conformance L2.2 loads a frozen golden schema for every `ESTATE_TOOLS` entry
  including the new one.
- Goal-based: `cargo test -p wicked-estate-mcp` passes with every count
  assertion at its new value — estate 12, total 30, read-only surface 20,
  unconditional floor 12, floor-plus-semantic 13 — and none weakened, ignored,
  or deleted. These assertions are the real gate: they carry the values, so a
  missed constant reds the crate.
- Reviewer check, not a pattern: read the three count-bearing files
  (`crates/wicked-estate-mcp/src/lib.rs`, `tests/sc009.rs`,
  `tests/conformance_schemas.rs`) and confirm no rustdoc line, comment,
  assertion message, or test name still states a pre-change count. Known sites
  include the rustdoc at `src/lib.rs:51`, `:62`, `:687`, `:1057`, `:1059`,
  `:1062`, `:1089`; the comments at `:2472`, `:2625`, `:2626`; the assertion
  messages at `:1077`, `:1112`, `:2496`, `:2524`, `:2630`; the file headers and
  inline comments of both test files; and these **five** test names:
  `tools_list_returns_eleven_unconditional_tools` → twelve,
  `tools_list_returns_twelve_with_semantic_available` → thirteen,
  `unified_tools_list_without_domains_returns_11_tools` → 12,
  `unified_tools_list_with_domains_returns_29_tools` → 30, and
  `conf_tool_count_11_estate_7_memory_7_knowledge_4_proposal` → 12. That list
  is a starting point, not a closed set — which is exactly why the gate is a
  read rather than a regex.

**Approach:**
- Freezing the golden is a deliverable, not a side effect: add
  `crates/wicked-estate-mcp/tests/conformance/schemas/Path.json` and the
  `ESTATE_TOOLS` entry together. `load_golden` panics on a missing file, so the
  roster cannot grow without the golden landing in the same change.
- Numbers live in prose, in assertion messages, and in test names here — not
  only in assertion values. Only the values fail loudly; a rustdoc line, a
  message string, or a function name saying eleven goes stale in a green
  workspace. Three rounds of review each found a new phrasing a widened regex
  missed (`the conditional 12th`, `= 11 + 3 + 3 + 2 = 19.`, a test name), which
  is evidence that no pattern closes the set. The gate is therefore an
  enumerated read of three named files, and it claims only what a read proves.

- The tool name must be added at every registration site, not just `all_tools()`:
  the `use` import, `all_tools`, `input_schema`, the `tools/call` dispatch match
  arm, and `response_cacheable`. A tool absent from the dispatch arm lists but
  does not call. There is **no** read-only keep-list to edit: read-only mode
  filters only the memory, knowledge and proposal lists through `is_write_tool`,
  and estate tools from `all_tools()` bypass that filter entirely — which is why
  the read-only criterion holds without a new registration site, and why the
  read-only surface total moves from 19 to 20 on its own.

**Done when:** `cargo test -p wicked-estate-mcp` passes and no count assertion was
weakened — every count assertion reads its new number because the roster grew.

### T6: the command is documented and the release is noted, with no count touched

**Depends on:** T5

**Touches:** `README.md`, `docs/getting-started.md`, `CHANGELOG.md`

**Tests:**
- Goal-based: `grep -n "wicked-estate path" README.md docs/getting-started.md`
  returns the command in each file's **CLI usage section** — `README.md`'s
  `wicked-estate index / query / blast-radius` block, and the usage section of
  `docs/getting-started.md`. Neither section carries a tool count, so this task
  has no count boundary to police.
- Goal-based: `CHANGELOG.md` has an `## [Unreleased]` entry naming both the
  `wicked-estate path` command and the MCP `Path` tool.
- Reviewer check: the diff for this task touches no tool count and no estate
  roster. Five review rounds established that no pattern closes the set of ways
  prose states a number, so this is a read of a three-file diff rather than a
  gate claiming otherwise. It is small enough to read.

**Done when:** both greps return the command and the entry, and the task's diff
is confined to the three files above.

## Rollout

- **Delivery:** big bang, no flag. The change is purely additive — a new CLI
  subcommand and a new MCP tool. Rollback is reverting the commit; nothing is
  irreversible, no data is migrated, no persisted shape changes.
- **Infrastructure:** none.
- **External-system integration:** none.
- **Deployment sequencing:** none — a single binary, and an MCP client that does
  not know about `Path` keeps working because `tools/list` growth is additive.

## Risks

- Count-bearing **prose** goes stale in a green workspace. The assertion values
  are safe — they live in files T5 touches and `cargo test -p wicked-estate-mcp`
  is its Done-when — but rustdoc, comments and test names fail silently. Three
  review rounds each found a phrasing a widened regex missed, so the mitigation
  is an enumerated read of three named files rather than a pattern claiming
  exhaustiveness. Residual risk accepted: a reader may still miss a line. That
  is a smaller, more honest risk than a gate that exits 1 over stale text.
- Scope leak into the split-out roster-and-count refresh. No fence guards this,
  by decision: a fence drawn through prose documents produced two blockers of
  its own. The guard is that T6 touches only three files and none of the six
  count-bearing documents, so the diff a reviewer reads is small.
- The `Path` tool duplicating `TraverseGraph`'s denormalization by copy rather
  than by shared helper, re-creating the class of drift §11 warns about.
  Mitigated by T4's explicit reuse of `edge_json`.
- `search()` returning many symbols for a common name turns one traversal into
  many. Bounded in practice by exact-name matching, but worth watching if an
  integrator reports a slow `path` on a large graph.

## Changelog

- 2026-09-29: spec approved by eugenelim
- 2026-09-30: plan approved by eugenelim
