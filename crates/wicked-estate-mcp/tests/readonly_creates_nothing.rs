//! Regression tests for crew#931: a read-only MCP server never creates a file.
//! Missing stores must not populate a governed state home or the default cwd;
//! existing stores must still serve the memory and knowledge domains.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

struct McpChild {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Result<Value, String>>,
    pending: BTreeMap<u64, Value>,
    deadline: Instant,
}

impl McpChild {
    fn spawn(home: &Path, cwd: &Path, state: Option<&Path>, readonly: bool) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
        cmd.env_clear()
            .env("HOME", home)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(state) = state {
            cmd.env("WICKED_HOME", state)
                .env("WICKED_ESTATE_DB", state.join("graph.db"));
        }
        if readonly {
            cmd.arg("--readonly");
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut child = cmd.spawn().expect("spawn wicked-estate-mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let response = line
                    .map_err(|e| format!("read server stdout: {e}"))
                    .and_then(|line| {
                        serde_json::from_str(&line)
                            .map_err(|e| format!("invalid server JSON ({e}): {line:?}"))
                    });
                let failed = response.is_err();
                if sender.send(response).is_err() || failed {
                    break;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            responses,
            pending: BTreeMap::new(),
            deadline,
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().expect("stdin still open");
        writeln!(stdin, "{message}").expect("write request");
        stdin.flush().expect("flush request");
    }

    fn recv_id(&mut self, id: u64) -> Value {
        if let Some(response) = self.pending.remove(&id) {
            return response;
        }
        loop {
            let response = self
                .responses
                .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| panic!("no response for id {id} within 20 seconds: {e}"))
                .unwrap_or_else(|e| panic!("{e}"));
            if let Some(response_id) = response["id"].as_u64() {
                if response_id == id {
                    return response;
                }
                self.pending.insert(response_id, response);
            }
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.recv_id(id)
    }

    fn initialize(&mut self) -> Value {
        self.request(
            1,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "readonly-regression", "version": "0"}
            }),
        )
    }

    fn initialized(&mut self) {
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    }

    fn finish(mut self) {
        drop(self.stdin.take());
        loop {
            if self.child.try_wait().expect("poll server exit").is_some() {
                return;
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.child.kill().expect("kill server after 20 seconds");
                self.child.wait().expect("reap server");
                return;
            }
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        drop(self.stdin.take());
        // Also reap the child if a request times out or an assertion panics.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn entries(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .expect("list test directory")
        .map(|entry| {
            entry
                .expect("read directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn readonly_with_wicked_home_creates_no_store() {
    let state = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut mcp = McpChild::spawn(home.path(), home.path(), Some(state.path()), true);
    let init = mcp.initialize();
    mcp.initialized();
    let memory = mcp.request(
        2,
        "tools/call",
        json!({"name": "memory.recall", "arguments": {"query": "release"}}),
    );
    let knowledge = mcp.request(
        3,
        "tools/call",
        json!({"name": "knowledge.recall", "arguments": {"query": "release"}}),
    );
    mcp.finish();

    // The domains are still served (empty), not disabled.
    for (tool, reply) in [("memory.recall", &memory), ("knowledge.recall", &knowledge)] {
        assert!(
            reply.get("error").is_none() && reply["result"]["isError"] != json!(true),
            "{tool} must answer (empty) under --readonly: {reply}"
        );
    }

    assert!(init.get("error").is_none(), "initialize failed: {init}");
    let names = entries(state.path());
    assert!(
        names.is_empty(),
        "read-only created state entries: {names:?}"
    );
}

#[test]
fn readonly_with_existing_stores_still_serves() {
    let state = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut writer = McpChild::spawn(home.path(), home.path(), Some(state.path()), false);
    let init = writer.initialize();
    writer.finish();
    assert!(init.get("error").is_none(), "store setup failed: {init}");
    let before = entries(state.path());
    for store in ["graph.db", "memory.db", "knowledge.db", "xedge.db"] {
        assert!(state.path().join(store).is_file(), "setup missing {store}");
    }

    let mut mcp = McpChild::spawn(home.path(), home.path(), Some(state.path()), true);
    let init = mcp.initialize();
    mcp.initialized();
    let list = mcp.request(2, "tools/list", json!({}));
    mcp.finish();
    assert!(init.get("error").is_none(), "initialize failed: {init}");
    let tools = list["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list missing tools: {list}"));
    for prefix in ["memory.", "knowledge."] {
        assert!(
            tools.iter().any(|tool| tool["name"]
                .as_str()
                .is_some_and(|name| name.starts_with(prefix))),
            "tools/list missing {prefix} domain: {list}"
        );
    }
    let after = entries(state.path());
    let unexpected: Vec<_> = after
        .difference(&before)
        .filter(|name| {
            !["-wal", "-shm"].iter().any(|suffix| {
                name.strip_suffix(suffix)
                    .is_some_and(|base| before.contains(base) && state.path().join(base).is_file())
            })
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "read-only created entries: {unexpected:?}"
    );
}

#[test]
fn readonly_missing_graph_creates_no_file_in_cwd() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let mut mcp = McpChild::spawn(home.path(), cwd.path(), None, true);
    let init = mcp.initialize();
    mcp.initialized();
    let _ = mcp.request(2, "tools/list", json!({}));
    mcp.finish();

    assert!(init.get("error").is_none(), "initialize failed: {init}");
    let names = entries(cwd.path());
    assert!(names.is_empty(), "read-only created cwd entries: {names:?}");
}
