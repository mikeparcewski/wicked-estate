# wicked-estate-mcp

MCP stdio server that exposes the wicked-estate retrieval tools to LLM agents over JSON-RPC 2.0.

## What it does

- Implements the MCP stdio transport (newline-delimited JSON-RPC 2.0) by hand — synchronous, no async overhead on a local stdio server.
- Routes `tools/list` and `tools/call` via unified dispatch across **24 tools**: 11 estate tools (in `wicked-estate-retrieve`, incl. `rules.recall`), 6 memory tools (`src/tools/memory.rs`), and 7 knowledge tools (`src/tools/knowledge.rs`).
- Injects a W7.4 staleness diagnostic (`STALENESS: commits_behind=N`) into every `tools/call` response when the server can determine commits landed since the last index run.
- Advertises `SemanticSearch` in `tools/list` only when a `VectorStore` is wired in at startup.
- `handle_request` and `handle_request_ctx` are pure functions (no I/O) so all routing logic is fully unit-tested without a running server.

## Key types / traits

| Item | Description |
|---|---|
| `handle_request(store, req)` | Route one JSON-RPC 2.0 request; returns a `serde_json::Value`. Pure, no I/O. |
| `handle_request_ctx(store, req, ctx)` | Like `handle_request` but injects `McpContext` (staleness + dim-guard); no live SemanticSearch. |
| `handle_request_with_semantic(store, req, ctx, semantic)` | Full routing — injects context **and** the live `SemanticSearch` tool (the serving loop uses this). |
| `McpContext` | Carries `commits_behind` plus the four `embedder_*` dim-guard fields (runtime vs store-meta id/dim). |
| `all_tools()` | Returns the ten always-on estate `RetrievalTool` instances (no `SemanticSearch` — it is stateful). |
| `DomainHandles` | Bundles the memory engine and knowledge engine handles passed into the unified dispatch loop. |
| `src/tools/memory.rs` | Dispatch path for the 6 memory tools (`memory.capture/recall/reflect/erase/learn/coverage`). |
| `src/tools/knowledge.rs` | Dispatch path for the 7 knowledge tools (`knowledge.ingest/write/relate/recall/coverage/relate_code/recall_about_code`). |
| `live_semantic_search(vec_store)` | Builds the live `SemanticSearch` backed by `default_embedder()` + the supplied `VectorStore`. |
| `input_schema(name)` | Returns the JSON Schema `inputSchema` for a named tool; used by `tools/list`. |

## Usage

```rust
use wicked_estate_mcp::{handle_request_ctx, McpContext};

let ctx = McpContext {
    commits_behind: None,
    // Dim-guard: SemanticSearch is advertised/dispatched only when the store's recorded
    // embedder identity + dim match the runtime embedder. All-None = fail closed (no semantic).
    ..Default::default()
};
// Per-line JSON-RPC loop (no live semantic tool):
let response = handle_request_ctx(&store, &request_value, &ctx);
if !response.is_null() {
    println!("{}", response);
}
```

## Crate features

No optional feature flags. The `wicked-estate-mcp` binary is built from `src/main.rs`; the library surface is the pure routing logic in `src/lib.rs`.

Part of **[wicked-estate](https://github.com/mikeparcewski/wicked-estate)** — a code + infrastructure
estate graph for LLM agents (definitions, who-calls-X, blast-radius, scoped context). Local-first,
tree-sitter + SQLite, single static binary. See the umbrella [`wicked-estate`](https://crates.io/crates/wicked-estate)
crate to use the whole thing.

MIT licensed.
