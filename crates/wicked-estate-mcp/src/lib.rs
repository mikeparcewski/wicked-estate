//! `wicked-estate-mcp` — minimal, correct MCP stdio server over JSON-RPC 2.0 (Wave 2.5, 4.3).
//!
//! # Design choice: hand-rolled vs `rmcp`
//!
//! [`rmcp`](https://crates.io/crates/rmcp) v1.7 is async-first (tokio). Our `GraphRead` trait
//! is **synchronous** — wrapping every call in `block_on` or `spawn_blocking` adds noise for zero
//! benefit on a local stdio server. The MCP stdio transport is ~150 lines of
//! newline-delimited JSON-RPC 2.0, so we hand-roll it here. The division of labour is:
//!
//! * [`handle_request`] — pure, testable, owns all routing logic. Zero I/O.
//! * `main.rs` — the read-stdin / write-stdout loop. Opens the store once at startup.
//!
//! # Protocol subset implemented
//!
//! | Method                     | Behaviour |
//! |----------------------------|-----------|
//! | `initialize`               | Returns `protocolVersion`, `capabilities`, `serverInfo`. |
//! | `notifications/initialized`| No-op (notification — no `id`). |
//! | `tools/list`               | Returns the [`wicked_estate_retrieve`] tools with JSON Schema. |
//! | `tools/call`               | Dispatches to the matching tool; wraps result in MCP envelope. |
//! | *(anything else)*          | JSON-RPC error `-32601` (Method Not Found). |
//!
//! # W7.4 Staleness
//!
//! Tool responses include a `STALENESS: commits_behind=N` diagnostic line when the
//! server can determine that commits have landed since the last index. The server
//! computes this once at startup via `wicked_estate::commits_behind`.

use serde_json::{Value, json};
use wicked_estate_core::{GraphRead, RetrievalTool};
use wicked_estate_knowledge::KnowledgeApi;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_retrieve::{
    BlastRadius, Communities, ContextBundle, FetchContent, Lineage, RankHotspots, RetrieveEntity,
    RulesInventory, SearchEntity, SemanticSearch, TraverseGraph,
};

pub mod resources;
pub mod tools;

// ─────────────────────────────────────────────────────────────────────────────
// Crate version (injected by Cargo at compile time)
// ─────────────────────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─────────────────────────────────────────────────────────────────────────────
// Tool registry
// ─────────────────────────────────────────────────────────────────────────────

/// All always-on retrieval tools in declaration order — the **10 unconditional read tools** (the
/// DoD-A4 floor): the original 7 plus the promoted `RankHotspots`, `Communities`, and `Lineage`
/// (each a real `RetrievalTool` over the read-only `&dyn GraphRead` surface; `Lineage` already
/// existed in `wicked-estate-retrieve` but was absent here — C-A3).
///
/// SemanticSearch is **not** here — it is stateful (owns a `VectorStore` connection), so it cannot
/// be a zero-sized entry rebuilt per request. It is constructed once at startup via
/// [`live_semantic_search`] and threaded into [`handle_request_with_semantic`], which merges it
/// with this list (when the dim-guard passes) to form the live dispatch registry. So `tools/list`
/// returns **10 or 11** tools: the 10 here unconditionally, +1 when semantic is available.
///
/// `Annotate` is intentionally **absent**: the v1 MCP surface is read-only (write is CLI-only,
/// design §2.2/§2.3), so no mutating tool appears in `tools/list`.
pub fn all_tools() -> Vec<Box<dyn RetrievalTool>> {
    vec![
        Box::new(SearchEntity),
        Box::new(RetrieveEntity),
        Box::new(TraverseGraph),
        Box::new(BlastRadius),
        Box::new(FetchContent),
        Box::new(ContextBundle),
        Box::new(RulesInventory),
        Box::new(RankHotspots),
        Box::new(Communities),
        Box::new(Lineage),
    ]
}

/// Build the **live** SemanticSearch tool backed by the real tiered embedder
/// (`default_embedder()` — FastEmbed → model2vec → hash), not a hardcoded hash embedder.
///
/// `vec_store` is the concrete store used for `nearest` vector lookups (a second `SqliteStore`
/// handle opened against the same DB). The same DB is passed to `invoke` as `&dyn GraphRead` for
/// node resolution. Built **once** at server start and shared across requests (it holds a DB
/// connection behind a `Mutex`, so it is neither cloneable nor cheap to rebuild). When
/// `default_embedder()` falls back to the lexical `HashEmbedder`, [`handle_tools_call_ctx`]
/// surfaces a per-call `LEXICAL-FALLBACK:` diagnostic (DoD-A6b).
pub fn live_semantic_search(
    vec_store: impl wicked_estate_retrieve::VectorStore + 'static,
) -> SemanticSearch {
    SemanticSearch::new(wicked_estate::default_embedder(), vec_store)
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON Schema definitions for each tool's input
//
// The MCP `tools/list` response requires an `inputSchema` that describes the
// tool's request object.  We derive these directly from the documented request
// shapes in `wicked-estate-retrieve/src/lib.rs` — the source of truth.
// ─────────────────────────────────────────────────────────────────────────────

fn search_entity_schema() -> Value {
    json!({
        "type": "object",
        "required": ["name"],
        "properties": {
            "name": {
                "type": "string",
                "description": "Symbol name to search for (exact or substring match)."
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results (default 20, max 100).",
                "default": 20,
                "maximum": 100
            }
        },
        "additionalProperties": false
    })
}

fn retrieve_entity_schema() -> Value {
    json!({
        "type": "object",
        "required": ["symbol"],
        "properties": {
            "symbol": {
                "type": "string",
                "description": "Stable symbol ID to retrieve."
            }
        },
        "additionalProperties": false
    })
}

fn traverse_graph_schema() -> Value {
    json!({
        "type": "object",
        "required": ["symbol"],
        "properties": {
            "symbol": {
                "type": "string",
                "description": "Start symbol ID for the traversal."
            },
            "depth": {
                "type": "integer",
                "description": "Maximum hop depth (default 4, max 16).",
                "default": 4,
                "maximum": 16
            },
            "direction": {
                "type": "string",
                "enum": ["dependencies", "dependents", "both"],
                "description": "Traversal direction (default: dependencies).",
                "default": "dependencies"
            },
            "edge_kinds": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Edge kinds to follow (empty = all kinds)."
            },
            "max_nodes": {
                "type": "integer",
                "description": "Node cap before truncation (default 200, max 1000).",
                "default": 200,
                "maximum": 1000
            }
        },
        "additionalProperties": false
    })
}

fn blast_radius_schema() -> Value {
    json!({
        "type": "object",
        "required": ["symbol"],
        "properties": {
            "symbol": {
                "type": "string",
                "description": "Symbol ID whose transitive dependents to enumerate."
            },
            "depth": {
                "type": "integer",
                "description": "Maximum hop depth (default 8, max 24).",
                "default": 8,
                "maximum": 24
            }
        },
        "additionalProperties": false
    })
}

fn fetch_content_schema() -> Value {
    json!({
        "type": "object",
        "required": ["symbol"],
        "properties": {
            "symbol": {
                "type": "string",
                "description": "Stable symbol ID whose source slice to fetch."
            }
        },
        "additionalProperties": false
    })
}

fn context_bundle_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": {
                "type": "string",
                "description": "Stable symbol ID of the seed (alternative to 'query')."
            },
            "query": {
                "type": "string",
                "description": "Name / FTS text to resolve the seed (alternative to 'symbol'); the top hit is used."
            },
            "budget": {
                "type": "integer",
                "description": "Character budget for the packed context (default 8000, hard-capped below the ~25K agent limit).",
                "default": 8000,
                "maximum": 24000
            }
        },
        "additionalProperties": false
    })
}

fn semantic_search_schema() -> Value {
    json!({
        "type": "object",
        "required": ["query"],
        "properties": {
            "query": {
                "type": "string",
                "description": "Natural-language query to embed and search by cosine similarity."
            },
            "k": {
                "type": "integer",
                "description": "Number of nearest results (default 10, max 100).",
                "default": 10,
                "maximum": 100
            }
        },
        "additionalProperties": false
    })
}

fn rules_inventory_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn lineage_schema() -> Value {
    json!({
        "type": "object",
        "required": ["symbol"],
        "properties": {
            "symbol": {
                "type": "string",
                "description": "Symbol ID whose transitive dependencies to enumerate."
            },
            "depth": {
                "type": "integer",
                "description": "Maximum hop depth (default 8, max 24).",
                "default": 8,
                "maximum": 24
            }
        },
        "additionalProperties": false
    })
}

fn rank_hotspots_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "description": "How many top-ranked symbols to return (default 20, max 200).",
                "default": 20,
                "maximum": 200
            },
            "seeds": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional symbol IDs to bias the ranking toward (personalized PageRank). Omit for global PageRank."
            }
        },
        "additionalProperties": false
    })
}

fn communities_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "description": "How many communities to return, largest first (default 20, max 200).",
                "default": 20,
                "maximum": 200
            },
            "min_size": {
                "type": "integer",
                "description": "Drop communities smaller than this (default 2).",
                "default": 2
            },
            "resolution": {
                "type": "number",
                "description": "Louvain resolution γ (default 1.0; > 1.0 yields smaller, tighter communities).",
                "default": 1.0
            }
        },
        "additionalProperties": false
    })
}

/// Returns the `inputSchema` for a given tool name.  Returns `None` when
/// the name is unknown (the caller treats this as an unregistered tool).
pub fn input_schema(name: &str) -> Option<Value> {
    match name {
        "SearchEntity" => Some(search_entity_schema()),
        "RetrieveEntity" => Some(retrieve_entity_schema()),
        "TraverseGraph" => Some(traverse_graph_schema()),
        "BlastRadius" => Some(blast_radius_schema()),
        "FetchContent" => Some(fetch_content_schema()),
        "ContextBundle" => Some(context_bundle_schema()),
        "SemanticSearch" => Some(semantic_search_schema()),
        "RulesInventory" => Some(rules_inventory_schema()),
        "RankHotspots" => Some(rank_hotspots_schema()),
        "Communities" => Some(communities_schema()),
        "Lineage" => Some(lineage_schema()),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON-RPC helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a JSON-RPC 2.0 success response.
fn ok_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC 2.0 error response.
fn err_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Method handlers
// ─────────────────────────────────────────────────────────────────────────────

fn handle_initialize(id: &Value) -> Value {
    ok_response(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "wicked-estate",
                "version": SERVER_VERSION
            }
        }),
    )
}

fn handle_tools_list_ctx(
    id: &Value,
    ctx: &McpContext,
    semantic: Option<&dyn RetrievalTool>,
) -> Value {
    let base_tools = all_tools();
    let mut tools: Vec<Value> = base_tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name(),
                "description": t.description(),
                "inputSchema": input_schema(t.name()).unwrap_or(json!({"type": "object"}))
            })
        })
        .collect();

    // Advertise SemanticSearch ONLY when the live tool is wired AND the dim-guard passes
    // (store embedder identity + dim match the runtime). A mismatch / missing-meta store leaves
    // it absent — honest absence, not advertised-but-silently-empty (DoD-A6a).
    if let Some(tool) = semantic {
        if semantic_advert(ctx).is_ok() {
            tools.push(json!({
                "name": tool.name(),
                "description": tool.description(),
                "inputSchema": input_schema(tool.name()).unwrap_or(json!({"type": "object"}))
            }));
        }
    }

    ok_response(id, json!({ "tools": tools }))
}

fn handle_tools_call_ctx(
    id: &Value,
    params: &Value,
    store: &dyn GraphRead,
    ctx: &McpContext,
    semantic: Option<&dyn RetrievalTool>,
) -> Value {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return err_response(id, -32602, "tools/call: 'name' parameter is required");
        }
    };

    // Default arguments to empty object if omitted.
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    // Dispatch registry = the always-on lexical/graph tools PLUS the LIVE SemanticSearch when it is
    // wired and the dim-guard passes. Resolving against this (not bare `all_tools()`, which excludes
    // SemanticSearch) is the DoD-A6b fix: a list→call for SemanticSearch now reaches a real instance
    // built with the runtime store + embedder. When the guard fails, semantic stays unadvertised
    // (handle_tools_list_ctx) AND unknown here — consistent fail-closed.
    let base_tools = all_tools();
    let live_semantic = semantic.filter(|_| semantic_advert(ctx).is_ok());
    let tool: &dyn RetrievalTool = match base_tools.iter().find(|t| t.name() == tool_name) {
        Some(t) => t.as_ref(),
        None => match live_semantic.filter(|t| t.name() == tool_name) {
            Some(t) => t,
            None => {
                return err_response(
                    id,
                    -32602,
                    &format!("tools/call: unknown tool '{tool_name}'"),
                );
            }
        },
    };

    match tool.invoke(store, &arguments) {
        Ok(result) => {
            // Build the MCP tool-result content array.
            let mut content_text = match serde_json::to_string(&result.content) {
                Ok(s) => s,
                Err(e) => format!("{{\"error\": \"serialisation failed: {e}\"}}"),
            };

            // Collect diagnostics: tool-level + W7.4 server-level staleness.
            let mut all_diags = result.diagnostics.clone();
            // DoD-A6b: when SemanticSearch is served by the lexical hash fallback (no semantic model
            // loaded), ride a per-call diagnostic in the response the agent reads — NOT eprintln —
            // so it knows results are lexical, not semantic (R6 applied to the embedder tier).
            if tool_name == "SemanticSearch"
                && ctx.embedder_runtime_id.as_deref() == Some("hash:v1")
            {
                all_diags.push(
                    "LEXICAL-FALLBACK: no semantic model loaded; results are lexical".to_string(),
                );
            }
            if let Some(n) = ctx.commits_behind {
                if n > 0 {
                    all_diags.push(format!(
                        "STALENESS: commits_behind={n} — re-run `wicked-estate index` to refresh"
                    ));
                }
            }

            // Append diagnostics as a second text block if any are present.
            let mut content = vec![json!({ "type": "text", "text": content_text })];
            if !all_diags.is_empty() {
                content_text = all_diags.join("\n");
                content.push(json!({ "type": "text", "text": content_text }));
            }

            ok_response(
                id,
                json!({
                    "content": content,
                    "isError": false
                }),
            )
        }
        // The retrieval tools are designed to return Ok even on missing symbols
        // (agent-behavior rule R1).  An Err here means a store-level failure —
        // report it as an MCP tool error, not a JSON-RPC error, so the agent
        // session can continue (R1: never abandon on isError).
        Err(e) => ok_response(
            id,
            json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true
            }),
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public routing entry-point
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// MCP server context (W7.4 staleness, Task F SemanticSearch)
// ─────────────────────────────────────────────────────────────────────────────

/// Per-request context passed through the MCP handler.
///
/// `commits_behind`: if known (computed once at server startup), this is injected
/// as an R5 staleness diagnostic into every `tools/call` response.
///
/// The four `embedder_*` fields drive the **dim-guard** (DoD-A6a, §3.1): SemanticSearch is
/// advertised (and dispatchable) **only** when the store's embedder identity + dim
/// (`embedder_meta_*`, read from store meta at startup) match the runtime embedder
/// (`embedder_runtime_*`, from `default_embedder()`). Identity, not dim, is the correctness key —
/// see [`semantic_advert`]. All-`None` (the `Default`) is the fail-closed state: no semantic.
#[derive(Debug, Default, Clone)]
pub struct McpContext {
    /// How many commits have landed in the indexed repo since the last `index` run.
    /// `None` = unknown (git absent, not a repo, first-run, etc.).
    pub commits_behind: Option<u64>,
    /// Identity of the runtime embedder (`default_embedder().id()`), e.g. `"hash:v1"`.
    /// `None` only if the server could not construct an embedder at all.
    pub embedder_runtime_id: Option<String>,
    /// Dimension of the runtime embedder (`default_embedder().dim()`).
    pub embedder_runtime_dim: Option<usize>,
    /// Embedder identity recorded in the store's `meta["embedder_id"]` at index time.
    /// `None` = the store predates embedder tagging (or was never `index --embeddings`'d).
    pub embedder_meta_id: Option<String>,
    /// Embedder dimension recorded in the store's `meta["embedder_dim"]` at index time.
    pub embedder_meta_dim: Option<usize>,
}

/// Decide whether SemanticSearch may be advertised / dispatched for this store + runtime.
///
/// Returns `Ok(())` when the store's recorded embedder identity **and** dimension match the
/// runtime embedder. Otherwise returns the loud diagnostic the agent should see (R6):
///
/// * store meta absent (`None`) → `EMBED-META-MISSING:` — the store predates tagging; serving
///   would silently return quietly-degraded results (`nearest` skips mismatched-dim rows), so we
///   fail closed and report honest absence.
/// * id or dim mismatch → `EMBED-MISMATCH: store=<id>/<dim>, runtime=<id>/<dim>; re-index`.
///
/// Identity is checked first and is decisive: two distinct models can share a dim (e.g. 384) yet
/// produce incomparable vectors, so dim-equality alone is insufficient.
fn semantic_advert(ctx: &McpContext) -> std::result::Result<(), String> {
    let (Some(meta_id), Some(meta_dim)) = (&ctx.embedder_meta_id, ctx.embedder_meta_dim) else {
        return Err(
            "EMBED-META-MISSING: store predates embedder tagging; semantic disabled, re-index with --embeddings"
                .to_string(),
        );
    };
    let runtime_id = ctx.embedder_runtime_id.as_deref().unwrap_or("<none>");
    let runtime_dim = ctx.embedder_runtime_dim;
    if Some(meta_id.as_str()) != Some(runtime_id) || Some(meta_dim) != runtime_dim {
        let rt_dim = runtime_dim.map_or_else(|| "?".to_string(), |d| d.to_string());
        return Err(format!(
            "EMBED-MISMATCH: store={meta_id}/{meta_dim}, runtime={runtime_id}/{rt_dim}; re-index"
        ));
    }
    Ok(())
}

/// Route one JSON-RPC 2.0 request object to the correct handler and return the
/// response value.  Pure function — no I/O, fully unit-testable.
///
/// Notifications (requests without `"id"`) are handled by returning
/// [`Value::Null`] — the caller's stdio loop must skip writing null responses.
pub fn handle_request(store: &dyn GraphRead, req: &Value) -> Value {
    handle_request_ctx(store, req, &McpContext::default())
}

/// Like `handle_request` but injects server-side context (staleness, dim-guard fields).
///
/// No live SemanticSearch tool — semantic is never advertised/dispatched via this path. Use
/// [`handle_request_with_semantic`] to wire the live instance (the serving loop does).
pub fn handle_request_ctx(store: &dyn GraphRead, req: &Value, ctx: &McpContext) -> Value {
    handle_request_with_semantic(store, req, ctx, None)
}

/// Full routing entry-point: injects context **and** the live SemanticSearch tool.
///
/// `semantic` is the real [`SemanticSearch`] instance (built with the runtime store + embedder).
/// `tools/list` advertises it and `tools/call` dispatches to it **only** when the dim-guard
/// ([`semantic_advert`]) passes — closing the DoD-A6b gap where no live semantic dispatch path
/// existed at all (the serving loop resolved against `all_tools()`, which excludes SemanticSearch).
pub fn handle_request_with_semantic(
    store: &dyn GraphRead,
    req: &Value,
    ctx: &McpContext,
    semantic: Option<&dyn RetrievalTool>,
) -> Value {
    // Extract the request id; absent id ⇒ notification.
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => handle_initialize(&id),

        "notifications/initialized" => {
            // Notification — no response required. Caller skips null.
            Value::Null
        }

        "tools/list" => handle_tools_list_ctx(&id, ctx, semantic),

        "tools/call" => handle_tools_call_ctx(&id, &params, store, ctx, semantic),

        // Unknown / unimplemented method.
        _ if id.is_null() => {
            // Unknown notification — silently drop.
            Value::Null
        }
        _ => err_response(&id, -32601, &format!("Method not found: '{method}'")),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified dispatch — DomainHandles + handle_request_unified
// ─────────────────────────────────────────────────────────────────────────────

/// Borrowed handles for the domain stores (memory + knowledge).
/// `None` ⇒ the corresponding domain tools return an error rather than crash.
pub struct DomainHandles<'a> {
    pub memory: &'a mut dyn MemoryApi<Error = anyhow::Error>,
    pub knowledge: &'a mut dyn KnowledgeApi,
}

/// Unified routing entry-point: estate tools + optional memory/knowledge tools + resources/prompts.
///
/// `domains = None` → estate-only mode (10/11 tools). `domains = Some(...)` → 23+ tools, resources,
/// and prompts. Memory/knowledge tools that arrive without domains return a clean JSON-RPC error.
/// `semantic` is the live SemanticSearch instance; when `None` the tool is neither advertised nor
/// dispatchable (consistent fail-closed, same as the dim-guard behaviour in the old path).
pub fn handle_request_unified(
    store: &dyn GraphRead,
    req: &Value,
    ctx: &McpContext,
    domains: Option<&mut DomainHandles<'_>>,
    semantic: Option<&dyn RetrievalTool>,
) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => handle_initialize_unified(&id),

        "notifications/initialized" => Value::Null,

        "tools/list" => tools_list_unified(&id, ctx, domains.is_some()),

        "resources/list" => resources::resources_list(&id),
        "resources/read" => {
            let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            resources::resources_read(&id, uri)
        }
        "prompts/list" => resources::prompts_list(&id),
        "prompts/get" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            resources::prompts_get(&id, name)
        }

        "tools/call" => {
            let tool = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            match tool {
                "SearchEntity" | "RetrieveEntity" | "TraverseGraph" | "BlastRadius"
                | "FetchContent" | "ContextBundle" | "RulesInventory" | "RankHotspots"
                | "Communities" | "Lineage" => {
                    handle_tools_call_ctx(&id, &params, store, ctx, None)
                }

                "SemanticSearch" => handle_tools_call_ctx(&id, &params, store, ctx, semantic),

                "memory.capture" | "memory.recall" | "memory.reflect" | "memory.erase"
                | "memory.learn" | "memory.coverage" => match domains {
                    Some(d) => tools::memory::dispatch(tool, &id, &params, store, d.memory),
                    None => err_response(&id, -32601, "memory domain not available"),
                },

                "knowledge.ingest"
                | "knowledge.write"
                | "knowledge.relate"
                | "knowledge.recall"
                | "knowledge.coverage"
                | "knowledge.relate_code"
                | "knowledge.recall_about_code" => match domains {
                    Some(d) => tools::knowledge::dispatch(tool, &id, &params, store, d.knowledge),
                    None => err_response(&id, -32601, "knowledge domain not available"),
                },

                _ => err_response(&id, -32602, &format!("unknown tool: {tool}")),
            }
        }

        _ if id.is_null() => Value::Null,
        _ => err_response(&id, -32601, &format!("Method not found: '{method}'")),
    }
}

fn handle_initialize_unified(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "serverInfo": { "name": "wicked-estate", "version": SERVER_VERSION }
        }
    })
}

fn tools_list_unified(id: &Value, ctx: &McpContext, domains_available: bool) -> Value {
    let base_tools = all_tools();
    let mut tools: Vec<Value> = base_tools
        .iter()
        .map(|t| {
            json!({
                "name":        t.name(),
                "description": t.description(),
                "inputSchema": input_schema(t.name()).unwrap_or(json!({"type":"object"}))
            })
        })
        .collect();

    if semantic_advert(ctx).is_ok() {
        tools.push(json!({
            "name":        "SemanticSearch",
            "description": "Semantic vector search over code symbols.",
            "inputSchema": semantic_search_schema()
        }));
    }

    if domains_available {
        tools.extend(memory_tool_schemas());
        tools.extend(knowledge_tool_schemas());
    }

    ok_response(id, json!({"tools": tools}))
}

fn memory_tool_schemas() -> Vec<Value> {
    vec![
        json!({"name":"memory.capture","description":"Capture a new memory node (episodic/semantic/procedural/archival).","inputSchema":{"type":"object","required":["content"],"properties":{"content":{"type":"string"},"kind":{"type":"string","enum":["working","episode","entity","fact","skill","archive"]},"tier":{"type":"string","enum":["working","episodic","semantic","procedural","archival"]},"scope":{"type":"string"},"about":{"type":"array","items":{"type":"string"}}}}}),
        json!({"name":"memory.recall","description":"Conversational recall: token-budgeted slice relevant to a query in scope.","inputSchema":{"type":"object","required":["query"],"properties":{"query":{"type":"string"},"scope":{"type":"string"},"seeds":{"type":"array","items":{"type":"string"}},"token_budget":{"type":"integer","default":2000}}}}),
        json!({"name":"memory.reflect","description":"Distil episodic memories in a scope into semantic facts (T2 tier). Returns distilled_facts list.","inputSchema":{"type":"object","properties":{"scope":{"type":"string"}}}}),
        json!({"name":"memory.erase","description":"Hard-delete all memories whose scope starts with the given prefix.","inputSchema":{"type":"object","required":["scope_prefix"],"properties":{"scope_prefix":{"type":"string"}}}}),
        json!({"name":"memory.learn","description":"Store a semantic fact and link it to code symbols atomically.","inputSchema":{"type":"object","required":["content","symbols"],"properties":{"content":{"description":"one specific, non-obvious fact","type":"string"},"scope":{"description":"e.g. project:my-repo","type":"string"},"symbols":{"description":"exact code symbol name(s) this fact concerns","items":{"type":"string"},"type":"array"},"tier":{"description":"semantic=fact/decision, procedural=how-it-works","enum":["semantic","procedural"],"type":"string"}}}}),
        json!({"name":"memory.coverage","description":"Coverage: memory node counts (total, by tier, by kind), optionally scoped.","inputSchema":{"type":"object","properties":{"scope_prefix":{"type":"string"}}}}),
    ]
}

fn knowledge_tool_schemas() -> Vec<Value> {
    vec![
        json!({"name":"knowledge.ingest","description":"Ingest a document as doc + chunk nodes.","inputSchema":{"type":"object","required":["title","chunks"],"properties":{"title":{"type":"string"},"chunks":{"type":"array","items":{"type":"string"}},"scope":{"type":"string"},"source":{"type":"string"}}}}),
        json!({"name":"knowledge.write","description":"Write ONE knowledge node.","inputSchema":{"type":"object","required":["content"],"properties":{"content":{"type":"string"},"class":{"type":"string","enum":["doc","section","chunk","concept"]},"scope":{"type":"string"},"source":{"type":"string"}}}}),
        json!({"name":"knowledge.relate","description":"Add a typed relation between two knowledge nodes, with confidence + evidence_count + provenance.","inputSchema":{"type":"object","required":["src","tgt","rel"],"properties":{"src":{"type":"string"},"tgt":{"type":"string"},"rel":{"type":"string"},"confidence":{"type":"number"},"evidence_count":{"type":"integer","minimum":0,"maximum":4294967295u64,"description":"audit counter: confirmations/contradictions (default 0; non-negative, fits u32, else -32602)"},"provenance":{"type":"string"}}}}),
        json!({"name":"knowledge.recall","description":"Hybrid recall (FTS + vector, RRF fused) over the knowledge base.","inputSchema":{"type":"object","required":["query"],"properties":{"query":{"type":"string"},"token_budget":{"type":"integer","default":2000}}}}),
        json!({"name":"knowledge.coverage","description":"Coverage: node counts per class.","inputSchema":{"type":"object","properties":{"class":{"type":"string","enum":["doc","section","chunk","concept"]}}}}),
        json!({"name":"knowledge.relate_code","description":"Link a knowledge node to estate code symbols via xedge.","inputSchema":{"type":"object","required":["knowledge_id","code_ids"],"properties":{"knowledge_id":{"type":"string"},"code_ids":{"type":"array","items":{"type":"string"}}}}}),
        json!({"name":"knowledge.recall_about_code","description":"Recall knowledge linked to code symbols (cross-store lookup).","inputSchema":{"type":"object","required":["code_ids"],"properties":{"code_ids":{"type":"array","items":{"type":"string"}}}}}),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wicked_estate_core::{
        Confidence, Edge, EdgeKind, GraphWrite, Language, Location, Node, NodeKind, ResolutionTier,
        Span, SymbolId,
    };
    use wicked_estate_store::MemStore;

    // ── Fixture ──────────────────────────────────────────────────────────────

    fn span(line: u32) -> Span {
        Span {
            start_byte: 0,
            end_byte: 0,
            start_line: line,
            start_col: 0,
            end_line: line,
            end_col: 0,
        }
    }

    fn node(id: &str, name: &str, kind: NodeKind, file: &str, line: u32) -> Node {
        Node::new(
            SymbolId(id.to_string()),
            kind,
            name,
            Language::new("rust"),
            Location::new(file, span(line)),
        )
    }

    fn call_edge(src: &str, tgt: &str) -> Edge {
        Edge::new(
            SymbolId(src.to_string()),
            SymbolId(tgt.to_string()),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
    }

    /// caller → middle → leaf  (blast-radius of leaf = {middle, caller})
    fn fixture() -> MemStore {
        let mut s = MemStore::new();
        s.begin_batch().unwrap();
        s.upsert_nodes(&[
            node("caller", "caller_fn", NodeKind::Function, "src/a.rs", 1),
            node("middle", "middle_fn", NodeKind::Function, "src/b.rs", 10),
            node("leaf", "leaf_fn", NodeKind::Function, "src/c.rs", 20),
        ])
        .unwrap();
        s.upsert_edges(&[call_edge("caller", "middle"), call_edge("middle", "leaf")])
            .unwrap();
        s.commit_batch().unwrap();
        s
    }

    // ── initialize ────────────────────────────────────────────────────────────

    #[test]
    fn initialize_returns_protocol_version() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        let result = &resp["result"];
        assert!(
            result["protocolVersion"].is_string(),
            "protocolVersion must be a string"
        );
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(
            result["serverInfo"]["name"].as_str().unwrap(),
            "wicked-estate"
        );
        assert!(result["serverInfo"]["version"].is_string());
    }

    // ── tools/list ────────────────────────────────────────────────────────────

    /// DoD-A4: `tools/list` exposes the **10 unconditional read tools** as a floor, with
    /// `SemanticSearch` conditionally present (the dim-guard), so the count is **10 or 11**.
    ///
    /// `handle_request` wires no semantic tool (`None`), so the dim-guard cannot pass and the bare
    /// floor is exactly 10. The conditional 11th is covered by the dim-guard gate tests
    /// (`semantic_advertised_*` / `semantic_not_advertised_*`) which drive
    /// `handle_request_with_semantic` with a live `Some(&tool)`.
    #[test]
    fn tools_list_returns_ten_unconditional_tools() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        let tools = resp["result"]["tools"]
            .as_array()
            .expect("tools must be array");
        assert_eq!(
            tools.len(),
            10,
            "the unconditional read-tool floor is exactly 10 (no semantic wired); got {}",
            tools.len()
        );
        // Annotate must NOT be on the read-only MCP surface (design §2.3).
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(
            !names.contains(&"Annotate"),
            "Annotate must NOT appear in tools/list (read-only surface)"
        );
    }

    /// DoD-A4: when SemanticSearch IS wired and the dim-guard passes, the count rises to **11** —
    /// the 10 unconditional + the conditional semantic tool. Falsifier for the floor being a hard
    /// ceiling.
    #[test]
    fn tools_list_returns_eleven_with_semantic_available() {
        let store = fixture();
        let fake = FakeSemantic;
        // Matching id + dim → dim-guard passes → SemanticSearch advertised as the 11th tool.
        let ctx = McpContext {
            embedder_runtime_id: Some("hash:v1".into()),
            embedder_runtime_dim: Some(64),
            embedder_meta_id: Some("hash:v1".into()),
            embedder_meta_dim: Some(64),
            ..Default::default()
        };
        assert!(
            semantic_advert(&ctx).is_ok(),
            "precondition: matching id+dim must pass the dim-guard"
        );
        let resp = handle_request_with_semantic(&store, &tools_list_req(), &ctx, Some(&fake));
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            11,
            "10 unconditional + 1 conditional SemanticSearch = 11; got {}",
            tools.len()
        );
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"SemanticSearch"),
            "SemanticSearch must be the 11th"
        );
    }

    #[test]
    fn tools_list_contains_expected_names() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        let names: Vec<&str> = resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();

        for expected in &[
            "SearchEntity",
            "RetrieveEntity",
            "TraverseGraph",
            "BlastRadius",
            "FetchContent",
            "ContextBundle",
            "RulesInventory",
            "RankHotspots",
            "Communities",
            "Lineage",
        ] {
            assert!(names.contains(expected), "expected tool {expected} in list");
        }
    }

    #[test]
    fn tools_list_each_tool_has_schema() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        for tool in resp["result"]["tools"].as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert!(
                tool["inputSchema"]["type"] == "object",
                "tool {name} must have an object inputSchema"
            );
            assert!(
                tool["description"].is_string(),
                "tool {name} must have a description"
            );
        }
    }

    #[test]
    fn tools_list_search_entity_schema_has_required_name() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        let tools = resp["result"]["tools"].as_array().unwrap();
        let search = tools.iter().find(|t| t["name"] == "SearchEntity").unwrap();
        let required = search["inputSchema"]["required"].as_array().unwrap();
        assert!(
            required.iter().any(|r| r == "name"),
            "SearchEntity schema must require 'name'"
        );
    }

    // ── tools/call — SearchEntity ─────────────────────────────────────────────

    #[test]
    fn tools_call_search_entity_finds_symbol() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "SearchEntity",
                "arguments": { "name": "middle_fn" }
            }
        });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 10);
        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );

        let content = resp["result"]["content"].as_array().unwrap();
        assert!(!content.is_empty(), "content must not be empty");

        let text = content[0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");
        let matches = parsed["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["name"].as_str().unwrap(), "middle_fn");
    }

    #[test]
    fn tools_call_search_entity_envelope_is_correct() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "SearchEntity",
                "arguments": { "name": "caller_fn" }
            }
        });
        let resp = handle_request(&store, &req);

        // Verify the full JSON-RPC envelope structure.
        assert!(resp.get("id").is_some());
        assert!(resp.get("result").is_some());
        assert!(
            resp.get("error").is_none(),
            "success responses must not have 'error'"
        );
        assert!(resp["result"]["content"].is_array());
        assert!(resp["result"]["isError"].is_boolean());
    }

    // ── tools/call — BlastRadius ──────────────────────────────────────────────

    #[test]
    fn tools_call_blast_radius_finds_dependents() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "BlastRadius",
                "arguments": { "symbol": "leaf", "depth": 8 }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(!resp["result"]["isError"].as_bool().unwrap_or(true));
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();

        let total = parsed["total"].as_u64().unwrap();
        assert_eq!(
            total, 2,
            "leaf has 2 transitive dependents: middle and caller"
        );

        let names: Vec<&str> = parsed["dependents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"middle_fn"));
        assert!(names.contains(&"caller_fn"));
        assert!(
            !names.contains(&"leaf_fn"),
            "start symbol excluded from blast radius"
        );
    }

    #[test]
    fn tools_call_blast_radius_envelope_is_correct() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "BlastRadius",
                "arguments": { "symbol": "caller" }
            }
        });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["id"], 21);
        assert!(resp["result"]["content"].is_array());
        // caller has no dependents — should still be isError: false (R1)
        assert!(!resp["result"]["isError"].as_bool().unwrap());
    }

    // ── tools/call — FetchContent ─────────────────────────────────────────────

    #[test]
    fn tools_call_fetch_content_missing_symbol_not_error() {
        // Symbol not in the fixture store → found=false, isError=false (R1).
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 25,
            "method": "tools/call",
            "params": {
                "name": "FetchContent",
                "arguments": { "symbol": "nonexistent_xyz" }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(!parsed["found"].as_bool().unwrap(), "found must be false");
    }

    #[test]
    fn tools_call_fetch_content_schema_in_list() {
        // FetchContent must appear in tools/list with an object inputSchema.
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 26, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        let tools = resp["result"]["tools"].as_array().unwrap();
        let fc = tools
            .iter()
            .find(|t| t["name"] == "FetchContent")
            .expect("FetchContent must be in tool list");
        assert_eq!(fc["inputSchema"]["type"], "object");
        let required = fc["inputSchema"]["required"].as_array().unwrap();
        assert!(
            required.iter().any(|r| r == "symbol"),
            "schema must require 'symbol'"
        );
    }

    // ── tools/call — ContextBundle ────────────────────────────────────────────

    #[test]
    fn tools_call_context_bundle_schema_in_list() {
        // ContextBundle must appear in tools/list with an object inputSchema.
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 27, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        let tools = resp["result"]["tools"].as_array().unwrap();
        let cb = tools
            .iter()
            .find(|t| t["name"] == "ContextBundle")
            .expect("ContextBundle must be in tool list");
        assert_eq!(cb["inputSchema"]["type"], "object");
        assert!(
            cb["description"].is_string(),
            "ContextBundle must have a description"
        );
    }

    #[test]
    fn tools_call_context_bundle_returns_content() {
        // A real seed returns a bundle (seed + neighbours) with isError=false.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 28,
            "method": "tools/call",
            "params": {
                "name": "ContextBundle",
                "arguments": { "symbol": "middle", "budget": 8000 }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );
        let content = resp["result"]["content"].as_array().unwrap();
        assert!(!content.is_empty(), "content must not be empty");

        let text = content[0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");
        assert_eq!(
            parsed["seed"]["name"].as_str().unwrap(),
            "middle_fn",
            "bundle must carry the resolved seed"
        );
        let neighbors = parsed["neighbors"].as_array().unwrap();
        assert!(
            !neighbors.is_empty(),
            "middle has a caller and a callee — neighbours must be present"
        );
    }

    // ── tools/call — RetrieveEntity ───────────────────────────────────────────

    #[test]
    fn tools_call_retrieve_entity_returns_symbol_details() {
        // RetrieveEntity through the real tools/call dispatch must return the SPECIFIC
        // node's details (name, kind, language, file:line) — not just a non-error envelope.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 60,
            "method": "tools/call",
            "params": {
                "name": "RetrieveEntity",
                "arguments": { "symbol": "leaf" }
            }
        });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 60);
        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );

        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");

        assert!(
            parsed["found"].as_bool().unwrap(),
            "the 'leaf' symbol exists in the fixture → found must be true"
        );
        // The RIGHT entity, not merely *an* entity.
        assert_eq!(
            parsed["symbol"].as_str().unwrap(),
            "leaf",
            "must echo the requested symbol id"
        );
        assert_eq!(parsed["name"].as_str().unwrap(), "leaf_fn");
        assert_eq!(parsed["kind"].as_str().unwrap(), "function");
        assert_eq!(parsed["language"].as_str().unwrap(), "rust");
        assert_eq!(parsed["file"].as_str().unwrap(), "src/c.rs");
        // Fixture put leaf at line 20 (0-based); line_1based must be +1.
        assert_eq!(parsed["line"].as_u64().unwrap(), 20);
        assert_eq!(parsed["line_1based"].as_u64().unwrap(), 21);
    }

    #[test]
    fn tools_call_retrieve_entity_missing_symbol_is_found_false_not_error() {
        // Negative/edge case: an absent symbol must yield an HONEST found=false through the
        // MCP envelope (R1: isError stays false), never a silent wrong entity.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 61,
            "method": "tools/call",
            "params": {
                "name": "RetrieveEntity",
                "arguments": { "symbol": "no_such_symbol_xyz" }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "missing symbol must NOT be an error (R1)"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(
            !parsed["found"].as_bool().unwrap(),
            "found must be false for an absent symbol"
        );
        // And it must NOT have leaked some other entity's name.
        assert!(
            parsed.get("name").is_none(),
            "no entity details must be present when not found"
        );
    }

    // ── tools/call — RulesInventory ───────────────────────────────────────────

    /// caller_fn --InvokedBy--> PricingRules (a RuleSet node).
    /// Edge convention: source = code call site (dependent), target = RuleSet (dependency).
    fn rules_fixture() -> MemStore {
        let mut s = MemStore::new();
        s.begin_batch().unwrap();
        s.upsert_nodes(&[
            node(
                "rs::pricing",
                "PricingRules",
                NodeKind::RuleSet,
                "rules/pricing.drl",
                1,
            ),
            node(
                "app::run_pricing",
                "run_pricing",
                NodeKind::Function,
                "src/pricing_service.rs",
                42,
            ),
        ])
        .unwrap();
        s.upsert_edges(&[Edge::new(
            SymbolId("app::run_pricing".to_string()),
            SymbolId("rs::pricing".to_string()),
            EdgeKind::InvokedBy,
            ResolutionTier::Parsed,
            "test-fixture",
        )])
        .unwrap();
        s.commit_batch().unwrap();
        s
    }

    #[test]
    fn tools_call_rules_inventory_returns_engines_and_invokers() {
        // RulesInventory through tools/call must return the RuleSet AND the code that invokes it.
        let store = rules_fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 62,
            "method": "tools/call",
            "params": {
                "name": "RulesInventory",
                "arguments": {}
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");

        assert_eq!(
            parsed["total"].as_u64().unwrap(),
            1,
            "exactly one RuleSet engine in the fixture"
        );
        let engines = parsed["engines"].as_array().unwrap();
        assert_eq!(engines.len(), 1);
        let engine = &engines[0];
        assert_eq!(engine["name"].as_str().unwrap(), "PricingRules");
        assert_eq!(engine["kind"].as_str().unwrap(), "rule_set");
        assert_eq!(engine["file"].as_str().unwrap(), "rules/pricing.drl");

        // The invoking code (the InvokedBy edge source) must be surfaced.
        let invoked_by = engine["invoked_by"].as_array().unwrap();
        assert!(
            invoked_by
                .iter()
                .any(|v| v.as_str() == Some("app::run_pricing")),
            "the invoking symbol must appear in invoked_by; got {invoked_by:?}"
        );
    }

    #[test]
    fn tools_call_rules_inventory_empty_graph_is_honest_empty_not_error() {
        // Negative/edge case: the base fixture has NO RuleSet nodes. RulesInventory must report an
        // honest empty inventory (total=0) through the envelope, not error and not a phantom engine.
        let store = fixture(); // caller/middle/leaf — no RuleSet
        let req = json!({
            "jsonrpc": "2.0",
            "id": 63,
            "method": "tools/call",
            "params": {
                "name": "RulesInventory",
                "arguments": {}
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "empty inventory is not an error (R1)"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            parsed["total"].as_u64().unwrap(),
            0,
            "no RuleSet nodes → total must be 0"
        );
        assert!(
            parsed["engines"].as_array().unwrap().is_empty(),
            "engines must be empty when no RuleSet exists"
        );
    }

    // ── tools/call — RankHotspots ─────────────────────────────────────────────

    #[test]
    fn tools_call_rank_hotspots_ranks_most_depended_on_first() {
        // Build a hub graph: caller_fn → middle_fn → leaf_fn (the fixture). PageRank flows toward
        // the most-depended-on node. `leaf` is the deepest sink (depended on by middle, transitively
        // by caller); `caller` depends on nothing. So `leaf_fn` must outrank `caller_fn`.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 64,
            "method": "tools/call",
            "params": {
                "name": "RankHotspots",
                "arguments": { "limit": 20 }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");

        let hotspots = parsed["hotspots"].as_array().unwrap();
        assert_eq!(
            hotspots.len(),
            3,
            "all 3 fixture nodes ranked; got {hotspots:?}"
        );

        // Scores must be in descending order (the tool guarantees high-score-first).
        let scores: Vec<f64> = hotspots
            .iter()
            .map(|h| h["score"].as_f64().expect("score must be a number"))
            .collect();
        for w in scores.windows(2) {
            assert!(
                w[0] >= w[1],
                "hotspots must be sorted by score descending; got {scores:?}"
            );
        }

        // The most-depended-on symbol (leaf, the sink) must rank strictly above the
        // depends-on-nothing source (caller). This is the load-bearing, fail-if-broken assertion.
        let rank_of = |name: &str| -> usize {
            hotspots
                .iter()
                .position(|h| h["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("{name} must be present in hotspots"))
        };
        assert!(
            rank_of("leaf_fn") < rank_of("caller_fn"),
            "the sink (leaf_fn, most depended-on) must outrank the source (caller_fn); \
             order was {:?}",
            hotspots
                .iter()
                .map(|h| h["name"].as_str().unwrap())
                .collect::<Vec<_>>()
        );
        let leaf_score = hotspots[rank_of("leaf_fn")]["score"].as_f64().unwrap();
        let caller_score = hotspots[rank_of("caller_fn")]["score"].as_f64().unwrap();
        assert!(
            leaf_score > caller_score,
            "leaf_fn PageRank ({leaf_score}) must exceed caller_fn ({caller_score})"
        );
    }

    // ── tools/call — Communities ──────────────────────────────────────────────

    /// Two disjoint fully-connected triangles → exactly two size-3 communities.
    /// Triangle 1: a1↔a2↔a3 ; Triangle 2: b1↔b2↔b3 ; no cross-group edges.
    fn two_triangle_fixture() -> MemStore {
        let mut s = MemStore::new();
        s.begin_batch().unwrap();
        s.upsert_nodes(&[
            node("a1", "a1_fn", NodeKind::Function, "src/a1.rs", 1),
            node("a2", "a2_fn", NodeKind::Function, "src/a2.rs", 2),
            node("a3", "a3_fn", NodeKind::Function, "src/a3.rs", 3),
            node("b1", "b1_fn", NodeKind::Function, "src/b1.rs", 4),
            node("b2", "b2_fn", NodeKind::Function, "src/b2.rs", 5),
            node("b3", "b3_fn", NodeKind::Function, "src/b3.rs", 6),
        ])
        .unwrap();
        s.upsert_edges(&[
            call_edge("a1", "a2"),
            call_edge("a2", "a3"),
            call_edge("a3", "a1"),
            call_edge("b1", "b2"),
            call_edge("b2", "b3"),
            call_edge("b3", "b1"),
        ])
        .unwrap();
        s.commit_batch().unwrap();
        s
    }

    #[test]
    fn tools_call_communities_returns_clusters() {
        // Communities through tools/call must surface the two distinct clusters, each of size 3.
        let store = two_triangle_fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 65,
            "method": "tools/call",
            "params": {
                "name": "Communities",
                "arguments": { "limit": 20, "min_size": 2 }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");

        let communities = parsed["communities"].as_array().unwrap();
        assert_eq!(
            parsed["total"].as_u64().unwrap(),
            2,
            "two disjoint triangles → exactly two communities; got {communities:?}"
        );
        assert_eq!(communities.len(), 2);

        // Each detected community must be a size-3 cluster (the triangles), with member symbols.
        for c in communities {
            assert_eq!(
                c["size"].as_u64().unwrap(),
                3,
                "each triangle is a size-3 community; got {c}"
            );
            assert!(
                !c["top_symbols"].as_array().unwrap().is_empty(),
                "a community must list its member top_symbols"
            );
        }

        // Non-vacuous separation: the two clusters must partition the a-group and the b-group —
        // no community may mix an a-node with a b-node (they share no edges).
        let group_of = |sym: &str| -> Option<char> { sym.chars().next() };
        for c in communities {
            let groups: std::collections::HashSet<char> = c["top_symbols"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .filter_map(group_of)
                .collect();
            assert_eq!(
                groups.len(),
                1,
                "a community must not mix the disjoint a/b groups; got {:?}",
                c["top_symbols"]
            );
        }
    }

    // ── tools/call — Lineage ──────────────────────────────────────────────────

    #[test]
    fn tools_call_lineage_returns_dependency_lineage() {
        // Lineage walks Dependencies (what does X depend on?). Fixture: caller → middle → leaf.
        // From `caller`, the transitive lineage is {middle, leaf}; `caller` itself is excluded.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 66,
            "method": "tools/call",
            "params": {
                "name": "Lineage",
                "arguments": { "symbol": "caller", "depth": 8 }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "isError must be false"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).expect("content text must be valid JSON");

        assert_eq!(
            parsed["total"].as_u64().unwrap(),
            2,
            "caller's transitive dependencies are middle + leaf"
        );
        let deps = parsed["dependencies"].as_array().unwrap();
        let names: Vec<&str> = deps.iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"middle_fn"),
            "direct dependency middle_fn must be in the lineage; got {names:?}"
        );
        assert!(
            names.contains(&"leaf_fn"),
            "transitive dependency leaf_fn must be in the lineage; got {names:?}"
        );
        assert!(
            !names.contains(&"caller_fn"),
            "the start symbol must be excluded from its own lineage"
        );

        // Depth must be accurate: middle is 1 hop, leaf is 2 hops from caller.
        let depth_of = |name: &str| -> u64 {
            deps.iter()
                .find(|d| d["name"].as_str() == Some(name))
                .unwrap()["depth"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(depth_of("middle_fn"), 1, "middle is one hop from caller");
        assert_eq!(depth_of("leaf_fn"), 2, "leaf is two hops from caller");
    }

    #[test]
    fn tools_call_lineage_missing_symbol_is_empty_not_error() {
        // Negative/edge case: an absent start symbol must yield an honest empty lineage
        // (total=0) through the envelope, not an error and not a wrong non-empty result (R1).
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 67,
            "method": "tools/call",
            "params": {
                "name": "Lineage",
                "arguments": { "symbol": "ghost_symbol_xyz" }
            }
        });
        let resp = handle_request(&store, &req);

        assert!(
            !resp["result"]["isError"].as_bool().unwrap_or(true),
            "missing start symbol must NOT be an error (R1)"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            parsed["total"].as_u64().unwrap(),
            0,
            "absent symbol → empty lineage"
        );
        assert!(
            parsed["dependencies"].as_array().unwrap().is_empty(),
            "dependencies must be empty for an absent symbol"
        );
    }

    // ── unknown method ────────────────────────────────────────────────────────

    #[test]
    fn unknown_method_returns_minus_32601() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "no_such_method",
            "params": {}
        });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32601);
        assert!(
            resp.get("result").is_none(),
            "error responses must not have 'result'"
        );
    }

    #[test]
    fn unknown_method_preserves_id() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": "string-id",
            "method": "nope",
            "params": {}
        });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["id"].as_str().unwrap(), "string-id");
    }

    // ── notifications ─────────────────────────────────────────────────────────

    #[test]
    fn notifications_initialized_returns_null() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let resp = handle_request(&store, &req);

        assert!(
            resp.is_null(),
            "notification response must be null (no response)"
        );
    }

    #[test]
    fn unknown_notification_returns_null() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "method": "some/unknown/notification" });
        let resp = handle_request(&store, &req);

        // Notifications have no id; we silently drop them.
        assert!(resp.is_null());
    }

    // ── tools/call — unknown tool ─────────────────────────────────────────────

    #[test]
    fn tools_call_unknown_tool_returns_error() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "tools/call",
            "params": { "name": "NonExistentTool", "arguments": {} }
        });
        let resp = handle_request(&store, &req);

        assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32602);
    }

    // ── tools/call — diagnostics surface ─────────────────────────────────────

    #[test]
    fn tools_call_diagnostics_appear_in_content() {
        // Any call that produces diagnostics (e.g. empty search) should surface them.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "tools/call",
            "params": {
                "name": "SearchEntity",
                "arguments": { "name": "zzz_nonexistent_xyz" }
            }
        });
        let resp = handle_request(&store, &req);

        let content = resp["result"]["content"].as_array().unwrap();
        // First block is the result JSON, second (if present) is diagnostics.
        // Either the diagnostic is in the second block or the first block's
        // total is 0 — both prove the tool handled the miss correctly.
        let first_text = content[0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(first_text).unwrap();
        assert_eq!(parsed["total"].as_u64().unwrap(), 0);
    }

    // ── input_schema helper ───────────────────────────────────────────────────

    #[test]
    fn input_schema_known_tools_return_some() {
        for name in &[
            "SearchEntity",
            "RetrieveEntity",
            "TraverseGraph",
            "BlastRadius",
            "FetchContent",
            "ContextBundle",
            "RankHotspots",
            "Communities",
            "Lineage",
        ] {
            assert!(
                input_schema(name).is_some(),
                "input_schema({name}) must return Some"
            );
        }
    }

    #[test]
    fn input_schema_unknown_tool_returns_none() {
        assert!(input_schema("Ghost").is_none());
    }

    // ── Low-confidence edge — diagnostics (R7) ────────────────────────────────

    #[test]
    fn tools_call_traverse_flags_low_confidence_edges() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                node("alpha", "alpha_fn", NodeKind::Function, "src/x.rs", 1),
                node("beta", "beta_fn", NodeKind::Function, "src/y.rs", 5),
            ])
            .unwrap();
        let mut low = call_edge("alpha", "beta");
        low.confidence = Confidence::new(0.3);
        store.upsert_edges(&[low]).unwrap();
        store.commit_batch().unwrap();

        let req = json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "tools/call",
            "params": {
                "name": "TraverseGraph",
                "arguments": { "symbol": "alpha", "direction": "dependencies", "depth": 3 }
            }
        });
        let resp = handle_request(&store, &req);

        let content = resp["result"]["content"].as_array().unwrap();
        // The R7 diagnostic must appear somewhere in the content blocks.
        let all_text: String = content
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            all_text.contains("R7-CONFIDENCE"),
            "R7 diagnostic must surface through the MCP envelope"
        );
    }

    // ── DoD-A6a + DoD-A6b: dim-guard + live SemanticSearch dispatch ────────────
    //
    // These gate tests operate at the routing level (handle_request_with_semantic) with a
    // synthetic live SemanticSearch tool + synthetic McpContext embedder ids — no real model2vec
    // or `semantic` feature needed (the spec permits a synthetic runtime embedder with a distinct
    // id()/dim() to simulate the runtime). The store's recorded embedder is modelled by the
    // `embedder_meta_*` ctx fields, exactly as main.rs reads them from store meta.

    /// Stand-in for the live SemanticSearch tool. Same `name()` so the registry/dispatch treats it
    /// identically; returns a sentinel so a test can prove dispatch actually reached it.
    #[derive(Debug)]
    struct FakeSemantic;
    impl RetrievalTool for FakeSemantic {
        fn name(&self) -> &str {
            "SemanticSearch"
        }
        fn description(&self) -> &str {
            "fake semantic search (test)"
        }
        fn invoke(
            &self,
            _store: &dyn GraphRead,
            _request: &Value,
        ) -> wicked_estate_core::Result<wicked_estate_core::query::RetrievalResult> {
            Ok(wicked_estate_core::query::RetrievalResult {
                content: json!({ "matches": [], "total": 0, "__fake_semantic_reached": true }),
                diagnostics: vec![],
            })
        }
    }

    /// Build a context with the four dim-guard fields set.
    fn guard_ctx(
        meta_id: Option<&str>,
        meta_dim: Option<usize>,
        rt_id: Option<&str>,
        rt_dim: Option<usize>,
    ) -> McpContext {
        McpContext {
            commits_behind: None,
            embedder_runtime_id: rt_id.map(str::to_string),
            embedder_runtime_dim: rt_dim,
            embedder_meta_id: meta_id.map(str::to_string),
            embedder_meta_dim: meta_dim,
        }
    }

    fn tool_names(resp: &Value) -> Vec<String> {
        resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn tools_list_req() -> Value {
        json!({ "jsonrpc": "2.0", "id": 100, "method": "tools/list", "params": {} })
    }

    fn semantic_call_req() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 101,
            "method": "tools/call",
            "params": { "name": "SemanticSearch", "arguments": { "query": "anything" } }
        })
    }

    // ── DoD-A6a ──

    #[test]
    fn dod_a6a_mismatch_id_dim_not_advertised_and_not_callable() {
        // Store embedded with HashEmbedder (hash:v1 / 128); runtime is a DIFFERENT 256-d model.
        // The falsifier this defeats: advertised-but-silently-empty. We require BOTH that it is
        // absent from tools/list AND that a direct call is rejected (unknown tool) — honest absence.
        let store = fixture();
        let ctx = guard_ctx(
            Some("hash:v1"),
            Some(128),
            Some("model2vec:potion"),
            Some(256),
        );
        let fake = FakeSemantic;

        let listed = handle_request_with_semantic(&store, &tools_list_req(), &ctx, Some(&fake));
        assert!(
            !tool_names(&listed).contains(&"SemanticSearch".to_string()),
            "id/dim mismatch must NOT advertise SemanticSearch"
        );

        let called = handle_request_with_semantic(&store, &semantic_call_req(), &ctx, Some(&fake));
        assert_eq!(
            called["result"]["isError"].as_bool(),
            None,
            "mismatch: SemanticSearch must be rejected as an unknown tool (JSON-RPC error), \
             not dispatched to a silently-empty result"
        );
        assert_eq!(
            called["error"]["code"].as_i64(),
            Some(-32602),
            "unknown-tool error expected when semantic is guarded off"
        );
    }

    #[test]
    fn dod_a6a_same_dim_different_id_not_advertised() {
        // Two distinct 384-d models: dim matches, identity does not. Identity is decisive.
        let store = fixture();
        let ctx = guard_ctx(
            Some("fastembed:bge-small-en-v1.5"),
            Some(384),
            Some("model2vec:other-384"),
            Some(384),
        );
        let fake = FakeSemantic;
        let listed = handle_request_with_semantic(&store, &tools_list_req(), &ctx, Some(&fake));
        assert!(
            !tool_names(&listed).contains(&"SemanticSearch".to_string()),
            "same-dim/different-id must NOT advertise — dim-equality is insufficient"
        );
        assert!(
            semantic_advert(&ctx)
                .unwrap_err()
                .starts_with("EMBED-MISMATCH:"),
            "diagnostic must be EMBED-MISMATCH"
        );
    }

    #[test]
    fn dod_a6a_none_meta_not_advertised_emits_meta_missing() {
        // Store predates embedder tagging (meta None) — fail closed.
        let store = fixture();
        let ctx = guard_ctx(None, None, Some("hash:v1"), Some(128));
        let fake = FakeSemantic;

        let listed = handle_request_with_semantic(&store, &tools_list_req(), &ctx, Some(&fake));
        assert!(
            !tool_names(&listed).contains(&"SemanticSearch".to_string()),
            "None meta must NOT advertise SemanticSearch (fail closed)"
        );
        let err = semantic_advert(&ctx).unwrap_err();
        assert!(
            err.starts_with("EMBED-META-MISSING:"),
            "None meta must yield EMBED-META-MISSING, got: {err}"
        );
    }

    #[test]
    fn dod_a6a_matched_id_dim_is_advertised() {
        // Store embedder identity + dim match the runtime → advertise.
        let store = fixture();
        let ctx = guard_ctx(Some("hash:v1"), Some(128), Some("hash:v1"), Some(128));
        let fake = FakeSemantic;

        assert!(
            semantic_advert(&ctx).is_ok(),
            "matched id/dim must pass the guard"
        );
        let listed = handle_request_with_semantic(&store, &tools_list_req(), &ctx, Some(&fake));
        assert!(
            tool_names(&listed).contains(&"SemanticSearch".to_string()),
            "matched id/dim must advertise SemanticSearch"
        );
    }

    #[test]
    fn dod_a6a_matched_but_no_live_tool_still_not_advertised() {
        // Guard passes but no live instance wired (e.g. :memory: db) → cannot advertise a tool that
        // does not exist. Belt-and-suspenders: advertise requires BOTH guard ok AND a live tool.
        let store = fixture();
        let ctx = guard_ctx(Some("hash:v1"), Some(128), Some("hash:v1"), Some(128));
        let listed = handle_request_with_semantic(&store, &tools_list_req(), &ctx, None);
        assert!(
            !tool_names(&listed).contains(&"SemanticSearch".to_string()),
            "no live tool ⇒ not advertised even when the guard passes"
        );
    }

    // ── DoD-A6b ──

    #[test]
    fn dod_a6b_list_then_call_reaches_live_semantic() {
        // The dispatch-path bug: handle_tools_call resolved against bare all_tools() (no
        // SemanticSearch) so a list→call never reached a live instance. With the fix + a passing
        // guard, the call must reach the live tool — proven by the sentinel in its content.
        let store = fixture();
        let ctx = guard_ctx(Some("hash:v1"), Some(128), Some("hash:v1"), Some(128));
        let fake = FakeSemantic;

        // 1) list advertises it
        let listed = handle_request_with_semantic(&store, &tools_list_req(), &ctx, Some(&fake));
        assert!(tool_names(&listed).contains(&"SemanticSearch".to_string()));

        // 2) call reaches it (not "unknown tool")
        let called = handle_request_with_semantic(&store, &semantic_call_req(), &ctx, Some(&fake));
        assert_eq!(
            called["result"]["isError"].as_bool(),
            Some(false),
            "SemanticSearch call must succeed via the live dispatch path"
        );
        let first_text = called["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(first_text).unwrap();
        assert_eq!(
            parsed["__fake_semantic_reached"].as_bool(),
            Some(true),
            "dispatch must reach the LIVE SemanticSearch instance, not bare all_tools()"
        );
    }

    #[test]
    fn dod_a6b_hash_fallback_rides_lexical_fallback_in_response_diagnostics() {
        // Runtime embedder is the hash fallback. A SemanticSearch call must carry LEXICAL-FALLBACK
        // in the RESPONSE the agent reads (not stderr). Falsifier defeated: we assert it appears in
        // the response content, so a stderr-only emission would fail this test.
        let store = fixture();
        let ctx = guard_ctx(Some("hash:v1"), Some(128), Some("hash:v1"), Some(128));
        let fake = FakeSemantic;

        let called = handle_request_with_semantic(&store, &semantic_call_req(), &ctx, Some(&fake));
        let response_text = called["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            response_text
                .contains("LEXICAL-FALLBACK: no semantic model loaded; results are lexical"),
            "hash-fallback SemanticSearch call must ride LEXICAL-FALLBACK in the response \
             diagnostics; got: {response_text}"
        );
    }

    // ── handle_request_unified ────────────────────────────────────────────────
    //
    // ADR-ESTATE-008: domain tools unavailable (domains=None) must return a JSON-RPC error
    // (not `isError:true` in the MCP result — the AGENT must know the tool doesn't exist, not
    // that it ran and returned an error).

    struct FakeMemory;
    impl wicked_estate_memory_core::MemoryApi for FakeMemory {
        type Error = anyhow::Error;
        fn capture(
            &mut self,
            _: wicked_estate_memory_core::CaptureRequest,
        ) -> Result<String, anyhow::Error> {
            Ok("fake-mem-id".to_string())
        }
        fn recall(
            &self,
            _: &wicked_estate_memory_core::RecallQuery,
        ) -> Result<Vec<wicked_estate_memory_core::RecalledItem>, anyhow::Error> {
            Ok(vec![])
        }
        fn reflect(
            &mut self,
            scope: &str,
            _: i64,
        ) -> Result<wicked_estate_memory_core::ReflectResult, anyhow::Error> {
            Ok(wicked_estate_memory_core::ReflectResult {
                scope: scope.to_string(),
                distilled_facts: vec![],
                node_count: 0,
            })
        }
        fn erase(&mut self, _: &str, _: i64) -> Result<u32, anyhow::Error> {
            Ok(0)
        }
        fn learn(
            &mut self,
            _: &str,
            _: &[String],
            _: &std::collections::HashMap<String, u64>,
            _: i64,
        ) -> Result<String, anyhow::Error> {
            Ok("fake-learn-id".to_string())
        }
        fn coverage(
            &self,
            _: Option<&str>,
        ) -> Result<wicked_estate_memory_core::MemoryCoverage, anyhow::Error> {
            Ok(wicked_estate_memory_core::MemoryCoverage {
                total: 0,
                by_tier: Default::default(),
                by_kind: Default::default(),
            })
        }
    }

    struct FakeKnowledge;
    impl wicked_estate_knowledge::KnowledgeApi for FakeKnowledge {
        fn ingest(
            &mut self,
            _: &str,
            _: &[String],
            _: &str,
            _: &str,
            _: i64,
        ) -> anyhow::Result<String> {
            Ok("fake-doc".to_string())
        }
        fn write_node(
            &mut self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: i64,
        ) -> anyhow::Result<String> {
            Ok("fake-node".to_string())
        }
        fn relate(
            &mut self,
            _: &str,
            _: &str,
            _: &str,
            _: f64,
            _: u32,
            _: &str,
        ) -> anyhow::Result<String> {
            Ok("fake-edge".to_string())
        }
        fn recall(
            &mut self,
            _: &str,
            _: usize,
            _: i64,
        ) -> anyhow::Result<Vec<wicked_estate_knowledge::KnowledgeItem>> {
            Ok(vec![])
        }
        fn coverage(
            &self,
            _: Option<&str>,
        ) -> anyhow::Result<wicked_estate_knowledge::KnowledgeCoverage> {
            Ok(wicked_estate_knowledge::KnowledgeCoverage {
                total: 0,
                by_class: Default::default(),
                recall_miss_count: 0,
            })
        }
        fn relate_code(
            &mut self,
            _: &str,
            _: &[String],
            _: &std::collections::HashMap<String, u64>,
        ) -> anyhow::Result<u32> {
            Ok(0)
        }
        fn recall_about_code(
            &self,
            _: &[String],
        ) -> anyhow::Result<Vec<wicked_estate_knowledge::KnowledgeItem>> {
            Ok(vec![])
        }
    }

    #[test]
    fn unified_no_domains_memory_tool_returns_json_rpc_error() {
        // ADR-ESTATE-008: without domains, memory.* tools return JSON-RPC error -32601,
        // NOT isError:true in the MCP result.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0", "id": 200,
            "method": "tools/call",
            "params": { "name": "memory.capture", "arguments": { "content": "hello" } }
        });
        let resp = handle_request_unified(&store, &req, &McpContext::default(), None, None);
        assert!(
            resp.get("error").is_some(),
            "must be a JSON-RPC error (not isError result)"
        );
        assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32601);
        assert!(
            resp.get("result").is_none(),
            "error responses must not have 'result'"
        );
    }

    #[test]
    fn unified_no_domains_knowledge_tool_returns_json_rpc_error() {
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0", "id": 201,
            "method": "tools/call",
            "params": { "name": "knowledge.ingest", "arguments": { "title": "t", "chunks": ["c"] } }
        });
        let resp = handle_request_unified(&store, &req, &McpContext::default(), None, None);
        assert!(resp.get("error").is_some());
        assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32601);
    }

    #[test]
    fn unified_estate_tools_work_without_domains() {
        // Estate tools (SearchEntity, etc.) must still work when domains=None.
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0", "id": 202,
            "method": "tools/call",
            "params": { "name": "SearchEntity", "arguments": { "name": "middle_fn" } }
        });
        let resp = handle_request_unified(&store, &req, &McpContext::default(), None, None);
        assert!(
            resp.get("result").is_some(),
            "estate tool must succeed without domains"
        );
        assert!(!resp["result"]["isError"].as_bool().unwrap_or(true));
    }

    #[test]
    fn unified_tools_list_with_domains_returns_23_tools() {
        // tools/list with domains=Some → 10 estate + 6 memory + 7 knowledge = 23 tools.
        // (SemanticSearch absent: no matching dim-guard in default McpContext)
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 203, "method": "tools/list", "params": {} });
        let mut fake_mem = FakeMemory;
        let mut fake_know = FakeKnowledge;
        let mut domains = DomainHandles {
            memory: &mut fake_mem
                as &mut dyn wicked_estate_memory_core::MemoryApi<Error = anyhow::Error>,
            knowledge: &mut fake_know as &mut dyn wicked_estate_knowledge::KnowledgeApi,
        };
        let resp = handle_request_unified(
            &store,
            &req,
            &McpContext::default(),
            Some(&mut domains),
            None,
        );
        let tools = resp["result"]["tools"]
            .as_array()
            .expect("tools must be array");
        assert_eq!(
            tools.len(),
            23,
            "10 estate + 6 memory + 7 knowledge = 23; got {}",
            tools.len()
        );
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"memory.capture"),
            "memory tools must appear"
        );
        assert!(
            names.contains(&"knowledge.ingest"),
            "knowledge tools must appear"
        );
        assert!(names.contains(&"SearchEntity"), "estate tools must appear");
    }

    #[test]
    fn unified_tools_list_without_domains_returns_10_tools() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 204, "method": "tools/list", "params": {} });
        let resp = handle_request_unified(&store, &req, &McpContext::default(), None, None);
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            10,
            "without domains: 10 estate tools only; got {}",
            tools.len()
        );
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(
            !names.contains(&"memory.capture"),
            "memory tools must NOT appear without domains"
        );
        assert!(
            !names.contains(&"knowledge.ingest"),
            "knowledge tools must NOT appear without domains"
        );
    }

    #[test]
    fn unified_memory_capture_responds_via_fake_domain() {
        // Positive path: with fake domains wired, memory.capture returns memory_id (HC-007).
        let store = fixture();
        let req = json!({
            "jsonrpc": "2.0", "id": 205,
            "method": "tools/call",
            "params": { "name": "memory.capture", "arguments": { "content": "test fact", "kind": "fact", "tier": "semantic", "scope": "test" } }
        });
        let mut fake_mem = FakeMemory;
        let mut fake_know = FakeKnowledge;
        let mut domains = DomainHandles {
            memory: &mut fake_mem
                as &mut dyn wicked_estate_memory_core::MemoryApi<Error = anyhow::Error>,
            knowledge: &mut fake_know as &mut dyn wicked_estate_knowledge::KnowledgeApi,
        };
        let resp = handle_request_unified(
            &store,
            &req,
            &McpContext::default(),
            Some(&mut domains),
            None,
        );
        assert!(resp.get("result").is_some());
        assert!(!resp["result"]["isError"].as_bool().unwrap_or(true));
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(
            parsed.get("memory_id").is_some(),
            "HC-007: response must contain memory_id"
        );
    }

    #[test]
    fn unified_resources_list_and_read() {
        let store = fixture();
        let list_req =
            json!({ "jsonrpc": "2.0", "id": 206, "method": "resources/list", "params": {} });
        let resp = handle_request_unified(&store, &list_req, &McpContext::default(), None, None);
        let resources = resp["result"]["resources"]
            .as_array()
            .expect("resources must be array");
        assert!(!resources.is_empty(), "bundled skills must be listed");

        // Pick the first resource and read it back.
        let first_uri = resources[0]["uri"].as_str().unwrap().to_string();
        let read_req = json!({ "jsonrpc": "2.0", "id": 207, "method": "resources/read", "params": { "uri": first_uri } });
        let read_resp =
            handle_request_unified(&store, &read_req, &McpContext::default(), None, None);
        let contents = read_resp["result"]["contents"].as_array().unwrap();
        assert!(!contents.is_empty(), "resources/read must return content");
        assert!(
            contents[0]["text"].as_str().is_some_and(|t| !t.is_empty()),
            "skill content must be non-empty"
        );
    }

    #[test]
    fn unified_prompts_get_expedition() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 208, "method": "prompts/get", "params": { "name": "expedition" } });
        let resp = handle_request_unified(&store, &req, &McpContext::default(), None, None);
        let messages = resp["result"]["messages"]
            .as_array()
            .expect("messages must be array");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"].as_str().unwrap(), "user");
        assert!(
            messages[0]["content"]["text"]
                .as_str()
                .is_some_and(|t| !t.is_empty())
        );
    }

    #[test]
    fn dod_a6b_non_hash_runtime_has_no_lexical_fallback() {
        // Negative control: a real semantic runtime (non-hash id) must NOT carry the lexical marker.
        let store = fixture();
        let ctx = guard_ctx(
            Some("model2vec:potion-base-8M"),
            Some(256),
            Some("model2vec:potion-base-8M"),
            Some(256),
        );
        let fake = FakeSemantic;
        let called = handle_request_with_semantic(&store, &semantic_call_req(), &ctx, Some(&fake));
        let response_text = called["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !response_text.contains("LEXICAL-FALLBACK"),
            "a non-hash runtime embedder must NOT emit LEXICAL-FALLBACK"
        );
    }
}
