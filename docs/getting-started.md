# Getting Started with wicked-estate

**W8.4** — build, index, query, and connect an agent. Real output from v0.0.1 on a two-file
Python repo.

---

## 1. Build

```bash
cargo build --release
```

Produces two binaries:

| Binary | Purpose |
|--------|---------|
| `target/release/wicked-estate` | CLI — index, query, blast-radius, rank, source, stats, scip, semantic, watch, subscribe, compact, tfstate, drift, cross-graph, clusters |
| `target/release/wicked-estate-mcp` | MCP stdio server — 5 retrieval tools for LLM agents |

Zero runtime deps. Single static binary on each target.

---

## 2. Index a repo

```bash
wicked-estate index <path> [--db <file|:memory:>]
```

Defaults to `.wicked-estate/graph.db` in the current directory. The DB file is created if it does
not exist. Use `:memory:` for a throwaway graph.

### Optional flags

| Flag | Behaviour |
|------|-----------|
| `--db <file\|:memory:>` | Override the default DB path (`.wicked-estate/graph.db`). Use `:memory:` for a throwaway graph. |
| `--history` | Opt-in to edge-history archival. Preserves provenance across re-indexes. Default: off (reduces bloat). |
| `--embeddings` | After indexing, compute and store embedding vectors for every symbol. Required before `semantic` search. Default: off. |

**Example — index this repo:**

```
$ wicked-estate index .
indexed . (.wicked-estate/graph.db) → 191 nodes, 244 edges, 22 files
  "calls" = 87
  "contains" = 104
  "imports" = 53
```

**Example — index a small demo:**

```
$ wicked-estate index ./demo --db /tmp/demo.db
indexed ./demo (/tmp/demo.db) → 8 nodes, 11 edges, 2 files
  "calls" = 4
  "contains" = 5
  "extends" = 1
  "imports" = 1
```

### What gets indexed

`wicked-estate index` walks the tree, dispatches each file to the matching tree-sitter extractor
(by extension), runs EXTRACT→RESOLVE, and writes nodes and edges to SQLite. Files whose
extension maps to no wired extractor are silently skipped. Check `wicked-estate stats` after
indexing to confirm coverage.

Wired extractors (as of v0.0.1): **rust**, **python**, **typescript**, **tsx**, **javascript**,
**go**, **java**, **c**, **cpp**, **csharp**, **ruby**, **bash**, **yaml**, **json**, plus
**cloudformation** and **kubernetes** via the `IaCExtractor`. See
`docs/language-coverage-matrix.md` for the full matrix.

---

## 3. Query — find symbols by name

```bash
wicked-estate query <name> [--db ...]
```

Substring match against all indexed symbol names. Returns kind, name, and location.

```
$ wicked-estate query bark --db /tmp/demo.db
1 match(es) for 'bark':
  Function bark (example.py:9)
```

---

## 4. Blast-radius — what depends on a symbol

```bash
wicked-estate blast-radius <name> [--db ...]
```

Returns the set of symbols that transitively depend on `<name>` via resolved `calls`, `imports`,
and `extends` edges. Always prints a coverage line — do not interpret an empty result as "safe
to change" without reading it.

```
$ wicked-estate blast-radius bark --db /tmp/demo.db
3 symbol(s) depend on 'bark':
  File main.py (main.py:1)
  Function speak (example.py:6)
  Function main (main.py:3)
coverage: 3 resolved dependent(s); 0 unresolved call(s) reference 'bark' — best-effort static
resolution, MAY be incomplete (precise tier pending)
```

### Coverage semantics — read this

The coverage line is mandatory (agent-behavior rule R3). It tells you:

- **resolved dependent(s)** — edges the resolver could assign a target. These are the dependents
  the graph *knows about*.
- **unresolved call(s)** — `@call.function` / `@call.method` captures that matched the symbol's
  name in source but whose callers could not be resolved (ambiguous, or no SCIP precise tier).

A non-zero unresolved count means the blast-radius is a lower bound. The `scip` command raises
this to a `confidence:1.0` precise tier for TypeScript/JavaScript repos.

---

## 5. Rank — most important symbols (PageRank)

```bash
wicked-estate rank [--db ...]
```

Returns the top 25 symbols by PageRank over the call/import graph. Use this to understand which
symbols are most load-bearing in a repo.

```
$ wicked-estate rank --db /tmp/demo.db
top 8 symbols by PageRank:
  0.2163  Function bark (example.py:9)
  0.1411  Class Dog (example.py:5)
  0.1411  Function speak (example.py:6)
  0.1235  Import example (main.py:1)
  0.1235  Function main (main.py:3)
  0.0848  File example.py (example.py:1)
  0.0848  File main.py (main.py:1)
  0.0848  Class Animal (example.py:1)
```

---

## 6. Source — print the source slice for a symbol

```bash
wicked-estate source <name> [--db ...]
```

Fetches the source text stored at index time for each matching symbol. Useful for reading a
function without opening the file.

```
$ wicked-estate source bark --db /tmp/demo.db
1 match(es) for 'bark':
  [Function] bark @ example.py:9
def bark():
    return "woof"
```

If the source column is empty (DB indexed before source storage was wired), re-run `index`.

---

## 7. Stats — graph summary

```bash
wicked-estate stats [--db ...]
```

```
$ wicked-estate stats --db /tmp/demo.db
nodes=8 edges=11 files=2
  edge "calls" = 4
  edge "contains" = 5
  edge "extends" = 1
  edge "imports" = 1
```

---

## 8. SCIP — precise resolution for TypeScript/JavaScript

```bash
wicked-estate scip <root> [--db ...] [--scip-file <path>]
```

Ingests a SCIP index (`index.scip`) produced by `scip-typescript` (or another SCIP indexer)
and promotes matching edges to `confidence:1.0, source:scip`. Run `index` first, then `scip`.

```bash
# First: tree-sitter extraction
wicked-estate index ./my-ts-project

# Then: precise SCIP tier (auto-runs npx scip-typescript if index.scip absent)
wicked-estate scip ./my-ts-project
# notice: ./my-ts-project/index.scip not found — attempting: npx @sourcegraph/scip-typescript@0.4.0 index
# scip: ingested 412 precise edge(s) from ./my-ts-project/index.scip into .wicked-estate/graph.db
```

After `scip`, blast-radius results for TypeScript symbols carry `confidence:1.0` edges and
the unresolved-call count should drop significantly.

**Requirements:** Node.js + `npx` available. The project must have `tsconfig.json` and
`node_modules` installed. If `npx` is not available, run
`npx @sourcegraph/scip-typescript@0.4.0 index` manually in the project root, then re-run
`wicked-estate scip`.

---

## 9. Incremental re-indexing

Re-running `wicked-estate index` on an already-indexed path is safe and fast. The indexer uses
xxh3 content hashes to detect changed files and re-extracts only those files plus their direct
importers. Unchanged files are not re-parsed.

```bash
# Edit a file, then re-index — only the changed file + its importers are re-processed
wicked-estate index . --db .wicked-estate/graph.db
```

The DB is written transactionally; a kill -9 mid-index leaves the graph in the last committed
state (no corruption).

---

## 10. MCP server — connect an LLM agent

The `wicked-estate-mcp` binary is an MCP stdio server (JSON-RPC 2.0 over stdin/stdout).

```bash
wicked-estate-mcp --db /path/to/graph.db
# or: wicked-estate-mcp                   # defaults to .wicked-estate/graph.db
# or: WICKED_ESTATE_DB=:memory: wicked-estate-mcp
```

### The 5 tools

| Tool | Description |
|------|-------------|
| `SearchEntity` | Search symbols by name (substring/BM25). Required: `name`. Optional: `limit` (default 20, max 100). |
| `RetrieveEntity` | Fetch full node details by stable symbol ID. Required: `symbol`. |
| `TraverseGraph` | Multi-hop graph traversal from a symbol. Required: `symbol`. Optional: `depth` (default 4, max 16), `direction` (`dependencies`/`dependents`/`both`), `edge_kinds`, `max_nodes` (default 200, max 1000). |
| `BlastRadius` | Enumerate all transitive dependents of a symbol. Required: `symbol`. Optional: `depth` (default 8, max 24). |
| `FetchContent` | Retrieve the source text stored for a symbol. Required: `symbol`. |

All tools honor the agent-behavior rules from `docs/agent-behavior-rules.md`:

- **R1** — Never returns `isError: true` for a missing symbol (returns empty result + diagnostic
  instead). An early `isError` causes session-wide abandonment.
- **R3** — Coverage gaps are always surfaced in `diagnostics` ("graph covers X; Y files not
  indexed").
- **R4** — Output is capped at ~25K characters.
- **R7** — Low-confidence edges (heuristic, <1.0) are labeled in output so the agent weights
  them appropriately. Look for `R7-CONFIDENCE` in diagnostics.

### Connecting from Claude / an MCP client

Full per-client setup (Claude Code, Cursor, Antigravity, Codex): **[mcp-integration.md](./mcp-integration.md)**.
Quick example — in your client's `mcpServers` config:

```json
{
  "mcpServers": {
    "wicked-estate": {
      "command": "/path/to/wicked-estate-mcp",
      "args": ["--db", "/path/to/repo/.wicked-estate/graph.db"]
    }
  }
}
```

Or with the env-var form:

```json
{
  "mcpServers": {
    "wicked-estate": {
      "command": "/path/to/wicked-estate-mcp",
      "env": { "WICKED_ESTATE_DB": "/path/to/repo/.wicked-estate/graph.db" }
    }
  }
}
```

The server advertises itself as `wicked-estate` with `protocolVersion: "2024-11-05"`.

### Typical agent workflow

```
1. wicked-estate index <repo>          # build the graph (one-time or incremental)
2. Launch wicked-estate-mcp            # agent connects
3. Agent calls SearchEntity("parse")  → finds candidate symbols
4. Agent calls RetrieveEntity(id)     → full node + edges
5. Agent calls BlastRadius(id)        → what breaks if I change this?
6. Agent calls FetchContent(id)       → read the source slice
```

---

## 11. Semantic search — embedding-based symbol lookup

```bash
wicked-estate semantic <query> [--db ...]
```

Finds symbols semantically similar to a natural-language or code query using embedding vectors.
Requires `--embeddings` to have been passed during a prior `index` run.

```
$ wicked-estate semantic "database connection handler" --db .wicked-estate/graph.db
3 semantic match(es) for 'database connection handler':
  [0.921] Function open_store (wicked-estate-store/src/sqlite.rs:42)
  [0.887] Function connect (wicked-estate-store/src/lib.rs:18)
  [0.831] Struct SqliteStore (wicked-estate-store/src/sqlite.rs:55)
note: embeddings are hash-based (HashEmbedder); results rank structural name similarity
```

If no embeddings are stored, the command returns zero results with a diagnostic note.

---

## 12. Watch — reactive re-index on file changes

```bash
wicked-estate watch <path> [--db ...] [--history]
```

Performs an initial full index of `<path>`, then watches the tree recursively with a 500ms
debounced watcher. Any create/modify/remove event triggers an incremental re-index. Runs until
Ctrl-C.

```
$ wicked-estate watch ./src
watch: initial index of ./src → 340 nodes, 512 edges, 28 files
watch: watching ./src — press Ctrl-C to stop
# (edit a file)
watch: re-indexed → 341 nodes, 515 edges, 28 files
```

`--history` opts in to edge-history archival for the session (same semantics as `index --history`).

---

## 13. Subscribe — one-shot change-log poll

```bash
wicked-estate subscribe [--db ...] [--since <seq>]
```

Prints all change-log entries since `<seq>` as JSON lines, then exits. The final line reports the
new high-watermark sequence number so the caller can resume without re-reading old entries.

```
$ wicked-estate subscribe --db .wicked-estate/graph.db --since 0
{"seq":1,"op":"upsert","target":"src/lib.rs"}
{"seq":2,"op":"upsert","target":"src/main.rs"}
{"next_seq":2}

# On the next poll, pass --since 2 to get only new changes.
$ wicked-estate subscribe --db .wicked-estate/graph.db --since 2
{"next_seq":2}
```

`op` is `"upsert"` or `"remove"`. Use this for lightweight event-driven tooling (CI hooks,
editor plugins) rather than polling `stats`.

---

## 14. Compact — prune cruft and vacuum the database

```bash
wicked-estate compact [--db <file>]
```

Removes dangling edges (edges whose source or target node no longer exists), stale cache rows,
orphaned embedding vectors, and orphaned content rows. Then checkpoints the WAL and runs
`VACUUM`. Not applicable to `:memory:` databases.

```
$ wicked-estate compact
compact(.wicked-estate/graph.db):
  dangling edges pruned:    12
  stale cache rows pruned:  4
  orphan embeddings pruned: 0
  orphan content rows pruned: 3
WAL checkpointed and VACUUM complete.
```

Run `compact` periodically on long-lived databases or after mass deletions to reclaim disk space.

---

## 15. IaC — index Terraform state

```bash
wicked-estate tfstate <file.tfstate> [--db ...]
```

Ingests a Terraform state file and creates `NodeKind::Other("resource")` nodes tagged
`origin=live` into the graph. Used to populate the "live" side of the estate for `drift` analysis.

```
$ wicked-estate tfstate terraform.tfstate
tfstate: upserted 34 live resource node(s) from 'terraform.tfstate' into .wicked-estate/graph.db
```

Run `wicked-estate index` on the IaC source files first (to populate the `origin=iac` side), then
`tfstate` to add the live side, then `drift` to compare them.

---

## 16. Drift — IaC vs live estate comparison

```bash
wicked-estate drift [--db ...]
```

Diffs the indexed IaC declarations (`origin=iac`) against live resource nodes (`origin=live`).
Reports managed (both sides present), undeployed (IaC only), and unmanaged (live only) resources.

```
$ wicked-estate drift
--- estate drift report ---
managed (iac + live):   28
undeployed (iac-only):  3
unmanaged (live-only):  1

UNMANAGED resources (live, no IaC declaration):
  aws_s3_bucket.manual-backup (terraform.tfstate)

UNDEPLOYED resources (IaC-declared, not in live state):
  aws_lambda_function.new-processor (infra/lambda.tf)
  ...
```

---

## 17. Cross-graph — federated search across multiple repos

```bash
wicked-estate cross-graph <name> --db <a.db> --db <b.db> [--db <c.db> ...]
# or: --dbs a.db,b.db,c.db
```

Searches for `<name>` across all specified per-repo databases and reports matching symbols and
their transitive dependents (blast-radius) per repo.

```
$ wicked-estate cross-graph open_store --db api/.wicked-estate/graph.db --db lib/.wicked-estate/graph.db
=== cross-graph search: 'open_store' across 2 repo(s) ===
2 match(es) total:

  [repo: api/.wicked-estate/graph.db]
    Function open_store (wicked-estate-store/src/lib.rs:18)

  [repo: lib/.wicked-estate/graph.db]
    Function open_store (wicked-estate-store/src/sqlite.rs:42)

=== cross-graph blast-radius: 'open_store' dependents ===
...
NOTE: cross-repo matching is by symbol name only. Cross-repo EDGES are not
resolved — each repo's graph contains only intra-repo edges.
```

Cross-repo matching is name-based; package-aware cross-repo edge resolution is a future step.

---

## 18. PostgreSQL backend

The default storage backend is SQLite (`:memory:` or a file). For shared-team graphs or
environments where multiple writers need concurrent access, build with the `postgres` feature and
point `--db` at a PostgreSQL connection string.

```bash
# Build with Postgres support
cargo build --release --features postgres

# Use any postgres:// or postgresql:// URL as the --db value
wicked-estate index . --db postgres://user:pass@localhost/wicked_graph
wicked-estate query handleRequest --db postgres://user:pass@localhost/wicked_graph
wicked-estate blast-radius handleRequest --db postgres://user:pass@localhost/wicked_graph
```

The schema is created automatically on first use. All CLI commands that accept `--db` accept a
Postgres URL.

### Capabilities

| Capability | SQLite | PostgreSQL |
|---|---|---|
| `shared_writers` | no (WAL gives concurrent readers, single writer) | **yes** — multiple processes can write concurrently |
| `server_side_traversal` | no | **yes** — `WITH RECURSIVE` CTE in-DB |
| `full_text_search` | yes (FTS5) | **yes** (ILIKE) |

### Running the conformance suite

```bash
TEST_POSTGRES_URL=postgres://user:pass@localhost/wicked_test \
  cargo test -p wicked-estate-store --features postgres
```

If `TEST_POSTGRES_URL` is not set the Postgres conformance test skips gracefully; it does not
fail the default `cargo test --workspace`.

---

## 19. OpenTelemetry instrumentation

wicked-estate emits spans and metrics via the `wicked-estate-observe` crate's `OtlpSink` — a
blocking OTLP HTTP/JSON exporter that fires best-effort and **never aborts the main operation**
if the collector is unreachable.

### Enable it

```bash
# Send telemetry to a local collector (e.g. Grafana Alloy, Jaeger, Honeycomb ingest)
export WICKED_OTEL_ENDPOINT=http://localhost:4318

# Optional: extra headers (e.g. auth token)
export WICKED_OTEL_HEADERS="x-honeycomb-team=my-api-key"

# Now run any command — spans and metrics flow automatically
wicked-estate index .
wicked-estate-mcp --db graph.db
```

If `WICKED_OTEL_ENDPOINT` is not set, the sink is a no-op `NoopSink` — zero overhead, no
network traffic.

### What is emitted

| Emission site | Signal |
|---|---|
| `index_path` (full + incremental) | Span: file count, node/edge counts, duration |
| `extract` per-file | Counter: files extracted by language |
| `resolve` pass | Counter: refs resolved / unresolved, tier breakdown |
| MCP tool calls | Span: tool name, result size, cache hit/miss |
| MCP cache | Metric: hit rate gauge |
| `GraphStore::store_fts` | Metric: FTS rebuild latency |

The service name reported to the collector is `wicked_estate`.

### Custom sink (Rust API)

```rust
use wicked_estate_observe::{open_otlp_sink, init_sink_from_env};

// From env vars (production)
let sink = init_sink_from_env();

// Explicit construction
let sink = open_otlp_sink("http://localhost:4318", &[("x-api-key", "tok")]);
```

---

## 20. Cloud collectors (AWS · Azure · GCP)

Live cloud state can be pulled directly from cloud provider APIs into the estate graph, feeding
the same `drift` analysis as `tfstate` but without a local state file. Collectors are
**observe-only** — they read resource metadata, never write to your cloud, and never persist
credentials.

### Build

```bash
# Individual provider flags
cargo build --release --features cloud-aws
cargo build --release --features cloud-azure
cargo build --release --features cloud-gcp    # stub — returns Err; see below

# All at once
cargo build --release --features cloud-all
```

Cloud features are opt-in and have no effect on the default binary.

### AWS (`cloud-aws`)

Uses the standard AWS credential chain (env vars → `~/.aws/credentials` → IAM role). Calls
**Resource Explorer v2** (primary — requires the explorer to be enabled in your account) plus
EC2 and IAM supplemental APIs to resolve resource properties and types.

Resource types are normalized to `aws_ec2_instance`, `aws_s3_bucket`, etc. (lower-snake, no
`AWS::` prefix).

**Prerequisites:** `aws configure` or `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_REGION`
in environment.

### Azure (`cloud-azure`)

Uses `azure_identity` (DefaultAzureCredential — env, CLI, managed identity). Queries the Azure
Resource Graph `resources` table to enumerate all resources in accessible subscriptions.

**Prerequisites:** `az login` or `AZURE_CLIENT_ID` / `AZURE_CLIENT_SECRET` / `AZURE_TENANT_ID`.

### GCP (`cloud-gcp`) — stub

The `GcpCollector` compiles and passes its type tests, but returns `Err` at runtime. Full
google-cloud-asset-v1 API wiring is pending a crate API migration. The seam is in place; the
full implementation is Wave 9/10.

### Programmatic usage

```rust
use wicked_estate_extract::cloud::open_cloud_collector;

let collector = open_cloud_collector("aws")?;  // or "azure", "gcp"
let nodes = collector.collect()?;
// nodes are NodeKind::Other("resource") with origin=live
// — pass to GraphStore to populate the live side for drift analysis
```

CLI integration (`wicked-estate collect-live --cloud aws`) is a Wave 9 task; for now use
`wicked-estate tfstate` to ingest a downloaded state file.

---

## 21. Clusters — community detection and semantic grouping

```bash
wicked-estate clusters [<min-size>] [--json] [--db ...]
    [--resolution <γ>] [--hierarchical] [--package-bias <f>]
    [--weight semantic [--k <n> | --eps <d> --min-pts <n>]]
```

Partitions the indexed graph into communities of related symbols. Two modes are available:

### Graph mode (default)

Multi-level Louvain over `CALLS` and `IMPORTS` edges. Unlike connected components (which collapse
any connected graph into one giant community), Louvain maximises partition modularity `Q`, so a
codebase that is connected but has cluster structure — two cliques joined by one bridge — is split
into the clusters. A good partition scores modularity `> 0.3`.

| Flag | Default | Effect |
|------|---------|--------|
| `--resolution <γ>` | `1.0` | `> 1.0` → finer, smaller communities; `< 1.0` → coarser. Raise γ to break up a too-large community. |
| `--hierarchical` | off | After the flat partition, re-runs Louvain at `resolution × 2.0` inside each community that still has genuine internal substructure. Cohesive communities (cliques) are left whole. |
| `--package-bias <f>` | `0.0` | Adds synthetic same-directory edges weighted at `f × median-real-edge-weight`, so directory structure biases — but does not force — the partition. Useful when imports alone understate package cohesion. |

**Worked example — this repo:**

```
$ wicked-estate clusters 3 --db .wicked-estate/graph.db
127 communities (graph, min_size=3, modularity=0.805):
  cluster 1: 48 symbols
    crates/wicked-estate-store/src/sqlite.rs::SqliteStore
    crates/wicked-estate-store/src/sqlite.rs::open_store
    crates/wicked-estate-store/src/sqlite.rs::compact
    crates/wicked-estate-store/src/sqlite.rs::upsert_nodes
    crates/wicked-estate-store/src/sqlite.rs::upsert_edges
    ... and 43 more
  cluster 2: 31 symbols
  ...
```

Modularity `0.805` is well above the `> 0.3` healthy threshold.

**Break up a large community:**

```bash
# Default run produces a 200-symbol mega-cluster in the store module → raise γ
wicked-estate clusters --resolution 1.8 --hierarchical --db .wicked-estate/graph.db
```

### Semantic mode

```bash
wicked-estate clusters --weight semantic [--db ...] \
    [--eps <d>] [--min-pts <n>]     # DBSCAN (default)
    [--k <n>]                        # k-means instead
```

Groups symbols by **embedding proximity** rather than call-graph structure. This bridges
vocabularies the call graph misses — a Kafka producer and a Pulsar producer embed near each other
even with zero shared edges.

| Flag | Default | Effect |
|------|---------|--------|
| `--eps <d>` | `0.25` | DBSCAN neighbourhood radius in cosine-distance space (`0.0`–`2.0`). |
| `--min-pts <n>` | `3` | DBSCAN: minimum points to form a dense region. Points below the threshold are noise and excluded from output. |
| `--k <n>` | — | Switch to k-means with exactly `k` clusters. When `--k` is present, `--eps` and `--min-pts` are ignored. |

**Requires:** an `--embeddings` index (pass `--embeddings` during `index`).

**Quality warning:** meaningful semantic clusters require the `fastembed` build feature. Without it
the binary falls back to `HashEmbedder` — a bag-of-words hash — and the resulting vectors are noise.
The CLI prints a note when embeddings are absent:

```
note: no embeddings found — re-index with `--embeddings` (build with the `fastembed` feature
for semantic quality) before `clusters --weight semantic`.
```

**Example:**

```bash
# Index with embeddings, then cluster semantically
wicked-estate index . --embeddings
wicked-estate clusters --weight semantic --eps 0.2 --min-pts 2 --db .wicked-estate/graph.db
# 43 clusters (semantic, min_size=2):
#   cluster 1: 18 symbols
#   cluster 2: 14 symbols
#   ...
```

### Common flags

| Flag | Effect |
|------|--------|
| `<min-size>` | Drop communities smaller than this. Default: `2`. |
| `--json` | Emit a JSON array-of-arrays of `SymbolId` strings for machine consumption instead of the human-readable summary. |

**Machine output:**

```bash
wicked-estate clusters 5 --json --db .wicked-estate/graph.db | jq 'length'
# 127
```

### Heavy-repo validation

`scripts/community-validation/` contains `validate-clusters.sh` for benchmarking partition quality
on large repos. Pass a repo path and optional resolution to compare modularity across parameter
settings before committing a γ value to your workflow.

---

## 22. Next steps

- **Add a language** — see `docs/add-lang.md`. Zero core changes required.
- **Extractor SDK** — `docs/extractor-sdk.md` (add-lang + `ExtraEdgeExtractor` for non-code edges).
- **Language coverage** — `docs/language-coverage-matrix.md` (auto-generated from
  `languages.toml` + `LANG_TABLE`).
- **Engine contract** — `docs/ENGINE-CONTRACT.md` (edge directions, symbol identity, agent rules).
- **Wave plan** — `docs/plan/WAVE-PLAN.md` (what is done, what is next).
