# wicked-estate-bench

Agent-eval benchmark harness for wicked-estate. Internal tooling — not published to crates.io.

## What it does

- Runs a frozen corpus of query scenarios against a live `wicked-estate` graph and scores the results.
- Outputs per-scenario pass/fail, precision, and recall against a hand-verified golden set.
- Acts as the truth oracle for W1.6+ benchmarking: no change to the retrieval or resolution layers may regress the benchmark without an explicit, evidence-backed exception.

## Usage

```sh
# Build the bench binary
cargo build -p wicked-estate-bench --release

# Run against a frozen corpus (from repo root)
./target/release/wicked-estate-bench --corpus bench/corpus/ --db bench/bench.db
```

## Corpus format

Each scenario is a JSON file:
```json
{
  "query": "handleRequest",
  "expected_symbols": ["src/handler.rs::handleRequest"],
  "tool": "SearchEntity"
}
```

## Gate

`cargo test --workspace` includes bench-type tests. The full run requires a corpus that is not committed (large); CI runs the subset in `bench/fixtures/`.

## Internal only

`publish = false` in Cargo.toml — this crate is never released to crates.io.
