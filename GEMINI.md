# GEMINI.md

**Single source of truth for build guardrails is [`CLAUDE.md`](./CLAUDE.md).** Read it before
working in this repo. It carries the Voice, the §1–§12 Working Style, the Universal Don'ts, the
locked decisions, the "don't rebuild the spine" table, and the commands/gates — all of which apply
to **every AI tool** (Claude Code, Gemini, Codex, Cursor, Copilot, Amp, …), not just Claude.

This file adds **no rules of its own** — same contract, same gates for every tool. Divergence
defeats the purpose. If you are reading this in Gemini CLI, treat `CLAUDE.md` as authoritative.

Fast orientation: this is a greenfield Rust code-graph parser built fleet-parallel behind a fixed
trait spine (`crates/wicked-estate-core/src/traits.rs`). Live plan: `docs/plan/WAVE-PLAN.md`. Hard
invariants: `docs/ENGINE-CONTRACT.md`. Don't fan out before the spine + its conformance test are
green.
