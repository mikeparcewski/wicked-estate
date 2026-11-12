//! MCP dispatch for the 10 `knowledge.*` tools: the 7 document tools plus the knowledge plane's
//! `knowledge.replace_source`, `knowledge.source_catalog` and `knowledge.neighbors`
//! (DES-KNOWLEDGE-DATA-001 §4, §10.1), with the document-owned links and `control` endpoints of
//! DES-PM-001 §3.4.

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use wicked_estate_core::{GraphRead, SymbolId};
use wicked_estate_knowledge::links::validate_links;
use wicked_estate_knowledge::plane::{RUN_KIND, control_deps, shown, valid_project_id};
use wicked_estate_knowledge::shape::{
    CONTROL_STORE, ESTATE_STORE, KNOWLEDGE_STORE, MAX_NEIGHBOR_DEPTH,
};
use wicked_estate_knowledge::{
    DocLink, KnowledgeApi, LinkDir, LinkEnd, NeighborsQuery, SourceDoc, SourceGeneration,
    SourceSection, Visible,
};
use wicked_estate_overlay::{TYPED_RELS, is_typed_rel};

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
        "knowledge.replace_source" => dispatch_replace_source(id, &args, knowledge, now),
        "knowledge.source_catalog" => dispatch_source_catalog(id, &args, knowledge),
        "knowledge.neighbors" => dispatch_neighbors(id, &args, store, knowledge),
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

/// A tool-execution failure (MCP `isError:true`), distinct from a JSON-RPC protocol error.
fn tool_error(id: &Value, text: String) -> Value {
    ok_resp(
        id,
        json!({"content":[{"type":"text","text":text}],"isError":true}),
    )
}

/// `visible` (DES-KNOWLEDGE-DATA-001 §4.3): the `source:<id>` / `repo:<id>` / `project:<id>` the
/// caller may see.
/// Omitted or null = the empty set (only dep-free knowledge). A present value must be an array of
/// well-formed deps — a malformed one is invalid params, never a silently smaller set.
pub(crate) fn parse_visible(id: &Value, args: &Value) -> Result<Visible, Value> {
    match args.get("visible") {
        None | Some(Value::Null) => Ok(Visible::none()),
        Some(Value::Array(xs)) => {
            let mut deps = Vec::with_capacity(xs.len());
            for x in xs {
                match x.as_str() {
                    Some(d) => deps.push(d),
                    None => {
                        return Err(json_rpc_error(
                            id,
                            -32602,
                            "invalid visible: every entry must be a source:<id>, repo:<id> or project:<id> string",
                        ));
                    }
                }
            }
            Visible::from_deps(deps).map_err(|e| json_rpc_error(id, -32602, &e))
        }
        Some(other) => Err(json_rpc_error(
            id,
            -32602,
            &format!(
                "invalid visible: expected an array of source:<id> / repo:<id> / project:<id>, got {}",
                shown(&other.to_string())
            ),
        )),
    }
}

pub(crate) fn fetch_epochs(store: &dyn GraphRead, ids: &[String]) -> HashMap<String, u64> {
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
    // Exactly one input mode, each field its own type: a malformed field is invalid params, never
    // silently ignored in favour of the other mode.
    let path = match args.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(p)) => Some(p.as_str()),
        Some(_) => return json_rpc_error(id, -32602, "path must be a string"),
    };
    let given_chunks = match args.get("chunks") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(a),
        Some(_) => return json_rpc_error(id, -32602, "chunks must be an array of strings"),
    };
    let chunks: Vec<String> = match (path, given_chunks) {
        (Some(_), Some(_)) => {
            return json_rpc_error(id, -32602, "give chunks or path, not both");
        }
        // A binary document: estate converts it to Markdown and chunks it (doc_convert).
        (Some(p), None) => match document_chunks(p) {
            Ok(chunks) => chunks,
            Err(reason) => return tool_error(id, format!("{p}: {reason}")),
        },
        (None, Some(a)) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => {
            return json_rpc_error(
                id,
                -32602,
                "chunks required (non-empty), or a document path",
            );
        }
    };
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .or(path)
        .unwrap_or("");
    match knowledge.ingest(&title, &chunks, scope, source, now) {
        Ok(doc_id) => mcp_result(id, json!({"doc_id": doc_id})),
        Err(e) => json_rpc_error(id, -32603, &e.to_string()),
    }
}

/// The chunks of the binary document at `path` (pdf, docx, pptx, xlsx, ...): read locally,
/// converted to Markdown in-process, redacted with the same rules as any `.md`, split at H1/H2.
/// The read is confined like the indexer's (wicked-estate#405): `path` must resolve, symlinks and
/// `..` included, inside the server's working directory, so this tool reads no more of the disk
/// than an index of that directory would. A file the secret-file rule excludes is never read.
/// Every refusal is a fixed reason, never document text.
fn document_chunks(path: &str) -> Result<Vec<String>, String> {
    use wicked_estate_extract::{doc_convert, redact};
    if !doc_convert::is_doc_path(path) {
        return Err(
            "not a binary document (pdf, doc/docx, ppt/pptx, xls/xlsx, odt/odp/ods, rtf, epub); \
             send its text as chunks"
                .into(),
        );
    }
    let cwd = std::env::current_dir().map_err(|_| "no working directory".to_string())?;
    let resolved = wicked_estate::confined::ConfinedRoot::new(&cwd)
        .and_then(|root| root.resolve(std::path::Path::new(path)))
        .ok_or_else(|| "not found inside the server's working directory; not read".to_string())?;
    if redact::is_excluded(path) || redact::is_excluded(&resolved.to_string_lossy()) {
        return Err("matches the secret-file rule; not read".into());
    }
    let bytes = doc_convert::read_capped(&resolved).map_err(|e| e.to_string())?;
    let md = doc_convert::to_markdown(path, &bytes).map_err(|e| e.to_string())?;
    let md = redact::redact_source(&format!("{path}.md"), &md);
    let chunks = doc_convert::chunk_markdown(&md);
    if chunks.is_empty() {
        return Err("no text".into());
    }
    Ok(chunks)
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
    let visible = match parse_visible(id, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match knowledge.recall_hits(&query, token_budget, scope_prefix.as_deref(), &visible, now) {
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
    let visible = match parse_visible(id, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match knowledge.recall_about_code_hits(&code_ids, &visible) {
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

/// A non-negative integer argument, or `default` when absent/null; anything else is invalid params.
fn opt_u64(
    id: &Value,
    args: &Value,
    field: &str,
    default: Option<u64>,
) -> Result<Option<u64>, Value> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| {
            json_rpc_error(
                id,
                -32602,
                &format!("{field} must be a non-negative integer"),
            )
        }),
    }
}

fn opt_string(id: &Value, v: Option<&Value>, at: &str) -> Result<Option<String>, Value> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(json_rpc_error(
            id,
            -32602,
            &format!("{at} must be a string"),
        )),
    }
}

fn opt_object(id: &Value, v: Option<&Value>, at: &str) -> Result<Option<Value>, Value> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(o @ Value::Object(_)) => Ok(Some(o.clone())),
        Some(_) => Err(json_rpc_error(
            id,
            -32602,
            &format!("{at} must be a JSON object"),
        )),
    }
}

/// The links one document owns (`docs[i].links`, DES-PM-001 §3.4), wire types only; the engine's
/// `validate_links` checks the rest before anything is read or written.
fn parse_doc_links(id: &Value, raw: Option<&Value>, at: &str) -> Result<Vec<DocLink>, Value> {
    let xs = match raw {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(xs)) => xs,
        Some(_) => {
            return Err(json_rpc_error(
                id,
                -32602,
                &format!("{at}.links must be an array"),
            ));
        }
    };
    let mut out = Vec::with_capacity(xs.len());
    for (k, l) in xs.iter().enumerate() {
        let at = format!("{at}.links[{k}]");
        let field = |name: &str| l.get(name).and_then(Value::as_str);
        let shape = || {
            json_rpc_error(
                id,
                -32602,
                &format!(
                    "{at} must be {{rel, dir: \"out\"|\"in\", end: {{store: \"control\"|\"knowledge\", id}}, deps?}}"
                ),
            )
        };
        let (Some(rel), Some(dir), Some(end)) = (
            field("rel"),
            field("dir").and_then(LinkDir::parse),
            l.get("end"),
        ) else {
            return Err(shape());
        };
        let (Some(store), Some(end_id)) = (
            end.get("store").and_then(Value::as_str),
            end.get("id").and_then(Value::as_str),
        ) else {
            return Err(shape());
        };
        let deps = match l.get("deps") {
            None | Some(Value::Null) => None,
            Some(Value::Array(ds)) => {
                let mut v = Vec::with_capacity(ds.len());
                for d in ds {
                    match d.as_str() {
                        Some(d) => v.push(d.to_string()),
                        None => return Err(shape()),
                    }
                }
                Some(v)
            }
            Some(_) => return Err(shape()),
        };
        out.push(DocLink {
            rel: rel.to_string(),
            dir,
            end: LinkEnd {
                store: store.to_string(),
                id: end_id.to_string(),
            },
            deps,
        });
    }
    Ok(out)
}

/// Parse a `knowledge.replace_source` request into a generation and each document's `links`, by
/// document key (wire types only; the engine's `SourceGeneration::validate` and `validate_links`
/// check the rest before anything is written).
fn parse_linked_generation(
    id: &Value,
    args: &Value,
) -> Result<(SourceGeneration, BTreeMap<String, Vec<DocLink>>), Value> {
    let source_id = match args.get("sourceId").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None => return Err(json_rpc_error(id, -32602, "sourceId (string) required")),
    };
    let generation = match opt_u64(id, args, "generation", None)? {
        Some(g) => g,
        None => {
            return Err(json_rpc_error(
                id,
                -32602,
                "generation (integer >= 1) required",
            ));
        }
    };
    let label = opt_string(id, args.get("label"), "label")?;
    let catalog = opt_object(id, args.get("catalog"), "catalog")?;
    let Some(raw_docs) = args.get("docs").and_then(Value::as_array) else {
        return Err(json_rpc_error(
            id,
            -32602,
            "docs (array, the COMPLETE generation; [] = the source has no relations) required",
        ));
    };
    let mut docs = Vec::with_capacity(raw_docs.len());
    let mut links: BTreeMap<String, Vec<DocLink>> = BTreeMap::new();
    for (i, d) in raw_docs.iter().enumerate() {
        let at = format!("docs[{i}]");
        if !d.is_object() {
            return Err(json_rpc_error(
                id,
                -32602,
                &format!("{at} must be an object"),
            ));
        }
        let Some(key) = d.get("key").and_then(Value::as_str) else {
            return Err(json_rpc_error(
                id,
                -32602,
                &format!("{at}.key (string) required"),
            ));
        };
        let Some(raw_sections) = d.get("sections").and_then(Value::as_array) else {
            return Err(json_rpc_error(
                id,
                -32602,
                &format!("{at}.sections (array of {{key, text}}) required"),
            ));
        };
        let mut sections = Vec::with_capacity(raw_sections.len());
        for (j, sec) in raw_sections.iter().enumerate() {
            match (
                sec.get("key").and_then(Value::as_str),
                sec.get("text").and_then(Value::as_str),
            ) {
                (Some(k), Some(t)) => sections.push(SourceSection {
                    key: k.to_string(),
                    text: t.to_string(),
                }),
                _ => {
                    return Err(json_rpc_error(
                        id,
                        -32602,
                        &format!("{at}.sections[{j}] must be {{key: string, text: string}}"),
                    ));
                }
            }
        }
        let doc_links = parse_doc_links(id, d.get("links"), &at)?;
        if !doc_links.is_empty() {
            // A duplicate doc key is refused by validate(); its links never merge.
            links.entry(key.to_string()).or_default().extend(doc_links);
        }
        docs.push(SourceDoc {
            key: key.to_string(),
            title: opt_string(id, d.get("title"), &format!("{at}.title"))?,
            entry: opt_object(id, d.get("entry"), &format!("{at}.entry"))?,
            sections,
        });
    }
    Ok((
        SourceGeneration {
            source_id,
            generation,
            label,
            catalog,
            docs,
        },
        links,
    ))
}

fn dispatch_replace_source(
    id: &Value,
    args: &Value,
    knowledge: &mut dyn KnowledgeApi,
    now: i64,
) -> Value {
    let (generation, links) = match parse_linked_generation(id, args) {
        Ok(g) => g,
        Err(e) => return e,
    };
    // Validate the WHOLE generation and its links before anything is written: a malformed one is
    // a client error and writes nothing, tombstones nothing.
    if let Err(e) = generation.validate() {
        return json_rpc_error(id, -32602, &e.to_string());
    }
    if let Err(e) = validate_links(&generation, &links) {
        return json_rpc_error(id, -32602, &e);
    }
    match knowledge.replace_source_linked(&generation, &links, now) {
        Ok(linked) => {
            let r = &linked.report;
            mcp_result(
                id,
                json!({
                    "sourceId": r.source_id,
                    "generation": r.generation,
                    "docs": r.docs,
                    "sections": r.sections,
                    "tombstoned": {
                        "nodes": r.tombstoned_nodes,
                        "doc_count": r.tombstoned_doc_count,
                        "docs": r.tombstoned_docs,
                    },
                    "orphaned_edges": {
                        "count": r.orphaned_edge_count,
                        "edges": r.orphaned_edges.iter().map(|e| json!({
                            "rel": e.rel, "from": e.from, "to": e.to, "owner": e.owner,
                        })).collect::<Vec<_>>(),
                    },
                    "links": {
                        "written": linked.links,
                        "stale_removed": linked.stale_removed,
                        "cleanup": linked.cleanup_error,
                    },
                }),
            )
        }
        Err(e) => tool_error(
            id,
            format!(
                "replace_source committed nothing and tombstoned nothing (any link rows it staged \
                 stay hidden, and the source's next generation deletes them): {e}"
            ),
        ),
    }
}

fn dispatch_source_catalog(id: &Value, args: &Value, knowledge: &dyn KnowledgeApi) -> Value {
    let Some(source_id) = args.get("sourceId").and_then(Value::as_str) else {
        return json_rpc_error(id, -32602, "sourceId (string) required");
    };
    let page = match opt_u64(id, args, "page", Some(0)) {
        Ok(p) => p.unwrap_or(0) as usize,
        Err(e) => return e,
    };
    let pinned = match opt_u64(id, args, "generation", None) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let visible = match parse_visible(id, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match knowledge.source_catalog(source_id, page, pinned, &visible) {
        Ok(Some(c)) => mcp_result(
            id,
            json!({
                "sourceId": c.source_id,
                "label": c.label,
                "generation": c.generation,
                "page": c.page,
                "pages": c.pages,
                "total_docs": c.total_docs,
                "untrusted": true,
                "catalog": c.catalog,
                "docs": c.docs,
            }),
        ),
        Ok(None) => tool_error(
            id,
            format!(
                "no visible catalog for source {:?} (pass visible with source:<that id> to read one)",
                shown(source_id)
            ),
        ),
        Err(e) => tool_error(id, format!("source_catalog: {e}")),
    }
}

fn dispatch_neighbors(
    id: &Value,
    args: &Value,
    store: &dyn GraphRead,
    knowledge: &dyn KnowledgeApi,
) -> Value {
    let Some(start) = args.get("id").and_then(Value::as_str) else {
        return json_rpc_error(id, -32602, "id (string) required");
    };
    let start_store = match args.get("store") {
        None | Some(Value::Null) => KNOWLEDGE_STORE.to_string(),
        Some(Value::String(s))
            if s == KNOWLEDGE_STORE || s == ESTATE_STORE || s == CONTROL_STORE =>
        {
            s.clone()
        }
        Some(_) => {
            return json_rpc_error(
                id,
                -32602,
                "store must be \"knowledge\", \"estate\" or \"control\"",
            );
        }
    };
    // A control id is syntax the caller wrote: a malformed one is invalid params (it reveals
    // nothing); a hidden or unknown well-formed one answers as an unknown node.
    if start_store == CONTROL_STORE {
        let well_formed = match start.split_once(':') {
            Some((RUN_KIND, run)) => valid_project_id(run),
            _ => control_deps(start, None).is_ok(),
        };
        if !well_formed {
            return json_rpc_error(
                id,
                -32602,
                "a control id is project:<id>, repo:<id>, pr:<repo id>#<n> or run:<id>",
            );
        }
    }
    let rels: Vec<String> = match args.get("kinds") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(xs)) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                match x.as_str() {
                    Some(r) if is_typed_rel(r) => out.push(r.to_string()),
                    _ => {
                        return json_rpc_error(
                            id,
                            -32602,
                            &format!("kinds entries must be one of {}", TYPED_RELS.join("|")),
                        );
                    }
                }
            }
            out
        }
        Some(_) => return json_rpc_error(id, -32602, "kinds must be an array of typed rels"),
    };
    let depth = match opt_u64(id, args, "depth", Some(1)) {
        Ok(Some(d)) if (1..=MAX_NEIGHBOR_DEPTH as u64).contains(&d) => d as usize,
        Ok(_) => {
            return json_rpc_error(id, -32602, &format!("depth must be 1-{MAX_NEIGHBOR_DEPTH}"));
        }
        Err(e) => return e,
    };
    let visible = match parse_visible(id, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let q = NeighborsQuery {
        store: start_store,
        id: start.to_string(),
        rels,
        depth,
        visible,
    };
    match knowledge.neighbors(&q, store) {
        Ok(Some(n)) => mcp_result(
            id,
            json!({
                "nodes": n.nodes.iter().map(|x| x.to_json()).collect::<Vec<_>>(),
                "edges": n.edges.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
                "truncated": n.truncated,
            }),
        ),
        Ok(None) => tool_error(
            id,
            format!(
                "no visible {} node {:?}",
                if q.store == CONTROL_STORE {
                    "control"
                } else {
                    "knowledge"
                },
                shown(start)
            ),
        ),
        Err(e) => tool_error(id, format!("neighbors: {e}")),
    }
}
