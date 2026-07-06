//! Integration tests for `wicked-estate-mcp` — L2.5 env var contracts and
//! L2.7 MCP config migration smoke tests.
//!
//! These tests spawn the real `wicked-estate-mcp` binary and communicate with
//! it over newline-delimited JSON-RPC 2.0 stdio.
//!
//! Covered:
//! - ENV-001: `WICKED_ESTATE_DB` custom path → file created at that path
//! - ENV-002: `WICKED_MEMORY_DB` custom path → file created at that path
//! - ENV-003: `WICKED_KNOWLEDGE_DB` custom path → file created at that path
//! - ENV-004: `WICKED_XEDGE_DB` custom path → file created at that path
//! - ENV-006: No env vars → estate at `CWD/.wicked-estate/graph.db` (DEFAULT_DB)
//! - SMOKE-001: Memory migration smoke — all 6 `memory.*` tools in `tools/list`
//! - SMOKE-002: Knowledge migration smoke — all 7 `knowledge.*` tools in `tools/list`
//!
//! # Default path reality (from main.rs)
//!
//! ```text
//! WICKED_ESTATE_DB  absent → ".wicked-estate/graph.db"  (relative to CWD; DEFAULT_DB)
//! WICKED_MEMORY_DB  absent → "$HOME/.wicked/memory.db"
//! WICKED_KNOWLEDGE_DB absent → "$HOME/.wicked/knowledge.db"
//! WICKED_XEDGE_DB   absent → "$HOME/.wicked/xedge.db"
//! ```
//!
//! SQLite (bundled, static) creates the DB file when the *parent directory*
//! already exists.  Tests for ENV-001..004 use a single tempdir as the parent
//! so all four stores open without contention.  ENV-006 pre-creates
//! `CWD/.wicked-estate/` and `$HOME/.wicked/` so the defaults resolve.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

// ── MCP subprocess wrapper ────────────────────────────────────────────────────

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl McpChild {
    /// Spawn the binary with extra env vars on top of the current environment.
    fn spawn_with_envs(binary: &str, envs: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(binary);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        Self::from_cmd(cmd)
    }

    /// Spawn with a completely cleared environment, the given vars, and the
    /// given working directory.  Keeps `PATH` in the supplied `envs` slice if
    /// dynamic linker resolution is needed; rusqlite is bundled so it is not.
    fn spawn_cleared(binary: &str, envs: &[(&str, &str)], current_dir: &std::path::Path) -> Self {
        let mut cmd = Command::new(binary);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .current_dir(current_dir);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        Self::from_cmd(cmd)
    }

    fn from_cmd(mut cmd: Command) -> Self {
        let mut child = cmd.spawn().expect("failed to spawn wicked-estate-mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        McpChild {
            child,
            stdin,
            reader: BufReader::new(stdout),
        }
    }

    /// Write one newline-terminated JSON-RPC request to the child's stdin.
    fn send(&mut self, req: &Value) {
        writeln!(self.stdin, "{}", req).expect("write to child stdin");
        self.stdin.flush().expect("flush child stdin");
    }

    /// Read one newline-terminated JSON-RPC response from the child's stdout.
    fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .expect("read from child stdout");
        assert!(
            n > 0,
            "server stdout closed before a response arrived (process may have crashed)"
        );
        serde_json::from_str(line.trim())
            .unwrap_or_else(|e| panic!("server sent invalid JSON ({e})\nraw: {line:?}"))
    }

    /// Send an MCP `initialize` request and return the response.
    fn initialize(&mut self) -> Value {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        });
        self.send(&req);
        self.recv()
    }

    /// Send a `tools/list` request and return the response.
    fn tools_list(&mut self) -> Value {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        self.send(&req);
        self.recv()
    }

    /// Kill the process and wait for it to exit.  Drops stdin first so the
    /// binary sees EOF and can shut down cleanly before the kill signal.
    fn finish(self) {
        let McpChild {
            mut child,
            stdin,
            reader,
        } = self;
        drop(stdin);
        drop(reader);
        child.kill().ok();
        child.wait().ok();
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Spawn a binary with all four store env vars pointing to distinct files in
/// `tmp`.  Returns the `McpChild` and the tempdir (caller must keep it alive).
fn spawn_all_domains() -> (McpChild, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("create tempdir");

    let estate_path = tmp.path().join("estate.db");
    let mem_path = tmp.path().join("memory.db");
    let know_path = tmp.path().join("knowledge.db");
    let xedge_path = tmp.path().join("xedge.db");

    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let mcp = McpChild::spawn_with_envs(
        binary,
        &[
            ("WICKED_ESTATE_DB", estate_path.to_str().unwrap()),
            ("WICKED_MEMORY_DB", mem_path.to_str().unwrap()),
            ("WICKED_KNOWLEDGE_DB", know_path.to_str().unwrap()),
            ("WICKED_XEDGE_DB", xedge_path.to_str().unwrap()),
        ],
    );
    (mcp, tmp)
}

// ── ENV-001 ────────────────────────────────────────────────────────────────────

/// ENV-001: WICKED_ESTATE_DB overrides the estate store path.
///
/// All four env vars are set to distinct files inside a tempdir.  After the
/// server responds to `initialize`, the file at the custom estate path must
/// exist (SQLite creates it on open if the parent directory is present).
#[test]
fn env_001_wicked_estate_db_custom_path() {
    let tmp = tempfile::tempdir().unwrap();
    let estate_path = tmp.path().join("custom_estate.db");
    let mem_path = tmp.path().join("memory.db");
    let know_path = tmp.path().join("knowledge.db");
    let xedge_path = tmp.path().join("xedge.db");

    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let mut mcp = McpChild::spawn_with_envs(
        binary,
        &[
            ("WICKED_ESTATE_DB", estate_path.to_str().unwrap()),
            ("WICKED_MEMORY_DB", mem_path.to_str().unwrap()),
            ("WICKED_KNOWLEDGE_DB", know_path.to_str().unwrap()),
            ("WICKED_XEDGE_DB", xedge_path.to_str().unwrap()),
        ],
    );

    let resp = mcp.initialize();
    mcp.finish();

    assert!(resp.get("error").is_none(), "initialize failed: {resp}");
    assert!(
        resp["result"]["capabilities"]["tools"].is_object(),
        "capabilities.tools must be present: {resp}"
    );
    assert!(
        estate_path.exists(),
        "WICKED_ESTATE_DB was not used for the estate store: \
         no file at {}",
        estate_path.display()
    );
}

// ── ENV-002 ────────────────────────────────────────────────────────────────────

/// ENV-002: WICKED_MEMORY_DB overrides the memory store path.
#[test]
fn env_002_wicked_memory_db_custom_path() {
    let tmp = tempfile::tempdir().unwrap();
    let estate_path = tmp.path().join("estate.db");
    let mem_path = tmp.path().join("custom_memory.db");
    let know_path = tmp.path().join("knowledge.db");
    let xedge_path = tmp.path().join("xedge.db");

    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let mut mcp = McpChild::spawn_with_envs(
        binary,
        &[
            ("WICKED_ESTATE_DB", estate_path.to_str().unwrap()),
            ("WICKED_MEMORY_DB", mem_path.to_str().unwrap()),
            ("WICKED_KNOWLEDGE_DB", know_path.to_str().unwrap()),
            ("WICKED_XEDGE_DB", xedge_path.to_str().unwrap()),
        ],
    );

    let resp = mcp.initialize();
    mcp.finish();

    assert!(resp.get("error").is_none(), "initialize failed: {resp}");
    assert!(
        resp["result"]["capabilities"]["tools"].is_object(),
        "capabilities.tools must be present: {resp}"
    );
    assert!(
        mem_path.exists(),
        "WICKED_MEMORY_DB was not used for the memory store: \
         no file at {}",
        mem_path.display()
    );
}

// ── ENV-003 ────────────────────────────────────────────────────────────────────

/// ENV-003: WICKED_KNOWLEDGE_DB overrides the knowledge store path.
#[test]
fn env_003_wicked_knowledge_db_custom_path() {
    let tmp = tempfile::tempdir().unwrap();
    let estate_path = tmp.path().join("estate.db");
    let mem_path = tmp.path().join("memory.db");
    let know_path = tmp.path().join("custom_knowledge.db");
    let xedge_path = tmp.path().join("xedge.db");

    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let mut mcp = McpChild::spawn_with_envs(
        binary,
        &[
            ("WICKED_ESTATE_DB", estate_path.to_str().unwrap()),
            ("WICKED_MEMORY_DB", mem_path.to_str().unwrap()),
            ("WICKED_KNOWLEDGE_DB", know_path.to_str().unwrap()),
            ("WICKED_XEDGE_DB", xedge_path.to_str().unwrap()),
        ],
    );

    let resp = mcp.initialize();
    mcp.finish();

    assert!(resp.get("error").is_none(), "initialize failed: {resp}");
    assert!(
        resp["result"]["capabilities"]["tools"].is_object(),
        "capabilities.tools must be present: {resp}"
    );
    assert!(
        know_path.exists(),
        "WICKED_KNOWLEDGE_DB was not used for the knowledge store: \
         no file at {}",
        know_path.display()
    );
}

// ── ENV-004 ────────────────────────────────────────────────────────────────────

/// ENV-004: WICKED_XEDGE_DB overrides the overlay/xedge store path.
#[test]
fn env_004_wicked_xedge_db_custom_path() {
    let tmp = tempfile::tempdir().unwrap();
    let estate_path = tmp.path().join("estate.db");
    let mem_path = tmp.path().join("memory.db");
    let know_path = tmp.path().join("knowledge.db");
    let xedge_path = tmp.path().join("custom_xedge.db");

    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let mut mcp = McpChild::spawn_with_envs(
        binary,
        &[
            ("WICKED_ESTATE_DB", estate_path.to_str().unwrap()),
            ("WICKED_MEMORY_DB", mem_path.to_str().unwrap()),
            ("WICKED_KNOWLEDGE_DB", know_path.to_str().unwrap()),
            ("WICKED_XEDGE_DB", xedge_path.to_str().unwrap()),
        ],
    );

    let resp = mcp.initialize();
    mcp.finish();

    assert!(resp.get("error").is_none(), "initialize failed: {resp}");
    assert!(
        resp["result"]["capabilities"]["tools"].is_object(),
        "capabilities.tools must be present: {resp}"
    );
    assert!(
        xedge_path.exists(),
        "WICKED_XEDGE_DB was not used for the xedge store: \
         no file at {}",
        xedge_path.display()
    );
}

// ── ENV-006 ────────────────────────────────────────────────────────────────────

/// ENV-006: No WICKED_* env vars → defaults are used.
///
/// Default paths (from main.rs):
/// - Estate: `DEFAULT_DB = ".wicked-estate/graph.db"` relative to CWD
/// - Memory/Knowledge/Xedge: `$HOME/.wicked/{memory,knowledge,xedge}.db`
///
/// We pre-create the expected parent directories so SQLite can create the
/// files, then assert the estate DB lands at `CWD/.wicked-estate/graph.db`.
#[test]
fn env_006_no_env_vars_uses_default_path() {
    let cwd_dir = tempfile::tempdir().unwrap();
    let home_dir = tempfile::tempdir().unwrap();

    // Pre-create parent directories the defaults require.
    std::fs::create_dir(cwd_dir.path().join(".wicked-estate")).expect("create CWD/.wicked-estate");
    std::fs::create_dir(home_dir.path().join(".wicked")).expect("create $HOME/.wicked");

    let binary = env!("CARGO_BIN_EXE_wicked-estate-mcp");
    let path_val = std::env::var("PATH").unwrap_or_default();

    let mut mcp = McpChild::spawn_cleared(
        binary,
        &[
            ("HOME", home_dir.path().to_str().unwrap()),
            ("PATH", &path_val),
        ],
        cwd_dir.path(),
    );

    let resp = mcp.initialize();
    mcp.finish();

    assert!(
        resp.get("error").is_none(),
        "initialize failed with default paths: {resp}"
    );

    // main.rs: const DEFAULT_DB: &str = ".wicked-estate/graph.db";
    let default_estate = cwd_dir.path().join(".wicked-estate").join("graph.db");
    assert!(
        default_estate.exists(),
        "default estate DB was not created at CWD/.wicked-estate/graph.db \
         (actual path: {})",
        default_estate.display()
    );
}

// ── SMOKE tests ───────────────────────────────────────────────────────────────

const MEMORY_TOOLS: &[&str] = &[
    "memory.capture",
    "memory.recall",
    "memory.reflect",
    "memory.erase",
    "memory.learn",
    "memory.coverage",
];

const KNOWLEDGE_TOOLS: &[&str] = &[
    "knowledge.ingest",
    "knowledge.write",
    "knowledge.relate",
    "knowledge.recall",
    "knowledge.coverage",
    "knowledge.relate_code",
    "knowledge.recall_about_code",
];

/// SMOKE-001: Replacing `wicked-memory-mcp` with `wicked-estate-mcp` — all 6
/// `memory.*` tools must appear in `tools/list` when all four stores are wired.
#[test]
fn smoke_001_memory_migration_all_6_tools_present() {
    let (mut mcp, _tmp) = spawn_all_domains();

    let init_resp = mcp.initialize();
    assert!(
        init_resp.get("error").is_none(),
        "initialize failed: {init_resp}"
    );

    let list_resp = mcp.tools_list();
    mcp.finish();

    assert!(
        list_resp.get("error").is_none(),
        "tools/list failed: {list_resp}"
    );

    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("tools/list result.tools must be an array");

    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in MEMORY_TOOLS {
        assert!(
            names.contains(expected),
            "SMOKE-001: memory tool '{expected}' missing from tools/list\ngot: {names:?}"
        );
    }
}

/// SMOKE-002: Replacing `wicked-knowledge-mcp` with `wicked-estate-mcp` — all
/// 7 `knowledge.*` tools must appear in `tools/list` when all four stores are wired.
#[test]
fn smoke_002_knowledge_migration_all_7_tools_present() {
    let (mut mcp, _tmp) = spawn_all_domains();

    let init_resp = mcp.initialize();
    assert!(
        init_resp.get("error").is_none(),
        "initialize failed: {init_resp}"
    );

    let list_resp = mcp.tools_list();
    mcp.finish();

    assert!(
        list_resp.get("error").is_none(),
        "tools/list failed: {list_resp}"
    );

    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("tools/list result.tools must be an array");

    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in KNOWLEDGE_TOOLS {
        assert!(
            names.contains(expected),
            "SMOKE-002: knowledge tool '{expected}' missing from tools/list\ngot: {names:?}"
        );
    }
}
