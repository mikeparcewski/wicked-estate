//! MCP dispatch for the 4 `proposal.*` tools (DES-MEM-FACETED-001 §5.0).
//!
//! The proposal queue is a type-generic, inert write surface: agents `proposal.submit` (safe even
//! under `--readonly` — the proposal is never recalled/applied until approved); operators
//! `proposal.approve` / `proposal.reject`. Validation mirrors `tools/memory.rs` exactly (fail-loud
//! `-32602` on a malformed arg, never a silent drop).

use serde_json::{Value, json};
use std::collections::BTreeMap;
use wicked_estate_core::GraphRead;
use wicked_estate_knowledge::shape::{APPLY_GRACE_SECS, check_repo_facet};
use wicked_estate_knowledge::{KnowledgeApi, parse_shape_links};
use wicked_estate_memory_core::{
    ApproveOutcome, Facets, MemoryApi, Proposal, ProposalState, valid_kind_type,
};

/// The proposal tools without a knowledge seam: a `knowledge` proposal's approval is refused
/// loudly (kept so the published signature does not change; the MCP router uses
/// [`dispatch_with_knowledge`]).
pub fn dispatch(
    tool: &str,
    id: &Value,
    params: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
) -> Value {
    dispatch_inner(tool, id, params, memory, None)
}

/// The proposal tools with the knowledge domain, so approving a `knowledge` proposal writes it.
/// `graph` is the attached code graph: a `relate_code[]` endpoint it holds is epoch-stamped.
pub fn dispatch_with_knowledge(
    tool: &str,
    id: &Value,
    params: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    knowledge: &mut dyn KnowledgeApi,
    graph: &dyn GraphRead,
) -> Value {
    dispatch_inner(tool, id, params, memory, Some((knowledge, graph)))
}

fn dispatch_inner(
    tool: &str,
    id: &Value,
    params: &Value,
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    knowledge: Option<(&mut dyn KnowledgeApi, &dyn GraphRead)>,
) -> Value {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    match tool {
        "proposal.submit" => dispatch_submit(id, &args, memory, now),
        "proposal.list" => dispatch_list(id, &args, memory),
        "proposal.approve" => dispatch_approve(id, &args, memory, knowledge, now),
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
    // Validate the kind_type SHAPE at the wire (a client argument error ⇒ -32602). With shape
    // pre-validated here, a subsequent submit_proposal error is an INTERNAL failure (-32603 below).
    if !valid_kind_type(&kind_type) {
        return json_rpc_error(
            id,
            -32602,
            &format!(
                "invalid kind_type {kind_type:?}: expected ^[a-z][a-z0-9_-]*(:[a-z][a-z0-9_-]*)?$"
            ),
        );
    }
    // payload MUST be a JSON object (a present non-object is invalid params, not a silent default).
    let payload = match args.get("payload") {
        Some(v @ Value::Object(_)) => v.clone(),
        _ => return json_rpc_error(id, -32602, "payload (JSON object) required"),
    };
    let facets = match parse_facets(id, args) {
        Ok(f) => f,
        Err(e) => return e,
    };
    // Fail-fast on built-in payloads (memory `tier`, knowledge provenance/class, policy `severity`). Without
    // this a worker's drifted value ("long_term", "durable" for tier; "high", "medium", "warning"
    // for severity) passes the INERT submit and only fails much LATER at approval — long after the
    // worker that could have corrected it is gone, so the learning is silently lost. Validating at
    // submit returns a -32602 the worker sees and RETRIES against, at capture time (LLM enum drift is
    // otherwise unfixable — an accept-and-normalize map over an unbounded synonym space is a guess).
    // Only a PRESENT value is checked (absent uses the downstream default). This deliberately couples
    // the generic queue to its built-in kinds: memory, knowledge, and policy.
    // A PRESENT value must be a string in the enum; absent/null uses the downstream default. A
    // present NON-string (number/object/bool) is invalid too — it would otherwise bypass this
    // check and fail only at approval, the very gap this guard closes.
    // `payload.reach` says what a memory or policy is ABOUT: `project` (that project/repo),
    // `operator` (how the operator works — every project), and for a memory `pattern` (a general
    // pattern the person may let cross projects). Checked here so a drifted value fails at submit.
    let reaches: &[&str] = if kind_type == "memory" {
        &["project", "pattern", "operator"]
    } else if kind_type.starts_with("policy:") {
        &["project", "operator"]
    } else {
        &[]
    };
    if !reaches.is_empty() {
        match payload.get("reach") {
            None | Some(Value::Null) => {}
            Some(Value::String(r)) if reaches.contains(&r.as_str()) => {}
            Some(other) => {
                return json_rpc_error(
                    id,
                    -32602,
                    &format!(
                        "invalid {kind_type} reach {other}: use EXACTLY one of {} (\"operator\" = how the operator works, every project)",
                        reaches.join("|")
                    ),
                );
            }
        }
    }
    // An operator memory lands at `operator:<user facet>` — one scope segment, so no `/`.
    if kind_type == "memory"
        && payload.get("reach").and_then(Value::as_str) == Some("operator")
        && facets.get("user").is_some_and(|u| u.contains('/'))
    {
        return json_rpc_error(
            id,
            -32602,
            "an operator memory's user facet names its operator scope and cannot contain '/'",
        );
    }
    if kind_type == "memory" {
        const TIERS: [&str; 5] = ["working", "episodic", "semantic", "procedural", "archival"];
        match payload.get("tier") {
            None | Some(Value::Null) => {}
            Some(Value::String(t)) if TIERS.contains(&t.as_str()) => {}
            Some(other) => {
                return json_rpc_error(
                    id,
                    -32602,
                    &format!(
                        "invalid memory tier {other}: use EXACTLY one of working|episodic|semantic|procedural|archival \
                         (a repo fact/decision is \"semantic\"; a how-to/convention is \"procedural\")"
                    ),
                );
            }
        }
    } else if kind_type == "knowledge" {
        for field in ["content", "source"] {
            if payload
                .get(field)
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
            {
                return json_rpc_error(
                    id,
                    -32602,
                    &format!("knowledge payload.{field} (non-empty string) required"),
                );
            }
        }
        if let Some(class) = payload.get("class") {
            if !matches!(
                class.as_str(),
                Some("doc" | "section" | "chunk" | "concept")
            ) {
                return json_rpc_error(
                    id,
                    -32602,
                    "invalid knowledge class: use doc|section|chunk|concept (default chunk)",
                );
            }
        }
        // A data shape's links and declared deps (DES-KNOWLEDGE-DATA-001 §4.3): shape-checked here
        // so the proposer hears a mistake; endpoints are resolved and deps derived at approve.
        match parse_shape_links(&payload)
            .and_then(|links| check_repo_facet(&links, facets.get("repo")))
        {
            Ok(()) => {}
            Err(e) => {
                return json_rpc_error(id, -32602, &format!("invalid knowledge proposal: {e}"));
            }
        }
        for field in ["scope", "observed_at", "source_rev"] {
            if payload.get(field).is_some_and(|v| !v.is_string()) {
                return json_rpc_error(
                    id,
                    -32602,
                    &format!("knowledge payload.{field} must be a string"),
                );
            }
        }
        if let Some(observed_at) = payload.get("observed_at").and_then(Value::as_str) {
            if chrono::DateTime::parse_from_rfc3339(observed_at).is_err() {
                return json_rpc_error(
                    id,
                    -32602,
                    "knowledge payload.observed_at must be an ISO-8601 timestamp with timezone",
                );
            }
        }
    } else if kind_type == "policy" || kind_type.starts_with("policy:") {
        // A policy MUST name a valid steering type as `policy:<type>` — a bare "policy" or an
        // invented suffix ("policy:data-integrity", "policy:safety") passes the generic kind_type
        // shape check but has no steering_type to land against, so it fails only at approval. Reject
        // it here so the worker retries with a real type. (These 7 mirror STEERING_TYPES.)
        const STEERING_TYPES: [&str; 7] = [
            "architecture",
            "development",
            "security",
            "testing",
            "operations",
            "compliance",
            "design-ux",
        ];
        let steering_type = kind_type.strip_prefix("policy:").unwrap_or("");
        if !STEERING_TYPES.contains(&steering_type) {
            return json_rpc_error(
                id,
                -32602,
                &format!(
                    "invalid policy kind_type {kind_type:?}: use policy:<type> where <type> is EXACTLY one of \
                     architecture|development|security|testing|operations|compliance|design-ux"
                ),
            );
        }
        const SEVERITIES: [&str; 4] = ["info", "warn", "error", "critical"];
        match payload.get("severity") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if SEVERITIES.contains(&s.as_str()) => {}
            Some(other) => {
                return json_rpc_error(
                    id,
                    -32602,
                    &format!(
                        "invalid policy severity {other}: use EXACTLY one of info|warn|error|critical \
                         (the middle band is \"warn\", not \"warning\"/\"medium\"/\"high\")"
                    ),
                );
            }
        }
    }
    // Provenance is stamped by the SERVER from env — never read from args.
    let provenance = stamp_provenance();
    match memory.submit_proposal(&kind_type, payload, facets, provenance, now) {
        Ok(proposal_id) => mcp_result(id, json!({"id": proposal_id})),
        // kind_type/payload shape is validated at the wire above ⇒ an error here is an INTERNAL
        // failure (e.g. a store write error), not a client argument error.
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
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
    knowledge: Option<(&mut dyn KnowledgeApi, &dyn GraphRead)>,
    now: i64,
) -> Value {
    let proposal_id = match args.get("id").and_then(|v| v.as_str()) {
        Some(i) => i.to_string(),
        None => return json_rpc_error(id, -32602, "id (string) required"),
    };
    let mut unverified_code = 0usize;
    // What happened to the approval journal, when it is worth telling the caller (never stderr:
    // this library also runs inside hosts whose stderr is not a terminal).
    let mut note: Option<String> = None;
    // Did the queue hand this approval to the knowledge writer (a `knowledge` proposal)?
    let mut wrote_knowledge = false;
    let approved = match knowledge {
        Some((knowledge, graph)) => {
            let approved =
                memory.approve_proposal_with_knowledge(&proposal_id, now, &mut |proposal, now| {
                    wrote_knowledge = true;
                    // Epoch-stamp every code endpoint the attached graph holds (DEC-X6).
                    let code_ids: Vec<String> = parse_shape_links(&proposal.payload)
                        .map(|l| l.relate_code.into_iter().map(|c| c.code_id).collect())
                        .unwrap_or_default();
                    let epochs = super::knowledge::fetch_epochs(graph, &code_ids);
                    unverified_code = code_ids.iter().filter(|c| !epochs.contains_key(*c)).count();
                    knowledge.write_proposal(proposal, &epochs, now)
                });
            match &approved {
                _ if !wrote_knowledge => {}
                // Invisible until complete, step 5: the proposal is approved; drop the journal.
                // Best effort — a journal left behind is completed by the next start-up sweep.
                Ok(_) => {
                    if let Err(e) = knowledge.finish_proposal(&proposal_id) {
                        note = Some(format!(
                            "approved; its journal stays until the next start-up sweep completes it ({e})"
                        ));
                    }
                }
                // A failed approve leaves nothing half-done: roll back whatever it wrote, but only
                // while the proposal is still pending (an approved one is never undone).
                Err(_) => {
                    let pending = memory
                        .list_proposals(Some("knowledge"), Some(ProposalState::Pending))
                        .map(|ps| ps.iter().any(|p| p.id == proposal_id))
                        .unwrap_or(false);
                    // Only THIS process's attempt: a concurrent retry's journal is never undone.
                    let own = knowledge.own_attempt(&proposal_id);
                    if let (true, Some(attempt)) = (pending, own) {
                        if let Err(e) = knowledge.rollback_proposal(&proposal_id, &attempt) {
                            note = Some(format!(
                                "the partial approval could not be rolled back now; the next start-up sweep will ({e})"
                            ));
                        }
                    }
                }
            }
            approved
        }
        None => memory.approve_proposal(&proposal_id, now),
    };
    match approved {
        Ok(ApproveOutcome::Promoted { active_id }) => {
            let mut out = json!({"outcome": "promoted", "active_id": active_id});
            if unverified_code > 0 {
                // The attached graph could not confirm these code endpoints (epoch 0): say so.
                out["unverified_code_endpoints"] = json!(unverified_code);
            }
            if let Some(note) = &note {
                out["warning"] = json!(note);
            }
            mcp_result(id, out)
        }
        Ok(ApproveOutcome::HandedOff { payload }) => {
            mcp_result(id, json!({"outcome": "handed_off", "payload": payload}))
        }
        Err(e) => match &note {
            Some(note) => json_rpc_error(id, -32603, &format!("{e}; {note}")),
            None => json_rpc_error(id, -32603, &e.to_string()),
        },
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

/// The start-up sweep of the invisible-until-complete protocol (DES-KNOWLEDGE-DATA-001 §10.1).
/// A knowledge approval with a journal is in flight or was interrupted; its node is hidden on every
/// read path either way. An approved proposal is completed (any age: every step is idempotent). A
/// pending, rejected or missing one is rolled back (node, edges, journal) once its journal is older
/// than [`APPLY_GRACE_SECS`]: a younger one may be another process's approval in progress. Write
/// mode only (a `--readonly` server writes nothing). `now` is unix seconds. Returns one line per
/// journal handled, for the server's log.
pub fn sweep_knowledge_approvals(
    memory: &mut dyn MemoryApi<Error = anyhow::Error>,
    knowledge: &mut dyn KnowledgeApi,
    now: i64,
) -> anyhow::Result<Vec<String>> {
    let in_flight = knowledge.applying_proposals()?;
    if in_flight.is_empty() {
        return Ok(Vec::new());
    }
    let states: BTreeMap<String, ProposalState> = memory
        .list_proposals(Some("knowledge"), None)?
        .into_iter()
        .map(|p| (p.id, p.state))
        .collect();
    let mut log = Vec::new();
    for journal in in_flight {
        let pid = journal.proposal_id;
        match states.get(&pid) {
            Some(ProposalState::Approved) => {
                knowledge.complete_proposal(&pid)?;
                log.push(format!("completed interrupted approval {pid}"));
            }
            // Possibly another process's approval in progress: leave it (it stays hidden).
            _ if now - journal.written_at < APPLY_GRACE_SECS => {}
            other => {
                // Exactly the attempt inspected: a retry that overwrote it since is left alone.
                if !knowledge.rollback_proposal(&pid, &journal.attempt)? {
                    continue;
                }
                let state = other.map(|s| s.as_str()).unwrap_or("missing");
                log.push(format!(
                    "rolled back interrupted approval {pid} (proposal {state})"
                ));
            }
        }
    }
    Ok(log)
}
