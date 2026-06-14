# Benchmark Methodology — the Truth Oracle

**Wave:** W0.6 (framework) → W1.6 (first baseline) → W8.1 (final gate) · **Crate:** `wicked-estate-bench`

The benchmark is how coherence is enforced across a parallel build: every change is measured, not
argued. Re-implements the `agent-eval` A/B design.

## Protocol (A/B)

For each `EvalTask` (a retrieval question with a known-good `gold_files` set), run an agent twice:
- **baseline arm** — agent with only grep/read.
- **treatment arm** — agent with wicked_estate's MCP tools available.

Capture `ArmMetrics` for each arm: `tool_calls`, `files_read`, `tokens_in`, `answer_file_recall`
(fraction of `gold_files` the agent surfaced).

## Headline metrics

- **Token reduction** = `baseline.tokens_in / treatment.tokens_in` (prior art reports ~76% fewer
  file reads; SoA reports 10–121× tokens — `EvalReport::mean_token_reduction`).
- **File recall (treatment)** = mean `answer_file_recall` with the tool (LocAgent hit 94% on
  SWE-bench Lite — `EvalReport::mean_file_recall_treatment`).
- **Resolution precision/recall** (W1.5.2 / W3.5) — heuristic-edge correctness vs a hand-labeled set.

## Corpus (frozen at W0.6)

`wicked_estate_bench::baseline_corpus()` — three repos spanning the decision languages:

| name | language | repo |
|---|---|---|
| `ts-axios` | TypeScript | axios/axios @ v1.7.9 |
| `py-flask` | Python | pallets/flask @ 3.1.0 |
| `poly-prior art` | polyglot | a public polyglot repo @ HEAD |

## The gate

- **W1.6 (GO/NO-GO):** treatment must show a *measurable* lift over the tree-sitter-tags-only
  baseline AND the storage bake-off must have a clear winner. Else STOP / reassess.
- **W8.1 (ship bar):** final treatment must exceed the best reviewed system's file-read reduction.

Baseline numbers are recorded at W1.6 (extraction must exist first); W0 ships only the framework
+ frozen corpus.
