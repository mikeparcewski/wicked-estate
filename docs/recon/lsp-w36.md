# Plan — W3.6 intent-routed LSP: ADR-009 + phase-0 lsp.rs fixes

Lane `lsp-w36`, branch `design/w36-lsp-intent`, base 764622f (main). Planner output per §10.
Inputs: `estate-review/REVIEW-adversarial-2026-08-28.md` (engine defect #5, D02-5, D03-10, PER-8),
`review-artifacts/findings.json`, `estate-review/RECON-lsp-intent-installer.md` +
`review-artifacts/lsp-recon.json`, plus a 4-lens lane recon (history/consumers/tests/risks) run at 764622f.

## 1. Findings acted on (with citations verified at 764622f)

| # | Finding | Evidence | Disposition |
|---|---|---|---|
| F1 | **Timeout is dead code**: `libc::setsockopt(SO_RCVTIMEO)` on the child's stdout **pipe** fd → ENOTSOCK, return value discarded; non-Unix branch is an explicit no-op; the 10s `Duration` in `RpcTransport::new` is never enforced. Module header (`lsp.rs:33-36`) advertises the broken mechanism as working. | `crates/wicked-estate-resolve/src/lsp.rs:173-203` (setsockopt + no-op), `:205-209` (blocking passthrough), `:213` (hardcoded 10s), `:33-36` (lying header) | Phase-0 fix (a), step S2 |
| F2 | **`LspTier` never sends `textDocument/didOpen`**: `definition/references/hover` (`lsp.rs:591-623`) call `self.client(language)?` then query directly; `did_open` exists (`lsp.rs:423-435`) but its only callers are the live test (`tests/lsp_live.rs:126-131`), which drives `LspClient` manually and so masks the defect. tsserver/pyright return empty for unopened docs. No languageId mapping exists (LSP requires `typescriptreact` for `.tsx`). | `lsp.rs:591-623`, `:423-435`, `:288-326` (registry keys), `tests/lsp_live.rs:76-131` | Phase-0 fix (b), step S3 |
| F3 | **lsp.rs is a §5 orphan** (review engine defect #5; D02-5): no `Resolver` impl, no `Edge`, no production caller. The `ignore`d-doctest half of the finding is already fixed on main — `lsp.rs:537-538` is ` ```no_run ` since #126 (0e5f4ca). | REVIEW:160; findings D02-5; `git log --oneline -- crates/wicked-estate-resolve/src/lsp.rs` → only 797ce58 + 0e5f4ca | ADR-009 defines the consumer (W3.6); phase-0 stays a client library, step S4 |
| F4 | **The prior recon is stale in a load-bearing way**: W3.6 (`docs/plan/WAVE-PLAN.md:119`), the §3.1 Lsp row (`docs/ENGINE-CONTRACT.md:140`), and the drift test (`crates/wicked-estate/src/lib.rs:2836`) all exist **on main** via #126 — not "only on lane B's unmerged branch". Every ADR citation is re-derived at 764622f, none copied from the recon. | Files at 764622f as cited | All doc steps cite main |
| F5 | **Recon gap 9 closed — there is no rmcp router**: MCP dispatch is hand-rolled (`handle_request_unified`, `mcp/src/lib.rs:661-666`); resources are a `OnceLock<Vec<McpResource>>` with `content: &'static str` (`resources.rs:8-10, 83-92`). A runtime-rendered installer skill is a local type widening, not a framework question. | grep `rmcp` over all Cargo.toml → 0 hits | ADR §installer, step S4 |
| F6 | **Recon gap closed — languageId mappings**: only two non-identity mappings needed over the 6 registry keys: `tsx→typescriptreact`, `jsx→javascriptreact`; identity for typescript/javascript/rust/python. | `lsp.rs:288-326`; LSP spec languageId vocabulary | Step S3 |
| F7 | **Drift-test interaction is bounded**: `slice_matches_engine_contract_table` asserts the anchor comment `// Activation table: docs/ENGINE-CONTRACT.md §3.1` appears exactly once in `wicked-estate/src/lib.rs`, parses the slice literal after it, and compares only §3.1 rows containing `yes (slice)`. The Lsp row's **notes cell is invisible to the test**; phase-0 changes neither the slice nor any `yes (slice)` row. | `crates/wicked-estate/src/lib.rs:1014, 2836-2900`; `docs/ENGINE-CONTRACT.md:140` | Steps S4/S5 constraints |
| F8 | **crew drops stderr and JSON.parses the whole stdout** of estate query arms (`exec.ts:98`; `graph.ts:1004-1010, 1041-1045`); stderr is logged only for the index arm. Any fallback marker must be an additive field inside the JSON payload / `RetrievalResult.diagnostics` — extra stdout lines break `JSON.parse`, stderr is invisible. | wicked-crew sources at cited lines (read-only reference) | ADR §warn-and-serve, step S4 |
| F9 | **Two mechanical MCP tripwires for the future consumer**: `response_cacheable_covers_exactly_the_graph_read_tools` asserts every `all_tools()` member is cacheable (`mcp/src/lib.rs:2327-2345`; allowlist `:723-739`); `conformance_schemas.rs:1-11` asserts tools/list is exactly 10+6+7 with golden files. `SemanticSearch` is the stateful-dispatch precedent (`lib.rs:56`). | Files at 764622f | ADR consumer-contract, step S4 |
| F10 | **Adjacent hang/corruption classes** the timeout fix must not leave behind: `Drop` does a blocking graceful shutdown and never kills/waits the child (`lsp.rs:514-521`, `:364`); a timed-out client left in `LspTier.clients` (`:551, :575-586`, no eviction) poisons every later query (and a mid-frame timeout desyncs the `BufReader`); `await_response` treats any id-carrying message as our response — a server→client request deserializes with `result` defaulting to `Null` (`:74-75`, `:241-255`, `NEXT_ID` from 1 at `:53`) → silent false `Ok(empty)`. | `lsp.rs` at cited lines | Steps S2/S3 (bounded scope, see D4/D8) |
| F11 | **`libc` is used only by the broken block** (`resolve/Cargo.toml:24`; uses confined to `lsp.rs:180-194`). §8: the fix must delete the dependency. | grep at 764622f | Step S2 deletes |
| F12 | **Locked-decision lineage is unbroken**: DESIGN-NOTES.md:28-29 → CLAUDE.md Locked decisions ("LSP is on-demand only, never bulk") → WAVE-PLAN:119 AC → ENGINE-CONTRACT:140 → REVIEW:124 explicit Reject of LSP escalation. ADR-009 relitigates nothing; it defines the sanctioned on-demand consumer. | Files at cited lines | ADR §planes, step S4 |
| F13 | **Coverage reality**: `ServerRegistry::standard()` = 6 keys → 3 servers vs 104 wired languages (`LANG_TABLE` count re-verified at 764622f); registry keys are tree-sitter grammar names; the per-node language column is **`nodes.language`** (`store/src/schema.sql:42`, inside `CREATE TABLE nodes` — the `symbols` table at `schema.sql:28-37` has only sid/sym/gen/had_node, **no language column**). The store persists only repo-relative file paths (`nodes.file`, `schema.sql:44`) and has no roots/repos table — a labelled multi-repo DB carries no repo root estate could derive. | `lsp.rs:288-326`; `store/src/schema.sql:28-49`; awk over `extract/src/treesitter.rs` | ADR §capability + §data promotion; root contract in D10/D13 |
| F14 | **All three registry servers are installed on this machine** (`/opt/homebrew/bin/{typescript-language-server,pyright-langserver,rust-analyzer}`); `lsp_live` passes live in ~4.2s. The BEFORE/AFTER didOpen measurement is runnable here; the probe-and-skip pattern (`tests/lsp_live.rs:1-25`, no `#[ignore]`) is the sanctioned shape for optional-binary tests. | which output; test run at 764622f | Steps S3/S5 measurements |

## 2. Decisions (all explicit — no TBD)

- **D1 — Timeout mechanism: one reader thread per client + `mpsc::recv_timeout`, on all platforms,
  with PER-REQUEST DEADLINE semantics.** A dedicated thread owns the child's stdout, parses complete
  Content-Length frames, and sends each parsed message over a channel. The caller computes
  `deadline = Instant::now() + budget` **once per request** and every await-loop iteration calls
  `recv_timeout(deadline.saturating_duration_since(Instant::now()))` — the request errs when the
  deadline passes **regardless of how many notification frames arrived in between**. Per-message
  `recv_timeout(budget)` is explicitly rejected: the await loop skips notifications and re-reads
  (today's shape at `lsp.rs:241`), so each notification would restart the clock and a
  chatty-but-never-answering server (rust-analyzer streaming progress during cold indexing, tsserver
  telemetry/logMessage) would hang forever while the contract says "Err within budget" (attack I1).
  Why thread+channel: `poll`/`select` is Unix-only and a `cfg`-split would recreate the
  shipped-untested-branch defect (`lsp.rs:196-199` is the scar); the thread+channel shape is pure std,
  works identically on Unix and Windows (global cross-platform rule), and lets the same change delete
  `TimeoutReader`, both `cfg` blocks, and the `libc` dependency (§8). Frames are parsed **in the
  thread**, so a timeout never leaves a half-read frame in a caller-visible buffer (kills the
  mid-frame-desync class, F10).
- **D2 — Timeout is injectable**: constructor parameter with a 10s default (`with_timeout` on
  `LspTier`/registry entry level). Why: the wedged-server test must run at ~500ms not 10s wall; W3.6
  needs per-surface budgets (crew's 30s vs rust-analyzer cold start); nothing pins 10s beyond the
  original (false) header comment.
- **D3 — On timeout: kill the child, evict the client from `LspTier.clients`, return `Err` — and the
  eviction path gets its OWN test that actually reaches it.** Why eviction: without it one 10s failure
  becomes permanent failure for the session (F10, cache poisoning); the transport cannot be trusted
  after a timeout by construction. Why a dedicated test (attack FEAS-2): the sleep-600 masquerade times
  out **inside `LspClient::spawn`'s initialize round-trip**, before `client()` ever inserts into
  `self.clients` (`lsp.rs:575-585` inserts only after `spawn` returns `Ok`; spawn does its own
  `send_and_receive` at `:374-415`) — so that test can never exercise eviction. The eviction test uses
  a **fake-server script fixture** (a small Python script: answers `initialize` correctly with a valid
  Content-Length frame, replies to `initialized`/`didOpen` notifications by ignoring them, then reads
  the next request and never replies — probe-and-skip on `python3`/`python`, same interpreter fallback
  the global rules use) registered via `ServerRegistry::register`, and asserts: (a) a first
  `LspTier::definition` on it returns `Err` within budget+margin, (b) the language key is gone from
  `clients` afterwards (a `#[cfg(test)] fn has_client(&self, lang) -> bool` accessor — test-only, not
  public API), (c) the child is killed (drop guard / no surviving process).
- **D4 — `Drop` gets a bounded shutdown then `kill()` + `wait()`.** In phase-0 scope. Why: `Drop` reads
  through the same transport — the reader-thread fix bounds the read anyway, but the never-killed child
  (`:364`) means the wedged-server test would leak a `sleep` per run and the future MCP warm pool would
  accumulate zombies. Smallest complete fix owns the child lifetime; a query-path-only fix fails its own
  test hygienically.
- **D5 — languageId mapping is a field on the registry entry, not a new file and not match arms.** Each
  `ServerRegistry` entry carries `language_id` (tsx→`typescriptreact`, jsx→`javascriptreact`, identity
  otherwise), keyed by tree-sitter grammar names as today. Why: "DATA next to ServerRegistry" (brief)
  without minting a fourth registry surface that the ADR's data-file promotion (D12) would immediately
  replace; one table moves wholesale at W3.6. Keys stay grammar names so the capability report is a
  straight join against `nodes.language` (F13).
- **D6 — Opened-docs cache is digest-keyed (`uri → content hash`), `didClose`+`didOpen` on change,
  and lives ON `LspClient`, not `LspTier`.** Why digest-keyed: the file is read anyway to build
  didOpen, hashing it is free (std `DefaultHasher`, no new deps); a once-only `HashSet` serves stale
  text on exactly the files the edit plane exists for (F10 stale-open-docs). Why per-client ownership
  (attack BR-4; the brief itself says "an opened-docs cache **per client**"): if the cache lived on
  `LspTier` and survived D3's eviction, a respawned server would have zero documents open while the
  cache claims they are — reintroducing blocker (b)'s empty-result defect on the first query after any
  timeout. A field on `LspClient` is dropped with the client on eviction, so the respawned client
  re-sends didOpen naturally; no invalidation code needed. `didChange` sync is deliberately NOT
  implemented — close/reopen is the minimal correct move for a stateless-per-query client library.
- **D7 — `LspTier` query paths do: registry lookup → file read → didOpen (cached per D6) → query —
  which requires a `file_uri_to_path` INVERSE helper that does not exist today.** The tier methods take
  `file_uri: &str` (`lsp.rs:591-623`) but the file read needs a filesystem path, and the only existing
  conversion is one-way (`path_to_file_uri`, `lsp.rs:712-725`, with an explicit Windows branch proving
  the asymmetry matters — attack I4/FEAS-7). Phase-0 adds `file_uri_to_path`: strip the `file://`
  scheme, percent-decode the needed subset (std-only, a few lines — `%20` et al.), and handle the
  Windows drive-letter form (`file:///C:/…` → `C:\…`) mirroring `path_to_file_uri`'s cfg-free string
  logic so the helper is testable on every platform. A naive `strip_prefix("file://")` would silently
  break every edit-plane query on Windows (file read errs) and on any path with spaces. File-read
  failure is a normal `Err` (no panic); files above 2 MB are opened without content re-send suppression
  games — no size cap in phase-0 (the write-side hang class is out of scope, see Not-in-scope;
  documented as a known limit in the module header).
- **D8 — `await_response` learns to distinguish by the `method` key, with one method-keyed reply
  special case.** Messages carrying `method` are never treated as our response; server→client
  **requests** (id + method) get a result reply so spec-compliant servers don't stall — `null` for
  everything EXCEPT `workspace/configuration`, which per LSP 3.17 must receive an **array with one
  element per requested item** (reply = array of nulls sized to `params.items.len()`; a bare `null`
  can make pyright error or stall internally right after didOpen — attack I5). Still protocol-level
  (one match on the method string + a params length read), not per-language logic. Why: didOpen
  increases server chatter (workspace/configuration, workDoneProgress/create), and the current code
  can deserialize a server request as our response with `result=Null` → silent false "no definition"
  (F10). This is the minimal guard that makes the didOpen fix provable rather than flaky; per-client
  id offsets are NOT added (the method-key check alone removes the misparse). The misparse fix gets a
  **deterministic unit test** (attack FEAS-5), not just incidental live coverage: feed the frame pump +
  await loop an in-memory `Read` scripted as [notification, server→client request whose `id` equals the
  expected response id, real response] — assert the real response's result is returned and the correct
  reply (null / sized null-array) was written for the server request. The ADR notes the pyright
  chatter path is otherwise exercised only when `pyright-langserver` is present (probe-and-skip).
- **D9 — Test shapes.** (1) A platform-independent mechanism unit test: the frame pump generic over
  `R: Read + Send + 'static`, fed a `Read` impl that blocks forever (Condvar park) — asserts
  `Err` within budget; pure std, compiles and runs on Windows, so the cross-platform claim is tested,
  not asserted. (1a) A **chatty-server deadline unit test** (attack I1 — the silent `sleep` masquerade
  cannot distinguish per-recv from deadline semantics): the frame pump fed a `Read` impl that yields
  well-formed **notification** frames every budget/4 and never a response — the request must `Err` at
  the deadline (within budget+margin), proving notifications do not restart the clock. Pure std, all
  platforms. (2) Integration `tests/lsp_timeout.rs`: register `sleep 600` as a masquerading server
  via `ServerRegistry::register`, assert `LspTier::definition` returns `Err` within budget+margin, child
  killed on both paths (drop guard); probe-and-skip on the `sleep` binary (honest skip on Windows —
  the mechanism tests above are the cross-platform coverage; no `#[ignore]` anywhere). This test
  exercises the spawn/initialize timeout path ONLY — the client never enters `clients`. (2a) The
  **eviction integration test** per D3: fake-server Python script fixture (answers initialize, wedges
  on the first real request), probe-and-skip on the interpreter; asserts Err-within-budget + key evicted
  (`#[cfg(test)]` accessor) + child killed. (3) A live probe-and-skip test driving `LspTier::definition`
  (not `LspClient`) on the `write_ts_fixture` layout plus a `.tsx` file (proves the languageId mapping
  against the real server); bounded retry loop, not a fixed 500ms sleep. (4) A pure-data unit test for
  the languageId table. (5) The deterministic misparse/reply unit test per D8. (6) Pure-string unit
  tests for `file_uri_to_path` per D7: a Unix path with `%20`, a Windows drive-letter URI
  (`file:///C:/…`), and a round-trip against `path_to_file_uri` — string logic, runs on all platforms.
- **D10 — Planes are defined by query shape, falsifiably, with an explicit workspace-root contract.**
  EDIT plane = exactly one `(file, line, col)` position per call, single symbol, single repo, answered
  live by LSP; **no array parameters, no file-enumeration parameters, no glob/name parameters** — bulk
  routing is **type-resistant at the tool boundary and invariant-banned inside it** (attack I3:
  one-position-per-call bans batch *parameters*, not internal *iteration*, so the ADR adds a second,
  code-checkable invariant: estate code may never invoke an edit-plane LSP query from an iteration over
  store contents, extraction output, or the file tree — LSP invocations per MCP/CLI call are **O(1)**,
  one position → at most one query per method; repeated *external* calls are on-demand by definition,
  internal fan-out is bulk by definition). UNDERSTAND plane = set-valued/graph-shaped
  (BlastRadius/Lineage/SearchEntity/hotspots), answered from the store, LSP never consulted.
  Definition-by-NAME (no position) is an understand-plane graph lookup by definition.
  **Workspace-root contract (attack BR-2)**: the store persists only repo-relative paths and no
  per-label repo root (F13) — so edit-plane tools take an explicit **absolute workspace root as a
  required parameter** (optionally alongside a repo label for attribution); estate never guesses roots
  for labelled DBs, `file` in the request is resolved as root-relative, and the `file://` URI +
  `LspTier::new(root_dir)` are both derived from that caller-supplied root. Labelled multi-repo DBs are
  therefore served **one repo per call by construction**. Coordinate contract pinned in the ADR:
  LSP-native 0-based lines, UTF-16 columns; the tools take LSP coordinates and the ADR documents the
  graph-node→position handoff hazards (byte spans, stale vs live text, **and repo-relative
  `nodes.file` → absolute-root mapping**). Why: no prior art exists (recon gap 1) — ADR-009 is the
  defining document, and the review rejected LSP escalation (REVIEW:124); the locked bulk ban must
  survive an implementer who never read it.
- **D11 — Warn-and-serve channel is a wire contract.** Missing LSP ⇒ serve graph results labeled with a
  sibling marker `LSP-FALLBACK:` in `RetrievalResult.diagnostics` (MCP; exactly R6's `GRAPH-FALLBACK`
  precedent, `docs/agent-behavior-rules.md:34-45`) and an additive field inside the existing JSON stdout
  payload (CLI twins). Extra stdout lines and stderr are banned for query arms, citing crew's code (F8).
  Hover has no graph analogue: fallback for Hover is explicit absence + marker, never a simulated hover.
  References-fallback is labeled as incoming Calls/Imports at graph confidence — partial by construction.
- **D11a — R4 result cap is part of the edit-plane wire contract (attack BR-1).** References on a
  hot symbol returns thousands of Locations; the CLI twin's stdout rides through crew's finite
  `execCapped` buffer (`exec.ts:21-24, 84-94` — throws on overflow) and the MCP path would dump an
  unranked wall violating R4 (`docs/agent-behavior-rules.md:27-29`, whose enforcement note assigns R4
  to "server + tool-impl responsibilities"). ADR-009 pins: a **hard result cap on References** (default
  100 locations; Definition/Hover are ≤1-ish by nature but share the cap plumbing), truncation surfaced
  as a structured **`LSP-TRUNCATED: <served>/<total>`** sibling marker in `RetrievalResult.diagnostics`
  next to `LSP-FALLBACK:`, and the same additive-JSON-field rule for the CLI twin (never extra stdout
  lines, never stderr — F8). Named in the D14 W3.6 implementation-AC list so the implementer cannot
  ship the unbounded dump.
- **D12 — ServerRegistry is promoted to DATA at W3.6 implementation, not phase-0.** One data file
  (grammar-name key → server binary+args → languageId → one pinned official docs/package pointer →
  optional `no_known_server` flag), `include_str`-loaded in languages.toml generated-artifact style.
  Why: rules-as-data (Universal Don'ts) with the in-repo precedent of #126's family-as-data move;
  phase-0 keeps the table in Rust because promoting a 6-row table twice (once without the pointer
  column, once with) is churn without a consumer. The ADR states the 6-key/3-server vs 104-language gap
  and the expansion path (go, java, csharp, c/cpp, ruby, php, kotlin… next; legacy families flagged
  `no_known_server`).
- **D13 — Capability vocabulary: `ok` / `missing` / `no-server-known` / `not-in-registry`.** The brief's
  "no-server-exists" is rendered as `no-server-known`, sourced only from curated `no_known_server` rows
  in the data file — nonexistence is a rotting claim (someone ships a COBOL LSP); a language with no row
  reports `not-in-registry`. Languages enumerated per repo from **`SELECT DISTINCT language FROM
  nodes`** — the column is `nodes.language` (`schema.sql:42`); the plan's earlier `FROM symbols` was
  wrong, `symbols` has no language column (attack BR-3/FEAS-3) — **keyed by scope prefix**
  (`nodes.scope`, `schema.sql:58`) for labelled multi-repo DBs, with the **probe root supplied by the
  caller** exactly as in D10 (the store has no roots table; attack BR-2). Zero schema change survives.
  Report cached with an explicit `refresh` parameter and post-install write-through re-probe — PATH
  changes are invisible to any mtime key, and garden's persistent stdio broker would otherwise serve
  stale `missing` forever.
- **D14 — MCP dispatch shape for the future tools (ADR contract, not phase-0 code):**
  Definition/References/Hover/LspCapabilities are constructed with owned state and dispatched **outside**
  `all_tools()` (SemanticSearch precedent, `mcp/src/lib.rs:56`), never added to `response_cacheable`
  (fail-safe default already excludes them, `:719`). W3.6 implementation AC must include: conformance
  count bump + golden schema files, a cache-exclusion test in the `cache_staleness.rs`
  real-binary-over-stdio style, the five "23 tools" doc claims (README.md:150/:191, CLAUDE.md:10/:232,
  docs/mcp-integration.md:30), **the D11a R4 result cap + `LSP-TRUNCATED` marker (a References answer
  is never an unbounded dump)**, and **the D10 required-root parameter on every edit-plane tool and
  CLI twin**. Tools are advertised unconditionally; a missing server is a warn-and-serve answer, not a
  hidden tool.
- **D15 — Installer skill: runtime-rendered `skill://` resource, pointer model, instruct-only.**
  Rendered from the D12 data file by widening `McpResource.content` from `&'static str` to
  `Cow<'static, str>` (F5 — hand-rolled router, local change; the compile-time claim at
  `resources.rs:1-2` gets amended in the same change). No curated install-command registry: one pinned
  official docs/package pointer per server; the agent fetches current instructions, executes under its
  own permission gate, re-probes the registry's **exact binary name** (npm `pyright` provides
  `pyright-langserver` — verify the binary, not "install succeeded"); offline degrades to the package
  name. Boundary stated verbatim in the ADR: **wicked-installer remains the sole scripted-install
  surface; this skill instructs, never executes; language servers stay out of registry.json.**
- **D16 — Demand-driven 1.0 write-back is an explicitly deferred optional phase** with named unsolved
  preconditions: scheme-2 id churn + `prune_dangling_edges` decay (write-back edges silently vanish on
  the next index and nothing re-emits them), UTF-16-vs-byte span correlation, ON CONFLICT keep-higher
  staleness, and mtime-cache thrash (each write-back clears the whole MCP response cache). The ADR
  describes the scip file+span correlation seam as the mechanism and lists these as gate criteria — it
  does not call the phase straightforward.
- **D17 — Drift-guard discipline: the §3.1 Lsp table row stays BYTE-IDENTICAL in phase-0 — no table
  edit of any kind.** The brief locks the §3.1 Lsp row to "updated ONLY when a consumer actually
  lands", and phase-0 lands no consumer. The plan's earlier S4 file list said "Lsp row notes cell only"
  — that contradicted this decision (attack FEAS-1/I2) and is resolved in favor of NO row edit: the
  brief's "ENGINE-CONTRACT updated" deliverable is satisfied by a **prose note OUTSIDE the §3.1 table**
  (under §3.1 / in the on-demand-tiers prose) pointing at ADR-009 and stating explicitly that phase-0
  changes no activation. When the consumer lands (W3.6 impl lane, not this one), only the Lsp row's
  **notes** cell changes and its activation cell must remain a non-`yes (slice)` value ("no — on-demand
  MCP/CLI only"). The drift test reads only `yes (slice)` rows (F7) so the prose note is mechanically
  invisible to it. New code in `wicked-estate/src/lib.rs` must never repeat the anchor string
  `// Activation table: docs/ENGINE-CONTRACT.md §3.1` (uniqueness assert). Stated in the ADR.
- **D18 — Doc corrections ride along**: lsp.rs module header §Timeout rewritten to name the real
  mechanism (F1); WAVE-PLAN W3.3's "10s per-request timeout" line becomes true and is reworded to name
  the mechanism; W3.6 row updated to point at ADR-009. Garden issue #347 re-scope note is **written into
  the ADR** (estate owns probe/serve/route; #347 re-scoped to CC-plugin wiring that consumes estate's
  surface) — no garden repo edits from this lane.
- **D19 — ADR number is 009** (docs/adr/ has 001–008), flagged provisional in merge notes (concurrent
  lanes may collide; renumber at merge is a filename+title change only).

## 3. Step list

### S1 — this plan (done by this commit)
- **Files**: `docs/recon/lsp-w36.md` (new).
- **Tests**: none (doc). **Deletes**: nothing.

### S2 — phase-0 fix (a): working cross-platform read timeout
- **Files**: `crates/wicked-estate-resolve/src/lsp.rs`, `crates/wicked-estate-resolve/Cargo.toml`,
  new `crates/wicked-estate-resolve/tests/lsp_timeout.rs`.
- **Change**: replace `TimeoutReader` with a frame-pump reader thread (generic over
  `R: Read + Send + 'static`) sending parsed frames over `mpsc`; **per-request deadline** at call sites
  (`recv_timeout(deadline.saturating_duration_since(now))` each iteration, D1 — never a fresh
  per-message budget); injectable timeout (D2); on timeout kill child + evict from `LspTier.clients` +
  `Err` (D3); `Drop` = bounded shutdown then `kill()`+`wait()` (D4); rewrite module header §Timeout
  (D18).
- **Tests**: (i) unit: blocking-`Read` frame pump returns `Err` within budget (all platforms);
  (i-a) unit: **chatty-server deadline test** — notification frames every budget/4, never a response →
  `Err` within budget+margin (D9(1a), all platforms); (ii) integration `lsp_timeout.rs`: `sleep 600`
  masquerade via `ServerRegistry::register`, `LspTier::definition` → `Err` within budget+margin, no
  hang, child killed (drop guard), probe-and-skip on `sleep` — spawn/initialize path only;
  (ii-a) integration: **eviction test** — Python fake-server fixture answers initialize then wedges on
  the first request; asserts `Err` within budget, language key evicted from `clients` (`#[cfg(test)]`
  accessor), child killed; probe-and-skip on the interpreter (D3/D9(2a)); (iii) existing 19 lsp.rs unit
  tests + `lsp_live` stay green (they reference neither `TimeoutReader` nor tier internals).
- **Deletes**: `TimeoutReader` struct, the `cfg(unix)` setsockopt block, the `cfg(not(unix))` no-op,
  `libc = "0.2"` from Cargo.toml (F11), the false §Timeout header text.

### S3 — phase-0 fix (b): didOpen + languageId + opened-docs cache
- **Files**: `crates/wicked-estate-resolve/src/lsp.rs`, `crates/wicked-estate-resolve/tests/lsp_live.rs`
  (additive test fns only).
- **Change**: `language_id` field on registry entries (D5); new `file_uri_to_path` inverse helper
  (scheme strip + percent-decode + Windows `file:///C:/` form, D7); `LspTier::definition/references/hover`
  do file read → digest-keyed didOpen (didClose+didOpen on digest change; **cache is a field on
  `LspClient`**, dropped with the client on D3 eviction, D6/D7); `await_response` method-key guard +
  replies to server→client requests (`null`, except `workspace/configuration` → null-array sized to
  `params.items.len()`, D8). `lsp.rs` remains a client library: no `Resolver` impl, no `Edge`, no slice
  wiring.
- **Tests**: pure-data languageId unit test; `file_uri_to_path` unit tests (Unix `%20`, Windows drive
  URI, round-trip — D9(6)); deterministic misparse/reply unit test (scripted frames: notification →
  colliding-id server request → real response, D8/D9(5)); live probe-and-skip test driving
  **`LspTier::definition`** on the TS fixture + a `.tsx` file, asserting non-empty, bounded retry (D9);
  two consecutive queries on the same file succeed (cache path, live).
- **BEFORE/AFTER mechanics (attack FEAS-6)** — the brief's release-binary BEFORE is inapplicable:
  lsp.rs is unreachable from any binary arm (F3 — no production caller), so no `wicked-estate` binary
  invocation can exercise LspTier. The honest source-level equivalent, recorded verbatim: (1) commit
  the new live test (asserting **non-empty**) in its own commit BEFORE the didOpen fix; (2) run it
  there — it FAILS, and the assertion output showing the empty result set is the BEFORE evidence;
  (3) land the didOpen fix commit; (4) the green run of the same test is the AFTER evidence. Both runs
  under the lane `CARGO_TARGET_DIR`; both command lines + outputs recorded in S5.
- **Deletes**: none — defect fix on an existing path; nothing is replaced (stated per §8; the deletion
  ledger for this lane lives in S2).

### S4 — ADR-009 + doc updates
- **Files**: new `docs/adr/ADR-009-intent-routed-lsp.md`; `docs/plan/WAVE-PLAN.md` (W3.6 row → ADR-009;
  W3.3 timeout wording, D18); `docs/ENGINE-CONTRACT.md` (**prose note OUTSIDE the §3.1 table only** —
  the Lsp table row stays byte-identical in phase-0, D17); `docs/agent-behavior-rules.md` (sibling
  `LSP-FALLBACK:` marker row beside `GRAPH-FALLBACK`, D11).
- **Change**: ADR sections = planes by query shape + O(1)-invocation invariant + workspace-root
  contract (D10); warn-and-serve wire contract (D11) + R4 result cap / `LSP-TRUNCATED` marker (D11a);
  capability report + vocabulary, scope-keyed, `nodes.language` (D13); MCP dispatch contract + W3.6
  implementation AC list incl. R4 cap and required-root param (D14); installer skill, pointer model,
  wicked-installer boundary verbatim (D15); ServerRegistry→data promotion + coverage gap + expansion
  path (D12); deferred write-back with gate criteria (D16); drift-guard discipline + "phase-0 changes
  no activation" statement (D17); garden #347 re-scope note (D18); crew budget note (CLI twin is
  structurally cold; warm sessions owned by the MCP server process; injectable timeout is the seam, D2).
- **Tests**: `cargo test -p wicked-estate slice_matches_engine_contract_table` green (proves §3.1
  discipline); `cargo test -p wicked-estate-resolve` green. Grep-check: the anchor string appears once in
  `wicked-estate/src/lib.rs` (unchanged).
- **Deletes**: the stale W3.3 timeout claim text (replaced by the true statement); nothing else.

### S5 — verification + measurements (evidence block)
- **Commands (record verbatim)**: `cargo build -p wicked-estate-resolve`,
  `cargo test -p wicked-estate-resolve`, `cargo clippy -p wicked-estate-resolve -- -D warnings`,
  `cargo fmt -p wicked-estate-resolve`, `cargo test -p wicked-estate` (drift test), all with
  `CARGO_TARGET_DIR=<lane>/target`. Wedged-server wall-clock recorded (must be ≪ 10s with the injected
  budget). didOpen BEFORE/AFTER outputs from S3's commit-ordered mechanics — with the explicit statement
  that the brief's release-binary BEFORE is inapplicable (lsp.rs unreachable from the binary, F3) and
  the source-level substitute is the honest equivalent. Probe results recorded (all three servers
  present on this machine — green-via-live, not green-via-skip; state which, F14).
- **Bench**: non-regression claimed as a statement, not a run — lsp.rs has no production caller, no
  edge emission, so no bench number can move; cited as such in the evidence block (§7).

## 4. Compatibility + migration

- **Stored graphs**: zero impact. Phase-0 emits no edges, touches no store code, no schema, no re-index,
  no migration. The ADR's deferred write-back phase would upsert 1.0 edges onto existing
  `(source,target,kind)` triples (schema.sql:72-81; keep-higher per ENGINE-CONTRACT:112) — no migration
  then either, but bench `ConfidenceBands.exact` and capability receipts move at that point (rebaseline
  via garden's gate-benchmark-rebaseline); explicitly deferred with gate criteria (D16).
- **Consumers**: crew/garden/studio see no behaviour change — lsp.rs has zero consumers outside its own
  crate at 764622f; crew's estate calls are all understand-plane. The public API of `wicked-estate-resolve`
  gains `with_timeout`-style constructors and a `language_id` registry field (additive); `TimeoutReader`
  and `RpcTransport` are private, so their replacement is invisible.
- **MCP surface**: unchanged in phase-0 (no new tools, tools/list still 10+6+7, resources unchanged).
  The consumer-phase changes (count bump, goldens, cache exclusion, `Cow` content) are specified as W3.6
  implementation AC in ADR-009, to be made deliberately, not patched around.
- **Windows**: the reader-thread mechanism is the same code on all platforms; the mechanism unit test is
  the cross-platform proof (pure std, no cfg). The `sleep`-masquerade integration test honestly skips
  where `sleep` is absent.

## 5. Falsifier

Run on a machine with `typescript-language-server` on PATH, `CARGO_TARGET_DIR` set to the lane target:

1. `cargo test -p wicked-estate-resolve --test lsp_timeout` — if any test hangs past budget+margin, or a
   `sleep` child survives the run (`pgrep -f 'sleep 600'`), fix (a) is falsified. The chatty-server
   deadline unit test (D9(1a)) falsifies per-message-reset semantics: notification frames every budget/4
   with no response must still `Err` at the deadline. The eviction test (D9(2a)) falsifies unreachable
   eviction: after a wedged in-map client times out, the language key must be gone from `clients`.
2. The new `LspTier::definition` live test (asserting non-empty) is committed BEFORE the didOpen fix and
   must FAIL there with output showing an empty result set (= BEFORE evidence); the same test must pass
   after the fix commit (= AFTER). A green pre-fix run means blocker (b) was misdiagnosed; a red
   post-fix run means the fix failed.
3. `cargo test -p wicked-estate slice_matches_engine_contract_table` must pass with zero edits to the
   slice literal or any `yes (slice)` row — if it required an edit, this lane changed activation and
   violated its own scope.
4. `grep -rn libc crates/wicked-estate-resolve/` must return 0 hits after S2 — a hit means §8 was
   violated.

## 6. Not in scope (this lane)

- Any `Resolver` impl, `Edge` emission, or slice wiring for lsp.rs (locked; W3.6 implementation lane).
- New MCP tools / CLI arms (Definition/References/Hover/LspCapabilities/doctor) — designed in ADR-009,
  built by the W3.6 implementation lane.
- The ServerRegistry→data-file promotion and the installer-skill resource itself (designed here,
  built at W3.6 implementation, D12/D15).
- Write-side (`stdin.write_all`) timeout and didOpen payload size caps — the wedged-writer class is
  documented in ADR-009 as a W3.6-implementation concern; phase-0 fixes the read path the brief names.
- `didChange` incremental sync, per-client id offsets, keying warm clients by `(binary,args,root)`
  (dedup of the 4-keys→1-tsserver fan-out) — noted in ADR-009 as W3.6 implementation items.
- Garden repo edits (#347 is re-scoped by a note in the ADR only); wicked-installer, crew, studio.
- MUST-NOT-TOUCH honored: version files, the resolve admissibility block
  (`resolve/src/lib.rs:54-121`), `remove_file` paths (`store/src/lib.rs`), `extract/src/plugin.rs`.

## 7. Merge notes for other lanes

- **ADR-009 number is provisional** (docs/adr/ has 001–008 at 764622f). If a concurrent lane lands an
  ADR first, renumber this one at merge — filename + title only, no content coupling.
- This lane edits `docs/plan/WAVE-PLAN.md` (W3.3 wording + W3.6 row), `docs/ENGINE-CONTRACT.md`
  (**prose note outside the §3.1 table only** — the Lsp table row stays byte-identical in phase-0,
  D17), and `docs/agent-behavior-rules.md` (one marker row). Any lane touching the same lines should
  merge textually; no lane should touch the §3.1 Lsp row or flip any activation cell.
- No lane may introduce the string `// Activation table: docs/ENGINE-CONTRACT.md §3.1` anywhere in
  `crates/wicked-estate/src/lib.rs` — the drift test asserts uniqueness.
- `crates/wicked-estate-resolve/src/lsp.rs`, `tests/lsp_live.rs`, new `tests/lsp_timeout.rs`, and
  `resolve/Cargo.toml` are owned by this lane for the duration.

## 8. Revision log — attack round 1 (all majors resolved, minors folded in)

| Issue | Severity | Resolution |
|---|---|---|
| I1 / FEAS-4 (per-message recv_timeout unbounded under notification chatter) | major | D1 rewritten to per-request DEADLINE semantics (`deadline = now + budget` once, remaining-time on every `recv_timeout`); new chatty-server deadline unit test D9(1a)/S2(i-a); falsifier 1 extended. |
| BR-1 (R4 / execCapped: unbounded References dump breaks crew) | major | New D11a: hard References cap (default 100), `LSP-TRUNCATED: <served>/<total>` sibling marker in diagnostics, additive-field rule for CLI twins; named in D14's W3.6 impl-AC list and S4. |
| BR-2 (no workspace root / absolute URI source for labelled multi-repo DBs) | major | D10 gains the workspace-root contract: required absolute-root parameter on every edit-plane tool (estate never guesses roots), repo-relative→root mapping added to the handoff hazard list, one repo per call by construction; D13 capability report keyed by `nodes.scope` prefix with caller-supplied probe root; F13 row corrected to state the store has no roots table. |
| FEAS-1 / I2 (S4 vs D17 contradiction on the §3.1 Lsp row) | major | Resolved in favor of NO table edit: §3.1 Lsp row byte-identical in phase-0; ENGINE-CONTRACT deliverable satisfied by a prose note OUTSIDE the table pointing at ADR-009; D17, S4 files, and merge notes rewritten consistently. |
| FEAS-2 (eviction path unreachable by every named test) | major | D3 rewritten: acknowledges the sleep masquerade times out inside `spawn` (pre-insertion, lsp.rs:575-585); new eviction integration test D9(2a)/S2(ii-a) — Python fake-server answers initialize then wedges; asserts Err + key evicted (`#[cfg(test)]` accessor) + child killed; falsifier 1 extended. |
| I3 (bulk ban only type-level; internal iteration could bulk-route) | minor | D10 softened to "type-resistant at the boundary, invariant-banned inside it" + new O(1)-invocations-per-call invariant (no LSP query from iteration over store/extraction/file-tree). |
| I4 / FEAS-7 (missing `file_uri_to_path` inverse; Windows drive URIs, percent-decoding) | minor | D7 names the inverse helper + hazards; S3 adds it with unit tests (Unix `%20`, Windows `file:///C:/`, round-trip), D9(6). |
| I5 (generic null reply is spec-invalid for `workspace/configuration`) | minor | D8: method-keyed reply special case — null-array sized to `params.items.len()` for `workspace/configuration`, null otherwise; pyright-path coverage caveat stated. |
| BR-3 / FEAS-3 (capability SQL cites nonexistent `symbols.language`) | minor | F13 + D13 corrected to `SELECT DISTINCT language FROM nodes` (`nodes.language`, schema.sql:42; `symbols` has no language column). |
| BR-4 (opened-docs cache surviving eviction reintroduces empty-result defect) | minor | D6: cache is a field on `LspClient` (per-client, per the brief), dropped with the client on eviction; respawned client re-sends didOpen naturally. |
| FEAS-5 (D8 misparse guard had no deterministic test) | minor | D8/D9(5)/S3: scripted-frames unit test (notification → colliding-id server request → real response) asserting correct response + correct reply. |
| FEAS-6 (BEFORE measurement mechanics unstated; release-binary BEFORE inapplicable) | minor | S3 gains explicit commit-ordered mechanics (test-first commit fails red = BEFORE; post-fix green = AFTER); S5 + falsifier 2 state the release-binary BEFORE is inapplicable (lsp.rs unreachable from the binary, F3) and why the substitute is honest. |

No objections rejected — every attack issue was verified against the cited lines and accepted.

## Fixer round 1 — correctness-1-C1 (gapless-flood deadline bypass)

The plan's fix (a) mechanism — "deadline computed once, each `recv_timeout` waits only the
remaining time" — was insufficient as stated: `recv_timeout(ZERO)` returns `Ok` when a frame
is already queued, so a server emitting gap-free notifications (channel permanently non-empty)
bypassed the deadline entirely and `await_response` hung forever, plus unbounded channel
growth. The plan's own falsifier 1 caught it. Resolution (commit 6b8e0ba):

- `recv_frame_by` now checks the wall clock against the deadline BEFORE touching the channel
  (`lsp.rs`, RpcTransport); overshoot is bounded by one frame parse.
- The frame-pump channel is bounded (`sync_channel(FRAME_CHANNEL_BOUND = 64)`) — a flooding
  server blocks the pump (backpressure into the OS pipe buffer) instead of growing memory.
- Pinned by `gapless_notification_flood_still_errs_at_the_deadline` (ChattyReader, delay ZERO,
  300ms budget). Red-check evidence: with BOTH hunks reverted the test hangs past a 60s cap;
  with only the deadline check reverted it passes racily (the bounded channel lets the queue
  drain when the consumer outruns the pump) — which is why the deterministic guarantee is the
  wall-clock check, and the bound is the memory cap, not the timeout mechanism.
