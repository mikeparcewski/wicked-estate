//! MCP dispatch for the 6 `memory.*` tools.

use serde_json::{Value, json};
use std::collections::HashMap;
use wicked_estate_core::{GraphRead, SymbolId};
use wicked_estate_memory_core::{CaptureRequest, MemoryApi, RecallQuery, Scope};

pub fn dispatch(
    tool: &str,
    id: &Value,
    params: &Value,
    store: &dyn GraphRead,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
) -> Value {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    match tool {
        "memory.capture" => dispatch_capture(id, &args, store, memory, now),
        "memory.recall" => dispatch_recall(id, &args, memory, now),
        "memory.reflect" => dispatch_reflect(id, &args, memory, now),
        "memory.erase" => dispatch_erase(id, &args, memory, now),
        "memory.learn" => dispatch_learn(id, &args, store, memory, now),
        "memory.coverage" => dispatch_coverage(id, &args, memory),
        _ => json_rpc_error(id, -32602, "unknown memory tool"),
    }
}

fn json_rpc_error(id: &Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}})
}

fn ok_resp(id: &Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn mcp_result(id: &Value, content: Value) -> Value {
    ok_resp(
        id,
        json!({"content":[{"type":"text","text":content.to_string()}],"isError":false}),
    )
}

/// Fetch symbol epochs from the estate graph for a list of symbol IDs.
fn fetch_epochs(store: &dyn GraphRead, about: &[String]) -> HashMap<String, u64> {
    let mut epochs = HashMap::new();
    for sym_id in about {
        if let Ok(Some(epoch)) = store.symbol_epoch(&SymbolId(sym_id.clone())) {
            epochs.insert(sym_id.clone(), epoch);
        }
    }
    epochs
}

fn dispatch_capture(
    id: &Value,
    args: &Value,
    store: &dyn GraphRead,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json_rpc_error(id, -32602, "content required"),
    };
    // FAIL-LOUD scope validation: the engine's lenient `Scope::parse` silently
    // discards malformed (colonless / empty-kind / empty-id) segments, which
    // re-routes the write to a DIFFERENT scope than the caller asked for —
    // typically root "" — where the caller's documented `memory.erase
    // scope_prefix` can never find it again. Reject at the wire instead.
    // Omitted / empty scope stays valid (root is the documented default).
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    if let Err(e) = Scope::parse_strict(scope) {
        return json_rpc_error(id, -32602, &format!("invalid scope: {e}"));
    }
    let about: Option<Vec<String>> = args.get("about").and_then(|v| v.as_array()).map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect()
    });
    let about_epochs = about.as_ref().map(|a| fetch_epochs(store, a));
    let mut req = CaptureRequest::default();
    req.content = content;
    req.kind = args
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("episode")
        .to_string();
    req.tier = args
        .get("tier")
        .and_then(|v| v.as_str())
        .unwrap_or("episodic")
        .to_string();
    req.scope = scope.to_string();
    req.now = now;
    req.about = about;
    req.about_epochs = about_epochs;
    match memory.capture(req) {
        Ok(memory_id) => mcp_result(id, json!({"memory_id": memory_id})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_recall(
    id: &Value,
    args: &Value,
    memory: &dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json_rpc_error(id, -32602, "query required"),
    };
    let scope = args
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let seeds: Vec<String> = args
        .get("seeds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let token_budget = args
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(2000) as usize;
    let rq = RecallQuery {
        query,
        scope,
        seeds,
        token_budget,
        now,
    };
    match memory.recall(&rq) {
        Ok(items) => {
            let wire: Vec<Value> = items
                .into_iter()
                .map(|item| {
                    json!({
                        "memory_id": item.id,
                        "scope":     item.scope,
                        "content":   item.content,
                        "tier":      item.tier,
                        "score":     item.score,
                    })
                })
                .collect();
            mcp_result(id, json!({"items": wire}))
        }
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_reflect(
    id: &Value,
    args: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    match memory.reflect(scope, now) {
        Ok(result) => mcp_result(id, serde_json::to_value(&result).unwrap_or(json!({}))),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_erase(
    id: &Value,
    args: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let scope_prefix = match args.get("scope_prefix").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return json_rpc_error(id, -32602, "scope_prefix (non-empty) required for erase"),
    };
    match memory.erase(scope_prefix, now) {
        Ok(n) => mcp_result(id, json!({"deleted_count": n})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_learn(
    id: &Value,
    args: &Value,
    store: &dyn GraphRead,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    // MCP wire name is "content" (frozen golden HC-007); MemoryApi trait name is "fact"
    let fact = match args.get("content").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => return json_rpc_error(id, -32602, "content required"),
    };
    let symbols: Vec<String> = args
        .get("symbols")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let symbol_epochs = fetch_epochs(store, &symbols);
    match memory.learn(&fact, &symbols, &symbol_epochs, now) {
        Ok(memory_id) => mcp_result(id, json!({"memory_id": memory_id})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_coverage(
    id: &Value,
    args: &Value,
    memory: &dyn MemoryApi<Error = anyhow::Error>,
) -> Value {
    let scope_prefix = args.get("scope_prefix").and_then(|v| v.as_str());
    match memory.coverage(scope_prefix) {
        Ok(cov) => mcp_result(id, serde_json::to_value(&cov).unwrap_or(json!({}))),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}
