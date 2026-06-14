# Registering the wicked-estate MCP server

`wicked-estate-mcp` is a standard **JSON-RPC 2.0 MCP server over stdio** (`initialize` →
`tools/list` → `tools/call`, protocol `2024-11-05`). Any MCP-capable client can drive it — below are
copy-paste recipes for **Claude Code, Cursor, Antigravity, and Codex**.

## Prerequisites (once)

```sh
cargo install wicked-estate              # puts wicked-estate + wicked-estate-mcp on PATH
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

It exposes 6 retrieval tools (`SearchEntity`, `RetrieveEntity`, `TraverseGraph`, `BlastRadius`,
`Lineage`, `FetchContent`) plus `SemanticSearch` when embeddings are present.

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

A Claude Code **plugin** may also bundle this server (declare the same `mcpServers` block in the
plugin), so installing the plugin registers wicked-estate automatically.

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

---

## Notes

- **Keep the graph fresh:** re-run `wicked-estate index <repo>` (incremental) or `wicked-estate watch
  <repo>` so the server answers against current code. The server reports staleness in tool
  `diagnostics` (agent-behavior rule R5) regardless.
- **Behavior contract:** the server never returns `isError` for a missing symbol (R1), caps output
  at ~25K chars (R4), and labels low-confidence edges (R7) — see `docs/agent-behavior-rules.md`.
- **Same shape, different files:** Claude Code / Cursor / Antigravity all use the `mcpServers` JSON
  block (only the file location differs); Codex uses the `[mcp_servers.*]` TOML table.
