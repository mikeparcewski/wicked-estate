//! `wicked-estate lineage` end to end, through the real binary — a RetrievalTool-bridge row since
//! W8.5 (#276), with the bridge's contract: a positional `<symbol>` (exact name or `SymbolId`,
//! value slots included), `--json` = the tool's `content` as one JSON document on stdout with every
//! diagnostic on stderr, text mode = prose then diagnostics on stdout, honest per-root freshness,
//! strict argv before any I/O.
//!
//! The oracle is not a renderer this crate owns: the CLI's `content` is compared with (a) a direct
//! `Lineage` invocation and (b) the MCP `tools/call` response — three independently produced JSON
//! values over the same SQLite file, built by the binary's own `index` over a real fixture.

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

/// `wicked-estate lineage <args> --json`: asserts exit 0 and that stdout is exactly one JSON
/// document (the tool's `content`); returns `{content, diagnostics}` with the stderr lines.
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
    let content: Value = serde_json::from_str(&stdout).expect("--json stdout must be JSON");
    assert!(
        content.get("dependencies").is_some() && content.get("diagnostics").is_none(),
        "stdout is the tool's content, not a wrapped result: {content}"
    );
    let diagnostics: Vec<String> = String::from_utf8(out.stderr)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    json!({ "content": content, "diagnostics": diagnostics })
}

/// The in-memory tool result, normalized through the same serializer the CLI and MCP print with.
/// Without the round trip the comparison depends on `serde_json`'s best-effort float parsing: a
/// `confidence.avg` such as `0.9093789458274841` reparses one ulp off (`…484`) unless the
/// `float_roundtrip` feature is on, so a byte-identical answer could compare unequal.
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

/// `content` must agree across the three paths. Diagnostics: the CLI carries every tool
/// diagnostic except the retrieval layer's staleness placeholder (it is the transport, and
/// REPLACES that cue with the real per-root statement), and at least one `STALENESS:` line.
fn assert_parity(dir: &Path, store: &SqliteStore, cli_args: &[&str], args: Value) -> Value {
    let cli = cli_json(dir, cli_args);
    let tool = direct(store, &args);
    assert_eq!(
        cli["content"], tool["content"],
        "CLI vs direct Lineage for {args}"
    );
    assert_eq!(
        cli["content"],
        mcp(store, &args)["content"],
        "CLI vs MCP Lineage for {args}"
    );
    let cli_diags: Vec<&str> = cli["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    for d in tool["diagnostics"].as_array().unwrap() {
        let d = d.as_str().unwrap();
        if d == wicked_estate_retrieve::STALENESS_PLACEHOLDER {
            assert!(
                !cli_diags.contains(&d),
                "the placeholder must be replaced: {cli_diags:?}"
            );
        } else {
            assert!(
                cli_diags.contains(&d),
                "tool diagnostic {d:?} missing: {cli_diags:?}"
            );
        }
    }
    assert!(
        cli_diags.iter().any(|d| d.starts_with("STALENESS: ")),
        "freshness is always stated: {cli_diags:?}"
    );
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
        &[&route, "--relation", "flows_to", "--depth", "8"],
        json!({"symbol": route, "depth": 8, "relation": "flows_to"}),
    );
    let c = &doc["content"];
    assert_horizon_keys(&doc);
    assert_eq!(c["searched_depth"], json!(8));

    // The AC-0004 chain, producer -> consumer, every hop present with its full evidence.
    let id = |suffix: &str| node_where(&store, |n| n.symbol.as_str().ends_with(suffix), suffix);
    let route_id = id("CustomerComponent#load().:local:routeId:");
    let field = id("CustomerComponent#:field:customerId:");
    let load_customer_id = id("CustomerComponent#loadCustomer().:param:id:");
    let service_id = id("CustomerService#getCustomer().:param:id:");
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

    // The summary describes exactly the flow hops.
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
    let doc = assert_parity(&s, &store, &[&load], json!({"symbol": load}));
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
    // The structural name resolves to the same symbol (an intentional bridge convenience).
    assert_eq!(cli_json(&s, &["load"])["content"], doc["content"]);
}

#[test]
fn a_depth_one_answer_excludes_frontier_hops() {
    let s = indexed_angular("depth_one");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");

    let doc = assert_parity(
        &s,
        &store,
        &[&route, "--relation", "flows_to", "--depth=1"],
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

/// W8.5: the tool owns its depth defaults and ceiling. Default 8, `0` is the tool's floor, `24`
/// is accepted, and an over-ceiling depth is CLAMPED and reported — not refused at argv.
#[test]
fn depth_default_zero_ceiling_and_over_ceiling() {
    let s = indexed_angular("depth_matrix");
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");
    for (given, searched, clamped) in [
        (None, 8, false),
        (Some("0"), 0, false),
        (Some("24"), 24, false),
        (Some("99"), 24, true),
    ] {
        let mut cli: Vec<&str> = vec![&route, "--relation", "flows_to"];
        let mut args = json!({"symbol": route, "relation": "flows_to"});
        if let Some(d) = given {
            cli.extend(["--depth", d]);
            args["depth"] = json!(d.parse::<u64>().unwrap());
        }
        let doc = assert_parity(&s, &store, &cli, args);
        assert_eq!(
            doc["content"]["searched_depth"],
            json!(searched),
            "{given:?}"
        );
        let has_clamp = doc["diagnostics"].as_array().unwrap().iter().any(|d| {
            d.as_str().unwrap() == "CLAMPED: depth=99 is above this tool's ceiling; used depth=24"
        });
        assert_eq!(has_clamp, clamped, "{given:?}: {doc}");
    }
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
        &[&route_id, "--relation", "flows_to"],
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

/// W8.5 (breaking): an unknown selector is an error, not an honest-empty answer with exit 0 —
/// an empty lineage for a typo is indistinguishable from a real leaf (R3). An ambiguous name
/// lists its candidates; an absent selector is a usage error.
#[test]
fn unknown_ambiguous_and_absent_selectors_fail_with_nothing_on_stdout() {
    let s = indexed_angular("selectors");
    let store = open(&s);
    let fail = |args: &[&str], want: &str| {
        let mut argv = vec!["lineage"];
        argv.extend_from_slice(args);
        argv.extend_from_slice(&["--json", "--db", "graph.db"]);
        let out = run(&s, &argv);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(out.stdout.is_empty(), "{args:?} printed to stdout");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(want), "{args:?}: want {want:?} in {stderr}");
    };
    fail(&["no such symbol#"], "no symbol named \"no such symbol#\"");
    fail(
        &["no such symbol#", "--relation", "flows_to"],
        "no symbol named",
    );
    fail(&[], "missing <symbol>");
    // A name two symbols share is ambiguous: every candidate id is listed.
    let shared: Vec<Node> = GraphRead::all_nodes(&store)
        .unwrap()
        .into_iter()
        .filter(|n| n.name == "id" && !n.is_value_flow_node())
        .collect();
    if shared.len() > 1 {
        fail(&["id"], "pass one SymbolId");
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
        |n| n.is_value_flow_node() && n.symbol.as_str().ends_with("fan().:param:seed:"),
        "seed slot",
    );

    let doc = assert_parity(
        &s,
        &store,
        &[&seed, "--relation", "flows_to", "--depth", "1"],
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
            &route,
            "--relation",
            "flows_to",
            "--db",
            "graph.db",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        serde_json::from_str::<Value>(&stdout).is_err() && !stdout.contains("\"dependencies\""),
        "text mode must not print the JSON document: {stdout}"
    );
    assert!(stdout.contains("not taint analysis"), "{stdout}");
    assert!(stdout.contains("semantics=value_preserving"), "{stdout}");
    assert!(stdout.contains("evidence=convention"), "{stdout}");
    assert!(stdout.contains("(tree-sitter-convention)"), "{stdout}");
    // The bridge's text contract: prose, then the diagnostics, on stdout — the real freshness
    // statement, never the retrieval layer's placeholder cue.
    assert!(stdout.contains("STALENESS: "), "{stdout}");
    assert!(
        !stdout.contains(wicked_estate_retrieve::STALENESS_PLACEHOLDER),
        "the retrieve placeholder must not reach the user: {stdout}"
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // `cli_json` asserts the converse: one JSON line on stdout, diagnostics on stderr.
    let doc = cli_json(&s, &[&route, "--relation", "flows_to"]);
    assert!(
        doc["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d.as_str().unwrap() != wicked_estate_retrieve::STALENESS_PLACEHOLDER)
    );
}

#[test]
fn malformed_arguments_fail_before_any_query() {
    let s = scratch("usage");
    // Every case is the complete argv. Several are the shared-loop hazards: a flag another
    // command owns (`--file`, `--top`, `--type`, `--force`) must not be swallowed and ignored, a
    // dangling `--db` must not fall back to the default store, and `--db --json` must not open a
    // file named `--json`.
    let cases: &[(&[&str], &str)] = &[
        (&["lineage", "--db", "graph.db"], "missing <symbol>"),
        (
            &["lineage", "a", "b", "--db", "graph.db"],
            "expected exactly one <symbol>",
        ),
        (&["lineage", "--symbol", "x"], "unknown flag \"--symbol\""),
        (&["lineage", "x", "--depth"], "--depth requires a number"),
        (
            &["lineage", "x", "--depth", "deep"],
            "--depth expects a non-negative integer",
        ),
        (
            &["lineage", "x", "--depth", "-1"],
            "--depth expects a non-negative integer",
        ),
        (
            &["lineage", "x", "--depth", "+5"],
            "--depth expects a non-negative integer",
        ),
        (
            &["lineage", "x", "--depth", "2", "--depth", "3"],
            "--depth given more than once",
        ),
        (
            &["lineage", "x", "--relation"],
            "--relation requires one of flows_to",
        ),
        (
            &["lineage", "x", "--relation", "calls"],
            "--relation expects one of flows_to",
        ),
        (
            &["lineage", "x", "--relation=FLOWS_TO"],
            "--relation expects one of flows_to",
        ),
        (&["lineage", "x", "--bogus"], "unknown flag \"--bogus\""),
        (
            &["lineage", "x", "--file", "a.ts"],
            "unknown flag \"--file\"",
        ),
        (&["lineage", "x", "--top", "5"], "unknown flag \"--top\""),
        (&["lineage", "x", "--type", "t"], "unknown flag \"--type\""),
        (&["lineage", "x", "--force"], "unknown flag \"--force\""),
        (&["lineage", "x", "--db"], "--db requires a database spec"),
        (
            &["lineage", "--db", "--json", "x"],
            "--db requires a database spec",
        ),
    ];
    for (args, want) in cases {
        let out = run(&s, args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(out.stdout.is_empty(), "{args:?} printed to stdout");
        assert!(
            stderr.contains("usage: wicked-estate lineage <symbol>"),
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
        let out = run(&s, &["lineage", "x", "--json", "--db", spec]);
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
    let out = run(&s, &["lineage", "x", "--json", "--db", "empty.db"]);
    assert!(!out.status.success(), "a zero-length file must fail");
    assert!(out.stdout.is_empty());
    assert_eq!(fs::metadata(s.join("empty.db")).unwrap().len(), 0);
}

// ── R5: freshness is honest, per root, and never the placeholder ────────────

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

/// The Angular fixture as a git repo whose graph is `behind` commits behind HEAD.
fn git_fixture(tag: &str, behind: usize) -> Scratch {
    let s = scratch(tag);
    let src =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    for entry in fs::read_dir(&src).unwrap() {
        let p = entry.unwrap().path();
        fs::copy(&p, s.join(p.file_name().unwrap())).unwrap();
    }
    // Commit dates are pinned so the count is exact whatever the clock: the indexed commit is
    // long before the db's mtime, the later ones long after it.
    git(&s, "2000-01-01T00:00:00Z", &["init", "-q"]);
    git(&s, "2000-01-01T00:00:00Z", &["add", "-A"]);
    git(&s, "2000-01-01T00:00:00Z", &["commit", "-qm", "fixture"]);
    index(&s);
    for i in 0..behind {
        git(
            &s,
            "2090-01-01T00:00:00Z",
            &["commit", "-q", "--allow-empty", "-m", &format!("later {i}")],
        );
    }
    s
}

fn staleness(doc: &Value) -> Vec<String> {
    doc["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap().to_string())
        .filter(|d| d.starts_with("STALENESS"))
        .collect()
}

#[test]
fn freshness_is_stated_for_stale_fresh_and_unknown_graphs() {
    // Stale: the real count, once, with the fix.
    let s = git_fixture("stale", 2);
    let store = open(&s);
    let route = node_where(&store, |n| n.name == "RouteParam:id", "RouteParam:id");
    let doc = assert_parity(
        &s,
        &store,
        &[&route, "--relation", "flows_to"],
        json!({"symbol": route, "relation": "flows_to"}),
    );
    let lines = staleness(&doc);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("STALENESS: 2 commit(s) since last index"),
        "{lines:?}"
    );

    // Fresh: zero is stated, because the root was checked.
    let s = git_fixture("fresh", 0);
    let doc = cli_json(&s, &[&route, "--relation", "flows_to"]);
    assert_eq!(
        staleness(&doc),
        vec!["STALENESS: 0 commits since last index".to_string()]
    );

    // Unknown: no git root to check, so zero is NOT claimed.
    let s = indexed_angular("unknown");
    let doc = cli_json(&s, &[&route, "--relation", "flows_to"]);
    let lines = staleness(&doc);
    assert!(
        !lines.is_empty() && lines.iter().all(|l| l.contains("unknown")),
        "{lines:?}"
    );
}

/// A multi-repo graph states freshness per root: the stale repo is named, and "0" is never
/// claimed for a root that was not checked.
#[test]
fn freshness_is_per_root_on_a_multi_repo_graph() {
    let a = git_fixture("multi_a", 0);
    let b = scratch("multi_b"); // not a git repo: its freshness is unknown
    let src =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typescript-value-lineage");
    for entry in fs::read_dir(&src).unwrap() {
        let p = entry.unwrap().path();
        fs::copy(&p, b.join(p.file_name().unwrap())).unwrap();
    }
    let db = a
        .parent()
        .unwrap()
        .join(format!("ci_lineagecli_multi_{}.db", std::process::id()));
    let _ = fs::remove_file(&db);
    let db_s = db.to_str().unwrap();
    for (dir, label) in [(&a, "ra"), (&b, "rb")] {
        let out = run(dir, &["index", ".", "--repo", label, "--db", db_s]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // One commit after `ra` was indexed: that root is stale by exactly one.
    git(
        &a,
        "2090-01-01T00:00:00Z",
        &["commit", "-q", "--allow-empty", "-m", "after ra"],
    );
    let store = SqliteStore::open(db_s).unwrap();
    let route = node_where(
        &store,
        |n| n.name == "RouteParam:id" && n.location.file.starts_with("ra/"),
        "ra RouteParam:id",
    );
    let out = run(
        &a,
        &[
            "lineage",
            &route,
            "--relation",
            "flows_to",
            "--json",
            "--db",
            db_s,
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("STALENESS"))
        .collect();
    assert!(
        lines.iter().any(|l| l.contains("1 commit(s) in 'ra'")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("unknown for repo 'rb'")),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains("0 commits")), "{lines:?}");
    let _ = fs::remove_file(&db);
}

#[test]
fn help_lists_lineage_as_static_semantic_lineage_not_taint() {
    let s = scratch("help");
    let top = run(&s, &["--help"]);
    assert!(top.status.success());
    let top = String::from_utf8(top.stdout).unwrap();
    assert!(
        top.contains("wicked-estate lineage <symbol> [--depth N] [--relation flows_to] [--json]"),
        "{top}"
    );
    let own = run(&s, &["lineage", "--help"]);
    assert!(own.status.success());
    let own = String::from_utf8(own.stdout).unwrap();
    assert!(own.contains("static semantic value lineage"), "{own}");
    assert!(own.contains("not taint"), "{own}");
    assert!(own.contains("clamps to its ceiling"), "{own}");
}
