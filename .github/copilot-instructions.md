# GitHub Copilot instructions

**Single source of truth for build guardrails is [`../CLAUDE.md`](../CLAUDE.md).** Read it before
working in this repo. It carries the Voice, the §1–§12 Working Style, the Universal Don'ts, the
locked decisions, the "don't rebuild the spine" table, and the commands/gates — all of which apply
to **every AI tool** (Claude Code, Gemini, Codex, Cursor, Copilot, Amp, …), not just Claude.

This file adds **no rules of its own** — same contract, same gates for every tool. Divergence
defeats the purpose. Treat `CLAUDE.md` as authoritative.

Fast orientation: greenfield Rust code-graph parser built fleet-parallel behind a fixed trait spine
(`crates/wicked-estate-core/src/traits.rs`). Hard invariants in `docs/ENGINE-CONTRACT.md`. Key
build discipline that bites if ignored: **per-crate builds only** (`cargo build -p <crate>`, never
`--workspace` from a fanned-out agent — §12), confidence + provenance on every `Edge`, stable
symbol IDs (never content-hash), bounded `traverse` only.
