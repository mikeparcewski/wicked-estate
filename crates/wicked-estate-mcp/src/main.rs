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
use std::io::{self, BufRead, Write};
use wicked_estate_mcp::{McpContext, handle_request_ctx};
use wicked_estate_store::{SqliteStore, open_store};

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
// Stdio loop
// ─────────────────────────────────────────────────────────────────────────────

fn run() -> Result<()> {
    let db_path = resolve_db_path();
    let store =
        open_store(&db_path).with_context(|| format!("failed to open store at '{db_path}'"))?;
    let store_ref: &dyn wicked_estate_core::GraphRead = &*store;

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

    let ctx = McpContext {
        commits_behind,
        has_semantic_search: has_semantic,
    };

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = line.context("failed to read from stdin")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // Malformed JSON — return parse error (-32700).
                let error_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                let bytes = serde_json::to_vec(&error_resp).unwrap_or_default();
                out.write_all(&bytes)?;
                out.write_all(b"\n")?;
                out.flush()?;
                continue;
            }
        };

        let resp = handle_request_ctx(store_ref, &req, &ctx);

        // Notifications return null — emit nothing.
        if resp.is_null() {
            continue;
        }

        let bytes = serde_json::to_vec(&resp).context("failed to serialise response")?;
        out.write_all(&bytes)?;
        out.write_all(b"\n")?;
        out.flush()?;
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("wicked-estate-mcp: fatal: {e:#}");
        std::process::exit(1);
    }
}
