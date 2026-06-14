# wicked-estate — Claude Code plugin

Registers the **wicked-estate** MCP server in Claude Code: a code + infrastructure estate graph for
agents (search, retrieve, blast-radius, lineage, fetch-content) across 91 languages plus a
mainframe/IaC estate layer.

## Prerequisites

The plugin launches the `wicked-estate-mcp` binary — install it and build a graph first:

```sh
cargo install wicked-estate                            # provides wicked-estate + wicked-estate-mcp on PATH
wicked-estate index . --db .wicked-estate/graph.db     # one-time (or `wicked-estate watch .` to keep it fresh)
```

## Install

```
/plugin marketplace add mikeparcewski/wicked-estate
/plugin install wicked-estate@wicked-estate
```

Restart Claude Code. The server runs against `${CLAUDE_PROJECT_DIR}/.wicked-estate/graph.db`.

Prefer no plugin? Register the server directly:
`claude mcp add wicked-estate -- wicked-estate-mcp --db "$PWD/.wicked-estate/graph.db"`

See [docs/mcp-integration.md](../../docs/mcp-integration.md) for Cursor / Antigravity / Codex.
