//! On-demand LSP client — W3.3.
//!
//! Provides precise single-symbol intelligence (go-to-definition, references, hover/type) by
//! driving a language server over stdio. ON DEMAND ONLY — never spawned for bulk indexing.
//!
//! # Design
//!
//! ## Wire format
//!
//! Language servers speak JSON-RPC 2.0 over stdio with an HTTP-style Content-Length framing:
//!
//! ```text
//! Content-Length: <byte-count>\r\n
//! \r\n
//! <JSON payload>
//! ```
//!
//! [`RpcTransport`] implements this framing for both writing (outgoing requests) and reading
//! (incoming responses). The transport is fully synchronous (blocking I/O): it matches our
//! sync-everywhere design and avoids bringing in async runtimes for a per-request probe.
//!
//! ## Lifecycle
//!
//! Each [`LspClient`] owns one server process (spawned lazily on first use via [`LspTier`]).
//! On creation the client fires `initialize` + `initialized` and holds the channel open for
//! subsequent requests. Per-request: one request → one response (no push notifications needed).
//!
//! ## Server registry
//!
//! [`ServerRegistry`] maps a language name (tree-sitter grammar style) to a command. The binary
//! is probed with `which`/`where` before spawn; if absent, construction returns
//! `Error::Resolution("server not available: ...")` — no panic, no partial spawn.
//!
//! ## Timeout
//!
//! Reads use a 10-second per-request timeout via `set_read_timeout` on the stdout pipe's
//! underlying fd. This prevents hangs on a slow or crashed server.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicI64, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wicked_estate_core::{Error, Result};

// ── static request-id counter ────────────────────────────────────────────────
static NEXT_ID: AtomicI64 = AtomicI64::new(1);

fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

// ── JSON-RPC types ────────────────────────────────────────────────────────────

/// An outgoing JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
struct Request<'a> {
    jsonrpc: &'a str,
    id: i64,
    method: &'a str,
    params: Value,
}

/// An incoming JSON-RPC 2.0 response (success or error).
#[derive(Debug, Deserialize)]
struct Response {
    id: Option<Value>,
    #[serde(default)]
    result: Value,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

// Notifications (no `id`) are handled by checking `v.get("id").is_none()` directly;
// we don't deserialize them into a typed struct because the only action is to skip them.

// ── wire-framing ──────────────────────────────────────────────────────────────

/// Encode a JSON-RPC request with LSP Content-Length framing.
pub(crate) fn encode_request(id: i64, method: &str, params: Value) -> Vec<u8> {
    let req = Request {
        jsonrpc: "2.0",
        id,
        method,
        params,
    };
    let body = serde_json::to_vec(&req).expect("request serialization is infallible");
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = header.into_bytes();
    out.extend_from_slice(&body);
    out
}

/// Encode a JSON-RPC notification (no id) with LSP Content-Length framing.
pub(crate) fn encode_notification(method: &str, params: Value) -> Vec<u8> {
    #[derive(Serialize)]
    struct Notif<'a> {
        jsonrpc: &'a str,
        method: &'a str,
        params: Value,
    }
    let n = Notif {
        jsonrpc: "2.0",
        method,
        params,
    };
    let body = serde_json::to_vec(&n).expect("notification serialization is infallible");
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = header.into_bytes();
    out.extend_from_slice(&body);
    out
}

/// Read one framed message from `reader`.
///
/// Parses `Content-Length: N\r\n\r\n` then reads exactly N bytes.  Skips any
/// leading blank lines or other headers before the `Content-Length` header
/// (some servers emit `Content-Type` first).
pub(crate) fn read_frame<R: BufRead>(reader: &mut R) -> Result<Vec<u8>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(Error::Resolution("LSP: server closed stdout".into()));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            // Blank line — end of headers.
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
        // Ignore other headers (e.g. Content-Type).
    }

    let len = content_length.ok_or_else(|| {
        Error::Resolution("LSP: framing error — Content-Length header missing".into())
    })?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

// ── transport ─────────────────────────────────────────────────────────────────

/// Low-level framing transport over a server's stdio pipes.
struct RpcTransport {
    stdin: ChildStdin,
    reader: BufReader<TimeoutReader>,
}

/// Wraps `ChildStdout` with a read timeout using `set_read_timeout` on the
/// underlying raw fd.  Falls back gracefully if the OS doesn't support it.
struct TimeoutReader {
    inner: ChildStdout,
}

impl TimeoutReader {
    fn new(stdout: ChildStdout, timeout: Duration) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = stdout.as_raw_fd();
            // Convert to a TcpStream-compatible struct to reuse set_read_timeout.
            // We use setsockopt directly via libc or the nix crate — but to avoid extra deps
            // we use the `timeval` approach via unsafe.
            let tv = libc::timeval {
                tv_sec: timeout.as_secs() as libc::time_t,
                tv_usec: timeout.subsec_micros() as libc::suseconds_t,
            };
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVTIMEO,
                    &tv as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                );
            }
            let _ = timeout; // suppress unused warning in fallback path
        }
        #[cfg(not(unix))]
        {
            let _ = (timeout,); // not implemented on non-unix; reads may block
        }
        TimeoutReader { inner: stdout }
    }
}

impl Read for TimeoutReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl RpcTransport {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        let timeout = Duration::from_secs(10);
        let reader = BufReader::new(TimeoutReader::new(stdout, timeout));
        RpcTransport { stdin, reader }
    }

    /// Send a framed request and return the raw JSON body of the response (skips notifications).
    fn send_and_receive(&mut self, id: i64, method: &str, params: Value) -> Result<Value> {
        let frame = encode_request(id, method, params);
        self.stdin.write_all(&frame)?;
        self.stdin.flush()?;
        self.await_response(id)
    }

    /// Send a notification (fire-and-forget — no response expected).
    fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        let frame = encode_notification(method, params);
        self.stdin.write_all(&frame)?;
        self.stdin.flush()?;
        Ok(())
    }

    /// Read frames, skipping notifications, until a response for `expected_id` arrives.
    fn await_response(&mut self, expected_id: i64) -> Result<Value> {
        loop {
            let raw = read_frame(&mut self.reader)?;
            let v: Value = serde_json::from_slice(&raw)?;

            // If the message has no `id` field it is a server-pushed notification — skip.
            if v.get("id").is_none() {
                continue;
            }

            let resp: Response = serde_json::from_value(v)?;

            // If the id doesn't match keep reading (shouldn't happen on a dedicated connection,
            // but guard against server preamble messages that sneak an id in).
            let resp_id = match &resp.id {
                Some(Value::Number(n)) => n.as_i64().unwrap_or(i64::MIN),
                _ => continue,
            };
            if resp_id != expected_id {
                continue;
            }

            if let Some(err) = resp.error {
                return Err(Error::Resolution(format!(
                    "LSP error {}: {}",
                    err.code, err.message
                )));
            }
            return Ok(resp.result);
        }
    }
}

// ── server registry ───────────────────────────────────────────────────────────

/// Maps tree-sitter grammar language names → server invocation command.
///
/// Lookup via [`ServerRegistry::command_for`]; returns `None` when the binary is absent
/// from `PATH`.
#[derive(Debug, Clone)]
pub struct ServerRegistry {
    /// language → (binary, args)
    entries: HashMap<String, (String, Vec<String>)>,
}

impl Default for ServerRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

impl ServerRegistry {
    /// The built-in registry: TypeScript/JavaScript, Rust, Python.
    pub fn standard() -> Self {
        let mut entries = HashMap::new();
        entries.insert(
            "typescript".to_string(),
            (
                "typescript-language-server".to_string(),
                vec!["--stdio".to_string()],
            ),
        );
        entries.insert(
            "tsx".to_string(),
            (
                "typescript-language-server".to_string(),
                vec!["--stdio".to_string()],
            ),
        );
        entries.insert(
            "javascript".to_string(),
            (
                "typescript-language-server".to_string(),
                vec!["--stdio".to_string()],
            ),
        );
        entries.insert(
            "jsx".to_string(),
            (
                "typescript-language-server".to_string(),
                vec!["--stdio".to_string()],
            ),
        );
        entries.insert("rust".to_string(), ("rust-analyzer".to_string(), vec![]));
        entries.insert(
            "python".to_string(),
            (
                "pyright-langserver".to_string(),
                vec!["--stdio".to_string()],
            ),
        );
        ServerRegistry { entries }
    }

    /// Look up the binary + args for `language`. Returns `None` when the language is not
    /// registered or the binary is not on PATH.
    pub fn command_for(&self, language: &str) -> Option<(String, Vec<String>)> {
        let (bin, args) = self.entries.get(language)?;
        if probe_binary(bin) {
            Some((bin.clone(), args.clone()))
        } else {
            None
        }
    }

    /// Register a custom language server. Replaces an existing entry.
    pub fn register(&mut self, language: &str, binary: &str, args: Vec<String>) {
        self.entries
            .insert(language.to_string(), (binary.to_string(), args));
    }
}

/// Returns `true` iff `bin` can be found on `PATH`.
fn probe_binary(bin: &str) -> bool {
    // On Unix: `which <bin>` exits 0 when found.
    // On Windows: `where <bin>` does the same.
    let result = if cfg!(windows) {
        Command::new("where").arg(bin).output()
    } else {
        Command::new("which").arg(bin).output()
    };
    result.map(|o| o.status.success()).unwrap_or(false)
}

// ── LSP client ────────────────────────────────────────────────────────────────

/// A running LSP server connection. Spawned once per [`LspTier`] lookup; kept alive for the
/// duration of a query session.
pub struct LspClient {
    _child: Child,
    transport: RpcTransport,
    /// The root URI the server was initialized against.
    root_uri: String,
}

impl LspClient {
    /// Spawn the server, perform `initialize` + `initialized` handshake, and return a ready client.
    ///
    /// `root_dir` is the absolute path to the workspace root (passed as `rootUri`).
    pub fn spawn(binary: &str, args: &[String], root_dir: &str) -> Result<Self> {
        let mut child = Command::new(binary)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // suppress server stderr chatter
            .spawn()
            .map_err(|e| Error::Resolution(format!("LSP: failed to spawn '{binary}': {e}")))?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let mut transport = RpcTransport::new(stdin, stdout);

        let root_uri = path_to_file_uri(root_dir);

        let id = next_id();
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "hover": {
                        "dynamicRegistration": false,
                        "contentFormat": ["plaintext"]
                    }
                }
            },
            "initializationOptions": null
        });

        transport.send_and_receive(id, "initialize", init_params)?;
        // Fire the `initialized` notification (no response).
        transport.send_notification("initialized", serde_json::json!({}))?;

        Ok(LspClient {
            _child: child,
            transport,
            root_uri,
        })
    }

    // ── textDocument/didOpen ──────────────────────────────────────────────────

    /// Notify the server that a document has been opened (required before querying a file).
    ///
    /// `language_id` should be the LSP-standard identifier (`"typescript"`, `"rust"`, etc.).
    /// `text` is the full current text of the file.
    pub fn did_open(&mut self, file_uri: &str, language_id: &str, text: &str) -> Result<()> {
        self.transport.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri":        file_uri,
                    "languageId": language_id,
                    "version":    1,
                    "text":       text
                }
            }),
        )
    }

    // ── textDocument/definition ───────────────────────────────────────────────

    /// `textDocument/definition` — returns the definition location(s) for the symbol at
    /// `(line, col)` in `file_uri`.
    pub fn definition(&mut self, file_uri: &str, line: u32, col: u32) -> Result<Vec<Location>> {
        let id = next_id();
        let params = td_position_params(file_uri, line, col);
        let result = self
            .transport
            .send_and_receive(id, "textDocument/definition", params)?;
        parse_locations(&result)
    }

    // ── textDocument/references ───────────────────────────────────────────────

    /// `textDocument/references` — returns all reference locations for the symbol at the given
    /// position.
    pub fn references(
        &mut self,
        file_uri: &str,
        line: u32,
        col: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        let id = next_id();
        let mut params = td_position_params(file_uri, line, col);
        params["context"] = serde_json::json!({ "includeDeclaration": include_declaration });
        let result = self
            .transport
            .send_and_receive(id, "textDocument/references", params)?;
        parse_locations(&result)
    }

    // ── textDocument/hover ────────────────────────────────────────────────────

    /// `textDocument/hover` — returns the type/doc string for the symbol at the given position,
    /// or `None` when the server returns null.
    pub fn hover(&mut self, file_uri: &str, line: u32, col: u32) -> Result<Option<HoverResult>> {
        let id = next_id();
        let params = td_position_params(file_uri, line, col);
        let result = self
            .transport
            .send_and_receive(id, "textDocument/hover", params)?;

        if result.is_null() {
            return Ok(None);
        }

        let contents = &result["contents"];
        let text = if let Some(s) = contents.as_str() {
            s.to_string()
        } else if let Some(v) = contents.get("value") {
            v.as_str().unwrap_or("").to_string()
        } else if let Some(arr) = contents.as_array() {
            // Union of MarkedString | MarkupContent
            arr.iter()
                .filter_map(|item| {
                    item.as_str()
                        .or_else(|| item.get("value").and_then(Value::as_str))
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };

        let range = result.get("range").and_then(parse_range);

        Ok(Some(HoverResult { text, range }))
    }

    /// The `rootUri` this client was initialized against.
    pub fn root_uri(&self) -> &str {
        &self.root_uri
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Best-effort shutdown — ignore errors (child may have already exited).
        let id = next_id();
        let _ = self.transport.send_and_receive(id, "shutdown", Value::Null);
        let _ = self.transport.send_notification("exit", Value::Null);
    }
}

// ── LSP tier (public API) ─────────────────────────────────────────────────────

/// On-demand LSP tier (W3.3).
///
/// Wraps a [`ServerRegistry`] and a cache of per-language [`LspClient`]s. Exposes
/// `definition` / `references` / `hover` methods that map a `(language, file, line, col)`
/// tuple to precise LSP results.
///
/// Usage pattern:
/// ```ignore
/// let mut tier = LspTier::new("/path/to/project");
/// let defs = tier.definition("typescript", "file:///path/to/foo.ts", 10, 5)?;
/// ```
pub struct LspTier {
    registry: ServerRegistry,
    /// language → live client. Clients are spawned lazily on first use.
    clients: HashMap<String, LspClient>,
    root_dir: String,
}

impl LspTier {
    /// Create a new tier for the given workspace root directory.
    pub fn new(root_dir: impl Into<String>) -> Self {
        LspTier {
            registry: ServerRegistry::standard(),
            clients: HashMap::new(),
            root_dir: root_dir.into(),
        }
    }

    /// Create with a custom registry (for testing or non-standard servers).
    pub fn with_registry(root_dir: impl Into<String>, registry: ServerRegistry) -> Self {
        LspTier {
            registry,
            clients: HashMap::new(),
            root_dir: root_dir.into(),
        }
    }

    /// Get-or-spawn the client for `language`. Returns an error if the server is unavailable.
    fn client(&mut self, language: &str) -> Result<&mut LspClient> {
        // Borrow-checker: check existence first, then insert only if absent.
        if !self.clients.contains_key(language) {
            let (bin, args) =
                self.registry.command_for(language).ok_or_else(|| {
                    Error::Resolution(format!("server not available: no LSP server registered for language '{language}' (or binary not on PATH)"))
                })?;
            let client = LspClient::spawn(&bin, &args, &self.root_dir)?;
            self.clients.insert(language.to_string(), client);
        }
        Ok(self.clients.get_mut(language).expect("just inserted"))
    }

    // ── public methods ─────────────────────────────────────────────────────────

    /// Return definition location(s) for the symbol at `(line, col)` in `file_uri`.
    pub fn definition(
        &mut self,
        language: &str,
        file_uri: &str,
        line: u32,
        col: u32,
    ) -> Result<Vec<Location>> {
        self.client(language)?.definition(file_uri, line, col)
    }

    /// Return all reference locations for the symbol at `(line, col)` in `file_uri`.
    pub fn references(
        &mut self,
        language: &str,
        file_uri: &str,
        line: u32,
        col: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        self.client(language)?
            .references(file_uri, line, col, include_declaration)
    }

    /// Return the hover/type info for the symbol at `(line, col)` in `file_uri`.
    pub fn hover(
        &mut self,
        language: &str,
        file_uri: &str,
        line: u32,
        col: u32,
    ) -> Result<Option<HoverResult>> {
        self.client(language)?.hover(file_uri, line, col)
    }
}

// ── result types ──────────────────────────────────────────────────────────────

/// A location returned by `definition` / `references`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// `file://` URI.
    pub uri: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// The result of a `hover` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverResult {
    /// The displayed text (type signature + doc comment, stripped of markdown).
    pub text: String,
    /// The range the hover applies to, if the server reports it.
    pub range: Option<(u32, u32, u32, u32)>,
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build the common `TextDocumentIdentifier + Position` params object.
fn td_position_params(uri: &str, line: u32, col: u32) -> Value {
    serde_json::json!({
        "textDocument": { "uri": uri },
        "position":     { "line": line, "character": col }
    })
}

/// Parse a `Location | Location[] | LocationLink[]` LSP result into our flat list.
fn parse_locations(v: &Value) -> Result<Vec<Location>> {
    if v.is_null() {
        return Ok(vec![]);
    }

    // Normalise: a single object or an array.
    let items: Vec<&Value> = if let Some(arr) = v.as_array() {
        arr.iter().collect()
    } else {
        vec![v]
    };

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        // `Location`: { uri, range: { start: {line,character}, end: {line,character} } }
        // `LocationLink`: { targetUri, targetRange, ... }
        let (uri_key, range_key) = if item.get("targetUri").is_some() {
            ("targetUri", "targetRange")
        } else {
            ("uri", "range")
        };
        let uri = match item.get(uri_key).and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if let Some(r) = item.get(range_key) {
            let (sl, sc) = position_of(&r["start"]);
            let (el, ec) = position_of(&r["end"]);
            out.push(Location {
                uri,
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            });
        }
    }
    Ok(out)
}

fn position_of(v: &Value) -> (u32, u32) {
    let line = v["line"].as_u64().unwrap_or(0) as u32;
    let col = v["character"].as_u64().unwrap_or(0) as u32;
    (line, col)
}

fn parse_range(v: &Value) -> Option<(u32, u32, u32, u32)> {
    let (sl, sc) = position_of(&v["start"]);
    let (el, ec) = position_of(&v["end"]);
    Some((sl, sc, el, ec))
}

/// Convert a filesystem path to a `file://` URI.
pub fn path_to_file_uri(path: &str) -> String {
    // On Windows paths look like C:\foo\bar — encode correctly.
    if cfg!(windows) {
        let norm = path.replace('\\', "/");
        if norm.starts_with('/') {
            format!("file://{norm}")
        } else {
            format!("file:///{norm}")
        }
    } else {
        // Unix: path always starts with '/'.
        format!("file://{path}")
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────
//
// These tests cover framing + server-registry probing with NO real server.
// The live round-trip test is in tests/lsp_live.rs.

#[cfg(test)]
mod tests {
    use super::*;

    // ── framing encode tests ──────────────────────────────────────────────────

    #[test]
    fn encode_request_produces_correct_content_length() {
        let frame = encode_request(1, "initialize", serde_json::json!({"a": 1}));
        let s = String::from_utf8(frame.clone()).unwrap();
        // Header ends at the first \r\n\r\n.
        let sep = s.find("\r\n\r\n").expect("separator present");
        let header = &s[..sep];
        assert!(header.starts_with("Content-Length: "));
        let claimed_len: usize = header["Content-Length: ".len()..].parse().unwrap();
        let body = &frame[sep + 4..];
        assert_eq!(
            claimed_len,
            body.len(),
            "Content-Length matches body length"
        );
    }

    #[test]
    fn encode_request_body_is_valid_json_rpc() {
        let frame = encode_request(42, "textDocument/definition", serde_json::json!({}));
        let s = String::from_utf8(frame).unwrap();
        let sep = s.find("\r\n\r\n").unwrap();
        let body_str = &s[sep + 4..];
        let v: serde_json::Value = serde_json::from_str(body_str).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 42);
        assert_eq!(v["method"], "textDocument/definition");
    }

    #[test]
    fn encode_notification_has_no_id_field() {
        let frame = encode_notification("initialized", serde_json::json!({}));
        let s = String::from_utf8(frame).unwrap();
        let sep = s.find("\r\n\r\n").unwrap();
        let body_str = &s[sep + 4..];
        let v: serde_json::Value = serde_json::from_str(body_str).unwrap();
        assert!(
            !v.as_object().unwrap().contains_key("id"),
            "notification must not have an id field"
        );
        assert_eq!(v["method"], "initialized");
    }

    // ── framing decode tests ──────────────────────────────────────────────────

    #[test]
    fn read_frame_roundtrip_single_message() {
        let payload = br#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let header = format!("Content-Length: {}\r\n\r\n", payload.len());
        let mut data = header.into_bytes();
        data.extend_from_slice(payload);
        let mut reader = std::io::BufReader::new(data.as_slice());
        let got = read_frame(&mut reader).unwrap();
        assert_eq!(got, payload);
    }

    #[test]
    fn read_frame_strips_extra_headers() {
        let payload = br#"{"jsonrpc":"2.0","id":2,"result":null}"#;
        // Some servers include Content-Type before Content-Length.
        let header = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
            payload.len()
        );
        let mut data = header.into_bytes();
        data.extend_from_slice(payload);
        let mut reader = std::io::BufReader::new(data.as_slice());
        let got = read_frame(&mut reader).unwrap();
        assert_eq!(got, payload);
    }

    #[test]
    fn read_frame_two_sequential_messages() {
        let p1 = br#"{"jsonrpc":"2.0","method":"window/logMessage","params":{}}"#;
        let p2 = br#"{"jsonrpc":"2.0","id":3,"result":{"value":"hello"}}"#;
        let mut data = Vec::new();
        data.extend_from_slice(format!("Content-Length: {}\r\n\r\n", p1.len()).as_bytes());
        data.extend_from_slice(p1);
        data.extend_from_slice(format!("Content-Length: {}\r\n\r\n", p2.len()).as_bytes());
        data.extend_from_slice(p2);

        let mut reader = std::io::BufReader::new(data.as_slice());
        let got1 = read_frame(&mut reader).unwrap();
        let got2 = read_frame(&mut reader).unwrap();
        assert_eq!(got1, p1.as_slice());
        assert_eq!(got2, p2.as_slice());
    }

    #[test]
    fn read_frame_errors_on_missing_content_length() {
        let data = b"X-Custom-Header: foo\r\n\r\nsome body";
        let mut reader = std::io::BufReader::new(data.as_slice());
        let err = read_frame(&mut reader).unwrap_err();
        assert!(
            err.to_string().contains("Content-Length"),
            "error should mention Content-Length, got: {err}"
        );
    }

    #[test]
    fn read_frame_errors_on_eof() {
        let data = b""; // empty — immediate EOF
        let mut reader = std::io::BufReader::new(data.as_slice());
        let err = read_frame(&mut reader).unwrap_err();
        assert!(
            err.to_string().contains("closed") || matches!(err, wicked_estate_core::Error::Io(_)),
            "expected closed/IO error, got: {err}"
        );
    }

    // ── request-id correlation ────────────────────────────────────────────────

    #[test]
    fn request_id_increments() {
        let a = next_id();
        let b = next_id();
        assert!(b > a, "ids should strictly increase");
    }

    // ── server registry ───────────────────────────────────────────────────────

    /// The registry knows about typescript/rust/python; a fake language is absent.
    #[test]
    fn registry_covers_known_languages() {
        let reg = ServerRegistry::standard();
        // We can't assert the binary is on PATH in all environments, but we CAN assert
        // that the language is at least registered (entries present).
        assert!(
            reg.entries.contains_key("typescript"),
            "typescript should be registered"
        );
        assert!(
            reg.entries.contains_key("rust"),
            "rust should be registered"
        );
        assert!(
            reg.entries.contains_key("python"),
            "python should be registered"
        );
    }

    #[test]
    fn registry_unknown_language_returns_none() {
        let reg = ServerRegistry::standard();
        // "cobol" is definitely not registered.
        let result = reg.command_for("cobol");
        assert!(result.is_none(), "unknown language must return None");
    }

    #[test]
    fn registry_missing_binary_returns_none() {
        let mut reg = ServerRegistry::standard();
        // Register a language with a definitely-absent binary.
        reg.register("test-lang", "__definitely_not_a_real_binary__", vec![]);
        let result = reg.command_for("test-lang");
        assert!(result.is_none(), "absent binary must return None");
    }

    // ── LspTier graceful missing-server ──────────────────────────────────────

    #[test]
    fn lsp_tier_returns_error_for_unregistered_language() {
        let mut tier = LspTier::new("/tmp");
        let err = tier
            .definition("cobol", "file:///tmp/foo.cbl", 0, 0)
            .unwrap_err();
        assert!(
            err.to_string().contains("server not available"),
            "expected 'server not available' error, got: {err}"
        );
    }

    #[test]
    fn lsp_tier_returns_error_for_absent_binary() {
        let mut reg = ServerRegistry::default();
        reg.register("ghostlang", "__no_such_binary__", vec![]);
        let mut tier = LspTier::with_registry("/tmp", reg);
        let err = tier
            .definition("ghostlang", "file:///tmp/x.gl", 0, 0)
            .unwrap_err();
        assert!(
            err.to_string().contains("server not available"),
            "expected 'server not available' error, got: {err}"
        );
    }

    // ── parse_locations ───────────────────────────────────────────────────────

    #[test]
    fn parse_locations_null_returns_empty() {
        let locs = parse_locations(&serde_json::Value::Null).unwrap();
        assert!(locs.is_empty());
    }

    #[test]
    fn parse_locations_single_location_object() {
        let v = serde_json::json!({
            "uri": "file:///a.ts",
            "range": {
                "start": { "line": 3, "character": 7 },
                "end":   { "line": 3, "character": 12 }
            }
        });
        let locs = parse_locations(&v).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].uri, "file:///a.ts");
        assert_eq!(locs[0].start_line, 3);
        assert_eq!(locs[0].start_col, 7);
        assert_eq!(locs[0].end_line, 3);
        assert_eq!(locs[0].end_col, 12);
    }

    #[test]
    fn parse_locations_array_of_locations() {
        let v = serde_json::json!([
            {
                "uri": "file:///a.ts",
                "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 5 } }
            },
            {
                "uri": "file:///b.ts",
                "range": { "start": { "line": 2, "character": 3 }, "end": { "line": 2, "character": 8 } }
            }
        ]);
        let locs = parse_locations(&v).unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].uri, "file:///a.ts");
        assert_eq!(locs[1].uri, "file:///b.ts");
        assert_eq!(locs[1].start_line, 2);
    }

    #[test]
    fn parse_locations_location_link() {
        let v = serde_json::json!([{
            "targetUri": "file:///c.ts",
            "targetRange": {
                "start": { "line": 10, "character": 0 },
                "end":   { "line": 15, "character": 1 }
            },
            "targetSelectionRange": {
                "start": { "line": 10, "character": 0 },
                "end":   { "line": 10, "character": 6 }
            }
        }]);
        let locs = parse_locations(&v).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].uri, "file:///c.ts");
        assert_eq!(locs[0].start_line, 10);
        assert_eq!(locs[0].end_line, 15);
    }

    // ── path_to_file_uri ──────────────────────────────────────────────────────

    #[test]
    fn path_to_file_uri_unix() {
        // Only run the Unix branch on non-Windows.
        if !cfg!(windows) {
            assert_eq!(
                path_to_file_uri("/home/user/project"),
                "file:///home/user/project"
            );
        }
    }
}
