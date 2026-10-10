# wicked-estate-bench

The benchmark harness for wicked-estate. It is internal tooling and is not published to crates.io.

## What it does (and does not)

- **Capability benchmark** (the default). For each repo it indexes into a fresh in-memory store and reports:
  - index speed, node and edge counts, resolver breakdown and the language matrix;
  - on-disk footprint;
  - blast-radius latency and node count for the top PageRank symbol;
  - a **direct reference resolution** diagnostic. This counts distinct direct `(dependent, relation)` pairs naming the symbol, resolved (by the name-binding resolvers) vs unresolved, over one population. It is `null` when the unresolved count cannot be read. It is a resolver-coverage diagnostic, not recall against labelled truth.
- **Memory recall@5 gate** (`--recall`): a lexical recall smoke test with a pass/fail gate (exit 1 below it).
- **Not here:** a golden-set precision/recall scorer, a frozen scenario corpus runner, or an agent A/B runner. The A/B types in `lib.rs` (`EvalReport`, `TaskOutcome`) are aggregation arithmetic with unit tests; nothing executes baseline-vs-treatment agent tasks.

## Usage

```sh
cargo build -p wicked-estate-bench --release

# Capability benchmark of the workspace root (rewrites docs/benchmarks/capability-report.md)
./target/release/wicked-estate-bench

# Specific repos, without rewriting the committed Markdown report; JSON goes to stdout
./target/release/wicked-estate-bench --no-report /path/to/repo-a /path/to/repo-b

# Memory recall gate
./target/release/wicked-estate-bench --recall
```

- An unknown flag or a path that does not exist is a usage error (exit 2).
- A requested repo that fails to benchmark fails the run (exit 1) after the report is printed, so a receipt never silently drops an input.
- "Est. tokens" is a `bytes / 4` payload proxy, not tokenizer billing.

## Gate

`cargo test --workspace` includes the bench's fixture tests (scaled footprint gate, a one-repo non-empty result, the direct-resolution falsifiers).

## Internal only

`publish = false` in Cargo.toml: this crate is never released to crates.io.
