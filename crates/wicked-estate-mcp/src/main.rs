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
use wicked_estate_core::AsyncGraphStore as _;
use wicked_estate_mcp::{McpContext, handle_request_ctx};
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

// ─────────────────────────────────────────────────────────────────────────────
// Async stdio loop
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let db_path = resolve_db_path();

    // 2. Open async connection pool instead of a single connection.
    let store = wicked_estate::open_async_store(&db_path)
        .with_context(|| format!("failed to open async store at '{db_path}'"))?;

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

    // Task F: build SemanticSearch with a second SqliteStore for vector lookup.
    let has_semantic = db_path != ":memory:";
    let _sem_store_for_future_ctx = if has_semantic {
        SqliteStore::open(&db_path).ok()
    } else {
        None
    };

    // 3. Build McpContext (unchanged — commits_behind, has_semantic_search).
    let ctx = McpContext {
        commits_behind,
        has_semantic_search: has_semantic,
    };

    // 4. Async stdin loop.
    // Request cache: key = "tool_name/args_json", value = full MCP response.
    // LLM agents routinely call the same tool with the same args multiple times per session;
    // this turns those repeated calls into memory lookups instead of SQL round-trips.
    //
    // Cache validity: we watch graph.db's mtime. Any external write (e.g. a concurrent
    // `wicked-estate index` run) changes the mtime and causes a full cache clear on the
    // next request, ensuring the agent always sees the current graph state.
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

        // Build a cache key for tools/call requests (read-only, deterministic).
        let cache_key = if req.get("method").and_then(|m| m.as_str()) == Some("tools/call") {
            let tool = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"]["arguments"].to_string();
            Some(format!("{tool}/{args}"))
        } else {
            None
        };

        // Cache hit: L1 (HashMap) then L2 (SQLite).
        if let Some(ref key) = cache_key {
            // L1 — fast in-memory hit.
            if let Some(cached) = request_cache.get(key) {
                // Patch the id to match the current request before returning.
                let mut hit = cached.clone();
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

        let ctx_clone = ctx.clone();
        let resp = store
            .with_read(move |graph| Ok(handle_request_ctx(graph, &req, &ctx_clone)))
            .await?;

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
