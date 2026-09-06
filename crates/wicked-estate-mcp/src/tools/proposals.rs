//! MCP dispatch for the 4 `proposal.*` tools (DES-MEM-FACETED-001 §5.0).
//!
//! The proposal queue is a type-generic, inert write surface: agents `proposal.submit` (safe even
//! under `--readonly` — the proposal is never recalled/applied until approved); operators
//! `proposal.approve` / `proposal.reject`. Validation mirrors `tools/memory.rs` exactly (fail-loud
//! `-32602` on a malformed arg, never a silent drop).

use serde_json::{Value, json};
use std::collections::BTreeMap;
use wicked_estate_memory_core::{ApproveOutcome, Facets, MemoryApi, Proposal, ProposalState};

pub fn dispatch(
    tool: &str,
    id: &Value,
    params: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
) -> Value {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    match tool {
        "proposal.submit" => dispatch_submit(id, &args, memory, now),
        "proposal.list" => dispatch_list(id, &args, memory),
        "proposal.approve" => dispatch_approve(id, &args, memory, now),
        "proposal.reject" => dispatch_reject(id, &args, memory, now),
        _ => json_rpc_error(id, -32602, "unknown proposal tool"),
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

/// FAIL-LOUD facets parsing — same rule as `tools/memory.rs`'s `parse_facets`: omitted/null ⇒
/// empty; a present value MUST be a JSON object deserialized through `Facets`' validated serde path.
fn parse_facets(id: &Value, args: &Value) -> Result<Facets, Value> {
    match args.get("facets") {
        None | Some(Value::Null) => Ok(Facets::default()),
        Some(v @ Value::Object(_)) => serde_json::from_value::<Facets>(v.clone())
            .map_err(|e| json_rpc_error(id, -32602, &format!("invalid facets: {e}"))),
        Some(other) => Err(json_rpc_error(
            id,
            -32602,
            &format!("invalid facets: expected a JSON object of axis:value strings, got {other}"),
        )),
    }
}

/// Authority-stamped provenance: the server reads `WICKED_RUN_ID`/`WICKED_RUN_UNIT`/
/// `WICKED_RUN_AGENT` from its OWN launch env and stamps whatever is set (empty map when none).
/// Provenance is NEVER taken from the caller's args — a worker cannot forge its own attribution.
fn stamp_provenance() -> BTreeMap<String, String> {
    let mut prov = BTreeMap::new();
    for (env_key, prov_key) in [
        ("WICKED_RUN_ID", "run_id"),
        ("WICKED_RUN_UNIT", "run_unit"),
        ("WICKED_RUN_AGENT", "run_agent"),
    ] {
        if let Ok(v) = std::env::var(env_key) {
            if !v.is_empty() {
                prov.insert(prov_key.to_string(), v);
            }
        }
    }
    prov
}

/// The wire shape for a listed proposal.
fn proposal_to_wire(p: &Proposal) -> Value {
    json!({
        "id": p.id,
        "kind_type": p.kind_type,
        "payload": p.payload,
        "facets": serde_json::to_value(&p.facets).unwrap_or_else(|_| json!({})),
        "provenance": p.provenance,
        "state": p.state.as_str(),
        "created_at": p.created_at,
    })
}

fn dispatch_submit(
    id: &Value,
    args: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let kind_type = match args.get("kind_type").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => return json_rpc_error(id, -32602, "kind_type (string) required"),
    };
    // payload MUST be a JSON object (a present non-object is invalid params, not a silent default).
    let payload = match args.get("payload") {
        Some(v @ Value::Object(_)) => v.clone(),
        _ => return json_rpc_error(id, -32602, "payload (JSON object) required"),
    };
    let facets = match parse_facets(id, args) {
        Ok(f) => f,
        Err(e) => return e,
    };
    // Provenance is stamped by the SERVER from env — never read from args.
    let provenance = stamp_provenance();
    match memory.submit_proposal(&kind_type, payload, facets, provenance, now) {
        Ok(proposal_id) => mcp_result(id, json!({"id": proposal_id})),
        // kind_type validation lives in the engine; surface it as invalid params, not an internal error.
        Err(e) => json_rpc_error(id, -32602, &e.to_string()),
    }
}

fn dispatch_list(
    id: &Value,
    args: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
) -> Value {
    // Optional kind_type filter: omitted/null ⇒ no filter; a present non-string is invalid params.
    let kind_type = match args.get("kind_type") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => {
            return json_rpc_error(
                id,
                -32602,
                &format!("invalid kind_type: expected a string, got {other}"),
            );
        }
    };
    // Optional state filter: a present value must be a known state token (fail-loud).
    let state = match args.get("state") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match ProposalState::parse(s) {
            Some(st) => Some(st),
            None => {
                return json_rpc_error(
                    id,
                    -32602,
                    &format!("invalid state {s:?}: expected pending|approved|rejected"),
                );
            }
        },
        Some(other) => {
            return json_rpc_error(
                id,
                -32602,
                &format!("invalid state: expected a string, got {other}"),
            );
        }
    };
    match memory.list_proposals(kind_type.as_deref(), state) {
        Ok(proposals) => {
            let wire: Vec<Value> = proposals.iter().map(proposal_to_wire).collect();
            mcp_result(id, json!({"proposals": wire}))
        }
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_approve(
    id: &Value,
    args: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let proposal_id = match args.get("id").and_then(|v| v.as_str()) {
        Some(i) => i.to_string(),
        None => return json_rpc_error(id, -32602, "id (string) required"),
    };
    match memory.approve_proposal(&proposal_id, now) {
        Ok(ApproveOutcome::Promoted { active_id }) => {
            mcp_result(id, json!({"outcome": "promoted", "active_id": active_id}))
        }
        Ok(ApproveOutcome::HandedOff { payload }) => {
            mcp_result(id, json!({"outcome": "handed_off", "payload": payload}))
        }
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

fn dispatch_reject(
    id: &Value,
    args: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    now: i64,
) -> Value {
    let proposal_id = match args.get("id").and_then(|v| v.as_str()) {
        Some(i) => i.to_string(),
        None => return json_rpc_error(id, -32602, "id (string) required"),
    };
    match memory.reject_proposal(&proposal_id, now) {
        Ok(()) => mcp_result(id, json!({"ok": true})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}
