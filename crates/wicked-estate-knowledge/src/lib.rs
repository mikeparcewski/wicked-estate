//! `wicked-knowledge-mcp` — the 3rd engine's MCP stdio server over JSON-RPC 2.0, mirroring
//! `wicked-memory-mcp` (G8). Exposes the knowledge engine's **7 tools** to agents:
//! `knowledge.ingest`, `knowledge.write`, `knowledge.relate`, `knowledge.recall`,
//! `knowledge.coverage`, and the cross-store differentiator `knowledge.relate_code` /
//! `knowledge.recall_about_code` (link knowledge to code symbols, then recall it FROM a code seed).
//!
//! `lib.rs` holds the **pure dispatcher** [`handle_request`] (unit-tested with canned JSON-RPC, no
//! live client needed); `main.rs` is the stdin→dispatch→stdout loop. `now` is passed in so the
//! dispatcher stays deterministic (the binary supplies the system clock). The store is selected by
//! `$WICKED_KNOWLEDGE_DB` — knowledge keeps its **OWN file + OWN FTS + single writer** (DEC-1), so
//! FTS dilution / multi-writer contention are structurally impossible.

use serde_json::{Value, json};
use std::collections::HashMap;

pub mod engine;
pub use engine::{KClass, KNode, KRecalled, KnowledgeEngine};
pub use wicked_estate_overlay::{XEdge, XedgeStore};

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Shown to the agent at `initialize` — what this engine is and how to drive it.
pub const SERVER_INSTRUCTIONS: &str = "\
wicked-knowledge: a curated, citable knowledge base with TYPED relations — the 3rd engine alongside \
code (wicked-estate) and memory (wicked-memory). Ingest documents with `knowledge.ingest`, add a \
single node with `knowledge.write`, type a relation between two nodes with `knowledge.relate` (always \
a typed Other(\"<rel>\") edge, never an opaque see-also slug), retrieve a grounded slice with \
`knowledge.recall`, and check coverage/gaps with `knowledge.coverage`. THE DIFFERENTIATOR: link a \
knowledge node to the code symbols it concerns with `knowledge.relate_code`, then \
`knowledge.recall_about_code` surfaces that knowledge FROM a code seed — zero lexical overlap, which \
keyword search cannot bridge. The relation-typing pass \
(the ontology-expedition skill) is the bar OVER a flat brain. See resources/list for the bundled \
skills (skill://…/SKILL.md).";

// The 5 knowledge skills owned by this crate, embedded at compile time so they travel with the
// binary (D-S.1–D-S.5). `CARGO_MANIFEST_DIR` resolves both in-workspace and from this crate's own
// published tarball (the `skills/` dir ships — no `include`/`exclude` in Cargo.toml). Exposed as
// `pub const` so consumers (e.g. `wicked-estate-mcp`) reference them instead of copying the files.

/// The `knowledge-ingest` skill (SKILL.md) — supports `knowledge.ingest`.
pub const KNOWLEDGE_INGEST_SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/knowledge-ingest/SKILL.md"
));
/// The `ontology-expedition` skill (SKILL.md) — the relation-typing pass; supports `knowledge.relate`.
pub const ONTOLOGY_EXPEDITION_SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/ontology-expedition/SKILL.md"
));
/// The `knowledge-curation` skill (SKILL.md) — supports `knowledge.write`/`knowledge.relate`.
pub const KNOWLEDGE_CURATION_SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/knowledge-curation/SKILL.md"
));
/// The `cited-answer` skill (SKILL.md) — supports `knowledge.recall`.
pub const CITED_ANSWER_SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/cited-answer/SKILL.md"
));
/// The `gap-hunting` skill (SKILL.md) — supports `knowledge.coverage`.
pub const GAP_HUNTING_SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/skills/gap-hunting/SKILL.md"
));

/// The 5 knowledge skills bundled WITH this server, surfaced as MCP resources (`skill://` scheme).
const SKILLS: &[(&str, &str, &str)] = &[
    (
        "knowledge-ingest",
        "Ingest a document into the knowledge base as doc + chunks (supports knowledge.ingest).",
        KNOWLEDGE_INGEST_SKILL,
    ),
    (
        "ontology-expedition",
        "The relation-typing pass — write TYPED Other(\"<rel>\") edges between concepts (the bar over a flat brain; supports knowledge.relate).",
        ONTOLOGY_EXPEDITION_SKILL,
    ),
    (
        "knowledge-curation",
        "Resolve duplicates collapse-but-surface and keep the base clean (supports knowledge.write/relate).",
        KNOWLEDGE_CURATION_SKILL,
    ),
    (
        "cited-answer",
        "Answer a question with a grounded, cited slice (supports knowledge.recall).",
        CITED_ANSWER_SKILL,
    ),
    (
        "gap-hunting",
        "Turn recall misses into ingest tasks (supports knowledge.coverage).",
        GAP_HUNTING_SKILL,
    ),
];

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}
fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
/// Wrap a plain string in the MCP `tools/call` content envelope.
fn content(text: String) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ] })
}
/// A TOOL-execution failure (MCP `isError:true`), distinct from a JSON-RPC protocol error.
fn tool_error(text: String) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ], "isError": true })
}

fn class_of(s: &str) -> KClass {
    match s {
        "kdoc" | "doc" => KClass::Doc,
        "ksection" | "section" => KClass::Section,
        "kconcept" | "concept" => KClass::Concept,
        _ => KClass::Chunk,
    }
}

/// The 5 knowledge tools with JSON-Schema input definitions (`tools/list`).
pub fn tool_defs() -> Value {
    json!([
        {
            "name": "knowledge.ingest",
            "description": "Ingest a document: write a doc node + one chunk node per chunk (each derived_from the doc).",
            "inputSchema": { "type": "object", "required": ["title", "chunks"], "properties": {
                "title": {"type": "string"},
                "chunks": {"type": "array", "items": {"type": "string"}},
                "scope": {"type": "string"},
                "source": {"type": "string", "description": "provenance, e.g. a file path or URL"}
            }}
        },
        {
            "name": "knowledge.write",
            "description": "Write ONE knowledge node (doc/section/chunk/concept). Returns its stable id.",
            "inputSchema": { "type": "object", "required": ["content"], "properties": {
                "content": {"type": "string"},
                "class": {"type": "string", "enum": ["doc","section","chunk","concept"]},
                "scope": {"type": "string"},
                "source": {"type": "string"}
            }}
        },
        {
            "name": "knowledge.relate",
            "description": "Add ONE TYPED relation (Other(\"<rel>\") edge) between two existing node ids, with confidence + provenance. Both endpoints must have a live node (else isError).",
            "inputSchema": { "type": "object", "required": ["src", "tgt", "rel"], "properties": {
                "src": {"type": "string"},
                "tgt": {"type": "string"},
                "rel": {"type": "string", "description": "the relation type, e.g. governs, refines, contradicts"},
                "confidence": {"type": "number"},
                "provenance": {"type": "string"}
            }}
        },
        {
            "name": "knowledge.recall",
            "description": "Standalone recall: the most relevant token-budgeted knowledge slice for a query (keyword ∪ vector, RRF-fused). Each item in `items` carries `node_id`, `class`, `label`, `body_snippet`, `score`, and `source` (provenance set at ingest, e.g. a file path or URL; empty string when not recorded).",
            "inputSchema": { "type": "object", "required": ["query"], "properties": {
                "query": {"type": "string"},
                "token_budget": {"type": "integer"}
            }}
        },
        {
            "name": "knowledge.coverage",
            "description": "Coverage: node counts (optionally per class) and the count of logged recall misses (gap-hunting input).",
            "inputSchema": { "type": "object", "properties": {
                "class": {"type": "string", "enum": ["doc","section","chunk","concept"]}
            }}
        },
        {
            "name": "knowledge.relate_code",
            "description": "Cross-store link: mark a knowledge node as `about` one or more code symbols (estate symbol ids), so recall can surface it FROM those code seeds even with zero lexical overlap.",
            "inputSchema": { "type": "object", "required": ["knowledge_id", "code_ids"], "properties": {
                "knowledge_id": {"type": "string", "description": "the knowledge node id returned by knowledge.write / knowledge.ingest"},
                "code_ids": {"type": "array", "items": {"type": "string"}, "description": "estate code symbol ids this knowledge is about"}
            }}
        },
        {
            "name": "knowledge.recall_about_code",
            "description": "The cross-store differentiator: recall the knowledge linked (`about`) to one or more code symbols — surfaces grounding docs FROM a code seed across the SEPARATE knowledge store, no keyword overlap needed.",
            "inputSchema": { "type": "object", "required": ["code_ids"], "properties": {
                "code_ids": {"type": "array", "items": {"type": "string"}, "description": "estate code symbol ids to recall knowledge about"}
            }}
        }
    ])
}

/// Pure JSON-RPC dispatcher. Returns `None` for notifications (no `id`, or a `notifications/*`
/// method) per JSON-RPC 2.0 §4.1.
pub fn handle_request(
    engine: &mut KnowledgeEngine,
    xedge: &XedgeStore,
    now: i64,
    req: &Value,
) -> Option<Value> {
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = match req.get("id").cloned() {
        Some(id) if !method.starts_with("notifications/") => id,
        _ => return None,
    };
    match method {
        "initialize" => Some(ok(
            &id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {}, "resources": {} },
                "serverInfo": { "name": "wicked-knowledge", "version": SERVER_VERSION },
                "instructions": SERVER_INSTRUCTIONS
            }),
        )),
        "tools/list" => Some(ok(&id, json!({ "tools": tool_defs() }))),
        "tools/call" => Some(handle_call(engine, xedge, now, &id, req)),
        "resources/list" => Some(ok(
            &id,
            json!({ "resources": SKILLS.iter().map(|(n, d, _)| json!({
                "uri": format!("skill://{n}/SKILL.md"),
                "name": format!("{n}/SKILL.md"),
                "description": d,
                "mimeType": "text/markdown"
            })).collect::<Vec<_>>() }),
        )),
        "resources/read" => {
            let uri = req
                .get("params")
                .and_then(|p| p.get("uri"))
                .and_then(|u| u.as_str())
                .unwrap_or("");
            match SKILLS
                .iter()
                .find(|(n, _, _)| uri == format!("skill://{n}/SKILL.md"))
            {
                Some((_, _, body)) => Some(ok(
                    &id,
                    json!({ "contents": [ { "uri": uri, "mimeType": "text/markdown", "text": body } ] }),
                )),
                None => Some(err(&id, -32602, &format!("unknown resource: {uri}"))),
            }
        }
        _ => Some(err(&id, -32601, "Method not found")),
    }
}

fn handle_call(
    engine: &mut KnowledgeEngine,
    xedge: &XedgeStore,
    now: i64,
    id: &Value,
    req: &Value,
) -> Value {
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let a = params.get("arguments").cloned().unwrap_or(json!({}));
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let arr = |k: &str| {
        a.get(k)
            .and_then(|v| v.as_array())
            .map(|xs| {
                xs.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let result: Result<Value, String> = match name {
        "knowledge.ingest" => {
            let Some(title) = s("title") else {
                return err(id, -32602, "title required");
            };
            let chunks = arr("chunks");
            if chunks.is_empty() {
                return err(id, -32602, "chunks required (non-empty)");
            }
            engine
                .ingest(
                    &title,
                    &chunks,
                    &s("scope").unwrap_or_default(),
                    &s("source").unwrap_or_default(),
                    now,
                )
                .map(|(doc, ch)| content(format!("ingested doc {} + {} chunk(s)", doc.0, ch.len())))
                .map_err(|e| e.to_string())
        }
        "knowledge.write" => {
            let Some(c) = s("content") else {
                return err(id, -32602, "content required");
            };
            let kn = KNode::new(
                class_of(&s("class").unwrap_or_else(|| "chunk".into())),
                c,
                s("scope").unwrap_or_default(),
                s("source").unwrap_or_default(),
                now,
            );
            engine
                .write(&kn)
                .map(|sym| content(format!("wrote {}", sym.0)))
                .map_err(|e| e.to_string())
        }
        "knowledge.relate" => {
            let (Some(src), Some(tgt), Some(rel)) = (s("src"), s("tgt"), s("rel")) else {
                return err(id, -32602, "src, tgt, rel all required");
            };
            let conf = a.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.8);
            let prov = s("provenance").unwrap_or_else(|| "knowledge.relate".into());
            match engine.relate(
                &wicked_estate_core::SymbolId(src),
                &wicked_estate_core::SymbolId(tgt),
                &rel,
                conf,
                &prov,
            ) {
                Ok(()) => Ok(content(format!("related (typed: {rel})"))),
                // A dangling endpoint is a TOOL error (isError:true), NOT a silent Ok (R1).
                Err(e) => return ok(id, tool_error(format!("relate failed: {e}"))),
            }
        }
        "knowledge.recall" => {
            let Some(q) = s("query") else {
                return err(id, -32602, "query required");
            };
            let budget = a
                .get("token_budget")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000) as usize;
            engine
                .recall(&q, budget, now)
                .map(|hits| {
                    if hits.is_empty() {
                        content("(no relevant knowledge)".into())
                    } else {
                        let lines: Vec<String> = hits
                            .iter()
                            .map(|h| {
                                if h.source.is_empty() {
                                    h.content.clone()
                                } else {
                                    format!("{} [{}]", h.content, h.source)
                                }
                            })
                            .collect();
                        content(lines.join("\n"))
                    }
                })
                .map_err(|e| e.to_string())
        }
        "knowledge.coverage" => {
            let class = s("class").map(|c| class_of(&c));
            engine
                .count(class)
                .map(|n| {
                    content(format!(
                        "{n} knowledge node(s); {} recall miss(es) logged",
                        engine.misses().len()
                    ))
                })
                .map_err(|e| e.to_string())
        }
        "knowledge.relate_code" => {
            let Some(kid) = s("knowledge_id") else {
                return err(id, -32602, "knowledge_id required");
            };
            let codes = arr("code_ids");
            if codes.is_empty() {
                return err(id, -32602, "code_ids required (non-empty)");
            }
            // The knowledge node must be live, else we'd write a dangling about-edge (R1: tool error).
            match engine.node(&wicked_estate_core::SymbolId(kid.clone())) {
                Ok(Some(_)) => {}
                Ok(None) => return ok(id, tool_error(format!("no live knowledge node {kid}"))),
                Err(e) => return err(id, -32603, &format!("node lookup failed: {e}")),
            }
            for code in &codes {
                if let Err(e) = xedge.put_edge(&XEdge::about(kid.clone(), code.clone(), 0)) {
                    return err(id, -32603, &format!("xedge write failed: {e}"));
                }
            }
            Ok(content(format!(
                "linked knowledge {kid} --about--> {} code symbol(s)",
                codes.len()
            )))
        }
        "knowledge.recall_about_code" => {
            let codes = arr("code_ids");
            if codes.is_empty() {
                return err(id, -32602, "code_ids required (non-empty)");
            }
            let reader = xedge.reader();
            let mut seen = std::collections::BTreeSet::new();
            let mut lines: Vec<String> = Vec::new();
            for code in &codes {
                let edges = match reader.in_edges("estate", code.as_str(), &["about"]) {
                    Ok(e) => e,
                    Err(e) => return err(id, -32603, &format!("xedge read failed: {e}")),
                };
                for xe in edges {
                    let kid = xe.source.stable_id;
                    if !seen.insert(kid.clone()) {
                        continue;
                    }
                    match engine.node(&wicked_estate_core::SymbolId(kid)) {
                        Ok(Some(node)) => {
                            if let Some(kn) = KNode::from_node(&node) {
                                if kn.source.is_empty() {
                                    lines.push(kn.content);
                                } else {
                                    lines.push(format!("{} [{}]", kn.content, kn.source));
                                }
                            }
                        }
                        Ok(None) => {} // knowledge erased since the link was written — skip
                        Err(e) => return err(id, -32603, &format!("node lookup failed: {e}")),
                    }
                }
            }
            if lines.is_empty() {
                Ok(content(
                    "(no knowledge linked to those code symbols)".into(),
                ))
            } else {
                Ok(content(lines.join("\n")))
            }
        }
        other => return err(id, -32601, &format!("unknown tool: {other}")),
    };

    match result {
        Ok(v) => ok(id, v),
        Err(e) => ok(id, tool_error(format!("tool error: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> KnowledgeEngine {
        KnowledgeEngine::in_memory().unwrap()
    }

    // The cross-store differentiator, agent-facing: write a knowledge doc, link it to a code symbol
    // via knowledge.relate_code, then knowledge.recall_about_code from that code seed surfaces the doc
    // — with ZERO lexical overlap between the code id and the doc text (keyword/FTS cannot bridge it).
    #[test]
    fn recall_about_code_surfaces_knowledge_from_a_code_seed() {
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        let call = |e: &mut KnowledgeEngine, xedge: &XedgeStore, name: &str, args: Value| {
            handle_request(
                e,
                xedge,
                1,
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
            )
            .unwrap()
        };
        let text = |v: &Value| {
            v["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string()
        };

        // Write a knowledge doc; capture its id from "wrote <id>".
        let w = call(
            &mut e,
            &xedge,
            "knowledge.write",
            json!({"content":"always pass an idempotency token so a retried submission never double-charges"}),
        );
        let kid = text(&w).strip_prefix("wrote ").unwrap().to_string();

        // A code symbol id with ZERO lexical overlap with the doc.
        let code_id = "rust::checkout_service::submit";

        // cross-OFF baseline: nothing linked yet.
        let before = call(
            &mut e,
            &xedge,
            "knowledge.recall_about_code",
            json!({"code_ids":[code_id]}),
        );
        assert!(
            text(&before).contains("no knowledge linked"),
            "baseline must be empty: {before}"
        );

        // Link the doc to the code symbol.
        let rel = call(
            &mut e,
            &xedge,
            "knowledge.relate_code",
            json!({"knowledge_id":kid,"code_ids":[code_id]}),
        );
        assert!(
            text(&rel).contains("--about-->"),
            "relate_code should confirm the link: {rel}"
        );

        // cross-ON: recall FROM the code seed surfaces the doc (the differentiator).
        let after = call(
            &mut e,
            &xedge,
            "knowledge.recall_about_code",
            json!({"code_ids":[code_id]}),
        );
        assert!(
            text(&after).contains("idempotency token"),
            "cross-store recall must surface the doc from the code seed: {after}"
        );
    }

    #[test]
    fn initialize_and_tools_list_has_seven_tools() {
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        let init = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "wicked-knowledge");
        let list = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 7);
        for t in [
            "knowledge.ingest",
            "knowledge.write",
            "knowledge.relate",
            "knowledge.recall",
            "knowledge.coverage",
            "knowledge.relate_code",
            "knowledge.recall_about_code",
        ] {
            assert!(tools.iter().any(|x| x["name"] == t), "missing tool {t}");
        }
    }

    #[test]
    fn ingest_then_recall_round_trips_over_jsonrpc() {
        // T-B-KMCP behavioral gate (M2's new-crate rule): canned JSON-RPC in → ids out → recall
        // returns the chunk. NOT a test that re-states the tool count.
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        let ing = handle_request(
            &mut e, &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"knowledge.ingest","arguments":{
                    "title":"Auth design",
                    "chunks":["Sessions are signed with a rotating JWT key.","Logout revokes the refresh token."],
                    "scope":"project:auth","source":"docs/auth.md"
                }}}),
        )
        .unwrap();
        assert!(
            ing["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("ingested doc"),
            "ingest should report success"
        );
        let rec = handle_request(
            &mut e,
            &xedge,
            2,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"knowledge.recall","arguments":{"query":"how are sessions signed"}}}),
        )
        .unwrap();
        assert!(
            rec["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("jwt"),
            "recall must surface the ingested JWT chunk"
        );
    }

    #[test]
    fn recall_response_includes_source_field_on_wire() {
        // S4 gate: knowledge.recall items MUST carry `source` (the provenance set at ingest).
        // This test drives the full JSON-RPC path and inspects the raw text block — the falsifier
        // for the "source is dropped when building KnowledgeItem" regression.
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        // Ingest with a known, non-empty source.
        let _ing = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"knowledge.ingest","arguments":{
                "title":"Caching design",
                "chunks":["The cache uses LRU eviction and a 60-second TTL."],
                "scope":"project:cache","source":"docs/cache.md"
            }}}),
        )
        .unwrap();
        let rec = handle_request(
            &mut e, &xedge,
            2,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"knowledge.recall","arguments":{"query":"how does the cache evict entries"}}}),
        )
        .unwrap();
        // The standalone knowledge dispatch formats items as "content [source]" plain text,
        // NOT as JSON (JSON items are the unified MCP path in wicked-estate-mcp). Assert the
        // source appears in the bracketed suffix form — more precise than a bare path substring,
        // so body_snippet text containing the path cannot false-positive.
        let text = rec["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("[docs/cache.md]"),
            "knowledge.recall wire text must carry source in [source] suffix; got: {text:?}"
        );
    }

    #[test]
    fn write_node_persists_and_is_recallable_over_jsonrpc() {
        // T-B-KMCP (write half): write ONE node through the real JSON-RPC dispatch, then prove it
        // (a) PERSISTED — coverage's node count goes 0 → 1 — and (b) is RECALLABLE — recall surfaces
        // its distinctive content. NOT a test that re-states the tool exists.
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        // before: an empty knowledge base reports zero nodes.
        let cov0 = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"knowledge.coverage","arguments":{}}}),
        )
        .unwrap();
        assert!(
            cov0["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("0 knowledge node(s)"),
            "fresh engine must report 0 nodes"
        );
        // write a single concept node with a distinctive token.
        let w = handle_request(
            &mut e, &xedge,
            2,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"knowledge.write","arguments":{
                    "content":"The reconciliation ledger is rebuilt nightly from the append-only journal.",
                    "class":"concept","scope":"project:ledger","source":"docs/ledger.md"}}}),
        )
        .unwrap();
        let wrote = w["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            wrote.starts_with("wrote "),
            "write should report the new id, got: {wrote}"
        );
        assert!(
            w["result"].get("isError").is_none(),
            "a valid write must NOT be a tool error"
        );
        // (a) persisted: coverage now reports exactly one node.
        let cov1 = handle_request(
            &mut e,
            &xedge,
            3,
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                "name":"knowledge.coverage","arguments":{}}}),
        )
        .unwrap();
        assert!(
            cov1["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("1 knowledge node(s)"),
            "after one write, coverage must report 1 node, got: {}",
            cov1["result"]["content"][0]["text"]
        );
        // (b) recallable: recall surfaces the just-written content by its distinctive token.
        let rec = handle_request(
            &mut e, &xedge,
            4,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                "name":"knowledge.recall","arguments":{"query":"how is the reconciliation ledger rebuilt"}}}),
        )
        .unwrap();
        assert!(
            rec["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("reconciliation ledger"),
            "recall must surface the written node, got: {}",
            rec["result"]["content"][0]["text"]
        );
    }

    #[test]
    fn coverage_reports_node_count_and_recall_misses_over_jsonrpc() {
        // T-B-COVERAGE: coverage is the gap-hunting metric — it must report the RIGHT counts.
        // Assert all three facets it exposes: a recall MISS against the empty base is tallied, the
        // total node count after a known fixture (1 doc + 2 chunks) is exact, and the per-class
        // count is exact. (The miss is driven against the empty base because that is the one
        // recall outcome that is empty DETERMINISTICALLY — the vector ANN always returns its k
        // nearest once any embedding exists, so an "off-topic" query on a populated base is not a
        // reliable miss; an empty base has nothing to return.)
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        // 1) baseline: empty base, no misses.
        let cov0 = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"knowledge.coverage","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(
            cov0["result"]["content"][0]["text"].as_str().unwrap(),
            "0 knowledge node(s); 0 recall miss(es) logged",
            "fresh base: zero nodes, zero misses"
        );
        // 2) a recall against the empty base is an honest miss → tallied by coverage.
        let miss = handle_request(
            &mut e, &xedge,
            2,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"knowledge.recall","arguments":{"query":"how does the cache evict entries"}}}),
        )
        .unwrap();
        assert!(
            miss["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("no relevant knowledge"),
            "recall on an empty base must be the honest-empty marker"
        );
        let cov_miss = handle_request(
            &mut e,
            &xedge,
            3,
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                "name":"knowledge.coverage","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(
            cov_miss["result"]["content"][0]["text"].as_str().unwrap(),
            "0 knowledge node(s); 1 recall miss(es) logged",
            "coverage must surface the one logged recall miss (the gap-hunting input)"
        );
        // 3) ingest a known fixture: 1 doc + 2 chunks = 3 nodes; the miss tally is unchanged.
        handle_request(
            &mut e, &xedge,
            4,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                "name":"knowledge.ingest","arguments":{
                    "title":"Caching design",
                    "chunks":["The cache uses an LRU eviction policy.","Stale entries expire after sixty seconds."],
                    "scope":"project:cache","source":"docs/cache.md"
                }}}),
        )
        .unwrap();
        let cov = handle_request(
            &mut e,
            &xedge,
            5,
            &json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
                "name":"knowledge.coverage","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(
            cov["result"]["content"][0]["text"].as_str().unwrap(),
            "3 knowledge node(s); 1 recall miss(es) logged",
            "coverage must count 1 doc + 2 chunks, miss tally still 1"
        );
        // 4) per-class: exactly one doc node (the fixture's single doc).
        let cov_doc = handle_request(
            &mut e,
            &xedge,
            6,
            &json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
                "name":"knowledge.coverage","arguments":{"class":"doc"}}}),
        )
        .unwrap();
        assert!(
            cov_doc["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("1 knowledge node(s)"),
            "per-class coverage must report exactly 1 doc node, got: {}",
            cov_doc["result"]["content"][0]["text"]
        );
    }

    #[test]
    fn relate_dangling_target_is_tool_error_not_silent_ok() {
        // T-B-RELATE (B-2/B6, the same-store half): relate to a node-less target returns isError:true,
        // never a silent Ok that persists a dangling-but-traversable edge.
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        // write one real node to be the source.
        let w = handle_request(
            &mut e, &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"knowledge.write","arguments":{"content":"a real concept","class":"concept"}}}),
        )
        .unwrap();
        let src_line = w["result"]["content"][0]["text"].as_str().unwrap();
        let src_id = src_line.trim_start_matches("wrote ").to_string();
        let rel = handle_request(
            &mut e,
            &xedge,
            2,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"knowledge.relate","arguments":{
                    "src":src_id,"tgt":"synthetic-does-not-exist","rel":"governs"}}}),
        )
        .unwrap();
        assert_eq!(
            rel["result"]["isError"], true,
            "a dangling target must be isError:true"
        );
    }

    #[test]
    fn five_skills_bundled_as_resources() {
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        let list = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
        )
        .unwrap();
        let rs = list["result"]["resources"].as_array().unwrap();
        assert_eq!(rs.len(), 5);
        for n in [
            "knowledge-ingest",
            "ontology-expedition",
            "knowledge-curation",
            "cited-answer",
            "gap-hunting",
        ] {
            let uri = format!("skill://{n}/SKILL.md");
            assert!(rs.iter().any(|r| r["uri"] == uri), "missing skill {n}");
            let read = handle_request(
                &mut e,
                &xedge,
                1,
                &json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":uri}}),
            )
            .unwrap();
            assert!(
                !read["result"]["contents"][0]["text"]
                    .as_str()
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn notifications_have_no_response_and_unknowns_error() {
        let mut e = engine();
        let xedge = XedgeStore::in_memory().unwrap();
        assert!(
            handle_request(
                &mut e,
                &xedge,
                1,
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .is_none()
        );
        let m = handle_request(
            &mut e,
            &xedge,
            1,
            &json!({"jsonrpc":"2.0","id":9,"method":"bogus"}),
        )
        .unwrap();
        assert_eq!(m["error"]["code"], -32601);
        let t = handle_request(&mut e, &xedge, 1, &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"knowledge.nope","arguments":{}}})).unwrap();
        assert_eq!(t["error"]["code"], -32601);
        let r = handle_request(&mut e, &xedge, 1, &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"knowledge.recall","arguments":{}}})).unwrap();
        assert_eq!(r["error"]["code"], -32602);
    }
}

// ── KnowledgeApi wire types (HC-007 frozen schemas) ──────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeItem {
    pub node_id: String,
    pub class: String,
    pub label: String,
    pub body_snippet: String,
    pub score: f64,
    /// Provenance of the knowledge node — the `source` field set at ingest time (e.g. a file path
    /// or URL). Empty string when no provenance was recorded. Always present on the wire (S4).
    /// `#[serde(default)]` keeps deserialization backward-compatible: older responses that
    /// predate this field deserialize to `""` rather than erroring.
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeCoverage {
    pub total: u32,
    pub by_class: HashMap<String, u32>,
    pub recall_miss_count: u32,
}

// ── KnowledgeApi trait ────────────────────────────────────────────────────────

pub trait KnowledgeApi {
    fn ingest(
        &mut self,
        title: &str,
        chunks: &[String],
        scope: &str,
        source: &str,
        now: i64,
    ) -> anyhow::Result<String>;
    fn write_node(
        &mut self,
        class: &str,
        content: &str,
        scope: &str,
        source: &str,
        now: i64,
    ) -> anyhow::Result<String>;
    fn relate(
        &mut self,
        src_id: &str,
        tgt_id: &str,
        rel: &str,
        confidence: f64,
        provenance: &str,
    ) -> anyhow::Result<String>;
    fn recall(
        &mut self,
        query: &str,
        token_budget: usize,
        now: i64,
    ) -> anyhow::Result<Vec<KnowledgeItem>>;
    fn coverage(&self, class: Option<&str>) -> anyhow::Result<KnowledgeCoverage>;
    fn relate_code(
        &mut self,
        knowledge_id: &str,
        symbol_ids: &[String],
        symbol_epochs: &HashMap<String, u64>,
    ) -> anyhow::Result<u32>;
    fn recall_about_code(&self, symbol_ids: &[String]) -> anyhow::Result<Vec<KnowledgeItem>>;
}

impl KnowledgeApi for KnowledgeEngine {
    fn ingest(
        &mut self,
        title: &str,
        chunks: &[String],
        scope: &str,
        source: &str,
        now: i64,
    ) -> anyhow::Result<String> {
        let (doc_sym, _) = KnowledgeEngine::ingest(self, title, chunks, scope, source, now)?;
        Ok(doc_sym.0)
    }

    fn write_node(
        &mut self,
        class: &str,
        content: &str,
        scope: &str,
        source: &str,
        now: i64,
    ) -> anyhow::Result<String> {
        let kn = KNode::new(class_of(class), content, scope, source, now);
        let sym = KnowledgeEngine::write(self, &kn)?;
        Ok(sym.0)
    }

    fn relate(
        &mut self,
        src_id: &str,
        tgt_id: &str,
        rel: &str,
        confidence: f64,
        provenance: &str,
    ) -> anyhow::Result<String> {
        let src = wicked_estate_core::SymbolId(src_id.to_string());
        let tgt = wicked_estate_core::SymbolId(tgt_id.to_string());
        if KnowledgeEngine::node(self, &src)?.is_none() {
            anyhow::bail!("no live knowledge node {src_id}");
        }
        KnowledgeEngine::relate(self, &src, &tgt, rel, confidence, provenance)?;
        Ok(format!("{src_id}--{rel}-->{tgt_id}"))
    }

    fn recall(
        &mut self,
        query: &str,
        token_budget: usize,
        now: i64,
    ) -> anyhow::Result<Vec<KnowledgeItem>> {
        let hits = KnowledgeEngine::recall(self, query, token_budget, now)?;
        Ok(hits
            .into_iter()
            .map(|h| KnowledgeItem {
                node_id: h.id.0,
                class: "chunk".to_string(),
                label: h.content.chars().take(60).collect(),
                body_snippet: h.content,
                score: h.score,
                source: h.source,
            })
            .collect())
    }

    fn coverage(&self, class: Option<&str>) -> anyhow::Result<KnowledgeCoverage> {
        let kclass = class.map(class_of);
        let total = KnowledgeEngine::count(self, kclass)? as u32;
        let mut by_class: HashMap<String, u32> = HashMap::new();
        for &kc in &[KClass::Doc, KClass::Section, KClass::Chunk, KClass::Concept] {
            let n = KnowledgeEngine::count(self, Some(kc))? as u32;
            by_class.insert(kc.as_kind().to_string(), n);
        }
        Ok(KnowledgeCoverage {
            total,
            by_class,
            recall_miss_count: self.misses().len() as u32,
        })
    }

    fn relate_code(
        &mut self,
        knowledge_id: &str,
        _symbol_ids: &[String],
        symbol_epochs: &HashMap<String, u64>,
    ) -> anyhow::Result<u32> {
        let kid_sym = wicked_estate_core::SymbolId(knowledge_id.to_string());
        if KnowledgeEngine::node(self, &kid_sym)?.is_none() {
            anyhow::bail!("no live knowledge node {knowledge_id}");
        }
        let xedge = self
            .xedge
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("xedge store not available"))?;
        let mut n = 0u32;
        for (sym_id, &epoch) in symbol_epochs {
            xedge.put_edge(&XEdge::about(knowledge_id, sym_id.as_str(), epoch))?;
            n += 1;
        }
        Ok(n)
    }

    fn recall_about_code(&self, symbol_ids: &[String]) -> anyhow::Result<Vec<KnowledgeItem>> {
        let xedge = self
            .xedge
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("xedge store not available"))?;
        let reader = xedge.reader();
        let mut items = Vec::new();
        for sym_id in symbol_ids {
            let edges = reader.in_edges("estate", sym_id, &["about"])?;
            for edge in edges {
                let kid = wicked_estate_core::SymbolId(edge.source.stable_id.clone());
                if let Some(node) = KnowledgeEngine::node(self, &kid)? {
                    if let Some(kn) = KNode::from_node(&node) {
                        let label: String = kn.content.chars().take(60).collect();
                        items.push(KnowledgeItem {
                            node_id: edge.source.stable_id.clone(),
                            class: kn.class.as_kind().to_string(),
                            label,
                            body_snippet: kn.content,
                            score: edge.confidence as f64,
                            source: kn.source,
                        });
                    }
                }
            }
        }
        Ok(items)
    }
}
