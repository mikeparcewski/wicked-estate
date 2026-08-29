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
//! A dedicated reader thread (the *frame pump*) owns the server's stdout, parses complete
//! Content-Length frames, and hands each parsed message over an `mpsc` channel. Every request
//! computes a **deadline** (`now + budget`, default 10s, injectable via
//! [`LspClient::spawn_with_timeout`] / [`LspTier::with_timeout`]) once, and each channel receive
//! waits only for the *remaining* time — so a chatty server that streams notifications but never
//! answers still errs at the deadline. The mechanism is pure std and identical on Unix and
//! Windows. On a timeout the child is killed and (in [`LspTier`]) the client is evicted, because
//! a transport that timed out can no longer be trusted. Known limit: writes (`stdin.write_all`)
//! are not yet bounded — a server that stops *reading* can still stall a very large payload
//! (W3.6 implementation concern, see ADR-009).

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicI64, Ordering},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, Instant},
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

/// Encode a JSON-RPC **response** (to a server→client request) with LSP framing.
/// `id` echoes the server's request id verbatim.
pub(crate) fn encode_response(id: &Value, result: Value) -> Vec<u8> {
    #[derive(Serialize)]
    struct Resp<'a> {
        jsonrpc: &'a str,
        id: &'a Value,
        result: Value,
    }
    let body = serde_json::to_vec(&Resp {
        jsonrpc: "2.0",
        id,
        result,
    })
    .expect("response serialization is infallible");
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

/// Default per-request budget. Injectable via [`LspClient::spawn_with_timeout`] and
/// [`LspTier::with_timeout`].
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Spawn the frame-pump thread: it owns the read side, parses complete Content-Length frames,
/// and sends each parsed message over the returned channel. Frames are parsed *in the thread*,
/// so a caller-side timeout never leaves a half-read frame in a caller-visible buffer.
///
/// The thread exits when the stream ends/errs (the terminal `Err` is forwarded once) or when
/// the receiving transport is dropped (the `send` fails).
fn spawn_frame_pump<R: Read + Send + 'static>(reader: R) -> Receiver<Result<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lsp-frame-pump".into())
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            loop {
                match read_frame(&mut reader) {
                    Ok(frame) => {
                        if tx.send(Ok(frame)).is_err() {
                            return; // transport dropped — stop pumping
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return; // EOF or framing error — the stream is done either way
                    }
                }
            }
        })
        .expect("spawning the LSP frame-pump thread must not fail");
    rx
}

/// Low-level framing transport over a server's stdio pipes.
///
/// Reading goes through the frame pump (see [`spawn_frame_pump`]); every receive is bounded by
/// a per-request deadline computed once in [`RpcTransport::await_response`].
struct RpcTransport {
    writer: Box<dyn Write + Send>,
    frames: Receiver<Result<Vec<u8>>>,
    timeout: Duration,
}

impl RpcTransport {
    fn new<W, R>(writer: W, reader: R, timeout: Duration) -> Self
    where
        W: Write + Send + 'static,
        R: Read + Send + 'static,
    {
        RpcTransport {
            writer: Box::new(writer),
            frames: spawn_frame_pump(reader),
            timeout,
        }
    }

    /// Send a framed request and return the raw JSON body of the response (skips notifications).
    fn send_and_receive(&mut self, id: i64, method: &str, params: Value) -> Result<Value> {
        let frame = encode_request(id, method, params);
        self.writer.write_all(&frame)?;
        self.writer.flush()?;
        self.await_response(id)
    }

    /// Send a notification (fire-and-forget — no response expected).
    fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        let frame = encode_notification(method, params);
        self.writer.write_all(&frame)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Receive the next parsed frame, waiting no longer than the remaining time to `deadline`.
    fn recv_frame_by(&self, deadline: Instant) -> Result<Vec<u8>> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.frames.recv_timeout(remaining) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => Err(Error::Resolution(format!(
                "LSP: request timed out after {:?}",
                self.timeout
            ))),
            Err(RecvTimeoutError::Disconnected) => Err(Error::Resolution(
                "LSP: reader thread terminated (server stdout closed)".into(),
            )),
        }
    }

    /// Read frames, skipping notifications, until a response for `expected_id` arrives — or the
    /// per-request deadline passes. The deadline is computed ONCE per request: notification
    /// frames arriving in between never restart the clock.
    fn await_response(&mut self, expected_id: i64) -> Result<Value> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let raw = self.recv_frame_by(deadline)?;
            let v: Value = serde_json::from_slice(&raw)?;

            // Any message carrying `method` is never our response: either a server-pushed
            // notification (no id — skip) or a server→client REQUEST (id + method — reply,
            // so spec-compliant servers don't stall waiting on us). Without this guard a
            // server request whose id collides with ours deserializes as our response with
            // `result` defaulting to Null — a silent false "no definition".
            if let Some(method) = v.get("method").and_then(Value::as_str) {
                if let Some(req_id) = v.get("id") {
                    let req_id = req_id.clone();
                    let method = method.to_string();
                    self.reply_to_server_request(&req_id, &method, v.get("params"))?;
                }
                continue;
            }

            // No `method`, no `id`: malformed — skip tolerantly.
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

    /// Reply to a server→client request: `null` for everything except
    /// `workspace/configuration`, which per LSP 3.17 must receive an **array with one
    /// element per requested item** — a bare `null` there is spec-invalid and can stall
    /// pyright right after `didOpen`.
    fn reply_to_server_request(
        &mut self,
        id: &Value,
        method: &str,
        params: Option<&Value>,
    ) -> Result<()> {
        let result = if method == "workspace/configuration" {
            let n = params
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(1);
            Value::Array(vec![Value::Null; n])
        } else {
            Value::Null
        };
        let frame = encode_response(id, result);
        self.writer.write_all(&frame)?;
        self.writer.flush()?;
        Ok(())
    }
}

/// True when `err` means the transport itself can no longer be trusted (timeout, closed or
/// desynced stream, I/O failure) — as opposed to a server-delivered error response
/// (`LSP error <code>: …`) or a local precondition failure (`server not available`, unreadable
/// file), after which the connection remains healthy. All matched strings are minted in this
/// module.
fn is_transport_fatal(err: &Error) -> bool {
    match err {
        Error::Io(_) | Error::Json(_) => true,
        _ => {
            let msg = err.to_string();
            msg.contains("LSP: request timed out")
                || msg.contains("LSP: reader thread terminated")
                || msg.contains("LSP: server closed stdout")
                || msg.contains("LSP: framing error")
        }
    }
}

// ── server registry ───────────────────────────────────────────────────────────

/// Grammar-name → LSP `languageId` for the rows where they differ (identity otherwise).
/// DATA next to the registry (rules-as-data); moves wholesale into the W3.6 registry data
/// file (ADR-009). LSP requires e.g. `typescriptreact` for `.tsx` documents — announcing
/// `typescript` makes tsserver mis-parse JSX.
const LANGUAGE_ID_OVERRIDES: &[(&str, &str)] =
    &[("tsx", "typescriptreact"), ("jsx", "javascriptreact")];

/// The LSP-standard `languageId` for a tree-sitter grammar name.
pub fn lsp_language_id(grammar: &str) -> &str {
    LANGUAGE_ID_OVERRIDES
        .iter()
        .find(|(g, _)| *g == grammar)
        .map(|(_, l)| *l)
        .unwrap_or(grammar)
}

/// One registry row: how to launch a language's server and which LSP `languageId` to
/// announce in `textDocument/didOpen` for its documents.
#[derive(Debug, Clone)]
pub struct ServerEntry {
    pub binary: String,
    pub args: Vec<String>,
    /// LSP-standard languageId (differs from the grammar name for tsx/jsx).
    pub language_id: String,
}

/// Maps tree-sitter grammar language names → server invocation command.
///
/// Lookup via [`ServerRegistry::command_for`]; returns `None` when the binary is absent
/// from `PATH`.
#[derive(Debug, Clone)]
pub struct ServerRegistry {
    /// language → registry row.
    entries: HashMap<String, ServerEntry>,
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
        for lang in ["typescript", "tsx", "javascript", "jsx"] {
            entries.insert(
                lang.to_string(),
                ServerEntry {
                    binary: "typescript-language-server".to_string(),
                    args: vec!["--stdio".to_string()],
                    language_id: lsp_language_id(lang).to_string(),
                },
            );
        }
        entries.insert(
            "rust".to_string(),
            ServerEntry {
                binary: "rust-analyzer".to_string(),
                args: vec![],
                language_id: "rust".to_string(),
            },
        );
        entries.insert(
            "python".to_string(),
            ServerEntry {
                binary: "pyright-langserver".to_string(),
                args: vec!["--stdio".to_string()],
                language_id: "python".to_string(),
            },
        );
        ServerRegistry { entries }
    }

    /// Look up the binary + args for `language`. Returns `None` when the language is not
    /// registered or the binary is not on PATH.
    pub fn command_for(&self, language: &str) -> Option<(String, Vec<String>)> {
        let entry = self.entries.get(language)?;
        if probe_binary(&entry.binary) {
            Some((entry.binary.clone(), entry.args.clone()))
        } else {
            None
        }
    }

    /// The LSP `languageId` to announce for `language`'s documents, if registered.
    pub fn language_id_for(&self, language: &str) -> Option<&str> {
        self.entries.get(language).map(|e| e.language_id.as_str())
    }

    /// Register a custom language server. Replaces an existing entry. The `languageId`
    /// is derived from the grammar name via [`lsp_language_id`].
    pub fn register(&mut self, language: &str, binary: &str, args: Vec<String>) {
        self.entries.insert(
            language.to_string(),
            ServerEntry {
                binary: binary.to_string(),
                args,
                language_id: lsp_language_id(language).to_string(),
            },
        );
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
    child: Child,
    transport: RpcTransport,
    /// The root URI the server was initialized against.
    root_uri: String,
    /// Set once the child has been killed — `Drop` then skips the graceful shutdown.
    dead: bool,
    /// didOpen cache: file URI → content digest. Owned by the client (not the tier) so
    /// eviction drops it with the client — a respawned server re-opens documents naturally
    /// instead of inheriting stale "already open" claims.
    open_docs: HashMap<String, u64>,
}

impl LspClient {
    /// Spawn the server, perform `initialize` + `initialized` handshake, and return a ready
    /// client, with the default per-request budget ([`DEFAULT_TIMEOUT`]).
    ///
    /// `root_dir` is the absolute path to the workspace root (passed as `rootUri`).
    pub fn spawn(binary: &str, args: &[String], root_dir: &str) -> Result<Self> {
        Self::spawn_with_timeout(binary, args, root_dir, DEFAULT_TIMEOUT)
    }

    /// [`LspClient::spawn`] with an explicit per-request budget. The budget bounds every
    /// response wait, including the `initialize` handshake — a wedged server is killed and
    /// reaped before this returns `Err`.
    pub fn spawn_with_timeout(
        binary: &str,
        args: &[String],
        root_dir: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let mut child = Command::new(binary)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // suppress server stderr chatter
            .spawn()
            .map_err(|e| Error::Resolution(format!("LSP: failed to spawn '{binary}': {e}")))?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let transport = RpcTransport::new(stdin, stdout, timeout);

        let root_uri = path_to_file_uri(root_dir);
        let mut client = LspClient {
            child,
            transport,
            root_uri,
            dead: false,
            open_docs: HashMap::new(),
        };

        if let Err(e) = client.handshake() {
            // Wedged or broken server — no graceful shutdown, just kill + reap.
            client.kill();
            return Err(e);
        }
        Ok(client)
    }

    /// `initialize` request + `initialized` notification.
    fn handshake(&mut self) -> Result<()> {
        let id = next_id();
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
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
        self.transport
            .send_and_receive(id, "initialize", init_params)?;
        // Fire the `initialized` notification (no response).
        self.transport
            .send_notification("initialized", serde_json::json!({}))
    }

    /// Kill the server process immediately (no graceful shutdown) and reap it.
    /// Used when the transport can no longer be trusted (timeout, desync).
    fn kill(&mut self) {
        self.dead = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
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

    /// Ensure the server has the **current** content of `file_uri` open: read the file,
    /// digest it, and send `textDocument/didOpen` when unseen — or `didClose` + `didOpen`
    /// when the content changed since it was last opened (minimal correct sync; no
    /// `didChange` incremental protocol). LSP servers return empty results for unopened
    /// documents, so every position query must go through here.
    pub fn ensure_open(&mut self, file_uri: &str, language_id: &str) -> Result<()> {
        let path = file_uri_to_path(file_uri)?;
        let text = std::fs::read_to_string(&path).map_err(|e| {
            Error::Resolution(format!("LSP: cannot read file for didOpen: {path}: {e}"))
        })?;
        let digest = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut h);
            h.finish()
        };
        match self.open_docs.get(file_uri) {
            Some(&d) if d == digest => return Ok(()), // already open with this content
            Some(_) => {
                // Content changed on disk since didOpen: close, then reopen below.
                self.transport.send_notification(
                    "textDocument/didClose",
                    serde_json::json!({ "textDocument": { "uri": file_uri } }),
                )?;
            }
            None => {}
        }
        self.did_open(file_uri, language_id, &text)?;
        self.open_docs.insert(file_uri.to_string(), digest);
        Ok(())
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
        if !self.dead {
            // Best-effort graceful shutdown, bounded by the per-request budget (the frame pump
            // makes the response wait finite). Errors ignored — the child may already be gone.
            let id = next_id();
            let _ = self.transport.send_and_receive(id, "shutdown", Value::Null);
            let _ = self.transport.send_notification("exit", Value::Null);
        }
        // Reap unconditionally so no child outlives its client (kill on an already-exited
        // process is a harmless error; wait returns the cached status).
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ── LSP tier (public API) ─────────────────────────────────────────────────────

/// On-demand LSP tier (W3.3).
///
/// Wraps a [`ServerRegistry`] and a cache of per-language [`LspClient`]s. Exposes
/// `definition` / `references` / `hover` methods that map a `(language, file, line, col)`
/// tuple to precise LSP results.
///
/// **Status (2026-08-28, ADR-007 superseding note):** a *client library* by design — no
/// `Resolver` impl, no `Edge` emission, never in a bulk resolver slice, per the locked decision
/// "LSP is on-demand only, never bulk". The W3.3 AC is met by `tests/lsp_live.rs`
/// (probe-and-skip against installed servers). The on-demand consumer (an MCP/CLI single-symbol
/// definition/references tool, `resolve.lsp` span) is the W3.6 follow-up in the wave plan.
///
/// Usage pattern (`no_run`: spawns a real language server):
/// ```no_run
/// # fn main() -> wicked_estate_core::Result<()> {
/// use wicked_estate_resolve::lsp::LspTier;
///
/// let mut tier = LspTier::new("/path/to/project");
/// let defs = tier.definition("typescript", "file:///path/to/foo.ts", 10, 5)?;
/// # let _ = defs;
/// # Ok(())
/// # }
/// ```
pub struct LspTier {
    registry: ServerRegistry,
    /// language → live client. Clients are spawned lazily on first use.
    clients: HashMap<String, LspClient>,
    root_dir: String,
    /// Per-request budget for clients spawned by this tier.
    timeout: Duration,
}

impl LspTier {
    /// Create a new tier for the given workspace root directory.
    pub fn new(root_dir: impl Into<String>) -> Self {
        LspTier {
            registry: ServerRegistry::standard(),
            clients: HashMap::new(),
            root_dir: root_dir.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create with a custom registry (for testing or non-standard servers).
    pub fn with_registry(root_dir: impl Into<String>, registry: ServerRegistry) -> Self {
        LspTier {
            registry,
            clients: HashMap::new(),
            root_dir: root_dir.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Builder-style override of the per-request budget. Applies to clients spawned after the
    /// call (existing live clients keep the budget they were spawned with).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get-or-spawn the client for `language`. Returns an error if the server is unavailable.
    fn client(&mut self, language: &str) -> Result<&mut LspClient> {
        // Borrow-checker: check existence first, then insert only if absent.
        if !self.clients.contains_key(language) {
            let (bin, args) =
                self.registry.command_for(language).ok_or_else(|| {
                    Error::Resolution(format!("server not available: no LSP server registered for language '{language}' (or binary not on PATH)"))
                })?;
            let client = LspClient::spawn_with_timeout(&bin, &args, &self.root_dir, self.timeout)?;
            self.clients.insert(language.to_string(), client);
        }
        Ok(self.clients.get_mut(language).expect("just inserted"))
    }

    /// If `res` is a transport-fatal error (timeout, closed/desynced stream, I/O failure),
    /// kill and evict the language's client so the next query respawns a fresh server —
    /// a client that timed out would otherwise poison every later query on its language.
    /// Server-delivered error responses and local precondition failures keep the client.
    fn evict_if_fatal<T>(&mut self, language: &str, res: Result<T>) -> Result<T> {
        if let Err(e) = &res {
            if is_transport_fatal(e) {
                if let Some(mut client) = self.clients.remove(language) {
                    client.kill();
                }
            }
        }
        res
    }

    /// Test-only visibility into the client cache (eviction assertions).
    #[cfg(test)]
    fn has_client(&self, language: &str) -> bool {
        self.clients.contains_key(language)
    }

    /// Get-or-spawn the client for `language` and make sure `file_uri`'s current content is
    /// open on it with the registry's `languageId` — LSP servers return empty results for
    /// unopened documents (the didOpen defect this fixes).
    fn prepared_client(&mut self, language: &str, file_uri: &str) -> Result<&mut LspClient> {
        let language_id = self
            .registry
            .language_id_for(language)
            .unwrap_or(language)
            .to_string();
        let client = self.client(language)?;
        client.ensure_open(file_uri, &language_id)?;
        Ok(client)
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
        let res = self
            .prepared_client(language, file_uri)
            .and_then(|c| c.definition(file_uri, line, col));
        self.evict_if_fatal(language, res)
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
        let res = self
            .prepared_client(language, file_uri)
            .and_then(|c| c.references(file_uri, line, col, include_declaration));
        self.evict_if_fatal(language, res)
    }

    /// Return the hover/type info for the symbol at `(line, col)` in `file_uri`.
    pub fn hover(
        &mut self,
        language: &str,
        file_uri: &str,
        line: u32,
        col: u32,
    ) -> Result<Option<HoverResult>> {
        let res = self
            .prepared_client(language, file_uri)
            .and_then(|c| c.hover(file_uri, line, col));
        self.evict_if_fatal(language, res)
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

/// Convert a `file://` URI back to a filesystem path — the inverse of [`path_to_file_uri`].
///
/// Percent-decodes `%XX` escapes (servers return e.g. `%20` for spaces) and handles the
/// Windows drive-letter form (`file:///C:/dir` → `C:\dir`). The drive form is detected by
/// shape, not by `cfg`, so the helper is testable on every platform. A naive
/// `strip_prefix("file://")` would silently break every query on Windows and on any path
/// containing spaces.
pub fn file_uri_to_path(uri: &str) -> Result<String> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| Error::Resolution(format!("LSP: not a file:// URI: {uri}")))?;
    let decoded = percent_decode(rest);
    // Windows drive form: "/C:" or "/C:/dir" → "C:\dir".
    let bytes = decoded.as_bytes();
    let is_drive_form = bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && (bytes.len() == 3 || bytes[3] == b'/');
    if is_drive_form {
        Ok(decoded[1..].replace('/', "\\"))
    } else {
        Ok(decoded)
    }
}

/// Percent-decode `%XX` hex escapes (std-only). Invalid escapes pass through verbatim.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
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

    // ── languageId mapping (pure data) ────────────────────────────────────────

    #[test]
    fn language_id_mapping_data() {
        assert_eq!(lsp_language_id("tsx"), "typescriptreact");
        assert_eq!(lsp_language_id("jsx"), "javascriptreact");
        assert_eq!(lsp_language_id("typescript"), "typescript");
        assert_eq!(lsp_language_id("rust"), "rust");
        // Registry rows carry the mapping.
        let reg = ServerRegistry::standard();
        assert_eq!(reg.language_id_for("tsx"), Some("typescriptreact"));
        assert_eq!(reg.language_id_for("jsx"), Some("javascriptreact"));
        assert_eq!(reg.language_id_for("python"), Some("python"));
        assert_eq!(reg.language_id_for("cobol"), None);
    }

    // ── file_uri_to_path (inverse of path_to_file_uri) ────────────────────────

    #[test]
    fn file_uri_to_path_unix_percent_decoding() {
        assert_eq!(
            file_uri_to_path("file:///home/user/my%20project/a.ts").unwrap(),
            "/home/user/my project/a.ts"
        );
    }

    #[test]
    fn file_uri_to_path_windows_drive_uri() {
        // Shape-detected, so this runs (and must pass) on every platform.
        assert_eq!(
            file_uri_to_path("file:///C:/Users/dev/proj/a.ts").unwrap(),
            "C:\\Users\\dev\\proj\\a.ts"
        );
        assert_eq!(
            file_uri_to_path("file:///c:/dir%20x/y.py").unwrap(),
            "c:\\dir x\\y.py"
        );
    }

    #[test]
    fn file_uri_to_path_rejects_non_file_uri() {
        let err = file_uri_to_path("https://example.com/a.ts").unwrap_err();
        assert!(err.to_string().contains("not a file:// URI"), "got: {err}");
    }

    #[test]
    fn file_uri_round_trips_with_path_to_file_uri() {
        // path_to_file_uri is cfg-gated, so exercise the current platform's branch.
        let path = if cfg!(windows) {
            "C:\\Users\\dev\\round trip"
        } else {
            "/tmp/round trip/dir"
        };
        assert_eq!(file_uri_to_path(&path_to_file_uri(path)).unwrap(), path);
    }

    // ── await_response: server→client requests are never our response (D8) ───

    /// A `Write` that captures everything for later assertions.
    #[derive(Clone)]
    struct SharedWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Deterministic misparse test (scripted frames, no live server): a notification, then a
    /// server→client request whose `id` COLLIDES with the expected response id, then the real
    /// response. The old code deserialized the server request as our response with `result`
    /// defaulting to `Null` — a silent false "no definition". Also asserts the replies:
    /// `workspace/configuration` gets a null-array sized to `params.items.len()` (LSP 3.17 —
    /// a bare null there can stall pyright); other server requests get a bare null.
    #[test]
    fn await_response_skips_server_requests_and_replies_correctly() {
        let expected_id = 42;
        let mut script = Vec::new();
        script.extend(encode_notification(
            "window/logMessage",
            serde_json::json!({"type": 3, "message": "preamble"}),
        ));
        script.extend(encode_request(
            expected_id, // deliberate id collision with our request
            "workspace/configuration",
            serde_json::json!({"items": [{"section": "python"}, {"section": "js"}]}),
        ));
        script.extend(encode_request(
            expected_id,
            "window/workDoneProgress/create",
            serde_json::json!({"token": "t1"}),
        ));
        script.extend(encode_response(
            &serde_json::json!(expected_id),
            serde_json::json!({"ok": true}),
        ));

        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut t = RpcTransport::new(
            SharedWriter(written.clone()),
            std::io::Cursor::new(script),
            Duration::from_secs(5),
        );

        let result = t.await_response(expected_id).unwrap();
        assert_eq!(
            result,
            serde_json::json!({"ok": true}),
            "the REAL response must be returned, not the colliding server request's default-Null"
        );

        let bytes = written.lock().unwrap().clone();
        let out = String::from_utf8(bytes).unwrap();
        assert!(
            out.contains(r#""result":[null,null]"#),
            "workspace/configuration must get one null per requested item, got: {out}"
        );
        assert!(
            out.contains(r#""result":null"#),
            "other server requests must get a bare null reply, got: {out}"
        );
    }

    // ── timeout mechanism (frame pump + per-request deadline) ────────────────
    //
    // Pure-std readers, no cfg — these run identically on Unix and Windows, so the
    // cross-platform claim is tested, not asserted. The leaked pump threads exit with
    // the test process.

    /// A `Read` that never produces a byte (simulates a wedged server stdout).
    struct BlockForever;

    impl Read for BlockForever {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(600));
            Ok(0)
        }
    }

    #[test]
    fn transport_times_out_on_a_reader_that_never_produces_a_frame() {
        let budget = Duration::from_millis(300);
        let mut t = RpcTransport::new(std::io::sink(), BlockForever, budget);
        let start = Instant::now();
        let err = t.await_response(1).unwrap_err();
        let elapsed = start.elapsed();
        assert!(
            err.to_string().contains("timed out"),
            "expected a timeout error, got: {err}"
        );
        assert!(
            elapsed >= budget,
            "returned before the deadline: {elapsed:?}"
        );
        assert!(
            elapsed < budget + Duration::from_secs(5),
            "timeout not honored — took {elapsed:?} for a {budget:?} budget"
        );
    }

    /// A `Read` that yields a well-formed **notification** frame every `delay`, forever —
    /// a chatty-but-never-answering server (rust-analyzer cold indexing, tsserver telemetry).
    struct ChattyReader {
        frame: Vec<u8>,
        pos: usize,
        delay: Duration,
    }

    impl Read for ChattyReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos == 0 {
                std::thread::sleep(self.delay);
            }
            let n = buf.len().min(self.frame.len() - self.pos);
            buf[..n].copy_from_slice(&self.frame[self.pos..self.pos + n]);
            self.pos = (self.pos + n) % self.frame.len();
            Ok(n)
        }
    }

    /// Deadline semantics, not per-message budget: notification frames arriving faster than
    /// the budget must NOT restart the clock. Under per-message `recv_timeout(budget)`
    /// semantics this test would never return (each notification arrives well within the
    /// budget and the await loop re-reads) — reaching the assertions at all proves the
    /// deadline is computed once per request.
    #[test]
    fn notification_chatter_does_not_restart_the_request_clock() {
        let budget = Duration::from_millis(400);
        let chatty = ChattyReader {
            frame: encode_notification(
                "window/logMessage",
                serde_json::json!({"type": 3, "message": "still indexing…"}),
            ),
            pos: 0,
            delay: budget / 4,
        };
        let mut t = RpcTransport::new(std::io::sink(), chatty, budget);
        let start = Instant::now();
        let err = t.await_response(7).unwrap_err();
        let elapsed = start.elapsed();
        assert!(
            err.to_string().contains("timed out"),
            "expected a timeout error, got: {err}"
        );
        assert!(
            elapsed >= budget,
            "errored before the deadline: {elapsed:?}"
        );
        assert!(
            elapsed < budget + Duration::from_secs(5),
            "deadline not honored under notification chatter — took {elapsed:?}"
        );
    }

    // ── eviction on timeout (D3) ──────────────────────────────────────────────

    /// A fake language server that answers `initialize` correctly, ignores notifications,
    /// then wedges (never replies) on the first real request. This is the only shape that
    /// reaches `LspTier`'s eviction path: a `sleep` masquerade times out inside
    /// `LspClient::spawn`'s handshake, *before* the client is ever inserted into the map.
    const WEDGE_SERVER_PY: &str = r#"
import json, sys, time

def read_msg():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        stripped = line.strip()
        if not stripped:
            break
        if stripped.lower().startswith(b"content-length:"):
            length = int(stripped.split(b":", 1)[1])
    return json.loads(sys.stdin.buffer.read(length))

def send(obj):
    body = json.dumps(obj).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if "id" in msg and msg.get("method") == "initialize":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {}}})
    elif "id" in msg:
        time.sleep(600)  # wedge: never reply to any further request
    # notifications: ignored
"#;

    /// Probe-and-skip helper: first python interpreter on PATH (macOS/Linux `python3`,
    /// Windows Git Bash/native `python`).
    fn python_on_path() -> Option<&'static str> {
        ["python3", "python"].into_iter().find(|b| probe_binary(b))
    }

    #[test]
    fn tier_evicts_and_kills_the_client_when_a_request_times_out() {
        let Some(interp) = python_on_path() else {
            println!("[lsp] SKIP: no python interpreter on PATH; eviction path not exercised");
            return;
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let script = tmp.path().join("wedge_server.py");
        std::fs::write(&script, WEDGE_SERVER_PY).unwrap();
        let src = tmp.path().join("main.wl");
        std::fs::write(&src, "hello wedge\n").unwrap();

        let mut reg = ServerRegistry::standard();
        reg.register(
            "wedgelang",
            interp,
            vec![script.to_str().unwrap().to_string()],
        );

        let budget = Duration::from_millis(700);
        let mut tier =
            LspTier::with_registry(tmp.path().to_str().unwrap(), reg).with_timeout(budget);
        let uri = path_to_file_uri(src.to_str().unwrap());

        let start = Instant::now();
        let err = tier.definition("wedgelang", &uri, 0, 0).unwrap_err();
        let elapsed = start.elapsed();

        assert!(
            err.to_string().contains("timed out"),
            "expected a timeout error (the fake server answered initialize, then wedged), got: {err}"
        );
        assert!(
            elapsed < budget + Duration::from_secs(8),
            "timeout not honored — took {elapsed:?} for a {budget:?} budget"
        );
        // The wedged client MUST be gone: one timeout must not poison every later query.
        assert!(
            !tier.has_client("wedgelang"),
            "client must be evicted from the tier after a request timeout"
        );
        // The child must be killed: pgrep by the (unique) tempdir script path, where available.
        if probe_binary("pgrep") {
            let out = Command::new("pgrep")
                .args(["-f", script.to_str().unwrap()])
                .output()
                .expect("pgrep runs");
            assert!(
                !out.status.success(),
                "wedged server child survived eviction: pids {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}
