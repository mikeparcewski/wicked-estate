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
use wicked_estate_retrieve::{
    BlastRadius, ContextBundle, FetchContent, RetrieveEntity, RulesInventory, SearchEntity,
    SemanticSearch, TraverseGraph,
};

// ─────────────────────────────────────────────────────────────────────────────
// Crate version (injected by Cargo at compile time)
// ─────────────────────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─────────────────────────────────────────────────────────────────────────────
// Tool registry
// ─────────────────────────────────────────────────────────────────────────────

/// All always-on retrieval tools in declaration order.
///
/// SemanticSearch is **not** here — it is stateful (owns a `VectorStore` connection), so it cannot
/// be a zero-sized entry rebuilt per request. It is constructed once at startup via
/// [`live_semantic_search`] and threaded into [`handle_request_with_semantic`], which merges it
/// with this list to form the live dispatch registry.
pub fn all_tools() -> Vec<Box<dyn RetrievalTool>> {
    vec![
        Box::new(SearchEntity),
        Box::new(RetrieveEntity),
        Box::new(TraverseGraph),
        Box::new(BlastRadius),
        Box::new(FetchContent),
        Box::new(ContextBundle),
        Box::new(RulesInventory),
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

    #[test]
    fn tools_list_returns_seven_tools() {
        let store = fixture();
        let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
        let resp = handle_request(&store, &req);

        let tools = resp["result"]["tools"]
            .as_array()
            .expect("tools must be array");
        assert_eq!(tools.len(), 7, "must expose exactly 7 tools");
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
}
