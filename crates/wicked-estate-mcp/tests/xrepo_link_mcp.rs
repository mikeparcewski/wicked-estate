//! #419: link status is measured after caching on BlastRadius and Path answers.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
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
                "clientInfo": { "name": "xrepo-link-test", "version": "0" }
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
}

impl Drop for McpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn blast_and_path_report_live_link_state_on_repeated_calls() {
    let root = tempfile::tempdir().unwrap();
    let provider = root.path().join("provider");
    let consumer = root.path().join("consumer");
    write(
        &provider,
        "package.json",
        r#"{"name":"acme-types","version":"1.2.0","types":"src/index.ts"}"#,
    );
    write(
        &provider,
        "src/index.ts",
        "export function makeOrder() { return 1; }\n",
    );
    write(
        &consumer,
        "package.json",
        r#"{"name":"consumer","dependencies":{"acme-types":"file:../provider"}}"#,
    );
    write(
        &consumer,
        "src/app.ts",
        "import { makeOrder } from 'acme-types';\nexport function run() { return makeOrder(); }\n",
    );
    let db = root.path().join("graph.db");
    let mut store = SqliteStore::open(&db).unwrap();
    wicked_estate::index_path_as(&mut store, &provider, Some("provider")).unwrap();
    wicked_estate::index_path_as(&mut store, &consumer, Some("consumer")).unwrap();
    let report =
        wicked_estate::link::link_project(&mut store, "p", &["provider".into(), "consumer".into()])
            .unwrap();
    assert_eq!(report.links, 1, "{report:?}");

    let mut mcp = McpChild::spawn(root.path());
    let args = json!({"symbol": "provider/src/index.ts"});
    for _ in 0..2 {
        let (content, diags) = mcp.tool("BlastRadius", args.clone());
        assert_eq!(
            content["crossings"].as_array().unwrap().len(),
            1,
            "{content}"
        );
        assert_eq!(content["link"][0]["project"], "p", "{content}");
        assert_eq!(content["link"][0]["stale"], false, "{content}");
        assert!(!diags.iter().any(|d| d.starts_with("LINK-STALE:")));
        let chars = serde_json::to_string(&content).unwrap().len()
            + diags.iter().map(|d| d.len() + 1).sum::<usize>();
        assert!(chars <= wicked_estate_retrieve::R4_CHAR_BUDGET);
    }

    wicked_estate::index_path_as(&mut store, &consumer, Some("consumer")).unwrap();
    // The same request observes the new state. Repeating it also checks its warm L1 hit:
    // graph-version invalidation may discard the pre-index cache entry.
    for _ in 0..2 {
        let (content, diags) = mcp.tool("BlastRadius", args.clone());
        assert_eq!(content["link"][0]["stale"], true, "{content}");
        assert_eq!(content["link"][0]["stale_labels"], json!(["consumer"]));
        assert!(
            diags
                .iter()
                .any(|d| { d.starts_with("LINK-STALE:") && d.contains("consumer") }),
            "{diags:?}"
        );
    }
    let (path, diags) = mcp.tool(
        "Path",
        json!({"from": "consumer/src/app.ts", "to": "provider/src/index.ts"}),
    );
    assert!(!path["crossings"].as_array().unwrap().is_empty(), "{path}");
    assert_eq!(path["link"][0]["project"], "p", "{path}");
    assert_eq!(path["link"][0]["stale"], true, "{path}");
    assert!(diags.iter().any(|d| d.starts_with("LINK-STALE:")));
}
