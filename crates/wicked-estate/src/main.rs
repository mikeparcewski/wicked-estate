//! `wicked-estate` — CLI over the indexing pipeline (`wicked_estate` lib).
//!
//!   wicked-estate index <path>           [--db <file|:memory:>] [--history] [--embeddings]
//!   wicked-estate scip  <root>           [--db ...] [--scip-file <path>]
//!   wicked-estate tfstate <file>         [--db ...]
//!   wicked-estate drift                  [--db ...]
//!   wicked-estate query <name>           [--db ...]
//!   wicked-estate blast-radius <name>    [--db ...]
//!   wicked-estate stats                  [--db ...]
//!   wicked-estate rank                   [--db ...]
//!   wicked-estate source <name>          [--db ...]
//!   wicked-estate semantic <query>       [--db ...]
//!   wicked-estate cross-graph <name>     --db <a.db> --db <b.db> ...
//!                                     (or --dbs a.db,b.db,c.db)
//!   wicked-estate watch <path>           [--db ...] [--history]
//!   wicked-estate subscribe              [--db ...] [--since <seq>]
//!   wicked-estate clusters [<min_size>]  [--json] [--db ...]
//!   wicked-estate fingerprint <name>     [--db ...]
//!   wicked-estate changed-since <sha>    [--json] [--db ...]
//!   wicked-estate annotate <name>        --key K --value V [--confidence F] [--provenance P] [--author A] [--db ...]
//!   wicked-estate annotations <name>     [--db ...]
//!   wicked-estate context <name>         [--budget <chars>] [--json] [--db ...]
//!   wicked-estate entrypoints            [--json] [--db ...]
//!   wicked-estate leaves                 [--json] [--db ...]
//!   wicked-estate dead-code              [--json] [--db ...]
//!   wicked-estate nodes [--kind K]       [--json] [--db ...]

mod scip_auto;

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;
use std::path::Path;
use std::time::Duration;
use wicked_estate_store::{GraphStoreMutExt, SqliteStore, open_store, open_store_ext};

fn to_any(e: wicked_estate_core::Error) -> anyhow::Error {
    anyhow::anyhow!(e.to_string())
}

fn ensure_db_dir(db: &str) -> Result<()> {
    if db == ":memory:" {
        return Ok(());
    }
    if let Some(parent) = Path::new(db).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// W7.4: emit staleness notice if git reports commits since the db was written.
/// Reads the indexed root from store meta; resolves the db path for mtime.
fn maybe_print_staleness(store: &dyn wicked_estate_store::GraphStoreMutExt, db: &str) {
    let root_str = match store.meta_get_key("indexed_root") {
        Some(r) => r,
        None => return, // never indexed yet
    };
    if let Some(n) = wicked_estate::commits_behind(Path::new(&root_str), db) {
        if n > 0 {
            println!(
                "STALENESS: {n} commit(s) since last index — run `wicked-estate index {root_str}` to refresh"
            );
        }
    }
}

fn loc(n: &wicked_estate_core::Node) -> String {
    format!("{}:{}", n.location.file, n.location.span.start_line + 1)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => ("help", &[][..]),
    };

    // Parse shared flags: `--db <spec>`, `--dbs a,b,c`, and `--scip-file <path>`;
    // everything else is positional.
    //
    // `--db` may be repeated; the LAST single `--db` value is used for single-db commands
    // (backward-compatible).  All `--db` values are collected into `db_paths` for the
    // `cross-graph` command.  `--dbs a,b,c` is an alias that accepts a comma-delimited list.
    let mut db = ".wicked-estate/graph.db".to_string();
    let mut db_paths: Vec<String> = Vec::new();
    let mut scip_file: Option<String> = None;
    let mut since: u64 = 0;
    // history_enabled: OFF by default; opt-in with `--history`.
    let mut history = false;
    // embeddings: OFF by default; opt-in with `--embeddings`.
    let mut embeddings = false;
    // Semantic-annotation flags for the `semantics` command (requirement↔functionality linking).
    let mut sem_description: Option<String> = None;
    let mut sem_requirement: Option<String> = None;
    let mut sem_validated: Option<bool> = None;
    // Annotation flags for the `annotate` command.
    let mut ann_key: Option<String> = None;
    let mut ann_value: Option<String> = None;
    let mut ann_confidence: f64 = 1.0;
    let mut ann_provenance: String = String::new();
    let mut ann_author: String = String::new();
    let mut positional: Vec<String> = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => {
                if let Some(v) = it.next() {
                    db = v.clone();
                    db_paths.push(v.clone());
                }
            }
            "--dbs" => {
                if let Some(v) = it.next() {
                    for part in v.split(',') {
                        let p = part.trim().to_string();
                        if !p.is_empty() {
                            db_paths.push(p.clone());
                            db = p; // last one becomes the single-db default
                        }
                    }
                }
            }
            "--scip-file" => {
                if let Some(v) = it.next() {
                    scip_file = Some(v.clone());
                }
            }
            "--since" => {
                if let Some(v) = it.next() {
                    since = v.parse::<u64>().unwrap_or(0);
                }
            }
            "--history" => {
                history = true;
            }
            "--embeddings" => {
                embeddings = true;
            }
            "--description" => {
                if let Some(v) = it.next() {
                    sem_description = Some(v.clone());
                }
            }
            "--requirement" => {
                if let Some(v) = it.next() {
                    sem_requirement = Some(v.clone());
                }
            }
            "--validated" => {
                if let Some(v) = it.next() {
                    sem_validated = Some(matches!(v.as_str(), "true" | "1" | "yes"));
                }
            }
            "--key" => {
                if let Some(v) = it.next() {
                    ann_key = Some(v.clone());
                }
            }
            "--value" => {
                if let Some(v) = it.next() {
                    ann_value = Some(v.clone());
                }
            }
            "--confidence" => {
                if let Some(v) = it.next() {
                    ann_confidence = v.parse::<f64>().unwrap_or(1.0);
                }
            }
            "--provenance" => {
                if let Some(v) = it.next() {
                    ann_provenance = v.clone();
                }
            }
            "--author" => {
                if let Some(v) = it.next() {
                    ann_author = v.clone();
                }
            }
            _ => positional.push(a.clone()),
        }
    }

    match cmd {
        "index" => {
            let path = positional.first().map(String::as_str).unwrap_or(".");
            ensure_db_dir(&db)?;
            let stats = if history && db != ":memory:" {
                // Caller explicitly opted in to history — open the concrete store to call
                // set_history_enabled(true) (inherent method, not on any trait), then box it.
                // Mirrors the `compact` arm pattern.
                let mut concrete = SqliteStore::open(&db).map_err(to_any)?;
                concrete.set_history_enabled(true).map_err(to_any)?;
                let mut store: Box<dyn GraphStoreMutExt> = Box::new(concrete);
                wicked_estate::index_path(store.as_mut(), Path::new(path)).map_err(to_any)?
            } else {
                // Default: history OFF (no-bloat-by-default).
                let mut store = open_store_ext(&db).map_err(to_any)?;
                wicked_estate::index_path(store.as_mut(), Path::new(path)).map_err(to_any)?
            };
            println!(
                "indexed {path} ({db}) → {} nodes, {} edges, {} files",
                stats.node_count, stats.edge_count, stats.file_count
            );
            for (k, v) in &stats.edges_by_kind {
                println!("  {k} = {v}");
            }
            // W5.2: optional embeddings pass — OFF by default, opt-in with --embeddings.
            // Runs as a separate step so index_path's public signature is unchanged.
            // :memory: is skipped (embeddings live in the same store; nothing to persist).
            if embeddings && db != ":memory:" {
                let mut emb_store = SqliteStore::open(&db).map_err(to_any)?;
                let embedder = wicked_estate::default_embedder();
                let n = wicked_estate::compute_embeddings(&mut emb_store, &*embedder)
                    .map_err(to_any)?;
                println!("embedded {n} symbols");
            }
        }
        "scip" => {
            let root_str = positional.first().map(String::as_str).unwrap_or(".");
            let root = Path::new(root_str);
            ensure_db_dir(&db)?;

            if let Some(explicit) = scip_file.as_deref() {
                let scip_path = Path::new(explicit);
                let mut store = open_store_ext(&db).map_err(to_any)?;
                let count =
                    wicked_estate::ingest_scip(store.as_mut(), root, scip_path).map_err(to_any)?;
                println!("scip (explicit): ingested {count} precise edge(s) from {explicit} into {db}");
                return Ok(());
            }

            let mut results = crate::scip_auto::auto_scip(root)?;

            let default_scip = root.join("index.scip");
            let already_listed = results.iter().any(|r| r.path == default_scip);
            if default_scip.exists() && !already_listed {
                results.insert(
                    0,
                    crate::scip_auto::ScipResult {
                        lang: "pregenerated",
                        path: default_scip.clone(),
                    },
                );
            }

            if results.is_empty() {
                println!(
                    "notice: no SCIP indexers ran — provide --scip-file or install a supported SCIP indexer"
                );
                return Ok(());
            }

            let mut store = open_store_ext(&db).map_err(to_any)?;
            for result in &results {
                if !result.path.exists() {
                    continue;
                }
                let count =
                    wicked_estate::ingest_scip(store.as_mut(), root, &result.path).map_err(to_any)?;
                let path_display = result.path.display();
                println!(
                    "scip ({}): ingested {count} precise edge(s) from {path_display} into {db}",
                    result.lang
                );
            }
        }
        // Task B: ingest a Terraform state file (live resource nodes → estate LIVE side).
        "tfstate" => {
            let file_path = positional
                .first()
                .context("usage: wicked-estate tfstate <file.tfstate> [--db ...]")?;
            let json = std::fs::read_to_string(file_path)
                .with_context(|| format!("cannot read tfstate file '{file_path}'"))?;
            ensure_db_dir(&db)?;
            let mut store = open_store_ext(&db).map_err(to_any)?;
            let n = wicked_estate::ingest_tfstate(store.as_mut(), &json).map_err(to_any)?;
            println!("tfstate: upserted {n} live resource node(s) from '{file_path}' into {db}");
        }
        // Task C: W10 drift report.
        "drift" => {
            let store = open_store(&db).map_err(to_any)?;
            let report = wicked_estate::estate_drift(&*store).map_err(to_any)?;
            println!("--- estate drift report ---");
            println!("managed (iac + live):   {}", report.managed.len());
            println!("undeployed (iac-only):  {}", report.undeployed.len());
            println!("unmanaged (live-only):  {}", report.unmanaged.len());
            if !report.unmanaged.is_empty() {
                println!("\nUNMANAGED resources (live, no IaC declaration):");
                for n in &report.unmanaged {
                    println!("  {} ({})", n.name, n.location.file);
                }
            }
            if !report.undeployed.is_empty() {
                println!("\nUNDEPLOYED resources (IaC-declared, not in live state):");
                for n in &report.undeployed {
                    println!("  {} ({})", n.name, n.location.file);
                }
            }
            if !report.managed.is_empty() {
                println!(
                    "\nMANAGED resources (iac + live, {} total):",
                    report.managed.len()
                );
                for n in report.managed.iter().take(20) {
                    println!("  {}", n.name);
                }
                if report.managed.len() > 20 {
                    println!("  ... and {} more", report.managed.len() - 20);
                }
            }
        }
        "query" => {
            let name = positional
                .first()
                .context("usage: wicked-estate query <name>")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
            println!("{} match(es) for '{name}':", hits.len());
            for n in &hits {
                println!("  {:?} {} ({})", n.kind, n.name, loc(n));
            }
        }
        "blast-radius" => {
            let name = positional
                .first()
                .context("usage: wicked-estate blast-radius <name>")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            let deps = wicked_estate::blast_radius_by_name(&*store, name, 12).map_err(to_any)?;
            let unresolved = store.unresolved_refs_for_name(name).map_err(to_any)?.len();
            if deps.is_empty() {
                println!("no resolved dependents for '{name}' (symbol may not be indexed)");
            } else {
                println!("{} symbol(s) depend on '{name}':", deps.len());
                for n in &deps {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
            }
            // Honest coverage — never let the absence of dependents read as "safe to change".
            println!(
                "coverage: {} resolved dependent(s); {unresolved} unresolved call(s) reference \
                 '{name}' — best-effort static resolution, MAY be incomplete (precise tier pending)",
                deps.len()
            );
        }
        "stats" => {
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            let s = store.stats().map_err(to_any)?;
            let db_mb = s.db_size_bytes as f64 / 1_048_576.0;
            println!(
                "nodes={} edges={} files={} db={:.1}MB",
                s.node_count, s.edge_count, s.file_count, db_mb
            );
            for (k, v) in &s.edges_by_kind {
                println!("  edge {k} = {v}");
            }
            if s.db_size_bytes > 500 * 1_048_576 {
                println!(
                    "  hint: db is {:.0}MB — run `wicked-estate compact` to reclaim space",
                    db_mb
                );
            }
            // W7: print git provenance if available.
            if let Ok(Some(info)) = store.repo_info() {
                print!("repo:");
                if let Some(c) = &info.commit {
                    let short = &c[..8.min(c.len())];
                    print!("  commit={short}");
                }
                if let Some(b) = &info.branch {
                    print!("  branch={b}");
                }
                if info.dirty {
                    print!("  dirty");
                }
                println!();
            }
        }
        "rank" | "hotspots" => {
            let store = open_store_ext(&db).map_err(to_any)?;
            let top = wicked_estate::important_symbols(store.as_ref(), 25).map_err(to_any)?;
            println!("top {} symbols by PageRank:", top.len());
            for (n, score) in &top {
                println!("  {score:.4}  {:?} {} ({})", n.kind, n.name, loc(n));
            }
        }
        "source" => {
            let name = positional
                .first()
                .context("usage: wicked-estate source <name>")?;
            let store = open_store(&db).map_err(to_any)?;
            let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
            if hits.is_empty() {
                println!("no symbols found for '{name}'");
            } else {
                println!("{} match(es) for '{name}':", hits.len());
                for n in &hits {
                    let src = store.symbol_source(n).map_err(to_any)?;
                    println!("  [{:?}] {} @ {}", n.kind, n.name, loc(n));
                    match src {
                        Some(text) => println!("{text}"),
                        None => println!("  (source not stored — re-run 'index' to populate)"),
                    }
                    println!();
                }
            }
        }
        // Task F: semantic search via embedding-based ANN.
        "semantic" => {
            let query = positional
                .first()
                .context("usage: wicked-estate semantic <query> [--db ...]")?;
            // SemanticSearch needs a concrete VectorStore (not the trait object). Open a separate
            // SqliteStore handle for the vector side; the main store handle is for GraphRead.
            use wicked_estate_retrieve::SemanticSearch;
            use wicked_estate_store::SqliteStore;
            ensure_db_dir(&db)?;
            let graph_store = open_store(&db).map_err(to_any)?;
            // Same embedder factory as index-time (FastEmbedder under `fastembed`, else lexical),
            // so the query vector shares the stored vectors' dimension.
            let sem_tool = if db == ":memory:" {
                let vec_store = wicked_estate_store::MemStore::new();
                SemanticSearch::new(wicked_estate::default_embedder(), vec_store)
            } else {
                let vec_store = SqliteStore::open(&db).map_err(to_any)?;
                SemanticSearch::new(wicked_estate::default_embedder(), vec_store)
            };
            use wicked_estate_core::RetrievalTool;
            let req = serde_json::json!({ "query": query, "k": 20 });
            match sem_tool.invoke(&*graph_store, &req) {
                Ok(result) => {
                    let matches = result.content["matches"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    println!("{} semantic match(es) for '{query}':", matches.len());
                    for m in &matches {
                        println!(
                            "  [{:.3}] {:?} {} ({}:{})",
                            m["similarity"].as_f64().unwrap_or(0.0),
                            m["kind"],
                            m["name"].as_str().unwrap_or("?"),
                            m["file"].as_str().unwrap_or("?"),
                            m["line"].as_u64().unwrap_or(0) + 1,
                        );
                    }
                    for d in &result.diagnostics {
                        eprintln!("note: {d}");
                    }
                }
                Err(e) => {
                    eprintln!("semantic search error: {e}");
                }
            }
        }
        // W12 — cross-graph / federated query (multi-repo).
        //
        // Usage:
        //   wicked-estate cross-graph <name> --db <a.db> --db <b.db> [--db <c.db> ...]
        //   wicked-estate cross-graph <name> --dbs a.db,b.db,c.db
        //
        // Prints, per repo, the matching symbols and a combined cross-repo blast-radius.
        "cross-graph" => {
            let name = positional
                .first()
                .context("usage: wicked-estate cross-graph <name> --db <a.db> --db <b.db> ...")?;

            if db_paths.is_empty() {
                anyhow::bail!(
                    "cross-graph requires at least one --db <path> or --dbs a,b,c argument"
                );
            }

            // ── Symbol search across all repos ───────────────────────────────
            println!(
                "=== cross-graph search: '{}' across {} repo(s) ===",
                name,
                db_paths.len()
            );
            let (search_results, search_errors) =
                wicked_estate::cross_graph_search(&db_paths, name).map_err(to_any)?;

            if search_results.is_empty() {
                println!("no matches for '{name}' in any of the specified databases");
            } else {
                println!("{} match(es) total:", search_results.len());
                // Group by repo for cleaner output.
                let mut current_repo = "";
                for (repo, node) in &search_results {
                    if repo.as_str() != current_repo {
                        println!("\n  [repo: {repo}]");
                        current_repo = repo.as_str();
                    }
                    println!("    {:?} {} ({})", node.kind, node.name, loc(node));
                }
            }

            for err in &search_errors {
                eprintln!("warning: {err}");
            }

            // ── Cross-repo blast-radius ───────────────────────────────────────
            println!("\n=== cross-graph blast-radius: '{}' dependents ===", name);
            let (br_results, br_errors) =
                wicked_estate::cross_graph_blast_radius(&db_paths, name, 12).map_err(to_any)?;

            if br_results.is_empty() {
                println!("no resolved dependents for '{name}' across the specified databases");
            } else {
                println!(
                    "{} dependent(s) total (union across repos):",
                    br_results.len()
                );
                let mut current_repo = "";
                for (repo, node) in &br_results {
                    if repo.as_str() != current_repo {
                        println!("\n  [repo: {repo}]");
                        current_repo = repo.as_str();
                    }
                    println!("    {:?} {} ({})", node.kind, node.name, loc(node));
                }
            }

            for err in &br_errors {
                eprintln!("warning: {err}");
            }

            println!(
                "\nNOTE: cross-repo matching is by symbol name only. Cross-repo EDGES are not"
            );
            println!("resolved — each repo's graph contains only intra-repo edges. Package-aware");
            println!("cross-repo edge resolution is a future step (package-resolver tier).");
        }
        // Task E: compact — prune cruft + vacuum the database.
        //
        // Usage:
        //   wicked-estate compact [--db <file>]
        //
        // Opens the database as a concrete SqliteStore and calls compact(). Prints the
        // CompactStats so the operator knows what was reclaimed. The :memory: pseudo-path
        // is rejected (nothing to compact in an ephemeral store).
        "compact" => {
            if db == ":memory:" {
                anyhow::bail!("compact does not apply to an in-memory store");
            }
            ensure_db_dir(&db)?;
            let mut store = SqliteStore::open(&db).map_err(to_any)?;
            let stats = store.compact().map_err(to_any)?;
            println!("compact({db}):");
            println!("  dangling edges pruned:   {}", stats.dangling_edges);
            println!("  stale cache rows pruned: {}", stats.stale_cache_rows);
            println!("  orphan embeddings pruned:{}", stats.orphan_embeddings);
            println!("  orphan content rows pruned:{}", stats.orphan_content);
            println!("WAL checkpointed and VACUUM complete.");
        }
        // W7.1: watch — initial full index then reactive re-index on any file change.
        //
        // Usage:
        //   wicked-estate watch <path>  [--db <file>] [--history]
        //
        // Performs an initial `index_path` on <path>, then watches <path> recursively using a
        // 500ms debounced watcher.  On each debounced batch, `index_path` is called again
        // (incremental — digest-skip makes it cheap).  Prints a summary line per cycle.
        // Runs until Ctrl-C.
        //
        // --history opts in to edge-history archival for the session (default: off).
        // The watch loop itself does not benefit from history, but enabling it means the
        // edge provenance is preserved for `subscribe` callers that want it.
        "watch" => {
            let path_str = positional.first().map(String::as_str).unwrap_or(".");
            let watch_path = Path::new(path_str);
            ensure_db_dir(&db)?;

            // Initial index.
            let mut store: Box<dyn GraphStoreMutExt> = if history && db != ":memory:" {
                let mut concrete = SqliteStore::open(&db).map_err(to_any)?;
                concrete.set_history_enabled(true).map_err(to_any)?;
                Box::new(concrete)
            } else {
                open_store_ext(&db).map_err(to_any)?
            };

            let stats = wicked_estate::index_path(store.as_mut(), watch_path).map_err(to_any)?;
            println!(
                "watch: initial index of {path_str} → {} nodes, {} edges, {} files",
                stats.node_count, stats.edge_count, stats.file_count
            );

            // Set up the debounced watcher.  The channel carries batched event results.
            // The callback moves `tx` and forwards each batch; the event loop reads from `rx`.
            let (tx, rx) = std::sync::mpsc::channel();
            let mut debouncer = new_debouncer(Duration::from_millis(500), None, move |res| {
                tx.send(res).ok();
            })
            .map_err(|e| anyhow::anyhow!("watch: failed to create debouncer: {e}"))?;
            debouncer
                .watch(watch_path, RecursiveMode::Recursive)
                .map_err(|e| anyhow::anyhow!("watch: failed to watch {path_str}: {e}"))?;

            println!("watch: watching {path_str} — press Ctrl-C to stop");

            // Event loop: blocks until the channel is closed (Ctrl-C drops the watcher).
            for result in rx {
                match result {
                    Ok(events) => {
                        // Only re-index when there is at least one create/modify/remove event.
                        // EventKind variants: Create, Modify, Remove, Access, Other.
                        let relevant = events.iter().any(|ev| {
                            matches!(
                                ev.kind,
                                notify::EventKind::Create(_)
                                    | notify::EventKind::Modify(_)
                                    | notify::EventKind::Remove(_)
                            )
                        });
                        if relevant {
                            match wicked_estate::index_path(store.as_mut(), watch_path) {
                                Ok(s) => {
                                    println!(
                                        "watch: re-indexed → {} nodes, {} edges, {} files",
                                        s.node_count, s.edge_count, s.file_count
                                    );
                                }
                                Err(e) => {
                                    eprintln!("watch: re-index error (non-fatal): {e}");
                                }
                            }
                        }
                    }
                    Err(errs) => {
                        for e in errs {
                            eprintln!("watch error: {e}");
                        }
                    }
                }
            }
        }
        // W7.1: subscribe — one-shot poll of the change-log since a cursor.
        //
        // Usage:
        //   wicked-estate subscribe  [--db <file>] [--since <seq>]
        //
        // Opens the store, calls `changes_since(since)`, and prints each Change as a JSON line:
        //   {"seq":N,"op":"upsert|remove","target":"path/to/file"}
        // Ends with a line reporting the new high-watermark seq so the caller can resume:
        //   {"next_seq":N}
        //
        // This is intentionally a one-shot poll.  A daemon would loop: sleep → poll → sleep.
        "subscribe" => {
            let store = open_store_ext(&db).map_err(to_any)?;
            let changes = store.changes_since(since).map_err(to_any)?;
            let mut max_seq = since;
            for c in &changes {
                let op_str = match c.op {
                    wicked_estate_core::ChangeOp::Upsert => "upsert",
                    wicked_estate_core::ChangeOp::Remove => "remove",
                };
                // Use serde_json for the target string so paths with special chars are safe.
                let target_json = serde_json::to_string(&c.target)
                    .unwrap_or_else(|_| format!("\"{}\"", c.target));
                println!(
                    "{{\"seq\":{},\"op\":\"{op_str}\",\"target\":{target_json}}}",
                    c.seq
                );
                if c.seq > max_seq {
                    max_seq = c.seq;
                }
            }
            // Emit the new high-watermark so the caller can resume from this point.
            println!("{{\"next_seq\":{max_seq}}}");
        }
        // Semantic linking: annotate a symbol with its description / matched requirement /
        // validation, or show the current annotations. (Set ⇄ Show by presence of --set flags.)
        "semantics" => {
            let symbol = positional.first().cloned().unwrap_or_default();
            if symbol.is_empty() {
                eprintln!(
                    "usage: wicked-estate semantics <symbol> [--description X] [--requirement Y] [--validated true|false] [--db ...]"
                );
            } else {
                let mut store = open_store_ext(&db).map_err(to_any)?;
                let setting = sem_description.is_some()
                    || sem_requirement.is_some()
                    || sem_validated.is_some();
                if setting {
                    wicked_estate::set_semantics(
                        &mut *store,
                        &symbol,
                        sem_description.as_deref(),
                        sem_requirement.as_deref(),
                        sem_validated,
                    )
                    .map_err(to_any)?;
                    println!("updated semantics for {symbol}");
                } else {
                    match wicked_estate::get_semantics(&*store, &symbol).map_err(to_any)? {
                        Some(s) => {
                            println!("symbol: {symbol}");
                            println!(
                                "  description: {}",
                                s.description.as_deref().unwrap_or("(none)")
                            );
                            println!(
                                "  requirement: {}",
                                s.requirement.as_deref().unwrap_or("(none)")
                            );
                            println!("  validated:   {}", s.requirement_validated);
                        }
                        None => println!("no semantics set for {symbol}"),
                    }
                }
            }
        }
        // Reverse link: every symbol annotated with a given requirement.
        "by-requirement" => {
            let req = positional.first().cloned().unwrap_or_default();
            let store = open_store_ext(&db).map_err(to_any)?;
            let hits = wicked_estate::symbols_for_requirement(&*store, &req).map_err(to_any)?;
            println!("symbols satisfying requirement {req:?}: {}", hits.len());
            for n in &hits {
                println!(
                    "  {} ({}:{})",
                    n.name,
                    n.location.file,
                    n.location.span.start_line + 1
                );
            }
        }
        // Agent B: community detection on the call/import graph.
        //
        // Usage:
        //   wicked-estate clusters [<min-size>] [--json] [--db ...]
        //
        // Detects connected communities using union-find over CALLS/IMPORTS edges.
        // Outputs community membership sorted by size descending.
        "clusters" => {
            let min_size = positional
                .iter()
                .find(|a| a.parse::<usize>().is_ok())
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(2);
            let json_out = positional.iter().any(|a| a == "--json");
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            let communities =
                wicked_estate_rank::detect_communities(store.as_ref(), min_size, false)
                    .map_err(to_any)?;
            if json_out {
                let j: Vec<Vec<String>> = communities
                    .iter()
                    .map(|c| c.iter().map(|s| s.to_string()).collect())
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!("{} communities (min_size={min_size}):", communities.len());
                for (i, c) in communities.iter().enumerate() {
                    println!("  cluster {}: {} symbols", i + 1, c.len());
                    for sym in c.iter().take(5) {
                        println!("    {sym}");
                    }
                    if c.len() > 5 {
                        println!("    ... and {} more", c.len() - 5);
                    }
                }
            }
        }
        // Agent C: budget context — ranked symbols fitting within a character budget.
        //
        // Usage:
        //   wicked-estate context <name> --budget <chars> [--json] [--db ...]
        //
        // Returns the highest-PageRank symbols reachable from <name> that fit within
        // the character budget, suitable for injecting into an LLM prompt.
        "context" => {
            let name = positional
                .first()
                .context("usage: wicked-estate context <name> --budget <chars>")?;
            let mut budget = 4096usize;
            let mut it2 = rest.iter();
            while let Some(a) = it2.next() {
                if a.as_str() == "--budget" {
                    if let Some(v) = it2.next() {
                        budget = v.parse::<usize>().unwrap_or(4096);
                    }
                }
            }
            let json_out = positional.iter().any(|a| a == "--json");
            // open_store_ext returns Box<dyn GraphStoreMutExt> so as_ref() satisfies
            // maybe_print_staleness's &dyn GraphStoreMutExt parameter.
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            let nodes =
                wicked_estate_retrieve::budget_context(&*store, name, budget)
                    .map_err(to_any)?;
            if json_out {
                let j: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "file": n.location.file,
                            "line": n.location.span.start_line + 1,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!(
                    "{} symbol(s) in context for '{}' (budget={budget} chars):",
                    nodes.len(),
                    name
                );
                for n in &nodes {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
            }
        }
        // Agent A: annotation API — tag any indexed symbol with arbitrary key/value metadata.
        //
        // Usage:
        //   wicked-estate annotate <name> --key K --value V [--confidence F] [--provenance P] [--author A] [--db ...]
        "annotate" => {
            let name = positional
                .first()
                .context("usage: wicked-estate annotate <name> --key K --value V [--db ...]")?;
            let key = ann_key
                .as_deref()
                .context("--key is required for the annotate command")?;
            let value = ann_value
                .as_deref()
                .context("--value is required for the annotate command")?;
            ensure_db_dir(&db)?;
            let mut store = SqliteStore::open(&db).map_err(to_any)?;
            let hits = wicked_estate::search(&store, name).map_err(to_any)?;
            let mut count = 0usize;
            for n in &hits {
                store
                    .annotate_node(
                        &n.symbol,
                        key,
                        value,
                        ann_confidence,
                        &ann_provenance,
                        &ann_author,
                    )
                    .map_err(to_any)?;
                count += 1;
            }
            println!("annotated {count} symbol(s) with {key}={value}");
        }
        // Agent A: show annotations for a symbol.
        //
        // Usage:
        //   wicked-estate annotations <name> [--db ...]
        "annotations" => {
            let name = positional
                .first()
                .context("usage: wicked-estate annotations <name> [--db ...]")?;
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let hits = wicked_estate::search(&store, name).map_err(to_any)?;
            if hits.is_empty() {
                println!("no symbols found for '{name}'");
            } else {
                for n in &hits {
                    let anns = store.get_annotations(&n.symbol).map_err(to_any)?;
                    println!("  [{:?}] {} ({})", n.kind, n.name, loc(n));
                    if anns.is_empty() {
                        println!("    (no annotations)");
                    } else {
                        for a in &anns {
                            println!(
                                "    {}={} [confidence={:.3} provenance={:?} author={:?}]",
                                a.key, a.value, a.confidence, a.provenance, a.author
                            );
                        }
                    }
                }
            }
        }
        // Agent D: stable hex fingerprint for a symbol (covers id+name+kind+file+signature).
        //
        // Usage:
        //   wicked-estate fingerprint <name> [--db ...]
        "fingerprint" => {
            let name = positional
                .first()
                .context("usage: wicked-estate fingerprint <name>")?;
            let store = open_store(&db).map_err(to_any)?;
            let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
            drop(store);
            if hits.is_empty() {
                println!("no symbol found matching '{name}'");
                return Ok(());
            }
            let store = SqliteStore::open(&db).map_err(to_any)?;
            for node in &hits {
                match store.node_fingerprint(&node.symbol).map_err(to_any)? {
                    Some(fp) => println!("{fp}  {:?} {} ({})", node.kind, node.name, loc(node)),
                    None => println!("(not indexed)  {} ", node.name),
                }
            }
        }
        // Agent D: symbols in files changed since a git SHA.
        //
        // Usage:
        //   wicked-estate changed-since <git-sha> [--json] [--db ...]
        "changed-since" => {
            let sha = positional
                .first()
                .context("usage: wicked-estate changed-since <git-sha>")?;
            let output = std::process::Command::new("git")
                .args(["diff", "--name-only", &format!("{sha}..HEAD")])
                .output()
                .context("git diff failed — is this a git repository?")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git diff failed: {stderr}");
            }
            let changed_files: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if changed_files.is_empty() {
                println!("no files changed since {sha}");
                return Ok(());
            }
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let json_out = positional.iter().any(|a| a == "--json");
            let mut all_nodes: Vec<wicked_estate_core::Node> = Vec::new();
            for file in &changed_files {
                let nodes = store.nodes_in_file(file).map_err(to_any)?;
                all_nodes.extend(nodes);
            }
            if json_out {
                let j: Vec<serde_json::Value> = all_nodes
                    .iter()
                    .map(|n| serde_json::json!({
                        "name": n.name,
                        "kind": format!("{:?}", n.kind),
                        "file": n.location.file,
                        "line": n.location.span.start_line + 1,
                    }))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!(
                    "{} symbol(s) in {} changed file(s) since {sha}:",
                    all_nodes.len(),
                    changed_files.len()
                );
                for file in &changed_files {
                    println!("  {file}:");
                    for n in all_nodes.iter().filter(|n| n.location.file == *file) {
                        println!("    {:?} {}", n.kind, n.name);
                    }
                }
            }
        }
        // Agent E: entrypoints — symbols with no callers/importers.
        //
        // Usage:
        //   wicked-estate entrypoints [--json] [--db ...]
        "entrypoints" => {
            let json_out = positional.iter().any(|a| a == "--json");
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let nodes = store.entrypoint_nodes().map_err(to_any)?;
            if json_out {
                let j: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "file": n.location.file,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!("{} entrypoint(s) (no callers/importers):", nodes.len());
                for n in nodes.iter().take(50) {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
                if nodes.len() > 50 {
                    println!("  ... and {} more", nodes.len() - 50);
                }
            }
        }
        // Agent E: leaves — symbols that call/import nothing.
        //
        // Usage:
        //   wicked-estate leaves [--json] [--db ...]
        "leaves" => {
            let json_out = positional.iter().any(|a| a == "--json");
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let nodes = store.leaf_nodes().map_err(to_any)?;
            if json_out {
                let j: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "file": n.location.file,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!("{} leaf symbol(s) (no callees/imports):", nodes.len());
                for n in nodes.iter().take(50) {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
                if nodes.len() > 50 {
                    println!("  ... and {} more", nodes.len() - 50);
                }
            }
        }
        // Agent E: dead-code candidates — symbols with no edges at all.
        //
        // Usage:
        //   wicked-estate dead-code [--json] [--db ...]
        "dead-code" => {
            let json_out = positional.iter().any(|a| a == "--json");
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let nodes = store.isolated_nodes().map_err(to_any)?;
            if json_out {
                let j: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "file": n.location.file,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!(
                    "{} isolated symbol(s) (no in-edges AND no out-edges — dead code candidates):",
                    nodes.len()
                );
                for n in nodes.iter().take(50) {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
                if nodes.len() > 50 {
                    println!("  ... and {} more", nodes.len() - 50);
                }
            }
        }
        // Agent E: nodes — bulk export all symbols, optionally filtered by kind.
        //
        // Usage:
        //   wicked-estate nodes [--kind K] [--json] [--db ...]
        "nodes" => {
            let kind = {
                let mut k = String::new();
                let mut it2 = positional.iter();
                while let Some(a) = it2.next() {
                    if a.as_str() == "--kind" {
                        k = it2.next().cloned().unwrap_or_default();
                    }
                }
                k
            };
            let json_out = positional.iter().any(|a| a == "--json");
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let nodes = store.nodes_by_kind(&kind).map_err(to_any)?;
            if json_out {
                let j: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "file": n.location.file,
                            "line": n.location.span.start_line + 1,
                            "signature": n.signature,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                let label = if kind.is_empty() {
                    "all".to_string()
                } else {
                    kind.clone()
                };
                println!("{} node(s) of kind '{label}':", nodes.len());
                for n in nodes.iter().take(100) {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
                if nodes.len() > 100 {
                    println!("  ... and {} more", nodes.len() - 100);
                }
            }
        }
        _ => {
            println!("wicked-estate {} — usage:", env!("CARGO_PKG_VERSION"));
            println!(
                "  wicked-estate index <path>         [--db <file|:memory:>] [--history] [--embeddings]"
            );
            println!("    --history     opt-in to edge-history archival (default: off)");
            println!(
                "    --embeddings  compute and store embedding vectors after indexing (default: off)"
            );
            println!("  wicked-estate scip  <root>         [--db ...] [--scip-file <path>]");
            println!(
                "    Ingest a SCIP index (precise call resolution). Requires `wicked-estate index`"
            );
            println!(
                "    to have been run first. Auto-runs npx scip-typescript if index.scip absent."
            );
            println!(
                "  wicked-estate tfstate <file>        [--db ...]  # index live Terraform state"
            );
            println!(
                "  wicked-estate drift                 [--db ...]  # IaC vs live resource diff (W10)"
            );
            println!("  wicked-estate query <name>          [--db ...]");
            println!("  wicked-estate blast-radius <name>   [--db ...]");
            println!(
                "  wicked-estate rank                  [--db ...]  # most important symbols (PageRank)"
            );
            println!(
                "  wicked-estate stats                 [--db ...]  # includes git provenance if indexed"
            );
            println!(
                "  wicked-estate source <name>         [--db ...]  # print source slice(s) for symbol"
            );
            println!(
                "  wicked-estate semantic <query>      [--db ...]  # embedding-based symbol search (requires prior --embeddings)"
            );
            println!("  wicked-estate cross-graph <name>   --db <a.db> --db <b.db> ...");
            println!(
                "    (or --dbs a.db,b.db)  # federated search + blast-radius across repos (W12)"
            );
            println!("  wicked-estate compact              [--db <file>]  # prune cruft + VACUUM");
            println!("  wicked-estate watch <path>         [--db ...] [--history]");
            println!(
                "    Initial full index then reactive re-index on file changes (Ctrl-C to stop)."
            );
            println!("    --history  opt-in to edge-history archival for the watch session.");
            println!("  wicked-estate subscribe            [--db ...] [--since <seq>]");
            println!("    One-shot poll: print change-log entries since <seq> as JSON lines.");
            println!("    Each line: {{\"seq\":N,\"op\":\"upsert|remove\",\"target\":\"path\"}}");
            println!(
                "    Final line: {{\"next_seq\":N}} — pass as --since on the next call to resume."
            );
            println!(
                "  wicked-estate clusters [<min-size>] [--json]  # community detection on call graph"
            );
            println!(
                "  wicked-estate context <name> --budget <chars> [--json]  # ranked context within char budget"
            );
            println!("  wicked-estate annotate <name> --key K --value V [--db ...]");
            println!("    --key         annotation key (required)");
            println!("    --value       annotation value (required)");
            println!("    --confidence  confidence score 0.0–1.0 (default: 1.0)");
            println!("    --provenance  provenance string (default: empty)");
            println!("    --author      author string (default: empty)");
            println!("  wicked-estate annotations <name>   [--db ...]");
            println!("    Show all annotations for matching symbols.");
            println!("  wicked-estate fingerprint <name>   [--db ...]  # stable hex fingerprint for symbol");
            println!("  wicked-estate changed-since <sha>  [--json] [--db ...]  # symbols in files changed since git SHA");
            println!("  wicked-estate entrypoints [--json]            # symbols with no callers/importers");
            println!("  wicked-estate leaves      [--json]            # symbols that call/import nothing");
            println!("  wicked-estate dead-code   [--json]            # symbols with no edges at all");
            println!("  wicked-estate nodes [--kind K] [--json]       # bulk export all symbols by kind");
        }
    }
    Ok(())
}
