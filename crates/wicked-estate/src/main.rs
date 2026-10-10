//! `wicked-estate` — CLI over the indexing pipeline (`wicked_estate` lib).
//!
//!   wicked-estate index <path>           [--db <file|:memory:>] [--repo <name>] [--history] [--embeddings] [--force]
//!                                     `--repo <name>` (alias `--as`) co-locates MANY repos in ONE db:
//!                                     every path this run stores is namespaced `<name>/…`. Without it
//!                                     the behaviour is unchanged. Edges do NOT resolve across repos.
//!   wicked-estate scip  <root>           [--db ...] [--repo <name>] [--scip-file <path>]
//!   wicked-estate tfstate <file>         [--db ...]
//!   wicked-estate import-telemetry <file.json> [--db ...]
//!   wicked-estate drift                  [--db ...]
//!   wicked-estate query <name>           [--db ...]
//!   wicked-estate blast-radius <name>    [--depth N] [--json] [--db ...]
//!   wicked-estate supports owners        [--json] [--db ...]
//!   wicked-estate supports edge --source <ID> --target <ID> --kind <KIND> [--json] [--db ...]
//!   wicked-estate supports retract --producer <P> --snapshot <S> [--json] [--db ...]
//!                                     the authoritative edge-support plane (TS-S2A): who supports
//!                                     an edge, every owner's generation, and retracting an owner
//!   wicked-estate stats                  [--db ...]
//!   wicked-estate source [<name>]        [--cluster <id>] [--file <path>] [--symbols id1,id2,...]
//!                                     [--json] [--max-total-chars <N>] [--max-node-chars <N>]
//!                                     [--signatures-only] [--db ...]
//!   wicked-estate semantic <query>       [--db ...]
//!   wicked-estate cross-graph <name>     --db <a.db> --db <b.db> ...
//!                                     (or --dbs a.db,b.db,c.db)
//!   wicked-estate watch <path>           [--db ...] [--repo <name>] [--history]
//!   wicked-estate subscribe              [--db ...] [--since <seq>]
//!   wicked-estate clusters [<min_size>]  [--json] [--annotate] [--db ...]
//!   wicked-estate fingerprint <name>     [--content] [--db ...]
//!   wicked-estate changed-since <sha>    [--json] [--db ...]
//!   wicked-estate annotate <name>        --key K --value V [--type T] [--confidence F] [--provenance P] [--author A]
//!                                        [--source-type S] [--extraction-method M] [--last-verified now|<secs>|YYYY-MM-DD] [--db ...]
//!   wicked-estate annotate --symbol <id> --key K --value V [same flags] [--db ...]
//!   wicked-estate annotations <name>     [--type T] [--json] [--db ...]
//!   wicked-estate annotations --symbol <id> [--type T] [--json] [--db ...]
//!   wicked-estate stale-annotations <cutoff-unix-seconds | YYYY-MM-DD> [--json] [--db ...]
//!   wicked-estate stale-annotations --older-than <N>{s,m,h,d,w} [--json] [--db ...]
//!   wicked-estate context <name>         [--budget <chars>] [--json] [--db ...]
//!   wicked-estate entrypoints            [--json] [--db ...]
//!   wicked-estate leaves                 [--json] [--db ...]
//!   wicked-estate dead-code              [--json] [--db ...]
//!   wicked-estate nodes [--kind K] [--annotated-with K[=V]] [--json] [--semantics] [--db ...]
//!   wicked-estate graph-view [--limit N] [--include-tests] [--include-trivial] [--ignore <pat>] [--db ...]
//!
//! RetrievalTool-backed commands (see `tool_bridge::COMMANDS`; strict flags, `--help` per command):
//!   wicked-estate traverse <symbol>      [--depth N] [--direction D] [--edge-kinds a,b] [--max-nodes N] [--json] [--db ...]
//!   wicked-estate rank                   [--limit N] [--seeds s1,s2] [--json] [--db ...]   (alias: hotspots)
//!   wicked-estate rules-inventory        [--json] [--db ...]
//!   wicked-estate rules-recall           [--severity S] [--rule-type S] [--language S] [--layer S]
//!                                     [--framework S] [--scope S] [--projects a,b] [--limit N] [--json] [--db ...]

mod cli_flags;
mod cutoff;
mod emit;
mod scip_auto;
mod source_bundle;
mod tool_bridge;
mod watch_coalesce;

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
    // :memory: and URL-shaped specs (a `postgres://…` resolved by the WICKED_RUNTIME
    // profile seam, or an explicit `sqlite://` spec) are not filesystem paths — treating
    // them as one would create junk directories like `postgres:` in the CWD. The store
    // factory owns opening those; only a bare file path needs its parent created here.
    if db == ":memory:" || db.contains("://") {
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
fn maybe_print_staleness(store: &dyn wicked_estate_store::GraphStoreMutExt, db: &str) {
    for line in staleness_report(store, db).stale {
        println!("{line}");
    }
}

/// What can be said about a db's freshness, per indexed root.
#[derive(Debug, Default)]
struct StalenessReport {
    /// One line per root with commits since its last index.
    stale: Vec<String>,
    /// Roots whose freshness could NOT be determined (no git history at the root, a db spec
    /// with no file mtime, …), named by repo label or root path. The cause is not guessed.
    unknown: Vec<String>,
    /// Roots whose commits-behind count was read, stale or not.
    checked: usize,
    /// The worst count over every checked root — the one number machine output reports (R5).
    worst: Option<u64>,
}

impl StalenessReport {
    /// Exactly the freshness statements a caller should see (agent rule R5): a positive
    /// "current" claim only when EVERY indexed root was checked and none is behind — partial
    /// coverage presented as complete is the R3 failure.
    fn statements(&self, db: &str) -> Vec<String> {
        let mut out = self.stale.clone();
        for what in &self.unknown {
            out.push(format!(
                "STALENESS: unknown for {what} — freshness could not be determined"
            ));
        }
        if self.checked == 0 && self.unknown.is_empty() {
            out.push(format!(
                "STALENESS: unknown — {db} records no indexed root to check"
            ));
        } else if out.is_empty() {
            out.push("STALENESS: 0 commits since last index".to_string());
        }
        out
    }
}

/// Fail CLOSED on a graph that is not there, for every read-only frontend over a RetrievalTool
/// (`supports`, and each `tool_bridge` command, `lineage` included): opening a missing SQLite path creates an empty
/// one, and an empty graph answers every question with an honest-empty result — `rank` with an
/// empty ranking and exit 0, an exact id as "absent" — indistinguishable from the real answer
/// (the `index` arm's wicked-core#170 class). The store factory decides what a spec names:
/// `sqlite://<path>` is a file too, and a zero-length file is not a graph (SQLite would grow it
/// into an empty one). `:memory:` and non-file backends are left to the factory.
/// (#247) Write one line to stdout; a reader that went away (`| head -1`) is not an error —
/// the line is simply not wanted. Every other write error is returned.
fn print_line(line: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    match writeln!(out, "{line}").and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn require_existing_graph(db: &str, cmd: &str) -> anyhow::Result<()> {
    if let wicked_estate_store::StoreBackend::Sqlite { path } =
        wicked_estate_store::StoreBackend::parse(db)
    {
        let no_graph = std::fs::metadata(&path).map_or(true, |m| m.len() == 0);
        if path != ":memory:" && no_graph {
            anyhow::bail!(
                "no graph at {db} ({cmd} never creates one) — run \
                 `wicked-estate index <path> --db {db}` first, or pass the right --db"
            );
        }
    }
    Ok(())
}

/// Freshness of every indexed root in this db. Reads the indexed root(s) from store meta and
/// measures each from the commit it was indexed at (#243; the db file's mtime only as the
/// fallback for a graph that recorded no commit).
fn staleness_report(
    store: &dyn wicked_estate_store::GraphStoreMutExt,
    db: &str,
) -> StalenessReport {
    let mut report = StalenessReport::default();
    // A multi-repo graph has one root per repo — check each, and name the label in the fix so the
    // operator re-indexes THAT repo and not whichever one happened to be indexed last.
    let repos = wicked_estate::repo_scope::registry(store);
    if !repos.is_empty() {
        for rec in repos {
            match wicked_estate::commits_behind_since(
                &wicked_estate::recorded_root_path(&rec.root, db),
                rec.info.commit.as_deref(),
                db,
            ) {
                Some(n) => {
                    report.checked += 1;
                    report.worst = Some(report.worst.map_or(n, |w| w.max(n)));
                    if n > 0 {
                        report.stale.push(format!(
                            "STALENESS: {n} commit(s) in '{label}' since last index — run \
                             `wicked-estate index {root} --repo {label}` to refresh",
                            label = rec.label,
                            root = rec.root,
                        ));
                    }
                }
                None => report.unknown.push(format!("repo '{}'", rec.label)),
            }
        }
        return report;
    }
    // Never indexed: no root to check (`statements` says so).
    let Some(root) = wicked_estate::indexed_root_path(store, db) else {
        return report;
    };
    let root_str = root.to_string_lossy().into_owned();
    let baseline = store.repo_info().ok().flatten().and_then(|i| i.commit);
    match wicked_estate::commits_behind_since(&root, baseline.as_deref(), db) {
        Some(n) => {
            report.checked += 1;
            report.worst = Some(n);
            if n > 0 {
                report.stale.push(format!(
                    "STALENESS: {n} commit(s) since last index — run `wicked-estate index {root_str}` to refresh"
                ));
            }
        }
        None => report.unknown.push(root_str),
    }
    report
}

/// `stats --json` (#201, #198): the graph's identity and freshness as ONE document — the counts,
/// a `provenance` block (FULL commit SHA, branch, dirty, canonical `indexed_root`, `indexed_at`,
/// `indexed_version`, `id_scheme`, `graph_version`), one block per co-located repo under
/// `repos` (each with its own `commits_behind`), and `staleness.commits_behind` — the worst over
/// every checked root, `null` when none could be checked (`unknown` names them). An integrator
/// caching derived analysis keys it on `provenance.commit`; one that must know "is this answer
/// current" reads `staleness.commits_behind` here instead of parsing the human `STALENESS:` line.
fn stats_json(
    store: &dyn wicked_estate_store::GraphStoreMutExt,
    db: &str,
    s: &wicked_estate_core::GraphStats,
) -> serde_json::Value {
    let report = staleness_report(store, db);
    let meta = |k: &str| store.meta_get_key(k);
    let indexed = store.indexed_files().unwrap_or_default();
    let repos: Vec<serde_json::Value> = wicked_estate::repo_scope::registry(store)
        .iter()
        .map(|rec| {
            let prefix = wicked_estate::repo_scope::prefix(&rec.label);
            let files = indexed.iter().filter(|f| f.starts_with(&prefix)).count();
            let key =
                |name: &str| meta(&wicked_estate::repo_scope::meta_key(Some(&rec.label), name));
            serde_json::json!({
                "label": rec.label,
                "root": rec.root,
                "subpath": rec.subpath,
                "files": files,
                "commit": rec.info.commit,
                "branch": rec.info.branch,
                "remote": rec.info.remote,
                "dirty": rec.info.dirty,
                "indexed_version": key("indexed_version"),
                "id_scheme": key("id_scheme"),
                "commits_behind": wicked_estate::commits_behind_since(
                    &wicked_estate::recorded_root_path(&rec.root, db),
                    rec.info.commit.as_deref(),
                    db,
                ),
            })
        })
        .collect();
    let info = store.repo_info().ok().flatten();
    serde_json::json!({
        "nodes": s.node_count,
        "edges": s.edge_count,
        "files": s.file_count,
        "unresolved": s.unresolved_ref_count,
        "db_size_bytes": s.db_size_bytes,
        "nodes_by_kind": s.nodes_by_kind,
        "edges_by_kind": s.edges_by_kind,
        "provenance": {
            "commit": info.as_ref().and_then(|i| i.commit.clone()),
            "branch": info.as_ref().and_then(|i| i.branch.clone()),
            "remote": info.as_ref().and_then(|i| i.remote.clone()),
            "dirty": info.as_ref().map(|i| i.dirty),
            "indexed_root": wicked_estate::indexed_root_path(store, db)
                .map(|p| p.to_string_lossy().into_owned()),
            "indexed_at": meta("indexed_at"),
            "indexed_version": meta("indexed_version"),
            "id_scheme": meta("id_scheme"),
            "graph_version": meta("graph_version"),
        },
        "repos": repos,
        "staleness": {
            "commits_behind": report.worst,
            "checked": report.checked,
            "unknown": report.unknown,
        },
    })
}

/// Warn when the database was indexed under a different binary version or an older symbol-id
/// scheme. Extraction fixes (e.g. COBOL paragraph spans) are not backfilled — a re-index is
/// required. An id-scheme change churns definition ids: annotations/xedges keyed on the old ids
/// are NOT carried over.
fn maybe_warn_version_mismatch(store: &dyn wicked_estate_store::GraphStoreMutExt, db: &str) {
    let current = env!("CARGO_PKG_VERSION");
    let current_scheme = wicked_estate_extract::SYMBOL_ID_SCHEME;
    // A labelled repo records its binary version under `repo:<label>:indexed_version` and never
    // the bare key, so reading only the bare one made this warning silently unreachable on every
    // multi-repo graph — the graphs most likely to hold rows from a stale binary, since each repo
    // is re-indexed on its own schedule. Warn per repo, and name the label in the fix.
    let repos = wicked_estate::repo_scope::registry(store);
    if !repos.is_empty() {
        for rec in repos {
            let key = wicked_estate::repo_scope::meta_key(Some(&rec.label), "indexed_version");
            if let Some(indexed) = store.meta_get_key(&key) {
                if indexed != current {
                    eprintln!(
                        "VERSION MISMATCH: '{label}' in {db} was indexed with v{indexed}, current \
                         binary is v{current}. Re-index to apply extraction fixes: \
                         `wicked-estate index {root} --repo {label}`.",
                        label = rec.label,
                        root = rec.root,
                    );
                }
            }
            // The registry row is evidence the repo was indexed; an absent scheme key means the
            // implicit flat scheme "1".
            let scheme_key = wicked_estate::repo_scope::meta_key(Some(&rec.label), "id_scheme");
            let scheme = store
                .meta_get_key(&scheme_key)
                .unwrap_or_else(|| "1".to_string());
            if scheme != current_scheme {
                eprintln!(
                    "SYMBOL-ID SCHEME MISMATCH: '{label}' in {db} holds scheme-{scheme} ids; \
                     symbol ids under types changed — annotations/xedges keyed on old ids are NOT \
                     carried over. Re-index: `wicked-estate index {root} --repo {label}`.",
                    label = rec.label,
                    root = rec.root,
                );
            }
        }
        return;
    }
    // Un-labelled graph. Warn on the scheme only when something was actually indexed —
    // `indexed_files` is the same evidence the index-time gate keys on, and it covers
    // pre-version DBs (which have nodes + digests but no `indexed_version` key).
    let previously_indexed = store
        .indexed_files()
        .map(|f| !f.is_empty())
        .unwrap_or(false);
    if previously_indexed {
        let scheme = store
            .meta_get_key("id_scheme")
            .unwrap_or_else(|| "1".to_string());
        if scheme != current_scheme {
            let root_hint = store
                .meta_get_key("indexed_root")
                .unwrap_or_else(|| "<path>".to_string());
            eprintln!(
                "SYMBOL-ID SCHEME MISMATCH: {db} holds scheme-{scheme} ids; symbol ids under \
                 types changed — annotations/xedges keyed on old ids are NOT carried over. \
                 Re-index: `wicked-estate index {root_hint}`."
            );
        }
    }
    let indexed = match store.meta_get_key("indexed_version") {
        Some(v) => v,
        None => return, // pre-version database — no key stored yet
    };
    if indexed != current {
        let root_hint = store
            .meta_get_key("indexed_root")
            .unwrap_or_else(|| "<path>".to_string());
        eprintln!(
            "VERSION MISMATCH: {db} was indexed with v{indexed}, current binary is v{current}. \
             Re-index to apply extraction fixes: `wicked-estate index {root_hint}`."
        );
    }
}

/// The tail of a blast-radius CUT line: tell the user to raise `--depth`, or that they cannot.
fn raise_hint(depth: u32, max_depth: u32) -> String {
    if depth < max_depth {
        ", re-run with a larger --depth".to_string()
    } else {
        format!(" (--depth is already at its maximum of {max_depth})")
    }
}

fn loc(n: &wicked_estate_core::Node) -> String {
    format!("{}:{}", n.location.file, n.location.span.start_line + 1)
}

// ─── correspond helpers ───────────────────────────────────────────────────────

/// Split a symbol name into lowercase tokens on camelCase, snake_case, digits, and separators.
fn correspond_tokens(name: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            let prev_lower = i > 0 && chars[i - 1].is_lowercase();
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if c.is_uppercase() && !cur.is_empty() && (prev_lower || next_lower) {
                tokens.push(std::mem::take(&mut cur).to_lowercase());
            }
            cur.push(c);
        } else {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur).to_lowercase());
            }
        }
    }
    if !cur.is_empty() {
        tokens.push(cur.to_lowercase());
    }
    // Filter single-char noise and apply stop-prefix stripping.
    // CRUD verbs (create/read/update/delete/fetch/save/load/store) are KEPT.
    const STRIP_PREFIXES: &[&str] = &[
        "get", "set", "is", "has", "do", "on", "to", "from", "with", "make", "build",
    ];
    // Strip the first token if it is a stop-prefix and there are more tokens.
    let toks: Vec<String> = tokens.into_iter().filter(|t| t.len() > 1).collect();
    if toks.len() > 1 && STRIP_PREFIXES.contains(&toks[0].as_str()) {
        toks[1..].to_vec()
    } else {
        toks
    }
}

/// Jaccard coefficient of two token sets.
fn token_jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let sa: std::collections::HashSet<&String> = a.iter().collect();
    let sb: std::collections::HashSet<&String> = b.iter().collect();
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// Normalize a signature string: type normalization + lowercasing.
fn normalize_sig(sig: &str) -> Vec<String> {
    const TYPE_MAP: &[(&str, &str)] = &[
        ("string", "STR"),
        ("str", "STR"),
        ("varchar", "STR"),
        ("int", "INT"),
        ("i32", "INT"),
        ("i64", "INT"),
        ("long", "INT"),
        ("integer", "INT"),
        ("number", "INT"),
        ("bool", "BOOL"),
        ("boolean", "BOOL"),
        ("float", "FLOAT"),
        ("f32", "FLOAT"),
        ("f64", "FLOAT"),
        ("double", "FLOAT"),
        ("void", "VOID"),
        ("unit", "VOID"),
        ("none", "VOID"),
    ];
    sig.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && t.len() > 1)
        .map(|t| {
            let low = t.to_lowercase();
            TYPE_MAP
                .iter()
                .find(|(k, _)| *k == low)
                .map_or(low, |(_, v)| v.to_string())
        })
        .collect()
}

/// Approximate arity from a signature string (count commas at depth 1 in parens + 1).
fn arity_from_sig(sig: &str) -> Option<usize> {
    let inner = sig
        .find('(')
        .and_then(|s| sig.rfind(')').map(|e| &sig[s + 1..e]))?;
    if inner.trim().is_empty() {
        return Some(0);
    }
    let mut depth = 0usize;
    let mut commas = 0usize;
    for c in inner.chars() {
        match c {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    Some(commas + 1)
}

/// Kind-match score: 1.0 exact, partial for compatible kinds, 0.0 otherwise.
fn kind_match_score(a: &wicked_estate_core::NodeKind, b: &wicked_estate_core::NodeKind) -> f64 {
    use wicked_estate_core::NodeKind as K;
    match (a, b) {
        (K::Function, K::Function)
        | (K::Method, K::Method)
        | (K::Class, K::Class)
        | (K::Struct, K::Struct)
        | (K::Trait, K::Trait)
        | (K::Interface, K::Interface)
        | (K::Enum, K::Enum)
        | (K::Macro, K::Macro)
        | (K::Module, K::Module)
        | (K::Namespace, K::Namespace)
        | (K::Constructor, K::Constructor) => 1.0,
        (K::Function, K::Method)
        | (K::Method, K::Function)
        | (K::Constructor, K::Function)
        | (K::Function, K::Constructor) => 0.8,
        (K::Class, K::Struct)
        | (K::Struct, K::Class)
        | (K::Class, K::Trait)
        | (K::Trait, K::Class)
        | (K::Class, K::Interface)
        | (K::Interface, K::Class)
        | (K::Struct, K::Trait)
        | (K::Trait, K::Struct)
        | (K::Interface, K::Trait)
        | (K::Trait, K::Interface)
        | (K::Module, K::Namespace)
        | (K::Namespace, K::Module) => 0.6,
        _ => 0.0,
    }
}

/// Returns true for node kinds that are worth including in correspondence analysis.
fn is_correspond_kind(k: &wicked_estate_core::NodeKind) -> bool {
    !matches!(
        k,
        wicked_estate_core::NodeKind::File
            | wicked_estate_core::NodeKind::Import
            | wicked_estate_core::NodeKind::Variable
            | wicked_estate_core::NodeKind::Parameter
            | wicked_estate_core::NodeKind::Field
            | wicked_estate_core::NodeKind::Constant
            | wicked_estate_core::NodeKind::TypeAlias
            | wicked_estate_core::NodeKind::Synthetic
    )
}

/// Names so common they appear in nearly every codebase — suppress name-similarity weight.
const STOP_NAMES: &[&str] = &[
    "init",
    "new",
    "main",
    "run",
    "start",
    "stop",
    "handle",
    "parse",
    "serialize",
    "deserialize",
    "encode",
    "decode",
    "connect",
    "close",
    "open",
    "read",
    "write",
    "log",
    "info",
    "warn",
    "error",
    "debug",
    "setup",
    "teardown",
    "beforeeach",
    "aftereach",
];

fn emit_cli_span(
    sink: &std::sync::Arc<dyn wicked_estate_core::TelemetrySink>,
    resource: &wicked_estate_core::observability::Resource,
    scope: &wicked_estate_core::observability::InstrumentationScope,
    name: &str,
    attrs: Vec<wicked_estate_core::observability::KeyValue>,
    start_ns: u64,
    end_ns: u64,
) {
    use wicked_estate_core::observability::*;
    let span = SpanData {
        context: SpanContext {
            trace_id: TraceId::INVALID,
            span_id: SpanId::INVALID,
            trace_flags: 0,
            is_remote: false,
        },
        parent_span_id: None,
        name: name.to_string(),
        kind: SpanKind::Internal,
        start_time_unix_nano: start_ns,
        end_time_unix_nano: end_ns,
        attributes: attrs,
        events: vec![],
        links: vec![],
        status: SpanStatus::ok(),
    };
    if let Err(e) = sink.export_spans(resource, scope, &[span]) {
        eprintln!("telemetry: {e}");
    }
}

/// Returns `true` when `file` is in a vendored-dependency directory.
///
/// Covers the common vendor directory conventions across ecosystems:
///   `vendor/`, `_vendor/`, `third_party/`, `external/`, `extern/`, `deps/`
pub fn is_vendor_file(file: &str) -> bool {
    if file.is_empty() {
        return false;
    }
    const VENDOR_DIRS: &[&str] = &[
        "vendor",
        "_vendor",
        "third_party",
        "external",
        "extern",
        "deps",
    ];
    let lower = file.to_lowercase();
    let parts: Vec<&str> = lower.split('/').collect();
    parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|seg| VENDOR_DIRS.contains(seg))
}

/// Returns `true` when `name` is a trivial/generic symbol that dominates PageRank
/// with no useful graph signal — stdlib trait methods, universal constructors,
/// common functional combinators, etc.
///
/// Pass `--include-trivial` to `graph-view` to bypass this filter.
pub fn is_trivial_name(name: &str) -> bool {
    // Exact lowercase match against a curated cross-language set.
    // Rust stdlib traits + methods: covers Iterator, Option, Result, Clone, Hash, Ord, …
    // JS/TS: constructor, toString, …   Python: __init__, __str__, …
    const TRIVIAL: &[&str] = &[
        // Constructors / factory
        "new",
        "create",
        "build",
        "init",
        "default",
        "from",
        "into",
        "try_from",
        "try_into",
        "constructor",
        // Rust stdlib enum constructors / variants that leak into symbol tables
        "some",
        "none",
        "ok",
        "err",
        // Universal accessor pattern
        "get",
        "get_mut",
        "set",
        "put",
        // Rust Clone / Drop / Display / Debug / PartialEq / Hash / Ord
        "clone",
        "drop",
        "fmt",
        "eq",
        "ne",
        "lt",
        "le",
        "gt",
        "ge",
        "hash",
        "partial_cmp",
        "cmp",
        // Conversion / borrow
        "as_ref",
        "as_mut",
        "as_bytes",
        "as_str",
        "as_slice",
        "as_ptr",
        "as_mut_ptr",
        "borrow",
        "borrow_mut",
        "deref",
        "deref_mut",
        // String / byte helpers
        "to_string",
        "to_owned",
        "to_vec",
        "into_string",
        "into_bytes",
        "trim",
        "trim_start",
        "trim_end",
        "to_lowercase",
        "to_uppercase",
        "starts_with",
        "ends_with",
        "contains",
        "replace",
        "split",
        "join",
        "chars",
        "bytes",
        "len",
        "is_empty",
        // Iterator combinators (Rust + JS/TS + Python)
        "iter",
        "iter_mut",
        "into_iter",
        "next",
        "map",
        "flat_map",
        "filter",
        "filter_map",
        "fold",
        "reduce",
        "collect",
        "flatten",
        "chain",
        "zip",
        "enumerate",
        "any",
        "all",
        "find",
        "position",
        "count",
        "sum",
        "product",
        "min",
        "max",
        "min_by",
        "max_by",
        "take",
        "skip",
        // Option / Result combinators
        "map_err",
        "and_then",
        "or_else",
        "unwrap",
        "expect",
        "unwrap_or",
        "unwrap_or_else",
        "ok_or",
        "ok_or_else",
        "transpose",
        // Collection mutations
        "push",
        "pop",
        "insert",
        "remove",
        "retain",
        "clear",
        "extend",
        "append",
        "drain",
        "truncate",
        "reserve",
        // Async / IO helpers
        "poll",
        "await",
        "flush",
        "close",
        "read",
        "write",
        "seek",
        "send",
        "recv",
        "try_send",
        "try_recv",
        // Python dunder noise
        "__init__",
        "__str__",
        "__repr__",
        "__len__",
        "__eq__",
        "__hash__",
        "__iter__",
        "__next__",
        "__enter__",
        "__exit__",
        "__getitem__",
        "__setitem__",
        "__delitem__",
        "__contains__",
        // JS/TS universal
        "tostring",
        "valueof",
        "symbol_iterator",
    ];
    let lower = name.to_lowercase();
    TRIVIAL.contains(&lower.as_str())
}

/// Returns `true` when `file` (repo-relative) looks like a test file.
///
/// Covers ~100 languages via directory-segment matching (exact, no false positives on names like
/// "contest.ts") and filename conventions:
///   - Directories: `test/`, `tests/`, `spec/`, `specs/`, `__tests__/`, `e2e/`, `integration/`,
///     `acceptance/`, `fixtures/`, `testdata/`
///   - Prefix:  `test_*`  (Python, Rust, Elixir, Erlang)
///   - Suffix:  `*_test.ext` | `*_spec.ext` | `*_tests.ext` | `*_suite.ext`  (Go, Dart, …)
///   - Mid-ext: `*.test.ext` | `*.spec.ext`  (JS / TS / JSX / TSX)
///
/// Callers that want to include test symbols should skip this predicate (pass `--include-tests`
/// to `graph-view`).
pub fn is_test_file(file: &str) -> bool {
    if file.is_empty() {
        return false;
    }
    let lower = file.to_lowercase();
    let parts: Vec<&str> = lower.split('/').collect();
    let basename = parts.last().copied().unwrap_or("");

    // Exact directory-segment match — avoids "contest/" or "testutils/" false positives.
    const TEST_DIRS: &[&str] = &[
        "test",
        "tests",
        "spec",
        "specs",
        "__tests__",
        "e2e",
        "integration",
        "acceptance",
        "fixtures",
        "testdata",
    ];
    if parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|seg| TEST_DIRS.contains(seg))
    {
        return true;
    }

    // test_ prefix: Python (test_foo.py), Rust (test_utils.rs), Elixir (test_helper.exs), …
    if basename.starts_with("test_") {
        return true;
    }

    // *_test.ext | *_spec.ext | *_tests.ext | *_suite.ext
    // Splits at the last dot; checks what precedes it ends with the suffix.
    if let Some(dot) = basename.rfind('.') {
        let stem = &basename[..dot];
        if stem.ends_with("_test")
            || stem.ends_with("_spec")
            || stem.ends_with("_tests")
            || stem.ends_with("_suite")
        {
            return true;
        }
    }

    // *.test.ext | *.spec.ext  (JS/TS: foo.test.ts, foo.spec.tsx, …)
    basename.contains(".test.") || basename.contains(".spec.")
}

/// Returns `true` when `file` matches `pattern`.
///
/// Pattern rules (applied to the lowercased file path):
///   - No `*` → substring match: `"tests/"` matches any path containing `tests/`
///   - Leading `*` only → suffix/contains: `"*_test.go"` matches any path containing `_test.go`
///   - Trailing `*` only → prefix: `"src/generated/*"` matches paths starting with `src/generated/`
///   - Both → substring of the middle part after stripping prefix/suffix wildcards
fn matches_ignore_pattern(file: &str, pattern: &str) -> bool {
    let file_l = file.to_lowercase();
    let pat_l = pattern.to_lowercase();
    if !pat_l.contains('*') {
        return file_l.contains(&pat_l);
    }
    let stripped_start = pat_l.strip_prefix('*').unwrap_or(&pat_l);
    let stripped_both = stripped_start.strip_suffix('*').unwrap_or(stripped_start);
    if pat_l.starts_with('*') && pat_l.ends_with('*') {
        return file_l.contains(stripped_both);
    }
    if pat_l.starts_with('*') {
        return file_l.contains(stripped_start.trim_end_matches('*'));
    }
    if pat_l.ends_with('*') {
        return file_l.starts_with(stripped_both);
    }
    file_l.contains(&pat_l)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => ("help", &[][..]),
    };
    // (#200) The conventional version flag: one line, nothing else on stdout, exit 0. Before
    // the bridge and the flag check — a caller gating on the indexer version must not have to
    // scrape the usage banner's first line.
    if matches!(cmd, "--version" | "-V" | "version") {
        // Its row owns nothing, so `version foo` is a usage error rather than an ignored operand.
        // No store spec is resolved yet — and none is needed: the row has no store rule.
        cli_flags::parse("version", rest, "").map_err(anyhow::Error::msg)?;
        println!("wicked-estate {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // The DEFAULT spec resolves through the WICKED_RUNTIME profile seam
    // (docs/team-runtime.md): team → WICKED_STORE_URL (shared Postgres, needs a
    // `--features postgres` build) > WICKED_ESTATE_DB > the local graph.db. An explicit
    // `--db` flag below still overrides whatever resolves here.
    let mut db =
        wicked_estate_store::resolve_store_spec(None, ".wicked-estate/graph.db").map_err(to_any)?;
    let otel_sink = wicked_estate_observe::init_sink_from_env();
    let otel_resource = wicked_estate_core::observability::Resource::service(
        "wicked_estate",
        env!("CARGO_PKG_VERSION"),
    );
    let otel_scope = wicked_estate_core::observability::InstrumentationScope::versioned(
        "wicked_estate",
        env!("CARGO_PKG_VERSION"),
    );
    // RetrievalTool-backed commands parse their own argv, strictly, so they bypass the shared
    // parser below — which would swallow flags other commands own (see `tool_bridge`).
    if let Some(bridged) = tool_bridge::lookup(cmd) {
        return tool_bridge::run(bridged, rest, db, &|name, attrs, start, end| {
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                name,
                attrs,
                start,
                end,
            )
        });
    }
    // Every bespoke arm's flags, values, operands and repeats are checked against its
    // `cli_flags::COMMANDS` row before any I/O, and the arm reads the coerced result (#197,
    // #206, W8.6). There is no second, permissive parse: a default applies only when its flag
    // is absent. A help request, or a command no row names, goes to the usage arm.
    // `db` is the resolved default here, so a store rule sees the store this run would open.
    let (cmd, args) = match cli_flags::parse(cmd, rest, &db).map_err(anyhow::Error::msg)? {
        cli_flags::Parsed::Unlisted | cli_flags::Parsed::Help => ("help", cli_flags::Args::none()),
        // `supports` reads `--db` itself, strictly.
        cli_flags::Parsed::SelfParsing => (cmd, cli_flags::Args::none()),
        cli_flags::Parsed::Args(a) => {
            if a.owns("db") {
                if let Some(spec) = a.all(&["db"]).last() {
                    db = spec.to_string();
                }
            }
            (cmd, a)
        }
    };

    match cmd {
        "index" => {
            let path = args.operand(0).unwrap_or(".");
            let (history, embeddings, force_reindex) = (
                args.switch("history"),
                args.switch("embeddings"),
                args.switch("force"),
            );
            // Fail CLOSED on a path that is not there. Walking a missing directory yields zero files,
            // and reporting that as `indexed <path> → 0 nodes` with exit 0 makes every upstream path
            // bug look like an empty repository: the caller gets a real, queryable, EMPTY graph and a
            // success code. That is how a wrong `--db`/root goes unnoticed for months
            // (wicked-core#170) and how three runs indexed the wrong repo without anyone being told
            // (wicked-crew#196). "Indexed a repo with no code" and "was handed a path that does not
            // exist" are different answers and must have different exit codes.
            let target = Path::new(path);
            if !target.exists() {
                anyhow::bail!(
                    "index path does not exist: {path}\n\
                     (nothing was indexed; if you meant the current directory, pass `.` explicitly)"
                );
            }
            ensure_db_dir(&db)?;
            let as_repo = args.str("repo");
            // --force: invalidate the stored digests so index_path treats every file as changed —
            // but only THIS repo's, or forcing one repo would silently make every other repo in a
            // co-located graph re-extract from scratch on its next run.
            if force_reindex && db != ":memory:" {
                let mut concrete = SqliteStore::open(&db).map_err(to_any)?;
                let scope = as_repo.map(wicked_estate::repo_scope::prefix);
                concrete
                    .clear_file_digests_under(scope.as_deref())
                    .map_err(to_any)?;
            }
            // The row refuses --history/--embeddings on an in-memory store (`Cond::Store`).
            let stats = if history {
                // Caller explicitly opted in to history — open the concrete store to call
                // set_history_enabled(true) (inherent method, not on any trait), then box it.
                // Mirrors the `compact` arm pattern.
                let mut concrete = SqliteStore::open(&db).map_err(to_any)?;
                concrete.set_history_enabled(true).map_err(to_any)?;
                let mut store: Box<dyn GraphStoreMutExt> = Box::new(concrete);
                wicked_estate::index_path_as(store.as_mut(), Path::new(path), as_repo)
                    .map_err(to_any)?
            } else {
                // Default: history OFF (no-bloat-by-default).
                let mut store = open_store_ext(&db).map_err(to_any)?;
                wicked_estate::index_path_as(store.as_mut(), Path::new(path), as_repo)
                    .map_err(to_any)?
            };
            // The counts are the WHOLE graph's, which in a multi-repo db is every repo — say so
            // rather than let a labelled run read as if it had produced all of them itself.
            match as_repo {
                Some(label) => println!(
                    "indexed {path} as '{label}' ({db}) → graph now has {} nodes, {} edges, {} files",
                    stats.node_count, stats.edge_count, stats.file_count
                ),
                None => println!(
                    "indexed {path} ({db}) → {} nodes, {} edges, {} files",
                    stats.node_count, stats.edge_count, stats.file_count
                ),
            }
            for (k, v) in &stats.edges_by_kind {
                println!("  {k} = {v}");
            }
            // Coarse event: one `wicked.estate.indexed` per index run, through the shared seam.
            emit::emit_event(&emit::EmitEvent::new(
                "wicked.estate.indexed",
                "estate.index",
                serde_json::json!({
                    "path": path,
                    "db": db,
                    "repo": as_repo,
                    "nodes": stats.node_count,
                    "edges": stats.edge_count,
                    "files": stats.file_count,
                }),
            ));
            // W5.2: optional embeddings pass — OFF by default, opt-in with --embeddings.
            // Runs as a separate step so index_path's public signature is unchanged.
            // :memory: is skipped (embeddings live in the same store; nothing to persist).
            if embeddings {
                let mut emb_store = SqliteStore::open(&db).map_err(to_any)?;
                let embedder = wicked_estate::default_embedder();
                let n = wicked_estate::compute_embeddings(&mut emb_store, &*embedder)
                    .map_err(to_any)?;
                println!("embedded {n} symbols");
            }
        }
        "scip" => {
            let root_str = args.operand(0).unwrap_or(".");
            let root = Path::new(root_str);
            ensure_db_dir(&db)?;
            let as_repo = args.str("repo");
            // SCIP paths are repo-relative; a labelled graph's nodes are not. Correlating the two
            // without knowing which repo this index belongs to matches nothing and reports "0
            // precise edges" — refuse instead of ingesting silence.
            {
                let probe = open_store_ext(&db).map_err(to_any)?;
                let known = wicked_estate::repo_scope::labels(probe.as_ref());
                if !known.is_empty() {
                    match as_repo {
                        None => anyhow::bail!(
                            "REPO COLLISION: {db} holds {n} labelled repo(s) [{list}] — say which \
                             one this SCIP index belongs to: `wicked-estate scip {root_str} --db \
                             {db} --repo <name>`",
                            n = known.len(),
                            list = known.join(", "),
                        ),
                        Some(l) if !known.iter().any(|k| k == l) => anyhow::bail!(
                            "unknown repo label '{l}' in {db} — this graph holds [{list}]",
                            list = known.join(", "),
                        ),
                        Some(_) => {}
                    }
                } else if let Some(l) = as_repo {
                    anyhow::bail!(
                        "--repo {l} was given but {db} is a single-repo graph (no labelled repos) \
                         — drop the flag, or index the repos with `--repo` first"
                    );
                }
            }

            if let Some(explicit) = args.str("scip-file") {
                let scip_path = Path::new(explicit);
                let mut store = open_store_ext(&db).map_err(to_any)?;
                let count = wicked_estate::ingest_scip_as(store.as_mut(), root, scip_path, as_repo)
                    .map_err(to_any)?;
                println!(
                    "scip (explicit): ingested {count} precise edge(s) from {explicit} into {db}"
                );
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
                    wicked_estate::ingest_scip_as(store.as_mut(), root, &result.path, as_repo)
                        .map_err(to_any)?;
                let path_display = result.path.display();
                println!(
                    "scip ({}): ingested {count} precise edge(s) from {path_display} into {db}",
                    result.lang
                );
            }
        }
        // Task B: ingest a Terraform state file (live resource nodes → estate LIVE side).
        "tfstate" => {
            let file_path = args.required(0);
            let json = std::fs::read_to_string(file_path)
                .with_context(|| format!("cannot read tfstate file '{file_path}'"))?;
            ensure_db_dir(&db)?;
            let mut store = open_store_ext(&db).map_err(to_any)?;
            let n = wicked_estate::ingest_tfstate(store.as_mut(), &json).map_err(to_any)?;
            println!("tfstate: upserted {n} live resource node(s) from '{file_path}' into {db}");
        }
        // Brain consolidation: bulk-import access_log + search_misses telemetry from a JSON file
        // produced by the brain-side export tool. The file shape is `TelemetryImport`
        // (`{ "access_log": [...], "search_misses": [...] }`, both arrays optional). Point `--db`
        // at the target SQLite store file (the knowledge db for knowledge telemetry; both signals
        // are opaque id/query strings so any SQLite store file works — graph or knowledge db).
        // SQLite-only today: the telemetry tables live in schema.sql and the import APIs are
        // SqliteStore methods, so non-SQLite specs fail fast instead of silently creating a junk
        // file named after the connection URL. Additive: never touches nodes/edges.
        "import-telemetry" => {
            let file_path = args.required(0);
            let json = std::fs::read_to_string(file_path)
                .with_context(|| format!("cannot read telemetry file '{file_path}'"))?;
            let payload: wicked_estate_store::TelemetryImport = serde_json::from_str(&json)
                .with_context(|| format!("invalid telemetry JSON in '{file_path}'"))?;
            // Parse the spec through the one store seam so `sqlite://<path>` and `:memory:`
            // behave like everywhere else, and non-SQLite backends get a clear error.
            let sqlite_path = match wicked_estate_store::StoreBackend::parse(&db) {
                wicked_estate_store::StoreBackend::Sqlite { path } => path,
                other => anyhow::bail!(
                    "import-telemetry is SQLite-only today (the telemetry tables live in the \
                     SQLite schema); got a non-SQLite store spec: {other:?}"
                ),
            };
            ensure_db_dir(&sqlite_path)?;
            let mut store = SqliteStore::open(&sqlite_path).map_err(to_any)?;
            let a = store
                .import_access_log(&payload.access_log)
                .map_err(to_any)?;
            let m = store
                .import_search_misses(&payload.search_misses)
                .map_err(to_any)?;
            println!(
                "import-telemetry: imported {a} access-log row(s), {m} search-miss(es) into {db}"
            );
        }
        // Task C: W10 drift report.
        "drift" => {
            let store = open_store(&db).map_err(to_any)?;
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let report = wicked_estate::estate_drift(&*store).map_err(to_any)?;
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
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
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.drift",
                vec![
                    wicked_estate_core::observability::KeyValue::int(
                        "drift.added",
                        report.unmanaged.len() as i64,
                    ),
                    wicked_estate_core::observability::KeyValue::int(
                        "drift.removed",
                        report.undeployed.len() as i64,
                    ),
                    wicked_estate_core::observability::KeyValue::int(
                        "drift.changed",
                        report.managed.len() as i64,
                    ),
                ],
                t_cmd_start,
                t_cmd_end,
            );
            // Coarse event: one `wicked.estate.drifted` per drift run, through the shared seam.
            // (Distinct from the OTel span above — that is telemetry; this is a bus event.)
            emit::emit_event(&emit::EmitEvent::new(
                "wicked.estate.drifted",
                "estate.drift",
                serde_json::json!({
                    "db": db,
                    "managed": report.managed.len(),
                    "undeployed": report.undeployed.len(),
                    "unmanaged": report.unmanaged.len(),
                }),
            ));
        }
        "query" => {
            let name = args.required(0);
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "query")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            maybe_warn_version_mismatch(store.as_ref(), &db);
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            if args.switch("json") {
                // (#199) The machine shape is `resolve --json`'s: one row per hit,
                // `{symbol_id,name,kind,file,line}` — the same keys, so a parser written against
                // either command reads the other.
                let rows: Vec<serde_json::Value> = hits.iter().map(resolve_row).collect();
                print_line(&serde_json::to_string_pretty(&rows)?)?;
            } else {
                print_line(&format!("{} match(es) for '{name}':", hits.len()))?;
                for n in &hits {
                    print_line(&format!("  {:?} {} ({})", n.kind, n.name, loc(n)))?;
                }
            }
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.query",
                vec![
                    wicked_estate_core::observability::KeyValue::str("symbol.name", name),
                    wicked_estate_core::observability::KeyValue::int(
                        "result.count",
                        hits.len() as i64,
                    ),
                ],
                t_cmd_start,
                t_cmd_end,
            );
        }
        "blast-radius" => {
            let json_out = args.switch("json");
            // `--depth N` (wicked-estate#190). DEFAULT 12 — the previously hardcoded horizon, so
            // existing invocations behave identically; the difference is that a cut at 12 is now
            // REPORTED instead of silent, and a deep estate chain can be followed by raising it.
            // The row bounds it by the MCP BlastRadius ceiling: an unbounded value hangs on a
            // cyclic graph (the recursive walk grows with depth), so it is refused, not clamped.
            let depth: u32 = args.u32("depth").unwrap_or(12);
            let max_depth = wicked_estate_retrieve::BLAST_DEPTH_CEILING;
            let name = args.required(0);
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "blast-radius")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            // Machine output must be exactly one JSON document — notices would corrupt it.
            if !json_out {
                maybe_print_staleness(store.as_ref(), &db);
                maybe_warn_version_mismatch(store.as_ref(), &db);
            }
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let br = wicked_estate::blast_radius_by_name(&*store, name, depth).map_err(to_any)?;
            let deps = &br.dependents;
            let unresolved = store.unresolved_refs_for_name(name).map_err(to_any)?.len();
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            // Bound the row count so the payload stays parseable by machine consumers: crew
            // reads this via execCapped, where an oversized payload is TRUNCATED mid-document
            // and JSON.parse throws. 25K chars (the R4 budget) with an ADDITIVE
            // `truncated_dependents` count — crew reads only `dependents`/`unresolved`.
            let (kept, dropped) = cap_blast_radius_rows(name, unresolved, &br, depth);
            if json_out {
                // Machine consumers (wicked-crew studio) get the same honesty contract as the
                // text path: dependents PLUS the unresolved count — absence of dependents must
                // never silently read as "safe to change".
                let out = blast_radius_json(name, &deps[..kept], dropped, unresolved, &br, depth);
                print_line(&serde_json::to_string(&out).map_err(|e| anyhow::anyhow!(e))?)?;
            } else if deps.is_empty() {
                print_line(&format!(
                    "no resolved dependents for '{name}' (symbol may not be indexed)"
                ))?;
            } else {
                print_line(&format!("{} symbol(s) depend on '{name}':", deps.len()))?;
                for n in deps.iter().take(kept) {
                    print_line(&format!("  {:?} {} ({})", n.kind, n.name, loc(n)))?;
                }
                if dropped > 0 {
                    print_line(&format!(
                        "  …and {dropped} more (output bounded at 25K chars)"
                    ))?;
                }
            }
            // Honest coverage — never let the absence of dependents read as "safe to change".
            // `unresolved` counts per-site unresolved references, defined once in
            // docs/ENGINE-CONTRACT.md §2.1. The depth/node cut is reported on the SAME line a
            // human reads for completeness (wicked-estate#190) — a cap the honesty line does not
            // cover is worse than having no honesty line (agent rule R3).
            if !json_out {
                let cut = match (br.depth_horizon_reached, br.node_cap_reached) {
                    (true, true) => format!(
                        "; CUT AT depth={depth} AND by the traversal node budget — more \
                         dependents exist{}",
                        raise_hint(depth, max_depth)
                    ),
                    (true, false) => format!(
                        "; CUT AT depth={depth} — more dependents exist beyond {depth} hops{}",
                        raise_hint(depth, max_depth)
                    ),
                    (false, true) => {
                        "; CUT by the traversal node budget — more dependents exist".to_string()
                    }
                    (false, false) => String::new(),
                };
                print_line(&format!(
                    "coverage: {} resolved dependent(s) within depth {depth}; {unresolved} \
                     unresolved call(s) reference '{name}' — best-effort static resolution, MAY \
                     be incomplete (precise tier pending){cut}",
                    deps.len()
                ))?;
                // How much to believe the rows above (wicked-estate#194), on the line after the
                // completeness line a human already reads.
                print_line(&format!("evidence: {}", evidence_text(&br.confidence)))?;
            }
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.blast_radius",
                vec![
                    wicked_estate_core::observability::KeyValue::str("symbol.name", name),
                    wicked_estate_core::observability::KeyValue::int(
                        "dependent.count",
                        deps.len() as i64,
                    ),
                ],
                t_cmd_start,
                t_cmd_end,
            );
        }
        // ── path ────────────────────────────────────────────────────────────
        //   wicked-estate path <from> <to> [--max-depth N] [--json]
        //
        // `cli_flags` classifies every token before either operand is read, so `path A --json`
        // is one operand (a usage error), never `to = "--json"`.
        "path" => {
            const DEFAULT_MAX_DEPTH: u32 = 12; // matches `blast-radius`
            const MAX_MAX_DEPTH: u32 = 16;
            const CLI_MAX_NODES: usize = 5_000;
            let json_out = args.switch("json");
            // The row refuses 0 and non-integers; above 16 clamps, as documented.
            let max_depth = args
                .u32("max-depth")
                .map_or(DEFAULT_MAX_DEPTH, |d| d.min(MAX_MAX_DEPTH));
            let (from, to) = (args.required(0), args.required(1));

            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "path")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            // Machine output must be exactly one JSON document — notices would corrupt it.
            if !json_out {
                maybe_print_staleness(store.as_ref(), &db);
                maybe_warn_version_mismatch(store.as_ref(), &db);
            }
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let result =
                wicked_estate_core::path_between(&*store, from, to, max_depth, CLI_MAX_NODES)
                    .map_err(to_any)?;
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;

            if json_out {
                let doc = path_json(from, to, &result);
                println!(
                    "{}",
                    serde_json::to_string(&doc).map_err(|e| anyhow::anyhow!(e))?
                );
            } else {
                let mut out = std::io::stdout().lock();
                write_path_text(&mut out, from, to, &result, max_depth, CLI_MAX_NODES)
                    .map_err(|e| anyhow::anyhow!(e))?;
            }
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.path",
                vec![
                    wicked_estate_core::observability::KeyValue::str("path.from", from),
                    wicked_estate_core::observability::KeyValue::str("path.to", to),
                    wicked_estate_core::observability::KeyValue::int(
                        "path.hops",
                        result.hops.len() as i64,
                    ),
                ],
                t_cmd_start,
                t_cmd_end,
            );
        }
        // ── supports ────────────────────────────────────────────────────────
        //   wicked-estate supports owners        [--json]
        //   wicked-estate supports edge --source <ID> --target <ID> --kind <KIND> [--json]
        //   wicked-estate supports retract --producer <P> --snapshot <S> [--json]
        //
        // The CLI face of the TS-S2A support plane (`docs/ENGINE-CONTRACT.md` §3.4): the same
        // `GraphRead::{support_owners, edge_supports}` / `GraphWrite::replace_edge_supports` every
        // store implements — no second implementation. `retract` is `replace_edge_supports(owner,
        // generation + 1, [])`, the documented way to clear an owner (e.g. before a downgrade).
        // Every subcommand refuses a missing/empty graph instead of creating one.
        "supports" => {
            let args = parse_supports_args(rest)?;
            if let Some(spec) = &args.db {
                db = spec.clone();
            }
            require_existing_graph(&db, "supports")?;
            let mut store = open_store_ext(&db).map_err(to_any)?;
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let doc = run_supports(store.as_mut(), &args)?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&doc).map_err(|e| anyhow::anyhow!(e))?
                );
            } else {
                let mut out = std::io::stdout().lock();
                write_supports_text(&mut out, &doc).map_err(|e| anyhow::anyhow!(e))?;
            }
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.supports",
                vec![wicked_estate_core::observability::KeyValue::str(
                    "supports.mode",
                    args.mode.name(),
                )],
                t_cmd_start,
                t_cmd_end,
            );
        }
        "stats" => {
            let json_out = args.switch("json");
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "stats")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            // Machine output is exactly one JSON document (#198): the freshness notice lives
            // INSIDE it (`staleness`), never as a line beside it.
            if !json_out {
                maybe_print_staleness(store.as_ref(), &db);
            }
            maybe_warn_version_mismatch(store.as_ref(), &db);
            let s = store.stats().map_err(to_any)?;
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&stats_json(store.as_ref(), &db, &s))?
                );
                return Ok(());
            }
            let db_mb = s.db_size_bytes as f64 / 1_048_576.0;
            println!(
                "nodes={} edges={} files={} unresolved={} db={:.1}MB",
                s.node_count, s.edge_count, s.file_count, s.unresolved_ref_count, db_mb
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
            // Multi-repo graph: one provenance block per repo. `repo_info()` is None here by
            // construction — a labelled index never writes the singular repo_* keys — so this is
            // the only place a co-located graph's provenance is reported.
            let repos = wicked_estate::repo_scope::registry(store.as_ref());
            if !repos.is_empty() {
                let indexed = store.indexed_files().unwrap_or_default();
                println!("repos ({}):", repos.len());
                for rec in &repos {
                    let prefix = wicked_estate::repo_scope::prefix(&rec.label);
                    let files = indexed.iter().filter(|f| f.starts_with(&prefix)).count();
                    print!("  {label}  files={files}", label = rec.label);
                    if let Some(c) = &rec.info.commit {
                        print!("  commit={}", &c[..8.min(c.len())]);
                    }
                    if let Some(b) = &rec.info.branch {
                        print!("  branch={b}");
                    }
                    if rec.info.dirty {
                        print!("  dirty");
                    }
                    println!("  root={}", rec.root);
                }
                println!("  (co-located, not linked: edges do not resolve across repos)");
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
        // Graph view for UI consumption — top-N code symbols by PageRank + inter-symbol edges.
        //
        //   wicked-estate graph-view [--limit N] [--include-tests] [--include-trivial]
        //                            [--ignore <pat>] [--db <file>]
        //
        // Returns JSON { nodes: [...], edges: [...] } to stdout.
        // Smart defaults (all opt-out):
        //   test files hidden    → pass --include-tests to restore
        //   trivial names hidden → pass --include-trivial to restore (get, new, len, map_err, …)
        //   vendor dirs hidden   → always filtered; use --ignore to add more
        //   --ignore <pat>       exclude additional file paths (repeatable; substring or *glob*)
        // Uses open_store_ext so overlay/injected cross-repo edges are included.
        "graph-view" => {
            use std::collections::HashSet;
            use wicked_estate_core::{Direction, EdgeKind, NodeKind};

            let limit = args.usize("limit").unwrap_or(80);
            let include_tests = args.switch("include-tests");
            let include_trivial = args.switch("include-trivial");
            let focus: Option<String> = args.str("focus").map(str::to_string);
            // Repeatable by design: every pattern applies.
            let ignore_patterns: Vec<String> = args
                .all(&["ignore"])
                .into_iter()
                .map(str::to_string)
                .collect();

            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "graph-view")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            // Oversample PageRank candidates so filters don't under-deliver.
            // Fetch 4× the requested limit (at least limit+200) so that after
            // removing tests, trivials, external, and vendor nodes we still
            // have `limit` meaningful symbols to return.
            let fetch_limit = (limit * 4).max(limit + 200);
            let top =
                wicked_estate::important_symbols(store.as_ref(), fetch_limit).map_err(to_any)?;

            // Exclude structural-only and rules-engine kinds; keep code-bearing kinds.
            // Namespace/Synthetic/Rule*/Condition/Action/Fact are not user code symbols.
            // File and Import stay listed even though important_symbols never returns them
            // (the ranked_symbols seam filters live results and the cache-read path cleans
            // stale pre-upgrade caches — Decision H / BR-1): `passes` is ALSO the BFS
            // expansion gate below, and File nodes enter the frontier via file-scope Calls
            // edges, then pull Import nodes and more Files through Imports edges
            // (round-1 R1-CORR-2).
            let excluded = [
                NodeKind::File,
                NodeKind::Import,
                NodeKind::Module,
                NodeKind::Namespace,
                NodeKind::Constant,
                NodeKind::Variable,
                NodeKind::Field,
                NodeKind::Parameter,
                NodeKind::Synthetic,
                NodeKind::Rule,
                NodeKind::RuleSet,
                NodeKind::Condition,
                NodeKind::Action,
                NodeKind::Fact,
            ];

            // Shared node filter (kind exclusion + external/vendor/test/trivial/ignore).
            let passes = |n: &wicked_estate_core::Node| -> bool {
                if excluded.contains(&n.kind) {
                    return false;
                }
                let file = &n.location.file;
                if file.is_empty() || file.starts_with('/') {
                    return false;
                }
                if file.starts_with("node_modules/") || is_vendor_file(file) {
                    return false;
                }
                if !include_tests && is_test_file(file) {
                    return false;
                }
                if !include_trivial && is_trivial_name(&n.name) {
                    return false;
                }
                if ignore_patterns
                    .iter()
                    .any(|p| matches_ignore_pattern(file, p))
                {
                    return false;
                }
                true
            };

            let ranked: Vec<&(wicked_estate_core::Node, f32)> =
                top.iter().filter(|(n, _)| passes(n)).collect();
            let rank_of: std::collections::HashMap<String, f32> = ranked
                .iter()
                .map(|(n, s)| (n.symbol.as_str().to_string(), *s))
                .collect();

            // CONNECTED SLICE: a plain top-N-by-PageRank slice renders as scattered islands —
            // the globally most-important symbols in a large graph are rarely each other's
            // neighbours, so almost no edges fall within the set. Seed with the top-ranked
            // core, then EXPAND along Calls/Imports edges (both directions, same filters,
            // breadth-first, capped per node) until `limit`, so the returned slice is a
            // readable neighbourhood. Backfill from the ranking if expansion runs dry.
            if limit == 0 {
                // `--limit 0` is a valid ask for an empty slice — and `.clamp(1, 0)` panics.
                println!("{}", serde_json::json!({ "nodes": [], "edges": [] }));
                return Ok(());
            }
            let mut selected: Vec<wicked_estate_core::Node> = Vec::new();
            let mut sel_ids: HashSet<String> = HashSet::new();
            if let Some(f) = &focus {
                // FOCUS (ego-graph) mode — the navigation primitive: seed with ONE symbol
                // (exact SymbolId, else name matches, capped) and expand its neighbourhood.
                // The focus seeds bypass the display filters (you asked for this node);
                // filters still gate what the expansion pulls in.
                let sid: wicked_estate_core::SymbolId = f.clone().into();
                let mut seeds: Vec<wicked_estate_core::Node> = Vec::new();
                if let Ok(Some(n)) = store.get_node(&sid) {
                    seeds.push(n);
                } else {
                    let q = wicked_estate_core::SymbolQuery {
                        exact_name: Some(f.clone()),
                        ..Default::default()
                    };
                    // TS-S1: exact-SymbolId focus above is deliberate; focusing by NAME is not a
                    // way back into synthetic value slots — a bare name never resolves to one.
                    // Filter BEFORE capping at 5: with the cap in the query, five same-name
                    // slots fill the window and the real symbol is never seen. An exact-name
                    // lookup is bounded by that name's cardinality, so the full fetch is cheap.
                    seeds.extend(
                        store
                            .find_symbols(&q)
                            .map_err(to_any)?
                            .into_iter()
                            .filter(wicked_estate_core::is_structural_symbol)
                            .take(5),
                    );
                }
                if seeds.is_empty() {
                    anyhow::bail!("graph-view --focus: no symbol matches '{f}'");
                }
                for n in seeds.into_iter().take(limit) {
                    if sel_ids.insert(n.symbol.as_str().to_string()) {
                        selected.push(n);
                    }
                }
            } else {
                let seed_count = (limit / 3).clamp(1, limit);
                for (n, _) in ranked.iter().take(seed_count) {
                    if sel_ids.insert(n.symbol.as_str().to_string()) {
                        selected.push((*n).clone());
                    }
                }
            }
            let mut frontier: Vec<wicked_estate_core::SymbolId> =
                selected.iter().map(|n| n.symbol.clone()).collect();
            while selected.len() < limit && !frontier.is_empty() {
                let mut next: Vec<wicked_estate_core::SymbolId> = Vec::new();
                'expand: for sym in &frontier {
                    // One budget across BOTH directions — 6 expansions per frontier node total.
                    let mut taken = 0usize;
                    for dir in [Direction::Dependencies, Direction::Dependents] {
                        let nbrs = store.neighbors(sym, dir).map_err(to_any)?;
                        for e in nbrs
                            .iter()
                            .filter(|e| matches!(e.kind, EdgeKind::Calls | EdgeKind::Imports))
                        {
                            if taken >= 6 {
                                break;
                            }
                            let other = if matches!(dir, Direction::Dependencies) {
                                &e.target
                            } else {
                                &e.source
                            };
                            if sel_ids.contains(other.as_str()) {
                                continue;
                            }
                            let Ok(Some(n)) = store.get_node(other) else {
                                continue;
                            };
                            if !passes(&n) {
                                continue;
                            }
                            sel_ids.insert(other.as_str().to_string());
                            selected.push(n);
                            next.push(other.clone());
                            taken += 1;
                            if selected.len() >= limit {
                                break 'expand;
                            }
                        }
                    }
                }
                frontier = next;
            }
            if focus.is_none() {
                for (n, _) in ranked.iter() {
                    if selected.len() >= limit {
                        break;
                    }
                    if sel_ids.insert(n.symbol.as_str().to_string()) {
                        selected.push((*n).clone());
                    }
                }
            }

            let node_ids: HashSet<&str> = selected.iter().map(|n| n.symbol.as_str()).collect();

            // Single-pass: collect outgoing edges, out-degree, and in-degree simultaneously.
            // out_deg_map[X] = number of Calls/Imports edges leaving X (full graph, from Dependencies).
            // in_deg_map[Y]  = number of Calls/Imports edges from top-N nodes pointing to Y
            //                  (within-set in-degree, appropriate for layout sizing in the UI).
            //                  It counts edges, so it equals Y's in-set rows in `edges` below.
            // This halves store calls vs. a separate per-node Dependents query per node.
            let mut in_set_edges: Vec<wicked_estate_core::Edge> = Vec::new();
            let mut out_deg_map: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut in_deg_map: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            for node in &selected {
                let nbrs = store
                    .neighbors(&node.symbol, Direction::Dependencies)
                    .map_err(to_any)?;
                let out_deg = nbrs
                    .iter()
                    .filter(|e| matches!(e.kind, EdgeKind::Calls | EdgeKind::Imports))
                    .count();
                out_deg_map.insert(node.symbol.as_str().to_string(), out_deg);

                for e in &nbrs {
                    if matches!(e.kind, EdgeKind::Calls | EdgeKind::Imports)
                        && node_ids.contains(e.target.as_str())
                    {
                        *in_deg_map.entry(e.target.as_str().to_string()).or_insert(0) += 1;
                        in_set_edges.push(e.clone());
                    }
                }
            }
            let edges_json = graph_view_edge_rows(&in_set_edges);

            let nodes_json: Vec<serde_json::Value> = selected
                .iter()
                .map(|n| {
                    let in_deg = in_deg_map.get(n.symbol.as_str()).copied().unwrap_or(0);
                    let out_deg = out_deg_map.get(n.symbol.as_str()).copied().unwrap_or(0);
                    serde_json::json!({
                        "id":     n.symbol.as_str(),
                        "name":   n.name,
                        "kind":   &n.kind,
                        "file":   n.location.file,
                        "lang":   n.language.as_str(),
                        // Expansion nodes are unranked → 0.0 (sizing treats them as leaf-weight).
                        "score":  rank_of.get(n.symbol.as_str()).copied().unwrap_or(0.0),
                        "inDeg":  in_deg,
                        "outDeg": out_deg,
                    })
                })
                .collect();

            let out = serde_json::to_string(&serde_json::json!({
                "nodes": nodes_json,
                "edges": edges_json,
            }))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!("{out}");
        }
        // Bulk SOURCE bundle — full bodies for an entire file / cluster / symbol-set in one call.
        //
        //   wicked-estate source [<name>] [--cluster <id>] [--file <path>] [--symbols id1,id2,...]
        //       [--json] [--max-total-chars <N>] [--max-node-chars <N>] [--signatures-only] [--db ...]
        //
        // Selectors (exactly one; precedence --symbols > --cluster > --file > <name>):
        //   --symbols  exactly those SymbolIds
        //   --cluster  members of that community (index into detect_communities, largest-first)
        //   --file     all nodes whose location.file == path
        //   <name>     fuzzy match (the legacy text behaviour)
        //
        // Non-`--json` `source <name>` behaviour is unchanged. `--json` emits a bundle object;
        // omitted budget = UNBOUNDED (the caller owns its context). This path is a pure READ —
        // it never opens the read-write `open_store_ext` dance the `clusters` arm uses.
        "source" => {
            let json_out = args.switch("json");
            let signatures_only = args.switch("signatures-only");
            let name = args.operand(0);
            // The row requires <name> or a selector, and --json for the budgets (#206), so a
            // usage error never reaches the store: opening a missing SQLite path creates it.
            let src_symbols = args.list("symbols");
            let src_cluster = args.usize("cluster");
            let src_file = args.str("file");
            let src_max_total = args.usize("max-total-chars");
            let src_max_node = args.usize("max-node-chars");
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "source")?;
            let store = open_store(&db).map_err(to_any)?;

            // Resolve the selector — one rule for both output modes (#206: the text path used to
            // drop every selector but <name>). Precedence: --symbols > --cluster > --file > <name>.
            let (nodes, selector): (Vec<wicked_estate_core::Node>, serde_json::Value) =
                if let Some(ids) = src_symbols {
                    let mut out = Vec::new();
                    for id in ids {
                        let sid = wicked_estate_core::symbol::SymbolId::from(id.as_str());
                        if let Some(n) = store.get_node(&sid).map_err(to_any)? {
                            out.push(n);
                        }
                    }
                    (out, serde_json::json!({ "symbols": ids }))
                } else if let Some(cid) = src_cluster {
                    // Members of community `cid` (index into detect_communities, largest-first).
                    let params = wicked_estate_rank::CommunityParams::default();
                    let communities =
                        wicked_estate_rank::detect_communities(&*store, &params).map_err(to_any)?;
                    let members = communities.get(cid).cloned().unwrap_or_default();
                    let mut out = Vec::new();
                    for sid in &members {
                        if let Some(n) = store.get_node(sid).map_err(to_any)? {
                            out.push(n);
                        }
                    }
                    (out, serde_json::json!({ "cluster": cid }))
                } else if let Some(path) = src_file {
                    let all = store.all_nodes().map_err(to_any)?;
                    let out: Vec<_> = all
                        .into_iter()
                        .filter(|n| n.location.file == path)
                        .collect();
                    (out, serde_json::json!({ "file": path }))
                } else {
                    let name = name.expect("the row requires <name> when no selector is given");
                    let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
                    (hits, serde_json::json!({ "name": name }))
                };

            if !json_out {
                // Text: one block per node, in selection order. `<name>`'s header is unchanged.
                let what = match &selector {
                    serde_json::Value::Object(o) => match o.iter().next() {
                        Some((k, serde_json::Value::String(v))) if k == "name" => format!("'{v}'"),
                        Some((k, serde_json::Value::String(v))) => format!("--{k} {v}"),
                        Some((k, serde_json::Value::Array(v))) => format!(
                            "--{k} {}",
                            v.iter()
                                .filter_map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        Some((k, v)) => format!("--{k} {v}"),
                        None => String::new(),
                    },
                    _ => String::new(),
                };
                if nodes.is_empty() {
                    println!("no symbols found for {what}");
                } else {
                    println!("{} match(es) for {what}:", nodes.len());
                    for n in &nodes {
                        println!("  [{:?}] {} @ {}", n.kind, n.name, loc(n));
                        if signatures_only {
                            println!("{}", n.signature.as_deref().unwrap_or("  (no signature)"));
                        } else {
                            match store.symbol_source(n).map_err(to_any)? {
                                Some(text) => println!("{text}"),
                                None => {
                                    println!("  (source not stored — re-run 'index' to populate)")
                                }
                            }
                        }
                        println!();
                    }
                }
            } else {
                let opts = source_bundle::BudgetOpts {
                    max_total_chars: src_max_total,
                    max_node_chars: src_max_node,
                    signatures_only,
                };
                let bundle = source_bundle::build_bundle(
                    nodes,
                    selector,
                    opts,
                    |n| store.symbol_source(n).ok().flatten(),
                    |f| store.file_git_sha(f).ok().flatten(),
                    |n| store.annotations(&n.symbol).unwrap_or_default(),
                );
                println!("{}", serde_json::to_string_pretty(&bundle)?);
            }
        }
        // Task F: semantic search via embedding-based ANN.
        "semantic" => {
            let query = args.required(0);
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
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            match sem_tool.invoke(&*graph_store, &req) {
                Ok(result) => {
                    let matches = result.content["matches"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let t_cmd_end = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64;
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
                    emit_cli_span(
                        &otel_sink,
                        &otel_resource,
                        &otel_scope,
                        "wicked_estate.semantic_search",
                        vec![wicked_estate_core::observability::KeyValue::int(
                            "result.count",
                            matches.len() as i64,
                        )],
                        t_cmd_start,
                        t_cmd_end,
                    );
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
            let name = args.required(0);
            // Every `--db`, and every item of `--dbs`, in argv order; the row requires one.
            let db_paths: Vec<String> = args
                .all(&["db", "dbs"])
                .into_iter()
                .map(str::to_string)
                .collect();

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
            let fed =
                wicked_estate::cross_graph_blast_radius(&db_paths, name, 12).map_err(to_any)?;
            let (br_results, br_errors) = (&fed.dependents, &fed.errors);

            if br_results.is_empty() {
                println!("no resolved dependents for '{name}' across the specified databases");
            } else {
                println!(
                    "{} dependent(s) total (union across repos):",
                    br_results.len()
                );
                let mut current_repo = "";
                for (repo, node) in br_results {
                    if repo.as_str() != current_repo {
                        println!("\n  [repo: {repo}]");
                        current_repo = repo.as_str();
                    }
                    println!("    {:?} {} ({})", node.kind, node.name, loc(node));
                }
            }
            // Per repo, never pooled (wicked-estate#194): each repo has its own resolution tiers.
            if !fed.confidence.is_empty() {
                println!();
                for (repo, c) in &fed.confidence {
                    println!("evidence [{repo}]: {}", evidence_text(c));
                }
            }

            for err in br_errors {
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
            let path_str = args.operand(0).unwrap_or(".");
            let watch_path = Path::new(path_str);
            let history = args.switch("history");
            ensure_db_dir(&db)?;

            // Initial index.
            // The row refuses --history on an in-memory store (`Cond::Store`).
            let mut store: Box<dyn GraphStoreMutExt> = if history {
                let mut concrete = SqliteStore::open(&db).map_err(to_any)?;
                concrete.set_history_enabled(true).map_err(to_any)?;
                Box::new(concrete)
            } else {
                open_store_ext(&db).map_err(to_any)?
            };

            let as_repo = args.str("repo");
            let stats = wicked_estate::index_path_as(store.as_mut(), watch_path, as_repo)
                .map_err(to_any)?;
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
                        // A-6: the debouncer already coalesced the raw FS-event storm into this
                        // one batch. `emits_for_batch` (the unit-tested coalescing core) returns
                        // how many coarse emits this batch warrants — exactly 1 for a relevant
                        // batch, 0 otherwise — so the loop never emits once-per-raw-event.
                        let emits =
                            watch_coalesce::emits_for_batch(events.iter().map(|ev| &ev.kind));
                        let raw_event_count = events.len();
                        for _ in 0..emits {
                            match wicked_estate::index_path_as(store.as_mut(), watch_path, as_repo)
                            {
                                Ok(s) => {
                                    println!(
                                        "watch: re-indexed → {} nodes, {} edges, {} files",
                                        s.node_count, s.edge_count, s.file_count
                                    );
                                    // One emit per coalesced batch (the 500ms debounce window
                                    // already folded the storm). `coalesced_events` records how
                                    // many raw events were folded into this single emit.
                                    emit::emit_event(&emit::EmitEvent::new(
                                        "wicked.estate.indexed",
                                        "estate.index",
                                        serde_json::json!({
                                            "path": path_str,
                                            "db": db,
                                            // Same field the `index` command emits. Without it a
                                            // subscriber to a co-located graph cannot tell WHICH
                                            // repo a watch re-index touched.
                                            "repo": as_repo,
                                            "nodes": s.node_count,
                                            "edges": s.edge_count,
                                            "files": s.file_count,
                                            "source": "watch",
                                            "coalesced": true,
                                            "coalesced_events": raw_event_count,
                                        }),
                                    ));
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
            let since = args.u64("since").unwrap_or(0);
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
            let symbol = args.required(0);
            let sem_description = args.str("description");
            let sem_requirement = args.str("requirement");
            let sem_validated = args.bool("validated");
            let sem_validated_by = args.str("validated-by");
            let mut store = open_store_ext(&db).map_err(to_any)?;
            let setting =
                sem_description.is_some() || sem_requirement.is_some() || sem_validated.is_some();
            if setting {
                wicked_estate::set_semantics(
                    &mut *store,
                    symbol,
                    sem_description,
                    sem_requirement,
                    sem_validated,
                    sem_validated_by,
                )
                .map_err(to_any)?;
                println!("updated semantics for {symbol}");
            } else {
                match wicked_estate::get_semantics(&*store, symbol).map_err(to_any)? {
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
                        // Only when something WAS validated. Printing "(unattributed)" against
                        // `validated: false` describes a claim nobody made, which reads as a
                        // defect in the record rather than the absence of a claim.
                        if s.requirement_validated {
                            println!(
                                "  validated by: {}",
                                s.requirement_validated_by.as_deref().unwrap_or(
                                    "(unattributed — written before authorship was recorded)"
                                )
                            );
                            if let Some(at) = s.requirement_validated_at {
                                println!("  validated at: {at}");
                            }
                        }
                    }
                    None => println!("no semantics set for {symbol}"),
                }
            }
        }
        // Reverse link: every symbol annotated with a given requirement.
        "by-requirement" => {
            let req = args.required(0);
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "by-requirement")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            let hits = wicked_estate::symbols_for_requirement(&*store, req).map_err(to_any)?;
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
        // Community / semantic clustering over the indexed graph.
        //
        // Usage:
        //   wicked-estate clusters [<min-size>] [--json] [--db ...]
        //       [--resolution <γ>] [--hierarchical] [--package-bias <f>]   # graph (Louvain)
        //       [--weight semantic [--k <n> | --eps <d> --min-pts <n>]]     # semantic (embeddings)
        //
        // Graph mode (default): multi-level Louvain over CALLS/IMPORTS. `--resolution` tunes
        // granularity (>1 finer), `--hierarchical` splits communities with substructure,
        // `--package-bias` lets directory structure inform the partition. Reports modularity.
        // Semantic mode (`--weight semantic`): clusters by embedding proximity (DBSCAN by default;
        // `--k` switches to k-means). Requires an `--embeddings` index.
        "clusters" => {
            let min_size = args.operand_usize(0).unwrap_or(2);
            let json_out = args.switch("json");
            let cluster_resolution = args.f64("resolution").unwrap_or(1.0);
            let cluster_package_bias = args.f64("package-bias").unwrap_or(0.0);
            let cluster_k = args.usize("k");
            // The row bounds eps to 0.0..=2.0, so the narrowing is exact enough for a radius.
            let cluster_eps = args.f64("eps").map_or(0.25, |e| e as f32);
            let cluster_min_pts = args.usize("min-pts").unwrap_or(3);
            // `--annotate` needs the write side; bind mutably (read methods still work via as_ref).
            let mut store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            maybe_warn_version_mismatch(store.as_ref(), &db);

            let semantic = args.str("weight") == Some("semantic");
            let (communities, modularity): (Vec<Vec<wicked_estate_core::SymbolId>>, Option<f64>) =
                if semantic {
                    use wicked_estate_store::SqliteStore;
                    let embeddings = if db == ":memory:" {
                        Vec::new()
                    } else {
                        SqliteStore::open(&db)
                            .map_err(to_any)?
                            .all_embeddings()
                            .map_err(to_any)?
                    };
                    if embeddings.is_empty() {
                        eprintln!(
                            "note: no embeddings found — re-index with `--embeddings` (build with \
                             the `fastembed` feature for semantic quality) before \
                             `clusters --weight semantic`."
                        );
                    }
                    let params = wicked_estate_rank::SemanticClusterParams {
                        algorithm: if cluster_k.is_some() {
                            wicked_estate_rank::ClusterAlgo::KMeans
                        } else {
                            wicked_estate_rank::ClusterAlgo::Dbscan
                        },
                        k: cluster_k.unwrap_or(16),
                        eps: cluster_eps,
                        min_pts: cluster_min_pts,
                        ..Default::default()
                    };
                    let mut c = wicked_estate_rank::semantic_clusters(&embeddings, &params);
                    c.retain(|cl| cl.len() >= min_size);
                    (c, None)
                } else {
                    let params = wicked_estate_rank::CommunityParams {
                        min_size,
                        include_singletons: false,
                        resolution: cluster_resolution,
                        hierarchical: args.switch("hierarchical"),
                        package_bias: cluster_package_bias,
                    };
                    let c = wicked_estate_rank::detect_communities(store.as_ref(), &params)
                        .map_err(to_any)?;
                    let q = wicked_estate_rank::modularity(store.as_ref(), &c, cluster_resolution)
                        .map_err(to_any)?;
                    (c, Some(q))
                };

            // Chunk 4 — opt-in mutation: write a `community`-type annotation on every member of
            // every detected community. `key="community"`, `value=<community index>` (the same
            // largest-first index `source --cluster <id>` uses), `author="system"`. Default OFF:
            // `clusters` is a pure read unless `--annotate` is passed. Writes via the
            // `GraphWrite::annotate` seam; the store stamps `ts`. No-op on un-indexed symbols.
            //
            // This is a system-derived CACHE: re-running must REPLACE, not accumulate. Each member's
            // (type="community", key="community") row is deleted before the append, so a second run
            // yields exactly one `community` annotation per member instead of duplicating it. Upsert
            // is the right default for cache-class annotations — no flag (unlike advisory `annotate`).
            if args.switch("annotate") {
                use wicked_estate_core::Annotation;
                let provenance = if semantic {
                    "clusters:semantic".to_string()
                } else {
                    format!("clusters:louvain:res={cluster_resolution}")
                };
                let mut written = 0usize;
                for (idx, members) in communities.iter().enumerate() {
                    for sym in members {
                        store
                            .delete_annotations(sym, Some("community"), "community")
                            .map_err(to_any)?;
                        let ann = Annotation::new("community", "community", idx.to_string())
                            .with_provenance(provenance.clone())
                            .with_author("system");
                        store.annotate(sym, ann).map_err(to_any)?;
                        written += 1;
                    }
                }
                println!(
                    "annotated {written} member(s) across {} community/communities with type=community",
                    communities.len()
                );
            }

            if json_out {
                if args.switch("summary") && !semantic {
                    // Enriched summary mode: emit per-community objects with metadata.
                    let summaries = wicked_estate_rank::summarize_communities(
                        store.as_ref(),
                        &communities,
                        cluster_resolution,
                    )
                    .map_err(to_any)?;
                    // zip communities (largest-first) with summaries (same order).
                    let j: Vec<serde_json::Value> = communities
                        .iter()
                        .zip(summaries.iter())
                        .enumerate()
                        .map(|(i, (members, summary))| {
                            serde_json::json!({
                                "id": i,
                                "size": summary.size,
                                "members": members.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                                "label_candidates": summary.top_symbols,
                                "dominant_files": summary.dominant_files,
                                "modularity_contribution": summary.modularity_contribution,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&j)?);
                } else {
                    // Default bare-array output (back-compat).
                    let j: Vec<Vec<String>> = communities
                        .iter()
                        .map(|c| c.iter().map(|s| s.to_string()).collect())
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&j)?);
                }
            } else {
                let mode = if semantic { "semantic" } else { "graph" };
                match modularity {
                    Some(q) => println!(
                        "{} communities ({mode}, min_size={min_size}, modularity={q:.3}):",
                        communities.len()
                    ),
                    None => println!(
                        "{} clusters ({mode}, min_size={min_size}):",
                        communities.len()
                    ),
                }
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
            let name = args.required(0);
            let budget = args.usize("budget").unwrap_or(4096);
            let json_out = args.switch("json");
            // open_store_ext returns Box<dyn GraphStoreMutExt> so as_ref() satisfies
            // maybe_print_staleness's &dyn GraphStoreMutExt parameter.
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            maybe_warn_version_mismatch(store.as_ref(), &db);
            let nodes =
                wicked_estate_retrieve::budget_context(&*store, name, budget).map_err(to_any)?;
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
        // Agent A: annotation API — tag any indexed symbol with a TYPED key/value note.
        //
        // Usage:
        //   wicked-estate annotate <name>        --key K --value V [--type T] [--confidence F] [--provenance P] [--author A] [--db ...]
        //   wicked-estate annotate --symbol <id> --key K --value V [--type T] [--confidence F] [--provenance P] [--author A] [--db ...]
        //
        // `--type` defaults to `note` (back-compat with pre-0.5 untyped annotate). It is a plain
        // string — a fixed convention (note/assumption/observation/comment/question/community) OR
        // any custom type; both are stored and queried identically (rules-as-DATA). Writes via the
        // type-aware `GraphWrite::annotate` seam; the store stamps `ts` (passed 0).
        //
        // `--replace` makes the write an idempotent UPSERT scoped to (type, key): before appending,
        // `delete_annotations(sym, Some(type), key)` clears the prior row(s) for that exact
        // (type, key) on that symbol, so re-projecting a cache-class / system-derived annotation
        // replaces rather than duplicates. Default OFF = append (advisory notes accumulate). The
        // replace path leaves other keys (and other types under the same key) on the symbol intact.
        "annotate" => {
            use wicked_estate_core::{Annotation, DEFAULT_ANNOTATION_TYPE, GraphWrite};
            let (key, value) = (args.given("key"), args.given("value"));
            let ty = args.str("type").unwrap_or(DEFAULT_ANNOTATION_TYPE);
            let ann_confidence = args.f64("confidence").unwrap_or(1.0);
            let ann_provenance = args.str("provenance").unwrap_or_default();
            let ann_author = args.str("author").unwrap_or_default();
            let ann_replace = args.switch("replace");
            // Evidence envelope (#204 follow-on). The row refuses an empty value and an instant
            // that is malformed or before 1970; absent means the `Annotation` defaults
            // (`unspecified` / `manual` / 0 = never verified).
            let source_type = args.str("source-type");
            let extraction_method = args.str("extraction-method");
            let last_verified = args.i64("last-verified").unwrap_or(0);
            ensure_db_dir(&db)?;
            let mut store = SqliteStore::open(&db).map_err(to_any)?;
            // Build the typed annotation once; clone per target. ts=0 → store stamps it.
            let make = |sym_present_value: &str| {
                let mut a = Annotation::new(ty, key, sym_present_value)
                    .with_confidence(ann_confidence)
                    .with_provenance(ann_provenance)
                    .with_author(ann_author)
                    .with_last_verified(last_verified);
                if let Some(st) = source_type {
                    a = a.with_source_type(st);
                }
                if let Some(em) = extraction_method {
                    a = a.with_extraction_method(em);
                }
                a
            };
            // Upsert helper: when `--replace`, delete the (type, key) row(s) first and accumulate
            // the deleted count; then append. Returns the number of rows replaced for this symbol.
            let upsert =
                |store: &mut SqliteStore, symbol: &wicked_estate_core::SymbolId| -> Result<usize> {
                    let replaced = if ann_replace {
                        store
                            .delete_annotations(symbol, Some(ty), key)
                            .map_err(to_any)?
                    } else {
                        0
                    };
                    store.annotate(symbol, make(value)).map_err(to_any)?;
                    Ok(replaced)
                };
            let mut count = 0usize;
            let mut replaced = 0usize;
            // The row admits exactly one of `<name>` and `--symbol`.
            if let Some(sym_str) = args.str("symbol") {
                let symbol = wicked_estate_core::symbol::SymbolId::from(sym_str);
                replaced += upsert(&mut store, &symbol)?;
                count = 1;
            } else {
                let name = args.required(0);
                let hits = wicked_estate::search(&store, name).map_err(to_any)?;
                for n in &hits {
                    let sym = n.symbol.clone();
                    replaced += upsert(&mut store, &sym)?;
                    count += 1;
                }
            }
            if ann_replace {
                println!(
                    "replaced [{ty}] {key}={value} on {count} symbol(s) ({replaced} prior row(s) removed)"
                );
            } else {
                println!("annotated {count} symbol(s) with [{ty}] {key}={value}");
            }
            // Coarse event: one `wicked.estate.annotated` per annotate run, through the seam.
            emit::emit_event(&emit::EmitEvent::new(
                "wicked.estate.annotated",
                "estate.annotate",
                serde_json::json!({
                    "db": db,
                    "ann_type": ty,
                    "key": key,
                    "count": count,
                    "replaced": replaced,
                }),
            ));
        }
        // Agent A: show TYPED annotations for a symbol.
        //
        // Usage:
        //   wicked-estate annotations <name>        [--type T] [--json] [--db ...]
        //   wicked-estate annotations --symbol <id> [--type T] [--json] [--db ...]
        //
        // Reads via the `GraphRead::annotations` seam (oldest-first). `--type T` filters to that
        // exact type (fixed convention OR custom, matched identically). `--json` emits the spec
        // shape `{symbol, annotations:[…]}`, items rendered by `wicked_estate_retrieve::annotation_json`
        // — one object per matched symbol (an array under `<name>`, a single object under
        // `--symbol`). The container split is deliberate (#203, kept): a name is a search that can
        // match many symbols, an id names one. `advisory:true` is emitted for assumption/question (computed from `type`,
        // not hard-coded). This direct read is NOT R4-capped — only structured payloads are.
        "annotations" => {
            let json_out = args.switch("json");
            let type_filter = args.str("type");
            // ADR-003: route through the open_store factory (backend-agnostic) — this arm
            // needs only GraphRead methods, which deref through Box<dyn GraphStore>.
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "annotations")?;
            let store = open_store(&db).map_err(to_any)?;

            // Fetch + apply the optional type filter for one symbol.
            let fetch = |sym: &wicked_estate_core::SymbolId| -> Result<Vec<wicked_estate_core::Annotation>> {
                let mut anns = store.annotations(sym).map_err(to_any)?;
                if let Some(t) = type_filter {
                    anns.retain(|a| a.r#type == t);
                }
                Ok(anns)
            };
            // The spec's per-symbol JSON object: {symbol, annotations:[...]}.
            let sym_json = |sym: &wicked_estate_core::SymbolId,
                            anns: &[wicked_estate_core::Annotation]| {
                serde_json::json!({
                    "symbol": sym.to_string(),
                    "annotations": anns.iter().map(wicked_estate_retrieve::annotation_json).collect::<Vec<_>>(),
                })
            };
            // Human line for one annotation (advisory marker shown when advisory).
            let print_ann = |indent: &str, a: &wicked_estate_core::Annotation| {
                let adv = if a.is_advisory() { " advisory" } else { "" };
                println!(
                    "{indent}[{}] {}={} [confidence={:.3} provenance={:?} author={:?}{adv}]",
                    a.r#type, a.key, a.value, a.confidence, a.provenance, a.author
                );
            };

            // The row admits exactly one of `<name>` and `--symbol`.
            if let Some(sym_str) = args.str("symbol") {
                let symbol = wicked_estate_core::symbol::SymbolId::from(sym_str);
                let anns = fetch(&symbol)?;
                if json_out {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&sym_json(&symbol, &anns))?
                    );
                } else if anns.is_empty() {
                    println!("(no annotations for symbol {sym_str})");
                } else {
                    for a in &anns {
                        print_ann("", a);
                    }
                }
            } else {
                let name = args.required(0);
                let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
                if json_out {
                    let mut arr: Vec<serde_json::Value> = Vec::with_capacity(hits.len());
                    for n in &hits {
                        let anns = fetch(&n.symbol)?;
                        arr.push(sym_json(&n.symbol, &anns));
                    }
                    println!("{}", serde_json::to_string_pretty(&arr)?);
                } else if hits.is_empty() {
                    println!("no symbols found for '{name}'");
                } else {
                    for n in &hits {
                        let anns = fetch(&n.symbol)?;
                        println!("  [{:?}] {} ({})", n.kind, n.name, loc(n));
                        if anns.is_empty() {
                            println!("    (no annotations)");
                        } else {
                            for a in &anns {
                                print_ann("    ", a);
                            }
                        }
                    }
                }
            }
        }
        // Freshness read: every (symbol, annotation) pair whose evidence-envelope `last_verified`
        // is strictly before the cutoff — i.e. the facts a re-verification window deems stale.
        // Never-verified rows (last_verified == 0) are stale for any positive cutoff. Thin surface
        // over the `GraphRead::annotations_stale_since` seam (ordered symbol then ts).
        //
        // Usage:
        //   wicked-estate stale-annotations <cutoff-unix-seconds | YYYY-MM-DD> [--json] [--db ...]
        //   wicked-estate stale-annotations --older-than <N>{s,m,h,d,w}       [--json] [--db ...]
        "stale-annotations" => {
            let json_out = args.switch("json");
            // Exactly one spelling of the cutoff (#205), which the row enforces: the
            // `<cutoff-unix-seconds | YYYY-MM-DD>` operand, or `--older-than <N><unit>`, never
            // both. The old `find_map` over all of argv took the first parseable token.
            let cutoff: i64 = match args.i64("older-than") {
                // `now` ≥ 0 and the window ≤ i64::MAX, so the difference cannot overflow.
                Some(window) => cutoff::now() - window,
                None => args
                    .operand_i64(0)
                    .expect("the row requires <cutoff> when --older-than is absent"),
            };
            let shown = format!("{cutoff} ({})", cutoff::format_utc(cutoff));
            // ADR-003: backend-agnostic factory — annotations_stale_since is a GraphRead method.
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "stale-annotations")?;
            let store = open_store(&db).map_err(to_any)?;
            let stale = store.annotations_stale_since(cutoff).map_err(to_any)?;
            if json_out {
                let arr: Vec<serde_json::Value> = stale
                    .iter()
                    .map(|(sym, a)| {
                        serde_json::json!({
                            "symbol": sym.to_string(),
                            "annotation": wicked_estate_retrieve::annotation_json(a),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if stale.is_empty() {
                println!("no annotations stale as of cutoff {shown}");
            } else {
                println!(
                    "{} annotation(s) stale as of cutoff {shown} (last_verified < {cutoff}):",
                    stale.len()
                );
                for (sym, a) in &stale {
                    println!(
                        "  {} [{}] {}={} [last_verified={} source_type={:?} extraction_method={:?}]",
                        sym,
                        a.r#type,
                        a.key,
                        a.value,
                        a.last_verified,
                        a.source_type,
                        a.extraction_method
                    );
                }
            }
        }
        // Agent D: stable hex fingerprint for a symbol (covers id+name+kind+file+signature).
        //
        // Usage:
        //   wicked-estate fingerprint <name>          [--db ...]   -- identity hash (id+name+kind+file+sig)
        //   wicked-estate fingerprint <name> --content [--db ...]  -- body hash (xxh3 of source slice)
        "fingerprint" => {
            let name = args.required(0);
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "fingerprint")?;
            let store = open_store(&db).map_err(to_any)?;
            let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
            drop(store);
            if hits.is_empty() {
                println!("no symbol found matching '{name}'");
                return Ok(());
            }
            if args.switch("content") {
                // Resolve paths against the stored index root so --content works
                // regardless of CWD (the indexed path is root-relative, not CWD-relative).
                let concrete = SqliteStore::open(&db).map_err(to_any)?;
                for node in &hits {
                    let rel = &node.location.file;
                    // In a multi-repo graph the path carries a `<label>/` prefix and belongs to
                    // that repo's root, not to `indexed_root`.
                    let resolved = wicked_estate::repo_scope::resolve_indexed_path(&concrete, rel)
                        .unwrap_or_else(|| std::path::PathBuf::from(rel));
                    let start = node.location.span.start_byte as usize;
                    let end = node.location.span.end_byte as usize;
                    match std::fs::read(&resolved) {
                        Ok(bytes) => {
                            let slice = bytes.get(start..end).unwrap_or(&[]);
                            let hash = xxhash_rust::xxh3::xxh3_64(slice);
                            println!("{hash:016x}  {:?} {} ({})", node.kind, node.name, loc(node));
                        }
                        Err(e) => {
                            println!(
                                "(cannot read {}: {e})  {:?} {} ({})",
                                resolved.display(),
                                node.kind,
                                node.name,
                                loc(node)
                            );
                        }
                    }
                }
            } else {
                let store = SqliteStore::open(&db).map_err(to_any)?;
                for node in &hits {
                    match store.node_fingerprint(&node.symbol).map_err(to_any)? {
                        Some(fp) => println!("{fp}  {:?} {} ({})", node.kind, node.name, loc(node)),
                        None => println!("(not indexed)  {} ", node.name),
                    }
                }
            }
        }
        // Agent D: symbols in files changed since a git SHA.
        //
        // Usage:
        //   wicked-estate changed-since <git-sha> [--json] [--db ...]
        "changed-since" => {
            let sha = args.required(0);
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
            let json_out = args.switch("json");
            if changed_files.is_empty() {
                if json_out {
                    println!("[]");
                } else {
                    println!("no files changed since {sha}");
                }
                return Ok(());
            }
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "changed-since")?;
            let store = SqliteStore::open(&db).map_err(to_any)?;
            let mut all_nodes: Vec<wicked_estate_core::Node> = Vec::new();
            for file in &changed_files {
                let nodes = store.nodes_in_file(file).map_err(to_any)?;
                all_nodes.extend(nodes);
            }
            if json_out {
                let j: Vec<serde_json::Value> = all_nodes
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
            let json_out = args.switch("json");
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "entrypoints")?;
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
            let json_out = args.switch("json");
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "leaves")?;
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
            let json_out = args.switch("json");
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "dead-code")?;
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
        // Agent E: nodes — bulk export all symbols, optionally filtered by kind or annotation.
        //
        // Usage:
        //   wicked-estate nodes [--kind K] [--annotated-with K[=V]] [--json] [--semantics] [--db ...]
        "nodes" => {
            use wicked_estate_core::GraphRead;
            let kind = args.str("kind").unwrap_or_default().to_string();
            let json_out = args.switch("json");
            // Opt-in: `nodes --json --semantics` adds four extra per-node keys the domain-brain
            // extraction engine needs — `rule_confidence`, `requirement`, `requirement_validated`,
            // `out_edges`. OFF by default so the plain `nodes --json` path pays neither the
            // per-node `get_semantics` read nor the `neighbors` edge fetch (and its shape is
            // unchanged for existing consumers).
            let with_semantics = args.switch("semantics");
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "nodes")?;
            let store = SqliteStore::open(&db).map_err(to_any)?;

            // Per-node JSON for the `--json` paths: base metadata + typed annotations.
            // `annotation_summary` is always present (exact, over the FULL set); `annotations` is
            // present only when non-empty and is R4-capped (advisory-first, ts desc, ≤ 20).
            let node_json = |n: &wicked_estate_core::Node| -> serde_json::Value {
                let all_anns = store.annotations(&n.symbol).unwrap_or_default();
                let mut obj = serde_json::json!({
                    "symbol_id": n.symbol.to_string(),
                    "name": n.name,
                    "kind": format!("{:?}", n.kind),
                    "file": n.location.file,
                    "line": n.location.span.start_line + 1,
                    "signature": n.signature,
                    "annotation_summary": wicked_estate_retrieve::annotation_summary(&all_anns),
                });
                if with_semantics {
                    use wicked_estate_core::Direction;
                    // `rule_confidence`: MAX confidence over this node's `business_rule` annotations
                    // (already in `all_anns` — no extra query), or null when there are none.
                    let rule_confidence = all_anns
                        .iter()
                        .filter(|a| a.r#type == "business_rule")
                        .map(|a| a.confidence)
                        .reduce(f64::max);
                    // `requirement` / `requirement_validated`: the requirement↔functionality link.
                    // Best-effort read (degrades to null/false, matching `all_anns` above).
                    let sem = wicked_estate::get_semantics(&store, n.symbol.as_str())
                        .ok()
                        .flatten();
                    // `out_edges`: DISTINCT outgoing edge kinds. Outgoing = source == id, i.e.
                    // `Direction::Dependencies`; deduped via a BTreeSet so the Vec comes out sorted.
                    let out_edges: Vec<String> = store
                        .neighbors(&n.symbol, Direction::Dependencies)
                        .unwrap_or_default()
                        .iter()
                        .map(|e| format!("{:?}", e.kind))
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    obj["rule_confidence"] = serde_json::json!(rule_confidence);
                    obj["requirement"] =
                        serde_json::json!(sem.as_ref().and_then(|s| s.requirement.clone()));
                    obj["requirement_validated"] =
                        serde_json::json!(sem.map(|s| s.requirement_validated).unwrap_or(false));
                    obj["out_edges"] = serde_json::json!(out_edges);
                }
                if !all_anns.is_empty() {
                    obj["annotations"] = serde_json::Value::Array(
                        wicked_estate_retrieve::payload_annotations_json(&all_anns),
                    );
                }
                obj
            };

            if let Some(ann_filter) = args.str("annotated-with") {
                // --annotated-with KEY or KEY=VALUE
                let (ann_key, ann_val) = if let Some((k, v)) = ann_filter.split_once('=') {
                    (k, Some(v))
                } else {
                    (ann_filter, None)
                };
                let nodes = store.find_by_annotation(ann_key, ann_val).map_err(to_any)?;
                if json_out {
                    let j: Vec<serde_json::Value> = nodes.iter().map(&node_json).collect();
                    println!("{}", serde_json::to_string_pretty(&j)?);
                } else {
                    let filter_desc = ann_val
                        .map(|v| format!("{ann_key}={v}"))
                        .unwrap_or_else(|| ann_key.to_string());
                    println!("{} node(s) annotated with '{filter_desc}':", nodes.len());
                    for n in nodes.iter().take(100) {
                        println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                    }
                    if nodes.len() > 100 {
                        println!("  ... and {} more", nodes.len() - 100);
                    }
                }
            } else {
                let nodes = store.nodes_by_kind(&kind).map_err(to_any)?;
                if json_out {
                    let j: Vec<serde_json::Value> = nodes.iter().map(&node_json).collect();
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
        }
        // First-class name → SymbolId resolution (Domain-Brain Contract 2 §4 #2).
        //
        // Usage:
        //   wicked-estate resolve <name> [--file F] [--kind K] [--json] [--db ...]
        //
        // Emits `[{symbol_id, name, kind, file, line}]` for every node whose simple name equals
        // <name>, optionally narrowed by exact `location.file == F` and/or case-insensitive
        // `kind == K` (matched against the Debug form `nodes --json` uses, e.g. "function").
        // This is the read a write path's precondition depends on: names are NOT unique — one name
        // can fan out to many SymbolIds (carddemo `MAIN-PARA` × 21) — so a consumer resolves
        // name → SymbolId HERE before calling `annotate --symbol <id>` / `semantics <id>`, where a
        // bare name is a silent no-op. Deterministic: `find_symbols(exact_name)` orders by SymbolId.
        "resolve" => {
            use wicked_estate_core::query::SymbolQuery;
            let json_out = args.switch("json");
            let file_filter = args.str("file");
            let kind_filter = args.str("kind");
            let include_values = args.switch("include-values");
            let name = args.required(0);

            // Brain-facing read surface → route through the open_store factory so it
            // is backend-agnostic (postgres:// under --features postgres) per ADR-003,
            // rather than pinning a new caller to SqliteStore. resolve only needs
            // GraphRead::find_symbols, a GraphStore supertrait method, so Box<dyn
            // GraphStore> derefs cleanly. (The other read arms are pre-existing debt —
            // a dedicated open_store migration, not this PHASE-1 surface's job.)
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "resolve")?;
            let store = open_store(&db).map_err(to_any)?;
            let q = SymbolQuery {
                exact_name: Some(name.to_string()),
                ..Default::default()
            };
            let mut nodes = store.find_symbols(&q).map_err(to_any)?;
            // (#234) Synthetic value slots share real symbols' names (`resolve runs` on a
            // 905-file repo: 63 slots beside 1 function). Like every other name-based arm, the
            // default answer is the structural symbols; `--include-values` is the way back in
            // (docs/ENGINE-CONTRACT.md §3.3).
            if !include_values {
                nodes.retain(wicked_estate_core::is_structural_symbol);
            }
            if let Some(f) = file_filter {
                nodes.retain(|n| n.location.file == f);
            }
            if let Some(k) = kind_filter {
                let kl = k.to_lowercase();
                nodes.retain(|n| format!("{:?}", n.kind).to_lowercase() == kl);
            }

            if json_out {
                let rows: Vec<serde_json::Value> = nodes.iter().map(resolve_row).collect();
                print_line(&serde_json::to_string_pretty(&rows)?)?;
            } else {
                print_line(&format!("{} match(es) for '{name}':", nodes.len()))?;
                for n in &nodes {
                    print_line(&format!(
                        "  {} {:?} ({}:{})",
                        n.name,
                        n.kind,
                        n.location.file,
                        n.location.span.start_line + 1
                    ))?;
                }
            }
        }
        // Cross-repo symbol correspondence.
        //
        // Usage:
        //   wicked-estate correspond --db-a A.db --db-b B.db [--kind <k>] [--top N] [--min-score F] [--json]
        //
        // Algorithm (lexical-only when no embeddings, RRF-fused when both DBs have embeddings):
        //   For each non-trivial symbol in DB-A, retrieve up to 20 BM25 candidates from DB-B
        //   (and up to 20 embedding-nearest when available), score with weighted signals, emit
        //   the top-N pairs above --min-score threshold.
        "correspond" => {
            use std::collections::HashMap;
            use wicked_estate_core::{GraphRead, query::SymbolQuery};
            use wicked_estate_retrieve::reciprocal_rank_fusion;

            let (path_a, path_b) = (args.given("db-a"), args.given("db-b"));

            let json_out = args.switch("json");
            let explain = args.switch("explain");
            let filter_kind = args.str("kind");
            let correspond_top = args.usize("top").unwrap_or(20);
            let correspond_min_score = args.f64("min-score").unwrap_or(0.35);

            let store_a = SqliteStore::open(path_a).map_err(to_any)?;
            let store_b = SqliteStore::open(path_b).map_err(to_any)?;

            let use_embed =
                store_a.capabilities().vector_search && store_b.capabilities().vector_search;

            // Load all scoreable nodes from A, optionally filtered by kind.
            let nodes_a_raw = store_a.nodes_by_kind("").map_err(to_any)?;
            let nodes_a: Vec<wicked_estate_core::Node> = nodes_a_raw
                .into_iter()
                .filter(|n| is_correspond_kind(&n.kind))
                .filter(|n| {
                    filter_kind
                        .is_none_or(|k| format!("{:?}", n.kind).to_lowercase() == k.to_lowercase())
                })
                .collect();

            // Build a SymbolId → Node map for B so we can look up matched nodes cheaply.
            let nodes_b_raw = store_b.nodes_by_kind("").map_err(to_any)?;
            let b_by_sym: HashMap<String, wicked_estate_core::Node> = nodes_b_raw
                .into_iter()
                .filter(|n| is_correspond_kind(&n.kind))
                .map(|n| (n.symbol.to_string(), n))
                .collect();

            struct Pair {
                a: String,
                b: String,
                a_name: String,
                b_name: String,
                score: f64,
                basis: String,
                name_j: f64,
                sig_j: f64,
                k_score: f64,
                arity_sim: f64,
                rrf_score: Option<f64>,
            }

            let mut pairs: Vec<Pair> = Vec::new();

            for node_a in &nodes_a {
                let norm_name_a = correspond_tokens(&node_a.name);
                if norm_name_a.is_empty() {
                    continue;
                }
                let is_stop = STOP_NAMES.contains(&node_a.name.to_lowercase().as_str())
                    || STOP_NAMES.contains(&norm_name_a.join("").as_str());

                // ── Pre-filter: BM25 name candidates from B ──────────────────
                let name_q = norm_name_a.join(" ");
                let fts_hits = store_b
                    .find_symbols(&SymbolQuery {
                        text: Some(name_q.clone()),
                        limit: Some(20),
                        ..SymbolQuery::default()
                    })
                    .map_err(to_any)?;
                let name_rank: Vec<wicked_estate_core::SymbolId> =
                    fts_hits.iter().map(|n| n.symbol.clone()).collect();

                // ── Embedding candidates from B (when available) ─────────────
                let embed_rank: Vec<wicked_estate_core::SymbolId> = if use_embed {
                    store_a
                        .embedding(&node_a.symbol)
                        .map_err(to_any)?
                        .map(|vec| {
                            store_b
                                .nearest(&vec, 20)
                                .map_err(to_any)
                                .unwrap_or_default()
                                .into_iter()
                                .map(|(s, _)| s)
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    vec![]
                };

                // ── Fuse lists (RRF when embeddings available) ───────────────
                let has_embed = !embed_rank.is_empty();
                let rrf_scored: Vec<(wicked_estate_core::SymbolId, f64)> = if has_embed {
                    reciprocal_rank_fusion(&[name_rank.clone(), embed_rank], 60.0)
                } else {
                    vec![]
                };
                let rrf_score_map: HashMap<String, f64> = rrf_scored
                    .iter()
                    .map(|(s, sc)| (s.to_string(), *sc))
                    .collect();

                let candidates: Vec<wicked_estate_core::SymbolId> = if has_embed {
                    rrf_scored.into_iter().take(15).map(|(s, _)| s).collect()
                } else {
                    name_rank
                };

                // Pre-compute per-node-a values used in the inner loop.
                let toks_a = correspond_tokens(&node_a.name);
                let sig_toks_a = node_a.signature.as_deref().map(normalize_sig);
                let arity_a = node_a.signature.as_deref().and_then(arity_from_sig);

                for sym_b in candidates {
                    let node_b = match b_by_sym.get(sym_b.as_str()) {
                        Some(n) => n,
                        None => continue,
                    };

                    // Kind must be at least partially compatible (non-zero score).
                    let k_score = kind_match_score(&node_a.kind, &node_b.kind);
                    if k_score == 0.0 {
                        continue;
                    }

                    let toks_b = correspond_tokens(&node_b.name);
                    let name_j = token_jaccard(&toks_a, &toks_b);

                    let sig_j = match (sig_toks_a.as_deref(), node_b.signature.as_deref()) {
                        (Some(sa), Some(sb)) => token_jaccard(sa, &normalize_sig(sb)),
                        _ => 0.0,
                    };

                    let arity_sim = match (
                        arity_a,
                        node_b.signature.as_deref().and_then(arity_from_sig),
                    ) {
                        (Some(aa), Some(ab)) => {
                            let d = (aa as f64 - ab as f64).abs();
                            let m = aa.max(ab) as f64;
                            if m == 0.0 {
                                1.0
                            } else {
                                1.0 - (d / m).min(1.0)
                            }
                        }
                        _ => 0.0,
                    };

                    let rrf_score = rrf_score_map.get(sym_b.as_str()).copied();
                    let mut score = if has_embed {
                        // Hybrid: RRF score is primary; kind + arity are boosts.
                        let rrf = rrf_score.unwrap_or(0.0);
                        rrf + 0.10 * k_score + 0.05 * arity_sim
                    } else {
                        // Lexical-only weighted sum (weights from recon formula).
                        0.50 * name_j + 0.25 * sig_j + 0.15 * k_score + 0.10 * arity_sim
                    };

                    // Stop-name penalty: rely on sig+kind to carry the pair.
                    if is_stop {
                        score *= 0.6;
                    }

                    // RRF scores are in ~[0.008, 0.033] range; scale threshold for hybrid mode.
                    let threshold = if has_embed {
                        correspond_min_score * 0.015
                    } else {
                        correspond_min_score
                    };
                    if score < threshold {
                        continue;
                    }

                    let basis = {
                        let mut parts: Vec<&str> = Vec::new();
                        if name_j > 0.0 {
                            parts.push("name");
                        }
                        if has_embed {
                            parts.push("embed");
                        }
                        if sig_j > 0.1 {
                            parts.push("sig");
                        }
                        if k_score == 1.0 {
                            parts.push("kind");
                        }
                        parts.join("+")
                    };

                    pairs.push(Pair {
                        a: node_a.symbol.to_string(),
                        b: node_b.symbol.to_string(),
                        a_name: node_a.name.clone(),
                        b_name: node_b.name.clone(),
                        score,
                        basis,
                        name_j,
                        sig_j,
                        k_score,
                        arity_sim,
                        rrf_score,
                    });
                }
            }

            pairs.sort_by(|x, y| {
                y.score
                    .partial_cmp(&x.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            pairs.dedup_by(|x, y| x.a == y.a && x.b == y.b);
            pairs.truncate(correspond_top);

            let embed_note = if use_embed {
                " [name+embed]"
            } else {
                " [name-only]"
            };
            if json_out {
                let j: Vec<serde_json::Value> = pairs
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "a": p.a,
                            "b": p.b,
                            "a_name": p.a_name,
                            "b_name": p.b_name,
                            "score": p.score,
                            "basis": p.basis,
                            "name_j": if explain { Some(p.name_j) } else { None },
                            "sig_j": if explain { Some(p.sig_j) } else { None },
                            "k_score": if explain { Some(p.k_score) } else { None },
                            "arity_sim": if explain { Some(p.arity_sim) } else { None },
                            "rrf_score": if explain { p.rrf_score } else { None },
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else if pairs.is_empty() {
                println!(
                    "no correspondences found (min-score={:.2}{embed_note})",
                    correspond_min_score
                );
            } else {
                println!(
                    "{} correspondence pair(s){embed_note} (min-score={:.2}):",
                    pairs.len(),
                    correspond_min_score
                );
                for p in &pairs {
                    println!(
                        "  {:.3}  {}  ↔  {}  [{}]  ({} ↔ {})",
                        p.score, p.a_name, p.b_name, p.basis, p.a, p.b
                    );
                    if explain {
                        let rrf_str = p.rrf_score.map_or("n/a".to_string(), |r| format!("{r:.4}"));
                        println!(
                            "        name_j={:.3}  sig_j={:.3}  kind={:.3}  arity={:.3}  rrf={}",
                            p.name_j, p.sig_j, p.k_score, p.arity_sim, rrf_str
                        );
                    }
                }
            }
        }
        "export" => {
            let format = args.str("format").unwrap_or("ndjson");
            let nodes_only = args.switch("nodes-only");
            let edges_only = args.switch("edges-only");

            // ADR-003: backend-agnostic factory — all_nodes/all_edges are GraphRead methods.
            // (#246) Fail closed on a missing graph — after the arm's own usage checks, before the
            // open that would otherwise create an empty one.
            require_existing_graph(&db, "export")?;
            let store = open_store(&db).map_err(to_any)?;
            let nodes = if !edges_only {
                store.all_nodes().map_err(to_any)?
            } else {
                vec![]
            };
            let edges = if !nodes_only {
                store.all_edges().map_err(to_any)?
            } else {
                vec![]
            };

            match format {
                "json" => {
                    let out = serde_json::json!({ "nodes": nodes, "edges": edges });
                    println!("{}", serde_json::to_string_pretty(&out)?);
                }
                _ => {
                    for node in &nodes {
                        println!("{}", serde_json::to_string(node)?);
                    }
                    for edge in &edges {
                        println!("{}", serde_json::to_string(edge)?);
                    }
                }
            }
        }
        "plugins" => {
            // `wicked-estate plugins list` — show runtime language plugins loaded from the plugins
            // dir ($WICKED_ESTATE_PLUGINS or ~/.wicked-estate/plugins). See PLUGIN.md.
            // `list` is the only subcommand, and the row refuses any other operand.
            if let Some(d) = wicked_estate_extract::plugin::plugins_dir() {
                println!("plugins dir: {}", d.display());
            }
            // Listings cover additive plugins AND every override plugin dir — active,
            // FAILED (built-in in use), armed, INERT, and DISABLED-duplicate (ADR-010).
            let listings = wicked_estate_extract::plugin::listings();
            if listings.is_empty() {
                println!(
                    "(no plugins loaded — drop a plugin dir into the plugins dir; see PLUGIN.md)"
                );
            } else {
                for l in listings {
                    let status = l
                        .status
                        .as_deref()
                        .map(|s| format!("  {s}"))
                        .unwrap_or_default();
                    println!(
                        "{}  exts=[{}]  license={}{status}",
                        l.name,
                        l.extensions.join(", "),
                        l.license.as_deref().unwrap_or("unspecified"),
                    );
                }
            }
        }
        _ => {
            println!("wicked-estate {} — usage:", env!("CARGO_PKG_VERSION"));
            println!(
                "  wicked-estate --version            # `wicked-estate <version>`, one line, exit 0"
            );
            println!(
                "  wicked-estate index <path>         [--db <file|:memory:>] [--repo <name>] [--history] [--embeddings] [--force]"
            );
            println!(
                "    --repo <name> co-locate MANY repos in ONE db (alias --as): namespaces this repo's"
            );
            println!(
                "                  paths as <name>/… so nothing collides, and records its provenance"
            );
            println!(
                "                  separately. Omit for a single-repo db (unchanged behaviour)."
            );
            println!("                  Co-location only — edges do NOT resolve across repos.");
            println!("    --history     opt-in to edge-history archival (default: off)");
            println!(
                "    --embeddings  compute and store embedding vectors after indexing (default: off)"
            );
            println!(
                "    --force       bypass incremental digest skip; re-extract all files (use after a binary upgrade)"
            );
            println!(
                "  wicked-estate scip  <root>         [--db ...] [--repo <name>] [--scip-file <path>]"
            );
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
                "  wicked-estate import-telemetry <file.json> [--db ...]  # import access_log + search_misses"
            );
            println!(
                "  wicked-estate drift                 [--db ...]  # IaC vs live resource diff (W10)"
            );
            println!("  wicked-estate query <name>          [--db ...]");
            println!(
                "    --json emits `resolve --json`'s rows [{{symbol_id,name,kind,file,line}}] (#199)"
            );
            println!("  wicked-estate blast-radius <name>   [--depth N] [--json] [--db ...]");
            println!("  wicked-estate path <from> <to>      [--max-depth N] [--json] [--db ...]");
            println!(
                "    The ordered hops from <from> to <to> — each with its edge kind and confidence."
            );
            println!(
                "    <from>/<to>: exact symbol name or SymbolId. --max-depth 1..=16 (default 12)."
            );
            // Generated from the bridge table, so a bridged command cannot ship undocumented.
            for c in tool_bridge::COMMANDS {
                println!("  wicked-estate {}", c.usage_line());
                let also = if c.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" (alias: {})", c.aliases.join(", "))
                };
                println!(
                    "    MCP {} over the CLI{also}; `wicked-estate {} --help` for flags.",
                    c.tool.name(),
                    c.name
                );
            }
            for line in SUPPORTS_USAGE.lines() {
                println!("  {}", line.trim_start_matches("usage: "));
            }
            println!(
                "  wicked-estate stats                 [--db ...]  # includes git provenance if indexed"
            );
            println!(
                "  wicked-estate source [<name>]       [--db ...]  # print source slice(s) for symbol"
            );
            println!(
                "    Bulk selectors (mutually exclusive; precedence --symbols > --cluster > --file > <name>):"
            );
            println!(
                "      --cluster <id>        all symbols in community <id> (see `clusters` output)"
            );
            println!("      --file <path>         all symbols whose location.file == path");
            println!("      --symbols <ids>       comma-separated SymbolIds (exact)");
            println!(
                "    Output options: --json  --signatures-only  --max-total-chars <N>  --max-node-chars <N>  (the budgets need --json)"
            );
            println!(
                "  wicked-estate semantic <query>      [--db ...]  # embedding-based symbol search (requires prior --embeddings)"
            );
            println!("  wicked-estate cross-graph <name>   --db <a.db> --db <b.db> ...");
            println!(
                "    (or --dbs a.db,b.db)  # federated search + blast-radius across repos (W12)"
            );
            println!("  wicked-estate compact              [--db <file>]  # prune cruft + VACUUM");
            println!("  wicked-estate watch <path>         [--db ...] [--repo <name>] [--history]");
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
                "  wicked-estate clusters [<min-size>] [--json] [--annotate]  # community detection / clustering"
            );
            println!(
                "    graph (default): Louvain over CALLS/IMPORTS — [--resolution <γ>] [--hierarchical] [--package-bias <f>]"
            );
            println!(
                "    --annotate    write a `community`-type annotation (author=system) on each member (default: off)"
            );
            println!(
                "    semantic: [--weight semantic [--k <n> | --eps <d> --min-pts <n>]]  (needs an --embeddings index)"
            );
            println!(
                "  wicked-estate context <name> --budget <chars> [--json]  # ranked context within char budget"
            );
            println!("  wicked-estate annotate <name> --key K --value V [--type T] [--db ...]");
            println!("    --key         annotation key (required)");
            println!("    --value       annotation value (required)");
            println!(
                "    --type        annotation type (default: note; note/assumption/observation/comment/question/community or custom)"
            );
            println!("    --confidence  confidence score 0.0–1.0 (default: 1.0)");
            println!("    --provenance  provenance string (default: empty)");
            println!("    --author      author string (default: empty)");
            println!(
                "    --source-type        what kind of source backed it (default: unspecified; code/config/sme-answer/static-analysis/runtime-trace/documentation or custom)"
            );
            println!(
                "    --extraction-method  how it was extracted, e.g. scip-rust@0.3 (default: manual)"
            );
            println!(
                "    --last-verified      now | <unix-seconds> | YYYY-MM-DD (UTC) — the freshness clock stale-annotations reads (default: 0 = never verified)"
            );
            println!("  wicked-estate annotations <name>   [--type T] [--json] [--db ...]");
            println!("  wicked-estate annotations --symbol <id> [--type T] [--json] [--db ...]");
            println!(
                "    Show annotations for matching symbols. --type filters. Each item carries `advisory`, `ts` (write time) and the"
            );
            println!(
                "    evidence envelope `source_type`, `extraction_method`, `last_verified` (0 = never verified)."
            );
            println!(
                "    --json container depends on the lookup: <name> is a search and emits an ARRAY [{{symbol, annotations:[...]}}, ...]"
            );
            println!(
                "    (one per matched symbol, possibly empty); --symbol <id> names exactly one and emits a single {{symbol, annotations:[...]}}."
            );
            println!(
                "  wicked-estate stale-annotations <cutoff-unix-seconds | YYYY-MM-DD> [--json] [--db ...]  # (symbol, annotation) pairs with last_verified < cutoff"
            );
            println!(
                "  wicked-estate stale-annotations --older-than <N>{{s,m,h,d,w}} [--json] [--db ...]  # cutoff = now - N (e.g. 90d)"
            );
            println!(
                "    Evidence-envelope freshness read: \"what needs re-verification?\". A date is 00:00:00 UTC. Never-verified rows (last_verified=0) are always stale."
            );
            println!(
                "  wicked-estate fingerprint <name>   [--db ...]  # stable hex fingerprint for symbol"
            );
            println!(
                "  wicked-estate changed-since <sha>  [--json] [--db ...]  # symbols in files changed since git SHA"
            );
            println!(
                "  wicked-estate entrypoints [--json]            # symbols with no callers/importers"
            );
            println!(
                "  wicked-estate leaves      [--json]            # symbols that call/import nothing"
            );
            println!(
                "  wicked-estate dead-code   [--json]            # symbols with no edges at all"
            );
            println!(
                "  wicked-estate nodes [--kind K] [--annotated-with K[=V]] [--json] [--semantics]  # filter symbols by kind or annotation"
            );
            println!(
                "    --json adds per-node annotation_summary {{count,by_type,has_advisory}} + an annotations[] array (R4-capped at 20)"
            );
            println!(
                "    --semantics (with --json) adds per-node requirement, requirement_validated, rule_confidence, out_edges[] for domain-brain"
            );
            println!(
                "  wicked-estate resolve <name> [--file F] [--kind K] [--include-values] [--json]  # name → [{{symbol_id,name,kind,file,line}}]"
            );
            println!(
                "    Resolve a simple name to its stable SymbolId(s) before an --symbol write (names are not unique)."
            );
            println!(
                "    Structural symbols only by default; --include-values adds synthetic value slots (ENGINE-CONTRACT §3.3)."
            );
            println!(
                "  wicked-estate graph-view [--limit N] [--focus <name|id>] [--include-tests] [--include-trivial] [--ignore <glob>]  # JSON {{nodes,edges}} around the hotspots"
            );
            println!(
                "    The one command that returns EDGES; its JSON shape is not yet a committed contract."
            );
            println!(
                "  wicked-estate by-requirement <requirement>  # symbols whose semantics name this requirement"
            );
            println!(
                "  wicked-estate semantics <symbol-id> [--description D] [--requirement R] [--validated true|false --validated-by W]  # read, or set with any flag"
            );
            println!(
                "  wicked-estate export [--format ndjson|json] [--nodes-only] [--edges-only]  # full graph export"
            );
            println!(
                "  wicked-estate correspond --db-a A.db --db-b B.db [--kind K] [--top N] [--min-score F] [--explain] [--json]"
            );
            println!(
                "  wicked-estate plugins list                   # runtime language plugins (drop-in grammars; see PLUGIN.md)"
            );
        }
    }
    Ok(())
}

/// `graph-view`'s edge rows: `{src, tgt}` plus the resolution evidence `kind`, `confidence`,
/// `provenance`, `resolved_by`, spelled as `path --json` ships it (wicked-estate#194).
///
/// One row per `(src, tgt, kind)`. A `calls` and an `imports` edge between one pair are two
/// facts; the old `src→tgt` key kept whichever came first and, once rows carry `kind`, would
/// have presented that arbitrary winner as the relation. The store's own primary key is
/// `(source, target, kind)`, so a repeat of the same triple is the same edge and is skipped.
fn graph_view_edge_rows(edges: &[wicked_estate_core::Edge]) -> Vec<serde_json::Value> {
    let mut seen = std::collections::HashSet::new();
    edges
        .iter()
        .filter(|e| seen.insert((e.source.as_str(), e.target.as_str(), &e.kind)))
        .map(|e| {
            serde_json::json!({
                "src": e.source.as_str(),
                "tgt": e.target.as_str(),
                "kind": &e.kind,
                "confidence": e.confidence.get(),
                "provenance": &e.provenance,
                "resolved_by": e.resolved_by,
            })
        })
        .collect()
}

#[cfg(test)]
mod graph_view_edge_rows_tests {
    use super::graph_view_edge_rows;
    use wicked_estate_core::{Edge, EdgeKind, ResolutionTier, SymbolId};

    fn edge(kind: EdgeKind, tier: ResolutionTier) -> Edge {
        Edge::new(
            SymbolId("a".into()),
            SymbolId("b".into()),
            kind,
            tier,
            "test",
        )
    }

    /// Two kinds between one pair are two rows, each with its own evidence — the trap a
    /// pair-only key fell into (#194 brief §2).
    #[test]
    fn parallel_kinds_between_one_pair_stay_separate_rows() {
        let rows = graph_view_edge_rows(&[
            edge(EdgeKind::Calls, ResolutionTier::Tags),
            edge(EdgeKind::Imports, ResolutionTier::Scip),
        ]);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0]["kind"], serde_json::json!("calls"));
        assert_eq!(rows[1]["kind"], serde_json::json!("imports"));
        assert_ne!(rows[0]["confidence"], rows[1]["confidence"]);
        for r in &rows {
            for key in [
                "src",
                "tgt",
                "kind",
                "confidence",
                "provenance",
                "resolved_by",
            ] {
                assert!(r.get(key).is_some(), "{key} missing: {r}");
            }
        }
    }

    /// The same `(src, tgt, kind)` twice is one edge, one row.
    #[test]
    fn a_repeated_triple_is_one_row() {
        let e = edge(EdgeKind::Calls, ResolutionTier::Tags);
        assert_eq!(graph_view_edge_rows(&[e.clone(), e]).len(), 1);
    }
}

/// The human `evidence:` text for a confidence envelope (wicked-estate#194), shared by
/// `blast-radius` and `cross-graph` so the two cannot word it differently.
fn evidence_text(c: &wicked_estate::EdgeConfidence) -> String {
    match (c.min, c.avg) {
        (Some(min), Some(avg)) => format!(
            "{} dependency edge(s); confidence min {min:.2}, avg {avg:.2}",
            c.edge_count
        ),
        _ => "no dependency edges inside the answer".to_string(),
    }
}

/// Serialized-size bound for the CLI blast-radius output (mirrors the retrieval tools'
/// R4 budget). crew parses the `--json` document from `execCapped` stdout, where an oversized
/// payload is cut mid-document and `JSON.parse` throws — bounding it here fixes that.
const BLAST_RADIUS_CHAR_BUDGET: usize = 25_000;

/// One serialized dependent row for the blast-radius `--json` output.
/// One `resolve --json` row — also `query --json`'s (#199): `{symbol_id,name,kind,file,line}`.
fn resolve_row(n: &wicked_estate_core::Node) -> serde_json::Value {
    serde_json::json!({
        "symbol_id": n.symbol.to_string(),
        "name": n.name,
        "kind": format!("{:?}", n.kind),
        "file": n.location.file,
        "line": n.location.span.start_line + 1,
    })
}

fn blast_radius_row(
    n: &wicked_estate_core::Node,
    br: &wicked_estate::BlastRadius,
) -> serde_json::Value {
    serde_json::json!({
        "id": n.symbol.as_str(),
        "name": n.name,
        "kind": &n.kind,
        "file": n.location.file,
        "line": n.location.span.start_line + 1,
        // (#191) Hops from the target along the walk that admitted the row — `1` is a direct
        // dependent. The MCP `BlastRadius` tool has always returned it; the CLI row now does too.
        "depth": br.depths.get(n.symbol.as_str()).copied().unwrap_or(0),
    })
}

/// Largest dependents prefix whose serialized document fits the char budget. Returns
/// `(kept, dropped)`. Binary search on prefix length — O(log n · serialize).
///
/// The envelope is MEASURED, not assumed: it is serialized with no rows and the rows get what
/// is left. A fixed allowance (the old `- 200`) cannot be right — `target` echoes a caller's
/// name of any length, and the confidence envelope renders two f32s at full precision. The
/// bound holds only while the envelope itself fits: a `<name>` near 25K chars leaves no room
/// and the document exceeds the budget with zero rows (as it did before).
fn cap_blast_radius_rows(
    name: &str,
    unresolved: usize,
    br: &wicked_estate::BlastRadius,
    depth: u32,
) -> (usize, usize) {
    let deps = &br.dependents;
    // `dropped` at its widest (every row) so its digits are never under-counted.
    let envelope = blast_radius_json(name, &[], deps.len(), unresolved, br, depth);
    let envelope_len = serde_json::to_string(&envelope).map_or(usize::MAX, |s| s.len());
    // The empty `[]` already counted in the envelope is replaced by the rows' own `[...]`.
    let row_budget = (BLAST_RADIUS_CHAR_BUDGET + 2).saturating_sub(envelope_len);
    let fits = |k: usize| -> bool {
        let rows: Vec<serde_json::Value> =
            deps[..k].iter().map(|n| blast_radius_row(n, br)).collect();
        serde_json::to_string(&rows).is_ok_and(|s| s.len() <= row_budget)
    };
    if fits(deps.len()) {
        return (deps.len(), 0);
    }
    let (mut lo, mut hi) = (0usize, deps.len());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (lo, deps.len() - lo)
}

/// The blast-radius `--json` document. `truncated_dependents` is ADDITIVE — existing consumers
/// (crew `projects/graph.ts`) read only `dependents` and `unresolved`.
///
/// The three honesty fields are DISJOINT causes and must stay that way:
///
/// - `unresolved` — calls to the name the resolver could not bind (coverage of the INPUT).
/// - `truncated_dependents` — rows dropped to stay inside the 25K-char output budget. This
///   counts ROWS WE HAVE BUT DID NOT PRINT, and nothing else. A depth cut is NOT folded in here;
///   doing so would destroy the only meaning this field has ever had.
/// - `depth_horizon_reached` / `node_cap_reached` — rows the TRAVERSAL never produced, because
///   `--depth` or the node budget stopped the walk (wicked-estate#190). `searched_depth` says
///   which horizon applied, so a consumer can re-run with a larger one.
///
/// `confidence` (wicked-estate#194) is `{min, avg, edge_count}` over the dependency edges that
/// admitted the rows (source a returned row; structural `contains`/`defines` excluded) — see
/// `wicked_estate::BlastRadius::confidence`. It is computed over the whole answer, before the
/// char-budget cut, so it describes the impact set, not the printed prefix.
fn blast_radius_json(
    name: &str,
    kept: &[wicked_estate_core::Node],
    dropped: usize,
    unresolved: usize,
    br: &wicked_estate::BlastRadius,
    depth: u32,
) -> serde_json::Value {
    serde_json::json!({
        "target": name,
        "dependents": kept.iter().map(|n| blast_radius_row(n, br)).collect::<Vec<_>>(),
        "unresolved": unresolved,
        "truncated_dependents": dropped,
        "searched_depth": depth,
        "depth_horizon_reached": br.depth_horizon_reached,
        "node_cap_reached": br.node_cap_reached,
        "confidence": {
            "min": br.confidence.min,
            "avg": br.confidence.avg,
            "edge_count": br.confidence.edge_count,
        },
    })
}

#[cfg(test)]
mod blast_radius_json_tests {
    use super::{BLAST_RADIUS_CHAR_BUDGET, blast_radius_json, cap_blast_radius_rows};
    use wicked_estate_core::{Language, Location, Node, NodeKind, Span, SymbolId};

    fn wide_node(i: usize) -> Node {
        Node::new(
            SymbolId(format!(
                "crate::very::deeply::nested::module::path::number::{i:05}::long_symbol_{i:05}"
            )),
            NodeKind::Function,
            format!("a_long_descriptive_function_name_number_{i:05}"),
            Language::new("rust"),
            Location::new(
                format!("src/very/deeply/nested/module/path/file_{i:05}.rs"),
                Span::ZERO,
            ),
        )
    }

    /// The JSON document stays parseable under the bound, reports the cut ADDITIVELY
    /// (`truncated_dependents`), and keeps the keys crew reads (`dependents`, `unresolved`).
    #[test]
    fn json_output_is_bounded_and_truncation_is_additive() {
        let deps: Vec<Node> = (0..2000).map(wide_node).collect();
        // `BlastRadius` is #[non_exhaustive] and this is the bin crate: build it field by field.
        let mut br = wicked_estate::BlastRadius::default();
        br.dependents = deps.clone();
        let (kept, dropped) = cap_blast_radius_rows("core_fn", 3, &br, 12);
        assert!(dropped > 0, "2000 wide rows must exceed the budget");
        assert_eq!(kept + dropped, deps.len());
        let out = blast_radius_json("core_fn", &deps[..kept], dropped, 3, &br, 12);
        let s = serde_json::to_string(&out).unwrap();
        assert!(
            s.len() <= BLAST_RADIUS_CHAR_BUDGET,
            "payload {} > {BLAST_RADIUS_CHAR_BUDGET}",
            s.len()
        );
        // The pre-existing contract keys crew parses are intact…
        assert!(out["dependents"].is_array());
        assert_eq!(out["unresolved"], serde_json::json!(3));
        assert_eq!(out["target"], serde_json::json!("core_fn"));
        // …and the new key is additive.
        assert_eq!(out["truncated_dependents"], serde_json::json!(dropped));
    }

    /// A small result is untouched: every row kept, `truncated_dependents: 0`.
    #[test]
    fn small_result_is_not_capped() {
        let deps: Vec<Node> = (0..3).map(wide_node).collect();
        let mut br = wicked_estate::BlastRadius::default();
        br.dependents = deps.clone();
        let (kept, dropped) = cap_blast_radius_rows("f", 0, &br, 12);
        assert_eq!((kept, dropped), (3, 0));
        let out = blast_radius_json("f", &deps, 0, 0, &wicked_estate::BlastRadius::default(), 12);
        assert_eq!(out["dependents"].as_array().unwrap().len(), 3);
        assert_eq!(out["truncated_dependents"], serde_json::json!(0));
    }

    /// wicked-estate#190: a DEPTH cut and an output-BUDGET cut are different facts and must not
    /// be conflated. `truncated_dependents` keeps meaning "rows we had but did not print";
    /// `depth_horizon_reached` means "rows the traversal never produced".
    #[test]
    fn depth_cut_is_reported_separately_from_the_char_budget_cut() {
        let deps: Vec<Node> = (0..3).map(wide_node).collect();
        let mut cut = wicked_estate::BlastRadius::default();
        cut.dependents = deps.clone();
        cut.depth_horizon_reached = true;
        cut.node_cap_reached = false;
        let out = blast_radius_json("f", &deps, 0, 0, &cut, 5);
        assert_eq!(out["depth_horizon_reached"], serde_json::json!(true));
        assert_eq!(out["node_cap_reached"], serde_json::json!(false));
        assert_eq!(out["searched_depth"], serde_json::json!(5));
        // The budget field is UNTOUCHED by a depth cut — nothing was dropped for size.
        assert_eq!(
            out["truncated_dependents"],
            serde_json::json!(0),
            "a depth cut must NOT be folded into the char-budget count"
        );
        assert_eq!(out["dependents"].as_array().unwrap().len(), 3);

        // …and the mirror: a budget cut with a complete traversal reports the budget only.
        let wide: Vec<Node> = (0..2000).map(wide_node).collect();
        let mut complete = wicked_estate::BlastRadius::default();
        complete.dependents = wide.clone();
        let (kept, dropped) = cap_blast_radius_rows("f", 0, &complete, 12);
        assert!(dropped > 0);
        let out = blast_radius_json("f", &wide[..kept], dropped, 0, &complete, 12);
        assert_eq!(out["truncated_dependents"], serde_json::json!(dropped));
        assert_eq!(out["depth_horizon_reached"], serde_json::json!(false));
    }

    /// wicked-estate#194: the confidence envelope rides the document in MCP `BlastRadius`'s
    /// shape, and no edges means `null`, never a fabricated certainty.
    #[test]
    fn confidence_envelope_is_present_and_null_without_edges() {
        let none = blast_radius_json("f", &[], 0, 0, &wicked_estate::BlastRadius::default(), 12);
        assert_eq!(
            none["confidence"],
            serde_json::json!({"min": null, "avg": null, "edge_count": 0})
        );
        let mut br = wicked_estate::BlastRadius::default();
        br.confidence.min = Some(0.5);
        br.confidence.avg = Some(0.75);
        br.confidence.edge_count = 4;
        let out = blast_radius_json("f", &[], 0, 0, &br, 12);
        assert_eq!(
            out["confidence"],
            serde_json::json!({"min": 0.5, "avg": 0.75, "edge_count": 4})
        );
        // The #190 honesty keys survive alongside it.
        for key in [
            "searched_depth",
            "depth_horizon_reached",
            "node_cap_reached",
        ] {
            assert!(out.get(key).is_some(), "{key} lost");
        }
    }

    /// The widest real envelope still fits: a long caller-supplied name, full-precision f32s
    /// and a huge edge count are measured, not covered by a guessed allowance.
    #[test]
    fn widest_envelope_still_fits_the_budget() {
        let name = "n".repeat(2_000);
        let mut br = wicked_estate::BlastRadius::default();
        br.dependents = (0..2000).map(wide_node).collect();
        br.confidence.min = Some(0.100_000_01);
        br.confidence.avg = Some(0.123_456_79);
        br.confidence.edge_count = usize::MAX;
        br.depth_horizon_reached = true;
        br.node_cap_reached = true;
        let (kept, dropped) = cap_blast_radius_rows(&name, usize::MAX, &br, u32::MAX);
        assert!(kept > 0 && dropped > 0);
        let out = blast_radius_json(
            &name,
            &br.dependents[..kept],
            dropped,
            usize::MAX,
            &br,
            u32::MAX,
        );
        let len = serde_json::to_string(&out).unwrap().len();
        assert!(len <= BLAST_RADIUS_CHAR_BUDGET, "payload {len}");
    }
}

#[cfg(test)]
mod ensure_db_dir_tests {
    use super::ensure_db_dir;

    #[test]
    fn url_shaped_specs_are_not_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        // A team-profile-resolved postgres:// spec (and an explicit sqlite:// spec) must
        // not create junk directories like `postgres:` in the CWD.
        ensure_db_dir("postgres://wicked@pg.internal:5432/estate").unwrap();
        ensure_db_dir("postgresql://wicked@pg.internal/estate").unwrap();
        ensure_db_dir("sqlite:///abs/never/created.db").unwrap();
        ensure_db_dir(":memory:").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        std::env::set_current_dir(cwd).unwrap();
        assert!(leftovers.is_empty(), "no junk dirs: {leftovers:?}");
    }

    #[test]
    fn bare_path_parent_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("nested/dir/graph.db");
        ensure_db_dir(db.to_str().unwrap()).unwrap();
        assert!(db.parent().unwrap().is_dir());
    }
}

// ─── path rendering ───────────────────────────────────────────────────────────

/// One denormalized hop endpoint — the same six fields the MCP `Path` tool emits, so a
/// script reading `--json` learns each hop's file and line without a second command.
///
/// Built from `PathResult::endpoints`, never from a store lookup: the traversal already
/// returned these nodes. (The MCP side reuses `edge_json`, which is private to
/// `wicked-estate-retrieve`; the shared thing across the two surfaces is the `PathResult`,
/// not the renderer.)
fn path_endpoint_json(
    id: &wicked_estate_core::SymbolId,
    endpoints: &[wicked_estate_core::Node],
) -> serde_json::Value {
    match endpoints.iter().find(|n| &n.symbol == id) {
        Some(n) => serde_json::json!({
            "symbol": n.symbol.as_str(),
            "name": n.name,
            "kind": &n.kind,
            "file": n.location.file,
            "line": n.location.span.start_line,
            "line_1based": n.location.span.start_line + 1,
        }),
        // Unreachable while the edge-admission rule holds; emit the bare id rather than
        // drop the hop, so the route stays traceable if it ever does.
        None => serde_json::json!({ "symbol": id.as_str() }),
    }
}

/// A hop endpoint for human eyes: `name (file:line)`.
///
/// A raw `SymbolId` is a long structured blob (`ts-rust . . . src/app/handler().`); the
/// point of this command is that a reader can name the intermediate functions and open
/// them, so the name and location lead. Resolved from `PathResult::endpoints`, so this
/// costs no store lookup; falls back to the id if an endpoint is somehow absent.
fn path_endpoint_label(
    id: &wicked_estate_core::SymbolId,
    endpoints: &[wicked_estate_core::Node],
) -> String {
    match endpoints.iter().find(|n| &n.symbol == id) {
        Some(n) => format!("{} ({})", n.name, loc(n)),
        None => id.as_str().to_string(),
    }
}

/// The `--json` document: exactly one object on stdout.
fn path_json(from: &str, to: &str, r: &wicked_estate_core::PathResult) -> serde_json::Value {
    let hops: Vec<serde_json::Value> = r
        .hops
        .iter()
        .map(|e| {
            serde_json::json!({
                "source": path_endpoint_json(&e.source, &r.endpoints),
                "target": path_endpoint_json(&e.target, &r.endpoints),
                "kind": &e.kind,
                "confidence": e.confidence.get(),
                "provenance": &e.provenance,
                "resolved_by": e.resolved_by,
            })
        })
        .collect();
    serde_json::json!({
        "from": from,
        "to": to,
        "hops": hops,
        "found": r.found,
        "depth_bounded": r.depth_bounded,
        "node_bounded": r.node_bounded,
        "unresolved": r.unresolved.map(|u| u.as_str()),
    })
}

/// Text mode: one line per hop, then an honest account of any bound that applied.
///
/// Writes to a caller-supplied sink rather than `println!` so every branch has a test that
/// dies when the branch is removed. Two of them — the node-budget line and the
/// proven-absence coverage line — cannot be provoked through the spawned binary at all,
/// because the CLI fixes `max_nodes` at 5 000 (D7); without a sink they were freely
/// deletable, which for the node-budget line means silently reporting a truncated search as
/// a proven absence (R3).
fn write_path_text(
    out: &mut impl std::io::Write,
    from: &str,
    to: &str,
    r: &wicked_estate_core::PathResult,
    max_depth: u32,
    max_nodes: usize,
) -> std::io::Result<()> {
    if let Some(side) = r.unresolved {
        writeln!(
            out,
            "no path: '{}' did not match any symbol name or SymbolId",
            if side == wicked_estate_core::Unresolved::From {
                from
            } else {
                to
            }
        )?;
        return Ok(());
    }
    if r.found {
        if r.hops.is_empty() {
            // (#230) Both operands RESOLVED to one candidate — not "the same symbol": 79 nodes
            // may be named `Props`, and this says only that one of them answered for both.
            writeln!(
                out,
                "'{from}' and '{to}' resolve to a candidate for both endpoints — zero hops"
            )?;
        } else {
            writeln!(out, "{} hop(s) from '{from}' to '{to}':", r.hops.len())?;
            for e in &r.hops {
                writeln!(
                    out,
                    "  {} -> {}  [{:?}] confidence {:.2} ({})",
                    path_endpoint_label(&e.source, &r.endpoints),
                    path_endpoint_label(&e.target, &r.endpoints),
                    e.kind,
                    e.confidence.get(),
                    e.resolved_by
                )?;
            }
        }
    } else {
        writeln!(out, "no path found from '{from}' to '{to}'")?;
    }
    // R3: a bounded search must never read as a proven absence. (#230) A FOUND route is not an
    // absence: the bound lines are for the `no path found` reader, who must know whether the
    // absence was proven or merely bounded; on a found route they only add noise.
    if r.found {
        return Ok(());
    }
    if r.depth_bounded {
        writeln!(
            out,
            "bound: the walk reached its depth frontier (--max-depth {max_depth}); \
             a longer route may exist beyond it"
        )?;
    }
    if r.node_bounded {
        writeln!(
            out,
            "bound: the walk exhausted its node budget ({max_nodes} nodes); \
             the search was cut off"
        )?;
    }
    if !r.found && !r.depth_bounded && !r.node_bounded {
        writeln!(
            out,
            "coverage: the whole reachable set was searched — no route exists"
        )?;
    }
    Ok(())
}

const SUPPORTS_USAGE: &str = "usage: wicked-estate supports owners [--json] [--db ...]\n  \
     wicked-estate supports edge --source <SYMBOL_ID> --target <SYMBOL_ID> --kind <KIND> [--json] [--db ...]\n  \
     wicked-estate supports retract --producer <P> --snapshot <S> [--json] [--db ...]\n  \
     owners   every support owner (producer, snapshot) and its last generation\n  \
     edge     the authoritative support rows behind one public edge (exact ids, no name resolution;\n           \
     --kind is an edge kind such as `calls`, `imports` or a tag such as `flows_to`)\n  \
     retract  replace the owner's support with nothing at its next generation";

/// The whole `supports` document — every field, not one section — stays under this. It is the one
/// R4 response budget (`docs/agent-behavior-rules.md`), the same figure every RetrievalTool uses;
/// rows past it are dropped in order and `truncated` says so, `total` stays exact.
const SUPPORTS_CHAR_BUDGET: usize = 25_000;

#[derive(Debug, PartialEq)]
enum SupportsMode {
    Owners,
    Edge {
        source: String,
        target: String,
        kind: String,
    },
    Retract {
        producer: String,
        snapshot: String,
    },
}

impl SupportsMode {
    fn name(&self) -> &'static str {
        match self {
            SupportsMode::Owners => "owners",
            SupportsMode::Edge { .. } => "edge",
            SupportsMode::Retract { .. } => "retract",
        }
    }
}

#[derive(Debug, PartialEq)]
struct SupportsArgs {
    mode: SupportsMode,
    json: bool,
    /// `--db`, if given; otherwise the resolved default store.
    db: Option<String>,
}

/// Parse `supports` from the RAW argument list, strictly (the rule bridged commands follow): every token is
/// classified, and an unknown, foreign, repeated or valueless flag, a flag another subcommand
/// owns, an empty value, or a stray operand fails with usage before any store is opened.
fn parse_supports_args(raw: &[String]) -> Result<SupportsArgs> {
    let usage = |why: String| anyhow::anyhow!("{SUPPORTS_USAGE}\n{why}");
    let (sub, rest) = raw
        .split_first()
        .ok_or_else(|| usage("a subcommand is required: owners | edge | retract".into()))?;
    let allowed: &[&str] = match sub.as_str() {
        "owners" => &[],
        "edge" => &["--source", "--target", "--kind"],
        "retract" => &["--producer", "--snapshot"],
        other => return Err(usage(format!("unknown subcommand {other:?}"))),
    };
    let mut values: std::collections::BTreeMap<&'static str, String> = Default::default();
    let mut json = false;
    let mut db: Option<String> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = |flag: &str| -> Result<String> {
            let v = match &inline {
                Some(v) => v.clone(),
                None => match it.next() {
                    None => return Err(usage(format!("{flag} requires a value"))),
                    Some(v) if v.starts_with("--") => {
                        return Err(usage(format!("{flag} needs a value, got the flag {v:?}")));
                    }
                    Some(v) => v.clone(),
                },
            };
            if v.is_empty() {
                return Err(usage(format!("{flag} must not be empty")));
            }
            Ok(v)
        };
        match flag {
            "--json" if inline.is_none() => {
                if json {
                    return Err(usage("--json given more than once".into()));
                }
                json = true;
            }
            // `--db value` only, like every bespoke command; `--db=` is refused, not ignored.
            "--db" if inline.is_none() => {
                if db.is_some() {
                    return Err(usage("--db given more than once".into()));
                }
                db = Some(value("--db")?);
            }
            f if allowed.contains(&f) => {
                let key = allowed
                    .iter()
                    .find(|k| **k == f)
                    .copied()
                    .unwrap_or_default();
                if values.contains_key(key) {
                    return Err(usage(format!("{f} given more than once")));
                }
                let v = value(f)?;
                values.insert(key, v);
            }
            f if f.starts_with('-') => {
                return Err(usage(format!("unknown flag {a:?} for `supports {sub}`")));
            }
            other => {
                return Err(usage(format!(
                    "unexpected operand {other:?}: `supports {sub}` takes flags only"
                )));
            }
        }
    }
    let mut take = |k: &'static str| -> Result<String> {
        values
            .remove(k)
            .ok_or_else(|| usage(format!("{k} is required for `supports {sub}`")))
    };
    let mode = match sub.as_str() {
        "owners" => SupportsMode::Owners,
        "edge" => SupportsMode::Edge {
            source: take("--source")?,
            target: take("--target")?,
            kind: take("--kind")?,
        },
        _ => SupportsMode::Retract {
            producer: take("--producer")?,
            snapshot: take("--snapshot")?,
        },
    };
    if let SupportsMode::Edge { kind, .. } = &mode {
        parse_edge_kind(kind).map_err(usage)?;
    }
    Ok(SupportsArgs { mode, json, db })
}

/// `--kind`: a built-in kind by its stored spelling (`calls`, `imports`, …), otherwise a tag
/// stored as `EdgeKind::Other` (`flows_to`). The same spelling `export`/`path --json` print.
/// A case variant of a built-in spelling (`Calls`) is refused rather than taken as the tag
/// `{"other":"Calls"}`: no store writes that tag, so it would answer an honest-looking empty
/// result for the kind the user meant (the accept-and-ignore class #197 closed for flags).
fn parse_edge_kind(kind: &str) -> std::result::Result<wicked_estate_core::EdgeKind, String> {
    let builtin = |s: &str| -> Option<wicked_estate_core::EdgeKind> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    };
    if let Some(k) = builtin(kind) {
        return Ok(k);
    }
    let lower = kind.to_lowercase();
    if lower != kind && builtin(&lower).is_some() {
        return Err(format!(
            "--kind {kind:?} is not a kind; did you mean {lower:?}?"
        ));
    }
    Ok(wicked_estate_core::EdgeKind::Other(kind.to_string()))
}

/// Keep rows, in order, while the whole document stays under [`SUPPORTS_CHAR_BUDGET`].
fn bounded_rows(
    rows: Vec<serde_json::Value>,
    frame: impl Fn(&[serde_json::Value], bool) -> serde_json::Value,
) -> serde_json::Value {
    let mut kept: Vec<serde_json::Value> = Vec::new();
    let total = rows.len();
    for row in rows {
        kept.push(row);
        let size = serde_json::to_string(&frame(&kept, true)).map_or(usize::MAX, |s| s.len());
        if size >= SUPPORTS_CHAR_BUDGET {
            kept.pop();
            break;
        }
    }
    let truncated = kept.len() < total;
    frame(&kept, truncated)
}

/// Run one `supports` subcommand and return its JSON document (text mode renders the same one).
fn run_supports(
    store: &mut dyn wicked_estate_store::GraphStoreMutExt,
    args: &SupportsArgs,
) -> Result<serde_json::Value> {
    use serde_json::json;
    match &args.mode {
        SupportsMode::Owners => {
            let owners = store.support_owners().map_err(to_any)?;
            let total = owners.len();
            let rows = owners
                .iter()
                .map(|o| serde_json::to_value(o).map_err(|e| anyhow::anyhow!(e)))
                .collect::<Result<Vec<_>>>()?;
            Ok(bounded_rows(
                rows,
                |kept, truncated| json!({"owners": kept, "total": total, "truncated": truncated}),
            ))
        }
        SupportsMode::Edge {
            source,
            target,
            kind,
        } => {
            let k = parse_edge_kind(kind).map_err(|e| anyhow::anyhow!(e))?;
            if let wicked_estate_core::EdgeKind::Other(tag) = &k {
                eprintln!(
                    "note: --kind {tag:?} is not a built-in kind; matching the tag {}",
                    json!({ "other": tag })
                );
            }
            let rows = store
                .edge_supports(
                    &wicked_estate_core::SymbolId(source.clone()),
                    &wicked_estate_core::SymbolId(target.clone()),
                    &k,
                )
                .map_err(to_any)?;
            let total = rows.len();
            let rows = rows
                .iter()
                .map(|r| serde_json::to_value(r).map_err(|e| anyhow::anyhow!(e)))
                .collect::<Result<Vec<_>>>()?;
            Ok(bounded_rows(rows, |kept, truncated| {
                json!({
                    "source": source, "target": target, "kind": k,
                    "supports": kept, "total": total, "truncated": truncated,
                })
            }))
        }
        SupportsMode::Retract { producer, snapshot } => {
            let owner =
                wicked_estate_core::SupportOwner::new(producer, snapshot).map_err(to_any)?;
            let Some(current) = store.support_generation(&owner).map_err(to_any)? else {
                anyhow::bail!(
                    "no support owner ({producer}, {snapshot}) in this graph — nothing to \
                     retract (`wicked-estate supports owners` lists them)"
                );
            };
            let next = current
                .checked_add(1)
                .filter(|g| *g <= wicked_estate_core::support::MAX_SUPPORT_GENERATION)
                .ok_or_else(|| anyhow::anyhow!("owner is at the maximum generation {current}"))?;
            store.begin_batch().map_err(to_any)?;
            let report = store
                .replace_edge_supports(&owner, next, &[])
                .map_err(to_any)?;
            store.commit_batch().map_err(to_any)?;
            Ok(json!({ "retracted": report }))
        }
    }
}

/// Text mode: a reading of the same document, one line per row, every cut named (R3).
fn write_supports_text(
    out: &mut impl std::io::Write,
    doc: &serde_json::Value,
) -> std::io::Result<()> {
    let cut = |out: &mut dyn std::io::Write, shown: usize| -> std::io::Result<()> {
        if doc["truncated"].as_bool() == Some(true) {
            writeln!(
                out,
                "truncated: showing {shown} of {} (R4 output budget) — use --json for the bounded document",
                doc["total"]
            )?;
        }
        Ok(())
    };
    if let Some(owners) = doc.get("owners").and_then(|v| v.as_array()) {
        if owners.is_empty() {
            writeln!(out, "no support owners in this graph")?;
        }
        for o in owners {
            writeln!(
                out,
                "{}\t{}\tgeneration {}",
                o["owner"]["producer"].as_str().unwrap_or(""),
                o["owner"]["snapshot"].as_str().unwrap_or(""),
                o["generation"]
            )?;
        }
        return cut(out, owners.len());
    }
    if let Some(rows) = doc.get("supports").and_then(|v| v.as_array()) {
        writeln!(
            out,
            "support for {} -[{}]-> {}: {} fact(s)",
            doc["source"].as_str().unwrap_or(""),
            match &doc["kind"] {
                serde_json::Value::String(s) => s.clone(),
                other => other
                    .get("other")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
            },
            doc["target"].as_str().unwrap_or(""),
            doc["total"]
        )?;
        for r in rows {
            let f = &r["fact"];
            // 1-based, like every other text path (`--json` keeps the raw 0-based span).
            let site = match (
                f["location"]["file"].as_str(),
                f["location"]["span"]["start_line"].as_u64(),
            ) {
                (Some(file), Some(line)) if !file.is_empty() => {
                    format!(" at {file}:{}", line + 1)
                }
                _ => String::new(),
            };
            writeln!(
                out,
                "  {}/{} gen {}  fact_id {}  confidence {} ({}, {}){site}",
                r["owner"]["producer"].as_str().unwrap_or(""),
                r["owner"]["snapshot"].as_str().unwrap_or(""),
                r["generation"],
                serde_json::Value::String(r["fact_id"].as_str().unwrap_or("").into()),
                f["confidence"],
                f["provenance"].as_str().unwrap_or(""),
                f["resolved_by"].as_str().unwrap_or("")
            )?;
        }
        return cut(out, rows.len());
    }
    let r = &doc["retracted"];
    writeln!(
        out,
        "retracted {} fact(s) of {}/{} at generation {} ({} public edge(s) re-projected)",
        r["retracted"],
        r["owner"]["producer"].as_str().unwrap_or(""),
        r["owner"]["snapshot"].as_str().unwrap_or(""),
        r["generation"],
        r["edges_touched"]
    )
}

#[cfg(test)]
mod path_render_tests {
    use super::*;
    use wicked_estate_core::path::{PathResult, Unresolved};
    use wicked_estate_core::{
        Edge, EdgeKind, Language, Location, Node, NodeKind, ResolutionTier, Span, SymbolId,
    };

    // These branches are unreachable from `tests/path_cli.rs`, which spawns the binary: the
    // CLI fixes `max_nodes` at 5 000 (D7), so the node-budget line cannot be provoked end to
    // end at all, and each spawn case costs a full `index` run. `path_json` and
    // `path_endpoint_json` are pure functions of a `PathResult`; `write_path_text` takes a
    // sink for the same reason, so all three are asserted directly here.

    /// Built at line 41 (0-based), so `line` is 41 and `line_1based` is 42. `Span::ZERO`
    /// would give 0 and 1 — both non-null, so dropping the `+ 1` or swapping the two values
    /// would pass a non-nullness assertion while sending a reader to the wrong line.
    const FIXTURE_LINE_0BASED: u32 = 41;

    fn node(id: &str, name: &str) -> Node {
        let mut span = Span::ZERO;
        span.start_line = FIXTURE_LINE_0BASED;
        Node::new(
            SymbolId(id.into()),
            NodeKind::Function,
            name,
            Language::new("rust"),
            Location::new("src/a.rs", span),
        )
    }

    fn hop(from: &str, to: &str) -> Edge {
        Edge::new(
            SymbolId(from.into()),
            SymbolId(to.into()),
            EdgeKind::Calls,
            ResolutionTier::Parsed,
            "test",
        )
    }

    fn found_result() -> PathResult {
        let mut r = PathResult::default();
        r.hops = vec![hop("a", "b")];
        r.endpoints = vec![node("a", "a_fn"), node("b", "b_fn")];
        r.found = true;
        r.depth_bounded = false;
        r.node_bounded = false;
        r.unresolved = None;
        r
    }

    #[test]
    fn json_endpoints_carry_the_six_fields_from_endpoints_not_a_lookup() {
        let doc = path_json("a_fn", "b_fn", &found_result());
        let hop = &doc["hops"][0];
        for end in ["source", "target"] {
            for field in ["symbol", "name", "kind", "file", "line", "line_1based"] {
                assert!(
                    !hop[end][field].is_null(),
                    "{end}.{field} must render from PathResult::endpoints"
                );
            }
            // VALUES, not merely presence. Non-nullness passes for a dropped `+ 1` or a
            // swap, and a consumer following the documented contract would be sent to the
            // wrong line with nothing red.
            assert_eq!(
                hop[end]["line"], FIXTURE_LINE_0BASED,
                "{end}.line is the 0-based span start"
            );
            assert_eq!(
                hop[end]["line_1based"],
                FIXTURE_LINE_0BASED + 1,
                "{end}.line_1based is one MORE than `line`, not equal to it"
            );
        }
        assert_eq!(doc["unresolved"], serde_json::Value::Null);
    }

    /// The CLI and the MCP tool are required to emit the same six endpoint fields. They use
    /// separate renderers (D9), so nothing but this test stops one from drifting.
    #[test]
    fn cli_endpoint_field_set_matches_the_mcp_tool() {
        let doc = path_json("a_fn", "b_fn", &found_result());
        let mut cli: Vec<&str> = doc["hops"][0]["source"]
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        cli.sort_unstable();
        // The set `wicked-estate-retrieve::endpoint_json` emits; that helper is private to
        // its crate, so the contract is pinned by value here rather than by calling it.
        let mut mcp = vec!["symbol", "name", "kind", "file", "line", "line_1based"];
        mcp.sort_unstable();
        assert_eq!(
            cli, mcp,
            "the CLI --json endpoint shape must match the MCP tool's; the two renderers are \
             separate (D9) and only this assertion holds them together"
        );
    }

    #[test]
    fn unresolved_to_names_the_to_operand_not_the_from_one() {
        let mut r = PathResult::default();
        r.unresolved = Some(Unresolved::To);
        let doc = path_json("resolvable", "missing", &r);
        assert_eq!(doc["unresolved"], "to");
        assert_eq!(doc["found"], false);
    }

    #[test]
    fn json_reports_each_bound_separately() {
        let mut bounded = PathResult::default();
        bounded.depth_bounded = true;
        bounded.node_bounded = true;
        let doc = path_json("a", "b", &bounded);
        assert_eq!(doc["found"], false);
        assert_eq!(doc["depth_bounded"], true);
        assert_eq!(doc["node_bounded"], true);
        assert_eq!(
            doc["unresolved"],
            serde_json::Value::Null,
            "a bounded absence is not an unresolved input"
        );
    }

    #[test]
    fn zero_hop_identity_route_is_found() {
        let mut r = PathResult::default();
        r.found = true;
        let doc = path_json("same", "same", &r);
        assert_eq!(doc["found"], true);
        assert_eq!(doc["hops"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn endpoint_absent_from_endpoints_falls_back_to_the_bare_id() {
        // Unreachable while the edge-admission rule holds, but the route must stay
        // traceable rather than lose the hop if it ever does.
        let v = path_endpoint_json(&SymbolId("ghost".into()), &[]);
        assert_eq!(v["symbol"], "ghost");
        assert!(v["name"].is_null());
    }

    fn rendered(r: &PathResult, from: &str, to: &str, max_nodes: usize) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_path_text(&mut buf, from, to, r, 12, max_nodes).expect("write to a Vec");
        String::from_utf8(buf).expect("utf8")
    }

    /// R3 on the TEXT surface. Every other test of this distinction goes through `--json`,
    /// so collapsing the unresolved branch into a bare "no path found" was free.
    #[test]
    fn text_unresolved_operand_does_not_read_as_a_proven_absence() {
        let mut unresolved = PathResult::default();
        unresolved.unresolved = Some(Unresolved::To);
        let text = rendered(&unresolved, "caller", "ghost", 5_000);
        assert!(
            text.contains("ghost"),
            "the unresolved operand's VALUE must appear: {text}"
        );
        assert!(
            !text.contains("caller"),
            "and not the operand that did resolve: {text}"
        );
        assert!(
            !text.contains("no route exists"),
            "an unusable input must not read as a proven absence: {text}"
        );

        let proven = PathResult::default();
        let absent = rendered(&proven, "a", "b", 5_000);
        assert!(
            absent.contains("no route exists"),
            "a genuine absence says so: {absent}"
        );
        assert_ne!(
            text, absent,
            "the two cases must not render identically — that is the R3 failure"
        );
    }

    /// The node-budget line. D7 fixes the CLI budget at 5 000, so the spawn fixtures cannot
    /// provoke this at all; without the sink it had no test and deleting it would report a
    /// truncated search as a proven absence.
    #[test]
    fn text_names_the_node_budget_when_it_bound() {
        let mut bounded = PathResult::default();
        bounded.node_bounded = true;
        let text = rendered(&bounded, "a", "b", 5_000);
        assert!(text.contains("node budget"), "{text}");
        assert!(
            text.contains("5000"),
            "the budget that bound must be named: {text}"
        );
        assert!(
            !text.contains("no route exists"),
            "a node-bounded search is not a proven absence: {text}"
        );
    }

    #[test]
    fn text_names_the_depth_frontier_when_it_bound() {
        let mut bounded = PathResult::default();
        bounded.depth_bounded = true;
        let text = rendered(&bounded, "a", "b", 5_000);
        assert!(text.contains("depth frontier"), "{text}");
        assert!(!text.contains("no route exists"), "{text}");
    }

    #[test]
    fn text_renders_one_line_per_hop_with_name_and_location() {
        let text = rendered(&found_result(), "a_fn", "b_fn", 5_000);
        let hops: Vec<&str> = text.lines().filter(|l| l.contains("->")).collect();
        assert_eq!(hops.len(), 1);
        // `loc()` renders 1-based, so line 41 shows as 42 — the same +1 the JSON pins.
        assert!(hops[0].contains("a_fn (src/a.rs:42)"), "{}", hops[0]);
        assert!(hops[0].contains("b_fn (src/a.rs:42)"), "{}", hops[0]);
        assert!(hops[0].contains("confidence"), "{}", hops[0]);
    }

    #[test]
    fn text_zero_hop_identity_route_says_so() {
        let mut same = PathResult::default();
        same.found = true;
        let text = rendered(&same, "x", "x", 5_000);
        assert!(
            text.contains("resolve to a candidate for both endpoints — zero hops"),
            "{text}"
        );
        assert!(!text.contains("no path"), "{text}");
    }

    /// The text label's fallback arm, symmetric with the JSON twin's.
    #[test]
    fn text_label_falls_back_to_the_bare_id() {
        assert_eq!(path_endpoint_label(&SymbolId("ghost".into()), &[]), "ghost");
    }
}
