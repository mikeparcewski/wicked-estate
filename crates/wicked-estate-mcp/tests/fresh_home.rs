//! e2e MAJOR-1 (estate#375): a write-mode server on a fresh home serves every domain.
//!
//! A fresh home has no `~/.wicked` directory. SQLite creates a missing database file but not its
//! directory, so the memory, knowledge and xedge stores failed to open and the server came up
//! with 12 graph tools and no memory or knowledge tools, silently (only a stderr line an agent
//! never reads). Each test runs the real binary under a temporary HOME.
//!
//! * The default layout (`$HOME/.wicked/*.db`) and the installer's explicit `WICKED_*_DB` env
//!   both get their directory created (0700 on unix) and serve all 33 tools.
//! * A store that still cannot open is reported on the wire: `initialize` `instructions`, the
//!   `tools/list` `_meta`, and the refusal text of every domain call name the cause.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

struct McpChild {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Value>,
    deadline: Instant,
}

impl McpChild {
    /// The server under `home`, its graph in `cwd` (an existing dir), plus `env`.
    fn spawn(home: &Path, cwd: &Path, env: &[(&str, String)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
        cmd.env_clear()
            .env("HOME", home)
            .env("WICKED_ESTATE_DB", cwd.join("graph.db"))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in env {
            if *k == "--readonly" {
                cmd.arg("--readonly");
                continue;
            }
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn wicked-estate-mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    break;
                };
                if sender.send(v).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            responses,
            deadline: Instant::now() + Duration::from_secs(30),
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let stdin = self.stdin.as_mut().expect("stdin open");
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(stdin, "{msg}").expect("write request");
        stdin.flush().expect("flush request");
        loop {
            let v = self
                .responses
                .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| panic!("no response for id {id}: {e}"));
            if v["id"].as_u64() == Some(id) {
                return v;
            }
        }
    }

    fn call(&mut self, id: u64, tool: &str, arguments: Value) -> Value {
        self.request(
            id,
            "tools/call",
            json!({"name": tool, "arguments": arguments}),
        )
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_names(list: &Value) -> Vec<String> {
    list["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list has no tools: {list}"))
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

/// The domain tools a write-mode server serves: 7 memory + 10 knowledge + 4 proposal.
fn domain_tool_count(names: &[String]) -> usize {
    names
        .iter()
        .filter(|n| {
            n.starts_with("memory.") || n.starts_with("knowledge.") || n.starts_with("proposal.")
        })
        .count()
}

/// The fresh-home path end to end: every domain served, a memory written and read back.
fn assert_serves_every_domain(mcp: &mut McpChild) {
    let init = mcp.request(1, "initialize", json!({"protocolVersion": "2024-11-05"}));
    assert!(init.get("error").is_none(), "initialize failed: {init}");
    assert!(
        init["result"].get("instructions").is_none(),
        "a healthy server must not report degraded domains: {init}"
    );
    let list = mcp.request(2, "tools/list", json!({}));
    let names = tool_names(&list);
    assert_eq!(
        domain_tool_count(&names),
        21,
        "fresh home must serve every memory/knowledge/proposal tool, got {names:?}"
    );
    assert_eq!(names.len(), 33, "fresh home must serve 33 tools: {names:?}");
    assert!(list["result"].get("_meta").is_none(), "{list}");

    let cap = mcp.call(
        3,
        "memory.capture",
        json!({"content": "fresh-home capture works", "kind": "fact"}),
    );
    assert!(
        cap.get("error").is_none() && cap["result"]["isError"] != json!(true),
        "memory.capture on a fresh home failed: {cap}"
    );
    let recall = mcp.call(4, "memory.recall", json!({"query": "fresh-home capture"}));
    let text = recall["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        text.contains("fresh-home capture works"),
        "the captured memory must recall: {recall}"
    );
}

#[cfg(unix)]
fn assert_private_dir(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode,
        0o700,
        "{} must be created 0700, is {mode:o}",
        dir.display()
    );
}
#[cfg(not(unix))]
fn assert_private_dir(dir: &Path) {
    assert!(dir.is_dir());
}

#[test]
fn fresh_home_default_layout_serves_all_33_tools() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let wicked = home.path().join(".wicked");
    assert!(!wicked.exists());
    let mut mcp = McpChild::spawn(home.path(), cwd.path(), &[]);
    assert_serves_every_domain(&mut mcp);
    drop(mcp);
    for store in ["memory.db", "knowledge.db", "xedge.db"] {
        assert!(wicked.join(store).is_file(), "missing ~/.wicked/{store}");
    }
    assert_private_dir(&wicked);
}

/// The env the installer writes for Claude Code (`WICKED_MEMORY_DB=~/.wicked/memory.db` etc.).
#[test]
fn fresh_home_installer_env_serves_all_33_tools() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let wicked = home.path().join(".wicked");
    let env: Vec<(&str, String)> = [
        ("WICKED_MEMORY_DB", "memory.db"),
        ("WICKED_KNOWLEDGE_DB", "knowledge.db"),
        ("WICKED_XEDGE_DB", "xedge.db"),
    ]
    .iter()
    .map(|(k, f)| (*k, wicked.join(f).to_string_lossy().into_owned()))
    .collect();
    let mut mcp = McpChild::spawn(home.path(), cwd.path(), &env);
    assert_serves_every_domain(&mut mcp);
    drop(mcp);
    assert_private_dir(&wicked);
}

/// An existing store directory keeps its own permissions: only a directory the server creates is
/// made private.
#[cfg(unix)]
#[test]
fn existing_store_dir_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let wicked = home.path().join(".wicked");
    std::fs::create_dir(&wicked).unwrap();
    std::fs::set_permissions(&wicked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut mcp = McpChild::spawn(home.path(), cwd.path(), &[]);
    assert_serves_every_domain(&mut mcp);
    drop(mcp);
    let mode = std::fs::metadata(&wicked).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

/// A store that cannot open (its directory path is a regular file) is reported on the wire, not
/// only on stderr: the agent learns WHY its memory tools are missing.
#[test]
fn unopenable_store_is_reported_loudly() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".wicked"), b"not a directory").unwrap();
    let mut mcp = McpChild::spawn(home.path(), cwd.path(), &[]);

    let init = mcp.request(1, "initialize", json!({"protocolVersion": "2024-11-05"}));
    let instructions = init["result"]["instructions"].as_str().unwrap_or_else(|| {
        panic!("a degraded server must say so in initialize instructions: {init}")
    });
    assert!(instructions.contains("DISABLED"), "{instructions}");
    assert!(
        instructions.contains(".wicked"),
        "names the path: {instructions}"
    );

    let list = mcp.request(2, "tools/list", json!({}));
    let names = tool_names(&list);
    assert_eq!(domain_tool_count(&names), 0, "{names:?}");
    let meta = &list["result"]["_meta"]["wicked-estate/domains"];
    assert_eq!(meta["available"], json!(false), "{list}");
    assert!(
        meta["reason"]
            .as_str()
            .is_some_and(|r| r.contains(".wicked")),
        "{list}"
    );

    for (id, tool, args) in [
        (3, "memory.capture", json!({"content": "x"})),
        (4, "knowledge.recall", json!({"query": "x"})),
        (5, "proposal.list", json!({})),
    ] {
        let reply = mcp.call(id, tool, args);
        let msg = reply["error"]["message"].as_str().unwrap_or("");
        assert!(
            msg.contains("not available") && msg.contains(".wicked"),
            "{tool} refusal must carry the cause: {reply}"
        );
    }
}

/// crew#931 still holds on a fresh home: a `--readonly` server creates no store directory (its
/// missing stores are served empty in memory), while its read tools still answer.
#[test]
fn fresh_home_readonly_creates_no_store_dir() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let mut mcp = McpChild::spawn(home.path(), cwd.path(), &[("--readonly", String::new())]);
    let init = mcp.request(1, "initialize", json!({"protocolVersion": "2024-11-05"}));
    assert!(init["result"].get("instructions").is_none(), "{init}");
    let recall = mcp.call(2, "memory.recall", json!({"query": "x"}));
    assert!(
        recall.get("error").is_none() && recall["result"]["isError"] != json!(true),
        "read-only memory.recall must answer (empty): {recall}"
    );
    drop(mcp);
    assert!(
        !home.path().join(".wicked").exists(),
        "a read-only server created ~/.wicked"
    );
}
