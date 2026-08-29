//! MCP dispatch for the 7 `knowledge.*` tools.

use serde_json::{Value, json};
use std::collections::HashMap;
use wicked_estate_core::{GraphRead, SymbolId};
use wicked_estate_knowledge::KnowledgeApi;

pub fn dispatch(
    tool: &str,
    id: &Value,
    params: &Value,
    store: &dyn GraphRead,
    knowledge: &mut dyn KnowledgeApi,
) -> Value {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    match tool {
        "knowledge.ingest" => dispatch_ingest(id, &args, knowledge, now),
        "knowledge.write" => dispatch_write(id, &args, knowledge, now),
        "knowledge.relate" => dispatch_relate(id, &args, knowledge),
        "knowledge.recall" => dispatch_recall(id, &args, knowledge, now),
        "knowledge.coverage" => dispatch_coverage(id, &args, knowledge),
        "knowledge.relate_code" => dispatch_relate_code(id, &args, store, knowledge),
        "knowledge.recall_about_code" => dispatch_recall_about_code(id, &args, knowledge),
        _ => json_rpc_error(id, -32602, "unknown knowledge tool"),
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

fn fetch_epochs(store: &dyn GraphRead, ids: &[String]) -> HashMap<String, u64> {
    let mut epochs = HashMap::new();
    for sym_id in ids {
        if let Ok(Some(epoch)) = store.symbol_epoch(&SymbolId(sym_id.clone())) {
            epochs.insert(sym_id.clone(), epoch);
        }
    }
    epochs
}

fn dispatch_ingest(id: &Value, args: &Value, knowledge: &mut dyn KnowledgeApi, now: i64) -> Value {
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json_rpc_error(id, -32602, "title required"),
    };
    let chunks: Vec<String> = match args.get("chunks").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => return json_rpc_error(id, -32602, "chunks required (non-empty)"),
    };
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("");
    match knowledge.ingest(&title, &chunks, scope, source, now) {
        Ok(doc_id) => mcp_result(id, json!({"doc_id": doc_id})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_write(id: &Value, args: &Value, knowledge: &mut dyn KnowledgeApi, now: i64) -> Value {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json_rpc_error(id, -32602, "content required"),
    };
    let class = args
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or("chunk");
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("");
    match knowledge.write_node(class, &content, scope, source, now) {
        Ok(node_id) => mcp_result(id, json!({"node_id": node_id})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_relate(id: &Value, args: &Value, knowledge: &mut dyn KnowledgeApi) -> Value {
    let (src, tgt, rel) = match (
        args.get("src").and_then(|v| v.as_str()),
        args.get("tgt").and_then(|v| v.as_str()),
        args.get("rel").and_then(|v| v.as_str()),
    ) {
        (Some(s), Some(t), Some(r)) => (s, t, r),
        _ => return json_rpc_error(id, -32602, "src, tgt, rel all required"),
    };
    let confidence = args
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.8);
    // evidence_count (brain consolidation): optional audit counter, default 0. Reject anything
    // that is not a non-negative integer fitting u32 — a silent `as` truncation (or a negative
    // value defaulting to 0) would corrupt the audit signal instead of surfacing the caller's bug.
    let evidence_count = match args.get("evidence_count") {
        None | Some(Value::Null) => 0u32,
        Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => n,
            None => {
                return json_rpc_error(
                    id,
                    -32602,
                    "evidence_count must be a non-negative integer <= 4294967295",
                );
            }
        },
    };
    let provenance = args
        .get("provenance")
        .and_then(|v| v.as_str())
        .unwrap_or("knowledge.relate");
    match knowledge.relate(src, tgt, rel, confidence, evidence_count, provenance) {
        Ok(edge_id) => mcp_result(id, json!({"edge_id": edge_id})),
        Err(e) => {
            // Dangling-endpoint is a tool error (isError:true), not a JSON-RPC error.
            ok_resp(
                id,
                json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
            )
        }
    }
}

fn dispatch_recall(id: &Value, args: &Value, knowledge: &mut dyn KnowledgeApi, now: i64) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json_rpc_error(id, -32602, "query required"),
    };
    let token_budget = args
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(2000) as usize;
    // Optional subtree filter (arch-R5), mirroring memory.recall's wire contract exactly:
    // omitted/null ⇒ no scope filtering (the pre-0.16 behavior); "" ⇒ root subtree = everything;
    // a present NON-string value is invalid params (fail loud — silently ignoring it would answer
    // from the WRONG visibility set, the same rule as memory.recall's scope_prefix).
    let scope_prefix: Option<String> = match args.get("scope_prefix") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => {
            return json_rpc_error(
                id,
                -32602,
                &format!("invalid scope_prefix: expected a string scope-path prefix, got {other}"),
            );
        }
    };
    match knowledge.recall(&query, token_budget, scope_prefix.as_deref(), now) {
        Ok(items) => {
            let wire: Vec<Value> = items
                .into_iter()
                .map(|item| serde_json::to_value(&item).unwrap_or(json!({})))
                .collect();
            mcp_result(id, json!({"items": wire}))
        }
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_coverage(id: &Value, args: &Value, knowledge: &dyn KnowledgeApi) -> Value {
    let class = args.get("class").and_then(|v| v.as_str());
    // Optional subtree filter — mirrors memory.coverage's lenient scope_prefix handling.
    let scope_prefix = args.get("scope_prefix").and_then(|v| v.as_str());
    match knowledge.coverage(class, scope_prefix) {
        Ok(cov) => mcp_result(id, serde_json::to_value(&cov).unwrap_or(json!({}))),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_relate_code(
    id: &Value,
    args: &Value,
    store: &dyn GraphRead,
    knowledge: &mut dyn KnowledgeApi,
) -> Value {
    let kid = match args.get("knowledge_id").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => return json_rpc_error(id, -32602, "knowledge_id required"),
    };
    let code_ids: Vec<String> = match args.get("code_ids").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => return json_rpc_error(id, -32602, "code_ids required (non-empty)"),
    };
    let symbol_epochs = fetch_epochs(store, &code_ids);
    match knowledge.relate_code(&kid, &code_ids, &symbol_epochs) {
        Ok(n) => mcp_result(id, json!({"xedge_count": n})),
        Err(e) => ok_resp(
            id,
            json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
        ),
    }
}

fn dispatch_recall_about_code(id: &Value, args: &Value, knowledge: &dyn KnowledgeApi) -> Value {
    let code_ids: Vec<String> = match args.get("code_ids").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => return json_rpc_error(id, -32602, "code_ids required (non-empty)"),
    };
    match knowledge.recall_about_code(&code_ids) {
        Ok(items) => {
            let wire: Vec<Value> = items
                .into_iter()
                .map(|item| serde_json::to_value(&item).unwrap_or(json!({})))
                .collect();
            mcp_result(id, json!({"items": wire}))
        }
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}
