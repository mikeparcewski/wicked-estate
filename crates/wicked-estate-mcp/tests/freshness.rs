//! wicked-estate#351 — per-query freshness through the REAL server binary.
//!
//! Freshness is stated after the response cache (main.rs's request loop), so these tests drive
//! the binary over stdio: a commit while the server runs is seen on the next call; an
//! uncommitted edit marks the rows it touches — on a fresh call, an L1 cache hit and, after a
//! restart, an L2 hit; a dirty file the answer does not touch marks nothing; re-indexing clears
//! the marks; deleting the file marks it `deleted`; the retrieval placeholder never reaches the
//! client.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use wicked_estate_core::{GraphRead, SymbolQuery};
use wicked_estate_store::SqliteStore;

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpChild {
    fn spawn(dir: &Path) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("WICKED_ESTATE_DB", dir.join("graph.db"))
            .env("WICKED_MEMORY_DB", dir.join("memory.db"))
            .env("WICKED_KNOWLEDGE_DB", dir.join("knowledge.db"))
            .env("WICKED_XEDGE_DB", dir.join("xedge.db"))
            .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
            .env_remove("WICKED_OTEL_ENDPOINT")
            .env_remove("WICKED_STORE_URL")
            .env_remove("WICKED_RUNTIME");
        let mut child = cmd.spawn().expect("spawn wicked-estate-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut me = McpChild {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        };
        let init = me.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "freshness-test", "version": "0" }
            }),
        );
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        me
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{req}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).unwrap();
        assert!(n > 0, "server closed stdout");
        serde_json::from_str(line.trim()).unwrap()
    }

    /// `(content, diagnostics)` of one `tools/call`.
    fn tool(&mut self, name: &str, args: Value) -> (Value, Vec<String>) {
        let resp = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(resp["result"]["isError"], json!(false), "{resp}");
        let blocks = resp["result"]["content"].as_array().unwrap();
        let content = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
        let diags = blocks
            .get(1)
            .and_then(|b| b["text"].as_str())
            .map(|t| t.split('\n').map(str::to_string).collect())
            .unwrap_or_default();
        (content, diags)
    }

    fn finish(self) {
        let McpChild {
            mut child, stdin, ..
        } = self;
        drop(stdin);
        child.wait().ok();
    }
}

fn git(dir: &Path, args: &[&str]) {
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
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const LIB: &str = "pub fn leaf() {}\n\npub fn caller() {\n    leaf();\n}\n";
const OTHER: &str = "pub fn unrelated() {}\n";

/// A committed git repo (`src/lib.rs`, `src/other.rs`) indexed into `<root>/graph.db`.
fn repo(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join(format!("repo_{tag}"));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), LIB).unwrap();
    std::fs::write(root.join("src/other.rs"), OTHER).unwrap();
    std::fs::write(root.join(".gitignore"), "*.db*\n").unwrap();
    git(&root, &["init", "-q", "."]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    reindex(&root);
    (tmp, root)
}

fn reindex(root: &Path) {
    let mut store = SqliteStore::open(root.join("graph.db")).unwrap();
    wicked_estate::index_path(&mut store, root).expect("index");
}

fn leaf_id(root: &Path) -> String {
    let store = SqliteStore::open(root.join("graph.db")).unwrap();
    let hits = store
        .find_symbols(&SymbolQuery {
            exact_name: Some("leaf".into()),
            ..Default::default()
        })
        .unwrap();
    hits[0].symbol.0.clone()
}

fn blast(mcp: &mut McpChild, id: &str) -> (Value, Vec<String>) {
    mcp.tool("BlastRadius", json!({ "symbol": id }))
}

fn stale_marks(content: &Value) -> Vec<String> {
    content["dependents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["stale"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn commits_are_seen_without_restart_and_the_placeholder_never_leaks() {
    let (_tmp, root) = repo("commits");
    let id = leaf_id(&root);
    let mut mcp = McpChild::spawn(&root);
    let (c, diags) = blast(&mut mcp, &id);
    assert_eq!(c["freshness"]["commits_behind"], json!(0), "{c}");
    assert!(
        diags
            .iter()
            .any(|d| d == "STALENESS: 0 commits since last index"),
        "R5 states 'fresh' explicitly: {diags:?}"
    );
    assert!(
        !diags
            .iter()
            .any(|d| d == wicked_estate_retrieve::STALENESS_PLACEHOLDER),
        "{diags:?}"
    );
    std::fs::write(root.join("README.md"), "later\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "later"]);
    // Same request: an L1 cache hit — re-measured, not replayed.
    let (c, diags) = blast(&mut mcp, &id);
    assert_eq!(c["freshness"]["commits_behind"], json!(1), "{c}");
    assert!(
        diags
            .iter()
            .any(|d| d.starts_with("STALENESS: 1 commit(s) since last index")),
        "{diags:?}"
    );
    mcp.finish();
}

/// #372 review: a re-index while the server runs must not leave an L1 entry from the OLD graph
/// to be annotated against the NEW graph's digests and certified fresh (a WAL commit can leave
/// the db file's mtime unchanged; the cache is keyed on the graph version too).
#[test]
fn a_reindex_under_a_running_server_never_replays_the_old_graph() {
    let (_tmp, root) = repo("reindex");
    let mut mcp = McpChild::spawn(&root);
    let (c, _) = mcp.tool("SearchEntity", json!({"name": "leaf"}));
    assert_eq!(c["resolved"], json!(true), "{c}");
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn renamed() {}\n\npub fn caller() {\n    renamed();\n}\n",
    )
    .unwrap();
    reindex(&root);
    let (c, _) = mcp.tool("SearchEntity", json!({"name": "leaf"}));
    assert_eq!(
        c["resolved"],
        json!(false),
        "the old graph's answer was replayed after a re-index: {c}"
    );
    mcp.finish();
}

#[test]
fn drift_marks_rows_on_fresh_calls_and_on_both_cache_levels() {
    let (_tmp, root) = repo("drift");
    let id = leaf_id(&root);
    let mut mcp = McpChild::spawn(&root);
    let (c, _) = blast(&mut mcp, &id);
    assert!(
        !c["dependents"].as_array().unwrap().is_empty(),
        "caller depends on leaf: {c}"
    );
    assert!(stale_marks(&c).is_empty(), "{c}");
    assert_eq!(c["freshness"]["stale_files_total"], json!(0));

    // A dirty file the answer does not touch marks nothing.
    std::fs::write(root.join("src/other.rs"), "pub fn unrelated() { }\n").unwrap();
    let (c, _) = blast(&mut mcp, &id);
    assert!(stale_marks(&c).is_empty(), "{c}");

    // An uncommitted edit to a file in the answer: the L1 hit is marked.
    std::fs::write(root.join("src/lib.rs"), format!("{LIB}// edited\n")).unwrap();
    let (c, diags) = blast(&mut mcp, &id);
    assert!(
        stale_marks(&c).iter().all(|m| m == "modified") && !stale_marks(&c).is_empty(),
        "{c}"
    );
    assert_eq!(
        c["freshness"]["stale_files"],
        json!([{"file": "src/lib.rs", "status": "modified"}])
    );
    assert!(
        diags
            .iter()
            .any(|d| d.contains("differ from the index") && d.contains("src/lib.rs modified")),
        "{diags:?}"
    );
    mcp.finish();

    // Restart: the L2 (SQLite) hit is marked too — the cache never stored freshness.
    let mut mcp = McpChild::spawn(&root);
    let (c, _) = blast(&mut mcp, &id);
    assert!(
        !stale_marks(&c).is_empty(),
        "L2 hit must be re-annotated: {c}"
    );
    // Revert the bytes: the same cached answer now carries no marks.
    std::fs::write(root.join("src/lib.rs"), LIB).unwrap();
    let (c, _) = blast(&mut mcp, &id);
    assert!(stale_marks(&c).is_empty(), "{c}");
    mcp.finish();

    // Edit, re-index: the marks are gone.
    std::fs::write(root.join("src/lib.rs"), format!("{LIB}// edited again\n")).unwrap();
    reindex(&root);
    let mut mcp = McpChild::spawn(&root);
    let (c, _) = blast(&mut mcp, &id);
    assert!(stale_marks(&c).is_empty(), "re-indexed: {c}");
    // Delete it: `deleted`.
    std::fs::remove_file(root.join("src/lib.rs")).unwrap();
    let (c, _) = blast(&mut mcp, &id);
    assert!(
        stale_marks(&c).iter().all(|m| m == "deleted") && !stale_marks(&c).is_empty(),
        "{c}"
    );
    mcp.finish();
}
