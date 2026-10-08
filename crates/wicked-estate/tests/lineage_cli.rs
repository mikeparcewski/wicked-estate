//! `wicked-estate lineage` end to end, through the real binary (TS-S1B).
//!
//! The CLI is a frontend over `wicked_estate_retrieve::Lineage`, so the oracle here is not a
//! renderer this crate owns: every `--json` document is compared with (a) a direct `Lineage`
//! invocation and (b) the MCP `tools/call` response, parsed back from its two text blocks into
//! `{content, diagnostics}`. Both sides read the same indexed SQLite file, built by the binary's
//! own `index` over a real TypeScript fixture.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use wicked_estate_core::{GraphRead, Node, RetrievalTool};
use wicked_estate_mcp::{McpContext, handle_request_unified};
use wicked_estate_retrieve::Lineage;
use wicked_estate_store::SqliteStore;

const R4_CHAR_BUDGET: usize = 25_000;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wicked-estate")
}

/// Owns a scratch directory and removes it on drop.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("ci_lineagecli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let s = Scratch(d);
    fs::create_dir_all(&*s).unwrap();
    s
}

/// The binary, run inside `dir`, hermetically: the `index` event emitter points at a missing
/// program and a spool inside `dir` (no `wicked-bus`, no write to `$HOME`); the store-selection
/// and telemetry variables the binary reads are cleared; and git discovery stops at `dir`, so a
/// scratch dir under a git checkout cannot turn on staleness notices.
fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .args(args)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-lineage-cli",
        )
        .env("WICKED_ESTATE_EMIT_DEADLETTER", dir.join("emit.ndjson"))
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env_remove("WICKED_OTEL_ENDPOINT")
        .env_remove("WICKED_OTEL_HEADERS")
        .env_remove("WICKED_ESTATE_DB")
        .env_remove("WICKED_STORE_URL")
        .env_remove("WICKED_RUNTIME")
        .output()
        .expect("spawn wicked-estate")
}

fn index(dir: &Path) {
    let out = run(dir, &["index", ".", "--db", "graph.db"]);
    assert!(
        out.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The committed Angular S1 fixture (`tests/fixtures/typescript-value-lineage`), indexed by the
/// binary into a scratch db.
fn indexed_angular(tag: &str) -> Scratch {
    let s = scratch(tag);
    let src =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    for entry in fs::read_dir(&src).unwrap() {
        let p = entry.unwrap().path();
        fs::copy(&p, s.join(p.file_name().unwrap())).unwrap();
    }
    index(&s);
    s
}

fn open(dir: &Path) -> SqliteStore {
    SqliteStore::open(dir.join("graph.db").to_str().unwrap()).expect("open indexed db")
}

fn node_where(store: &SqliteStore, pred: impl Fn(&Node) -> bool, what: &str) -> String {
    let hits: Vec<Node> = GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|n| pred(n))
        .collect();
    assert_eq!(hits.len(), 1, "expected exactly one {what}, got {hits:?}");
    hits[0].symbol.as_str().to_string()
}

/// `wicked-estate lineage … --json`: asserts exit 0, stdout is exactly one JSON document with
/// exactly the `RetrievalResult` keys, and stderr carries no prose.
fn cli_json(dir: &Path, args: &[&str]) -> Value {
    let mut argv = vec!["lineage"];
    argv.extend_from_slice(args);
    argv.extend_from_slice(&["--json", "--db", "graph.db"]);
    let out = run(dir, &argv);
    assert!(
        out.status.success(),
        "lineage {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "--json must print exactly one line: {stdout}"
    );
    let doc: Value = serde_json::from_str(&stdout).expect("--json stdout must be JSON");
    let keys: Vec<&str> = doc
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, vec!["content", "diagnostics"], "{doc}");
    assert!(
        out.stderr.is_empty(),
        "--json must not print notices: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    doc
}

/// The in-memory tool result, normalized through the same serializer the CLI and MCP print with.
/// Without the round trip the comparison depends on `serde_json`'s best-effort float parsing: a
/// `confidence.avg` such as `0.9093789458274841` reparses one ulp off (`…484`) unless the
/// `float_roundtrip` feature is on, so `assert_eq!(cli, direct)` could fail on a richer fixture
/// with byte-identical output (15 of 850 cases on a real repo).
fn direct(store: &SqliteStore, args: &Value) -> Value {
    let r = Lineage.invoke(store, args).unwrap();
    let doc = json!({ "content": r.content, "diagnostics": r.diagnostics });
    serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap()
}

/// The MCP `tools/call` response for `Lineage`, normalized back into `{content, diagnostics}`:
/// the first text block is the serialized content, the optional second is the diagnostics joined
/// by `\n` (`handle_tools_call_ctx`).
fn mcp(store: &SqliteStore, args: &Value) -> Value {
    mcp_with(store, args, &McpContext::default())
}

fn mcp_with(store: &SqliteStore, args: &Value, ctx: &McpContext) -> Value {
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "Lineage", "arguments": args },
    });
    let resp = handle_request_unified(store, &req, ctx, None, None);
    let result = &resp["result"];
    assert_eq!(result["isError"], json!(false), "{resp}");
    let blocks = result["content"].as_array().unwrap();
    assert!(blocks.len() <= 2, "{resp}");
    let content: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    let diagnostics: Vec<String> = blocks
        .get(1)
        .map(|b| {
            b["text"]
                .as_str()
                .unwrap()
                .split('\n')
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    json!({ "content": content, "diagnostics": diagnostics })
}

/// The whole normalized result must agree across the three paths. A diagnostic containing `\n`
/// would make the MCP split ambiguous, so that is ruled out first.
fn assert_parity(dir: &Path, store: &SqliteStore, cli_args: &[&str], args: Value) -> Value {
    let cli = cli_json(dir, cli_args);
    for d in cli["diagnostics"].as_array().unwrap() {
        assert!(!d.as_str().unwrap().contains('\n'), "{d}");
    }
    assert_eq!(
        cli,
        direct(store, &args),
        "CLI vs direct Lineage for {args}"
    );
    assert_eq!(cli, mcp(store, &args), "CLI vs MCP Lineage for {args}");
    cli
}

fn hops(doc: &Value) -> Vec<(String, String)> {
    doc["content"]["flows"]
        .as_array()
        .expect("flows_to mode returns `flows`")
        .iter()
        .map(|h| {
            (
                h["producer"].as_str().unwrap().to_string(),
                h["consumer"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn dep_ids(doc: &Value) -> BTreeSet<String> {
    doc["content"]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["symbol"].as_str().unwrap().to_string())
        .collect()
}

fn assert_horizon_keys(doc: &Value) {
    for key in [
        "depth_horizon_reached",
        "node_cap_reached",
        "searched_depth",
    ] {
        assert!(
            doc["content"].get(key).is_some(),
            "pre-S1 horizon key `{key}` missing: {doc}"
        );
    }
}

// ── parity ───────────────────────────────────────────────────────────────────

#[test]
fn flows_to_matches_mcp_and_direct_lineage_on_the_angular_fixture() {
    let s = indexed_angular("flows_parity");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");

    let doc = assert_parity(
        &s,
        &store,
        &["--symbol", &route, "--relation", "flows_to", "--depth", "8"],
        json!({"symbol": route, "depth": 8, "relation": "flows_to"}),
    );
    let c = &doc["content"];
    assert_horizon_keys(&doc);
    assert_eq!(c["searched_depth"], json!(8));

    // The AC-0004 chain, producer -> consumer, every hop present with its full evidence.
    let id = |suffix: &str| node_where(&store, |n| n.symbol.as_str().ends_with(suffix), suffix);
    let route_id = id("CustomerComponent#load().:local:routeId:");
    let field = id("CustomerComponent#:field:customerId:");
    let load_customer_id = id("CustomerComponent#loadCustomer().:local:id:");
    let service_id = id("CustomerService#getCustomer().:local:id:");
    let got = hops(&doc);
    for pair in [
        (route.clone(), route_id.clone()),
        (route_id, field.clone()),
        (field, load_customer_id.clone()),
        (load_customer_id, service_id),
    ] {
        assert!(got.contains(&pair), "missing hop {pair:?} in {got:?}");
    }
    for h in c["flows"].as_array().unwrap() {
        for key in [
            "flow_semantics",
            "flow_evidence",
            "constructs",
            "flow_rules",
            "flow_support",
        ] {
            assert!(
                h[key].as_array().is_some_and(|a| !a.is_empty()),
                "hop lacks `{key}`: {h}"
            );
        }
        assert!(
            h["confidence"].is_number() && h["resolved_by"].is_string(),
            "{h}"
        );
        assert!(h["file"].is_string() && h["line"].is_number(), "{h}");
    }

    // The summary describes exactly the flow hops: the slots' `File` `Contains` edges (and any
    // other relation the traversal touched) are not counted.
    let confs: Vec<f64> = c["flows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["confidence"].as_f64().unwrap())
        .collect();
    assert_eq!(c["confidence"]["edge_count"], json!(confs.len()));
    let min = confs.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!((c["confidence"]["min"].as_f64().unwrap() - min).abs() < 1e-6);
    let avg = confs.iter().sum::<f64>() / confs.len() as f64;
    assert!((c["confidence"]["avg"].as_f64().unwrap() - avg).abs() < 1e-5);
    assert_eq!(
        c["total"],
        json!(c["dependencies"].as_array().unwrap().len())
    );
    assert_eq!(c["truncated"], json!(false));
}

#[test]
fn default_dependency_lineage_is_the_unchanged_tool_result() {
    let s = indexed_angular("default_parity");
    let store = open(&s);
    let load = node_where(
        &store,
        |n| n.name == "load" && !n.is_value_flow_node(),
        "structural `load` method",
    );

    // No `--depth`: the tool's own default applies, identically on every path.
    let doc = assert_parity(&s, &store, &["--symbol", &load], json!({"symbol": load}));
    assert!(
        doc["content"].get("flows").is_none(),
        "default lineage must not gain the flows_to evidence array: {doc}"
    );
    assert_horizon_keys(&doc);
    assert_eq!(doc["content"]["searched_depth"], json!(8));
    assert!(
        dep_ids(&doc)
            .iter()
            .any(|d| d.ends_with("CustomerComponent#loadCustomer().")),
        "load -> loadCustomer is a resolved Calls dependency: {doc}"
    );
}

#[test]
fn a_depth_one_answer_excludes_frontier_hops() {
    let s = indexed_angular("depth_one");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");

    let doc = assert_parity(
        &s,
        &store,
        &["--symbol", &route, "--relation", "flows_to", "--depth=1"],
        json!({"symbol": route, "depth": 1, "relation": "flows_to"}),
    );
    let deps = dep_ids(&doc);
    assert_eq!(deps.len(), 1, "{doc}");
    let mut answered = deps.clone();
    answered.insert(route.clone());
    let got = hops(&doc);
    assert_eq!(got.len(), 1, "{doc}");
    for (p, c) in &got {
        assert!(
            answered.contains(p) && answered.contains(c),
            "hop {p} -> {c} has an end outside the answer"
        );
    }
    assert_eq!(doc["content"]["confidence"]["edge_count"], json!(1));
    assert_eq!(doc["content"]["depth_horizon_reached"], json!(true));
    assert_eq!(doc["content"]["truncated"], json!(true));
    assert_horizon_keys(&doc);
}

#[test]
fn an_exact_value_slot_id_is_accepted_although_name_search_hides_it() {
    let s = indexed_angular("value_slot");
    let store = open(&s);
    let route_id = node_where(
        &store,
        |n| {
            n.symbol
                .as_str()
                .ends_with("CustomerComponent#load().:local:routeId:")
        },
        "routeId slot",
    );

    let doc = assert_parity(
        &s,
        &store,
        &["--symbol", &route_id, "--relation", "flows_to"],
        json!({"symbol": route_id, "relation": "flows_to"}),
    );
    assert!(
        dep_ids(&doc)
            .iter()
            .any(|d| d.ends_with("CustomerComponent#:field:customerId:")),
        "{doc}"
    );

    // The name-oriented consumer still hides the slot: the exact id is the only way in.
    let q = run(&s, &["query", "routeId", "--db", "graph.db"]);
    assert!(q.status.success());
    let q = String::from_utf8(q.stdout).unwrap();
    assert!(q.contains("0 match(es) for 'routeId'"), "{q}");
}

#[test]
fn an_absent_exact_id_is_an_honest_empty_result_not_an_error() {
    let s = indexed_angular("absent");
    let store = open(&s);
    for (cli, args) in [
        (
            vec!["--symbol", "no such symbol#"],
            json!({"symbol": "no such symbol#"}),
        ),
        (
            vec!["--symbol", "no such symbol#", "--relation", "flows_to"],
            json!({"symbol": "no such symbol#", "relation": "flows_to"}),
        ),
    ] {
        let doc = assert_parity(&s, &store, &cli, args);
        assert_eq!(doc["content"]["total"], json!(0), "{doc}");
        assert!(
            doc["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d.as_str().unwrap().contains("no such symbol#")),
            "{doc}"
        );
    }
}

// ── R4: one shared budget, owned by the tool ────────────────────────────────

#[test]
fn dependencies_and_flows_share_the_tools_single_budget() {
    let s = scratch("budget");
    let mut src = String::from("export function fan(seed: string) {\n");
    for i in 0..300 {
        src.push_str(&format!(
            "    const consumer_with_a_deliberately_long_descriptive_name_{i} = seed;\n"
        ));
    }
    src.push_str("}\n");
    fs::write(s.join("fan.ts"), src).unwrap();
    index(&s);
    let store = open(&s);
    let seed = node_where(
        &store,
        |n| n.is_value_flow_node() && n.symbol.as_str().ends_with("fan().:local:seed:"),
        "seed slot",
    );

    let doc = assert_parity(
        &s,
        &store,
        &["--symbol", &seed, "--relation", "flows_to", "--depth", "1"],
        json!({"symbol": seed, "depth": 1, "relation": "flows_to"}),
    );
    let c = &doc["content"];
    let deps = c["dependencies"].as_array().unwrap().len();
    let flows = c["flows"].as_array().unwrap().len();
    assert!(deps > 0 && flows > 0, "neither array may be starved");
    assert!(
        deps < 300 || flows < 300,
        "the fixture must overflow the budget"
    );
    assert_eq!(c["truncated"], json!(true), "a dropped row sets truncated");
    let content_len = serde_json::to_string(c).unwrap().len();
    assert!(
        content_len <= R4_CHAR_BUDGET,
        "content is {content_len} chars; the CLI adds no second budget and must not exceed the tool's"
    );
}

// ── transport: text vs JSON, usage, help ────────────────────────────────────

#[test]
fn text_mode_is_prose_and_json_mode_is_only_json() {
    let s = indexed_angular("modes");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");

    let out = run(
        &s,
        &[
            "lineage",
            "--symbol",
            &route,
            "--relation",
            "flows_to",
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        serde_json::from_str::<Value>(&stdout).is_err() && !stdout.contains("\"content\""),
        "text mode must not print the JSON document: {stdout}"
    );
    assert!(stdout.contains("not taint analysis"), "{stdout}");
    assert!(stdout.contains("semantics=value_preserving"), "{stdout}");
    assert!(stdout.contains("evidence=convention"), "{stdout}");
    assert!(stdout.contains("(tree-sitter-convention)"), "{stdout}");
    assert!(!stdout.contains("note:"), "{stdout}");
    // The tool's `STALENESS: commits_behind not available at this layer …` diagnostic is the
    // retrieval layer's cue to its host, not a user notice: this frontend runs the real check
    // itself and prints it on stdout (see the stale-graph test), so the cue is not echoed.
    assert!(
        !stderr.contains("STALENESS"),
        "the retrieve placeholder must not reach the user: {stderr}"
    );

    // The tool's other diagnostics still go to stderr as `note: …`.
    let out = run(
        &s,
        &["lineage", "--symbol", "absent-id", "--db", "graph.db"],
    );
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("note: Lineage: no dependencies found for 'absent-id'"),
        "diagnostics go to stderr: {stderr}"
    );
    assert!(!stderr.contains("STALENESS"), "{stderr}");

    // `cli_json` asserts the converse: one JSON line, nothing on stderr.
    cli_json(&s, &["--symbol", &route, "--relation", "flows_to"]);
}

#[test]
fn malformed_arguments_fail_before_any_query() {
    let s = scratch("usage");
    // Every case is the complete argv. Several are the shared-loop hazards: a flag another
    // command owns (`--file`, `--top`, `--type`, `--force`) must not be swallowed and ignored, a
    // dangling `--db` must not fall back to the default store, and `--db --json` must not open a
    // file named `--json`.
    let cases: &[(&[&str], &str)] = &[
        (
            &["lineage", "--db", "graph.db"],
            "--symbol <SYMBOL_ID> is required",
        ),
        (
            &["lineage", "--db", "graph.db", "--symbol"],
            "--symbol requires a value",
        ),
        (
            &["lineage", "--db", "graph.db", "--symbol", "--json"],
            "--symbol needs a value, got the flag \"--json\"",
        ),
        (
            &["lineage", "--db", "graph.db", "--symbol", ""],
            "--symbol must not be empty",
        ),
        (
            &[
                "lineage", "--symbol", "a", "--symbol", "b", "--db", "graph.db",
            ],
            "--symbol given more than once",
        ),
        (
            &["lineage", "--symbol", "x", "--depth"],
            "--depth requires a value",
        ),
        (
            &["lineage", "--symbol", "x", "--depth", "deep"],
            "--depth must be a non-negative integer",
        ),
        (
            &["lineage", "--symbol", "x", "--depth", "-1"],
            "--depth must be a non-negative integer",
        ),
        (
            &["lineage", "--symbol", "x", "--depth", "+5"],
            "--depth must be a non-negative integer",
        ),
        (
            &["lineage", "--symbol", "x", "--depth", "25"],
            "--depth 25 is above the maximum of 24",
        ),
        (
            &["lineage", "--symbol", "x", "--depth", "2", "--depth", "3"],
            "--depth given more than once",
        ),
        (
            &["lineage", "--symbol", "x", "--relation"],
            "--relation requires a value",
        ),
        (
            &["lineage", "--symbol", "x", "--relation", "calls"],
            "unsupported --relation \"calls\"",
        ),
        (
            &["lineage", "--symbol", "x", "--relation=FLOWS_TO"],
            "unsupported --relation \"FLOWS_TO\"",
        ),
        (
            &["lineage", "--symbol", "x", "--bogus"],
            "unknown flag \"--bogus\"",
        ),
        (
            &["lineage", "--symbol", "x", "--file", "a.ts"],
            "unknown flag \"--file\"",
        ),
        (
            &["lineage", "--symbol", "x", "--top", "5"],
            "unknown flag \"--top\"",
        ),
        (
            &["lineage", "--symbol", "x", "--type", "t"],
            "unknown flag \"--type\"",
        ),
        (
            &["lineage", "--symbol", "x", "--force"],
            "unknown flag \"--force\"",
        ),
        (
            &["lineage", "--symbol", "x", "--db"],
            "--db requires a value",
        ),
        (
            &["lineage", "--db", "--json", "--symbol", "x"],
            "--db needs a value, got the flag \"--json\"",
        ),
        (&["lineage", "RouteParam:id"], "does not resolve names"),
    ];
    for (args, want) in cases {
        let out = run(&s, args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(out.stdout.is_empty(), "{args:?} printed to stdout");
        assert!(
            stderr.contains("usage: wicked-estate lineage --symbol <SYMBOL_ID>"),
            "{args:?}: {stderr}"
        );
        assert!(stderr.contains(want), "{args:?}: want {want:?} in {stderr}");
    }
    for created in ["graph.db", "--json", ".wicked-estate"] {
        assert!(
            !s.join(created).exists(),
            "a usage error must not open (and so create) a store: {created}"
        );
    }
}

#[test]
fn a_missing_graph_fails_closed_instead_of_answering_empty() {
    let s = scratch("missing_db");
    // A bare path and the `sqlite://` spelling of the same path are both file specs.
    for spec in ["typo.db", "sqlite://typo.db"] {
        let out = run(&s, &["lineage", "--symbol", "x", "--json", "--db", spec]);
        assert!(!out.status.success(), "{spec} must fail");
        assert!(out.stdout.is_empty(), "{spec} printed to stdout");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(&format!("no graph at {spec}")), "{stderr}");
        assert!(
            !s.join("typo.db").exists(),
            "lineage must never create a graph ({spec})"
        );
    }
    // A zero-length file is not a graph either, and must not be grown into an empty one.
    fs::write(s.join("empty.db"), b"").unwrap();
    let out = run(
        &s,
        &["lineage", "--symbol", "x", "--json", "--db", "empty.db"],
    );
    assert!(!out.status.success(), "a zero-length file must fail");
    assert!(out.stdout.is_empty());
    assert_eq!(fs::metadata(s.join("empty.db")).unwrap().len(), 0);
}

// ── R5: the server-level staleness line is part of the parity ───────────────

fn git(dir: &Path, date: &str, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The Angular fixture as a git repo whose graph is exactly two commits behind HEAD.
fn stale_fixture(tag: &str) -> Scratch {
    let s = scratch(tag);
    let src =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    for entry in fs::read_dir(&src).unwrap() {
        let p = entry.unwrap().path();
        fs::copy(&p, s.join(p.file_name().unwrap())).unwrap();
    }
    // Commit dates are pinned so the count is exact whatever the clock: the indexed commit is
    // long before the db's mtime, the two later ones long after it.
    git(&s, "2000-01-01T00:00:00Z", &["init", "-q"]);
    git(&s, "2000-01-01T00:00:00Z", &["add", "-A"]);
    git(&s, "2000-01-01T00:00:00Z", &["commit", "-qm", "fixture"]);
    index(&s);
    for msg in ["later one", "later two"] {
        git(
            &s,
            "2090-01-01T00:00:00Z",
            &["commit", "-q", "--allow-empty", "-m", msg],
        );
    }
    s
}

#[test]
fn json_on_a_stale_graph_carries_the_same_staleness_line_as_mcp() {
    let s = stale_fixture("stale");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");
    let cli = cli_json(&s, &["--symbol", &route, "--relation", "flows_to"]);
    let args = json!({"symbol": route, "relation": "flows_to"});

    // The MCP server computes `commits_behind` once at startup; this is the value it would hold.
    let ctx = McpContext {
        commits_behind: Some(2),
        ..McpContext::default()
    };
    assert_eq!(cli, mcp_with(&store, &args, &ctx), "CLI vs stale MCP");
    let diags = cli["diagnostics"].as_array().unwrap();
    assert_eq!(
        diags.last().unwrap(),
        &json!(wicked_estate::staleness_diagnostic(2)),
        "{cli}"
    );
    // Pinned as a literal: both frontends share one function, so comparing them with each other
    // cannot see the wording itself change.
    assert_eq!(
        wicked_estate::staleness_diagnostic(2),
        "STALENESS: commits_behind=2 — re-run `wicked-estate index` to refresh"
    );
    // Everything before the server line is the tool's own result, untouched.
    let mut tool = direct(&store, &args);
    tool["diagnostics"]
        .as_array_mut()
        .unwrap()
        .push(json!(wicked_estate::staleness_diagnostic(2)));
    assert_eq!(cli, tool);
}

#[test]
fn text_mode_on_a_stale_graph_prints_the_real_notice_and_not_the_placeholder() {
    let s = stale_fixture("stale_text");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");
    let out = run(
        &s,
        &[
            "lineage",
            "--symbol",
            &route,
            "--relation",
            "flows_to",
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    // One real notice, on stdout, the same one `query`, `path` and `blast-radius` print …
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.starts_with("STALENESS: 2 commit(s) since last index"))
            .count(),
        1,
        "{stdout}"
    );
    // … and not the tool's `commits_behind not available at this layer` cue beside it, which
    // would tell the reader the opposite of the line above.
    assert!(
        !stderr.contains("STALENESS"),
        "the retrieve placeholder must not reach the user: {stderr}"
    );
}

#[test]
fn help_lists_lineage_as_static_semantic_lineage_not_taint() {
    let s = scratch("help");
    for args in [&["--help"][..], &["lineage", "--help"][..]] {
        let out = run(&s, args);
        assert!(out.status.success());
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(
            stdout.contains(
                "wicked-estate lineage --symbol <SYMBOL_ID> [--depth N] [--relation flows_to] [--json]"
            ),
            "{stdout}"
        );
        assert!(stdout.contains("static semantic value lineage"), "{stdout}");
        assert!(stdout.contains("not taint analysis"), "{stdout}");
    }
}

// ── #244: a NAME operand gets a hint in text mode ───────────────────────────

/// `lineage --symbol` takes an exact id. An operand that matches no id but one or more symbol
/// NAMES still answers an honest empty leaf (exit 0) — and text mode now says so, naming
/// `resolve <name> --json` as the way to an id. `--json` carries no such note: that document must
/// stay the MCP response.
#[test]
fn text_mode_hints_when_the_operand_is_a_symbol_name() {
    let s = indexed_angular("name_hint");
    let store = open(&s);
    let name = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .find(|n| !n.is_value_flow_node() && n.kind != wicked_estate_core::NodeKind::File)
        .map(|n| n.name)
        .expect("a structural symbol with a name");
    let text = run(&s, &["lineage", "--symbol", &name, "--db", "graph.db"]);
    assert!(text.status.success(), "{text:?}");
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(
        stderr.contains("matches no symbol id")
            && stderr.contains(&format!("wicked-estate resolve {name} --json")),
        "text mode must hint at resolve for a name operand: {stderr}"
    );
    let json = run(
        &s,
        &["lineage", "--symbol", &name, "--json", "--db", "graph.db"],
    );
    assert!(json.status.success(), "{json:?}");
    let stderr = String::from_utf8_lossy(&json.stderr);
    assert!(
        !stderr.contains("matches no symbol id"),
        "--json carries no hint: {stderr}"
    );
    // An exact id that exists prints no hint either.
    let id = node_where(
        &store,
        |n| {
            n.name == name
                && !n.is_value_flow_node()
                && n.kind != wicked_estate_core::NodeKind::File
        },
        "the named symbol",
    );
    let text = run(&s, &["lineage", "--symbol", &id, "--db", "graph.db"]);
    assert!(text.status.success(), "{text:?}");
    assert!(
        !String::from_utf8_lossy(&text.stderr).contains("matches no symbol id"),
        "an exact id gets no hint"
    );
}
