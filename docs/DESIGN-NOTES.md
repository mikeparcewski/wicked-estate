# Design Notes — the principles behind wicked-estate

wicked-estate is a deliberate consolidation of hard-won lessons about building a code graph that an
LLM agent can actually rely on. These are the load-bearing principles — *why* the engine is shaped
the way it is. Each one is a scar: the failure mode it prevents is worse than it looks.

## Identity & schema

- **Stable symbol identity — never content-hash or line number.** Keying a node by a hash of its
  body or by its line makes every edit look like a delete + re-create: edges break, history is lost,
  blast-radius lies. Identity must be a stable `(scheme, qualified-name)` that survives reformatting
  and line shifts. (ADR-002)
- **Every edge carries `{confidence, provenance, resolved_by}`.** A graph that can't say *how sure*
  it is, or *who* produced an edge, can't be trusted by an agent or audited by a human. A heuristic
  edge must never be presentable as a fact.
- **Edge direction is an invariant:** `source = dependent`, `target = dependency`. Blast-radius is
  transitive *dependents*, and it must follow **every** dependency edge kind — a blast radius that
  only follows calls silently under-reports (e.g. the security profile that protects a dataset, or
  the config that wires a resource). Silent under-reporting is the most dangerous failure a
  code-intelligence tool can have.

## Extraction & resolution

- **Two-phase EXTRACT → RESOLVE.** Parse once into nodes + *unresolved* references; resolve
  separately. Resolution is then swappable and improvable **without re-parsing** — you can add a
  precise tier later without touching the extractors.
- **Layered resolution, cheap → precise.** Broad-but-fuzzy name/scope/import-map resolution first;
  precise indexers (SCIP) and on-demand language servers only where they earn their cost. Label the
  tier on every edge. On-demand precise resolution — never bulk — keeps it affordable.
- **Rules as data, not per-language code.** Extraction/resolution logic belongs in query files and
  config, not in compiled `match language { … }` arms. A new language should be **a manifest row + a
  query file — zero core change**. The corollary: a capability matrix must be *generated from that
  data*, never hand-maintained — hand-maintained matrices rot the moment a language is added.
- **One fix is a hypothesis about every language.** Extractors built in parallel share defect
  classes; when you fix one (e.g. a quote-leak in string-literal call targets), re-audit the rest and
  fix at the shared seam, not in N copies.

## Traversal & retrieval

- **Bounded traversal only.** Every graph walk carries `max_depth` + `max_nodes`. No unbounded
  whole-graph walks; use a real recursive traversal, never N-queries-per-node.
- **Rank, then budget.** PageRank-style importance + token-budgeted context packing is what turns "a
  graph" into "the *right* 25K characters for this prompt." Beyond a budget the agent ignores the
  output anyway.
- **Hybrid retrieval: graph + full-text core, embeddings an optional sidecar fused via RRF.** Keep
  the default dependency-free and offline; offer a tiered embedder ladder (lexical → static-semantic
  → contextual-semantic) so semantic quality is a *choice*, not a forced heavy dependency.

## The agent contract

The biggest unclaimed win is **how the tool behaves toward the agent consuming it**:

- Never return an error early in a session — a single hard error makes an agent abandon the tool for
  the rest of the conversation.
- An unindexed/empty graph should expose **zero** tools, not erroring ones.
- **Partial coverage is worse than none** — silently answering for 60% of the repo teaches the agent
  to trust answers that are wrong for the other 40%.
- Cap output, always report **staleness**, emit a loud marker when the agent must fall back to
  reading files, and keep confidence **visible**.

## Storage & estate

- **Storage behind a trait + capability negotiation.** A local embedded store is the right default;
  an external/server database must drop in as one module behind the same trait, with retrieval
  negotiating what the store can do. Footprint is a feature — intern symbols, content-address +
  compress source, prune aggressively.
- **The "estate" is just more languages.** Infrastructure-as-code and mainframe artifacts
  (security, data, messaging definitions) are extractors/collectors feeding the *same* graph;
  resources are nodes, dependencies are edges. The payoff is **cross-domain joins** — the one graph
  where "what protects the dataset this batch job writes?" is a single query — and **drift** as a
  graph diff between declared and live state.

## Build discipline

- **Spine before fan-out.** Define the trait seams + their conformance tests serially first; only
  then fan out file-disjoint work behind them. The spine is what makes massive parallelism cohere.
- **Gate on a continuous benchmark, and keep green non-negotiable.** Tune against an A/B
  agent-eval oracle (not vibes), and never lower the bar to go faster — profile and fix the cause.
