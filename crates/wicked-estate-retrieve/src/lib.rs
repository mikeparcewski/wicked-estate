//! `wicked-estate-retrieve` — RetrievalTool impls: 4-tool agent API + RRF hybrid (Wave 4.3, 5.3).
//!
//! # Tools
//!
//! | Tool name          | Purpose                                               |
//! |--------------------|-------------------------------------------------------|
//! | `SearchEntity`     | Find symbols by exact / substring name                |
//! | `RetrieveEntity`   | Full detail for a single symbol id                    |
//! | `TraverseGraph`    | Bounded multi-hop walk from a start symbol            |
//! | `BlastRadius`      | Transitive dependents (reverse-reachability on Calls) |
//! | `Lineage`          | Transitive dependencies (what a symbol depends on)    |
//! | `RankHotspots`     | Most central symbols by PageRank (W4.1)               |
//! | `Communities`      | Louvain communities + summaries (W4.1)                |
//! | `ContextPack`      | Token-budgeted ranked elided-stub context (W4.2)      |
//! | `SemanticSearch`   | Embedding-based ANN search (W5.2)                     |
//! | `RulesInventory`   | List all rules-engine nodes + invoking code (W15)     |
//!
//! Agent-behavior rules honored:
//! * R1 — never `isError: true`; empty results come back as empty `content` + diagnostic.
//! * R3 — coverage noted in `diagnostics` when a MemStore has no FTS support.
//! * R4 — output capped via `limit` / `max_nodes` / `token_budget`.
//! * R5 — placeholder staleness note emitted (full git-rev check is the MCP layer's job).
//! * R7 — low-confidence edges flagged in diagnostics when present in traversals.

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use wicked_estate_core::{
    Annotation, Direction, EdgeKind, GraphRead, NodeKind, Result, RetrievalResult, RetrievalTool,
    SymbolId, SymbolQuery, TraversalSpec, is_advisory,
};

// W12 — one-shot context bundle tool (seed + ranked neighbours + budgeted stubs). Lives in its
// own module; reuses `render_stub` / `render_context` below for budgeting (no reinvention).
pub mod context_bundle;
pub use context_bundle::ContextBundle;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an optional `u64` field from the request JSON.
fn opt_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key)?.as_u64()
}

/// Emit a staleness note reminding the MCP layer to embed `commits_behind`.
fn staleness_note() -> String {
    "STALENESS: commits_behind not available at this layer — embed git rev-list delta in the MCP response".to_string()
}

// Source inlining is **unbounded by default**: when a caller opts into `include_source`, it gets
// the full source unless it sets a budget. The engine does not decide the payload size — the caller
// owns its context window (agents range from small to 200K–1M tokens), so policy lives with the
// consumer, not here. R4 still governs the *default* path (source is opt-in; without it the payload
// stays tight). Callers that want a tight response set `max_source_chars` (per slice) and/or
// `max_total_source_chars` (across matches); when either bites, truncation is **loud**
// (`source_truncated: true` + `byte_range`) so an agent never silently reasons over a cut body.
const SOURCE_UNBOUNDED: usize = usize::MAX;

/// JSON value for `node.kind`, falling back to `null` on the (impossible) serialize failure.
fn kind_json(kind: &wicked_estate_core::NodeKind) -> Value {
    serde_json::to_value(kind).unwrap_or(Value::Null)
}

/// 1-based start line (the field the CLI prints as `line+1`). The on-disk span is 0-based.
fn line_1based(node: &wicked_estate_core::Node) -> u64 {
    node.location.span.start_line as u64 + 1
}

/// Denormalize a [`SymbolId`] endpoint into `{symbol, name, kind, file, line, line_1based}` so graph-tool
/// callers don't need an N+1 `RetrieveEntity` round-trip per edge endpoint.
///
/// `cache` memoizes node lookups within a single tool invocation: an edge's two endpoints, and the
/// endpoints shared across many edges, resolve with at most one `get_node` per distinct id. A
/// `None` cache entry records a confirmed miss (id not in the graph) so we never re-query it.
///
/// When the id is **not** a node in the graph (e.g. a dangling edge to a symbol from another file),
/// only `{symbol}` is emitted — the caller still gets the id, just without the denormalized detail.
fn endpoint_json(
    store: &dyn GraphRead,
    id: &SymbolId,
    cache: &mut HashMap<SymbolId, Option<wicked_estate_core::Node>>,
) -> Result<Value> {
    if !cache.contains_key(id) {
        let node = store.get_node(id)?;
        cache.insert(id.clone(), node);
    }
    match cache.get(id).and_then(|o| o.as_ref()) {
        Some(node) => Ok(json!({
            "symbol": node.symbol.as_str(),
            "name": node.name,
            "kind": kind_json(&node.kind),
            "file": node.location.file,
            "line": node.location.span.start_line as u64,
            "line_1based": line_1based(node),
        })),
        // Endpoint not in the graph — return the bare id so the edge is still traceable.
        None => Ok(json!({ "symbol": id.as_str() })),
    }
}

/// Provenance rendered as the serde snake_case tag (R7 — provenance visible on every edge).
fn provenance_json(p: &wicked_estate_core::Provenance) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

/// Denormalize one [`wicked_estate_core::Edge`] into a self-contained JSON object: both endpoints
/// expanded (via [`endpoint_json`]) plus the edge's `{confidence, provenance, resolved_by}` inline,
/// so an agent reading a traversal never has to look an endpoint up or guess an edge's trust (R7).
fn edge_json(
    store: &dyn GraphRead,
    edge: &wicked_estate_core::Edge,
    cache: &mut HashMap<SymbolId, Option<wicked_estate_core::Node>>,
) -> Result<Value> {
    Ok(json!({
        "source": endpoint_json(store, &edge.source, cache)?,
        "target": endpoint_json(store, &edge.target, cache)?,
        "kind": kind_json_edge(&edge.kind),
        "confidence": edge.confidence.get(),
        "provenance": provenance_json(&edge.provenance),
        "resolved_by": edge.resolved_by,
    }))
}

/// JSON value for an [`wicked_estate_core::EdgeKind`].
fn kind_json_edge(kind: &EdgeKind) -> Value {
    serde_json::to_value(kind).unwrap_or(Value::Null)
}

/// Parse the source-inlining options from a request:
/// * `include_source` (default `false`),
/// * `max_source_chars` — optional per-slice cap; **unbounded** if omitted,
/// * `max_total_source_chars` — optional across-matches total budget; **unbounded** if omitted.
///
/// Defaults are unbounded ("all") so the caller — which knows its own context window — owns the
/// budget; the engine does not impose one. When a cap is set and bites, truncation is loud
/// (`source_truncated` + `byte_range`). Returns `(include, per_slice_cap, total_budget)`.
fn parse_source_opts(request: &Value) -> (bool, usize, usize) {
    let include = request
        .get("include_source")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_chars = opt_u64(request, "max_source_chars")
        .map(|v| v as usize)
        .unwrap_or(SOURCE_UNBOUNDED);
    let total_budget = opt_u64(request, "max_total_source_chars")
        .map(|v| v as usize)
        .unwrap_or(SOURCE_UNBOUNDED);
    (include, max_chars, total_budget)
}

/// Attach source/provenance fields to a node's JSON payload, in place, honoring the per-node and
/// total-payload char budgets (R4). Mutates `obj`, returns the number of source chars charged
/// against the running `total_budget_remaining`.
///
/// Behavior:
/// * Always adds `blob_sha` when the store has a git blob SHA for the node's file (Deliverable 5;
///   file-granularity — see the report note). Absent otherwise.
/// * When `include_source` and the store can produce a slice (`symbol_source`), adds:
///   - `source` — the byte slice, truncated to `min(max_source_chars, total_budget_remaining)`;
///   - `byte_range` — `[start_byte, end_byte]` from the span (always, when source is attempted);
///   - `source_truncated` — `true` iff the stored slice was longer than what was emitted.
/// * When `include_source` but no slice is available (zero span / content not stored), adds
///   `source: null` + the `byte_range` so the caller can fetch the bytes itself, and pushes a
///   diagnostic.
fn attach_source(
    obj: &mut serde_json::Map<String, Value>,
    store: &dyn GraphRead,
    node: &wicked_estate_core::Node,
    include_source: bool,
    max_source_chars: usize,
    total_budget_remaining: &mut usize,
    diag: &mut Vec<String>,
) -> Result<()> {
    // blob_sha is independent of include_source: it is a cheap, content-addressed file-version id.
    if let Some(sha) = store.file_git_sha(&node.location.file)? {
        obj.insert("blob_sha".to_string(), json!(sha));
    }

    if !include_source {
        return Ok(());
    }

    let span = node.location.span;
    let byte_range = json!([span.start_byte, span.end_byte]);

    match store.symbol_source(node)? {
        Some(src) => {
            let per_node_cap = max_source_chars.min(*total_budget_remaining);
            // Truncate on a char boundary so we never split a multi-byte UTF-8 scalar.
            let (slice, truncated) = if src.chars().count() > per_node_cap {
                let taken: String = src.chars().take(per_node_cap).collect();
                (taken, true)
            } else {
                (src, false)
            };
            *total_budget_remaining = total_budget_remaining.saturating_sub(slice.chars().count());
            obj.insert("source".to_string(), json!(slice));
            obj.insert("byte_range".to_string(), byte_range);
            if truncated {
                obj.insert("source_truncated".to_string(), json!(true));
            }
        }
        None => {
            // Source requested but unavailable — be honest (R3/R5): emit null + the byte range so
            // the agent can still locate the bytes, and say why once.
            obj.insert("source".to_string(), Value::Null);
            obj.insert("byte_range".to_string(), byte_range);
            diag.push(format!(
                "source unavailable for '{}' (file content not stored or zero-span span); \
                 byte_range provided — re-run 'index' to populate content",
                node.symbol.as_str()
            ));
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Annotations in structured payloads  (Chunk 3 — typed-annotation consumer surface)
// ─────────────────────────────────────────────────────────────────────────────

/// R4 payload cap: at most this many annotation **items** are inlined per entity. When the symbol
/// has more, advisory-class (`assumption`/`question`) are kept first, then the rest by `ts`
/// descending; `annotation_summary.count` / `by_type` always reflect the TRUE totals so a consumer
/// sees it was capped (mirrors the source-bundle "summary is always exact" rule). The CLI
/// `annotations` query is **not** capped — only payloads.
const MAX_PAYLOAD_ANNOTATIONS: usize = 20;

/// Render one [`Annotation`] as the payload JSON object the consumer spec fixes:
/// `{ type, key, value, confidence, provenance, author, ts, advisory, source_type,
/// extraction_method, last_verified }`. `advisory` is computed from the type via [`is_advisory`]
/// (gate "is this a fact?" off the computed field, never the type string — a custom advisory-like
/// type can opt in later without consumer changes). The evidence-envelope trio (`source_type` /
/// `extraction_method` / `last_verified`) is surfaced so a consuming agent can answer "what kind of
/// source backed this, by what method, and is it still fresh?" — the audit-traceability the
/// envelope adds. Additive: existing consumers that ignore the new keys are unaffected.
fn annotation_item_json(a: &Annotation) -> Value {
    json!({
        "type": a.r#type,
        "key": a.key,
        "value": a.value,
        "confidence": a.confidence,
        "provenance": a.provenance,
        "author": a.author,
        "ts": a.ts,
        "advisory": is_advisory(&a.r#type),
        "source_type": a.source_type,
        "extraction_method": a.extraction_method,
        "last_verified": a.last_verified,
    })
}

/// Build the `(annotations, annotation_summary)` payload pair for a symbol, or `None` when the
/// symbol has **no** annotations (so callers omit both fields — additive, R4-friendly).
///
/// * `annotations` — the inlined items, **capped** at [`MAX_PAYLOAD_ANNOTATIONS`]. When the symbol
///   has more, advisory-class items come first, then by `ts` descending (newest first); within a
///   group, input order is otherwise preserved (stable sort).
/// * `annotation_summary` — `{ "count": N, "by_type": {…}, "has_advisory": bool }`. `count` and
///   `by_type` reflect the **TRUE** totals over ALL annotations (not the capped slice), so a
///   consumer can tell the inline list was truncated. `has_advisory` is true iff any annotation
///   (capped or not) is advisory.
fn annotation_payload(store: &dyn GraphRead, id: &SymbolId) -> Result<Option<(Value, Value)>> {
    let anns = store.annotations(id)?;
    if anns.is_empty() {
        return Ok(None);
    }

    // ── summary over the TRUE totals (always exact, never the capped slice) ──
    let total = anns.len();
    let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
    let mut has_advisory = false;
    for a in &anns {
        *by_type.entry(a.r#type.clone()).or_insert(0) += 1;
        has_advisory |= is_advisory(&a.r#type);
    }
    let summary = json!({
        "count": total,
        "by_type": by_type,
        "has_advisory": has_advisory,
    });

    // ── capped inline list: advisory-class first, then ts desc (stable) ──
    // Only sort/cap when over the limit; under the cap the list keeps insertion order untouched.
    let items: Vec<Value> = if total > MAX_PAYLOAD_ANNOTATIONS {
        let mut ranked: Vec<&Annotation> = anns.iter().collect();
        // Stable sort: primary = advisory first (false sorts after true), secondary = ts desc.
        ranked.sort_by(|a, b| {
            let adv = is_advisory(&b.r#type).cmp(&is_advisory(&a.r#type)); // true (1) before false (0)
            adv.then_with(|| b.ts.cmp(&a.ts)) // newer ts first
        });
        ranked
            .into_iter()
            .take(MAX_PAYLOAD_ANNOTATIONS)
            .map(annotation_item_json)
            .collect()
    } else {
        anns.iter().map(annotation_item_json).collect()
    };

    Ok(Some((Value::Array(items), summary)))
}

// ─────────────────────────────────────────────────────────────────────────────
// SearchEntity
// ─────────────────────────────────────────────────────────────────────────────

/// Search symbols by exact or substring name.
///
/// **Request shape**
/// ```json
/// { "name": "foo", "limit": 20 }
/// ```
/// * `name` (required) — searched as both an exact match and a substring match against `name +
///   signature`.  If the store has native FTS the text field drives it; otherwise the in-process
///   filter is used.
/// * `limit` (optional, default 20, max 100) — max results.
/// * `include_source` (optional, default `false`) — attach each match's exact byte slice, bounded
///   by `max_source_chars` per match plus a total-payload budget across matches (R4).
/// * `max_source_chars` (optional) — per-match char cap; **unbounded** if omitted.
/// * `max_total_source_chars` (optional) — total source budget across all matches; **unbounded** if
///   omitted. Set either to constrain output for a small-context caller; when a cap bites, source is
///   truncated first and marked loudly (`source_truncated` + `byte_range`). `limit` bounds match count.
///
/// **Response `content` shape**
/// ```json
/// { "matches": [ { "symbol": "…", "name": "foo", "kind": "function", "file": "src/lib.rs",
///                  "line": 11, "line_1based": 12, "end_line": 14, "end_line_1based": 15,
///                  "signature": "fn foo() -> u32",
///                  "blob_sha": "…",                       // present iff file has a git blob SHA
///                  "source": "fn foo() …",                // present iff include_source
///                  "byte_range": [120, 168], "source_truncated": false }, … ], "total": 3 }
/// ```
/// `line` stays 0-based for compat; `line_1based` matches the CLI's printed line.
#[derive(Debug, Default)]
pub struct SearchEntity;

impl RetrievalTool for SearchEntity {
    fn name(&self) -> &str {
        "SearchEntity"
    }

    fn description(&self) -> &str {
        "Search the code graph for symbols matching a name (exact or substring). \
         Returns ranked matches with name, kind, file:line, and signature."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let name_val = match request.get("name").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({ "matches": [], "total": 0 }),
                    diagnostics: vec![
                        "SearchEntity: 'name' field is required and must be a non-empty string"
                            .to_string(),
                    ],
                });
            }
        };

        let raw_limit = opt_u64(request, "limit").unwrap_or(20).min(100) as usize;

        // Two-pass: exact first, then substring (deduplicated).
        let exact_query = SymbolQuery {
            exact_name: Some(name_val.clone()),
            limit: Some(raw_limit),
            ..Default::default()
        };
        let mut exact_hits = store.find_symbols(&exact_query)?;

        let text_query = SymbolQuery {
            text: Some(name_val.clone()),
            limit: Some(raw_limit),
            ..Default::default()
        };
        let text_hits = store.find_symbols(&text_query)?;

        // Merge: exact first, then text hits not already present.
        let exact_ids: std::collections::HashSet<_> =
            exact_hits.iter().map(|n| n.symbol.clone()).collect();
        for n in text_hits {
            if !exact_ids.contains(&n.symbol) {
                exact_hits.push(n);
            }
        }
        exact_hits.truncate(raw_limit);

        let mut diag = Vec::new();

        if exact_hits.is_empty() {
            diag.push(format!(
                "SearchEntity: no symbols found matching '{name_val}'"
            ));
        }
        if !store.capabilities().full_text_search {
            diag.push(
                "COVERAGE: store does not support native FTS; using in-process substring filter"
                    .to_string(),
            );
        }
        diag.push(staleness_note());

        let (include_source, max_source_chars, total_source_budget) = parse_source_opts(request);
        // Unbounded by default (the caller owns its context budget); `max_total_source_chars`
        // constrains total source across matches, `max_source_chars` per slice. `limit` already
        // bounds the match count. Only consumed when include_source is set; truncation is loud.
        let mut budget_remaining: usize = total_source_budget;

        let mut matches: Vec<Value> = Vec::with_capacity(exact_hits.len());
        for n in &exact_hits {
            let mut obj = serde_json::Map::new();
            obj.insert("symbol".to_string(), json!(n.symbol.as_str()));
            obj.insert("name".to_string(), json!(n.name));
            obj.insert("kind".to_string(), kind_json(&n.kind));
            obj.insert("file".to_string(), json!(n.location.file));
            // Existing 0-based `line` kept for compat; 1-based + end lines added additively.
            obj.insert("line".to_string(), json!(n.location.span.start_line));
            obj.insert("line_1based".to_string(), json!(line_1based(n)));
            obj.insert("end_line".to_string(), json!(n.location.span.end_line));
            obj.insert(
                "end_line_1based".to_string(),
                json!(n.location.span.end_line as u64 + 1),
            );
            obj.insert(
                "signature".to_string(),
                json!(n.signature.as_deref().unwrap_or("")),
            );
            attach_source(
                &mut obj,
                store,
                n,
                include_source,
                max_source_chars,
                &mut budget_remaining,
                &mut diag,
            )?;
            matches.push(Value::Object(obj));
        }

        let total = matches.len();
        Ok(RetrievalResult {
            content: json!({ "matches": matches, "total": total }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RetrieveEntity
// ─────────────────────────────────────────────────────────────────────────────

/// Full detail for a single symbol id.
///
/// **Request shape**
/// ```json
/// { "symbol": "<id>", "include_source": false, "max_source_chars": 2000 }
/// ```
/// * `symbol` (required) — stable [`SymbolId`].
/// * `include_source` (optional, default `false`) — when `true`, attach the symbol's exact byte
///   slice (full body by default). Default-off keeps the payload small (R4).
/// * `max_source_chars` (optional) — per-slice char cap; **unbounded** if omitted. When set and it
///   bites, the slice is truncated and marked loudly (`source_truncated` + `byte_range`).
///
/// **Response `content` shape**
/// ```json
/// { "found": true, "symbol": "…", "name": "foo", "kind": "function",
///   "language": "rust", "file": "src/lib.rs",
///   "line": 11, "line_1based": 12, "end_line": 14, "end_line_1based": 15,
///   "signature": "fn foo() -> u32", "doc": "…",
///   "blob_sha": "…",                                  // present iff the file has a git blob SHA
///   "source": "fn foo() -> u32 { … }",                // present iff include_source
///   "byte_range": [120, 168], "source_truncated": false,
///   "annotations": [ { "type":"assumption", "key":"…", "value":"…", "confidence":0.7,
///                      "provenance":"…", "author":"…", "ts":1718500000,
///                      "advisory":true } ],            // present iff the symbol has annotations
///   "annotation_summary": { "count": 3, "by_type": {"note":2,"assumption":1},
///                           "has_advisory": true } }   // present iff the symbol has annotations
/// ```
/// `line` stays 0-based for backward compatibility; `line_1based` matches what the CLI prints.
/// `annotations` / `annotation_summary` are additive and emitted **only when the symbol has
/// annotations**. `annotations` is R4-capped at 20 items (advisory-class first, then `ts` desc);
/// `annotation_summary.count` / `by_type` always reflect the TRUE totals, so a consumer can tell
/// the inline list was capped. Per-item `advisory` is computed from the type (assumption/question).
/// When the symbol is not found `found` is `false` and the other fields are absent; a diagnostic
/// is emitted instead of an error (R1).
#[derive(Debug, Default)]
pub struct RetrieveEntity;

impl RetrievalTool for RetrieveEntity {
    fn name(&self) -> &str {
        "RetrieveEntity"
    }

    fn description(&self) -> &str {
        "Retrieve full detail for a single symbol by its stable id. \
         Returns name, kind, language, file:line, signature, and doc comment."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let id_str = match request.get("symbol").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({ "found": false }),
                    diagnostics: vec!["RetrieveEntity: 'symbol' field is required".to_string()],
                });
            }
        };

        let id = SymbolId(id_str.clone());
        let mut diag = vec![staleness_note()];

        match store.get_node(&id)? {
            None => {
                diag.push(format!(
                    "RetrieveEntity: symbol '{id_str}' not found in graph"
                ));
                Ok(RetrievalResult {
                    content: json!({ "found": false, "symbol": id_str }),
                    diagnostics: diag,
                })
            }
            Some(node) => {
                // Single node: the per-slice cap is the only bound (no across-matches total).
                // Unbounded by default; `max_source_chars` constrains if the caller sets it.
                let (include_source, max_source_chars, _total) = parse_source_opts(request);
                let mut budget_remaining: usize = SOURCE_UNBOUNDED;

                let mut obj = serde_json::Map::new();
                obj.insert("found".to_string(), json!(true));
                obj.insert("symbol".to_string(), json!(node.symbol.as_str()));
                obj.insert("name".to_string(), json!(node.name));
                obj.insert("kind".to_string(), kind_json(&node.kind));
                obj.insert("language".to_string(), json!(node.language.as_str()));
                obj.insert("file".to_string(), json!(node.location.file));
                // Existing 0-based `line` preserved for compat; richer line fields added additively.
                obj.insert("line".to_string(), json!(node.location.span.start_line));
                obj.insert("line_1based".to_string(), json!(line_1based(&node)));
                obj.insert("end_line".to_string(), json!(node.location.span.end_line));
                obj.insert(
                    "end_line_1based".to_string(),
                    json!(node.location.span.end_line as u64 + 1),
                );
                obj.insert(
                    "signature".to_string(),
                    json!(node.signature.as_deref().unwrap_or("")),
                );
                obj.insert("doc".to_string(), json!(node.doc.as_deref().unwrap_or("")));
                attach_source(
                    &mut obj,
                    store,
                    &node,
                    include_source,
                    max_source_chars,
                    &mut budget_remaining,
                    &mut diag,
                )?;

                // Typed annotations (Chunk 3): inline `annotations` + `annotation_summary`, but
                // only when the symbol actually has annotations (additive — absent otherwise). The
                // inline list is R4-capped; the summary always carries the true totals.
                if let Some((annotations, summary)) = annotation_payload(store, &node.symbol)? {
                    obj.insert("annotations".to_string(), annotations);
                    obj.insert("annotation_summary".to_string(), summary);
                }

                Ok(RetrievalResult {
                    content: Value::Object(obj),
                    diagnostics: diag,
                })
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TraverseGraph
// ─────────────────────────────────────────────────────────────────────────────

/// Bounded multi-hop traversal from a start symbol.
///
/// **Request shape**
/// ```json
/// { "symbol": "<id>", "depth": 4, "direction": "dependencies",
///   "edge_kinds": ["calls", "imports"], "max_nodes": 200 }
/// ```
/// * `depth`      — optional, default 4, max 16.
/// * `direction`  — `"dependencies"` (default) | `"dependents"` | `"both"`.
/// * `edge_kinds` — optional array of edge-kind strings; empty = all kinds.
/// * `max_nodes`  — optional, default 200, max 1 000.
///
/// **Response `content` shape**
/// ```json
/// { "nodes": [ { "symbol": "…", "name": "…", "kind": "…",
///                "file": "…", "line": 11, "line_1based": 12 }, … ],
///   "edges": [ { "source": { "symbol": "…", "name": "…", "kind": "…",
///                            "file": "…", "line": 11, "line_1based": 12 },
///                "target": { … }, "kind": "calls",
///                "confidence": 1.0, "provenance": "parsed",
///                "resolved_by": "scip-rust" }, … ],
///   "depths": { "<id>": 1, … }, "truncated": false }
/// ```
/// Edge endpoints are **denormalized** (name/kind/file/line inline) so an agent never needs an
/// N+1 `RetrieveEntity` per endpoint; each edge also carries `{confidence, provenance, resolved_by}`
/// so heuristic edges are never mistaken for facts (R7).
#[derive(Debug, Default)]
pub struct TraverseGraph;

fn parse_direction(v: &Value) -> Direction {
    match v.get("direction").and_then(|d| d.as_str()) {
        Some("dependents") => Direction::Dependents,
        Some("both") => Direction::Both,
        _ => Direction::Dependencies,
    }
}

fn parse_edge_kinds(v: &Value) -> Vec<EdgeKind> {
    let Some(arr) = v.get("edge_kinds").and_then(|a| a.as_array()) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|item| {
            let s = item.as_str()?;
            // Deserialize from a quoted JSON string so serde_json handles the snake_case mapping.
            serde_json::from_str::<EdgeKind>(&format!("\"{s}\"")).ok()
        })
        .collect()
}

impl RetrievalTool for TraverseGraph {
    fn name(&self) -> &str {
        "TraverseGraph"
    }

    fn description(&self) -> &str {
        "Bounded multi-hop walk from a start symbol. Returns the subgraph (nodes, edges, depths) \
         reachable within the given depth and node caps. Supports forward (dependencies), \
         reverse (dependents), and bidirectional traversal. \
         For rules engine connections: use edge_kinds=[\"invoked_by\"] to trace code→rules, \
         [\"governs\"] for ruleset→rule structure, [\"evaluates\"] for rule→condition."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let id_str = match request.get("symbol").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({ "nodes": [], "edges": [], "depths": {}, "truncated": false }),
                    diagnostics: vec!["TraverseGraph: 'symbol' field is required".to_string()],
                });
            }
        };

        let max_depth = opt_u64(request, "depth").unwrap_or(4).min(16) as u32;
        let max_nodes = opt_u64(request, "max_nodes").unwrap_or(200).min(1_000) as usize;
        let direction = parse_direction(request);
        let edge_kinds = parse_edge_kinds(request);

        let spec = TraversalSpec {
            direction,
            edge_kinds,
            max_depth,
            max_nodes,
            min_confidence: 0.0,
        };

        let start = SymbolId(id_str.clone());
        let mut diag = vec![staleness_note()];

        let subgraph = store.traverse(&start, &spec)?;

        if subgraph.nodes.is_empty() {
            diag.push(format!(
                "TraverseGraph: no nodes reachable from '{id_str}' under given spec"
            ));
        }
        if subgraph.truncated {
            diag.push(format!(
                "TraverseGraph: result truncated (max_depth={max_depth}, max_nodes={max_nodes})"
            ));
        }

        // R7 — flag any low-confidence edges.
        let low_conf: usize = subgraph
            .edges
            .iter()
            .filter(|e| e.confidence.get() < 0.5)
            .count();
        if low_conf > 0 {
            diag.push(format!(
                "R7-CONFIDENCE: {low_conf} edge(s) below 0.5 confidence in result set"
            ));
        }

        let nodes_json: Vec<Value> = subgraph
            .nodes
            .iter()
            .map(|n| {
                json!({
                    "symbol": n.symbol.as_str(),
                    "name": n.name,
                    "kind": kind_json(&n.kind),
                    "file": n.location.file,
                    "line": n.location.span.start_line,
                    "line_1based": line_1based(n),
                })
            })
            .collect();

        // Denormalize edge endpoints (Deliverable 1): each endpoint expands to
        // {symbol,name,kind,file,line_1based} and the edge carries {confidence,provenance,
        // resolved_by} inline (R7) — no N+1 RetrieveEntity follow-ups. Endpoint lookups are
        // memoized for the whole result set.
        let mut node_cache: HashMap<SymbolId, Option<wicked_estate_core::Node>> = HashMap::new();
        // Seed the cache with nodes already in hand (avoids re-querying for endpoints).
        for n in &subgraph.nodes {
            node_cache.insert(n.symbol.clone(), Some(n.clone()));
        }
        let mut edges_json: Vec<Value> = Vec::with_capacity(subgraph.edges.len());
        for e in &subgraph.edges {
            edges_json.push(edge_json(store, e, &mut node_cache)?);
        }

        Ok(RetrievalResult {
            content: json!({
                "nodes": nodes_json,
                "edges": edges_json,
                "depths": subgraph.depths,
                "truncated": subgraph.truncated,
            }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BlastRadius
// ─────────────────────────────────────────────────────────────────────────────

/// Transitive dependents (reverse-reachability on `Calls` edges).
///
/// **Request shape**
/// ```json
/// { "symbol": "<id>", "depth": 8 }
/// ```
/// * `depth` — optional, default 8, max 24.
///
/// **Response `content` shape**
/// ```json
/// { "dependents": [ { "symbol": "…", "name": "…", "kind": "…", "file": "…",
///                     "line": 11, "line_1based": 12, "depth": 1 }, … ],
///   "total": 5, "truncated": false,
///   "unresolved_callers": 2,
///   "confidence": { "min": 0.6, "avg": 0.9, "edge_count": 3 },
///   "summary": { "total": 5,
///                "by_kind": { "function": 4, "method": 1 },
///                "top_files": [ { "file": "src/a.rs", "count": 3 }, … ],
///                "top_by_pagerank": [ { "symbol": "…", "name": "…", "score": 0.12 }, … ] } }
/// ```
/// The start symbol itself is excluded from `dependents`.
/// `unresolved_callers` counts call-site references to the symbol's name that the resolver
/// could not bind — potential dependents that may be missing from `dependents`.  Always
/// present, even when zero.
/// `summary` gives an at-a-glance picture (counts by kind, hottest files, highest-PageRank
/// dependents) so an agent can triage a large blast without reading every entry (R4-friendly).
#[derive(Debug, Default)]
pub struct BlastRadius;

impl RetrievalTool for BlastRadius {
    fn name(&self) -> &str {
        "BlastRadius"
    }

    fn description(&self) -> &str {
        "Transitive dependents of a symbol (reverse-reachability on Calls edges). \
         Answers 'what breaks if I change this symbol?' \
         Reports coverage: resolved dependents plus the count of unresolved call-site \
         references that could not be bound (potential missing dependents)."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let id_str = match request.get("symbol").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({
                        "dependents": [],
                        "total": 0,
                        "truncated": false,
                        "unresolved_callers": 0,
                        "confidence": { "min": null, "avg": null, "edge_count": 0 },
                    }),
                    diagnostics: vec!["BlastRadius: 'symbol' field is required".to_string()],
                });
            }
        };

        let max_depth = opt_u64(request, "depth").unwrap_or(8).min(24) as u32;

        let spec = TraversalSpec::blast_radius(max_depth);
        let start = SymbolId(id_str.clone());
        let mut diag = vec![staleness_note()];

        let subgraph = store.traverse(&start, &spec)?;

        // Resolve the symbol's simple name (for unresolved-ref lookup).  If the symbol is in
        // the graph, use its recorded name; otherwise fall back to the last segment of the id.
        let symbol_name: String = store
            .get_node(&start)?
            .map(|n| n.name.clone())
            .unwrap_or_else(|| {
                // Best-effort: strip package prefix and use the last "."-delimited segment.
                id_str
                    .rsplit('.')
                    .next()
                    .unwrap_or(id_str.as_str())
                    .to_string()
            });

        // Unresolved-ref coverage: calls/imports to `symbol_name` the resolver could not bind.
        let unresolved_callers = store.unresolved_refs_for_name(&symbol_name)?.len();

        // Dependent nodes (start excluded), kept as a Vec<&Node> so we can both render them and
        // compute the summary without a second pass over the subgraph.
        let dependent_nodes: Vec<&wicked_estate_core::Node> = subgraph
            .nodes
            .iter()
            .filter(|n| n.symbol.as_str() != id_str)
            .collect();

        let dependents: Vec<Value> = dependent_nodes
            .iter()
            .map(|n| {
                let depth = subgraph.depths.get(n.symbol.as_str()).copied().unwrap_or(0);
                json!({
                    "symbol": n.symbol.as_str(),
                    "name": n.name,
                    "kind": kind_json(&n.kind),
                    "file": n.location.file,
                    "line": n.location.span.start_line,
                    "line_1based": line_1based(n),
                    "depth": depth,
                })
            })
            .collect();

        // Compact summary (Deliverable 4): counts by kind, top files by member count, and
        // (best-effort) top members by personalized PageRank seeded on the start symbol.
        let summary = blast_summary(store, &start, &dependent_nodes)?;

        // Confidence stats over the traversal's edges (edges entering the blast-radius set).
        let (conf_min, conf_avg, edge_count) = {
            let confs: Vec<f32> = subgraph.edges.iter().map(|e| e.confidence.get()).collect();
            if confs.is_empty() {
                (None, None, 0usize)
            } else {
                let min = confs.iter().cloned().fold(f32::INFINITY, f32::min);
                let avg = confs.iter().sum::<f32>() / confs.len() as f32;
                (Some(min), Some(avg), confs.len())
            }
        };

        let conf_json = json!({
            "min": conf_min,
            "avg": conf_avg,
            "edge_count": edge_count,
        });

        let total = dependents.len();

        if dependents.is_empty() {
            diag.push(format!(
                "BlastRadius: no dependents found for '{id_str}' (it may be a leaf or not yet indexed)"
            ));
        }
        if subgraph.truncated {
            diag.push(format!(
                "BlastRadius: result truncated at depth={max_depth} / max_nodes=5000"
            ));
        }

        // Coverage note — always emitted (even when unresolved_callers == 0) so callers always
        // know the completeness posture of the result.
        diag.push(format!(
            "coverage: {total} dependent(s) via resolved calls; \
             {unresolved_callers} unresolved call(s) reference '{symbol_name}' — \
             blast-radius is best-effort static resolution and MAY be incomplete \
             (precise tier pending)."
        ));

        Ok(RetrievalResult {
            content: json!({
                "dependents": dependents,
                "total": total,
                "truncated": subgraph.truncated,
                "unresolved_callers": unresolved_callers,
                "confidence": conf_json,
                "summary": summary,
            }),
            diagnostics: diag,
        })
    }
}

/// Top-N cap for the per-file and per-PageRank lists in the blast-radius summary (R4 — keep it
/// compact regardless of blast size).
const BLAST_SUMMARY_TOP_N: usize = 10;

/// Build the compact `summary` object for a blast radius:
/// ```json
/// { "total": 12,
///   "by_kind": { "function": 9, "method": 3 },
///   "top_files": [ { "file": "src/a.rs", "count": 4 }, … ],     // ≤ BLAST_SUMMARY_TOP_N
///   "top_by_pagerank": [ { "symbol": "…", "name": "…", "score": 0.12 }, … ] }  // ≤ N, optional
/// ```
/// `top_by_pagerank` is best-effort: personalized PageRank is seeded on the changed symbol, then
/// filtered to the dependent set. It is omitted (absent) only if ranking errors; an empty graph
/// yields an empty array. Counts are exact over the resolved dependents.
fn blast_summary(
    store: &dyn GraphRead,
    start: &SymbolId,
    dependents: &[&wicked_estate_core::Node],
) -> Result<Value> {
    use std::collections::BTreeMap;

    // by_kind — BTreeMap for deterministic key order in the JSON.
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    // file -> member count.
    let mut file_counts: HashMap<String, u64> = HashMap::new();
    for n in dependents {
        let kind_key = match kind_json(&n.kind) {
            Value::String(s) => s,
            other => other.to_string(),
        };
        *by_kind.entry(kind_key).or_insert(0) += 1;
        *file_counts.entry(n.location.file.clone()).or_insert(0) += 1;
    }

    // top_files — by count desc, then path asc for determinism.
    let mut file_vec: Vec<(String, u64)> = file_counts.into_iter().collect();
    file_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top_files: Vec<Value> = file_vec
        .iter()
        .take(BLAST_SUMMARY_TOP_N)
        .map(|(file, count)| json!({ "file": file, "count": count }))
        .collect();

    // top_by_pagerank — personalized PR seeded on the changed symbol, filtered to dependents.
    let dependent_ids: HashSet<&str> = dependents.iter().map(|n| n.symbol.as_str()).collect();
    let name_by_id: HashMap<&str, &str> = dependents
        .iter()
        .map(|n| (n.symbol.as_str(), n.name.as_str()))
        .collect();
    let top_by_pagerank: Value =
        match wicked_estate_rank::ranked_symbols(store, std::slice::from_ref(start), usize::MAX) {
            Ok(ranked) => {
                let list: Vec<Value> = ranked
                    .into_iter()
                    .filter(|(id, _)| dependent_ids.contains(id.as_str()))
                    .take(BLAST_SUMMARY_TOP_N)
                    .map(|(id, score)| {
                        json!({
                            "symbol": id.as_str(),
                            "name": name_by_id.get(id.as_str()).copied().unwrap_or(""),
                            "score": score,
                        })
                    })
                    .collect();
                Value::Array(list)
            }
            // Ranking is an enrichment, not load-bearing: degrade to absent rather than failing the
            // whole tool (R1) if PageRank errors.
            Err(_) => Value::Null,
        };

    let mut summary = serde_json::Map::new();
    summary.insert("total".to_string(), json!(dependents.len()));
    summary.insert(
        "by_kind".to_string(),
        serde_json::to_value(&by_kind).unwrap_or(Value::Null),
    );
    summary.insert("top_files".to_string(), Value::Array(top_files));
    if !top_by_pagerank.is_null() {
        summary.insert("top_by_pagerank".to_string(), top_by_pagerank);
    }
    Ok(Value::Object(summary))
}

// ─────────────────────────────────────────────────────────────────────────────
// Lineage  (W12 — transitive dependencies; complement of BlastRadius)
// ─────────────────────────────────────────────────────────────────────────────

/// Transitive **dependencies** of a symbol (what it depends on — forward-reachability on
/// `Calls` + `Imports` edges).
///
/// This is the directional complement of [`BlastRadius`]: where `BlastRadius` walks
/// *Dependents* (who calls *me*?), `Lineage` walks *Dependencies* (what do *I* call?).
/// The full transitive closure lets agents understand the dependency chain before a refactor.
///
/// **Request shape**
/// ```json
/// { "symbol": "<id>", "depth": <n> }
/// ```
/// * `symbol` (required) — stable [`SymbolId`] of the start symbol.
/// * `depth`  (optional, default 8, max 24) — maximum traversal hops.
///
/// **Response `content` shape**
/// ```json
/// { "dependencies": [ { "symbol": "…", "name": "…", "kind": "…",
///                        "file": "…", "line": 0, "depth": 1 }, … ],
///   "total": 5, "truncated": false,
///   "confidence": { "min": 0.6, "avg": 0.9, "edge_count": 3 } }
/// ```
/// The start symbol itself is **excluded** from `dependencies`.
/// Agent-behavior rules honored: R1 (no error on empty), R5 (staleness note), R7 (low-confidence
/// flag), R4 (depth + node caps).
#[derive(Debug, Default)]
pub struct Lineage;

impl RetrievalTool for Lineage {
    fn name(&self) -> &str {
        "Lineage"
    }

    fn description(&self) -> &str {
        "Transitive dependencies of a symbol (forward-reachability on Calls+Imports edges). \
         Answers 'what does this symbol depend on?' — the complement of BlastRadius. \
         Use to understand the full dependency chain before a refactor or to build a \
         change-impact picture from the dependency side."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let id_str = match request.get("symbol").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({
                        "dependencies": [],
                        "total": 0,
                        "truncated": false,
                        "confidence": { "min": null, "avg": null, "edge_count": 0 },
                    }),
                    diagnostics: vec!["Lineage: 'symbol' field is required".to_string()],
                });
            }
        };

        let max_depth = opt_u64(request, "depth").unwrap_or(8).min(24) as u32;

        // Walk forward (Dependencies) along Calls + Imports edges — bounded.
        let spec = TraversalSpec {
            direction: Direction::Dependencies,
            edge_kinds: vec![EdgeKind::Calls, EdgeKind::Imports],
            max_depth,
            max_nodes: 5_000,
            min_confidence: 0.0,
        };

        let start = SymbolId(id_str.clone());
        let mut diag = vec![staleness_note()];

        let subgraph = store.traverse(&start, &spec)?;

        // Exclude the start node itself.
        let dependencies: Vec<Value> = subgraph
            .nodes
            .iter()
            .filter(|n| n.symbol.as_str() != id_str)
            .map(|n| {
                let depth = subgraph.depths.get(n.symbol.as_str()).copied().unwrap_or(0);
                json!({
                    "symbol": n.symbol.as_str(),
                    "name": n.name,
                    "kind": serde_json::to_value(&n.kind).unwrap_or(Value::Null),
                    "file": n.location.file,
                    "line": n.location.span.start_line,
                    "depth": depth,
                })
            })
            .collect();

        // Confidence stats over the traversal's edges.
        let (conf_min, conf_avg, edge_count) = {
            let confs: Vec<f32> = subgraph.edges.iter().map(|e| e.confidence.get()).collect();
            if confs.is_empty() {
                (None, None, 0usize)
            } else {
                let min = confs.iter().cloned().fold(f32::INFINITY, f32::min);
                let avg = confs.iter().sum::<f32>() / confs.len() as f32;
                (Some(min), Some(avg), confs.len())
            }
        };

        let conf_json = json!({
            "min": conf_min,
            "avg": conf_avg,
            "edge_count": edge_count,
        });

        if dependencies.is_empty() {
            diag.push(format!(
                "Lineage: no dependencies found for '{id_str}' \
                 (it may be a leaf or not yet indexed)"
            ));
        }
        let node_truncated = subgraph.truncated;
        if node_truncated {
            diag.push(format!(
                "Lineage: result truncated at depth={max_depth} / max_nodes=5000"
            ));
        }

        // R7 — flag any low-confidence edges.
        let low_conf: usize = subgraph
            .edges
            .iter()
            .filter(|e| e.confidence.get() < 0.5)
            .count();
        if low_conf > 0 {
            diag.push(format!(
                "R7-CONFIDENCE: {low_conf} edge(s) below 0.5 confidence in lineage"
            ));
        }

        // R4 (DoD-A8) — `max_nodes=5000` bounds the traversal, but 5000 wide rows can still exceed
        // the 25K-char budget. Cap the serialized `dependencies` array under the budget and emit a
        // loud truncation diagnostic when it bites, on top of the depth/node-cap note above.
        let envelope_overhead = 160; // dependencies + total + truncated + confidence scaffolding.
        let (dependencies, dropped) = cap_rows_to_budget(dependencies, envelope_overhead);
        if dropped > 0 {
            diag.push(format!(
                "Lineage: result truncated to {} dependency/dependencies to stay under the \
                 {}-char R4 budget ({} dropped); lower `depth`",
                dependencies.len(),
                R4_CHAR_BUDGET,
                dropped
            ));
        }
        let truncated = node_truncated || dropped > 0;
        let total = dependencies.len();

        Ok(RetrievalResult {
            content: json!({
                "dependencies": dependencies,
                "total": total,
                "truncated": truncated,
                "confidence": conf_json,
            }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// R4 char-budget cap (shared by the graph-summary tools)
// ─────────────────────────────────────────────────────────────────────────────

/// Hard ceiling on a tool's serialized `content` (R4, CLAUDE.md runtime contract: output < 25K
/// chars). The graph-summary tools (`RankHotspots`, `Communities`) can return one row per node on a
/// wide graph, so they cap the row count to keep the payload under this budget and emit a loud
/// truncation diagnostic when the cap bites — an agent must never silently reason over a cut body.
const R4_CHAR_BUDGET: usize = 25_000;

/// Trim `items` (already in priority order: best first) until the serialized JSON array fits under
/// [`R4_CHAR_BUDGET`], leaving headroom for the surrounding envelope keys. Returns the kept rows and
/// the number dropped. Binary-search on length keeps this O(log n · serialize), not O(n) re-serialize
/// per drop, on a wide graph.
fn cap_rows_to_budget(items: Vec<Value>, envelope_overhead: usize) -> (Vec<Value>, usize) {
    let budget = R4_CHAR_BUDGET.saturating_sub(envelope_overhead);
    let fits =
        |rows: &[Value]| -> bool { serde_json::to_string(rows).is_ok_and(|s| s.len() <= budget) };
    if fits(&items) {
        return (items, 0);
    }
    // Largest prefix length `keep` (0..=items.len()) whose serialization fits.
    let (mut lo, mut hi) = (0usize, items.len());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(&items[..mid]) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let keep = lo;
    let dropped = items.len() - keep;
    let mut kept = items;
    kept.truncate(keep);
    (kept, dropped)
}

// ─────────────────────────────────────────────────────────────────────────────
// RankHotspots — global PageRank importance ranking (W4.1 promotion)
// ─────────────────────────────────────────────────────────────────────────────

/// **`RankHotspots`** — the most central symbols in the graph by (personalized) PageRank over
/// Calls + Imports edges.
///
/// Answers *"what are the load-bearing symbols here?"* — the functions/types the most code flows
/// through. Use to orient in an unfamiliar repo, to pick refactor targets, or to seed a
/// change-impact review. Wraps [`wicked_estate_rank::ranked_symbols`] (the `Ranker`/PageRank
/// backing) and denormalizes each ranked id into `{symbol,name,kind,file,line,score}`.
///
/// **Request shape**
/// ```json
/// { "limit": <n>, "seeds": ["<id>", …] }
/// ```
/// * `limit` (optional, default 20, max 200) — how many top symbols to return.
/// * `seeds` (optional) — symbol ids to bias the ranking toward (personalized PageRank, Aider
///   repo-map pattern); omit for standard global PageRank.
///
/// **Response `content` shape**
/// ```json
/// { "hotspots": [ { "symbol": "…", "name": "…", "kind": "…",
///                    "file": "…", "line": 0, "score": 0.0123 }, … ],
///   "total": 20, "truncated": false }
/// ```
/// Agent-behavior rules honored: R1 (empty graph → empty list, no error), R4 (`limit` + the
/// [`R4_CHAR_BUDGET`] char cap), R5 (staleness note).
#[derive(Debug, Default)]
pub struct RankHotspots;

impl RetrievalTool for RankHotspots {
    fn name(&self) -> &str {
        "RankHotspots"
    }

    fn description(&self) -> &str {
        "Most central symbols by PageRank over Calls+Imports edges. \
         Answers 'what are the load-bearing symbols here?' — the code the most paths flow through. \
         Use to orient in an unfamiliar repo, pick refactor targets, or seed a change-impact \
         review. Optionally bias toward `seeds` for a personalized (subsystem-local) ranking."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let limit = opt_u64(request, "limit").unwrap_or(20).clamp(1, 200) as usize;

        // Optional personalization seeds.
        let seeds: Vec<SymbolId> = request
            .get("seeds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| SymbolId(s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let mut diag = vec![staleness_note()];

        // Global / personalized PageRank, top-`limit` symbols (already sorted high-score first).
        let ranked = wicked_estate_rank::ranked_symbols(store, &seeds, limit)?;

        // Denormalize each id into a row; a single get_node cache avoids N+1 churn on shared ids.
        let mut cache: HashMap<String, Option<wicked_estate_core::Node>> = HashMap::new();
        let mut hotspots: Vec<Value> = Vec::with_capacity(ranked.len());
        for (id, score) in &ranked {
            let node = match cache.get(id.as_str()) {
                Some(n) => n.clone(),
                None => {
                    let n = store.get_node(id)?;
                    cache.insert(id.0.clone(), n.clone());
                    n
                }
            };
            let (name, kind, file, line, line1) = match &node {
                Some(n) => (
                    Value::String(n.name.clone()),
                    kind_json(&n.kind),
                    Value::String(n.location.file.clone()),
                    json!(n.location.span.start_line),
                    json!(line_1based(n)),
                ),
                None => (
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                ),
            };
            hotspots.push(json!({
                "symbol": id.as_str(),
                "name": name,
                "kind": kind,
                "file": file,
                "line": line,
                "line_1based": line1,
                "score": score,
            }));
        }

        if hotspots.is_empty() {
            diag.push(
                "RankHotspots: graph is empty or has no Calls/Imports edges (nothing to rank)"
                    .to_string(),
            );
        }

        // R4 — cap the payload under 25K chars. `ranked_symbols` is bounded by `limit` (≤200), but
        // wide rows (long ids/paths) can still push the array over budget; trim loudly if so.
        let envelope_overhead = 96; // {"hotspots":[…],"total":N,"truncated":true} scaffolding.
        let (hotspots, dropped) = cap_rows_to_budget(hotspots, envelope_overhead);
        let truncated = dropped > 0;
        if truncated {
            diag.push(format!(
                "RankHotspots: result truncated to {} row(s) to stay under the {}-char R4 budget \
                 ({} dropped); raise specificity or lower `limit`",
                hotspots.len(),
                R4_CHAR_BUDGET,
                dropped
            ));
        }

        let total = hotspots.len();
        Ok(RetrievalResult {
            content: json!({
                "hotspots": hotspots,
                "total": total,
                "truncated": truncated,
            }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Communities — graph-structural community detection (Louvain, W4.1)
// ─────────────────────────────────────────────────────────────────────────────

/// **`Communities`** — modularity communities over the Calls + Imports graph, each summarized with
/// its top-PageRank members and dominant files.
///
/// Answers *"what are the natural subsystems here?"* — clusters of symbols more tightly connected
/// to each other than to the rest of the graph. Use to map an unfamiliar codebase into modules or
/// to scope a refactor to a cohesive unit.
///
/// Wraps [`wicked_estate_rank::detect_communities`] (multi-level **Louvain** over the graph) +
/// [`wicked_estate_rank::summarize_communities`]. This is the **graph-structural** (unconditional,
/// `GraphRead`-only) community tool; embedding-proximity clustering (`semantic_clusters`) needs the
/// semantic sidecar and is **not** part of the read-only MCP surface (see the design's §2.3).
///
/// **Request shape**
/// ```json
/// { "limit": <n>, "min_size": <m>, "resolution": <γ> }
/// ```
/// * `limit` (optional, default 20, max 200) — how many communities to return (largest first).
/// * `min_size` (optional, default 2) — drop communities smaller than this.
/// * `resolution` (optional, default 1.0) — Louvain γ; `> 1.0` yields smaller, tighter communities.
///
/// **Response `content` shape**
/// ```json
/// { "communities": [ { "size": 12, "top_symbols": ["…"], "dominant_files": ["…"],
///                       "modularity_contribution": 0.07 }, … ],
///   "total": 8, "truncated": false }
/// ```
/// Agent-behavior rules honored: R1 (empty graph → empty list), R4 (`limit` + [`R4_CHAR_BUDGET`]),
/// R5 (staleness note).
#[derive(Debug, Default)]
pub struct Communities;

impl RetrievalTool for Communities {
    fn name(&self) -> &str {
        "Communities"
    }

    fn description(&self) -> &str {
        "Modularity (Louvain) communities over Calls+Imports edges, each summarized with its \
         top-PageRank members and dominant files. Answers 'what are the natural subsystems here?' \
         — clusters of symbols more tightly connected to each other than to the rest of the graph. \
         Use to map an unfamiliar codebase into modules or scope a refactor to a cohesive unit."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let limit = opt_u64(request, "limit").unwrap_or(20).clamp(1, 200) as usize;
        let min_size = opt_u64(request, "min_size").unwrap_or(2).max(1) as usize;
        let resolution = request
            .get("resolution")
            .and_then(|v| v.as_f64())
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(1.0);

        let mut diag = vec![staleness_note()];

        let params = wicked_estate_rank::CommunityParams {
            min_size,
            include_singletons: false,
            resolution,
            hierarchical: false,
            package_bias: 0.0,
        };

        // Detect (Louvain) then summarize (top-PageRank members + dominant files per community).
        let partition = wicked_estate_rank::detect_communities(store, &params)?;
        let summaries = wicked_estate_rank::summarize_communities(store, &partition, resolution)?;

        if summaries.is_empty() {
            diag.push(format!(
                "Communities: no communities of size >= {min_size} found \
                 (graph may be empty, edgeless, or fully fragmented)"
            ));
        }

        // summarize_communities returns largest-first; take the top `limit` before serializing.
        let total_detected = summaries.len();
        let mut rows: Vec<Value> = summaries
            .iter()
            .take(limit)
            .map(|s| {
                json!({
                    "size": s.size,
                    "top_symbols": s.top_symbols,
                    "dominant_files": s.dominant_files,
                    "modularity_contribution": s.modularity_contribution,
                })
            })
            .collect();

        let mut dropped_by_limit = total_detected.saturating_sub(rows.len());

        // R4 — cap under 25K chars; community rows carry up to 5 symbols + 3 files each, so a wide
        // graph with many communities can exceed budget even after `limit`. Trim loudly if so.
        let envelope_overhead = 96;
        let (capped, dropped_by_budget) = cap_rows_to_budget(rows, envelope_overhead);
        rows = capped;
        dropped_by_limit += dropped_by_budget;
        let truncated = dropped_by_limit > 0;
        if dropped_by_budget > 0 {
            diag.push(format!(
                "Communities: result truncated to {} community/communities to stay under the \
                 {}-char R4 budget ({} dropped); lower `limit`",
                rows.len(),
                R4_CHAR_BUDGET,
                dropped_by_budget
            ));
        }

        let total = rows.len();
        Ok(RetrievalResult {
            content: json!({
                "communities": rows,
                "total": total,
                "truncated": truncated,
            }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reciprocal Rank Fusion
// ─────────────────────────────────────────────────────────────────────────────

/// Reciprocal Rank Fusion over multiple ranked lists of [`SymbolId`]s.
///
/// RRF score for a symbol appearing at rank `r` (1-based) in a list:
/// `score += 1 / (k + r)`
///
/// The combined score is the sum across all lists.  `k` defaults to 60 (the value from the
/// original Cormack/Clarke paper; pass `60.0` unless you have tuned data).
///
/// Returns a `Vec<(SymbolId, f64)>` sorted descending by score, deduplicated.  Symbols that
/// appear high in **multiple** lists are ranked first, which is the desired property for fusing
/// graph-hop results with name-search results.
pub fn reciprocal_rank_fusion(lists: &[Vec<SymbolId>], k: f64) -> Vec<(SymbolId, f64)> {
    let mut scores: HashMap<SymbolId, f64> = HashMap::new();
    for list in lists {
        for (idx, id) in list.iter().enumerate() {
            let rank = (idx + 1) as f64;
            *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (k + rank);
        }
    }
    let mut out: Vec<(SymbolId, f64)> = scores.into_iter().collect();
    // Stable descending sort (higher score = better rank).
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// render_context  (W4.2 — token-budgeted elided-stub context)
// ─────────────────────────────────────────────────────────────────────────────

/// Default token budget when the caller does not supply one.
const DEFAULT_TOKEN_BUDGET: usize = 2_000;

/// Hard cap on the token budget — R4 (output < ~25K chars).
/// 6 000 tokens × 4 chars/token ≈ 24 000 chars — safely under the 25 K limit.
const MAX_TOKEN_BUDGET: usize = 6_000;

/// Candidate neighbourhood depth when gathering context around seeds.
/// Small on purpose: we want immediate callers/callees, not the whole graph.
const CONTEXT_DEPTH: u32 = 2;

/// Candidate node cap for the neighbourhood traversal.
const CONTEXT_MAX_NODES: usize = 200;

/// How many candidate symbols to feed to the ranker.
const RANK_CANDIDATE_CAP: usize = 500;

/// Estimate the number of tokens in a string (chars / 4, rounding up).
fn estimate_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

/// Render a single [`wicked_estate_core::Node`] as an elided stub:
///
/// ```text
/// <kind> <name><signature>
///   /// <first line of doc>
///   // <file>:<line>
/// ```
///
/// Full bodies are never included — only the signature + first doc line.
fn render_stub(node: &wicked_estate_core::Node) -> String {
    use wicked_estate_core::NodeKind;

    let kind_str = match &node.kind {
        NodeKind::Function => "fn",
        NodeKind::Method => "fn",
        NodeKind::Constructor => "fn",
        NodeKind::Class => "class",
        NodeKind::Struct => "struct",
        NodeKind::Enum => "enum",
        NodeKind::Interface => "interface",
        NodeKind::Trait => "trait",
        NodeKind::Module => "mod",
        NodeKind::Namespace => "namespace",
        NodeKind::TypeAlias => "type",
        NodeKind::Constant => "const",
        NodeKind::Variable => "var",
        NodeKind::Macro => "macro",
        NodeKind::Field => "field",
        NodeKind::Parameter => "param",
        NodeKind::Import => "import",
        NodeKind::File => "file",
        NodeKind::Synthetic => "synthetic",
        NodeKind::Rule => "rule",
        NodeKind::RuleSet => "ruleset",
        NodeKind::Condition => "condition",
        NodeKind::Action => "action",
        NodeKind::Fact => "fact",
        NodeKind::Other(s) => s.as_str(),
    };

    let sig = node.signature.as_deref().unwrap_or("");
    let header = if sig.is_empty() {
        format!("{kind_str} {}", node.name)
    } else {
        format!("{kind_str} {sig}")
    };

    let mut lines = vec![header];

    // First line of doc comment only — never the full body.
    if let Some(doc) = &node.doc {
        let first_line = doc.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            lines.push(format!("  /// {first_line}"));
        }
    }

    lines.push(format!(
        "  // {}:{}",
        node.location.file, node.location.span.start_line
    ));

    lines.join("\n")
}

/// Token-budgeted, ranked, elided-stub context renderer (W4.2).
///
/// # Algorithm
///
/// 1. **Gather candidates** — the seeds themselves plus their bounded neighbourhood
///    (both directions, depth [`CONTEXT_DEPTH`], cap [`CONTEXT_MAX_NODES`]).
///    This pulls in immediate callers and callees.
///
/// 2. **Rank** — personalised PageRank via [`wicked_estate_rank::ranked_symbols`] seeded on the
///    supplied `seeds`, capped at [`RANK_CANDIDATE_CAP`].  Seeds themselves score very
///    high (100× teleport weight), followed by their neighbourhood symbols.
///
/// 3. **Render highest-rank first** — each symbol is rendered as an *elided stub*
///    (`<kind> <name><sig>` + first doc line + `// <file>:<line>`), never a full body.
///
/// 4. **Pack until budget** — tokens ≈ chars / 4.  When the next stub would exceed the
///    budget, stop and record how many symbols were omitted.
///
/// # Returns
///
/// A `String` containing the packed context.  The last line is always a comment
/// reporting how many symbols were included and how many were omitted.
///
/// Empty seeds return an empty string (no panic).
pub fn render_context(
    store: &dyn GraphRead,
    seeds: &[SymbolId],
    token_budget: usize,
) -> Result<String> {
    if seeds.is_empty() {
        return Ok(String::new());
    }

    let budget = token_budget.clamp(1, MAX_TOKEN_BUDGET);

    // ── 1. Gather candidate set ──────────────────────────────────────────────
    // Start with the seeds, then pull in both-direction neighbours.
    let mut candidate_ids: HashSet<SymbolId> = seeds.iter().cloned().collect();

    let neighbour_spec = TraversalSpec {
        direction: Direction::Both,
        edge_kinds: vec![],
        max_depth: CONTEXT_DEPTH,
        max_nodes: CONTEXT_MAX_NODES,
        min_confidence: 0.0,
    };

    for seed in seeds {
        let subgraph = store.traverse(seed, &neighbour_spec)?;
        for node in &subgraph.nodes {
            candidate_ids.insert(node.symbol.clone());
        }
    }

    // ── 2. Rank ───────────────────────────────────────────────────────────────
    // ranked_symbols returns ALL nodes in the store ranked by personalized PR.
    // We keep only the candidates we gathered above, preserving their rank order.
    let ranked = wicked_estate_rank::ranked_symbols(store, seeds, RANK_CANDIDATE_CAP)?;

    // Build an ordered list: ranked entries that are in our candidate set.
    let mut ordered: Vec<SymbolId> = ranked
        .into_iter()
        .filter_map(|(id, _score)| {
            if candidate_ids.contains(&id) {
                Some(id)
            } else {
                None
            }
        })
        .collect();

    // Append any candidates not yet in the ranked list (they scored below the cap).
    // Seeds are guaranteed to appear in the ranked list because of their high teleport weight,
    // but neighbourhood nodes may not be.
    let ranked_set: HashSet<SymbolId> = ordered.iter().cloned().collect();
    for id in &candidate_ids {
        if !ranked_set.contains(id) {
            ordered.push(id.clone());
        }
    }

    // ── 3 & 4. Render highest-rank first, pack until budget ──────────────────
    let mut output_lines: Vec<String> = Vec::new();
    let mut chars_used: usize = 0;
    let mut included: usize = 0;
    let mut omitted: usize = 0;

    for id in &ordered {
        let Some(node) = store.get_node(id)? else {
            // Node not in store (stale candidate from traversal); skip silently.
            continue;
        };

        let stub = render_stub(&node);
        // +1 for the newline separator between stubs.
        let stub_tokens = estimate_tokens(&stub) + 1;

        if chars_used > 0 && estimate_tokens(&stub) + chars_used.div_ceil(4) > budget {
            omitted += 1;
            continue;
        }

        // Check if adding this stub would exceed the budget.
        let new_chars = chars_used + stub.len() + 1; // +1 for '\n'
        if estimate_tokens(&"\n".repeat(new_chars)) + stub_tokens > budget && chars_used > 0 {
            omitted += 1;
            continue;
        }

        output_lines.push(stub);
        chars_used += output_lines.last().unwrap().len() + 1;
        included += 1;

        // Early exit once we've clearly exceeded the budget.
        if estimate_tokens(&" ".repeat(chars_used)) >= budget {
            // Count remaining as omitted.
            omitted += ordered.len().saturating_sub(included + omitted);
            break;
        }
    }

    // Trailing summary comment.
    let summary = format!("// {included} symbol(s) shown; {omitted} omitted (token budget)");
    output_lines.push(summary);

    Ok(output_lines.join("\n"))
}

// ─────────────────────────────────────────────────────────────────────────────
// ContextPack  RetrievalTool
// ─────────────────────────────────────────────────────────────────────────────

/// Token-budgeted ranked elided-stub context pack (W4.2).
///
/// **Request shape**
/// ```json
/// { "seeds": ["<symbol-id>", …], "token_budget": 2000 }
/// ```
/// OR resolve by name:
/// ```json
/// { "name": "<symbol name>", "token_budget": 2000 }
/// ```
/// * `seeds` — one or more stable [`SymbolId`] strings.
/// * `name`  — symbol name to resolve; all matching symbols become seeds (alternative to `seeds`).
/// * `token_budget` — optional, default [`DEFAULT_TOKEN_BUDGET`], capped at [`MAX_TOKEN_BUDGET`].
///
/// **Response `content` shape**
/// ```json
/// { "context": "fn foo(x: i32) -> u32\n  // src/lib.rs:12\n…", "included": 5, "omitted": 3 }
/// ```
/// * `context` — the packed stub text, ready to paste into an LLM prompt.
/// * `included` — count of symbols rendered.
/// * `omitted`  — count of candidate symbols that did not fit in the budget.
#[derive(Debug, Default)]
pub struct ContextPack;

impl RetrievalTool for ContextPack {
    fn name(&self) -> &str {
        "ContextPack"
    }

    fn description(&self) -> &str {
        "Produce a token-budgeted, ranked, elided-stub context block for the given seed symbols. \
         Renders signature + first doc line (never full bodies), highest-PageRank symbols first, \
         packed within the requested token budget. \
         Use this to efficiently fill an LLM context window with the most relevant code."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        // ── parse seeds ──────────────────────────────────────────────────────
        let mut seeds: Vec<SymbolId> = Vec::new();
        let mut diag: Vec<String> = Vec::new();

        // Accept either explicit `seeds` array or a `name` to resolve.
        if let Some(arr) = request.get("seeds").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if !s.is_empty() {
                        seeds.push(SymbolId(s.to_string()));
                    }
                }
            }
        }

        if seeds.is_empty() {
            if let Some(name) = request.get("name").and_then(|v| v.as_str()) {
                if !name.is_empty() {
                    let q = SymbolQuery {
                        text: Some(name.to_string()),
                        limit: Some(20),
                        ..Default::default()
                    };
                    let hits = store.find_symbols(&q)?;
                    for node in &hits {
                        seeds.push(node.symbol.clone());
                    }
                    if seeds.is_empty() {
                        diag.push(format!(
                            "ContextPack: no symbols found matching name '{name}'"
                        ));
                    }
                }
            }
        }

        if seeds.is_empty() {
            diag.push(
                "ContextPack: provide 'seeds' (array of symbol ids) or 'name' (symbol name)"
                    .to_string(),
            );
            diag.push(staleness_note());
            return Ok(RetrievalResult {
                content: json!({ "context": "", "included": 0, "omitted": 0 }),
                diagnostics: diag,
            });
        }

        // ── parse token_budget ───────────────────────────────────────────────
        let token_budget = opt_u64(request, "token_budget")
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_TOKEN_BUDGET)
            .min(MAX_TOKEN_BUDGET);

        // ── render ───────────────────────────────────────────────────────────
        let context_text = render_context(store, &seeds, token_budget)?;

        // Count included/omitted from the summary line in the rendered text.
        // The last line is always "// N symbol(s) shown; M omitted (token budget)".
        let (included, omitted) = parse_summary_line(&context_text);

        // R4 size note.
        let char_count = context_text.len();
        if char_count > 20_000 {
            diag.push(format!(
                "R4-SIZE: context is {char_count} chars — approaching the ~25K agent cap"
            ));
        }
        diag.push(staleness_note());

        Ok(RetrievalResult {
            content: json!({
                "context": context_text,
                "included": included,
                "omitted": omitted,
            }),
            diagnostics: diag,
        })
    }
}

/// Parse `included` and `omitted` from the final summary line rendered by `render_context`.
/// Returns `(0, 0)` on any parse failure.
fn parse_summary_line(text: &str) -> (usize, usize) {
    // Line format: "// N symbol(s) shown; M omitted (token budget)"
    let Some(last) = text.lines().last() else {
        return (0, 0);
    };
    let stripped = last.trim_start_matches("// ");
    // "N symbol(s) shown; M omitted (token budget)"
    let parts: Vec<&str> = stripped.split(';').collect();
    if parts.len() < 2 {
        return (0, 0);
    }
    let included = parts[0]
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let omitted = parts[1]
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    (included, omitted)
}

// ─────────────────────────────────────────────────────────────────────────────
// FetchContent
// ─────────────────────────────────────────────────────────────────────────────

/// Return the exact source slice for a single symbol.
///
/// **Request shape**
/// ```json
/// { "symbol": "<id>" }
/// ```
///
/// **Response `content` shape**
/// ```json
/// { "found": true, "symbol": "…", "name": "foo",
///   "file": "src/lib.rs", "line": 12,
///   "source": "fn foo() -> u32 { … }" }
/// ```
/// When the symbol is not found, or the file's source has not been stored,
/// `found` is `false` and `source` is absent; a diagnostic is emitted instead
/// of an error (R1).
#[derive(Debug, Default)]
pub struct FetchContent;

impl RetrievalTool for FetchContent {
    fn name(&self) -> &str {
        "FetchContent"
    }

    fn description(&self) -> &str {
        "Return the exact source slice for a symbol (requires content to have been stored \
         during indexing). Returns the raw source text extracted from the symbol's byte span, \
         plus file:line for provenance. Returns found=false (no error) when the symbol is \
         absent or its file content has not been stored."
    }

    fn invoke(&self, store: &dyn GraphRead, request: &Value) -> Result<RetrievalResult> {
        let id_str = match request.get("symbol").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({ "found": false }),
                    diagnostics: vec!["FetchContent: 'symbol' field is required".to_string()],
                });
            }
        };

        let id = SymbolId(id_str.clone());
        let mut diag = vec![staleness_note()];

        let node = match store.get_node(&id)? {
            None => {
                diag.push(format!(
                    "FetchContent: symbol '{id_str}' not found in graph"
                ));
                return Ok(RetrievalResult {
                    content: json!({ "found": false, "symbol": id_str }),
                    diagnostics: diag,
                });
            }
            Some(n) => n,
        };

        let source = store.symbol_source(&node)?;

        if source.is_none() {
            diag.push(format!(
                "FetchContent: source not available for '{id_str}' \
                 (file content not stored or span is zero — re-run 'index' to populate)"
            ));
        }

        Ok(RetrievalResult {
            content: json!({
                "found": true,
                "symbol": node.symbol.as_str(),
                "name": node.name,
                "file": node.location.file,
                "line": node.location.span.start_line,
                "source": source,
            }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// W5.2 — Embedder abstraction + HashEmbedder + VectorStore + SemanticSearch
// ─────────────────────────────────────────────────────────────────────────────

/// Embed text into a fixed-dimensional vector for semantic (ANN) search.
///
/// **Object-safe**: implementations must be `Send + Sync` so the trait can be used behind `dyn`.
///
/// # Provided implementations
///
/// * [`HashEmbedder`] — deterministic, dependency-free, no model download.  Not semantic-quality
///   but proves the wiring and passes the test suite.
/// * (Optional, `fastembed` feature) `FastEmbedder` — real semantic quality via the
///   `fastembed` crate; gated behind `#[cfg(feature = "fastembed")]` so the default build and
///   test run download nothing.
pub trait Embedder: Send + Sync {
    /// Stable identity of this embedder (model family + variant), e.g. `"hash:v1"`,
    /// `"fastembed:bge-small-en-v1.5"`, `"model2vec:minishlab/potion-base-8M"`.
    ///
    /// **Identity, not dimension, is the correctness key**: two distinct 384-d models produce
    /// incomparable vectors, so a store embedded with one cannot be queried with the other even
    /// though the dims match. The dim-guard (`index --embeddings` writes this to store meta; the
    /// MCP server compares it against the runtime embedder) keys on this string, not `dim()`.
    fn id(&self) -> &str;

    /// Embed `text` into a `dim()`-dimensional L2-normalised vector.
    fn embed(&self, text: &str) -> Vec<f32>;

    /// Dimensionality of the output vector.
    fn dim(&self) -> usize;

    /// Whether this embedder carries real semantic signal (`true` for model-backed embedders —
    /// the default) or is a lexical fallback whose nearest-neighbour lists must NOT be fused as a
    /// peer semantic retriever (`false`; overridden by [`HashEmbedder`]).
    ///
    /// Recall pipelines gate their vector candidate list on this: fusing a hash fallback's
    /// "nearest" as if it were semantics injects rank noise that degrades every query class
    /// (measured: the S3 parity bench, keyword-only vs hash-fused on 60 queries × 5 classes).
    fn is_semantic(&self) -> bool {
        true
    }
}

/// Forwarding impl so a `Box<dyn Embedder>` — the runtime-selected embedder (`FastEmbedder` under
/// the `fastembed` feature, else `HashEmbedder`) — satisfies the `impl Embedder` call sites
/// (`SemanticSearch::new`, `compute_embeddings`) without monomorphising per concrete type.
impl Embedder for Box<dyn Embedder> {
    fn id(&self) -> &str {
        (**self).id()
    }
    fn embed(&self, text: &str) -> Vec<f32> {
        (**self).embed(text)
    }
    fn dim(&self) -> usize {
        (**self).dim()
    }
    fn is_semantic(&self) -> bool {
        (**self).is_semantic()
    }
}

// ── HashEmbedder ─────────────────────────────────────────────────────────────

/// Deterministic, zero-dependency bag-of-words embedder.
///
/// Splits text into whitespace-delimited tokens, hashes each token into a bucket in a
/// fixed-dimension vector via FNV-1a (no external dep), accumulates TF-style counts, then
/// L2-normalises the result.  Output is reproducible across runs and platforms.
///
/// Quality: not semantically meaningful — exists to prove the wiring and serve as the default
/// in tests and in `SemanticSearch` when no real embedder is provided.  Swap for `FastEmbedder`
/// (feature `fastembed`) or any other `Embedder` impl for production use.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    /// Create a `HashEmbedder` with `dim` output dimensions.
    /// Panics if `dim` is zero.
    pub fn new(dim: usize) -> Self {
        assert!(dim > 0, "HashEmbedder: dim must be > 0");
        Self { dim }
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new(128)
    }
}

/// FNV-1a 32-bit hash of a byte slice.  No external dep, no `std::collections::HashMap`.
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    h
}

impl Embedder for HashEmbedder {
    fn id(&self) -> &str {
        // Dimension-independent: every HashEmbedder shares the same FNV-1a bag-of-words algorithm,
        // so identity is the algorithm version. The dim is tagged separately in store meta.
        "hash:v1"
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0_f32; self.dim];
        for token in text.split_whitespace() {
            let h = fnv1a(token.as_bytes()) as usize;
            v[h % self.dim] += 1.0;
        }
        // L2-normalise.
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }

    fn dim(&self) -> usize {
        self.dim
    }

    /// A hashed bag-of-words is a lexical proxy, not semantics ("not semantically meaningful —
    /// exists to prove the wiring", above). Recall pipelines must not fuse its neighbours as a
    /// peer semantic retriever.
    fn is_semantic(&self) -> bool {
        false
    }
}

// ── FastEmbedder (optional, feature `fastembed`) ────────────────────────────────

/// Real semantic embedder backed by an ONNX model (BAAI/bge-small-en-v1.5, 384-dim) via the
/// `fastembed` crate. Gated behind the `fastembed` feature so the default build pulls no ONNX
/// runtime and downloads no model. The model is fetched from HuggingFace on first construction
/// and cached on disk thereafter, so [`FastEmbedder::new`] is fallible (network / disk).
///
/// Output vectors are L2-normalised, matching [`HashEmbedder`], so the same cosine path applies.
/// IMPORTANT: index-time and query-time must use the SAME embedder — this model's dimension (384)
/// differs from `HashEmbedder`'s default (128), so a store embedded with one cannot be queried
/// with the other. The feature flag fixes the embedder per-binary, keeping that consistent.
///
/// `fastembed::TextEmbedding::embed` takes `&mut self`, so the model lives behind a `Mutex` to
/// satisfy the `&self` + `Send + Sync` contract of [`Embedder`].
#[cfg(feature = "fastembed")]
pub struct FastEmbedder {
    model: std::sync::Mutex<fastembed::TextEmbedding>,
    dim: usize,
}

#[cfg(feature = "fastembed")]
impl FastEmbedder {
    /// Load the default model (bge-small-en-v1.5); downloads + caches it on first use.
    pub fn new() -> wicked_estate_core::Result<Self> {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
        let mut model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15).with_show_download_progress(false),
        )
        .map_err(|e| {
            wicked_estate_core::Error::Invalid(format!("fastembed: model load failed: {e}"))
        })?;
        // Probe the output dimensionality once so `dim()` reflects the model, not a hard-coded guess.
        let probe = model.embed(vec!["probe"], None).map_err(|e| {
            wicked_estate_core::Error::Invalid(format!("fastembed: probe embed failed: {e}"))
        })?;
        let dim = probe.first().map_or(384, Vec::len);
        Ok(Self {
            model: std::sync::Mutex::new(model),
            dim,
        })
    }
}

#[cfg(feature = "fastembed")]
impl Embedder for FastEmbedder {
    fn id(&self) -> &str {
        // The default model is fixed to BGE-small-en-v1.5 in `FastEmbedder::new`; the id names it
        // so a store embedded with BGE is never silently queried with a different model.
        "fastembed:bge-small-en-v1.5"
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let result = self
            .model
            .lock()
            .expect("FastEmbedder mutex poisoned")
            .embed(vec![text], None);
        match result {
            Ok(mut batch) if !batch.is_empty() => {
                let mut v = batch.swap_remove(0);
                // L2-normalise (bge output is already unit-norm; enforce for cosine parity).
                let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in &mut v {
                        *x /= norm;
                    }
                }
                v
            }
            // Post-load inference failure is rare; degrade visibly to a zero vector (sorts last in
            // cosine) rather than panicking in the infallible trait method.
            Ok(_) => vec![0.0; self.dim],
            Err(err) => {
                eprintln!("EMBED-FALLBACK: fastembed inference failed: {err}");
                vec![0.0; self.dim]
            }
        }
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

// ── Model2VecEmbedder (optional, feature `model2vec`) ───────────────────────────

/// Static semantic embedder backed by a distilled model2vec model (default `minishlab/potion-base-8M`)
/// via the `model2vec-rs` crate. Gated behind the `model2vec` feature. Unlike [`FastEmbedder`] it
/// pulls NO ONNX runtime — embeddings are a token→vector lookup + pooling, so it is far lighter and
/// ~10-100× faster, at the cost of being non-contextual (quality below BGE, well above the lexical
/// [`HashEmbedder`]).
///
/// Configurable: set `CI_MODEL2VEC_MODEL` to any model2vec HF repo id or local path to swap the
/// model without recompiling. Output is L2-normalised (`normalize = true`), matching the other
/// embedders + the cosine path. `StaticModel` is `Send + Sync` and `encode_single` takes `&self`,
/// so no interior mutability is needed. The model is fetched on first construction (fallible).
#[cfg(feature = "model2vec")]
pub struct Model2VecEmbedder {
    model: model2vec_rs::model::StaticModel,
    dim: usize,
    /// Stable identity `"model2vec:<repo_or_path>"` — the model is configurable (`CI_MODEL2VEC_MODEL`),
    /// so the id must carry the actual loaded model, not a hard-coded default. Cached at construction
    /// because [`Embedder::id`] returns a borrowed `&str`.
    id: String,
}

#[cfg(feature = "model2vec")]
impl Model2VecEmbedder {
    /// Default model (`minishlab/potion-base-8M`), or `CI_MODEL2VEC_MODEL` if set.
    pub fn new() -> wicked_estate_core::Result<Self> {
        let model_id = std::env::var("CI_MODEL2VEC_MODEL")
            .unwrap_or_else(|_| "minishlab/potion-base-8M".to_string());
        Self::from_model(&model_id)
    }

    /// Load a specific model2vec model by HuggingFace repo id or local path.
    pub fn from_model(repo_or_path: &str) -> wicked_estate_core::Result<Self> {
        use model2vec_rs::model::StaticModel;
        // normalize = true → unit vectors, matching HashEmbedder/FastEmbedder + the cosine path.
        let model =
            StaticModel::from_pretrained(repo_or_path, None, Some(true), None).map_err(|e| {
                wicked_estate_core::Error::Invalid(format!(
                    "model2vec: load '{repo_or_path}' failed: {e}"
                ))
            })?;
        let dim = model.encode_single("probe").len();
        if dim == 0 {
            return Err(wicked_estate_core::Error::Invalid(
                "model2vec: model produced a zero-dimension embedding".into(),
            ));
        }
        Ok(Self {
            model,
            dim,
            id: format!("model2vec:{repo_or_path}"),
        })
    }
}

#[cfg(feature = "model2vec")]
impl Embedder for Model2VecEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let v = self.model.encode_single(text);
        // Empty/blank input can yield an empty vector; keep dim consistent for storage + cosine.
        if v.is_empty() { vec![0.0; self.dim] } else { v }
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

// ── VectorStore ───────────────────────────────────────────────────────────────

/// Object-safe, `Send`-only wrapper around the concrete-store `nearest` method.
///
/// `nearest` lives as an *inherent* method on `SqliteStore` and `MemStore` (not on the
/// `GraphRead` trait, which must stay object-safe and topology-only).  `VectorStore` is the
/// thin bridge trait that lets `SemanticSearch` hold a `Box<dyn VectorStore>` without
/// knowing which concrete store it has.
///
/// # Thread-safety note
///
/// `VectorStore` requires only `Send` (not `Sync`) because `rusqlite::Connection` is `Send`
/// but not `Sync`.  `SemanticSearch` wraps the `Box<dyn VectorStore>` in a `Mutex` so the
/// whole struct is `Send + Sync` as required by `RetrievalTool`.
pub trait VectorStore: Send {
    /// Retrieve the `k` nearest symbols to `query_vec` by cosine similarity.
    fn nearest(
        &self,
        query_vec: &[f32],
        k: usize,
    ) -> wicked_estate_core::Result<Vec<(SymbolId, f32)>>;

    /// Return every stored `(symbol, embedding)` pair. Order is unspecified.
    ///
    /// Powers whole-corpus analyses (semantic clustering) that need the full vector set rather
    /// than a top-k query. A store with no vector layer returns an empty `Vec`.
    fn all_embeddings(&self) -> wicked_estate_core::Result<Vec<(SymbolId, Vec<f32>)>>;
}

impl VectorStore for wicked_estate_store::MemStore {
    fn nearest(
        &self,
        query_vec: &[f32],
        k: usize,
    ) -> wicked_estate_core::Result<Vec<(SymbolId, f32)>> {
        wicked_estate_store::MemStore::nearest(self, query_vec, k)
    }

    fn all_embeddings(&self) -> wicked_estate_core::Result<Vec<(SymbolId, Vec<f32>)>> {
        wicked_estate_store::MemStore::all_embeddings(self)
    }
}

impl VectorStore for wicked_estate_store::SqliteStore {
    fn nearest(
        &self,
        query_vec: &[f32],
        k: usize,
    ) -> wicked_estate_core::Result<Vec<(SymbolId, f32)>> {
        wicked_estate_store::SqliteStore::nearest(self, query_vec, k)
    }

    fn all_embeddings(&self) -> wicked_estate_core::Result<Vec<(SymbolId, Vec<f32>)>> {
        wicked_estate_store::SqliteStore::all_embeddings(self)
    }
}

// ── semantic_search free function ─────────────────────────────────────────────

/// Find the `k` semantically nearest symbols to `query` text.
///
/// Embeds `query` with `embedder`, calls `store.nearest`, and resolves the returned
/// `SymbolId`s to nodes by looking them up in `graph`.
///
/// # Design note
///
/// `S: VectorStore` is a concrete bound rather than `&dyn GraphRead` because the `nearest`
/// method lives on the concrete types, not on the `GraphRead` trait.  Pass the same store
/// for both `graph` and the `S` parameter — they just need different types since we call
/// different method sets.  Alternatively, use [`SemanticSearch`] as a `RetrievalTool`.
pub fn semantic_search<S: VectorStore>(
    graph: &dyn GraphRead,
    store: &S,
    embedder: &dyn Embedder,
    query: &str,
    k: usize,
) -> wicked_estate_core::Result<Vec<(SymbolId, f32)>> {
    let qvec = embedder.embed(query);
    let hits = store.nearest(&qvec, k)?;
    // Filter to symbols that actually exist in the graph (embeddings may be stale).
    let mut out = Vec::with_capacity(hits.len());
    for (id, sim) in hits {
        if graph.get_node(&id)?.is_some() {
            out.push((id, sim));
        }
    }
    Ok(out)
}

// ── hybrid_search ─────────────────────────────────────────────────────────────

/// Fuse name/graph search results with semantic-nearest results via RRF.
///
/// # Arguments
///
/// * `name_results`     — ordered `SymbolId`s from a name/FTS search.
/// * `graph_results`    — ordered `SymbolId`s from a graph-hop traversal.
/// * `semantic_results` — ordered `SymbolId`s from [`semantic_search`] / `nearest`.
/// * `k`                — RRF constant (60.0 per the Cormack/Clarke paper).
///
/// Returns the RRF-fused ranked list via [`reciprocal_rank_fusion`].
pub fn hybrid_search(
    name_results: Vec<SymbolId>,
    graph_results: Vec<SymbolId>,
    semantic_results: Vec<SymbolId>,
    k: f64,
) -> Vec<(SymbolId, f64)> {
    reciprocal_rank_fusion(&[name_results, graph_results, semantic_results], k)
}

// ── SemanticSearch RetrievalTool ──────────────────────────────────────────────

/// Semantic (embedding-based) search over indexed symbols.
///
/// **Request shape**
/// ```json
/// { "query": "<natural language text>", "k": 10 }
/// ```
/// * `query` (required) — text to embed and search.
/// * `k`     (optional, default 10, max 100) — number of results.
///
/// **Response `content` shape**
/// ```json
/// { "matches": [ { "symbol": "…", "name": "…", "kind": "…",
///                  "file": "…", "line": 0, "similarity": 0.92 }, … ],
///   "total": N }
/// ```
///
/// # `&dyn GraphRead` constraint
///
/// `RetrievalTool::invoke` receives `&dyn GraphRead`, but `nearest` lives on the *concrete*
/// store types (not on the trait).  `SemanticSearch` therefore stores a `Mutex<Box<dyn
/// VectorStore>>` alongside the embedder.  Construct via [`SemanticSearch::new`] passing the
/// concrete store.  The same physical store is passed to `invoke` as `&dyn GraphRead` for
/// node resolution.
pub struct SemanticSearch {
    embedder: Box<dyn Embedder>,
    // Mutex: VectorStore is Send but not Sync (rusqlite::Connection is Send-only).
    // Wrapping in Mutex<Box<...>> makes SemanticSearch Send + Sync as required by RetrievalTool.
    vector_store: std::sync::Mutex<Box<dyn VectorStore>>,
}

impl std::fmt::Debug for SemanticSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticSearch").finish_non_exhaustive()
    }
}

impl SemanticSearch {
    /// Create a `SemanticSearch` tool backed by `store` for vector lookup and `embedder` for
    /// query embedding.
    ///
    /// Pass the same physical store to `invoke` as the `&dyn GraphRead` argument for node
    /// resolution.  The store is wrapped in a `Mutex` internally so `invoke` can be called
    /// from multiple threads (each call locks briefly only during the `nearest` scan).
    pub fn new(embedder: impl Embedder + 'static, store: impl VectorStore + 'static) -> Self {
        Self {
            embedder: Box::new(embedder),
            vector_store: std::sync::Mutex::new(Box::new(store)),
        }
    }

    /// Create a `SemanticSearch` tool using the default [`HashEmbedder`] (128-dim, deterministic).
    pub fn with_hash_embedder(store: impl VectorStore + 'static) -> Self {
        Self::new(HashEmbedder::default(), store)
    }
}

impl RetrievalTool for SemanticSearch {
    fn name(&self) -> &str {
        "SemanticSearch"
    }

    fn description(&self) -> &str {
        "Semantic (embedding-based) symbol search. Embeds the query text and returns the \
         k nearest symbols by cosine similarity. Complements name/FTS search when the caller \
         knows the concept but not the exact symbol name. Fuse with name results via hybrid_search."
    }

    fn invoke(
        &self,
        store: &dyn GraphRead,
        request: &Value,
    ) -> wicked_estate_core::Result<RetrievalResult> {
        let query_str = match request.get("query").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                return Ok(RetrievalResult {
                    content: json!({ "matches": [], "total": 0 }),
                    diagnostics: vec![
                        "SemanticSearch: 'query' field is required and must be a non-empty string"
                            .to_string(),
                    ],
                });
            }
        };

        let k = opt_u64(request, "k").unwrap_or(10).min(100) as usize;
        let qvec = self.embedder.embed(&query_str);

        let vs = self
            .vector_store
            .lock()
            .expect("VectorStore mutex poisoned");
        let hits = vs.nearest(&qvec, k)?;
        drop(vs); // release mutex before the graph node lookups

        let mut matches = Vec::with_capacity(hits.len());
        for (id, sim) in &hits {
            let Some(node) = store.get_node(id)? else {
                continue;
            };
            matches.push(json!({
                "symbol": node.symbol.as_str(),
                "name": node.name,
                "kind": serde_json::to_value(&node.kind).unwrap_or(Value::Null),
                "file": node.location.file,
                "line": node.location.span.start_line,
                "similarity": sim,
            }));
        }

        let mut diag = vec![staleness_note()];
        if matches.is_empty() {
            diag.push(format!(
                "SemanticSearch: no symbols found for query '{query_str}' \
                 (embeddings may not have been populated yet)"
            ));
        }
        if !store.capabilities().vector_search {
            diag.push(
                "COVERAGE: store does not advertise vector_search capability; \
                 embeddings may not be stored"
                    .to_string(),
            );
        }

        let total = matches.len();
        Ok(RetrievalResult {
            content: json!({ "matches": matches, "total": total }),
            diagnostics: diag,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RulesInventory
// ─────────────────────────────────────────────────────────────────────────────

/// Inventory of all rules-engine nodes and the code that invokes them (W15).
///
/// **Request shape** — no required parameters; the scan is whole-graph.
/// ```json
/// {}
/// ```
///
/// **Response `content` shape**
/// ```json
/// { "engines": [
///     { "symbol": "…", "name": "…", "kind": "rule_set", "file": "…",
///       "invoked_by": [ "src/caller.rs::run_rules", … ] }
///   ],
///   "total": 1
/// }
/// ```
/// * `engines` — one entry per `NodeKind::RuleSet` node in the graph.
/// * `invoked_by` — symbols that carry an `InvokedBy` edge whose **target** is this
///   RuleSet (i.e. code → rules engine boundary).
/// * The result is bounded by the graph size; no pagination parameter is needed because
///   real codebases seldom have more than a handful of rules engines.
#[derive(Debug, Default)]
pub struct RulesInventory;

impl RetrievalTool for RulesInventory {
    fn name(&self) -> &str {
        "RulesInventory"
    }

    fn description(&self) -> &str {
        "List all rules-engine nodes (RuleSet, Rule) in the graph and the code that invokes them. \
         Use this to discover what business rules engines are present and which code files call \
         them. Returns: [{name, kind, file, invoked_by: [code_files]}]"
    }

    fn invoke(&self, store: &dyn GraphRead, _request: &Value) -> Result<RetrievalResult> {
        let all_nodes = store.all_nodes()?;
        let all_edges = store.all_edges()?;

        let rule_sets: Vec<_> = all_nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::RuleSet))
            .collect();

        let engines: Vec<Value> = rule_sets
            .iter()
            .map(|rs| {
                // Find all edges where kind == InvokedBy AND target == this RuleSet.
                // Edge direction convention: source = dependent, target = dependency.
                // An InvokedBy edge records: source = code call site, target = RuleSet.
                let invokers: Vec<String> = all_edges
                    .iter()
                    .filter(|e| e.kind == EdgeKind::InvokedBy && e.target == rs.symbol)
                    .map(|e| e.source.0.clone())
                    .collect();

                json!({
                    "symbol": rs.symbol.as_str(),
                    "name": rs.name,
                    "kind": "rule_set",
                    "file": rs.location.file,
                    "invoked_by": invokers,
                })
            })
            .collect();

        let total = engines.len();
        let mut diag = vec![staleness_note()];
        if engines.is_empty() {
            diag.push(
                "RulesInventory: no RuleSet nodes found — \
                 rules engine nodes are populated by the W15 extractor"
                    .to_string(),
            );
        }

        Ok(RetrievalResult {
            content: json!({ "engines": engines, "total": total }),
            diagnostics: diag,
        })
    }
}

// budget_context
// ─────────────────────────────────────────────────────────────────────────────

/// Returns the highest-ranked graph neighbors of `name` that fit within `max_chars`.
///
/// Algorithm:
/// 1. Find all nodes matching `name` via `find_symbols`.
/// 2. For each match, collect direct callers (in-edges) and callees (out-edges); score them 2.0.
///    Also collect up to 10 FTS-ranked symbols from a name-based search; score them 1.0.
/// 3. Sort candidates by score descending.
/// 4. Pack into the budget by accumulating `node.name.len() + node.location.file.len() + 100`
///    per node (a lightweight proxy that avoids deserializing the data field).
/// 5. Return the packed list (seed node excluded).
pub fn budget_context(
    store: &dyn wicked_estate_core::traits::GraphRead,
    name: &str,
    max_chars: usize,
) -> wicked_estate_core::Result<Vec<wicked_estate_core::Node>> {
    if max_chars == 0 {
        return Ok(Vec::new());
    }

    let seed_query = SymbolQuery {
        text: Some(name.to_string()),
        limit: Some(20),
        ..Default::default()
    };
    let seeds = store.find_symbols(&seed_query)?;
    if seeds.is_empty() {
        return Ok(Vec::new());
    }

    let seed_ids: HashSet<SymbolId> = seeds.iter().map(|n| n.symbol.clone()).collect();

    let mut scores: HashMap<SymbolId, f64> = HashMap::new();

    for seed in &seeds {
        let out_edges = store.neighbors(&seed.symbol, Direction::Dependencies)?;
        for e in out_edges {
            if !seed_ids.contains(&e.target) {
                *scores.entry(e.target.clone()).or_insert(0.0) += 2.0;
            }
        }
        let in_edges = store.neighbors(&seed.symbol, Direction::Dependents)?;
        for e in in_edges {
            if !seed_ids.contains(&e.source) {
                *scores.entry(e.source.clone()).or_insert(0.0) += 2.0;
            }
        }
    }

    let fts_query = SymbolQuery {
        text: Some(name.to_string()),
        limit: Some(10),
        ..Default::default()
    };
    let fts_hits = store.find_symbols(&fts_query)?;
    for node in fts_hits {
        if !seed_ids.contains(&node.symbol) {
            scores.entry(node.symbol.clone()).or_insert(1.0);
        }
    }

    let mut ranked: Vec<(SymbolId, f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.0.cmp(&b.0.0))
    });

    let mut out: Vec<wicked_estate_core::Node> = Vec::new();
    let mut chars_used: usize = 0;

    for (id, _) in &ranked {
        let Some(node) = store.get_node(id)? else {
            continue;
        };
        let node_size = node.name.len() + node.location.file.len() + 100;
        if chars_used + node_size > max_chars {
            break;
        }
        chars_used += node_size;
        out.push(node);
    }

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wicked_estate_core::{
        Confidence, EdgeKind, Language, Location, Node, NodeKind, ResolutionTier, Span, SymbolId,
    };
    use wicked_estate_core::{Edge, GraphWrite};
    use wicked_estate_store::MemStore;

    // ── Fixture helpers ──────────────────────────────────────────────────────

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

    fn make_node(id: &str, name: &str, kind: NodeKind, file: &str, line: u32) -> Node {
        Node::new(
            SymbolId(id.to_string()),
            kind,
            name,
            Language::new("rust"),
            Location::new(file, span(line)),
        )
    }

    fn make_call_edge(src: &str, tgt: &str) -> Edge {
        Edge::new(
            SymbolId(src.to_string()),
            SymbolId(tgt.to_string()),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test-fixture",
        )
    }

    /// Build a call chain: `caller` → `middle` → `leaf`
    ///
    ///  Blast-radius of `leaf` should include `middle` and `caller`.
    fn fixture_store() -> MemStore {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node("caller", "caller_fn", NodeKind::Function, "src/a.rs", 1),
                make_node("middle", "middle_fn", NodeKind::Function, "src/b.rs", 10),
                make_node("leaf", "leaf_fn", NodeKind::Function, "src/c.rs", 20),
            ])
            .unwrap();
        store
            .upsert_edges(&[
                make_call_edge("caller", "middle"), // caller calls middle
                make_call_edge("middle", "leaf"),   // middle calls leaf
            ])
            .unwrap();
        store.commit_batch().unwrap();
        store
    }

    // ── SearchEntity ─────────────────────────────────────────────────────────

    #[test]
    fn search_entity_exact_hit() {
        let store = fixture_store();
        let tool = SearchEntity;
        let res = tool.invoke(&store, &json!({"name": "middle_fn"})).unwrap();

        let matches = res.content["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1, "exactly one match");
        assert_eq!(matches[0]["name"].as_str().unwrap(), "middle_fn");
        assert_eq!(matches[0]["file"].as_str().unwrap(), "src/b.rs");
        assert_eq!(matches[0]["line"].as_u64().unwrap(), 10);
    }

    #[test]
    fn search_entity_substring_hit() {
        let store = fixture_store();
        let tool = SearchEntity;
        let res = tool.invoke(&store, &json!({"name": "_fn"})).unwrap();

        let matches = res.content["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 3, "all three *_fn symbols match substring");
    }

    #[test]
    fn search_entity_no_results_returns_empty_not_error() {
        let store = fixture_store();
        let tool = SearchEntity;
        let res = tool
            .invoke(&store, &json!({"name": "does_not_exist_xyz"}))
            .unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        assert!(
            !res.diagnostics.is_empty(),
            "diagnostic expected for no results"
        );
    }

    #[test]
    fn search_entity_missing_name_field_returns_diagnostic() {
        let store = fixture_store();
        let tool = SearchEntity;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap_or(0), 0);
        assert!(
            res.diagnostics.iter().any(|d| d.contains("'name' field")),
            "should explain missing field"
        );
    }

    #[test]
    fn search_entity_respects_limit() {
        let store = fixture_store();
        let tool = SearchEntity;
        let res = tool
            .invoke(&store, &json!({"name": "_fn", "limit": 2}))
            .unwrap();

        let matches = res.content["matches"].as_array().unwrap();
        assert!(matches.len() <= 2, "limit must be respected");
    }

    // ── RetrieveEntity ───────────────────────────────────────────────────────

    #[test]
    fn retrieve_entity_found() {
        let store = fixture_store();
        let tool = RetrieveEntity;
        let res = tool.invoke(&store, &json!({"symbol": "leaf"})).unwrap();

        assert!(res.content["found"].as_bool().unwrap());
        assert_eq!(res.content["name"].as_str().unwrap(), "leaf_fn");
        assert_eq!(res.content["file"].as_str().unwrap(), "src/c.rs");
    }

    #[test]
    fn retrieve_entity_not_found_returns_found_false_not_error() {
        let store = fixture_store();
        let tool = RetrieveEntity;
        let res = tool
            .invoke(&store, &json!({"symbol": "nonexistent"}))
            .unwrap();

        assert!(!res.content["found"].as_bool().unwrap());
        assert!(
            res.diagnostics.iter().any(|d| d.contains("not found")),
            "should note symbol not found"
        );
    }

    #[test]
    fn retrieve_entity_missing_field_returns_diagnostic() {
        let store = fixture_store();
        let tool = RetrieveEntity;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert!(!res.content["found"].as_bool().unwrap());
        assert!(!res.diagnostics.is_empty());
    }

    // ── TraverseGraph ────────────────────────────────────────────────────────

    #[test]
    fn traverse_graph_dependencies() {
        let store = fixture_store();
        let tool = TraverseGraph;
        // Follow dependencies (forward): caller → middle → leaf
        let res = tool
            .invoke(
                &store,
                &json!({"symbol": "caller", "depth": 4, "direction": "dependencies"}),
            )
            .unwrap();

        let nodes = res.content["nodes"].as_array().unwrap();
        let names: Vec<&str> = nodes.iter().map(|n| n["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"middle_fn"), "middle reachable from caller");
        assert!(
            names.contains(&"leaf_fn"),
            "leaf reachable from caller via middle"
        );
    }

    #[test]
    fn traverse_graph_dependents_from_middle() {
        let store = fixture_store();
        let tool = TraverseGraph;
        let res = tool
            .invoke(
                &store,
                &json!({"symbol": "middle", "direction": "dependents", "depth": 4}),
            )
            .unwrap();

        let nodes = res.content["nodes"].as_array().unwrap();
        let names: Vec<&str> = nodes.iter().map(|n| n["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"caller_fn"),
            "caller is a dependent of middle"
        );
    }

    #[test]
    fn traverse_graph_depths_recorded() {
        let store = fixture_store();
        let tool = TraverseGraph;
        let res = tool
            .invoke(
                &store,
                &json!({"symbol": "caller", "direction": "dependencies", "depth": 4}),
            )
            .unwrap();

        let depths = &res.content["depths"];
        // middle is at depth 1, leaf at depth 2
        assert_eq!(depths["middle"].as_u64().unwrap_or(99), 1);
        assert_eq!(depths["leaf"].as_u64().unwrap_or(99), 2);
    }

    #[test]
    fn traverse_graph_missing_symbol_returns_empty_not_error() {
        let store = fixture_store();
        let tool = TraverseGraph;
        let res = tool.invoke(&store, &json!({"symbol": "ghost"})).unwrap();

        let nodes = res.content["nodes"].as_array().unwrap();
        assert!(nodes.is_empty());
        assert!(!res.diagnostics.is_empty());
    }

    // ── BlastRadius ───────────────────────────────────────────────────────────

    #[test]
    fn blast_radius_of_leaf_includes_callers() {
        let store = fixture_store();
        let tool = BlastRadius;
        let res = tool
            .invoke(&store, &json!({"symbol": "leaf", "depth": 8}))
            .unwrap();

        let deps = res.content["dependents"].as_array().unwrap();
        let names: Vec<&str> = deps.iter().map(|d| d["name"].as_str().unwrap()).collect();

        // Both middle (direct) and caller (transitive) call leaf.
        assert!(names.contains(&"middle_fn"), "middle calls leaf directly");
        assert!(
            names.contains(&"caller_fn"),
            "caller transitively reaches leaf"
        );

        // Leaf itself should NOT appear in its own blast radius.
        assert!(
            !names.contains(&"leaf_fn"),
            "start symbol excluded from blast radius"
        );

        // Total matches.
        assert_eq!(res.content["total"].as_u64().unwrap(), 2);

        // Coverage fields are always present.
        assert!(
            res.content.get("unresolved_callers").is_some(),
            "unresolved_callers field present"
        );
        assert!(
            res.content.get("confidence").is_some(),
            "confidence field present"
        );
    }

    #[test]
    fn blast_radius_leaf_fn_depth_field() {
        let store = fixture_store();
        let tool = BlastRadius;
        let res = tool.invoke(&store, &json!({"symbol": "leaf"})).unwrap();

        let deps = res.content["dependents"].as_array().unwrap();
        let middle = deps
            .iter()
            .find(|d| d["name"].as_str().unwrap() == "middle_fn")
            .unwrap();
        assert_eq!(middle["depth"].as_u64().unwrap(), 1);
    }

    #[test]
    fn blast_radius_of_top_level_caller_is_empty() {
        let store = fixture_store();
        let tool = BlastRadius;
        let res = tool.invoke(&store, &json!({"symbol": "caller"})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        assert!(
            res.diagnostics.iter().any(|d| d.contains("no dependents")),
            "diagnostic expected for empty blast radius"
        );
        // Coverage note emitted even when there are zero dependents.
        assert!(
            res.diagnostics.iter().any(|d| d.contains("coverage:")),
            "coverage diagnostic always emitted"
        );
    }

    #[test]
    fn blast_radius_missing_field() {
        let store = fixture_store();
        let tool = BlastRadius;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap_or(0), 0);
        assert!(!res.diagnostics.is_empty());
    }

    #[test]
    fn blast_radius_reports_unresolved_callers_and_coverage_diagnostic() {
        use wicked_estate_core::{EdgeKind, Location, Span, UnresolvedRef};

        // Build: resolved call chain AND an unresolved ref to the same target name.
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node(
                    "target",
                    "target_fn",
                    NodeKind::Function,
                    "src/target.rs",
                    1,
                ),
                make_node(
                    "known_caller",
                    "known_caller_fn",
                    NodeKind::Function,
                    "src/a.rs",
                    5,
                ),
            ])
            .unwrap();
        // Resolved edge: known_caller → target
        store
            .upsert_edges(&[make_call_edge("known_caller", "target")])
            .unwrap();
        // Unresolved ref: something called "target_fn" from a site that the resolver could not bind.
        let unresolved = UnresolvedRef::new(
            SymbolId("mystery_caller".to_string()),
            "target_fn",
            EdgeKind::Calls,
            Location::new("src/mystery.rs", Span::ZERO),
        );
        store.upsert_unresolved_refs(&[unresolved]).unwrap();
        store.commit_batch().unwrap();

        let tool = BlastRadius;
        let res = tool.invoke(&store, &json!({"symbol": "target"})).unwrap();

        // Resolved callers appear in dependents.
        let deps = res.content["dependents"].as_array().unwrap();
        let names: Vec<&str> = deps.iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"known_caller_fn"),
            "resolved caller in dependents"
        );

        // unresolved_callers >= 1 (the mystery ref).
        let unresolved_callers = res.content["unresolved_callers"].as_u64().unwrap();
        assert!(
            unresolved_callers >= 1,
            "at least one unresolved caller reported"
        );

        // Coverage diagnostic is present.
        assert!(
            res.diagnostics.iter().any(|d| d.contains("coverage:")),
            "coverage diagnostic emitted"
        );

        // The diagnostic names the symbol and mentions unresolved callers.
        let cov_diag = res
            .diagnostics
            .iter()
            .find(|d| d.contains("coverage:"))
            .unwrap();
        assert!(
            cov_diag.contains("target_fn"),
            "coverage note names the symbol"
        );
        assert!(
            cov_diag.contains("unresolved"),
            "coverage note mentions unresolved"
        );

        // Confidence stats are present and non-null (there is at least one edge).
        let conf = &res.content["confidence"];
        assert!(conf["edge_count"].as_u64().unwrap() >= 1, "edge count >= 1");
        assert!(conf["min"].as_f64().is_some(), "min confidence populated");
        assert!(conf["avg"].as_f64().is_some(), "avg confidence populated");
    }

    #[test]
    fn blast_radius_coverage_note_present_when_zero_unresolved() {
        // Even with no unresolved refs, a coverage note must be emitted.
        let store = fixture_store();
        let tool = BlastRadius;
        let res = tool.invoke(&store, &json!({"symbol": "leaf"})).unwrap();

        assert!(
            res.diagnostics.iter().any(|d| d.contains("coverage:")),
            "coverage diagnostic emitted even when unresolved_callers == 0"
        );
        assert_eq!(
            res.content["unresolved_callers"].as_u64().unwrap(),
            0,
            "zero unresolved callers for a well-known fixture"
        );
    }

    // ── Lineage ───────────────────────────────────────────────────────────────

    /// Build a chain with imports too: `root` calls `mid`, `mid` imports `leaf`.
    fn lineage_fixture() -> MemStore {
        use wicked_estate_core::ResolutionTier;
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node("root", "root_fn", NodeKind::Function, "src/root.rs", 1),
                make_node("mid", "mid_fn", NodeKind::Function, "src/mid.rs", 10),
                make_node("leaf", "leaf_fn", NodeKind::Function, "src/leaf.rs", 20),
            ])
            .unwrap();
        // root --calls--> mid --imports--> leaf
        let call_edge = Edge::new(
            SymbolId("root".to_string()),
            SymbolId("mid".to_string()),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test-fixture",
        );
        let import_edge = Edge::new(
            SymbolId("mid".to_string()),
            SymbolId("leaf".to_string()),
            EdgeKind::Imports,
            ResolutionTier::Parsed,
            "test-fixture",
        );
        store.upsert_edges(&[call_edge, import_edge]).unwrap();
        store.commit_batch().unwrap();
        store
    }

    #[test]
    fn lineage_of_root_includes_mid_and_leaf() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool
            .invoke(&store, &json!({"symbol": "root", "depth": 8}))
            .unwrap();

        let deps = res.content["dependencies"].as_array().unwrap();
        let names: Vec<&str> = deps.iter().map(|d| d["name"].as_str().unwrap()).collect();

        assert!(
            names.contains(&"mid_fn"),
            "direct call target must be in lineage"
        );
        assert!(
            names.contains(&"leaf_fn"),
            "transitive import target must be in lineage"
        );
        // Root itself must NOT appear.
        assert!(
            !names.contains(&"root_fn"),
            "start symbol excluded from lineage"
        );
        assert_eq!(res.content["total"].as_u64().unwrap(), 2);
    }

    #[test]
    fn lineage_depth_field_is_accurate() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool.invoke(&store, &json!({"symbol": "root"})).unwrap();

        let deps = res.content["dependencies"].as_array().unwrap();
        let mid = deps
            .iter()
            .find(|d| d["name"].as_str().unwrap() == "mid_fn")
            .unwrap();
        let leaf = deps
            .iter()
            .find(|d| d["name"].as_str().unwrap() == "leaf_fn")
            .unwrap();
        assert_eq!(mid["depth"].as_u64().unwrap(), 1, "mid is at depth 1");
        assert_eq!(leaf["depth"].as_u64().unwrap(), 2, "leaf is at depth 2");
    }

    #[test]
    fn lineage_of_leaf_is_empty() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool.invoke(&store, &json!({"symbol": "leaf"})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("no dependencies")),
            "diagnostic expected for leaf with no dependencies"
        );
    }

    #[test]
    fn lineage_missing_symbol_returns_empty_not_error() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool
            .invoke(&store, &json!({"symbol": "ghost_xyz"}))
            .unwrap();

        // R1: must be Ok, never Err.
        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("no dependencies")),
            "diagnostic must be present for missing symbol"
        );
    }

    #[test]
    fn lineage_missing_field_returns_diagnostic() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap_or(0), 0);
        assert!(
            res.diagnostics.iter().any(|d| d.contains("Lineage")),
            "diagnostic must name the tool"
        );
    }

    #[test]
    fn lineage_staleness_note_always_present() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool.invoke(&store, &json!({"symbol": "root"})).unwrap();
        assert!(
            res.diagnostics.iter().any(|d| d.contains("STALENESS")),
            "R5 staleness note must always be present"
        );
    }

    #[test]
    fn lineage_confidence_fields_populated() {
        let store = lineage_fixture();
        let tool = Lineage;
        let res = tool.invoke(&store, &json!({"symbol": "root"})).unwrap();

        let conf = &res.content["confidence"];
        assert!(
            conf["edge_count"].as_u64().unwrap() >= 1,
            "at least one edge in lineage"
        );
        assert!(conf["min"].as_f64().is_some(), "min confidence populated");
        assert!(conf["avg"].as_f64().is_some(), "avg confidence populated");
    }

    // ── Reciprocal Rank Fusion ───────────────────────────────────────────────

    fn id(s: &str) -> SymbolId {
        SymbolId(s.to_string())
    }

    #[test]
    fn rrf_single_list_preserves_order() {
        let list = vec![id("a"), id("b"), id("c")];
        let ranked = reciprocal_rank_fusion(&[list], 60.0);
        let ids: Vec<&str> = ranked.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(
            ids,
            ["a", "b", "c"],
            "order preserved in single-list fusion"
        );
    }

    #[test]
    fn rrf_item_in_multiple_lists_ranks_first() {
        // "shared" appears rank-1 in list A and rank-2 in list B.
        // "only_a" appears rank-2 in A; "only_b" appears rank-1 in B.
        let list_a = vec![id("shared"), id("only_a")];
        let list_b = vec![id("only_b"), id("shared")];

        let ranked = reciprocal_rank_fusion(&[list_a, list_b], 60.0);
        let ids: Vec<&str> = ranked.iter().map(|(s, _)| s.as_str()).collect();

        // "shared" gets 1/(60+1) + 1/(60+2) > either solo entry.
        assert_eq!(
            ids[0], "shared",
            "'shared' should rank first (appears in both lists)"
        );
    }

    #[test]
    fn rrf_scores_decrease_with_rank() {
        let list = vec![id("first"), id("second"), id("third")];
        let ranked = reciprocal_rank_fusion(&[list], 60.0);

        // Score at rank 1 > rank 2 > rank 3 with k=60.
        assert!(ranked[0].1 > ranked[1].1);
        assert!(ranked[1].1 > ranked[2].1);
    }

    #[test]
    fn rrf_empty_lists_returns_empty() {
        let ranked = reciprocal_rank_fusion(&[], 60.0);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rrf_deduplicates_across_lists() {
        let list_a = vec![id("x"), id("y")];
        let list_b = vec![id("x"), id("z")];
        let ranked = reciprocal_rank_fusion(&[list_a, list_b], 60.0);

        let ids: Vec<&str> = ranked.iter().map(|(s, _)| s.as_str()).collect();
        let count_x = ids.iter().filter(|&&s| s == "x").count();
        assert_eq!(count_x, 1, "x should appear exactly once after dedup");
        assert_eq!(ranked.len(), 3, "total unique symbols = 3");
    }

    #[test]
    fn rrf_k_affects_score_magnitude() {
        let list = vec![id("a")];
        let r60 = reciprocal_rank_fusion(std::slice::from_ref(&list), 60.0);
        let r1 = reciprocal_rank_fusion(std::slice::from_ref(&list), 1.0);

        // With k=1, rank-1 score = 1/(1+1)=0.5; with k=60 it's 1/(60+1)≈0.016.
        assert!(r1[0].1 > r60[0].1, "smaller k yields larger scores");
    }

    // ── ContextPack / render_context ─────────────────────────────────────────

    /// Build a richer fixture: `hub` is called by both `alpha` and `beta`; `leaf` is called
    /// only by `alpha`.  Hub should rank higher than leaf (more in-edges in the PageRank graph).
    fn context_fixture() -> MemStore {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();

        let mut hub = make_node("hub", "hub_fn", NodeKind::Function, "src/hub.rs", 1);
        hub.signature = Some("fn hub_fn(x: i32) -> u32".to_string());
        hub.doc =
            Some("The central hub function.\nExtra doc line that should not appear.".to_string());

        let mut alpha = make_node("alpha", "alpha_fn", NodeKind::Function, "src/alpha.rs", 10);
        alpha.signature = Some("fn alpha_fn()".to_string());

        let mut beta = make_node("beta", "beta_fn", NodeKind::Function, "src/beta.rs", 20);
        beta.signature = Some("fn beta_fn()".to_string());

        let mut leaf = make_node("leaf", "leaf_fn", NodeKind::Function, "src/leaf.rs", 30);
        leaf.signature = Some("fn leaf_fn() -> bool".to_string());
        leaf.doc = Some("A leaf function.".to_string());

        store.upsert_nodes(&[hub, alpha, beta, leaf]).unwrap();
        store
            .upsert_edges(&[
                make_call_edge("alpha", "hub"),
                make_call_edge("beta", "hub"),
                make_call_edge("alpha", "leaf"),
            ])
            .unwrap();
        store.commit_batch().unwrap();
        store
    }

    #[test]
    fn render_context_respects_token_budget() {
        let store = context_fixture();
        // A very small budget — only a few tokens.
        let budget = 20usize;
        let result = render_context(&store, &[SymbolId("hub".into())], budget).unwrap();
        // chars should be roughly budget * 4.
        assert!(
            result.len() <= budget * 4 + 200,
            "output chars ({}) should be within ~budget*4 ({}) chars; summary line allowed overhead",
            result.len(),
            budget * 4
        );
    }

    #[test]
    fn render_context_stubs_contain_signatures_not_bodies() {
        let store = context_fixture();
        let result =
            render_context(&store, &[SymbolId("hub".into())], DEFAULT_TOKEN_BUDGET).unwrap();

        // Signature present.
        assert!(
            result.contains("fn hub_fn(x: i32) -> u32"),
            "hub signature must appear in output"
        );
        // No full body markers — stubs have no opening braces followed by bodies.
        // The stub format is `fn sig`, `  /// doc`, `  // file:line` — no `{` body.
        assert!(
            !result.contains("{\n"),
            "function bodies (opening brace + newline) must not appear in stub output"
        );
        // First doc line appears; second doc line does not.
        assert!(
            result.contains("The central hub function."),
            "first doc line must appear"
        );
        assert!(
            !result.contains("Extra doc line"),
            "second doc line must NOT appear (elided)"
        );
    }

    #[test]
    fn render_context_hub_before_leaf() {
        // hub is called by both alpha and beta; leaf is called only by alpha.
        // When seeded on alpha, hub should appear before leaf in the output (higher rank).
        let store = context_fixture();
        let result =
            render_context(&store, &[SymbolId("alpha".into())], DEFAULT_TOKEN_BUDGET).unwrap();

        let hub_pos = result.find("hub_fn");
        let leaf_pos = result.find("leaf_fn");

        assert!(hub_pos.is_some(), "hub_fn must appear in context");
        assert!(leaf_pos.is_some(), "leaf_fn must appear in context");
        assert!(
            hub_pos.unwrap() < leaf_pos.unwrap(),
            "hub_fn (more-depended-upon) must appear before leaf_fn in the output"
        );
    }

    #[test]
    fn render_context_empty_seeds_returns_empty_no_panic() {
        let store = context_fixture();
        let result = render_context(&store, &[], DEFAULT_TOKEN_BUDGET).unwrap();
        assert!(result.is_empty(), "empty seeds must return empty string");
    }

    #[test]
    fn context_pack_tool_basic_invoke() {
        let store = context_fixture();
        let tool = ContextPack;
        let res = tool
            .invoke(&store, &json!({"seeds": ["hub"], "token_budget": 1000}))
            .unwrap();

        let context = res.content["context"].as_str().unwrap();
        assert!(
            !context.is_empty(),
            "context must be non-empty for valid seeds"
        );
        assert!(
            res.content["included"].as_u64().unwrap() >= 1,
            "at least one symbol included"
        );
        // R5 staleness note must always appear.
        assert!(
            res.diagnostics.iter().any(|d| d.contains("STALENESS")),
            "staleness diagnostic must be present"
        );
    }

    #[test]
    fn context_pack_tool_name_resolution() {
        let store = context_fixture();
        let tool = ContextPack;
        // Resolve by name instead of by id.
        let res = tool
            .invoke(&store, &json!({"name": "hub_fn", "token_budget": 1000}))
            .unwrap();

        let context = res.content["context"].as_str().unwrap();
        assert!(
            context.contains("hub_fn"),
            "hub_fn must appear when resolved by name"
        );
    }

    #[test]
    fn context_pack_tool_empty_seeds_diagnostic_no_panic() {
        let store = context_fixture();
        let tool = ContextPack;
        let res = tool.invoke(&store, &json!({})).unwrap();

        // Must be R1 compliant — no error, just empty content + diagnostic.
        assert_eq!(res.content["included"].as_u64().unwrap_or(0), 0);
        assert!(
            res.diagnostics.iter().any(|d| d.contains("ContextPack")),
            "diagnostic must name the tool"
        );
    }

    #[test]
    fn context_pack_token_budget_capped_at_max() {
        let store = context_fixture();
        let tool = ContextPack;
        // Request a budget far above the cap.
        let res = tool
            .invoke(&store, &json!({"seeds": ["hub"], "token_budget": 999_999}))
            .unwrap();
        // The output chars must stay well within MAX_TOKEN_BUDGET * 4 chars.
        let context = res.content["context"].as_str().unwrap();
        assert!(
            context.len() <= MAX_TOKEN_BUDGET * 4 + 500,
            "output must not exceed cap * 4 chars (+ small overhead for summary line), got {}",
            context.len()
        );
    }

    // ── Agent-behavior rules (R1 / R4 / R7) ─────────────────────────────────

    #[test]
    fn r1_no_error_on_unknown_symbol_all_tools() {
        let store = fixture_store();
        let req = json!({"symbol": "ghost_xyz"});

        // All tools must return Ok, never Err, for missing symbols.
        assert!(RetrieveEntity.invoke(&store, &req).is_ok());
        assert!(TraverseGraph.invoke(&store, &req).is_ok());
        assert!(BlastRadius.invoke(&store, &req).is_ok());
    }

    #[test]
    fn r7_low_confidence_edge_flagged_in_diagnostics() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node("alpha", "alpha_fn", NodeKind::Function, "src/x.rs", 1),
                make_node("beta", "beta_fn", NodeKind::Function, "src/y.rs", 5),
            ])
            .unwrap();

        // Manually create a low-confidence edge.
        let mut low_edge = make_call_edge("alpha", "beta");
        low_edge.confidence = Confidence::new(0.3); // below 0.5 threshold
        store.upsert_edges(&[low_edge]).unwrap();
        store.commit_batch().unwrap();

        let tool = TraverseGraph;
        let res = tool
            .invoke(
                &store,
                &json!({"symbol": "alpha", "direction": "dependencies", "depth": 3}),
            )
            .unwrap();

        assert!(
            res.diagnostics.iter().any(|d| d.contains("R7-CONFIDENCE")),
            "low-confidence edge must be flagged"
        );
    }

    // ── FetchContent ──────────────────────────────────────────────────────────

    /// Build a store with a node whose span covers a known text slice.
    fn content_fixture() -> MemStore {
        use wicked_estate_core::{GraphWrite, Span};

        let src = "fn greet() { \"hello\" }";
        // "greet" is at bytes 3..8 in the source above.
        let node_span = Span {
            start_byte: 3,
            end_byte: 8,
            start_line: 0,
            start_col: 3,
            end_line: 0,
            end_col: 8,
        };
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        let mut n = make_node("greet_sym", "greet", NodeKind::Function, "src/greet.rs", 0);
        n.location.span = node_span;
        store.upsert_nodes(&[n]).unwrap();
        store.commit_batch().unwrap();
        store.set_file_content("src/greet.rs", src).unwrap();
        store
    }

    #[test]
    fn fetch_content_returns_source_slice() {
        let store = content_fixture();
        let tool = FetchContent;
        let res = tool
            .invoke(&store, &json!({"symbol": "greet_sym"}))
            .unwrap();

        assert!(
            res.content["found"].as_bool().unwrap(),
            "found must be true"
        );
        assert_eq!(
            res.content["source"].as_str().unwrap(),
            "greet",
            "source slice must match"
        );
        assert_eq!(res.content["file"].as_str().unwrap(), "src/greet.rs");
    }

    #[test]
    fn fetch_content_missing_symbol_returns_found_false_not_error() {
        let store = content_fixture();
        let tool = FetchContent;
        let res = tool
            .invoke(&store, &json!({"symbol": "nonexistent"}))
            .unwrap();

        assert!(
            !res.content["found"].as_bool().unwrap(),
            "found must be false"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("not found")),
            "diagnostic must note symbol not found"
        );
    }

    #[test]
    fn fetch_content_missing_field_returns_diagnostic() {
        let store = content_fixture();
        let tool = FetchContent;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert!(
            !res.content["found"].as_bool().unwrap(),
            "found must be false"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("FetchContent")),
            "diagnostic must name the tool"
        );
    }

    #[test]
    fn fetch_content_zero_span_returns_found_true_source_null_with_diagnostic() {
        // A node with Span::ZERO — symbol exists but content cannot be extracted.
        let store = fixture_store(); // nodes have Span::ZERO
        let tool = FetchContent;
        let res = tool.invoke(&store, &json!({"symbol": "leaf"})).unwrap();

        assert!(
            res.content["found"].as_bool().unwrap(),
            "found must be true (node exists)"
        );
        // source is null (Span::ZERO → None → JSON null)
        assert!(
            res.content["source"].is_null(),
            "source must be null for zero-span node"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("FetchContent")),
            "diagnostic must explain missing source"
        );
    }

    #[test]
    fn fetch_content_r1_compliance_no_error_on_missing() {
        let store = fixture_store();
        let res = FetchContent.invoke(&store, &json!({"symbol": "ghost_xyz"}));
        assert!(
            res.is_ok(),
            "FetchContent must return Ok, never Err, for missing symbols"
        );
    }

    #[test]
    fn fetch_content_staleness_note_always_present() {
        let store = content_fixture();
        let tool = FetchContent;
        let res = tool
            .invoke(&store, &json!({"symbol": "greet_sym"}))
            .unwrap();
        assert!(
            res.diagnostics.iter().any(|d| d.contains("STALENESS")),
            "R5 staleness note must always be present"
        );
    }

    // ── Enriched payloads: additive line fields ──────────────────────────────

    #[test]
    fn retrieve_entity_line_1based_is_zero_based_plus_one() {
        let store = fixture_store();
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "middle"}))
            .unwrap();
        let line = res.content["line"].as_u64().unwrap();
        let line_1b = res.content["line_1based"].as_u64().unwrap();
        assert_eq!(line_1b, line + 1, "line_1based must equal line + 1");
        // end_line fields are additive and present.
        assert!(res.content.get("end_line").is_some(), "end_line present");
        assert_eq!(
            res.content["end_line_1based"].as_u64().unwrap(),
            res.content["end_line"].as_u64().unwrap() + 1,
            "end_line_1based == end_line + 1"
        );
    }

    #[test]
    fn search_entity_line_1based_present_and_correct() {
        let store = fixture_store();
        let res = SearchEntity
            .invoke(&store, &json!({"name": "middle_fn"}))
            .unwrap();
        let m = &res.content["matches"][0];
        assert_eq!(
            m["line_1based"].as_u64().unwrap(),
            m["line"].as_u64().unwrap() + 1,
            "SearchEntity match line_1based == line + 1"
        );
        assert!(m.get("end_line").is_some(), "end_line present on match");
    }

    // ── Enriched payloads: opt-in bounded source ──────────────────────────────

    #[test]
    fn retrieve_entity_source_omitted_by_default() {
        let store = content_fixture();
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "greet_sym"}))
            .unwrap();
        assert!(res.content["found"].as_bool().unwrap());
        assert!(
            res.content.get("source").is_none(),
            "source must be ABSENT when include_source is not set (default off)"
        );
        // blob_sha is independent of include_source and present for a stored file.
        assert!(
            res.content.get("blob_sha").is_some(),
            "blob_sha present (file content was stored → git blob sha computed)"
        );
    }

    #[test]
    fn retrieve_entity_source_present_when_requested() {
        let store = content_fixture();
        let res = RetrieveEntity
            .invoke(
                &store,
                &json!({"symbol": "greet_sym", "include_source": true}),
            )
            .unwrap();
        assert_eq!(
            res.content["source"].as_str().unwrap(),
            "greet",
            "exact byte slice [3..8] of the stored source"
        );
        let range = res.content["byte_range"].as_array().unwrap();
        assert_eq!(range[0].as_u64().unwrap(), 3);
        assert_eq!(range[1].as_u64().unwrap(), 8);
        // Not truncated (slice is shorter than the default cap) → flag absent.
        assert!(
            res.content.get("source_truncated").is_none(),
            "source_truncated absent when not truncated"
        );
    }

    #[test]
    fn retrieve_entity_source_bounded_and_truncation_marked() {
        let store = content_fixture();
        // max_source_chars=2 forces truncation of the 5-char "greet" slice.
        let res = RetrieveEntity
            .invoke(
                &store,
                &json!({"symbol": "greet_sym", "include_source": true, "max_source_chars": 2}),
            )
            .unwrap();
        let src = res.content["source"].as_str().unwrap();
        assert_eq!(src.chars().count(), 2, "slice bounded to max_source_chars");
        assert_eq!(src, "gr", "first 2 chars of 'greet'");
        assert!(
            res.content["source_truncated"].as_bool().unwrap(),
            "source_truncated must be true when the slice was cut"
        );
        // byte_range always reflects the FULL span, even when the emitted slice is truncated.
        let range = res.content["byte_range"].as_array().unwrap();
        assert_eq!(range[0].as_u64().unwrap(), 3);
        assert_eq!(range[1].as_u64().unwrap(), 8);
    }

    #[test]
    fn retrieve_entity_source_requested_but_unavailable_emits_null_and_byte_range() {
        // fixture_store nodes have Span::ZERO and no stored content → no slice.
        let store = fixture_store();
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "leaf", "include_source": true}))
            .unwrap();
        assert!(res.content["found"].as_bool().unwrap());
        assert!(
            res.content["source"].is_null(),
            "source is null when unavailable"
        );
        assert!(
            res.content.get("byte_range").is_some(),
            "byte_range still provided so the caller can locate bytes"
        );
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("source unavailable")),
            "honest diagnostic when source requested but missing"
        );
    }

    // ── RetrieveEntity: typed annotations (Chunk 3) ───────────────────────────
    // `Annotation` + `GraphWrite` are already in scope via `use super::*` + the test-module imports.

    /// A store with one symbol carrying the supplied annotations,
    /// each stamped with the given `ts` so the cap-ordering test is deterministic.
    fn annotated_store(extra: Vec<Annotation>) -> MemStore {
        let mut store = fixture_store();
        let id = SymbolId("leaf".to_string());
        for a in extra {
            store.annotate(&id, a).unwrap();
        }
        store
    }

    #[test]
    fn retrieve_entity_omits_annotation_fields_when_none() {
        // A plain fixture symbol has no annotations → both fields absent (additive).
        let store = fixture_store();
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "leaf"}))
            .unwrap();
        assert!(res.content["found"].as_bool().unwrap());
        assert!(
            res.content.get("annotations").is_none(),
            "annotations must be ABSENT when the symbol has none"
        );
        assert!(
            res.content.get("annotation_summary").is_none(),
            "annotation_summary must be ABSENT when the symbol has none"
        );
    }

    #[test]
    fn retrieve_entity_emits_annotations_and_summary_shape() {
        let store = annotated_store(vec![
            Annotation::new("note", "owner", "team-graph"),
            Annotation::new("note", "perf", "hot path"),
            Annotation::new("assumption", "thread-safety", "assumed Send+Sync")
                .with_confidence(0.7)
                .with_provenance("manual")
                .with_author("alice"),
        ]);
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "leaf"}))
            .unwrap();

        let anns = res.content["annotations"].as_array().unwrap();
        assert_eq!(anns.len(), 3, "all 3 annotations inlined (under the cap)");

        // Every item carries the full fixed shape, including the computed `advisory` field.
        for item in anns {
            for f in [
                "type",
                "key",
                "value",
                "confidence",
                "provenance",
                "author",
                "ts",
                "advisory",
            ] {
                assert!(item.get(f).is_some(), "annotation item missing field '{f}'");
            }
        }

        // The assumption item must carry advisory:true and its fields round-trip.
        let assumption = anns
            .iter()
            .find(|a| a["type"] == "assumption")
            .expect("assumption present");
        assert!(
            assumption["advisory"].as_bool().unwrap(),
            "assumption → advisory:true (computed from type, not the type string)"
        );
        assert_eq!(assumption["key"].as_str().unwrap(), "thread-safety");
        assert_eq!(assumption["confidence"].as_f64().unwrap(), 0.7);
        assert_eq!(assumption["author"].as_str().unwrap(), "alice");

        // A note item is NOT advisory.
        let note = anns
            .iter()
            .find(|a| a["type"] == "note")
            .expect("note present");
        assert!(
            !note["advisory"].as_bool().unwrap(),
            "note → advisory:false"
        );

        // Summary: exact totals + by_type + has_advisory.
        let summary = &res.content["annotation_summary"];
        assert_eq!(summary["count"].as_u64().unwrap(), 3, "true total count");
        assert_eq!(summary["by_type"]["note"].as_u64().unwrap(), 2);
        assert_eq!(summary["by_type"]["assumption"].as_u64().unwrap(), 1);
        assert!(
            summary["has_advisory"].as_bool().unwrap(),
            "has_advisory true when an assumption is present"
        );
    }

    #[test]
    fn retrieve_entity_summary_has_advisory_false_without_advisory_types() {
        let store = annotated_store(vec![
            Annotation::new("note", "k1", "v1"),
            Annotation::new("observation", "k2", "v2"),
            Annotation::new("custom-thing", "k3", "v3"),
        ]);
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "leaf"}))
            .unwrap();
        let summary = &res.content["annotation_summary"];
        assert_eq!(summary["count"].as_u64().unwrap(), 3);
        // Custom type is stored/counted identically; never advisory.
        assert_eq!(summary["by_type"]["custom-thing"].as_u64().unwrap(), 1);
        assert!(
            !summary["has_advisory"].as_bool().unwrap(),
            "no advisory types → has_advisory:false"
        );
        // Each non-advisory item carries advisory:false.
        for item in res.content["annotations"].as_array().unwrap() {
            assert!(!item["advisory"].as_bool().unwrap());
        }
    }

    #[test]
    fn retrieve_entity_caps_at_20_advisory_first_then_ts_desc_with_true_total() {
        // 19 notes (ts 100..118) + 3 advisory (assumption ts 5, question ts 50, assumption ts 200)
        // = 22 total, over the 20 cap. Expect: 3 advisory kept first (regardless of their low ts),
        // then the 17 newest notes; summary.count shows the TRUE 22.
        let mut extra: Vec<Annotation> = Vec::new();
        for i in 0..19u32 {
            extra.push(Annotation {
                ts: 100 + i as i64,
                ..Annotation::new("note", format!("n{i}"), "v")
            });
        }
        extra.push(Annotation {
            ts: 5,
            ..Annotation::new("assumption", "old-assumption", "v")
        });
        extra.push(Annotation {
            ts: 50,
            ..Annotation::new("question", "mid-question", "v")
        });
        extra.push(Annotation {
            ts: 200,
            ..Annotation::new("assumption", "new-assumption", "v")
        });

        let store = annotated_store(extra);
        let res = RetrieveEntity
            .invoke(&store, &json!({"symbol": "leaf"}))
            .unwrap();

        let anns = res.content["annotations"].as_array().unwrap();
        assert_eq!(anns.len(), 20, "inline list capped at 20");

        // Summary reflects the TRUE total of 22, not the capped 20 (consumer sees it was capped).
        let summary = &res.content["annotation_summary"];
        assert_eq!(
            summary["count"].as_u64().unwrap(),
            22,
            "summary.count is the TRUE total, not the capped length"
        );
        assert_eq!(summary["by_type"]["note"].as_u64().unwrap(), 19);
        assert_eq!(summary["by_type"]["assumption"].as_u64().unwrap(), 2);
        assert_eq!(summary["by_type"]["question"].as_u64().unwrap(), 1);
        assert!(summary["has_advisory"].as_bool().unwrap());

        // The 3 advisory items must ALL survive the cap — kept first regardless of their ts being
        // among the lowest (5/50/200 vs notes at 100..118). Then exactly 17 notes fill the rest.
        let advisory_count = anns
            .iter()
            .filter(|a| a["advisory"].as_bool().unwrap())
            .count();
        assert_eq!(
            advisory_count, 3,
            "all 3 advisory-class items kept first under the cap"
        );
        let note_count = anns.iter().filter(|a| a["type"] == "note").count();
        assert_eq!(note_count, 17, "remaining 17 slots filled by notes");

        // The first 3 items are the advisory ones, ordered by ts desc within the advisory group:
        // new-assumption(200), question(50), old-assumption(5).
        assert_eq!(anns[0]["key"].as_str().unwrap(), "new-assumption");
        assert_eq!(anns[1]["key"].as_str().unwrap(), "mid-question");
        assert_eq!(anns[2]["key"].as_str().unwrap(), "old-assumption");

        // The kept notes are the NEWEST 17 (ts 102..118): the two oldest (ts 100 "n0", 101 "n1")
        // are dropped. Verify n0/n1 are absent and the newest n18 (ts 118) is present.
        let keys: HashSet<&str> = anns.iter().filter_map(|a| a["key"].as_str()).collect();
        assert!(!keys.contains("n0"), "oldest note (ts 100) dropped by cap");
        assert!(
            !keys.contains("n1"),
            "2nd-oldest note (ts 101) dropped by cap"
        );
        assert!(keys.contains("n18"), "newest note (ts 118) kept");
        assert!(keys.contains("n2"), "note ts 102 kept (boundary)");
    }

    #[test]
    fn retrieve_entity_annotations_r1_missing_symbol_unchanged() {
        // Missing-symbol path is untouched: found:false, no annotation fields, no error (R1).
        let store = annotated_store(vec![Annotation::new("assumption", "k", "v")]);
        let res = RetrieveEntity.invoke(&store, &json!({"symbol": "ghost_xyz"}));
        assert!(res.is_ok(), "missing symbol must never error (R1)");
        let res = res.unwrap();
        assert!(!res.content["found"].as_bool().unwrap(), "found:false");
        assert!(
            res.content.get("annotations").is_none(),
            "no annotations on the missing-symbol path"
        );
        assert!(res.content.get("annotation_summary").is_none());
    }

    #[test]
    fn search_entity_source_omitted_by_default_present_when_requested() {
        let store = content_fixture();
        // Default: no source.
        let res = SearchEntity
            .invoke(&store, &json!({"name": "greet"}))
            .unwrap();
        let m = &res.content["matches"][0];
        assert!(
            m.get("source").is_none(),
            "SearchEntity source absent by default"
        );
        // Requested: source present + bounded.
        let res2 = SearchEntity
            .invoke(&store, &json!({"name": "greet", "include_source": true}))
            .unwrap();
        let m2 = &res2.content["matches"][0];
        assert_eq!(m2["source"].as_str().unwrap(), "greet");
    }

    #[test]
    fn search_entity_total_source_budget_caps_inlined_source() {
        // The across-matches total budget (max_total_source_chars) is the R4 headroom control:
        // a tiny value truncates source even though the per-slice cap would allow more.
        let store = content_fixture();
        let res = SearchEntity
            .invoke(
                &store,
                &json!({"name": "greet", "include_source": true, "max_total_source_chars": 3}),
            )
            .unwrap();
        let m = &res.content["matches"][0];
        let src = m["source"].as_str().unwrap();
        assert!(
            src.chars().count() <= 3,
            "total budget must cap source, got {src:?}"
        );
        assert!(
            m["source_truncated"].as_bool().unwrap_or(false),
            "truncation must be flagged when the total budget bites"
        );
    }

    #[test]
    fn search_entity_source_unbounded_by_default() {
        // The deliberate contract: include_source with NO budget returns the full body, not
        // truncated. The caller owns its context window; the engine imposes no default cap.
        let store = content_fixture();
        let res = SearchEntity
            .invoke(&store, &json!({"name": "greet", "include_source": true}))
            .unwrap();
        let m = &res.content["matches"][0];
        assert_eq!(m["source"].as_str().unwrap(), "greet");
        assert!(
            !m["source_truncated"].as_bool().unwrap_or(false),
            "no budget set → source must not be truncated"
        );
    }

    // ── Enriched payloads: denormalized edge endpoints (TraverseGraph) ─────────

    #[test]
    fn traverse_graph_edges_carry_denormalized_endpoints_and_provenance() {
        let store = fixture_store();
        let res = TraverseGraph
            .invoke(
                &store,
                &json!({"symbol": "caller", "direction": "dependencies", "depth": 4}),
            )
            .unwrap();

        let edges = res.content["edges"].as_array().unwrap();
        assert!(!edges.is_empty(), "expected at least one edge");
        // Find the caller→middle edge.
        let e = edges
            .iter()
            .find(|e| e["source"]["symbol"].as_str() == Some("caller"))
            .expect("caller edge present");

        // Endpoints are denormalized objects, not bare strings.
        assert_eq!(e["source"]["name"].as_str().unwrap(), "caller_fn");
        assert_eq!(e["target"]["symbol"].as_str().unwrap(), "middle");
        assert_eq!(e["target"]["name"].as_str().unwrap(), "middle_fn");
        assert_eq!(e["target"]["kind"].as_str().unwrap(), "function");
        assert_eq!(e["target"]["file"].as_str().unwrap(), "src/b.rs");
        // Endpoints carry both 0-based `line` and `line_1based` (= line + 1).
        let line_0 = e["target"]["line"].as_u64().unwrap();
        let line_1b = e["target"]["line_1based"].as_u64().unwrap();
        assert_eq!(line_0, 10, "endpoint 0-based line == stored start_line");
        assert_eq!(line_1b, 11, "endpoint line_1based == start_line + 1");
        assert_eq!(line_1b, line_0 + 1, "line_1based == line + 1");

        // R7: edge carries confidence + provenance + resolved_by inline.
        assert!(e["confidence"].as_f64().is_some(), "confidence present");
        assert_eq!(
            e["provenance"].as_str().unwrap(),
            "parsed",
            "provenance is the snake_case tier tag"
        );
        assert_eq!(
            e["resolved_by"].as_str().unwrap(),
            "test-fixture",
            "resolved_by present"
        );

        // Nodes also gained line_1based.
        let nodes = res.content["nodes"].as_array().unwrap();
        let mid = nodes
            .iter()
            .find(|n| n["symbol"].as_str() == Some("middle"))
            .unwrap();
        assert_eq!(mid["line_1based"].as_u64().unwrap(), 11);
    }

    // ── Enriched payloads: BlastRadius summary ────────────────────────────────

    #[test]
    fn blast_radius_summary_present_with_correct_counts() {
        let store = fixture_store();
        let res = BlastRadius
            .invoke(&store, &json!({"symbol": "leaf", "depth": 8}))
            .unwrap();

        let summary = &res.content["summary"];
        assert!(summary.is_object(), "summary object present");
        // leaf's dependents = {middle, caller} → total 2, both functions.
        assert_eq!(summary["total"].as_u64().unwrap(), 2);
        assert_eq!(
            summary["by_kind"]["function"].as_u64().unwrap(),
            2,
            "both dependents are functions"
        );

        // top_files: middle in src/b.rs, caller in src/a.rs → two files, count 1 each.
        let top_files = summary["top_files"].as_array().unwrap();
        assert_eq!(top_files.len(), 2, "two distinct dependent files");
        let total_in_files: u64 = top_files.iter().map(|f| f["count"].as_u64().unwrap()).sum();
        assert_eq!(total_in_files, 2, "file counts sum to dependent total");

        // top_by_pagerank present (ranking succeeds on this fixture) and bounded.
        let pr = summary["top_by_pagerank"].as_array().unwrap();
        assert!(pr.len() <= 2, "pagerank list bounded to dependent count");
        // Each entry names a real dependent.
        for entry in pr {
            let n = entry["name"].as_str().unwrap();
            assert!(
                n == "middle_fn" || n == "caller_fn",
                "pagerank entry must be a dependent, got {n}"
            );
        }
    }

    #[test]
    fn blast_radius_dependents_carry_line_1based() {
        let store = fixture_store();
        let res = BlastRadius
            .invoke(&store, &json!({"symbol": "leaf"}))
            .unwrap();
        let deps = res.content["dependents"].as_array().unwrap();
        let middle = deps
            .iter()
            .find(|d| d["name"].as_str().unwrap() == "middle_fn")
            .unwrap();
        assert_eq!(
            middle["line_1based"].as_u64().unwrap(),
            middle["line"].as_u64().unwrap() + 1,
            "dependent line_1based == line + 1"
        );
    }

    #[test]
    fn blast_radius_summary_empty_when_no_dependents() {
        let store = fixture_store();
        let res = BlastRadius
            .invoke(&store, &json!({"symbol": "caller"}))
            .unwrap();
        let summary = &res.content["summary"];
        assert_eq!(summary["total"].as_u64().unwrap(), 0, "no dependents");
        assert!(
            summary["top_files"].as_array().unwrap().is_empty(),
            "no files when no dependents"
        );
    }

    // ── HashEmbedder ─────────────────────────────────────────────────────────

    #[test]
    fn hash_embedder_output_has_correct_dim() {
        let emb = HashEmbedder::new(64);
        let v = emb.embed("hello world");
        assert_eq!(v.len(), 64);
        assert_eq!(emb.dim(), 64);
    }

    #[test]
    fn hash_embedder_is_deterministic() {
        let emb = HashEmbedder::new(64);
        let v1 = emb.embed("fn nearest cosine similarity");
        let v2 = emb.embed("fn nearest cosine similarity");
        assert_eq!(v1, v2, "HashEmbedder must be deterministic");
    }

    #[test]
    fn hash_embedder_is_l2_normalised() {
        let emb = HashEmbedder::new(32);
        let v = emb.embed("some symbol name");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        // Allow floating-point rounding; norm should be ~1.0.
        assert!(
            (norm - 1.0).abs() < 1e-5 || norm == 0.0,
            "HashEmbedder output must be L2-normalised (norm={norm})"
        );
    }

    #[test]
    fn hash_embedder_empty_text_returns_zero_vector() {
        let emb = HashEmbedder::new(16);
        let v = emb.embed("");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert_eq!(norm, 0.0, "empty text → zero vector");
    }

    #[test]
    fn hash_embedder_different_text_gives_different_output() {
        let emb = HashEmbedder::new(64);
        let v1 = emb.embed("foo");
        let v2 = emb.embed("bar");
        // Not guaranteed, but astronomically unlikely to collide for short distinct tokens.
        assert_ne!(v1, v2, "distinct tokens should embed to distinct vectors");
    }

    #[test]
    fn hash_embedder_is_not_semantic_and_trait_defaults_to_semantic() {
        // The lexical fallback self-identifies so recall pipelines can gate their vector list.
        assert!(!HashEmbedder::new(64).is_semantic());

        // A model-backed embedder that doesn't override gets the `true` default.
        struct ModelLike;
        impl Embedder for ModelLike {
            fn id(&self) -> &str {
                "model:test"
            }
            fn embed(&self, _text: &str) -> Vec<f32> {
                vec![1.0]
            }
            fn dim(&self) -> usize {
                1
            }
        }
        assert!(ModelLike.is_semantic(), "trait default must be semantic");

        // The Box forwarding impl must preserve the override, not the default.
        let boxed: Box<dyn Embedder> = Box::new(HashEmbedder::new(8));
        assert!(
            !boxed.is_semantic(),
            "Box<dyn Embedder> must forward is_semantic"
        );
    }

    /// The defining property of a REAL semantic embedder vs the lexical HashEmbedder: text that is
    /// semantically related but shares NO tokens must embed closer than unrelated text. Runs only
    /// under `--features fastembed` (downloads the model on first use). HashEmbedder fails this by
    /// construction (zero shared tokens → near-orthogonal), which is exactly why it isn't semantic.
    #[cfg(feature = "fastembed")]
    #[test]
    fn fastembed_is_semantic_not_lexical() {
        let e = FastEmbedder::new().expect("fastembed model load");
        assert_eq!(e.dim(), 384, "bge-small-en-v1.5 is 384-dim");
        let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        // "cat/mat" vs "kitten/rug" share no content words but mean nearly the same thing.
        let anchor = e.embed("the cat sat on the mat");
        let related = e.embed("a kitten rested on the rug");
        let unrelated = e.embed("quarterly revenue exceeded the analyst forecast");
        let sim_related = cos(&anchor, &related);
        let sim_unrelated = cos(&anchor, &unrelated);
        assert!(
            sim_related > sim_unrelated,
            "semantically related text must be nearer than unrelated: related={sim_related}, unrelated={sim_unrelated}"
        );
    }

    /// model2vec must also be semantic, not lexical: zero-shared-token related text embeds closer
    /// than unrelated text. Runs only under `--features model2vec` (downloads the model on first
    /// use). The lexical HashEmbedder fails this; model2vec passing proves real (static) semantics.
    #[cfg(feature = "model2vec")]
    #[test]
    fn model2vec_is_semantic_not_lexical() {
        let e = Model2VecEmbedder::new().expect("model2vec model load");
        assert!(e.dim() > 0, "model must report a positive dimension");
        let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        let anchor = e.embed("the cat sat on the mat");
        let related = e.embed("a kitten rested on the rug");
        let unrelated = e.embed("quarterly revenue exceeded the analyst forecast");
        let sim_related = cos(&anchor, &related);
        let sim_unrelated = cos(&anchor, &unrelated);
        assert!(
            sim_related > sim_unrelated,
            "model2vec: related text must be nearer than unrelated: related={sim_related}, unrelated={sim_unrelated}"
        );
    }

    // ── semantic_search free function ────────────────────────────────────────

    fn embed_fixture() -> MemStore {
        // Three symbols; each embedded as a unit vector on a different axis.
        // With dim=4, axis 0 → "alpha", axis 1 → "beta", axis 2 → "gamma".
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node("alpha", "alpha_fn", NodeKind::Function, "src/a.rs", 1),
                make_node("beta", "beta_fn", NodeKind::Function, "src/b.rs", 2),
                make_node("gamma", "gamma_fn", NodeKind::Function, "src/c.rs", 3),
            ])
            .unwrap();
        store.commit_batch().unwrap();
        // Axis-aligned unit vectors: each symbol lives in its own dimension.
        let mut axis = |sym: &str, idx: usize| {
            let mut v = vec![0.0f32; 4];
            v[idx] = 1.0;
            store.set_embedding(&SymbolId(sym.to_string()), &v).unwrap();
        };
        axis("alpha", 0);
        axis("beta", 1);
        axis("gamma", 2);
        store
    }

    #[test]
    fn semantic_search_returns_nearest_first() {
        let store = embed_fixture();
        // Query close to axis 0 → should return "alpha" first.
        // We bypass HashEmbedder's output and directly call semantic_search with a known vec.
        let query_vec = vec![0.99_f32, 0.01, 0.0, 0.0];
        let results = store.nearest(&query_vec, 3).unwrap();
        assert_eq!(results[0].0, SymbolId("alpha".to_string()));
        assert!(results[0].1 > results[1].1, "similarity must decrease");

        // Also test the free function.
        struct FixedEmbedder(Vec<f32>);
        impl Embedder for FixedEmbedder {
            fn id(&self) -> &str {
                "test:fixed"
            }
            fn embed(&self, _: &str) -> Vec<f32> {
                self.0.clone()
            }
            fn dim(&self) -> usize {
                self.0.len()
            }
        }
        let fe = FixedEmbedder(query_vec);
        let hits = semantic_search(&store, &store, &fe, "anything", 3).unwrap();
        assert_eq!(hits[0].0, SymbolId("alpha".to_string()));
    }

    #[test]
    fn semantic_search_filters_out_stale_embeddings() {
        // "orphan" has an embedding but no graph node.
        let mut store = MemStore::new();
        store
            .set_embedding(&SymbolId("orphan".to_string()), &[1.0_f32, 0.0])
            .unwrap();
        // No node upsert for "orphan".

        struct FixedEmbedder;
        impl Embedder for FixedEmbedder {
            fn id(&self) -> &str {
                "test:fixed"
            }
            fn embed(&self, _: &str) -> Vec<f32> {
                vec![1.0, 0.0]
            }
            fn dim(&self) -> usize {
                2
            }
        }
        let hits = semantic_search(&store, &store, &FixedEmbedder, "query", 10).unwrap();
        assert!(
            hits.iter().all(|(id, _)| id.0 != "orphan"),
            "stale embeddings (no matching node) must be filtered out"
        );
    }

    // ── hybrid_search ────────────────────────────────────────────────────────
    // (Note: `id` is already defined earlier in this test module; reuse it.)

    #[test]
    fn hybrid_search_promotes_symbol_in_all_three_lists() {
        // "shared" is rank-1 in all three lists — should win.
        let name_list = vec![id("shared"), id("name_only")];
        let graph_list = vec![id("graph_only"), id("shared")];
        let sem_list = vec![id("sem_only"), id("sem_only2"), id("shared")];

        let fused = hybrid_search(name_list, graph_list, sem_list, 60.0);
        assert_eq!(fused[0].0, id("shared"), "'shared' must be first after RRF");
    }

    #[test]
    fn hybrid_search_empty_lists_returns_empty() {
        let fused = hybrid_search(vec![], vec![], vec![], 60.0);
        assert!(fused.is_empty());
    }

    #[test]
    fn hybrid_search_deduplicates() {
        let name_list = vec![id("x"), id("y")];
        let graph_list = vec![id("x"), id("z")];
        let sem_list = vec![id("x")];
        let fused = hybrid_search(name_list, graph_list, sem_list, 60.0);
        let count_x = fused.iter().filter(|(s, _)| s.0 == "x").count();
        assert_eq!(count_x, 1, "'x' must appear exactly once");
    }

    // ── SemanticSearch RetrievalTool ─────────────────────────────────────────

    #[test]
    fn semantic_search_tool_returns_matches() {
        let store = embed_fixture();
        // Build a SemanticSearch with a MemStore clone for the VectorStore side.
        let vs_store = embed_fixture(); // separate instance (same data)
        let tool = SemanticSearch::with_hash_embedder(vs_store);

        // The HashEmbedder with dim=4 will hash "alpha_fn" tokens into the 4-bucket vector.
        // We don't care about *which* symbol ranks first here — just that the tool wires correctly.
        let res = tool
            .invoke(&store, &json!({"query": "alpha_fn", "k": 3}))
            .unwrap();

        assert!(res.content["total"].as_u64().unwrap() <= 3, "k=3 respected");
        // R1: always Ok, never error.
        // R5: staleness note present.
        assert!(
            res.diagnostics.iter().any(|d| d.contains("STALENESS")),
            "R5 staleness note"
        );
    }

    #[test]
    fn semantic_search_tool_missing_query_returns_diagnostic() {
        let store = embed_fixture();
        let vs_store = embed_fixture();
        let tool = SemanticSearch::with_hash_embedder(vs_store);
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap_or(0), 0);
        assert!(
            res.diagnostics.iter().any(|d| d.contains("SemanticSearch")),
            "diagnostic must name the tool"
        );
    }

    #[test]
    fn semantic_search_tool_empty_store_returns_empty_not_error() {
        let store = MemStore::new();
        let vs_store = MemStore::new();
        let tool = SemanticSearch::with_hash_embedder(vs_store);
        let res = tool
            .invoke(&store, &json!({"query": "anything", "k": 5}))
            .unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap_or(0), 0);
        // R1: must be Ok, never Err.
    }

    // ── budget_context ───────────────────────────────────────────────────────

    #[test]
    fn test_budget_context_empty_budget() {
        let store = fixture_store();
        let result = budget_context(&store, "middle_fn", 0).unwrap();
        assert!(result.is_empty(), "max_chars=0 must return empty vec");
    }

    #[test]
    fn test_budget_context_no_symbol() {
        let store = fixture_store();
        let result = budget_context(&store, "does_not_exist_xyz_abc", 4096).unwrap();
        assert!(
            result.is_empty(),
            "unknown symbol name must return empty vec"
        );
    }

    #[test]
    fn test_budget_context_within_budget() {
        let store = fixture_store();
        // "middle_fn" has caller (caller_fn) and callee (leaf_fn) — both should appear.
        let result = budget_context(&store, "middle_fn", 4096).unwrap();
        let names: Vec<&str> = result.iter().map(|n| n.name.as_str()).collect();
        assert!(
            names.contains(&"caller_fn") || names.contains(&"leaf_fn"),
            "at least one neighbor of middle_fn must appear in context; got {names:?}"
        );
        assert!(
            !names.contains(&"middle_fn"),
            "seed node must not appear in its own context"
        );
    }

    // ── RulesInventory ───────────────────────────────────────────────────────

    fn make_invoked_by_edge(src: &str, tgt: &str) -> Edge {
        Edge::new(
            SymbolId(src.to_string()),
            SymbolId(tgt.to_string()),
            EdgeKind::InvokedBy,
            ResolutionTier::Parsed,
            "test-fixture",
        )
    }

    #[test]
    fn rules_inventory_finds_ruleset_and_invoking_code() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node(
                    "rs::engine",
                    "PricingRules",
                    NodeKind::RuleSet,
                    "rules/pricing.drl",
                    1,
                ),
                make_node(
                    "app::run_pricing",
                    "run_pricing",
                    NodeKind::Function,
                    "src/pricing_service.rs",
                    42,
                ),
            ])
            .unwrap();
        store
            .upsert_edges(&[make_invoked_by_edge("app::run_pricing", "rs::engine")])
            .unwrap();
        store.commit_batch().unwrap();

        let tool = RulesInventory;
        let res = tool.invoke(&store, &json!({})).unwrap();

        // One engine returned.
        let engines = res.content["engines"].as_array().unwrap();
        assert_eq!(engines.len(), 1, "exactly one RuleSet expected");
        let engine = &engines[0];
        assert_eq!(engine["name"].as_str().unwrap(), "PricingRules");
        assert_eq!(engine["kind"].as_str().unwrap(), "rule_set");
        assert_eq!(engine["file"].as_str().unwrap(), "rules/pricing.drl");

        // The invoking function appears in `invoked_by`.
        let invoked_by = engine["invoked_by"].as_array().unwrap();
        assert!(
            invoked_by
                .iter()
                .any(|v| v.as_str() == Some("app::run_pricing")),
            "invoking symbol must appear in invoked_by; got {invoked_by:?}"
        );
    }

    #[test]
    fn rules_inventory_empty_store_returns_empty_not_error() {
        let store = MemStore::new();
        let tool = RulesInventory;
        let res = tool.invoke(&store, &json!({})).unwrap();

        assert_eq!(res.content["total"].as_u64().unwrap(), 0);
        let engines = res.content["engines"].as_array().unwrap();
        assert!(engines.is_empty());
        // Diagnostic must mention RulesInventory (coverage note per R3).
        assert!(
            res.diagnostics.iter().any(|d| d.contains("RulesInventory")),
            "diagnostic must name the tool when no engines found"
        );
    }

    #[test]
    fn rules_inventory_ruleset_with_no_invokers() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[make_node(
                "rs::orphan",
                "OrphanRules",
                NodeKind::RuleSet,
                "rules/orphan.drl",
                1,
            )])
            .unwrap();
        store.commit_batch().unwrap();

        let tool = RulesInventory;
        let res = tool.invoke(&store, &json!({})).unwrap();

        let engines = res.content["engines"].as_array().unwrap();
        assert_eq!(engines.len(), 1);
        let invoked_by = engines[0]["invoked_by"].as_array().unwrap();
        assert!(
            invoked_by.is_empty(),
            "no InvokedBy edges → invoked_by must be empty"
        );
    }

    // ── DoD-A8: R4 < 25K char ceiling on RankHotspots / Communities / Lineage ──
    //
    // Each test builds a WIDE graph whose untruncated payload would exceed 25K chars, invokes the
    // tool, and asserts BOTH: (1) the serialized `content` is < 25,000 chars, and (2) a truncation
    // diagnostic is present when the cap bites. The falsifier each defeats is a >25K payload on a
    // large repo. A paired "narrow graph" assertion proves the cap does NOT fire spuriously (no
    // truncation diag, full result) on a small graph — the cap is data-driven, not unconditional.

    /// Long ids + long file paths make each row fat, so a few hundred rows blow past 25K chars.
    fn wide_node(i: usize) -> Node {
        // ~120-char id + ~80-char path per node → ~hundreds of chars per serialized row.
        let id = format!(
            "crate::very::deeply::nested::module::path::segment::number::{i:05}::symbol_with_a_long_descriptive_name_{i:05}"
        );
        let name = format!("a_long_descriptive_function_name_number_{i:05}");
        let file = format!("src/very/deeply/nested/module/path/segment/file_{i:05}.rs");
        make_node(&id, &name, NodeKind::Function, &file, (i % 1000) as u32)
    }

    fn wide_id(i: usize) -> String {
        format!(
            "crate::very::deeply::nested::module::path::segment::number::{i:05}::symbol_with_a_long_descriptive_name_{i:05}"
        )
    }

    /// R4 ceiling — RankHotspots. Build a wide hub graph (N callers → 1 core) so PageRank ranks
    /// every node, then cap with a high `limit` so the char budget (not `limit`) is the binding cap.
    #[test]
    fn rank_hotspots_caps_output_under_25k_chars_with_diag() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        let n = 400usize;
        let mut nodes: Vec<Node> = (0..n).map(wide_node).collect();
        nodes.push(make_node(
            "core",
            "core_fn",
            NodeKind::Function,
            "src/core.rs",
            1,
        ));
        store.upsert_nodes(&nodes).unwrap();
        let edges: Vec<Edge> = (0..n)
            .map(|i| make_call_edge(&wide_id(i), "core"))
            .collect();
        store.upsert_edges(&edges).unwrap();
        store.commit_batch().unwrap();

        let tool = RankHotspots;
        // limit=200 (the max) → 201 nodes ranked, but only ≤200 returned; wide rows still exceed 25K.
        let res = tool.invoke(&store, &json!({ "limit": 200 })).unwrap();

        let payload = serde_json::to_string(&res.content).unwrap();
        assert!(
            payload.len() < R4_CHAR_BUDGET,
            "RankHotspots payload must be < {R4_CHAR_BUDGET} chars, got {}",
            payload.len()
        );
        assert_eq!(
            res.content["truncated"],
            json!(true),
            "must report truncation"
        );
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.contains("truncated") && d.contains("R4")),
            "a truncation diagnostic must be present when capped; got {:?}",
            res.diagnostics
        );
    }

    /// Falsifier guard — a narrow graph must NOT trigger the R4 cap (no truncation diag, full list).
    #[test]
    fn rank_hotspots_no_truncation_on_narrow_graph() {
        let store = fixture_store(); // 3 short-named nodes
        let tool = RankHotspots;
        let res = tool.invoke(&store, &json!({ "limit": 200 })).unwrap();
        let payload = serde_json::to_string(&res.content).unwrap();
        assert!(payload.len() < R4_CHAR_BUDGET);
        assert_eq!(
            res.content["truncated"],
            json!(false),
            "narrow graph must not truncate"
        );
        assert!(
            !res.diagnostics.iter().any(|d| d.contains("R4 budget")),
            "no R4 truncation diag on a narrow graph; got {:?}",
            res.diagnostics
        );
        assert_eq!(
            res.content["total"].as_u64().unwrap(),
            3,
            "all 3 nodes ranked"
        );
    }

    /// R4 ceiling — Lineage. Build a star where `root` depends on N wide-named leaves (root → leaf).
    /// Lineage walks Dependencies from `root`, so all N leaves are dependencies; wide rows exceed 25K.
    #[test]
    fn lineage_caps_output_under_25k_chars_with_diag() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        let n = 400usize;
        let mut nodes: Vec<Node> = (0..n).map(wide_node).collect();
        nodes.push(make_node(
            "root",
            "root_fn",
            NodeKind::Function,
            "src/root.rs",
            1,
        ));
        store.upsert_nodes(&nodes).unwrap();
        // root → leaf_i (root depends on each leaf): Dependencies direction reaches all leaves.
        let edges: Vec<Edge> = (0..n)
            .map(|i| make_call_edge("root", &wide_id(i)))
            .collect();
        store.upsert_edges(&edges).unwrap();
        store.commit_batch().unwrap();

        let tool = Lineage;
        let res = tool
            .invoke(&store, &json!({ "symbol": "root", "depth": 24 }))
            .unwrap();

        let payload = serde_json::to_string(&res.content).unwrap();
        assert!(
            payload.len() < R4_CHAR_BUDGET,
            "Lineage payload must be < {R4_CHAR_BUDGET} chars, got {}",
            payload.len()
        );
        assert_eq!(
            res.content["truncated"],
            json!(true),
            "must report truncation"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("R4 budget")),
            "a char-budget truncation diagnostic must be present when capped; got {:?}",
            res.diagnostics
        );
    }

    /// Falsifier guard — a narrow graph must NOT trigger Lineage's R4 cap.
    #[test]
    fn lineage_no_truncation_on_narrow_graph() {
        let store = fixture_store(); // caller → middle → leaf
        let tool = Lineage;
        // Lineage from `caller` reaches middle + leaf (2 deps), tiny payload.
        let res = tool
            .invoke(&store, &json!({ "symbol": "caller", "depth": 24 }))
            .unwrap();
        let payload = serde_json::to_string(&res.content).unwrap();
        assert!(payload.len() < R4_CHAR_BUDGET);
        assert_eq!(
            res.content["truncated"],
            json!(false),
            "narrow graph must not truncate"
        );
        assert!(
            !res.diagnostics.iter().any(|d| d.contains("R4 budget")),
            "no R4 truncation diag on a narrow graph; got {:?}",
            res.diagnostics
        );
        assert_eq!(res.content["total"].as_u64().unwrap(), 2, "middle + leaf");
    }

    /// R4 ceiling — Communities. Build many disjoint wide-named triangles (each a size-3 community)
    /// so the summary list (≤5 top_symbols + ≤3 files per row) exceeds 25K chars across communities.
    #[test]
    fn communities_caps_output_under_25k_chars_with_diag() {
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        let groups = 300usize;
        let mut nodes: Vec<Node> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();
        for g in 0..groups {
            let a = wide_id(g * 3);
            let b = wide_id(g * 3 + 1);
            let c = wide_id(g * 3 + 2);
            nodes.push(wide_node(g * 3));
            nodes.push(wide_node(g * 3 + 1));
            nodes.push(wide_node(g * 3 + 2));
            // Triangle (fully connected) → a tight modularity community of size 3, disjoint from
            // every other group (no cross-group edges).
            edges.push(make_call_edge(&a, &b));
            edges.push(make_call_edge(&b, &c));
            edges.push(make_call_edge(&c, &a));
        }
        store.upsert_nodes(&nodes).unwrap();
        store.upsert_edges(&edges).unwrap();
        store.commit_batch().unwrap();

        let tool = Communities;
        // limit=200 (max) → up to 200 community rows; wide top_symbols/dominant_files exceed 25K.
        let res = tool
            .invoke(&store, &json!({ "limit": 200, "min_size": 2 }))
            .unwrap();

        let payload = serde_json::to_string(&res.content).unwrap();
        assert!(
            payload.len() < R4_CHAR_BUDGET,
            "Communities payload must be < {R4_CHAR_BUDGET} chars, got {}",
            payload.len()
        );
        assert_eq!(
            res.content["truncated"],
            json!(true),
            "must report truncation"
        );
        assert!(
            res.diagnostics.iter().any(|d| d.contains("R4 budget")),
            "a char-budget truncation diagnostic must be present when capped; got {:?}",
            res.diagnostics
        );
    }

    /// Falsifier guard — a narrow graph (one small community) must NOT trigger Communities' R4 cap.
    #[test]
    fn communities_no_truncation_on_narrow_graph() {
        // One triangle → exactly one size-3 community, tiny payload.
        let mut store = MemStore::new();
        store.begin_batch().unwrap();
        store
            .upsert_nodes(&[
                make_node("a", "a_fn", NodeKind::Function, "src/a.rs", 1),
                make_node("b", "b_fn", NodeKind::Function, "src/b.rs", 2),
                make_node("c", "c_fn", NodeKind::Function, "src/c.rs", 3),
            ])
            .unwrap();
        store
            .upsert_edges(&[
                make_call_edge("a", "b"),
                make_call_edge("b", "c"),
                make_call_edge("c", "a"),
            ])
            .unwrap();
        store.commit_batch().unwrap();

        let tool = Communities;
        let res = tool
            .invoke(&store, &json!({ "limit": 200, "min_size": 2 }))
            .unwrap();
        let payload = serde_json::to_string(&res.content).unwrap();
        assert!(payload.len() < R4_CHAR_BUDGET);
        assert_eq!(
            res.content["truncated"],
            json!(false),
            "narrow graph must not truncate"
        );
        assert!(
            !res.diagnostics.iter().any(|d| d.contains("R4 budget")),
            "no R4 truncation diag on a narrow graph; got {:?}",
            res.diagnostics
        );
    }
}
