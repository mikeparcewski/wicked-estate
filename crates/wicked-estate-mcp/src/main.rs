//! `wicked-estate-mcp` — MCP stdio server binary (Wave 2.5).
//!
//! Reads newline-delimited JSON-RPC 2.0 from stdin, writes responses to stdout.
//! One response per request; notifications (no `id` field) produce no output.
//!
//! # Usage
//!
//! ```sh
//! wicked-estate-mcp --db /path/to/graph.db
//! wicked-estate-mcp                       # defaults to .wicked-estate/graph.db
//! WICKED_ESTATE_DB=:memory: wicked-estate-mcp
//! ```

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use tokio::io::AsyncBufReadExt;
use wicked_estate_knowledge::{KnowledgeApi, KnowledgeEngine};
use wicked_estate_mcp::{DomainHandles, McpContext, handle_request_unified};
use wicked_estate_memory::MemoryEngine;
use wicked_estate_memory_core::MemoryApi;
use wicked_estate_overlay::XedgeStore;
use wicked_estate_retrieve::Embedder as _;
use wicked_estate_store::SqliteStore;

// ─────────────────────────────────────────────────────────────────────────────
// CLI / env resolution
// ─────────────────────────────────────────────────────────────────────────────

const DEFAULT_DB: &str = ".wicked-estate/graph.db";

fn resolve_db_path() -> String {
    // Priority: --db <path> > WICKED_ESTATE_DB env > default.
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--db" {
            if let Some(path) = args.next() {
                return path;
            }
        }
    }
    std::env::var("WICKED_ESTATE_DB").unwrap_or_else(|_| DEFAULT_DB.to_string())
}

fn wicked_home() -> String {
    std::env::var("WICKED_HOME").unwrap_or_else(|_| {
        std::env::var("HOME")
            .map(|h| format!("{h}/.wicked"))
            .unwrap_or_else(|_| ".wicked".to_string())
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Async stdio loop
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Telemetry helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Emit a cache-hit/miss counter metric as a fire-and-forget background task so that
/// the blocking `reqwest` HTTP call in `OtlpSink::export_metrics` never stalls a Tokio
/// worker thread in the MCP request loop.
fn emit_cache_counter(
    sink: std::sync::Arc<dyn wicked_estate_core::TelemetrySink>,
    resource: wicked_estate_core::observability::Resource,
    scope: wicked_estate_core::observability::InstrumentationScope,
    level: &str,
    hit: bool,
) {
    use wicked_estate_core::observability::*;
    let name = if hit {
        "wicked_estate.mcp.cache.hits"
    } else {
        "wicked_estate.mcp.cache.misses"
    };
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let metric = Metric {
        name: name.to_string(),
        description: String::new(),
        unit: "1".to_string(),
        data: MetricData::Sum {
            data_points: vec![NumberDataPoint {
                attributes: vec![KeyValue::str("cache.level", level)],
                start_time_unix_nano: t,
                time_unix_nano: t,
                value: MetricValue::I64(1),
            }],
            temporality: AggregationTemporality::Delta,
            is_monotonic: true,
        },
    };
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            if let Err(e) = sink.export_metrics(&resource, &scope, &[metric]) {
                eprintln!("telemetry: {e}");
            }
        })
        .await
        .ok();
    });
}

/// Emit a per-tool latency histogram metric as a fire-and-forget background task so that
/// the blocking `reqwest` HTTP call in `OtlpSink::export_metrics` never stalls a Tokio
/// worker thread in the MCP request loop.
fn emit_tool_duration(
    sink: std::sync::Arc<dyn wicked_estate_core::TelemetrySink>,
    resource: wicked_estate_core::observability::Resource,
    scope: wicked_estate_core::observability::InstrumentationScope,
    tool_name: &str,
    duration_ms: f64,
) {
    use wicked_estate_core::observability::*;
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let metric = Metric {
        name: "wicked_estate.mcp.tool.duration_ms".to_string(),
        description: "Per-tool invocation latency".to_string(),
        unit: "ms".to_string(),
        data: MetricData::Histogram {
            data_points: vec![HistogramDataPoint {
                attributes: vec![KeyValue::str("tool.name", tool_name)],
                start_time_unix_nano: t,
                time_unix_nano: t,
                count: 1,
                sum: duration_ms,
                bucket_counts: vec![1],
                explicit_bounds: vec![],
            }],
            temporality: AggregationTemporality::Delta,
        },
    };
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            if let Err(e) = sink.export_metrics(&resource, &scope, &[metric]) {
                eprintln!("telemetry: {e}");
            }
        })
        .await
        .ok();
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let db_path = resolve_db_path();

    // 2. Open async connection pool instead of a single connection.
    let store = wicked_estate::open_async_store(&db_path)
        .with_context(|| format!("failed to open async store at '{db_path}'"))?;

    let otel_sink = wicked_estate_observe::init_sink_from_env();
    let otel_resource = wicked_estate_core::observability::Resource::service(
        "wicked_estate_mcp",
        env!("CARGO_PKG_VERSION"),
    );
    let otel_scope = wicked_estate_core::observability::InstrumentationScope::versioned(
        "wicked_estate_mcp",
        env!("CARGO_PKG_VERSION"),
    );

    // W7.4: compute staleness once at startup. Best-effort — None on any failure.
    let commits_behind: Option<u64> = {
        // We need the indexed root from meta. Open a second ext handle for meta access.
        let indexed_root = if db_path != ":memory:" {
            wicked_estate_store::open_store_ext(&db_path)
                .ok()
                .and_then(|s| s.meta_get_key("indexed_root"))
        } else {
            None
        };
        if let Some(root) = indexed_root {
            wicked_estate::commits_behind(std::path::Path::new(&root), &db_path)
        } else {
            None
        }
    };

    // Dim-guard (DoD-A6a): read the store's recorded embedder identity + dim, and compute the
    // runtime embedder's. SemanticSearch is advertised/dispatched only when they match.
    let (embedder_meta_id, embedder_meta_dim) = if db_path != ":memory:" {
        wicked_estate_store::open_store_ext(&db_path)
            .ok()
            .map(|s| {
                (
                    s.meta_get_key("embedder_id"),
                    s.meta_get_key("embedder_dim").and_then(|d| d.parse().ok()),
                )
            })
            .unwrap_or((None, None))
    } else {
        (None, None)
    };

    // Build the LIVE SemanticSearch once (it owns a DB connection behind a Mutex — not cloneable,
    // not cheap to rebuild). Shared across requests via Arc. A second SqliteStore handle backs the
    // `nearest` vector lookups; the same DB is passed to invoke as &dyn GraphRead for node lookup.
    // `default_embedder()` is the tiered FastEmbed→model2vec→hash selector — its id/dim are the
    // runtime side of the guard.
    let runtime_embedder = wicked_estate::default_embedder();
    let embedder_runtime_id = Some(runtime_embedder.id().to_string());
    let embedder_runtime_dim = Some(runtime_embedder.dim());
    drop(runtime_embedder);

    let semantic: Option<std::sync::Arc<wicked_estate_retrieve::SemanticSearch>> =
        if db_path != ":memory:" {
            SqliteStore::open(&db_path).ok().map(|vec_store| {
                std::sync::Arc::new(wicked_estate_mcp::live_semantic_search(vec_store))
            })
        } else {
            None
        };

    // 3. Build McpContext with the dim-guard fields.
    let ctx = McpContext {
        commits_behind,
        embedder_runtime_id,
        embedder_runtime_dim,
        embedder_meta_id,
        embedder_meta_dim,
    };

    // 3b. Open domain stores (fail-soft: if any fails, all domains = None).
    let home = wicked_home();
    let memory_path =
        std::env::var("WICKED_MEMORY_DB").unwrap_or_else(|_| format!("{home}/memory.db"));
    let knowledge_path =
        std::env::var("WICKED_KNOWLEDGE_DB").unwrap_or_else(|_| format!("{home}/knowledge.db"));
    let xedge_path =
        std::env::var("WICKED_XEDGE_DB").unwrap_or_else(|_| format!("{home}/xedge.db"));

    // DES-001 §8.4: guard against two engines sharing the same file (data corruption).
    {
        let paths: [(&str, &str); 4] = [
            ("estate", &db_path),
            ("memory", &memory_path),
            ("knowledge", &knowledge_path),
            ("xedge", &xedge_path),
        ];
        let real: Vec<(&str, std::path::PathBuf)> = paths
            .iter()
            .filter(|(_, p)| *p != ":memory:")
            .map(|(name, p)| {
                (
                    *name,
                    std::fs::canonicalize(p).unwrap_or_else(|_| std::path::PathBuf::from(p)),
                )
            })
            .collect();
        for i in 0..real.len() {
            for j in (i + 1)..real.len() {
                if real[i].1 == real[j].1 {
                    panic!(
                        "Store path collision: '{}' and '{}' both resolve to '{}'",
                        real[i].0,
                        real[j].0,
                        real[i].1.display()
                    );
                }
            }
        }
    }

    let domains_result: anyhow::Result<(MemoryEngine, KnowledgeEngine)> = (|| {
        let xedge = std::sync::Arc::new(
            XedgeStore::open(&xedge_path)
                .map_err(|e| anyhow::anyhow!("xedge store unavailable ({xedge_path}): {e}"))?,
        );
        let mem_engine = MemoryEngine::open(&memory_path)
            .map_err(|e| anyhow::anyhow!("memory store unavailable ({memory_path}): {e}"))?
            .with_xedge_store(std::sync::Arc::clone(&xedge));
        let know_engine = KnowledgeEngine::open(&knowledge_path)
            .map_err(|e| anyhow::anyhow!("knowledge store unavailable ({knowledge_path}): {e}"))?
            .with_xedge_store(xedge);
        Ok((mem_engine, know_engine))
    })();

    let mut domain_engines = match domains_result {
        Ok(pair) => Some(pair),
        Err(e) => {
            eprintln!("[wicked-estate] WARN: {e}");
            eprintln!("[wicked-estate] WARN: memory and knowledge tools disabled (domains=None)");
            None
        }
    };

    // 3c. Sync SqliteStore for the unified dispatch (estate read path for epoch lookups).
    let sync_store = SqliteStore::open(&db_path)
        .unwrap_or_else(|_| SqliteStore::in_memory().expect("in-memory fallback"));

    // 4. Async stdin loop.
    // Request cache: key = "tool_name/args_json", value = full MCP response.
    // LLM agents routinely call the same tool with the same args multiple times per session;
    // this turns those repeated calls into memory lookups instead of SQL round-trips.
    //
    // Cache validity: we watch graph.db's mtime. Any external write (e.g. a concurrent
    // `wicked-estate index` run) changes the mtime and causes a full cache clear on the
    // next request, ensuring the agent always sees the current graph state.
    //
    // SCOPE (#102): only GRAPH-domain read tools participate (`response_cacheable`). Both cache
    // levels are versioned/invalidated exclusively by graph-store changes, so memory.* and
    // knowledge.* responses must never enter them: a memory.capture / knowledge.ingest bumps
    // nothing the cache watches, and a cached recall would be served stale indefinitely.
    let mut request_cache: HashMap<String, serde_json::Value> = HashMap::new();
    let mut cache_db_mtime: Option<std::time::SystemTime> = if db_path != ":memory:" {
        std::fs::metadata(&db_path)
            .ok()
            .and_then(|m| m.modified().ok())
    } else {
        None
    };

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let stdout = std::io::stdout();

    while let Some(line) = lines
        .next_line()
        .await
        .context("failed to read from stdin")?
    {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // Malformed JSON — return parse error (-32700).
                let error_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                let bytes = serde_json::to_vec(&error_resp).unwrap_or_default();
                let mut out = stdout.lock();
                out.write_all(&bytes)?;
                out.write_all(b"\n")?;
                out.flush()?;
                continue;
            }
        };

        // Invalidate cache if the DB file was modified since last check (external re-index).
        if let Some(ref mut baseline) = cache_db_mtime {
            if let Ok(current) = std::fs::metadata(&db_path).and_then(|m| m.modified()) {
                if current != *baseline {
                    request_cache.clear();
                    *baseline = current;
                }
            }
        }

        // Build a cache key for tools/call requests — graph-domain read tools only (#102).
        // Memory/knowledge tools bypass both cache levels: their stores' writes never bump the
        // graph version this cache is keyed on, so a hit could be stale forever.
        let cache_key = if req.get("method").and_then(|m| m.as_str()) == Some("tools/call") {
            let tool = req["params"]["name"].as_str().unwrap_or("");
            if wicked_estate_mcp::response_cacheable(tool) {
                let args = req["params"]["arguments"].to_string();
                Some(format!("{tool}/{args}"))
            } else {
                None
            }
        } else {
            None
        };

        // Cache hit: L1 (HashMap) then L2 (SQLite).
        if let Some(ref key) = cache_key {
            // L1 — fast in-memory hit.
            if let Some(cached) = request_cache.get(key) {
                // Patch the id to match the current request before returning.
                let mut hit = cached.clone();
                emit_cache_counter(
                    std::sync::Arc::clone(&otel_sink),
                    otel_resource.clone(),
                    otel_scope.clone(),
                    "l1",
                    true,
                );
                hit["id"] = req["id"].clone();
                let bytes =
                    serde_json::to_vec(&hit).context("failed to serialise cached response")?;
                let mut out = stdout.lock();
                out.write_all(&bytes)?;
                out.write_all(b"\n")?;
                out.flush()?;
                continue;
            }
            // L2 — SQLite persistent cache.
            if let Ok(Some(raw)) = store.cache_get(key).await {
                if let Ok(cached) = serde_json::from_str::<serde_json::Value>(&raw) {
                    // Warm L1 so subsequent hits stay in-memory.
                    request_cache.insert(key.clone(), cached.clone());
                    emit_cache_counter(
                        std::sync::Arc::clone(&otel_sink),
                        otel_resource.clone(),
                        otel_scope.clone(),
                        "l2",
                        true,
                    );
                    let mut hit = cached;
                    hit["id"] = req["id"].clone();
                    let bytes =
                        serde_json::to_vec(&hit).context("failed to serialise cached response")?;
                    let mut out = stdout.lock();
                    out.write_all(&bytes)?;
                    out.write_all(b"\n")?;
                    out.flush()?;
                    continue;
                }
            }
        }

        emit_cache_counter(
            std::sync::Arc::clone(&otel_sink),
            otel_resource.clone(),
            otel_scope.clone(),
            "l1",
            false,
        );
        let tool_name = req["params"]["name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let t_tool = std::time::Instant::now();
        let ctx_clone = ctx.clone();

        // Use block_in_place so we can call synchronous domain engines on the current thread.
        let resp = tokio::task::block_in_place(|| {
            let semantic_ref = semantic
                .as_ref()
                .map(|s| s.as_ref() as &dyn wicked_estate_core::RetrievalTool);
            let mut domains = domain_engines.as_mut().map(|(m, k)| DomainHandles {
                memory: m as &mut dyn MemoryApi<Error = anyhow::Error>,
                knowledge: k as &mut dyn KnowledgeApi,
            });
            handle_request_unified(
                &sync_store,
                &req,
                &ctx_clone,
                domains.as_mut(),
                semantic_ref,
            )
        });
        emit_tool_duration(
            std::sync::Arc::clone(&otel_sink),
            otel_resource.clone(),
            otel_scope.clone(),
            &tool_name,
            t_tool.elapsed().as_millis() as f64,
        );

        // Emit a log record when the tool returns isError=true — fire-and-forget so the
        // blocking reqwest call in OtlpSink::export_logs does not stall the event loop.
        if resp
            .get("result")
            .and_then(|r| r.get("isError"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            use wicked_estate_core::observability::*;
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let err_msg = resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("unknown error");
            let log = LogRecord {
                time_unix_nano: t,
                observed_time_unix_nano: t,
                severity_number: SeverityNumber::Error,
                severity_text: "ERROR".to_string(),
                body: AttributeValue::Str(format!("tool={tool_name} error={err_msg}")),
                attributes: vec![KeyValue::str("tool.name", &tool_name)],
                trace_id: None,
                span_id: None,
            };
            let sink_clone = std::sync::Arc::clone(&otel_sink);
            let resource_clone = otel_resource.clone();
            let scope_clone = otel_scope.clone();
            tokio::spawn(async move {
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = sink_clone.export_logs(&resource_clone, &scope_clone, &[log]) {
                        eprintln!("telemetry: {e}");
                    }
                })
                .await
                .ok();
            });
        }

        // Store in cache (tools/call only; skip notifications which return null).
        if let Some(key) = cache_key {
            request_cache.insert(key.clone(), resp.clone());
            // L2 — persist to SQLite for cross-restart reuse; best-effort, ignore errors.
            if !resp.is_null() {
                if let Ok(raw) = serde_json::to_string(&resp) {
                    let _ = store.cache_put(&key, &raw).await;
                }
            }
        }

        // Notifications return null — emit nothing.
        if resp.is_null() {
            continue;
        }

        let bytes = serde_json::to_vec(&resp).context("failed to serialise response")?;
        let mut out = stdout.lock();
        out.write_all(&bytes)?;
        out.write_all(b"\n")?;
        out.flush()?;
    }

    Ok(())
}
