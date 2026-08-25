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
//!   wicked-estate blast-radius <name>    [--db ...]
//!   wicked-estate stats                  [--db ...]
//!   wicked-estate rank                   [--db ...]
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
//!   wicked-estate annotate <name>        --key K --value V [--type T] [--confidence F] [--provenance P] [--author A] [--db ...]
//!   wicked-estate annotate --symbol <id> --key K --value V [--type T] [--confidence F] [--provenance P] [--author A] [--db ...]
//!   wicked-estate annotations <name>     [--type T] [--json] [--db ...]
//!   wicked-estate annotations --symbol <id> [--type T] [--json] [--db ...]
//!   wicked-estate stale-annotations <cutoff> [--json] [--db ...]
//!   wicked-estate context <name>         [--budget <chars>] [--json] [--db ...]
//!   wicked-estate entrypoints            [--json] [--db ...]
//!   wicked-estate leaves                 [--json] [--db ...]
//!   wicked-estate dead-code              [--json] [--db ...]
//!   wicked-estate nodes [--kind K] [--annotated-with K[=V]] [--json] [--semantics] [--db ...]
//!   wicked-estate graph-view [--limit N] [--include-tests] [--include-trivial] [--ignore <pat>] [--db ...]

mod emit;
mod scip_auto;
mod source_bundle;
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
/// Reads the indexed root from store meta; resolves the db path for mtime.
fn maybe_print_staleness(store: &dyn wicked_estate_store::GraphStoreMutExt, db: &str) {
    // A multi-repo graph has one root per repo — check each, and name the label in the fix so the
    // operator re-indexes THAT repo and not whichever one happened to be indexed last.
    let repos = wicked_estate::repo_scope::registry(store);
    if !repos.is_empty() {
        for rec in repos {
            if let Some(n) = wicked_estate::commits_behind(Path::new(&rec.root), db) {
                if n > 0 {
                    println!(
                        "STALENESS: {n} commit(s) in '{label}' since last index — run \
                         `wicked-estate index {root} --repo {label}` to refresh",
                        label = rec.label,
                        root = rec.root,
                    );
                }
            }
        }
        return;
    }
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

/// Warn when the database was indexed under a different binary version.
/// Extraction fixes (e.g. COBOL paragraph spans) are not backfilled — a re-index is required.
/// Annotations are stored separately and are preserved across re-indexes.
fn maybe_warn_version_mismatch(store: &dyn wicked_estate_store::GraphStoreMutExt, db: &str) {
    let current = env!("CARGO_PKG_VERSION");
    // A labelled repo records its binary version under `repo:<label>:indexed_version` and never
    // the bare key, so reading only the bare one made this warning silently unreachable on every
    // multi-repo graph — the graphs most likely to hold rows from a stale binary, since each repo
    // is re-indexed on its own schedule. Warn per repo, and name the label in the fix.
    let repos = wicked_estate::repo_scope::registry(store);
    if !repos.is_empty() {
        for rec in repos {
            let key = wicked_estate::repo_scope::meta_key(Some(&rec.label), "indexed_version");
            let Some(indexed) = store.meta_get_key(&key) else {
                continue;
            };
            if indexed != current {
                eprintln!(
                    "VERSION MISMATCH: '{label}' in {db} was indexed with v{indexed}, current \
                     binary is v{current}. Re-index to apply extraction fixes: \
                     `wicked-estate index {root} --repo {label}` (your annotations are preserved).",
                    label = rec.label,
                    root = rec.root,
                );
            }
        }
        return;
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
             Re-index to apply extraction fixes: `wicked-estate index {root_hint}` \
             (your annotations are preserved)."
        );
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

    // Parse shared flags: `--db <spec>`, `--dbs a,b,c`, and `--scip-file <path>`;
    // everything else is positional.
    //
    // `--db` may be repeated; the LAST single `--db` value is used for single-db commands
    // (backward-compatible).  All `--db` values are collected into `db_paths` for the
    // `cross-graph` command.  `--dbs a,b,c` is an alias that accepts a comma-delimited list.
    //
    // The DEFAULT spec resolves through the WICKED_RUNTIME profile seam
    // (docs/team-runtime.md): team → WICKED_STORE_URL (shared Postgres, needs a
    // `--features postgres` build) > WICKED_ESTATE_DB > the local graph.db. An explicit
    // `--db` flag below still overrides whatever resolves here.
    let mut db =
        wicked_estate_store::resolve_store_spec(None, ".wicked-estate/graph.db").map_err(to_any)?;
    let mut db_paths: Vec<String> = Vec::new();
    let mut scip_file: Option<String> = None;
    let mut since: u64 = 0;
    // history_enabled: OFF by default; opt-in with `--history`.
    let mut history = false;
    // embeddings: OFF by default; opt-in with `--embeddings`.
    let mut embeddings = false;
    // --force: bypass incremental digest skip; re-extract all files even if unchanged.
    let mut force_reindex = false;
    // --repo <name>: index into a MULTI-REPO graph under this label. None = single-repo mode.
    let mut repo_label: Option<String> = None;
    // Semantic-annotation flags for the `semantics` command (requirement↔functionality linking).
    let mut sem_description: Option<String> = None;
    let mut sem_requirement: Option<String> = None;
    let mut sem_validated: Option<bool> = None;
    let mut sem_validated_by: Option<String> = None;
    // Annotation flags for the `annotate` command.
    let mut ann_key: Option<String> = None;
    let mut ann_value: Option<String> = None;
    let mut ann_confidence: f64 = 1.0;
    let mut ann_provenance: String = String::new();
    let mut ann_author: String = String::new();
    // --type <t>: annotation type. Write side (annotate) defaults to `note`; read side
    // (annotations) treats absence as "no filter". A plain string — fixed convention OR custom.
    let mut ann_type: Option<String> = None;
    // --symbol <SymbolId>: target a single node by stable ID (annotate + annotations).
    let mut ann_symbol: Option<String> = None;
    // --replace: idempotent upsert by (type, key) for the `annotate` command. Default OFF =
    // append (today's behavior). When set, delete_annotations(sym, Some(type), key) before the
    // append, so re-projecting a cache-class annotation replaces the row instead of duplicating it.
    let mut ann_replace = false;
    // --content: fingerprint uses body byte-slice hash instead of identity hash.
    let mut fp_content = false;
    // correspond command flags.
    let mut db_a: Option<String> = None;
    let mut db_b: Option<String> = None;
    let mut correspond_top: usize = 20;
    let mut correspond_min_score: f64 = 0.35;
    // --annotated-with KEY or KEY=VALUE: filter nodes by annotation.
    let mut annotated_with: Option<String> = None;
    // `clusters` command tuning — community detection (graph) + semantic clustering.
    let mut cluster_resolution: f64 = 1.0;
    let mut cluster_hierarchical = false;
    let mut cluster_package_bias: f64 = 0.0;
    let mut cluster_weight: String = "graph".to_string();
    // `--summary`: emit enriched per-community objects instead of bare member-id arrays.
    let mut cluster_summary = false;
    // `--annotate`: opt-in mutation — write a `community`-type annotation on every member of every
    // detected community (Chunk 4). Default OFF: `clusters` is read-only unless this is passed.
    let mut cluster_annotate = false;
    let mut cluster_k: Option<usize> = None;
    let mut cluster_eps: f32 = 0.25;
    let mut cluster_min_pts: usize = 3;
    // `source` bundle selectors + budget. Selectors are mutually-exclusive; precedence is
    // resolved in the arm (--symbols > --cluster > --file > <name>).
    let mut src_cluster: Option<usize> = None;
    let mut src_file: Option<String> = None;
    let mut src_symbols: Option<String> = None;
    // Budget caps for the `source` bundle. None = unbounded (the caller owns its context).
    let mut src_max_total: Option<usize> = None;
    let mut src_max_node: Option<usize> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut help_requested = false;
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
            "--force" => {
                force_reindex = true;
            }
            // `--repo <name>` names the repo this run is indexing, so several repos can share
            // one db. Noun-shaped like the CLI's other value flags (`--db`, `--file`,
            // `--symbol`), and it is the word that shows up in `stats`, in the guard's errors,
            // and as the path prefix on every row. `--as` is accepted as an alias.
            // A missing value is a HARD error here, unlike the other value flags. Every other one
            // degrades to a visible default; this one degrades to "index un-labelled", which is a
            // different write to the graph than the caller asked for and looks like success.
            // `--repo --force` fell into the same hole twice: `--force` became the label AND was
            // dropped as a flag.
            "--repo" | "--as" => match it.next() {
                Some(v) if !v.starts_with('-') => repo_label = Some(v.clone()),
                Some(v) => anyhow::bail!(
                    "{a} needs a repo name, got the flag `{v}` — write `{a} <name> {v}`"
                ),
                None => anyhow::bail!("{a} needs a repo name (e.g. `{a} ledger`)"),
            },
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
            "--validated-by" => {
                if let Some(v) = it.next() {
                    sem_validated_by = Some(v.clone());
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
            "--type" => {
                if let Some(v) = it.next() {
                    ann_type = Some(v.clone());
                }
            }
            "--symbol" => {
                if let Some(v) = it.next() {
                    ann_symbol = Some(v.clone());
                }
            }
            "--replace" => {
                ann_replace = true;
            }
            "--content" => {
                fp_content = true;
            }
            "--db-a" => {
                if let Some(v) = it.next() {
                    db_a = Some(v.clone());
                }
            }
            "--db-b" => {
                if let Some(v) = it.next() {
                    db_b = Some(v.clone());
                }
            }
            "--top" => {
                if let Some(v) = it.next() {
                    correspond_top = v.parse::<usize>().unwrap_or(20);
                }
            }
            "--min-score" => {
                if let Some(v) = it.next() {
                    correspond_min_score = v.parse::<f64>().unwrap_or(0.35);
                }
            }
            "--annotated-with" => {
                if let Some(v) = it.next() {
                    annotated_with = Some(v.clone());
                }
            }
            "--resolution" => {
                if let Some(v) = it.next() {
                    cluster_resolution = v.parse::<f64>().unwrap_or(1.0);
                }
            }
            "--hierarchical" => {
                cluster_hierarchical = true;
            }
            "--summary" => {
                cluster_summary = true;
            }
            "--annotate" => {
                cluster_annotate = true;
            }
            "--package-bias" => {
                if let Some(v) = it.next() {
                    cluster_package_bias = v.parse::<f64>().unwrap_or(0.0);
                }
            }
            "--weight" => {
                if let Some(v) = it.next() {
                    cluster_weight = v.clone();
                }
            }
            "--k" => {
                if let Some(v) = it.next() {
                    cluster_k = Some(v.parse::<usize>().unwrap_or(16));
                }
            }
            "--eps" => {
                if let Some(v) = it.next() {
                    cluster_eps = v.parse::<f32>().unwrap_or(0.25);
                }
            }
            "--min-pts" => {
                if let Some(v) = it.next() {
                    cluster_min_pts = v.parse::<usize>().unwrap_or(3);
                }
            }
            "--cluster" => {
                if let Some(v) = it.next() {
                    src_cluster = v.parse::<usize>().ok();
                }
            }
            "--file" => {
                if let Some(v) = it.next() {
                    src_file = Some(v.clone());
                }
            }
            "--symbols" => {
                if let Some(v) = it.next() {
                    src_symbols = Some(v.clone());
                }
            }
            "--max-total-chars" => {
                if let Some(v) = it.next() {
                    src_max_total = v.parse::<usize>().ok();
                }
            }
            "--max-node-chars" => {
                if let Some(v) = it.next() {
                    src_max_node = v.parse::<usize>().ok();
                }
            }
            // Before the catch-all: otherwise these land in `positional` and the `index` arm
            // treats `--help` as a path to walk, printing "indexed --help → 0 nodes" and exiting 0.
            "--help" | "-h" => help_requested = true,
            _ => positional.push(a.clone()),
        }
    }
    // Re-dispatch to the usage arm. `help` matches no command, so it falls through to `_`.
    let cmd = if help_requested { "help" } else { cmd };

    let otel_sink = wicked_estate_observe::init_sink_from_env();
    let otel_resource = wicked_estate_core::observability::Resource::service(
        "wicked_estate",
        env!("CARGO_PKG_VERSION"),
    );
    let otel_scope = wicked_estate_core::observability::InstrumentationScope::versioned(
        "wicked_estate",
        env!("CARGO_PKG_VERSION"),
    );

    match cmd {
        "index" => {
            let path = positional.first().map(String::as_str).unwrap_or(".");
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
            let as_repo = repo_label.as_deref();
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
            let stats = if history && db != ":memory:" {
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
            let as_repo = repo_label.as_deref();
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

            if let Some(explicit) = scip_file.as_deref() {
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
        // Brain consolidation: bulk-import access_log + search_misses telemetry from a JSON file
        // produced by the brain-side export tool. The file shape is `TelemetryImport`
        // (`{ "access_log": [...], "search_misses": [...] }`, both arrays optional). Point `--db`
        // at the target SQLite store file (the knowledge db for knowledge telemetry; both signals
        // are opaque id/query strings so any SQLite store file works — graph or knowledge db).
        // SQLite-only today: the telemetry tables live in schema.sql and the import APIs are
        // SqliteStore methods, so non-SQLite specs fail fast instead of silently creating a junk
        // file named after the connection URL. Additive: never touches nodes/edges.
        "import-telemetry" => {
            let file_path = positional
                .first()
                .context("usage: wicked-estate import-telemetry <file.json> [--db ...]")?;
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
            let name = positional
                .first()
                .context("usage: wicked-estate query <name>")?;
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
            println!("{} match(es) for '{name}':", hits.len());
            for n in &hits {
                println!("  {:?} {} ({})", n.kind, n.name, loc(n));
            }
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.query",
                vec![
                    wicked_estate_core::observability::KeyValue::str("symbol.name", name.as_str()),
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
            let name = positional
                .first()
                .context("usage: wicked-estate blast-radius <name>")?;
            let store = open_store_ext(&db).map_err(to_any)?;
            let json_out = positional.iter().any(|a| a == "--json");
            // Machine output must be exactly one JSON document — notices would corrupt it.
            if !json_out {
                maybe_print_staleness(store.as_ref(), &db);
                maybe_warn_version_mismatch(store.as_ref(), &db);
            }
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let deps = wicked_estate::blast_radius_by_name(&*store, name, 12).map_err(to_any)?;
            let unresolved = store.unresolved_refs_for_name(name).map_err(to_any)?.len();
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            if json_out {
                // Machine consumers (wicked-crew studio) get the same honesty contract as the
                // text path: dependents PLUS the unresolved count — absence of dependents must
                // never silently read as "safe to change".
                let out = serde_json::json!({
                    "target": name,
                    "dependents": deps
                        .iter()
                        .map(|n| serde_json::json!({
                            "id": n.symbol.as_str(),
                            "name": n.name,
                            "kind": &n.kind,
                            "file": n.location.file,
                            "line": n.location.span.start_line + 1,
                        }))
                        .collect::<Vec<_>>(),
                    "unresolved": unresolved,
                });
                println!(
                    "{}",
                    serde_json::to_string(&out).map_err(|e| anyhow::anyhow!(e))?
                );
            } else if deps.is_empty() {
                println!("no resolved dependents for '{name}' (symbol may not be indexed)");
            } else {
                println!("{} symbol(s) depend on '{name}':", deps.len());
                for n in &deps {
                    println!("  {:?} {} ({})", n.kind, n.name, loc(n));
                }
            }
            // Honest coverage — never let the absence of dependents read as "safe to change".
            if !json_out {
                println!(
                    "coverage: {} resolved dependent(s); {unresolved} unresolved call(s) reference \
                     '{name}' — best-effort static resolution, MAY be incomplete (precise tier pending)",
                    deps.len()
                );
            }
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.blast_radius",
                vec![
                    wicked_estate_core::observability::KeyValue::str("symbol.name", name.as_str()),
                    wicked_estate_core::observability::KeyValue::int(
                        "dependent.count",
                        deps.len() as i64,
                    ),
                ],
                t_cmd_start,
                t_cmd_end,
            );
        }
        "stats" => {
            let store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            maybe_warn_version_mismatch(store.as_ref(), &db);
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
        "rank" | "hotspots" => {
            let store = open_store_ext(&db).map_err(to_any)?;
            let t_cmd_start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let top = wicked_estate::important_symbols(store.as_ref(), 25).map_err(to_any)?;
            let t_cmd_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            println!("top {} symbols by PageRank:", top.len());
            for (n, score) in &top {
                println!("  {score:.4}  {:?} {} ({})", n.kind, n.name, loc(n));
            }
            emit_cli_span(
                &otel_sink,
                &otel_resource,
                &otel_scope,
                "wicked_estate.rank",
                vec![wicked_estate_core::observability::KeyValue::int(
                    "symbol.count",
                    top.len() as i64,
                )],
                t_cmd_start,
                t_cmd_end,
            );
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

            let mut limit = 80usize;
            let mut include_tests = false;
            let mut include_trivial = false;
            let mut focus: Option<String> = None;
            let mut ignore_patterns: Vec<String> = Vec::new();
            {
                let mut it = positional.iter();
                while let Some(a) = it.next() {
                    match a.as_str() {
                        "--limit" => {
                            if let Some(v) = it.next() {
                                limit = v.parse().unwrap_or(80);
                            }
                        }
                        "--focus" => match it.next() {
                            Some(v) => focus = Some(v.clone()),
                            None => anyhow::bail!("graph-view --focus requires a value"),
                        },
                        "--include-tests" => include_tests = true,
                        "--include-trivial" => include_trivial = true,
                        "--ignore" => {
                            if let Some(p) = it.next() {
                                ignore_patterns.push(p.clone());
                            }
                        }
                        _ => {}
                    }
                }
            };

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
            let excluded = [
                NodeKind::File,
                NodeKind::Module,
                NodeKind::Namespace,
                NodeKind::Import,
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
                        limit: Some(5),
                        ..Default::default()
                    };
                    seeds.extend(store.find_symbols(&q).map_err(to_any)?);
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
            // in_deg_map[Y]  = number of top-N nodes with a Calls/Imports edge pointing to Y
            //                  (within-set in-degree, appropriate for layout sizing in the UI).
            // This halves store calls vs. a separate per-node Dependents query per node.
            let mut edges_json: Vec<serde_json::Value> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
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
                        let key = format!("{}→{}", e.source.as_str(), e.target.as_str());
                        if seen.insert(key) {
                            edges_json.push(serde_json::json!({
                                "src": e.source.as_str(),
                                "tgt": e.target.as_str(),
                            }));
                        }
                    }
                }
            }

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
            let json_out = positional.iter().any(|a| a == "--json");
            let signatures_only = positional.iter().any(|a| a == "--signatures-only");
            // The positional <name> is the first arg that is not a recognised bare flag.
            let name = positional
                .iter()
                .find(|a| !a.starts_with("--"))
                .map(String::as_str);
            let store = open_store(&db).map_err(to_any)?;

            if !json_out {
                // ── Legacy text path (unchanged): fuzzy <name> → bodies to stdout. ──
                let name = name.context("usage: wicked-estate source <name>")?;
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
                            None => {
                                println!("  (source not stored — re-run 'index' to populate)")
                            }
                        }
                        println!();
                    }
                }
            } else {
                // ── JSON bundle path: resolve the selector, build the bundle, print it. ──
                // Precedence: --symbols > --cluster > --file > <name>.
                let (nodes, selector): (Vec<wicked_estate_core::Node>, serde_json::Value) =
                    if let Some(csv) = &src_symbols {
                        let ids: Vec<String> = csv
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let mut out = Vec::new();
                        for id in &ids {
                            let sid = wicked_estate_core::symbol::SymbolId::from(id.as_str());
                            if let Some(n) = store.get_node(&sid).map_err(to_any)? {
                                out.push(n);
                            }
                        }
                        (out, serde_json::json!({ "symbols": ids }))
                    } else if let Some(cid) = src_cluster {
                        // Members of community `cid` (index into detect_communities, largest-first).
                        let params = wicked_estate_rank::CommunityParams::default();
                        let communities = wicked_estate_rank::detect_communities(&*store, &params)
                            .map_err(to_any)?;
                        let members = communities.get(cid).cloned().unwrap_or_default();
                        let mut out = Vec::new();
                        for sid in &members {
                            if let Some(n) = store.get_node(sid).map_err(to_any)? {
                                out.push(n);
                            }
                        }
                        (out, serde_json::json!({ "cluster": cid }))
                    } else if let Some(path) = &src_file {
                        let all = store.all_nodes().map_err(to_any)?;
                        let out: Vec<_> = all
                            .into_iter()
                            .filter(|n| &n.location.file == path)
                            .collect();
                        (out, serde_json::json!({ "file": path }))
                    } else {
                        let name = name.context(
                            "usage: wicked-estate source [<name>] [--cluster <id>] \
                             [--file <path>] [--symbols id1,id2,...] --json",
                        )?;
                        let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
                        (hits, serde_json::json!({ "name": name }))
                    };

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

            let as_repo = repo_label.as_deref();
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
                    "usage: wicked-estate semantics <symbol> [--description X] [--requirement Y] [--validated true|false --validated-by <actor>] [--db ...]"
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
                        sem_validated_by.as_deref(),
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
            let min_size = positional
                .iter()
                .find(|a| a.parse::<usize>().is_ok())
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(2);
            let json_out = positional.iter().any(|a| a == "--json");
            // `--annotate` needs the write side; bind mutably (read methods still work via as_ref).
            let mut store = open_store_ext(&db).map_err(to_any)?;
            maybe_print_staleness(store.as_ref(), &db);
            maybe_warn_version_mismatch(store.as_ref(), &db);

            let semantic = cluster_weight == "semantic";
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
                        hierarchical: cluster_hierarchical,
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
            if cluster_annotate {
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
                if cluster_summary && !semantic {
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
            let key = ann_key
                .as_deref()
                .context("--key is required for the annotate command")?;
            let value = ann_value
                .as_deref()
                .context("--value is required for the annotate command")?;
            let ty = ann_type.as_deref().unwrap_or(DEFAULT_ANNOTATION_TYPE);
            ensure_db_dir(&db)?;
            let mut store = SqliteStore::open(&db).map_err(to_any)?;
            // Build the typed annotation once; clone per target. ts=0 → store stamps it.
            let make = |sym_present_value: &str| {
                Annotation::new(ty, key, sym_present_value)
                    .with_confidence(ann_confidence)
                    .with_provenance(ann_provenance.clone())
                    .with_author(ann_author.clone())
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
            if let Some(sym_str) = &ann_symbol {
                let symbol = wicked_estate_core::symbol::SymbolId::from(sym_str.as_str());
                replaced += upsert(&mut store, &symbol)?;
                count = 1;
            } else {
                let name = positional.first().context(
                    "usage: wicked-estate annotate <name> --key K --value V [--type T] [--replace] [--db ...]\n       \
                     wicked-estate annotate --symbol <id> --key K --value V [--type T] [--replace] [--db ...]",
                )?;
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
        // shape `{symbol, annotations:[{type,key,value,confidence,provenance,author,ts,advisory}]}`
        // — one object per matched symbol (an array under `<name>`, a single object under
        // `--symbol`). `advisory:true` is emitted for assumption/question (computed from `type`,
        // not hard-coded). This direct read is NOT R4-capped — only structured payloads are.
        "annotations" => {
            let json_out = positional.iter().any(|a| a == "--json");
            let type_filter = ann_type.as_deref();
            // ADR-003: route through the open_store factory (backend-agnostic) — this arm
            // needs only GraphRead methods, which deref through Box<dyn GraphStore>.
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
                    "annotations": anns.iter().map(source_bundle::annotation_json).collect::<Vec<_>>(),
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

            if let Some(sym_str) = &ann_symbol {
                let symbol = wicked_estate_core::symbol::SymbolId::from(sym_str.as_str());
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
                let name = positional.first().context(
                    "usage: wicked-estate annotations <name> [--type T] [--json] [--db ...]\n       \
                     wicked-estate annotations --symbol <id> [--type T] [--json] [--db ...]",
                )?;
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
        // is strictly before <cutoff> (Unix-seconds) — i.e. the facts a re-verification window
        // deems stale. Never-verified rows (last_verified == 0) are stale for any positive cutoff.
        // Thin surface over the `GraphRead::annotations_stale_since` seam (ordered symbol then ts).
        //
        // Usage:
        //   wicked-estate stale-annotations <cutoff> [--json] [--db ...]
        "stale-annotations" => {
            let json_out = positional.iter().any(|a| a == "--json");
            let cutoff: i64 = positional
                .iter()
                .find_map(|a| a.parse::<i64>().ok())
                .context(
                    "usage: wicked-estate stale-annotations <cutoff-unix-seconds> [--json] [--db ...]",
                )?;
            // ADR-003: backend-agnostic factory — annotations_stale_since is a GraphRead method.
            let store = open_store(&db).map_err(to_any)?;
            let stale = store.annotations_stale_since(cutoff).map_err(to_any)?;
            if json_out {
                let arr: Vec<serde_json::Value> = stale
                    .iter()
                    .map(|(sym, a)| {
                        serde_json::json!({
                            "symbol": sym.to_string(),
                            "annotation": source_bundle::annotation_json(a),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if stale.is_empty() {
                println!("no annotations stale as of cutoff {cutoff}");
            } else {
                println!(
                    "{} annotation(s) stale as of cutoff {cutoff} (last_verified < {cutoff}):",
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
            let name = positional
                .first()
                .context("usage: wicked-estate fingerprint <name> [--content] [--db ...]")?;
            let store = open_store(&db).map_err(to_any)?;
            let hits = wicked_estate::search(&*store, name).map_err(to_any)?;
            drop(store);
            if hits.is_empty() {
                println!("no symbol found matching '{name}'");
                return Ok(());
            }
            if fp_content {
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
            let json_out = positional.iter().any(|a| a == "--json");
            if changed_files.is_empty() {
                if json_out {
                    println!("[]");
                } else {
                    println!("no files changed since {sha}");
                }
                return Ok(());
            }
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
        // Agent E: nodes — bulk export all symbols, optionally filtered by kind or annotation.
        //
        // Usage:
        //   wicked-estate nodes [--kind K] [--annotated-with K[=V]] [--json] [--semantics] [--db ...]
        "nodes" => {
            use wicked_estate_core::GraphRead;
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
            // Opt-in: `nodes --json --semantics` adds four extra per-node keys the domain-brain
            // extraction engine needs — `rule_confidence`, `requirement`, `requirement_validated`,
            // `out_edges`. OFF by default so the plain `nodes --json` path pays neither the
            // per-node `get_semantics` read nor the `neighbors` edge fetch (and its shape is
            // unchanged for existing consumers).
            let with_semantics = positional.iter().any(|a| a == "--semantics");
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
                    "annotation_summary": source_bundle::annotation_summary(&all_anns),
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
                    let capped: Vec<serde_json::Value> =
                        source_bundle::cap_annotations_for_payload(all_anns)
                            .iter()
                            .map(source_bundle::annotation_json)
                            .collect();
                    obj["annotations"] = serde_json::Value::Array(capped);
                }
                obj
            };

            if let Some(ann_filter) = &annotated_with {
                // --annotated-with KEY or KEY=VALUE
                let (ann_key, ann_val) = if let Some((k, v)) = ann_filter.split_once('=') {
                    (k, Some(v))
                } else {
                    (ann_filter.as_str(), None)
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
            let json_out = positional.iter().any(|a| a == "--json");
            // `--file` is globally parsed into `src_file`; `--kind` lands in `positional` (like `nodes`).
            let file_filter = src_file.clone();
            let mut kind_filter: Option<String> = None;
            let mut name: Option<String> = None;
            let mut it2 = positional.iter();
            while let Some(a) = it2.next() {
                match a.as_str() {
                    "--json" => {}
                    "--kind" => kind_filter = it2.next().cloned(),
                    other => {
                        if name.is_none() {
                            name = Some(other.to_string());
                        }
                    }
                }
            }
            let name =
                name.context("usage: wicked-estate resolve <name> [--file F] [--kind K] [--json]")?;

            // Brain-facing read surface → route through the open_store factory so it
            // is backend-agnostic (postgres:// under --features postgres) per ADR-003,
            // rather than pinning a new caller to SqliteStore. resolve only needs
            // GraphRead::find_symbols, a GraphStore supertrait method, so Box<dyn
            // GraphStore> derefs cleanly. (The other read arms are pre-existing debt —
            // a dedicated open_store migration, not this PHASE-1 surface's job.)
            let store = open_store(&db).map_err(to_any)?;
            let q = SymbolQuery {
                exact_name: Some(name.clone()),
                ..Default::default()
            };
            let mut nodes = store.find_symbols(&q).map_err(to_any)?;
            if let Some(f) = &file_filter {
                nodes.retain(|n| &n.location.file == f);
            }
            if let Some(k) = &kind_filter {
                let kl = k.to_lowercase();
                nodes.retain(|n| format!("{:?}", n.kind).to_lowercase() == kl);
            }

            if json_out {
                let rows: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "symbol_id": n.symbol.to_string(),
                            "name": n.name,
                            "kind": format!("{:?}", n.kind),
                            "file": n.location.file,
                            "line": n.location.span.start_line + 1,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                println!("{} match(es) for '{name}':", nodes.len());
                for n in &nodes {
                    println!(
                        "  {} {:?} ({}:{})",
                        n.name,
                        n.kind,
                        n.location.file,
                        n.location.span.start_line + 1
                    );
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

            let path_a = db_a
                .as_deref()
                .context("--db-a <path> is required for the correspond command")?;
            let path_b = db_b
                .as_deref()
                .context("--db-b <path> is required for the correspond command")?;

            let json_out = positional.iter().any(|a| a == "--json");
            let explain = positional.iter().any(|a| a == "--explain");
            let filter_kind = {
                let mut k: Option<String> = None;
                let mut it2 = positional.iter();
                while let Some(a) = it2.next() {
                    if a.as_str() == "--kind" {
                        k = it2.next().cloned();
                    }
                }
                k
            };

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
                        .as_deref()
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
            let format = {
                let mut f = "ndjson".to_string();
                let mut it2 = positional.iter();
                while let Some(a) = it2.next() {
                    if a.as_str() == "--format" {
                        f = it2.next().cloned().unwrap_or_else(|| "ndjson".to_string());
                    }
                }
                f
            };
            let nodes_only = positional.iter().any(|a| a == "--nodes-only");
            let edges_only = positional.iter().any(|a| a == "--edges-only");

            // ADR-003: backend-agnostic factory — all_nodes/all_edges are GraphRead methods.
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

            match format.as_str() {
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
            let sub = positional.first().map(String::as_str).unwrap_or("list");
            match sub {
                "list" => {
                    if let Some(d) = wicked_estate_extract::plugin::plugins_dir() {
                        println!("plugins dir: {}", d.display());
                    }
                    let loaded = wicked_estate_extract::plugin::loaded();
                    if loaded.is_empty() {
                        println!(
                            "(no plugins loaded — drop a plugin dir into the plugins dir; see PLUGIN.md)"
                        );
                    } else {
                        for p in loaded {
                            println!(
                                "{}  exts=[{}]  license={}",
                                p.name,
                                p.extensions.join(", "),
                                p.license.as_deref().unwrap_or("unspecified"),
                            );
                        }
                    }
                }
                other => {
                    eprintln!(
                        "unknown `plugins` subcommand `{other}` (try: wicked-estate plugins list)"
                    );
                }
            }
        }
        _ => {
            println!("wicked-estate {} — usage:", env!("CARGO_PKG_VERSION"));
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
                "    Bulk selectors (mutually exclusive; precedence --symbols > --cluster > --file > <name>):"
            );
            println!(
                "      --cluster <id>        all symbols in community <id> (see `clusters` output)"
            );
            println!("      --file <path>         all symbols whose location.file == path");
            println!("      --symbols <ids>       comma-separated SymbolIds (exact)");
            println!(
                "    Output options: --json  --signatures-only  --max-total-chars <N>  --max-node-chars <N>"
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
            println!("  wicked-estate annotations <name>   [--type T] [--json] [--db ...]");
            println!(
                "    Show annotations for matching symbols. --type filters; --json emits {{symbol, annotations:[...]}} with an `advisory` flag."
            );
            println!(
                "  wicked-estate stale-annotations <cutoff> [--json] [--db ...]  # (symbol, annotation) pairs with last_verified < cutoff"
            );
            println!(
                "    Evidence-envelope freshness read: \"what needs re-verification?\". Never-verified rows (last_verified=0) are always stale."
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
                "  wicked-estate resolve <name> [--file F] [--kind K] [--json]  # name → [{{symbol_id,name,kind,file,line}}]"
            );
            println!(
                "    Resolve a simple name to its stable SymbolId(s) before an --symbol write (names are not unique)."
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
