# ADR-009 — Intent-routed LSP: the W3.6 on-demand consumer

**Status:** Accepted (design) — implementation is the W3.6 lane. Phase-0 fixes (transport
timeout, didOpen) landed with this ADR. *ADR number provisional at merge (docs/adr/ has
001–008 at the base commit).*

**Context:** `lsp.rs` (W3.3) is a working LSP client library with no production caller — a
CLAUDE.md §5 orphan named by the 2026-08-28 adversarial review (engine defect #5, D02-5,
D03-10, PER-8). The locked decision stands: **LSP is on-demand only, never bulk** (CLAUDE.md
Locked decisions; `docs/DESIGN-NOTES.md`; REVIEW's explicit Reject of LSP escalation). This
ADR defines the sanctioned consumer precisely enough that a future implementer cannot
bulk-route by accident. There is no prior art for this routing — this document is the
defining one.

---

## 1. The two planes (routing by query shape)

Every code-intelligence question estate answers belongs to exactly one plane, determined by
the **shape of the query**, not by who asks:

| Plane | Query shape | Answered by | LSP? |
|---|---|---|---|
| **EDIT** | exactly one `(file, line, col)` position; single symbol; single repo | live language server | **LSP first**; missing LSP ⇒ warn-and-serve graph results (§3) |
| **UNDERSTAND** | set-valued / graph-shaped: BlastRadius, Lineage, SearchEntity, hotspots, definition-by-NAME | the store (graph + FTS) | **never** — LSP is not consulted, regardless of availability |

Rules that make the split falsifiable:

- **Edit-plane tools are position-anchored.** The MCP tools (`Definition`, `References`,
  `Hover`) and their CLI twins take exactly one `(file, line, col)` — **no array
  parameters, no file-enumeration parameters, no glob or name parameters**. Bulk routing is
  *type-resistant at the tool boundary*.
- **…and invariant-banned inside it.** Parameter shape bans batch *inputs*, not internal
  *iteration* — a warm-pool primer or a References "enricher" could bulk-route through the
  single-position API without violating any signature. So the second, code-checkable
  invariant: **estate code may never invoke an edit-plane LSP query from an iteration over
  store contents, extraction output, or the file tree. LSP invocations per MCP/CLI call are
  O(1)** — one position, at most one query per method. Repeated *external* calls are
  on-demand by definition; internal fan-out is bulk by definition. Review gate: any loop
  whose body reaches `LspTier` is a defect.
- **Definition-by-NAME (no position) is an understand-plane graph lookup.** Only a position
  makes a query edit-plane.
- The bulk ban itself is locked and not relitigated here; this ADR only defines the
  on-demand consumer the lock always anticipated (WAVE-PLAN W3.6).

### 1.1 Workspace-root contract

The store persists only **repo-relative** `nodes.file` paths (`store/src/schema.sql:44`) and
has **no roots/repos table** — a labelled multi-repo DB (crew's co-located graphs) carries no
repo root estate could derive. Therefore:

- Every edit-plane tool and CLI twin takes an explicit **absolute workspace root as a
  required parameter** (optionally alongside a repo label for attribution). Estate never
  guesses roots.
- The request's `file` is resolved root-relative; the `file://` URI and `LspTier::new(root)`
  both derive from the caller-supplied root.
- Labelled multi-repo DBs are served **one repo per call by construction**.
- Coordinate contract: LSP-native **0-based lines, UTF-16 columns**. Handoff hazards a
  caller mapping graph nodes to positions must handle: byte spans vs UTF-16 columns, stale
  DB text vs live file text, and repo-relative `nodes.file` → absolute-root mapping.

## 2. Phase-0 (landed with this ADR)

Two blockers fixed in `lsp.rs`, which **stays a client library**: no `Resolver` impl, no
`Edge` emission, no slice wiring, no MCP surface change (tools/list stays 10+6+7).

1. **Transport timeout was dead code** — `setsockopt(SO_RCVTIMEO)` on a pipe fd (ENOTSOCK,
   return value discarded), non-Unix branch a no-op. Replaced by a frame-pump reader thread +
   `mpsc` with **per-request deadline** semantics (notification chatter never restarts the
   clock), injectable budget (default 10s), kill + evict on timeout, bounded `Drop`. One
   mechanism on Unix and Windows; `libc` deleted.
2. **`LspTier` never sent `textDocument/didOpen`** — tsserver/pyright answered empty for
   every query. The tier now reads the file and opens it per query (digest-keyed cache **on
   `LspClient`**, so eviction drops it), with `languageId` as data
   (`tsx→typescriptreact`, `jsx→javascriptreact`, identity otherwise) on the registry rows,
   a `file_uri_to_path` inverse (percent-decoding, Windows drive form), and an
   `await_response` method-key guard that replies to server→client requests
   (`workspace/configuration` gets a null-array sized to `params.items.len()` per LSP 3.17).

**Phase-0 changes no activation**: the ENGINE-CONTRACT §3.1 Lsp row is byte-identical;
the drift test (`slice_matches_engine_contract_table`) sees nothing. Known limits deferred
to W3.6 implementation: write-side (`stdin.write_all`) timeout, didOpen payload size caps,
`didChange` incremental sync, per-client id offsets, warm-client keying by
`(binary, args, root)`.

## 3. Warn-and-serve (missing LSP) — wire contract

When the edit plane cannot reach a language server, estate **serves labeled graph results**
instead of erroring (R1: an early `isError` causes session-wide tool abandonment):

- **Marker:** `LSP-FALLBACK:` as a sibling of R6's `GRAPH-FALLBACK:` in
  `RetrievalResult.diagnostics` (the R3/R5/R6/R7 channel — `docs/agent-behavior-rules.md`).
- **CLI twins:** the marker is an **additive field inside the existing JSON stdout
  payload**. Never extra stdout lines, never stderr — crew `JSON.parse`s the whole stdout of
  estate query arms and drops stderr entirely (crew `exec.ts:98`, `graph.ts:1004-1010`).
- **Fallback content:** Definition ⇒ graph definition lookup at graph confidence;
  References ⇒ incoming Calls/Imports edges at graph confidence, labeled **partial by
  construction**; Hover has no graph analogue ⇒ explicit absence + marker, never a simulated
  hover. Confidence stays visible (R7).

### 3.1 Result cap (R4)

References on a hot symbol returns thousands of locations; crew's `execCapped` throws on
buffer overflow, and an unranked wall violates R4 (~25K chars — enforcement assigned to
tool impls, `agent-behavior-rules.md`). Contract:

- **Hard result cap on References — default 100 locations.** Definition/Hover are ≤1-ish by
  nature but share the cap plumbing.
- Truncation is surfaced, never silent: **`LSP-TRUNCATED: <served>/<total>`** as a
  structured sibling marker in `diagnostics`, additive JSON field on the CLI twin.

## 4. Capability report

A per-repo report of which extracted languages have a live LSP path:

- **Languages:** `SELECT DISTINCT language FROM nodes` (the column is `nodes.language`,
  `schema.sql:42`; the `symbols` table has no language column), filtered by `nodes.scope`
  prefix for labelled multi-repo DBs. Zero schema change.
- **Probe:** ServerRegistry binary probe per language, run against a **caller-supplied
  probe root** (same root contract as §1.1).
- **Vocabulary:** `ok` (server registered + on PATH) / `missing` (registered, binary absent)
  / `no-server-known` (curated `no_known_server` rows only — nonexistence is a rotting
  claim, so it is data, never inference) / `not-in-registry` (no row at all).
- **Caching:** cached with an explicit `refresh` parameter and a **post-install
  write-through re-probe** — PATH changes are invisible to mtime keys, and a persistent
  stdio broker would otherwise serve stale `missing` forever.
- **Surfaces:** `LspCapabilities` MCP tool + a CLI `doctor` arm (W3.6 implementation).

## 5. MCP dispatch contract (W3.6 implementation AC)

`Definition` / `References` / `Hover` / `LspCapabilities` are constructed with owned state
and dispatched **outside `all_tools()`** (the `SemanticSearch` precedent, `mcp/src/lib.rs`),
and are **never added to `response_cacheable`** — LSP answers depend on PATH and live file
content (`response_cacheable_covers_exactly_the_graph_read_tools` forces every `all_tools()`
member cacheable; these tools must not be members). Tools are advertised unconditionally: a
missing server is a warn-and-serve answer, not a hidden tool (R2 applies to the graph being
absent, not to a degraded edit plane).

The W3.6 implementation lane MUST also deliver, as acceptance criteria:

1. conformance tool-count bump + golden schema files (`conformance_schemas.rs` asserts
   exactly 10+6+7 today);
2. a cache-exclusion test in the `cache_staleness.rs` real-binary-over-stdio style;
3. the five "23 tools" doc-claim updates (README.md ×2, CLAUDE.md ×2,
   docs/mcp-integration.md ×1);
4. the §3.1 References cap + `LSP-TRUNCATED` marker (a References answer is never an
   unbounded dump);
5. the §1.1 required-root parameter on every edit-plane tool and CLI twin;
6. the ENGINE-CONTRACT §3.1 Lsp row **notes-cell-only** update (§8).

**Budget note:** the CLI twin is structurally cold (one process per invocation — spawn,
initialize, query, exit); warm sessions belong to the MCP server process, which owns
long-lived `LspTier` state. The injectable timeout (phase-0) is the seam for per-surface
budgets (e.g. crew's 30s outer budget vs rust-analyzer cold start).

## 6. ServerRegistry → data (at W3.6 implementation, not phase-0)

Today's registry covers **6 grammar keys → 3 servers** (typescript/tsx/javascript/jsx →
typescript-language-server; rust → rust-analyzer; python → pyright-langserver) against
**104 wired languages** in `languages.toml`. At W3.6 implementation the registry is promoted
to one data file (rules-as-data; the #126 family-as-data precedent), one row per grammar
name:

```
grammar key → server binary + args → languageId → one pinned official docs/package pointer
            → optional no_known_server flag
```

`include_str!`-loaded in the languages.toml generated-artifact style. Phase-0 keeps the
table in Rust (with `languageId` already a data column) because promoting a 6-row table
twice — once without the pointer column, once with — is churn without a consumer.
Expansion path: go, java, csharp, c/cpp, ruby, php, kotlin next; legacy families (COBOL,
JCL, …) get curated `no_known_server` rows.

## 7. Installer skill — pointer model, instruct-only

Served as a **`skill://` MCP resource rendered at runtime** from the §6 data file. The
hand-rolled resource router serves `&'static str` today; rendering widens
`McpResource.content` to `Cow<'static, str>` — a local type change, no framework involved
(there is no rmcp router anywhere in the workspace).

**Pointer model — no curated install-command registry.** Install commands rot (npm↔brew,
version pins, OS differences). Estate ships, per server: the probe mapping (exact binary
name) and **one pinned official docs/package pointer**. The consuming agent fetches current
instructions from the pointer, executes them **under its own permission gate**, then
**re-probes the exact binary name** (npm's `pyright` package provides `pyright-langserver` —
verify the binary, not "install succeeded"). Offline degrades to the package name.

**Boundary, stated verbatim: wicked-installer remains the sole scripted-install surface;
this skill instructs, never executes; language servers stay out of registry.json.**

### Garden #347 re-scope

Estate owns probe / serve / route (capability report, installer skill resource, edit-plane
tools). wicked-garden #347 is re-scoped to the CC-plugin wiring that **consumes** estate's
surface — skill discovery/registration on the garden side, zero duplication of the probe or
pointer data. (Note only — no garden repo edits from this lane.)

## 8. Drift-guard discipline (§3.1)

- **Phase-0 does not touch the ENGINE-CONTRACT §3.1 activation table.** The Lsp row stays
  byte-identical: phase-0 lands no consumer, and the row is locked to change "only when a
  consumer actually lands".
- When the W3.6 consumer lands, only the Lsp row's **notes** cell changes; its activation
  cell must remain a non-`yes (slice)` value (e.g. "no — on-demand MCP/CLI only"). LSP
  never enters the production resolver slice.
- New code in `crates/wicked-estate/src/lib.rs` must never repeat the anchor string
  `// Activation table: docs/ENGINE-CONTRACT.md §3.1` — the drift test asserts uniqueness.

## 9. Deferred: demand-driven 1.0 write-back (optional phase, not scheduled)

Mechanism, when it comes: correlate an LSP answer to graph nodes via the **SCIP file+span
correlation seam** and upsert a 1.0 `Lsp`-tier edge (ON CONFLICT keep-higher — no schema
change, no migration). It is **not** straightforward; gate criteria that must be solved
first, in writing:

1. **Scheme-2 id churn + `prune_dangling_edges` decay** — write-back edges attach to ids
   that churn on ordinary edits and silently vanish on the next index with nothing
   re-emitting them;
2. **UTF-16-vs-byte span correlation** (LSP columns are UTF-16 code units; extractor spans
   are bytes);
3. **keep-higher staleness** — a stale 1.0 edge wins dedup over a fresh lower-tier truth;
4. **mtime-cache thrash** — each write-back invalidates the whole MCP response cache;
5. **bench movement** — `ConfidenceBands.exact` moves; requires an explicit rebaseline
   (garden's gate-benchmark-rebaseline), never a silent shift.

## 10. Consequences

- The orphan gets a defined, locked-decision-compliant consumer; the bulk ban survives
  implementers who never read the history (type-resistant boundary + O(1) invariant).
- crew/studio see no behavior change until W3.6 lands its tools; every consumer-visible
  break (tool count, goldens, docs) is a deliberate AC in §5, not a patch-around.
- A future language = one data row + one pointer (§6); a missing server degrades loudly
  (§3) instead of failing or lying.
