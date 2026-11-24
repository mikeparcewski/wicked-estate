# Registering the wicked-estate MCP server

`wicked-estate-mcp` is a standard **JSON-RPC 2.0 MCP server over stdio** (`initialize` →
`tools/list` → `tools/call`, protocol `2024-11-05`). Any MCP-capable client can drive it — below are
copy-paste recipes for **Claude Code, Cursor, Antigravity, and Codex**.

> **On "plugins":** only **Claude Code** has a distributable *plugin* package — wicked-estate ships
> one (`plugins/wicked-estate/`, installable via the marketplace below). Cursor, Antigravity, and
> Codex have no plugin-package format; you register the MCP server via a config file or a CLI
> command (one step, but not a packaged plugin). All four end up running the same stdio server.

## Prerequisites (once)

```sh
cargo install wicked-estate wicked-estate-mcp   # two crates — the CLI + the MCP server
wicked-estate index /path/to/repo --db /path/to/repo/.wicked-estate/graph.db
```

The server is invoked the same way everywhere:

| | |
|---|---|
| **command** | `wicked-estate-mcp` (absolute path if it isn't on the client's PATH) |
| **args** | `["--db", "/abs/path/to/repo/.wicked-estate/graph.db"]` |
| **transport** | stdio |

Use an **absolute** DB path — clients launch the server with an unpredictable working directory.
(Alternatively set `WICKED_ESTATE_DB` in the server's `env`; `--db` takes precedence.)

It exposes **36 tools across 3 domains** plus `SemanticSearch` when embeddings are present:

**Estate (12 tools):** `SearchEntity`, `RetrieveEntity`, `TraverseGraph`, `Path`, `BlastRadius`, `FetchContent`, `ContextBundle`, `RulesInventory`, `rules.recall`, `RankHotspots`, `Communities`, `Lineage`

`RankHotspots` (PageRank) and `Communities` (Louvain) count `Calls` and `Imports` edges only.
`References` edges, including everything `scip` ingests, are not counted, so a symbol that is only
referenced, never called or imported, ranks low. See FEATURES.md §6.

**Memory (9 tools):** `memory.capture`, `memory.recall`, `memory.reflect`, `memory.erase`, `memory.retire`, `memory.unretire`, `memory.learn`, `memory.coverage`, `memory.list`

**Proposal queue (4 tools, memory domain):** `proposal.submit`, `proposal.list`, `proposal.approve`, `proposal.reject`

**Knowledge (11 tools):** `knowledge.ingest`, `knowledge.forget`, `knowledge.write`, `knowledge.relate`, `knowledge.recall`, `knowledge.coverage`, `knowledge.relate_code`, `knowledge.recall_about_code`, `knowledge.replace_source`, `knowledge.source_catalog`, `knowledge.neighbors`

`knowledge.forget` takes exactly one of `id` (uuid or full SymbolId), `source` (an exact citation
string, including file paths and URLs), or `scope_prefix` (a whole-segment subtree). An empty prefix
requires `confirm_erase_all: true`. It returns `{forgotten, skipped_live_source}`; deleting a document
also deletes its derived chunks and owned links. Only knowledge kinds are eligible. Live-source
structure is refused by id and otherwise skipped and counted: refresh that structure with
`knowledge.replace_source`, which accepts registry ids and covers live data sources only.

To refresh citation-sourced documents, call `knowledge.ingest` with `replace_source: true` and a
non-empty `source`; it replaces knowledge with that exact source inside the call's `scope` subtree
and returns `{doc_id, replaced}`. The same source ingested under another scope is left alone; an
empty `scope` is the root, so it replaces the source in every scope.
An omitted or false flag keeps the existing append behavior. `source_rev` and `observed_at` remain
accepted but are not stored. An emptied topic can use `knowledge.forget {"source": "..."}`.

`knowledge.recall` accepts `require: ["source:<id>"]` alongside `visible`. Every required dep must
be visible and present in the item's effective deps (stored deps plus scope deps). Both filters run
before ranking and token budgeting, so other sources cannot consume a source-pinned recall's budget.
Omitted or null `require` imposes no requirement; malformed entries or deps outside `visible` are
invalid params.

`--readonly` hides and refuses `knowledge.forget`. `proposal.submit` remains available when its
memory store exists; when that store is missing, submission is refused with the missing path and
instructions to start a writable server once or select an existing store. Reads still serve empty,
and the read-only server creates no store.

Memory locks are owned by verified governance packs. Ordinary `memory.capture` writes into a
one-segment `org:` or `team:` scope, writes to locked ids, `memory.retire` / `memory.unretire` and
`memory.erase` of locked nodes fail with JSON-RPC -32603 and `memory_locked`. A subtree erase
containing any locked node refuses the whole erase and names the locked ids. Reflection,
consolidation and Working expiry skip locked nodes. Recall inherits locked one-segment org/team
memories across projects, then applies the normal facet gate.

`memory.list {"admit_for":{"scope":"project:x","intent":{"project":"p1"}}}` applies
exactly recall's ancestor/operator/locked-layer scope admission and facet gate, without query
retrieval, ranking or a token budget. Combining `admit_for` with `scope_prefix` or `retired:true`,
or supplying a malformed scope/intent, is -32602. An implementer without admission support returns
-32601. Omit `admit_for` for the complete management inventory.

The pack write and verification functions (`apply_pack_memories`, `pack_memories`) are library-only
in `wicked-estate-memory`; they add no MCP tools (the count remains 35, including 9 memory tools).
Signature verification and sync belong to the governance caller.

---

## Claude Code

**CLI (simplest):**

```sh
# project scope → writes .mcp.json (shareable, checked in); -s user for global
claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/.wicked-estate/graph.db"
claude mcp list          # verify
```

**Or a project `.mcp.json`** (repo root):

```json
{
  "mcpServers": {
    "wicked-estate": {
      "command": "wicked-estate-mcp",
      "args": ["--db", "${workspaceFolder}/.wicked-estate/graph.db"]
    }
  }
}
```

**Or install the bundled plugin** (one step — registers the MCP server for you):

```
/plugin marketplace add mikeparcewski/wicked-estate
/plugin install wicked-estate@wicked-estate
```

The plugin lives in `plugins/wicked-estate/` (manifest + a `.mcp.json` pointing at
`${CLAUDE_PROJECT_DIR}/.wicked-estate/graph.db`). Prereq either way: `cargo install wicked-estate wicked-estate-mcp`.

## Cursor

Global `~/.cursor/mcp.json`, or per-project `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "wicked-estate": {
      "command": "wicked-estate-mcp",
      "args": ["--db", "/abs/path/to/repo/.wicked-estate/graph.db"]
    }
  }
}
```

Then enable it in **Cursor → Settings → MCP**.

## Antigravity

Google's agentic IDE — the successor to the (retiring) Gemini CLI. Antigravity (IDE / 2.0 / CLI)
shares one MCP config at **`~/.gemini/config/mcp_config.json`**. Edit it directly, or from the IDE:
agent panel → **…** → **Manage MCP Servers** → **View raw config**.

```json
{
  "mcpServers": {
    "wicked-estate": {
      "command": "wicked-estate-mcp",
      "args": ["--db", "/abs/path/to/repo/.wicked-estate/graph.db"]
    }
  }
}
```

(Antigravity uses `serverUrl` — not `url` — for *remote HTTP* MCP servers; N/A here, this server is
stdio.)

## Codex CLI

Codex uses **TOML**, not JSON — `~/.codex/config.toml`:

```toml
[mcp_servers.wicked-estate]
command = "wicked-estate-mcp"
args = ["--db", "/abs/path/to/repo/.wicked-estate/graph.db"]
```

Or, on recent Codex, via the CLI:

```sh
codex mcp add wicked-estate -- wicked-estate-mcp --db /abs/path/to/repo/.wicked-estate/graph.db
```

---

## Notes

- **Keep the graph fresh:** re-run `wicked-estate index <repo>` (incremental) or `wicked-estate watch
  <repo>` so the server answers against current code. The server reports staleness in tool
  `diagnostics` (agent-behavior rule R5) regardless.
- **Behavior contract:** the server never returns `isError` for a missing symbol (R1), caps output
  at ~25K chars (R4), and labels low-confidence edges (R7) — see `docs/agent-behavior-rules.md`.
- **Same shape, different files:** Claude Code / Cursor / Antigravity all use the `mcpServers` JSON
  block (only the file location differs); Codex uses the `[mcp_servers.*]` TOML table.
