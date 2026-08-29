# AGENTS.md

## Contributor docs

Contributor docs: [`docs/`](./docs/)

Invariants and contracts: [`docs/ENGINE-CONTRACT.md`](./docs/ENGINE-CONTRACT.md).
Extension recipes: [`docs/add-lang.md`](./docs/add-lang.md) (new language),
[`docs/extractor-sdk.md`](./docs/extractor-sdk.md) (drop-in edge rules),
[`PLUGIN.md`](./PLUGIN.md) (runtime grammar plugins). Decision records:
[`docs/adr/`](./docs/adr/). Onboarding: [`docs/getting-started.md`](./docs/getting-started.md).
Upgrading from 0.14.x: [`docs/MIGRATION-0.15.md`](./docs/MIGRATION-0.15.md).
This pointer block is the machine-readable anchor agents grep for.

**Single source of truth for build guardrails is [`CLAUDE.md`](./CLAUDE.md).** Read it before
working in this repo. It carries the Voice, the §1–§12 Working Style, the Universal Don'ts, the
locked decisions, the "don't rebuild the spine" table, and the commands/gates — all of which apply
to every AI tool (Claude Code, Codex, Cursor, Amp, …), not just Claude.

Fast orientation: this is a greenfield Rust code-graph parser built fleet-parallel behind a fixed
trait spine (`crates/wicked-estate-core/src/traits.rs`). Live plan: `docs/plan/WAVE-PLAN.md`. Hard invariants:
`docs/ENGINE-CONTRACT.md`. Don't fan out before the spine + its conformance test are green.
