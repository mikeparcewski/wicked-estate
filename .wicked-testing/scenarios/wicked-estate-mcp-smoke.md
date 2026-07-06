---
name: wicked-estate-mcp-smoke
description: |
  Smoke tests for the unified wicked-estate-mcp binary (v0.13.0).
  Verifies the MCP stdio server starts, responds to initialize, exposes
  the expected 23 tools across estate/memory/knowledge domains, and
  handles unknown methods gracefully. Uses the fixture databases from
  the pre-build gate to exercise all three domains in a single invocation.
version: "1.0"
category: cli
tags: [smoke, mcp, unified, estate, memory, knowledge]
tools:
  required: [cargo, python3]
timeout: 120
assertions:
  - id: A1
    description: Binary starts and responds to MCP initialize with protocolVersion, capabilities (tools, resources, prompts), and serverInfo
  - id: A2
    description: tools/list returns exactly 23 tools in the default build (10 estate + 6 memory + 7 knowledge)
  - id: A3
    description: tools/list with domains active (all 4 fixture stores open) includes memory.capture and knowledge.ingest
  - id: A4
    description: SearchEntity responds with a non-error MCP result
  - id: A5
    description: memory.recall responds with a non-error MCP result (non-empty items from memory fixture)
  - id: A6
    description: knowledge.recall responds with a non-error MCP result (non-empty items from knowledge fixture)
  - id: A7
    description: Unknown method returns JSON-RPC error -32601 and server continues serving
---

## Setup

```bash
ESTATE_ROOT="$(pwd)"
BINARY="$ESTATE_ROOT/target/debug/wicked-estate-mcp"
FIXTURES_DIR="$ESTATE_ROOT/crates/wicked-estate-mcp/tests/fixtures"

# Build the binary (debug is fine for smoke)
cargo build -p wicked-estate-mcp 2>&1 | tail -3

if [ ! -f "$BINARY" ]; then
  echo "ERROR: binary not found at $BINARY" >&2
  exit 1
fi

# Prepare temp fixture copies
WT_TMP="${TMPDIR:-${TEMP:-/tmp}}/wicked-estate-smoke-$$"
mkdir -p "$WT_TMP"
cp "$FIXTURES_DIR/estate_v0120.db"    "$WT_TMP/estate.db"
cp "$FIXTURES_DIR/memory_v0121.db"    "$WT_TMP/memory.db"
cp "$FIXTURES_DIR/knowledge_v0121.db" "$WT_TMP/knowledge.db"
cp "$FIXTURES_DIR/xedge_v0121.db"    "$WT_TMP/xedge.db"
[ -f "$FIXTURES_DIR/memory_v0121.db.memext" ] && cp "$FIXTURES_DIR/memory_v0121.db.memext" "$WT_TMP/memory.db.memext"

echo "Fixtures copied to $WT_TMP"
```

## Steps

### Step 1 — Initialize: capabilities and serverInfo

```bash
python3 - <<'PYEOF'
import subprocess, json, sys, os

binary = os.environ.get("BINARY", "./target/debug/wicked-estate-mcp")
tmp = os.environ.get("WT_TMP", "/tmp/wicked-estate-smoke")

env = dict(os.environ)
env["WICKED_ESTATE_DB"]   = os.path.join(tmp, "estate.db")
env["WICKED_MEMORY_DB"]   = os.path.join(tmp, "memory.db")
env["WICKED_KNOWLEDGE_DB"] = os.path.join(tmp, "knowledge.db")
env["WICKED_XEDGE_DB"]    = os.path.join(tmp, "xedge.db")

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)

def rpc(proc, req_id, method, params=None):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline().strip()
        if not line:
            continue
        msg = json.loads(line)
        if msg.get("id") == req_id:
            return msg

init = rpc(proc, 1, "initialize", {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {"name": "wt-smoke", "version": "0"}
})
proc.kill(); proc.wait()

assert "error" not in init, f"initialize error: {init}"
caps = init["result"]["capabilities"]
assert "tools" in caps, f"missing tools capability: {caps}"
assert "resources" in caps, f"missing resources capability: {caps}"
assert "prompts" in caps, f"missing prompts capability: {caps}"
assert init["result"]["protocolVersion"] == "2024-11-05", f"bad protocolVersion: {init['result']}"
assert "wicked-estate" in init["result"]["serverInfo"]["name"], f"bad serverInfo: {init['result']}"

print("A1 PASS: initialize OK — protocolVersion, tools/resources/prompts capabilities, serverInfo")
PYEOF
```

### Step 2 — tools/list: 23 tools in default build

```bash
python3 - <<'PYEOF'
import subprocess, json, os

binary = os.environ.get("BINARY", "./target/debug/wicked-estate-mcp")
tmp = os.environ.get("WT_TMP", "/tmp/wicked-estate-smoke")

env = dict(os.environ)
env["WICKED_ESTATE_DB"]   = os.path.join(tmp, "estate.db")
env["WICKED_MEMORY_DB"]   = os.path.join(tmp, "memory.db")
env["WICKED_KNOWLEDGE_DB"] = os.path.join(tmp, "knowledge.db")
env["WICKED_XEDGE_DB"]    = os.path.join(tmp, "xedge.db")

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)

def rpc(proc, req_id, method, params=None):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline().strip()
        if not line: continue
        msg = json.loads(line)
        if msg.get("id") == req_id: return msg

rpc(proc, 1, "initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wt","version":"0"}})
tools_resp = rpc(proc, 2, "tools/list", {})
proc.kill(); proc.wait()

assert "error" not in tools_resp, f"tools/list error: {tools_resp}"
tools = tools_resp["result"]["tools"]
names = sorted(t["name"] for t in tools)

EXPECTED_ESTATE = ["BlastRadius","Communities","ContextBundle","FetchContent","Lineage","RankHotspots","RetrieveEntity","RulesInventory","SearchEntity","TraverseGraph"]
EXPECTED_MEMORY = ["memory.capture","memory.coverage","memory.erase","memory.learn","memory.recall","memory.reflect"]
EXPECTED_KNOWLEDGE = ["knowledge.coverage","knowledge.ingest","knowledge.recall","knowledge.recall_about_code","knowledge.relate","knowledge.relate_code","knowledge.write"]

assert len(tools) == 23, f"Expected 23 tools, got {len(tools)}: {names}"

present = set(names)
for tool in EXPECTED_ESTATE + EXPECTED_MEMORY + EXPECTED_KNOWLEDGE:
    assert tool in present, f"Missing tool: {tool}"

estate_count = sum(1 for n in names if n not in [t for t in EXPECTED_MEMORY+EXPECTED_KNOWLEDGE])
memory_count = sum(1 for n in names if n.startswith("memory."))
knowledge_count = sum(1 for n in names if n.startswith("knowledge."))

assert estate_count == 10, f"Expected 10 estate tools, got {estate_count}"
assert memory_count == 6, f"Expected 6 memory tools, got {memory_count}"
assert knowledge_count == 7, f"Expected 7 knowledge tools, got {knowledge_count}"

print(f"A2 PASS: 23 tools (estate={estate_count}, memory={memory_count}, knowledge={knowledge_count})")
print(f"A3 PASS: memory.capture and knowledge.ingest present in tools list")
PYEOF
```

### Step 3 — SearchEntity with estate fixture

```bash
python3 - <<'PYEOF'
import subprocess, json, os

binary = os.environ.get("BINARY", "./target/debug/wicked-estate-mcp")
tmp = os.environ.get("WT_TMP", "/tmp/wicked-estate-smoke")

env = dict(os.environ)
env["WICKED_ESTATE_DB"]   = os.path.join(tmp, "estate.db")
env["WICKED_MEMORY_DB"]   = os.path.join(tmp, "memory.db")
env["WICKED_KNOWLEDGE_DB"] = os.path.join(tmp, "knowledge.db")
env["WICKED_XEDGE_DB"]    = os.path.join(tmp, "xedge.db")

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)

def rpc(proc, req_id, method, params=None):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline().strip()
        if not line: continue
        msg = json.loads(line)
        if msg.get("id") == req_id: return msg

rpc(proc, 1, "initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wt","version":"0"}})
search = rpc(proc, 2, "tools/call", {"name": "SearchEntity", "arguments": {"name": "seed_fn", "limit": 5}})
proc.kill(); proc.wait()

assert "error" not in search, f"SearchEntity RPC error: {search}"
result = search["result"]
assert not result.get("isError", False), f"SearchEntity isError: {result}"
body = json.loads(result["content"][0]["text"])
assert len(body.get("matches", [])) > 0, f"SearchEntity: empty matches from estate fixture: {body}"

print(f"A4 PASS: SearchEntity returned {body['total']} matches for 'seed_fn'")
PYEOF
```

### Step 4 — memory.recall with memory fixture

```bash
python3 - <<'PYEOF'
import subprocess, json, os

binary = os.environ.get("BINARY", "./target/debug/wicked-estate-mcp")
tmp = os.environ.get("WT_TMP", "/tmp/wicked-estate-smoke")

env = dict(os.environ)
env["WICKED_ESTATE_DB"]   = os.path.join(tmp, "estate.db")
env["WICKED_MEMORY_DB"]   = os.path.join(tmp, "memory.db")
env["WICKED_KNOWLEDGE_DB"] = os.path.join(tmp, "knowledge.db")
env["WICKED_XEDGE_DB"]    = os.path.join(tmp, "xedge.db")

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)

def rpc(proc, req_id, method, params=None):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline().strip()
        if not line: continue
        msg = json.loads(line)
        if msg.get("id") == req_id: return msg

rpc(proc, 1, "initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wt","version":"0"}})
recall = rpc(proc, 2, "tools/call", {"name": "memory.recall", "arguments": {"query": "seed", "token_budget": 512}})
proc.kill(); proc.wait()

assert "error" not in recall, f"memory.recall RPC error: {recall}"
result = recall["result"]
assert not result.get("isError", False), f"memory.recall isError: {result}"
body = json.loads(result["content"][0]["text"])
items = body.get("items", [])
assert len(items) > 0, f"memory.recall: empty items from memory fixture: {body}"

print(f"A5 PASS: memory.recall returned {len(items)} items for query 'seed'")
PYEOF
```

### Step 5 — knowledge.recall with knowledge fixture

```bash
python3 - <<'PYEOF'
import subprocess, json, os

binary = os.environ.get("BINARY", "./target/debug/wicked-estate-mcp")
tmp = os.environ.get("WT_TMP", "/tmp/wicked-estate-smoke")

env = dict(os.environ)
env["WICKED_ESTATE_DB"]   = os.path.join(tmp, "estate.db")
env["WICKED_MEMORY_DB"]   = os.path.join(tmp, "memory.db")
env["WICKED_KNOWLEDGE_DB"] = os.path.join(tmp, "knowledge.db")
env["WICKED_XEDGE_DB"]    = os.path.join(tmp, "xedge.db")

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)

def rpc(proc, req_id, method, params=None):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline().strip()
        if not line: continue
        msg = json.loads(line)
        if msg.get("id") == req_id: return msg

rpc(proc, 1, "initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wt","version":"0"}})
recall = rpc(proc, 2, "tools/call", {"name": "knowledge.recall", "arguments": {"query": "seed", "token_budget": 512}})
proc.kill(); proc.wait()

assert "error" not in recall, f"knowledge.recall RPC error: {recall}"
result = recall["result"]
assert not result.get("isError", False), f"knowledge.recall isError: {result}"
body = json.loads(result["content"][0]["text"])
items = body.get("items", [])
assert len(items) > 0, f"knowledge.recall: empty items from knowledge fixture: {body}"

print(f"A6 PASS: knowledge.recall returned {len(items)} items for query 'seed'")
PYEOF
```

### Step 6 — Unknown method returns -32601, server continues

```bash
python3 - <<'PYEOF'
import subprocess, json, os

binary = os.environ.get("BINARY", "./target/debug/wicked-estate-mcp")
tmp = os.environ.get("WT_TMP", "/tmp/wicked-estate-smoke")

env = dict(os.environ)
env["WICKED_ESTATE_DB"]   = os.path.join(tmp, "estate.db")
env["WICKED_MEMORY_DB"]   = os.path.join(tmp, "memory.db")
env["WICKED_KNOWLEDGE_DB"] = os.path.join(tmp, "knowledge.db")
env["WICKED_XEDGE_DB"]    = os.path.join(tmp, "xedge.db")

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env)

def rpc(proc, req_id, method, params=None):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline().strip()
        if not line: continue
        msg = json.loads(line)
        if msg.get("id") == req_id: return msg

rpc(proc, 1, "initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wt","version":"0"}})

# Send unknown method
unknown = rpc(proc, 2, "completely.unknown.method", {})
assert "error" in unknown, f"Expected error for unknown method, got: {unknown}"
assert unknown["error"]["code"] == -32601, f"Expected -32601, got: {unknown['error']}"

# Verify server continues
tools_resp = rpc(proc, 3, "tools/list", {})
proc.kill(); proc.wait()

assert "error" not in tools_resp, f"Server stopped after unknown method: {tools_resp}"
assert len(tools_resp["result"]["tools"]) == 23, f"tools/list broken after unknown method"

print("A7 PASS: Unknown method returned -32601, server continued serving")
PYEOF
```

## Cleanup

```bash
rm -rf "$WT_TMP"
echo "Cleanup done"
```
