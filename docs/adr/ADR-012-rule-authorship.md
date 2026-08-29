# ADR-012 — Rule authorship: git is the source of truth, promotion only by doc PR, no `rules.write` MCP tool

**Status:** Accepted · **Date:** 2026-08-29 · **Lane:** rules-surface (arch-R8)
**Implements:** `crates/wicked-estate-retrieve/src/rules_recall.rs` (the read-only recall surface),
`crates/wicked-estate-mcp/src/lib.rs` (tool registry — no rule-mutation entry),
`crates/wicked-estate-mcp/tests/rules_surface.rs` (the conformance tripwire)
**Resolves:** the arch-R8 recon finding — without a locked authorship contract, an agent could
author the deterministic gate that later judges its own runs.

## Context

Conformance rules (`PAT-*` patterns / `POL-*` policies) persist on the estate graph as native
`NodeKind::Rule` nodes and are what the governed-run gates (wicked-core's wicked-governance:
PreToolUse gate-hook, output gate, phase-boundary checks — all deny-dominates) enforce
deterministically. The wicked platform's core invariant is **evaluator ≠ creator**: a worker must
be structurally unable to grade its own work. Rules are the sharpest instance — a rule is an
evaluator that outlives the run that proposed it. If the MCP surface offered any rule write path,
an LLM agent could mint or soften the guardrail that later gates its own output, and no audit of
individual runs would catch it.

## Decision

1. **Git-tracked docs are the source of truth for rules.** The graph's `Rule` nodes are a
   rebuildable projection; every rule's provenance traces to a document at a git commit
   (`provenance.source_kinds: ["doc"]`, `provenance.ref` = the doc path). Losing the store loses
   nothing normative.
2. **Promotion to an enforceable `Rule` happens only via a human-merged doc PR.** LLM-extracted
   rule candidates (garden's domain/modernize surfaces, review findings, session learnings) may
   land in the KNOWLEDGE store as ordinary knowledge nodes labeled non-normative
   (`status=proposed` in the candidate's text/frontmatter) — visible to `knowledge.recall`, never
   to the gates. A candidate becomes a rule only when a human merges the doc PR that states it and
   the governance ingest (`wicked-core rules ingest`, fail-closed INV-C1..C4 validation) projects
   it into the rule store. The repo PR-merge protocol (bot reviewers + CI, root CLAUDE.md) applies.
3. **There is deliberately NO `rules.write` (or any rule-mutation) MCP tool.** The absence of a
   write surface IS the guardrail. The estate MCP surface exposes exactly two rules-ish tools,
   both read-only: `RulesInventory` (RuleSet engines + Rule-node counts) and `rules.recall`
   (faceted, severity-ordered recall). Retirement (`retire_rule`) and registration
   (`register_rule`) exist only behind wicked-core's CLI/API on the human side of the seam.
4. **Manual curation stays limited to the knowledge lane** — `knowledge.relate`
   confidence/evidence tuning and proposed-candidate upkeep. Nothing on the agent surface touches
   `NodeKind::Rule`.

## Enforcement

This ADR is conformance-tested, not aspirational:
`crates/wicked-estate-mcp/tests/rules_surface.rs::tool_registry_exposes_no_rule_mutation_tool`
asserts on the ACTUAL advertised tool registry (all domains active) that `rules.recall` is present
and that no advertised rules-ish tool name carries a mutating verb (write / register / retire /
upsert / update / delete / create / mutate / ingest / set), and
`rules_write_call_is_unknown_tool` asserts a `tools/call` for `rules.write` / `rules.register` /
`rules.retire` is rejected as an unknown tool without side effects. Adding a mutating rules tool
fails the suite — change this ADR first, on purpose, in a reviewed PR.

## Consequences

* Agents (and CI) get full deterministic-rule VISIBILITY (`rules.recall` facets:
  language/layer/framework/severity/rule_type/scope) with zero authorship authority.
* Rule changes are as slow as a doc PR — deliberately. An urgent bad-rule retirement is a
  human CLI/API action (wicked-core), not an agent action (kill-switch workflow tracked as
  arch-R22, outside this ADR).
* The ingest seam (JSON bundles today; a MarkdownAdapter on the same `SourceAdapter` trait is
  arch-R1, in wicked-core) is the ONLY door from prose to enforceable rule, so drift detection
  and provenance live in one place.
