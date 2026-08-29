---
id: wicked-estate-adr-011
title: "Graph-backed architecture wiki: deterministic guardrails + cited guidance from one doc corpus"
status: active
date: 2026-08-29
applies_to: [wicked-estate, wicked-core, wicked-garden, wicked-crew, wicked-studio]
scope: wiki:architecture
domain: architecture-wiki
---
# ADR-011 — Graph-backed architecture wiki

**Status:** Accepted · **Date:** 2026-08-29 · **Lane:** architecture-wiki (recon-2026-08, arch-R1..R25)
**Basis:** ADR-005 §1 (content store — graph + text + vectors as one queryable brain),
DES-OUTGOV-001 §3 (wicked-core `.product/` — Domain→RuleSet carriage, rule provenance),
`recon-2026-08/RECON-ARCH-WIKI.md` (the adversarially-reviewed design this ADR records).
**Consumes/consumed by:** wicked-core `crates/wicked-governance` (MarkdownAdapter, `rules ingest
--dir`, gates), estate `rules.recall` + `knowledge.recall` (MCP), wicked-garden `mem`/`search`
domains, wicked-crew `/api/v1/governance` + acceptance view.

This is the first document the MarkdownAdapter ingests: its own frontmatter follows the contract
it defines, and its `## Rules` section mints the wiki's founding enforceable rules.

## Context

The wicked ecosystem's architectural doctrine — planes, contracts, event grammar, agent-behavior
rules, storage doctrine, ADRs — lives as unnormalized prose across nine source families (five
incompatible ADR conventions among them), while BOTH halves of a working wiki already shipped:

- **Deterministic lane:** wicked-governance persists `ConformanceRule`/`Policy` as native estate
  `NodeKind::Rule` nodes behind fail-closed invariants (INV-C1..C4), enforced deny-dominates by
  the PreToolUse gate-hook, output gate, and phase-boundary checks; wicked-crew serves audited
  retire-not-delete CRUD.
- **Non-deterministic lane:** estate knowledge/memory stores with garden's `ingest`/`recall`/
  `answer` skills (mandatory citations).

What was missing is the corpus and its import path: the workspace stores held no doctrine, no
markdown could ingest, ungrouped `Rule` nodes were invisible over MCP, and two spellings of the
`governs` edge split the guardrail graph. This ADR records how the corpus becomes a populated,
connected, machine-consumable wiki — by populating and connecting shipped substrates, never by
building a new product.

## Decision (the design spine)

1. **Git-tracked markdown with ONE frontmatter convention is the source of truth.** The graph is
   a rebuildable projection; every deterministic rule's provenance is a doc path at a git commit.
   Losing a store loses nothing normative.
2. **One parser, normalized corpus.** The `MarkdownAdapter` on wicked-governance's existing
   `SourceAdapter` seam is the ONLY parse path (exposed as `wicked-core rules ingest --dir`); all
   output materializes through `normalize_bundle`'s fail-closed invariants, and a malformed doc
   fails LOUD per file. The small owned legacy corpus was normalized once (AW-12) instead of
   growing per-convention adapters.
3. **The same docs ingest into knowledge** (garden mem-ingest, scope `wiki:<area>`, stable source
   URIs) for cited `answer`; the enforceable twin's `PAT-/POL-` id is embedded in chunk text so
   one recall surfaces guidance beside its machine-enforced counterpart.
4. **Consumption is recall, wired end to end.** `rules.recall` (faceted, severity-ordered,
   read-only) serves the deterministic lane over MCP; `knowledge.recall {scope_prefix:
   "wiki:…"}` serves scoped guidance; gate denials cite wiki rule ids via obligations
   (`attach_recalled_rules`), so an agent can trace a denial back to the doc that caused it.
5. **Severity + enforcement class is the guardrail/guidance mode switch.** Frontmatter
   `enforcement_class` types every statement at ingest: **(a) policy** — regex-triggerable →
   deterministic `Policy`, critical/error hard-denies; **(b) validator** — structurally checkable
   → pinned `DeterministicValidator`; **(c) guidance** — semantic-only → knowledge + obligations,
   NEVER a fake deterministic rule. Honesty rule: only what a machine can actually check claims
   the deterministic lane.
6. **Promotion to enforceable rules happens only via a human-merged doc PR** (the authorship
   contract, ADR-012): LLM-extracted candidates land as non-normative `status=proposed`
   knowledge; there is deliberately NO `rules.write` MCP tool — the absent write surface is the
   guardrail (evaluator≠creator extended to guardrail authorship).
7. **Fan-out, not unification, across the deliberate store split** (§fan-out below): one manifest
   keyed on stable `PAT-/POL-` ids feeds the enforcement, discovery, and knowledge lanes, each
   smoke-verified after import.
8. **Symbol links are durable-by-name, derived-by-id:** rule→code `Governs` edges are re-derived
   from qualified `symbol_refs` after every index (`rules relink`, AW-9), because estate 0.15
   epoch-drops xedges on re-index. Unresolvable refs surface as drift findings — never dropped
   silently.

## §adr-contract — the ONE ADR frontmatter contract

Every ADR in every wicked repo opens with this YAML frontmatter (AW-12 / arch-R12). It is a
subset of the MarkdownAdapter's frontmatter convention, so every ADR is ingestable as-is:

```yaml
---
id: wicked-estate-adr-011        # required — <repo>-adr-<number>, unique workspace-wide
title: "Graph-backed …"          # required — quoted scalar (one outer quote pair)
status: active                   # required — active | draft | superseded | retired
date: 2026-08-29                 # required — ISO YYYY-MM-DD (decision date)
supersedes: [<ids>]              # optional — FULL supersession only, by contract id
applies_to: [<scopes>]           # optional — repos/planes/phases the decision binds
---
```

- **id** — `<repo>-adr-<number>` keeps the repo's own number spelling (estate `011`, garden and
  interactive `0011`) and is the graph node identity plus the fan-out manifest key.
- **status vocabulary is the adapter's, not classic ADR-speak:** an accepted, in-force decision
  is `active`; `superseded`/`retired` docs stay parsed and preserved but their minted rules carry
  `retired = true` (withdrawn from recall). Classic "Accepted" prose in the body stays untouched.
- **supersedes** records FULL supersession only and materializes as a `supersedes` edge
  (wicked-apps-core `SUPERSEDES`) plus `retired=true` on the superseded doc's rules; PARTIAL
  supersession ("the brain half of X") stays in prose, never as an edge.
- **applies_to is a facet, never an edge** (arch-R17): it matches SELECT/Targets wildcard
  scoping (wicked-apps-core `APPLIES_TO`).
- The full adapter convention (`enforcement_class`, `scope`, `domain`, `confidence`, `targets`,
  `## Rules` items) remains available to any ADR that mints enforceable rules — like this one.
- **Retired-source provenance:** ADRs lifted from retired repos (wicked-brain, wicked-testing)
  ingest read-only with retired-source provenance ranked below live sources.

Normalized under this contract as of 2026-08-29: estate `docs/adr/ADR-001..012` (011 = this
doc), garden `docs/adr/0001..0007` (the duplicate-numbered 0005 companion renumbered to 0007),
interactive `docs/adr/0001..0027` (inline `(ADR-00NN)` code tags written out as real files).
The contract's executable test lives beside the parser:
`wicked-core/crates/wicked-governance/tests/adr_contract.rs`.

## §edge-vocabulary — one graph, two spellings, pinned

Native `EdgeKind::Governs` and stringly `EdgeKind::Other("governs")` coexist on the estate graph
(brain-migration legacy). Unpinned, a wiki populating `Rule` nodes would split its guardrail
graph in two — a native-only traversal misses the string spelling and vice versa. The pin
(AW-19 / arch-R17):

- Any edge whose **target is a code-graph symbol** MUST be native `EdgeKind::Governs` — exactly
  what `ConformanceRule::governs_edge` and `conform` emit.
- `Other("governs")` is legal ONLY between knowledge-store nodes (wicked-estate-knowledge's
  DEC-2 relation grammar keeps every knowledge relation stringly, even `governs`).
- wicked-governance ships the runtime check (`edge_vocab::assert_edge_vocabulary`) and a
  source-scan conformance test (`tests/edge_vocab_lint.rs`) so a new stringly mint site cannot
  land in wicked-core unnoticed.
- **Deprecation window:** recall/traversal surfaces that must see EVERY governs relationship
  (estate `TraverseGraph`, knowledge-lane queries, the relink pass) match BOTH spellings until
  the knowledge lane's migration to a single spelling is decided and executed; the window closes
  by explicit decision recorded against this ADR, never by silent attrition.
- Supersession lineage uses the `supersedes` string kind (doc→doc only); `applies_to` never
  becomes an edge.

## §authorship — no rules.write, promotion by doc PR (ADR-012)

The wiki extends evaluator≠creator to guardrail authorship. Git is the source of truth; the
MarkdownAdapter is the only path from prose to enforceable `Rule`; promotion happens only via a
human-merged doc PR (repo PR-merge protocol, bot reviewers included). The MCP surface is
read-only by design — `rules.recall` exists, `rules.write` deliberately does not, and estate's
`rules_surface` conformance test trips if a rule-mutating tool ever appears. Agent-extracted
candidates are visible to recall only as labeled non-normative knowledge until a human merges
them. Emergency retirement is operator-only via crew's audited retire-not-delete API — an agent
never retires the rule that would have judged it (arch-R22).

## §fan-out — one import, every lane a governed run reads

Enforcement and discovery deliberately read DIFFERENT stores in the same governed run
(post-FINDING-067: workers' gates read the operational daemon store; the injected estate MCP
binds the run repo's code-graph db). An import into one home silently misses the other lane, so
every import is a fan-out keyed on stable `PAT-/POL-` ids (AW-5 / arch-R3):

1. **Enforcement copy** → crew `POST /api/v1/governance/{policies,rules}` for live daemons
   (audited), or `wicked-core rules ingest` for offline stores — never the CLI against a
   daemon-held store (single-writer invariant).
2. **Discovery copy** → the repo/project graph as `NodeKind::Rule`.
3. **Rationale** → the knowledge sidecar, scope `wiki:<area>`, source = the wiki URI.

Every import ends with per-lane smoke verification (`rules/preview`, `rules.recall`,
`knowledge.recall`) against the same `--db` a worker is handed.

**Cross-repo doctrine placement (arch-R20):** graphs and sidecars are per-repo, and edges do not
resolve across repos — but the highest-value rules (root CLAUDE.md, TARGET-ARCHITECTURE, event
grammar) span repos. Default: **replicate-to-every-repo** — the fan-out manifest's
`scope: workspace` flag copies the rule into every live repo's discovery store (pure data,
zero engine change; id-keyed idempotent re-ingest keeps the N copies in sync). The alternative
(a workspace-root store with new resolution machinery) is deliberately deferred unless
replication cost bites; AW-6 records the estate-owner ruling on this section.

## §lifecycle — drift, supersession, retirement

- **Re-ingest on merge is idempotent and id-keyed** — a doc change is a self-healing non-event;
  `wicked-core rules drift` reports the residue that cannot self-heal (orphaned rules from
  deleted docs, uningested new docs, unresolvable symbol_refs) via provenance stamped
  `<path>@<git sha>` + `last_verified` (AW-10 / arch-R7).
- **A superseded/deleted doc propagates as retirement of its derived rules** (manifest-keyed,
  across all fan-out lanes), never as silent orphaning; `supersedes` edges carry the lineage.
- Corpus lifecycle emits 4-segment bus events (`wicked.estate.rule.ingested/retired`,
  `wicked.estate.doc.drifted` — AW-22) so crew/studio observe corpus changes and regeneration
  triggers fire.
- The wiki pipeline's own "it works" is evidence-gated by a repeatable golden-path scenario:
  this doc → ingest → `rules.recall` returns it → a governed run trips a trigger → the denial
  cites the wiki URI → the ConformanceClaim appears in `GET /runs/:id/acceptance` (AW-25 /
  arch-R21).

## Rules

- `POL-1101` (critical): Promotion to an enforceable Rule node happens only via a human-merged
  doc PR flowing through the MarkdownAdapter ingest; no MCP tool may create, mutate, or retire
  Rule nodes, and agent-extracted rule candidates land only as non-normative proposed knowledge.
- `PAT-1102` (error): An edge whose target is a code-graph symbol must use the native Governs
  edge kind; the stringly governs spelling is legal only between knowledge-store nodes.
- `PAT-1103` (warn): While the governs deprecation window is open, any recall or traversal that
  claims to see every governs relationship must match both the native and the stringly spelling.
- `POL-1104` (error): Every ADR opens with the one frontmatter contract — id, title, status,
  date, with optional supersedes and applies_to — and parses under the MarkdownAdapter
  convention; a new ADR that does not parse does not merge.

## Consequences

- The doctrine becomes recallable with citations (`answer` names its sources) and enforceable
  where it is honestly checkable — and the gap between those two is explicit (`enforcement_class`),
  never papered over.
- Normalization is a one-time cost already paid (AW-12); from here the contract is cheaper than
  the drift it prevents, and `adr_contract.rs` keeps it executable.
- The graph stays a projection: rebuilding every store from HEAD is always legal and lossless
  for normative content.
- Until the seed corpus lands (AW-13) this wiki is machinery without content; the value claim of
  this ADR is measured by the population/connection scoreboard (arch-R23), not asserted.
