# Testing Taxonomy — wicked_estate

This document defines the five test categories for the engine, what each guards,
where the tests live, how to run them, and the gate link from CLAUDE.md §9.

---

## Gate rule (CLAUDE.md §9)

Every change must leave these commands green before it can be considered done:

```bash
cargo build --workspace                                # 0 warnings
cargo test  --workspace                                # all categories below
cargo clippy --workspace --all-targets -- -D warnings  # 0 lint errors
cargo fmt --all                                        # no diff
```

From Wave 1.6 the agent-eval benchmark must also not regress.
No test may be deleted, lowered in assertion strength, or marked `#[ignore]`
to pass this gate — profile and fix the root cause instead.

---

## Category 1: Unit tests

**What it guards:** individual type correctness, constructor invariants, and module-level
logic in isolation. Examples: `Symbol::id()` stability across location changes, hex
round-trips for `TraceId`/`SpanId`, `Edge::dedup_key()` stability, `Confidence::new`
clamping.

**Where it lives:** inline `#[cfg(test)]` modules at the bottom of each source file.
Canonical examples:
- `crates/wicked-estate-core/src/symbol.rs` — ADR-002 identity invariants
- `crates/wicked-estate-core/src/observability.rs` — serde + factory smoke tests
- `crates/wicked-estate-rank/src/lib.rs` — hub topology, dangling nodes, sum-to-one
- `crates/wicked-estate-store/src/lib.rs` — vector storage, compact, edge_history

**How to run:**
```bash
cargo test --workspace
```
Or for a single module:
```bash
cargo test -p wicked-estate-core symbol
```

---

## Category 2: Conformance tests (GraphStore contract)

**What it guards:** every `GraphStore` implementation must satisfy the full
`graph_store_suite` contract: node/edge CRUD, idempotent upsert, the
dependent→dependency edge-direction invariant, bounded reverse-reachability
(blast-radius), `find_symbols`, `unresolved_refs`, `file_digest`, `file_content`,
`symbol_source` byte-slice extraction, `remove_file`, `prune_dangling_edges`,
`file_git_sha` correctness against the `git hash-object` reference value,
`changes_since`, `repo_info`, and `edge_history` archival.

Any `GraphStore` that does not pass this suite is incomplete — it cannot be used
in production. See `docs/adr/ADR-003-storage-backends.md`.

**Where it lives:**
- Suite definition: `crates/wicked-estate-core/src/conformance.rs` (`graph_store_suite<S>`)
- MemStore caller: `crates/wicked-estate-store/src/lib.rs` (not yet wired as a dedicated test
  — tracked as a Wave 1.1 task)
- SQLiteStore caller: `crates/wicked-estate-store/src/sqlite.rs`

**How to run:**
```bash
cargo test --workspace
# Specifically the conformance tests:
cargo test -p wicked-estate-store conformance
```

---

## Category 3: Integration tests (wicked-estate-bench fixture + multi-repo)

**What it guards:** end-to-end pipeline correctness across the full
extract → resolve → store → rank → retrieve stack on real (frozen) corpus fixtures.
Catches regressions that unit tests cannot: cross-crate wiring, schema drift,
edge-direction bugs that only appear at graph scale.

**Where it lives:** `crates/wicked-estate-bench/src/lib.rs` (benchmark harness + frozen corpus).
Fixture corpora are pinned at a fixed commit SHA so results are reproducible.
See `docs/benchmark-methodology.md` for corpus provenance and freezing policy.

**How to run:**
```bash
cargo test -p wicked-estate-bench
# Bench mode (not a gate, but the regression oracle):
cargo bench -p wicked-estate-bench
```

---

## Category 4: Property tests

**What it guards:** invariants that must hold for *every* generated input, not just
hand-crafted examples. Catches edge cases that example tests never consider:
unusual tier combinations, degenerate graphs (empty, single-node, all-dangling),
extreme float inputs, and serde fidelity across all enum variants.

Uses [`proptest`](https://docs.rs/proptest) with bounded strategies (≤30 nodes,
≤60 edges, string lengths ≤32, collection sizes ≤8) and a fixed seed for
deterministic reproduction.

**Where it lives:**
- `crates/wicked-estate-core/tests/property_tests.rs`
- `crates/wicked-estate-rank/tests/property_tests.rs`

**Invariants encoded:**

| ID | Invariant | File | Rule |
|----|-----------|------|------|
| P1 | `Edge::new(src, tgt, kind, tier, resolver)` yields `confidence ∈ (0.0, 1.0]`, finite, and non-empty `resolved_by`, for every `ResolutionTier` variant. | `wicked-estate-core` | CLAUDE.md: "never emit an Edge without confidence+provenance" |
| P1b | `Confidence::new(raw)` clamps any finite f32 into `[0.0, 1.0]`. | `wicked-estate-core` | Encodes the `Confidence` contract |
| P2 | `SymbolId(s).as_str() == s` and `Display` equals `s`. (`Symbol` has `Display` but no `FromStr`; a full `Symbol::parse` round-trip is not applicable — `SymbolId` is the canonical storage key, per ADR-002.) | `wicked-estate-core` | ADR-002 stable identity |
| P3 | Serde round-trip for `AttributeValue` (all 6 variants), `SpanData`, `Metric` (Sum + Gauge), and `LogRecord`: `from_str(&to_string(x)) == x`. | `wicked-estate-core` | OTel data-model fidelity |
| P4 | All global PageRank scores are finite and ≥ 0 for any graph (0–30 nodes, 0–60 edges). | `wicked-estate-rank` | PageRank stochastic invariant |
| P5 | Ranking the same graph twice yields bit-identical scores (determinism). | `wicked-estate-rank` | Reproducibility |
| P6 | Single-node graph → score ≈ 1.0 (within 1e-3). Empty graph → empty map. | `wicked-estate-rank` | Degenerate-graph safety |
| P7 | A seeded node's personalized score ≥ its global score (within tolerance). `SEED_WEIGHT` (≈100×) is a positive bias; it cannot demote the seed. | `wicked-estate-rank` | Personalized PageRank semantic |

**How to run:**
```bash
cargo test --workspace
# Explicitly:
cargo test -p wicked-estate-core  --test property_tests
cargo test -p wicked-estate-rank  --test property_tests
```

To reproduce a specific failing case, set `PROPTEST_CASES` or use the
`proptest-regressions/` files that proptest writes automatically on failure.

---

## Category 5: Regression gate (wicked-estate-bench footprint + speed ceilings)

**What it guards:** the agent-eval benchmark from Wave 1.6 must not regress.
Ceilings are defined in `docs/benchmark-methodology.md`. A regression is a
failure — there is no "we'll fix it later" path; see CLAUDE.md §9.

Two axes:
- **Footprint** — peak resident memory during a full index of the frozen corpus.
- **Speed** — wall-clock time to complete extract + resolve + store + rank, measured
  as the P95 of repeated runs (removes cold-start noise).

**Where it lives:** `crates/wicked-estate-bench/src/lib.rs` (Criterion harness).

**How to run:**
```bash
cargo bench -p wicked-estate-bench
# The benchmark harness emits a PASS/FAIL verdict against the ceiling file.
```

The ceiling file is `docs/benchmarks/ceilings.toml` (tracked in version control).
Any PR that causes the harness to print `REGRESSION` is a NO-GO — not a CONDITIONAL,
not "merge and fix forward". See CLAUDE.md §7 ("The verdict is the verdict").

---

## Adding a new GraphStore implementation

1. Wire `graph_store_suite(&mut your_store)` in a `#[test]` in your crate.
2. Ensure `cargo test -p <your-crate>` passes with 0 warnings before opening a PR.
3. Register the store in `wicked_estate_store::open_store` factory (ADR-003).

## Adding a new language

Languages are data (`crates/wicked-estate-extract/languages.toml` + a `.scm` query file).
No new test infrastructure is required — the parity gate (`≥73 languages`) in the
conformance suite catches regressions automatically.
