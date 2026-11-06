//! e2e MAJOR-2: MCP `BlastRadius`, `TraverseGraph` and `Lineage` resolve a symbol NAME or a file
//! PATH the way the CLI does, and a target the graph does not hold is never a confident empty.
//!
//! Before the fix the MCP tools walked `symbol` as a raw `SymbolId`: `BlastRadius {"symbol":
//! "tax"}` answered `resolved: false, total: 0` while `wicked-estate blast-radius tax` found the
//! dependents, and `TraverseGraph` / `Lineage` by name answered an unlabelled empty subgraph.
//!
//! The oracle is the CLI binary over the same SQLite file, built by the binary's own `index`
//! over a real fixture; the MCP side is the unified `tools/call` dispatch.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use wicked_estate_mcp::{McpContext, handle_request_unified};
use wicked_estate_store::SqliteStore;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wicked-estate"))
        .current_dir(dir)
        .args(args)
        .env(
            "WICKED_ESTATE_EMIT_PROGRAM",
            "wicked-bus-absent-name-parity",
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

/// A Python repo indexed by the binary: `src/pricing.py` defines `tax`, called in-file and from
/// `src/checkout.py`. `duplicate` adds a second `tax` in `src/legacy.py` with its own caller.
fn indexed(tag: &str, duplicate: bool) -> Scratch {
    let dir = std::env::temp_dir().join(format!("ci_nameparity_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/pricing.py"),
        "def tax(amount):\n    return amount * 0.2\n\n\
         def total(amount):\n    return amount + tax(amount)\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/checkout.py"),
        "from src.pricing import tax, total\n\n\
         def checkout(amount):\n    return total(amount)\n\n\
         def receipt(amount):\n    return tax(amount) + total(amount)\n",
    )
    .unwrap();
    if duplicate {
        fs::write(
            dir.join("src/legacy.py"),
            "def tax(x):\n    return x\n\n\
             def old_total(x):\n    return x + tax(x)\n",
        )
        .unwrap();
    }
    let out = run(&dir, &["index", ".", "--db", "graph.db"]);
    assert!(
        out.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Scratch(dir)
}

fn open(dir: &Path) -> SqliteStore {
    SqliteStore::open(dir.join("graph.db").to_str().unwrap()).expect("open indexed db")
}

/// `wicked-estate <args> --json --db graph.db`, parsed (exit 0 asserted).
fn cli(dir: &Path, args: &[&str]) -> Value {
    let mut argv = args.to_vec();
    argv.extend_from_slice(&["--json", "--db", "graph.db"]);
    let out = run(dir, &argv);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("--json stdout must be JSON")
}

/// The MCP `tools/call` response: `Ok(content)` for a tool answer, `Err(text)` for `isError`.
fn mcp(store: &SqliteStore, tool: &str, args: Value) -> Result<Value, String> {
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": tool, "arguments": args },
    });
    let resp = handle_request_unified(store, &req, &McpContext::default(), None, None);
    let result = &resp["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if result["isError"] == json!(true) {
        return Err(text.to_string());
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {resp}")))
}

fn ids(rows: &Value, key: &str) -> BTreeSet<String> {
    rows.as_array()
        .unwrap_or_else(|| panic!("not an array: {rows}"))
        .iter()
        .map(|r| r[key].as_str().unwrap().to_string())
        .collect()
}

/// The SymbolId of the one node named `name` in `file`.
fn id_of(store: &SqliteStore, name: &str, file: &str) -> String {
    let hits: Vec<_> = wicked_estate_core::GraphRead::all_nodes(store)
        .unwrap()
        .into_iter()
        .filter(|n| n.name == name && n.location.file == file)
        .collect();
    assert_eq!(hits.len(), 1, "{name} in {file}: {hits:?}");
    hits[0].symbol.as_str().to_string()
}

/// The fails-without-the-fix check: a name and a file path each answer the CLI's dependents on
/// MCP, `resolved: true`. The full id answers what the name answers (the CLI's `blast-radius`
/// matches names only; an exact id is the extra form every MCP graph tool accepts, as `Path`
/// always has).
#[test]
fn blast_radius_by_name_path_and_id_matches_cli() {
    let dir = indexed("blast", false);
    let store = open(&dir.0);
    let tax_id = id_of(&store, "tax", "src/pricing.py");
    let by_name = ids(
        &cli(&dir.0, &["blast-radius", "tax", "--depth", "8"])["dependents"],
        "id",
    );
    assert!(!by_name.is_empty(), "the fixture must give tax dependents");
    let by_path = ids(
        &cli(&dir.0, &["blast-radius", "src/pricing.py", "--depth", "8"])["dependents"],
        "id",
    );
    for (target, want) in [
        ("tax", &by_name),
        ("src/pricing.py", &by_path),
        (tax_id.as_str(), &by_name),
    ] {
        let got = mcp(&store, "BlastRadius", json!({"symbol": target, "depth": 8})).unwrap();
        assert_eq!(got["resolved"], json!(true), "{target}: {got}");
        assert_eq!(
            &ids(&got["dependents"], "symbol"),
            want,
            "MCP vs CLI BlastRadius for {target}"
        );
        assert_eq!(got["total"].as_u64(), Some(want.len() as u64), "{got}");
    }
}

/// A name two symbols share answers the union of their dependents, as the CLI does.
#[test]
fn blast_radius_shared_name_is_the_cli_union() {
    let dir = indexed("blast_dup", true);
    let store = open(&dir.0);
    let cli_doc = cli(&dir.0, &["blast-radius", "tax", "--depth", "8"]);
    let want = ids(&cli_doc["dependents"], "id");
    let got = mcp(&store, "BlastRadius", json!({"symbol": "tax", "depth": 8})).unwrap();
    assert_eq!(got["resolved"], json!(true), "{got}");
    assert_eq!(ids(&got["dependents"], "symbol"), want);
    let legacy_caller = id_of(&store, "old_total", "src/legacy.py");
    assert!(
        want.contains(&legacy_caller),
        "union must hold {legacy_caller}: {want:?}"
    );
}

/// TraverseGraph and Lineage by name answer what the CLI's bridged `traverse` / `lineage` answer.
#[test]
fn traverse_and_lineage_by_name_match_cli() {
    let dir = indexed("walk", false);
    let store = open(&dir.0);
    let cases = [
        ("traverse", "TraverseGraph", "total", "nodes"),
        ("lineage", "Lineage", "total", "dependencies"),
        ("traverse", "TraverseGraph", "src/pricing.py", "nodes"),
    ];
    for (cmd, tool, target, rows) in cases {
        let cli_doc = cli(&dir.0, &[cmd, target]);
        let got = mcp(&store, tool, json!({"symbol": target})).unwrap();
        assert_eq!(got["resolved"], json!(true), "{tool} {target}: {got}");
        let want = ids(&cli_doc[rows], "symbol");
        assert!(
            !want.is_empty(),
            "the fixture must give {cmd} {target} rows"
        );
        assert_eq!(
            ids(&got[rows], "symbol"),
            want,
            "MCP vs CLI {tool} {target}"
        );
    }
}

/// A name two symbols share is refused naming both, on MCP as on the CLI — never a walk from one
/// of them picked silently.
#[test]
fn traverse_and_lineage_refuse_a_shared_name() {
    let dir = indexed("walk_dup", true);
    let store = open(&dir.0);
    for (cmd, tool) in [("traverse", "TraverseGraph"), ("lineage", "Lineage")] {
        let out = run(&dir.0, &[cmd, "tax", "--json", "--db", "graph.db"]);
        assert!(!out.status.success(), "the CLI refuses an ambiguous {cmd}");
        let err = mcp(&store, tool, json!({"symbol": "tax"})).unwrap_err();
        assert!(err.contains("names 2 symbols"), "{tool}: {err}");
        assert!(
            err.contains(&id_of(&store, "tax", "src/legacy.py")),
            "{err}"
        );
        assert!(
            err.contains(&id_of(&store, "tax", "src/pricing.py")),
            "{err}"
        );
    }
}

/// A target the graph does not hold is `resolved: false` with suggestions on all three tools: an
/// empty answer that is a miss says so.
#[test]
fn unknown_target_is_a_labelled_miss_on_every_tool() {
    let dir = indexed("miss", false);
    let store = open(&dir.0);
    for (tool, rows) in [
        ("BlastRadius", "dependents"),
        ("TraverseGraph", "nodes"),
        ("Lineage", "dependencies"),
    ] {
        let got = mcp(&store, tool, json!({"symbol": "taxx"})).unwrap();
        assert_eq!(got["resolved"], json!(false), "{tool}: {got}");
        assert_eq!(got["query"], json!("taxx"), "{tool}: {got}");
        assert_eq!(got[rows], json!([]), "{tool}: {got}");
        let names: Vec<&str> = got["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["name"].as_str())
            .collect();
        assert!(names.contains(&"tax"), "{tool} must suggest tax: {got}");
    }
}
