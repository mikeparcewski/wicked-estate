//! `knowledge.ingest {path}` over the REAL binary: a binary document (docx, pptx, text PDF) is
//! converted to Markdown in-process, redacted like any `.md`, chunked, and recalled with its path as
//! the cited source. A scanned PDF is refused as needing OCR (nothing is uploaded: the converter has
//! no network code); a secret-named file, or one outside the server's working directory, is never
//! read.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl Mcp {
    fn spawn(tmp: &tempfile::TempDir) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wicked-estate-mcp"));
        cmd.current_dir(tmp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, f) in [
            ("WICKED_ESTATE_DB", "estate.db"),
            ("WICKED_MEMORY_DB", "memory.db"),
            ("WICKED_KNOWLEDGE_DB", "knowledge.db"),
            ("WICKED_XEDGE_DB", "xedge.db"),
        ] {
            cmd.env(k, tmp.path().join(f));
        }
        let mut child = cmd.spawn().expect("spawn wicked-estate-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        let mut mcp = Mcp {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        let init = mcp.request(
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "doc-ingest-test", "version": "0"}}),
        );
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        mcp
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{req}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        assert!(
            self.reader.read_line(&mut line).unwrap() > 0,
            "server closed"
        );
        serde_json::from_str(line.trim()).unwrap()
    }

    /// The tool's result: `(is_error, text of content[0])`.
    fn call(&mut self, name: &str, args: Value) -> (bool, String) {
        let resp = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert!(resp.get("error").is_none(), "{name}: JSON-RPC error {resp}");
        (
            resp["result"]["isError"].as_bool().unwrap_or(false),
            resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

fn fixture(tmp: &tempfile::TempDir, name: &str, as_name: &str) -> String {
    let from = format!(
        "{}/../wicked-estate-extract/tests/fixtures/docs/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let to = tmp.path().join(as_name);
    std::fs::copy(from, &to).unwrap();
    to.to_str().unwrap().to_string()
}

#[test]
fn knowledge_ingest_path_converts_documents_and_refuses_a_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::spawn(&tmp);

    let docx = fixture(&tmp, "spec.docx", "spec.docx");
    let (err, text) = mcp.call(
        "knowledge.ingest",
        json!({"title": "Ledger spec", "path": docx, "scope": "project:ledger"}),
    );
    assert!(!err, "docx ingests: {text}");
    let pptx = fixture(&tmp, "deck.pptx", "deck.pptx");
    let (err, text) = mcp.call("knowledge.ingest", json!({"title": "Plan", "path": pptx}));
    assert!(!err, "pptx ingests: {text}");
    let pdf = fixture(&tmp, "runbook.pdf", "runbook.pdf");
    let (err, text) = mcp.call("knowledge.ingest", json!({"title": "Runbook", "path": pdf}));
    assert!(!err, "text pdf ingests: {text}");

    let (_, recalled) = mcp.call(
        "knowledge.recall",
        json!({"query": "reconcile_ledger posted entry bank line", "token_budget": 4000}),
    );
    let recalled: Value = serde_json::from_str(&recalled).unwrap();
    let items = recalled["items"].as_array().expect("items");
    let hit = items
        .iter()
        .find(|i| {
            i["body_snippet"]
                .as_str()
                .unwrap_or("")
                .contains("reconcile_ledger")
        })
        .unwrap_or_else(|| panic!("docx chunk recalled: {recalled}"));
    assert_eq!(hit["source"].as_str(), Some(docx.as_str()), "cited by path");
    // The Access section carries the fixture's AWS-key canary (doc_convert's unit test proves the
    // raw conversion holds it): recall that very chunk and see it masked.
    let (_, access) = mcp.call(
        "knowledge.recall",
        json!({"query": "Access use key here", "token_budget": 4000}),
    );
    let access: Value = serde_json::from_str(&access).unwrap();
    let chunk = access["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|i| {
            i["source"].as_str() == Some(docx.as_str())
                && i["body_snippet"].as_str().unwrap_or("").contains("Use key")
        })
        .unwrap_or_else(|| panic!("the Access chunk is recalled: {access}"));
    let body = chunk["body_snippet"].as_str().unwrap();
    assert!(
        !body.contains("AKIAABCDEFGHIJKLMNOP"),
        "redacted like markdown: {body}"
    );
    assert!(body.contains("here"), "{body}");

    let scan = fixture(&tmp, "scanned.pdf", "scanned.pdf");
    let (err, text) = mcp.call("knowledge.ingest", json!({"title": "Scan", "path": scan}));
    assert!(err, "a scan is refused: {text}");
    assert!(text.contains("needs OCR"), "{text}");

    let secret = fixture(&tmp, "spec.docx", "credentials.docx");
    let (err, text) = mcp.call("knowledge.ingest", json!({"title": "C", "path": secret}));
    assert!(
        err && text.contains("secret-file rule"),
        "never read: {text}"
    );

    let elsewhere = tempfile::tempdir().unwrap();
    let outside = fixture(&elsewhere, "spec.docx", "private.docx");
    let (err, text) = mcp.call("knowledge.ingest", json!({"title": "P", "path": outside}));
    assert!(
        err && text.contains("working directory"),
        "confined like the indexer: {text}"
    );

    let resp = mcp.request(
        "tools/call",
        json!({"name": "knowledge.ingest",
               "arguments": {"title": "T", "path": docx, "chunks": ["c"]}}),
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "path and chunks together: {resp}"
    );
    let resp = mcp.request(
        "tools/call",
        json!({"name": "knowledge.ingest",
               "arguments": {"title": "T", "path": docx, "chunks": "not-an-array"}}),
    );
    assert_eq!(
        resp["error"]["code"], -32602,
        "a malformed chunks field: {resp}"
    );
}
